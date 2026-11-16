//! The filters of the analogue building blocks: their responses against
//! the analogue transfer functions, the pitch at which the four-pole
//! filters oscillate, what the fixed passes leave of their distortion's
//! equations, and their bounds under any input (TODO-6430).

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::needless_range_loop,
    reason = "a test, whose reference filter keeps the equations' stage numbers"
)]

#[path = "analog_support.rs"]
mod support;

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::filter::Bands;
use neoteletext_audio_instruments::analog::filter::{
    CUTOFF_LIMIT, DAMPING_FLOOR, DAMPING_LIMIT, Ladder, ONE_Q28, OnePole, OtaCascade, PASSES, RESONANCE_LIMIT,
    SATURATION, Slopes, StateVariable, excess, saturate,
};
use neoteletext_audio_instruments::analog::oscillator::Noise;
use neoteletext_audio_instruments::analog::{LEVEL, LIMIT, STEP_LIMIT, frequency};
use support::{Random, amplitude, decibels};

/// The rates the filters are measured at: twice and four times the
/// output rate, and the output rate itself.
const RATES: [u32; 3] = [96_000, 192_000, 48_000];

/// A cutoff's step.
fn step(hz: f64, rate: u32) -> u32 {
    frequency((hz * 1000.0).round() as u32, rate)
}

/// A number in Q28.
fn q28(value: f64) -> i32 {
    (value * f64::from(ONE_Q28)).round() as i32
}

/// The gain of a filter at `hz`, for a sine of a given level, measured
/// once the filter has had `settle` seconds to settle in. The tests use a
/// level 36 dB below an oscillator, far below any distortion.
fn gain_at(level: f64, settle: f64, mut filter: impl FnMut(i32) -> i32, hz: f64, rate: u32) -> f64 {
    let cycles = hz / f64::from(rate);
    let settle = (settle * f64::from(rate)) as usize;
    // Twelve cycles, and two thousand samples at least.
    let count = ((12.0 / cycles) as usize).max(2048);
    let mut outputs = Vec::with_capacity(count);
    for index in 0..settle + count {
        let output = filter((level * (2.0 * PI * cycles * index as f64).sin()) as i32);
        if index >= settle {
            outputs.push(f64::from(output));
        }
    }
    amplitude(&outputs, cycles) / level
}

/// The time a filter of a cutoff takes to settle: twelve of its time
/// constants, and a twentieth of a second at least.
fn settling(cutoff: f64) -> f64 {
    (2.0 / cutoff).max(0.05)
}

/// The analogue frequency, as a multiple of the cutoff, that a
/// trapezoidal filter with a prewarped cutoff puts at `hz` (Zavalishin,
/// equation 3.15).
fn warped(hz: f64, cutoff: f64, rate: u32) -> f64 {
    (PI * hz / f64::from(rate)).tan() / (PI * cutoff / f64::from(rate)).tan()
}

/// A complex number, as far as the transfer functions need one.
#[derive(Clone, Copy)]
struct Complex(f64, f64);

impl Complex {
    fn times(self, other: Self) -> Self {
        Self(self.0 * other.0 - self.1 * other.1, self.0 * other.1 + self.1 * other.0)
    }
    fn plus(self, real: f64) -> Self {
        Self(self.0 + real, self.1)
    }
    fn magnitude(self) -> f64 {
        self.0.hypot(self.1)
    }
}

/// |(1 + jw)^power / (k + (1 + jw)⁴)|: the four-pole loop's output after
/// `4 - power` stages (Zavalishin, equation 5.1).
fn four_pole(frequency: f64, resonance: f64, power: usize) -> f64 {
    let stage = Complex(1.0, frequency);
    let square = stage.times(stage);
    let fourth = square.times(square);
    let numerator = (0..power).fold(Complex(1.0, 0.0), |product, _| product.times(stage));
    numerator.magnitude() / fourth.plus(resonance).magnitude()
}

