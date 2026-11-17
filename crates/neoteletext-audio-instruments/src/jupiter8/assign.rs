//! Which voice plays which key: the instrument's four assign modes.
//!
//! Source: owner's manual, printed page 12, "Assign Modes", quoted at
//! each mode below; service notes, page 24, "TEST MODES": "Among key
//! assignments, POLY-1 only changes assigning order - tapping single key
//! (same key) will assign modules from A to H one by one". What the
//! manual leaves open is said where it is decided.

use super::patch::AssignMode;

/// Voices of the instrument.
///
/// Source: Roland's specifications: "8 voice polyphonic synthesizer".
pub const VOICES: usize = 8;

/// Keys the solo and unison modes remember as held: more than two hands
/// hold, and more than MIDI usually sends. Of more keys than these, solo
/// remembers the lowest, whose key it plays, and unison the first.
///
/// Source: own choice.
const HELD_KEYS: usize = 16;

/// What a voice is asked to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    /// The key it sounds, or last sounded.
    pub key: u8,
    /// Whether its gate is on: its key is down, or the hold keeps it.
    pub gate: bool,
    /// How many times it has been triggered: when this has moved, its
    /// envelopes start again. A key that changes without a trigger is a
    /// slide to the new key.
    pub triggers: u32,
    /// Whether its release is to be cut short (poly 2).
    pub damped: bool,
    /// Its key is up and the hold keeps it.
    kept: bool,
    /// When it was last given a key, on the assigner's clock.
    started: u32,
}

impl Slot {
    const REST: Self = Self { key: 0, gate: false, triggers: 0, damped: false, kept: false, started: 0 };
}

/// A key the solo and unison modes hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Held {
    key: u8,
    /// Whether the key itself is down; if not, the hold keeps it.
    down: bool,
}

/// The assigner of one half of the instrument: of all eight voices, or
/// of four.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Assigner {
    mode: AssignMode,
    /// The voices it assigns: the first so many of its slots.
    count: usize,
    slots: [Slot; VOICES],
    clock: u32,
    held: [Held; HELD_KEYS],
    holding: usize,
    hold: bool,
}

impl Assigner {
    /// An assigner of `count` voices, at most [`VOICES`], with none
    /// playing.
    #[must_use]
    pub const fn new(mode: AssignMode, count: usize) -> Self {
        Self {
            mode,
            count: if count > VOICES { VOICES } else { count },
            slots: [Slot::REST; VOICES],
            clock: 0,
            held: [Held { key: 0, down: false }; HELD_KEYS],
            holding: 0,
            hold: false,
        }
    }

    /// Changes the mode or the number of voices. If either differs from
    /// what it was, every voice is released and the keys held are
    /// forgotten.
    pub fn configure(&mut self, mode: AssignMode, count: usize) {
        let count = count.min(VOICES);
        if mode != self.mode || count != self.count {
            self.release_all();
            self.mode = mode;
            self.count = count;
        }
    }

    /// The voices, for the instrument to read: the first so many are
    /// this assigner's.
    #[must_use]
    pub const fn slots(&self) -> &[Slot; VOICES] {
        &self.slots
    }

    /// Whether any key is held, by a finger or by the hold.
    #[must_use]
    pub fn any_held(&self) -> bool {
        self.active().iter().any(|slot| slot.gate)
    }

    fn active(&self) -> &[Slot] {
        self.slots.get(..self.count).unwrap_or(&[])
    }

    fn active_mut(&mut self) -> &mut [Slot] {
        self.slots.get_mut(..self.count).unwrap_or(&mut [])
    }

    fn start(slot: &mut Slot, key: u8, clock: &mut u32) {
        *clock = clock.wrapping_add(1);
        *slot = Slot {
            key,
            gate: true,
            triggers: slot.triggers.wrapping_add(1),
            damped: false,
            kept: false,
            started: *clock,
        };
    }

