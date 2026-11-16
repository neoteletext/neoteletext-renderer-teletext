//! The bucket-brigade chorus of the analogue building blocks: its delay
//! and its sweep against what is set, the clock's bounds, and its filters
//! (TODO-6430).

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

use neoteletext_audio_instruments::analog::chorus::{CLOCK_HZ, Chorus, DELAY_MICROSECONDS, STAGES, clock_hz};
use neoteletext_audio_instruments::analog::lfo::Shape;
use neoteletext_audio_instruments::analog::{LEVEL, LIMIT, frequency};
use support::{Random, decibels};

const RATE: u32 = 48_000;
/// The tone the delay is measured with, in hertz: low, so that the
/// filters delay it by a figure that is easy to compute.
const PILOT_HZ: f64 = 200.0;

/// A cutoff's step.
fn step(hz: f64) -> u32 {
    frequency((hz * 1000.0).round() as u32, RATE)
}

/// The phase by which a one-pole low-pass at `cutoff` lags at `hz`, in
/// radians, on the trapezoidal rule's axis.
fn one_pole_lag(hz: f64, cutoff: f64) -> f64 {
    let w = (PI * hz / f64::from(RATE)).tan() / (PI * cutoff.min(0.45 * f64::from(RATE)) / f64::from(RATE)).tan();
    w.atan()
}

/// The same for the third-order filter: a one-pole stage and a two-pole
/// stage of damping one. Its cutoff is held to a quarter of the rate.
fn third_order_lag(hz: f64, cutoff: f64) -> f64 {
    let cutoff = cutoff.min(f64::from(RATE) / 4.0);
    let w = (PI * hz / f64::from(RATE)).tan() / (PI * cutoff / f64::from(RATE)).tan();
    w.atan() + w.atan2(1.0 - w * w)
}

/// The delay the filters add at the pilot tone, in seconds, for a line
/// whose delay is `delay` seconds and whose filters cut at `cutoff`.
fn filters_delay(delay: f64, cutoff: f64) -> f64 {
    let insertion = 0.3 * f64::from(STAGES) / 2.0 / delay;
    (2.0 * third_order_lag(PILOT_HZ, cutoff) + one_pole_lag(PILOT_HZ, insertion)) / (2.0 * PI * PILOT_HZ)
}

/// The delay of each output, in seconds, over each stretch of `window`
/// samples: the pilot tone goes in, and its phase is read at each output.
/// The delays are taken to start near `near` seconds, which settles the
/// whole cycles a phase cannot tell.
fn delays(chorus: &mut Chorus, seconds: f64, window: usize, near: f64) -> Vec<[f64; 2]> {
    let turn = 2.0 * PI * PILOT_HZ / f64::from(RATE);
    let count = (seconds * f64::from(RATE)) as usize / window;
    let mut found = Vec::with_capacity(count);
    let mut last = [near; 2];
    for block in 0..count {
        let mut sums = [(0.0, 0.0); 2];
        for index in block * window..(block + 1) * window {
            let angle = turn * index as f64;
            let outputs = chorus.process((f64::from(LEVEL) / 2.0 * angle.sin()) as i32);
            for (sum, output) in sums.iter_mut().zip(outputs) {
                sum.0 += f64::from(output) * angle.sin();
                sum.1 += f64::from(output) * angle.cos();
            }
        }
        let mut delays = [0.0; 2];
        for ((delay, sum), last) in delays.iter_mut().zip(sums).zip(&mut last) {
            // sin(a - p) correlates as (cos p, -sin p).
            let lag = (-sum.1).atan2(sum.0) / (2.0 * PI * PILOT_HZ);
            let period = 1.0 / PILOT_HZ;
            *delay = lag + ((*last - lag) / period).round() * period;
            *last = *delay;
        }
        found.push(delays);
    }
    found
}

#[test]
fn the_delay_range_is_the_clocks() {
    // 256 stages at 10 kHz to 200 kHz.
    assert_eq!((CLOCK_HZ, STAGES, DELAY_MICROSECONDS), ((10_000, 200_000), 256, (640, 12_800)));
    assert_eq!((clock_hz(640), clock_hz(12_800), clock_hz(3_200), clock_hz(0)), (200_000, 10_000, 40_000, u32::MAX));
}