/// The test frequencies around a cutoff, below 0.45 of the rate.
fn around(cutoff: f64, rate: u32) -> Vec<f64> {
    [0.1, 0.25, 0.5, 0.8, 1.0, 1.25, 2.0, 4.0]
        .iter()
        .map(|multiple| multiple * cutoff)
        .filter(|&hz| hz < 0.45 * f64::from(rate) && hz > 20.0)
        .collect()
}

#[test]
fn the_one_pole_filter_is_the_analogue_one_on_a_warped_axis() {
    let mut worst = 0.0f64;
    for rate in RATES {
        for cutoff in [30.0, 300.0, 3_000.0, 15_000.0] {
            for hz in around(cutoff, rate) {
                let w = warped(hz, cutoff, rate);
                let level = f64::from(LEVEL) / 64.0;
                let mut filter = OnePole::new();
                filter.set_cutoff(step(cutoff, rate));
                let low = decibels(gain_at(level, settling(cutoff), |x| filter.low_pass(x), hz, rate) * w.hypot(1.0));
                filter.reset();
                let high =
                    decibels(gain_at(level, settling(cutoff), |x| filter.high_pass(x), hz, rate) * w.hypot(1.0) / w);
                worst = worst.max(low.abs()).max(high.abs());
                assert!(low.abs() < 0.05 && high.abs() < 0.05, "{cutoff} Hz at {hz} Hz and {rate}: {low} {high}");
            }
        }
    }
    println!("one pole: within {worst:.4} dB of the analytic response");
}

#[test]
fn the_four_pole_filters_are_the_analogue_loop_on_a_warped_axis() {
    let mut worst = 0.0f64;
    let mut worst_unwarped = 0.0f64;
    // The lowest cutoff takes the longest to settle, and is measured at
    // one rate; four times the output rate at one cutoff.
    let cases: [(u32, &[f64]); 3] = [
        (96_000, &[200.0, 1_000.0, 5_000.0, 12_000.0]),
        (192_000, &[1_000.0]),
        (48_000, &[1_000.0, 5_000.0, 12_000.0]),
    ];
    for (rate, cutoffs) in cases {
        for &cutoff in cutoffs {
            for resonance in [0.0, 1.0, 3.0, 3.8] {
                for hz in around(cutoff, rate) {
                    let w = warped(hz, cutoff, rate);
                    // Where the response is more than 50 dB down the
                    // output is a few hundred units, too few to measure
                    // a tenth of a decibel on.
                    if four_pole(w, resonance, 0) < 1.0 / 300.0 {
                        continue;
                    }
                    // A resonance of 3.8 raises the signal inside the loop
                    // twentyfold at the cutoff: the sine is 30 dB lower
                    // still around it, to stay out of the curve. And its
                    // ringing takes 12.5 / cutoff seconds to fall by e;
                    // at a resonance of 3, 2.3 / cutoff.
                    let peaked = resonance > 3.5 && (0.4..2.5).contains(&(hz / cutoff));
                    let level = f64::from(LEVEL) / if peaked { 2048.0 } else { 64.0 };
                    let settle = (if resonance > 3.5 { 100.0 } else { 20.0 } / cutoff).max(0.02);
                    let mut ladder = Ladder::new();
                    ladder.set(step(cutoff, rate), q28(resonance));
                    let found =
                        decibels(gain_at(level, settle, |x| ladder.tick(x), hz, rate) / four_pole(w, resonance, 0));

                    let mut cascade = OtaCascade::new();
                    cascade.set(step(cutoff, rate), q28(resonance));
                    let mut outputs = [0.0; 2];
                    for (output, tap) in outputs.iter_mut().zip([2, 0]) {
                        cascade.reset();
                        let pick = |slopes: Slopes| if tap == 2 { slopes.two_pole } else { slopes.four_pole };
                        let found = gain_at(level, settle, |x| pick(cascade.tick(x)), hz, rate);
                        *output = decibels(found / four_pole(w, resonance, tap));
                    }
                    let [two, four] = outputs;

                    let error = found.abs().max(four.abs()).max(two.abs());
                    worst = worst.max(error);
                    // Next to the peak of a resonance of 3.8, 26 dB high
                    // and narrow, a hundredth of a per cent of cutoff is
                    // a tenth of a decibel.
                    let tolerance = if resonance > 3.5 && (0.7..1.3).contains(&(hz / cutoff)) { 0.5 } else { 0.1 };
                    assert!(
                        error < tolerance,
                        "{cutoff} Hz, k {resonance}, at {hz} Hz and {rate}: {found} {four} {two}"
                    );
                    // Against the analogue filter on its own axis, where
                    // the cutoff is a twentieth of the rate or less and
                    // at the cutoff or below.
                    if cutoff <= f64::from(rate) / 20.0 && hz <= cutoff {
                        let analogue = four_pole(hz / cutoff, resonance, 0);
                        let unwarped = decibels(four_pole(w, resonance, 0) / analogue).abs();
                        worst_unwarped = worst_unwarped.max(unwarped);
                    }
                }
            }
        }
    }
    println!("four poles: within {worst:.4} dB of the analytic response on the warped axis;");
    println!(
        "the warped axis is within {worst_unwarped:.4} dB of the analogue one to the cutoff, for cutoffs to rate / 20"
    );
    assert!(worst_unwarped < 0.5, "{worst_unwarped}");
}

