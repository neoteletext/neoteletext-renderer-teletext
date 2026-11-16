//! The oscillators of the analogue building blocks: their shapes and
//! levels, and what aliasing their band-limited edges leave (TODO-6430).

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

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::oscillator::{Noise, Oscillator, Waves, waveform};
use neoteletext_audio_instruments::analog::{LEVEL, STEP_LIMIT, frequency};
use support::{decibels, decimate, float, on_and_off_the_grid, spectrum};

/// Output samples of a spectrum: bins are 48,000 / 65,536 Hz apart.
const POINTS: usize = 1 << 16;
/// Output samples left out before a spectrum is taken.
const SETTLE: usize = 256;
/// The bin of 20 kHz.
const TOP: usize = 20_000 * POINTS / 48_000;

/// The step of a tone on bin `bin` of the output's spectrum, for an
/// oscillator at `factor` times the output rate.
fn bin_step(bin: usize, factor: u32) -> u32 {
    ((bin as u64) << 32).div_ceil(POINTS as u64 * u64::from(factor)) as u32
}

/// Renders a waveform at `factor` times the output rate and brings it
/// down: `control` sets the oscillator up, and may change it before each
/// sample.
fn render(factor: u32, pick: fn(&Waves) -> i32, mut control: impl FnMut(&mut Oscillator, usize)) -> Vec<f64> {
    let mut oscillator = Oscillator::new();
    let count = (POINTS + SETTLE) * factor as usize;
    let signal: Vec<i32> = (0..count)
        .map(|index| {
            control(&mut oscillator, index);
            pick(&oscillator.tick(None))
        })
        .collect();
    float(&decimate(&signal, factor)[SETTLE..])
}

/// How far below the strongest harmonic the strongest tone that is no
/// harmonic lies, below 20 kHz, in decibels, for a wave whose harmonics
/// are `base` bins apart.
fn aliasing(wave: &[f64], base: usize) -> f64 {
    let (on, off) = on_and_off_the_grid(&spectrum(wave), base, TOP);
    decibels(off / on)
}

/// The fundamentals measured, as bins: 80 Hz to 4 kHz. Each is odd, so
/// that no tone folded once lands on a harmonic.
const FUNDAMENTALS: [usize; 6] = [109, 601, 1_365, 2_731, 4_097, 5_461];

