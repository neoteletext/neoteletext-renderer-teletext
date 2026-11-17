//! What is left of the aliasing when oscillator B moves oscillator A's
//! frequency, its pulse width or the cutoff through Poly-Mod at audio
//! rate, against the 80 dB this crate asks of its oscillators
//! (`src/analog/oscillator.rs`): held to what was measured.
//!
//! Oscillator A is synchronised to oscillator B, and only A is heard, so
//! everything the instrument would make is a harmonic of B's pitch:
//! whatever stands between the harmonics is the emulation's own. It is
//! read as the oscillators' test reads it, the strongest tone below
//! 20 kHz that is no harmonic against the strongest harmonic.

#![cfg(feature = "synth53")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod synth53_support;

use neoteletext_audio_instruments::synth53::{Knob, Model, Patch, Switch};
use synth53_support::{MODELS, NOTE_ON, RATE, instrument, pitch, plain, render};

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
    let mut synth53 = instrument(model, &alone);
    synth53.event(NOTE_ON, key, 100);
    render(&mut synth53, 0.5);
    let fundamental = pitch(&render(&mut synth53, 2.0));
    let mut synth53 = instrument(model, patch);
    synth53.event(NOTE_ON, key, 100);
    render(&mut synth53, 0.5);
    between_the_harmonics(&render(&mut synth53, 1.5), fundamental).0
}

/// The keys measured: the five Cs from 131 Hz to 2,093 Hz.
const KEYS: [u8; 5] = [48, 60, 72, 84, 96];
/// Poly-Mod's amount from oscillator B: a quarter, a half and all of it.
const AMOUNTS: [u8; 3] = [30, 60, 120];

/// What oscillator A's frequency under Poly-Mod is held below, at every
/// amount and on every key, in decibels: the worst measured, -91 dB,
/// and 3 dB. With one frequency for each sample, taken from oscillator
/// B's band-limited output, the worst was -47 dB.
const FREQUENCY: f64 = -88.0;
/// The same for oscillator A's pulse width: at a quarter and a half of
/// the amount (the worst measured -92 dB) and at all of it. There, on
/// revision 3, the width is at the end of its range for most of every
/// cycle and little of the pulse is left to measure against: -80 dB as
/// the levels stand, and -74 dB with the levels of the triangle and the
/// envelopes that another change of this round gives revision 3. With
/// one width for each sample the worst was -32 dB.
const PULSE_WIDTH: [f64; 3] = [-88.0, -88.0, -71.0];
/// The same for oscillator A's frequency and pulse width together, from
/// oscillator B's triangle: the worst measured, -90 dB, and 3 dB. On
/// revision 3 at all of the amount the width leaves the foot of its
/// range just after each synchronised start and runs beside the phase,
/// so that where the pulse rises hangs on where the width left its end;
/// while a stretch took the width in one straight line through that
/// corner, the two keys from 1,047 Hz up measured -76 and -56 dB.
const TOGETHER: f64 = -87.0;
/// The same for the cutoff, for each amount and each key: the worse of
/// the two revisions as measured, and 3 dB. With the cutoff set from
/// oscillator B's band-limited output these were -75 to -34 dB at a
/// quarter, -59 to -25 dB at a half and -31 to -22 dB at all of it.
const CUTOFF: [[f64; 5]; 3] = [
    // Measured: -94, -92, -85, -75, -53.
    [-90.0, -88.0, -82.0, -72.0, -50.0],
    // Measured: -94, -90, -73, -56, -52.
    [-90.0, -87.0, -70.0, -53.0, -49.0],
    // Measured: -74, -65, -59, -53, -54.
    [-71.0, -62.0, -56.0, -50.0, -51.0],
];

/// What a figure is held below, by the amount's and the key's place.
type Bound<'a> = &'a dyn Fn(usize, usize) -> Option<f64>;

#[test]
fn aliasing_under_poly_mod_at_audio_rate() {
    // An unoptimised build measures one amount on two keys, a release
    // build everything.
    let quick = cfg!(debug_assertions);
    for model in MODELS {
        // Without Poly-Mod the instrument is as clean as its oscillators,
        // which is what shows that this measurement reads aliasing.
        for key in KEYS.iter().copied().filter(|&key| !quick || key == 72) {
            let clean = measured(model, &synchronised(model), key);
            assert!(clean < -80.0, "{model:?} key {key}: {clean}");
        }
        let pulse = |patch: Patch| patch.with_switch(Switch::OscAPulse, true).with_switch(Switch::OscASawtooth, false);
        // Oscillator B's triangle in place of its sawtooth: it jumps
        // nowhere.
        let triangle =
            |patch: Patch| patch.with_switch(Switch::OscBSawtooth, false).with_switch(Switch::OscBTriangle, true);
        let frequency = synchronised(model).with_switch(Switch::PolyModFrequencyA, true);
        let width = pulse(synchronised(model)).with_switch(Switch::PolyModPulseWidthA, true);
        // The cutoff knob at 6, no resonance.
        let cutoff = synchronised(model).with(Knob::Cutoff, 72).with_switch(Switch::PolyModFilter, true);
        let bound = |row: usize, column: usize| CUTOFF.get(row).and_then(|row| row.get(column)).copied();
        let together = pulse(frequency).with_switch(Switch::PolyModPulseWidthA, true);
        let destinations: [(&str, Patch, Bound); 6] = [
            ("sawtooth to oscillator A's frequency", frequency, &|_, _| Some(FREQUENCY)),
            ("triangle to oscillator A's frequency", triangle(frequency), &|_, _| Some(FREQUENCY)),
            ("sawtooth to oscillator A's pulse width", width, &|row, _| PULSE_WIDTH.get(row).copied()),
            ("triangle to oscillator A's pulse width", triangle(width), &|row, _| PULSE_WIDTH.get(row).copied()),
            ("triangle to oscillator A's frequency and pulse width", triangle(together), &|_, _| Some(TOGETHER)),
            ("sawtooth to the cutoff", cutoff, &bound),
        ];
        for (name, patch, bound) in destinations {
            for (row, amount) in AMOUNTS.into_iter().enumerate().filter(|&(row, _)| !quick || row == 1) {
                let mut found = Vec::new();
                for (column, key) in KEYS.into_iter().enumerate().filter(|&(_, key)| !quick || key == 60 || key == 84) {
                    let level = measured(model, &patch.with(Knob::PolyModOscB, amount), key);
                    found.push(format!("{level:.0}"));
                    let held = bound(row, column).unwrap_or(f64::MIN);
                    assert!(level < held, "{model:?}: {name}, amount {amount}, key {key}: {level} dB, not {held} dB");
                }
                println!("{model:?}: oscillator B's {name}, amount {amount}: {} dB", found.join(", "));
            }
        }
    }
}
