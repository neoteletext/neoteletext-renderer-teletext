//! A voice's oscillator and waveform generator: the 24-bit phase
//! accumulator, the four waveforms made from its bits, and what the
//! waveform selector does when several, or none, are selected (Yannes
//! interview, "the Oscillator is a 24-bit phase-accumulating design").

use super::ChipModel;

/// The accumulator's width: output frequency is the frequency register
/// times the clock over 2²⁴ (data sheet, registers 0 and 1: "Fout =
/// (Fn * Fclk / 16777216) Hz").
const ACCUMULATOR_BITS: u32 = 24;
const ACCUMULATOR_MASK: u32 = (1 << ACCUMULATOR_BITS) - 1;
/// The accumulator's top bit: it folds the triangle, and its rise is what
/// synchronises and ring-modulates the next voice.
const ACCUMULATOR_MSB: u32 = 1 << (ACCUMULATOR_BITS - 1);
/// The waveform's width: the upper twelve bits of the accumulator feed a
/// 12-bit converter (Yannes interview).
const WAVEFORM_BITS: u32 = 12;
/// The largest waveform value.
pub(super) const WAVEFORM_MAX: u16 = (1 << WAVEFORM_BITS) - 1;
/// Shift from the accumulator to the sawtooth.
const SAWTOOTH_SHIFT: u32 = ACCUMULATOR_BITS - WAVEFORM_BITS;

/// The accumulator bit whose rise shifts the noise register.
///
/// Source: Asger Alstrup, "Examination of SID noise waveform", Table 1:
/// the noise value changes every $100 cycles at frequency $1000 and every
/// $20 at $8000, which is once for every 2²⁰ the accumulator advances, the
/// period of its bit 19.
const NOISE_CLOCK: u32 = 1 << 19;
/// The noise register's width (Yannes interview: "a 23-bit pseudo-random
/// sequence generator").
const NOISE_BITS: u32 = 23;
const NOISE_MASK: u32 = (1 << NOISE_BITS) - 1;
/// The two register bits whose exclusive-or enters at bit 0 on a shift.
///
/// Source: Alstrup, "Examination of SID noise waveform" ("the bits 22 and
/// 17 of the internal register provides the feed for bit 0 through an
/// eor-gate"); oxyron.de SID reference says the same.
const NOISE_FEEDBACK: (u32, u32) = (22, 17);
/// The register bits read as the upper eight bits of the noise waveform,
/// highest output bit first.
///
/// Source: Alstrup, "Examination of SID noise waveform": register $1B
/// shows bits 22, 20, 16, 13, 11, 7, 4 and 2. The designer says twelve
/// bits reach the converter; which register bits the lower four are is in
/// no public source, and they are zero here (own choice).
const NOISE_TAPS: [u32; 8] = [22, 20, 16, 13, 11, 7, 4, 2];
/// The noise register after a reset by the test bit, and at power-on.
///
/// Source: Alstrup, "Examination of SID noise waveform": the sequence read
/// after the test bit begins $FE, $FC, $FC..., reproduced from "the initial
/// value of the internal register" of twenty ones over three zeros. That
/// power-on leaves the same value is an own choice.
const NOISE_RESET: u32 = 0x7F_FFF8;
/// The noise waveform of [`NOISE_RESET`]: $FE in its upper eight bits, as
/// Alstrup read it. The test `the_reset_waveform_is_the_reset_registers`
/// holds the two together.
const NOISE_RESET_WAVEFORM: u16 = 0xFE0;
/// Shift from the eight tapped bits to the top of the waveform.
const NOISE_WAVEFORM_SHIFT: u32 = WAVEFORM_BITS - 8;
/// Cycles the test bit must be held before the noise register is reset.
///
/// Source: Alstrup, "Examination of SID noise waveform": "the reseting of
/// a waveform with the TEST-bit does not reset the waveform immediately,
/// but rather $2000-$8000 cycles later (this figure varies greatly)". The
/// low end of the measured range is the own choice, so that a player
/// holding the bit for one 50 Hz frame (19,656 cycles) gets its reset.
const NOISE_RESET_CYCLES: u16 = 0x2000;

