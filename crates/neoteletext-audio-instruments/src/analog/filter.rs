//! The filters: a one-pole low-pass and high-pass, the four-pole
//! transistor ladder, the four-pole cascade of transconductance
//! amplifiers, and the two-pole state-variable filter.
//!
//! # The method
//!
//! Each is its analogue block diagram with every integrator replaced by a
//! trapezoidal one, and the loops that then have no delay in them solved
//! in each sample, as Zavalishin's "The Art of VA Filter Design" (rev.
//! 2.1.2) sets out: the integrator and its instantaneous response `g x +
//! s` (section 3.9, equations 3.26 to 3.29), the cutoff prewarped so that
//! it falls where it is asked for (section 3.8, equation 3.13), the
//! state-variable filter (section 4.4, equation 4.14) and the ladder
//! (section 5.3, equation 5.8). For a filter that does not distort, the
//! solution is exact, and the response is the analogue filter's with its
//! frequency axis bent by the tangent, nothing else.
//!
//! # The distortion of the four-pole filters
//!
//! A stage of the transistor ladder follows `dy/dt = w (tanh(x) -
//! tanh(y))` (Stinchcombe, "Analysis of the Moog Transistor Ladder and
//! Derivative Filters", section 2.1.1, equation 5: the current of a
//! differential pair is the hyperbolic tangent of the voltage across it),
//! and a stage of the amplifier cascade `dy/dt = w tanh(x - y)`, its
//! amplifier having both the input and the output across its inputs. The
//! trapezoidal equations are then implicit in a curve, and are solved by
//! a documented approximation with a fixed amount of work:
//!
//! - each `tanh(v)` is written `v - e(v)`, the straight line less an
//!   excess that is zero for small signals;
//! - with every excess taken as known, the filter is linear, and its
//!   loop is solved exactly, as for the filter that does not distort;
//! - the excesses are read again from the signals that solution gives,
//!   and the solution is made again: [`PASSES`] times in all, the first
//!   from the excesses of the sample before.
//!
//! For one stage by itself a pass shrinks the error of its excess by G
//! tanh², with G = g / (1 + g) below one whatever the cutoff. That the
//! passes converge around the whole loop is not proved here but measured:
//! `tests/analog_filter.rs` compares the result with the equations solved
//! to convergence in floating point, for the error that is left
//! ([`PASSES`] gives the figures) and for the pitch of the oscillation,
//! which agrees to a third of a cent. At small signals the excesses
//! vanish and the filter is the linear one exactly.
//!
//! # Oscillation
//!
//! From a resonance of four the loop oscillates. Just above four the
//! oscillation is small and its pitch is the cutoff's: 2.1 cents flat at
//! 4.02. Driven further into the curve every stage's gain falls, and the
//! pitch with it: 19 cents flat at 4.2, 70 cents at 5 and 100 cents (the
//! cascade 117) at 6. That is what the equations do, solved here or
//! solved out; an instrument whose filter is played as an oscillator
//! sets its resonance and its tuning with that in hand.
//!
//! # Bounds
//!
//! Every parameter value is valid. A cutoff is a
//! [`frequency`](super::frequency) step and is held to [`CUTOFF_LIMIT`]
//! (the state-variable filter's to [`STEP_LIMIT`]); resonance is held to
//! [`RESONANCE_LIMIT`]; every state is held to [`LIMIT`](super::LIMIT).
//! Setting a cutoff or a resonance costs a few products and no division,
//! and may be done for every sample.
//!
//! Each integrator carries the fraction of a unit its products leave, so
//! what it adds up is exact however low the cutoff
//! ([`integrate`](super::fixed::integrate)).

use super::fixed::{amplify, integrate, limit, mul_high, mul_high_unsigned, ratio, scale};
use super::tables::{EXCESS, EXCESS_RANGE_SHIFT, EXCESS_STEPS_SHIFT, WARP, WARP_STEPS_SHIFT};
use super::{LEVEL, STEP_LIMIT};

