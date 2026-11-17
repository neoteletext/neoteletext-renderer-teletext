//! The chip's analogue half, and the board after it: the voices' levels
//! and offsets, the state-variable filter, the mixer with its volume
//! control, and the Commodore 64's output stage. It runs at the decimator's
//! intermediate rate, a quarter of the chip clock.
//!
//! Signals are in voice units: one unit is one step of the 12-bit waveform
//! at one step of the 8-bit envelope, so a voice at full envelope spans
//! [`FULL_SWING`] units.

use super::oscillator::WAVEFORM_MAX;
use super::{ChipClock, ChipModel};

/// The span of one voice at full envelope, in voice units.
const FULL_SWING: i32 = WAVEFORM_MAX as i32 * 255;

/// The same span in millivolts at the audio output of a 6581 at full
/// volume.
///
/// Source: Levente Harsfalvi in "The C64 Digi" (C=Hacking 20, "Tests and
/// results"), measured on a 6581 with one voice in the mixer and its
/// envelope fully open: 5.29 V with the waveform at $FFF and 6.34 V with it
/// at 0.
const FULL_SWING_MILLIVOLTS: i32 = 6_340 - 5_290;

/// The waveform value at which a 6581's envelope has no effect on the
/// voice's level, and what each voice adds to the mixer whatever its
/// envelope, in voice units.
///
/// Source: "The C64 Digi", "Tests and results". A voice with its envelope
/// closed puts the output at 5.70 V, between the 6.34 V of waveform 0 and
/// the 5.29 V of waveform $FFF at an open envelope: a higher waveform
/// value is a lower voltage, and the envelope has no effect at (6.34 -
/// 5.70) / (6.34 - 5.29) of the waveform's range. Each voice with a closed
/// envelope raises the output by 0.26 V ("5.69 - 5.43 = 0.26 volts").
/// These offsets are what the volume register multiplies when it is used
/// to play samples (Yannes interview: "By stopping an Oscillator, it was
/// possible to apply a DC voltage to this D/A"). The reference program's
/// 6581 agrees on the voices: measured from its output on 17 November
/// 2026, taking each silent voice out of the mixer lowers the volume
/// register's step by 0.22 to 0.26 of a voice's full swing, against the
/// 0.26 / 1.05 here.
const WAVEFORM_ZERO_6581: i32 = (6_340 - 5_700) * WAVEFORM_MAX as i32 / FULL_SWING_MILLIVOLTS;
const VOICE_OFFSET_6581: i32 = 260 * FULL_SWING / FULL_SWING_MILLIVOLTS;
/// What a 6581's mixer adds by itself, in voice units: 0.77 of a voice's
/// full swing.
///
/// Source: fitted to the output of reSIDfp's default 6581, measured 17
/// November 2026: with the chip silent, the step heard when the volume
/// goes from 0 to 15 is 1.59 times the span of a triangle at full volume,
/// and 0.77 times it with all three voices taken out of the mixer; with
/// this offset the two are 1.57 and 0.80 here. The chip of "The C64 Digi"
/// measured far less, 0.72 V from volume 0 to 15 against the 1.05 V of a
/// voice, and its mixer alone 0.04 V the other way; chips differ in how
/// loudly they play samples, and the offset before, taken from that
/// article, gave a step of 0.73 (TODO-6427 fault 4).
const MIXER_OFFSET_6581: i32 = FULL_SWING / 100 * 77;

/// The same for an 8580: the waveform swings about its middle, and each
/// voice and the mixer add 0.07 of a voice's full swing.
///
/// Source: fitted to the output of reSIDfp's default 8580, measured 17
/// November 2026: with the chip silent, the step heard when the volume
/// goes from 0 to 15 is 0.28 times the span of a triangle at full volume,
/// and it falls by 0.07 for each voice taken out of the mixer, to 0.07.
/// A voice held at waveform $FFF and at 0 with its envelope open moves
/// the step by 0.46 one way and 0.57 the other: the middle of the
/// waveform, less the voice's 0.07. That is five times quieter than the
/// 6581, as public accounts have it: "There is no (or no significant) DC
/// offset on the master volume D/A in the 8580" ("The C64 Digi", "The
/// 8580"), and samples played by the volume register are "very quiet" on
/// it (retroisle.com). (TODO-6427 fault 4: the offsets before, the
/// mixer's alone at 0.05, gave a step of 0.05.)
const WAVEFORM_ZERO_8580: i32 = (WAVEFORM_MAX as i32 + 1) / 2;
const VOICE_OFFSET_8580: i32 = FULL_SWING / 100 * 7;
const MIXER_OFFSET_8580: i32 = FULL_SWING / 100 * 7;

/// The levels of a chip model's converters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Levels {
    /// The waveform value at which the envelope has no effect on the
    /// voice's level: the waveform swings about it.
    pub(super) waveform_zero: i32,
    /// What a voice adds to the mixer whatever its envelope, in voice
    /// units.
    pub(super) voice_offset: i32,
    /// What the mixer adds by itself.
    mixer_offset: i32,
}

impl Levels {
    /// The levels of a chip model.
    pub(super) const fn of(model: ChipModel) -> Self {
        match model {
            ChipModel::Mos6581 => Self {
                waveform_zero: WAVEFORM_ZERO_6581,
                voice_offset: VOICE_OFFSET_6581,
                mixer_offset: MIXER_OFFSET_6581,
            },
            ChipModel::Mos8580 => Self {
                waveform_zero: WAVEFORM_ZERO_8580,
                voice_offset: VOICE_OFFSET_8580,
                mixer_offset: MIXER_OFFSET_8580,
            },
        }
    }
}

