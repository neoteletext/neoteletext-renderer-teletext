//! The controls' laws: what each knob's and switch's position means in
//! octaves, hertz, seconds and levels, and the arithmetic that turns an
//! interval in octaves into a frequency.
//!
//! The figures come from Moog's service manual of the Minimoog
//! (`Minimoog_Service_Manual_synthfool.pdf`: the schematics, figures 9-2
//! to 9-17, and the parts lists of section 7), R. J. Folkman's Technical
//! Service Manual of 1973 (the adjustment procedures of its section VII)
//! and the owner's manual, "A guide to the operation of the Mini Moog
//! Model D". Each constant names its figure and component; where the
//! drawings give two ends and not the travel between them, or could not
//! be read, the constant says that it is an own choice and why.

use crate::analog::Oversampling;
use crate::analog::envelope::ENVELOPE_ONE;
use crate::analog::filter::ONE_Q28;
use crate::analog::fixed::mul_high_unsigned;
pub use crate::analog::octave::{CENTS, OCTAVE, OCTAVE_BITS, SEMITONES, cents, keys, octaves};
use crate::analog::octave::{ONE_Q30, square_root};

/// How many times the output rate the voice runs at: the ladder distorts,
/// and the oscillators want it for their aliasing to be 80 dB down.
///
/// Source: own choice, from the measurements of the building blocks
/// (`analog::oscillator`, `analog::filter`).
pub const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// The voice's rate in hertz.
pub const RATE: u32 = OVERSAMPLING.rate();

/// `amount * part / whole`, rounded towards zero; nothing for a whole of
/// zero. It divides, and belongs where a control changes.
#[must_use]
pub fn share_of(amount: i32, part: i32, whole: i32) -> i32 {
    let product = i64::from(amount).saturating_mul(i64::from(part));
    i32::try_from(product.checked_div(i64::from(whole)).unwrap_or(0)).unwrap_or(0)
}

/// Sixty-fourths of an octave: the table's steps.
const TABLE_BITS: u32 = 6;
/// Bits of an interval below a step of the table.
const REST_BITS: u32 = OCTAVE_BITS - TABLE_BITS;
/// The natural logarithm of two in Q14.
///
/// Source: ln 2 = 0.693147180559945, times 2¹⁴, rounded.
const LN2_Q14: u32 = 11_357;
/// The right shift that makes the rest of an interval, times
/// [`LN2_Q14`], its natural logarithm in Q32.
const LOG_SHIFT: u32 = OCTAVE_BITS + 14 - 32;

/// 2^(k/64) in Q30 for k from 0 to 63: each is a product of the square
/// root of two taken one to six times.
///
/// Source: computed here; the roots are whole square roots of numbers in
/// Q60, so each entry is within a few units of 2⁻³⁰ of its value.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const SIXTY_FOURTHS: [u32; 1 << TABLE_BITS] = {
    // roots[i] = 2^(1 / 2^(i+1)).
    let mut roots = [0u64; TABLE_BITS as usize];
    let mut value = 2 * ONE_Q30;
    let mut index = 0;
    while index < roots.len() {
        value = square_root((value as u128) << 30) as u64;
        roots[index] = value;
        index += 1;
    }
    let mut table = [0u32; 1 << TABLE_BITS];
    let mut entry = 0;
    while entry < table.len() {
        let mut ratio = ONE_Q30;
        let mut bit = 0;
        while bit < roots.len() {
            // The top bit of the entry's number is the half octave.
            if entry & (1 << (roots.len() - 1 - bit)) != 0 {
                ratio = (ratio * roots[bit]) >> 30;
            }
            bit += 1;
        }
        table[entry] = ratio as u32;
        entry += 1;
    }
    table
};

