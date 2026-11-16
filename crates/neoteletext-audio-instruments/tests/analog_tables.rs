//! The tables of the analogue building blocks against their generator and
//! their claims, and the decimators made from them (TODO-6430).

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "../examples/analog_tables.rs"]
mod generator;
#[path = "analog_support.rs"]
mod support;

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::decimator::Decimator;
use neoteletext_audio_instruments::analog::tables::{
    BLAMP, BLEP, BLEP_PHASES_SHIFT, BLEP_TAPS, EXCESS, EXCESS_RANGE_SHIFT, EXCESS_STEPS_SHIFT, HALF_BAND_SHARP,
    HALF_BAND_WIDE, WARP, WARP_STEPS_SHIFT,
};
use neoteletext_audio_instruments::analog::{LEVEL, LIMIT};
use support::{amplitude, decibels, float};

/// Whether two tables agree to one unit, which another machine's
/// arithmetic may round the other way.
fn close<A: Copy + Into<i64>, B: Copy + Into<i64>>(made: &[A], kept: &[B]) -> bool {
    made.len() == kept.len() && made.iter().zip(kept).all(|(&made, &kept)| (made.into() - kept.into()).abs() <= 1)
}

#[test]
fn the_tables_are_what_their_generator_writes() {
    assert_eq!(1usize << EXCESS_STEPS_SHIFT, generator::EXCESS_STEPS);
    assert_eq!(1usize << EXCESS_RANGE_SHIFT, generator::EXCESS_RANGE);
    assert_eq!(1usize << WARP_STEPS_SHIFT, generator::WARP_STEPS);
    assert_eq!((BLEP_TAPS, 1usize << BLEP_PHASES_SHIFT), (generator::BLEP_TAPS, generator::BLEP_PHASES));
    assert!(close(&generator::excess_table(), &EXCESS));
    assert!(close(&generator::warp_table(), &WARP));
    for (made, kept) in generator::blep_table().iter().zip(&BLEP) {
        assert!(close(made, kept));
    }
    for (made, kept) in generator::blamp_table().iter().zip(&BLAMP) {
        assert!(close(made, kept));
    }
    assert_eq!((generator::blep_table().len(), generator::blamp_table().len()), (BLEP.len(), BLAMP.len()));
    assert!(close(&generator::half_band(generator::SHARP_REACH, generator::SHARP_ATTENUATION_DB), &HALF_BAND_SHARP));
    assert!(close(&generator::half_band(generator::WIDE_REACH, generator::WIDE_ATTENUATION_DB), &HALF_BAND_WIDE));
}

#[test]
fn the_excess_read_with_straight_lines_is_within_a_ten_thousandth() {
    let steps = f64::from(1u32 << EXCESS_STEPS_SHIFT);
    let mut worst = 0.0f64;
    for point in 1..(EXCESS.len() - 1) * 64 {
        let t = point as f64 / 64.0 / steps;
        let (index, weight) = (point / 64, (point % 64) as f64 / 64.0);
        let read =
            (f64::from(EXCESS[index]) * (1.0 - weight) + f64::from(EXCESS[index + 1]) * weight) / 4_294_967_296.0;
        // As a share of what the curve takes off, at every level.
        worst = worst.max((read * t.powi(3) / (t - t.tanh()) - 1.0).abs());
    }
    assert!(worst < 1.0e-4, "{worst}");
    // Where the table ends the tangent is one to seven places.
    let end = f64::from(1u32 << EXCESS_RANGE_SHIFT);
    assert!((1.0 - end.tanh()).abs() < 1.0e-6);
    assert_eq!(EXCESS.len(), ((1usize << EXCESS_RANGE_SHIFT) << EXCESS_STEPS_SHIFT) + 1);
}

#[test]
fn a_residual_row_ends_where_the_next_sample_begins() {
    let last = BLEP.len() - 1;
    let centre = BLEP_TAPS / 2;
    for table in [&BLEP, &BLAMP] {
        for tap in 0..BLEP_TAPS - 1 {
            // The step's own tap apart: the last row holds the value
            // before the step there, and the first the value after it.
            let jump = if std::ptr::eq(table, &BLEP) && tap == centre - 1 { 1 << 15 } else { 0 };
            assert_eq!(i32::from(table[last][tap]), i32::from(table[0][tap + 1]) + jump, "tap {tap}");
        }
        // And the residual has died away at both ends.
        assert!(table[0][0].abs() <= 1 && table[last][BLEP_TAPS - 1].abs() <= 1);
    }
    // A step's residual is half the step either side of it.
    assert!(
        (i32::from(BLEP[0][centre]) + (1 << 14)).abs() <= 2
            && (i32::from(BLEP[last][centre - 1]) - (1 << 14)).abs() <= 2
    );
}

