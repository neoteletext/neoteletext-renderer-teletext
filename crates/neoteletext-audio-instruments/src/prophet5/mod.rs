//! Sequential Circuits' Prophet-5 (1978 to 1984): five voices of two
//! oscillators, a mixer with noise, a four-pole low-pass filter that
//! oscillates, two envelopes and an amplifier, with Poly-Mod inside each
//! voice and one low-frequency oscillator and Wheel-Mod for all of them,
//! rendered from MIDI messages to 16-bit mono samples at 48,000 Hz.
//!
//! Two instruments are here ([`Model`]): revision 3, with the Curtis
//! chips (CEM3340 oscillators, CEM3320 filter, CEM3310 envelopes), and
//! revisions 1 and 2, with the SSM chips (SSM2030, SSM2040, SSM2050).
//!
//! # The instrument
//!
//! - **Oscillators.** A sawtooth and a pulse from oscillator A, which
//!   can be synchronised to oscillator B; a sawtooth, a triangle and a
//!   pulse from oscillator B, which can leave the keyboard and can drop
//!   to low frequencies. Each frequency knob moves in semitones; pitch
//!   is a volt to the octave from the lowest key's C of 16.35 Hz.
//! - **Mixer and filter.** Both oscillators and white noise into four
//!   transconductance stages with feedback
//!   ([`OtaCascade`](crate::analog::filter::OtaCascade)). The feedback
//!   is taken after the filter's output buffer on both revisions, and
//!   nothing makes up the level the passband loses as the resonance
//!   rises. The cutoff follows its knob in semitones, the keys when its
//!   switch is on, and the filter envelope.
//! - **Envelopes.** Two in each voice, each a chip that charges a
//!   capacitor towards 1.3 times its peak in the attack and along plain
//!   exponentials after ([`RcEnvelope`](crate::analog::envelope::RcEnvelope)).
//! - **Poly-Mod.** The filter envelope and oscillator B, each by its
//!   amount, to oscillator A's frequency, its pulse width and the
//!   cutoff: oscillator B at audio rate, sample by sample.
//! - **Wheel-Mod.** The low-frequency oscillator's sawtooth, triangle
//!   and square in any combination, mixed with pink noise, by the
//!   modulation wheel's amount to either oscillator's frequency or
//!   pulse width and to the cutoff.
//! - **The keys.** Each new key takes the voice used longest ago and a
//!   repeated key its own voice again; in unison all five voices play
//!   the lowest key held, and the glide works.
//! - **The panel's numbers.** A program stores each knob as the seven
//!   bits the instrument's converter made of it, and every control
//!   voltage is that number of twelfths of a volt: a semitone of pitch
//!   and of cutoff ([`patch`], [`law`]).
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from
//! Sequential Circuits' technical manuals of revision 2 and of revision
//! 3, its operation manual and its original patch sheets, and the data
//! sheets of the six chips, with no code and no table from any emulator
//! or plugin, and without the instrument's firmware: what the processor
//! does is taken from the manuals' prose alone. No reference program
//! exists that could be run beside it. Each constant names its source
//! where it is defined, in [`law`], the voice and here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **The depths of Poly-Mod, of Wheel-Mod and of the filter envelope**
//!   are in no document read: the resistors that scale them are, and
//!   the voltages they scale are own estimates.
//! - **The envelopes of revisions 1 and 2 are not straighter** than
//!   revision 3's here: both chips' data sheets give an attack towards
//!   1.3 times the peak (13 V and 10 V; 6.5 V and 5 V) and exponential
//!   decays. They differ in their times.
//! - **The two filters are one block** with each revision's law of
//!   cutoff and resonance; neither chip's own distortion is modelled,
//!   only the block's hyperbolic tangent.
//! - **The CEM3340's pulse** is high about the sawtooth's fall, a half
//!   of its width either side; here it starts at the fall. Only a
//!   sawtooth and a pulse sounded together by one oscillator differ for
//!   it. Its synchronisation, which reverses a triangle, is taken as
//!   the sawtooth's restart, which the manual's circuit is said to give.
//! - **The firmware's timing** (a loop of 6 to 11 ms) is left out: a
//!   key sounds and a knob moves at once.
//! - **The glide's rates** between none and its slowest, the pitch of a
//!   knob's steps between its ends, and where a released voice stands
//!   when keys are assigned are own choices, each said where it is made.
//! - **No scale mode, no sequencer or cassette interface, no A-440
//!   tone**, and no automatic tuning: each oscillator is off by a fixed
//!   small amount instead.
//! - **Velocity is ignored**, as the instrument's keyboard has none.

