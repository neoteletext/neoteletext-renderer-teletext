//! Yamaha's CS-80 (1977): eight voices of two complete lines each, every
//! line an oscillator, a high-pass and a low-pass filter in series, a
//! filter envelope and an amplifier with its own; keys that sense how
//! hard they are struck and how hard each is pressed; a sub oscillator, a
//! ring modulator and a tremolo, rendered from MIDI messages to 16-bit
//! mono samples at 48,000 Hz.
//!
//! # The instrument
//!
//! - **Two channels.** Every key sounds two lines, one of channel I and
//!   one of channel II, each with its own tone (a row of the panel, a
//!   memory or a preset), its own stop of 16' to 2', and the MIX lever
//!   between them. Channel II can be detuned.
//! - **Oscillator** (IG00153, with the waveshaper IG00158). Its frequency
//!   is in proportion to the key voltage, 523.25 Hz to the volt at 8'. It
//!   gives a sawtooth, a square whose width is set from 50% to 90% and
//!   moved by the channel's own slow oscillator, noise, and a sine that
//!   goes past the filters to the amplifier.
//! - **Filters** (IG00156, twice): a high-pass then a low-pass, each a
//!   state-variable filter of 12 dB an octave with its own resonance
//!   ([`StateVariable`](crate::analog::filter::StateVariable)). Their
//!   cutoffs follow the key, and move an octave for each volt. The
//!   resonance reaches a Q of 5 by its lever and 12.5 at most; the
//!   filters do not oscillate by themselves.
//! - **Envelopes.** The filters' (IG00152) starts at an initial level
//!   below the levers' setting, rises to an attack level above it, decays
//!   to the setting and on release returns to the initial level. The
//!   amplifier's (IG00159) is attack, decay, sustain and release. Both
//!   are capacitors charged and discharged
//!   ([`RcEnvelope`]).
//! - **Touch.** How hard a key is struck (MIDI velocity) and how hard it
//!   is then pressed, each key by itself (MIDI polyphonic key pressure),
//!   move the brilliance and the level by each tone's four levers;
//!   pressure also speeds the sub oscillator and deepens its vibrato and
//!   wah, and a hard stroke can start a note flat.
//! - **After the voices**, at the output rate: the ring modulator, whose
//!   own oscillator an attack and decay envelope speeds up; the
//!   tremolo/chorus; the volume.
//!
//! # What MIDI's controls are on this instrument
//!
//! - **Pitch bend is the ribbon**, which adds to or takes from the key
//!   voltage, and so multiplies every frequency: full up doubles it, an
//!   octave, as the owner's manual gives the ribbon's upward range; full
//!   down takes it to nothing, the manual's "sub-audio frequency", held
//!   here to eight octaves down. A bend is therefore a straight line in
//!   frequency, not in pitch, and far wider downwards than up. The
//!   filters follow it as they follow the keys.
//! - **Controller 64 is the foot switch** with its SUSTAIN assigner on:
//!   down, a released note dies away in the SUSTAIN lever's time instead
//!   of the tone's release time, if that is longer. It does not hold
//!   notes, as the instrument's does not.
//! - **Controller 1**, the modulation wheel, which the instrument has
//!   not, lets the sub oscillator move the pitch by up to
//!   [`WHEEL_CENTS`], as pressure does through the touch response's VCO
//!   lever.
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Yamaha's service manual (KEC-4223), its adjustment procedure and its
//! owner's manual, and Jürgen Haible's analysis of the filter, with no
//! code and no table from any emulator or plugin. One reference program
//! was run as a sealed program and its output measured. Each constant
//! names its source where it is defined, in [`law`], the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **The custom chips' insides** are not published. What a control
//!   voltage does between a chip's documented ends is an own choice
//!   wherever its `Source:` line says so: above all how many octaves or
//!   decibels a volt of touch, of the keyboard control and of the sub
//!   oscillator is, and how far the overall levers reach.
//! - **BRASS 1 is brighter than the one real instrument heard**, at
//!   every stroke: on middle C its filter comes to rest near 2 kHz at the
//!   softest stroke and near 12 kHz at the hardest, where a recording of
//!   a real instrument, struck no one knows how hard, has it near 0.9
//!   kHz. The touch response is not the cause of the softest stroke's
//!   octave, since it adds nothing there; that octave lies in what the
//!   preset's low-pass row was read as, in where the recorded
//!   instrument's overall levers stood, or in its trim, and no document
//!   read says which. How many volts a stroke gives is in no document
//!   either: a velocity of 127 is taken as the most the detector can
//!   pass, in a straight line below (own choice), and a pressure of 127
//!   as the firm pressure the after-touch is trimmed with
//!   (`~/devel/teletext-reference/synthesizers/cs-80/validation/`).
//! - **The filters do not distort**, and the loss of resonance at high
//!   frequencies that Haible finds (180 pF across the damping) is left
//!   out.
//! - **The tremolo/chorus is what the mono output has of it.** The
//!   circuit puts the sound through two delay lines of fixed length and
//!   moves the levels of the left and right outputs against each other;
//!   in their sum only the amplifiers' imbalance is left, a tremolo of a
//!   few per cent. The profile is mono (DEC-6437 item 2).
//! - **No expression pedal and no wah**, no external input (the sub
//!   oscillator's EXT position is its off position), no coarse and fine
//!   PITCH knob, no HIGH/LOW output switch.
//! - **Portamento is per voice**, each from the pitch it last had; the
//!   instrument's key assigner was not read closely enough to say that
//!   this is what it does.
//! - **The four memories** are not kept: a memory is a tone, and
//!   [`Patch`] holds two.

