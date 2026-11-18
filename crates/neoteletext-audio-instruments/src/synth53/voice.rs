//! One voice: two oscillators, the mixer, the four-pole filter, two
//! envelopes and the amplifier, with Poly-Mod between them.
//!
//! A voice runs at twice the output rate. Its control voltages are
//! analogue and continuous in the instrument; here what moves slowly is
//! worked out at each tick of [`TICK_SAMPLES`] samples (the envelopes,
//! the pitches, the cutoff, the pulse widths), and only what oscillator
//! B does to oscillator A and to the filter through Poly-Mod, which
//! moves at audio rate, at every sample.
//!
//! # Poly-Mod from oscillator B
//!
//! What Poly-Mod carries of oscillator B is taken from B's phase, not
//! from B's band-limited output: the voltage in the instrument has no
//! ripple either side of an edge, and through the exponential
//! converters that ripple became tones of its own. Each sample is made
//! in stretches that end where that voltage jumps or turns (B's cycle
//! starting, its pulse falling, its triangle turning):
//!
//! - **Oscillator A's frequency** is given for each end of a stretch,
//!   and the building block's oscillator puts A's edges where the
//!   moving frequency does and rounds the corners its jumps make
//!   ([`Oscillator::run`]).
//! - **Oscillator A's pulse width** slides through a stretch and jumps
//!   between two, and the pulse turns where it jumps
//!   ([`Oscillator::jump_width`]). A stretch ends too where the width
//!   meets an end of its range, none or the whole of a cycle, and stays:
//!   the width turns a corner there, and where the pulse next turns can
//!   hang on where that corner was.
//! - **The cutoff**, which the filter takes once a sample, is its mean
//!   over the sample.
//!
//! `tests/synth53_aliasing.rs` measures what is no harmonic, below
//! 20 kHz, against the strongest harmonic, on the five Cs from 131 Hz to
//! 2,093 Hz, with oscillator A synchronised to B. The frequency: 91 dB
//! down or better at every amount on every key (47 dB at worst before).
//! The pulse width: 92 dB or better to half the amount, and 80 dB at all
//! of it (32 dB before), where on revision 3 the width is at the end of
//! its range for most of every cycle and little of the pulse is left to
//! measure against. The cutoff, from B's sawtooth with the knob at
//! 6: at a quarter of the amount 94, 92, 85, 75 and 53 dB on the five
//! keys; at a half 94, 90, 73, 56 and 52 dB; at all of it 74, 65, 59, 53
//! and 54 dB (before: 75 to 34 dB, 59 to 25 dB and 31 to 22 dB). The
//! cutoff is short of 80 dB from the C of 1,047 Hz up at a quarter and
//! from the C of 523 Hz up beyond: what is left are the harmonics from
//! 77 kHz to 92 kHz of a filter whose cutoff jumps once a cycle, folded
//! about the voices' rate; rounding the corner the jump makes in the
//! filter's output was tried and gained 3 dB.
//!
//! Five voices with everything on render 12.8 times faster than real
//! time (16.4 before the stretches), and 8.4 to 10.2 times with
//! oscillator B at the top of its knob under the keyboard's top octave,
//! where every other sample is made in two stretches or three: the most
//! a sample costs (`tests/synth53_instrument.rs`, `rendering_speed`,
//! 17 November 2026, one core of a 3.7 GHz Intel Core i9-10900KF, with
//! other work on the machine).

