//! Writes `src/analog/tables.rs`, the tables of the analogue building
//! blocks, to standard output:
//!
//! `cargo run -p neoteletext-audio-instruments --features analog --example analog_tables > crates/neoteletext-audio-instruments/src/analog/tables.rs`
//!
//! The tables are a saturation curve, the filters' frequency warping, the
//! residuals that band-limit an oscillator's steps and corners, and the
//! decimators' low-pass filters. The test `tests/analog_tables.rs` includes
//! this file and compares what it makes with the tables in the crate.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a table generator working in f64 on small integers"
)]

use core::f64::consts::PI;

/// Steps of the saturation table for each unit of its argument.
pub const TANH_STEPS: usize = 32;
/// The saturation table's last argument: tanh(8) is 1 to seven places.
pub const TANH_RANGE: usize = 8;
/// Unity of the saturation table: Q16, with the value 1 held to 65,535.
pub const TANH_ONE: f64 = 65_536.0;

/// Steps of the warping table over a quarter turn.
pub const WARP_STEPS: usize = 256;
/// Unity of the warping table: Q30, so that pi, its largest value, fits.
pub const WARP_ONE: f64 = 1_073_741_824.0;

/// Samples a step's residual covers: half before the step and half after.
pub const BLEP_TAPS: usize = 16;
/// Fractional positions of a step between two samples.
pub const BLEP_PHASES: usize = 64;
/// The stop band's design attenuation of the step's low-pass, in decibels.
pub const BLEP_ATTENUATION_DB: f64 = 96.0;
/// Unity of the step residual: Q15.
pub const BLEP_ONE: f64 = 32_768.0;
/// Unity of the corner residual: Q17 of a sample's slope.
pub const BLAMP_ONE: f64 = 131_072.0;
/// Points of the numerical integral in each sample.
const INTEGRAL_POINTS: usize = 512;

/// The half-band filters' unity: Q32 for the taps beside the centre.
pub const HALF_BAND_ONE: f64 = 4_294_967_296.0;
/// The sharp half-band filter: the farthest tap from the centre, and its
/// design attenuation. It takes twice the output rate to the output rate.
pub const SHARP_REACH: usize = 47;
/// The sharp filter's design attenuation in decibels.
pub const SHARP_ATTENUATION_DB: f64 = 92.0;
/// The wide half-band filter, which takes four times the output rate to
/// twice: its farthest tap and design attenuation.
pub const WIDE_REACH: usize = 15;
/// The wide filter's design attenuation in decibels.
pub const WIDE_ATTENUATION_DB: f64 = 96.0;

