//! What is left of the aliasing when oscillator B moves oscillator A's
//! frequency, its pulse width or the cutoff through Poly-Mod at audio
//! rate: a measurement, against the 80 dB this crate asks of its
//! oscillators (`src/analog/oscillator.rs`).
//!
//! Oscillator A is synchronised to oscillator B, and only A is heard, so
//! everything the instrument would make is a harmonic of B's pitch:
//! whatever stands between the harmonics is the emulation's own. It is
//! read as the oscillators' test reads it, the strongest tone below
//! 20 kHz that is no harmonic against the strongest harmonic.

#![cfg(feature = "prophet5")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod prophet5_support;

use neoteletext_audio_instruments::prophet5::{Knob, Model, Patch, Switch};
use prophet5_support::{MODELS, NOTE_ON, RATE, instrument, pitch, plain, render};

/// Samples of the spectrum's window: 1.4 s, bins of 0.73 Hz.
const WINDOW: usize = 1 << 16;
/// Bins either side of a harmonic that are the harmonic's: the window's
/// main lobe is four either side, and the pitch is known to a bin at the
/// highest harmonic.
const LOBE: f64 = 6.0;
/// The highest frequency a listener hears, in hertz.
const AUDIBLE: f64 = 20_000.0;

/// The magnitudes of a signal's spectrum under a Blackman-Harris window,
/// whose side lobes are 92 dB down, by a radix-2 transform.
fn spectrum(signal: &[f64]) -> Vec<f64> {
    let turn = |index: usize| std::f64::consts::TAU * index as f64 / WINDOW as f64;
    let window = |index: usize| {
        0.358_75 - 0.488_29 * turn(index).cos() + 0.141_28 * turn(2 * index).cos() - 0.011_68 * turn(3 * index).cos()
    };
    let mut real: Vec<f64> = signal[..WINDOW].iter().enumerate().map(|(index, &value)| value * window(index)).collect();
    let mut imaginary = vec![0.0f64; WINDOW];
    let bits = WINDOW.trailing_zeros();
    for index in 0..WINDOW {
        let reversed = index.reverse_bits() >> (usize::BITS - bits);
        if index < reversed {
            real.swap(index, reversed);
        }
    }
    let mut length = 2;
    while length <= WINDOW {
        for start in (0..WINDOW).step_by(length) {
            for offset in 0..length / 2 {
                let (sine, cosine) = (-std::f64::consts::TAU * offset as f64 / length as f64).sin_cos();
                let (low, high) = (start + offset, start + offset + length / 2);
                let (re, im) =
                    (real[high] * cosine - imaginary[high] * sine, real[high] * sine + imaginary[high] * cosine);
                (real[high], imaginary[high]) = (real[low] - re, imaginary[low] - im);
                (real[low], imaginary[low]) = (real[low] + re, imaginary[low] + im);
            }
        }
        length *= 2;
    }
    real.iter().zip(&imaginary).take(WINDOW / 2).map(|(re, im)| re.hypot(*im)).collect()
}

/// The strongest tone below 20 kHz that is no harmonic of `fundamental`,
/// in decibels against the strongest harmonic, and its frequency.
fn between_the_harmonics(signal: &[f64], fundamental: f64) -> (f64, f64) {
    let magnitudes = spectrum(signal);
    let bin = RATE / WINDOW as f64;
    let strongest = magnitudes.iter().fold(0.0f64, |most, &magnitude| most.max(magnitude));
    let (mut worst, mut worst_at) = (0.0f64, 0.0f64);
    for (index, &magnitude) in magnitudes.iter().enumerate().skip(8) {
        let hertz = index as f64 * bin;
        if hertz > AUDIBLE {
            break;
        }
        let harmonic = (hertz / fundamental).round();
        if (hertz - harmonic * fundamental).abs() > LOBE * bin && magnitude > worst {
            (worst, worst_at) = (magnitude, hertz);
        }
    }
    (20.0 * (worst / strongest).log10(), worst_at)
}

/// Oscillator A alone, a twelfth above oscillator B and synchronised to
/// it, with oscillator B's sawtooth as Poly-Mod's source.
fn synchronised(model: Model) -> Patch {
    plain(model)
        .with(Knob::OscAFrequency, model.frequency_knob(24 + 19))
        .with_switch(Switch::Sync, true)
        .with_switch(Switch::OscBSawtooth, true)
}

/// What stands between the harmonics of a patch's key, in decibels: the
/// harmonics of oscillator B's pitch, which is read from oscillator B
/// alone through an open filter.
fn measured(model: Model, patch: &Patch, key: u8) -> f64 {
    let alone = patch
        .with(Knob::MixerOscA, 0)
        .with(Knob::MixerOscB, 120)
        .with(Knob::PolyModOscB, 0)
        .with(Knob::Cutoff, 127)
        .with(Knob::Resonance, 0);
    let mut prophet = instrument(model, &alone);
    prophet.event(NOTE_ON, key, 100);
    render(&mut prophet, 0.5);
    let fundamental = pitch(&render(&mut prophet, 2.0));
    let mut prophet = instrument(model, patch);
    prophet.event(NOTE_ON, key, 100);
    render(&mut prophet, 0.5);
    between_the_harmonics(&render(&mut prophet, 1.5), fundamental).0
}

#[test]
#[ignore = "a measurement, which needs a release build: cargo test --release -- --ignored --nocapture aliasing"]
fn aliasing_under_poly_mod_at_audio_rate() {
    const KEYS: [u8; 5] = [48, 60, 72, 84, 96];
    for model in MODELS {
        // Without Poly-Mod the instrument is as clean as its oscillators,
        // which is what shows that this measurement reads aliasing.
        for key in KEYS {
            let clean = measured(model, &synchronised(model), key);
            assert!(clean < -80.0, "{model:?} key {key}: {clean}");
        }
        let pulse = |patch: Patch| patch.with_switch(Switch::OscAPulse, true).with_switch(Switch::OscASawtooth, false);
        let destinations = [
            ("oscillator A's frequency", synchronised(model).with_switch(Switch::PolyModFrequencyA, true)),
            ("oscillator A's pulse width", pulse(synchronised(model)).with_switch(Switch::PolyModPulseWidthA, true)),
            // The cutoff knob at 6, no resonance.
            ("the cutoff", synchronised(model).with(Knob::Cutoff, 72).with_switch(Switch::PolyModFilter, true)),
        ];
        for (name, patch) in destinations {
            for amount in [30u8, 60, 120] {
                let found: Vec<String> = KEYS
                    .iter()
                    .map(|&key| format!("{:.0}", measured(model, &patch.with(Knob::PolyModOscB, amount), key)))
                    .collect();
                println!("{model:?}: oscillator B to {name}, amount {amount}: keys {KEYS:?}: {} dB", found.join(", "));
            }
        }
    }
}
