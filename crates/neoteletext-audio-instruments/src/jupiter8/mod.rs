//! Roland's Jupiter-8 (1981): eight voices of two oscillators each, with
//! cross modulation and sync, a high-pass filter, a low-pass filter of 12
//! or 24 dB an octave, two envelopes and one low-frequency oscillator,
//! as one instrument or as two of four voices each, layered or split
//! across the keyboard; rendered from MIDI messages to 16-bit mono
//! samples at 48,000 Hz.
//!
//! # The instrument
//!
//! - **Oscillators.** Each is a sawtooth that an integrator and a
//!   comparator make, from which the other waveforms are shaped: VCO-1
//!   gives a triangle, a sawtooth, a pulse or a square, VCO-2 a sine, a
//!   sawtooth, a pulse or noise. VCO-2 can be started again by each of
//!   VCO-1's cycles (sync), can move VCO-1's frequency at audio rate
//!   (cross modulation: its sawtooth and its pulse by up to six octaves
//!   upwards, its sine by up to three either way, its noise by one and a
//!   half), and can leave the keyboard for a low range. Under cross modulation the
//!   oscillators run at four times the output rate, and VCO-1's
//!   frequency is set for every sample of 5.2 µs and held to 48 kHz;
//!   otherwise they run at twice the output rate.
//! - **Mixer and filters.** The two are mixed, pass a one-pole high-pass
//!   filter, and then the IR3109's four stages with feedback
//!   ([`OtaCascade`](crate::analog::filter::OtaCascade)), tapped after
//!   the second stage or the fourth. At the top of its slider the
//!   resonance rings and does not oscillate, as the factory trims it.
//! - **Envelopes.** Two of the IR3R01 chip, each charging a capacitor.
//!   Envelope 1 can be turned upside down; either can have its times
//!   follow the keyboard ([`law::KEY_FOLLOW`] says how, and what of it
//!   is inferred).
//! - **Two halves.** In the key mode [`KeyMode::Whole`] all eight
//!   voices play the upper patch. In [`KeyMode::Dual`] every key plays
//!   both patches, four voices each, and in [`KeyMode::Split`] the keys
//!   below the split point play the lower patch and the others the
//!   upper. Each half has its own low-frequency oscillator and its own
//!   assign mode: poly 1, poly 2, solo or unison ([`assign`](self)).
//!   The output is the instrument's MIX jack: both halves, each at its
//!   share of the balance (DEC-6437 item 2).
//! - **The bender panel**, which is no part of a patch, is
//!   [`Performance`]: how far the bender moves the oscillators and the
//!   cutoff, how far the modulation wheel lets the low-frequency
//!   oscillator move them, and the portamento.
//!
//! # MIDI
//!
//! The instrument has none: its keys, bender and pedals are mapped onto
//! MIDI as [`Jupiter8::event`] says, as this crate's Juno maps its own.
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Roland's service notes (second edition), owner's manual,
//! specifications and parameter correspondence table of the Jupiter-8,
//! Roland's factory patch sheets, and the published write-ups named
//! where each figure is used, with no code and no table from any
//! emulator or plugin. No reference program was run: none that may be is
//! open. Each constant names its source where it is defined, in [`law`],
//! the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **How a slider travels between its documented points** is an own
//!   choice for the envelopes' times, the low-frequency oscillator's
//!   rate and delay, the resonance and the key follow ([`law`]).
//! - **The envelopes' key follow** rests on a reading of the schematic
//!   and on an inference of what the chip's mode pin does
//!   ([`law::KEY_FOLLOW`]).
//! - **The high-pass filter's range, the depth of the low-frequency
//!   oscillator on the cutoff and on the amplifier, VCO-2's low range
//!   and the portamento's times** are in no document read, and are own
//!   choices.
//! - **The filter's cutoff** follows the service notes' figures for the
//!   later serial numbers, on a reading of adjustments 14 and 16 that
//!   their drawings leave open ([`law::CUTOFF_BASE`]).
//! - **Cross modulation** is bounded at 48 kHz. With the sync on, what
//!   it aliases below 20 kHz is 86 dB below the strongest harmonic or
//!   better with the slider at 5 or less and keys to the A of 880 Hz,
//!   80 dB on the A of 1,760 Hz, with VCO-2's sine or sawtooth; with
//!   its pulse VCO-1 is a plain sawtooth three octaves up at the slider's
//!   5, of which the filter folds back 77 dB and 68 dB on those two keys,
//!   and at 10 one of 28 kHz, 39 dB (`tests/jupiter8_oscillator.rs`).
//!   Without the sync, before the filter, nothing folded stands out to
//!   the C of 1,047 Hz with the slider at 2.5 or less, 90 dB there at 5,
//!   and 87 dB on the C of 4,186 Hz at 2.5 (the voice's own test, which
//!   says how). Under VCO-2's sine VCO-1 is a little flat on high keys:
//!   0.8 cent on the C of 2,093 Hz at 5. The oscillators' waveforms are the building
//!   block's ideal ones, without the glitch the instrument's triangle
//!   and sine carry where the sawtooth starts again.
//! - **The resonance at its top** is a feedback of 3.95, where the
//!   service notes' drawing of the ringing would have a little more than
//!   the filter can take without oscillating by itself.
//! - **Eight voices cost** 6 to 14 times less than real time on one core
//!   of a 3.7 GHz Intel Core i9-10900KF, the 6 under the deepest cross
//!   modulation on the highest keys.
//! - **No arpeggiator**, no tape memory, no external control voltages;
//!   the hold is the pedal alone, without the panel's two selectors: it
//!   holds both halves in whole and dual and the lower half in split, as
//!   the instrument's pedal does.
//! - **Of the bender panel**, which every factory patch sheet draws
//!   beside its patch, the LFO MOD RISE TIME knob is not here: the
//!   instrument's touch pad switches the panel's modulation on, and the
//!   knob lets it rise (owner's manual, printed page 14); the modulation
//!   wheel that stands in for the pad is moved by hand. Nor is the
//!   MASTER TUNE knob.
//! - **Velocity is ignored**, as the instrument's keyboard has none.

