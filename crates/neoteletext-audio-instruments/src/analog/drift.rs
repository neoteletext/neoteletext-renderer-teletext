//! What makes two voices of one instrument differ: the tolerances of
//! their parts, fixed for each voice, and a slow drift.
//!
//! Both come from a generator seeded for the voice, so an instrument is
//! the same every time it is made from the same seed. Amounts are in the
//! caller's own units (cents, a share of a time), and an amount of zero
//! gives exactly zero: an instrument with no spread is a perfect one.
//!
//! Source: own design. The instruments' documents give tolerances and
//! trimming procedures, not distributions: each offset here is uniform
//! within its amount.

use super::fixed::{mul_high, scale};
use super::oscillator::Noise;

/// What a voice's number is multiplied by before it is mixed into a seed,
/// so that neighbouring voices get unrelated sequences.
///
/// Source: own choice: the golden ratio's fraction in 32 bits, odd, so
/// that no two voices share a seed.
const VOICE_STRIDE: u32 = 0x9E37_79B9;

/// The generator of one voice's tolerances and drift.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Variation {
    noise: Noise,
}

impl Variation {
    /// The generator of voice `voice` of an instrument made from `seed`.
    #[must_use]
    pub const fn new(seed: u32, voice: u32) -> Self {
        let mut noise = Noise::new(seed ^ voice.wrapping_add(1).wrapping_mul(VOICE_STRIDE));
        // The first words of a generator whose seed has few bits set are
        // alike; they are left behind.
        let _ = noise.word();
        let _ = noise.word();
        Self { noise }
    }

    /// The next offset: uniform between -`amount` and `amount`, and zero
    /// for an amount of zero. An instrument draws its voice's offsets in
    /// a fixed order when the voice is made: tuning, cutoff, the times
    /// of its envelopes.
    pub const fn offset(&mut self, amount: i32) -> i32 {
        let word = self.noise.word().cast_signed();
        if amount == 0 { 0 } else { scale(amount, word) }
    }
}

/// A slow, bounded wandering: every so many samples a new point is drawn
/// within the amount, and the value glides towards it, a fixed share of
/// the way in each sample. It never leaves the amount, and is zero for
/// an amount of zero.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Drift {
    variation: Variation,
    amount: i32,
    /// Samples between two points, and those left before the next.
    interval: u32,
    wait: u32,
    /// The right shift that makes the share of the way glided in a
    /// sample.
    glide: u32,
    target: i32,
    value: i32,
}

impl Drift {
    /// The glide at which the value covers nearly all the way to a point
    /// before the next is drawn: a share of 2⁻ⁿ each sample closes the
    /// distance to e⁻⁴ of itself in 4 * 2ⁿ samples.
    const SETTLING: u32 = 4;
    /// The slowest glide, which keeps a distance's share above nothing
    /// for any amount worth the name.
    const GLIDE_LIMIT: u32 = 24;

    /// A drift for voice `voice` of an instrument made from `seed`, of
    /// nothing, with a new point every `interval` samples; zero is taken
    /// as one.
    #[must_use]
    pub const fn new(seed: u32, voice: u32, interval: u32) -> Self {
        let interval = if interval == 0 { 1 } else { interval };
        // The largest power of two of samples that settles in an
        // interval.
        let glide = (interval / Self::SETTLING).checked_ilog2();
        let glide = match glide {
            Some(glide) if glide < Self::GLIDE_LIMIT => glide,
            Some(_) => Self::GLIDE_LIMIT,
            None => 0,
        };
        Self { variation: Variation::new(seed, voice), amount: 0, interval, wait: 0, glide, target: 0, value: 0 }
    }

    /// Sets how far the value may wander either way. Zero stops it at
    /// zero at once.
    pub const fn set_amount(&mut self, amount: i32) {
        self.amount = amount.saturating_abs();
        if self.amount == 0 {
            self.target = 0;
            self.value = 0;
        } else {
            // What is beyond a smaller amount comes inside it.
            self.target = clamp(self.target, self.amount);
            self.value = clamp(self.value, self.amount);
        }
    }

    /// One sample: the value, between -amount and amount.
    #[inline]
    pub const fn tick(&mut self) -> i32 {
        if self.wait == 0 {
            self.wait = self.interval;
            self.target = self.variation.offset(self.amount);
        }
        self.wait = self.wait.wrapping_sub(1);
        // A share of the distance, and a unit at least while there is
        // any: the value then reaches every point in the end.
        let distance = self.target.saturating_sub(self.value);
        let step = distance >> self.glide;
        let step = if step == 0 { distance.signum() } else { step };
        self.value = self.value.wrapping_add(step);
        self.value
    }
}

/// A value held within an amount either way.
const fn clamp(value: i32, amount: i32) -> i32 {
    if value > amount {
        amount
    } else if value < amount.wrapping_neg() {
        amount.wrapping_neg()
    } else {
        value
    }
}

/// Fraction bits of a detuning in cents.
pub const CENTS_BITS: u32 = 16;
/// The largest detuning [`detune`] applies, in cents with [`CENTS_BITS`]
/// of fraction: a semitone.
pub const CENTS_LIMIT: i32 = 100 << CENTS_BITS;

/// The natural logarithm of two over 1,200, the logarithm of a cent's
/// ratio, in Q40: times a detuning in cents with 16 bits of fraction it
/// is the logarithm of the ratio in Q24 after the product's upper word.
///
/// Source: computed here from ln(2) = 0.693147180559945309, written as a
/// whole number of 10⁻¹⁸.
const LOG_CENT_Q40: i32 = ((693_147_180_559_945_309_i128 << 40) / (1_200 * 1_000_000_000_000_000_000)) as i32;
/// The left shift from that logarithm in Q24 to Q31.
const LOG_SHIFT: u32 = 7;
/// A third in Q31, for the series' third term.
const THIRD_Q31: i32 = i32::MAX / 3;

/// A frequency step detuned by so many cents, with [`CENTS_BITS`] of
/// fraction, held to [`CENTS_LIMIT`] either way: the step times
/// 2^(cents / 1200), by the exponential's series to its third power,
/// within a thousandth of a cent. A step above half the rate is beyond
/// it.
#[inline]
#[must_use]
pub const fn detune(step: u32, cents: i32) -> u32 {
    let cents = clamp(cents, CENTS_LIMIT);
    // x = cents * ln(2) / 1200, below 0.058 in magnitude, in Q31.
    let x = mul_high(cents, LOG_CENT_Q40) << LOG_SHIFT;
    // e^x - 1 = x (1 + x/2 (1 + x/3)).
    let inner = scale(x, THIRD_Q31);
    let middle = (x >> 1).wrapping_add(scale(x >> 1, inner));
    let change = x.wrapping_add(scale(x, middle));
    step.wrapping_add_signed(scale(step.cast_signed(), change))
}