/// Fraction bits of a filter coefficient.
const COEFFICIENT_BITS: u32 = 16;
/// Fraction bits of the damping, which reaches the square root of two.
const DAMPING_BITS: u32 = 15;

/// The largest magnitude a filter state may reach, in voice units. Three
/// voices are 2²² at most and the highest resonance here four times that;
/// the bound keeps every product in the filter inside 32 bits whatever the
/// registers hold.
const STATE_LIMIT: i32 = 1 << 25;
/// The largest magnitude of the mixer's sum: with the volume's four bits
/// it stays below 2³⁰, which [`scale`] takes.
const MIX_LIMIT: i32 = 1 << 26;

/// The cutoff register's width (data sheet, registers $15 and $16: "an
/// 11-bit number").
const CUTOFF_MAX: u32 = (1 << 11) - 1;

/// Fraction bits of the 6581's rise in hertz for each register step, and
/// of the bend's coefficient below that: as many as keep the square of
/// the bend's width times the coefficient inside 32 bits.
const CUTOFF_SLOPE_BITS: u32 = 8;
const CUTOFF_BEND_BITS: u32 = 11;

/// The 6581's cutoff curve: the middle one of seven real chips, in four
/// parts. From the bottom of the register the cutoff lies at a floor; it
/// bends upwards to a straight rise; the rise slackens at a knee; and it
/// stops at a top.
///
/// Source: the shape of its foot is an own model of the designer's
/// account that the cutoff is set by "FETs as voltage-controlled
/// resistors" (Yannes interview): a transistor does not conduct below its
/// threshold, so the bottom of the register leaves the cutoff at its
/// floor; above it the conductance, and with it the cutoff, rises in
/// proportion; the bend between is a parabola that meets both lines with
/// their slopes. That the 6581's curve is far from the straight line of
/// the data sheet, and that no two chips share one, is public ("SID
/// filter measurements", Michiel Boland: "On the old SID this did not
/// really work"; "one probably won't find two identical 6581 chips", "The
/// C64 Digi"; csdb.dk forum, "6581 filter curves"). The data sheet's own
/// figures give a floor of 140 Hz (30 Hz with 2,200 pF capacitors, a
/// cutoff inversely proportional to them, and the Commodore 64's 470 pF)
/// and a top of 12 kHz.
///
/// The figures are fitted to real chips, measured 17 November 2026 from
/// what their owners published:
///
/// - Stone Oakvalley's recordings of his 6581 R2 (date code 4982) and R4
///   (3387) playing Plogue's one-page BASIC SID benchmark, which sends
///   noise through each filter output with no resonance while the
///   cutoff's upper byte counts from 0 to 255 (6581-8580.com, "The SID
///   Chips visualized & benched", 1 September 2015). Measured: the corner
///   of a two-pole low-pass fitted to the noise's spectrum in each 64
///   register steps, against the same chip's unfiltered noise.
/// - Michiel Boland's spectrogram of four real 6581s, his b1, b2, b5 and
///   b6, playing noise through the band-pass while the register sweeps
///   logarithmically ("SID filter measurements", November 2024).
///   Measured: the brightest frequency of each column of the picture,
///   which can be read to about a tenth, and no higher than 6 kHz.
/// - Stone Oakvalley's recordings of tunes on his 6581 R3 (2383), which
///   has no test recording: in the two tunes that sweep the filter its
///   spectrum is brighter than the R2's and darker than no other's.
///
/// The seven keep one order of brightness along the register: b2, b6 and
/// the R3, then the R2, then b5, the R4 and b1. The R2 is the middle chip,
/// and this curve is fitted to its corners. In hertz, the darkest chip,
/// the R2, the brightest chip, and this curve:
///
/// | Register | Darkest | R2 | Brightest | Here |
/// |---|---|---|---|---|
/// | $100 | 275 | 414 | 1,949 | 400 |
/// | $1A0 | 280 | 799 | 5,600 | 581 |
/// | $220 | 290 | 1,348 | above 6,000 | 1,388 |
/// | $2A0 | 330 | 2,723 | above 6,000 | 2,839 |
/// | $320 | 363 | 4,741 | above 6,000 | 4,936 |
/// | $3A0 | 450 | 7,479 | above 6,000 | 7,668 |
/// | $420 | 600 | 11,121 | above 6,000 | 10,612 |
/// | $4A0 | 800 | 12,010 | above 6,000 | 12,660 |
/// | $520 | 950 | 15,231 | above 6,000 | 14,708 |
/// | $7FF | 6,900 | above 16,000 | above 16,000 | 16,000 |
///
/// Two emulators run as separate programs (sidplayfp 2.16.0, both
/// engines at their defaults, measured the same way as the R2) lie
/// between the R2 and b6 below register $300, at 2.0 and 2.4 kHz for
/// $220, and with the R2 above it; the darkest and brightest settings of
/// the one that has a setting span 0.4 to 8.3 kHz there.
///
/// On every one of these chips the cutoff also rises with the level of
/// the signal in the filter, most where the curve bends; that is not
/// modelled, and the R2's corners are those of one voice of noise at
/// sustain 12. The test `the_6581_curve_is_the_middle_chips` holds the
/// curve to them. (TODO-6427 fault 2: the first curve, with a floor of
/// 140 Hz to register 384 and a top of 12 kHz, lay below the R2 by half
/// between $220 and $320; the second, fitted to one emulator's output,
/// lay above it by up to two between $1A0 and $2A0.)
mod curve_6581 {
    use super::CUTOFF_SLOPE_BITS;