use crate::analog::envelope::{ENVELOPE_ONE, REST_LEVEL, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{ONE_Q28, OnePole, OtaCascade};
use crate::analog::fixed::{amplify, mul_high_unsigned, ratio, scale};
use crate::analog::oscillator::{Oscillator, Waves, waveform};
use crate::analog::{LEVEL, Oversampling, frequency};

use super::Model;
use super::law::{self, Law, Mixing, OCTAVE, OSCILLATION_ONSET, SEMITONES, UNIT};
use super::patch::{Knob, Patch, Switch};

/// How far above the output rate the voices run.
///
/// Source: own choice: the oscillators' edges are 91 dB clean at twice
/// the output rate and 48 dB at the rate itself
/// ([`crate::analog::oscillator`]), and the filter distorts.
pub const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// The voices' rate in hertz.
pub const RATE: u32 = OVERSAMPLING.rate();
/// Bits of the samples in a tick.
const TICK_BITS: u32 = 4;
/// Samples of the voices between two ticks: a sixth of a millisecond.
///
/// Source: own choice: a power of two for the amplifier's slope, and
/// short enough for the shortest envelope. An attack whose time constant
/// is 0.44 ms, 2.6 ticks, takes four ticks to its peak: the amplifier's
/// gain, which runs in a straight line through each tick, stays within
/// 2% of the peak of the curve the envelope's capacitor draws; the
/// cutoff, which is set once a tick, takes that envelope in four steps
/// of 41%, 28%, 19% and 12% of its swing. Measured on 17 November 2026
/// against a build with a tick of one sample, over the first 6 ms of a
/// sawtooth with both attacks at 0: where the note has anything of its
/// own (from the C of 262 Hz up) each band's energy is within 0.4 dB;
/// under the lowest C with the envelope amount at its top, the steps
/// add 7 to 10 dB to what is there from 6 kHz to 20 kHz, which is then
/// 53 dB below the attack's energy under 2 kHz. No click and no zipper
/// was found, and the cutoff is left as it is.
pub const TICK_SAMPLES: u32 = 1 << TICK_BITS;
/// Ticks a second.
pub const TICK_RATE: u32 = RATE / TICK_SAMPLES;
const _: () = assert!(TICK_RATE == 6_000 && RATE.is_multiple_of(TICK_SAMPLES));

/// The frequency step, at the voices' rate, of the lowest key with an
/// oscillator's frequency knob at 0: the C of 16.35 Hz, 57 semitones
/// below an A of 440 Hz, times 2³² over 96,000.
///
/// Source: TM3 section 2-13 (page 32): the instrument tunes each
/// oscillator at "the ten Cs C0-C9", and section 2-4 (page 21): "the
/// basic oscillator range is nine octaves; resulting from 0-5V supplied
/// by the KBD and 0-4V supplied by the OSC FREQ knob": the lowest key
/// with the knob at 0 is C0. TM2 adjustment 4-18 (page 117) agrees: with
/// the knob 39 semitones up, the lowest key "should be about 155.5 Hz",
/// which is 39 semitones above 16.35 Hz. The number is 440 × 2^(-57/12) ×
/// 2³² / 96,000 = 731,558.1, which a test below checks, and
/// `tests/synth53_pitch.rs` by the pitch.
const STEP_OF_C0: u32 = 731_558;
/// The frequency step of 440 Hz, where the filters are tuned.
const STEP_OF_A: u32 = frequency(440_000, RATE);

/// The widest pulse: a cycle less a sliver, in Q24. From here the pulse
/// is high throughout: it "degenerates to dc" (OM section 2-3, page 15).
const WIDTH_FULL: i32 = UNIT - 1;
/// The left shift from a share of a cycle in Q24 to an oscillator's
/// width.
const WIDTH_SHIFT: u32 = 32 - law::OCTAVE_BITS;

/// The octaves of pitch for each unit of Wheel-Mod: 100 / 182, in Q31.
///
/// Source: TM3 sheet SD334 (page 47): Wheel-Mod enters each oscillator's
/// summer through 182 kΩ (R3104, R3103) against the 100 kΩ of a volt to
/// the octave; TM2 sheet SD312 (page 75): R3117 and R3100, 182 kΩ.
const WHEEL_PITCH: i32 = ((100i64 << 31) / 182) as i32;
/// The octaves of cutoff for each unit of Wheel-Mod: 100 / 13.3, in Q28.
///
/// Source: TM3 sheet SD334: R399, 13.3 kΩ; TM2 sheet SD312: R398,
/// 13.3 kΩ.
const WHEEL_CUTOFF: i32 = ((1_000i64 << 28) / 133) as i32;
/// The octaves of oscillator A's pitch for each volt of Poly-Mod: 100 /
/// 301, in Q28.
///
/// Source: TM3 SD431 (page 50): R4357, 301 kΩ; TM2 SD411 (page 85):
/// R498, 300 kΩ.
const POLY_MOD_PITCH: i32 = ((100i64 << 28) / 301) as i32;
/// The octaves of cutoff for each volt of Poly-Mod: 100 / 54.9, in Q28.
///
/// Source: TM3 SD431: R4181, 54.9 kΩ; TM2 SD411 (page 85): R4105,
/// 54.9 kΩ, from the switch that Poly-Mod's filter switch closes into
/// the cutoff's summer (the 100 kΩ beside it, R4109, is the filter
/// envelope amount's).
const POLY_MOD_CUTOFF: i32 = ((1_000i64 << 28) / 549) as i32;

/// The volts of Poly-Mod that oscillator B's sawtooth makes, peak to
/// peak, with its amount at the top, in Q24.
///
/// Source: own estimate from TM3 SD431 as far as it was read, not a
/// figure of the manual, which gives no depth: the sawtooth's 10 V
/// through 150 kΩ onto the amount amplifier's 330 Ω input is 22 mV; a
/// CA3280 passes about 19.2 times its control current for each volt
/// there; a control current taken as 275 µA (ten volts through the 33 kΩ
/// and 3.3 kΩ the mixer's amplifiers have) then gives 116 µA into the
/// 30 kΩ the two amounts share (R4108): 3.5 V.
const POLY_MOD_OSC_B_VOLTS: i64 = (UNIT as i64) * 35 / 10;
/// The volts of Poly-Mod that the filter envelope's peak makes with its
/// amount at the top, in Q24.
///
/// Source: own choice: the envelope's own 5 V (CEM3310 data sheet, page
/// 2: "Attack peak voltage 5.0 V"), as an amount amplifier at a gain of
/// one would pass it; the resistors that set it were not all read.
const POLY_MOD_ENVELOPE_VOLTS: i32 = 5 * UNIT;
/// The octaves the filter envelope's peak moves the cutoff with the
/// envelope amount at the top.
///
/// Source: own choice: what Poly-Mod's path gives the same envelope
/// ([`POLY_MOD_ENVELOPE_VOLTS`] times 100 / 54.9), since TM3 section 2-7
/// (page 22) calls that path "a redundant path for the FILT ENV GEN
/// output"; neither manual gives the depth.
const ENVELOPE_CUTOFF: i32 = (POLY_MOD_ENVELOPE_VOLTS as i64 * 1_000 / 549) as i32;

/// The right shift of the mixer's sum into the filter: a waveform at
/// full level then stands at an eighth of the level where the filter's
/// stages saturate, and both oscillators and the noise together at
/// three eighths.
///
/// Source: own choice. SSM2040 data sheet, page 2: a stage distorts by
/// 0.1% at 30 mV, and the application's 10 V are 20 mV there; CEM3320
/// data sheet, page 2: 0.1% "in passband" 3 dB below clipping. Both
/// filters are nearly clean at one waveform's level.
const DRIVE_SHIFT: u32 = 2;
/// The right shift of the noise that is always in the filter's input: 78
/// dB below a waveform. An amplifier's own noise is what starts a real
/// filter oscillating; without any, this one would stay at rest for
/// ever.
///
/// Source: own choice of the level.
const HISS_SHIFT: u32 = 13;
/// The lowest and highest cutoff a filter is set to, as octaves from
/// 440 Hz: 1.7 Hz, and 56 kHz, which the filter block holds to 0.45 of
/// the rate.
///
/// Source: own choice of where the sum of the control voltages stops:
/// CEM3320 data sheet, page 1: "12 octave range minimum", typically
/// 10,000 to 1; SSM2040 data sheet, page 1: "10,000 to 1".
const CUTOFF_BOUNDS: (i32, i32) = (-8 * OCTAVE, 7 * OCTAVE);
/// The lowest and highest pitch an oscillator is set to, as octaves
/// from C0: the CEM3340's and the SSM2030's ranges are wider than this.
///
/// Source: own choice: 0.004 Hz to the oscillator block's bound.
const PITCH_BOUNDS: (i32, i32) = (-12 * OCTAVE, 11 * OCTAVE);

/// What a patch asks of every voice, worked out once when it is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "each is a switch of the panel, on or off")]
pub struct Settings {
    /// The waveforms each oscillator makes.
    waves_a: u8,
    waves_b: u8,
    /// The mixer's gain of oscillator A's sawtooth and pulse, and of
    /// oscillator B's sawtooth, pulse and triangle, in Q30.
    mix_a: [i32; 2],
    mix_b: [i32; 3],
    noise: i32,
    /// What Poly-Mod takes of oscillator B's sawtooth, pulse and
    /// triangle, in Q30: the first two between zero and their peak, the
    /// triangle either side of zero.
    source_b: [i32; 3],
    /// The gain from that to octaves of oscillator A's pitch, to octaves
    /// of cutoff and to a share of a cycle of pulse width, in Q28; zero
    /// where its switch is off.
    poly_pitch: i32,
    poly_cutoff: i32,
    poly_width: i32,
    /// Volts of Poly-Mod at the filter envelope's peak, and its three
    /// switches.
    poly_envelope: i32,
    poly_to_pitch: bool,
    poly_to_width: bool,
    poly_to_cutoff: bool,
    sync: bool,
    /// Octaves above C0 of each oscillator before the key.
    pitch_a: i32,
    pitch_b: i32,
    keyboard_b: bool,
    /// Pulse widths as shares of a cycle in Q24.
    width_a: i32,
    width_b: i32,
    wheel_pitch_a: bool,
    wheel_pitch_b: bool,
    wheel_width_a: bool,
    wheel_width_b: bool,
    wheel_cutoff: bool,
    /// Octaves from 440 Hz of the cutoff before the key, the envelope
    /// and the wheel.
    cutoff: i32,
    keyboard_filter: bool,
    /// The cutoff follows the key at half its rate: FILTER KEYBOARD TRACK
    /// at half (S53-41).
    half_tracking: bool,
    resonance: i32,
    /// Octaves the filter envelope's peak moves the cutoff.
    envelope_cutoff: i32,
    /// The envelopes' coefficients for a tick: attack, decay, release,
    /// the release with its switch off, and the sustain level.
    filter_envelope: Times,
    amplifier_envelope: Times,
    release: bool,
}

/// One envelope's settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Times {
    attack: i32,
    decay: i32,
    release: i32,
    shortest: i32,
    sustain: i32,
}

