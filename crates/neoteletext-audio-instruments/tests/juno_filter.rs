//! The Juno's low-pass filter: where its cutoff lies for the slider, the
//! key and the envelope, its slope, and its resonance up to oscillation;
//! and the high-pass switch (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
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
#[path = "juno_support.rs"]
mod support;

use analog::{amplitude, decibels};
use neoteletext_audio_instruments::juno::{Model, Patch, Slider};
use support::{MODELS, RATE, cents, flat, pitch, play, rms, samples, square, whistle};

/// Each voice's filter may be off by so many cents (JUNO-106 service
/// notes, adjustments 7 and 8), and the oscillation's pitch is trimmed
/// to within a few more.
const TOLERANCE: f64 = 16.0;

/// The frequency the filter oscillates at for a patch and a key, once it
/// has grown to its full size: while it grows, the loop's poles lie 36
/// cents above where its limiting then holds it.
fn oscillation(model: Model, patch: &Patch, key: u8) -> f64 {
    pitch(&play(model, &flat(model, *patch), key, 5.0, 0.0)[samples(4.0)..])
}

/// The cutoff the documents give for a position of the slider at middle
/// C: the Juno-106's 5 Hz to 50 kHz across the slider; the Juno-60's 248
/// Hz at 3 of 10 and 17.2 octaves across it, held to the chip's 40 kHz.
fn documented(model: Model, slider: u8) -> f64 {
    let travel = f64::from(slider) / 127.0;
    match model {
        Model::Juno106 => 5.0 * 10_000f64.powf(travel),
        Model::Juno60 => (248.0 * (17.2 * (travel - 0.3)).exp2()).min(40_000.0),
    }
}

#[test]
fn the_oscillation_is_at_the_cutoff_the_documents_give() {
    for model in MODELS {
        for slider in [32u8, 38, 48, 64, 80] {
            let found = oscillation(model, &whistle(slider), 60);
            let off = cents(documented(model, slider), found);
            println!(
                "{model:?}: cutoff slider {slider}: oscillates at {found:.1} Hz, {off:.1} cents from the documents"
            );
            assert!(off.abs() < TOLERANCE, "{model:?} at {slider}: {found} Hz");
        }
    }
    // The Juno-60's calibration point itself: 3 of 10 is 248 Hz.
    let found = oscillation(Model::Juno60, &whistle(38), 60);
    assert!(cents(248.0 * (17.2f64 * (38.0 / 127.0 - 0.3)).exp2(), found).abs() < TOLERANCE);
}

#[test]
fn the_cutoff_follows_the_keyboard_about_middle_c_and_the_envelope() {
    for model in MODELS {
        let slider = 64;
        let middle = oscillation(model, &whistle(slider), 60);
        // With the key follow at its top, two octaves up the keyboard
        // are two octaves of cutoff (the service notes trim C4 to 248 Hz
        // and C6 to 992 Hz); at its foot the key does nothing.
        let follow = whistle(slider).with(Slider::VcfKeyboard, 127);
        for (key, octaves) in [(36u8, -2.0), (60, 0.0), (84, 2.0)] {
            let off = cents(middle * f64::exp2(octaves), oscillation(model, &follow, key));
            assert!(off.abs() < 3.0, "{model:?} key {key}: {off} cents");
        }
        assert!(cents(middle, oscillation(model, &whistle(slider), 84)).abs() < 1.0);
        let half = cents(middle, oscillation(model, &whistle(slider).with(Slider::VcfKeyboard, 64), 84));
        assert!((half - 2400.0 * 64.0 / 127.0).abs() < 3.0, "{model:?}: {half}");

        // The envelope at its peak and sustain, with its slider at a
        // quarter, moves the cutoff a quarter of its whole depth: 14
        // octaves on the Juno-106, 12.08 on the Juno-60; upside down, it
        // moves it down as far.
        let depth = if model == Model::Juno106 { 14.0 } else { 12.08 };
        let up = whistle(slider - 16).with(Slider::VcfEnvelope, 32);
        let moved = cents(oscillation(model, &whistle(slider - 16), 60), oscillation(model, &up, 60)) / 1200.0;
        let mut down = whistle(slider + 16).with(Slider::VcfEnvelope, 32);
        down.envelope_inverted = true;
        let lowered = cents(oscillation(model, &whistle(slider + 16), 60), oscillation(model, &down, 60)) / 1200.0;
        println!("{model:?}: the envelope at a quarter moves the cutoff {moved:.3} octaves up, {lowered:.3} down");
        assert!((moved - depth * 32.0 / 127.0).abs() < 0.02 && (lowered + depth * 32.0 / 127.0).abs() < 0.02);
    }
}

