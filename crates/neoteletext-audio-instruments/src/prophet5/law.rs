//! Intervals, shares and the two revisions' laws: the arithmetic that
//! turns a control voltage, held as an interval in octaves, into a
//! frequency; and what a knob's stored number means on each revision.
//!
//! Every control voltage of the instrument that sets a frequency is one
//! volt to the octave (technical manual for revision 3, section 2-4:
//! "The IC is scaled at 1V/octave"; section 2-6 of the filter: "Its
//! scale matches the VCO scale of 1V/OCT"), so an interval in octaves is
//! the voltage itself, and the converter's step of a twelfth of a volt is
//! a semitone.
//!
//! The manuals are cited as "TM3" (Sequential Circuits, "Prophet-5
//! Synthesizer Technical Manual", second edition, TM1000D.2, October
//! 1981, for revisions 3.0 to 3.2), "TM2" (the same for revision 2,
//! TM1000C, January 1980) and "OM" (the operation manual CM1000D,
//! February 1982, for revision 3.3), with the page of the scan.

use crate::analog::filter::ONE_Q28;

/// Fraction bits of an interval in octaves.
pub const OCTAVE_BITS: u32 = 24;
/// One octave.
pub const OCTAVE: i32 = 1 << OCTAVE_BITS;
/// Semitones in an octave.
pub const SEMITONES: i32 = 12;
/// Cents in an octave.
const CENTS: i32 = 1_200;

/// An interval of so many cents, in octaves.
#[must_use]
pub const fn cents(cents: i32) -> i32 {
    octaves(cents, CENTS)
}

/// An interval of so many semitones, in octaves.
#[must_use]
pub const fn semitones(semitones: i32) -> i32 {
    octaves(semitones, SEMITONES)
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
/// fraction, held to a word: within a thousandth of a cent of it. It is
/// what an exponential converter makes of a control voltage.
///
/// It takes 64-bit products, and belongs where a control changes, not in
/// the loop that renders samples.
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

/// Bits of the table of [`bend`]: 256 steps to the octave.
const BEND_BITS: u32 = 8;
/// Bits of an interval below a step of that table.
const BEND_REST_BITS: u32 = OCTAVE_BITS - BEND_BITS;
/// The natural logarithm of two in Q32.
///
/// Source: ln 2 = 0.693147180559945, times 2³², rounded.
const LN2_Q32: u32 = 2_977_044_472;

/// 2^(k/256) in Q31 for k from 0 to 255, each a product of the square
/// root of two taken one to eight times.
///
/// Source: computed here, as [`SIXTEENTHS`] is.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const BENDS: [u32; 1 << BEND_BITS] = {
    let mut roots = [0u64; BEND_BITS as usize];
    let mut value = 2 * ONE_Q30;
    let mut index = 0;
    while index < roots.len() {
        value = square_root((value as u128) << 30) as u64;
        roots[index] = value;
        index += 1;
    }
    let mut table = [0u32; 1 << BEND_BITS];
    let mut entry = 0;
    while entry < table.len() {
        let mut ratio = ONE_Q30;
        let mut bit = 0;
        while bit < roots.len() {
            if entry & (1 << (roots.len() - 1 - bit)) != 0 {
                ratio = (ratio * roots[bit]) >> 30;
            }
            bit += 1;
        }
        table[entry] = (ratio << 1) as u32;
        entry += 1;
    }
    table
};

/// The widest interval [`bend`] takes either way: beyond it a step has
/// left a word or vanished.
const BEND_LIMIT: i32 = 32 * OCTAVE;