impl Times {
    fn new(patch: &Patch, law: &Law, knobs: [Knob; 4]) -> Self {
        let [attack, decay, sustain, release] = knobs;
        let coefficient = |steps: i32| {
            let (anchor, at, step) = law.envelope;
            let octaves = i64::from(steps.saturating_sub(at)).saturating_mul(i64::from(step));
            let octaves = i32::try_from(octaves).unwrap_or(0);
            let constant = law::raise(anchor, octaves).clamp(law.envelope_bounds.0, law.envelope_bounds.1);
            time_constant(constant, TICK_RATE)
        };
        let steps = |knob: Knob| i32::from(patch.get(knob).min(law.knob_top));
        Self {
            attack: coefficient(steps(attack)),
            decay: coefficient(steps(decay)),
            release: coefficient(steps(release)),
            shortest: coefficient(0),
            // The sustain's voltage reaches the envelope's peak at ten
            // volts, 120 steps, on both revisions.
            sustain: law::share(ENVELOPE_ONE, steps(sustain).min(SUSTAIN_FULL), SUSTAIN_FULL),
        }
    }

    fn apply(&self, envelope: &mut RcEnvelope, release: bool) {
        envelope.set_attack(self.attack);
        envelope.set_decay(self.decay);
        envelope.set_release(if release { self.release } else { self.shortest });
        envelope.set_sustain(self.sustain);
    }
}

/// The steps at which a sustain knob's voltage is the envelope's peak:
/// ten volts.
///
/// Source: TM3 section 2-7 (page 22): "the SUS level is set by 0 to +5V"
/// from a knob of 0 to 10 V; SSM2050 data sheet, page 2: the peak is
/// 10 V.
const SUSTAIN_FULL: i32 = 120;
/// The steps of a mixer knob at full level, and of every amount: ten
/// volts.
const AMOUNT_FULL: i32 = 120;

