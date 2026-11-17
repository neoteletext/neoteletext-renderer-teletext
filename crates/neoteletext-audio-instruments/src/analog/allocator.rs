//! The voice allocator: which of an instrument's voices plays which key.
//!
//! An instrument gives it key-downs, key-ups and the sustain pedal, and
//! reads back, for each of its N voices, the key it is to sound, whether
//! its gate is on, and a count of triggers: when the count has moved
//! since the instrument last looked, the voice's envelopes start again.
//! The allocator knows nothing of sound; the instrument tells it when a
//! released voice has fallen silent ([`Allocator::finished`]).
//!
//! # Polyphonic modes
//!
//! A key takes, in order: the voice already sounding that key; a voice
//! at rest; the voice released longest ago; the voice held longest. That
//! is the order of this crate's synth7 rack. Which voice at rest is taken
//! is the mode's ([`Assign`]): the next after the last one taken, round
//! and round, so that a released note rings on while others play; or the
//! first, so that the same keys land on the same voices.
//!
//! # Monophonic modes and unison
//!
//! One note sounds, on the first voice alone or on all of them at once.
//! Which of the keys held it is, is the priority ([`Priority`]); whether
//! a new key starts the envelopes again while another is held is the
//! trigger ([`Trigger`]). A key released while others are held hands the
//! note back to one of them without a new trigger.
//!
//! Source: own design, from general knowledge of how such instruments
//! assign keys (a monophonic instrument's note priority and single or
//! multiple trigger; a polyphonic one's assignment round the ring or
//! from the first voice, and its unison). It has not been checked
//! against the five instruments' manuals: each instrument maps its own
//! modes on these from its own documents when it is built.

/// Keys a monophonic mode remembers as held, in the order they went
/// down: more than two hands hold. A further key pushes out the oldest.
///
/// Source: own choice.
pub const HELD_KEYS: usize = 16;

/// Which voice at rest a polyphonic mode gives a new key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assign {
    /// The next at rest after the last voice taken, round and round.
    Cyclic,
    /// The first at rest.
    First,
}

/// Which of the keys held a monophonic mode sounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    /// The key that went down last.
    Last,
    /// The lowest key.
    Low,
    /// The highest key.
    High,
}

/// When a monophonic mode starts the envelopes again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// Only for a key that goes down when none is held.
    Single,
    /// For every key that goes down and takes the note.
    Multiple,
}

/// How keys are given to voices.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// A voice for each key.
    Poly(Assign),
    /// One note on the first voice alone.
    Mono(Priority, Trigger),
    /// One note on every voice at once.
    Unison(Priority, Trigger),
}

/// What a voice is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// Nothing: it may be given a key.
    Rest,
    /// Its key is down.
    Held,
    /// Its key is up and the sustain pedal holds it.
    Sustained,
    /// Its key is up and it is dying away.
    Released,
}

/// What the instrument reads of one voice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Voice {
    /// What it is doing.
    pub stage: Stage,
    /// The key it sounds, or last sounded.
    pub key: u8,
    /// How hard that key was struck.
    pub velocity: u8,
    /// How many times it has been triggered: when this has moved, its
    /// envelopes start again. A key change without a trigger is a slide
    /// to the new key.
    pub triggers: u32,
    /// When it was last triggered and when it was released, on the
    /// allocator's clock of key events.
    started: u64,
    released: u64,
}

impl Voice {
    const fn new() -> Self {
        Self { stage: Stage::Rest, key: 0, velocity: 0, triggers: 0, started: 0, released: 0 }
    }

    /// Whether the voice's gate is on: its key is down, or the pedal
    /// holds it.
    #[must_use]
    pub const fn gate(&self) -> bool {
        matches!(self.stage, Stage::Held | Stage::Sustained)
    }
}

/// The allocator of an instrument of `N` voices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Allocator<const N: usize> {
    mode: Mode,
    voices: [Voice; N],
    pedal: bool,
    /// Counts key events, to order voices by age.
    clock: u64,
    /// The voice after the last one a cyclic mode took.
    next: usize,
    /// The keys held, oldest first, with their velocities, for the
    /// monophonic modes.
    held: [(u8, u8); HELD_KEYS],
    held_count: usize,
}

impl<const N: usize> Allocator<N> {
    /// An allocator with every voice at rest.
    #[must_use]
    pub const fn new(mode: Mode) -> Self {
        Self {
            mode,
            voices: [Voice::new(); N],
            pedal: false,
            clock: 0,
            next: 0,
            held: [(0, 0); HELD_KEYS],
            held_count: 0,
        }
    }

    /// The mode.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Changes the mode. Every voice that sounds is released, and the
    /// keys held are forgotten: the next key starts afresh.
    pub fn set_mode(&mut self, mode: Mode) {
        self.all_keys_up();
        self.mode = mode;
    }

    /// The voices, for the instrument to read.
    #[must_use]
    pub const fn voices(&self) -> &[Voice; N] {
        &self.voices
    }

    /// Voices that are not at rest.
    #[must_use]
    pub fn sounding(&self) -> usize {
        self.voices.iter().filter(|voice| voice.stage != Stage::Rest).count()
    }

