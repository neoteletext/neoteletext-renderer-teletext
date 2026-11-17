//! The controls' laws: what each slider's position means in hertz,
//! octaves and seconds on each model, and the arithmetic that turns an
//! interval in octaves into a frequency.
//!
//! Roland's documents give the ends of most ranges and a few calibration
//! points; how a slider travels between the ends they do not say. Each
//! figure below names its document, and each law between two documented
//! ends says that it is an own choice and why.

use crate::analog::envelope::ENVELOPE_ONE;

use super::patch::SLIDER_MAX;

/// Fraction bits of an interval in octaves.
pub const OCTAVE_BITS: u32 = 24;
/// One octave.
pub const OCTAVE: i32 = 1 << OCTAVE_BITS;
/// Semitones in an octave.
const SEMITONES: i32 = 12;
/// Cents in an octave.
const CENTS: i32 = 1_200;

/// An interval of so many cents, in octaves.
#[must_use]
pub const fn cents(cents: i32) -> i32 {
    octaves(cents, CENTS)
}

/// `numerator / denominator` octaves, for a fraction below 128 in
/// magnitude; a denominator of zero gives none.
#[must_use]
pub const fn octaves(numerator: i32, denominator: i32) -> i32 {
    match ((numerator as i64) << OCTAVE_BITS).checked_div(denominator as i64) {
        Some(octaves) => octaves as i32,
        None => 0,
    }
}

/// The interval from one key to another, in octaves.
#[must_use]
pub const fn keys(from: u8, to: u8) -> i32 {
    octaves((to as i32).wrapping_sub(from as i32), SEMITONES)
}

/// One in Q30, the fixed point of a ratio between one and two.
const ONE_Q30: u64 = 1 << 30;
/// Sixteenths of an octave: the table's steps.
const TABLE_BITS: u32 = 4;
/// Bits of an interval below a step of the table.
const REST_BITS: u32 = OCTAVE_BITS - TABLE_BITS;
/// The natural logarithm of two in Q30.
///
/// Source: ln 2 = 0.693147180559945, times 2³⁰, rounded.
const LN2_Q30: u64 = 744_261_118;

/// The whole square root of a number, by Newton's iteration.
const fn square_root(value: u128) -> u128 {
    if value < 2 {
        return value;
    }
    let mut estimate = value;
    let mut next = value.div_ceil(2);
    while next < estimate {
        estimate = next;
        next = match value.checked_div(estimate) {
            Some(quotient) => estimate.wrapping_add(quotient) / 2,
            None => estimate,
        };
    }
    estimate
}

/// 2^(k/16) in Q30 for k from 0 to 15: each is a product of the square
/// root of two taken one to four times.
///
/// Source: computed here; the roots are whole square roots of numbers in
/// Q60, so each entry is within a few units of 2⁻³⁰ of its value.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const SIXTEENTHS: [u64; 1 << TABLE_BITS] = {
    // roots[i] = 2^(1 / 2^(i+1)).
    let mut roots = [0u64; TABLE_BITS as usize];
    let mut value = 2 * ONE_Q30;
    let mut index = 0;
    while index < roots.len() {
        value = square_root((value as u128) << 30) as u64;
        roots[index] = value;
        index += 1;
    }
    let mut table = [ONE_Q30; 1 << TABLE_BITS];
    let mut entry = 0;
    while entry < table.len() {
        let mut bit = 0;
        while bit < roots.len() {
            // The top bit of the entry's number is the half octave.
            if entry & (1 << (roots.len() - 1 - bit)) != 0 {
                table[entry] = (table[entry] * roots[bit]) >> 30;
            }
            bit += 1;
        }
        entry += 1;
    }
    table
};