mod assign;
pub mod law;
pub mod patch;
mod voice;

use crate::analog::decimator::Decimator;
use crate::analog::drift::Variation;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, time_constant};
use crate::analog::filter::ONE_Q28;
use crate::analog::fixed::{amplify, mul_high};
use crate::analog::oscillator::Noise;
use crate::analog::to_sample;

use assign::Assigner;
pub use assign::VOICES;
pub use patch::{AssignMode, LfoWave, PATCH_BYTES, Param, Patch, PatchError, PwmSource, Vco1Wave, Vco2Wave};
use voice::{
    CUTOFF_TOLERANCE, Controls, Lfo, OSCILLATOR_SAMPLES, Settings, TICK_RATE, TICK_SAMPLES, TUNE_TOLERANCE, Voice,
};

/// The instrument's halves: the upper, whose patch [`KeyMode::Whole`]
/// plays, and the lower.
const HALVES: usize = 2;
/// The upper half's place.
const UPPER: usize = 0;
/// The lower half's place.
const LOWER: usize = 1;
/// Voices of a half in dual and split.
///
/// Source: owner's manual, printed page 3: "Dual and Split modes divide
/// the JP-8 into two four-voice instruments".
const HALF_VOICES: usize = VOICES / HALVES;

/// How the keyboard is shared between the two patches.
///
/// Source: owner's manual, printed page 3: "The Whole Mode operates as
/// one eight voice polyphonic instrument, while Dual and Split modes
/// divide the JP-8 into two four-voice instruments that may even be
/// assigned different patches. Split Mode divides the keyboard into
/// Lower and Upper sections while Dual Mode layers two patches from each
/// keypress."
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyMode {
    /// Eight voices of the upper patch.
    Whole,
    /// Every key plays both patches, four voices each.
    Dual,
    /// Keys below the split point play the lower patch, the others the
    /// upper, four voices each.
    Split,
}

/// Which keys the portamento glides.
///
/// Source: owner's manual, printed page 14: "polyphonic portamento that
/// may be assigned to the entire keyboard, to the Upper end of a split
/// keyboard, or switched 'Off' entirely".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Portamento {
    /// None.
    Off,
    /// Both halves.
    On,
    /// The upper half alone.
    UpperOnly,
}

/// The bender panel beside the keyboard: no part of a patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Performance {
    /// How far the bender at either end moves the oscillators that a
    /// patch lets it reach, in cents, up to [`Performance::BEND_MAX`].
    pub bend_vco: u16,
    /// How far the bender at either end moves the cutoff, in cents, up
    /// to [`Performance::CUTOFF_MAX`].
    pub bend_vcf: u16,
    /// How far the modulation wheel at its top lets the low-frequency
    /// oscillator move the pitch either way, in cents, up to
    /// [`Performance::BEND_MAX`].
    pub lfo_vco: u16,
    /// The same for the cutoff, up to [`Performance::CUTOFF_MAX`].
    pub lfo_vcf: u16,
    /// Which keys glide. MIDI controller 65 switches it too.
    pub portamento: Portamento,
    /// The portamento's time, 0 to 127; MIDI controller 5 sets it too.
    pub portamento_time: u8,
}

impl Performance {
    /// The most the bender and the wheel move the pitch: an octave.
    ///
    /// Source: service notes, adjustment 25-1, BENDER LEVEL: with the
    /// bender "at extreme left" on the key A3, "adjust VCO BEND to
    /// display 1 cycle", the one cycle that A2 showed, and with it "at
    /// extreme right" on the key A1 a trimmer is set for the same: an
    /// octave either way. The adjustment moves the panel's own VCO BEND
    /// slider to reach the octave, and its drawing has that slider's
    /// knob below the top, so the slider's top is an octave or more, by
    /// how much no document says. Own choice: the octave is the top; and
    /// the wheel's share of the oscillator the same, which adjustment
    /// 22's octave for the panel's own LFO MOD agrees with.
    pub const BEND_MAX: u16 = 1_200;
    /// The most the bender and the wheel move the cutoff: four octaves.
    ///
    /// Source: own choice; no document gives it.
    pub const CUTOFF_MAX: u16 = 4_800;

    /// The panel as a MIDI instrument usually stands: the bender two
    /// semitones, the wheel a vibrato of half a semitone, the cutoff
    /// left alone, no portamento.
    ///
    /// Source: own choice, as this crate's Juno and DX7.
    pub const DEFAULT: Self =
        Self { bend_vco: 200, bend_vcf: 0, lfo_vco: 50, lfo_vcf: 0, portamento: Portamento::Off, portamento_time: 0 };
}

impl Default for Performance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The key the keyboard splits at when the instrument is made: keys
/// from it up play the upper patch.
///
/// Source: own choice: middle C. Service notes, page 32: the first
/// instruments had a "fixed key split point", later ones a variable one;
/// neither page says where the fixed one lay.
pub const SPLIT_DEFAULT: u8 = 60;
/// The balance at which both halves are at their full level.
pub const BALANCE_CENTRE: u8 = 64;

