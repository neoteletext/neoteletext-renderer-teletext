//! What the amplifier cascade's distortion folds back below 20 kHz when
//! an oscillator drives it as the instruments do: a measurement
//! (TODO-6438).
//!
//! The oscillators by themselves leave nothing that is no harmonic
//! within 91 dB (`tests/analog_oscillator.rs`). Each stage of the
//! cascade bends the difference between its input and its output, and
//! what the bending makes above the rate's half comes back. Measured
//! against the strongest harmonic, for a sawtooth of 220 Hz, 880 Hz,
//! 1,760 Hz, 4,186 Hz and 7 kHz with no resonance, at twice the output
//! rate, where the Juno, the Jupiter-8 and the Prophet-5 run it:
//!
//! | cutoff | waveforms at full level | 220 Hz | 880 Hz | 1,760 Hz | 4,186 Hz | 7 kHz |
//! |---|---|---|---|---|---|---|
//! | 20 kHz | one | 109 dB | 97 dB | 91 dB | 83 dB | 78 dB |
//! | 20 kHz | two in step | 97 dB | 85 dB | 79 dB | 71 dB | 66 dB |
//! | 20 kHz | three in step | 90 dB | 78 dB | 72 dB | 65 dB | 59 dB |
//! | 5 kHz | one | 105 dB | 93 dB | 86 dB | 71 dB | 61 dB |
//! | 5 kHz | two in step | 94 dB | 82 dB | 74 dB | 60 dB | 49 dB |
//! | 5 kHz | three in step | 88 dB | 75 dB | 68 dB | 53 dB | 42 dB |
//!
//! One waveform at full level is each instrument's plain patch, an
//! eighth of the level at which a stage saturates; two in step are both
//! oscillators of the Jupiter-8 or the Prophet-5, or the Juno's sawtooth
//! and pulse; three are the Juno's sawtooth, pulse and sub-oscillator,
//! and the most the Prophet-5's mixer gives. Each doubling of the level
//! costs 12 dB. A fundamental near or above the cutoff reads worse than
//! it sounds, since the harmonic it is measured against is itself
//! reduced.
//!
//! As far open as the cascade goes, 0.45 of the rate, a stage's output
//! follows its input and there is next to nothing to bend: 103 dB or
//! better for one waveform. At four times the output rate the same
//! cutoffs gain 2 to 6 dB: the cascade bends differences, which the
//! higher rate does not make smaller, where the ladder bends the
//! signals themselves and gains 8 dB from each doubling.
//!
//! It is a measurement of the filter as it stands, held to what was
//! measured: nothing was changed to make it pass.

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod support;

use neoteletext_audio_instruments::analog::filter::{CUTOFF_LIMIT, OtaCascade};
use neoteletext_audio_instruments::analog::oscillator::Oscillator;
use neoteletext_audio_instruments::analog::{LEVEL, frequency};
use support::{decibels, decimate, float, on_and_off_the_grid, spectrum};

/// Output samples of a spectrum: bins are 48,000 / 65,536 Hz apart.
const POINTS: usize = 1 << 16;
/// Output samples left out before a spectrum is taken.
const SETTLE: usize = 1_024;
/// The bin of 20 kHz.
const TOP: usize = 20_000 * POINTS / 48_000;
/// The fundamentals measured, as bins: 220 Hz, 880 Hz, 1,760 Hz, 4,186 Hz
/// (the top C of a keyboard of five octaves at 8') and 7 kHz. Each is
/// odd, so that no tone folded once lands on a harmonic.
const FUNDAMENTALS: [usize; 5] = [301, 1_201, 2_403, 5_715, 9_557];
/// The right shift of a mixer's sum into the cascade, in the Juno, the
/// Jupiter-8 and the Prophet-5 alike (each one's `DRIVE_SHIFT`): one
/// waveform at full level is an eighth of the level at which a stage
/// saturates.
const DRIVE_SHIFT: u32 = 2;

