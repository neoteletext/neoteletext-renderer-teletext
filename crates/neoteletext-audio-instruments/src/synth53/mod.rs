//! synth53, synth52 and synth51: five voices of two
//! oscillators, a mixer with noise, a four-pole low-pass filter that
//! oscillates, two envelopes and an amplifier, with Poly-Mod inside each
//! voice and one low-frequency oscillator and Wheel-Mod for all of them,
//! rendered from MIDI messages to 16-bit mono samples at 48,000 Hz.
//!
//! Which instrument it models is said in
//! `neoteletext-common/docs/audio/synthesizers.md`.
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
//!   repeated key its own voice again; in unison one to five voices play
//!   one key, the lowest or the last held as the program says, detuned
//!   about it, and the glide works.
//!
//! # How it is played and programmed
//!
//! Through the 2020 instrument's MIDI interface, as
//! `neoteletext-common/docs/audio/synth53-synth52-synth51.md` (cited as
//! "S53-n") adopts it: a program of 128 bytes, its parameters set by
//! control changes and NRPN, whole programs in system exclusive dumps
//! ([`interface`], without the synthesis), played by the receiver through
//! [`midi::Midi`]. The parameters the instrument modelled lacks and that
//! the document builds (S53-41 to S53-43, and the pitch wheel's range and
//! brightness, S53-8 and S53-12) are here; the others are carried and not
//! acted on.
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
//! - **The pitch of a knob's steps** between its ends, and where a
//!   released voice stands when keys are assigned, are own choices, each
//!   said where it is made.
//! - **No scale mode, no sequencer or cassette interface, no A-440
//!   tone**, and no automatic tuning: each oscillator is off by a fixed
//!   small amount instead.
//! - **Velocity is ignored**, as the instrument's keyboard has none.

pub mod interface;
pub mod law;
pub mod midi;
pub mod patch;
mod voice;

use crate::analog::allocator::{Allocator, Assign, HELD_KEYS, Mode, Priority, Stage, Trigger};
use crate::analog::decimator::Decimator;
use crate::analog::drift::Variation;
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::amplify;
use crate::analog::oscillator::Noise;
use crate::analog::volume::{self, VolumeGain};
use crate::analog::{frequency, to_sample};

use interface::{Program, Variant, parameter};
use law::{Law, Mixing, OCTAVE, UNIT};
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
    /// synth53: the later revision, with the Curtis chips.
    Synth53,
    /// synth52: the earlier revisions, with the SSM chips.
    Synth52,
    /// synth51: the same model as [`Self::Synth52`] today (DEC-6461 item
    /// 6): the two earliest revisions share their oscillator and filter
    /// chips, and nothing here tells them apart.
    Synth51,
}