/// The pitch bend value that bends nothing.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// A switch controller is on at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const SWITCH_ON: u8 = 64;
/// MIDI status: note off.
const STATUS_NOTE_OFF: u8 = 0x80;
/// MIDI status: note on.
const STATUS_NOTE_ON: u8 = 0x90;
/// MIDI status: control change.
const STATUS_CONTROL_CHANGE: u8 = 0xb0;
/// MIDI status: pitch bend.
const STATUS_PITCH_BEND: u8 = 0xe0;
/// Controller: modulation wheel.
const CONTROL_MODULATION_WHEEL: u8 = 1;
/// Controller: portamento time.
const CONTROL_PORTAMENTO_TIME: u8 = 5;
/// Controller: volume.
const CONTROL_VOLUME: u8 = 7;
/// Controller: balance.
const CONTROL_BALANCE: u8 = 8;
/// Controller: sustain pedal, the instrument's hold.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: portamento on or off.
const CONTROL_PORTAMENTO: u8 = 65;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// The seed of the instrument's noise and of its voices' tolerances.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4a75_7038;

/// The portamento's longest time constant, in millionths of a second,
/// with the time at 127.
///
/// Source: own choice. Owner's manual, printed page 14: "Pitch changes
/// are instantaneous with the Portamento control set at '0', increasing
/// in the time needed between notes as the control is turned clockwise";
/// no document gives a time. The glide is the charging of a capacitor
/// (service notes, interface board schematic, "S/H PORTAMENTO"), so it
/// covers the same share of any interval in the same time.
const PORTAMENTO_LONGEST: u32 = 1_000_000;
/// The most of its whole way that a half's share of the output moves in
/// a sample when the balance is moved: all of it in 256 samples, 5.3 ms,
/// the time a volume controller's gain takes
/// ([`VOLUME_SLEW`](crate::analog::volume::VOLUME_SLEW)).
///
/// Source: own choice. The balance is a potentiometer between the two
/// halves' outputs; its shares were worked out once for each call to
/// render, so a balance message moved them between one sample and the
/// next.
const BALANCE_SLEW: i32 = ENVELOPE_ONE >> 8;
/// The most of its travel the bender moves in a tick: from its centre
/// to an end in 128 ticks, 21 ms.
///
/// Source: own choice. The bender is a lever under a hand, which passes
/// every place between two; a pitch bend message puts it at its new
/// place in no time, and where the bender panel sends it to the cutoff
/// too, the filter moved two octaves between one tick and the next: a
/// step 1.9 times the note's own largest
/// (`tests/jupiter8_control_sweep.rs`). The time is long against a
/// tick and short against a hand.
const BEND_SLEW: i32 = PITCH_BEND_CENTRE >> 7;
/// Messages kept while all sound off takes the sound out, to be taken
/// when it has gone: more than come in the 5.3 ms that lasts but from
/// a stream that means to flood, whose next message ends the wait.
///
/// Source: own choice.
const DEFERRED: usize = 16;
/// The share of the mixed output that is a sample, in Q28: 659 / 1000.
/// One voice's sawtooth through an open filter at the amplifier's full
/// level then peaks 18.2 dB below full scale, and eight struck together
/// 3.2 dB below.
/// What nears full scale is bent under it by the output's limiter
/// ([`crate::output::limit`]).
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's half, as it was first, the note stood 2.4 dB below it.
const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 659 / 1000) as i32;
/// Cents in an octave.
const CENTS: i32 = 1_200;

/// The Jupiter-8.
///
/// It holds no reference and allocates nothing. Two instruments given
/// the same patches, settings and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Jupiter8 {
    key_mode: KeyMode,
    split: u8,
    balance: u8,
    performance: Performance,
    patches: [Patch; HALVES],
    /// Each patch's sliders as they stand, on their way to where the
    /// patch has them, with the patch's switches: what the voices are
    /// given ([`Jupiter8::slide`]).
    panels: [Patch; HALVES],
    /// The upper and the lower half's share of the output as it stands,
    /// in Q30, on its way to what the balance asks.
    shares: [i32; HALVES],
    /// All sound off has come and the sound is on its way out: the gain
    /// that takes it there, which the voices are cut at the end of; and
    /// the messages that came meanwhile, in order, and how many.
    cut: Option<crate::analog::volume::VolumeGain>,
    deferred: [(u8, u8, u8); DEFERRED],
    deferred_count: usize,
    settings: [Settings; HALVES],
    assigners: [Assigner; HALVES],
    lfos: [Lfo; HALVES],
    noises: [Noise; HALVES],
    voices: [Voice; VOICES],
    /// The trigger count and the key each voice last acted on.
    seen: [(u32, u8); VOICES],
    /// Whether the hold pedal is down.
    pedal: bool,
    /// The pitch bend, less its centre: where the last message put the
    /// bender, and where it stands on its way there.
    bend: i32,
    bend_now: i32,
    wheel: u8,
    volume: u8,
    /// The volume's gain as it stands, in Q28, on its way to what the
    /// controller asks.
    volume_gain: crate::analog::volume::VolumeGain,
    /// Samples of the filters until the next tick.
    tick_wait: u32,
    decimator: Decimator,
    ignored: u64,
}

impl Default for Jupiter8 {
    fn default() -> Self {
        Self::new()
    }
}