/// `step * 2^octaves`, held to a word, within a fiftieth of a cent: what
/// [`raise`] gives, by a table and one term of the series, in 32-bit
/// products and no division, for the loop that renders samples, where an
/// oscillator at audio rate moves another's frequency or the cutoff.
#[inline]
#[must_use]
pub fn bend(step: u32, octaves: i32) -> u32 {
    use crate::analog::fixed::mul_high_unsigned;
    let octaves = octaves.clamp(-BEND_LIMIT, BEND_LIMIT);
    let whole = octaves >> OCTAVE_BITS;
    let fraction = (octaves & (OCTAVE - 1)).cast_unsigned();
    let coarse = BENDS.get((fraction >> BEND_REST_BITS) as usize).copied().unwrap_or(1 << 31);
    // The rest is below 1/256 octave: 2^r = 1 + r ln 2 to within 4e-6.
    let rest = (fraction & ((1 << BEND_REST_BITS) - 1)) << (u32::BITS - OCTAVE_BITS);
    let fine = coarse.saturating_add(mul_high_unsigned(coarse, mul_high_unsigned(rest, LN2_Q32)));
    // The ratio is between one and two in Q31: the product is half the
    // result.
    let half = mul_high_unsigned(step, fine);
    if whole >= 0 {
        let shift = whole.unsigned_abs().saturating_add(1);
        if half.leading_zeros() < shift { u32::MAX } else { half << shift }
    } else {
        // A step loses its low bits here; a fall of an octave or more
        // from a step that has few is beyond a fiftieth of a cent.
        half.checked_shr(whole.unsigned_abs().saturating_sub(1)).unwrap_or(0)
    }
}

/// `amount * part / whole`, rounded towards zero; nothing for a whole of
/// zero, and the nearest a word holds for what leaves it.
#[must_use]
pub fn share(amount: i32, part: i32, whole: i32) -> i32 {
    let product = i64::from(amount).saturating_mul(i64::from(part)).checked_div(i64::from(whole)).unwrap_or(0);
    i32::try_from(product).unwrap_or(if product < 0 { i32::MIN } else { i32::MAX })
}

/// `amount * level` for a level in Q30: a control voltage through an
/// amplifier whose gain is the level.
#[inline]
#[must_use]
pub const fn gain(amount: i32, level: i32) -> i32 {
    crate::analog::fixed::mul_high(amount, level) << 2
}

/// One in Q24: one volt, one octave, or the whole of a cycle, as the
/// quantity is.
pub const UNIT: i32 = OCTAVE;

/// How the waveforms of one oscillator that are switched on come
/// together.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mixing {
    /// Each through its own resistor into an amplifier's input, where
    /// the currents add: what is on is at its full level whatever else
    /// is.
    Currents,
    /// Each through its own resistor to one node that a follower reads:
    /// what is on shares the node, and two waveforms are each halved.
    Node,
}

/// What differs between revision 3 and revisions 1 and 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Law {
    /// The largest number the converter is given for a knob: what is
    /// stored above it plays as it.
    pub knob_top: u8,
    /// The semitones an oscillator's frequency knob gives: its number
    /// times the first over the second, rounded down, and at most the
    /// third.
    pub frequency: (i32, i32, i32),
    /// Octaves of oscillator B's fine tuning for each step of its knob.
    pub fine_step: i32,
    /// With oscillator B's low-frequency switch on: the offset in
    /// octaves, and the octaves each step of its frequency knob then
    /// gives, or nothing where the knob keeps its semitones.
    pub low_frequency: (i32, Option<i32>),
    /// The pulse's width as a share of a cycle in Q24: at the knob's 0,
    /// and what each step adds.
    pub pulse_width: (i32, i32),
    /// The share of a cycle that one unit of Wheel-Mod, and one volt of
    /// Poly-Mod, move the pulse width, in Q24.
    pub pulse_width_modulation: (i32, i32),
    /// How one oscillator's waveforms come together.
    pub mixing: Mixing,
    /// The levels of the pulse and of the triangle against the
    /// sawtooth's, in Q30.
    pub levels: (i32, i32),
    /// The cutoff with the knob at 0 and no voltage from the keys, in
    /// octaves from 440 Hz; and the converter's number for the lowest
    /// key.
    pub cutoff: (i32, i32),
    /// The resonance knob's number at which the filter begins to
    /// oscillate, and whether the amplifier that sets the feedback
    /// follows the Curtis chip's curve or a straight line.
    pub resonance: (i32, bool),
    /// An envelope's time constant in millionths of a second at a knob
    /// number, that number, and the octaves of time each step adds.
    pub envelope: (u32, i32, i32),
    /// The shortest and longest time constant the envelope chip
    /// reaches, in millionths of a second.
    pub envelope_bounds: (u32, u32),
    /// The low-frequency oscillator's frequency at the knob's 0 in
    /// thousandths of a hertz, the octaves each step adds, and the level
    /// of its square against its sawtooth in Q30.
    pub lfo: (u32, i32, i32),
    /// The difference, in octaves, within which the glide closes in
    /// along an exponential; beyond it, it moves at its set rate.
    pub glide_zone: i32,
    /// How far each oscillator's tuning and each filter's cutoff are
    /// off, at most, fixed for the voice, in octaves.
    pub tolerance: (i32, i32),
    /// Whether what blocks direct current stands before the filter (the
    /// mixer's capacitors) or after it (the output buffer's), and its
    /// corner in thousandths of a hertz.
    pub blocking: (bool, u32),
    /// How flat the filter oscillates at the resonance knob's top, in
    /// octaves: the cutoff is raised by its share above the onset, so
    /// that the oscillation is where the factory trims it.
    pub oscillation_flat: i32,
}

