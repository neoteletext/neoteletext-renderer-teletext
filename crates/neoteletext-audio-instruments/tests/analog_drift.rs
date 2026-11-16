//! The tolerances and the drift of the analogue building blocks: fixed
//! for a seed and a voice, bounded, and nothing at all for an amount of
//! nothing (TODO-6430).

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

use neoteletext_audio_instruments::analog::drift::{CENTS_BITS, CENTS_LIMIT, Drift, Variation, detune};

/// Five cents, in the fixed point of [`detune`].
const FIVE_CENTS: i32 = 5 << CENTS_BITS;

#[test]
fn a_voices_offsets_are_fixed_by_its_seed_and_its_number() {
    let draw = |seed: u32, voice: u32| -> Vec<i32> {
        let mut variation = Variation::new(seed, voice);
        (0..8).map(|_| variation.offset(FIVE_CENTS)).collect()
    };
    assert_eq!(draw(1, 0), draw(1, 0));
    // Each voice of an instrument differs from every other, and each
    // instrument from the next.
    let voices: Vec<Vec<i32>> = (0..8).map(|voice| draw(1, voice)).collect();
    for (first, offsets) in voices.iter().enumerate() {
        for other in &voices[first + 1..] {
            assert_ne!(offsets, other);
        }
    }
    assert_ne!(draw(1, 0), draw(2, 0));
    // A seed of nothing is a seed.
    assert!(draw(0, 0).iter().any(|&offset| offset != 0));
}

#[test]
fn offsets_fill_their_amount_evenly_and_never_leave_it() {
    let mut sum = 0.0;
    let mut square = 0.0;
    let mut count = 0.0;
    for voice in 0..2_000 {
        let mut variation = Variation::new(99, voice);
        for _ in 0..4 {
            let offset = variation.offset(FIVE_CENTS);
            assert!(offset.abs() <= FIVE_CENTS);
            let share = f64::from(offset) / f64::from(FIVE_CENTS);
            sum += share;
            square += share * share;
            count += 1.0;
        }
    }
    // Uniform: a mean of nothing and a mean square of a third.
    assert!(
        (sum / count).abs() < 0.02 && (square / count - 1.0 / 3.0).abs() < 0.02,
        "{} {}",
        sum / count,
        square / count
    );
    // A negative amount is as good as its positive.
    let mut variation = Variation::new(5, 5);
    assert!(variation.offset(-FIVE_CENTS).abs() <= FIVE_CENTS);
}

#[test]
fn no_amount_is_no_variation_at_all() {
    for voice in 0..16 {
        let mut variation = Variation::new(1234, voice);
        let mut drift = Drift::new(1234, voice, 4_800);
        assert!((0..16).all(|_| variation.offset(0) == 0));
        assert!((0..100_000).all(|_| drift.tick() == 0));
        // And an amount taken away again stops the drift at zero.
        drift.set_amount(FIVE_CENTS);
        for _ in 0..10_000 {
            drift.tick();
        }
        drift.set_amount(0);
        assert!((0..10_000).all(|_| drift.tick() == 0));
    }
}

#[test]
fn drift_is_slow_bounded_and_the_same_from_the_same_seed() {
    // A new point every second at a control rate of 750 a second.
    let interval = 750;
    let run = |seed: u32, voice: u32| -> Vec<i32> {
        let mut drift = Drift::new(seed, voice, interval);
        drift.set_amount(FIVE_CENTS);
        (0..120 * interval).map(|_| drift.tick()).collect()
    };
    let values = run(7, 3);
    assert_eq!(values, run(7, 3));
    assert_ne!(values, run(7, 4));
    assert!(values.iter().all(|value| value.abs() <= FIVE_CENTS));
    // It wanders over most of its range, both ways.
    let (least, most) = (values.iter().min().copied().unwrap_or(0), values.iter().max().copied().unwrap_or(0));
    assert!(least < -FIVE_CENTS / 2 && most > FIVE_CENTS / 2, "{least} {most}");
    // Slowly: no sample moves it by more than a hundredth of the range,
    // and it does not jump when a new point is drawn.
    let steepest = values.windows(2).map(|pair| (pair[1] - pair[0]).abs()).max().unwrap_or(0);
    assert!(steepest <= 2 * FIVE_CENTS / 100, "{steepest}");
    // And it goes on wandering: where it stands as each new point is
    // drawn is seldom where it stood at the last.
    let ends: Vec<i32> = values.iter().skip(interval as usize - 1).step_by(interval as usize).copied().collect();
    assert!(ends.windows(2).filter(|pair| pair[0] != pair[1]).count() > 100);

    // A smaller amount brings it inside at once.
    let mut drift = Drift::new(7, 3, interval);
    drift.set_amount(CENTS_LIMIT);
    for _ in 0..10 * interval {
        drift.tick();
    }
    drift.set_amount(FIVE_CENTS);
    assert!((0..10 * interval).all(|_| drift.tick().abs() <= FIVE_CENTS));
    // No interval is an interval of one sample.
    let mut quick = Drift::new(1, 1, 0);
    quick.set_amount(1000);
    assert!((0..1000).all(|_| quick.tick().abs() <= 1000));
}

#[test]
fn a_detuned_step_is_the_step_times_two_to_the_cents_over_1200() {
    let mut worst = 0.0f64;
    for step in [1u32 << 12, 39_370_534, 1 << 28, 1 << 30, (1 << 31) - 1] {
        for hundredths in (-10_000..=10_000).step_by(37) {
            let cents = f64::from(hundredths) / 100.0;
            let fixed = (cents * f64::from(1u32 << CENTS_BITS)).round() as i32;
            let exact = f64::from(step) * (cents / 1200.0).exp2();
            let found = f64::from(detune(step, fixed));
            // In cents, with half a unit of the step for its rounding.
            let error =
                1200.0 * ((found + 0.5) / exact).log2().abs().min(1200.0 * ((found - 0.5) / exact).log2().abs());
            if step >= 1 << 24 {
                worst = worst.max(error);
            }
            assert!((found - exact).abs() <= 2.0 + exact * 1.0e-6, "{step} by {cents}: {found} for {exact}");
        }
    }
    assert!(worst < 1.0e-3, "{worst}");
    // Nothing is nothing, and more than a semitone is a semitone.
    assert_eq!(detune(123_456_789, 0), 123_456_789);
    assert_eq!(detune(1 << 28, i32::MAX), detune(1 << 28, CENTS_LIMIT));
    assert_eq!(detune(1 << 28, i32::MIN), detune(1 << 28, -CENTS_LIMIT));
    assert_eq!(detune(0, CENTS_LIMIT), 0);
}