/// `step * 2^octaves` for an interval in octaves with [`OCTAVE_BITS`] of
/// fraction, held to a word: the exponential converter of an oscillator
/// and of the filter. It is within a hundredth of a cent of the
/// exponential for a step of 16 bits or more, takes no division and no
/// 64-bit product, and runs for every sample: which is why it is not
/// [`crate::analog::octave::raise`], whose 64-bit and 128-bit products
/// belong where a control changes.
#[inline]
#[must_use]
pub fn raise(step: u32, octaves: i32) -> u32 {
    let whole = octaves >> OCTAVE_BITS;
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let coarse = SIXTY_FOURTHS.get((fraction >> REST_BITS) as usize).copied().unwrap_or(ONE_Q30 as u32);
    // The rest is below a sixty-fourth of an octave: y = rest * ln 2 is
    // below 0.011, and e^y = 1 + y + y²/2 to within 2⁻²².
    let rest = fraction & ((1 << REST_BITS) - 1);
    let y = rest.wrapping_mul(LN2_Q14) >> LOG_SHIFT;
    let fine = y.wrapping_add(mul_high_unsigned(y, y) >> 1);
    // The ratio, from one to two, in Q30; and half the step times it.
    let ratio = coarse.wrapping_add(mul_high_unsigned(coarse, fine));
    let half = mul_high_unsigned(step, ratio << 1);
    match whole.wrapping_add(1) {
        shift @ 0..=31 => {
            let shift = shift.cast_unsigned();
            if half.leading_zeros() < shift { u32::MAX } else { half << shift }
        }
        shift @ -31..=-1 => half >> shift.unsigned_abs(),
        i32::MIN..=-32 => 0,
        _ => u32::MAX,
    }
}

/// The key whose pitch at 8' is [`PITCH`], and that pitch in thousandths
/// of a hertz: the instrument's A-440.
///
/// Source: Folkman, section VII, "Oscillator Tuning (With Equipment)":
/// with the range switches at 2' the low A is adjusted "for 440 Hz" and
/// the high A, three octaves up, "for 3520 Hz"; two octaves down, at 8',
/// the low A is 110 Hz, which is key 45 of MIDI, so that at 8' every key
/// sounds its MIDI pitch.
pub const PITCH_KEY: u8 = 69;
/// See [`PITCH_KEY`].
pub const PITCH: u32 = 440_000;

/// The lowest key of the keyboard, at which the keyboard's voltage is
/// zero, and its highest.
///
/// Source: service manual, figure 9-2, the keyboard's input of the
/// oscillator board: "(LO F) 0 TO 3.64V (HI C), 1.02V/OCT"; owner's
/// manual, page 1: "a 44-note keyboard". From the tuning above, the low F
/// at 8' is 87.3 Hz, key 41.
pub const KEYBOARD: (u8, u8) = (41, 84);

/// What each position of a range switch adds to the pitch, in octaves
/// from 8': LO, 32', 16', 8', 4', 2'.
///
/// Source: service manual, figure 9-17, the range switches' string: four
/// matched 1 kΩ resistors (R25 to R28) between the taps of 2', 4', 8',
/// 16' and 32', then R29, 4.75 kΩ, to the tap of LO: an octave a step,
/// and LO 4.75 octaves below 32'. Owner's manual, page 4: "all positions
/// except the lowest are separated by one octave".
pub const RANGES: [i32; 6] = [octaves(-27, 4), -2 * OCTAVE, -OCTAVE, 0, OCTAVE, 2 * OCTAVE];

/// How far the pitch wheel bends at either end, in octaves: 0.664, 797
/// cents.
///
/// Source: service manual, figure 9-2: the pitch wheel gives "5V ±2V,
/// ±5TH(+)" into R1, 301 kΩ, of the primary summer, whose feedback R2 is
/// 100 kΩ and whose output is "-1.00V/OCT": 2 V * 100 / 301 octaves. The
/// owner's manual, page 8, says "as much as half an octave up or down".
pub const BEND: i32 = octaves(200, 301);

/// How far the TUNE knob moves the pitch at either end, in cents: 242.
///
/// Source: service manual, figure 9-2: the knob gives "7.5V ±2.5V,
/// ±2ST(+)" into R3, 1.24 MΩ, of the same summer: 2.5 V * 100 / 1,240
/// octaves.
pub const TUNE_CENTS: i16 = 242;