#[test]
fn a_steady_delay_is_the_delay_set() {
    let cutoff = 10_000.0;
    let mut worst = 0.0f64;
    // The last three are beyond the clock's range, and are held to it.
    for (microseconds, held) in [
        (640, 640),
        (1_000, 1_000),
        (1_660, 1_660),
        (3_500, 3_500),
        (5_350, 5_350),
        (12_800, 12_800),
        (100, 640),
        (0, 640),
        (50_000, 12_800),
    ] {
        let mut chorus = Chorus::new(RATE);
        chorus.set_filter_cutoff(step(cutoff));
        chorus.set_delay(microseconds, 0);
        let set = f64::from(held) / 1.0e6;
        let found = delays(&mut chorus, 0.5, 2_400, set)[9];
        let expected = set + filters_delay(set, cutoff);
        for found in found {
            worst = worst.max((found - expected).abs());
            assert!((found - expected).abs() < 3.0e-6, "{microseconds} us: {found} for {expected}");
        }
    }
    println!("steady delay: within {:.2} millionths of a second", worst * 1.0e6);
}

#[test]
fn the_sweep_is_the_depth_set_and_opposite_in_the_two_outputs() {
    // The middle and the sweep of a slow chorus: 3.5 ms, 1.85 ms either
    // way, a triangle of 0.5 Hz.
    let (base, depth, cutoff) = (3.5e-3, 1.85e-3, 10_000.0);
    let mut chorus = Chorus::new(RATE);
    chorus.set_filter_cutoff(step(cutoff));
    chorus.set_delay(3_500, 1_850);
    chorus.set_rate(step(0.5));
    // Windows of 10 ms over two cycles of the triangle.
    let window = 480;
    let found = delays(&mut chorus, 4.0, window, base);
    let mut worst = 0.0f64;
    for (block, delays) in found.iter().enumerate().skip(2) {
        // The triangle at the middle of the window.
        let phase = ((block as f64 + 0.5) * window as f64 / f64::from(RATE) * 0.5).fract();
        let triangle = if phase < 0.25 {
            4.0 * phase
        } else if phase < 0.75 {
            2.0 - 4.0 * phase
        } else {
            4.0 * phase - 4.0
        };
        for (delay, sign) in delays.iter().zip([1.0, -1.0]) {
            let set = base + sign * depth * triangle;
            // The window's length blurs the triangle's corners by less
            // than the tolerance.
            let expected = set + filters_delay(set, cutoff);
            worst = worst.max((delay - expected).abs());
        }
    }
    let (mut least, mut most) = (f64::MAX, f64::MIN);
    for delays in &found[2..] {
        least = least.min(delays[0]).min(delays[1]);
        most = most.max(delays[0]).max(delays[1]);
    }
    println!(
        "sweep: {:.3} ms to {:.3} ms for {:.3} ms to {:.3} ms set, within {:.1} millionths of a second throughout",
        least * 1.0e3,
        most * 1.0e3,
        (base - depth) * 1.0e3,
        (base + depth) * 1.0e3,
        worst * 1.0e6
    );
    assert!(worst < 5.0e-6, "{worst}");
    // The ends of the sweep, with the filters' delay there; a window of
    // 10 ms that holds a corner reads up to 19 millionths short of it.
    assert!((least - (base - depth) - filters_delay(base - depth, cutoff)).abs() < 20.0e-6, "{least}");
    assert!((most - (base + depth) - filters_delay(base + depth, cutoff)).abs() < 20.0e-6, "{most}");
}

#[test]
fn a_sweep_beyond_the_clocks_range_is_held_to_it() {
    let mut chorus = Chorus::new(RATE);
    chorus.set_filter_cutoff(step(10_000.0));
    chorus.set_delay(1_000, 5_000);
    chorus.set_rate(step(2.0));
    let found = delays(&mut chorus, 1.0, 240, 1.0e-3);
    let least = found[2..].iter().map(|delays| delays[0].min(delays[1])).fold(f64::MAX, f64::min);
    let most = found[2..].iter().map(|delays| delays[0].max(delays[1])).fold(f64::MIN, f64::max);
    // 0.64 ms at the least, with the filters' 34 millionths.
    assert!(least > 0.64e-3 && least < 0.70e-3, "{least}");
    assert!(most > 5.9e-3 && most < 6.1e-3, "{most}");
}

/// The gain of the first output for a tone, the chorus steady.
fn gain(chorus: &mut Chorus, hz: f64) -> f64 {
    let turn = 2.0 * PI * hz / f64::from(RATE);
    let (mut real, mut imaginary) = (0.0, 0.0);
    for index in 0..9_600 {
        let angle = turn * f64::from(index);
        let output = f64::from(chorus.process((f64::from(LEVEL) / 2.0 * angle.sin()) as i32)[0]);
        if index >= 4_800 {
            real += output * angle.sin();
            imaginary += output * angle.cos();
        }
    }
    real.hypot(imaginary) / 2_400.0 / (f64::from(LEVEL) / 2.0)
}