/// Cycles between each fall of one unit of a waveform value held with no
/// waveform selected.
///
/// Source: codebase64, "Vicious Sid Demo Routine Explained", section
/// "Examining Waveform $00": "the current waveform level is sustained",
/// and "the output of the voice will fade from $ff to $00 within some
/// seconds". The rate is an own choice within that: 4,095 units at 512
/// cycles each are 2.1 seconds at 1 MHz, and the fall is linear for want of
/// a measured curve.
const FLOATING_FALL_CYCLES: u16 = 512;

/// Bits of a voice's control register (data sheet, register 4).
pub(super) mod control {
    /// The envelope's gate.
    pub(in super::super) const GATE: u8 = 0x01;
    /// Synchronise this oscillator to the previous voice's.
    pub(super) const SYNC: u8 = 0x02;
    /// Ring-modulate the triangle with the previous voice's oscillator.
    pub(super) const RING: u8 = 0x04;
    /// Hold the oscillator at zero.
    pub(super) const TEST: u8 = 0x08;
    /// Select the triangle.
    pub(super) const TRIANGLE: u8 = 0x10;
    /// Select the sawtooth.
    pub(super) const SAWTOOTH: u8 = 0x20;
    /// Select the pulse.
    pub(super) const PULSE: u8 = 0x40;
    /// Select the noise.
    pub(super) const NOISE: u8 = 0x80;
    /// All four waveform selections.
    pub(super) const WAVEFORMS: u8 = TRIANGLE | SAWTOOTH | PULSE | NOISE;
}

/// One oscillator with its waveform generator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Oscillator {
    accumulator: u32,
    frequency: u32,
    pulse_width: u32,
    control: u8,
    noise: u32,
    /// The noise waveform for the register as it stands.
    noise_waveform: u16,
    /// The waveform value last driven onto the converter's inputs.
    held: u16,
    /// Cycles since the held value last fell.
    held_cycles: u16,
    /// Cycles the test bit has been held, up to the reset.
    test_cycles: u16,
    /// The accumulator's top bit rose in the cycle just run.
    msb_rose: bool,
}

impl Oscillator {
    /// An oscillator as the chip's reset leaves it: every register zero.
    pub(super) const fn new() -> Self {
        Self {
            accumulator: 0,
            frequency: 0,
            pulse_width: 0,
            control: 0,
            noise: NOISE_RESET,
            noise_waveform: NOISE_RESET_WAVEFORM,
            held: 0,
            held_cycles: 0,
            test_cycles: 0,
            msb_rose: false,
        }
    }

    /// Writes the low byte of the frequency (registers 0, 7 and 14).
    pub(super) fn write_frequency_low(&mut self, value: u8) {
        self.frequency = (self.frequency & 0xFF00) | u32::from(value);
    }

    /// Writes the high byte of the frequency (registers 1, 8 and 15).
    pub(super) fn write_frequency_high(&mut self, value: u8) {
        self.frequency = (self.frequency & 0x00FF) | (u32::from(value) << 8);
    }

    /// Writes the low byte of the pulse width (registers 2, 9 and 16).
    pub(super) fn write_pulse_low(&mut self, value: u8) {
        self.pulse_width = (self.pulse_width & 0x0F00) | u32::from(value);
    }

    /// Writes the upper four bits of the pulse width (registers 3, 10 and
    /// 17); the register's upper four bits are not used.
    pub(super) fn write_pulse_high(&mut self, value: u8) {
        self.pulse_width = (self.pulse_width & 0x00FF) | (u32::from(value & 0x0F) << 8);
    }