/// How far the FREQUENCY knob of oscillator 2 or 3 moves its pitch at
/// either end, in cents: seven semitones.
///
/// Source: service manual, figure 9-2: each knob gives "7.5V ±2.5V,
/// ±5TH(+)" into 402 kΩ (R54, R96) of its oscillator's summer, where the
/// keyboard's volt an octave comes through 90.9 kΩ and the scale trimmer:
/// 2.5 V * 91 to 101 / 402 is 0.57 to 0.63 octave. Own choice of the
/// seven semitones within that, which the sound charts' dial of -7 to 7
/// agrees with. The owner's manual of 1971, page 5, of the earlier
/// oscillator board, says "as much as a major sixth".
pub const FREQUENCY_CENTS: i16 = 700;

/// How far the same knob moves oscillator 3 with its keyboard control
/// off, in octaves either way.
///
/// Source: service manual, figure 9-2, at the OSC 3 CONTROL switch:
/// "±2.75 OCT"; owner's manual, page 5: "a frequency sweep of 6 octaves
/// rather than one octave".
pub const FREE_FREQUENCY: i32 = octaves(11, 4);

/// The key oscillator 3 sounds, with its FREQUENCY knob in the middle,
/// when its keyboard control is off.
///
/// Source: own choice: the middle of the keyboard. The drawing shows a
/// fixed current in place of the keyboard's (R98, 69.8 kΩ, to -10 V) and
/// not what pitch it makes. At LO this is 2.7 Hz, and the sound charts'
/// vibrato settings of 2.5 to 4 on the dial come to 5 to 8 Hz.
pub const FREE_KEY: u8 = 62;

/// The share of a cycle a rectangular wave is high: square, wide and
/// narrow, in units of 2⁻³² cycle.
///
/// Source: service manual, figure 9-2, the pulse width input: "0V=RECT
/// [square], -2.5V=15% D/CY"; figure 9-17, each waveform switch's string
/// of 1.5 kΩ, 1 kΩ and 7.5 kΩ from ground to -10 V (R34, R33, R32),
/// whose taps are 0 V, -1.5 V and -2.5 V. On the line through the two
/// figures given, -1.5 V is 29%.
pub const WIDTHS: [u32; 3] = [1 << 31, percent(29), percent(15)];

/// So many hundredths of a cycle.
const fn percent(percent: u64) -> u32 {
    ((percent << 32) / 100) as u32
}

/// The sawtooth's share of the sawtooth-triangular wave, in Q31; the
/// triangle has the rest.
///
/// Source: service manual, figure 9-17, the waveform switches: the
/// triangle through 10 kΩ (R31, R37) and the sawtooth through 47 kΩ
/// (R30, R36) to one contact: 10 / 57 of the sawtooth.
pub const SHARK_SAWTOOTH: i32 = ((10i64 << 31) / 57) as i32;

/// The level of the noise in the mixer against an oscillator at the same
/// volume, in Q31: 2 dB down at the voice's rate, which is 5 dB down in
/// the half of its band that the output carries.
///
/// Source: Folkman, section VII, "Noise Level": "factory-set to yield
/// -5dB maximum in the white position". Own reading of what the 5 dB are
/// below: an oscillator's sawtooth, by root mean square, in the band
/// that is heard.
pub const NOISE_LEVEL: i32 = 1_705_806_895;

/// The right shift of the white noise that is always in the mixer,
/// whatever its switches: the circuit's own noise, 84 dB below an
/// oscillator.
///
/// Source: own choice. With nothing at all at its input the ladder
/// block would stay at rest for ever at any EMPHASIS; the instrument's
/// filter starts to oscillate by itself.
pub const FLOOR_SHIFT: u32 = 14;

/// The lowest corner of the pink noise's filters, in thousandths of a
/// hertz, and how many there are, each two octaves above the last and
/// half as strong: together they fall 3 dB an octave from 25 Hz to
/// 6.4 kHz, within half a decibel.
///
/// Source: own design. The instrument filters its white noise through
/// three stages of resistors and capacitors (service manual, figure 9-8,
/// circuit 3) for "white, pink, and red noise outputs" (Folkman, section
/// II); their values were not worked through.
pub const PINK_CORNER: u32 = 25_000;
/// See [`PINK_CORNER`].
pub const PINK_STAGES: usize = 5;
/// The left shift of the pink noise, which makes it as strong as the
/// white, and of its lowest stage alone as the red noise.
///
/// Source: own choice: five stages of equal power, each 39 / 48,000 of
/// the white noise's, come to a 250th of it, and 16 squared brings that
/// back; one stage needs 32.
pub const PINK_SHIFT: u32 = 4;
/// See [`PINK_SHIFT`].
pub const RED_SHIFT: u32 = 5;