pub mod law;
pub mod patch;
pub mod presets;
mod voice;

use crate::SAMPLE_RATE;
use crate::analog::allocator::{Allocator, Assign, Mode, Stage};
use crate::analog::chorus::Chorus;
use crate::analog::decimator::Decimator;
use crate::analog::envelope::{ENVELOPE_ONE, RcEnvelope, apply, time_constant};
use crate::analog::filter::ONE_Q28;
use crate::analog::fixed::amplify;
use crate::analog::oscillator::Noise;
use crate::analog::{frequency, to_sample};

use law::OCTAVE;
pub use patch::{Control, Feet, Function, Global, PATCH_BYTES, Patch, PatchError, TONE_BYTES, Tone};
use voice::{Channel, Controls, RIBBON_REST, Shared, TICK_RATE, TICK_SAMPLES, Voice, sine};
pub use voice::{OVERSAMPLING, RATE};

/// Voices of the instrument.
///
/// Source: owner's manual, page 31, "Overall Picture": "The CS-80 is an
/// eight note polyphonic synthesizer. However, there are really 16 main
/// oscillators divided into two ranks".
pub const VOICES: usize = 8;

/// How far the modulation wheel at its top lets the sub oscillator move
/// the pitch either way, in cents.
///
/// Source: own choice: the instrument has no such wheel.
pub const WHEEL_CENTS: i32 = 50;

/// The pitch bend value that bends nothing, and the bits below it.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// The least the ribbon makes of a frequency, in Q28: eight octaves down.
///
/// Source: own choice of the floor. Owner's manual, page 14, item 19: "you
/// can move the highest note on the keyboard down to a sub-audio
/// frequency", and upwards "approximately one octave".
const RIBBON_FLOOR: u32 = RIBBON_REST >> 8;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// The foot switch is down at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const PEDAL_DOWN: u8 = 64;
/// MIDI status: note off.
const STATUS_NOTE_OFF: u8 = 0x80;
/// MIDI status: note on.
const STATUS_NOTE_ON: u8 = 0x90;
/// MIDI status: polyphonic key pressure.
const STATUS_KEY_PRESSURE: u8 = 0xa0;
/// MIDI status: control change.
const STATUS_CONTROL_CHANGE: u8 = 0xb0;
/// MIDI status: channel pressure.
const STATUS_CHANNEL_PRESSURE: u8 = 0xd0;
/// MIDI status: pitch bend.
const STATUS_PITCH_BEND: u8 = 0xe0;
/// Controller: modulation wheel.
const CONTROL_MODULATION_WHEEL: u8 = 1;
/// Controller: volume.
const CONTROL_VOLUME: u8 = 7;
/// Controller: sustain pedal.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// The seed of the instrument's noise and of its oscillators' places and
/// tolerances.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4353_3830;