impl Model {
    /// The model's laws.
    #[must_use]
    pub const fn law(self) -> &'static Law {
        match self {
            Self::Synth53 => &law::SYNTH53,
            Self::Synth52 | Self::Synth51 => &law::SYNTH52,
        }
    }

    /// The kind of the family that plays this model (S53-1).
    #[must_use]
    pub const fn variant(self) -> Variant {
        match self {
            Self::Synth53 => Variant::Synth53,
            Self::Synth52 => Variant::Synth52,
            Self::Synth51 => Variant::Synth51,
        }
    }

    /// The model a kind plays (S53-1): synth51 is synth52's model today.
    #[must_use]
    pub const fn of(variant: Variant) -> Self {
        match variant {
            Variant::Synth53 => Self::Synth53,
            Variant::Synth52 => Self::Synth52,
            Variant::Synth51 => Self::Synth51,
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
const _: () = assert!(
    PITCH_BEND_SEMITONES * crate::midi::common::CENTS_PER_SEMITONE as i32 == interface::DEFAULT_BEND_RANGE_CENTS as i32
);
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
/// square, and a triangle of half of that either way, 3.3 semitones, on
/// revision 3 and of a quarter, 1.6 semitones, on revisions 1 and 2;
/// the patch sheets, which are of those, ask for the wheel "up slightly
/// (1/8 to 1/4) to engage vibrato" (program 1-1), which is then 20 to
/// 40 cents either way.
pub const WHEEL_MOD: i32 = UNIT;
/// The triangle of the low-frequency oscillator against its sawtooth: a
/// half either way, as a right shift of a triangle of one either way.
///
/// Source: TM3 sheet SD334 (page 47): the triangle, 5 V peak to peak
/// (CEM3340 data sheet, page 2), is shifted to stand about zero by an
/// amplifier that also doubles it (U380: the triangle at the
/// non-inverting input, R3148 of 100 kΩ from the inverting input to the
/// reference and R3147 of 100 kΩ back), and then passes 160 kΩ (R3131)
/// as the sawtooth's 10 V do (R3133): the sawtooth's swing, half of it
/// either side. On revisions 1 and 2 the converter makes the same of
/// the sawtooth's 10 V (TM2 sheet SD312, page 75: the sawtooth through
/// 10 kΩ, R365, to an inverting input with 20 kΩ back, R362), and how
/// much of it Wheel-Mod has is the sheet's nodes ([`Modulator::tick`]).
const LFO_TRIANGLE_SHIFT: u32 = 1;
/// The gain of the filtered noise as Wheel-Mod has it, in Q28.
///
/// Source: own choice: white noise through the filter below keeps a
/// sixth of its level at this module's tick; three times that is about
/// half a unit, the triangle's peak.
const PINK_GAIN: i32 = 3 * ONE_Q28;
/// The corner of the filter that makes Wheel-Mod's noise of white
/// noise, in thousandths of a hertz.
///
/// Source: TM3 sheet SD334 (page 47): R393, 100 kΩ, with C372, 0.01 µF,
/// about the amplifier that follows the noise source; TM2 sheet SD312
/// (page 75): R353 and C333, the same; 1 / (2 π R C) is 159 Hz. TM2
/// section 2-21 (page 43) calls the result pink noise.
const PINK_CORNER: u32 = 159_155;

/// Ticks in one loop of the instrument's computer: six thousandths of a
/// second. Once a loop the computer compares each knob with the number
/// it holds for it and moves the number one step towards the knob, and
/// then sends every number through the converter to the voices: a knob
/// that is turned moves its control voltage one step of the converter's
/// seven bits each loop, however far and fast it was turned.
///
/// Source: TM3 section 2-12 (page 29): "outputting one of 24 entries
/// from a Scratchpad RAM table of pot settings, at the same time the
/// actual pot is being sampled through the ADC "window" comparator
/// ... As long as the voltages don't match, the table value is
/// decremented or incremented until they do match. Once all the pots
/// have been checked ... the DAC again converts the pot values, this
/// time distributing them and the oscillator and filter CVs to the
/// synth. Thus, the DAC output, Vdac, steps between 62 values (24 pots
/// and 38 CVs) during each 6-ms loop". Own reading of it: one value of
/// the converter for each knob in a loop is one comparison, which says
/// only higher or lower, and so one step. Page 31 gives 11 ms as "the
/// longest loop time", which keys held lengthen; the shortest is taken.
/// Each voltage is there at once when its turn comes: "a few
/// microseconds for each S/H to acquire a specific value of Vdac"
/// (page 31), so a step of the number is a step of the voltage. The
/// voice sheets were not searched for a capacitor on any one voltage's
/// way from its S/H, and the manual of revisions 1 and 2 was not read
/// for the loop; their computer is taken to do the same.
const LOOP_TICKS: u32 = TICK_RATE * 6 / 1_000;
const _: () = assert!(LOOP_TICKS == 36);

/// Messages kept while all sound off takes the sound out, to be taken
/// when it has gone: more than come in the 5.3 ms that lasts but from
/// a stream that means to flood, whose next message ends the wait.
///
/// Source: own choice.
const DEFERRED: usize = 16;

/// The most voices unison stacks: all five (S53-42 item 1, "counts above
/// five ... play all five of synth5x's").
const UNISON_MOST: u8 = VOICES as u8;
const _: () = assert!(VOICES <= u8::MAX as usize);
/// Cents of unison detune either way for each step of UNISON DETUNE at the
/// outer voices: `±3d` cents (S53-42 item 2; NeoTeletext's own law, the
/// family document's section 14 item 4).
const DETUNE_CENTS_PER_STEP: i32 = 3;
/// Cents in an octave, for the detune's law.
const CENTS_PER_OCTAVE: i32 = 1_200;

/// Something the instrument is told, kept while all sound off takes the
/// sound out (S53-20: "messages that arrive meanwhile are taken in order
/// when the sound has gone").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Command {
    /// A channel message for [`Synth53::event`].
    Event(u8, u8, u8),
    /// A whole program ([`Synth53::set_program`]).
    Program(Program),
    /// The pitch wheel's range, in cents.
    BendRange(u16),
    /// Brightness, controller 74.
    Brightness(u8),
}

/// The key priority of unison and whether a new key restarts the
/// envelopes, by RETRIGGER AND UNISON's value 0 to 3 (S53-42 item 3): low
/// note, low note retrigger, last note, last note retrigger (User's Guide,
/// page 45, in that order).
const fn key_priority(value: u8) -> (Priority, Trigger) {
    match value {
        0 => (Priority::Low, Trigger::Single),
        1 => (Priority::Low, Trigger::Multiple),
        2 => (Priority::Last, Trigger::Single),
        _ => (Priority::Last, Trigger::Multiple),
    }
}

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
/// The share of the mixed output that is a sample, in Q28: 506 / 1000.
/// One voice's sawtooth through an open filter then peaks 17.3 dB below
/// full scale on synth53, and five struck together 4.7 dB below.
/// What nears full scale is bent under it by the output's limiter
/// ([`crate::output::limit`]).
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's half, as it was first, the note stood 0.1 dB above it on a
/// synth53 and 0.3 dB below it on synth52.
const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 506 / 1000) as i32;

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

    /// One tick: what Wheel-Mod has of the shapes that are on, in Q24,
    /// and the phase moves on. The sawtooth and the square stand on zero: TM3
    /// section 2-4 (page 21), "All 3340 outputs are positive-going", and
    /// only the triangle is shifted to stand about zero (sheet SD334).
    /// The triangle is lowest where the sawtooth falls, and the square
    /// is high about that fall, as the CEM3340 makes its pulse of the
    /// triangle (data sheet, page 4, figure 1); on revisions 1 and 2 the
    /// triangle is highest there ([`Law::triangle_from_top`]).
    fn tick(&mut self, patch: &Patch, law: &Law) -> i32 {
        let phase = self.phase;
        self.phase = phase.wrapping_add(self.step);
        let rise = (phase >> Self::SHIFT).cast_signed();
        let (sawtooth, triangle, square) =
            (patch.on(Switch::LfoSawtooth), patch.on(Switch::LfoTriangle), patch.on(Switch::LfoSquare));
        // From -1/2 at the cycle's start to 1/2 at its middle.
        let folded = if phase < Self::HALF { rise } else { UNIT.saturating_sub(rise) };
        let turning = (folded << 2).saturating_sub(UNIT) >> LFO_TRIANGLE_SHIFT;
        let turning = if law.triangle_from_top { turning.saturating_neg() } else { turning };
        let high = phase.wrapping_add(Self::HALF >> 1) < Self::HALF;
        let level = if high { law::gain(UNIT, law.lfo.2) } else { 0 };
        match law.mixing {
            // Revision 3: each shape through its own resistor into a
            // summer (TM3 sheet SD334).
            Mixing::Currents => {
                let mut sum = 0i32;
                if sawtooth {
                    sum = sum.saturating_add(rise);
                }
                if triangle {
                    sum = sum.saturating_add(turning);
                }
                if square {
                    sum = sum.saturating_add(level);
                }
                sum
            }
            // Source: TM2 sheet SD312 (page 75). The sawtooth and the
            // pulse each through 10 kΩ (R385, R386) to a node with
            // 200 kΩ to ground (R384), as an oscillator's in a voice
            // ([`law::node`]); a follower reads it (a quarter of U372)
            // and feeds Wheel-Mod's amplifier through 10 kΩ (R387),
            // where the triangle comes in through its switch and 10 kΩ
            // (R373). With the triangle on, each side is halved, the
            // follower standing there whether or not it carries a shape:
            // the triangle alone is half of its 10 V, a quarter of the
            // sawtooth's swing either way.
            Mixing::Node => {
                let [sawtooth_share, square_share, _] = law::node(sawtooth, square);
                let followed = law::gain(rise, sawtooth_share).saturating_add(law::gain(level, square_share));
                if triangle { (followed >> 1).saturating_add(turning >> 1) } else { followed }
            }
        }
    }
}

