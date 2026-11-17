//! From the chip clock, about a million samples a second, to 48,000: two
//! stages, each a real low-pass filter.
//!
//! The first is a cascaded integrator-comb filter (Hogenauer, "An
//! economical class of digital filters for decimation and interpolation",
//! IEEE Transactions on Acoustics, Speech and Signal Processing, 1981). It
//! takes every chip cycle and gives one sample in [`CIC_RATIO`], whatever
//! the clock: the intermediate rate, 246,312 Hz on a PAL machine and
//! 255,682 Hz on an NTSC one. The second is a windowed-sinc filter read at
//! the exact instant of each output sample, between two intermediate
//! samples, so the ratio that is not a whole number costs no drift and no
//! jitter.
//!
//! What the two stages leave of anything that would fold into the band
//! below 20 kHz: the comb filter passes at most -74.9 dB of the bands
//! around the intermediate rate (-81 dB of what folds below 20 kHz), and
//! the sinc filter at most -76 dB of everything above 28 kHz (29 kHz on
//! NTSC). `tests/sid_decimator.rs` measures both.

use super::ChipClock;
use super::fir_table::{FILTER, ONE_SHIFT, PHASES, TAPS};
use crate::SAMPLE_RATE;

/// Chip cycles for each intermediate sample.
pub(super) const CIC_RATIO: u32 = 4;
/// Integrators, and combs, of the first stage. Each adds 20 dB at least
/// to what the stage removes around the intermediate rate.
const CIC_ORDER: usize = 4;
/// The first stage's gain, its ratio to the power of its order, as a
/// shift: 4⁴ = 2⁸.
const CIC_GAIN_SHIFT: u32 = 8;

/// The first stage: integrators at the chip clock, combs at the
/// intermediate rate. The integrators wrap, as the structure allows: the
/// combs' differences are exact while the result fits the word, and three
/// voices at their largest, 2²² units, times the gain of 2⁸ do.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Cic {
    integrators: [i32; CIC_ORDER],
    combs: [i32; CIC_ORDER],
}

impl Cic {
    /// An empty filter.
    pub(super) const fn new() -> Self {
        Self { integrators: [0; CIC_ORDER], combs: [0; CIC_ORDER] }
    }

    /// One chip cycle's sample.
    #[inline]
    pub(super) fn integrate(&mut self, sample: i32) {
        let [first, second, third, fourth] = &mut self.integrators;
        *first = first.wrapping_add(sample);
        *second = second.wrapping_add(*first);
        *third = third.wrapping_add(*second);
        *fourth = fourth.wrapping_add(*third);
    }

    /// The intermediate sample, after [`CIC_RATIO`] cycles.
    #[inline]
    pub(super) fn decimate(&mut self) -> i32 {
        let mut value = self.integrators[CIC_ORDER - 1];
        for comb in &mut self.combs {
            let difference = value.wrapping_sub(*comb);
            *comb = value;
            value = difference;
        }
        value >> CIC_GAIN_SHIFT
    }
}

/// Parts of an intermediate sample in which the output's position is
/// counted: [`CIC_RATIO`] times the output rate, so that the chip clock in
/// hertz is the position's advance for each output sample, exactly.
const POSITION_UNITS: u32 = CIC_RATIO * SAMPLE_RATE;
/// Position units in one phase of the filter table.
const PHASE_UNITS: u32 = POSITION_UNITS / PHASES as u32;
/// Fraction bits of the weight between two phases of the table.
const BLEND_BITS: u32 = 8;
/// 2²⁴ over [`PHASE_UNITS`], rounded down: position units within a phase,
/// times this, are the blend weight in 24 fraction bits, with no division.
const BLEND_SCALE: u32 = (1 << 24) / PHASE_UNITS;
/// Shift from that product to [`BLEND_BITS`] fraction bits.
const BLEND_SHIFT: u32 = 24 - BLEND_BITS;

