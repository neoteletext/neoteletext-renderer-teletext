//! What synthD leaves below 20 kHz that is no harmonic of its note:
//! the aliasing of the whole instrument, its oscillators through the
//! ladder and the amplifier's curve after it (TODO-6438).
//!
//! The oscillators by themselves are 91 dB clean at the voice's rate
//! (`tests/analog_oscillator.rs`). The ladder's stages bend their edges,
//! and what that makes above 172 kHz comes back below 20 kHz when it is
//! sampled at 192 kHz: the figures held here are what the instrument
//! does, and from 880 Hz up with a sawtooth at 10 they are not the 80 dB
//! asked of the oscillators. At 96 kHz, where the voice first ran, each
//! was 6 to 9 dB worse: the sawtooth at 10 left 83.6, 76.9, 72.1, 66.3,
//! 60.4 and 52.7 dB, and three in step 71.2 to 39.6 dB.

// Without the feature there is nothing here to test.
#![cfg(feature = "synthd")]
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
#[path = "synthd_support.rs"]
mod support;

use analog::{decibels, spectrum};
use neoteletext_audio_instruments::synthd::{Patch, Range, Waveform};
use support::{NOTE_ON, RATE, instrument, render};

/// Samples of a spectrum: bins are 48,000 / 262,144 Hz apart.
const POINTS: usize = 1 << 18;
/// Bins either side of a harmonic that are the harmonic's: twice the
/// main lobe of the spectrum's window.
const GUARD: f64 = 2.0 * analog::LOBE as f64;
/// How well a harmonic's place is known, as a share of its frequency:
/// the pitch is within a hundredth of a cent (`tests/synthd_pitch.rs`
/// holds it to a cent; `law::raise` says the hundredth).
const PITCH_TOLERANCE: f64 = 1.0e-5;
/// The band measured, in hertz: what is heard, above the window's skirt
/// of whatever is left near nothing.
const BAND: (f64, f64) = (20.0, 20_000.0);
/// Seconds played before the spectrum is taken.
const SETTLE: f64 = 0.5;
/// How far above its figure here a measurement may come, in decibels.
const MARGIN: f64 = 1.5;

/// How far below the strongest harmonic the strongest tone that is no
/// harmonic lies, in the band, in decibels, for a key that sounds
/// `hertz` hertz.
fn aliasing(patch: &Patch, key: u8, hertz: f64) -> f64 {
    let mut synthd = instrument(patch);
    synthd.event(NOTE_ON, key, 100);
    let _ = render(&mut synthd, SETTLE);
    let signal = render(&mut synthd, POINTS as f64 / RATE);
    assert_eq!(signal.len(), POINTS);
    let magnitudes = spectrum(&signal);
    let bin = RATE / POINTS as f64;
    let line = hertz / bin;
    let (from, to) = ((BAND.0 / bin) as usize, (BAND.1 / bin) as usize);
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (index, &magnitude) in magnitudes.iter().enumerate().take(to).skip(from) {
        let harmonic = (index as f64 / line).round().max(1.0);
        let from_line = (index as f64 - harmonic * line).abs();
        if from_line <= GUARD + harmonic * line * PITCH_TOLERANCE {
            on = on.max(magnitude);
        } else {
            off = off.max(magnitude);
        }
    }
    decibels(off / on)
}

/// The notes measured: a key, the range it is played at, and the octaves
/// that range is from 8'. They sound 110, 220, 440, 880, 1,760 and
/// 4,186 Hz, the last being the keyboard's top key at 2'.
const NOTES: [(u8, Range, i32); 6] = [
    (45, Range::Eight, 0),
    (57, Range::Eight, 0),
    (69, Range::Eight, 0),
    (81, Range::Eight, 0),
    (81, Range::Four, 1),
    (84, Range::Two, 2),
];

/// A mixer's setting, the waveform of the oscillators in it and their
/// volumes, with what it was measured to leave at each of [`NOTES`], in
/// decibels (17 November 2026, a release build).
type Case = (&'static str, Waveform, [u8; 3], [f64; 6]);

/// The filter open (CUTOFF FREQUENCY at 5), no EMPHASIS, no drift; the
/// oscillators of one case at one pitch and in step.
const CASES: [Case; 6] = [
    ("triangle at 10", Waveform::Triangle, [100, 0, 0], [-101.7, -100.3, -99.7, -99.0, -98.7, -103.6]),
    ("sawtooth at 5", Waveform::Sawtooth, [50, 0, 0], [-88.8, -91.8, -87.6, -85.2, -79.9, -72.1]),
    ("sawtooth at 10", Waveform::Sawtooth, [100, 0, 0], [-91.5, -85.3, -80.3, -74.4, -68.4, -60.8]),
    ("square at 10", Waveform::Square, [100, 0, 0], [-91.6, -85.2, -79.6, -73.9, -67.9, -60.5]),
    ("narrow at 10", Waveform::Narrow, [100, 0, 0], [-86.1, -79.4, -74.2, -68.0, -62.3, -56.4]),
    ("three sawtooths at 10", Waveform::Sawtooth, [100, 100, 100], [-77.6, -71.0, -65.7, -59.7, -53.7, -46.4]),
];

/// The oscillators' own bound, which the instrument's path meets where
/// nothing bends: `tests/analog_oscillator.rs` holds them to it.
const RULE: f64 = -80.0;

#[test]
fn what_the_instrument_aliases_is_measured() {
    // A debug build takes the two ends of each case.
    let skipped = |index: usize| cfg!(debug_assertions) && index != 0 && index != NOTES.len() - 1;
    for (name, waveform, volumes, measured) in CASES {
        for (index, ((key, range, octaves), expected)) in NOTES.into_iter().zip(measured).enumerate() {
            if skipped(index) {
                continue;
            }
            let mut patch = Patch::DEFAULT;
            for (oscillator, volume) in patch.oscillators.iter_mut().zip(volumes) {
                oscillator.range = range;
                oscillator.waveform = waveform;
                oscillator.volume = volume;
                oscillator.on = volume > 0;
            }
            let hertz = 440.0 * ((f64::from(key) - 69.0) / 12.0 + f64::from(octaves)).exp2();
            let found = aliasing(&patch, key, hertz);
            println!("{name} at {hertz:.0} Hz: {found:.1} dB (measured {expected:.1} dB)");
            assert!(found < expected + MARGIN, "{name} at {hertz:.0} Hz: {found:.1} dB for {expected:.1} dB");
        }
    }
    // Where nothing bends the edges, the instrument keeps the
    // oscillators' bound: the triangle at every pitch.
    let (_, _, _, triangle) = CASES[0];
    assert!(triangle.iter().all(|&found| found + MARGIN < RULE));
}
