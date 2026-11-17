//! Intervals in octaves, and the exponential that turns one into a ratio:
//! what an exponential converter makes of a control voltage, for every
//! instrument made from these blocks.
//!
//! An interval is a signed number of octaves with [`OCTAVE_BITS`] of
//! fraction. [`raise`] is the exact exponential, for where a control
//! changes; an instrument that needs the exponential for every sample
//! keeps a cheaper one of its own beside its laws.

/// Fraction bits of an interval in octaves.
pub const OCTAVE_BITS: u32 = 24;
/// One octave.
pub const OCTAVE: i32 = 1 << OCTAVE_BITS;
/// Semitones in an octave.
pub const SEMITONES: i32 = 12;
/// Cents in an octave.
pub const CENTS: i32 = 1_200;

/// `numerator / denominator` octaves, for a fraction below 128 in
/// magnitude; a denominator of zero gives none.
#[must_use]
pub const fn octaves(numerator: i32, denominator: i32) -> i32 {
    match ((numerator as i64) << OCTAVE_BITS).checked_div(denominator as i64) {
        Some(octaves) => octaves as i32,
        None => 0,
    }
}

/// An interval of so many cents, in octaves.
#[must_use]
pub const fn cents(cents: i32) -> i32 {
    octaves(cents, CENTS)
}

/// An interval of so many semitones, in octaves.
#[must_use]
pub const fn semitones(semitones: i32) -> i32 {
    octaves(semitones, SEMITONES)
}

/// The interval from one key of MIDI to another, in octaves.
#[must_use]
pub const fn keys(from: u8, to: u8) -> i32 {
    octaves((to as i32).wrapping_sub(from as i32), SEMITONES)
}

/// One in Q30, the fixed point of a ratio between one and two.
pub const ONE_Q30: u64 = 1 << 30;
/// Sixteenths of an octave: the table's steps.
const TABLE_BITS: u32 = 4;
/// Bits of an interval below a step of the table.
const REST_BITS: u32 = OCTAVE_BITS - TABLE_BITS;
/// The natural logarithm of two in Q30.
///
/// Source: ln 2 = 0.693147180559945, times 2³⁰, rounded.
const LN2_Q30: u64 = 744_261_118;

/// The whole square root of a number, by Newton's iteration: what the
/// tables of powers of two are worked out with, by the compiler.
#[must_use]
pub const fn square_root(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut estimate = value;
    let mut next = value.div_ceil(2);
    while next < estimate {
        estimate = next;
        next = match value.checked_div(estimate) {
            Some(quotient) => estimate.wrapping_add(quotient) / 2,
            None => estimate,
        };
    }
    estimate
}

/// 2^(k/16) in Q30 for k from 0 to 15: each is a product of the square
/// root of two taken one to four times.
///
/// Source: computed here; the roots are whole square roots of numbers in
/// Q60, so each entry is within a few units of 2⁻³⁰ of its value.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const SIXTEENTHS: [u64; 1 << TABLE_BITS] = {
    // roots[i] = 2^(1 / 2^(i+1)).
    let mut roots = [0u64; TABLE_BITS as usize];
    let mut value = 2 * ONE_Q30;
    let mut index = 0;
    while index < roots.len() {
        value = square_root((value as u128) << 30) as u64;
        roots[index] = value;
        index += 1;
    }
    let mut table = [ONE_Q30; 1 << TABLE_BITS];
    let mut entry = 0;
    while entry < table.len() {
        let mut bit = 0;
        while bit < roots.len() {
            // The top bit of the entry's number is the half octave.
            if entry & (1 << (roots.len() - 1 - bit)) != 0 {
                table[entry] = (table[entry] * roots[bit]) >> 30;
            }
            bit += 1;
        }
        entry += 1;
    }
    table
};

/// `value * 2^octaves` for an interval in octaves with [`OCTAVE_BITS`] of
/// fraction, held to a word: within a thousandth of a cent of it. It is
/// what an exponential converter makes of a control voltage.
///
/// It takes 64-bit products, and belongs where a control changes, not in
/// the loop that renders samples.
#[must_use]
pub fn raise(value: u32, octaves: i32) -> u32 {
    let whole = octaves >> OCTAVE_BITS;
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let coarse = SIXTEENTHS.get((fraction >> REST_BITS) as usize).copied().unwrap_or(ONE_Q30);
    // The rest is below a sixteenth of an octave: y = rest * ln 2 is
    // below 0.044, and e^y = 1 + y + y²/2 + y³/6 to within 2⁻²⁷.
    let rest = u64::from(fraction & ((1 << REST_BITS) - 1));
    // Every factor is below 2³¹, so no product leaves its word.
    let y = rest.wrapping_mul(LN2_Q30) >> OCTAVE_BITS;
    let square = y.wrapping_mul(y) >> 30;
    let cube = square.wrapping_mul(y) >> 30;
    let fine = ONE_Q30.wrapping_add(y).wrapping_add(square / 2).wrapping_add(cube / 6);
    let ratio = coarse.wrapping_mul(fine) >> 30;
    let scaled = u128::from(value).wrapping_mul(u128::from(ratio));
    let shifted = match whole {
        // Beyond these the result is zero or leaves a word anyway.
        ..=-64 => 0,
        -63..=-1 => scaled >> whole.unsigned_abs().saturating_add(30),
        0..=31 => (scaled << whole.unsigned_abs()) >> 30,
        _ => u128::MAX,
    };
    u32::try_from(shifted).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{OCTAVE, cents, keys, octaves, raise, semitones};

    #[test]
    fn raising_is_the_power_of_two_to_a_thousandth_of_a_cent() {
        let mut worst = 0.0f64;
        for octaves in (-12 * OCTAVE..12 * OCTAVE).step_by(250_007) {
            let exact = 1.0e6 * (f64::from(octaves) / f64::from(OCTAVE)).exp2();
            let found = f64::from(raise(1_000_000, octaves));
            // Whole units are all a result has.
            let error = ((found - exact).abs() - 1.0).max(0.0) / exact;
            worst = worst.max(error);
        }
        assert!(worst < 0.001 * 2.0f64.ln() / 1200.0, "{worst}");
        assert_eq!((raise(5, 40 * OCTAVE), raise(u32::MAX, -70 * OCTAVE), raise(8, -3 * OCTAVE)), (u32::MAX, 0, 1));
    }

    #[test]
    fn an_interval_is_its_fraction_of_an_octave() {
        assert_eq!((octaves(1, 2), octaves(-3, 1), octaves(1, 0)), (OCTAVE / 2, -3 * OCTAVE, 0));
        assert_eq!((cents(1_200), semitones(12), keys(60, 72), keys(72, 60)), (OCTAVE, OCTAVE, OCTAVE, -OCTAVE));
    }
}
