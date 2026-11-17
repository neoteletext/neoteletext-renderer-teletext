//! Roland's Juno-106 (1984) and Juno-60 (1982): six voices of one
//! oscillator each, a high-pass filter, a four-pole low-pass filter that
//! oscillates, one envelope, one low-frequency oscillator and a chorus of
//! two bucket-brigade delay lines, rendered from MIDI messages to 16-bit
//! mono samples at 48,000 Hz.
//!
//! # The instrument
//!
//! - **Oscillator.** A clock divided by a 16-bit timer, so a key's pitch
//!   is the clock over a whole number. The Juno-106 divides 8 MHz, and
//!   its processor puts the bender and the low-frequency oscillator into
//!   the divisor; the Juno-60 divides a master oscillator of 1.9 MHz that
//!   the bender and the low-frequency oscillator move. From each cycle
//!   come a sawtooth, a pulse whose width is set by hand or moved, a
//!   square an octave below, and there is noise. The Juno-106's sawtooth
//!   is charged by a voltage from its 12-bit converter, so its height is
//!   12 V only as nearly as the converter's steps allow, and falls off
//!   above the highest key.
//! - **Filter.** The IR3109's four stages with feedback through one more
//!   amplifier ([`OtaCascade`](crate::analog::filter::OtaCascade)); the
//!   input is raised with the resonance, as the circuit does; from the
//!   slider's last tenth it oscillates, in tune.
//! - **Envelope.** The Juno-106's is computed by its processor every
//!   4.2 ms and smoothed by 1 ms of resistor and capacitor; the Juno-60's
//!   is a chip charging a capacitor.
//! - **After the voices**, at the output rate: the capacitor the sum
//!   passes into the jack board through, the high-pass filter (a
//!   switch of four positions), the amplifier's level, and the chorus:
//!   two delay lines swept oppositely by one slow triangle, mixed with
//!   the dry signal as the instrument's mono output mixes them
//!   (DEC-6437 item 2).
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Roland's service notes and owner's manuals of the Juno-106, Juno-60
//! and Juno-6, Roland's specification pages, the chorus block's figures
//! of the MN3009 and
//! the published write-ups named where each figure is used, with no code
//! and no table from any emulator or plugin. Two reference programs were
//! run as sealed programs and their output measured; where a figure here
//! was chosen with such a measurement in view, its `Source:` line says
//! so. Each constant names its source where it is defined, in
//! [`law`], the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **How a slider travels between its documented ends** is an own
//!   choice for the envelope's times, the low-frequency oscillator's
//!   rate, the resonance and the amplifier's level ([`law`]).
//! - **The Juno-106's cutoff is in open conflict with its calibration.**
//!   The slider covers the specification's 5 Hz to 50 kHz, which puts
//!   the service notes' test program 3 (FREQ 6.3, RES 10, KYBD 10, key
//!   C4) at 1,650 Hz, where adjustment 7-1 trims every voice to 248 Hz,
//!   2.7 octaves below. No one exponential passes through both ends of
//!   the specification and that point, and a law hung from the point
//!   with its top at 50 kHz was tried and is no Juno-106: flat over the
//!   slider's lowest third and 41 Hz at its middle. What was read and
//!   what would settle it are at [`law::JUNO_106`]; the test of the
//!   calibration is in `tests/juno_filter.rs`, ignored until it is
//!   settled.
//! - **The chorus's delay and sweep** are in no document read; the
//!   lines' filters are the building block's third-order ones at the
//!   schematic's 10 kHz, where the instrument has fourth-order ones; the
//!   chip's noise, which the instrument is known for, is left out.
//! - **The Juno-60's low-frequency oscillator** is taken as a triangle.
//! - **The bass lift** of the Juno-106's high-pass switch at 0 is one
//!   pole's shelf from two resistors and a capacitor of a path whose
//!   other parts were not read.
//! - **No portamento, no arpeggiator, no unison or Poly 2**, no key
//!   transpose, no bender on the filter; the bender's and the wheel's
//!   depths, which are knobs beside the instrument's keyboard and no part
//!   of a patch, are fixed ([`PITCH_BEND_SEMITONES`],
//!   [`WHEEL_CENTS`]).
//! - **Velocity is ignored**, as both instruments ignore it.