    /// The floor, in hertz.
    pub(super) const FLOOR_HZ: u32 = 400;
    /// Where the bend from the floor to the rise starts and stops, in
    /// register steps.
    pub(super) const BEND: (u32, u32) = (320, 904);
    /// The middle of the bend: where the straight rise would meet the
    /// floor.
    pub(super) const THRESHOLD: u32 = u32::midpoint(BEND.0, BEND.1);
    /// The rise above the bend, in hertz a register step.
    pub(super) const SLOPE_HZ: u32 = 23;
    /// The same in [`CUTOFF_SLOPE_BITS`] fraction bits.
    pub(super) const SLOPE: u32 = SLOPE_HZ << CUTOFF_SLOPE_BITS;
    /// The register step at which the rise slackens.
    pub(super) const KNEE: u32 = 1056;
    /// The rise above the knee, in hertz a register step.
    pub(super) const UPPER_SLOPE_HZ: u32 = 16;
    /// The same in [`CUTOFF_SLOPE_BITS`] fraction bits.
    pub(super) const UPPER_SLOPE: u32 = UPPER_SLOPE_HZ << CUTOFF_SLOPE_BITS;
    /// The top, in hertz.
    pub(super) const TOP_HZ: u32 = 16_000;
}
/// What a squared distance into the 6581's bend is multiplied by, in
/// [`CUTOFF_BEND_BITS`] fraction bits more than the slope's: the slope
/// over twice the bend's width, so that the slope grows from nothing to
/// all of itself across the bend. Worked out here, once, so that setting
/// the cutoff does not divide.
const CUTOFF_BEND_COEFFICIENT: u32 =
    (curve_6581::SLOPE << CUTOFF_BEND_BITS) / (2 * (curve_6581::BEND.1 - curve_6581::BEND.0));

/// The 8580's cutoff at the bottom of the register, in hertz (data sheet:
/// "ranges from 30 Hz to 12 kHz").
const CUTOFF_FLOOR_8580_HZ: u32 = 30;
/// The 8580's rise: 6.75 Hz a register step, as a multiplier and a shift,
/// which puts the top of the register at 13.8 kHz.
///
/// Source: that the line is straight is public: the 8580's cutoff "is a
/// straight line on a linear scale, and is closer to the designers'
/// actual specifications" (retroisle.com), which are the data sheet's
/// 12 kHz. Its slope is fitted to the output of reSIDfp's default 8580,
/// measured 17 November 2026 as the corner of its low-pass for noise at
/// the highest resonance: 1,733 Hz at register $100, 3,496 at $200, 7,057
/// at $400, 10,689 at $600 and 13,803 at $7FF, where this line has 1,758,
/// 3,486, 6,942, 10,398 and 13,847. The slope before, 12,500 Hz over the
/// register, lay a tenth lower (TODO-6427 fault 6).
const CUTOFF_SLOPE_8580: (u32, u32) = (27, 2);

/// The damping (the reciprocal of Q) with no resonance: the square root of
/// two, a filter with no peak at all, in [`DAMPING_BITS`] fraction bits.
///
/// Source: own choice. Data sheet, register $17: "16 resonance settings
/// ranging linearly from no resonance (0) to maximum resonance (15)".
const DAMPING_NONE: u32 = 46_341;
/// The 6581's damping at the highest resonance: a Q of 2.4, a peak of
/// 7.6 dB (1 / 2.4 = 5 / 12). Between none and the highest the damping
/// falls in equal steps, as a ladder of weighted resistors feeding back a
/// share of the output would make it (Yannes interview).
///
/// Source: fitted to the output of reSIDfp's default 6581, measured 17
/// November 2026: noise through its low-pass at the highest resonance
/// fits a two-pole response with a Q of 2.0 to 2.5 for cutoff registers
/// from $400 up, and 1.0 to 1.1 at resonance 8, where equal steps of
/// damping from none give 1.13. That the older chip has the less is
/// public: the designer could not get high resonance from the 6581 ("It
/// would also have greatly improved the filter, particularly in achieving
/// high resonance", Yannes interview), and the 8580 "can achieve higher
/// resonances" (Wikipedia, "MOS Technology 6581", "Revisions").
const DAMPING_HIGHEST_6581: u32 = (5 << DAMPING_BITS) / 12;
/// What each step of the 8580's resonance multiplies its damping by, in
/// [`COEFFICIENT_BITS`] fraction bits: 0.9192, the fifteenth root of
/// 0.2828, so that the highest resonance is a Q of 2.5 (a peak of 8 dB;
/// 1 / 2.5 = 0.2828 * √2).
///
/// Source: fitted to the output of reSIDfp's default 8580, measured 17
/// November 2026: noise through its low-pass fits a two-pole response
/// with a Q of 0.67 to 0.72 with no resonance, 1.36 to 1.44 at resonance
/// 8 and 2.42 to 2.65 at resonance 15, at every cutoff from $0C0 to $780.
/// Equal ratios from 0.707 to 2.5 give 1.39 at resonance 8; equal steps
/// of damping, as on the 6581, would give 1.14. The Q of 4 before was an
/// own round figure and peaked 4 dB above the reference (TODO-6427 fault
/// 6). The test `the_8580s_highest_resonance_is_the_one_fitted` works the
/// fifteen steps out again.
const DAMPING_RATIO_8580: u32 = 60_241;
/// Resonance settings above none.
const RESONANCE_STEPS: u32 = 15;

/// The 6581's high-pass output is mixed 3 dB below the others: 1/√2 in
/// [`COEFFICIENT_BITS`] fraction bits.
///
/// Source: retroisle.com, "The SID (Sound Interface Device) Chip": "the
/// highpass component of the filter was mixed in 3 dB attenuated compared
/// to the other outputs, making the sound more bassy".
const HIGH_PASS_GAIN_6581: u32 = 46_341;

