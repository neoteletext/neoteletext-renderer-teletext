//! synth6581's decimator: the filter table against its generator and its
//! claims, what aliasing is left at the output, and the count of chip
//! cycles against samples (REQ-6412 item 4).

// Without synth6581's feature there is nothing here to test.
#![cfg(feature = "synth6581")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test"
)]

#[path = "../examples/synth6581_fir_table.rs"]
mod generator;
#[path = "synth6581_support.rs"]
mod support;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth6581::fir_table::{FILTER, ONE_SHIFT, PHASES, TAPS};
use neoteletext_audio_instruments::synth6581::{ChipClock, Synth6581, Variant};
use support::{CLOCKS, decibels, pitch, render, spectrum, start};

const SAWTOOTH: u8 = 0x20;
const PULSE: u8 = 0x40;
const MODE_VOLUME: u8 = 24;

#[test]
fn the_table_is_what_its_generator_writes() {
    assert_eq!((TAPS, PHASES, 1i32 << ONE_SHIFT), (generator::TAPS, generator::PHASES, generator::ONE));
    let generated = generator::table();
    assert_eq!(generated.len(), FILTER.len());
    for (phase, (made, kept)) in generated.iter().zip(&FILTER).enumerate() {
        for (tap, (&made, &kept)) in made.iter().zip(kept).enumerate() {
            // One unit for a rounding that another machine's sine may
            // take the other way.
            assert!((i32::from(made) - i32::from(kept)).abs() <= 1, "phase {phase} tap {tap}: {made} and {kept}");
        }
    }
}

/// A row's gain at `hz` when its taps are `rate` apart.
fn gain(row: &[i16; TAPS], hz: f64, rate: f64) -> f64 {
    let step = 2.0 * std::f64::consts::PI * hz / rate;
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (tap, &weight) in row.iter().enumerate() {
        real += f64::from(weight) * (step * tap as f64).cos();
        imaginary += f64::from(weight) * (step * tap as f64).sin();
    }
    real.hypot(imaginary) / f64::from(1i32 << ONE_SHIFT)
}

#[test]
fn the_table_is_flat_to_20_khz_and_76_db_down_from_28_khz() {
    // The stop band's edge is 28 kHz at the PAL intermediate rate, and the
    // same share of the NTSC rate, 29.1 kHz. What lies between 24 kHz and
    // the edge folds to between the edge's mirror, 20 kHz (18.9 kHz), and
    // 24 kHz.
    for clock in CLOCKS {
        let rate = f64::from(clock.hz()) / 4.0;
        let edge = generator::STOP_HZ / generator::INTERMEDIATE_HZ * rate;
        let (mut ripple, mut leak) = (0.0f64, 0.0f64);
        for row in &FILTER {
            let mut hz = 0.0;
            while hz <= generator::PASS_HZ {
                ripple = ripple.max(decibels(gain(row, hz, rate)).abs());
                hz += 250.0;
            }
            let mut hz = edge;
            while hz <= rate / 2.0 {
                leak = leak.max(gain(row, hz, rate));
                hz += 97.0;
            }
        }
        println!(
            "{clock:?}: second stage ripple {ripple:.4} dB to 20 kHz, stop band {:.1} dB from {edge:.0} Hz",
            decibels(leak)
        );
        assert!(ripple < 0.01, "{clock:?}: {ripple} dB of ripple");
        assert!(decibels(leak) < -76.0, "{clock:?}: stop band at {} dB", decibels(leak));
    }
}

#[test]
fn the_first_stage_is_75_db_down_where_it_folds() {
    // Four boxcars of four cycles: (sin(4x) / (4 sin(x)))^4 with x = π f /
    // clock. What folds into the band the second stage passes, 0 to 24
    // kHz, lies within 24 kHz of a multiple of the intermediate rate.
    for clock in CLOCKS {
        let hz = f64::from(clock.hz());
        let response = |f: f64| {
            let x = std::f64::consts::PI * f / hz;
            ((4.0 * x).sin() / (4.0 * x.sin())).abs().powi(4)
        };
        let (mut audible, mut any) = (0.0f64, 0.0f64);
        for image in 1..=2 {
            let centre = f64::from(image) * hz / 4.0;
            let mut offset = -24_000.0;
            while offset <= 24_000.0 {
                let level = response(centre + offset);
                any = any.max(level);
                if offset.abs() <= 20_000.0 {
                    audible = audible.max(level);
                }
                offset += 100.0;
            }
        }
        println!(
            "{clock:?}: first stage {:.1} dB within 24 kHz of its images, {:.1} dB within 20 kHz",
            decibels(any),
            decibels(audible)
        );
        assert!(decibels(any) < -74.9, "{clock:?}: {} dB", decibels(any));
        assert!(decibels(audible) < -81.0, "{clock:?}: {} dB", decibels(audible));
        // And it takes a third of a decibel at 20 kHz.
        assert!(decibels(response(20_000.0)) > -0.36);
    }
}