    /// Writes the control register (registers 4, 11 and 18). The gate bit
    /// is the envelope's and is not kept here.
    pub(super) fn write_control(&mut self, value: u8) {
        if value & control::TEST != 0 {
            if self.control & control::TEST == 0 {
                self.test_cycles = 0;
            }
            // Data sheet, register 4: the test bit "resets and locks
            // Oscillator 1 at zero until the TEST bit is cleared".
            self.accumulator = 0;
        }
        self.control = value;
    }

    /// The accumulator's top bit, which the next voice's ring modulation
    /// reads.
    #[inline]
    pub(super) const fn msb(&self) -> bool {
        self.accumulator & ACCUMULATOR_MSB != 0
    }

    /// Whether the accumulator's top bit rose in the cycle just run.
    #[inline]
    pub(super) const fn msb_rose(&self) -> bool {
        self.msb_rose
    }

    /// One chip cycle of the accumulator and the noise register.
    #[inline]
    pub(super) fn advance(&mut self) {
        if self.control & control::TEST != 0 {
            self.msb_rose = false;
            if self.test_cycles < NOISE_RESET_CYCLES {
                self.test_cycles = self.test_cycles.wrapping_add(1);
                if self.test_cycles == NOISE_RESET_CYCLES {
                    self.set_noise(NOISE_RESET);
                }
            }
            return;
        }
        let before = self.accumulator;
        let after = before.wrapping_add(self.frequency) & ACCUMULATOR_MASK;
        self.accumulator = after;
        let rose = !before & after;
        self.msb_rose = rose & ACCUMULATOR_MSB != 0;
        if rose & NOISE_CLOCK != 0 {
            let feedback = ((self.noise >> NOISE_FEEDBACK.0) ^ (self.noise >> NOISE_FEEDBACK.1)) & 1;
            self.set_noise(((self.noise << 1) | feedback) & NOISE_MASK);
        }
    }

    /// Clears the accumulator if this oscillator is synchronised and the
    /// previous voice's top bit rose in the cycle just run.
    ///
    /// Source: Yannes interview ("Hard Sync was accomplished by clearing the
    /// accumulator of an Oscillator based on the accumulator MSB of the
    /// previous oscillator"). That it is the bit's rise, once in each of
    /// the other oscillator's periods as the data sheet's "synchronizes the
    /// fundamental frequency" needs, is an own choice between its two edges.
    #[inline]
    pub(super) fn synchronise(&mut self, source_rose: bool) {
        if source_rose && self.control & control::SYNC != 0 {
            self.accumulator = 0;
        }
    }

    fn set_noise(&mut self, register: u32) {
        self.noise = register;
        self.noise_waveform = noise_waveform(register);
    }

    /// The sawtooth: the accumulator's upper twelve bits (Yannes interview).
    #[inline]
    const fn sawtooth(&self) -> u16 {
        (self.accumulator >> SAWTOOTH_SHIFT) as u16
    }

    /// The triangle: the accumulator's top bit inverts the eleven below it,
    /// which are then shifted left one place (Yannes interview). With ring
    /// modulation the previous voice's top bit inverts them once more: the
    /// triangle multiplied by that oscillator's square wave, the "ring
    /// modulated combination" of the data sheet. The interview has that
    /// bit put in the place of the oscillator's own; exclusive-or with both
    /// is the reading taken here (own choice), since only that is a ring
    /// modulation of the triangle.
    #[inline]
    const fn triangle(&self, source_msb: bool) -> u16 {
        let invert = self.msb() != (source_msb && self.control & control::RING != 0);
        let ramp = (self.accumulator >> (SAWTOOTH_SHIFT - 1)) as u16 & WAVEFORM_MAX;
        if invert { !ramp & WAVEFORM_MAX & !1 } else { ramp & !1 }
    }