/// The zeroth-order modified Bessel function of the first kind, by its
/// power series.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let mut k = 1.0;
    while term > sum * 1e-17 {
        term *= (x / (2.0 * k)) * (x / (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

/// Kaiser's window for an attenuation in decibels, at `position` from -1
/// to 1 across it (Kaiser 1974, as in Oppenheim and Schafer,
/// "Discrete-Time Signal Processing", section 7.5).
fn kaiser(attenuation_db: f64, position: f64) -> f64 {
    if position.abs() >= 1.0 {
        return 0.0;
    }
    let beta = 0.1102 * (attenuation_db - 8.7);
    bessel_i0(beta * (1.0 - position * position).sqrt()) / bessel_i0(beta)
}

/// sin(pi x) / (pi x).
fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 { 1.0 } else { (PI * x).sin() / (PI * x) }
}

/// The saturation curve: tanh at every 1 / [`TANH_STEPS`] from 0 to
/// [`TANH_RANGE`], in Q16.
#[must_use]
pub fn tanh_table() -> Vec<u16> {
    (0..=TANH_RANGE * TANH_STEPS)
        .map(|index| ((index as f64 / TANH_STEPS as f64).tanh() * TANH_ONE).round().min(65_535.0) as u16)
        .collect()
}

/// The warping: for an angle x of 0 to a quarter turn, pi times
/// tan(x) / (x (1 + tan(x))), written sin(x) / (x (sin(x) + cos(x))) so
/// that the quarter turn itself is finite. Times the cutoff as a share of
/// the sampling rate, it is g / (1 + g) for the prewarped g = tan(x).
#[must_use]
pub fn warp_table() -> Vec<u32> {
    (0..=WARP_STEPS)
        .map(|index| {
            let x = PI / 2.0 * index as f64 / WARP_STEPS as f64;
            let ratio = if index == 0 { 1.0 } else { x.sin() / (x * (x.sin() + x.cos())) };
            (PI * ratio * WARP_ONE).round() as u32
        })
        .collect()
}

/// The low-pass a step is seen through, at `offset` samples from its
/// centre: a sinc that cuts at half the sampling rate, under Kaiser's
/// window over [`BLEP_TAPS`] samples.
fn step_low_pass(offset: f64) -> f64 {
    sinc(offset) * kaiser(BLEP_ATTENUATION_DB, offset / (BLEP_TAPS as f64 / 2.0))
}

/// The band-limited step and ramp, less the ideal ones, at every
/// 1 / ([`BLEP_PHASES`] * [`INTEGRAL_POINTS`]) of a sample from
/// -[`BLEP_TAPS`] / 2 to [`BLEP_TAPS`] / 2: the running integral of the
/// low-pass by the trapezium rule, and the running integral of that.
fn residuals() -> (Vec<f64>, Vec<f64>) {
    let points = BLEP_TAPS * INTEGRAL_POINTS;
    let half = BLEP_TAPS as f64 / 2.0;
    let width = 1.0 / INTEGRAL_POINTS as f64;
    let mut step = vec![0.0; points + 1];
    for point in 1..=points {
        let (before, after) = ((point - 1) as f64 * width - half, point as f64 * width - half);
        step[point] = step[point - 1] + (step_low_pass(before) + step_low_pass(after)) * width / 2.0;
    }
    let area = step[points];
    for (point, value) in step.iter_mut().enumerate() {
        // The ideal step is 1 from the centre on.
        *value = *value / area - if point >= points / 2 { 1.0 } else { 0.0 };
    }
    let mut ramp = vec![0.0; points + 1];
    for point in 1..=points {
        // The step's residual jumps by one at the centre: the trapezium
        // ending there takes the value from below.
        let after = if point == points / 2 { step[point] + 1.0 } else { step[point] };
        ramp[point] = ramp[point - 1] + (step[point - 1] + after) * width / 2.0;
    }
    (step, ramp)
}

/// One residual sampled as the oscillators read it: row `phase` is for a
/// step that happened `phase` / [`BLEP_PHASES`] of a sample before a
/// sample instant, and its tap `tap` is the residual
/// `tap - BLEP_TAPS / 2 + phase / BLEP_PHASES` samples after the step.
///
/// The step's residual jumps down by `jump` at the step itself. The last
/// row meets it at one tap, and takes the value from before the step
/// there: a position between the last two rows is read from both, and is
/// before the step.
fn sampled(residual: &[f64], one: f64, jump: f64) -> Vec<[i16; BLEP_TAPS]> {
    let stride = INTEGRAL_POINTS / BLEP_PHASES;
    (0..=BLEP_PHASES)
        .map(|phase| {
            let mut row = [0i16; BLEP_TAPS];
            for (tap, slot) in row.iter_mut().enumerate() {
                let point = tap * INTEGRAL_POINTS + phase * stride;
                let before = if phase == BLEP_PHASES && point == residual.len() / 2 { jump } else { 0.0 };
                *slot = ((residual[point] + before) * one).round() as i16;
            }
            row
        })
        .collect()
}

/// The step residual's table, in Q15 of the step's height.
#[must_use]
pub fn blep_table() -> Vec<[i16; BLEP_TAPS]> {
    sampled(&residuals().0, BLEP_ONE, 1.0)
}

/// The corner residual's table, in Q17 of the change of slope in a sample.
#[must_use]
pub fn blamp_table() -> Vec<[i16; BLEP_TAPS]> {
    sampled(&residuals().1, BLAMP_ONE, 0.0)
}

/// A half-band low-pass: the taps at 1, 3, 5 ... `reach` samples from the
/// centre, in Q32. The centre tap is a half and the even taps are zero; a
/// sinc that cuts at a quarter of the sampling rate under Kaiser's window.
/// The taps are scaled so that they sum to a quarter exactly, which makes
/// the filter's gain one.
#[must_use]
pub fn half_band(reach: usize, attenuation_db: f64) -> Vec<i32> {
    let real: Vec<f64> = (1..=reach)
        .step_by(2)
        .map(|tap| {
            let offset = tap as f64;
            0.5 * sinc(offset / 2.0) * kaiser(attenuation_db, offset / (reach + 1) as f64)
        })
        .collect();
    let sum: f64 = real.iter().sum();
    let mut taps: Vec<i32> = real.iter().map(|value| (value / sum * HALF_BAND_ONE / 4.0).round() as i32).collect();
    let total: i64 = taps.iter().map(|&tap| i64::from(tap)).sum();
    // The nearest tap, the largest, takes what rounding left.
    taps[0] += (HALF_BAND_ONE as i64 / 4 - total) as i32;
    taps
}

/// A number as Rust source, its digits in threes once there are more than
/// five.
fn separated(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) && digits.len() > 5 {
            text.push('_');
        }
        text.push(digit);
    }
    if value < 0 { format!("-{text}") } else { text }
}

/// Prints a table of numbers, a few to a line.
fn print_numbers<T: Copy + Into<i64>>(values: &[T], to_a_line: usize) {
    for line in values.chunks(to_a_line) {
        let numbers: Vec<String> = line.iter().map(|&value| separated(value.into())).collect();
        println!("    {},", numbers.join(", "));
    }
}

/// Prints a residual table.
fn print_rows(rows: &[[i16; BLEP_TAPS]]) {
    for row in rows {
        let numbers: Vec<String> = row.iter().map(|&value| separated(i64::from(value))).collect();
        println!("    [{}],", numbers.join(", "));
    }
}