/// One in Q30.
const ONE_Q30_LEVEL: i32 = 1 << 30;

/// Revision 3: the Curtis chips.
pub const REV3: Law = Law {
    // Source: TM3 section 2-12 (page 29): "The DAC's full scale voltage
    // is 10.67V, but most CVs are limited to 10V by the software"; ten
    // volts are 120 steps of a twelfth of a volt.
    knob_top: 120,
    // Source: TM3 test table (pages 59 and 60): "PITCH ADJUSTS IN
    // SEMITONES OVER 4-OCTAVE RANGE"; section 2-4 (page 21): "0-4V
    // supplied by the OSC FREQ knob". Own choice of how the 120 steps of
    // the knob become 48 semitones, which the manual does not say: two
    // semitones for every five steps, rounded down.
    frequency: (2, 5, 48),
    // Source: TM3 test table (page 60): "PITCH ADJUSTS CONTINUOUSLY OVER
    // 1-SEMITONE RANGE"; OM section 2-4 (page 16): "up from the basic
    // pitch setting".
    fine_step: octaves(1, SEMITONES * 120),
    // Source: TM3 section 2-4 (page 21): "the OSC B LO FREQ switch adds
    // a -7.5V offset through B SUM CV ... the INIT FREQ control doubles
    // its range (to 9V) when OSC B LO FREQ is ON"; nine volts over the
    // knob's 120 steps. The test table (page 60) has ".1-40 HZ" with the
    // keyboard off: C0 less 7.5 octaves is 0.09 Hz, and nine octaves
    // above that 46 Hz.
    low_frequency: (octaves(-15, 2), Some(octaves(9, 120))),
    // Source: TM3 voice schematic SD431 (page 50): the pulse-width
    // voltage passes a summer of 100 kΩ in and 52.3 kΩ back (R4163,
    // R4162) to the CEM3340's pin 5; CEM3340 data sheet, page 2: the
    // "PWM input voltage" is 0 V for 0% and 5.0 V for 100%. A step of a
    // twelfth of a volt is then 0.523 / 60 of a cycle; the knob's 5 is
    // 52%, and the pulse is gone at 0 and above 9.6, as OM section 2-3
    // (page 15) says of the knob's ends.
    pulse_width: (0, (UNIT as i64 * 523 / 60_000) as i32),
    // Source: the same schematics. Wheel-Mod enters the pulse-width
    // summers through 15 kΩ (R397, R398, sheet SD334, page 47) against
    // 52.3 kΩ back: 3.49 V at the pin, 69.7% of a cycle, for each volt;
    // Poly-Mod through 30.1 kΩ (R4172): 34.8%.
    pulse_width_modulation: ((UNIT as i64 * 697 / 1_000) as i32, (UNIT as i64 * 348 / 1_000) as i32),
    // Source: TM3 SD431 (page 50): each waveform through its own
    // resistor into the mixer amplifier's input, a 330 Ω shunt: the
    // sawtooth through 150 kΩ, the pulse through 200 kΩ, the triangle
    // through 150 kΩ. CEM3340 data sheet, page 2: the sawtooth is 0 to
    // 10 V, the triangle 0 to 5 V, and "the pulse peak is 1.5 volts
    // below the supply", 13.5 V: currents of 67, 67.5 and 33 µA peak to
    // peak, so the pulse is as large as the sawtooth and the triangle
    // half.
    mixing: Mixing::Currents,
    levels: (ONE_Q30_LEVEL, ONE_Q30_LEVEL / 2),
    // Source: TM3 adjustment 4-20 (pages 70 and 71): with the cutoff
    // voltage "as close to 2.000V as the quantized voltage steps will
    // allow", 24 steps, and the filter oscillating, "Alternately play A3
    // and A4 (highest A) while tuning ... for 440- and 880-Hz". A3 is
    // the 45th key above the lowest, so with no steps and no key the
    // cutoff is 69 semitones below 440 Hz. Own choice: the lowest key
    // is taken to give no voltage.
    cutoff: (octaves(-69, SEMITONES), 0),
    // Source: OM section 2-6 (page 17): "As the knob setting is
    // increased beyond 7, the filter breaks into oscillation"; TM3 test
    // table (page 62): "ALL FILTERS OSCILLATE BETWEEN 7 AND 9-1/2 ON
    // DIAL". Own choice of 8 within that, 96 steps. The curve is the
    // CEM3320's resonance amplifier's ([`resonance`]).
    resonance: (96, true),
    // Source: TM3 test table (page 62): at 6 on the dial, 72 steps,
    // "APPROX 1-SEC ATTACK"; at 10, "GREATER THAN 20-SEC RELEASE"; OM
    // section 2-6 (page 19): "approximately 1 millisecond ... to 30
    // seconds", "setting 5 ... gives a period of approximately 1/2
    // second". CEM3310 data sheet, page 1: "Zero to -5V Varies the Times
    // from 2mS to 20S", four decades, and TM3 section 2-7 (page 22):
    // the times "are controlled by 0 to -5V". Own choice among them:
    // four decades over the knob's ten volts through one second of
    // attack at 6, which gives 0.4 s at 5 and 40 s at 10, and 4 ms at 0
    // where the operation manual says 1 ms. An attack lasts ln(6.5 /
    // 1.5) = 1.466 time constants (the data sheet's asymptote and
    // peak), so the time constant at 72 steps is 0.682 s; a step is
    // 0.4 / 12 decade, 0.11073 octave.
    envelope: (682_128, 72, octaves(110_730, 1_000_000)),
    // Source: none is needed: the knob's range is inside the chip's
    // (CEM3310 data sheet, page 2: "Time control range 50,000:1" at
    // least).
    envelope_bounds: (1, u32::MAX),
    // Source: OM section 2-9 (page 20): "from approximatley 0.04 Hz ...
    // to 20 Hz"; TM3 sheet SD334 (page 47): the knob's voltage enters
    // the oscillator's CEM3340 through 110 kΩ (R3136) where 100 kΩ is a
    // volt to the octave: 0.909 octave to the volt, 9.09 octaves over
    // ten volts, 0.04 Hz to 21.8 Hz. The square comes through 200 kΩ
    // from 13.5 V and the sawtooth through 160 kΩ from 10 V: 1.08 of
    // it.
    lfo: (40, octaves(909, 12_000), (ONE_Q30_LEVEL as i64 * 108 / 100) as i32),
    // Source: own choice: the glide's amplifier is linear only over a
    // small difference (TM3 sheet SD334: a CA3280 with 4.7 kΩ at its
    // input); a semitone is taken.
    glide_zone: octaves(1, SEMITONES),
    // Source: TM3 section 2-4 (page 21): the oscillators are tuned
    // "within 1/128 semitone", 0.8 cent; own choice of 1.5 cents for
    // what an instrument has drifted since it last tuned itself. TM3
    // adjustment 4-20 (page 70): for the filters "there is no
    // computer-correction of filter tuning" and "no temperature
    // compensation"; own choice of 10 cents.
    tolerance: (octaves(15, 12_000), cents(10)),
    // Source: TM3 SD431 (page 50): the filter's output passes C4164,
    // 2.2 µF, into R4499, 100 kΩ: 1 / (2 π R C) is 0.72 Hz.
    blocking: (false, 723),
    // Source: measured on this filter at the feedback of the knob's top
    // (`tests/prophet5_filter.rs`); adjustment 4-20 tunes the filters
    // while they oscillate.
    oscillation_flat: cents(58),
};