/// synth53.
///
/// It holds no reference and allocates nothing. Two instruments of one
/// model given the same patch and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Synth53 {
    model: Model,
    /// The program of the adopted interface the patch was set from, whose
    /// additions the instrument modelled lacks (S53-41 to S53-43).
    program: Program,
    patch: Patch,
    /// The numbers the computer holds for the knobs, each on its way to
    /// where the patch has its knob ([`LOOP_TICKS`]), with the patch's
    /// switches: what the voices are given.
    panel: Patch,
    /// Ticks until the computer's next loop.
    loop_wait: u32,
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
    /// How far the pitch wheel bends at either end, in cents (S53-8).
    bend_range_cents: u16,
    /// Controller 74: dial steps added to the cutoff (S53-12).
    brightness: u8,
    /// Unison and the key priority the allocator was last set for, which
    /// a change of either lets go of every key (S53-19).
    assignment: (bool, u8),
    wheel: u8,
    volume: u8,
    /// The volume's gain as it stands, in Q28, on its way to what the
    /// controller asks.
    volume_gain: VolumeGain,
    /// All sound off has come and the sound is on its way out: the gain
    /// that takes it there, which the voices are cut at the end of.
    cut: Option<VolumeGain>,
    /// The messages that came meanwhile, in order, and how many.
    deferred: [Command; DEFERRED],
    deferred_count: usize,
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