#[allow(dead_code, reason = "the tables' test includes this file for the tables alone")]
fn main() {
    println!("//! The tables of the analogue building blocks, written by");
    println!("//! `examples/analog_tables.rs`: do not edit.");
    println!("//!");
    println!("//! Source: own design throughout. The window is Kaiser's (1974), as in");
    println!("//! Oppenheim and Schafer, \"Discrete-Time Signal Processing\", section 7.5.");
    println!();
    println!("/// Steps of [`TANH`] for each unit of its argument.");
    println!("pub const TANH_STEPS_SHIFT: u32 = {};", TANH_STEPS.trailing_zeros());
    println!("/// The hyperbolic tangent at every 1 / {TANH_STEPS} from 0 to {TANH_RANGE}, in Q16, with 1");
    println!("/// held to 65,535. Read with a straight line between entries it is within");
    println!("/// 0.0001 of the function (`tests/analog_tables.rs`).");
    println!("#[rustfmt::skip]");
    println!("pub const TANH: [u16; {}] = [", TANH_RANGE * TANH_STEPS + 1);
    print_numbers(&tanh_table(), 16);
    println!("];");
    println!();
    println!("/// Steps of [`WARP`] over a quarter turn, as a shift.");
    println!("pub const WARP_STEPS_SHIFT: u32 = {};", WARP_STEPS.trailing_zeros());
    println!("/// For an angle x at every 1 / {WARP_STEPS} of a quarter turn, pi * tan(x) /");
    println!("/// (x * (1 + tan(x))) in Q30: times a cutoff as a share of the sampling rate,");
    println!("/// it is g / (1 + g) for the prewarped g = tan(x) of a trapezoidal");
    println!("/// integrator (Zavalishin, \"The Art of VA Filter Design\" rev. 2.1.2,");
    println!("/// equations 3.13 and 3.29).");
    println!("#[rustfmt::skip]");
    println!("pub const WARP: [u32; {}] = [", WARP_STEPS + 1);
    print_numbers(&warp_table(), 8);
    println!("];");
    println!();
    println!("/// Samples a residual covers: half before the step and half after.");
    println!("pub const BLEP_TAPS: usize = {BLEP_TAPS};");
    println!("/// Fractional positions of a step between two samples, as a shift; the");
    println!("/// tables have one row more, so that every position has a row on each side.");
    println!("pub const BLEP_PHASES_SHIFT: u32 = {};", BLEP_PHASES.trailing_zeros());
    println!("/// What a unit step seen through a low-pass differs by from the step");
    println!("/// itself, in Q15: `BLEP[phase][tap]` is the difference `tap - {}` +", BLEP_TAPS / 2);
    println!("/// `phase / {BLEP_PHASES}` samples after the step. The low-pass is a sinc that cuts at");
    println!("/// half the sampling rate under a Kaiser window for {BLEP_ATTENUATION_DB} dB over {BLEP_TAPS} samples");
    println!("/// (the band-limited step of Brandt, \"Hard sync without aliasing\", ICMC");
    println!("/// 2001, kept symmetric rather than minimum-phase).");
    println!("#[rustfmt::skip]");
    println!("pub const BLEP: [[i16; BLEP_TAPS]; {}] = [", BLEP_PHASES + 1);
    print_rows(&blep_table());
    println!("];");
    println!();
    println!("/// The same for a corner: what a ramp that starts with a slope of one a");
    println!("/// sample differs by, seen through the same low-pass, in Q17 of a sample:");
    println!("/// the integral of [`BLEP`].");
    println!("#[rustfmt::skip]");
    println!("pub const BLAMP: [[i16; BLEP_TAPS]; {}] = [", BLEP_PHASES + 1);
    print_rows(&blamp_table());
    println!("];");
    println!();
    println!("/// The sharp half-band low-pass, from twice the output rate to the output");
    println!("/// rate: the taps 1, 3, 5 ... {SHARP_REACH} samples from the centre, in Q32. The centre");
    println!("/// tap is a half and the even taps are zero. Kaiser window for {SHARP_ATTENUATION_DB} dB.");
    println!("#[rustfmt::skip]");
    println!("pub const HALF_BAND_SHARP: [i32; {}] = [", SHARP_REACH.div_ceil(2));
    print_numbers(&half_band(SHARP_REACH, SHARP_ATTENUATION_DB), 8);
    println!("];");
    println!();
    println!("/// The wide half-band low-pass, from four times the output rate to twice:");
    println!("/// the taps 1, 3, 5 ... {WIDE_REACH} samples from the centre, in Q32. Kaiser window");
    println!("/// for {WIDE_ATTENUATION_DB} dB.");
    println!("#[rustfmt::skip]");
    println!("pub const HALF_BAND_WIDE: [i32; {}] = [", WIDE_REACH.div_ceil(2));
    print_numbers(&half_band(WIDE_REACH, WIDE_ATTENUATION_DB), 8);
    println!("];");
}