/// Revisions 1 and 2: the SSM chips.
pub const REV2: Law = Law {
    // Source: TM2 section 2-12 (page 31): "The 7-bit DAC has a range of
    // 128 steps, thus 0 - 10.583V (127 X 1/12V)"; nothing there limits a
    // voltage below it.
    knob_top: 127,
    // Source: TM2 adjustment 4-4 (page 109): "an exact five-octave range
    // of either OSC FREQ knob"; section 2-12 (page 31): "each step
    // results in a frequency change of exactly one semitone", and "the
    // DAC MSB is not used for frequency control since only about half
    // the full range is needed". Own reading: the knob's seven bits less
    // the lowest are the semitones.
    frequency: (1, 2, 60),
    // Source: TM2 sheet SD312 (page 75): the fine tuning's voltage
    // enters oscillator B's summer through 10 MΩ (R3107) against 100 kΩ:
    // a hundredth of a volt for each volt, 1.27 semitones at the top.
    fine_step: octaves(1, SEMITONES * 100),
    // Source: TM2 section 2-8 (page 27): "For LO FREQ operation, a
    // switch applies negative voltage to the SMR"; sheet SD312: -15 V
    // through two 100 kΩ (R3133, R3134) and a third (R3103) into the
    // summer, which was read as 5 V, five octaves, and is not certain.
    low_frequency: (octaves(-5, 1), None),
    // Source: TM2 section 2-23 (page 45): "Control sensitivity is 10%/V
    // (100% @ 0V decreasing to 0% at 10V)".
    pulse_width: (UNIT, -(UNIT / 120)),
    // Source: TM2 sheets SD411 and SD312: Wheel-Mod enters the
    // pulse-width summer through 13.3 kΩ (R492) against 100 kΩ: 7.5 V,
    // 75% of a cycle, for each volt; Poly-Mod through 30.1 kΩ (R494):
    // 33.2%. More voltage is a narrower pulse.
    pulse_width_modulation: (-((UNIT as i64 * 750 / 1_000) as i32), -((UNIT as i64 * 332 / 1_000) as i32)),
    // Source: TM2 SD411 (page 85): the waveforms that are switched on
    // meet at one node before a follower or an amplifier ([`Mixing`],
    // and [`node`]). Section 2-23 (page 45): "The sawtooth is 0-10V",
    // "Pulse output is 0-7.5V". Own choice: the triangle, whose size the
    // manual does not give, is taken as large as the sawtooth.
    mixing: Mixing::Node,
    levels: ((ONE_Q30_LEVEL as i64 * 3 / 4) as i32, ONE_Q30_LEVEL),
    // Source: TM2 adjustment 4-17 (pages 114 and 115): with the cutoff
    // voltage "as close to +3.083-V reading as the quantized voltage
    // steps will allow", 37 steps, and the filter oscillating, key C3 is
    // trimmed "to resonate at A-440". Adjustment 4-3 (page 109): key B4,
    // 59 semitones above the lowest, gives "exactly 5.000V", 60 steps,
    // so the lowest key gives one step and C3 37: with no steps at all
    // the cutoff is 74 semitones below 440 Hz.
    cutoff: (octaves(-74, SEMITONES), 1),
    // Source: own choice: the onset is taken where revision 3's is; TM2
    // says only that the filters are tuned "at full resonance, where
    // they oscillate" (page 114). The feedback amplifier is an SSM2020
    // "operated in linear mode" (section 2-23, page 47).
    resonance: (96, false),
    // Source: TM2 adjustments 4-10 and 4-11 (pages 112 and 113): with
    // the attack's voltage "as close to 6.250V as the quantized voltage
    // steps" allow, 75 steps, the envelopes are trimmed "for 0.9-sec ATK
    // period". SSM2050 data sheet, page 2, of the same 100 kΩ in series
    // with each time input as the instrument has (R417 to R419, R424 to
    // R426): "TIME SENSITIVITY 2V/DECADE", so a step is a twenty-fourth
    // of a decade, 0.13841 octave. An attack to 10 V of the way to 13 V
    // (the same page) lasts ln(13 / 3) = 1.466 time constants: 0.614 s.
    envelope: (613_779, 75, octaves(138_413, 1_000_000)),
    // Source: SSM2050 data sheet, page 2, "Time range", typical: 1 to
    // 100,000 ms, taken as the attack's time, 1.466 time constants.
    envelope_bounds: (682, 68_212_824),
    // Source: Sequential's original patch sheet 1-1, "Brass": the
    // vibrato is "approximately 5 cycles per second" with the knob drawn
    // at about 6, 72 steps; TM2 sheet SD312 (page 75): the knob's
    // voltage is scaled by 75 kΩ into 100 kΩ (R368, R371) ahead of a
    // converter of a volt to the octave, 0.75 octave to the volt: 0.22
    // Hz at 0 and 54 Hz at the top. The manual gives no range. The
    // pulse is 7.5 V against the sawtooth's 10 V (section 2-23).
    lfo: (221, octaves(75, 1_200), (ONE_Q30_LEVEL as i64 * 3 / 4) as i32),
    // Source: TM2 section 2-21 (page 43): diodes "clamp the difference
    // between these to not exceed 0.6V".
    glide_zone: octaves(6, 10),
    // Source: TM2 section 2-15 (page 36): the tuning's steps are "about
    // 1 cent"; own choice of 3 cents for the drift of this revision's
    // oscillators since they were last tuned (Electric Druid, "CEM3340
    // VCO designs", of the SSM oscillators: "they were having tuning
    // problems"). TM2 adjustment 4-17 (page 115): "The filters are not
    // temperature-compensated, so they will drift", and "2 - 6
    // beats-per-second is acceptable": own choice of 15 cents.
    tolerance: (octaves(3, 1_200), cents(15)),
    // Source: TM2 SD411 (page 85): each oscillator's waveforms pass a
    // capacitor of 0.1 µF (C409, C408) into 200 kΩ (R481, R459): 8 Hz.
    blocking: (true, 7_958),
    // Source: measured, as revision 3's.
    oscillation_flat: cents(89),
};