pub mod law;
pub mod patch;
mod voice;

use crate::SAMPLE_RATE;
use crate::analog::allocator::{Allocator, Assign, Mode, Stage};
use crate::analog::chorus::Chorus;
use crate::analog::decimator::Decimator;
use crate::analog::drift::Variation;
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::{amplify, mul_high, scale};
use crate::analog::lfo::{LFO_ONE, Lfo, Shape};
use crate::analog::oscillator::Noise;
use crate::analog::{frequency, to_sample};

use law::{Law, OCTAVE};
pub use patch::{ChorusMode, Patch, PatchError, PwmSource, Range, Slider};
use voice::{CUTOFF_TOLERANCE, Controls, FRAME_TICKS, Settings, TICK_RATE, TICK_SAMPLES, Voice};

/// Voices of the instrument.
///
/// Source: JUNO-106 service notes, page 1: "6 Voice"; JUNO-60 service
/// notes, page 14, "Key assignment": "Six channels".
pub const VOICES: usize = 6;

/// Which instrument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    /// The Juno-106.
    Juno106,
    /// The Juno-60.
    Juno60,
}

/// How far the pitch bend wheel bends at either end, in semitones.
///
/// Source: own choice: the usual default of a MIDI instrument, and this
/// crate's DX7's. On the instruments it is a knob of up to an octave
/// (JUNO-106) or 700 cents (JUNO-60), outside the patch.
pub const PITCH_BEND_SEMITONES: i32 = 2;
/// How far the modulation wheel at its top makes the low-frequency
/// oscillator move the pitch either way, in cents.
///
/// Source: own choice. On the instruments it is the bender panel's LFO
/// knob, outside the patch.
pub const WHEEL_CENTS: i32 = 50;

/// The pitch bend value that bends nothing, and the bits below it.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// [`PITCH_BEND_SEMITONES`] as a key.
const PITCH_BEND_KEYS: u8 = PITCH_BEND_SEMITONES as u8;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// The sustain pedal is down at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const PEDAL_DOWN: u8 = 64;
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
/// Controller: the last of the mode messages (omni off, omni on, mono on,
/// poly on), each of which is an all notes off too.
///
/// Source: JUNO-106 owner's manual, MIDI Implementation 2.1, Notes: "Mode
/// messages (123 - 127) are also recognized as ALL NOTES OFF."
const CONTROL_POLY_ON: u8 = 127;

/// The seed of the instrument's noise, of its voices' tolerances and of
/// its oscillators that hold a random level.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4a75_6e6f;

