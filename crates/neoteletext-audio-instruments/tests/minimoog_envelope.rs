//! The Minimoog's contours against the owner's manual and the service
//! manual's parts: the attack's and the decay's time constants, the
//! sustain level, and what the DECAY switch does to a released note
//! (TODO-6432).

// Without the feature there is nothing here to test.
#![cfg(feature = "minimoog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "minimoog_support.rs"]
mod support;

use neoteletext_audio_instruments::minimoog::{ContourPatch, Patch, Range};
use support::{cents, peaks, pitch, play, samples, triangle, whistle};

/// The window the level is read in, in seconds: a few cycles of the
/// test's tone.
const WINDOW: f64 = 0.002;

/// A triangle of 3,520 Hz whose loudness follows the contour.
fn tone(contour: ContourPatch, decay_on: bool) -> Patch {
    let mut patch = triangle();
    patch.oscillators[0].range = Range::Two;
    patch.loudness_contour = contour;
    patch.decay_on = decay_on;
    patch
}

/// The level in each window of a note held and released, the full level
/// being one.
fn levels(patch: &Patch, held: f64, after: f64) -> Vec<f64> {
    let full = peaks(&play(&tone(ContourPatch { attack: 0, decay: 0, sustain: 100 }, false), 81, 0.2, 0.0), WINDOW);
    let full = full[50];
    peaks(&play(patch, 81, held, after), WINDOW).iter().map(|level| level / full).collect()
}

/// The level at a time.
fn at(levels: &[f64], seconds: f64) -> f64 {
    levels[(seconds / WINDOW).round() as usize]
}

#[test]
fn the_attack_is_the_capacitor_charging_towards_twice_the_peak() {
    // A time constant of 600 ms: the level is 2 (1 - e^(-t / 0.6)) until
    // it reaches the peak, after 0.6 ln 2 = 0.416 s.
    let found = levels(&tone(ContourPatch { attack: 600, decay: 0, sustain: 100 }, false), 0.8, 0.0);
    for seconds in [0.05, 0.15, 0.3, 0.4] {
        let expected = 2.0 * (1.0 - f64::exp(-seconds / 0.6));
        println!("{seconds} s: {:.4} for {expected:.4}", at(&found, seconds));
        assert!((at(&found, seconds) - expected).abs() < 0.02);
    }
    assert!((at(&found, 0.45) - 1.0).abs() < 0.01 && (at(&found, 0.7) - 1.0).abs() < 0.01);
}

#[test]
fn the_decay_falls_to_the_sustain_level_with_the_dials_time_constant() {
    // 400 ms on the dial, sustain at 2.5: the level is 0.25 + 0.75
    // e^(-t / 0.4).
    let found = levels(&tone(ContourPatch { attack: 0, decay: 400, sustain: 25 }, false), 3.0, 0.0);
    for seconds in [0.1, 0.4, 0.8, 1.6] {
        let expected = 0.25 + 0.75 * f64::exp(-seconds / 0.4);
        println!("{seconds} s: {:.4} for {expected:.4}", at(&found, seconds));
        assert!((at(&found, seconds) - expected).abs() < 0.01);
    }
    assert!((at(&found, 2.9) - 0.25).abs() < 0.005);
}

#[test]
fn the_ends_of_the_dials_are_a_millisecond_and_ten_seconds() {
    // Ten seconds: after one second of decay to nothing, e^(-0.1).
    let slow = levels(&tone(ContourPatch { attack: 0, decay: 10_000, sustain: 0 }, false), 1.2, 0.0);
    println!("10 s: {:.4} after 1 s", at(&slow, 1.0));
    assert!((at(&slow, 1.0) - f64::exp(-0.1)).abs() < 0.01);
    // The shortest: the peak within three milliseconds of the key.
    let fast = levels(&tone(ContourPatch { attack: 0, decay: 0, sustain: 100 }, false), 0.1, 0.0);
    assert!(at(&fast, 0.004) > 0.98, "{}", at(&fast, 0.004));
}

#[test]
fn the_decay_switch_makes_the_decay_time_the_release() {
    let contour = ContourPatch { attack: 0, decay: 500, sustain: 100 };
    // On: the released note falls as e^(-t / 0.5).
    let on = levels(&tone(contour, true), 0.3, 1.5);
    for seconds in [0.25, 0.5, 1.0] {
        let expected = f64::exp(-seconds / 0.5);
        println!("on, {seconds} s: {:.4} for {expected:.4}", at(&on, 0.3 + seconds));
        assert!((at(&on, 0.3 + seconds) - expected).abs() < 0.01);
    }
    // Off: it is cut, with a time constant of 15 ms.
    let off = levels(&tone(contour, false), 0.3, 0.3);
    for seconds in [0.016, 0.03] {
        let expected = f64::exp(-seconds / 0.015);
        println!("off, {seconds} s: {:.4} for {expected:.4}", at(&off, 0.3 + seconds));
        assert!((at(&off, 0.3 + seconds) - expected).abs() < 0.06);
    }
    assert!(at(&off, 0.5) < 0.001);
}

#[test]
fn the_filters_contour_is_the_same_generator() {
    // The filter whistling, its cutoff raised by the contour: with the
    // sustain at 5 it settles half as far up as with it at 10.
    let settled = |sustain: u8| {
        let mut patch = whistle(-300);
        patch.keyboard_control = [false, false];
        patch.contour_amount = 20;
        patch.filter_contour = ContourPatch { attack: 0, decay: 100, sustain };
        let signal = play(&patch, 60, 3.0, 0.0);
        pitch(&signal[samples(2.0)..])
    };
    let (none, half, full) = (settled(0), settled(50), settled(100));
    println!("{:.1} and {:.1} cents", cents(none, half), cents(none, full));
    assert!((cents(none, full) - 2.0 * cents(none, half)).abs() < 6.0);
}