/// Each product of a sample and a tap gives up this many bits before the
/// sum, so that the sum of [`TAPS`] of them fits 32 bits: a sample is
/// below 2¹⁵ and a row's taps sum to less than 2¹⁸ in magnitude.
const PRODUCT_SHIFT: u32 = 4;
/// Bits given up before the two phases' sums are blended, so that their
/// difference times the weight fits 32 bits.
const BLEND_PRE_SHIFTS: (u32, u32) = (5, 3);
/// Shift from the two sums' difference times the weight back to the sums'
/// own fraction bits.
const BLEND_POST_SHIFT: u32 = BLEND_BITS - BLEND_PRE_SHIFTS.1;
/// Fraction bits left in the blended sum.
const SUM_FRACTION_BITS: u32 = ONE_SHIFT - PRODUCT_SHIFT - BLEND_PRE_SHIFTS.0;
/// A half in those bits, to round the output.
const SUM_HALF: i32 = 1 << (SUM_FRACTION_BITS - 1);

/// The second stage, with the position of the output in the intermediate
/// samples.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Resampler {
    /// The last [`TAPS`] intermediate samples, each written twice, [`TAPS`]
    /// apart, so that they are always found in order in one run.
    history: [i16; 2 * TAPS],
    /// Where the next sample goes, below [`TAPS`].
    next: usize,
    /// The output's position after the newest sample it has passed: a
    /// phase of the table, and position units within the phase.
    phase: u32,
    units: u32,
    /// The position's advance for each output sample: whole intermediate
    /// samples, phases, and units within a phase.
    step: (u32, u32, u32),
}

impl Resampler {
    /// An empty filter at position zero.
    pub(super) const fn new(clock: ChipClock) -> Self {
        let hz = clock.hz();
        let within = hz % POSITION_UNITS;
        Self {
            history: [0; 2 * TAPS],
            next: 0,
            phase: 0,
            units: 0,
            step: (hz / POSITION_UNITS, within / PHASE_UNITS, within % PHASE_UNITS),
        }
    }

    /// Moves to the next output sample's position, and says how many
    /// intermediate samples it needs first: the whole part of the clock
    /// over [`POSITION_UNITS`], or one more.
    #[inline]
    pub(super) fn advance(&mut self) -> u32 {
        let (whole, phases, units) = self.step;
        let mut samples = whole;
        self.units = self.units.wrapping_add(units);
        self.phase = self.phase.wrapping_add(phases);
        if self.units >= PHASE_UNITS {
            self.units = self.units.wrapping_sub(PHASE_UNITS);
            self.phase = self.phase.wrapping_add(1);
        }
        if self.phase >= PHASES as u32 {
            self.phase = self.phase.wrapping_sub(PHASES as u32);
            samples = samples.wrapping_add(1);
        }
        samples
    }

    /// Takes an intermediate sample.
    #[inline]
    pub(super) fn push(&mut self, sample: i16) {
        if let Some(slot) = self.history.get_mut(self.next) {
            *slot = sample;
        }
        if let Some(slot) = self.history.get_mut(self.next.wrapping_add(TAPS)) {
            *slot = sample;
        }
        let next = self.next.wrapping_add(1);
        self.next = if next >= TAPS { 0 } else { next };
    }

    /// The output sample at the current position.
    #[inline]
    pub(super) fn output(&self) -> i16 {
        let phase = self.phase as usize;
        let (Some(window), Some(before), Some(after)) = (
            self.history.get(self.next..self.next.wrapping_add(TAPS)),
            FILTER.get(phase),
            FILTER.get(phase.wrapping_add(1)),
        ) else {
            return 0;
        };
        let mut sums = (0i32, 0i32);
        for ((&sample, &tap_before), &tap_after) in window.iter().zip(before).zip(after) {
            let sample = i32::from(sample);
            sums.0 = sums.0.wrapping_add(sample.wrapping_mul(i32::from(tap_before)) >> PRODUCT_SHIFT);
            sums.1 = sums.1.wrapping_add(sample.wrapping_mul(i32::from(tap_after)) >> PRODUCT_SHIFT);
        }
        // Between two rows of the table the filter is taken as a straight
        // line: at 32 rows that is below -90 dB of a 20 kHz tone.
        let weight = (self.units.wrapping_mul(BLEND_SCALE) >> BLEND_SHIFT).cast_signed();
        let base = sums.0 >> BLEND_PRE_SHIFTS.0;
        let difference = (sums.1 >> BLEND_PRE_SHIFTS.0).wrapping_sub(base) >> BLEND_PRE_SHIFTS.1;
        let blended = base.wrapping_add(difference.wrapping_mul(weight) >> BLEND_POST_SHIFT);
        let rounded = blended.wrapping_add(SUM_HALF) >> SUM_FRACTION_BITS;
        // The chip's output came through the limiter (`analogue`), so
        // only this filter's overshoot of a signal already at the
        // limiter's ceiling can be beyond a sample, and is held to it.
        rounded.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_wrap,
    reason = "a test"
)]
mod tests {
    use super::{CIC_RATIO, ChipClock, Cic, FILTER, ONE_SHIFT, PHASES, Resampler, TAPS};
    use crate::SAMPLE_RATE;