/// The chorus's sweep rates in thousandths of a hertz: I, II, and both
/// buttons of the Juno-60.
///
/// Source: Florian Anwander's schematic of the Juno-60's chorus
/// oscillator, the notes written beside its outputs: I, 20 V peak to
/// peak, one cycle in 2 s; II, 20 V, 1.2 s; I+II, 1.6 V. His page
/// "Roland choruses and ensemble effects" gives 0.5 Hz, 0.8 Hz and, for
/// I+II, "8% amount, 1Hz, sine like"; and of the Juno-106: "no
/// frequencies defined ... I assume they are similar to the Juno-60".
/// Own derivation for I+II: the oscillator is an integrator between two
/// thresholds, so with its swing cut to 8% a cycle takes at most 8% of
/// the slower setting's 2 s, 0.16 s; the schematic's note is read as
/// 0.1 s, which one reference program's 9.7 Hz agrees with, and the
/// page's 1 Hz cannot be.
const CHORUS_RATES: [u32; 3] = [500, 833, 10_000];
/// The chorus's middle delay and its sweep either way with one button
/// down, in millionths of a second.
///
/// Source: own choice: no document read gives either (Anwander: "At the
/// moment I can provide only the modulation rates"). They lie inside the
/// MN3009's 0.64 to 12.8 ms (the chorus block's figures, from its data
/// sheet), and near one reference program's measured
/// 1.6 to 5.2 ms.
const CHORUS_DELAY: (u32, u32) = (3_500, 1_850);
/// The sweep with both buttons down, as a share of the sweep with one,
/// in hundredths.
///
/// Source: the same schematic: 1.6 V against 20 V.
const CHORUS_BOTH_PERCENT: u32 = 8;
/// The cutoff of the filters before and after each delay line, in
/// thousandths of a hertz.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): each
/// is two stages of 22 kΩ twice with 820 pF and 680 pF, then 1,800 pF and
/// 270 pF: 1 / (2 π R √(C1 C2)) is 9.7 kHz and 10.4 kHz, with the two
/// stages' peaks of a fourth-order filter with none overall.
const CHORUS_FILTER: u32 = 10_000_000;
/// The share of each delay line against the dry signal in the mono
/// output: 47 / 78, in Q31.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): each
/// output amplifier (IC6) adds the dry signal through 47 kΩ (R71, R73)
/// and one line through 39 kΩ (R72, R74), a line being 47 / 39 of the
/// dry signal; the mono jack takes the mean of the two outputs, in which
/// the dry signal is whole and each line half that.
const CHORUS_LINE_GAIN: i32 = ((47i64 << 31) / 78) as i32;

/// The corner below which the voices' sum does not pass, in thousandths
/// of a hertz: what takes away the steady part of a pulse that is not a
/// square, which would otherwise stand in the output for as long as the
/// note.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): the
/// voices' sum enters the board through C14, 10 µF, across R39, 33 kΩ,
/// before the high-pass switch IC3: 1 / (2 π R C) is 0.48 Hz. JUNO-60
/// service notes, chorus board schematic (page 11): the same parts
/// before its high-pass switch IC5, C47, 10 µF, across R70, 33 kΩ.
const COUPLING_CORNER: u32 = 482;

/// The corner of the bass lift, in thousandths of a hertz.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): in
/// the path that position 0 of the high-pass switch adds, R22, 47 kΩ,
/// with C9, 0.047 µF: 1 / (2 π R C) is 72 Hz.
const BASS_BOOST_CORNER: u32 = 72_000;
/// The gain of the low frequencies the lift adds, in Q28: 100 / 47.
///
/// Source: the same schematic, as far as it was read: in that path the
/// amplifier has R18, 100 kΩ, against R20, 47 kΩ. Below the corner
/// the lift is then 1 + 100 / 47, 9.9 dB, and at 55 Hz 8.2 dB; one
/// reference program, measured, lifts 55 Hz by 8.3 dB.
const BASS_BOOST_GAIN: i32 = ((100i64 << 28) / 47) as i32;

/// The amplifier's level slider moves the level by this either way, in
/// octaves of amplitude: 6 dB.
///
/// Source: own choice: the slider is marked -5 to +5 and no document
/// gives its range.
const VCA_LEVEL_RANGE: i32 = OCTAVE;
/// The slider's middle.
const VCA_LEVEL_CENTRE: i32 = 64;
/// The share of the mixed output that is a sample, in Q28: 494 / 1000.
/// One voice's sawtooth through an open filter then peaks 16.8 dB below
/// full scale, and six struck together 3.4 dB below.
/// What nears full scale is bent under it by the output's limiter
/// ([`crate::output::limit`]).
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's half, as it was first, the note stood 0.1 dB above it.
const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 494 / 1000) as i32;

/// The whole of the low-frequency oscillator's delay that is silence;
/// the rise to full depth then takes half as long again.
///
/// Source: own choice of the rise. JUNO-60 service notes, adjustment 7-4
/// and figure 31: with the delay at 10 the modulation "2 seconds later,
/// begins to reappear" and grows; the specifications' 1.5 s and 3 s are
/// taken as the silence.
const LFO_FADE_SHARE: u32 = 2;
/// Millionths of a second in a second.
const MICROSECONDS: u64 = 1_000_000;
/// The square of the largest volume, which the volume's square is a share
/// of.
const VOLUME_FULL: i64 = (DATA_MAX as i64) * (DATA_MAX as i64);