    /// The pulse: every bit set while the accumulator's upper twelve bits
    /// are at or above the pulse width.
    ///
    /// Source: data sheet, registers 2 and 3: "`PWout` = (`PWn`/40.95) %",
    /// the share of the period the pulse is high at the output, and a width
    /// of zero is a constant level. The waveform reaches the output
    /// inverted, a higher value a lower level ([`super::Voice::level`]), so
    /// the bits are set for the rest of the period: the share at or above
    /// the width. Which share is set matters only when the pulse is
    /// combined with another waveform, whose bits it masks; measured
    /// against reSIDfp on 17 November 2026, this way gives triangle and
    /// pulse at width $200 within 1 dB of it, and the other way 17 dB
    /// below (TODO-6427 fault 1). The test bit holds the pulse "at a DC
    /// level" (register 4); that the level is the set one whatever the
    /// width is the requirement's reading of it.
    #[inline]
    const fn pulse(&self) -> u16 {
        if self.control & control::TEST != 0 || (self.accumulator >> SAWTOOTH_SHIFT) >= self.pulse_width {
            WAVEFORM_MAX
        } else {
            0
        }
    }

    /// The waveform that reaches the converter in the cycle just run, 0 to
    /// [`WAVEFORM_MAX`]. `source_msb` is the previous voice's top bit.
    #[inline]
    pub(super) fn waveform(&mut self, source_msb: bool, model: ChipModel) -> u16 {
        let selected = self.control & control::WAVEFORMS;
        let value = match selected {
            0 => return self.floating(),
            control::TRIANGLE => self.triangle(source_msb),
            control::SAWTOOTH => self.sawtooth(),
            control::PULSE => self.pulse(),
            control::NOISE => self.noise_waveform,
            _ => self.combined(selected, source_msb, model),
        };
        self.held = value;
        self.held_cycles = 0;
        value
    }

    /// No waveform selected: nothing drives the converter's inputs, which
    /// keep their last value while it leaks away.
    fn floating(&mut self) -> u16 {
        self.held_cycles = self.held_cycles.wrapping_add(1);
        if self.held_cycles >= FLOATING_FALL_CYCLES {
            self.held_cycles = 0;
            self.held = self.held.saturating_sub(1);
        }
        self.held
    }

    /// Several waveforms selected at once: the selector's transistors short
    /// the selected waveforms' bits together, and a low bit wins (Yannes
    /// interview: "The combination was actually a logical `ANDing` of the
    /// bits of each waveform").
    fn combined(&mut self, selected: u8, source_msb: bool, model: ChipModel) -> u16 {
        let mut value = WAVEFORM_MAX;
        if selected & control::TRIANGLE != 0 {
            value &= self.triangle(source_msb);
        }
        if selected & control::SAWTOOTH != 0 {
            value &= self.sawtooth();
        }
        if selected & control::PULSE != 0 {
            value &= self.pulse();
        }
        if selected & control::NOISE != 0 {
            value &= self.noise_waveform;
            // Yannes interview: selecting another waveform with the noise
            // "could lock up the pseudo-random sequence generator by filling
            // it with zeroes". The lines the other waveform pulls low are
            // the register's own outputs, so their zeros are written into
            // the register's tapped bits (own model of that): the register
            // drains to zero, where its feedback keeps it until the test
            // bit resets it (data sheet, note after register 4).
            if value != self.noise_waveform {
                self.set_noise(self.noise & noise_keep_mask(value));
            }
            return value;
        }
        match model {
            ChipModel::Mos6581 => pulled_6581(value, selected),
            // Source: the 8580's combinations "come very close to matching
            // the original SID specification (which stated that they will
            // be combined as a binary AND)" (retroisle.com, "The SID
            // (Sound Interface Device) Chip").
            ChipModel::Mos8580 => value,
        }
    }

    /// The upper eight bits of the waveform of the cycle last run, as
    /// register $1B reads them for voice 3.
    pub(super) const fn read(&self) -> u8 {
        (self.held >> NOISE_WAVEFORM_SHIFT) as u8
    }
}

/// The waveform's top line.
const WAVEFORM_TOP: u16 = 1 << (WAVEFORM_BITS - 1);

