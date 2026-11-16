//! The arithmetic the blocks share: products of two 32-bit numbers taken
//! in 16-bit halves, so that a processor with no 64-bit product does not
//! need one (REQ-GEN-016), and the bounds that keep every sum inside 32
//! bits.

use super::LIMIT;

/// The low half of a 32-bit word.
const HALF_MASK: u32 = 0xFFFF;
/// Bits of a half.
const HALF_BITS: u32 = 16;

/// `a * b / 2³²`, rounded down, exactly: the upper word of the 64-bit
/// product, from four 32-bit products of the halves.
#[inline]
#[must_use]
pub const fn mul_high(a: i32, b: i32) -> i32 {
    let (a_high, a_low) = (a >> HALF_BITS, a.cast_unsigned() & HALF_MASK);
    let (b_high, b_low) = (b >> HALF_BITS, b.cast_unsigned() & HALF_MASK);
    // A signed half is at most 2¹⁵ in magnitude and an unsigned one below
    // 2¹⁶, so each product below fits its word.
    let middle_a = a_high.wrapping_mul(b_low.cast_signed());
    let middle_b = b_high.wrapping_mul(a_low.cast_signed());
    let low = a_low.wrapping_mul(b_low) >> HALF_BITS;
    let carry =
        (middle_a.cast_unsigned() & HALF_MASK).wrapping_add(middle_b.cast_unsigned() & HALF_MASK).wrapping_add(low)
            >> HALF_BITS;
    a_high
        .wrapping_mul(b_high)
        .wrapping_add(middle_a >> HALF_BITS)
        .wrapping_add(middle_b >> HALF_BITS)
        .wrapping_add(carry.cast_signed())
}

/// `a * b / 2³²`, rounded down, exactly, for unsigned words.
#[inline]
#[must_use]
pub const fn mul_high_unsigned(a: u32, b: u32) -> u32 {
    let (a_high, a_low) = (a >> HALF_BITS, a & HALF_MASK);
    let (b_high, b_low) = (b >> HALF_BITS, b & HALF_MASK);
    let middle_a = a_high.wrapping_mul(b_low);
    let middle_b = b_high.wrapping_mul(a_low);
    let low = a_low.wrapping_mul(b_low) >> HALF_BITS;
    let carry = (middle_a & HALF_MASK).wrapping_add(middle_b & HALF_MASK).wrapping_add(low) >> HALF_BITS;
    a_high
        .wrapping_mul(b_high)
        .wrapping_add(middle_a >> HALF_BITS)
        .wrapping_add(middle_b >> HALF_BITS)
        .wrapping_add(carry)
}

/// The top bit of a word: a half in Q32, and one in Q31.
const TOP_BIT: u32 = 1 << 31;
/// 48 / 17 and 32 / 17 in Q30: the straight line that is nearest to 1 / d
/// for d from a half to one, within a seventeenth of it.
///
/// Source: the line that makes the largest relative error least, a
/// textbook start for Newton's iteration on a reciprocal; the figures are
/// computed here.
const RECIPROCAL_LINE: (u32, u32) = (((48u64 << 30) / 17) as u32, ((32u64 << 30) / 17) as u32);
/// Newton's steps on the reciprocal: each squares the relative error, so
/// three take a seventeenth to below 2⁻³¹.
const RECIPROCAL_STEPS: u32 = 3;

/// A Q31 number below 2³² doubled, held to the largest word.
#[inline]
const fn double(value: u32) -> u32 {
    if value >= TOP_BIT { u32::MAX } else { value << 1 }
}

/// 2⁶³ over a divisor whose top bit is set, held to the largest word: the
/// reciprocal in Q31 of a number from a half to one in Q32. It is within
/// four units of the quotient, by Newton's iteration x(2 - dx) from a
/// straight line, with no division.
#[inline]
#[must_use]
pub const fn reciprocal(divisor: u32) -> u32 {
    let mut estimate = double(RECIPROCAL_LINE.0.wrapping_sub(mul_high_unsigned(divisor, RECIPROCAL_LINE.1)));
    let mut step = 0;
    while step < RECIPROCAL_STEPS {
        // 2 - dx in Q31; dx is near one, so it wraps to near one too.
        let correction = 0u32.wrapping_sub(mul_high_unsigned(divisor, estimate));
        estimate = double(mul_high_unsigned(estimate, correction));
        step = step.wrapping_add(1);
    }
    estimate
}

/// `numerator / denominator` in Q32, for a numerator below the
/// denominator; one, less a unit, for anything larger, and for a
/// denominator of zero. It is within 2⁻²⁸ of the quotient, with no
/// division.
#[inline]
#[must_use]
pub const fn ratio(numerator: u32, denominator: u32) -> u32 {
    if numerator >= denominator {
        return u32::MAX;
    }
    let shift = denominator.leading_zeros();
    double(mul_high_unsigned(numerator << shift, reciprocal(denominator << shift)))
}