    #[test]
    fn the_comb_filter_has_unity_gain_and_the_boxcars_impulse_response() {
        let mut cic = Cic::new();
        let mut outputs = [0i32; 8];
        for (index, output) in outputs.iter_mut().enumerate() {
            for cycle in 0..CIC_RATIO {
                cic.integrate(if index == 0 && cycle == 0 { 256 * 1000 } else { 0 });
            }
            *output = cic.decimate();
        }
        // The response is four boxcars of four convolved: 1 4 10 20 31 40
        // 44 40 31 20 10 4 1. An impulse in a block's first cycle is read
        // three, seven and eleven cycles later.
        assert_eq!(outputs, [20_000, 40_000, 4_000, 0, 0, 0, 0, 0]);

        let mut cic = Cic::new();
        let mut last = 0;
        for _ in 0..16 {
            for _ in 0..CIC_RATIO {
                cic.integrate(-1_234_567);
            }
            last = cic.decimate();
        }
        assert_eq!(last, -1_234_567);
    }

    #[test]
    fn the_comb_filter_is_exact_though_its_integrators_wrap() {
        let mut cic = Cic::new();
        let mut last = 0;
        // Long enough for every integrator to wrap many times.
        for _ in 0..100_000 {
            for _ in 0..CIC_RATIO {
                cic.integrate(4_000_000);
            }
            last = cic.decimate();
        }
        assert_eq!(last, 4_000_000);
    }

    #[test]
    fn every_row_of_the_table_has_unity_gain_and_fits_the_sum() {
        assert_eq!(FILTER.len(), PHASES + 1);
        for row in &FILTER {
            assert_eq!(row.len(), TAPS);
            assert_eq!(row.iter().map(|&tap| i32::from(tap)).sum::<i32>(), 1 << ONE_SHIFT);
            // The bound the sum's shift relies on.
            assert!(row.iter().map(|&tap| i32::from(tap).abs()).sum::<i32>() < 1 << 18);
        }
        // The last row is the first, one sample later.
        assert_eq!(FILTER[PHASES][1..], FILTER[0][..TAPS - 1]);
    }

    #[test]
    fn a_constant_passes_unchanged_at_every_position() {
        for clock in [ChipClock::Pal, ChipClock::Ntsc] {
            let mut resampler = Resampler::new(clock);
            for sample in 0..2_000 {
                for _ in 0..resampler.advance() {
                    resampler.push(-12_345);
                }
                if sample > TAPS {
                    assert_eq!(resampler.output(), -12_345);
                }
            }
        }
    }

    #[test]
    fn the_position_advances_by_the_clock_without_drift() {
        for clock in [ChipClock::Pal, ChipClock::Ntsc] {
            let mut resampler = Resampler::new(clock);
            let mut intermediate = 0u64;
            // Four seconds of output are four times the clock in chip
            // cycles, and so the clock in intermediate samples, exactly.
            for _ in 0..CIC_RATIO * SAMPLE_RATE {
                intermediate += u64::from(resampler.advance());
            }
            assert_eq!(intermediate, u64::from(clock.hz()));
            assert_eq!((resampler.phase, resampler.units), (0, 0));
        }
    }
}