impl Settings {
    /// The settings of a patch on a model.
    #[allow(clippy::too_many_lines, reason = "one line for each control of the panel")]
    pub fn new(patch: &Patch, model: Model) -> Self {
        let law = model.law();
        let steps = |knob: Knob| i32::from(patch.get(knob).min(law.knob_top));
        let amount = |knob: Knob| law::share(ENVELOPE_ONE, steps(knob).min(AMOUNT_FULL), AMOUNT_FULL);
        let on = |switch: Switch| patch.on(switch);

        let (a_saw, a_pulse) = (on(Switch::OscASawtooth), on(Switch::OscAPulse));
        let (b_saw, b_pulse, b_triangle) = (on(Switch::OscBSawtooth), on(Switch::OscBPulse), on(Switch::OscBTriangle));
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        // How much of each waveform there is where they meet.
        let (pulse_level, triangle_level) = law.levels;
        let whole = |on: bool| if on { ENVELOPE_ONE } else { 0 };
        let (shares_a, shares_b) = match law.mixing {
            Mixing::Currents => ([whole(a_saw), whole(a_pulse), 0], [whole(b_saw), whole(b_pulse), whole(b_triangle)]),
            Mixing::Node => (law::node(a_saw, a_pulse), law::followed_node(b_saw, b_pulse, b_triangle)),
        };
        // The oscillator block's triangle is lowest where its sawtooth
        // falls; a revision whose triangle is at its top there has it
        // turned over, in the mixer and in Poly-Mod alike.
        let triangle_level = if law.triangle_from_top { triangle_level.saturating_neg() } else { triangle_level };
        let levels = [ENVELOPE_ONE, pulse_level, triangle_level];
        let weigh = |shares: [i32; 3], level: i32| {
            let [saw, pulse, triangle] = shares;
            let [saw_size, pulse_size, triangle_size] = levels;
            [(saw, saw_size), (pulse, pulse_size), (triangle, triangle_size)]
                .map(|(share, size)| law::gain(law::gain(share, size), level))
        };
        // Source: TM3 sheet SD431 (page 50): each oscillator's mixer
        // amplifier, half of U464, takes the sawtooth at the input
        // marked "+" (R4364; R4241) and the pulse and the triangle at
        // the other (R4366; R4250 and R4285), so those two are upside
        // down against the sawtooth in the mix. Poly-Mod's own node
        // (R4243, R4251, R4280) takes all three one way round. Revision
        // 2's waveforms all reach the one input marked "+" of their
        // mixer amplifier, the other grounded (TM2 SD411, pages 85 and
        // 86: half of U473, an SSM2020, for each oscillator).
        let turned = |[saw, pulse, triangle]: [i32; 3]| match law.mixing {
            Mixing::Currents => [saw, pulse.saturating_neg(), triangle.saturating_neg()],
            Mixing::Node => [saw, pulse, triangle],
        };
        let [mix_a_saw, mix_a_pulse, _] = turned(weigh(shares_a, amount(Knob::MixerOscA)));
        let mix_b = turned(weigh(shares_b, amount(Knob::MixerOscB)));
        // Poly-Mod takes the waveforms as they stand, the sawtooth and
        // the pulse above zero and so at half their swing either side of
        // their own middle; the triangle is about zero already.
        let [source_saw, source_pulse, source_triangle] = weigh(shares_b, ENVELOPE_ONE);
        let source_b = [source_saw >> 1, source_pulse >> 1, source_triangle >> 1];

        let poly_osc_b = POLY_MOD_OSC_B_VOLTS
            .saturating_mul(i64::from(steps(Knob::PolyModOscB).min(AMOUNT_FULL)))
            .checked_div(i64::from(AMOUNT_FULL))
            .unwrap_or(0);
        // Volts for a waveform at its peak, in Q28 of a signal's level.
        let poly_gain = i32::try_from(poly_osc_b << (28 - law::OCTAVE_BITS)).unwrap_or(0);
        let through = |on: bool, gain: i32| {
            let product = i64::from(poly_gain).saturating_mul(i64::from(gain)) >> 28;
            if on { i32::try_from(product).unwrap_or(0) } else { 0 }
        };
        let (to_pitch, to_width, to_cutoff) =
            (on(Switch::PolyModFrequencyA), on(Switch::PolyModPulseWidthA), on(Switch::PolyModFilter));

        let semitones = |knob: Knob| {
            let (times, over, most) = law.frequency;
            (steps(knob).saturating_mul(times).checked_div(over).unwrap_or(0)).min(most)
        };
        let low = on(Switch::OscBLowFrequency);
        let pitch_b = match (low, law.low_frequency) {
            (true, (offset, Some(step))) => offset.saturating_add(steps(Knob::OscBFrequency).saturating_mul(step)),
            (true, (offset, None)) => offset.saturating_add(law::semitones(semitones(Knob::OscBFrequency))),
            (false, _) => law::semitones(semitones(Knob::OscBFrequency)),
        };
        let width = |knob: Knob| law.pulse_width.0.saturating_add(steps(knob).saturating_mul(law.pulse_width.1));

        let resonance = law::resonance(law, steps(Knob::Resonance));
        // Above the onset the oscillation is flat by its share of the
        // most; the cutoff is raised to meet it.
        let top = law::resonance(law, i32::from(law.knob_top));
        let above = resonance.saturating_sub(OSCILLATION_ONSET).max(0);
        let tuning = law::share(law.oscillation_flat, above, top.saturating_sub(OSCILLATION_ONSET).max(1));

        Self {
            waves_a: flag(a_saw, waveform::SAWTOOTH) | flag(a_pulse, waveform::PULSE),
            waves_b: flag(b_saw, waveform::SAWTOOTH)
                | flag(b_pulse, waveform::PULSE)
                | flag(b_triangle, waveform::TRIANGLE),
            mix_a: [mix_a_saw, mix_a_pulse],
            mix_b,
            noise: amount(Knob::MixerNoise),
            source_b,
            poly_pitch: through(to_pitch, POLY_MOD_PITCH),
            poly_cutoff: through(to_cutoff, POLY_MOD_CUTOFF),
            poly_width: through(to_width, law.pulse_width_modulation.2 << (28 - law::OCTAVE_BITS)),
            poly_envelope: law::gain(POLY_MOD_ENVELOPE_VOLTS, amount(Knob::PolyModFilterEnvelope)),
            poly_to_pitch: to_pitch,
            poly_to_width: to_width,
            poly_to_cutoff: to_cutoff,
            sync: on(Switch::Sync),
            pitch_a: law::semitones(semitones(Knob::OscAFrequency)),
            pitch_b: pitch_b.saturating_add(steps(Knob::OscBFine).saturating_mul(law.fine_step)),
            keyboard_b: on(Switch::OscBKeyboard),
            width_a: width(Knob::OscAPulseWidth),
            width_b: width(Knob::OscBPulseWidth),
            wheel_pitch_a: on(Switch::WheelFrequencyA),
            wheel_pitch_b: on(Switch::WheelFrequencyB),
            wheel_width_a: on(Switch::WheelPulseWidthA),
            wheel_width_b: on(Switch::WheelPulseWidthB),
            wheel_cutoff: on(Switch::WheelFilter),
            cutoff: law.cutoff.0.saturating_add(law::semitones(steps(Knob::Cutoff))).saturating_add(tuning),
            keyboard_filter: on(Switch::FilterKeyboard),
            half_tracking: false,
            resonance,
            envelope_cutoff: law::gain(ENVELOPE_CUTOFF, amount(Knob::FilterEnvelopeAmount)),
            filter_envelope: Times::new(
                patch,
                law,
                [Knob::FilterAttack, Knob::FilterDecay, Knob::FilterSustain, Knob::FilterRelease],
            ),
            amplifier_envelope: Times::new(
                patch,
                law,
                [Knob::AmplifierAttack, Knob::AmplifierDecay, Knob::AmplifierSustain, Knob::AmplifierRelease],
            ),
            release: on(Switch::Release),
        }
    }

    /// The settings with the cutoff following the key at half the rate
    /// the keyboard switch gives it, or at the whole (S53-41: "Half is read
    /// as half the key's voltage, as the name says").
    #[must_use]
    pub const fn with_half_tracking(mut self, half: bool) -> Self {
        self.half_tracking = half;
        self
    }

    /// Whether oscillator B moves anything through Poly-Mod, which is
    /// then worked out at every sample.
    const fn poly_at_audio_rate(&self) -> bool {
        self.poly_pitch != 0 || self.poly_cutoff != 0 || self.poly_width != 0
    }
}

/// What every voice is given at a tick: the instrument's modulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// Wheel-Mod: the wheel's share of the mix of the low-frequency
    /// oscillator and the noise, in Q24 units ([`super::WHEEL_MOD`]).
    pub wheel: i32,
    /// Octaves the pitch wheel and the master tune move both
    /// oscillators.
    pub pitch: i32,
    /// In unison, the octaves above the lowest key that the glide has
    /// reached; otherwise nothing, and each voice takes its own key.
    pub unison: Option<i32>,
    /// Whether the release footswitch is down.
    pub pedal: bool,
}

