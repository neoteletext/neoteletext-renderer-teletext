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
//!   VCO-1's cycles (sync), can move VCO-1's frequency by up to six
//!   octaves either way at audio rate (cross modulation), and can leave
//!   the keyboard for a low range. Under cross modulation the
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
//!   it aliases below 20 kHz is 70 dB below the strongest harmonic or
//!   better with the slider at 5 or less and keys to the A of 880 Hz,
//!   56 dB on the A of 1,760 Hz, and with the slider at 10 as little as
//!   30 dB (`tests/jupiter8_oscillator.rs`); without the sync it could
//!   not be measured. The oscillators' waveforms are the building
//!   block's ideal ones, without the glitch the instrument's triangle
//!   and sine carry where the sawtooth starts again.
//! - **The resonance at its top** is a feedback of 3.95, where the
//!   service notes' drawing of the ringing would have a little more than
//!   the filter can take without oscillating by itself.
//! - **Eight voices cost** 9 to 14 times less than real time on one core
//!   of a 3.7 GHz Intel Core i9-10900KF.
//! - **No arpeggiator**, no tape memory, no external control voltages;
//!   the hold is one pedal for both halves.
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
    /// bender "at extreme left" the key A3 is trimmed to show the one
    /// cycle that A2 showed, and with it "at extreme right" the key A1:
    /// an octave either way with the sensitivity at its top. Own choice:
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
/// The right shift of the mixed output before it is a sample: one voice's
/// sawtooth through an open filter at the amplifier's full level then
/// peaks 18 dB below full scale, and eight together come to about full
/// scale. What is beyond is saturated.
///
/// Source: own choice, as for this crate's Juno.
const OUTPUT_SHIFT: u32 = 1;
/// The square of the largest volume, which the volume's square is a share
/// of.
const VOLUME_FULL: i64 = (DATA_MAX as i64) * (DATA_MAX as i64);
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
    settings: [Settings; HALVES],
    assigners: [Assigner; HALVES],
    lfos: [Lfo; HALVES],
    noises: [Noise; HALVES],
    voices: [Voice; VOICES],
    /// The trigger count and the key each voice last acted on.
    seen: [(u32, u8); VOICES],
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
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
            settings: [settings; HALVES],
            assigners: [Assigner::new(AssignMode::Poly1, VOICES), Assigner::new(AssignMode::Poly1, 0)],
            lfos: [Lfo::new(0), Lfo::new(1)],
            noises: [Noise::new(SEED), Noise::new(SEED.rotate_left(16))],
            voices,
            seen: [(0, 0); VOICES],
            bend: 0,
            wheel: 0,
            volume: DATA_MAX,
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
        fresh.apply();
        *self = fresh;
    }

    /// Gives the instrument its upper patch, which [`KeyMode::Whole`]
    /// plays on every voice. It takes effect at once, on the voices that
    /// are sounding too, as moving the panel's controls does; a change
    /// of the assign mode releases the voices it assigns.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patches[UPPER] = *patch;
        self.apply();
    }

    /// Gives the instrument its lower patch, which dual and split play.
    pub fn set_lower_patch(&mut self, patch: &Patch) {
        self.patches[LOWER] = *patch;
        self.apply();
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
    /// releases every voice.
    pub fn set_key_mode(&mut self, key_mode: KeyMode) {
        self.key_mode = key_mode;
        self.apply();
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
        for (((assigner, patch), settings), (lfo, count)) in
            self.assigners.iter_mut().zip(&self.patches).zip(&mut self.settings).zip(self.lfos.iter_mut().zip(counts))
        {
            *settings = Settings::new(patch);
            assigner.configure(patch.assign_mode(), count);
            lfo.configure(patch);
        }
        for index in 0..VOICES {
            let (half, _) = self.place(index);
            if let (Some(voice), Some(settings)) = (self.voices.get_mut(index), self.settings.get(half)) {
                voice.configure(settings);
            }
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
    ///   (64: the hold, on from 64), portamento (65: on from 64 for both
    ///   halves, off below), all sound off (120), reset all controllers
    ///   (121, which leaves the volume, the balance and the portamento)
    ///   and all notes off (123).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven: the bender.
    ///
    /// Anything else is ignored and counted
    /// ([`Jupiter8::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
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
                for assigner in &mut self.assigners {
                    assigner.hold(value >= SWITCH_ON);
                }
            }
            CONTROL_PORTAMENTO => {
                self.performance.portamento = if value >= SWITCH_ON { Portamento::On } else { Portamento::Off };
            }
            CONTROL_ALL_SOUND_OFF => {
                self.release_all();
                for index in 0..VOICES {
                    let (half, _) = self.place(index);
                    if let (Some(voice), Some(settings)) = (self.voices.get_mut(index), self.settings.get(half)) {
                        voice.cut(settings);
                    }
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                for assigner in &mut self.assigners {
                    assigner.hold(false);
                }
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

    /// Releases every voice, held by a key or by the hold, and lets the
    /// hold go: for when the input stops and no note off will come.
    pub fn release_all(&mut self) {
        for assigner in &mut self.assigners {
            assigner.release_all();
        }
    }

    /// A tick: the voices take what the keys and the controls ask.
    fn control(&mut self) {
        let cents = |cents: u16| law::octaves(i32::from(cents), CENTS);
        let performance = self.performance;
        let bend = law::share_of(cents(performance.bend_vco), self.bend, PITCH_BEND_CENTRE);
        let bend_cutoff = law::share_of(cents(performance.bend_vcf), self.bend, PITCH_BEND_CENTRE);
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
        // The square of the controller's share, in Q28 (own choice of
        // the law, as a volume control's taper).
        let square = i64::from(self.volume).saturating_mul(i64::from(self.volume));
        let volume = i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / VOLUME_FULL).unwrap_or(ONE_Q28);
        // Each half is in full from the centre to its own end, and falls
        // in a straight line to nothing at the other (own choice).
        let share = |towards: u8, centre: u8| {
            let level =
                i64::from(towards.min(centre)).saturating_mul(i64::from(ENVELOPE_ONE)).checked_div(i64::from(centre));
            i32::try_from(level.unwrap_or(0)).unwrap_or(ENVELOPE_ONE)
        };
        let upper = share(self.balance, BALANCE_CENTRE);
        let lower = share(DATA_MAX.saturating_sub(self.balance), DATA_MAX.saturating_sub(BALANCE_CENTRE));
        let mixed =
            |[first, second]: [i32; HALVES]| (mul_high(first, upper) << 2).wrapping_add(mul_high(second, lower) << 2);
        for sample in out {
            let earlier = mixed(self.voices_sample());
            let later = mixed(self.voices_sample());
            let signal = self.decimator.process(earlier, later);
            *sample = to_sample(amplify(signal, volume) >> OUTPUT_SHIFT);
        }
    }
}