/// How far a low line pulls its neighbours low on a 6581, for a selection
/// of waveforms without the noise: the lines above it that it reaches,
/// then the lines below it.
///
/// Source: own model, with its reach fitted. The model rests on one
/// public fact: on a 6581 the top bit of sawtooth with triangle never
/// rises (codebase64, "detecting sid type", the routine from
/// "Mathematica", which waits for bit 7 of register $1B with waveform $30
/// and finds it only on an 8580), where the plain AND has it set for a
/// quarter of the period with the bit below it low. So a line is also
/// pulled low by a low line near it: the weak drivers of the older chip
/// lose to their neighbours ("In reality, the final output isn't exactly
/// a binary AND of the selected waveforms, especially on the 6581",
/// retroisle.com). How far the pull reaches is fitted, for each
/// selection, to the output of reSIDfp's default 6581, measured 17
/// November 2026 as each selection's level against the triangle's, in
/// decibels, at pulse widths $800, $200 and $C00; the level this model
/// gives follows each:
///
/// - sawtooth and triangle: -19.7 (here -19.7);
/// - pulse and triangle: -5.7, -2.9 and -21.4 (-4.9, -2.1 and -23.1);
/// - pulse and sawtooth: -25.3, -11.3 and -25.1 (-26.0, -25.7 and -26.0:
///   no reach gives the louder middle one along with the other two);
/// - all three: -25.3, -24.9 and -25.3 (silent, -19.7 and silent).
///
/// A reach of one line upwards alone, the model before, gave -10.6 for
/// sawtooth and triangle and +0.4 for pulse and sawtooth at width $800
/// (TODO-6427 fault 3). No table of a chip's samples is used.
const fn pull_reach_6581(selected: u8) -> (u32, u32) {
    match selected & (control::TRIANGLE | control::SAWTOOTH | control::PULSE) {
        PULSE_TRIANGLE => (2, 1),
        PULSE_SAWTOOTH => (8, 0),
        _ => (1, 1),
    }
}
const PULSE_TRIANGLE: u8 = control::PULSE | control::TRIANGLE;
const PULSE_SAWTOOTH: u8 = control::PULSE | control::SAWTOOTH;
/// The furthest any 6581 pull reaches either way, which bounds the work
/// of a cycle; the test `the_6581s_pull_is_bounded` holds
/// [`pull_reach_6581`] to it.
#[cfg(test)]
const PULL_REACH_MAX: u32 = 8;

/// The AND of the selected waveforms as a 6581 puts it out: each line is
/// low if a line within the selection's reach of it is low.
#[inline]
const fn pulled_6581(and: u16, selected: u8) -> u16 {
    let (above, below) = pull_reach_6581(selected);
    let mut value = and;
    let mut step = 0;
    while step < above {
        // A line is pulled low by the low line below it; the bottom line
        // has none.
        value &= (value << 1) | 1;
        step = step.saturating_add(1);
    }
    let mut step = 0;
    while step < below {
        // And by the low line above it; the top line has none.
        value &= (value >> 1) | WAVEFORM_TOP;
        step = step.saturating_add(1);
    }
    value & WAVEFORM_MAX
}

/// The noise waveform for a register value: the tapped bits as the upper
/// eight of twelve.
fn noise_waveform(register: u32) -> u16 {
    let mut value = 0u16;
    for &tap in &NOISE_TAPS {
        value = (value << 1) | u16::from(register.wrapping_shr(tap) & 1 != 0);
    }
    value << NOISE_WAVEFORM_SHIFT
}

/// The register bits to keep when the noise's output lines carry
/// `waveform`: every bit but the taps whose line is low.
fn noise_keep_mask(waveform: u16) -> u32 {
    let mut mask = NOISE_MASK;
    // The first tap is the waveform's top bit.
    let mut line = 1u16 << (WAVEFORM_BITS - 1);
    for &tap in &NOISE_TAPS {
        if waveform & line == 0 {
            mask &= !1u32.wrapping_shl(tap);
        }
        line >>= 1;
    }
    mask
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test"
)]
mod tests {
    use super::{
        ChipModel, NOISE_RESET, NOISE_RESET_CYCLES, NOISE_RESET_WAVEFORM, Oscillator, PULL_REACH_MAX, WAVEFORM_MAX,
        control, noise_waveform, pull_reach_6581, pulled_6581,
    };
    use std::collections::BTreeSet;
    use std::vec::Vec;