/// The Juno.
///
/// It holds no reference and allocates nothing. Two instruments of one
/// model given the same patch and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Juno {
    model: Model,
    law: &'static Law,
    patch: Patch,
    settings: Settings,
    allocator: Allocator<VOICES>,
    voices: [Voice; VOICES],
    /// The trigger count each voice last acted on.
    triggers: [u32; VOICES],
    noise: Noise,
    /// The oscillator after its delay, and the same without, for the
    /// wheel.
    lfo: Lfo,
    vibrato: Lfo,
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
    /// Samples of the voices until the next tick, and ticks until the
    /// next frame.
    tick_wait: u32,
    frame_wait: u32,
    /// The share of the way a held control voltage moves in a tick.
    smoothing: i32,
    decimator: Decimator,
    coupling: OnePole,
    high_pass: OnePole,
    bass: OnePole,
    level: i32,
    chorus: Chorus,
    ignored: u64,
}

impl Juno {
    /// An instrument at rest playing its model's plain patch
    /// ([`Patch::default_for`]).
    #[must_use]
    pub fn new(model: Model) -> Self {
        let law = match model {
            Model::Juno106 => &law::JUNO_106,
            Model::Juno60 => &law::JUNO_60,
        };
        let mut voice = 0u32;
        let voices = core::array::from_fn(|_| {
            let trim = Variation::new(SEED, voice).offset(CUTOFF_TOLERANCE);
            voice = voice.wrapping_add(1);
            Voice::new(model, trim)
        });
        let patch = Patch::default_for(model);
        let mut juno = Self {
            model,
            law,
            patch,
            settings: Settings::new(&patch, model, law),
            // Each key takes the next voice round the six, and a seventh
            // key the voice of the first: JUNO-60 service notes, page 14,
            // "Key assignment"; JUNO-106 service notes, page 18, the test
            // mode's "ROTARY".
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices,
            triggers: [0; VOICES],
            noise: Noise::new(SEED),
            lfo: Lfo::new(SEED),
            vibrato: Lfo::new(SEED),
            bend: 0,
            wheel: 0,
            volume: DATA_MAX,
            tick_wait: 0,
            frame_wait: 0,
            smoothing: voice::hold_smoothing(),
            decimator: Decimator::sharp(),
            coupling: OnePole::new(),
            high_pass: OnePole::new(),
            bass: OnePole::new(),
            level: ONE_Q28,
            chorus: Self::empty_chorus(),
            ignored: 0,
        };
        juno.coupling.set_cutoff(frequency(COUPLING_CORNER, SAMPLE_RATE));
        juno.bass.set_cutoff(frequency(BASS_BOOST_CORNER, SAMPLE_RATE));
        juno.apply_patch();
        juno
    }

    /// The chorus with nothing in its lines, before a patch sets its
    /// delay and its sweep.
    fn empty_chorus() -> Chorus {
        let mut chorus = Chorus::new(SAMPLE_RATE);
        chorus.set_filter_cutoff(frequency(CHORUS_FILTER, SAMPLE_RATE));
        chorus
    }

    /// Which instrument this is.
    #[must_use]
    pub const fn model(&self) -> Model {
        self.model
    }

