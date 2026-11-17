//! The Minimoog's pitch: every key in every range against the service
//! manual's tuning, the knobs and the wheel that move it, and oscillator
//! 3 off the keyboard (TODO-6432).

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

use neoteletext_audio_instruments::minimoog::{Patch, Range, Waveform, law};
use support::{NOTE_ON, PITCH_BEND, cents, instrument, pitch, play, render, samples, triangle};

/// The pitch of a key at 8' by the service manual's tuning: the low A,
/// key 45, is 440 Hz at 2' and so 110 Hz at 8'.
fn tuned(key: u8) -> f64 {
    110.0 * ((f64::from(key) - 45.0) / 12.0).exp2()
}

/// The octaves each range lies from 8'.
const RANGES: [(Range, f64); 6] = [
    (Range::Lo, -6.75),
    (Range::ThirtyTwo, -2.0),
    (Range::Sixteen, -1.0),
    (Range::Eight, 0.0),
    (Range::Four, 1.0),
    (Range::Two, 2.0),
];

/// The steady pitch of one key of a patch.
fn sounded(patch: &Patch, key: u8, seconds: f64) -> f64 {
    let signal = play(patch, key, seconds, 0.0);
    pitch(&signal[samples(0.1)..])
}

#[test]
fn every_key_in_every_audible_range_is_within_a_cent_of_the_manuals_tuning() {
    let mut worst = 0.0f64;
    for (range, octaves) in &RANGES[1..] {
        let mut patch = triangle();
        patch.oscillators[0].range = *range;
        for key in (law::KEYBOARD.0..=law::KEYBOARD.1).step_by(6).chain([law::KEYBOARD.1]) {
            let expected = tuned(key) * octaves.exp2();
            // The lowest notes need longer for as many cycles.
            let found = sounded(&patch, key, if expected < 60.0 { 4.0 } else { 1.2 });
            let error = cents(expected, found);
            println!("{range:?} key {key}: {found:.4} Hz for {expected:.4} Hz, {error:+.3} cents");
            worst = worst.max(error.abs());
        }
    }
    assert!(worst < 1.0, "{worst} cents");
}

#[test]
fn the_tuning_procedures_two_notes_are_440_and_3520_hertz_at_two_feet() {
    // Folkman, section VII: at 2', "3520 Hz while holding high \"A\"" and
    // the low A "for 440 Hz".
    let mut patch = triangle();
    patch.oscillators[0].range = Range::Two;
    for (key, expected) in [(45, 440.0), (81, 3_520.0)] {
        let found = sounded(&patch, key, 1.2);
        println!("key {key}: {found:.3} Hz");
        assert!(cents(expected, found).abs() < 1.0, "{found}");
    }
}

#[test]
fn lo_is_four_and_three_quarter_octaves_below_32_feet() {
    let mut patch = triangle();
    patch.oscillators[0].range = Range::Lo;
    let expected = tuned(84) * (-6.75f64).exp2();
    let found = sounded(&patch, 84, 12.0);
    println!("LO, high C: {found:.4} Hz for {expected:.4} Hz");
    assert!(cents(expected, found).abs() < 2.0, "{found}");
}

#[test]
fn the_knobs_and_the_wheel_move_the_pitch_as_the_schematic_says() {
    let key = 57;
    let base = sounded(&triangle(), key, 1.2);

    // TUNE: 242 cents either way.
    for tune in [-242i16, 100, 242] {
        let mut patch = triangle();
        patch.tune = tune;
        let moved = cents(base, sounded(&patch, key, 1.2));
        println!("tune {tune}: {moved:+.2} cents");
        assert!((moved - f64::from(tune)).abs() < 1.0);
    }

    // Oscillator 2's FREQUENCY: seven semitones either way; oscillator 1
    // is not moved by it.
    for frequency in [-700i16, 3, 700] {
        let mut patch = triangle();
        patch.oscillators[0].on = false;
        patch.oscillators[1].on = true;
        patch.oscillators[1].waveform = Waveform::Triangle;
        patch.oscillators[1].frequency = frequency;
        let moved = cents(base, sounded(&patch, key, 1.2));
        println!("oscillator 2 frequency {frequency}: {moved:+.2} cents");
        assert!((moved - f64::from(frequency)).abs() < 1.0);
    }

    // The pitch wheel: 2 V * 100 / 301 octaves either way, 797 cents.
    let wheel = 1200.0 * 200.0 / 301.0;
    for (low, high, expected) in [(0x7f, 0x7f, wheel * 8191.0 / 8192.0), (0, 0, -wheel), (0, 0x50, wheel / 4.0)] {
        let mut minimoog = instrument(&triangle());
        minimoog.event(PITCH_BEND, low, high);
        minimoog.event(NOTE_ON, key, 100);
        let signal = render(&mut minimoog, 1.2);
        let moved = cents(base, pitch(&signal[samples(0.1)..]));
        println!("wheel {high:02x}{low:02x}: {moved:+.2} cents for {expected:+.2}");
        assert!((moved - expected).abs() < 1.0);
    }
}

#[test]
fn oscillator_3_off_the_keyboard_keeps_its_pitch_and_its_knob_covers_five_and_a_half_octaves() {
    let mut patch = triangle();
    patch.oscillators[0].on = false;
    patch.oscillators[2].on = true;
    patch.oscillators[2].range = Range::Eight;
    patch.oscillator_3_control = false;
    let middle = tuned(law::FREE_KEY);
    for (frequency, octaves) in [(0i16, 0.0), (700, 2.75), (-700, -2.75), (350, 1.375)] {
        patch.oscillators[2].frequency = frequency;
        let expected = middle * f64::exp2(octaves);
        for key in [45, 81] {
            let found = sounded(&patch, key, 2.0);
            println!("frequency {frequency}, key {key}: {found:.3} Hz for {expected:.3} Hz");
            assert!(cents(expected, found).abs() < 1.0);
        }
    }
    // On the keyboard it follows the keys as the others do.
    patch.oscillator_3_control = true;
    patch.oscillators[2].frequency = 0;
    assert!(cents(tuned(81), sounded(&patch, 81, 1.2)).abs() < 1.0);
}