#[test]
fn the_state_variable_filter_is_the_analogue_one_on_a_warped_axis() {
    let mut worst = 0.0f64;
    let cases: [(u32, &[f64]); 3] = [
        (96_000, &[200.0, 1_000.0, 5_000.0, 11_000.0]),
        (192_000, &[1_000.0]),
        (48_000, &[1_000.0, 5_000.0, 11_000.0]),
    ];
    for (rate, cutoffs) in cases {
        for &cutoff in cutoffs {
            for damping in [2.0f64.sqrt(), 1.0, 0.25, 0.05] {
                for hz in around(cutoff, rate) {
                    let w = warped(hz, cutoff, rate);
                    // 1 / |1 - w² + j 2R w|.
                    let denominator = (1.0 - w * w).hypot(damping * w);
                    let mut filter = StateVariable::new();
                    filter.set(step(cutoff, rate), q28(damping));
                    // The ringing falls by e in 1 / (pi * cutoff * 2R)
                    // seconds.
                    let settle = settling(cutoff) * (2.0 / damping).max(1.0);
                    let level = f64::from(LEVEL) / 64.0;
                    let mut outputs = [0.0; 3];
                    for (band, output) in outputs.iter_mut().enumerate() {
                        filter.reset();
                        let pick = |bands: Bands| [bands.low, bands.band, bands.high][band];
                        let found = gain_at(level, settle, |x| pick(filter.tick(x)), hz, rate);
                        *output = decibels(found * denominator / w.powi(band as i32));
                    }
                    let [low, band, high] = outputs;
                    let error = low.abs().max(band.abs()).max(high.abs());
                    worst = worst.max(error);
                    let tolerance = if damping < 0.1 && (0.7..1.3).contains(&(hz / cutoff)) { 0.5 } else { 0.1 };
                    assert!(error < tolerance, "{cutoff} Hz, 2R {damping}, at {hz} Hz and {rate}: {low} {band} {high}");
                }
            }
        }
    }
    println!("state variable: within {worst:.4} dB of the analytic response");
}

/// The pitch of `samples` from its rising zero crossings, each placed
/// between two samples by a straight line.
fn pitch(samples: &[i32], rate: u32) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0 && pair[1] >= 0)
        .map(|(index, pair)| index as f64 + f64::from(-pair[0]) / f64::from(pair[1] - pair[0]))
        .collect();
    assert!(crossings.len() > 8, "no oscillation");
    (crossings.len() - 1) as f64 * f64::from(rate) / (crossings[crossings.len() - 1] - crossings[0])
}

/// The last fifth of `seconds` of a filter set off by a click.
fn oscillation(rate: u32, seconds: f64, mut tick: impl FnMut(i32) -> i32) -> Vec<i32> {
    let count = (seconds * f64::from(rate)) as usize;
    let all: Vec<i32> = (0..count).map(|index| tick(if index == 0 { LEVEL } else { 0 })).collect();
    all[count - count / 5..].to_vec()
}