/// The largest component of `samples` below 20 kHz that is not within
/// `guard` bins of a harmonic of `hz`, against the fundamental, in
/// decibels.
fn inharmonic(samples: &[i16], hz: f64, guard: usize) -> f64 {
    let found = spectrum(samples);
    let bin_hz = f64::from(SAMPLE_RATE) / samples.len() as f64;
    let fundamental = found[(hz / bin_hz).round() as usize - guard..][..2 * guard].iter().copied().fold(0.0, f64::max);
    let mut worst = 0.0f64;
    for (bin, &level) in found.iter().enumerate().skip(guard) {
        let at = bin as f64 * bin_hz;
        if at > 20_000.0 {
            break;
        }
        let harmonic = (at / hz).round();
        if (at - harmonic * hz).abs() > guard as f64 * bin_hz {
            worst = worst.max(level);
        }
    }
    decibels(worst / fundamental)
}

#[test]
fn what_folds_below_20_khz_is_75_db_under_the_tone() {
    // Frequency $8000 is a period of exactly 512 chip cycles, so every
    // component the chip itself makes, its own images included, is a
    // harmonic of 1,924 Hz (1,998 Hz). The harmonics above 28 kHz fold, at
    // 48 kHz, to frequencies between the harmonics below 20 kHz; whatever
    // is found there is the decimator's doing.
    const SAMPLES: usize = 1 << 16;
    for clock in CLOCKS {
        for (control, name) in [(SAWTOOTH, "sawtooth"), (PULSE, "pulse")] {
            let mut synth6581 = Synth6581::new(Variant::Synth8580, clock);
            synth6581.write(MODE_VOLUME, 0x0F);
            synth6581.write(3, 0x08);
            start(&mut synth6581, 0, 0x8000, control);
            let _ = render(&mut synth6581, 0.5);
            let mut samples = vec![0i16; SAMPLES];
            synth6581.render(&mut samples);
            let hz = pitch(0x8000, clock);
            let level = inharmonic(&samples, hz, 8);
            println!("{clock:?} {name} at {hz:.1} Hz: largest component between harmonics {level:.1} dB");
            assert!(level < -75.0, "{clock:?} {name}: {level} dB");
        }
    }
}

#[test]
fn a_tone_of_no_whole_period_has_only_the_chips_own_images() {
    // At a pitch whose period is no whole number of cycles, the chip's own
    // sampling of its waveforms puts components between the harmonics, as
    // the real chip's does: a sawtooth of period P cycles has them at 1 /
    // P and less of its level. The decimator must add nothing near that.
    let clock = ChipClock::Pal;
    let mut synth6581 = Synth6581::new(Variant::Synth8580, clock);
    synth6581.write(MODE_VOLUME, 0x0F);
    start(&mut synth6581, 0, 0xFFFF, SAWTOOTH);
    let _ = render(&mut synth6581, 0.5);
    let mut samples = vec![0i16; 1 << 16];
    synth6581.render(&mut samples);
    let level = inharmonic(&samples, pitch(0xFFFF, clock), 8);
    // Period 256.004 cycles: the chip's own images are 48 dB down at most.
    println!("sawtooth at {:.1} Hz: largest component between harmonics {level:.1} dB", pitch(0xFFFF, clock));
    assert!(level < -46.0, "{level} dB");
}

#[test]
fn ten_seconds_of_samples_are_ten_seconds_of_chip_cycles() {
    for clock in CLOCKS {
        let mut synth6581 = Synth6581::new(Variant::Synth6581, clock);
        let mut block = [0i16; 960];
        let mut rendered = 0u64;
        for second in 1..=10u64 {
            for _ in 0..50 {
                synth6581.render(&mut block);
                rendered += block.len() as u64;
            }
            // Exact to the four cycles of one intermediate sample, at
            // every second and not on average alone.
            let cycles = u64::from(clock.hz()) * second;
            assert!(
                synth6581.cycles().abs_diff(cycles) < 4,
                "{clock:?} second {second}: {} for {cycles}",
                synth6581.cycles()
            );
        }
        assert_eq!(rendered, 10 * u64::from(SAMPLE_RATE));
        // An odd number of samples at a time comes to the same count.
        let mut odd = Synth6581::new(Variant::Synth6581, clock);
        let mut left = rendered as usize;
        while left > 0 {
            let count = left.min(977);
            odd.render(&mut block[..count.min(960)]);
            left -= count.min(960);
        }
        assert_eq!(odd.cycles(), synth6581.cycles());
    }
}