/// `value * 2^octaves` for an interval in octaves with [`OCTAVE_BITS`] of
/// fraction, held to a word: within a thousandth of a cent of it.
///
/// It takes 64-bit products, and belongs where a control changes.
#[must_use]
pub fn raise(value: u32, octaves: i32) -> u32 {
    let whole = octaves >> OCTAVE_BITS;
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let coarse = SIXTEENTHS.get((fraction >> REST_BITS) as usize).copied().unwrap_or(ONE_Q30);
    // The rest is below a sixteenth of an octave: y = rest * ln 2 is
    // below 0.044, and e^y = 1 + y + y²/2 + y³/6 to within 2⁻²⁷.
    let rest = u64::from(fraction & ((1 << REST_BITS) - 1));
    // Every factor is below 2³¹, so no product leaves its word.
    let y = rest.wrapping_mul(LN2_Q30) >> OCTAVE_BITS;
    let square = y.wrapping_mul(y) >> 30;
    let cube = square.wrapping_mul(y) >> 30;
    let fine = ONE_Q30.wrapping_add(y).wrapping_add(square / 2).wrapping_add(cube / 6);
    let ratio = coarse.wrapping_mul(fine) >> 30;
    let scaled = u128::from(value).wrapping_mul(u128::from(ratio));
    let shifted = match whole {
        // Beyond these the result is zero or leaves a word anyway.
        ..=-64 => 0,
        -63..=-1 => scaled >> whole.unsigned_abs().saturating_add(30),
        0..=31 => (scaled << whole.unsigned_abs()) >> 30,
        _ => u128::MAX,
    };
    u32::try_from(shifted).unwrap_or(u32::MAX)
}

/// A value between two ends for a slider's position, along the cube of
/// the position: `low + (high - low) * (slider / 127)³`.
///
/// Source: own choice of the law for the times of both models' envelopes.
/// Roland gives only the ends. A straight line would put half of every
/// range in the slider's first tenth of a second; the pure exponential
/// between ends 2,000 to 8,000 times apart puts the slider's middle at
/// 0.07 to 0.15 s. The cube lies between what the two reference programs
/// measured show at a quarter, a half and three quarters of the travel
/// (the validation table in the instrument's rdm record), and keeps both
/// ends exact.
#[must_use]
pub fn cubic(low: u32, high: u32, slider: u8) -> u32 {
    const FULL: u64 = (SLIDER_MAX as u64).pow(3);
    let position = u64::from(slider.min(SLIDER_MAX));
    let span = u64::from(high.saturating_sub(low));
    let raised = span.saturating_mul(position.pow(3)).saturating_add(FULL / 2) / FULL;
    low.saturating_add(u32::try_from(raised).unwrap_or(u32::MAX))
}

/// A value between two ends for a slider's position, along an
/// exponential: `low * (high / low)^(slider / 127)`, given the whole
/// interval from low to high in octaves.
#[must_use]
pub fn exponential(low: u32, octaves: i32, slider: u8) -> u32 {
    raise(low, share(octaves, slider))
}

/// `amount * slider / 127`, rounded towards zero.
#[must_use]
pub fn share(amount: i32, slider: u8) -> i32 {
    const FULL: i64 = SLIDER_MAX as i64;
    let product = i64::from(amount).saturating_mul(i64::from(slider.min(SLIDER_MAX))) / FULL;
    i32::try_from(product).unwrap_or(0)
}

/// `amount * part / whole`, rounded towards zero; nothing for a whole of
/// zero.
#[must_use]
pub fn share_of(amount: i32, part: i32, whole: i32) -> i32 {
    let product = i64::from(amount).saturating_mul(i64::from(part)).checked_div(i64::from(whole));
    i32::try_from(product.unwrap_or(0)).unwrap_or(0)
}

/// A slider's position as a level in Q30: 127 is one.
#[must_use]
pub fn level(slider: u8) -> i32 {
    share(ENVELOPE_ONE, slider)
}