/// A slow oscillator's frequency with 5 V at its speed input, in
/// thousandths of a hertz: a cycle in 150 ms. The pulse width's, the sub
/// oscillator's and the ring modulator's are all trimmed to it.
///
/// Source: adjustment procedure, pages 17 and 19: pulse width modulator,
/// step 1; sub oscillator, step 1; ring modulator, step 3: each "150 ms
/// (± 5%)" with "+5.0V" at its terminal.
const SPEED_AT_FIVE_VOLTS: u32 = 6_667;
/// The octaves a volt speeds the pulse width's oscillator and the ring
/// modulator's: from 150 ms at 5 V to 5 ms at the lever's top, log₂ 30
/// in five volts.
///
/// Source: the same steps: "5 ms (± 5%)" at FAST, and at "SPEED ... max".
/// Own choice: the same below 5 V, which puts the lever's foot at 4.5 s.
const FAST_OCTAVES_A_VOLT: i32 = law::octaves(981_378, 1_000_000);
/// The same for the sub oscillator: to 18 ms, log₂ (150 / 18) in five
/// volts, which puts the lever's foot at 1.25 s.
///
/// Source: adjustment procedure, page 17, sub oscillator: "18ms (± 5%)"
/// at FAST.
const SUB_OCTAVES_A_VOLT: i32 = law::octaves(611_780, 1_000_000);
/// The volts pressure adds to the sub oscillator's speed with the touch
/// response's SPEED lever at the top.
///
/// Source: adjustment procedure, page 25, step 6: "+6.0 ± .3V" at the SP
/// terminal, the key firmly depressed.
const TOUCH_SPEED_VOLTS: i32 = 6;
/// The volts the ring modulator's envelope adds to its speed at its peak
/// with DEPTH at the top.
///
/// Source: own choice: the half of the SPEED lever's travel.
const RING_DEPTH_VOLTS: i32 = 5;
/// The ring modulator's shortest attack, in millionths of a second: the
/// 31 ms of 5 V halved for each of five volts more.
///
/// Source: adjustment procedure, page 19, step 2: "31ms (± 5%)" with
/// "+5.0V at A terminal". Own choice: an octave to the volt, as the
/// voices' envelopes.
const RING_ATTACK_SHORTEST: u32 = 31_000 >> 5;

/// The tremolo's and the chorus's fastest cycles, in thousandths of a
/// hertz: 56 ms and 0.5 s.
///
/// Source: adjustment procedure, page 21, tremolo/chorus, step 1: with
/// SPEED at max, "56ms (± 5%)" at DO1 and ".5s (± 5%)" at DO2.
const EFFECT_FASTEST: (u32, u32) = (17_857, 2_000);
/// The octaves the SPEED knob covers down from the fastest.
///
/// Source: own choice: the knob's network was not read.
const EFFECT_SPEED_SPAN: i32 = 3 * OCTAVE;
/// The delay of the effect's lines, in millionths of a second: 512
/// stages clocked with a period of 10 µs.
///
/// Source: service manual, page 58, OE2 board, note 2: "IC 4: MN3001";
/// adjustment procedure, page 21, step 2: the clock is set "between 7 -
/// 12.5 µs". That the MN3001 is two lines of 512 stages is general
/// knowledge of the part; its data sheet is not among the documents
/// filed.
const EFFECT_DELAY: u32 = 2_560;
/// The cutoff of the filters about the lines, in thousandths of a hertz.
///
/// Source: service manual, page 57, OE2 circuit diagram, as far as it
/// was read: each filter stage is 10 kΩ twice with 0.0018 µF and 680 pF,
/// 1 / (2 π R √(C1 C2)) = 14.4 kHz.
const EFFECT_FILTER: u32 = 14_400_000;
/// How far the effect at full depth moves the mono output's level either
/// way for each of its two oscillators, in Q30: 7 / 246.
///
/// Source: adjustment procedure, page 21, step 4: the four modulation
/// amplifiers are trimmed to ".65", ".58", ".58" and ".65 Vp-p"; service
/// manual, pages 55 and 57, as read: the left output is the first line
/// through the first two amplifiers, the right the second line through
/// the other two, and each oscillator drives one amplifier of each side
/// oppositely. In the sum of the two outputs an oscillator's share is
/// (0.65 - 0.58) / (2 × (0.65 + 0.58)). Own choice: the amplifiers swing
/// their whole gain at full depth.
const EFFECT_MONO_SHARE: i32 = (ENVELOPE_ONE as i64 * 7 / 246) as i32;
/// The level with the effect on, in Q30: nine tenths.
///
/// Source: own choice. Owner's manual, page 13, item 15: the "overall CS-80
/// output volume decreases slightly".
const EFFECT_LEVEL: i32 = (ENVELOPE_ONE as i64 * 9 / 10) as i32;