impl Jupiter8 {
    /// An instrument at rest: [`KeyMode::Whole`], both patches
    /// [`Patch::DEFAULT`], the bender panel [`Performance::DEFAULT`].
    #[must_use]
    pub fn new() -> Self {
        let settings = Settings::new(&Patch::DEFAULT);
        let mut number = 0u32;
        let voices = core::array::from_fn(|_| {
            let mut variation = Variation::new(SEED, number);
            number = number.wrapping_add(1);
            let tune = [variation.offset(TUNE_TOLERANCE), variation.offset(TUNE_TOLERANCE)];
            Voice::new(tune, variation.offset(CUTOFF_TOLERANCE))
        });
        let mut jupiter = Self {
            key_mode: KeyMode::Whole,
            split: SPLIT_DEFAULT,
            balance: BALANCE_CENTRE,
            performance: Performance::DEFAULT,
            patches: [Patch::DEFAULT; HALVES],
            panels: [Patch::DEFAULT; HALVES],
            shares: Self::shares(BALANCE_CENTRE),
            cut: None,
            deferred: [(0, 0, 0); DEFERRED],
            deferred_count: 0,
            settings: [settings; HALVES],
            assigners: [Assigner::new(AssignMode::Poly1, VOICES), Assigner::new(AssignMode::Poly1, 0)],
            lfos: [Lfo::new(0), Lfo::new(1)],
            noises: [Noise::new(SEED), Noise::new(SEED.rotate_left(16))],
            voices,
            seen: [(0, 0); VOICES],
            pedal: false,
            bend: 0,
            bend_now: 0,
            wheel: 0,
            volume: DATA_MAX,
            volume_gain: crate::analog::volume::VolumeGain::full(),
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        jupiter.apply();
        jupiter
    }

    /// Puts the instrument back as [`Jupiter8::new`] made it, but for
    /// its patches, key mode, split point, balance and bender panel,
    /// which it keeps: silent, its controllers at rest, the count of
    /// ignored messages at zero.
    pub fn reset(&mut self) {
        let mut fresh = Self::new();
        fresh.key_mode = self.key_mode;
        fresh.split = self.split;
        fresh.balance = self.balance;
        fresh.performance = self.performance;
        fresh.patches = self.patches;
        fresh.panels = self.patches;
        fresh.shares = Self::shares(self.balance);
        fresh.apply();
        *self = fresh;
    }

    /// Gives the instrument its upper patch, which [`KeyMode::Whole`]
    /// plays on every voice. It takes effect on the voices that are
    /// sounding too, as moving the panel's controls does: the switches
    /// and selectors at once, and each slider a step of its 256 at every
    /// tick, its whole travel in 43 thousandths of a second, as a hand
    /// moves it through every place between. With no voice sounding,
    /// all of it is there at once. A change of the assign mode releases
    /// the voices it assigns, those the hold kept too, and a pedal that
    /// is down holds what is played after.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.take_patch(UPPER, patch);
    }

    /// Gives the instrument its lower patch, which dual and split play.
    pub fn set_lower_patch(&mut self, patch: &Patch) {
        self.take_patch(LOWER, patch);
    }

    /// Whether a control is a slider, which passes every place between
    /// two, and not a switch or a selector, which has none between: the
    /// controls that hold every value of a byte.
    fn slides(place: usize) -> bool {
        patch::MAXIMA.get(place).is_some_and(|&most| most == u8::MAX)
    }

    fn take_patch(&mut self, half: usize, patch: &Patch) {
        let sounding = self.sounding_voices() > 0;
        if let (Some(kept), Some(panel)) = (self.patches.get_mut(half), self.panels.get_mut(half)) {
            *kept = *patch;
            let mut bytes = patch.to_bytes();
            if sounding {
                for ((byte, held), place) in bytes.iter_mut().zip(panel.to_bytes()).zip(0usize..) {
                    if Self::slides(place) {
                        *byte = held;
                    }
                }
            }
            *panel = Patch::from_bytes(&bytes).unwrap_or(*patch);
        }
        self.apply();
    }

    /// A tick of the sliders: each goes one step towards where its patch
    /// has it.
    ///
    /// Source: own choice of the rate. The instrument's processor reads
    /// its sliders and sends each voltage to the voices through a
    /// converter and a sample-and-hold; the service notes' drawings of
    /// them were not read for the time a voltage takes, so the time is
    /// not the circuit's. A step of a 256th at each tick of a sixth of a
    /// millisecond is less than a sample-and-hold's own step would be.
    fn slide(&mut self) {
        if self.panels == self.patches {
            return;
        }
        for (panel, patch) in self.panels.iter_mut().zip(&self.patches) {
            let mut bytes = panel.to_bytes();
            for (held, wanted) in bytes.iter_mut().zip(patch.to_bytes()) {
                *held = match (*held).cmp(&wanted) {
                    core::cmp::Ordering::Less => held.saturating_add(1),
                    core::cmp::Ordering::Equal => *held,
                    core::cmp::Ordering::Greater => held.saturating_sub(1),
                };
            }
            *panel = Patch::from_bytes(&bytes).unwrap_or(*patch);
        }
        self.settle();
    }