/// One in Q31, less a unit: the largest coefficient of [`scale`].
const ONE_Q31: i32 = i32::MAX;
/// One in Q28, the fixed point of a resonance and of a damping.
pub const ONE_Q28: i32 = 1 << 28;

/// The highest cutoff of the one-pole and four-pole filters, as a
/// frequency step: 0.45 of the sampling rate.
///
/// Source: own choice. The trapezoidal integrator's gain grows without
/// bound as the cutoff nears half the sampling rate (Zavalishin, equation
/// 3.13); 0.45 lets a filter at the output rate open to 21.6 kHz.
pub const CUTOFF_LIMIT: u32 = 1_932_735_283;
const _: () = assert!(CUTOFF_LIMIT as u64 == ((45u64 << 32) / 100));

/// The highest resonance of the four-pole filters, in Q28. A linear
/// four-pole loop oscillates at four (Zavalishin, section 5.1; Stilson
/// and Smith, "Analyzing the Moog VCF with considerations for digital
/// implementation", ICMC 1996, section 2), and what is above four sets
/// how hard the oscillation is driven into the curve.
///
/// Source: own choice of the bound, half as much again as oscillation
/// needs.
pub const RESONANCE_LIMIT: i32 = 6 * ONE_Q28;

/// The level at which the four-pole filters' stages saturate: their
/// `tanh` is `SATURATION * tanh(v / SATURATION)` (Zavalishin, equation
/// 6.7).
///
/// Source: own choice: twice an oscillator's peak, so that one oscillator
/// at full level is bent by 8% at its peaks and three together are
/// clearly driven. An instrument that wants more or less drive scales
/// what it feeds the filter.
pub const SATURATION: i32 = 2 * LEVEL;

/// Times the loop of a four-pole filter is solved in each sample.
///
/// Source: own choice, from the error measured in
/// `tests/analog_filter.rs`: against the equations solved out, on a naive
/// sawtooth 12 dB above an oscillator's level at twice the output rate,
/// two passes leave an error 20 dB below the output at worst, three 34 dB
/// and four 44 dB, the worst being the cascade at a cutoff of 16 kHz; at
/// a cutoff of 2 kHz three leave 64 dB, and at an oscillator's own level
/// 84 dB.
pub const PASSES: usize = 3;

/// The left shift from a signal's magnitude to t, its share of
/// [`SATURATION`], in Q28.
const T_SHIFT: u32 = 28 - SATURATION.trailing_zeros();
/// The magnitude at which [`EXCESS`] ends.
const EXCESS_END: u32 = SATURATION.cast_unsigned() << EXCESS_RANGE_SHIFT;
/// Bits of t in Q28 below an entry of [`EXCESS`].
const EXCESS_INDEX_SHIFT: u32 = 28 - EXCESS_STEPS_SHIFT;
/// The mask of what lies between two entries.
const EXCESS_FRACTION_MASK: u32 = (1 << EXCESS_INDEX_SHIFT) - 1;
/// The left shift that brings that fraction to the top of a word.
const EXCESS_FRACTION_SHIFT: u32 = 32 - EXCESS_INDEX_SHIFT;
/// The left shift of t, below eight in Q28, before it is squared: the
/// upper word of the square is then t² in Q26, and fits.
const SQUARE_SHIFT: u32 = 1;
/// The left shift of t² (t - tanh(t)) / t³ in Q26 to Q32.
const SHARE_SHIFT: u32 = 6;
/// The left shift of a magnitude below [`EXCESS_END`] to the top of a
/// word.
const MAGNITUDE_SHIFT: u32 = 31 - SATURATION.trailing_zeros() - EXCESS_RANGE_SHIFT + 1;
const _: () = assert!(SATURATION.count_ones() == 1 && EXCESS_END == 1 << (32 - MAGNITUDE_SHIFT));

