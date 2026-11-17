//! The low-frequency oscillator of the analogue building blocks: its
//! shapes, its rate, and its delay and fade (TODO-6430).

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

#[path = "analog_support.rs"]
mod support;

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::frequency;
use neoteletext_audio_instruments::analog::lfo::{LFO_ONE, Lfo, Shape};
use support::{decibels, spectrum};

/// The rate the oscillator is ticked at here: once every 64 output
/// samples.
const RATE: u32 = 750;
const ONE: f64 = LFO_ONE as f64;

/// One cycle of a shape, 4,096 samples long.
fn cycle(shape: Shape) -> Vec<f64> {
    let mut lfo = Lfo::new(1);
    lfo.set_shape(shape);
    lfo.set_step(1 << 20);
    lfo.key();
    (0..4_096).map(|_| f64::from(lfo.tick()) / ONE).collect()
}

#[test]
fn the_triangle_and_the_square_are_exact() {
    for (index, (&triangle, &square)) in cycle(Shape::Triangle).iter().zip(&cycle(Shape::Square)).enumerate() {
        let phase = index as f64 / 4_096.0;
        let expected = if phase < 0.25 {
            4.0 * phase
        } else if phase < 0.75 {
            2.0 - 4.0 * phase
        } else {
            4.0 * phase - 4.0
        };
        assert!((triangle - expected).abs() < 1e-9, "triangle at {phase}: {triangle}");
        assert!((square - if phase < 0.5 { 1.0 } else { -1.0 }).abs() < 1e-9, "square at {phase}");
    }
}

#[test]
fn the_sine_is_within_a_thousandth_and_its_harmonics_60_db_down() {
    let wave = cycle(Shape::Sine);
    let mut worst = 0.0f64;
    for (index, &value) in wave.iter().enumerate() {
        worst = worst.max((value - (2.0 * PI * index as f64 / 4_096.0).sin()).abs());
        assert!(value.abs() <= 1.0);
    }
    assert!(worst < 1.1e-3, "{worst}");
    // Sixteen cycles under the window: the fundamental on bin 16, and
    // nothing else above 60 dB below it.
    let long: Vec<f64> = wave.iter().cycle().take(1 << 16).copied().collect();
    let magnitudes = spectrum(&long);
    let harmonics = magnitudes[32..].iter().fold(0.0f64, |most, &magnitude| most.max(magnitude));
    let found = decibels(harmonics / magnitudes[16]);
    println!("sine: within {worst:.5} of the function, harmonics {found:.1} dB");
    assert!((magnitudes[16] - 1.0).abs() < 2e-3 && found < -60.0, "{found}");
}

#[test]
fn the_rate_is_the_step_at_the_rate_it_is_ticked_at() {
    // 6.25 Hz ticked 750 times a second: 25 cycles in 4 s, counted by
    // the square's rises.
    let mut lfo = Lfo::new(1);
    lfo.set_shape(Shape::Square);
    lfo.set_step(frequency(6_250, RATE));
    let levels: Vec<i32> = (0..4 * RATE).map(|_| lfo.tick()).collect();
    let rises = levels.windows(2).filter(|pair| pair[0] < 0 && pair[1] > 0).count();
    assert!((24..=25).contains(&rises), "{rises}");
}