pub mod law;
pub mod patch;
mod voice;

use crate::analog::allocator::{Allocator, Assign, Mode, Priority, Stage, Trigger};
use crate::analog::decimator::Decimator;
use crate::analog::drift::Variation;
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::amplify;
use crate::analog::oscillator::Noise;
use crate::analog::{frequency, to_sample};

use law::{Law, OCTAVE, SEMITONES, UNIT};
pub use patch::{Knob, Patch, Switch};
use voice::{Controls, Settings, TICK_RATE, TICK_SAMPLES, Voice};

/// Voices of the instrument.
///
/// Source: TM3 section 2-2 (page 17): five voices, each "a complete
/// synthesizer with two VCOs, a MIXER, VCF, FINAL VCA, and two ENV GENs".
pub const VOICES: usize = 5;

/// Which instrument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    /// Revision 3 (serial numbers from 1301): the Curtis chips.
    Rev3,
    /// Revisions 1 and 2 (serial numbers to 1299): the SSM chips.
    Rev2,
}

impl Model {
    /// The model's laws.
    #[must_use]
    pub const fn law(self) -> &'static Law {
        match self {
            Self::Rev3 => &law::REV3,
            Self::Rev2 => &law::REV2,
        }
    }

    /// The number of an oscillator's frequency knob that raises it so
    /// many semitones on this model, whose knobs differ: four octaves on
    /// revision 3 and five on revisions 1 and 2. Twenty-four semitones
    /// put the A above middle C at 440 Hz.
    #[must_use]
    pub fn frequency_knob(self, semitones: u8) -> u8 {
        let (times, over, most) = self.law().frequency;
        let steps = i32::from(semitones).min(most).saturating_mul(over).saturating_add(times.saturating_sub(1));
        u8::try_from(steps.checked_div(times).unwrap_or(0)).unwrap_or(patch::KNOB_MAX).min(patch::KNOB_MAX)
    }
}

/// The key of the keyboard's lowest note, its C0.
///
/// Source: TM3 section 2-11 (page 28): 61 keys, C0 to C5, "a key number
/// (1 - 61)". Own choice of the MIDI key: with a frequency knob two
/// octaves up, which the operation manual's programs call the normal
/// range, the keyboard's third A is then MIDI's A of 440 Hz.
pub const LOWEST_KEY: u8 = 36;

/// How far the pitch wheel bends at either end, in semitones.
///
/// Source: OM section 1-4 (page 9): "the pitch of all voices may be
/// varied up or down by about a 5th"; TM3 test table (page 61): "RAISES
/// AND LOWERS PITCH BY AT LEAST A FIFTH".
pub const PITCH_BEND_SEMITONES: i32 = 7;
/// How far the master tune goes either way, in cents.
///
/// Source: TM2 section 2-18 (page 38): "allowing over a semitone of flat
/// or sharp correction"; own choice of the round figure.
pub const MASTER_TUNE_CENTS: i32 = 100;