/// Thousandths of a fall's time that are its time constant: the time to
/// a tenth of the way, as the voices' envelopes.
const FALL_THOUSANDTHS: u64 = 434;
/// Thousandths of an attack's time that are its time constant.
const RISE_THOUSANDTHS: u64 = 1_000_000 / RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS as u64;
/// Thousandths.
const THOUSAND: u64 = 1_000;
/// The square of the largest volume, which the volume's square is a share
/// of.
const VOLUME_FULL: i64 = (DATA_MAX as i64) * (DATA_MAX as i64);
/// Samples of output in a tick of the voices.
const TICK_OUTPUT_SAMPLES: u32 = TICK_SAMPLES / OVERSAMPLING.factor();
/// Ticks of the output's slow oscillators a second: they move with the
/// voices' ticks.
const _: () = assert!(TICK_RATE * TICK_OUTPUT_SAMPLES == SAMPLE_RATE);

/// A slow oscillator: a phase and its step.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Wave {
    phase: u32,
    step: u32,
}

impl Wave {
    /// Moves on, and gives the phase it had.
    const fn tick(&mut self) -> u32 {
        let phase = self.phase;
        self.phase = phase.wrapping_add(self.step);
        phase
    }
}

/// The left shift from a signal's one to a level's.
const SINE_SHIFT: u32 = 6;
const _: () = assert!(crate::analog::LEVEL << SINE_SHIFT == ENVELOPE_ONE);

/// The sine of a phase as a level, in Q30 either way.
const fn sine_level(phase: u32) -> i32 {
    sine(phase) << SINE_SHIFT
}

/// A time of an envelope as its coefficient at the ticks' rate.
fn coefficient(time: u32, thousandths: u64) -> i32 {
    let constant = u64::from(time).saturating_mul(thousandths) / THOUSAND;
    time_constant(u32::try_from(constant).unwrap_or(u32::MAX), TICK_RATE)
}

/// The CS-80.
///
/// It holds no reference and allocates nothing. Two instruments given the
/// same patch and messages and rendered in calls of any lengths give the
/// same samples.
#[derive(Clone, Debug)]
pub struct Cs80 {
    patch: Patch,
    channels: [Channel; 2],
    shared: Shared,
    allocator: Allocator<VOICES>,
    voices: [Voice; VOICES],
    /// The trigger count each voice last acted on.
    triggers: [u32; VOICES],
    /// Each channel's noise generator, and the sub oscillator's.
    noise: [Noise; 2],
    sub_noise: Noise,
    sub: Wave,
    /// The pressure the sub oscillator's speed was last set for.
    sub_pressed: i32,
    pwm: [Wave; 2],
    ring: Wave,
    ring_envelope: RcEnvelope,
    /// The ring modulator's share of the sound, in Q30.
    ring_modulation: i32,
    /// The tremolo's and the chorus's oscillators, and how far each
    /// moves the level, in Q30.
    effect: [Wave; 2],
    effect_depth: [i32; 2],
    line: Chorus,
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
    pedal: bool,
    /// Samples of the voices until the next tick.
    tick_wait: u32,
    decimator: Decimator,
    ignored: u64,
}

impl Default for Cs80 {
    fn default() -> Self {
        Self::new()
    }
}

