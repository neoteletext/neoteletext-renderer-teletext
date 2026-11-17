//! The output stage every instrument shares: the limiter its last sample
//! passes through.
//!
//! The instruments play one note at one level ([`crate::REFERENCE_LEVEL_TENTHS`]),
//! and a level worth hearing leaves less room than every note an
//! instrument can sound needs when all of them peak in the same sample.
//! [`limit`] is what stands between that sum and the 16 bits of a sample.
//! Up to its knee it changes nothing, not one bit; above it bends over
//! and comes to rest at the ceiling, so that an output never wraps and
//! never meets a corner.
//!
//! Source: own choice. Every note of an instrument peaking in the same
//! sample is rare (keys struck together drift apart at once); a soft
//! saturation there is what an analogue output stage does with it; and
//! one loudness for one note on every instrument matters more to a
//! channel's author than a clean sum in that case.

/// The largest magnitude [`limit`] gives: a sample's largest positive
/// value, so that the two signs are alike and the most negative sample is
/// never made.
pub const LIMIT_CEILING: i16 = i16::MAX;

/// Bits of the span of input above the knee over which the limiter bends:
/// 2¹⁵, the whole of a sample's full scale again. An input that far above
/// the knee, 4.9 dB above full scale, and any beyond, gives the ceiling.
///
/// Source: own choice: a power of two, so that the curve needs no
/// division; and with [`LIMIT_ORDER`] it puts the knee near 3 dB below
/// full scale.
const SPAN_BITS: u32 = 15;
/// The span itself.
const SPAN: u32 = 1 << SPAN_BITS;
/// The order of the limiter's curve. Above the knee, with `u` the share
/// of the span the input has crossed, the output rises by
/// `(SPAN / ORDER) * (1 - (1 - u)^ORDER)`: its slope is one at the knee,
/// where it meets the straight line below, and falls to nothing at the
/// span's end, with its curvature, where it meets the ceiling.
///
/// Source: own choice: the fourth power is two squarings.
const LIMIT_ORDER: u32 = 4;
/// What the output rises by between the knee and the ceiling.
const RISE: u32 = SPAN / LIMIT_ORDER;
/// The magnitude up to which [`limit`] changes nothing: 24,575 of a
/// sample's 32,768, which is 2.5 dB below full scale.
///
/// Source: own choice, with the two above: the ceiling less the rise.
pub const LIMIT_KNEE: i16 = LIMIT_CEILING - RISE as i16;
const _: () = assert!(LIMIT_KNEE == 24_575);

/// The sample of an output, in a sample's units and of any size: itself
/// up to [`LIMIT_KNEE`] either way, and above that bent smoothly towards
/// [`LIMIT_CEILING`], which it reaches 2¹⁵ above the knee and keeps.
///
/// It holds nothing from one sample to the next, never falls as its
/// input rises, treats the two signs alike, and costs two multiplications
/// at most.
#[must_use]
pub const fn limit(sample: i32) -> i16 {
    let magnitude = sample.unsigned_abs();
    let knee = LIMIT_KNEE.unsigned_abs() as u32;
    let limited = if magnitude <= knee {
        magnitude
    } else {
        let above = magnitude.wrapping_sub(knee);
        if above >= SPAN {
            LIMIT_CEILING.unsigned_abs() as u32
        } else {
            // 1 - u in Q15, then its square and its fourth power: each is
            // at most 2¹⁵, so each product is inside 32 bits.
            let left = SPAN.wrapping_sub(above);
            let square = left.wrapping_mul(left) >> SPAN_BITS;
            let fourth = square.wrapping_mul(square) >> SPAN_BITS;
            (LIMIT_CEILING.unsigned_abs() as u32).wrapping_sub(fourth >> LIMIT_ORDER.trailing_zeros())
        }
    };
    // At most the ceiling, so it is a sample of either sign.
    let limited = limited as i16;
    if sample < 0 { limited.wrapping_neg() } else { limited }
}
const _: () = assert!(LIMIT_ORDER == 4, "limit() takes the fourth power by squaring twice");

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, reason = "a test")]
mod tests {
    use super::{LIMIT_CEILING, LIMIT_KNEE, SPAN, limit};

    #[test]
    fn nothing_changes_up_to_the_knee() {
        for sample in i16::MIN..=i16::MAX {
            if sample.unsigned_abs() <= LIMIT_KNEE.unsigned_abs() {
                assert_eq!(limit(i32::from(sample)), sample);
            }
        }
    }

    #[test]
    fn the_curve_never_falls_and_never_steps_by_more_than_the_input() {
        let mut last = limit(0);
        for sample in 1..=i32::from(LIMIT_KNEE) + SPAN.cast_signed() + 1_000 {
            let now = limit(sample);
            assert!(now >= last, "{sample}: {now} after {last}");
            assert!(now - last <= 1, "{sample}: {now} after {last}");
            last = now;
        }
        assert_eq!(last, LIMIT_CEILING);
    }

    #[test]
    fn the_two_signs_are_alike_and_the_ends_are_the_ceiling() {
        for sample in (0..=200_000).chain([1 << 20, 1 << 29, i32::MAX]) {
            let positive = limit(sample);
            assert_eq!(limit(-sample), -positive, "{sample}");
        }
        assert_eq!(limit(i32::MAX), LIMIT_CEILING);
        assert_eq!(limit(i32::MIN), -LIMIT_CEILING);
    }

    #[test]
    fn the_curve_follows_its_law() {
        // 1 - (1 - u)^4 of a quarter of the span, to a sample's step or
        // two: each of the two shifts rounds down.
        let rise = f64::from(SPAN) / 4.0;
        for above in (0..SPAN).step_by(97) {
            let share = f64::from(above) / f64::from(SPAN);
            let expected = f64::from(LIMIT_KNEE) + rise * (1.0 - (1.0 - share).powi(4));
            let found = f64::from(limit(i32::from(LIMIT_KNEE) + above.cast_signed()));
            assert!((found - expected).abs() <= 2.0, "{above}: {found} for {expected}");
        }
    }

    #[test]
    fn the_knee_is_two_and_a_half_decibels_under_full_scale() {
        let decibels = 20.0 * (f64::from(LIMIT_KNEE) / 32_768.0).log10();
        assert!((decibels + 2.5).abs() < 0.01, "{decibels}");
    }
}