    fn oscillator(frequency: u16, control: u8) -> Oscillator {
        let mut oscillator = Oscillator::new();
        oscillator.write_frequency_low(frequency as u8);
        oscillator.write_frequency_high((frequency >> 8) as u8);
        oscillator.write_control(control);
        oscillator
    }

    /// The waveform for `cycles` cycles, with no previous voice.
    fn run(oscillator: &mut Oscillator, cycles: usize, model: ChipModel) -> Vec<u16> {
        (0..cycles)
            .map(|_| {
                oscillator.advance();
                oscillator.waveform(false, model)
            })
            .collect()
    }

    #[test]
    fn the_reset_waveform_is_the_reset_registers() {
        assert_eq!(noise_waveform(NOISE_RESET), NOISE_RESET_WAVEFORM);
        assert_eq!(Oscillator::new().noise, NOISE_RESET);
    }

    #[test]
    fn the_sawtooth_is_the_accumulators_upper_twelve_bits() {
        // Frequency $1000 advances the upper twelve bits by one a cycle.
        let wave = run(&mut oscillator(0x1000, control::SAWTOOTH), 8192, ChipModel::Mos6581);
        for (cycle, &value) in wave.iter().enumerate() {
            assert_eq!(usize::from(value), (cycle + 1) % 4096);
        }
    }

    #[test]
    fn the_triangle_rises_and_falls_in_steps_of_two() {
        let wave = run(&mut oscillator(0x1000, control::TRIANGLE), 4096, ChipModel::Mos6581);
        for (cycle, &value) in wave.iter().enumerate() {
            let phase = (cycle + 1) % 4096;
            let expected = if phase < 2048 { phase * 2 } else { (4095 - phase) * 2 };
            assert_eq!(usize::from(value), expected, "cycle {cycle}");
        }
        assert_eq!(wave.iter().copied().max(), Some(WAVEFORM_MAX - 1));
    }

    #[test]
    fn the_pulse_is_set_for_the_share_of_the_period_past_the_width() {
        for width in [0u16, 1, 0x400, 0x800, 0xC00, 0xFFF] {
            let mut oscillator = oscillator(0x1000, control::PULSE);
            oscillator.write_pulse_low(width as u8);
            oscillator.write_pulse_high((width >> 8) as u8);
            let wave = run(&mut oscillator, 4096, ChipModel::Mos6581);
            assert!(wave.iter().all(|&value| value == 0 || value == WAVEFORM_MAX));
            // Set past the width, so low at the inverted output for the
            // width's share of the period.
            let set = wave.iter().filter(|&&value| value == WAVEFORM_MAX).count();
            assert_eq!(set, 4096 - usize::from(width), "width {width:#x}");
        }
    }

    #[test]
    fn the_test_bit_holds_the_oscillator_at_zero_and_the_pulse_high() {
        let mut oscillator = oscillator(0x4321, control::SAWTOOTH);
        run(&mut oscillator, 1000, ChipModel::Mos6581);
        oscillator.write_control(control::SAWTOOTH | control::TEST);
        assert!(run(&mut oscillator, 100, ChipModel::Mos6581).iter().all(|&value| value == 0));
        oscillator.write_control(control::PULSE | control::TEST);
        assert!(run(&mut oscillator, 100, ChipModel::Mos6581).iter().all(|&value| value == WAVEFORM_MAX));
        // Released, it starts from zero.
        oscillator.write_control(control::SAWTOOTH);
        assert_eq!(run(&mut oscillator, 2, ChipModel::Mos6581), [4, 8]);
    }