#[test]
fn the_four_pole_filters_oscillate_at_the_cutoff() {
    // Just above four the oscillation is small, the curve is hardly
    // touched, and the pitch is the cutoff's.
    let mut worst = 0.0f64;
    // The oscillation grows by e in 128 / cutoff seconds: at 880 Hz, two
    // seconds bring it to its level.
    let cases: [(u32, &[f64]); 2] = [(96_000, &[880.0, 1_760.0, 7_040.0]), (192_000, &[3_520.0])];
    for (rate, cutoffs) in cases {
        for &cutoff in cutoffs {
            let mut ladder = Ladder::new();
            let mut cascade = OtaCascade::new();
            ladder.set(step(cutoff, rate), q28(4.02));
            cascade.set(step(cutoff, rate), q28(4.02));
            let waves = [
                ("ladder", oscillation(rate, 2_200.0 / cutoff, |x| ladder.tick(x))),
                ("cascade", oscillation(rate, 2_200.0 / cutoff, |x| cascade.tick(x).four_pole)),
            ];
            for (name, samples) in waves {
                let cents = 1200.0 * (pitch(&samples, rate) / cutoff).log2();
                let peak = samples.iter().map(|sample| sample.abs()).max().unwrap_or(0);
                println!(
                    "{name} at {rate}, k 4.02, cutoff {cutoff} Hz: {cents:+.2} cents, peak {:.3} of the saturation level",
                    f64::from(peak) / f64::from(SATURATION)
                );
                worst = worst.max(cents.abs());
                assert!(peak > SATURATION / 256 && peak < SATURATION, "{name}: {peak}");
            }
        }
    }
    println!("oscillation just above four: within {worst:.2} cents of the cutoff");
    assert!(worst < 2.5, "{worst}");
}

#[test]
fn harder_oscillation_is_flatter_as_the_equations_say() {
    // Driven further into the curve, every stage's gain falls and with it
    // the pitch: that is the circuit's equations, not their solution
    // here, which keeps to within a cent of the equations solved out.
    let rate = 96_000;
    let cutoff = 3_520.0;
    for resonance in [4.2, 5.0, 6.0] {
        let mut ladder = Ladder::new();
        let mut cascade = OtaCascade::new();
        ladder.set(step(cutoff, rate), q28(resonance));
        cascade.set(step(cutoff, rate), q28(resonance));
        let mut solved_ladder = ReferenceLadder::new(cutoff, rate, resonance, false);
        let mut solved_cascade = ReferenceLadder::new(cutoff, rate, resonance, true);
        let cents = |samples: &[i32]| 1200.0 * (pitch(samples, rate) / cutoff).log2();
        let found = [
            cents(&oscillation(rate, 0.5, |x| ladder.tick(x))),
            cents(&oscillation(rate, 0.5, |x| cascade.tick(x).four_pole)),
        ];
        let solved = [
            cents(&oscillation(rate, 0.5, |x| solved_ladder.tick(f64::from(x)) as i32)),
            cents(&oscillation(rate, 0.5, |x| solved_cascade.tick(f64::from(x)) as i32)),
        ];
        println!(
            "k {resonance}: ladder {:+.2} cents (solved out {:+.2}), cascade {:+.2} cents (solved out {:+.2})",
            found[0], solved[0], found[1], solved[1]
        );
        for (found, solved) in found.iter().zip(solved) {
            assert!(*found < 0.0 && (found - solved).abs() < 1.0, "k {resonance}: {found} for {solved}");
        }
    }
}

#[test]
fn below_four_the_loop_rings_and_dies_away() {
    for rate in [96_000u32, 48_000] {
        let mut ladder = Ladder::new();
        let mut cascade = OtaCascade::new();
        // At 4 kHz and 3.9 the ringing falls by e in half a millisecond
        // 6 times over: 60 dB in 22 ms.
        ladder.set(step(4_000.0, rate), q28(3.9));
        cascade.set(step(4_000.0, rate), q28(3.9));
        let mut last = (0, 0);
        for index in 0..rate / 2 {
            let input = if index == 0 { LEVEL } else { 0 };
            last = (ladder.tick(input), cascade.tick(input).four_pole);
        }
        assert!(last.0.abs() < 64 && last.1.abs() < 64, "{last:?} at {rate}");
    }
}