impl Synth53 {
    /// An instrument at rest playing [`Patch::DEFAULT`], with the default
    /// program's additions (S53-30).
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
        let mut synth53 = Self {
            model,
            program: Program::default_for(model.variant()),
            patch: Patch::DEFAULT,
            panel: Patch::DEFAULT,
            loop_wait: 0,
            settings: Settings::new(&Patch::DEFAULT, model),
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices,
            triggers: [0; VOICES],
            noise: Noise::new(SEED),
            lfo: Modulator { phase: 0, step: 0 },
            wheel_noise: Noise::new(SEED.rotate_left(16)),
            pink,
            bend: 0,
            bend_range_cents: interface::DEFAULT_BEND_RANGE_CENTS,
            brightness: 0,
            assignment: (false, 0),
            wheel: 0,
            volume: DATA_MAX,
            volume_gain: VolumeGain::full(),
            cut: None,
            deferred: [Command::Event(0, 0, 0); DEFERRED],
            deferred_count: 0,
            pedal: false,
            tune: 0,
            glide: 0,
            tick_wait: 0,
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        synth53.apply_patch();
        synth53
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
    ///
    /// In unison the program's RETRIGGER AND UNISON chooses the key and
    /// the trigger (S53-42 item 3); its value 0 is the instrument
    /// modelled's. Polyphonically it changes nothing (S53-40).
    fn mode(&self) -> Mode {
        if self.patch.on(Switch::Unison) {
            let (priority, trigger) = key_priority(self.program.get(parameter::RETRIGGER_AND_UNISON));
            Mode::Unison(priority, trigger)
        } else {
            Mode::Poly(Assign::Cyclic)
        }
    }

    /// Sets the allocator for unison and the key priority as they now
    /// stand, letting go of every key and keeping the pedal, if either
    /// changed since it was last set (S53-19). It is called before a key
    /// is taken and before samples are rendered, so that messages of one
    /// tick that change either and change it back let go of nothing.
    fn settle_assignment(&mut self) {
        let assignment = (self.patch.on(Switch::Unison), self.program.get(parameter::RETRIGGER_AND_UNISON));
        if assignment != self.assignment {
            self.assignment = assignment;
            // The allocator lets go of every key, and of the pedal with
            // them; the pedal has not moved, and is given back.
            self.allocator.set_mode(self.mode());
            self.allocator.sustain(self.pedal);
        }
    }

    /// Which instrument this is.
    #[must_use]
    pub const fn model(&self) -> Model {
        self.model
    }

    /// Puts the instrument back as [`Synth53::new`] made it, but for
    /// the patch and the master tune, which it keeps: silent, its
    /// controllers at rest, the next key on the first voice, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let (program, patch, tune) = (self.program, self.patch, self.tune);
        *self = Self::new(self.model);
        self.tune = tune;
        self.program = program;
        self.set_patch(&patch);
    }

    /// Gives the instrument a program of the adopted interface (S53-18,
    /// S53-29, S53-34): its panel by S53-35 to S53-37, as
    /// [`Synth53::set_patch`] sets it, and its additions (S53-41 to
    /// S53-43). The pitch wheel's range is not taken from it (S53-8).
    /// While all sound off takes the sound out it waits for it to have
    /// gone.
    pub fn set_program(&mut self, program: &Program) {
        if self.defer(Command::Program(*program)) {
            return;
        }
        self.program = *program;
        let patch = Patch::of_program(program, self.model);
        self.set_patch(&patch);
    }