    /// A key goes down.
    pub fn key_down(&mut self, key: u8) {
        match self.mode {
            AssignMode::Poly1 | AssignMode::Poly2 => self.poly_down(key),
            AssignMode::Solo | AssignMode::Unison => {
                let solo = self.mode == AssignMode::Solo;
                let fingers = self.held().iter().any(|held| held.down);
                if let Some(held) = self.held_mut().iter_mut().find(|held| held.key == key) {
                    held.down = true;
                } else {
                    // A key beyond the voices takes the place of the
                    // oldest key that only the hold keeps (owner's
                    // manual, printed page 13, quoted at `poly_down`).
                    let voices = if solo { self.count.min(1) } else { self.count };
                    if self.holding >= voices.min(HELD_KEYS)
                        && let Some(kept) = self.held().iter().find(|held| !held.down).map(|held| held.key)
                    {
                        self.forget(|held| held.key == kept);
                    }
                    // With every place taken by a key under a finger,
                    // solo gives up its highest key for a lower one: the
                    // lowest key down is then always among those
                    // remembered.
                    if solo
                        && self.holding >= HELD_KEYS
                        && let Some(highest) = self.held().iter().map(|held| held.key).max()
                        && key < highest
                    {
                        self.forget(|held| held.key == highest);
                    }
                    if let Some(place) = self.held.get_mut(self.holding) {
                        *place = Held { key, down: true };
                        self.holding = self.holding.wrapping_add(1);
                    }
                }
                // Own reading: in solo a key struck with no other under
                // a finger starts the envelopes again, though the hold
                // kept a note sounding.
                if solo
                    && !fingers
                    && let Some(slot) = self.active_mut().first_mut()
                {
                    slot.gate = false;
                }
                self.share_out();
            }
        }
    }

    /// A key in a polyphonic mode.
    ///
    /// Source: owner's manual: "Poly-1, Poly-2 and Unison Modes all
    /// follow the same basic keyboard logic, following first note
    /// priority. When eight notes are played and held, no new notes can
    /// be sounded until one of the original eight is released. When eight
    /// notes have been played and released (even if their Release time is
    /// incomplete), new notes will take over the voices of the previous
    /// notes in the order originally played." And of poly 2: "only the
    /// last note or notes played together receive their natural Release
    /// length. Any notes played previously will have instantaneous
    /// Releases".
    ///
    /// Own reading: poly 1 takes, of the voices whose key is up, the one
    /// that was given its key longest ago, which is the service notes'
    /// "modules from A to H one by one"; poly 2 takes the first such
    /// voice and cuts short every other. A key that is already sounding
    /// (which a keyboard cannot send, and MIDI can) starts its own voice
    /// again.
    ///
    /// Source: owner's manual, printed page 13, "Hold Selectors": "Any
    /// notes played after the Hold function is turned on will sustain
    /// until that Hold function is removed or until more notes are played
    /// than there are voices available. In this case, voices will be
    /// taken in the order that they were originally used so long as their
    /// keys are not physically held on the keyboard." So when every voice
    /// is sounding, a new key takes the voice the hold has kept longest.
    fn poly_down(&mut self, key: u8) {
        let cyclic = self.mode == AssignMode::Poly1;
        let mut clock = self.clock;
        let slots = self.active_mut();
        let own = slots.iter().position(|slot| slot.gate && slot.key == key);
        let free = || {
            let up = slots.iter().enumerate().filter(|(_, slot)| !slot.gate);
            if cyclic {
                up.min_by_key(|(_, slot)| slot.started).map(|(index, _)| index)
            } else {
                up.map(|(index, _)| index).next()
            }
        };
        let kept = || {
            let kept = slots.iter().enumerate().filter(|(_, slot)| slot.kept);
            kept.min_by_key(|(_, slot)| slot.started).map(|(index, _)| index)
        };
        let Some(taken) = own.or_else(free).or_else(kept) else {
            return;
        };
        if !cyclic {
            for slot in slots.iter_mut().filter(|slot| !slot.gate) {
                slot.damped = true;
            }
        }
        if let Some(slot) = slots.get_mut(taken) {
            Self::start(slot, key, &mut clock);
        }
        self.clock = clock;
    }

    /// A key goes up. With the hold on, what it was sounding goes on.
    pub fn key_up(&mut self, key: u8) {
        let hold = self.hold;
        match self.mode {
            AssignMode::Poly1 | AssignMode::Poly2 => {
                for slot in self.active_mut().iter_mut().filter(|slot| slot.gate && !slot.kept && slot.key == key) {
                    slot.kept = hold;
                    slot.gate = hold;
                }
            }
            AssignMode::Solo | AssignMode::Unison => {
                if hold {
                    if let Some(held) = self.held_mut().iter_mut().find(|held| held.key == key) {
                        held.down = false;
                    }
                } else {
                    self.forget(|held| held.key == key);
                }
                self.share_out();
            }
        }
    }