/// One unit of Wheel-Mod, in Q24: what the low-frequency oscillator's
/// sawtooth reaches at its top with the wheel fully up.
///
/// Source: own choice, since neither manual gives Wheel-Mod's voltage,
/// only the resistors it passes: one volt. Through the 182 kΩ of an
/// oscillator's summer that is 0.55 octave for the sawtooth and the
/// square, and a triangle of a quarter of that either way, 1.6
/// semitones; the patch sheets ask for the wheel "up slightly (1/8 to
/// 1/4) to engage vibrato" (program 1-1), which is then 20 to 40 cents
/// either way.
pub const WHEEL_MOD: i32 = UNIT;
/// The triangle of the low-frequency oscillator against its sawtooth: a
/// quarter either way.
///
/// Source: TM3 sheet SD334 (page 47): the triangle, 5 V peak to peak
/// (CEM3340 data sheet, page 2) and shifted to stand about zero, passes
/// 160 kΩ as the sawtooth's 10 V does: half its swing, half of it either
/// side.
const LFO_TRIANGLE_SHIFT: u32 = 2;
/// The gain of the filtered noise as Wheel-Mod has it, in Q28.
///
/// Source: own choice: white noise through the filter below keeps a
/// sixth of its level at this module's tick; three times that is about
/// half a unit, the level of the triangle's peak to peak.
const PINK_GAIN: i32 = 3 * ONE_Q28;
/// The corner of the filter that makes Wheel-Mod's noise of white
/// noise, in thousandths of a hertz.
///
/// Source: TM3 sheet SD334 (page 47): R393, 100 kΩ, with C372, 0.01 µF,
/// about the amplifier that follows the noise source; TM2 sheet SD312
/// (page 75): R353 and C333, the same; 1 / (2 π R C) is 159 Hz. TM2
/// section 2-21 (page 43) calls the result pink noise.
const PINK_CORNER: u32 = 159_155;

/// The glide's slowest rate, in octaves a second in Q24.
///
/// Source: TM3 test table (page 61): with the glide at 10, "MINIMUM: 5
/// SECS TO SLEW 5 OCTAVES".
const GLIDE_SLOWEST: u32 = OCTAVE.cast_unsigned();
/// The octaves of rate that each step of the glide knob below its top
/// adds: the rate doubles for each twelve steps, a thousand octaves a
/// second with the knob at 0.
///
/// Source: own choice. The manuals give the slowest rate and "NO GLIDE"
/// at 0; between, the circuit's control current falls as the knob rises
/// (TM3 section 2-12, page 22: "as GLIDE CV increases, Iabc decreases").
const GLIDE_STEP: i32 = OCTAVE / SEMITONES;

/// The pitch bend value that bends nothing.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// The pedal is down at this value and above.
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
/// Controller: the pedal.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// The seed of the instrument's noise and of its voices' tolerances.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x5072_6f35;
/// The share of the mixed output that is a sample, in Q28: 319 / 1000.
/// One voice's sawtooth through an open filter then peaks 21.3 dB below
/// full scale on a rev 3, and five struck together 8.7 dB below.
/// What is beyond full scale is saturated.
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's half, as it was first, the note stood 4.1 dB above it on a
/// rev 3 and 3.7 dB on a rev 2.
const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 319 / 1000) as i32;
/// The square of the largest volume, which the volume's square is a share
/// of.
const VOLUME_FULL: i64 = (DATA_MAX as i64) * (DATA_MAX as i64);

/// The low-frequency oscillator: a sawtooth, a triangle and a square
/// from one phase, each with its own switch, added together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Modulator {
    phase: u32,
    step: u32,
}

impl Modulator {
    /// Half a cycle of phase.
    const HALF: u32 = 1 << 31;
    /// The right shift from a phase to the sawtooth's rise from zero to
    /// [`WHEEL_MOD`].
    const SHIFT: u32 = 32 - law::OCTAVE_BITS;