    /// Puts the instrument back as [`Juno::new`] made it, but for the
    /// patch, which it keeps: silent, its controllers at rest, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let patch = self.patch;
        *self = Self::new(self.model);
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect at
    /// once, on the voices that are sounding too, as moving the panel's
    /// controls does. A setting the model has not is taken as the
    /// nearest it has ([`ChorusMode::Both`], [`PwmSource::Envelope`]).
    pub fn set_patch(&mut self, patch: &Patch) {
        // While the chorus is off its lines are not run, to spare the
        // work, and so keep what they held when it was switched off. The
        // instrument's lines run on and have long emptied by the time
        // they are heard again: a chorus switched on starts empty.
        if self.patch.chorus == ChorusMode::Off && patch.chorus != ChorusMode::Off {
            self.chorus = Self::empty_chorus();
        }
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
        let law = self.law;
        self.settings = Settings::new(&patch, self.model, law);
        for voice in &mut self.voices {
            voice.configure(&self.settings);
        }

        let rate = law::exponential(law.lfo_rate.0, law.lfo_rate.1, patch.get(Slider::LfoRate));
        self.lfo.set_step(frequency(rate, TICK_RATE));
        self.vibrato.set_step(frequency(rate, TICK_RATE));
        let delay = law::share(i32::try_from(law.lfo_delay).unwrap_or(i32::MAX), patch.get(Slider::LfoDelay));
        let ticks = u64::from(delay.cast_unsigned()).saturating_mul(u64::from(TICK_RATE)) / MICROSECONDS;
        let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
        self.lfo.set_delay(ticks, ticks / LFO_FADE_SHARE);

        let corner = law.hpf.get(usize::from(patch.hpf.min(patch::HPF_MAX))).copied().unwrap_or(0);
        self.high_pass.set_cutoff(frequency(corner, SAMPLE_RATE));
        if corner == 0 {
            self.high_pass.reset();
        }
        // The bass lift's capacitor is followed only while the lift is
        // heard; what it held when the switch left 0 is not brought back
        // when the switch returns.
        if !(law.bass_boost && patch.hpf == 0) {
            self.bass.reset();
        }
        let slider = i32::from(patch.get(Slider::VcaLevel)).saturating_sub(VCA_LEVEL_CENTRE);
        let octaves = slider.saturating_mul(VCA_LEVEL_RANGE) / VCA_LEVEL_CENTRE;
        self.level = law::raise(ONE_Q28.cast_unsigned(), octaves).cast_signed();

        let mode = match (patch.chorus, self.model) {
            (ChorusMode::Both, Model::Juno106) => ChorusMode::Two,
            (mode, _) => mode,
        };
        let (rate, percent, shape, opposite) = match mode {
            ChorusMode::Off | ChorusMode::One => (CHORUS_RATES[0], 100, Shape::Triangle, true),
            ChorusMode::Two => (CHORUS_RATES[1], 100, Shape::Triangle, true),
            // With both buttons down the second line is no longer swept
            // the other way (Anwander: "In the 'fast mode' the second
            // BBD is controlled in a non inverted way"), and the sweep is
            // "sine like".
            ChorusMode::Both => (CHORUS_RATES[2], CHORUS_BOTH_PERCENT, Shape::Sine, false),
        };
        self.chorus.set_delay(CHORUS_DELAY.0, CHORUS_DELAY.1.saturating_mul(percent) / 100);
        self.chorus.set_rate(frequency(rate, SAMPLE_RATE));
        self.chorus.set_shape(shape);
        self.chorus.set_opposite(opposite);
    }