/// One of the five voices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Voice {
    law: &'static Law,
    oscillator_a: Oscillator,
    oscillator_b: Oscillator,
    filter: OtaCascade,
    blocker: OnePole,
    filter_envelope: RcEnvelope,
    amplifier_envelope: RcEnvelope,
    /// Semitones of the key above the lowest key of the keyboard.
    key: i32,
    gate: bool,
    /// What a tick left for the samples: oscillator A's step, its width
    /// and the cutoff's step before oscillator B's Poly-Mod.
    step_a: u32,
    width_a: i32,
    cutoff_step: u32,
    /// Oscillator B's width as it was last given it.
    width_b: u32,
    /// What Poly-Mod had of oscillator B at the end of the last sample,
    /// with oscillator A's step and the cutoff's under it, while no
    /// tick has moved them since.
    carried: Option<(i32, u32, u32)>,
    /// The mean of each oscillator's pulse at its width, which the mixer
    /// does not pass.
    mean_a: i32,
    mean_b: i32,
    /// The amplifier's gain in Q30 and what it gains each sample.
    gain: i32,
    gain_step: i32,
    /// The voice's own offsets, in octaves: of each oscillator's tuning
    /// and of the cutoff.
    trim_a: i32,
    trim_b: i32,
    trim_cutoff: i32,
}

impl Voice {
    /// A voice at rest with its own tolerances, in octaves.
    pub fn new(model: Model, trims: [i32; 3]) -> Self {
        let law = model.law();
        let [trim_a, trim_b, trim_cutoff] = trims;
        let mut blocker = OnePole::new();
        blocker.set_cutoff(frequency(law.blocking.1, RATE));
        Self {
            law,
            oscillator_a: Oscillator::new(),
            oscillator_b: Oscillator::new(),
            filter: OtaCascade::new(),
            blocker,
            filter_envelope: RcEnvelope::new(),
            amplifier_envelope: RcEnvelope::new(),
            key: 0,
            gate: false,
            step_a: 0,
            width_a: 0,
            cutoff_step: 0,
            width_b: 0,
            carried: None,
            mean_a: 0,
            mean_b: 0,
            gain: 0,
            gain_step: 0,
            trim_a,
            trim_b,
            trim_cutoff,
        }
    }

    /// Takes a patch's settings: the oscillators' waveforms.
    pub const fn configure(&mut self, settings: &Settings) {
        self.oscillator_a.enable(settings.waves_a);
        self.oscillator_b.enable(settings.waves_b);
    }

    /// A key for this voice, as semitones above the keyboard's lowest:
    /// both envelopes start their attack again from where they are
    /// (CEM3310 data sheet, page 2, and SSM2050 data sheet, page 2, the
    /// waveforms of a second trigger).
    pub const fn start(&mut self, key: i32) {
        self.key = key;
        self.gate = true;
        self.filter_envelope.retrigger();
        self.amplifier_envelope.retrigger();
    }

    /// A key for this voice whose envelopes go on as they are: a note
    /// that follows another in unison.
    pub const fn legato(&mut self, key: i32) {
        self.key = key;
        self.gate = true;
    }

    /// The key is up.
    pub const fn release(&mut self) {
        self.gate = false;
    }

    /// Silences the voice at once.
    pub fn cut(&mut self) {
        let fresh = Self {
            law: self.law,
            oscillator_a: Oscillator::new(),
            oscillator_b: Oscillator::new(),
            filter: OtaCascade::new(),
            blocker: self.blocker.clone(),
            filter_envelope: RcEnvelope::new(),
            amplifier_envelope: RcEnvelope::new(),
            key: self.key,
            gate: false,
            step_a: 0,
            width_a: 0,
            cutoff_step: 0,
            width_b: 0,
            carried: None,
            mean_a: 0,
            mean_b: 0,
            gain: 0,
            gain_step: 0,
            trim_a: self.trim_a,
            trim_b: self.trim_b,
            trim_cutoff: self.trim_cutoff,
        };
        *self = fresh;
        self.blocker.reset();
    }

    /// The amplifier envelope's position, as a page's rules read it
    /// (page-rules.md, RULE-31).
    pub fn meter(&self) -> (crate::midi::MeterStage, u8) {
        self.amplifier_envelope.meter()
    }

    /// Whether the voice makes no sound: its key up and its amplifier
    /// closed.
    pub fn silent(&self) -> bool {
        !self.gate && self.amplifier_envelope.stage() == Stage::Rest && self.gain < REST_LEVEL
    }

    /// Whether nothing of the voice moves that a later note would hear:
    /// it is silent and its filter envelope has fallen to rest too. Only
    /// then is the voice left unrendered. The filter envelope's capacitor
    /// discharges whatever the amplifier passes, and the filter follows
    /// it: a voice stopped while the cutoff was still up would give its
    /// next key the envelope, and the filter, of the moment it stopped.
    pub fn at_rest(&self) -> bool {
        self.silent() && self.filter_envelope.stage() == Stage::Rest
    }