    #[test]
    fn sync_clears_the_accumulator_when_the_source_rises() {
        let mut source = oscillator(0x8000, control::SAWTOOTH);
        let mut synced = oscillator(0x1234, control::SAWTOOTH | control::SYNC);
        let mut free = oscillator(0x1234, control::SAWTOOTH);
        let mut resets = 0;
        for cycle in 1..=2048u32 {
            source.advance();
            synced.advance();
            free.advance();
            synced.synchronise(source.msb_rose());
            free.synchronise(source.msb_rose());
            // The source's top bit rises every 512 cycles, first at 256.
            if cycle % 512 == 256 {
                assert!(source.msb_rose());
                assert_eq!(synced.waveform(false, ChipModel::Mos6581), 0, "cycle {cycle}");
                resets += 1;
            } else {
                assert!(!source.msb_rose());
            }
        }
        assert_eq!(resets, 4);
        // The oscillator without the sync bit ran free.
        assert_eq!(usize::from(free.waveform(false, ChipModel::Mos6581)), (2048 * 0x1234) >> 12);
    }

    #[test]
    fn ring_modulation_inverts_the_triangle_by_the_sources_top_bit() {
        let mut plain = oscillator(0x1000, control::TRIANGLE);
        let mut ring = oscillator(0x1000, control::TRIANGLE | control::RING);
        for _ in 0..4096 {
            plain.advance();
            ring.advance();
            let value = plain.waveform(true, ChipModel::Mos6581);
            // Without the ring bit the source changes nothing.
            assert_eq!(value, plain.waveform(false, ChipModel::Mos6581));
            assert_eq!(ring.waveform(false, ChipModel::Mos6581), value);
            assert_eq!(ring.waveform(true, ChipModel::Mos6581), (WAVEFORM_MAX - 1) - value);
        }
    }

    #[test]
    fn noise_shifts_once_for_each_rise_of_accumulator_bit_19() {
        // Alstrup's Table 1: frequency $8000 changes the value every $20
        // cycles, and his listing of the first values read at $1B.
        let mut oscillator = oscillator(0x8000, control::NOISE);
        let wave = run(&mut oscillator, 32 * 40, ChipModel::Mos6581);
        let values: Vec<u8> = wave.chunks(32).map(|chunk| (chunk[16] >> 4) as u8).collect();
        for chunk in wave[15..].as_chunks::<32>().0 {
            assert!(chunk.iter().all(|&value| value == chunk[0]));
        }
        // The reset value and its first shift both read $FE.
        let listed = [
            0b1111_1110u8,
            0b1111_1100,
            0b1111_1100,
            0b1111_1100,
            0b1111_1000,
            0b1111_1000,
            0b1111_1000,
            0b1111_1000,
            0b1111_0000,
            0b1111_0000,
            0b1110_0000,
            0b1110_0000,
            0b1110_0000,
            0b1100_0000,
            0b1100_0000,
            0b1100_0000,
            0b1100_0000,
            0b1000_0001,
            0b1000_0001,
            0b0000_0011,
            0b0000_0011,
            0b0000_0011,
            0b0000_0110,
            0b0000_0110,
            0b0000_0100,
            0b0000_0100,
            0b0000_1100,
            0b0000_1000,
            0b0001_1000,
            0b0001_1000,
            0b0001_1000,
            0b0011_0000,
        ];
        assert_eq!(values[..listed.len()], listed);
    }

    #[test]
    fn noise_never_reaches_zero_and_does_not_repeat_early() {
        let mut oscillator = oscillator(0xFFFF, control::NOISE);
        let mut seen = BTreeSet::new();
        for _ in 0..200_000 * 16 {
            oscillator.advance();
            assert_ne!(oscillator.noise, 0);
            seen.insert(oscillator.noise);
        }
        // A shift every sixteen cycles and a little more: every state new.
        assert!(seen.len() >= 199_990, "{} states", seen.len());
    }