/// Where the 6581's filter input begins to saturate, in voice units, and
/// the width of the knee above that as a power of two; beyond the knee the
/// input is flat, half the knee's width above its start.
///
/// Source: own model. The 6581's filter "often distorts" (Wikipedia) and
/// "is strongly nonlinear" ("The C64 Digi"); its amplifiers are inverters
/// with little gain (Yannes interview), which run out of swing. One voice
/// at full envelope, about 2²⁰ units, passes untouched; three together
/// are bent. The knee is a parabola from full gain to none.
const SATURATION_START: i32 = 1 << 20;
const SATURATION_WIDTH_BITS: u32 = 21;
/// The level beyond the knee.
const SATURATION_FLAT: i32 = SATURATION_START + (1 << (SATURATION_WIDTH_BITS - 1));
/// Bits dropped from the distance into the knee before it is squared, so
/// that the square fits 32 bits: the distance is below 2²¹.
const SATURATION_PRE_SHIFT: u32 = 6;
/// Shift from that square to distance² / (2 * width), the parabola's fall.
const SATURATION_POST_SHIFT: u32 = SATURATION_WIDTH_BITS + 1 - 2 * SATURATION_PRE_SHIFT;

/// What the 6581's filter input is multiplied by with no resonance and
/// with the highest, in [`COEFFICIENT_BITS`] fraction bits: 0.67 and
/// 0.80, with equal steps between.
///
/// Source: fitted to the output of reSIDfp's default 6581, measured 17
/// November 2026: noise through its low-pass is 3.5 dB below the same
/// noise sent past the filter in the pass band with no resonance, 2.9 dB
/// at resonance 8 and 1.9 dB at resonance 15, at a fifth of full level as
/// at full level.
const FILTER_GAIN_6581: (u32, u32) = (43_909, 52_429);

/// What the mixer's sum times the volume's sixteen steps is multiplied by
/// to give output units, in [`COEFFICIENT_BITS`] fraction bits, for each
/// chip.
///
/// Source: own choice of scale, fitted to the output of reSIDfp's default
/// 6581 and 8580 (sidplayfp 2.16.0, run as a separate program), measured 17
/// November 2026: one triangle at full envelope and full volume spans
/// 9,530 output units from trough to peak there on a 6581 and 7,437 on an
/// 8580, and with these gains 9,417 and 7,533 here, each within 0.15 dB.
/// Three pulses in step at full volume, the loudest three voices can be
/// with no filter, then peak at 14,100 of a sample's 32,767 on a 6581 and
/// 11,300 on an 8580: 7 dB and 9 dB of headroom for the filter's resonance
/// and the volume register's steps (TODO-6427 fault 5; a gain of 64 for
/// both chips before it clipped three of nine tunes).
const fn output_gain(model: ChipModel) -> u32 {
    match model {
        ChipModel::Mos6581 => 40,
        ChipModel::Mos8580 => 32,
    }
}

/// The share of the difference that the output stage's low-pass follows
/// in each intermediate sample, in [`COEFFICIENT_BITS`] fraction bits, for
/// each clock: 1 - exp(-4 / (clock * R * C)).
///
/// Source: Commodore 64 schematic for assembly 250469 (zimmers.net,
/// `250469-rev.A.pdf`, sheet 2): the chip's audio output passes R9, 10 kΩ,
/// to C74, 1 nF, at the base of the emitter follower Q3. The same parts
/// follow a 6581 on the earlier boards; the note that R8, the 1 kΩ load,
/// "should be removed for 8580 SID" changes the level only, which this
/// model does not carry. The test `the_output_low_pass_is_the_boards_rc`
/// computes these again.
const fn output_low_pass(clock: ChipClock) -> u32 {
    match clock {
        ChipClock::Pal => 21_868,
        ChipClock::Ntsc => 21_214,
    }
}

/// Shift of the AC coupling's follower: the output loses its DC level
/// with a corner of the intermediate rate over 2π * 2¹¹, 19 Hz.
///
/// Source: own choice near the board's values: the output leaves through
/// C77, 10 µF (the same schematic), into a load the board does not fix;
/// 1 kΩ, the data sheet's standard load, would be 16 Hz.
const COUPLING_SHIFT: u32 = 11;
/// Fraction bits the coupling's state keeps below a sample: one more than
/// its shift, so that the level it settles to is within half a sample of
/// the true one.
const COUPLING_FRACTION_BITS: u32 = COUPLING_SHIFT + 1;
/// The largest magnitude the output stage takes, four times a sample's
/// range: with the coupling's fraction bits it stays inside 32 bits.
const OUTPUT_LIMIT: i32 = 1 << 17;

/// 2π * 2¹⁶ over the intermediate rate, in [`COEFFICIENT_BITS`] fraction
/// bits: the filter coefficient for a cutoff of one hertz. The coefficient
/// of the two integrators is 2 sin(π f / rate), taken as 2π f / rate: at
/// the highest cutoff here the two differ by less than one per cent.
///
/// Source: Hal Chamberlin, "Musical Applications of Microprocessors"
/// (1985), the digital state-variable filter. The test
/// `the_cutoff_scale_is_two_pi_over_the_rate` computes these again.
const fn cutoff_scale(clock: ChipClock) -> u32 {
    match clock {
        ChipClock::Pal => 109_561,
        ChipClock::Ntsc => 105_546,
    }
}

/// Bits of the mode and volume register (data sheet, register $18).
mod mode {
    pub(super) const LOW_PASS: u8 = 0x10;
    pub(super) const BAND_PASS: u8 = 0x20;
    pub(super) const HIGH_PASS: u8 = 0x40;
    pub(super) const VOICE_3_OFF: u8 = 0x80;
    pub(super) const VOLUME: u8 = 0x0F;
}

