//! What the tests of synth8 share: playing a patch and a few
//! keys, and measuring what comes out, in f64. Each test file includes it
//! as a module; by itself it tests nothing.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth8")]
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
use neoteletext_audio_instruments::synth8::{Param, Patch, Synth8};

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
/// The instrument's keys by the names its service notes use: C0 is MIDI
/// key 36.
pub const C0: u8 = 36;
/// The service notes' C2: middle C.
pub const C2: u8 = 60;
/// The service notes' A2: the A of the tuning reference.
pub const A2: u8 = 69;
/// The service notes' C4.
pub const C4: u8 = 84;
/// The top key, C5.
pub const C5: u8 = 96;

/// Samples in so many seconds.
#[must_use]
pub fn samples(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// Renders so many seconds, as full scale 1.
pub fn render(synth8: &mut Synth8, seconds: f64) -> Vec<f64> {
    let mut out = vec![0i16; samples(seconds)];
    synth8.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

/// An instrument playing a patch on all eight voices.
#[must_use]
pub fn instrument(patch: &Patch) -> Synth8 {
    let mut synth8 = Synth8::new();
    synth8.set_patch(patch);
    synth8
}

/// Plays one key of a patch for `held` seconds and `after` more with the
/// key up; gives both parts in one signal.
#[must_use]
pub fn play(patch: &Patch, key: u8, held: f64, after: f64) -> Vec<f64> {
    let mut synth8 = instrument(patch);
    synth8.event(NOTE_ON, key, 100);
    let mut signal = render(&mut synth8, held);
    synth8.event(NOTE_OFF, key, 0);
    signal.extend(render(&mut synth8, after));
    signal
}

/// VCO-1's square alone through an open filter, the amplifier at full
/// sustain.
#[must_use]
pub fn square() -> Patch {
    Patch::DEFAULT.with(Param::Vco1Wave, 3)
}

/// VCO-2 alone, with a waveform, in the mixer.
#[must_use]
pub fn second(wave: u8) -> Patch {
    Patch::DEFAULT.with(Param::MixVco1, 0).with(Param::MixVco2, 255).with(Param::Vco2Wave, wave)
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

/// The largest magnitude of a signal.
#[must_use]
pub fn peak(signal: &[f64]) -> f64 {
    signal.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()))
}

/// The root mean square of a signal.
#[must_use]
pub fn rms(signal: &[f64]) -> f64 {
    (signal.iter().map(|value| value * value).sum::<f64>() / signal.len() as f64).sqrt()
}

/// The magnitude of a signal's component at a frequency, as the peak of
/// the sine that has it, through a Hann window: Goertzel's sum taken
/// directly.
#[must_use]
pub fn component(signal: &[f64], hertz: f64) -> f64 {
    let length = signal.len() as f64;
    let (mut real, mut imaginary, mut weight) = (0.0f64, 0.0f64, 0.0f64);
    for (index, &value) in signal.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / length).cos();
        let angle = std::f64::consts::TAU * hertz * index as f64 / RATE;
        real += window * value * angle.cos();
        imaginary += window * value * angle.sin();
        weight += window;
    }
    2.0 * real.hypot(imaginary) / weight
}

/// A level in decibels against another.
#[must_use]
pub fn decibels(level: f64, against: f64) -> f64 {
    20.0 * (level / against).log10()
}

/// The four-term Blackman-Harris window's coefficients: side lobes 92 dB
/// down, a main lobe four bins either way.
const WINDOW: [f64; 4] = [0.358_75, 0.488_29, 0.141_28, 0.011_68];

/// The magnitudes of a signal's spectrum through a Blackman-Harris
/// window, as the peak of the sine that has each: `length` samples, a
/// power of two, give `length / 2` bins of `RATE / length` hertz.
#[must_use]
pub fn spectrum(signal: &[f64], length: usize) -> Vec<f64> {
    assert!(length.is_power_of_two() && signal.len() >= length);
    let window = |index: usize| {
        let turn = std::f64::consts::TAU * index as f64 / length as f64;
        WINDOW[0] - WINDOW[1] * turn.cos() + WINDOW[2] * (2.0 * turn).cos() - WINDOW[3] * (3.0 * turn).cos()
    };
    let mut real: Vec<f64> = (0..length).map(|index| signal[index] * window(index)).collect();
    let mut imaginary = vec![0.0f64; length];
    // The radix-two transform in place: bit reversal, then butterflies.
    let bits = length.trailing_zeros();
    for index in 0..length {
        let reversed = index.reverse_bits() >> (usize::BITS - bits);
        if reversed > index {
            real.swap(index, reversed);
        }
    }
    let mut span = 2;
    while span <= length {
        let angle = -std::f64::consts::TAU / span as f64;
        for start in (0..length).step_by(span) {
            for offset in 0..span / 2 {
                let (sine, cosine) = (angle * offset as f64).sin_cos();
                let (low, high) = (start + offset, start + offset + span / 2);
                let (re, im) =
                    (real[high] * cosine - imaginary[high] * sine, real[high] * sine + imaginary[high] * cosine);
                (real[high], imaginary[high]) = (real[low] - re, imaginary[low] - im);
                real[low] += re;
                imaginary[low] += im;
            }
        }
        span *= 2;
    }
    // The window's mean is its first coefficient.
    (0..length / 2).map(|bin| real[bin].hypot(imaginary[bin]) * 2.0 / (WINDOW[0] * length as f64)).collect()
}

/// The hertz of a spectrum's bin.
#[must_use]
pub fn bin_hertz(spectrum: &[f64]) -> f64 {
    RATE / (2 * spectrum.len()) as f64
}

/// The strongest line of a spectrum from `bottom` to `top` hertz, which
/// leaves out what does not alternate: its frequency,
/// placed between bins by the parabola through the three logarithms
/// about the peak, and its level.
#[must_use]
pub fn strongest(spectrum: &[f64], bottom: f64, top: f64) -> (f64, f64) {
    let width = bin_hertz(spectrum);
    let last = ((top / width) as usize).min(spectrum.len() - 2);
    let first = ((bottom / width).ceil() as usize).max(1);
    let bin = (first..=last).max_by(|&a, &b| spectrum[a].total_cmp(&spectrum[b])).unwrap_or(1);
    let (before, at, after) = (spectrum[bin - 1].ln(), spectrum[bin].ln(), spectrum[bin + 1].ln());
    let offset = 0.5 * (before - after) / (before - 2.0 * at + after);
    ((bin as f64 + offset) * width, spectrum[bin])
}

/// The strongest bin of a spectrum from `clear` hertz to `top` hertz for
/// which `belongs` says whether its frequency is part of the sound: the
/// strongest that is, and the strongest that is not.
#[must_use]
pub fn sound_and_the_rest(spectrum: &[f64], clear: f64, top: f64, belongs: impl Fn(f64) -> bool) -> (f64, f64) {
    let width = bin_hertz(spectrum);
    let (mut sound, mut rest) = (0.0f64, 0.0f64);
    for (bin, &level) in spectrum.iter().enumerate() {
        let hertz = bin as f64 * width;
        if hertz > top {
            break;
        }
        if hertz < clear {
            continue;
        }
        if belongs(hertz) {
            sound = sound.max(level);
        } else {
            rest = rest.max(level);
        }
    }
    (sound, rest)
}

/// How far a frequency is from the nearest whole multiple of a spacing.
#[must_use]
pub fn off_the_grid(hertz: f64, spacing: f64) -> f64 {
    (hertz - (hertz / spacing).round() * spacing).abs()
}