    #[test]
    fn noise_with_another_waveform_locks_up_until_the_test_bit_resets_it() {
        let mut oscillator = oscillator(0xFFFF, control::NOISE | control::SAWTOOTH);
        run(&mut oscillator, 20_000, ChipModel::Mos8580);
        assert_eq!(oscillator.noise, 0);
        oscillator.write_control(control::NOISE);
        assert!(run(&mut oscillator, 20_000, ChipModel::Mos8580).iter().all(|&value| value == 0));
        // A short test pulse does not reset it; a long one does.
        oscillator.write_control(control::NOISE | control::TEST);
        run(&mut oscillator, usize::from(NOISE_RESET_CYCLES) - 1, ChipModel::Mos8580);
        assert_eq!(oscillator.noise, 0);
        run(&mut oscillator, 1, ChipModel::Mos8580);
        assert_eq!(oscillator.noise, NOISE_RESET);
        oscillator.write_control(control::NOISE);
        assert!(run(&mut oscillator, 20_000, ChipModel::Mos8580).iter().any(|&value| value != 0));
    }

    #[test]
    fn combined_waveforms_are_the_and_and_the_6581_loses_its_top_bit() {
        for selected in [0x30u8, 0x50, 0x60, 0x70] {
            let mut combined = oscillator(0x1000, selected);
            combined.write_pulse_high(0x08);
            let mut parts: Vec<Oscillator> = [control::TRIANGLE, control::SAWTOOTH, control::PULSE]
                .iter()
                .filter(|&&bit| selected & bit != 0)
                .map(|&bit| {
                    let mut part = oscillator(0x1000, bit);
                    part.write_pulse_high(0x08);
                    part
                })
                .collect();
            let mut old = combined.clone();
            for _ in 0..4096 {
                combined.advance();
                old.advance();
                let mut and = WAVEFORM_MAX;
                for part in &mut parts {
                    part.advance();
                    and &= part.waveform(false, ChipModel::Mos8580);
                }
                assert_eq!(combined.waveform(false, ChipModel::Mos8580), and);
                let pulled = old.waveform(false, ChipModel::Mos6581);
                assert_eq!(pulled & !and, 0, "the 6581 only pulls bits low");
                if selected == 0x30 {
                    assert_eq!(pulled & 0x800, 0, "sawtooth with triangle never sets the top bit");
                }
            }
        }
    }

    #[test]
    fn the_6581s_pull_is_bounded() {
        for selected in 0..=255u8 {
            let (above, below) = pull_reach_6581(selected);
            assert!(above <= PULL_REACH_MAX && below <= PULL_REACH_MAX, "{selected:#04x}");
            // One low line reaches that many lines and no further.
            let lone = pulled_6581(WAVEFORM_MAX & !0x100, selected);
            let reached = (0x100u32 << (above + 1)) - (0x100u32 >> below);
            assert_eq!(u32::from(lone), u32::from(WAVEFORM_MAX) & !reached, "{selected:#04x}");
            assert_eq!(pulled_6581(WAVEFORM_MAX, selected), WAVEFORM_MAX);
            assert_eq!(pulled_6581(0, selected), 0);
        }
    }

    #[test]
    fn no_waveform_holds_the_last_value_and_lets_it_fall() {
        // Stopped at zero with a width of zero, the pulse is set.
        let mut oscillator = oscillator(0, control::PULSE);
        assert_eq!(run(&mut oscillator, 10, ChipModel::Mos6581)[9], WAVEFORM_MAX);
        oscillator.write_control(0);
        // Two scan lines later the value is still there.
        assert_eq!(run(&mut oscillator, 126, ChipModel::Mos6581)[125], WAVEFORM_MAX);
        // After two seconds it is nearly gone, and after three it is.
        let later = run(&mut oscillator, 2_000_000, ChipModel::Mos6581);
        assert!(later[1_999_999] < WAVEFORM_MAX / 8);
        assert!(later.windows(2).all(|pair| pair[1] <= pair[0]));
        assert_eq!(run(&mut oscillator, 1_000_000, ChipModel::Mos6581)[999_999], 0);
    }
}
