//! What the tests of the CS-80 share: playing a patch and a few keys, and
//! measuring what comes out, in f64. Each test file includes it as a
//! module; by itself it tests nothing.

// Without the feature there is nothing here to test.
#![cfg(feature = "cs80")]
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
use neoteletext_audio_instruments::cs80::{Control, Cs80, Global, Patch, Tone};

/// The output rate as f64.
pub const RATE: f64 = SAMPLE_RATE as f64;
/// MIDI status: note on.
pub const NOTE_ON: u8 = 0x90;
/// Note off.
pub const NOTE_OFF: u8 = 0x80;
/// Polyphonic key pressure.
pub const KEY_PRESSURE: u8 = 0xa0;
/// Control change.
pub const CONTROL: u8 = 0xb0;
/// Channel pressure.
pub const CHANNEL_PRESSURE: u8 = 0xd0;
/// Pitch bend.
pub const PITCH_BEND: u8 = 0xe0;

/// Samples in so many seconds.
#[must_use]
pub fn samples(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// Renders so many seconds in the instrument's own scale, in which the
/// levels Yamaha gives are stated: the whole of the mixed output as full
/// scale 1, with the output's gain, which sets the level the crate's
/// instruments share, taken out again.
pub fn render(cs80: &mut Cs80, seconds: f64) -> Vec<f64> {
    /// One in Q28.
    const ONE_Q28: f64 = 268_435_456.0;
    let gain = f64::from(neoteletext_audio_instruments::cs80::OUTPUT_GAIN) / ONE_Q28;
    let mut out = vec![0i16; samples(seconds)];
    cs80.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0 / gain).collect()
}

/// An instrument playing a patch.
#[must_use]
pub fn instrument(patch: &Patch) -> Cs80 {
    let mut cs80 = Cs80::new();
    cs80.set_patch(patch);
    cs80
}

/// The panel at rest with channel I's tone changed: each control given
/// is set. The overall RESONANCE lever, which rests at its centre, is put
/// at its foot, so that a filter whose own lever is at its foot has the
/// chip's least Q, 0.5.
#[must_use]
pub fn panel(controls: &[(Control, u8)]) -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.set(Global::Resonance, 0);
    for &(control, value) in controls {
        patch.tones[0].set(control, value);
    }
    patch
}

/// A tone with these controls set on the panel's tone at rest.
#[must_use]
pub fn tone(controls: &[(Control, u8)]) -> Tone {
    panel(controls).tones[0]
}

/// The sine alone, past the filters, the amplifier a gate.
pub const SINE: [(Control, u8); 3] = [(Control::Sawtooth, 0), (Control::VcfLevel, 0), (Control::Sine, 127)];

/// Plays one key of a patch at a velocity for `held` seconds and `after`
/// more with the key up; gives both parts in one signal.
#[must_use]
pub fn play(patch: &Patch, key: u8, velocity: u8, held: f64, after: f64) -> Vec<f64> {
    let mut cs80 = instrument(patch);
    cs80.event(NOTE_ON, key, velocity);
    let mut signal = render(&mut cs80, held);
    cs80.event(NOTE_OFF, key, 0);
    signal.extend(render(&mut cs80, after));
    signal
}

/// The level of a frequency in a signal: the magnitude of its one
/// Fourier component, as the amplitude of a sine.
#[must_use]
pub fn component(signal: &[f64], hertz: f64) -> f64 {
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (index, &value) in signal.iter().enumerate() {
        // A Hann window keeps a neighbour's skirt out.
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / signal.len() as f64).cos();
        let angle = std::f64::consts::TAU * hertz * index as f64 / RATE;
        real += value * window * angle.cos();
        imaginary += value * window * angle.sin();
    }
    4.0 * real.hypot(imaginary) / signal.len() as f64
}

/// Decibels of a ratio of amplitudes.
#[must_use]
pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.log10()
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

/// The level of a tone through time: the root mean square over `window`
/// samples about each sample, times √2, so that a sine's is its peak.
#[must_use]
pub fn envelope(signal: &[f64], window: usize) -> Vec<f64> {
    let mut sum = 0.0;
    let mut levels = Vec::with_capacity(signal.len());
    for (index, &value) in signal.iter().enumerate() {
        sum += value * value;
        if index >= window {
            sum -= signal[index - window] * signal[index - window];
        }
        levels.push((2.0 * sum.max(0.0) / window.min(index + 1) as f64).sqrt());
    }
    levels
}

/// The first time in seconds at which a series of levels, one for each
/// sample, is at or beyond `level` (rising) or at or below it (falling),
/// from the sample `from` on.
#[must_use]
pub fn reaches(levels: &[f64], from: usize, level: f64, rising: bool) -> f64 {
    let found = levels[from..].iter().position(|&value| if rising { value >= level } else { value <= level });
    found.map_or(f64::INFINITY, |index| index as f64 / RATE)
}

/// How often a signal crosses zero upwards, in hertz: the middle
/// frequency of a narrow band of noise.
#[must_use]
pub fn crossing_rate(signal: &[f64]) -> f64 {
    let crossings = signal.windows(2).filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0).count();
    crossings as f64 * RATE / signal.len() as f64
}