/// The gain of a half-band filter at `cycles` cycles a sample of its
/// input.
fn half_band_gain(taps: &[i32], cycles: f64) -> f64 {
    let unity = 4_294_967_296.0;
    0.5 + taps
        .iter()
        .enumerate()
        .map(|(index, &tap)| 2.0 * f64::from(tap) / unity * (2.0 * PI * cycles * (2 * index + 1) as f64).cos())
        .sum::<f64>()
}

#[test]
fn the_half_band_filters_pass_20_khz_and_stop_what_would_fold_below_it() {
    // (taps, input rate, the stop band's edge, the least attenuation.)
    let filters: [(&[i32], f64, f64, f64); 2] =
        [(&HALF_BAND_SHARP, 96_000.0, 28_000.0, 90.0), (&HALF_BAND_WIDE, 192_000.0, 76_000.0, 90.0)];
    for (taps, rate, stop, attenuation) in filters {
        assert_eq!(taps.iter().map(|&tap| i64::from(tap)).sum::<i64>(), 1 << 30);
        let mut ripple = 0.0f64;
        let mut leak = f64::NEG_INFINITY;
        for hz in (0..=20_000).step_by(50) {
            ripple = ripple.max(decibels(half_band_gain(taps, f64::from(hz) / rate).abs()).abs());
        }
        let mut hz = stop;
        while hz <= rate / 2.0 {
            leak = leak.max(decibels(half_band_gain(taps, hz / rate).abs()));
            hz += 50.0;
        }
        println!("{rate} Hz: ripple {ripple:.5} dB to 20 kHz, {leak:.1} dB from {stop} Hz");
        assert!(ripple < 0.01, "{ripple}");
        assert!(leak < -attenuation, "{leak}");
    }
}

#[test]
fn a_decimator_passes_a_constant_and_a_low_tone_and_removes_a_high_one() {
    for (mut decimator, rate) in [(Decimator::sharp(), 96_000.0), (Decimator::wide(), 192_000.0)] {
        let mut last = 0;
        for _ in 0..200 {
            last = decimator.process(LEVEL, LEVEL);
        }
        assert!((last - LEVEL).abs() <= 32, "{last}");

        let tone = |hz: f64| -> Vec<i32> {
            (0..1 << 14)
                .map(|index| (f64::from(LEVEL) * (2.0 * PI * hz / rate * f64::from(index)).sin()) as i32)
                .collect()
        };
        let through = |decimator: &Decimator, hz: f64| -> Vec<f64> {
            let mut decimator = decimator.clone();
            let halved: Vec<i32> =
                tone(hz).as_chunks::<2>().0.iter().map(|&[earlier, later]| decimator.process(earlier, later)).collect();
            float(&halved)
        };
        // 1 kHz comes through at its level; what lies 1 kHz below half
        // the input rate would fold onto it, and does not.
        let passed = amplitude(&through(&decimator, 1_000.0)[256..], 2.0 * 1_000.0 / rate);
        let folded = amplitude(&through(&decimator, rate / 2.0 - 1_000.0)[256..], 2.0 * 1_000.0 / rate);
        assert!(decibels(passed / f64::from(LEVEL)).abs() < 0.01);
        assert!(decibels(folded / f64::from(LEVEL)) < -90.0, "{}", decibels(folded / f64::from(LEVEL)));
    }
}

#[test]
fn a_decimator_is_late_by_its_reach() {
    for mut decimator in [Decimator::sharp(), Decimator::wide()] {
        let delay = decimator.delay();
        // An impulse at an even input sample comes out largest
        // `delay / 2` output samples later, give or take the half sample.
        let outputs: Vec<i32> = (0..64).map(|index| decimator.process(if index == 0 { LEVEL } else { 0 }, 0)).collect();
        let peak = (0..64).max_by_key(|&index| outputs[index].abs()).unwrap_or(0);
        assert!(peak == delay / 2 || peak == delay.div_ceil(2), "{peak} for {delay}");
        // Samples beyond the bound are held to it, not wrapped.
        let mut last = 0;
        for _ in 0..200 {
            last = decimator.process(i32::MAX, i32::MAX);
        }
        assert!((last - LIMIT).abs() <= 32, "{last}");
    }
}
