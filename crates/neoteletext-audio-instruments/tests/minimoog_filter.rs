//! The Minimoog's filter against the service manual's adjustments: where
//! it oscillates, how the dial, the keyboard and the contour move it, its
//! slope, and what the EMPHASIS does to what passes (TODO-6432).

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

#[path = "analog_support.rs"]
mod analog;
#[path = "minimoog_support.rs"]
mod support;

use analog::{amplitude, decibels};
use neoteletext_audio_instruments::minimoog::{Patch, Range};
use support::{RATE, cents, pitch, play, rms, samples, triangle, whistle};

/// The pitch the filter whistles at on a key, once it has grown.
fn whistled(patch: &Patch, key: u8) -> f64 {
    let signal = play(patch, key, 2.5, 0.0);
    pitch(&signal[samples(1.5)..])
}

#[test]
fn the_filter_range_adjustment_holds_minus_one_on_the_dial_is_440_hertz() {
    // Folkman, section VII, "Filter Range": keyboard control off, CUTOFF
    // FREQUENCY at -1, EMPHASIS at 10: zero beats with the A-440.
    let mut patch = whistle(-100);
    patch.keyboard_control = [false, false];
    for key in [41, 60, 84] {
        let found = whistled(&patch, key);
        println!("key {key}: {found:.3} Hz, {:+.2} cents from 440", cents(440.0, found));
        assert!(cents(440.0, found).abs() < 3.0, "{found}");
    }
}

#[test]
fn the_filter_scale_adjustment_holds_the_filter_tracks_the_keyboard() {
    // "Filter Scale": both keyboard control switches on; the third A from
    // the bottom is tuned by the dial to "two octaves above 440", and the
    // low A then gives zero beats.
    let patch = whistle(-133);
    let high = whistled(&patch, 69);
    let low = whistled(&patch, 45);
    println!("third A: {high:.2} Hz; low A: {low:.2} Hz; {:.2} cents apart", cents(low, high));
    assert!(cents(1_760.0, high).abs() < 8.0 && cents(440.0, low).abs() < 8.0);
    assert!((cents(low, high) - 2_400.0).abs() < 3.0);
    // "Repeat these adjustments until the filter will track three
    // octaves."
    let top = whistled(&patch, 81);
    assert!((cents(low, top) - 3_600.0).abs() < 5.0, "{}", cents(low, top));
}

#[test]
fn each_keyboard_control_switch_adds_its_share() {
    for (switches, share) in [([true, false], 1.0 / 3.0), ([false, true], 2.0 / 3.0)] {
        let mut patch = whistle(0);
        patch.keyboard_control = switches;
        let moved = cents(whistled(&patch, 45), whistled(&patch, 69));
        println!("{switches:?}: {moved:.2} cents over two octaves of keys");
        assert!((moved - 2_400.0 * share).abs() < 4.0);
    }
}

#[test]
fn a_division_of_the_dial_is_an_octave() {
    let mut patch = whistle(0);
    patch.keyboard_control = [false, false];
    for (dial, expected) in [(-300i16, 110.0), (-200, 220.0), (0, 880.0), (100, 1_760.0), (300, 7_040.0)] {
        patch.cutoff = dial;
        let found = whistled(&patch, 60);
        println!("dial {dial}: {found:.2} Hz, {:+.2} cents", cents(expected, found));
        assert!(cents(expected, found).abs() < 8.0, "{found}");
    }
}

#[test]
fn regeneration_starts_between_seven_and_eight_on_the_dial() {
    // "Regeneration should start when the EMPHASIS control is between 7
    // and 8."
    let level = |emphasis: u8| {
        let mut patch = whistle(-100);
        patch.emphasis = emphasis;
        let signal = play(&patch, 60, 6.0, 0.0);
        rms(&signal[samples(5.0)..])
    };
    let (below, above, top) = (level(70), level(80), level(100));
    println!("emphasis 7: {:.1} dB; 8: {:.1} dB; 10: {:.1} dB", decibels(below), decibels(above), decibels(top));
    assert!(below < 0.001 && above > 0.01 && top > above);
}

#[test]
fn the_oscillation_at_ten_is_nearly_a_sine() {
    // Owner's manual, page 13: "produces a pure sine wave tone".
    let mut patch = whistle(-100);
    patch.keyboard_control = [false, false];
    let signal = play(&patch, 60, 3.0, 0.0);
    let steady = &signal[samples(2.0)..];
    let fundamental = pitch(steady);
    let first = amplitude(steady, fundamental / RATE);
    let third = amplitude(steady, 3.0 * fundamental / RATE);
    println!("third harmonic {:.1} dB below the fundamental", decibels(first / third));
    assert!(first / third > 10.0);
}

#[test]
fn the_contour_at_ten_raises_the_cutoff_eight_and_a_half_octaves() {
    // A fifth of the amount: 1.7 octaves, with the sustain holding the
    // peak.
    let mut patch = whistle(-300);
    patch.keyboard_control = [false, false];
    let base = whistled(&patch, 60);
    patch.contour_amount = 20;
    let raised = whistled(&patch, 60);
    let expected = 1_200.0 * 0.2 * 400.0 / 47.0;
    println!("{:.1} cents for {expected:.1}", cents(base, raised));
    assert!((cents(base, raised) - expected).abs() < 10.0);
}

#[test]
fn the_slope_is_24_decibels_an_octave() {
    // A quiet sawtooth of 55 Hz through the filter at 440 Hz: its
    // harmonics against one another, by 1 / (1 + s)^4.
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].range = Range::Sixteen;
    patch.oscillators[0].volume = 10;
    patch.cutoff = -100;
    let signal = play(&patch, 45, 3.0, 0.0);
    let steady = &signal[samples(1.0)..];
    let harmonic = |number: f64| amplitude(steady, number * 55.0 / RATE);
    let response = |ratio: f64| -40.0 * (1.0 + ratio * ratio).log10();
    let sawtooth = |number: f64| -20.0 * number.log10();
    for number in [8.0, 16.0, 32.0] {
        let found = decibels(harmonic(number) / harmonic(1.0));
        let expected = sawtooth(number) + response(number / 8.0) - response(0.125);
        println!("harmonic {number}: {found:.2} dB for {expected:.2} dB");
        assert!((found - expected).abs() < 1.0);
    }
}

#[test]
fn the_emphasis_lowers_what_passes_below_the_cutoff() {
    // 1 / (1 + k): at 3.75 on the dial the feedback is two, and the low
    // frequencies are 9.5 dB down.
    let mut patch = triangle();
    patch.oscillators[0].volume = 10;
    let level = |patch: &Patch| rms(&play(patch, 45, 1.0, 0.0)[samples(0.5)..]);
    let open = level(&patch);
    patch.emphasis = 37;
    patch.cutoff = 500;
    let fed = decibels(level(&patch) / open);
    let expected = decibels(1.0 / (1.0 + 4.0 * 37.0 / 75.0));
    println!("{fed:.2} dB for {expected:.2} dB");
    assert!((fed - expected).abs() < 0.5);
}