    /// How many messages [`Juno::event`] has ignored since the
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
    /// part and hears every channel, as the Juno-106 does when it is
    /// switched on (its owner's manual, MIDI Implementation 2.1: "OMNI
    /// ON").
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is not used.
    /// - Control change (0xBn) of the modulation wheel (1), volume (7),
    ///   the sustain pedal (64, down from 64), all sound off (120), reset
    ///   all controllers (121, which leaves the volume), all notes off
    ///   (123) and the four mode messages (124 to 127), which are an all
    ///   notes off and nothing more: the instrument stays one part that
    ///   hears every channel.
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven.
    ///
    /// Anything else is ignored and counted ([`Juno::ignored_events`]):
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
            STATUS_NOTE_ON => {
                // The oscillator's delay starts when a key goes down
                // with no other held (JUNO-60 service notes, adjustment
                // 7-4, "TRIG MODE to AUTO").
                if !self.allocator.voices().iter().any(|voice| voice.stage == Stage::Held) {
                    self.lfo.key();
                }
                self.allocator.key_down(data1, data2);
            }
            STATUS_CONTROL_CHANGE => self.control_change(data1, data2),
            STATUS_PITCH_BEND => {
                self.bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
            }
            _ => self.ignore(),
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            CONTROL_MODULATION_WHEEL => self.wheel = value,
            CONTROL_VOLUME => self.volume = value,
            CONTROL_SUSTAIN => self.allocator.sustain(value >= PEDAL_DOWN),
            CONTROL_ALL_SOUND_OFF => {
                self.allocator.all_keys_up();
                for (index, voice) in self.voices.iter_mut().enumerate() {
                    voice.cut(&self.settings);
                    self.allocator.finished(index);
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.allocator.sustain(false);
            }
            CONTROL_ALL_NOTES_OFF..=CONTROL_POLY_ON => {
                // As a note off for every key: the pedal still holds.
                let voices = *self.allocator.voices();
                for voice in voices.iter().filter(|voice| voice.stage == Stage::Held) {
                    self.allocator.key_up(voice.key);
                }
            }
            _ => self.ignore(),
        }
    }

    /// Releases every voice, held by a key or by the pedal, and lifts
    /// the pedal: for when the input stops and no note off will come.
    pub fn release_all(&mut self) {
        self.allocator.all_keys_up();
    }

    /// A tick, and at its turn a frame: the voices take what the keys
    /// and the controls ask.
    fn control(&mut self) {
        let lfo = self.lfo.tick();
        let vibrato = self.vibrato.tick();
        if self.frame_wait == 0 {
            self.frame_wait = match self.model {
                Model::Juno106 => FRAME_TICKS,
                Model::Juno60 => 1,
            };
            // The bender's whole travel either way is so many keys.
            let bend = law::share_of(law::keys(0, PITCH_BEND_KEYS), self.bend, PITCH_BEND_CENTRE);
            let wheel = law::share(law::cents(WHEEL_CENTS), self.wheel);
            let controls = Controls { lfo, pitch: bend.saturating_add(mul_high(wheel, vibrato) << 2) };
            let keys = *self.allocator.voices();
            for (index, ((voice, key), seen)) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers).enumerate() {
                if key.stage == Stage::Rest {
                    continue;
                }
                if *seen != key.triggers {
                    *seen = key.triggers;
                    voice.start(key.key, &self.settings);
                }
                if !key.gate() {
                    voice.release();
                }
                voice.frame(&self.settings, controls);
                if key.stage == Stage::Released && voice.silent() {
                    self.allocator.finished(index);
                }
            }
        }
        self.frame_wait = self.frame_wait.saturating_sub(1);
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                voice.tick(&self.settings, self.law, self.smoothing);
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
        let noise = self.noise.tick();
        let mut sum = 0i32;
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                sum = sum.wrapping_add(voice.sample(&self.settings, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of six voices and
    /// the chorus, whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        // The square of the controller's share, in Q28 (own choice of
        // the law, as a volume control's taper).
        let square = i64::from(self.volume).saturating_mul(i64::from(self.volume));
        let volume = i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / VOLUME_FULL).unwrap_or(ONE_Q28);
        let boost = self.law.bass_boost && self.patch.hpf == 0;
        let chorus = self.patch.chorus != ChorusMode::Off;
        for sample in out {
            let earlier = self.voices_sample();
            let later = self.voices_sample();
            let mut signal = self.decimator.process(earlier, later);
            signal = self.coupling.high_pass(signal);
            signal = self.high_pass.high_pass(signal);
            if boost {
                signal = signal.saturating_add(amplify(self.bass.low_pass(signal), BASS_BOOST_GAIN));
            }
            signal = amplify(amplify(signal, self.level), volume);
            if chorus {
                let [first, second] = self.chorus.process(signal);
                signal = signal.saturating_add(scale(first.saturating_add(second), CHORUS_LINE_GAIN));
            }
            *sample = to_sample(amplify(signal, OUTPUT_GAIN));
        }
    }
}

/// The low-frequency oscillator's peak, which the laws' depths are
/// multiplied by.
const _: () = assert!(LFO_ONE == crate::analog::envelope::ENVELOPE_ONE);