/// What differs between the two models, but for the oscillator's clock
/// and the envelope's kind, which the voice holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Law {
    /// The cutoff with the slider at 0, the key at middle C and nothing
    /// modulating, in thousandths of a hertz.
    pub cutoff_base: u32,
    /// The octaves the cutoff slider covers from 0 to 127.
    pub cutoff_span: i32,
    /// The lowest and highest cutoff the filter chip reaches, in
    /// thousandths of a hertz.
    pub cutoff_bounds: (u32, u32),
    /// The octaves the envelope at its peak moves the cutoff with its
    /// slider at 127.
    pub cutoff_envelope: i32,
    /// The octaves the low-frequency oscillator at its peak moves the
    /// cutoff either way with its slider at 127.
    pub cutoff_lfo: i32,
    /// The interval the low-frequency oscillator at its peak moves the
    /// pitch either way with its slider at 127, in octaves.
    pub pitch_lfo: i32,
    /// The attack's shortest and longest time, in millionths of a second.
    pub attack: (u32, u32),
    /// The decay's and the release's shortest and longest time.
    pub decay: (u32, u32),
    /// The low-frequency oscillator's lowest rate in thousandths of a
    /// hertz, and the octaves to its highest.
    pub lfo_rate: (u32, i32),
    /// The longest delay of the low-frequency oscillator, in millionths
    /// of a second.
    pub lfo_delay: u32,
    /// The high-pass filter's cutoff at each position of its switch, in
    /// thousandths of a hertz; zero is no filter.
    pub hpf: [u32; 4],
    /// Whether position 0 of the high-pass switch lifts the bass.
    pub bass_boost: bool,
}

/// The base-two logarithm of 10,000 in octaves: the cutoff's range of 5
/// Hz to 50 kHz.
///
/// Source: log₂ 10,000 = 13.287712.
const LOG2_TEN_THOUSAND: i32 = octaves(13_287_712, 1_000_000);

/// The Juno-106.
pub const JUNO_106: Law = Law {
    // Source: JUNO-106 service notes, page 1, Specifications: "VCF CUTOFF
    // FREQ. 5Hz to 50kHz". Own choice: that the slider alone covers that
    // range, exponentially, as the chip's converter makes of a voltage
    // (AMSynths, "All about the IR3109 chip": "anti-log circuitry"). The
    // notes' adjustment 7-1 (page 19: test bank 3, FREQ 6.3, KYBD 10, key
    // C4, trimmed to 248 Hz) cannot be met by any exponential through
    // both ends of the specification with the key follow's pivot at C4,
    // and is not met here: this law gives 1,655 Hz there.
    cutoff_base: 5_000,
    cutoff_span: LOG2_TEN_THOUSAND,
    cutoff_bounds: (5_000, 50_000_000),
    // Source: the same Specifications: "ENV MOD. ±14 octaves", "LFO MOD.
    // ±3.5 octaves" (VCF), "LFO MOD. ±400 cents" (DCO).
    cutoff_envelope: 14 * OCTAVE,
    cutoff_lfo: octaves(7, 2),
    pitch_lfo: cents(400),
    // Source: the same Specifications: "ATTACK TIME 1.5ms to 3s", "DECAY
    // TIME 1.5ms to 12s", "RELEASE TIME 1.5ms to 12s".
    attack: (1_500, 3_000_000),
    decay: (1_500, 12_000_000),
    // Source: the same Specifications: "LFO RATE 0.1Hz to 30Hz", "DELAY
    // TIME 0 to 3s". Own choice: the rate is exponential in the slider
    // and the delay a straight line. The octaves are log₂ 300 = 8.228819.
    lfo_rate: (100, octaves(8_228_819, 1_000_000)),
    lfo_delay: 3_000_000,
    // Source: JUNO-106 service notes, jack board schematic (page 15):
    // the switch IC3 puts C11, 4,700 pF, or C10, 0.015 µF, in series
    // with 47 kΩ into the summing amplifier IC4a: 1 / (2 π R C) is 720.5
    // Hz and 225.8 Hz. Position 1 is flat and position 0 lifts the bass
    // (owner's manual, printed page 18).
    hpf: [0, 0, 225_800, 720_500],
    bass_boost: true,
};

