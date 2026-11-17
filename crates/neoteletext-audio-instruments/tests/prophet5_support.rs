//! What the tests of the Prophet-5 share: playing a patch and a few keys, and
//! measuring what comes out, in f64. Each test file includes it as a
//! module; by itself it tests nothing.

// Without the feature there is nothing here to test.
#![cfg(feature = "prophet5")]
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
use neoteletext_audio_instruments::prophet5::{Knob, Model, Patch, Prophet5, Switch};

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
/// Both models.
pub const MODELS: [Model; 2] = [Model::Rev3, Model::Rev2];

/// Samples in so many seconds.
#[must_use]
pub fn samples(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// Renders so many seconds, as full scale 1.
pub fn render(prophet: &mut Prophet5, seconds: f64) -> Vec<f64> {
    let mut out = vec![0i16; samples(seconds)];
    prophet.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

/// An instrument playing a patch.
#[must_use]
pub fn instrument(model: Model, patch: &Patch) -> Prophet5 {
    let mut prophet = Prophet5::new(model);
    prophet.set_patch(patch);
    prophet
}

/// Plays one key of a patch for `held` seconds and `after` more with the
/// key up; gives both parts in one signal.
#[must_use]
pub fn play(model: Model, patch: &Patch, key: u8, held: f64, after: f64) -> Vec<f64> {
    let mut prophet = instrument(model, patch);
    prophet.event(NOTE_ON, key, 100);
    let mut signal = render(&mut prophet, held);
    prophet.event(NOTE_OFF, key, 0);
    signal.extend(render(&mut prophet, after));
    signal
}

/// The plain patch with both frequency knobs two octaves up on the
/// model, where the A above middle C is 440 Hz.
#[must_use]
pub fn plain(model: Model) -> Patch {
    let knob = model.frequency_knob(24);
    Patch::DEFAULT.with(Knob::OscAFrequency, knob).with(Knob::OscBFrequency, knob)
}

/// No waveform at all, the resonance at its top: the filter's own
/// oscillation, with the cutoff knob at `cutoff` steps.
#[must_use]
pub fn whistle(model: Model, cutoff: u8) -> Patch {
    plain(model).with(Knob::Cutoff, cutoff).with(Knob::Resonance, 127).with_switch(Switch::OscASawtooth, false)
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

/// The frequency of each cycle of a tone whose pitch moves: the time of
/// each rising crossing of zero after the first, in seconds, and the
/// frequency of the cycle it ends.
#[must_use]
pub fn cycles(signal: &[f64]) -> Vec<(f64, f64)> {
    let peak = signal.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
    let (mut last, mut armed, mut found) = (None, false, Vec::new());
    for (index, pair) in signal.windows(2).enumerate() {
        if pair[0] < -0.3 * peak {
            armed = true;
        }
        if armed && pair[0] < 0.0 && pair[1] >= 0.0 {
            armed = false;
            let now = (index as f64 + pair[0] / (pair[0] - pair[1])) / RATE;
            if let Some(before) = last {
                found.push((now, 1.0 / (now - before)));
            }
            last = Some(now);
        }
    }
    found
}

/// The amplitude of one frequency in a signal, by correlation with a
/// sine and a cosine.
#[must_use]
pub fn amplitude(signal: &[f64], hertz: f64) -> f64 {
    let (mut sine, mut cosine) = (0.0, 0.0);
    for (index, &value) in signal.iter().enumerate() {
        let angle = std::f64::consts::TAU * hertz * index as f64 / RATE;
        sine += value * angle.sin();
        cosine += value * angle.cos();
    }
    2.0 * sine.hypot(cosine) / signal.len() as f64
}

/// The plain patch with the filter closed to the note's own pitch and
/// following the keys, which leaves little but the fundamental: a tone
/// whose peaks are its level, for reading envelopes.
#[must_use]
pub fn pure(model: Model) -> Patch {
    plain(model).with(
        Knob::Cutoff,
        match model {
            Model::Rev3 => 36,
            Model::Rev2 => 40,
        },
    )
}