    /// A tick: the envelopes move on, and the pitches, the pulse widths,
    /// the cutoff and the amplifier's slope are set.
    pub fn tick(&mut self, settings: &Settings, controls: Controls) {
        let law = self.law;
        let release = settings.release || controls.pedal;
        settings.filter_envelope.apply(&mut self.filter_envelope, release);
        settings.amplifier_envelope.apply(&mut self.amplifier_envelope, release);
        self.filter_envelope.gate(self.gate);
        self.amplifier_envelope.gate(self.gate);
        let envelope = self.filter_envelope.tick();
        let level = self.amplifier_envelope.tick();
        self.gain_step = level.saturating_sub(self.gain) >> TICK_BITS;

        // The key's voltage: the voice's own, or in unison the one all
        // share.
        let key = controls.unison.unwrap_or_else(|| law::semitones(self.key));
        let wheel = controls.wheel;
        let through = |on: bool, amount: i32| if on { amount } else { 0 };
        let poly_envelope = law::gain(settings.poly_envelope, envelope);

        let pitch_a = settings
            .pitch_a
            .saturating_add(key)
            .saturating_add(controls.pitch)
            .saturating_add(self.trim_a)
            .saturating_add(through(settings.wheel_pitch_a, scale(wheel, WHEEL_PITCH)))
            .saturating_add(through(settings.poly_to_pitch, amplify(poly_envelope, POLY_MOD_PITCH)));
        self.step_a = law::raise(STEP_OF_C0, pitch_a.clamp(PITCH_BOUNDS.0, PITCH_BOUNDS.1));
        let pitch_b = settings
            .pitch_b
            .saturating_add(through(settings.keyboard_b, key))
            .saturating_add(controls.pitch)
            .saturating_add(self.trim_b)
            .saturating_add(through(settings.wheel_pitch_b, scale(wheel, WHEEL_PITCH)));
        self.oscillator_b.set_step(law::raise(STEP_OF_C0, pitch_b.clamp(PITCH_BOUNDS.0, PITCH_BOUNDS.1)));

        let (wheel_width_a, wheel_width_b, poly_width) = law.pulse_width_modulation;
        let moved =
            |amount: i32, by: i32| i32::try_from((i64::from(amount).saturating_mul(i64::from(by))) >> law::OCTAVE_BITS);
        self.width_a = settings
            .width_a
            .saturating_add(through(settings.wheel_width_a, moved(wheel, wheel_width_a).unwrap_or(0)))
            .saturating_add(through(settings.poly_to_width, moved(poly_envelope, poly_width).unwrap_or(0)));
        let width_b =
            settings.width_b.saturating_add(through(settings.wheel_width_b, moved(wheel, wheel_width_b).unwrap_or(0)));
        self.width_b = width(width_b);
        self.oscillator_b.set_width(self.width_b);
        self.carried = None;
        self.mean_a = mean(self.width_a);
        self.mean_b = mean(width_b);

        // The filter's key voltage counts from the lowest key's number.
        let key_steps = law::semitones(law.cutoff.1);
        let tracked = key.saturating_add(key_steps);
        let tracked = if settings.half_tracking { tracked >> 1 } else { tracked };
        let cutoff = settings
            .cutoff
            .saturating_add(self.trim_cutoff)
            .saturating_add(through(settings.keyboard_filter, tracked))
            .saturating_add(law::gain(settings.envelope_cutoff, envelope))
            .saturating_add(through(settings.wheel_cutoff, amplify(wheel, WHEEL_CUTOFF)))
            .saturating_add(through(settings.poly_to_cutoff, amplify(poly_envelope, POLY_MOD_CUTOFF)));
        self.cutoff_step = law::raise(STEP_OF_A, cutoff.clamp(CUTOFF_BOUNDS.0, CUTOFF_BOUNDS.1));
        // What oscillator B moves through Poly-Mod is set at every
        // sample instead.
        if settings.poly_pitch == 0 {
            self.oscillator_a.set_step(self.step_a);
        }
        if settings.poly_width == 0 {
            self.oscillator_a.set_width(width(self.width_a));
        }
        if settings.poly_cutoff == 0 {
            self.filter.set(self.cutoff_step, settings.resonance);
        }
    }

    /// What Poly-Mod has of oscillator B at a phase of B's cycle: the
    /// sawtooth and the pulse from zero up, the triangle either side.
    /// It is taken from the phase and not from B's band-limited output,
    /// whose ripple either side of each edge is no part of the voltage
    /// that the instrument's Poly-Mod carries.
    #[inline]
    fn source(&self, settings: &Settings, phase: u32) -> i32 {
        let [saw, pulse, triangle] = settings.source_b;
        let folded = if phase < HALF_CYCLE { phase } else { phase.wrapping_neg() };
        apply((phase >> SAWTOOTH_SHIFT).cast_signed(), saw)
            .wrapping_add(apply(if phase < self.width_b { 2 * LEVEL } else { 0 }, pulse))
            .wrapping_add(apply((folded >> (SAWTOOTH_SHIFT - 1)).cast_signed().wrapping_sub(LEVEL), triangle))
    }

    /// What oscillator B runs from a phase of its cycle to where what
    /// Poly-Mod has of it next jumps or turns: the start of its cycle,
    /// where its pulse falls, and where its triangle turns.
    #[inline]
    fn to_the_mark(&self, settings: &Settings, phase: u32) -> Option<u32> {
        let [_, pulse, triangle] = settings.source_b;
        let mut nearest = phase.wrapping_neg();
        if pulse != 0 && phase < self.width_b {
            nearest = self.width_b.wrapping_sub(phase);
        } else if triangle != 0 && phase < HALF_CYCLE {
            nearest = HALF_CYCLE.wrapping_sub(phase);
        }
        if triangle != 0 && phase < HALF_CYCLE {
            nearest = nearest.min(HALF_CYCLE.wrapping_sub(phase));
        }
        // At the very start of its cycle the next start is a cycle away.
        if nearest == 0 { None } else { Some(nearest) }
    }

