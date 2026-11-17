//! The controls' laws: what a lever's position means in octaves, hertz
//! and seconds, and the arithmetic that turns an interval in octaves into
//! a ratio.
//!
//! Yamaha's custom chips are controlled by voltages, and the service
//! manual gives each chip's ends and a calibration point or two. Wherever
//! two points are given they are an octave to the volt: the envelopes'
//! times double for each volt (adjustment procedure, pages 7 and 13: 4 ms
//! at 8 V and 125 ms at 3 V), and the high-pass filter's cutoff found
//! from the alignment procedure's one point and one octave to the volt is
//! the chip's one stated cutoff within 5% ([`CUTOFF_AT_NOTHING`]). A
//! lever is taken to travel its ten volts in a straight line.

use crate::analog::envelope::ENVELOPE_ONE;
use crate::analog::octave::ONE_Q30;
pub use crate::analog::octave::{OCTAVE, OCTAVE_BITS, SEMITONES, cents, keys, octaves};

/// The largest position of a lever in a patch.
///
/// Source: own choice: the largest value of a MIDI data byte. The
/// instrument's levers are continuous.
pub const LEVER_MAX: u8 = 127;
/// The middle of a lever that rests at its centre.
pub const LEVER_CENTRE: u8 = 64;

/// Bits of the table's index: thirty-seconds of an octave.
const TABLE_BITS: u32 = 5;
/// Bits of an interval below a step of the table.
const REST_BITS: u32 = OCTAVE_BITS - TABLE_BITS;
/// The thirty-second root of two in Q60.
///
/// Source: 2^(1/32) = 1.0218971486541166, times 2⁶⁰, rounded.
const STEP_Q60: u128 = 1_178_167_198_179_750_971;
/// The natural logarithm of two in Q30.
///
/// Source: ln 2 = 0.693147180559945, times 2³⁰, rounded.
const LN2_Q30: u64 = 744_261_118;

/// 2^(k/32) in Q30 for k from 0 to 31.
///
/// Source: computed here, each from the last by [`STEP_Q60`] in Q60, so
/// that thirty-one products leave every entry within a unit of Q30.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const POWERS: [u64; 1 << TABLE_BITS] = {
    let mut table = [ONE_Q30; 1 << TABLE_BITS];
    let mut power: u128 = 1 << 60;
    let mut entry = 0;
    while entry < table.len() {
        table[entry] = ((power + (1 << 29)) >> 30) as u64;
        power = (power * STEP_Q60) >> 60;
        entry += 1;
    }
    table
};