/// The corner below which the mixer's output does not pass, in
/// thousandths of a hertz: what takes the rectangular waves' steady part
/// away.
///
/// Source: service manual, figure 9-2: each oscillator reaches the
/// mixer's "audio node" through 33 kΩ and "10µF": 1 / (2 π R C) is
/// 0.48 Hz.
pub const MIXER_CORNER: u32 = 482;

/// The gain of the external input's preamplifier with its volume at 10,
/// in Q28, and the corner below which the returned output does not pass,
/// in thousandths of a hertz.
///
/// Source: own choice. The preamplifier is "a three transistor high-gain
/// amplifier" (Folkman, section II) whose gain was not worked out from
/// figure 9-11, and no document read describes the patch; six times,
/// with the knob's law taken as a square, brings the loop's gain to one
/// near 4 on the dial, below which the returned output thickens the
/// sound and above which it overdrives the filter. The corner is 1 µF,
/// the preamplifier's output capacitor C20, into 10 kΩ taken for what
/// follows it.
pub const EXTERNAL_GAIN: i32 = 6 * ONE_Q28;
/// See [`EXTERNAL_GAIN`].
pub const EXTERNAL_CORNER: u32 = 15_900;

/// The cutoff frequency, in thousandths of a hertz, with the CUTOFF
/// FREQUENCY knob at [`CUTOFF_ANCHOR_DIAL`] hundredths of a division, no
/// keyboard control and no contour.
///
/// Source: Folkman, section VII, "Filter Range": "Turn KEYBOARD CONTROL
/// switches 1 and 2 off. Set CUTOFF FREQUENCY at -1, EMPHASIS at 10.
/// Turn on A-440 and adjust filter Range Trimpot ... for zero beats."
pub const CUTOFF_ANCHOR: u32 = 440_000;
/// See [`CUTOFF_ANCHOR`].
pub const CUTOFF_ANCHOR_DIAL: i16 = -100;
/// A division of the CUTOFF FREQUENCY dial, which runs from -5 to 5, in
/// octaves: one.
///
/// Source: service manual, figure 9-17: the knob, R11, 5 kΩ between +10 V
/// and -10 V, reaches the filter's control node through R55, 200 kΩ,
/// where the keyboard's volt an octave reaches it through 100 kΩ (R53,
/// 300 kΩ, beside R54, 150 kΩ): 20 V * 100 / 200 are ten octaves.
pub const CUTOFF_DIVISION: i32 = OCTAVE;

/// The shares of the keyboard's voltage the two KEYBOARD CONTROL switches
/// add to the cutoff, in Q31: a third and two thirds, and both together
/// the whole.
///
/// Source: service manual, figure 9-17: SW16, "FILTER KBD 1/3", R53,
/// 300 kΩ; SW17, "FILTER KBD 2/3", R54, 150 kΩ. Folkman, section VII,
/// "Filter Scale": with both on, the filter tuned to 1,760 Hz on the
/// third A gives 440 Hz on the low A.
pub const KEYBOARD_CONTROL: [i32; 2] = [i32::MAX / 3, (i32::MAX / 3) * 2];

/// How far the filter's contour at its peak raises the cutoff with the
/// AMOUNT OF CONTOUR at 10, in octaves: 8.5.
///
/// Source: service manual, figure 9-11: the contour reaches the control
/// node through R74, 47 kΩ, so each volt is 100 / 47 octave. Own reading
/// of the contour's peak as 4 V, from figure 9-7: the sustain knob, which
/// at 10 holds the peak (owner's manual, page 12), gives 10 V through
/// R13, 4.7 kΩ, into R1, 3 kΩ: 3.9 V.
pub const CONTOUR: i32 = octaves(400, 47);