    /// One tick: the sum of the shapes that are on, in Q24, and the
    /// phase moves on. The sawtooth and the square stand on zero: TM3
    /// section 2-4 (page 21), "All 3340 outputs are positive-going", and
    /// only the triangle is shifted to stand about zero (sheet SD334).
    /// The triangle is lowest where the sawtooth falls, and the square
    /// is high about that fall, as the CEM3340 makes its pulse of the
    /// triangle (data sheet, page 4, figure 1).
    fn tick(&mut self, patch: &Patch, law: &Law) -> i32 {
        let phase = self.phase;
        self.phase = phase.wrapping_add(self.step);
        let rise = (phase >> Self::SHIFT).cast_signed();
        let mut sum = 0i32;
        if patch.on(Switch::LfoSawtooth) {
            sum = sum.saturating_add(rise);
        }
        if patch.on(Switch::LfoTriangle) {
            // From -1 at the cycle's start to 1 at its middle.
            let folded = if phase < Self::HALF { rise } else { UNIT.saturating_sub(rise) };
            sum = sum.saturating_add((folded << 2).saturating_sub(UNIT) >> LFO_TRIANGLE_SHIFT);
        }
        if patch.on(Switch::LfoSquare) && phase.wrapping_add(Self::HALF >> 1) < Self::HALF {
            sum = sum.saturating_add(law::gain(UNIT, law.lfo.2));
        }
        sum
    }
}

/// The Prophet-5.
///
/// It holds no reference and allocates nothing. Two instruments of one
/// model given the same patch and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Prophet5 {
    model: Model,
    patch: Patch,
    settings: Settings,
    allocator: Allocator<VOICES>,
    voices: [Voice; VOICES],
    /// The trigger count each voice last acted on.
    triggers: [u32; VOICES],
    noise: Noise,
    lfo: Modulator,
    /// The source of Wheel-Mod's noise and the filter that colours it.
    wheel_noise: Noise,
    pink: OnePole,
    /// The pitch bend, less its centre.
    bend: i32,
    wheel: u8,
    volume: u8,
    pedal: bool,
    /// The master tune, in octaves.
    tune: i32,
    /// The glide's output: octaves above the lowest key.
    glide: i32,
    /// Samples of the voices until the next tick.
    tick_wait: u32,
    decimator: Decimator,
    ignored: u64,
}

