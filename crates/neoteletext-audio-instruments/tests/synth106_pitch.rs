//! synth106's pitch: the clock over a whole number, on every key and in
//! every range, and the bender (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth106")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "synth106_support.rs"]
mod support;

use neoteletext_audio_instruments::synth106::{Model, PITCH_BEND_SEMITONES, Range};
use support::{MODELS, NOTE_ON, PITCH_BEND, cents, flat, instrument, pitch, play, render, square};

/// The lowest and highest key of the instruments' keyboards.
const KEYBOARD: (u8, u8) = (36, 96);

/// The octaves a range moves the pitch.
fn octaves(range: Range) -> f64 {
    match range {
        Range::Sixteen => -1.0,
        Range::Eight => 0.0,
        Range::Four => 1.0,
    }
}

/// What the instrument's divider gives for a key, from the service
/// notes' figures: synth106's 8 MHz over 8, 4 or 2 and the nearest
/// divisor of the key at 8'; synth60's 4,305 * 440 Hz over the
/// nearest divisor of the key in its range.
fn divided(model: Model, key: u8, range: Range) -> f64 {
    let equal = |shift: f64| 440.0 * ((f64::from(key) - 69.0) / 12.0 + shift).exp2();
    match model {
        Model::Synth106 => {
            let divisor = (2.0e6 / equal(0.0)).round().clamp(1.0, 65_535.0);
            2.0e6 * octaves(range).exp2() / divisor
        }
        Model::Synth60 => {
            let master = 4_305.0 * 440.0;
            master / (master / equal(octaves(range))).round().clamp(1.0, 65_535.0)
        }
    }
}

#[test]
fn every_key_is_the_clock_over_its_divisor_and_within_a_cent_of_the_scale() {
    for model in MODELS {
        for range in [Range::Sixteen, Range::Eight, Range::Four] {
            let mut patch = flat(model, square());
            patch.range = range;
            let (mut worst_divided, mut worst_equal) = (0.0f64, 0.0f64);
            for key in (KEYBOARD.0..=KEYBOARD.1).step_by(5).chain([KEYBOARD.1]) {
                let signal = play(model, &patch, key, 1.3, 0.0);
                let found = pitch(&signal[support::samples(0.2)..]);
                let from_divided = cents(divided(model, key, range), found);
                let equal = 440.0 * ((f64::from(key) - 69.0) / 12.0 + octaves(range)).exp2();
                assert!(from_divided.abs() < 0.1, "{model:?} {range:?} key {key}: {found} Hz, {from_divided} cents");
                worst_divided = worst_divided.max(from_divided.abs());
                worst_equal = worst_equal.max(cents(equal, found).abs());
            }
            println!("{model:?} {range:?}: {worst_divided:.3} cents from the divider, {worst_equal:.2} from the scale");
            // synth60's divisors at 4' are half those at 8': the
            // top octave is within two cents, as its divider is.
            let bound = if (model, range) == (Model::Synth60, Range::Four) { 2.0 } else { 1.0 };
            assert!(worst_equal < bound, "{model:?} {range:?}: {worst_equal} cents");
        }
    }
}

#[test]
fn the_bender_at_either_end_bends_by_its_semitones() {
    for model in MODELS {
        let patch = flat(model, square());
        let centre = pitch(&play(model, &patch, 69, 1.0, 0.0)[support::samples(0.2)..]);
        for (low, high, sign) in [(0x7f, 0x7f, 1.0), (0, 0, -1.0)] {
            let mut synth106 = instrument(model, &patch);
            synth106.event(PITCH_BEND, low, high);
            synth106.event(NOTE_ON, 69, 100);
            let found = pitch(&render(&mut synth106, 1.0)[support::samples(0.2)..]);
            let bent = cents(centre, found);
            // synth106's bend is in its divisor: within a cent of it.
            assert!((bent - sign * 100.0 * f64::from(PITCH_BEND_SEMITONES)).abs() < 1.5, "{model:?}: {bent} cents");
        }
    }
}

#[test]
fn the_range_switch_moves_a_note_that_is_sounding() {
    // A patch takes effect at once on the voices that are sounding, as
    // moving the panel's controls does: a key held while the range goes
    // from 8' to 4' and to 16' sounds an octave up and an octave down
    // (TODO-6438: synth60's kept the divisor its key was struck with).
    for model in MODELS {
        let patch = flat(model, square());
        let mut synth106 = instrument(model, &patch);
        synth106.event(NOTE_ON, 69, 100);
        let eight = pitch(&render(&mut synth106, 0.7)[support::samples(0.2)..]);
        for (range, octaves) in [(Range::Four, 1.0), (Range::Sixteen, -1.0)] {
            let mut moved = patch;
            moved.range = range;
            synth106.set_patch(&moved);
            let found = pitch(&render(&mut synth106, 0.7)[support::samples(0.2)..]);
            let off = cents(eight * f64::exp2(octaves), found);
            println!("{model:?}: held through a change to {range:?}: {found:.2} Hz from {eight:.2} Hz");
            assert!(off.abs() < 2.0, "{model:?} {range:?}: {found} Hz from {eight} Hz");
        }
    }
}