type Pick = (&'static str, fn(&Waves) -> i32, u32);

/// Each waveform, with the pulse at a half and at a tenth.
const WAVES: [Pick; 5] = [
    ("sawtooth", |waves| waves.sawtooth, 1 << 31),
    ("square", |waves| waves.pulse, 1 << 31),
    ("pulse 10%", |waves| waves.pulse, 429_496_730),
    ("triangle", |waves| waves.triangle, 1 << 31),
    ("sub", |waves| waves.sub, 1 << 31),
];

/// The worst aliasing of every waveform at every fundamental, at a rate.
fn worst_aliasing(factor: u32, fundamentals: &[usize]) -> f64 {
    let mut worst = f64::NEG_INFINITY;
    for (name, pick, width) in WAVES {
        for &bin in fundamentals {
            let wave = render(factor, pick, |oscillator, index| {
                if index == 0 {
                    oscillator.set_step(bin_step(bin, factor));
                    oscillator.set_width(width);
                }
            });
            let found = if name == "sub" { sub_aliasing(&wave, bin) } else { aliasing(&wave, bin) };
            println!("{factor}x {name} at {:.0} Hz: {found:.1} dB", bin as f64 * 48_000.0 / POINTS as f64);
            worst = worst.max(found);
        }
    }
    worst
}

/// The sub-oscillator's aliasing: its harmonics lie on the odd multiples
/// of half the fundamental's bin, which is no whole bin; the grid is
/// taken at half bins by measuring against twice the spectrum's length.
fn sub_aliasing(wave: &[f64], bin: usize) -> f64 {
    let magnitudes = spectrum(wave);
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (index, &magnitude) in magnitudes.iter().enumerate().take(TOP).skip(support::LOBE + 1) {
        // Twice the bin against the harmonics' spacing in half bins.
        let from_line = (2 * index % bin).min(bin - 2 * index % bin);
        if from_line <= 1 {
            on = on.max(magnitude);
        } else if from_line > 2 * support::LOBE {
            off = off.max(magnitude);
        }
    }
    decibels(off / on)
}

#[test]
fn aliasing_is_80_db_down_at_twice_the_output_rate() {
    let worst = worst_aliasing(2, &FUNDAMENTALS);
    println!("worst at twice the rate: {worst:.1} dB");
    assert!(worst < -80.0, "{worst}");
}

#[test]
fn aliasing_is_80_db_down_at_four_times_the_output_rate() {
    let worst = worst_aliasing(4, &FUNDAMENTALS);
    println!("worst at four times the rate: {worst:.1} dB");
    assert!(worst < -80.0, "{worst}");
}

#[test]
fn aliasing_at_the_output_rate_is_what_the_module_says() {
    // The table's low-pass falls between 0.3 and 0.7 of the sampling
    // rate: at the output rate the harmonics between 28 kHz and 33 kHz
    // come back between 15 kHz and 20 kHz only partly reduced.
    let low = worst_aliasing(1, &FUNDAMENTALS[..3]);
    let high = worst_aliasing(1, &FUNDAMENTALS[3..]);
    println!("worst at the output rate: {low:.1} dB to 1 kHz, {high:.1} dB to 4 kHz");
    assert!(low < -45.0, "{low}");
    assert!(high < -30.0, "{high}");
}

#[test]
fn a_pulse_whose_width_moves_at_audio_rate_has_no_clicks() {
    // The width swings between 10% and 90% at 7/20 of the pulse's
    // frequency: every tone of the result is a multiple of one twentieth
    // of the fundamental, here 37 bins, and whatever lies between those
    // lines is aliasing or a click.
    let base = 37;
    for factor in [2, 4] {
        let rate = f64::from(factor) * POINTS as f64;
        let wave = render(
            factor,
            |waves| waves.pulse,
            |oscillator, index| {
                if index == 0 {
                    oscillator.set_step(bin_step(20 * base, factor));
                }
                let swing = (2.0 * PI * 7.0 * base as f64 * index as f64 / rate).sin();
                oscillator.set_width(((0.5 + 0.4 * swing) * 4_294_967_296.0) as u32);
            },
        );
        let found = aliasing(&wave, base);
        println!("{factor}x pulse of moving width: {found:.1} dB");
        assert!(found < -80.0, "{found}");
    }
}

#[test]
fn a_synchronised_oscillator_is_band_limited_too() {
    // The follower's waveform repeats with the leader, so its tones are
    // the leader's harmonics, whatever its own frequency.
    let leader_bin = 601;
    for factor in [2, 4] {
        for (name, pick, width) in WAVES {
            if name == "sub" {
                continue;
            }
            let mut leader = Oscillator::new();
            let mut follower = Oscillator::new();
            leader.enable(0);
            leader.set_step(bin_step(leader_bin, factor));
            // The follower runs 2.83 times as fast as the leader.
            follower.set_step(bin_step(1_701, factor));
            follower.set_width(width);
            let count = (POINTS + SETTLE) * factor as usize;
            let signal: Vec<i32> = (0..count)
                .map(|_| {
                    let _ = leader.tick(None);
                    pick(&follower.tick(leader.started()))
                })
                .collect();
            let wave = float(&decimate(&signal, factor)[SETTLE..]);
            let found = aliasing(&wave, leader_bin);
            println!("{factor}x synchronised {name}: {found:.1} dB");
            assert!(found < -80.0, "{name} at {factor}x: {found}");
        }
    }
}

/// One cycle's worth of samples of each waveform at 100 Hz and the
/// output rate, after the oscillator's delay.
fn shapes() -> Vec<Waves> {
    let mut oscillator = Oscillator::new();
    oscillator.set_step(frequency(100_000, 48_000));
    oscillator.set_width(1 << 30);
    (0..480 * 3).map(|_| oscillator.tick(None)).collect()
}

#[test]
fn the_waveforms_have_their_shapes_and_levels() {
    let waves = shapes();
    let level = f64::from(LEVEL);
    // The first cycle after the delay: sample `DELAY + n` is the naive
    // wave at phase (n + 1) / 480, away from the edges.
    for (n, wave) in waves[Oscillator::DELAY..Oscillator::DELAY + 480].iter().enumerate() {
        let phase = (n + 1) as f64 / 480.0;
        let near_edge = |edge: f64| (phase - edge).abs() < 10.0 / 480.0 || (phase - edge).abs() > 470.0 / 480.0;
        if !near_edge(0.0) {
            assert!((f64::from(wave.sawtooth) / level - (2.0 * phase - 1.0)).abs() < 1e-3, "sawtooth at {phase}");
        }
        if !near_edge(0.0) && !near_edge(0.25) {
            let expected = if phase < 0.25 { 1.0 } else { -1.0 };
            assert!((f64::from(wave.pulse) / level - expected).abs() < 1e-3, "pulse at {phase}");
        }
        if !near_edge(0.0) && !near_edge(0.5) {
            let expected = if phase < 0.5 { 4.0 * phase - 1.0 } else { 3.0 - 4.0 * phase };
            assert!((f64::from(wave.triangle) / level - expected).abs() < 1e-3, "triangle at {phase}");
        }
    }
    // The sub-oscillator turns once a cycle: two cycles are one of its.
    let sub: Vec<i32> = waves[Oscillator::DELAY + 20..].iter().step_by(480).take(2).map(|wave| wave.sub).collect();
    assert!((f64::from(sub[0]) / level).abs() > 0.999 && sub[0].signum() == -sub[1].signum(), "{sub:?}");
    // No waveform overshoots by more than the low-pass's ripple, 9%.
    for wave in &waves {
        for value in [wave.sawtooth, wave.pulse, wave.triangle, wave.sub] {
            assert!(f64::from(value).abs() < 1.2 * level);
        }
    }
}

#[test]
fn the_harmonics_have_the_levels_of_the_ideal_waves_to_20_khz() {
    // At twice the rate: a sawtooth's harmonic n is 2 / (pi n) of its
    // peak, and the 16th of 1,100 Hz is at 17.6 kHz.
    let bin = 1_501;
    let wave = render(
        2,
        |waves| waves.sawtooth,
        |oscillator, index| {
            if index == 0 {
                oscillator.set_step(bin_step(bin, 2));
            }
        },
    );
    let magnitudes = spectrum(&wave);
    for harmonic in [1, 2, 8, 16, 18] {
        let expected = 2.0 * f64::from(LEVEL) / (PI * harmonic as f64);
        let found = decibels(magnitudes[bin * harmonic] / expected);
        assert!(found.abs() < 0.1, "harmonic {harmonic}: {found} dB");
    }
}

#[test]
fn a_disabled_waveform_is_silent_and_the_rest_do_not_change() {
    let mut all = Oscillator::new();
    let mut one = Oscillator::new();
    one.enable(waveform::SAWTOOTH);
    for oscillator in [&mut all, &mut one] {
        oscillator.set_step(frequency(440_000, 96_000));
    }
    for _ in 0..2_000 {
        let (all, one) = (all.tick(None), one.tick(None));
        assert_eq!(one, Waves { sawtooth: all.sawtooth, ..Waves::default() });
    }
}

#[test]
fn every_step_and_width_is_valid() {
    let mut random = support::Random(7);
    let mut oscillator = Oscillator::new();
    let mut leader = Oscillator::new();
    for round in 0..200_000 {
        if round % 16 == 0 {
            oscillator.set_step(random.word() >> (random.word() % 32));
            leader.set_step(random.word() >> (random.word() % 32));
        }
        oscillator.set_width(random.word() & (random.word() | random.word()));
        let _ = leader.tick(None);
        let waves = oscillator.tick(if round % 3 == 0 { leader.started() } else { None });
        for value in [waves.sawtooth, waves.pulse, waves.triangle, waves.sub] {
            // Edges closer than the residual's length add up, but stay
            // far inside the range.
            assert!(value.abs() < 8 * LEVEL, "{value}");
        }
    }
    assert!(oscillator.step() <= STEP_LIMIT);
    oscillator.set_step(u32::MAX);
    assert_eq!(oscillator.step(), STEP_LIMIT);
    oscillator.set_step(0);
    let phase = oscillator.phase();
    let _ = oscillator.tick(None);
    assert_eq!(oscillator.phase(), phase);
}

#[test]
fn noise_is_the_same_from_the_same_seed_and_white() {
    let mut first = Noise::new(1234);
    let mut second = Noise::new(1234);
    let mut other = Noise::new(1235);
    let samples: Vec<i32> = (0..1 << 16).map(|_| first.tick()).collect();
    assert!(samples.iter().all(|&sample| sample == second.tick()));
    assert!(samples.iter().any(|&sample| sample != other.tick()));
    // Zero is a seed like any other.
    assert_ne!(Noise::new(0).tick(), 0);

    // Uniform between the levels: a mean of nothing and a mean square of
    // a third; and nothing of one sample in the next.
    let level = f64::from(LEVEL);
    let count = samples.len() as f64;
    let mean = samples.iter().map(|&sample| f64::from(sample) / level).sum::<f64>() / count;
    let square = samples.iter().map(|&sample| (f64::from(sample) / level).powi(2)).sum::<f64>() / count;
    let lagged =
        samples.windows(2).map(|pair| f64::from(pair[0]) / level * f64::from(pair[1]) / level).sum::<f64>() / count;
    assert!(mean.abs() < 0.01 && (square - 1.0 / 3.0).abs() < 0.01 && lagged.abs() < 0.01, "{mean} {square} {lagged}");
    // Flat: each octave band from 750 Hz up holds its share of the power,
    // within 1 dB.
    let magnitudes = spectrum(&float(&samples));
    let power = |from: usize, to: usize| magnitudes[from..to].iter().map(|m| m * m).sum::<f64>() / (to - from) as f64;
    let reference = power(1 << 10, 1 << 15);
    for octave in 10..15 {
        let found = 10.0 * (power(1 << octave, 2 << octave) / reference).log10();
        assert!(found.abs() < 1.0, "octave from bin {}: {found} dB", 1 << octave);
    }
}