impl Prophet5 {
    /// An instrument at rest playing [`Patch::DEFAULT`].
    #[must_use]
    pub fn new(model: Model) -> Self {
        let law = model.law();
        let mut voice = 0u32;
        let voices = core::array::from_fn(|_| {
            let mut variation = Variation::new(SEED, voice);
            voice = voice.wrapping_add(1);
            let (tuning, cutoff) = law.tolerance;
            Voice::new(model, [variation.offset(tuning), variation.offset(tuning), variation.offset(cutoff)])
        });
        let mut pink = OnePole::new();
        pink.set_cutoff(frequency(PINK_CORNER, TICK_RATE));
        let mut prophet = Self {
            model,
            patch: Patch::DEFAULT,
            settings: Settings::new(&Patch::DEFAULT, model),
            allocator: Allocator::new(Self::mode(&Patch::DEFAULT)),
            voices,
            triggers: [0; VOICES],
            noise: Noise::new(SEED),
            lfo: Modulator { phase: 0, step: 0 },
            wheel_noise: Noise::new(SEED.rotate_left(16)),
            pink,
            bend: 0,
            wheel: 0,
            volume: DATA_MAX,
            pedal: false,
            tune: 0,
            glide: 0,
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        prophet.apply_patch();
        prophet
    }

    /// How a patch assigns keys.
    ///
    /// Polyphonic (TM3 section 2-2, page 17): "Voice 1 is assigned to
    /// the first key hit, Voice 2 to the second key, and so on. ... The
    /// earliest-used voice is reassigned to each new note played.
    /// Repeated notes key the same voice." The allocator is never told
    /// that a voice has fallen silent, as the instrument's processor
    /// never knows it, so after the first five keys it gives a new key
    /// the voice already playing that key, else the voice whose key
    /// went up longest ago, else, with five keys held, the voice keyed
    /// longest ago, whose note "disappears even though the key may
    /// still be held". Own choice: that a voice whose key is up is taken
    /// before one whose key is held; the manual does not say.
    ///
    /// Unison (TM3 section 2-3, page 20): "a UNISON CV derived from the
    /// lowest note played on the keyboard becomes the main pitch control
    /// for all voices"; "The five GATEs occur simultaneously, but the
    /// envelope generators only re-trigger if no keys are held."
    fn mode(patch: &Patch) -> Mode {
        if patch.on(Switch::Unison) { Mode::Unison(Priority::Low, Trigger::Single) } else { Mode::Poly(Assign::Cyclic) }
    }

    /// Which instrument this is.
    #[must_use]
    pub const fn model(&self) -> Model {
        self.model
    }

    /// Puts the instrument back as [`Prophet5::new`] made it, but for
    /// the patch and the master tune, which it keeps: silent, its
    /// controllers at rest, the next key on the first voice, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let (patch, tune) = (self.patch, self.tune);
        *self = Self::new(self.model);
        self.tune = tune;
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect at
    /// once, on the voices that are sounding too, as moving the panel's
    /// controls does; switching unison on or off lets go of every key.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
        self.apply_patch();
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    /// Sets the master tune knob, in cents either way, held to
    /// [`MASTER_TUNE_CENTS`]. It is no part of a patch.
    pub fn set_master_tune(&mut self, cents: i32) {
        self.tune = law::cents(cents.clamp(-MASTER_TUNE_CENTS, MASTER_TUNE_CENTS));
    }

    fn apply_patch(&mut self) {
        let law = self.model.law();
        self.settings = Settings::new(&self.patch, self.model);
        for voice in &mut self.voices {
            voice.configure(&self.settings);
        }
        let mode = Self::mode(&self.patch);
        if mode != self.allocator.mode() {
            self.allocator.set_mode(mode);
        }
        let steps = i32::from(self.patch.get(Knob::LfoFrequency).min(law.knob_top));
        let rate = law::raise(law.lfo.0, steps.saturating_mul(law.lfo.1));
        self.lfo.step = frequency(rate, TICK_RATE);
    }

    /// How many messages [`Prophet5::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        let keyed = self.allocator.voices().iter().zip(&self.voices);
        keyed.filter(|(key, voice)| key.stage != Stage::Rest && !voice.silent()).count()
    }