/// What the curve takes off a signal: `value - SATURATION * tanh(value /
/// SATURATION)`, within 0.0001 of itself at every level
/// (`tests/analog_filter.rs`). For a signal x = t `SATURATION` it is x
/// times 1 - tanh(t) / t, which is t² times the table's entry.
#[inline]
#[must_use]
pub fn excess(value: i32) -> i32 {
    let magnitude = value.unsigned_abs();
    let taken = if magnitude >= EXCESS_END {
        // Beyond the table the tangent is one.
        magnitude.wrapping_sub(SATURATION.cast_unsigned())
    } else {
        let t = magnitude << T_SHIFT;
        let index = (t >> EXCESS_INDEX_SHIFT) as usize;
        let (Some(&before), Some(&after)) = (EXCESS.get(index), EXCESS.get(index.wrapping_add(1))) else {
            return 0;
        };
        // The table falls all the way.
        let fall = mul_high_unsigned(before.wrapping_sub(after), (t & EXCESS_FRACTION_MASK) << EXCESS_FRACTION_SHIFT);
        let square = mul_high_unsigned(t << SQUARE_SHIFT, t << SQUARE_SHIFT);
        let share = mul_high_unsigned(square, before.wrapping_sub(fall));
        let taken = mul_high_unsigned(magnitude << MAGNITUDE_SHIFT, share << SHARE_SHIFT) >> MAGNITUDE_SHIFT;
        // What is left is never beyond the saturation level, whatever
        // the products' last bits.
        taken.max(magnitude.saturating_sub(SATURATION.cast_unsigned()))
    };
    if value < 0 { taken.cast_signed().wrapping_neg() } else { taken.cast_signed() }
}

/// `SATURATION * tanh(value / SATURATION)`.
#[inline]
#[must_use]
pub fn saturate(value: i32) -> i32 {
    value.wrapping_sub(excess(value))
}

/// Bits of a frequency step below an entry of [`WARP`]: the table covers
/// half the sampling rate, 2³¹ of step.
const WARP_INDEX_SHIFT: u32 = 31 - WARP_STEPS_SHIFT;
/// The mask of what lies between two entries.
const WARP_FRACTION_MASK: u32 = (1 << WARP_INDEX_SHIFT) - 1;
/// The left shift that brings that fraction to Q31.
const WARP_FRACTION_SHIFT: u32 = 31 - WARP_INDEX_SHIFT;
/// The left shift of the difference between two entries, which is below
/// 2²⁵ in magnitude.
const WARP_DIFFERENCE_SHIFT: u32 = 6;
/// The right shift that makes the upper word of the product of the two,
/// so shifted, the difference times the fraction of the way.
const WARP_PART_SHIFT: u32 = WARP_DIFFERENCE_SHIFT - 1;

/// G = g / (1 + g) in Q31 for the prewarped integrator gain g = tan(pi f /
/// rate) of a cutoff given as a frequency step (Zavalishin, equations
/// 3.13 and 3.29), within 0.01% of it. The step is held to `limit`.
#[inline]
fn warp(step: u32, limit: u32) -> i32 {
    let step = step.min(limit);
    let index = (step >> WARP_INDEX_SHIFT) as usize;
    let (Some(&before), Some(&after)) = (WARP.get(index), WARP.get(index.wrapping_add(1))) else {
        return ONE_Q31;
    };
    // The line between two entries; the table falls from pi and turns up
    // again short of its end.
    let difference = after.wrapping_sub(before).cast_signed();
    let fraction = ((step & WARP_FRACTION_MASK) << WARP_FRACTION_SHIFT).cast_signed();
    let part = mul_high(difference << WARP_DIFFERENCE_SHIFT, fraction) >> WARP_PART_SHIFT;
    let entry = before.wrapping_add(part.cast_unsigned());
    // The table is Q30 of pi times the ratio; times the step it is G in
    // Q30.
    (mul_high_unsigned(step, entry) << 1).cast_signed()
}

/// A one-pole filter: a low-pass of 6 dB an octave, and the high-pass
/// that is the input less it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OnePole {
    gain: i32,
    state: i32,
    fraction: u32,
}

