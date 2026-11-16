//! What the SID's tests share: rendering, register values for a pitch, and
//! the measurements made on samples. Each test file includes it as a
//! module; by itself it tests nothing.

// Without the SID's feature there is nothing here to test.
#![cfg(feature = "sid")]
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
use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, Sid};

/// Both models.
pub const MODELS: [ChipModel; 2] = [ChipModel::Mos6581, ChipModel::Mos8580];
/// Both clocks.
pub const CLOCKS: [ChipClock; 2] = [ChipClock::Pal, ChipClock::Ntsc];

/// The first register of a voice, 0 to 2.
#[must_use]
pub fn voice(index: u8) -> u8 {
    index * 7
}

/// The frequency register for a pitch: data sheet, registers 0 and 1,
/// "Fout = (Fn * Fclk / 16777216) Hz".
#[must_use]
pub fn frequency_register(hz: f64, clock: ChipClock) -> u16 {
    (hz * 16_777_216.0 / f64::from(clock.hz())).round() as u16
}

/// The pitch a frequency register gives.
#[must_use]
pub fn pitch(register: u16, clock: ChipClock) -> f64 {
    f64::from(register) * f64::from(clock.hz()) / 16_777_216.0
}

/// Sets a voice's frequency register.
pub fn set_frequency(sid: &mut Sid, index: u8, register: u16) {
    sid.write(voice(index), register as u8);
    sid.write(voice(index) + 1, (register >> 8) as u8);
}

/// Starts a voice at once and at full level: the fastest attack, sustain
/// at the peak, the control register as given with its gate set.
pub fn start(sid: &mut Sid, index: u8, register: u16, control: u8) {
    set_frequency(sid, index, register);
    sid.write(voice(index) + 5, 0x00);
    sid.write(voice(index) + 6, 0xF0);
    sid.write(voice(index) + 4, control | 0x01);
}

/// Renders `seconds` of samples.
#[must_use]
pub fn render(sid: &mut Sid, seconds: f64) -> Vec<i16> {
    let mut samples = vec![0i16; (seconds * f64::from(SAMPLE_RATE)).round() as usize];
    sid.render(&mut samples);
    samples
}

/// The amplitude of the component of `samples` at `hz`, by correlation
/// with a sine and a cosine under a Hann window.
#[must_use]
pub fn amplitude(samples: &[i16], hz: f64) -> f64 {
    let count = samples.len() as f64;
    let step = 2.0 * std::f64::consts::PI * hz / f64::from(SAMPLE_RATE);
    let (mut real, mut imaginary, mut weight) = (0.0, 0.0, 0.0);
    for (index, &sample) in samples.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * index as f64 / count).cos();
        let angle = step * index as f64;
        real += window * f64::from(sample) * angle.cos();
        imaginary += window * f64::from(sample) * angle.sin();
        weight += window;
    }
    2.0 * real.hypot(imaginary) / weight
}

/// A ratio in decibels.
#[must_use]
pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The root mean square of `samples`.
#[must_use]
pub fn rms(samples: &[i16]) -> f64 {
    (samples.iter().map(|&sample| f64::from(sample) * f64::from(sample)).sum::<f64>() / samples.len() as f64).sqrt()
}

/// The pitch of `samples` from its rising zero crossings, each placed
/// between two samples by a straight line.
#[must_use]
pub fn zero_crossing_pitch(samples: &[i16]) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0 && pair[1] >= 0)
        .map(|(index, pair)| index as f64 + f64::from(-pair[0]) / f64::from(pair[1] - pair[0]))
        .collect();
    let (first, last) = (crossings[0], crossings[crossings.len() - 1]);
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE) / (last - first)
}

/// The magnitudes of the discrete Fourier transform of `samples`, whose
/// length is a power of two, under a four-term Blackman-Harris window
/// (side lobes 92 dB down; Harris, "On the use of windows for harmonic
/// analysis with the discrete Fourier transform", Proceedings of the IEEE,
/// 1978), scaled so that a sine reads its amplitude.
#[must_use]
pub fn spectrum(samples: &[i16]) -> Vec<f64> {
    let count = samples.len();
    assert!(count.is_power_of_two());
    let mut real = vec![0.0f64; count];
    let mut imaginary = vec![0.0f64; count];
    let mut weight = 0.0;
    for (index, &sample) in samples.iter().enumerate() {
        let turn = 2.0 * std::f64::consts::PI * index as f64 / count as f64;
        let window = 0.35875 - 0.48829 * turn.cos() + 0.14128 * (2.0 * turn).cos() - 0.01168 * (3.0 * turn).cos();
        real[index] = window * f64::from(sample);
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
        let angle = -2.0 * std::f64::consts::PI / span as f64;
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

/// A small generator of test values (a 64-bit linear congruential
/// generator with Knuth's MMIX constants), so that a seed names a run.
pub struct Random(pub u64);

impl Random {
    /// The next 32 bits.
    pub fn word(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    /// The next byte.
    pub fn byte(&mut self) -> u8 {
        (self.word() >> 24) as u8
    }
}