/// The filter, the mixer and the output stage.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Analogue {
    model: ChipModel,
    clock: ChipClock,
    cutoff: u32,
    resonance: u8,
    routing: u8,
    mode_volume: u8,
    /// The integrators' coefficient for the cutoff register: the 8580's,
    /// and the 6581's with no signal in the filter.
    coefficient: u32,
    damping: u32,
    /// What the 6581's filter input is multiplied by at this resonance.
    filter_gain: u32,
    low: i32,
    band: i32,
    /// The output stage's low-pass.
    output: i32,
    /// The DC level the coupling capacitor has taken up.
    coupling: i32,
}

impl Analogue {
    /// The analogue half as the chip's reset leaves it.
    pub(super) const fn new(model: ChipModel, clock: ChipClock) -> Self {
        Self {
            model,
            clock,
            cutoff: 0,
            resonance: 0,
            routing: 0,
            mode_volume: 0,
            coefficient: coefficient(model, clock, 0),
            damping: DAMPING_NONE,
            filter_gain: FILTER_GAIN_6581.0,
            low: 0,
            band: 0,
            output: 0,
            coupling: 0,
        }
    }

    /// Writes the cutoff's low three bits (register $15).
    pub(super) fn write_cutoff_low(&mut self, value: u8) {
        self.cutoff = (self.cutoff & !0x7) | u32::from(value & 0x7);
        self.coefficient = coefficient(self.model, self.clock, self.cutoff);
    }

    /// Writes the cutoff's upper eight bits (register $16).
    pub(super) fn write_cutoff_high(&mut self, value: u8) {
        self.cutoff = (self.cutoff & 0x7) | (u32::from(value) << 3);
        self.coefficient = coefficient(self.model, self.clock, self.cutoff);
    }

    /// Writes the resonance and the routing bits (register $17).
    pub(super) fn write_resonance_routing(&mut self, value: u8) {
        self.resonance = value >> 4;
        self.routing = value & 0x0F;
        self.damping = damping(self.model, self.resonance);
        let rise = FILTER_GAIN_6581.1.saturating_sub(FILTER_GAIN_6581.0);
        self.filter_gain =
            FILTER_GAIN_6581.0.saturating_add(rise.saturating_mul(u32::from(self.resonance)) / RESONANCE_STEPS);
    }

    /// Writes the filter's mode and the volume (register $18).
    pub(super) fn write_mode_volume(&mut self, value: u8) {
        self.mode_volume = value;
    }

    /// Whether a voice, 0 to 2, goes through the filter.
    pub(super) const fn filters(&self, voice: u8) -> bool {
        self.routing >> voice & 1 != 0
    }

    /// Whether voice 3 is cut from the direct path (data sheet, register
    /// $18, "3 OFF").
    pub(super) const fn voice_3_off(&self) -> bool {
        self.mode_volume & mode::VOICE_3_OFF != 0
    }

    /// One intermediate sample: the sum of the voices sent to the filter
    /// and of those sent past it, in voice units, to one output sample.
    #[inline]
    pub(super) fn process(&mut self, filtered: i32, direct: i32) -> i16 {
        let input = match self.model {
            ChipModel::Mos6581 => scale(saturate(filtered), self.filter_gain),
            ChipModel::Mos8580 => filtered,
        };
        let coefficient = self.coefficient;
        // The two-integrator loop (Yannes interview: "a classic multi-mode
        // (state variable) VCF design"), in Chamberlin's digital form. Its
        // poles are inside the unit circle while the coefficient f and the
        // damping q keep f * q < 2 and f² + 2 f q < 4; the largest f here
        // is 0.42 and the largest q 1.42. The bounds on the states are for
        // the 6581's saturating input, which no such condition covers.
        let damped = scale(self.band, self.damping).wrapping_mul(1 << (COEFFICIENT_BITS - DAMPING_BITS));
        let high = input.wrapping_sub(self.low).wrapping_sub(damped);
        self.band = self.band.wrapping_add(scale(high, coefficient)).clamp(-STATE_LIMIT, STATE_LIMIT);
        self.low = self.low.wrapping_add(scale(self.band, coefficient)).clamp(-STATE_LIMIT, STATE_LIMIT);

        // "The C64 Digi": "the filter inverts phase", measured through the
        // low-pass. Each integrator of the loop inverts once more, so the
        // band-pass is upright and the high-pass inverted like the low.
        let mut mix = direct.wrapping_add(Levels::of(self.model).mixer_offset);
        if self.mode_volume & mode::LOW_PASS != 0 {
            mix = mix.wrapping_sub(self.low);
        }
        if self.mode_volume & mode::BAND_PASS != 0 {
            mix = mix.wrapping_add(self.band);
        }
        if self.mode_volume & mode::HIGH_PASS != 0 {
            mix = mix.wrapping_sub(match self.model {
                ChipModel::Mos6581 => scale(high, HIGH_PASS_GAIN_6581),
                ChipModel::Mos8580 => high,
            });
        }
        let volume = i32::from(self.mode_volume & mode::VOLUME);
        let level = scale(mix.clamp(-MIX_LIMIT, MIX_LIMIT).wrapping_mul(volume), output_gain(self.model))
            .clamp(-OUTPUT_LIMIT, OUTPUT_LIMIT);

        // The board: a low-pass, then the coupling capacitor.
        let step = scale(level.wrapping_sub(self.output), output_low_pass(self.clock));
        self.output = self.output.wrapping_add(step);
        let raised = self.output.wrapping_mul(1 << COUPLING_FRACTION_BITS);
        self.coupling = self.coupling.wrapping_add(raised.wrapping_sub(self.coupling) >> COUPLING_SHIFT);
        let coupled = self.output.wrapping_sub(self.coupling >> COUPLING_FRACTION_BITS);
        coupled.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }

    /// The filter's two states, for the tests that bound them.
    #[cfg(test)]
    const fn states(&self) -> (i32, i32) {
        (self.low, self.band)
    }
}