/// The transistor ladder's equations solved to convergence in f64: the
/// trapezoidal rule on dy/dt = w (tanh(x) - tanh(y)) for each stage, with
/// u = x - k y4, by the same split into a line and an excess, repeated
/// until it no longer moves.
struct ReferenceLadder {
    gain: f64,
    resonance: f64,
    states: [f64; 4],
    excesses: [f64; 5],
    cascade: bool,
    feedback_excess: f64,
}

impl ReferenceLadder {
    fn new(cutoff: f64, rate: u32, resonance: f64, cascade: bool) -> Self {
        let g = (PI * cutoff / f64::from(rate)).tan();
        Self { gain: g / (1.0 + g), resonance, states: [0.0; 4], excesses: [0.0; 5], cascade, feedback_excess: 0.0 }
    }

    fn tick(&mut self, input: f64) -> f64 {
        let level = f64::from(SATURATION);
        let excess = |value: f64| value - level * (value / level).tanh();
        let g = self.gain;
        let mut outputs = [0.0; 4];
        for _ in 0..40 {
            let mut known = [0.0; 4];
            let mut sum = 0.0;
            for stage in 0..4 {
                let offset =
                    if self.cascade { -self.excesses[stage] } else { self.excesses[stage + 1] - self.excesses[stage] };
                known[stage] = self.states[stage] + g * (offset - self.states[stage]);
                sum = sum * g + known[stage];
            }
            let entering = (input + self.feedback_excess - self.resonance * sum) / (1.0 + self.resonance * g.powi(4));
            let mut signal = entering;
            for stage in 0..4 {
                signal = signal * g + known[stage];
                outputs[stage] = signal;
            }
            if self.cascade {
                let mut before = entering;
                for stage in 0..4 {
                    self.excesses[stage] = excess(before - outputs[stage]);
                    before = outputs[stage];
                }
                self.feedback_excess = excess(self.resonance * outputs[3]);
            } else {
                self.excesses[0] = excess(entering);
                for stage in 0..4 {
                    self.excesses[stage + 1] = excess(outputs[stage]);
                }
            }
        }
        for stage in 0..4 {
            self.states[stage] = 2.0 * outputs[stage] - self.states[stage];
        }
        outputs[3]
    }
}

#[test]
fn the_passes_are_close_to_the_distortions_equations_solved_out() {
    let rate = 96_000;
    let mut worst = f64::NEG_INFINITY;
    for cascade in [false, true] {
        for (cutoff, resonance, drive) in [
            (500.0, 0.0, 1.0),
            (2_000.0, 0.0, 4.0),
            (2_000.0, 3.0, 1.0),
            (2_000.0, 3.0, 4.0),
            (8_000.0, 3.5, 4.0),
            (16_000.0, 2.0, 4.0),
        ] {
            let mut ladder = Ladder::new();
            let mut ota = OtaCascade::new();
            ladder.set(step(cutoff, rate), q28(resonance));
            ota.set(step(cutoff, rate), q28(resonance));
            let mut reference = ReferenceLadder::new(cutoff, rate, resonance, cascade);
            let (mut error, mut power) = (0.0, 0.0);
            // A sawtooth of 440 Hz, naive: its edges are the hardest
            // thing a filter is given.
            for index in 0..rate / 16 {
                let phase = (f64::from(index) * 440.0 / f64::from(rate)).fract();
                let input = (drive * f64::from(LEVEL) * (2.0 * phase - 1.0)) as i32;
                let found = if cascade { ota.tick(input).four_pole } else { ladder.tick(input) };
                let expected = reference.tick(f64::from(input));
                error += (f64::from(found) - expected).powi(2);
                power += expected.powi(2);
            }
            let found = 10.0 * (error / power).log10();
            println!(
                "{} at {cutoff} Hz, k {resonance}, drive {drive}: {PASSES} passes differ by {found:.1} dB",
                if cascade { "cascade" } else { "ladder" }
            );
            worst = worst.max(found);
        }
    }
    println!("worst: {worst:.1} dB");
    assert!(worst < -30.0, "{worst}");
}