/// The conductances that meet at revision 2's waveform node, in units
/// of a millionth of a siemens: the sawtooth's and the pulse's 10 kΩ,
/// the triangle's 3.3 kΩ, and the 200 kΩ to ground.
///
/// Source: TM2 SD411 (page 85): R436 and R435, 10 kΩ; R433, 3.3 kΩ; R434,
/// 200 kΩ; and of oscillator A R456, R457 and R481, the same.
const NODE_MICROSIEMENS: (i64, i64, i64, i64) = (100, 100, 303, 5);

/// The share of each waveform at the node where those that are switched
/// on meet, in Q30: its conductance over the sum of those that are on
/// and the load's. Sawtooth, pulse, triangle.
#[must_use]
pub fn node(sawtooth: bool, pulse: bool, triangle: bool) -> [i32; 3] {
    let (saw, pul, tri, load) = NODE_MICROSIEMENS;
    let on = [(sawtooth, saw), (pulse, pul), (triangle, tri)];
    let sum = on.iter().filter(|(on, _)| *on).map(|&(_, siemens)| siemens).sum::<i64>().saturating_add(load);
    on.map(|(on, siemens)| {
        let part = if on { siemens } else { 0 };
        i32::try_from((part << 30).checked_div(sum).unwrap_or(0)).unwrap_or(0)
    })
}