/// `value * coefficient / 2¹⁶`, rounded down, for a coefficient below 2¹⁶
/// and a value below 2³⁰ in magnitude. It is two 32-bit multiplications of
/// the value's halves, so that a processor with no 64-bit product does
/// not need one.
#[inline]
const fn scale(value: i32, coefficient: u32) -> i32 {
    let upper = value >> COEFFICIENT_BITS;
    let lower = value.cast_unsigned() & ((1 << COEFFICIENT_BITS) - 1);
    upper
        .wrapping_mul(coefficient.cast_signed())
        .wrapping_add((lower.wrapping_mul(coefficient) >> COEFFICIENT_BITS).cast_signed())
}

/// The 6581's saturating filter input: unchanged up to the knee, flat
/// beyond it.
#[inline]
const fn saturate(value: i32) -> i32 {
    let magnitude = value.saturating_abs();
    if magnitude <= SATURATION_START {
        return value;
    }
    let into = magnitude.wrapping_sub(SATURATION_START);
    let bent = if into >= 1 << SATURATION_WIDTH_BITS {
        SATURATION_FLAT
    } else {
        // The bits dropped before the square leave the parabola's end a
        // few units above the flat level; it is held to it.
        let cut = into >> SATURATION_PRE_SHIFT;
        let parabola = magnitude.wrapping_sub(cut.wrapping_mul(cut) >> SATURATION_POST_SHIFT);
        if parabola > SATURATION_FLAT { SATURATION_FLAT } else { parabola }
    };
    if value < 0 { bent.wrapping_neg() } else { bent }
}

/// The damping, the reciprocal of Q, for a resonance setting of 0 to 15.
/// It is worked out when the register is written, not for each sample.
const fn damping(model: ChipModel, resonance: u8) -> u32 {
    match model {
        ChipModel::Mos6581 => {
            let span = DAMPING_NONE.saturating_sub(DAMPING_HIGHEST_6581);
            DAMPING_NONE.saturating_sub(span.saturating_mul(resonance as u32) / RESONANCE_STEPS)
        }
        ChipModel::Mos8580 => {
            let mut damping = DAMPING_NONE.cast_signed();
            let mut step = 0;
            while step < resonance {
                damping = scale(damping, DAMPING_RATIO_8580);
                step = step.saturating_add(1);
            }
            damping.cast_unsigned()
        }
    }
}

/// The cutoff in hertz for a value of the cutoff register.
pub(super) const fn cutoff_hz(model: ChipModel, register: u32) -> u32 {
    let register = if register > CUTOFF_MAX { CUTOFF_MAX } else { register };
    match model {
        ChipModel::Mos6581 => {
            let rise = if register <= curve_6581::BEND.0 {
                0
            } else if register < curve_6581::BEND.1 {
                // The parabola whose slope grows from nothing to the
                // line's across the bend: slope * d² / (2 * width).
                let into = register.saturating_sub(curve_6581::BEND.0);
                into.saturating_mul(into).saturating_mul(CUTOFF_BEND_COEFFICIENT) >> CUTOFF_BEND_BITS
            } else if register <= curve_6581::KNEE {
                curve_6581::SLOPE.saturating_mul(register.saturating_sub(curve_6581::THRESHOLD))
            } else {
                let knee = curve_6581::SLOPE.saturating_mul(curve_6581::KNEE.saturating_sub(curve_6581::THRESHOLD));
                knee.saturating_add(curve_6581::UPPER_SLOPE.saturating_mul(register.saturating_sub(curve_6581::KNEE)))
            };
            let hz = curve_6581::FLOOR_HZ.saturating_add(rise >> CUTOFF_SLOPE_BITS);
            if hz > curve_6581::TOP_HZ { curve_6581::TOP_HZ } else { hz }
        }
        ChipModel::Mos8580 => {
            CUTOFF_FLOOR_8580_HZ.saturating_add(register.saturating_mul(CUTOFF_SLOPE_8580.0) >> CUTOFF_SLOPE_8580.1)
        }
    }
}

/// The integrators' coefficient for a value of the cutoff register.
const fn coefficient(model: ChipModel, clock: ChipClock, register: u32) -> u32 {
    cutoff_hz(model, register).saturating_mul(cutoff_scale(clock)) >> COEFFICIENT_BITS
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::panic,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "a test"
)]
mod tests {
    use super::{
        Analogue, CUTOFF_BEND_COEFFICIENT, CUTOFF_MAX, ChipClock, ChipModel, DAMPING_BITS, DAMPING_NONE, FULL_SWING,
        Levels, MIX_LIMIT, SATURATION_FLAT, SATURATION_START, STATE_LIMIT, coefficient, curve_6581, cutoff_hz,
        cutoff_scale, damping, output_low_pass, saturate, scale,
    };