/// The cutoffs measured, in hertz: as far open as the cascade goes,
/// 0.45 of a rate of 96 kHz; 20 kHz, where the Jupiter-8's and the
/// Juno's cutoff sliders end; and 5 kHz.
const CUTOFFS: [u32; 3] = [43_200, 20_000, 5_000];

/// A sawtooth of `waveforms` oscillators in step at `factor` times the
/// output rate, through the cascade with no resonance: the strongest
/// tone below 20 kHz that is no harmonic against the strongest
/// harmonic, in decibels.
fn aliasing(factor: u32, bin: usize, waveforms: i32, cutoff: u32) -> f64 {
    let mut oscillator = Oscillator::new();
    oscillator.set_step(((bin as u64) << 32).div_ceil(POINTS as u64 * u64::from(factor)) as u32);
    let mut filter = OtaCascade::new();
    filter.set(frequency(cutoff * 1_000, 48_000 * factor).min(CUTOFF_LIMIT), 0);
    let count = (POINTS + SETTLE) * factor as usize;
    let signal: Vec<i32> = (0..count)
        .map(|_| filter.tick((oscillator.tick(None).sawtooth * waveforms) >> DRIVE_SHIFT).four_pole)
        .collect();
    let (on, off) = on_and_off_the_grid(&spectrum(&float(&decimate(&signal, factor)[SETTLE..])), bin, TOP);
    decibels(off / on)
}

/// What is held at twice the output rate, for each cutoff, each number
/// of waveforms and each fundamental: the figure measured, and 3 dB.
const HELD: [[[f64; 5]; 3]; 3] = [
    [
        [-127.0, -115.0, -110.0, -104.0, -99.0],
        [-117.0, -105.0, -99.0, -93.0, -89.0],
        [-110.0, -98.0, -92.0, -86.0, -82.0],
    ],
    [[-106.0, -94.0, -88.0, -80.0, -75.0], [-94.0, -82.0, -76.0, -68.0, -63.0], [-87.0, -75.0, -69.0, -61.0, -56.0]],
    [[-102.0, -89.0, -82.0, -68.0, -57.0], [-91.0, -78.0, -71.0, -56.0, -45.0], [-84.0, -72.0, -64.0, -49.0, -38.0]],
];

#[test]
fn what_the_cascade_folds_back_is_measured() {
    // One waveform at full level is each instrument's plain patch; two
    // in step are both oscillators of the Jupiter-8 or the Prophet-5, or
    // the Juno's sawtooth and pulse; three, the Juno's sawtooth, pulse
    // and sub-oscillator, and the most the Prophet-5's mixer gives.
    println!("a sawtooth through the cascade, dB: at 220, 880, 1,760, 4,186 and 7,000 Hz");
    // An unoptimised build measures the one cutoff at twice the rate.
    let quick = cfg!(debug_assertions);
    for (cutoff, held) in CUTOFFS.into_iter().zip(HELD).filter(|&(cutoff, _)| !quick || cutoff == 20_000) {
        for (waveforms, held) in (1i32..).zip(held) {
            let found = |factor: u32| FUNDAMENTALS.map(|bin| aliasing(factor, bin, waveforms, cutoff));
            let (twice, four) = (found(2), if quick { [f64::NAN; 5] } else { found(4) });
            let level = f64::from(waveforms * (LEVEL >> DRIVE_SHIFT)) / f64::from(2 * LEVEL);
            println!(
                "cutoff {cutoff} Hz, {waveforms} at full level ({level:.3} of saturation): {twice:6.1?} at twice the output rate, {four:6.1?} at four times"
            );
            for ((found, held), bin) in twice.into_iter().zip(held).zip(FUNDAMENTALS) {
                assert!(found < held, "cutoff {cutoff}, {waveforms} waveforms, bin {bin}: {found} dB, not {held} dB");
            }
        }
    }
}