/// The feedback at which a four-pole loop begins to oscillate, in Q28.
///
/// Source: Zavalishin, "The Art of VA Filter Design", section 5.1;
/// CEM3320 data sheet, page 5: "AOSC = 12dB or 4 in the case of the low
/// pass filter"; SSM2040 data sheet, page 4, the graph of resonance
/// against feedback, which ends at 4.0.
pub const OSCILLATION_ONSET: i32 = 4 * ONE_Q28;

/// The knee of the CEM3320's resonance amplifier, in steps of the knob:
/// its transconductance goes as I / (1 + I / 133 µA), and a step of a
/// twelfth of a volt through the instrument's 200 kΩ is 0.4167 µA.
///
/// Source: CEM3320 data sheet, figure 6 (page 6), read off the curve:
/// about 1,000 µmho at 100 µA, 1,400 at 200 and 1,650 at 300, which I /
/// (1 + 0.0075 I) fits to 2%; page 5: "the slope of the curve becomes
/// more gradual as the control current increases". TM3 SD431 (page 50):
/// the resonance's voltage reaches the chip's pin 9 through R4414,
/// 200 kΩ.
const RESONANCE_KNEE_STEPS: i64 = 320;

/// The filter's feedback in Q28 for the resonance knob's number: four at
/// the law's onset, along the amplifier's curve or a straight line.
#[must_use]
pub fn resonance(law: &Law, steps: i32) -> i32 {
    let (onset, curved) = law.resonance;
    let (steps, onset) = (i64::from(steps.max(0)), i64::from(onset.max(1)));
    let four = i64::from(OSCILLATION_ONSET);
    let feedback = if curved {
        // g(n) = n / (1 + n / knee), as a share of g(onset).
        let numerator = four.saturating_mul(steps).saturating_mul(RESONANCE_KNEE_STEPS.saturating_add(onset));
        numerator.checked_div(RESONANCE_KNEE_STEPS.saturating_add(steps).saturating_mul(onset)).unwrap_or(0)
    } else {
        four.saturating_mul(steps).checked_div(onset).unwrap_or(0)
    };
    i32::try_from(feedback).unwrap_or(i32::MAX)
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::arithmetic_side_effects, reason = "a test, in f64")]
mod tests {
    use super::{OCTAVE, bend, raise, share};

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
    fn a_share_is_the_quotient_and_never_panics() {
        assert_eq!((share(120, 60, 120), share(-120, 60, 120), share(7, 1, 0)), (60, -60, 0));
        assert_eq!(share(i32::MAX, i32::MAX, 1), i32::MAX);
    }

    #[test]
    fn bending_is_raising_to_a_fiftieth_of_a_cent() {
        let mut worst = 0.0f64;
        for octaves in (-6 * OCTAVE..6 * OCTAVE).step_by(100_003) {
            let step = 40_000_000u32;
            let exact = f64::from(step) * (f64::from(octaves) / f64::from(OCTAVE)).exp2();
            if exact > 4.0e9 {
                continue;
            }
            let found = f64::from(bend(step, octaves));
            let error = ((found - exact).abs() - 2.0).max(0.0) / exact;
            worst = worst.max(error);
        }
        assert!(worst < 0.02 * 2.0f64.ln() / 1200.0, "{worst}");
        assert_eq!((bend(5, 40 * OCTAVE), bend(u32::MAX, -70 * OCTAVE), bend(1 << 20, OCTAVE)), (u32::MAX, 0, 1 << 21));
    }
}