impl OnePole {
    /// A filter at rest with its cutoff at zero: it passes nothing low.
    #[must_use]
    pub const fn new() -> Self {
        Self { gain: 0, state: 0, fraction: 0 }
    }

    /// Sets the cutoff, the frequency 3 dB down, as a frequency step held
    /// to [`CUTOFF_LIMIT`].
    pub fn set_cutoff(&mut self, step: u32) {
        self.gain = warp(step, CUTOFF_LIMIT);
    }

    /// One sample: the low-pass output. The high-pass output is the input
    /// less it.
    #[inline]
    pub fn low_pass(&mut self, input: i32) -> i32 {
        // Zavalishin, section 3.10: v = (x - s) G, y = v + s, s = y + v.
        let difference = limit(input).wrapping_sub(self.state);
        let output = self.state.wrapping_add(scale(difference, self.gain));
        self.state = limit(self.state.wrapping_add(integrate(difference, self.gain, &mut self.fraction)));
        output
    }

    /// One sample: the high-pass output.
    #[inline]
    pub fn high_pass(&mut self, input: i32) -> i32 {
        let input = limit(input);
        input.wrapping_sub(self.low_pass(input))
    }

    /// Empties the filter.
    pub const fn reset(&mut self) {
        self.state = 0;
        self.fraction = 0;
    }
}

/// Stages of a four-pole filter.
const STAGES: usize = 4;

/// What the two four-pole filters share: four trapezoidal one-pole stages
/// in a loop with feedback, solved with the stages' distortion taken as
/// known offsets.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Loop {
    /// G = g / (1 + g) in Q31.
    gain: i32,
    /// G to the fourth, in Q31.
    gain_fourth: i32,
    /// The feedback k, in Q28.
    feedback: i32,
    /// 1 / (1 + k G⁴) in Q31.
    solution: i32,
    states: [i32; STAGES],
    /// What each state holds below a unit.
    fractions: [u32; STAGES],
}

impl Loop {
    const fn new() -> Self {
        Self { gain: 0, gain_fourth: 0, feedback: 0, solution: ONE_Q31, states: [0; STAGES], fractions: [0; STAGES] }
    }

    const fn reset(&mut self) {
        self.states = [0; STAGES];
        self.fractions = [0; STAGES];
    }

    fn set(&mut self, step: u32, resonance: i32) {
        self.gain = warp(step, CUTOFF_LIMIT);
        self.feedback = resonance.clamp(0, RESONANCE_LIMIT);
        let square = scale(self.gain, self.gain);
        self.gain_fourth = scale(square, square);
        // 1 + k G⁴ in Q27, which holds the seven it can reach.
        let loop_gain = mul_high(self.feedback, self.gain_fourth);
        let denominator = (ONE_Q28 >> 1).wrapping_add(loop_gain).cast_unsigned();
        self.solution = (ratio((ONE_Q28 >> 1).cast_unsigned(), denominator) >> 1).cast_signed();
    }

    /// Solves the loop for one sample. `offsets[i]` is what distortion
    /// adds to the input of stage `i`, and `feedback_offset` what it adds
    /// to the input of the loop. Gives the loop's input after feedback
    /// and the four stages' outputs, and leaves the states as they were.
    #[inline]
    fn solve(&self, input: i32, offsets: &[i32; STAGES], feedback_offset: i32) -> (i32, [i32; STAGES]) {
        // A stage with input x gives y = s + G (x - s): G x and a part
        // that is known before the loop is solved (Zavalishin, equation
        // 3.29).
        let mut known = [0i32; STAGES];
        let mut sum = 0i32;
        for ((known, &state), &offset) in known.iter_mut().zip(&self.states).zip(offsets) {
            *known = state.wrapping_add(scale(offset.wrapping_sub(state), self.gain));
            // The four in series: G⁴ x + S, S gathered by Horner's rule
            // (Zavalishin, section 5.3).
            sum = limit(scale(sum, self.gain).wrapping_add(*known));
        }
        // Equation 5.8: u = (x - k S) / (1 + k G⁴).
        let fed = limit(input).wrapping_add(limit(feedback_offset)).wrapping_sub(amplify(sum, self.feedback));
        let entering = scale(limit(fed), self.solution);
        let mut outputs = [0i32; STAGES];
        let mut signal = entering;
        for (output, &known) in outputs.iter_mut().zip(&known) {
            signal = limit(scale(signal, self.gain).wrapping_add(known));
            *output = signal;
        }
        (entering, outputs)
    }