    /// A small generator of test values (a 64-bit linear congruential
    /// generator; the constants are Knuth's MMIX).
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 32) as u32
        }
    }

    /// e^-(numerator / denominator) in 32 fraction bits, by its power
    /// series in integers.
    fn exp_negative(numerator: u128, denominator: u128) -> u128 {
        let one = 1u128 << 64;
        let x = (numerator << 64) / denominator;
        let (mut sum, mut term, mut negative) = (one, one, true);
        for k in 1..40u128 {
            term = term * x / one / k;
            sum = if negative { sum - term } else { sum + term };
            negative = !negative;
        }
        sum >> 32
    }

    #[test]
    fn the_output_low_pass_is_the_boards_rc() {
        // R9 = 10 kΩ and C74 = 1 nF: RC = 10 µs, so an intermediate sample
        // of four cycles is 400,000 / clock time constants.
        for clock in [ChipClock::Pal, ChipClock::Ntsc] {
            let share = (1u128 << 32) - exp_negative(400_000, u128::from(clock.hz()));
            let rounded = (share + (1 << 15)) >> 16;
            assert_eq!(u128::from(output_low_pass(clock)), rounded, "{clock:?}");
        }
    }

    #[test]
    fn the_cutoff_scale_is_two_pi_over_the_rate() {
        // 2π * 2³² / (clock / 4), with π to sixteen digits.
        const PI_E15: u128 = 3_141_592_653_589_793;
        for clock in [ChipClock::Pal, ChipClock::Ntsc] {
            let scale = 2 * PI_E15 * (1 << 32) * 4 / (u128::from(clock.hz()) * 1_000_000_000_000_000);
            assert!(u128::from(cutoff_scale(clock)).abs_diff(scale) <= 1, "{clock:?}: {scale}");
        }
    }

    #[test]
    fn levels_are_the_measured_ones() {
        let levels = Levels::of(ChipModel::Mos6581);
        assert_eq!(levels.waveform_zero, 2496);
        // 0.26 V of the 1.05 V that 4095 * 255 units are, and 0.77 of
        // those units.
        assert_eq!(levels.voice_offset, 258_570);
        assert_eq!(levels.mixer_offset, 804_034);
        let levels = Levels::of(ChipModel::Mos8580);
        assert_eq!((levels.waveform_zero, levels.voice_offset, levels.mixer_offset), (2048, 73_094, 73_094));
    }

    #[test]
    fn the_8580s_highest_resonance_is_the_one_fitted() {
        // No resonance is a Q of 1 / √2 on both chips, and the damping
        // falls with every step.
        for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
            assert_eq!(damping(model, 0), DAMPING_NONE);
            for resonance in 1..16 {
                assert!(damping(model, resonance) < damping(model, resonance - 1), "{model:?} {resonance}");
            }
        }
        // The highest: 1 / 2.4 on the 6581 and 1 / 2.5 on the 8580, in 15
        // fraction bits, the 8580's within a half per cent for the
        // rounding of its fifteen steps; and its middle, 1 / 1.39.
        assert_eq!(damping(ChipModel::Mos6581, 15), 13_653);
        assert!(damping(ChipModel::Mos8580, 15).abs_diff((1 << DAMPING_BITS) * 2 / 5) < 66);
        assert!(damping(ChipModel::Mos8580, 8).abs_diff((1 << DAMPING_BITS) * 100 / 139) < 240);
    }

    #[test]
    fn scale_is_the_64_bit_product() {
        let mut random = Random(1);
        for _ in 0..100_000 {
            let value = (random.next() as i32) >> 2;
            let coefficient = random.next() >> 16;
            let exact = (i64::from(value) * i64::from(coefficient)) >> 16;
            assert_eq!(i64::from(scale(value, coefficient)), exact, "{value} * {coefficient}");
        }
    }

    #[test]
    fn the_cutoff_rises_with_the_register_on_both_chips() {
        for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
            for register in 1..=CUTOFF_MAX {
                assert!(cutoff_hz(model, register) >= cutoff_hz(model, register - 1), "{model:?} at {register}");
                for clock in [ChipClock::Pal, ChipClock::Ntsc] {
                    assert!(coefficient(model, clock, register) >= coefficient(model, clock, register - 1));
                }
            }
            assert!(cutoff_hz(model, CUTOFF_MAX) > 20 * cutoff_hz(model, 0));
        }
    }

    #[test]
    fn the_6581_curve_has_a_floor_a_bend_two_rises_and_a_top() {
        use curve_6581::{BEND, FLOOR_HZ, KNEE, SLOPE_HZ, THRESHOLD, TOP_HZ, UPPER_SLOPE_HZ};
        let hz = |register: u32| cutoff_hz(ChipModel::Mos6581, register);
        assert_eq!((hz(0), hz(BEND.0)), (FLOOR_HZ, FLOOR_HZ));
        // The straight rise from where it would meet the floor, half way
        // along the bend; then the slacker rise from the knee.
        for register in [BEND.1, u32::midpoint(BEND.1, KNEE), KNEE] {
            assert_eq!(hz(register), FLOOR_HZ + (register - THRESHOLD) * SLOPE_HZ, "{register}");
        }
        assert_eq!(hz(KNEE + 100), hz(KNEE) + 100 * UPPER_SLOPE_HZ);
        // The bend leaves the floor level and meets the line at the
        // line's slope, and its arithmetic stays inside 32 bits.
        assert!(hz(BEND.0 + 1) - hz(BEND.0) <= 1);
        let near = SLOPE_HZ - 1..=SLOPE_HZ + 1;
        assert!(near.contains(&(hz(BEND.1) - hz(BEND.1 - 1))), "{}", hz(BEND.1) - hz(BEND.1 - 1));
        assert!(near.contains(&(hz(BEND.1 + 1) - hz(BEND.1))));
        let width = u64::from(BEND.1 - BEND.0);
        assert!(u32::try_from(width * width * u64::from(CUTOFF_BEND_COEFFICIENT)).is_ok());
        // The top is reached inside the register and held.
        assert!(hz(CUTOFF_MAX - 256) == TOP_HZ && hz(CUTOFF_MAX) == TOP_HZ);
        assert!(hz(KNEE) < TOP_HZ);
    }

    #[test]
    fn the_6581_curve_is_the_middle_chips() {
        // The low-pass corners of Stone Oakvalley's 6581 R2, the middle
        // one of seven real chips, measured 17 November 2026 from his
        // recording of the benchmark (the table at `curve_6581`), in
        // hertz for each register. The curve passes within three tenths
        // of each. TODO-6427 fault 2: the curve fitted to one emulator's
        // output had 2,826 Hz at $220, twice the R2's, and the one before
        // it 526 Hz, two fifths.
        let middle = [
            (0x160, 465),
            (0x1A0, 799),
            (0x1E0, 924),
            (0x220, 1_348),
            (0x260, 1_662),
            (0x2A0, 2_723),
            (0x2E0, 3_603),
            (0x320, 4_741),
            (0x360, 6_017),
            (0x3A0, 7_479),
            (0x3E0, 9_176),
            (0x420, 11_121),
            (0x460, 10_624),
            (0x4A0, 12_010),
            (0x4E0, 13_362),
            (0x520, 15_231),
        ];
        for (register, measured) in middle {
            let found = cutoff_hz(ChipModel::Mos6581, register);
            assert!(found.abs_diff(measured) * 10 <= measured * 3, "${register:03X}: {found} Hz for {measured}");
        }
        // And between the darkest and the brightest of the seven where
        // both could be read: Michiel Boland's b1 and b2.
        for (register, darkest, brightest) in [(0x100, 275, 1_949), (0x180, 278, 5_248), (0x200, 288, 6_918)] {
            let found = cutoff_hz(ChipModel::Mos6581, register);
            assert!(found > darkest && found < brightest, "${register:03X}: {found} Hz");
        }
    }

    #[test]
    fn the_8580_curve_is_a_straight_line() {
        assert_eq!(cutoff_hz(ChipModel::Mos8580, 0), 30);
        assert_eq!(cutoff_hz(ChipModel::Mos8580, 1024), 30 + 6_912);
        assert_eq!(cutoff_hz(ChipModel::Mos8580, CUTOFF_MAX), 30 + 13_817);
        // The corners of the reference program's low-pass, measured from
        // its output on 17 November 2026 (CUTOFF_SLOPE_8580): the line
        // passes within a twentieth of each.
        for (register, hz) in [(0x100, 1_733), (0x200, 3_496), (0x400, 7_057), (0x600, 10_689), (0x7FF, 13_803)] {
            let found = cutoff_hz(ChipModel::Mos8580, register);
            assert!(found.abs_diff(hz) * 20 < hz, "${register:03X}: {found} Hz for {hz}");
        }
    }

    #[test]
    fn the_coefficient_stays_inside_the_stable_range() {
        // f * q < 2 and f² + 2 f q < 4 with the damping q at most √2: in
        // 16 fraction bits, with q rounded up to 1.4143.
        const ONE: u64 = 1 << 16;
        const DAMPING: u64 = 92_688;
        for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
            for clock in [ChipClock::Pal, ChipClock::Ntsc] {
                let highest =
                    u64::from((0..=CUTOFF_MAX).map(|register| coefficient(model, clock, register)).max().unwrap());
                assert!(highest * DAMPING < 2 * ONE * ONE, "{model:?} {clock:?}: {highest}");
                assert!(highest * highest + 2 * highest * DAMPING < 4 * ONE * ONE, "{model:?} {clock:?}: {highest}");
                // And 2π f / rate stands for 2 sin(π f / rate) within one
                // per cent: the coefficient is below a half.
                assert!(highest < ONE / 2, "{model:?} {clock:?}: {highest}");
                assert!(coefficient(model, clock, 0) > 0);
            }
        }
    }

    #[test]
    fn saturation_is_monotonic_bounded_and_odd() {
        let mut last = saturate(-MIX_LIMIT);
        for value in (-MIX_LIMIT..=MIX_LIMIT).step_by(4_099) {
            let bent = saturate(value);
            assert!(bent >= last, "at {value}");
            assert!(bent.abs() <= SATURATION_FLAT);
            assert_eq!(saturate(-value), -bent);
            if value.abs() <= SATURATION_START {
                assert_eq!(bent, value);
            }
            last = bent;
        }
    }

    #[test]
    fn the_filter_stays_bounded_for_any_registers_and_any_input() {
        let mut random = Random(6581);
        for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
            for clock in [ChipClock::Pal, ChipClock::Ntsc] {
                let mut analogue = Analogue::new(model, clock);
                for _ in 0..2_000 {
                    analogue.write_cutoff_low(random.next() as u8);
                    analogue.write_cutoff_high(random.next() as u8);
                    analogue.write_resonance_routing(random.next() as u8);
                    analogue.write_mode_volume(random.next() as u8);
                    // Inputs of any sign up to three voices at their
                    // largest, held, alternating and random.
                    let largest = 3 * (FULL_SWING + Levels::of(model).voice_offset);
                    let kind = random.next() % 3;
                    for sample in 0..200 {
                        let input = match kind {
                            0 => largest,
                            1 if sample % 2 == 0 => largest,
                            1 => -largest,
                            _ => (random.next() as i32) % largest,
                        };
                        let _ = analogue.process(input, -input);
                        let (low, band) = analogue.states();
                        assert!(low.abs() <= STATE_LIMIT && band.abs() <= STATE_LIMIT);
                    }
                }
            }
        }
    }

    #[test]
    fn a_silent_filter_settles_to_silence() {
        for model in [ChipModel::Mos6581, ChipModel::Mos8580] {
            let mut analogue = Analogue::new(model, ChipClock::Pal);
            analogue.write_cutoff_high(0x80);
            analogue.write_resonance_routing(0xF7);
            analogue.write_mode_volume(0x7F);
            for _ in 0..1_000 {
                let _ = analogue.process(FULL_SWING, 0);
            }
            for _ in 0..200_000 {
                let _ = analogue.process(0, 0);
            }
            let (low, band) = analogue.states();
            // Rounding leaves a few units of the 2²⁰ of a voice.
            assert!(low.abs() < 64 && band.abs() < 64, "{model:?}: {low} {band}");
        }
    }
}