    /// The hold goes on or off. Off, it lets go of every key that is
    /// already up.
    ///
    /// Source: owner's manual, printed page 13, "Hold Selectors": "allow
    /// notes played to carry on indefinitely at their programmed Sustain
    /// levels".
    pub fn hold(&mut self, on: bool) {
        if on == self.hold {
            return;
        }
        self.hold = on;
        if !on {
            for slot in self.active_mut().iter_mut().filter(|slot| slot.kept) {
                slot.kept = false;
                slot.gate = false;
            }
            self.forget(|held| !held.down);
            if matches!(self.mode, AssignMode::Solo | AssignMode::Unison) {
                self.share_out();
            }
        }
    }

    /// Every key goes up and the hold with it: every voice is released.
    pub fn release_all(&mut self) {
        self.hold = false;
        self.holding = 0;
        for slot in &mut self.slots {
            slot.gate = false;
            slot.kept = false;
        }
    }

    /// Every key under a finger goes up; what the hold keeps goes on.
    pub fn keys_up(&mut self) {
        let mut keys = [0u8; VOICES + HELD_KEYS];
        let mut count = 0usize;
        let down = self.active().iter().filter(|slot| slot.gate && !slot.kept).map(|slot| slot.key);
        let held = self.held().iter().filter(|held| held.down).map(|held| held.key);
        for (place, key) in keys.iter_mut().zip(down.chain(held)) {
            *place = key;
            count = count.wrapping_add(1);
        }
        for &key in keys.iter().take(count) {
            self.key_up(key);
        }
    }

    fn held(&self) -> &[Held] {
        self.held.get(..self.holding).unwrap_or(&[])
    }

    fn held_mut(&mut self) -> &mut [Held] {
        self.held.get_mut(..self.holding).unwrap_or(&mut [])
    }

    /// Forgets the held keys that match, keeping the others in order.
    fn forget(&mut self, matches: impl Fn(&Held) -> bool) {
        let mut kept = 0usize;
        for index in 0..self.holding {
            let Some(&held) = self.held.get(index) else {
                continue;
            };
            if !matches(&held) {
                if let Some(place) = self.held.get_mut(kept) {
                    *place = held;
                }
                kept = kept.wrapping_add(1);
            }
        }
        self.holding = kept;
    }

    /// Gives the keys held to the voices in solo and unison.
    ///
    /// Source: owner's manual: "The Solo Assign Mode effectively turns
    /// the Jupiter-8 into a single voice synthesizer ... allows solo
    /// playing using special solo synthesizer techniques such as holding
    /// one key while alternating another to get tight, fast trills ...
    /// The Solo Mode uses Low Note priority." And: "All 8 synthesizers
    /// will be assigned to any one key pressed, dividing to four each for
    /// two keys, and so on until eight notes are played with one
    /// synthesizer each."
    ///
    /// Own reading: in solo a key that takes the note over while another
    /// is held does not start the envelopes again. In unison the first
    /// keys held, as many as there are voices, are dealt round the voices
    /// in turn, so that every voice sounds whatever the number of keys
    /// (the manual: "the total output level will remain the same
    /// regardless of the number of notes played"); a voice whose key
    /// changes starts again.
    fn share_out(&mut self) {
        let mut keys = [0u8; HELD_KEYS];
        let mut holding = 0usize;
        for (place, held) in keys.iter_mut().zip(self.held()) {
            *place = held.key;
            holding = holding.wrapping_add(1);
        }
        let keys = keys.get(..holding).unwrap_or(&[]);
        let solo = self.mode == AssignMode::Solo;
        let mut clock = self.clock;
        let slots = self.active_mut();
        let sharing = keys.len().min(slots.len());
        let lowest = keys.iter().copied().min();
        for (index, slot) in slots.iter_mut().enumerate() {
            let key = if solo {
                lowest.filter(|_| index == 0)
            } else {
                index.checked_rem(sharing).and_then(|turn| keys.get(turn)).copied()
            };
            match key {
                None => slot.gate = false,
                Some(key) if !slot.gate || (!solo && slot.key != key) => Self::start(slot, key, &mut clock),
                Some(key) => slot.key = key,
            }
        }
        self.clock = clock;
    }
}