impl Cs80 {
    /// An instrument at rest playing [`Patch::DEFAULT`].
    #[must_use]
    pub fn new() -> Self {
        let mut number = 0u32;
        let voices = core::array::from_fn(|_| {
            let voice = Voice::new(SEED, number);
            number = number.wrapping_add(1);
            voice
        });
        let mut line = Chorus::new(SAMPLE_RATE);
        line.set_filter_cutoff(frequency(EFFECT_FILTER, SAMPLE_RATE));
        line.set_delay(EFFECT_DELAY, 0);
        let mut cs80 = Self {
            patch: Patch::DEFAULT,
            channels: [Channel::new(&Patch::DEFAULT, 0), Channel::new(&Patch::DEFAULT, 1)],
            shared: Shared::new(&Patch::DEFAULT),
            // Each key takes the next voice round the eight: adjustment
            // procedure, page 6, the note beside figure 4: depressing a
            // key repeatedly "will allow you to select a desired master
            // board".
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices,
            triggers: [0; VOICES],
            noise: [Noise::new(SEED), Noise::new(SEED.rotate_left(8))],
            sub_noise: Noise::new(SEED.rotate_left(16)),
            sub: Wave::default(),
            sub_pressed: -1,
            pwm: [Wave::default(); 2],
            ring: Wave::default(),
            ring_envelope: RcEnvelope::new(),
            ring_modulation: 0,
            effect: [Wave::default(); 2],
            effect_depth: [0; 2],
            line,
            bend: 0,
            wheel: 0,
            volume: DATA_MAX,
            pedal: false,
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        cs80.apply_patch();
        cs80
    }

    /// Puts the instrument back as [`Cs80::new`] made it, but for the
    /// patch, which it keeps: silent, its controllers at rest, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let patch = self.patch;
        *self = Self::new();
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect at
    /// once, on the voices that are sounding too, as moving the panel's
    /// levers does.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
        self.apply_patch();
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    fn apply_patch(&mut self) {
        let patch = self.patch;
        let lever = |global: Global| patch.get(global);
        self.channels = [Channel::new(&patch, 0), Channel::new(&patch, 1)];
        self.shared = Shared::new(&patch);
        for voice in &mut self.voices {
            voice.configure(&self.channels);
        }
        for (wave, tone) in self.pwm.iter_mut().zip(&patch.tones) {
            let speed = law::speed(SPEED_AT_FIVE_VOLTS, FAST_OCTAVES_A_VOLT, tone.get(Control::PwmSpeed), 0);
            wave.step = frequency(speed, TICK_RATE);
        }
        // The speed is set again at the next tick.
        self.sub_pressed = -1;

        self.ring_modulation = law::level(lever(Global::RingModulation));
        let attack = law::time(RING_ATTACK_SHORTEST, lever(Global::RingAttack));
        let decay = law::time(law::FALL_SHORTEST, lever(Global::RingDecay));
        self.ring_envelope.set_attack(coefficient(attack, RISE_THOUSANDTHS));
        self.ring_envelope.set_decay(coefficient(decay, FALL_THOUSANDTHS));
        self.ring_envelope.set_sustain(0);

        let slower = law::share(EFFECT_SPEED_SPAN, DATA_MAX.saturating_sub(lever(Global::EffectSpeed)));
        let depth = apply(law::level(lever(Global::EffectDepth)), EFFECT_MONO_SHARE);
        let [tremolo, chorus] = &mut self.effect;
        tremolo.step = frequency(law::raise(EFFECT_FASTEST.0, slower.saturating_neg()), SAMPLE_RATE);
        chorus.step = frequency(law::raise(EFFECT_FASTEST.1, slower.saturating_neg()), SAMPLE_RATE);
        self.effect_depth =
            [if patch.on(Global::Tremolo) { depth } else { 0 }, if patch.on(Global::Chorus) { depth } else { 0 }];
    }

    /// How many messages [`Cs80::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        self.allocator.sounding()
    }