    /// The instrument says that a released voice has fallen silent: it
    /// goes to rest. A voice that is not released, or that is not there,
    /// is left alone.
    pub fn finished(&mut self, voice: usize) {
        if let Some(voice) = self.voices.get_mut(voice)
            && voice.stage == Stage::Released
        {
            voice.stage = Stage::Rest;
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }

    /// A key goes down.
    pub fn key_down(&mut self, key: u8, velocity: u8) {
        let now = self.tick();
        match self.mode {
            Mode::Poly(assign) => self.poly_down(key, velocity, assign, now),
            Mode::Mono(priority, trigger) | Mode::Unison(priority, trigger) => {
                self.mono_down(key, velocity, priority, trigger, now);
            }
        }
    }

    /// A key goes up. With the pedal down, what it was sounding goes on.
    pub fn key_up(&mut self, key: u8) {
        let now = self.tick();
        match self.mode {
            Mode::Poly(_) => {
                let after = if self.pedal { Stage::Sustained } else { Stage::Released };
                for voice in &mut self.voices {
                    if voice.stage == Stage::Held && voice.key == key {
                        voice.stage = after;
                        voice.released = now;
                    }
                }
            }
            Mode::Mono(priority, _) | Mode::Unison(priority, _) => self.mono_up(key, priority, now),
        }
    }

    /// The sustain pedal goes down or up. Up, it lets go of every voice
    /// whose key is already up.
    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        if !down {
            let now = self.tick();
            for voice in &mut self.voices {
                if voice.stage == Stage::Sustained {
                    voice.stage = Stage::Released;
                    voice.released = now;
                }
            }
        }
    }

    /// Every key goes up and the pedal with it: every voice that sounds
    /// is released.
    pub fn all_keys_up(&mut self) {
        let now = self.tick();
        self.pedal = false;
        self.held_count = 0;
        for voice in &mut self.voices {
            if voice.gate() {
                voice.stage = Stage::Released;
                voice.released = now;
            }
        }
    }

    fn poly_down(&mut self, key: u8, velocity: u8, assign: Assign, now: u64) {
        // In order: the voice already sounding this key; a voice at rest;
        // the voice released longest ago; the voice held longest.
        let again = self.voices.iter().position(|voice| voice.stage != Stage::Rest && voice.key == key);
        let resting = || {
            let from = match assign {
                Assign::Cyclic => self.next,
                Assign::First => 0,
            };
            (0..N)
                .map(|offset| from.wrapping_add(offset).checked_rem(N).unwrap_or(0))
                .find(|&index| self.voices.get(index).is_some_and(|voice| voice.stage == Stage::Rest))
        };
        let stolen = || {
            (0..N).min_by_key(|&index| match self.voices.get(index) {
                Some(voice) if voice.stage == Stage::Released => (0u8, voice.released),
                Some(voice) => (1, voice.started),
                None => (2, 0),
            })
        };
        let Some(index) = again.or_else(resting).or_else(stolen) else {
            return;
        };
        if let Some(voice) = self.voices.get_mut(index) {
            voice.stage = Stage::Held;
            voice.key = key;
            voice.velocity = velocity;
            voice.triggers = voice.triggers.wrapping_add(1);
            voice.started = now;
        }
        if again.is_none() {
            self.next = index.wrapping_add(1).checked_rem(N).unwrap_or(0);
        }
    }

    /// The key a priority picks of those held, with its velocity.
    fn chosen(&self, priority: Priority) -> Option<(u8, u8)> {
        let held = self.held.get(..self.held_count)?;
        match priority {
            Priority::Last => held.last().copied(),
            Priority::Low => held.iter().copied().min_by_key(|&(key, _)| key),
            Priority::High => held.iter().copied().max_by_key(|&(key, _)| key),
        }
    }

    /// The voices a monophonic mode plays on: the first, or all.
    fn mono_voices(&mut self) -> &mut [Voice] {
        match self.mode {
            Mode::Unison(..) => &mut self.voices,
            _ => self.voices.get_mut(..1).unwrap_or(&mut []),
        }
    }

    fn forget(&mut self, key: u8) {
        let mut kept = 0;
        for index in 0..self.held_count {
            let Some(&entry) = self.held.get(index) else { break };
            if entry.0 != key {
                if let Some(slot) = self.held.get_mut(kept) {
                    *slot = entry;
                }
                kept = kept.wrapping_add(1);
            }
        }
        self.held_count = kept;
    }

    fn mono_down(&mut self, key: u8, velocity: u8, priority: Priority, trigger: Trigger, now: u64) {
        // The gate was on if a key, or the pedal, held the note.
        let sounding = self.mono_voices().first().is_some_and(Voice::gate);
        let before = self.mono_voices().first().map(|voice| voice.key);
        self.forget(key);
        if self.held_count == HELD_KEYS {
            // The oldest key held gives way.
            self.held.copy_within(1.., 0);
            self.held_count = HELD_KEYS.wrapping_sub(1);
        }
        if let Some(slot) = self.held.get_mut(self.held_count) {
            *slot = (key, velocity);
            self.held_count = self.held_count.wrapping_add(1);
        }
        let Some((chosen, chosen_velocity)) = self.chosen(priority) else {
            return;
        };
        // The note is the priority's choice: the new key's, or the one it
        // was with (or, if that one has just been pushed out, another's).
        // The new key triggers when no note was sounding, and, if the
        // mode says so, whenever it takes the note.
        let triggers = !sounding || (chosen == key && trigger == Trigger::Multiple && before != Some(key));
        for voice in self.mono_voices() {
            voice.stage = Stage::Held;
            voice.key = chosen;
            voice.velocity = chosen_velocity;
            if triggers {
                voice.triggers = voice.triggers.wrapping_add(1);
                voice.started = now;
            }
        }
    }

    fn mono_up(&mut self, key: u8, priority: Priority, now: u64) {
        self.forget(key);
        let chosen = self.chosen(priority);
        let after = if self.pedal { Stage::Sustained } else { Stage::Released };
        for voice in self.mono_voices() {
            match chosen {
                // The note goes back to a key still held, with no new
                // trigger.
                Some((key, velocity)) if voice.gate() => {
                    voice.key = key;
                    voice.velocity = velocity;
                }
                None if voice.stage == Stage::Held => {
                    voice.stage = after;
                    voice.released = now;
                }
                _ => {}
            }
        }
    }
}