/// The Juno-60.
pub const JUNO_60: Law = Law {
    // Source: JUNO-60 service notes, page 24. Adjustment 8-2: test patch
    // 84 (FREQ 3 of 10, RES 10, KYBD 0) is trimmed to 248 Hz. Adjustment
    // 8-7: patch 87 (FREQ 3.5) under the full LFO swings "between
    // 40-50Hz and 4-5kHz", whose middle is 450 Hz: half a division of
    // the slider is log₂ (450 / 248) = 0.86 octave, so the ten divisions
    // cover 17.2 octaves and the slider's foot is 248 Hz less 5.16
    // octaves, 6.94 Hz.
    cutoff_base: 6_940,
    cutoff_span: octaves(172, 10),
    // Source: JUNO-6 service notes, page 1, Specifications, of the same
    // filter circuit: "VCF 4Hz to 40kHz".
    cutoff_bounds: (4_000, 40_000_000),
    // Source: JUNO-60 service notes, adjustment 8-9: patch 93 (FREQ 0,
    // ENV 10, sustain 10) is trimmed to "approx. 30kHz": log₂ (30,000 /
    // 6.94) = 12.08 octaves (the JUNO-6's Specifications say "ENV
    // modulation (10 octaves max.)" and "LFO modulation (6 octaves
    // max.)"). Adjustment 8-7 above: from 45 Hz to 4.5 kHz
    // is ±3.32 octaves.
    cutoff_envelope: octaves(1_208, 100),
    cutoff_lfo: octaves(332, 100),
    // Source: JUNO-60 service notes, page 14, "Master Oscillators":
    // "Variable range: LFO ±300 cents".
    pitch_lfo: cents(300),
    // Source: JUNO-60 service notes, page 1, Specifications: "ATTACK
    // Time (1ms - 3s)", "DECAY Time (2ms - 12s)", "RELEASE Time (2ms -
    // 12s)".
    attack: (1_000, 3_000_000),
    decay: (2_000, 12_000_000),
    // Source: the same Specifications: "LFO RATE (0.3 - 20Hz)"; the
    // octaves are log₂ (20 / 0.3) = 6.058894. The delay is adjustment
    // 7-4's (page 24), which every instrument was trimmed to: with DELAY
    // TIME at 10 the modulation "2 seconds later, begins to reappear";
    // the Specifications say "DELAY TIME (0 - 1.5s)".
    lfo_rate: (300, octaves(6_058_894, 1_000_000)),
    lfo_delay: 2_000_000,
    // Source: own choice, NOT read from the Juno-60's schematic: position
    // 0 is flat (its owner's manual), and the three others are the
    // Juno-106's two corners with their geometric mean between.
    hpf: [0, 225_800, 403_300, 720_500],
    bass_boost: false,
};

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{OCTAVE, cubic, raise};

    #[test]
    fn raising_is_the_power_of_two_to_a_thousandth_of_a_cent() {
        let mut worst = 0.0f64;
        for octaves in (-12 * OCTAVE..12 * OCTAVE).step_by(250_007) {
            let exact = 1.0e6 * (f64::from(octaves) / f64::from(OCTAVE)).exp2();
            let found = f64::from(raise(1_000_000, octaves));
            // Whole units are all a result has.
            let error = ((found - exact).abs() - 1.0).max(0.0) / exact;
            worst = worst.max(error);
        }
        assert!(worst < 0.001 * 2.0f64.ln() / 1200.0, "{worst}");
        assert_eq!((raise(5, 40 * OCTAVE), raise(u32::MAX, -70 * OCTAVE), raise(8, -3 * OCTAVE)), (u32::MAX, 0, 1));
    }

    #[test]
    fn the_cube_keeps_both_ends() {
        assert_eq!((cubic(1_500, 12_000_000, 0), cubic(1_500, 12_000_000, 127)), (1_500, 12_000_000));
        assert_eq!(cubic(0, 1_000, 255), 1_000);
    }
}
