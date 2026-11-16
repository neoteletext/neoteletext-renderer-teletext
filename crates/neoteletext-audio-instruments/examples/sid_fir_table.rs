//! Writes `src/sid/fir_table.rs`, the anti-aliasing filter of the SID's
//! decimator, to standard output:
//!
//! `cargo run -p neoteletext-audio-instruments --example sid_fir_table > crates/neoteletext-audio-instruments/src/sid/fir_table.rs`
//!
//! The filter is a windowed sinc (Kaiser window; Kaiser 1974, as given in
//! Oppenheim and Schafer, "Discrete-Time Signal Processing", section 7.5)
//! sampled at [`PHASES`] + 1 fractional positions. The test
//! `tests/sid_decimator.rs` includes this file and compares what it makes
//! with the table in the crate.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a table generator working in f64 on small integers"
)]

/// Taps of each phase, at the decimator's intermediate rate.
pub const TAPS: usize = 160;
/// Fractional positions between two intermediate samples.
pub const PHASES: usize = 32;
/// Unity gain in the table's fixed point (Q17): the largest tap, about 0.195,
/// still fits sixteen bits.
pub const ONE: i32 = 1 << 17;

/// The pass band's edge, in hertz: the top of hearing.
pub const PASS_HZ: f64 = 20_000.0;
/// The stop band's edge, in hertz, at the PAL intermediate rate: what lies
/// above it would fold below 20 kHz at 48 kHz.
pub const STOP_HZ: f64 = 28_000.0;
/// The PAL intermediate rate, the slower of the two and so the one the
/// edges are fitted to: 985,248 Hz / 4.
pub const INTERMEDIATE_HZ: f64 = 246_312.0;
/// The stop band's design attenuation in decibels.
pub const ATTENUATION_DB: f64 = 80.0;

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

/// The prototype low-pass at `offset` intermediate samples from its
/// centre.
fn prototype(offset: f64) -> f64 {
    // Kaiser's formula for the window's shape from the attenuation.
    let beta = 0.1102 * (ATTENUATION_DB - 8.7);
    // The cutoff, in cycles a sample, is the middle of the transition.
    let cutoff = f64::midpoint(PASS_HZ, STOP_HZ) / INTERMEDIATE_HZ;
    let half = TAPS as f64 / 2.0;
    let position = offset / half;
    if position.abs() >= 1.0 {
        return 0.0;
    }
    let window = bessel_i0(beta * (1.0 - position * position).sqrt()) / bessel_i0(beta);
    let angle = 2.0 * core::f64::consts::PI * cutoff * offset;
    let sinc = if angle.abs() < 1e-12 { 1.0 } else { angle.sin() / angle };
    2.0 * cutoff * sinc * window
}

/// The table: row `phase` weighs the window of the last [`TAPS`]
/// intermediate samples, oldest first, for an output instant
/// `TAPS / 2 - 1 + phase / PHASES` samples after the oldest. Each row sums
/// to [`ONE`] exactly.
#[must_use]
pub fn table() -> Vec<[i16; TAPS]> {
    (0..=PHASES)
        .map(|phase| {
            let centre = (TAPS / 2 - 1) as f64 + phase as f64 / PHASES as f64;
            let real: Vec<f64> = (0..TAPS).map(|tap| prototype(tap as f64 - centre)).collect();
            let gain: f64 = real.iter().sum();
            let mut row = [0i16; TAPS];
            for (slot, value) in row.iter_mut().zip(&real) {
                *slot = (value / gain * f64::from(ONE)).round() as i16;
            }
            // Rounding leaves the sum a few units off unity; the largest tap
            // takes the difference, where it changes the shape least.
            let sum: i32 = row.iter().map(|&tap| i32::from(tap)).sum();
            let largest = (0..TAPS).max_by_key(|&tap| row[tap]).unwrap_or(0);
            row[largest] += (ONE - sum) as i16;
            row
        })
        .collect()
}

#[allow(dead_code, reason = "the decimator's test includes this file for `table` alone")]
fn main() {
    println!("//! The SID decimator's anti-aliasing filter, written by");
    println!("//! `examples/sid_fir_table.rs`: do not edit. {TAPS} taps for each of {PHASES} + 1");
    println!("//! fractional positions at the intermediate rate; Kaiser window for");
    println!("//! {ATTENUATION_DB} dB; pass band to {PASS_HZ} Hz and stop band from {STOP_HZ} Hz at");
    println!("//! {INTERMEDIATE_HZ} Hz; each row sums to {ONE}, unity in Q17.");
    println!("//!");
    println!("//! Source: own design; the Kaiser window formulas are Kaiser's (1974), as");
    println!("//! in Oppenheim and Schafer, \"Discrete-Time Signal Processing\", section 7.5.");
    println!();
    println!("/// Taps of each row.");
    println!("pub const TAPS: usize = {TAPS};");
    println!("/// Fractional positions between two intermediate samples; the table has");
    println!("/// one row more, so that every position has a row on each side.");
    println!("pub const PHASES: usize = {PHASES};");
    println!("/// Unity gain in the table's fixed point: Q17.");
    println!("pub const ONE_SHIFT: u32 = {};", ONE.trailing_zeros());
    println!("/// The coefficients: `FILTER[phase][tap]` weighs the window of the last");
    println!("/// [`TAPS`] intermediate samples, oldest first.");
    println!("#[rustfmt::skip]");
    println!("pub const FILTER: [[i16; TAPS]; PHASES + 1] = [");
    for row in table() {
        let numbers: Vec<String> = row.iter().map(ToString::to_string).collect();
        println!("    [{}],", numbers.join(", "));
    }
    println!("];");
}