#[test]
fn the_resonance_rises_to_oscillation_as_large_as_a_square_wave() {
    for model in MODELS {
        // No oscillation below the last tenth of the slider.
        let quiet = play(model, &flat(model, whistle(64).with(Slider::VcfResonance, 110)), 60, 1.5, 0.0);
        assert!(rms(&quiet[samples(1.0)..]) < 1.0e-3, "{model:?}: oscillates at 110");
        let loud = play(model, &flat(model, whistle(64)), 60, 2.0, 0.0);
        let wave = play(model, &flat(model, square()), 60, 1.0, 0.0);
        let (whistle, wave) = (&loud[samples(1.5)..], &wave[samples(0.5)..]);
        let swing = |signal: &[f64]| signal.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
        // The square's swing without its edges' overshoot: its mean
        // magnitude.
        let flat_top = wave.iter().map(|value| value.abs()).sum::<f64>() / wave.len() as f64;
        let ratio = decibels(swing(whistle) / flat_top);
        println!("{model:?}: the oscillation at the top swings {ratio:.1} dB against a square wave");
        // JUNO-60 service notes, adjustments 5-1 and 8-1: both 4 V peak
        // to peak.
        assert!(ratio.abs() < 1.5, "{model:?}: {ratio} dB");
        // And it is a sine: its second and third harmonics are far down.
        let tone = pitch(whistle) / RATE;
        let harmonics =
            decibels(amplitude(whistle, 2.0 * tone).max(amplitude(whistle, 3.0 * tone)) / amplitude(whistle, tone));
        assert!(harmonics < -30.0, "{model:?}: harmonics {harmonics} dB");
    }
}

#[test]
fn the_filter_falls_24_db_an_octave_and_resonance_lifts_the_cutoff() {
    for model in MODELS {
        // A sawtooth at 110 Hz: its 16th and 32nd harmonics, an octave
        // apart, stand 6 dB apart. Through the filter with its cutoff
        // near 440 Hz they are two and three octaves above it.
        let slider =
            (0..=127u8).min_by_key(|&slider| cents(440.0, documented(model, slider)).abs() as i64).unwrap_or(0);
        let saw = |cutoff: u8, resonance: u8| {
            let patch = Patch::DEFAULT.with(Slider::VcfFrequency, cutoff).with(Slider::VcfResonance, resonance);
            let mut patch = flat(model, patch);
            patch.vca_gate = true;
            play(model, &patch, 45, 1.5, 0.0).split_off(samples(0.5))
        };
        let tone = pitch(&saw(127, 0)) / RATE;
        let saw = |cutoff: u8, resonance: u8| {
            let signal = saw(cutoff, resonance);
            move |harmonic: f64| amplitude(&signal, harmonic * tone)
        };
        let (open, closed) = (saw(127, 0), saw(slider, 0));
        let slope = decibels(closed(32.0) / closed(16.0)) - decibels(open(32.0) / open(16.0));
        println!("{model:?}: an octave further above the cutoff is {slope:.1} dB lower");
        // Four poles two and three octaves above their cutoff differ by
        // 23.3 dB, and by 24 only further up.
        assert!((-25.5..-21.5).contains(&slope), "{model:?}: {slope} dB");
        // Resonance below oscillation lifts what lies at the cutoff over
        // what lies below it.
        let resonant = saw(slider, 100);
        let lift = decibels(resonant(4.0) / resonant(1.0)) - decibels(closed(4.0) / closed(1.0));
        println!("{model:?}: resonance at 100 lifts the cutoff {lift:.1} dB over the fundamental");
        assert!(lift > 9.0, "{model:?}: {lift} dB");
    }
}

#[test]
fn the_high_pass_switch_cuts_or_lifts_the_bass() {
    // The fundamental of a square wave at 55 Hz against its level with
    // the switch where the filter is flat.
    let fundamental = |model: Model, position: u8| {
        let mut patch = square();
        patch.hpf = position;
        let signal = play(model, &patch, 33, 1.5, 0.0);
        let tone = pitch(&signal[samples(0.5)..]) / RATE;
        amplitude(&signal[samples(0.5)..], tone)
    };
    // JUNO-106: 0 lifts the bass (100 / 47 of what lies below 72 Hz is
    // added: 8.2 dB at 55 Hz), 1 is flat, 2 and 3 are one-pole high-pass
    // filters at 225.8 Hz and 720.5 Hz, which leave 55 Hz 12.5 dB and
    // 22.4 dB down.
    let flat = fundamental(Model::Juno106, 1);
    let levels = [0u8, 2, 3].map(|position| decibels(fundamental(Model::Juno106, position) / flat));
    println!("Juno106: 55 Hz at positions 0, 2 and 3 against 1: {levels:.1?} dB");
    assert!((levels[0] - 8.2).abs() < 0.5 && (levels[1] + 12.5).abs() < 0.5 && (levels[2] + 22.4).abs() < 0.5);
    // JUNO-60: 0 is flat, and each position above cuts more.
    let flat = fundamental(Model::Juno60, 0);
    let levels = [1u8, 2, 3].map(|position| decibels(fundamental(Model::Juno60, position) / flat));
    println!("Juno60: 55 Hz at positions 1, 2 and 3 against 0: {levels:.1?} dB");
    assert!(levels[0] < -10.0 && levels[1] < levels[0] - 3.0 && levels[2] < levels[1] - 3.0);
}