    /// Moves each trapezoidal integrator on: s = s + 2 G (x - s) for the
    /// stage's input x, which is the stage before with its offset.
    #[inline]
    fn advance(&mut self, entering: i32, outputs: &[i32; STAGES], offsets: &[i32; STAGES]) {
        let mut before = entering;
        for (((state, fraction), &output), &offset) in
            self.states.iter_mut().zip(&mut self.fractions).zip(outputs).zip(offsets)
        {
            let difference = limit(before.wrapping_add(offset)).wrapping_sub(*state);
            *state = limit(state.wrapping_add(integrate(difference, self.gain, fraction)));
            before = output;
        }
    }
}

/// The transistor ladder: a four-pole low-pass of 24 dB an octave whose
/// every stage saturates, with feedback from the last stage to the first
/// that gives resonance and, from four up, oscillation at the cutoff
/// (the Minimoog's filter).
///
/// Small signals pass as through `1 / (k + (1 + s)⁴)` (Zavalishin,
/// equation 5.1; Stilson and Smith 1996, section 2): the gain below the
/// cutoff falls to `1 / (1 + k)` as the resonance rises.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ladder {
    core: Loop,
    /// The excess of the loop's input and of each stage's output, as last
    /// read.
    excesses: [i32; STAGES + 1],
}

impl Default for Ladder {
    fn default() -> Self {
        Self::new()
    }
}

impl Ladder {
    /// A filter at rest, closed, with no resonance.
    #[must_use]
    pub const fn new() -> Self {
        Self { core: Loop::new(), excesses: [0; STAGES + 1] }
    }

    /// Sets the cutoff as a frequency step, held to [`CUTOFF_LIMIT`], and
    /// the resonance k in Q28, held between zero and [`RESONANCE_LIMIT`].
    pub fn set(&mut self, cutoff: u32, resonance: i32) {
        self.core.set(cutoff, resonance);
    }

    /// One sample.
    #[inline]
    pub fn tick(&mut self, input: i32) -> i32 {
        let mut outputs = [0i32; STAGES];
        let mut offsets = [0i32; STAGES];
        let mut entering = 0;
        for _ in 0..PASSES {
            // Stage i has tanh(y[i-1]) - tanh(y[i]) across it: the
            // straight difference, and the excesses' the other way.
            for (offset, pair) in offsets.iter_mut().zip(self.excesses.windows(2)) {
                if let [before, after] = *pair {
                    *offset = after.wrapping_sub(before);
                }
            }
            (entering, outputs) = self.core.solve(input, &offsets, 0);
            let signals = core::iter::once(entering).chain(outputs);
            for (kept, signal) in self.excesses.iter_mut().zip(signals) {
                *kept = excess(signal);
            }
        }
        self.core.advance(entering, &outputs, &offsets);
        outputs.last().copied().unwrap_or(0)
    }

    /// Empties the filter.
    pub const fn reset(&mut self) {
        self.core.reset();
        self.excesses = [0; STAGES + 1];
    }
}

/// The two outputs of an [`OtaCascade`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Slopes {
    /// After the second stage: 12 dB an octave.
    pub two_pole: i32,
    /// After the fourth stage: 24 dB an octave.
    pub four_pole: i32,
}