    /// Takes one MIDI 1.0 channel voice message. The instrument is one
    /// part and hears every channel.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is how hard the key is struck.
    /// - Polyphonic key pressure (0xAn): how hard the key `data1` is
    ///   pressed, `data2`.
    /// - Channel pressure (0xDn): how hard every key that is down is
    ///   pressed, `data1`; `data2` is not used.
    /// - Control change (0xBn) of the modulation wheel (1), volume (7),
    ///   the foot switch (64, down from 64), all sound off (120), reset
    ///   all controllers (121, which leaves the volume) and all notes off
    ///   (123).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven: the ribbon.
    ///
    /// Anything else is ignored and counted ([`Cs80::ignored_events`]):
    /// any other status or controller, a data byte above 127. Nothing is
    /// an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & 0xf0 {
            STATUS_NOTE_OFF => self.allocator.key_up(data1),
            STATUS_NOTE_ON if data2 == 0 => self.allocator.key_up(data1),
            STATUS_NOTE_ON => self.key_down(data1, data2),
            STATUS_KEY_PRESSURE => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held && key.key == data1 {
                        voice.press(data2);
                    }
                }
            }
            STATUS_CHANNEL_PRESSURE => {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Held {
                        voice.press(data1);
                    }
                }
            }
            STATUS_CONTROL_CHANGE => self.control_change(data1, data2),
            STATUS_PITCH_BEND => {
                self.bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
            }
            _ => self.ignore(),
        }
    }

    fn key_down(&mut self, key: u8, velocity: u8) {
        if !self.allocator.voices().iter().any(|voice| voice.stage == Stage::Held) {
            // The ring modulator's envelope starts with the first key
            // (owner's manual, page 14, the note under D), and a sustain
            // of the second kind ends: "The next chord or note played
            // ends any previous sustain" (page 11, B).
            self.ring_envelope.retrigger();
            if self.patch.on(Global::SustainMode) {
                for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
                    if key.stage == Stage::Released {
                        voice.damp();
                    }
                }
            }
        }
        self.allocator.key_down(key, velocity);
        // A key newly down is not yet pressed.
        for (voice, held) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if held.stage == Stage::Held && held.key == key {
                voice.press(0);
            }
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            CONTROL_MODULATION_WHEEL => self.wheel = value,
            CONTROL_VOLUME => self.volume = value,
            CONTROL_SUSTAIN => self.pedal = value >= PEDAL_DOWN,
            CONTROL_ALL_SOUND_OFF => {
                self.allocator.all_keys_up();
                for (index, voice) in self.voices.iter_mut().enumerate() {
                    voice.cut(&self.channels);
                    self.allocator.finished(index);
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                for voice in &mut self.voices {
                    voice.press(0);
                }
            }
            CONTROL_ALL_NOTES_OFF => self.allocator.all_keys_up(),
            _ => self.ignore(),
        }
    }

    /// Releases every voice: for when the input stops and no note off
    /// will come.
    pub fn release_all(&mut self) {
        self.allocator.all_keys_up();
    }

    /// The sub oscillator's waveform at a phase, in Q30 either way.
    fn sub_wave(&mut self, phase: u32) -> i32 {
        /// The shift from a phase to a level that falls or rises through
        /// two in a cycle.
        const RAMP_SHIFT: u32 = 1;
        let rising = (phase >> RAMP_SHIFT).cast_signed().wrapping_sub(ENVELOPE_ONE);
        match Function::from_position(self.patch.get(Global::SubFunction)) {
            Function::Sine => sine_level(phase),
            Function::Sawtooth => rising.wrapping_neg(),
            Function::InvertedSawtooth => rising,
            Function::Square => {
                if phase < 1 << 31 {
                    ENVELOPE_ONE
                } else {
                    -ENVELOPE_ONE
                }
            }
            Function::Noise => self.sub_noise.tick_level(),
            Function::External => 0,
        }
    }

    /// A tick: the slow oscillators move on, and the voices take what the
    /// keys and the controls ask.
    fn control(&mut self) {
        let patch = self.patch;
        let keys = *self.allocator.voices();
        // One sub oscillator serves every key: the hardest pressed sets
        // its speed.
        let mut pressed = 0;
        for (voice, key) in self.voices.iter().zip(&keys) {
            if key.stage == Stage::Held {
                pressed = pressed.max(voice.pressure());
            }
        }
        if pressed != self.sub_pressed {
            self.sub_pressed = pressed;
            let volts = law::share(TOUCH_SPEED_VOLTS * OCTAVE, patch.get(Global::TouchSpeed));
            let speed = law::speed(
                SPEED_AT_FIVE_VOLTS,
                SUB_OCTAVES_A_VOLT,
                patch.get(Global::SubSpeed),
                law::scaled(volts, pressed),
            );
            self.sub.step = frequency(speed, TICK_RATE);
        }
        let sub_phase = self.sub.tick();
        let sub = self.sub_wave(sub_phase);
        let [pwm_one, pwm_two] = &mut self.pwm;
        let pwm = [sine_level(pwm_one.tick()), sine_level(pwm_two.tick())];

        let rise = self.ring_envelope.tick();
        let volts = law::share(RING_DEPTH_VOLTS * OCTAVE, patch.get(Global::RingDepth));
        let speed = law::speed(
            SPEED_AT_FIVE_VOLTS,
            FAST_OCTAVES_A_VOLT,
            patch.get(Global::RingSpeed),
            law::scaled(volts, rise),
        );
        self.ring.step = frequency(speed, SAMPLE_RATE);

        // The ribbon adds its share of the key voltage: a bend of b
        // makes every frequency 1 + b times what it was.
        let ribbon = law::share_of(RIBBON_REST.cast_signed(), self.bend, PITCH_BEND_CENTRE);
        let controls = Controls {
            sub,
            pwm,
            ribbon: RIBBON_REST.saturating_add_signed(ribbon).max(RIBBON_FLOOR),
            wheel: law::share(law::cents(WHEEL_CENTS), self.wheel),
            pedal: self.pedal,
        };
        for (index, ((voice, key), seen)) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers).enumerate() {
            if key.stage == Stage::Rest {
                continue;
            }
            if *seen != key.triggers {
                *seen = key.triggers;
                voice.start(key.key, key.velocity, &self.shared);
            }
            if !key.gate() {
                voice.release();
            }
            voice.tick(&self.channels, &self.shared, controls);
            if key.stage == Stage::Released && voice.silent() {
                self.allocator.finished(index);
            }
        }
    }

    /// One sample of the voices' sum, at their rate.
    #[inline]
    fn voices_sample(&mut self) -> i32 {
        if self.tick_wait == 0 {
            self.tick_wait = TICK_SAMPLES;
            self.control();
        }
        self.tick_wait = self.tick_wait.saturating_sub(1);
        let [noise_one, noise_two] = &mut self.noise;
        let noise = [noise_one.tick(), noise_two.tick()];
        let mut sum = 0i32;
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                sum = sum.saturating_add(voice.sample(&self.channels, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of sixteen lines and
    /// the effect, whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        // The square of the controller's share, in Q28 (own choice of
        // the law, as a volume control's taper).
        let square = i64::from(self.volume).saturating_mul(i64::from(self.volume));
        let volume = i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / VOLUME_FULL).unwrap_or(ONE_Q28);
        let ring = self.ring_modulation;
        let effect = self.patch.on(Global::Effect);
        let [tremolo_depth, chorus_depth] = self.effect_depth;
        for sample in out {
            let earlier = self.voices_sample();
            let later = self.voices_sample();
            let mut signal = self.decimator.process(earlier, later);
            if ring != 0 {
                // The sound times the oscillator takes the place of the
                // sound, by the MODULATION lever's share.
                let carrier = sine_level(self.ring.tick());
                let gain = ENVELOPE_ONE.saturating_sub(ring).saturating_add(apply(carrier, ring));
                signal = apply(signal, gain);
            }
            if effect {
                let [tremolo, chorus] = &mut self.effect;
                let moved = apply(sine_level(tremolo.tick()), tremolo_depth)
                    .saturating_sub(apply(sine_level(chorus.tick()), chorus_depth));
                let [line, _] = self.line.process(signal);
                signal = apply(
                    apply(line, EFFECT_LEVEL),
                    ENVELOPE_ONE.saturating_sub(EFFECT_MONO_SHARE << 1).saturating_add(moved),
                );
            }
            *sample = to_sample(amplify(signal, volume));
        }
    }
}