/// `value * 2^octaves` for an interval in octaves with [`OCTAVE_BITS`] of
/// fraction, held to a word: within a hundredth of a cent of it.
///
/// It takes 64-bit products, and belongs where a control changes.
///
/// It is kept beside [`crate::analog::octave::raise`], which computes the
/// same power from a table of sixteenths: the two round differently in
/// the last places of a word (one result in six, by up to three parts in
/// ten million), and every figure this instrument was measured with came
/// through this one.
#[must_use]
pub fn raise(value: u32, octaves: i32) -> u32 {
    let whole = octaves >> OCTAVE_BITS;
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let coarse = POWERS.get((fraction >> REST_BITS) as usize).copied().unwrap_or(ONE_Q30);
    // The rest is below a thirty-second of an octave: y = rest * ln 2 is
    // below 0.022, and e^y = 1 + y + y²/2 + y³/6 to within 2⁻³⁰.
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

/// `amount * lever / 127`, rounded towards zero.
#[must_use]
pub fn share(amount: i32, lever: u8) -> i32 {
    const FULL: i64 = LEVER_MAX as i64;
    let product = i64::from(amount).saturating_mul(i64::from(lever.min(LEVER_MAX))) / FULL;
    i32::try_from(product).unwrap_or(0)
}

/// `amount * part / whole`, rounded towards zero; nothing for a whole of
/// zero.
#[must_use]
pub fn share_of(amount: i32, part: i32, whole: i32) -> i32 {
    let product = i64::from(amount).saturating_mul(i64::from(part)).checked_div(i64::from(whole));
    i32::try_from(product.unwrap_or(0)).unwrap_or(0)
}

/// A lever's position as a level in Q30: 127 is one.
#[must_use]
pub fn level(lever: u8) -> i32 {
    share(ENVELOPE_ONE, lever)
}

/// A lever that rests at its centre, as a share of `amount` either way:
/// 0 is all of it downwards, 64 nothing and 127 all of it upwards.
#[must_use]
pub fn centred(amount: i32, lever: u8) -> i32 {
    let lever = i32::from(lever.min(LEVER_MAX));
    let centre = i32::from(LEVER_CENTRE);
    let offset = lever.saturating_sub(centre);
    // The upward travel is one position shorter than the downward.
    let travel = if offset < 0 { centre } else { i32::from(LEVER_MAX).saturating_sub(centre) };
    share_of(amount, offset, travel)
}

/// `a * b` for an interval in octaves and a level in Q30.
#[must_use]
pub const fn scaled(interval: i32, level: i32) -> i32 {
    (((interval as i64).wrapping_mul(level as i64)) >> 30) as i32
}

/// The volts a lever travels, and so the octaves of whatever it sets.
///
/// Source: service manual, page 42, "VCF-EG IC (IG00152)", pins 9 to 11
/// and 14 to 16, and "VCF IC (IG00156)", pin 7: each control is "the
/// voltage between zero V and 10V".
pub const LEVER_VOLTS: i32 = 10;

/// A volt of a control voltage, in the units of an octave: a control of
/// one octave to the volt moves an interval by its voltage.
pub const VOLT: i32 = OCTAVE;

/// The cutoff of either filter, in thousandths of a hertz, with nothing
/// at its control input and the key voltage at 0.25 V (the C below middle
/// C): 32.70 Hz, the C two octaves below that key.
///
/// Source: adjustment procedure, page 11, "VCF - Complete VCF alignment":
/// with the lowest key down (0.125 V) at 2', whose note is 261.6 Hz, and
/// the resonance at its most, each filter's trimmer is set for the most
/// output with "+4.0V at FH terminal" (the high-pass lever) and "+3.3V at
/// FL terminal" (the low-pass lever): the cutoff is then on the note. Own
/// reading: the cutoff is in proportion to the key voltage, as the
/// oscillator's frequency is (service manual, page 42, VCF IC, pin 2:
/// "the designated voltage of the key will supplied to the pin
/// (0.25-4.0V)"), and the high-pass filter's doubles for each volt of its
/// lever, so that 4 V are four octaves; the low-pass filter's then
/// doubles for each 3.3 / 4 V, which is the ratio of the two resistors
/// its control currents meet ([`LPF_LEVER`]). The chip's own figure
/// agrees within 5%: page 42, pin 4, "When the VK is 0.25V and Vf is 5V,
/// the cut off frequency is set to just 1KHz"; this law gives 1,046 Hz.
pub const CUTOFF_AT_NOTHING: u32 = 32_703;
/// The key whose voltage is the 0.25 V of [`CUTOFF_AT_NOTHING`].
///
/// Source: service manual, page 42, "VCO III IC (IG00153)", the table
/// under pin 2: "0.250V 130.8Hz (C2)"; MIDI 1.0 numbers that C 48.
pub const CUTOFF_REFERENCE_KEY: u8 = 48;
/// The volts at the middle of a lever's travel.
pub const LEVER_MIDDLE_VOLTS: i32 = 5;

/// What a control voltage does to each filter's cutoff, in octaves to the
/// volt, in Q30: first the high-pass filter's, then the low-pass
/// filter's.
///
/// Every control reaches a filter's cutoff input as a current through its
/// own resistor, into a resistor to earth that is 2.7 kΩ at the high-pass
/// filter (R6) and 3.3 kΩ at the low-pass filter (R7); a control's share
/// is the lever's 39 kΩ over its own resistor, and the low-pass filter's
/// is 3.3 / 2.7 larger throughout.
///
/// Source: service manual, page 41, M circuit diagram, with its table
/// "Constant value of R6, R7" (rank A: 2.7 kΩ and 3.3 kΩ): the filters'
/// envelope reaches the high-pass filter through 100 kΩ and the low-pass
/// filter through 47 kΩ. Page 39, R51-52 circuit diagram: on each voice's
/// FH line the lever comes through 39 kΩ, the keyboard control (KC), the
/// touch response (IB, AB, TF) and the sub oscillator (SUB) through 120
/// kΩ each and the overall brilliance (BR) through 680 kΩ; on its FL line
/// they come through 39 kΩ each and the brilliance through 220 kΩ.
pub type Weights = (i32, i32);
/// A share of one in Q30.
const fn weight(numerator: i64, denominator: i64) -> i32 {
    match (numerator << 30).checked_div(denominator) {
        Some(share) => share as i32,
        None => 0,
    }
}
/// The low-pass filter's own lever: 3.3 / 2.7 octaves to the volt.
pub const LPF_LEVER: i32 = weight(33, 27);
/// See [`Weights`]: a filter's own lever.
pub const LEVER: Weights = (weight(1, 1), LPF_LEVER);
/// The filters' envelope: 39 / 100, and 39 / 47 of the low-pass lever's.
pub const ENVELOPE: Weights = (weight(39, 100), weight(39 * 33, 47 * 27));
/// The keyboard control, the touch response and the sub oscillator: 39 /
/// 120, and the low-pass lever's own.
pub const MODULATION: Weights = (weight(39, 120), LPF_LEVER);
/// The overall brilliance lever: 39 / 680, and 39 / 220 of the low-pass
/// lever's.
pub const BRILLIANCE: Weights = (weight(39, 680), weight(39 * 33, 220 * 27));

/// The lowest cutoff, in thousandths of a hertz.
///
/// Source: Jürgen Haible, "Analyzing the CS-80 VCF" (2001): "Lower cutoff
/// range is limited by 4.7M resistors across gm cells", with "10uS ==
/// 1kHz": 1 / 4.7 MΩ is 0.213 µS, 21.3 Hz.
pub const CUTOFF_FLOOR: u32 = 21_277;
/// The highest cutoff, in thousandths of a hertz.
///
/// Source: own choice: no document read gives the cells' largest current.
/// It is inside the quarter of the voices' rate that the filter block
/// reaches.
pub const CUTOFF_CEILING: u32 = 20_000_000;

/// The Q at a resonance lever's foot, in Q28: 0.5.
///
/// Source: service manual, page 42, "VCF IC (IG00156)", pin 7, VQ: "When
/// the control voltage is 10V (Min.), Q=0.5".
pub const Q_FOOT: u64 = 1 << 27;
/// The Q at a resonance lever's top, in Q28: 5.
///
/// Source: the same pin: "When the control voltage is 0V (Max.), Q=5".
pub const Q_TOP: u64 = 5 << 28;

/// A filter's damping, 1 / Q, in Q28, for the sum of the positions of
/// its own resonance lever and the overall one: the Q rises in a straight
/// line with the levers, from [`Q_FOOT`] to [`Q_TOP`] over one lever's
/// travel and as far again over the other's, 9.5 with both at their tops,
/// and never past [`DAMPING_LEAST`].
///
/// Source: own choice of the line between the chip's two stated ends,
/// which no document read draws, made by twelve recordings of a real
/// instrument's BRASS 1, whose low-pass resonance row stands at 5.19 V:
/// their filters' peaks fit a Q of 1.8 to 3.3, 2.7 in the middle
/// (`~/devel/teletext-reference/synthesizers/cs-80/validation/table.py`).
/// A Q in a straight line with the voltage is 2.66 there; a damping
/// falling tenfold in equal ratios for equal steps, as this was first
/// written, 1.51, and a damping in a straight line 0.88. That the overall
/// lever adds its travel is a reading of the service manual, page 39,
/// R51-52 circuit diagram: each voice's Q line is fed by its filter's
/// lever (QH or QL) and by the overall lever (RES) through 220 kΩ each.
#[must_use]
pub fn damping(levers: u32) -> u32 {
    const FULL: u64 = LEVER_MAX as u64;
    /// One in Q28, squared: a Q28 over a Q28 in Q28.
    const ONE_SQUARED: u64 = 1 << 56;
    let q = Q_FOOT.saturating_add(Q_TOP.saturating_sub(Q_FOOT).saturating_mul(u64::from(levers)) / FULL);
    let damping = ONE_SQUARED.checked_div(q).unwrap_or(u64::MAX);
    u32::try_from(damping).unwrap_or(u32::MAX).max(DAMPING_LEAST)
}
/// The least damping the filter reaches whatever is added to the
/// resonance, in Q28: a Q of 12.5.
///
/// Source: Haible, "Analyzing the CS-80 VCF" (2001): "maximum Q is
/// limited by 1.2M resistors across damping gm cells", and "approx. 20uS
/// == minimum Q": 1 / 1.2 MΩ is 0.833 µS against the 20.833 µS of a Q of
/// 0.5, a Q 25 times higher. The filter therefore never oscillates by
/// itself.
pub const DAMPING_LEAST: u32 = (1 << 28) * 2 / 25;

/// The shortest attack of either envelope, in millionths of a second, the
/// lever at SHORT.
///
/// Source: service manual, pages 42 and 52, pin 9, AT: "the attack time
/// is controlled from 1 mS until 1S"; adjustment procedure, pages 7 and
/// 13: 4 ms with 8 V at the terminal and 125 ms with 3 V, a doubling for
/// each volt less, which from 1 ms at 10 V is 1.02 s at 0 V.
pub const ATTACK_SHORTEST: u32 = 1_000;
/// The shortest decay and release, in millionths of a second.
///
/// Source: the same pages, pins 10 and 11, 1DT and 2DT: "from 10mS until
/// 10 S".
pub const FALL_SHORTEST: u32 = 10_000;
/// The octaves of time a time lever covers: one for each volt.
pub const TIME_SPAN: i32 = LEVER_VOLTS * OCTAVE;

/// A time lever's time in millionths of a second, from the shortest.
#[must_use]
pub fn time(shortest: u32, lever: u8) -> u32 {
    raise(shortest, share(TIME_SPAN, lever))
}

/// A frequency in thousandths of a hertz that is `frequency` at five
/// volts and `octaves_a_volt` further for each volt, at a lever's
/// position; `more` is added to the lever's volts, in octaves of one to
/// the volt.
#[must_use]
pub fn speed(frequency: u32, octaves_a_volt: i32, lever: u8, more: i32) -> u32 {
    let volts = share(LEVER_VOLTS * OCTAVE, lever).saturating_sub(LEVER_MIDDLE_VOLTS * OCTAVE).saturating_add(more);
    let interval = i64::from(volts).saturating_mul(i64::from(octaves_a_volt)) >> OCTAVE_BITS;
    raise(frequency, i32::try_from(interval).unwrap_or(0))
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{OCTAVE, centred, raise};

    #[test]
    fn raising_is_the_power_of_two_to_a_hundredth_of_a_cent() {
        let mut worst = 0.0f64;
        for octaves in (-12 * OCTAVE..12 * OCTAVE).step_by(250_007) {
            let exact = 1.0e6 * (f64::from(octaves) / f64::from(OCTAVE)).exp2();
            let found = f64::from(raise(1_000_000, octaves));
            // Whole units are all a result has.
            let error = ((found - exact).abs() - 1.0).max(0.0) / exact;
            worst = worst.max(error);
        }
        assert!(worst < 0.01 * 2.0f64.ln() / 1200.0, "{worst}");
        assert_eq!((raise(5, 40 * OCTAVE), raise(u32::MAX, -70 * OCTAVE), raise(8, -3 * OCTAVE)), (u32::MAX, 0, 1));
    }

    #[test]
    fn a_centred_lever_rests_at_nothing_and_ends_at_all() {
        assert_eq!(
            (centred(1_000, 0), centred(1_000, 64), centred(1_000, 127), centred(1_000, 255)),
            (-1_000, 0, 1_000, 1_000)
        );
    }
}