/// The dial position of the EMPHASIS at which the filter begins to
/// oscillate, in tenths of a division.
///
/// Source: Folkman, section VII, "Regeneration Cal.": "Regeneration
/// should start when the EMPHASIS control is between 7 and 8. If it does
/// not, set EMPHASIS control to 7.5 and rotate the Regeneration Cal.
/// trimpot ... until regeneration starts."
pub const REGENERATION_DIAL: u8 = 75;
/// The loop's feedback at that position, in Q28: a four-pole loop
/// oscillates at four (Stinchcombe, "Analysis of the Moog Transistor
/// Ladder and Derivative Filters", section 2.3, equation 22).
pub const REGENERATION: i32 = 4 * ONE_Q28;
/// The feedback with the EMPHASIS at 10, in Q28: 4.3.
///
/// Source: own choice. Above four the feedback sets how hard the
/// oscillation is driven into the stages' curve; the owner's manual,
/// page 13, says the filter at 10 "produces a pure sine wave tone", so
/// it is kept near four. The knob's law between its ends (a 50 kΩ
/// reverse audio taper, figure 9-17, R14) is taken as two straight
/// lines through the calibration point.
pub const EMPHASIS_TOP: i32 = (43 * ONE_Q28 as i64 / 10) as i32;
/// How much the cutoff is raised for each unit of feedback above four,
/// in octaves, so that the oscillation stays where the dial says: 94
/// cents for each unit.
///
/// Source: own measurement of the ladder block, whose oscillation falls
/// below its cutoff as it is driven (`analog::filter`, "Oscillation");
/// `tests/minimoog_filter.rs` holds the result to the service manual's
/// calibration.
pub const REGENERATION_TRIM: i32 = cents(94);

/// The level at which the amplifier after the ladder saturates, as a
/// hyperbolic tangent: the ladder block's own level, twice an
/// oscillator's peak. One oscillator at 10 is then bent by 8% at its
/// peaks, and three together lose 2.4 dB.
///
/// Source: service manual, figure 9-11, circuit 6: the ladder's top
/// stage is read through C1 and C5 by a pair of transistors, Q5 and Q7,
/// with their emitters joined, which is the same curve as a stage's
/// (Stinchcombe, section 2.1.1). Own choice of the level, which the
/// drawing's values were not worked through for; one reference program,
/// measured with three oscillators at 10 and its filter open, loses
/// 2.6 dB.
pub const OUTPUT_STAGE: i32 = crate::analog::filter::SATURATION;

/// The level a contour's attack charges towards, the peak being one: one
/// unit short of twice it, the most the envelope block holds.
///
/// Source: own reading of service manual, figure 9-7: the timing
/// capacitor is charged from the 9.3 V rail through the attack knob, and
/// the peak is the 4 V and a diode's drop of [`CONTOUR`]: 9.3 / 4.5 is
/// 2.07.
pub const ATTACK_TARGET: i32 = i32::MAX;
/// The time constants of a contour's attack and decay at the ends of
/// their dials, in millionths of a second: the dial is marked in
/// milliseconds and seconds, and a patch holds the marking.
///
/// Source: service manual, figure 9-7: the timing capacitors C5 and C2,
/// 10 µF; figure 9-17 and table 7-3: the ATTACK and DECAY knobs, 1 MΩ
/// audio taper (R12, R13, R15, R16): 10 s; in series with them R7 and
/// R42, 100 Ω: 1 ms. Owner's manual, page 11: "as short as 10
/// milliseconds or as long as 10 seconds".
pub const CONTOUR_TIMES: (u32, u32) = (1_000, 10_000_000);
/// The time constant of the fall when a key is released with the DECAY
/// switch off, in millionths of a second.
///
/// Source: own reading of service manual, figure 9-12: the contour
/// generators' "dump" line with R1401, 1.5 kΩ, at the DECAY switch, and
/// the 10 µF: 15 ms. Owner's manual, page 14: "cut off immediately".
pub const DUMP_TIME: u32 = 15_000;