    /// The upper patch.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        let [upper, _] = &self.patches;
        upper
    }

    /// The lower patch.
    #[must_use]
    pub const fn lower_patch(&self) -> &Patch {
        let [_, lower] = &self.patches;
        lower
    }

    /// Sets how the keyboard is shared between the patches. A change
    /// between whole and the other two releases every voice, and a
    /// change to split releases what the hold kept of the upper half; a
    /// pedal that is down holds what is played after, in the halves it
    /// reaches ([`Jupiter8::event`]).
    pub fn set_key_mode(&mut self, key_mode: KeyMode) {
        let moved = (self.key_mode == KeyMode::Whole) != (key_mode == KeyMode::Whole);
        self.key_mode = key_mode;
        self.apply();
        if moved {
            // The last four voices now read another assigner's slots,
            // which count their triggers by themselves: what each voice
            // last acted on is its new slot as it stands, released, or a
            // key whose count and number happened to match the old
            // slot's would not start its voice.
            for index in 0..VOICES {
                let (half, place) = self.place(index);
                let slot = self.assigners.get(half).and_then(|assigner| assigner.slots().get(place));
                if let (Some(seen), Some(slot)) = (self.seen.get_mut(index), slot) {
                    *seen = (slot.triggers, slot.key);
                }
            }
        }
    }

    /// The key mode.
    #[must_use]
    pub const fn key_mode(&self) -> KeyMode {
        self.key_mode
    }

    /// Sets the split point: in [`KeyMode::Split`] the keys from it up
    /// play the upper patch. A key above 127 is taken as 127.
    pub const fn set_split(&mut self, key: u8) {
        self.split = if key > DATA_MAX { DATA_MAX } else { key };
    }

    /// Sets the balance of the two halves, 0 to 127: at 0 the lower half
    /// alone, at [`BALANCE_CENTRE`] both in full, at 127 the upper alone.
    /// In [`KeyMode::Whole`] the halves are the first four voices and the
    /// last four, as they are in the instrument. MIDI controller 8 sets
    /// it too.
    pub const fn set_balance(&mut self, balance: u8) {
        self.balance = if balance > DATA_MAX { DATA_MAX } else { balance };
    }

    /// Sets the bender panel; each depth is held to its most.
    pub fn set_performance(&mut self, performance: &Performance) {
        self.performance = Performance {
            bend_vco: performance.bend_vco.min(Performance::BEND_MAX),
            bend_vcf: performance.bend_vcf.min(Performance::CUTOFF_MAX),
            lfo_vco: performance.lfo_vco.min(Performance::BEND_MAX),
            lfo_vcf: performance.lfo_vcf.min(Performance::CUTOFF_MAX),
            portamento: performance.portamento,
            portamento_time: performance.portamento_time.min(DATA_MAX),
        };
    }

    /// The bender panel.
    #[must_use]
    pub const fn performance(&self) -> &Performance {
        &self.performance
    }

    /// The half a voice belongs to, and its place among that half's
    /// voices.
    const fn place(&self, voice: usize) -> (usize, usize) {
        match self.key_mode {
            KeyMode::Whole => (UPPER, voice),
            KeyMode::Dual | KeyMode::Split => (voice / HALF_VOICES, voice % HALF_VOICES),
        }
    }

    fn apply(&mut self) {
        let counts = match self.key_mode {
            KeyMode::Whole => [VOICES, 0],
            KeyMode::Dual | KeyMode::Split => [HALF_VOICES; HALVES],
        };
        for ((assigner, patch), count) in self.assigners.iter_mut().zip(&self.patches).zip(counts) {
            assigner.configure(patch.assign_mode(), count);
        }
        self.settle();
        // A change of mode let every hold go: the pedal still stands.
        self.hold();
    }

    /// The voices and the low-frequency oscillators take the panels as
    /// they stand.
    fn settle(&mut self) {
        for ((panel, settings), lfo) in self.panels.iter().zip(&mut self.settings).zip(&mut self.lfos) {
            *settings = Settings::new(panel);
            lfo.configure(panel);
        }
        for index in 0..VOICES {
            let (half, _) = self.place(index);
            if let (Some(voice), Some(settings)) = (self.voices.get_mut(index), self.settings.get(half)) {
                voice.configure(settings);
            }
        }
    }

    /// The upper and the lower half's share of the output for a
    /// balance, in Q30: each is in full from the centre to its own end,
    /// and falls in a straight line to nothing at the other (own
    /// choice).
    fn shares(balance: u8) -> [i32; HALVES] {
        let share = |towards: u8, centre: u8| {
            let level =
                i64::from(towards.min(centre)).saturating_mul(i64::from(ENVELOPE_ONE)).checked_div(i64::from(centre));
            i32::try_from(level.unwrap_or(0)).unwrap_or(ENVELOPE_ONE)
        };
        let upper = share(balance, BALANCE_CENTRE);
        let lower = share(DATA_MAX.saturating_sub(balance), DATA_MAX.saturating_sub(BALANCE_CENTRE));
        let mut shares = [0; HALVES];
        if let Some(kept) = shares.get_mut(UPPER) {
            *kept = upper;
        }
        if let Some(kept) = shares.get_mut(LOWER) {
            *kept = lower;
        }
        shares
    }

    /// Gives each half the hold the pedal asks of it.
    ///
    /// Source: owner's manual, printed page 13, "Hold Selectors": "In
    /// either the Whole or Dual Key Modes, the DP-2 will activate the
    /// Hold function for the entire keyboard ... When used with the Split
    /// Key Mode, the DP-2 will activate the Hold function on the Lower
    /// end of the keyboard only".
    fn hold(&mut self) {
        let split = self.key_mode == KeyMode::Split;
        for (assigner, half) in self.assigners.iter_mut().zip(0usize..) {
            assigner.hold(self.pedal && !(split && half == UPPER));
        }
    }

    /// How many messages [`Jupiter8::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active()).count()
    }

    /// Takes one MIDI 1.0 channel voice message. The instrument is one
    /// part and hears every channel.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is not used.
    /// - Control change (0xBn) of the modulation wheel (1: the bender
    ///   panel's LFO modulation, as far as [`Performance`] says),
    ///   portamento time (5), volume (7), balance (8), the sustain pedal
    ///   (64: the hold, on from 64; of the lower half alone in
    ///   [`KeyMode::Split`]), portamento (65: on from 64 for both
    ///   halves, off below), all sound off (120, which takes the sound
    ///   out over five thousandths of a second, then silences every
    ///   voice, and leaves the hold as the pedal has it; a message that
    ///   comes before then is taken when the sound has gone), reset all
    ///   controllers (121, which leaves the volume, the balance and the
    ///   portamento) and all notes off (123). The balance moves to its
    ///   new place over five thousandths of a second.
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven: the bender,
    ///   which goes to its new place at an end's travel in 21
    ///   thousandths of a second.
    ///
    /// Anything else is ignored and counted
    /// ([`Jupiter8::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        // While all sound off takes the sound out, a message waits for
        // it to have gone; with no room to wait, the sound is cut now.
        if self.cut.is_some() {
            if let Some(slot) = self.deferred.get_mut(self.deferred_count) {
                *slot = (status, data1, data2);
                self.deferred_count = self.deferred_count.saturating_add(1);
                return;
            }
            self.cut();
        }
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & 0xf0 {
            STATUS_NOTE_OFF => self.key_up(data1),
            STATUS_NOTE_ON if data2 == 0 => self.key_up(data1),
            STATUS_NOTE_ON => self.key_down(data1),
            STATUS_CONTROL_CHANGE => self.control_change(data1, data2),
            STATUS_PITCH_BEND => {
                self.bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
            }
            _ => self.ignore(),
        }
    }

    const fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn key_down(&mut self, key: u8) {
        let halves = match self.key_mode {
            KeyMode::Whole => [true, false],
            KeyMode::Dual => [true, true],
            KeyMode::Split => [key >= self.split, key < self.split],
        };
        for ((assigner, lfo), plays) in self.assigners.iter_mut().zip(&mut self.lfos).zip(halves) {
            if plays {
                // The oscillator's delay starts with a key when no other
                // is held (service notes, adjustment 21: "approx. 4 sec
                // after the key first depressed").
                if !assigner.any_held() {
                    lfo.key();
                }
                assigner.key_down(key);
            }
        }
    }

    fn key_up(&mut self, key: u8) {
        // Both halves: the split point may have moved since the key went
        // down, and a half that does not hold the key ignores it.
        for assigner in &mut self.assigners {
            assigner.key_up(key);
        }
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            CONTROL_MODULATION_WHEEL => self.wheel = value,
            CONTROL_PORTAMENTO_TIME => self.performance.portamento_time = value,
            CONTROL_VOLUME => self.volume = value,
            CONTROL_BALANCE => self.balance = value,
            CONTROL_SUSTAIN => {
                self.pedal = value >= SWITCH_ON;
                self.hold();
            }
            CONTROL_PORTAMENTO => {
                self.performance.portamento = if value >= SWITCH_ON { Portamento::On } else { Portamento::Off };
            }
            // The sound goes out over the 5.3 ms a volume's gain takes,
            // and the voices are cut and the keys let go when it has
            // gone: cut where they stood, the signal would fall to
            // nothing in one sample.
            CONTROL_ALL_SOUND_OFF => {
                if self.sounding_voices() == 0 {
                    self.cut();
                } else if self.cut.is_none() {
                    self.cut = Some(crate::analog::volume::VolumeGain::full());
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                self.hold();
            }
            CONTROL_ALL_NOTES_OFF => {
                // As a note off for every key: the hold still holds.
                for assigner in &mut self.assigners {
                    assigner.keys_up();
                }
            }
            _ => self.ignore(),
        }
    }

    /// Silences every voice at once and lets every key go: what all
    /// sound off comes to. The pedal has not moved, and its hold is
    /// given back; the messages that waited are then taken, in order.
    fn cut(&mut self) {
        self.cut = None;
        for assigner in &mut self.assigners {
            assigner.release_all();
        }
        self.hold();
        for index in 0..VOICES {
            let (half, _) = self.place(index);
            if let (Some(voice), Some(settings)) = (self.voices.get_mut(index), self.settings.get(half)) {
                voice.cut(settings);
            }
        }
        self.panels = self.patches;
        self.settle();
        let (deferred, count) = (self.deferred, self.deferred_count);
        self.deferred_count = 0;
        for &(status, data1, data2) in deferred.iter().take(count) {
            self.event(status, data1, data2);
        }
    }

    /// Releases every voice, held by a key or by the hold, and lets the
    /// hold go: for when the input stops and no note off will come.
    pub fn release_all(&mut self) {
        self.pedal = false;
        for assigner in &mut self.assigners {
            assigner.release_all();
        }
    }

    /// A tick: the voices take what the keys and the controls ask.
    fn control(&mut self) {
        self.slide();
        let cents = |cents: u16| law::octaves(i32::from(cents), CENTS);
        let performance = self.performance;
        let step = self.bend.saturating_sub(self.bend_now).clamp(-BEND_SLEW, BEND_SLEW);
        self.bend_now = self.bend_now.saturating_add(step);
        let bend = law::share_of(cents(performance.bend_vco), self.bend_now, PITCH_BEND_CENTRE);
        let bend_cutoff = law::share_of(cents(performance.bend_vcf), self.bend_now, PITCH_BEND_CENTRE);
        let wheel = |cents_at_top: u16| law::share_of(cents(cents_at_top), i32::from(self.wheel), i32::from(DATA_MAX));
        let (wheel_pitch, wheel_cutoff) = (wheel(performance.lfo_vco), wheel(performance.lfo_vcf));
        let time = law::share_of(
            i32::try_from(PORTAMENTO_LONGEST).unwrap_or(i32::MAX),
            i32::from(performance.portamento_time),
            i32::from(DATA_MAX),
        );
        let glide = time_constant(time.cast_unsigned(), TICK_RATE);

        let mut controls = [Controls { lfo: 0, bend, vibrato: 0, cutoff: 0, glide: IMMEDIATE }; HALVES];
        for ((controls, lfo), half) in controls.iter_mut().zip(&mut self.lfos).zip(0usize..) {
            let (full, delayed) = lfo.tick();
            controls.lfo = delayed;
            controls.vibrato = mul_high(wheel_pitch, full) << 2;
            controls.cutoff = bend_cutoff.saturating_add(mul_high(wheel_cutoff, full) << 2);
            let glides = match performance.portamento {
                Portamento::Off => false,
                Portamento::On => true,
                Portamento::UpperOnly => half == UPPER,
            };
            if glides {
                controls.glide = glide;
            }
        }

        for index in 0..VOICES {
            let (half, place) = self.place(index);
            let (Some(voice), Some(seen), Some(settings), Some(controls)) =
                (self.voices.get_mut(index), self.seen.get_mut(index), self.settings.get(half), controls.get(half))
            else {
                continue;
            };
            let Some(slot) = self.assigners.get(half).and_then(|assigner| assigner.slots().get(place)) else {
                continue;
            };
            if slot.gate && *seen != (slot.triggers, slot.key) {
                voice.start(slot.key, seen.0 != slot.triggers, settings);
                *seen = (slot.triggers, slot.key);
            }
            if !voice.active() {
                continue;
            }
            if !slot.gate {
                voice.release(slot.damped);
            }
            voice.tick(settings, *controls);
        }
    }

    /// One sample of the two halves' voices, at the filters' rate.
    #[inline]
    fn voices_sample(&mut self) -> [i32; HALVES] {
        if self.tick_wait == 0 {
            self.tick_wait = TICK_SAMPLES;
            self.control();
        }
        self.tick_wait = self.tick_wait.saturating_sub(1);
        let mut noise = [[0i32; OSCILLATOR_SAMPLES]; HALVES];
        for (noise, generator) in noise.iter_mut().zip(&mut self.noises) {
            for sample in noise {
                *sample = generator.tick();
            }
        }
        // The first four voices are the upper boards' and the last four
        // the lower's, whatever the key mode: the balance is theirs.
        let mut sums = [0i32; HALVES];
        let whole = self.key_mode == KeyMode::Whole;
        for (index, voice) in self.voices.iter_mut().enumerate() {
            if !voice.active() {
                continue;
            }
            let board = index / HALF_VOICES;
            let half = if whole { UPPER } else { board };
            if let (Some(settings), Some(&noise), Some(sum)) =
                (self.settings.get(half), noise.get(half), sums.get_mut(board))
            {
                *sum = sum.wrapping_add(voice.sample(settings, noise));
            }
        }
        sums
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of eight voices,
    /// whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        let mut volume = crate::analog::volume::wanted(self.volume);
        let mut wanted = Self::shares(self.balance);
        for sample in out {
            for (share, &wanted) in self.shares.iter_mut().zip(&wanted) {
                let step = wanted.saturating_sub(*share).clamp(-BALANCE_SLEW, BALANCE_SLEW);
                *share = share.saturating_add(step);
            }
            let [upper, lower] = self.shares;
            let mixed = |[first, second]: [i32; HALVES]| {
                (mul_high(first, upper) << 2).wrapping_add(mul_high(second, lower) << 2)
            };
            let mut earlier = mixed(self.voices_sample());
            let mut later = mixed(self.voices_sample());
            // Faded before the decimator, which then ends on silence:
            // what it still holds when the voices are cut is nothing.
            if let Some(cut) = &mut self.cut {
                let gain = cut.towards(0);
                (earlier, later) = (amplify(earlier, gain), amplify(later, gain));
                if gain == 0 {
                    // The messages that waited are taken now, the
                    // volume's and the balance's among them.
                    self.cut();
                    volume = crate::analog::volume::wanted(self.volume);
                    wanted = Self::shares(self.balance);
                }
            }
            let signal = self.decimator.process(earlier, later);
            *sample = to_sample(amplify(amplify(signal, self.volume_gain.towards(volume)), OUTPUT_GAIN));
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "a test"
)]
mod tests {
    extern crate std;
    use super::{AssignMode, HALF_VOICES, HALVES, Jupiter8, KeyMode, LOWER, Param, Patch, UPPER, VOICES};
    use core::fmt::Write;
    use std::collections::BTreeSet;
    use std::{string::String, vec, vec::Vec};

