//! What the tests of the analogue building blocks share: spectra and
//! levels of signals, in f64. Each test file includes it as a module; by
//! itself it tests nothing.

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
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

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::decimator::Decimator;

/// A ratio in decibels.
#[must_use]
pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The amplitude of the component of `samples` at `cycles` cycles a
/// sample, by correlation with a sine and a cosine under a Hann window.
#[must_use]
pub fn amplitude(samples: &[f64], cycles: f64) -> f64 {
    let count = samples.len() as f64;
    let (mut real, mut imaginary, mut weight) = (0.0, 0.0, 0.0);
    for (index, &sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * PI * index as f64 / count).cos();
        let angle = 2.0 * PI * cycles * index as f64;
        real += window * sample * angle.cos();
        imaginary += window * sample * angle.sin();
        weight += window;
    }
    2.0 * real.hypot(imaginary) / weight
}

/// The magnitudes of the discrete Fourier transform of `samples`, whose
/// length is a power of two, under a four-term Blackman-Harris window
/// (side lobes 92 dB down; Harris, "On the use of windows for harmonic
/// analysis with the discrete Fourier transform", Proceedings of the IEEE,
/// 1978), scaled so that a sine reads its amplitude.
#[must_use]
pub fn spectrum(samples: &[f64]) -> Vec<f64> {
    let count = samples.len();
    assert!(count.is_power_of_two());
    let mut real = vec![0.0f64; count];
    let mut imaginary = vec![0.0f64; count];
    let mut weight = 0.0;
    for (index, &sample) in samples.iter().enumerate() {
        let turn = 2.0 * PI * index as f64 / count as f64;
        let window = 0.35875 - 0.48829 * turn.cos() + 0.14128 * (2.0 * turn).cos() - 0.01168 * (3.0 * turn).cos();
        real[index] = window * sample;
        weight += window;
    }
    // In place, radix two: reorder by reversed bits, then the butterflies.
    let bits = count.trailing_zeros();
    for index in 0..count {
        let reversed = index.reverse_bits() >> (usize::BITS - bits);
        if reversed > index {
            real.swap(index, reversed);
        }
    }
    let mut span = 2;
    while span <= count {
        let angle = -2.0 * PI / span as f64;
        for start in (0..count).step_by(span) {
            for offset in 0..span / 2 {
                let (sine, cosine) = (angle * offset as f64).sin_cos();
                let (low, high) = (start + offset, start + offset + span / 2);
                let (re, im) =
                    (real[high] * cosine - imaginary[high] * sine, real[high] * sine + imaginary[high] * cosine);
                real[high] = real[low] - re;
                imaginary[high] = imaginary[low] - im;
                real[low] += re;
                imaginary[low] += im;
            }
        }
        span *= 2;
    }
    (0..count / 2).map(|bin| 2.0 * real[bin].hypot(imaginary[bin]) / weight).collect()
}

/// Bins either side of a tone that the window's main lobe covers.
pub const LOBE: usize = 6;

/// For a signal whose every wanted tone lies on a multiple of `base` bins
/// of `spectrum`: the strongest wanted tone, and the strongest of
/// everything else below `top` bins.
#[must_use]
pub fn on_and_off_the_grid(spectrum: &[f64], base: usize, top: usize) -> (f64, f64) {
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (bin, &magnitude) in spectrum.iter().enumerate().take(top).skip(LOBE + 1) {
        let from_line = (bin % base).min(base - bin % base);
        if from_line == 0 {
            on = on.max(magnitude);
        } else if from_line > LOBE {
            off = off.max(magnitude);
        }
    }
    (on, off)
}

/// Brings `signal`, at `factor` times the output rate, down to the output
/// rate through the crate's decimators.
#[must_use]
pub fn decimate(signal: &[i32], factor: u32) -> Vec<i32> {
    let halve = |signal: &[i32], mut decimator: Decimator| -> Vec<i32> {
        signal.as_chunks::<2>().0.iter().map(|&[earlier, later]| decimator.process(earlier, later)).collect()
    };
    match factor {
        1 => signal.to_vec(),
        2 => halve(signal, Decimator::sharp()),
        4 => halve(&halve(signal, Decimator::wide()), Decimator::sharp()),
        _ => panic!("no decimator for a factor of {factor}"),
    }
}

/// A signal as f64.
#[must_use]
pub fn float(signal: &[i32]) -> Vec<f64> {
    signal.iter().map(|&sample| f64::from(sample)).collect()
}

/// A small generator of test values (a 64-bit linear congruential
/// generator with Knuth's MMIX constants), so that a seed names a run.
pub struct Random(pub u64);

impl Random {
    /// The next 32 bits.
    pub fn word(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }
}