    /// The program the instrument was last given.
    #[must_use]
    pub const fn program(&self) -> &Program {
        &self.program
    }

    /// Sets how far the pitch wheel bends at either end, in cents, held
    /// to an octave (S53-8).
    pub fn set_bend_range_cents(&mut self, cents: u16) {
        if !self.defer(Command::BendRange(cents)) {
            self.bend_range_cents = cents.min(interface::MOST_BEND_RANGE_CENTS);
        }
    }

    /// Sets brightness, controller 74: so many dial steps added to the
    /// cutoff, the sum held at the model's top (S53-12).
    pub fn set_brightness(&mut self, value: u8) {
        if !self.defer(Command::Brightness(value)) {
            self.brightness = value.min(DATA_MAX);
            self.apply_patch();
        }
    }

    /// While all sound off takes the sound out, keeps `command` to be taken
    /// when it has gone, or with no room cuts the sound now; answers
    /// whether it was kept.
    fn defer(&mut self, command: Command) -> bool {
        if self.cut.is_none() {
            return false;
        }
        if let Some(slot) = self.deferred.get_mut(self.deferred_count) {
            *slot = command;
            self.deferred_count = self.deferred_count.saturating_add(1);
            return true;
        }
        self.cut();
        false
    }

    /// Gives the instrument a patch, which it copies. It takes effect on
    /// the voices that are sounding too, as moving the panel's controls
    /// does: the switches at once, and each knob one step of its 128
    /// every six thousandths of a second, as the instrument's computer
    /// follows a knob that is turned. With no voice sounding, all of it
    /// is there at once. Switching unison on or off, or the key priority,
    /// lets go of every key, and leaves the pedal as it is, once the
    /// messages of the tick are all taken (S53-19).
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
        if self.sounding_voices() == 0 {
            self.panel = *patch;
        } else {
            // The switches, which are the top bit of each byte.
            let mut bytes = patch.stored();
            for (byte, held) in bytes.iter_mut().zip(self.panel.stored()) {
                *byte = (*byte & !patch::KNOB_MAX) | (held & patch::KNOB_MAX);
            }
            self.panel = Patch::from_stored(&bytes);
        }
        self.apply_patch();
    }

    /// A loop of the computer: the number it holds for each knob goes
    /// one step towards where the patch has the knob.
    fn scan(&mut self) {
        if self.panel == self.patch {
            return;
        }
        let mut bytes = self.panel.stored();
        for (byte, wanted) in bytes.iter_mut().zip(self.patch.stored()) {
            let (held, wanted) = (*byte & patch::KNOB_MAX, wanted & patch::KNOB_MAX);
            let next = match held.cmp(&wanted) {
                core::cmp::Ordering::Less => held.saturating_add(1),
                core::cmp::Ordering::Equal => held,
                core::cmp::Ordering::Greater => held.saturating_sub(1),
            };
            *byte = (*byte & !patch::KNOB_MAX) | next;
        }
        self.panel = Patch::from_stored(&bytes);
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
        // Brightness turns the cutoff knob so many steps higher, as far as
        // its top (S53-12).
        let mut panel = self.panel;
        let cutoff = panel.get(Knob::Cutoff).min(law.knob_top);
        panel.set(Knob::Cutoff, cutoff.saturating_add(self.brightness).min(law.knob_top));
        // FILTER KEYBOARD TRACK at 1, half (S53-41).
        let half = self.program.get(parameter::FILTER_KEYBOARD_TRACK) == 1 && self.patch.on(Switch::FilterKeyboard);
        self.settings = Settings::new(&panel, self.model).with_half_tracking(half);
        for voice in &mut self.voices {
            voice.configure(&self.settings);
        }
        let steps = i32::from(self.panel.get(Knob::LfoFrequency).min(law.knob_top));
        let rate = law::raise(law.lfo.0, steps.saturating_mul(law.lfo.1));
        self.lfo.step = frequency(rate, TICK_RATE);
    }

    /// How many messages [`Synth53::event`] has ignored since the
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
    ///   (123). All sound off takes the sound out over five thousandths
    ///   of a second, then silences every voice, and leaves the pedal
    ///   where it is; a message that comes before then is taken when
    ///   the sound has gone. The pedal holds the notes whose keys go up while it is
    ///   down, as a MIDI instrument's does, and is besides the
    ///   instrument's release footswitch: while it is down the
    ///   envelopes' releases are as the knobs say though the release
    ///   switch is off (OM section 1-5, page 10).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven.
    ///
    /// Anything else is ignored and counted
    /// ([`Synth53::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        // While all sound off takes the sound out, a message waits for
        // it to have gone; with no room to wait, the sound is cut now.
        if self.defer(Command::Event(status, data1, data2)) {
            return;
        }
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        let kind = status & crate::midi::KIND_MASK;
        if matches!(kind, STATUS_NOTE_ON | STATUS_NOTE_OFF) {
            self.settle_assignment();
        }
        match kind {
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
            // The sound goes out over the 5.3 ms a volume's gain takes,
            // and the voices are cut when it has gone: cut where they
            // stood, the signal would fall to nothing in one sample.
            // The keys are let go then and not before: a release at its
            // shortest is quicker than the fade.
            CONTROL_ALL_SOUND_OFF => {
                if self.sounding_voices() == 0 {
                    self.cut();
                } else if self.cut.is_none() {
                    self.cut = Some(VolumeGain::full());
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                self.allocator.sustain(false);
            }
            CONTROL_ALL_NOTES_OFF => {
                self.settle_assignment();
                // As a note off for every key: the pedal still holds. In
                // unison a key that goes up hands its note to another
                // still held, so the key that sounds is let go until
                // none does: once for each key the allocator remembers
                // at most, and once for each voice otherwise.
                for _ in 0..HELD_KEYS {
                    let held = self.allocator.voices().iter().find(|voice| voice.stage == Stage::Held);
                    let Some(key) = held.map(|voice| voice.key) else {
                        break;
                    };
                    self.allocator.key_up(key);
                }
            }
            _ => self.ignore(),
        }
    }

    /// Silences every voice at once: what all sound off comes to.
    fn cut(&mut self) {
        self.cut = None;
        self.allocator.all_keys_up();
        // The allocator lets go of the pedal with the keys; the pedal
        // has not moved, and is given back.
        self.allocator.sustain(self.pedal);
        for (index, voice) in self.voices.iter_mut().enumerate() {
            voice.cut();
            voice.configure(&self.settings);
            self.allocator.finished(index);
        }
        self.take_deferred();
    }

    /// Takes the messages that waited for the sound to go, in order.
    fn take_deferred(&mut self) {
        let (deferred, count) = (self.deferred, self.deferred_count);
        self.deferred_count = 0;
        for command in deferred.iter().take(count) {
            match *command {
                Command::Event(status, data1, data2) => self.event(status, data1, data2),
                Command::Program(program) => self.set_program(&program),
                Command::BendRange(cents) => self.set_bend_range_cents(cents),
                Command::Brightness(value) => self.set_brightness(value),
            }
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
        let steps = i32::from(self.panel.get(Knob::Glide).min(law.knob_top));
        if steps == 0 {
            self.glide = target;
            return target;
        }
        // Octaves a tick at full rate.
        let (slowest, step, most) = law.glide;
        let faster = i32::from(law.knob_top).saturating_sub(steps).saturating_mul(step);
        let most = most.saturating_mul(OCTAVE.cast_unsigned() / TICK_RATE);
        let rate = law::raise(slowest / TICK_RATE, faster).min(most).min(i32::MAX.cast_unsigned()).cast_signed();
        let distance = target.saturating_sub(self.glide);
        let within = distance.clamp(law.glide_zone.saturating_neg(), law.glide_zone);
        let step = law::share(rate, within, law.glide_zone);
        // A share cut to nothing would never arrive.
        self.glide = if step == 0 || step.abs() >= distance.abs() { target } else { self.glide.saturating_add(step) };
        self.glide
    }

    /// A tick: the voices take what the keys and the controls ask.
    fn control(&mut self) {
        if self.loop_wait == 0 {
            self.loop_wait = LOOP_TICKS;
            self.scan();
        }
        self.loop_wait = self.loop_wait.saturating_sub(1);
        let law = self.model.law();
        let lfo = self.lfo.tick(&self.patch, law);
        let pink = amplify(self.pink.low_pass(self.wheel_noise.tick()), PINK_GAIN);
        // The source mix moves the two levels opposite ways (TM3 section
        // 2-3, page 20), and the wheel is the amount of their sum.
        let mix = i32::from(self.panel.get(Knob::SourceMix).min(law.knob_top));
        let top = i32::from(law.knob_top);
        let source = law::share(lfo, top.saturating_sub(mix), top).saturating_add(law::share(pink, mix, top));
        // The wheel adds to the low-frequency oscillator's initial amount,
        // the sum held to the whole (S53-43): `min(1, initial ÷ 120 +
        // wheel ÷ 127)`, over the common denominator 120 × 127.
        let initial = i32::from(self.program.get(parameter::LFO_INITIAL_AMOUNT));
        let (initial_top, wheel_top) = (i32::from(interface::KNOB_TOP), i32::from(DATA_MAX));
        let whole = initial_top.saturating_mul(wheel_top);
        let amount =
            initial.saturating_mul(wheel_top).saturating_add(i32::from(self.wheel).saturating_mul(initial_top));
        let wheel = law::share(source, amount.min(whole), whole);
        // The bender's whole travel either way is the range (S53-8).
        let range = law::cents(i32::from(self.bend_range_cents));
        let bend = law::share(range, self.bend, PITCH_BEND_CENTRE);
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
        // Unison stacks voices 1 to the count; the others are let go
        // (S53-42 item 1).
        let stacked = if unison.is_some() { self.unison_count() } else { UNISON_MOST };
        let detune = self.program.get(parameter::UNISON_DETUNE);
        for (index, ((voice, key), seen)) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers).enumerate() {
            if key.stage == Stage::Rest {
                continue;
            }
            let mut controls = controls;
            if unison.is_some() {
                let detuned = Self::detune(index, stacked, detune);
                controls.pitch = controls.pitch.saturating_add(detuned);
            }
            if usize::from(stacked) <= index {
                *seen = key.triggers;
                voice.release();
            } else if *seen != key.triggers {
                *seen = key.triggers;
                voice.start(Self::key_number(key.key));
            } else if key.gate() {
                voice.legato(Self::key_number(key.key));
            } else {
                voice.release();
            }
            if !voice.at_rest() {
                voice.tick(&self.settings, controls);
            }
        }
    }

    /// The voices unison stacks: UNISON VOICE COUNT's value v stacks
    /// v + 1, and all five from 4 up (S53-42 item 1).
    fn unison_count(&self) -> u8 {
        self.program.get(parameter::UNISON_VOICE_COUNT).saturating_add(1).min(UNISON_MOST)
    }

    /// The tuning of voice `index` of `count` stacked at UNISON DETUNE
    /// `detune`, in octaves: `(2k − (n − 1)) × d × 3 ÷ (n − 1)` cents, the
    /// voices spread evenly over `±3d` cents, and none for one voice
    /// (S53-42 item 2).
    fn detune(index: usize, count: u8, detune: u8) -> i32 {
        let span = i32::from(count).saturating_sub(1);
        if span <= 0 {
            return 0;
        }
        let place = i32::try_from(index).unwrap_or(0).saturating_mul(2).saturating_sub(span);
        let cents = place.saturating_mul(i32::from(detune)).saturating_mul(DETUNE_CENTS_PER_STEP);
        law::octaves(cents, CENTS_PER_OCTAVE.saturating_mul(span))
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
            if key.stage != Stage::Rest && !voice.at_rest() {
                sum = sum.wrapping_add(voice.sample(&self.settings, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of five voices,
    /// whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        if !out.is_empty() {
            self.settle_assignment();
        }
        let mut volume = volume::wanted(self.volume);
        for sample in out {
            let mut earlier = self.voices_sample();
            let mut later = self.voices_sample();
            // Faded before the decimator, which then ends on silence:
            // what it still holds when the voices are cut is nothing.
            if let Some(cut) = &mut self.cut {
                let gain = cut.towards(0);
                (earlier, later) = (amplify(earlier, gain), amplify(later, gain));
                if gain == 0 {
                    // The messages that waited are taken now, the
                    // volume's among them.
                    self.cut();
                    volume = volume::wanted(self.volume);
                }
            }
            let signal = amplify(self.decimator.process(earlier, later), self.volume_gain.towards(volume));
            *sample = to_sample(amplify(signal, OUTPUT_GAIN));
        }
    }
}

/// The shares of this module are of a unit that is also a signal's full
/// level and an envelope's quarter.
const _: () = assert!(WHEEL_MOD == crate::analog::LEVEL);
/// All notes off lets go of one key in each turn, and has a turn for
/// every key the allocator remembers: there are as many for the voices.
const _: () = assert!(VOICES <= HELD_KEYS);