    /// One sample of oscillator A under oscillator B's Poly-Mod, B having
    /// run from the phase `before` in this sample; and the cutoff's step
    /// for the sample.
    ///
    /// The sample is made in stretches ([`Oscillator::run`]), each to
    /// where what Poly-Mod has of B next jumps or turns. Oscillator A's
    /// frequency at each end of a stretch is its own under Poly-Mod's
    /// voltage there, and its width slides to what that voltage makes
    /// it; where the voltage jumps, the width jumps
    /// ([`Oscillator::jump_width`]), and where B's cycle starts, A's
    /// does if it is synchronised. A stretch in which the width would
    /// meet an end of its range ends there, so that the width stays at
    /// the end from that instant and not from the stretch's end. The cutoff, which the filter takes
    /// once a sample, is its mean over the sample: each stretch's for
    /// that stretch's share.
    #[inline]
    fn poly_modulated(&mut self, settings: &Settings, before: u32) -> (Waves, u32) {
        let step = self.oscillator_b.step();
        let mut phase = before;
        let (mut from, mut pitch_from, mut cutoff_from) = self.carried.unwrap_or_else(|| self.under(settings, before));
        let mut cutoff = 0u32;
        let stretches = settings.poly_pitch != 0 || settings.poly_width != 0;
        let mut now = u32::MAX;
        // What B has still to run in the sample, to the unit.
        let mut left = step;
        let mut ends = WIDTH_ENDS;
        for _ in 0..STRETCHES {
            let mut mark = self.to_the_mark(settings, phase).filter(|&distance| distance <= left);
            let mut run = mark.unwrap_or(left);
            let mut end = phase.wrapping_add(run);
            // Before a jump the voltage is as it was a unit of phase
            // short of it.
            let (mut to, mut pitch_to, mut cutoff_to) =
                self.under(settings, if mark.is_some() { end.wrapping_sub(1) } else { end });
            let mut jumps = mark.is_some();
            // The width is held to its range: where it meets an end the
            // stretch ends, with no jump.
            if settings.poly_width != 0
                && ends > 0
                && let Some(share) = self.meets_an_end(settings, from, to)
            {
                let shorter = mul_high_unsigned(run, share);
                if shorter > 0 && shorter < run {
                    ends = ends.saturating_sub(1);
                    run = shorter;
                    mark = Some(run);
                    jumps = false;
                    end = phase.wrapping_add(run);
                    (to, pitch_to, cutoff_to) = self.under(settings, end);
                }
            }
            left = left.saturating_sub(run);
            let until = if left == 0 { 0 } else { ratio(left, step) };
            if stretches {
                if settings.poly_width != 0 {
                    self.oscillator_a.set_width(self.width_under(settings, to));
                }
                self.oscillator_a.run(until, pitch_from, pitch_to);
            }
            // The whole sample is one stretch more often than not.
            let middle = (cutoff_from >> 1).wrapping_add(cutoff_to >> 1);
            let span = now.saturating_sub(until);
            cutoff = if span == u32::MAX { middle } else { cutoff.saturating_add(mul_high_unsigned(middle, span)) };
            phase = end;
            (from, pitch_from, cutoff_from) = (to, pitch_to, cutoff_to);
            if jumps {
                if stretches && settings.sync && phase == 0 {
                    self.oscillator_a.synchronise();
                }
                (from, pitch_from, cutoff_from) = self.under(settings, phase);
                if stretches && settings.poly_width != 0 && from != to {
                    self.oscillator_a.jump_width(self.width_under(settings, from));
                }
            }
            now = until;
            if left == 0 {
                break;
            }
        }
        self.carried = Some((from, pitch_from, cutoff_from));
        let waves = if stretches {
            self.oscillator_a.finish()
        } else {
            self.oscillator_a.tick(if settings.sync { self.oscillator_b.started() } else { None })
        };
        (waves, cutoff)
    }

    /// Where in a stretch oscillator A's width, on its way from what
    /// one voltage of Poly-Mod makes it to what another does, first
    /// meets an end of its range, as the share of the stretch run by
    /// then in Q32; nothing if it meets neither.
    #[inline]
    fn meets_an_end(&self, settings: &Settings, from: i32, to: i32) -> Option<u32> {
        let first = self.width_a.saturating_add(amplify(from, settings.poly_width));
        let last = self.width_a.saturating_add(amplify(to, settings.poly_width));
        let whole = first.abs_diff(last);
        let mut nearest = None;
        for bound in [0, WIDTH_FULL] {
            if (first < bound && bound < last) || (first > bound && bound > last) {
                let share = ratio(first.abs_diff(bound), whole);
                nearest = Some(nearest.map_or(share, |other: u32| other.min(share)));
            }
        }
        nearest
    }

    /// What Poly-Mod has of oscillator B at a phase, and oscillator A's
    /// step and the cutoff's step under it.
    #[inline]
    fn under(&self, settings: &Settings, phase: u32) -> (i32, u32, u32) {
        let source = self.source(settings, phase);
        let pitch = if settings.poly_pitch == 0 {
            self.step_a
        } else {
            law::bend(self.step_a, amplify(source, settings.poly_pitch))
        };
        let cutoff = if settings.poly_cutoff == 0 {
            self.cutoff_step
        } else {
            law::bend(self.cutoff_step, amplify(source, settings.poly_cutoff))
        };
        (source, pitch, cutoff)
    }

    /// Oscillator A's width under what Poly-Mod has of oscillator B.
    #[inline]
    fn width_under(&self, settings: &Settings, source: i32) -> u32 {
        width(self.width_a.saturating_add(amplify(source, settings.poly_width)))
    }

    /// One sample. `noise` is the instrument's one noise generator.
    #[inline]
    pub fn sample(&mut self, settings: &Settings, noise: i32) -> i32 {
        let before = self.oscillator_b.phase();
        let b = self.oscillator_b.tick(None);
        let a = if settings.poly_at_audio_rate() {
            let (a, cutoff) = self.poly_modulated(settings, before);
            if settings.poly_cutoff != 0 {
                self.filter.set(cutoff, settings.resonance);
            }
            a
        } else {
            self.oscillator_a.tick(if settings.sync { self.oscillator_b.started() } else { None })
        };
        let [a_saw, a_pulse] = settings.mix_a;
        let [b_saw, b_pulse, b_triangle] = settings.mix_b;
        let mixed = apply(a.sawtooth, a_saw)
            .wrapping_add(apply(a.pulse.wrapping_sub(self.mean_a), a_pulse))
            .wrapping_add(apply(b.sawtooth, b_saw))
            .wrapping_add(apply(b.pulse.wrapping_sub(self.mean_b), b_pulse))
            .wrapping_add(apply(b.triangle, b_triangle))
            .wrapping_add(apply(noise, settings.noise))
            .wrapping_add(noise >> HISS_SHIFT)
            >> DRIVE_SHIFT;
        let filtered = if self.law.blocking.0 {
            self.filter.tick(self.blocker.high_pass(mixed)).four_pole
        } else {
            self.blocker.high_pass(self.filter.tick(mixed).four_pole)
        };
        self.gain = self.gain.wrapping_add(self.gain_step);
        apply(filtered, self.gain)
    }
}

/// Half a cycle of phase, where the triangle turns down.
const HALF_CYCLE: u32 = 1 << 31;
/// The shift from a phase to a sawtooth's rise from nothing to twice
/// [`LEVEL`].
const SAWTOOTH_SHIFT: u32 = 32 - LEVEL.trailing_zeros() - 1;
/// Stretches a sample of oscillator A is made in under Poly-Mod, at
/// most: oscillator B's pulse falls, its triangle turns, its cycle
/// starts, and the sample ends; one to spare; and one for each of
/// [`WIDTH_ENDS`].
const STRETCHES: u32 = 5 + WIDTH_ENDS;
/// Times in a sample that a stretch is ended where oscillator A's pulse
/// width meets an end of its range, at most: once for each end.
const WIDTH_ENDS: u32 = 2;

/// An oscillator's width for a share of a cycle in Q24, held between
/// none and the whole.
fn width(share: i32) -> u32 {
    share.clamp(0, WIDTH_FULL).cast_unsigned() << WIDTH_SHIFT
}