/// The cascade of four transconductance amplifier stages with feedback
/// around all four, tapped after the second and the fourth stage: the
/// filter of Roland's IR3109, and of the Curtis CEM3320 and the SSM2040
/// wired the same way.
///
/// Each stage is an amplifier that charges a capacitor with the
/// hyperbolic tangent of the difference between its input and its own
/// buffered output; the feedback passes through one more such amplifier,
/// whose soft clipping "acts to limit the amount of feedback that's
/// possible" (Electric Druid, "Roland filter designs with the IR3109 or
/// AS3109", on the Juno-6's resonance amplifier). The Jupiter-8 takes
/// both the two-pole and the four-pole output (the same article).
///
/// Small signals pass as through the ladder, `1 / (k + (1 + s)⁴)` at the
/// fourth stage and `(1 + s)² / (k + (1 + s)⁴)` at the second.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OtaCascade {
    core: Loop,
    /// The excess of the difference across each stage, and of the
    /// feedback, as last read.
    excesses: [i32; STAGES],
    feedback_excess: i32,
}

impl Default for OtaCascade {
    fn default() -> Self {
        Self::new()
    }
}

impl OtaCascade {
    /// A filter at rest, closed, with no resonance.
    #[must_use]
    pub const fn new() -> Self {
        Self { core: Loop::new(), excesses: [0; STAGES], feedback_excess: 0 }
    }

    /// Sets the cutoff as a frequency step, held to [`CUTOFF_LIMIT`], and
    /// the resonance k in Q28, held between zero and [`RESONANCE_LIMIT`].
    pub fn set(&mut self, cutoff: u32, resonance: i32) {
        self.core.set(cutoff, resonance);
    }

    /// One sample.
    #[inline]
    pub fn tick(&mut self, input: i32) -> Slopes {
        let mut outputs = [0i32; STAGES];
        let mut offsets = [0i32; STAGES];
        let mut entering = 0;
        for _ in 0..PASSES {
            // Stage i has tanh(y[i-1] - y[i]) across it: the straight
            // difference less its excess. The feedback is tanh(k y): k y
            // less its excess, and the loop subtracts it.
            for (offset, &kept) in offsets.iter_mut().zip(&self.excesses) {
                *offset = kept.wrapping_neg();
            }
            (entering, outputs) = self.core.solve(input, &offsets, self.feedback_excess);
            let mut before = entering;
            for (kept, &output) in self.excesses.iter_mut().zip(&outputs) {
                *kept = excess(before.wrapping_sub(output));
                before = output;
            }
            self.feedback_excess = excess(amplify(before, self.core.feedback));
        }
        self.core.advance(entering, &outputs, &offsets);
        let [_, two_pole, _, four_pole] = outputs;
        Slopes { two_pole, four_pole }
    }

    /// Empties the filter.
    pub const fn reset(&mut self) {
        self.core.reset();
        self.excesses = [0; STAGES];
        self.feedback_excess = 0;
    }
}

/// The lowest damping of the state-variable filter, in Q28: a resonance
/// peak of 36 dB.
///
/// Source: own choice. With no damping at all the filter rings for ever;
/// this is far beyond what the instruments' resonance controls reach.
pub const DAMPING_FLOOR: i32 = ONE_Q28 / 64;
/// The highest damping, in Q28: two real poles three octaves apart.
///
/// Source: own choice.
pub const DAMPING_LIMIT: i32 = 4 * ONE_Q28;

/// The three outputs of a [`StateVariable`] filter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Bands {
    /// The low-pass output, 12 dB an octave.
    pub low: i32,
    /// The band-pass output, 6 dB an octave either side.
    pub band: i32,
    /// The high-pass output, 12 dB an octave.
    pub high: i32,
}

/// The two-pole state-variable filter, with its low-pass, band-pass and
/// high-pass outputs at once (the Yamaha CS-80 has one as its high-pass
/// filter and one as its low-pass filter in each voice). It does not
/// distort.
///
/// The low-pass is `1 / (s² + 2 R s + 1)` (Zavalishin, section 4.1), with
/// the damping given as 2R, the reciprocal of Q: the square root of two
/// has no peak, and one has a peak of 0 dB at the cutoff.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StateVariable {
    /// g, the prewarped integrator gain, in Q31: below one.
    gain: i32,
    /// 2R in Q28.
    damping: i32,
    /// 1 / (1 + 2 R g + g²) in Q31.
    solution: i32,
    states: [i32; 2],
    /// What each state holds below a unit.
    fractions: [u32; 2],
}