/// `value * coefficient` for a coefficient in Q31, below one in magnitude.
/// The result is within two units below the exact product.
#[inline]
#[must_use]
pub const fn scale(value: i32, coefficient: i32) -> i32 {
    mul_high(value, coefficient) << 1
}

/// `value * gain` for a gain in Q28, below eight in magnitude, held to
/// [`LIMIT`] either way.
#[inline]
#[must_use]
pub const fn amplify(value: i32, gain: i32) -> i32 {
    /// Q28 against the Q32 of [`mul_high`].
    const SHIFT: u32 = 4;
    /// The product at which the result reaches the bound.
    const BOUND: i32 = LIMIT >> SHIFT;
    let product = mul_high(value, gain);
    if product >= BOUND {
        LIMIT
    } else if product <= -BOUND {
        -LIMIT
    } else {
        product << SHIFT
    }
}

/// A signal held to [`LIMIT`] either way.
#[inline]
#[must_use]
pub const fn limit(value: i32) -> i32 {
    if value > LIMIT {
        LIMIT
    } else if value < -LIMIT {
        -LIMIT
    } else {
        value
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_wrap, reason = "a test")]
mod tests {
    use super::{amplify, limit, mul_high, mul_high_unsigned, ratio, reciprocal, scale};
    use crate::analog::LIMIT;

    /// A 64-bit linear congruential generator with Knuth's MMIX constants.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 32) as u32
        }
    }

    #[test]
    fn the_products_are_the_64_bit_products() {
        let mut random = Random(1);
        let edges = [0, 1, -1, i32::MAX, i32::MIN, 0xFFFF, 0x1_0000, -0x1_0000, -0xFFFF];
        let check = |a: i32, b: i32| {
            assert_eq!(i64::from(mul_high(a, b)), (i64::from(a) * i64::from(b)) >> 32, "{a} * {b}");
            let (a, b) = (a.cast_unsigned(), b.cast_unsigned());
            assert_eq!(u64::from(mul_high_unsigned(a, b)), (u64::from(a) * u64::from(b)) >> 32, "{a} * {b}");
        };
        for a in edges {
            for b in edges {
                check(a, b);
            }
        }
        for _ in 0..200_000 {
            check(random.next() as i32, random.next() as i32);
        }
    }

    #[test]
    fn a_ratio_is_the_quotient_to_28_bits() {
        let mut random = Random(3);
        let check = |numerator: u32, denominator: u32| {
            let exact = (u64::from(numerator) << 32) / u64::from(denominator);
            let found = u64::from(ratio(numerator, denominator));
            assert!(exact.abs_diff(found) <= 16, "{numerator} / {denominator}: {found} for {exact}");
        };
        for denominator in [1, 2, 3, 0x7FFF_FFFF, 0x8000_0000, 0x8000_0001, u32::MAX] {
            for numerator in [0, 1, denominator / 2, denominator - 1] {
                if numerator < denominator {
                    check(numerator, denominator);
                }
            }
        }
        for _ in 0..200_000 {
            let denominator = (random.next() >> (random.next() % 32)).max(1);
            check(random.next() % denominator, denominator);
        }
        assert_eq!((ratio(5, 5), ratio(6, 5), ratio(0, 0)), (u32::MAX, u32::MAX, u32::MAX));
        // The reciprocal of the smallest divisor is two, held to the
        // largest word, and of the largest, one.
        assert!(reciprocal(1 << 31) >= u32::MAX - 4 && reciprocal(u32::MAX).abs_diff(1 << 31) <= 4);
    }

    #[test]
    fn scaling_and_gain_are_close_and_bounded() {
        let mut random = Random(2);
        for _ in 0..100_000 {
            let value = (random.next() as i32) >> 2;
            let coefficient = random.next() as i32;
            let exact = (i64::from(value) * i64::from(coefficient)) >> 31;
            let found = i64::from(scale(value, coefficient));
            assert!((0..=1).contains(&(exact - found)), "{value} * {coefficient}");

            let exact = ((i64::from(value) * i64::from(coefficient)) >> 28).clamp(-i64::from(LIMIT), i64::from(LIMIT));
            let found = i64::from(amplify(value, coefficient));
            assert!((exact - found).abs() < 16, "{value} * {coefficient}: {found} for {exact}");
            assert!(found.abs() <= i64::from(LIMIT));
        }
        assert_eq!((limit(i32::MAX), limit(i32::MIN), limit(-5)), (LIMIT, -LIMIT, -5));
    }
}
