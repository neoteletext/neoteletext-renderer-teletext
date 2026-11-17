//! What the tests of the Minimoog share: playing a patch and a few keys,
//! and measuring what comes out, in f64. Each test file includes it as a
//! module; by itself it tests nothing.

// Without the feature there is nothing here to test.
#![cfg(feature = "minimoog")]
#![allow(
    dead_code,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    reason = "test support in f64, of which each test file uses a part"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::minimoog::{Minimoog, Patch, Waveform};

/// The output rate as f64.
pub const RATE: f64 = SAMPLE_RATE as f64;
/// MIDI status: note on.
pub const NOTE_ON: u8 = 0x90;
/// Note off.
pub const NOTE_OFF: u8 = 0x80;
/// Control change.
pub const CONTROL: u8 = 0xb0;
/// Pitch bend.
pub const PITCH_BEND: u8 = 0xe0;

/// Samples in so many seconds.
#[must_use]
pub fn samples(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// Renders so many seconds, as full scale 1.
pub fn render(minimoog: &mut Minimoog, seconds: f64) -> Vec<f64> {
    let mut out = vec![0i16; samples(seconds)];
    minimoog.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

/// An instrument playing a patch, its oscillators not drifting.
#[must_use]
pub fn instrument(patch: &Patch) -> Minimoog {
    let mut minimoog = Minimoog::new();
    minimoog.set_drift(0);
    minimoog.set_patch(patch);
    minimoog
}

/// Plays one key of a patch for `held` seconds and `after` more with the
/// key up; gives both parts in one signal.
#[must_use]
pub fn play(patch: &Patch, key: u8, held: f64, after: f64) -> Vec<f64> {
    let mut minimoog = instrument(patch);
    minimoog.event(NOTE_ON, key, 100);
    let mut signal = render(&mut minimoog, held);
    minimoog.event(NOTE_OFF, key, 0);
    signal.extend(render(&mut minimoog, after));
    signal
}

/// Oscillator 1's triangle alone through the open filter.
#[must_use]
pub fn triangle() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].waveform = Waveform::Triangle;
    patch
}

/// No oscillator in the mixer, the EMPHASIS at 10, the keyboard's whole
/// control: the filter's own oscillation, with the CUTOFF FREQUENCY dial
/// at `cutoff` hundredths.
#[must_use]
pub fn whistle(cutoff: i16) -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].on = false;
    patch.emphasis = 100;
    patch.cutoff = cutoff;
    patch.keyboard_control = [true, true];
    patch
}

/// The frequency in hertz of a steady tone, from the times of its rising
/// crossings of its mean, each placed between two samples: good to a few
/// millionths over a second.
#[must_use]
pub fn pitch(signal: &[f64]) -> f64 {
    let mean = signal.iter().sum::<f64>() / signal.len() as f64;
    // Smoothed over four samples, so that ripple beside a crossing does
    // not count twice.
    let smooth: Vec<f64> = signal.windows(4).map(|window| window.iter().sum::<f64>() / 4.0 - mean).collect();
    let peak = smooth.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
    let (mut first, mut last, mut count, mut armed) = (None, 0.0, 0usize, false);
    for (index, pair) in smooth.windows(2).enumerate() {
        // A crossing counts only after the tone has been well below.
        if pair[0] < -0.3 * peak {
            armed = true;
        }
        if armed && pair[0] < 0.0 && pair[1] >= 0.0 {
            armed = false;
            last = index as f64 + pair[0] / (pair[0] - pair[1]);
            first.get_or_insert(last);
            count += 1;
        }
    }
    match first {
        Some(first) if count > 1 => (count - 1) as f64 * RATE / (last - first),
        _ => 0.0,
    }
}

/// The interval from one frequency to another in cents.
#[must_use]
pub fn cents(from: f64, to: f64) -> f64 {
    1200.0 * (to / from).log2()
}

/// The largest magnitude in each window of `window` seconds.
#[must_use]
pub fn peaks(signal: &[f64], window: f64) -> Vec<f64> {
    signal
        .chunks(samples(window))
        .map(|chunk| chunk.iter().fold(0.0f64, |peak, &value| peak.max(value.abs())))
        .collect()
}

/// The root mean square of a signal.
#[must_use]
pub fn rms(signal: &[f64]) -> f64 {
    (signal.iter().map(|value| value * value).sum::<f64>() / signal.len() as f64).sqrt()
}