#[test]
fn sample_and_hold_steps_once_a_cycle_to_levels_from_its_seed() {
    let run = |seed: u32| -> Vec<i32> {
        let mut lfo = Lfo::new(seed);
        lfo.set_shape(Shape::SampleAndHold);
        lfo.set_step(frequency(10_000, RATE));
        (0..40 * RATE).map(|_| lfo.tick()).collect()
    };
    let levels = run(7);
    assert_eq!(levels, run(7));
    assert_ne!(levels, run(8));
    // 75 samples to a cycle: the level changes there and nowhere else.
    let changes: Vec<usize> =
        levels.windows(2).enumerate().filter(|(_, pair)| pair[0] != pair[1]).map(|(index, _)| index + 1).collect();
    assert_eq!(changes.len(), 399);
    assert!(changes.iter().enumerate().all(|(cycle, &at)| at == 75 * (cycle + 1)));
    // The levels fill the range evenly: a mean of nothing, a mean square
    // of a third.
    let held: Vec<f64> = levels.iter().step_by(75).map(|&level| f64::from(level) / ONE).collect();
    let mean = held.iter().sum::<f64>() / held.len() as f64;
    let square = held.iter().map(|level| level * level).sum::<f64>() / held.len() as f64;
    assert!(mean.abs() < 0.1 && (square - 1.0 / 3.0).abs() < 0.06, "{mean} {square}");
    assert!(held.iter().all(|level| level.abs() <= 1.0));
}

#[test]
fn a_key_brings_the_delay_and_then_the_fade() {
    // Half a second of nothing, then a second to full depth.
    let mut lfo = Lfo::new(1);
    lfo.set_shape(Shape::Square);
    lfo.set_step(frequency(50_000, RATE));
    lfo.set_delay(RATE / 2, RATE);
    lfo.key();
    let depth: Vec<f64> = (0..3 * RATE).map(|_| (f64::from(lfo.tick()) / ONE).abs()).collect();
    for (index, &depth) in depth.iter().enumerate() {
        let seconds = index as f64 / f64::from(RATE);
        let expected = (seconds - 0.5).clamp(0.0, 1.0);
        assert!((depth - expected).abs() < 2e-3, "{seconds} s: {depth}");
    }
    // The cycle ran on through the silence: a key does not restart it,
    // and restarting it is asked for by name.
    assert_ne!(lfo.phase(), 0);
    lfo.restart();
    assert_eq!(lfo.phase(), 0);
    // A second key starts the silence again.
    lfo.key();
    assert_eq!(lfo.tick(), 0);
    // No delay and no fade: full depth from the first sample.
    lfo.set_delay(0, 0);
    lfo.key();
    assert_eq!(lfo.tick().abs(), LFO_ONE);
    // A fade alone starts at once.
    lfo.set_delay(0, 10);
    lfo.key();
    let risen: Vec<i32> = (0..12).map(|_| lfo.tick().abs()).collect();
    assert!(risen[0] == 0 && risen[5] > LFO_ONE / 3 && risen[11] == LFO_ONE, "{risen:?}");
}

#[test]
fn a_shorter_delay_set_while_waiting_shortens_the_wait() {
    // A key with two seconds of delay; a tenth of a second on, the delay
    // is set to nothing, as a patch change does under a held note. The
    // modulation is there at once, not two seconds later.
    let mut lfo = Lfo::new(1);
    lfo.set_shape(Shape::Square);
    lfo.set_step(frequency(50_000, RATE));
    lfo.set_delay(2 * RATE, 0);
    lfo.key();
    for _ in 0..RATE / 10 {
        assert_eq!(lfo.tick(), 0);
    }
    lfo.set_delay(0, 0);
    assert_eq!(lfo.tick().abs(), LFO_ONE);
    // Shortened to a wait that has not yet passed, it ends when that
    // has passed since the change at the latest; and with a fade, the
    // fade follows it.
    lfo.set_delay(2 * RATE, 0);
    lfo.key();
    let _ = lfo.tick();
    lfo.set_delay(RATE / 10, RATE / 10);
    let depth: Vec<i32> = (0..RATE / 4).map(|_| lfo.tick().abs()).collect();
    assert!(depth[(RATE / 10) as usize - 1] == 0 && depth[(RATE / 4) as usize - 1] == LFO_ONE, "{depth:?}");
    // A longer delay set while waiting leaves the wait as it was, and a
    // delay set with no key waiting changes nothing until a key.
    lfo.set_delay(RATE, 0);
    assert_eq!(lfo.tick().abs(), LFO_ONE);
}