impl Default for StateVariable {
    fn default() -> Self {
        Self::new()
    }
}

impl StateVariable {
    /// A filter at rest, closed, with a damping of one.
    #[must_use]
    pub const fn new() -> Self {
        Self { gain: 0, damping: ONE_Q28, solution: ONE_Q31, states: [0; 2], fractions: [0; 2] }
    }

    /// Sets the cutoff as a frequency step, held to [`STEP_LIMIT`], a
    /// quarter of the sampling rate, and the damping 2R in Q28, held
    /// between [`DAMPING_FLOOR`] and [`DAMPING_LIMIT`].
    pub fn set(&mut self, cutoff: u32, damping: i32) {
        // g = G / (1 - G), which reaches one at a quarter of the rate.
        let ratio_gain = warp(cutoff, STEP_LIMIT).cast_unsigned();
        let gain = ratio(ratio_gain, (1u32 << 31).wrapping_sub(ratio_gain)) >> 1;
        self.gain = gain.cast_signed();
        self.damping = damping.clamp(DAMPING_FLOOR, DAMPING_LIMIT);
        // 1 + 2 R g + g² in Q27, which holds the six it can reach.
        let denominator = (ONE_Q28 >> 1)
            .wrapping_add(mul_high(self.damping, self.gain))
            .wrapping_add(mul_high(self.gain, self.gain) >> 3);
        self.solution = (ratio((ONE_Q28 >> 1).cast_unsigned(), denominator.cast_unsigned()) >> 1).cast_signed();
    }

    /// One sample.
    #[inline]
    pub fn tick(&mut self, input: i32) -> Bands {
        let [first, second] = self.states;
        // Zavalishin, equation 4.14: the high-pass output solves the
        // loop; the two integrators follow.
        let fed = limit(input)
            .saturating_sub(amplify(first, self.damping))
            .saturating_sub(scale(first, self.gain))
            .saturating_sub(second);
        let high = scale(limit(fed), self.solution);
        let band = limit(scale(high, self.gain).wrapping_add(first));
        let low = limit(scale(band, self.gain).wrapping_add(second));
        let [first_fraction, second_fraction] = &mut self.fractions;
        self.states = [
            limit(first.wrapping_add(integrate(high, self.gain, first_fraction))),
            limit(second.wrapping_add(integrate(band, self.gain, second_fraction))),
        ];
        Bands { low, band, high }
    }

    /// Empties the filter.
    pub const fn reset(&mut self) {
        self.states = [0; 2];
        self.fractions = [0; 2];
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a test, in f64"
)]
mod tests {
    use std::println;

    use super::{CUTOFF_LIMIT, WARP, warp};

    #[test]
    fn the_warped_gain_is_the_tangents_to_a_part_in_ten_thousand() {
        // The difference between two entries must fit the product's shift.
        assert!(WARP.windows(2).all(|pair| pair[0].abs_diff(pair[1]) < 1 << 25));
        let mut worst = 0.0f64;
        // From 0.7 Hz at the output rate: below, the gain has too few
        // bits for the figure.
        let mut step = 1u32 << 16;
        while step < CUTOFF_LIMIT {
            let g = (core::f64::consts::PI * f64::from(step) / 4_294_967_296.0).tan();
            let exact = g / (1.0 + g);
            let found = f64::from(warp(step, CUTOFF_LIMIT)) / 2_147_483_648.0;
            let error = (found / exact - 1.0).abs();
            if error > worst {
                worst = error;
                println!("step {step}: {found} for {exact}: {error:e}");
            }
            step += step / 97 + 1;
        }
        assert!(worst < 1.0e-4, "{worst}");
    }
}