#[test]
fn the_filters_are_two_of_the_third_order_and_the_chips_own() {
    // 3.2 ms: a clock of 40 kHz, whose own cutoff is the data sheet's
    // 12 kHz; and a delay of a whole number of samples, 153.6 apart, so
    // 3.125 ms, 150 samples, where the line's cubic does nothing.
    let delay = 3.125e-3;
    let insertion = 0.3 * 128.0 / delay;
    let w = |hz: f64, cutoff: f64| (PI * hz / f64::from(RATE)).tan() / (PI * cutoff / f64::from(RATE)).tan();
    let mut worst = 0.0f64;
    for cutoff in [4_000.0, 8_000.0] {
        let mut chorus = Chorus::new(RATE);
        chorus.set_filter_cutoff(step(cutoff));
        chorus.set_delay(3_125, 0);
        for hz in [500.0, 2_000.0, 4_000.0, 8_000.0, 12_000.0] {
            // Butterworth's third order: 1 / sqrt(1 + w⁶), twice over.
            let expected = 1.0 / (1.0 + w(hz, cutoff).powi(6)) / w(hz, insertion).hypot(1.0);
            let found = decibels(gain(&mut chorus, hz) / expected);
            worst = worst.max(found.abs());
            assert!(found.abs() < 0.2, "{cutoff} Hz at {hz} Hz: {found} dB");
        }
    }
    println!("filters: within {worst:.3} dB of two third-order low-passes and the chip's one pole");
    // Left to itself the cutoff is a third of the clock at the longest
    // delay: 12.8 ms is a clock of 10 kHz, and 3.3 kHz is 3 dB down in
    // each of the two filters.
    let mut chorus = Chorus::new(RATE);
    chorus.set_delay(6_400, 6_400);
    chorus.set_delay(12_800, 0);
    let found = decibels(gain(&mut chorus, 10_000.0 / 3.0) * w(10_000.0 / 3.0, 3_000.0).hypot(1.0));
    assert!((found + 6.02).abs() < 0.2, "{found}");
}

#[test]
fn the_cubic_between_samples_is_flat_to_a_few_kilohertz() {
    // Half way between two samples, the worst place: 2 kHz and 5 kHz
    // against the same tones at a whole sample.
    let mut whole = Chorus::new(RATE);
    let mut half = Chorus::new(RATE);
    for chorus in [&mut whole, &mut half] {
        chorus.set_filter_cutoff(step(12_000.0));
    }
    whole.set_delay(3_125, 0);
    // 150.5 samples.
    half.set_delay(3_135, 0);
    for (hz, loss) in [(2_000.0, 0.01), (5_000.0, 0.06), (10_000.0, 0.7)] {
        let found = decibels(gain(&mut half, hz) / gain(&mut whole, hz));
        println!("half way between samples at {hz} Hz: {found:.3} dB");
        assert!(found <= 0.01 && found > -loss, "{hz} Hz: {found}");
    }
}

#[test]
fn every_setting_and_input_leaves_the_chorus_bounded_and_the_same_each_time() {
    let run = || -> Vec<[i32; 2]> {
        let mut random = Random(3);
        let mut chorus = Chorus::new(RATE);
        (0..200_000u32)
            .map(|round| {
                if round % 512 == 0 {
                    chorus.set_delay(random.word() >> (random.word() % 32), random.word() >> (random.word() % 32));
                    chorus.set_rate(random.word() >> (random.word() % 32));
                    chorus.set_shape(
                        [Shape::Triangle, Shape::Sine, Shape::Square, Shape::SampleAndHold][round as usize / 512 % 4],
                    );
                    if round % 4096 == 0 {
                        chorus.set_filter_cutoff(random.word());
                    }
                }
                let input = match (round / 8192) % 3 {
                    0 => random.word().cast_signed() >> 4,
                    1 => {
                        if round % 2 == 0 {
                            i32::MAX
                        } else {
                            i32::MIN
                        }
                    }
                    _ => i32::MAX,
                };
                chorus.process(input)
            })
            .collect()
    };
    let outputs = run();
    assert!(outputs.iter().flatten().all(|output| output.abs() <= LIMIT));
    assert_eq!(outputs, run());
    // Any rate makes a chorus, the rate of nothing included.
    for rate in [0, 1, 8_000, 96_000, u32::MAX] {
        let mut chorus = Chorus::new(rate);
        chorus.set_delay(5_000, 2_000);
        assert!(chorus.process(LEVEL).iter().all(|output| output.abs() <= LIMIT));
    }
}