    /// Takes one MIDI 1.0 channel voice message. The instrument is one
    /// part and hears every channel; it has no MIDI of its own, so what
    /// each message does is this module's choice of the nearest thing
    /// the instrument has.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is not used. Key 36 is the
    ///   keyboard's lowest ([`LOWEST_KEY`]); keys beyond its 61 play as
    ///   a longer keyboard would.
    /// - Control change (0xBn) of the modulation wheel (1), volume (7),
    ///   the pedal (64, down from 64), all sound off (120), reset all
    ///   controllers (121, which leaves the volume) and all notes off
    ///   (123). The pedal holds the notes whose keys go up while it is
    ///   down, as a MIDI instrument's does, and is besides the
    ///   instrument's release footswitch: while it is down the
    ///   envelopes' releases are as the knobs say though the release
    ///   switch is off (OM section 1-5, page 10).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven.
    ///
    /// Anything else is ignored and counted
    /// ([`Prophet5::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & 0xf0 {
            STATUS_NOTE_OFF => self.allocator.key_up(data1),
            STATUS_NOTE_ON if data2 == 0 => self.allocator.key_up(data1),
            STATUS_NOTE_ON => self.allocator.key_down(data1, data2),
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
            CONTROL_SUSTAIN => {
                self.pedal = value >= PEDAL_DOWN;
                self.allocator.sustain(self.pedal);
            }
            CONTROL_ALL_SOUND_OFF => {
                self.allocator.all_keys_up();
                self.pedal = false;
                for (index, voice) in self.voices.iter_mut().enumerate() {
                    voice.cut();
                    voice.configure(&self.settings);
                    self.allocator.finished(index);
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                self.allocator.sustain(false);
            }
            CONTROL_ALL_NOTES_OFF => {
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
        self.pedal = false;
    }

    /// A key as semitones above the keyboard's lowest.
    fn key_number(key: u8) -> i32 {
        i32::from(key).saturating_sub(i32::from(LOWEST_KEY))
    }

    /// The glide: the unison voltage follows the key at the knob's rate,
    /// and within the law's zone of it along an exponential.
    fn glide(&mut self, target: i32) -> i32 {
        let law = self.model.law();
        let steps = i32::from(self.patch.get(Knob::Glide).min(law.knob_top));
        if steps == 0 {
            self.glide = target;
            return target;
        }
        // Octaves a tick at full rate.
        let faster = i32::from(law.knob_top).saturating_sub(steps).saturating_mul(GLIDE_STEP);
        let rate = law::raise(GLIDE_SLOWEST / TICK_RATE, faster).min(i32::MAX.cast_unsigned()).cast_signed();
        let distance = target.saturating_sub(self.glide);
        let within = distance.clamp(law.glide_zone.saturating_neg(), law.glide_zone);
        let step = law::share(rate, within, law.glide_zone);
        // A share cut to nothing would never arrive.
        self.glide = if step == 0 || step.abs() >= distance.abs() { target } else { self.glide.saturating_add(step) };
        self.glide
    }

    /// A tick: the voices take what the keys and the controls ask.
    fn control(&mut self) {
        let law = self.model.law();
        let lfo = self.lfo.tick(&self.patch, law);
        let pink = amplify(self.pink.low_pass(self.wheel_noise.tick()), PINK_GAIN);
        // The source mix moves the two levels opposite ways (TM3 section
        // 2-3, page 20), and the wheel is the amount of their sum.
        let mix = i32::from(self.patch.get(Knob::SourceMix).min(law.knob_top));
        let top = i32::from(law.knob_top);
        let source = law::share(lfo, top.saturating_sub(mix), top).saturating_add(law::share(pink, mix, top));
        let wheel = law::share(source, i32::from(self.wheel), i32::from(DATA_MAX));
        // The bender's whole travel either way is so many semitones.
        let bend = law::share(law::semitones(PITCH_BEND_SEMITONES), self.bend, PITCH_BEND_CENTRE);
        let keys = *self.allocator.voices();
        let unison = if matches!(self.allocator.mode(), Mode::Unison(..)) {
            let key = keys
                .first()
                .filter(|key| key.stage != Stage::Rest)
                .map(|key| law::semitones(Self::key_number(key.key)));
            // With no key yet, the voltage stays where it was.
            Some(self.glide(key.unwrap_or(self.glide)))
        } else {
            None
        };
        let controls = Controls { wheel, pitch: bend.saturating_add(self.tune), unison, pedal: self.pedal };
        for ((voice, key), seen) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers) {
            if key.stage == Stage::Rest {
                continue;
            }
            if *seen != key.triggers {
                *seen = key.triggers;
                voice.start(Self::key_number(key.key));
            } else if key.gate() {
                voice.legato(Self::key_number(key.key));
            } else {
                voice.release();
            }
            if !voice.silent() {
                voice.tick(&self.settings, controls);
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
            if key.stage != Stage::Rest && !voice.silent() {
                sum = sum.wrapping_add(voice.sample(&self.settings, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of five voices,
    /// whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        // The square of the controller's share, in Q28 (own choice of
        // the law, as a volume control's taper).
        let square = i64::from(self.volume).saturating_mul(i64::from(self.volume));
        let volume = i32::try_from(square.saturating_mul(i64::from(ONE_Q28)) / VOLUME_FULL).unwrap_or(ONE_Q28);
        for sample in out {
            let earlier = self.voices_sample();
            let later = self.voices_sample();
            let signal = amplify(self.decimator.process(earlier, later), volume);
            *sample = to_sample(amplify(signal, OUTPUT_GAIN));
        }
    }
}

/// The shares of this module are of a unit that is also a signal's full
/// level and an envelope's quarter.
const _: () = assert!(WHEEL_MOD == crate::analog::LEVEL);
