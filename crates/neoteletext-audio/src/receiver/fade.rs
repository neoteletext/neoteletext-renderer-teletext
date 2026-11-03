//! The fades either side of a block given as silence (REQ-AUD-013,
//! `design/audio.md` section 8.2): [`crate::MIN_FADE_MS`], so the gap does
//! not click, applied to the samples next to it.

use super::MS_PER_SECOND;
use crate::{MIN_FADE_MS, SAMPLE_RATES};

/// The highest rate of the profile: the table is in ascending order.
const MAX_RATE: u32 = match SAMPLE_RATES.last() {
    Some(&rate) => rate,
    None => 0,
};
/// The most samples a loss fade spans: [`MIN_FADE_MS`] at the highest rate.
pub(super) const MAX_FADE_SAMPLES: usize = (MAX_RATE * MIN_FADE_MS as u32 / MS_PER_SECOND) as usize;
/// A fade's gain is a fraction of `1 << GAIN_BITS`, so each sample costs a
/// multiplication and a shift, and the fade one division (REQ-GEN-016).
const GAIN_BITS: u32 = 16;
const GAIN_ONE: u32 = 1 << GAIN_BITS;

/// The samples of a loss fade at `rate_hz`: 5 ms (40 at 8 kHz, 240 at 48 kHz).
pub(super) fn fade_samples(rate_hz: u32) -> usize {
    let samples = rate_hz.saturating_mul(u32::from(MIN_FADE_MS)).checked_div(MS_PER_SECOND).unwrap_or(0);
    usize::try_from(samples).unwrap_or(0).min(MAX_FADE_SAMPLES)
}

#[cfg(any(feature = "pcm16", feature = "ima"))]
/// Ramps `samples` up from silence: sample `n` of `L` is scaled by
/// (n + 1) / (L + 1), so neither end is a step.
pub(super) fn fade_in(samples: &mut [i16]) {
    ramp(samples, false);
}

/// Ramps `samples` down to silence: sample `n` of `L` is scaled by
/// (L − n) / (L + 1).
pub(super) fn fade_out(samples: &mut [i16]) {
    ramp(samples, true);
}

/// Ramps up from silence the samples of a sound that fall in the `length`
/// samples from sample `from`, when `samples` hold the sound from sample
/// `at`: the fade in at a voice's position, which may fall anywhere in a
/// block, or across two writes (DEC-2470). Sample `from + n` is scaled by
/// (n + 1) / (length + 1), as [`fade_in`] scales sample `n`.
#[cfg(feature = "opus")]
pub(super) fn fade_in_from(samples: &mut [i16], at: u32, from: u32, length: usize) {
    let length = u32::try_from(length).unwrap_or(u32::MAX);
    let step = GAIN_ONE.checked_div(length.saturating_add(1)).unwrap_or(0);
    for (position, sample) in (at..).zip(samples.iter_mut()) {
        let Some(offset) = position.checked_sub(from).filter(|&offset| offset < length) else { continue };
        let gain = i32::try_from(offset.saturating_add(1).saturating_mul(step)).unwrap_or(0);
        let scaled = i32::from(*sample).saturating_mul(gain) >> GAIN_BITS;
        *sample = i16::try_from(scaled).unwrap_or(0);
    }
}

fn ramp(samples: &mut [i16], down: bool) {
    let length = u32::try_from(samples.len()).unwrap_or(u32::MAX);
    let step = GAIN_ONE.checked_div(length.saturating_add(1)).unwrap_or(0);
    for (index, sample) in (0_u32..).zip(samples.iter_mut()) {
        let steps = if down { length.saturating_sub(index) } else { index.saturating_add(1) };
        // At most 65 535: the product with a sample fits 32 bits.
        let gain = i32::try_from(steps.saturating_mul(step)).unwrap_or(0);
        let scaled = i32::from(*sample).saturating_mul(gain) >> GAIN_BITS;
        *sample = i16::try_from(scaled).unwrap_or(0);
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::panic)]
mod tests {
    // Tests of REQ-AUD-013.
    use super::*;

    #[test]
    fn a_fade_spans_five_milliseconds_at_every_rate() {
        let expected = [40, 55, 80, 110, 120, 160, 220, 240];
        for (&rate, &samples) in SAMPLE_RATES.iter().zip(&expected) {
            assert_eq!(fade_samples(rate), samples, "{rate} Hz");
        }
        assert_eq!(MAX_FADE_SAMPLES, 240);
        assert!(SAMPLE_RATES.windows(2).all(|pair| pair[0] < pair[1]), "the table ascends");
    }

    #[cfg(any(feature = "pcm16", feature = "ima"))]
    #[test]
    fn fades_ramp_without_a_step_at_either_end() {
        let mut rising = [10_000_i16; 4];
        fade_in(&mut rising);
        // (n + 1) / 5 of full level, in 16-bit fixed point.
        assert_eq!(rising, [1999, 3999, 5999, 7999]);
        let mut falling = [10_000_i16; 4];
        fade_out(&mut falling);
        assert_eq!(falling, [7999, 5999, 3999, 1999]);
        let mut negative = [i16::MIN; 3];
        fade_out(&mut negative);
        assert!(negative.iter().all(|&sample| sample < 0 && sample > i16::MIN));
    }

    // REQ-AUD-013, DEC-2470: a fade in at a voice's position matches the
    // block fade, wherever the position falls, and spans two writes.
    #[cfg(feature = "opus")]
    #[test]
    fn a_fade_in_from_a_position_ramps_only_the_samples_after_it() {
        let mut first = [10_000_i16; 6];
        let mut second = [10_000_i16; 6];
        // The sound's samples 100 to 105, then 106 to 111; the voice at 103.
        fade_in_from(&mut first, 100, 103, 4);
        fade_in_from(&mut second, 106, 103, 4);
        assert_eq!(first, [10_000, 10_000, 10_000, 1999, 3999, 5999]);
        assert_eq!(second, [7999, 10_000, 10_000, 10_000, 10_000, 10_000]);
    }
}
