//! The Juno's pulse: its width across the whole of the slider, by hand,
//! under the low-frequency oscillator and under the envelope (TODO-6438).

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

#[path = "juno_support.rs"]
mod support;

use neoteletext_audio_instruments::juno::{Model, Patch, PwmSource, Slider};
use support::{MODELS, flat, play, samples, square};

/// The key played: low, so that a cycle is hundreds of samples and its
/// high share is read to a fraction of a per cent.
const KEY: u8 = 36;

/// The share of a signal's samples that lie above the middle of its
/// swing: a pulse's duty cycle.
fn duty(signal: &[f64]) -> f64 {
    let (low, high) =
        signal.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &v| (low.min(v), high.max(v)));
    let middle = f64::midpoint(low, high);
    signal.iter().filter(|&&value| value > middle).count() as f64 / signal.len() as f64
}

/// The duty cycle of the pulse of a patch, over `seconds` after the note
/// has settled.
fn pulse_duty(model: Model, patch: &Patch, seconds: f64) -> f64 {
    let signal = play(model, &flat(model, *patch), KEY, 0.5 + seconds, 0.0);
    duty(&signal[samples(0.5)..])
}

#[test]
fn the_pulse_widens_by_hand_from_a_square_to_95_per_cent_at_the_sliders_top() {
    // JUNO-106 service notes, adjustment 10: "50% duty cycle" at PWM 0
    // and "93 - 97%" at 10; JUNO-60 service notes, adjustments 9-1 and
    // 9-2: 50% and 95%.
    for model in MODELS {
        let width = |slider: u8| pulse_duty(model, &square().with(Slider::DcoPwm, slider), 1.0);
        let found = [0u8, 64, 126, 127].map(width);
        println!("{model:?}: the pulse's duty at 0, 64, 126 and 127 by hand: {found:.4?}");
        let expected = [0u8, 64, 126, 127].map(|slider| 0.5 + 0.45 * f64::from(slider) / 127.0);
        for (found, expected) in found.iter().zip(expected) {
            assert!((found - expected).abs() < 0.004, "{model:?}: {found} for {expected}");
        }
    }
}

#[test]
fn the_juno_60s_envelope_at_its_peak_holds_the_pulse_at_the_sliders_width() {
    // The envelope held at its peak by a sustain of 10 moves the width
    // as far as the slider says: at the slider's top, to 95%.
    let mut patch = square().with(Slider::DcoPwm, 127).with(Slider::Sustain, 127);
    patch.pwm = PwmSource::Envelope;
    let found = pulse_duty(Model::Juno60, &patch, 1.0);
    println!("Juno60: the pulse's duty under the envelope at its peak: {found:.4}");
    assert!((found - 0.95).abs() < 0.004, "{found}");
}

#[test]
fn the_oscillator_sweeps_the_pulse_between_a_square_and_the_sliders_width() {
    // The duty in each tenth of a second, some thirteen cycles of the
    // tone, under the slowest sweep: one cycle of the Juno-106's 0.1 Hz,
    // three of the Juno-60's 0.3 Hz. It runs from a square to the
    // slider's width and no further either way.
    for model in MODELS {
        let mut patch = square().with(Slider::DcoPwm, 127);
        patch.pwm = PwmSource::Lfo;
        let signal = play(model, &flat(model, patch), 48, 10.5, 0.0);
        let duties: Vec<f64> = signal[samples(0.5)..].chunks_exact(samples(0.1)).map(duty).collect();
        let (narrowest, widest) =
            duties.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &duty| (low.min(duty), high.max(duty)));
        println!("{model:?}: under the oscillator the pulse's duty runs from {narrowest:.3} to {widest:.3}");
        assert!((narrowest - 0.5).abs() < 0.03 && (widest - 0.95).abs() < 0.03, "{model:?}: {narrowest} to {widest}");
    }
}

#[test]
fn a_wide_pulses_steady_part_does_not_stand_in_the_output() {
    // JUNO-106 service notes, jack board schematic: the voices' sum
    // passes 10 µF across 33 kΩ, a time constant of 0.33 s. A pulse of
    // 95% has a mean of nine tenths of its height; it is let through at
    // first, 37% of it is left after one time constant, and none in the
    // end. Without the capacitor it stood at 0.111 of full scale for as
    // long as the note (TODO-6438).
    for model in MODELS {
        let signal = play(model, &flat(model, square().with(Slider::DcoPwm, 127)), 60, 3.0, 0.0);
        let mean = |from: f64, to: f64| {
            let part = &signal[samples(from)..samples(to)];
            part.iter().sum::<f64>() / part.len() as f64
        };
        let (first, later, last) = (mean(0.01, 0.03), mean(0.31, 0.35), mean(2.5, 3.0));
        println!(
            "{model:?}: a 95% pulse's mean is {first:.4} at first, {later:.4} after 0.33 s and {last:.5} in the end"
        );
        assert!(first > 0.09, "{model:?}: {first}");
        assert!((later / first - (-1.0f64).exp() / (-0.06f64).exp()).abs() < 0.04, "{model:?}: {later} after {first}");
        assert!(last.abs() < 5.0e-4, "{model:?}: {last}");
    }
}