#[test]
fn the_curve_is_the_hyperbolic_tangent_at_the_saturation_level() {
    let level = f64::from(SATURATION);
    let (mut worst, mut worst_share) = (0.0f64, 0.0f64);
    let mut last = i32::MIN;
    let mut value = -(1i64 << 31);
    while value < 1 << 31 {
        let t = value as f64 / level;
        let found = saturate(value as i32);
        worst = worst.max((f64::from(found) / level - t.tanh()).abs());
        // What the curve takes off, as a share of itself, wherever it
        // is more than a few units.
        let taken = value as f64 - level * t.tanh();
        if taken.abs() > 4096.0 {
            worst_share = worst_share.max((f64::from(excess(value as i32)) / taken - 1.0).abs());
        }
        // It rises all the way, to the products' last bits, and stays
        // within the saturation level.
        let fallen = i64::from(last) - i64::from(found);
        assert!(fallen <= i64::from(SATURATION >> 12) && found.abs() <= SATURATION, "the curve at {value}");
        last = last.max(found);
        value += 9_973;
    }
    println!("the curve: within {worst:e} of tanh, and what it takes off within {worst_share:e} of itself");
    assert!(worst < 2.0e-4 && worst_share < 1.0e-3, "{worst} {worst_share}");
    assert_eq!((saturate(0), excess(0), saturate(i32::MAX)), (0, 0, SATURATION));
    // A signal 40 dB below the saturation level loses a third of its
    // cube: a thirty-thousandth of itself, 11 units.
    assert!((excess(SATURATION / 100) - 11).abs() <= 1);
}

#[test]
fn every_parameter_and_input_leaves_the_filters_bounded() {
    let mut random = Random(11);
    let mut noise = Noise::new(5);
    let mut ladder = Ladder::new();
    let mut cascade = OtaCascade::new();
    let mut variable = StateVariable::new();
    let mut pole = OnePole::new();
    for round in 0..160_000u32 {
        if round % 64 == 0 {
            // Every cutoff, and resonances and dampings beyond their
            // ranges either way.
            let cutoff = random.word() >> (random.word() % 32);
            let resonance = (random.word() as i32) >> (random.word() % 8);
            ladder.set(cutoff, resonance);
            cascade.set(cutoff, resonance);
            variable.set(cutoff, resonance);
            pole.set_cutoff(cutoff);
        }
        let input = match (round / 4096) % 4 {
            0 => noise.tick() << 7,
            1 => {
                if round % 2 == 0 {
                    i32::MAX
                } else {
                    i32::MIN
                }
            }
            2 => i32::MAX,
            _ => noise.tick(),
        };
        let slopes = cascade.tick(input);
        let bands = variable.tick(input);
        for output in [
            ladder.tick(input),
            slopes.two_pole,
            slopes.four_pole,
            bands.low,
            bands.band,
            bands.high,
            pole.low_pass(input),
            pole.high_pass(input),
        ] {
            assert!(output.abs() <= 2 * LIMIT, "{output} in round {round}");
        }
    }
    // The widest settings, then silence: nothing is left ringing at the
    // bound.
    ladder.set(CUTOFF_LIMIT, RESONANCE_LIMIT);
    cascade.set(CUTOFF_LIMIT, RESONANCE_LIMIT);
    variable.set(STEP_LIMIT, DAMPING_FLOOR);
    let mut last = [0i32; 3];
    for _ in 0..24_000 {
        last = [ladder.tick(0), cascade.tick(0).four_pole, variable.tick(0).low];
    }
    // The two loops oscillate, inside their curve; the third dies away.
    assert!(last[0].abs() < 2 * SATURATION && last[1].abs() < 2 * SATURATION && last[2].abs() < LEVEL, "{last:?}");
    variable.set(STEP_LIMIT, DAMPING_LIMIT);
    assert!(variable.tick(LEVEL).high.abs() <= LEVEL);
}