    /// Marsaglia's xorshift in 64 bits: the same sequence every time.
    struct Random(u64);

    impl Random {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 20) % bound
        }
    }

    /// Voices a half assigns in the key mode.
    fn voices(jupiter: &Jupiter8, half: usize) -> usize {
        match jupiter.key_mode {
            KeyMode::Whole => [VOICES, 0][half],
            KeyMode::Dual | KeyMode::Split => HALF_VOICES,
        }
    }

    /// The voices of a half whose gate is on, each with its key.
    fn gated(jupiter: &Jupiter8, half: usize) -> Vec<(usize, u8)> {
        let slots = jupiter.assigners[half].slots();
        (0..voices(jupiter, half)).filter(|&place| slots[place].gate).map(|place| (place, slots[place].key)).collect()
    }

    /// What holds whatever was played; `down` are the keys under a
    /// finger.
    fn check(jupiter: &Jupiter8, down: &BTreeSet<u8>, log: &str) {
        for half in 0..HALVES {
            let gated = gated(jupiter, half);
            let held = jupiter.pedal && !(jupiter.key_mode == KeyMode::Split && half == UPPER);
            for &(place, key) in &gated {
                assert!(down.contains(&key) || held, "half {half}, voice {place}: key {key} is up, and no hold\n{log}");
            }
            match jupiter.patches[half].assign_mode() {
                AssignMode::Poly1 | AssignMode::Poly2 => {
                    let keys: BTreeSet<u8> = gated.iter().map(|&(_, key)| key).collect();
                    assert_eq!(keys.len(), gated.len(), "half {half}: two voices on one key, {gated:?}\n{log}");
                }
                AssignMode::Solo => assert!(gated.len() <= 1, "half {half}: solo on {gated:?}\n{log}"),
                AssignMode::Unison => {
                    assert!(gated.is_empty() || gated.len() == voices(jupiter, half), "half {half}: {gated:?}\n{log}");
                }
            }
        }
    }

    #[test]
    fn random_playing_leaves_no_voice_stuck_and_no_key_without_its_voice() {
        // Keys, the pedal, the key mode, each half's assign mode, the
        // split point and rendering, in any order (owner's manual,
        // printed pages 12 and 13, for what must hold):
        // - a voice's gate is on only for a key that is down, or under a
        //   hold;
        // - in the poly modes no two voices of a half have one key, and
        //   a key gets a voice if one is free, or kept by the hold alone,
        //   or has that key already;
        // - solo has one voice, on the key struck or a lower one;
        // - unison has every voice of the half or none, and a key
        //   struck while fewer keys than voices sound is among them;
        // - after a tick, a voice whose gate is on sounds;
        // - with every key and the pedal up, every voice dies away.
        let sequences: u64 = if cfg!(debug_assertions) { 100 } else { 1_500 };
        for seed in 1..=sequences {
            let mut random = Random(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut jupiter = Jupiter8::new();
            let mut down: BTreeSet<u8> = BTreeSet::new();
            let mut log = String::new();
            let mut out = [0i16; 40];
            for _ in 0..400 {
                match random.below(100) {
                    0..=39 => {
                        let key = 48 + random.below(24) as u8;
                        let halves: &[usize] = match jupiter.key_mode {
                            KeyMode::Whole => &[UPPER],
                            KeyMode::Dual => &[UPPER, LOWER],
                            KeyMode::Split => {
                                if key >= jupiter.split {
                                    &[UPPER]
                                } else {
                                    &[LOWER]
                                }
                            }
                        };
                        let before: Vec<Vec<(usize, u8)>> = halves.iter().map(|&half| gated(&jupiter, half)).collect();
                        jupiter.event(0x90, key, 100);
                        let _ = write!(log, "down {key}; ");
                        for (&half, before) in halves.iter().zip(before) {
                            let count = voices(&jupiter, half);
                            let after = gated(&jupiter, half);
                            let kept = before.iter().any(|&(_, sounding)| !down.contains(&sounding));
                            let had = before.iter().any(|&(_, sounding)| sounding == key);
                            let has = after.iter().any(|&(_, sounding)| sounding == key);
                            let keys: BTreeSet<u8> = before.iter().map(|&(_, sounding)| sounding).collect();
                            let owed = match jupiter.patches[half].assign_mode() {
                                AssignMode::Poly1 | AssignMode::Poly2 => before.len() < count || kept || had,
                                AssignMode::Solo => {
                                    assert!(after.len() == 1 && after[0].1 <= key, "half {half}: {after:?}\n{log}");
                                    false
                                }
                                AssignMode::Unison => keys.len() < count,
                            };
                            assert!(
                                !owed || has,
                                "half {half}: key {key} has no voice, {before:?} to {after:?}\n{log}"
                            );
                        }
                        down.insert(key);
                    }
                    40..=64 => {
                        let key = match down.iter().nth(random.below(down.len().max(1) as u64) as usize) {
                            Some(&key) if random.below(4) != 0 => key,
                            _ => 48 + random.below(24) as u8,
                        };
                        // A note off, or a note on of no velocity.
                        jupiter.event(if random.below(2) == 0 { 0x80 } else { 0x90 }, key, 0);
                        down.remove(&key);
                        let _ = write!(log, "up {key}; ");
                    }
                    65..=74 => {
                        let on = random.below(2) == 0;
                        jupiter.event(0xb0, 64, if on { 127 } else { 0 });
                        let _ = write!(log, "pedal {on}; ");
                    }
                    75..=79 => {
                        let mode = [KeyMode::Whole, KeyMode::Dual, KeyMode::Split][random.below(3) as usize];
                        jupiter.set_key_mode(mode);
                        let _ = write!(log, "{mode:?}; ");
                    }
                    80..=84 => {
                        // A patch of another assign mode, with or without
                        // cross modulation: the oscillators' rate.
                        let (mode, cross, upper) =
                            (random.below(4) as u8, random.below(2) as u8 * 40, random.below(2) == 0);
                        let patch = Patch::DEFAULT.with(Param::AssignMode, mode).with(Param::CrossMod, cross);
                        if upper {
                            jupiter.set_patch(&patch);
                        } else {
                            jupiter.set_lower_patch(&patch);
                        }
                        let _ = write!(log, "{} patch, assign mode {mode}; ", if upper { "upper" } else { "lower" });
                    }
                    85..=87 => {
                        let split = 52 + random.below(16) as u8;
                        jupiter.set_split(split);
                        let _ = write!(log, "split at {split}; ");
                    }
                    88 => {
                        jupiter.event(0xb0, 123, 0);
                        down.clear();
                        log.push_str("all notes off; ");
                    }
                    89 => {
                        jupiter.event(0xb0, 121, 0);
                        log.push_str("reset controllers; ");
                    }
                    _ => {
                        // Eight samples or more hold a tick.
                        let length = 8 + random.below(32) as usize;
                        jupiter.render(&mut out[..length]);
                        let _ = write!(log, "render {length}; ");
                        for index in 0..VOICES {
                            let (half, place) = jupiter.place(index);
                            let slot = jupiter.assigners[half].slots()[place];
                            assert!(
                                !slot.gate || jupiter.voices[index].active(),
                                "voice {index} has key {} and is silent\n{log}",
                                slot.key
                            );
                        }
                    }
                }
                check(&jupiter, &down, &log);
            }
            for &key in &down {
                jupiter.event(0x80, key, 0);
            }
            jupiter.event(0xb0, 64, 0);
            for half in 0..HALVES {
                assert!(gated(&jupiter, half).is_empty(), "half {half} holds {:?}\n{log}", gated(&jupiter, half));
            }
            // The default patch's release is its shortest.
            let mut silence = vec![0i16; 24_000];
            jupiter.render(&mut silence);
            assert_eq!(jupiter.sounding_voices(), 0, "voices never freed\n{log}");
        }
    }
}