/// The glide's time constant with the GLIDE knob at 10, in millionths of
/// a second; the knob's law is the cube of its position.
///
/// Source: service manual, figure 9-7, the keyboard circuit: the glide
/// capacitor C6, "1 mf"; table 7-3: the GLIDE knob R2, 5 MΩ: 5 s. Own
/// choice of the cube for its "No. 1 taper".
pub const GLIDE_TIME: u32 = 5_000_000;

/// How far the modulation mix at full swing moves the oscillators with
/// the wheel at its top, in octaves in Q28: 0.875 either way.
///
/// Source: service manual, figure 9-2, at the modulation wheel: "1.75
/// P-P MAX", into R5, 100 kΩ, of the primary summer, "1V/OCT".
pub const OSCILLATOR_MODULATION: i32 = (7 * ONE_Q28 as i64 / 8) as i32;
/// How far it moves the cutoff, in octaves in Q28: 2.65 either way.
///
/// Source: service manual, figure 9-17: the same signal reaches the
/// filter's control node through R52, 33 kΩ, by the FILTER MODULATION
/// switch: 100 / 33 times as far.
pub const FILTER_MODULATION: i32 = (7 * ONE_Q28 as i64 * 100 / (8 * 33)) as i32;

/// The widest drift of an oscillator's pitch either way an instrument
/// may be given, in cents, and what it is given when it is made.
///
/// Source: own choice. Folkman's trouble chart lists "Oscillators drift
/// and exhibit poor tracking" as a fault the later oscillator board
/// cures, and gives no figure.
pub const DRIFT_LIMIT_CENTS: u8 = 50;
/// See [`DRIFT_LIMIT_CENTS`].
pub const DRIFT_DEFAULT_CENTS: u8 = 2;
/// Seconds between the points an oscillator's drift wanders to.
///
/// Source: own choice.
pub const DRIFT_SECONDS: u32 = 2;

/// The largest value of a knob whose dial runs from 0 to 10: tenths of a
/// division.
pub const KNOB_MAX: u8 = 100;

/// A knob's position, in tenths of a division from 0 to 10, as a share in
/// Q31.
#[must_use]
pub fn share(knob: u8) -> i32 {
    share_of(i32::MAX, i32::from(knob.min(KNOB_MAX)), i32::from(KNOB_MAX))
}

/// A knob's position as a level in Q30.
#[must_use]
pub fn level(knob: u8) -> i32 {
    share_of(ENVELOPE_ONE, i32::from(knob.min(KNOB_MAX)), i32::from(KNOB_MAX))
}

/// The square of a knob's position, as a share in Q31: the law taken for
/// the volume knobs.
///
/// Source: own choice, as an audio taper; the oscillators' volume knobs
/// are linear (table 7-3, R6 to R8), and [`share`] is theirs.
#[must_use]
pub fn square(knob: u8) -> i32 {
    let knob = i32::from(knob.min(KNOB_MAX));
    share_of(i32::MAX, knob.saturating_mul(knob), i32::from(KNOB_MAX).saturating_mul(i32::from(KNOB_MAX)))
}

/// The loop's feedback for an EMPHASIS position, in Q28.
#[must_use]
pub fn emphasis(knob: u8) -> i32 {
    let knob = knob.min(KNOB_MAX);
    if knob <= REGENERATION_DIAL {
        share_of(REGENERATION, i32::from(knob), i32::from(REGENERATION_DIAL))
    } else {
        let above = i32::from(knob.saturating_sub(REGENERATION_DIAL));
        let travel = i32::from(KNOB_MAX.saturating_sub(REGENERATION_DIAL));
        REGENERATION.saturating_add(share_of(EMPHASIS_TOP.saturating_sub(REGENERATION), above, travel))
    }
}

/// The glide's time constant for a GLIDE position, in millionths of a
/// second.
#[must_use]
pub fn glide(knob: u8) -> u32 {
    let knob = u64::from(knob.min(KNOB_MAX));
    let full = u64::from(KNOB_MAX);
    let cube = knob.saturating_mul(knob).saturating_mul(knob);
    let time = u64::from(GLIDE_TIME).saturating_mul(cube).checked_div(full.saturating_mul(full).saturating_mul(full));
    u32::try_from(time.unwrap_or(0)).unwrap_or(GLIDE_TIME)
}