/// The mean of a pulse between -[`LEVEL`] and [`LEVEL`] that is high
/// for a share of a cycle in Q24. A voice that is at rest is not
/// rendered, so what blocks direct current in the instrument, where
/// every oscillator runs for ever, has not settled when a note starts:
/// the pulse's mean, which is all the direct current a voice has, is
/// taken off here instead, and the blocking is left the rest.
fn mean(share: i32) -> i32 {
    (share.clamp(0, UNIT) << 1).wrapping_sub(LEVEL)
}

/// The interval between two keys a semitone apart, which the key numbers
/// are counted in.
const _: () = assert!(SEMITONES == 12 && ONE_Q28 == 1 << 28 && UNIT == LEVEL);

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{Controls, RATE, STEP_OF_C0, Settings, UNIT, Voice, WIDTH_SHIFT};
    use crate::synth53::Model;
    use crate::synth53::patch::{Knob, Patch, Switch};

    #[test]
    fn oscillator_bs_triangle_is_at_its_foot_where_the_cycle_starts_on_revision_3_and_at_its_top_on_revision_2() {
        // CEM3340 data sheet, figure 1, for revision 3; TM2 section 2-23
        // and sheet SD411 for revision 2, whose converter gives 5 V where
        // the sawtooth is at 0 V and at 10 V and -5 V at its middle.
        let patch = Patch::DEFAULT
            .with_switch(Switch::OscBSawtooth, false)
            .with_switch(Switch::OscBPulse, false)
            .with_switch(Switch::OscBTriangle, true)
            .with_switch(Switch::PolyModFrequencyA, true)
            .with(Knob::PolyModOscB, 120)
            .with(Knob::MixerOscB, 120);
        for (model, from_top) in [(Model::Synth53, false), (Model::Synth52, true)] {
            let settings = Settings::new(&patch, model);
            let voice = Voice::new(model, [0; 3]);
            let (start, middle) = (voice.source(&settings, 0), voice.source(&settings, super::HALF_CYCLE));
            assert_ne!(start, 0, "{model:?}");
            assert!((start + middle).abs() <= 8, "{model:?}: as far above as below, {start} and {middle}");
            assert_eq!(start > 0, from_top, "{model:?}: Poly-Mod has {start} at the start and {middle} at the middle");
            // The mixer has the same triangle, turned over once more on
            // revision 3, where it enters the other input.
            let [_, _, mixed] = settings.mix_b;
            assert!(mixed < 0, "{model:?}: {mixed}");
        }
    }

    #[test]
    fn a_cycle_of_oscillator_b_that_starts_as_a_sample_ends_synchronises_oscillator_a() {
        // Oscillator B's pulse falls a quarter of the way through a
        // sample and its cycle starts exactly where the sample ends.
        // What was left of the sample after the fall was worked out
        // again from a rounded share of it, came out a unit short of
        // the start, and the start was then seen by no sample: oscillator
        // A ran on unsynchronised for a cycle of B.
        let (step, fall) = (0x0400_0000u32, 0x0100_0000u32);
        let patch = Patch::DEFAULT
            .with_switch(Switch::Sync, true)
            .with_switch(Switch::OscAPulse, true)
            .with_switch(Switch::OscBPulse, true)
            .with_switch(Switch::PolyModPulseWidthA, true)
            .with(Knob::PolyModOscB, 120);
        for model in [Model::Synth53, Model::Synth52] {
            let settings = Settings::new(&patch, model);
            let mut voice = Voice::new(model, [0; 3]);
            voice.configure(&settings);
            voice.start(24);
            voice.tick(&settings, Controls { wheel: 0, pitch: 0, unison: None, pedal: false });
            for _ in 0..7 {
                let _ = voice.sample(&settings, 0);
            }
            assert_ne!(voice.oscillator_a.phase(), 0, "{model:?}: oscillator A is under way");
            // Oscillator B a sample short of its cycle's start.
            voice.oscillator_b.set_step(step);
            voice.oscillator_b.restart();
            voice.oscillator_b.rewind(step);
            voice.width_b = step.wrapping_neg() + fall;
            voice.oscillator_b.set_width(voice.width_b);
            voice.carried = None;
            let _ = voice.sample(&settings, 0);
            assert_eq!(voice.oscillator_b.phase(), 0, "{model:?}: oscillator B's cycle starts with the sample");
            assert_eq!(
                (voice.oscillator_a.phase(), voice.oscillator_a.started()),
                (0, Some(0)),
                "{model:?}: oscillator A starts its cycle there too"
            );
        }
    }

    #[test]
    fn wheel_mod_narrows_revision_2s_two_pulses_through_their_own_resistors() {
        // TM2 SD411: 13.3 kΩ against 100 kΩ for oscillator A (R492,
        // R493); SD312: 15 kΩ against 100 kΩ for oscillator B (R392,
        // R395). A tenth of a volt of Wheel-Mod narrows A's pulse by
        // 7.52% of a cycle and B's by 6.67%.
        let patch = Patch::DEFAULT
            .with_switch(Switch::WheelPulseWidthA, true)
            .with_switch(Switch::WheelPulseWidthB, true)
            .with(Knob::OscAPulseWidth, 60)
            .with(Knob::OscBPulseWidth, 60);
        let settings = Settings::new(&patch, Model::Synth52);
        let mut voice = Voice::new(Model::Synth52, [0; 3]);
        voice.start(24);
        voice.tick(&settings, Controls { wheel: UNIT / 10, pitch: 0, unison: None, pedal: false });
        let share = |width: f64| width / f64::from(UNIT);
        let (a, b) = (share(f64::from(voice.width_a)), share(f64::from(voice.width_b >> WIDTH_SHIFT)));
        assert!((a - (0.5 - 0.01 / 0.133)).abs() < 0.001 && (b - (0.5 - 0.01 / 0.15)).abs() < 0.001, "{a} and {b}");
    }

    #[test]
    fn the_step_of_c0_is_the_number_its_source_gives() {
        // 440 Hz, 57 semitones down, as a share of the voices' rate in
        // 32 bits; a unit of the step is a four hundredth of a cent.
        let exact = 440.0 * (-57.0f64 / 12.0).exp2() * 4_294_967_296.0 / f64::from(RATE);
        assert!((f64::from(STEP_OF_C0) - exact).abs() <= 0.5, "{STEP_OF_C0} for {exact}");
    }
}
