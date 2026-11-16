//! A voice's envelope generator: an 8-bit up/down counter clocked through
//! a rate divider, and through a second divider that lengthens the steps of
//! decay and release as the level falls (Yannes interview, "The Envelope
//! Generator was simply an 8-bit up/down counter").

/// Chip cycles for one step of the envelope's rate divider, for each of the
/// sixteen values of an attack, decay or release nibble.
///
/// Source: Kevin Horton, "New SID ADSR Info!" (2 August 2008), which reads
/// the divider's table off the 6581's die: a 15-bit shift register with
/// feedback from its last two bits, compared with one of sixteen values and
/// reset to all ones on a match, one cycle later. The periods here are the
/// number of cycles from reset to match, plus that one; the test
/// `periods_are_the_die_table` derives them again from the die's compare
/// values. They agree with the data sheet's Table 2 ("Envelope Rates":
/// 2 ms to 8 s for a rise of 256 steps at 1 MHz) to within one cycle,
/// except the fastest, which is 9 cycles where 2 ms would give 8.
const RATE_PERIODS: [u16; 16] = [9, 32, 63, 95, 149, 220, 267, 313, 392, 977, 1954, 3126, 3907, 11720, 19532, 31251];

/// States of the rate divider. It is a 15-bit shift register that never
/// holds zero, so it has 2¹⁵ - 1 states; a divider whose compare value was
/// moved behind it goes all the way round them before it matches again,
/// which is the delay of about 32,768 cycles that players work around.
///
/// Source: Kevin Horton, "New SID ADSR Info!" (15-bit register, taps on
/// its last two bits); codebase64, "Classic hard-restart and about ADSR in
/// generally" ("The LFSR must then run a full cycle before the new
/// comparison value can be reached. Full cycle takes 32768 cycles").
const RATE_STATES: u16 = 32_767;

/// The level at which decay and release begin: the counter's top.
const PEAK: u8 = 255;

/// Where the falling envelope changes the number of rate steps it waits
/// before each fall, as (level, steps).
///
/// Source: Plogue R&D, "SID 6581R3 ADSR tables, up close." (15 March
/// 2010), a recording of register $1C of a 6581R4AR during release: "(93)
/// switches to 2 waits before a drop", 54 to 4, 26 to 8, 14 to 16 and 6 to
/// 30. The same article decodes the die's second table to the divisors 2,
/// 4, 8, 16 and 30, and US patent 4,677,890, describing its figure 10, has
/// a divider that "provides a divide by 1, divide by 30, divide by 16,
/// divide by 4 and divide by 2". One step at the peak is the linear start
/// they all imply.
const EXPONENTIAL_STEPS: [(u8, u8); 6] = [(PEAK, 1), (93, 2), (54, 4), (26, 8), (14, 16), (6, 30)];

/// Rate steps before each fall once the level has reached zero.
///
/// Source: own choice. No public measurement says what the second divider
/// holds at zero; one step makes a release that begins below level 6, right
/// after a gate from silence, as fast as the rate says.
const STEPS_AT_ZERO: u8 = 1;

/// Sustain levels are the sixteen values whose two nibbles are alike.
///
/// Source: data sheet, register 6 ("16 linear steps, with a SUSTAIN value
/// of 0 selecting zero amplitude and a SUSTAIN value of 15 ($F) selecting
/// the peak amplitude"): 255 / 15 = 17.
const SUSTAIN_STEP: u8 = 17;

/// What the counter is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// Rising to the peak at the attack rate.
    Attack,
    /// Falling to the sustain level at the decay rate, then holding it.
    DecaySustain,
    /// Falling to zero at the release rate.
    Release,
}

/// One envelope generator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Envelope {
    level: u8,
    phase: Phase,
    attack: u8,
    decay: u8,
    sustain: u8,
    release: u8,
    /// How far the rate divider is from its reset state, in cycles.
    rate_position: u16,
    /// The position at which the divider matches: the selected period less
    /// the cycle its reset takes.
    rate_match: u16,
    /// Rate steps counted towards the next fall.
    exponential_count: u8,
    /// Rate steps for each fall at the current level.
    exponential_steps: u8,
    /// The counter stops at zero until the next gate.
    held_at_zero: bool,
    gate: bool,
}

impl Envelope {
    /// A silent generator, as the chip's reset leaves it.
    pub(super) const fn new() -> Self {
        Self {
            level: 0,
            phase: Phase::Release,
            attack: 0,
            decay: 0,
            sustain: 0,
            release: 0,
            rate_position: 0,
            rate_match: period_match(0),
            exponential_count: 0,
            exponential_steps: STEPS_AT_ZERO,
            held_at_zero: true,
            gate: false,
        }
    }

    /// The counter's value, 0 to 255: what register $1C reads for voice 3.
    #[inline]
    pub(super) const fn level(&self) -> u8 {
        self.level
    }

    /// Writes the attack and decay nibbles (registers 5, 12 and 19).
    pub(super) fn write_attack_decay(&mut self, value: u8) {
        self.attack = value >> 4;
        self.decay = value & 0x0F;
        self.select_rate();
    }

    /// Writes the sustain and release nibbles (registers 6, 13 and 20).
    pub(super) fn write_sustain_release(&mut self, value: u8) {
        self.sustain = (value >> 4).wrapping_mul(SUSTAIN_STEP);
        self.release = value & 0x0F;
        self.select_rate();
    }

    /// Follows the gate bit of the control register: a rising gate starts
    /// the attack from whatever level was reached, a falling one the
    /// release (data sheet, note after register 6).
    pub(super) fn set_gate(&mut self, gate: bool) {
        if gate && !self.gate {
            self.phase = Phase::Attack;
            self.held_at_zero = false;
        } else if !gate && self.gate {
            self.phase = Phase::Release;
        }
        self.gate = gate;
        self.select_rate();
    }

    /// Selects the divider's compare value for the phase. The divider's
    /// position is left alone: that is the chip's delay when the new value
    /// is already behind it.
    fn select_rate(&mut self) {
        let nibble = match self.phase {
            Phase::Attack => self.attack,
            Phase::DecaySustain => self.decay,
            Phase::Release => self.release,
        };
        self.rate_match = period_match(nibble);
    }

    /// One chip cycle.
    #[inline]
    pub(super) fn clock(&mut self) {
        if self.rate_position != self.rate_match {
            let next = self.rate_position.wrapping_add(1);
            self.rate_position = if next >= RATE_STATES { 0 } else { next };
            return;
        }
        self.rate_position = 0;
        self.step();
    }

    /// One step of the rate divider.
    fn step(&mut self) {
        if self.phase == Phase::Attack {
            // The attack is linear: every rate step counts. The counter
            // wraps if an attack begins at the peak, as the chip's does
            // (codebase64, "A new kind of hard-restart": "an attack
            // triggered when env=$ff causes env to wrap back to $00").
            self.exponential_count = 0;
            self.set_level(self.level.wrapping_add(1));
            if self.level == PEAK {
                self.phase = Phase::DecaySustain;
                self.select_rate();
            }
            return;
        }
        self.exponential_count = self.exponential_count.wrapping_add(1);
        if self.exponential_count < self.exponential_steps {
            return;
        }
        self.exponential_count = 0;
        // Yannes interview: a comparator on the sustain value stops the
        // clock when the counter has come down to it. Equality is the
        // reading taken here (own choice): a sustain level raised above the
        // counter lets it fall on, and it never counts up.
        if self.held_at_zero || (self.phase == Phase::DecaySustain && self.level == self.sustain) {
            return;
        }
        self.set_level(self.level.wrapping_sub(1));
        if self.level == 0 {
            self.held_at_zero = true;
        }
    }

    /// Changes the level and, at the levels the second table names, the
    /// number of rate steps for each fall.
    fn set_level(&mut self, level: u8) {
        self.level = level;
        if level == 0 {
            self.exponential_steps = STEPS_AT_ZERO;
        }
        for &(at, steps) in &EXPONENTIAL_STEPS {
            if level == at {
                self.exponential_steps = steps;
            }
        }
    }
}

/// The divider position that matches for a rate nibble.
const fn period_match(nibble: u8) -> u16 {
    let period = match nibble & 0x0F {
        0 => RATE_PERIODS[0],
        1 => RATE_PERIODS[1],
        2 => RATE_PERIODS[2],
        3 => RATE_PERIODS[3],
        4 => RATE_PERIODS[4],
        5 => RATE_PERIODS[5],
        6 => RATE_PERIODS[6],
        7 => RATE_PERIODS[7],
        8 => RATE_PERIODS[8],
        9 => RATE_PERIODS[9],
        10 => RATE_PERIODS[10],
        11 => RATE_PERIODS[11],
        12 => RATE_PERIODS[12],
        13 => RATE_PERIODS[13],
        14 => RATE_PERIODS[14],
        _ => RATE_PERIODS[15],
    };
    period.saturating_sub(1)
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
    use super::{Envelope, PEAK, RATE_PERIODS, RATE_STATES};
    use std::vec::Vec;

    /// The compare values of the die's table, in the order of the rate
    /// nibble (Kevin Horton, "New SID ADSR Info!").
    const DIE_COMPARE: [u16; 16] = [
        0x7F00, 0x0006, 0x003C, 0x0330, 0x20C0, 0x6755, 0x3800, 0x500E, 0x1212, 0x0222, 0x1848, 0x59B8, 0x3840, 0x77E2,
        0x7625, 0x0A93,
    ];

    /// One shift of the die's 15-bit register: the exclusive-or of its last
    /// two bits enters at the first.
    fn shift(register: u16) -> u16 {
        ((register << 1) | (((register >> 14) ^ (register >> 13)) & 1)) & 0x7FFF
    }

    #[test]
    fn periods_are_the_die_table() {
        for (nibble, &compare) in DIE_COMPARE.iter().enumerate() {
            let mut register = 0x7FFF;
            let mut shifts = 0u16;
            while register != compare {
                register = shift(register);
                shifts += 1;
                assert!(shifts < RATE_STATES, "rate {nibble} never matches");
            }
            // The match takes one more cycle, to reset the register.
            assert_eq!(RATE_PERIODS[nibble], shifts + 1, "rate {nibble}");
        }
    }

    #[test]
    fn the_divider_has_every_state_but_zero() {
        let mut register = 0x7FFF;
        let mut states = 0u16;
        loop {
            register = shift(register);
            states += 1;
            if register == 0x7FFF {
                break;
            }
        }
        assert_eq!(states, RATE_STATES);
    }

    /// Cycles until the level first equals `target`.
    fn cycles_until(envelope: &mut Envelope, target: u8, limit: u32) -> u32 {
        let mut cycles = 0;
        while envelope.level() != target {
            envelope.clock();
            cycles += 1;
            assert!(cycles < limit, "level {target} not reached in {limit} cycles");
        }
        cycles
    }

    #[test]
    fn attack_takes_the_data_sheets_time() {
        // Data sheet Table 2, attack column, in milliseconds at 1 MHz.
        let milliseconds = [2u32, 8, 16, 24, 38, 56, 68, 80, 100, 250, 500, 800, 1000, 3000, 5000, 8000];
        for (nibble, &expected) in milliseconds.iter().enumerate() {
            let mut envelope = Envelope::new();
            envelope.write_attack_decay((nibble as u8) << 4);
            envelope.write_sustain_release(0xF0);
            envelope.set_gate(true);
            let cycles = cycles_until(&mut envelope, PEAK, 9_000_000);
            assert_eq!(cycles, 255 * u32::from(RATE_PERIODS[nibble]), "rate {nibble}");
            // Within 2 per cent of the table, except the fastest rate,
            // whose 9 cycles a step are 2.3 ms against the table's 2 ms.
            let nominal = expected * 1000;
            let tolerance = if nibble == 0 { nominal / 5 } else { nominal / 50 };
            assert!(cycles.abs_diff(nominal) <= tolerance, "rate {nibble}: {cycles} cycles for {expected} ms");
        }
    }

    #[test]
    fn decay_and_release_take_three_times_the_attack() {
        // Data sheet Table 2, decay/release column, in milliseconds.
        let milliseconds = [6u32, 24, 48, 72, 114, 168, 204, 240, 300, 750, 1500, 2400, 3000, 9000, 15000, 24000];
        for nibble in [0usize, 2, 5, 9, 12] {
            let mut envelope = Envelope::new();
            envelope.write_attack_decay(nibble as u8);
            envelope.write_sustain_release(nibble as u8);
            envelope.set_gate(true);
            cycles_until(&mut envelope, PEAK, 10_000);
            let decay = cycles_until(&mut envelope, 0, 30_000_000);
            let nominal = milliseconds[nibble] * 1000;
            // The piecewise curve is the table's time to within 6 per cent
            // (20 per cent at the fastest rate, as for the attack).
            let tolerance = if nibble == 0 { nominal / 5 } else { nominal * 6 / 100 };
            assert!(decay.abs_diff(nominal) <= tolerance, "rate {nibble}: {decay} cycles for {nominal}");

            // The same fall from the peak by release.
            let mut envelope = Envelope::new();
            envelope.write_sustain_release(0xF0 | nibble as u8);
            envelope.set_gate(true);
            cycles_until(&mut envelope, PEAK, 10_000);
            envelope.set_gate(false);
            let release = cycles_until(&mut envelope, 0, 30_000_000);
            assert!(release.abs_diff(decay) <= u32::from(RATE_PERIODS[nibble]), "rate {nibble}");
        }
    }

    #[test]
    fn the_fall_slows_at_the_measured_levels() {
        let mut envelope = Envelope::new();
        envelope.set_gate(true);
        cycles_until(&mut envelope, PEAK, 10_000);
        let mut waits = Vec::new();
        let mut cycles = 0u32;
        let mut level = envelope.level();
        while envelope.level() != 0 {
            envelope.clock();
            cycles += 1;
            if envelope.level() != level {
                waits.push((level, cycles / u32::from(RATE_PERIODS[0])));
                level = envelope.level();
                cycles = 0;
            }
        }
        for &(from, steps) in &waits[1..] {
            let expected = match from {
                94..=255 => 1,
                55..=93 => 2,
                27..=54 => 4,
                15..=26 => 8,
                7..=14 => 16,
                _ => 30,
            };
            assert_eq!(steps, expected, "leaving level {from}");
        }
    }

    #[test]
    fn decay_stops_at_the_sustain_level() {
        for nibble in 0u8..16 {
            let mut envelope = Envelope::new();
            envelope.write_sustain_release(nibble << 4);
            envelope.set_gate(true);
            for _ in 0..200_000 {
                envelope.clock();
            }
            assert_eq!(envelope.level(), nibble * 17);
        }
    }

    #[test]
    fn the_level_holds_at_zero_after_release() {
        let mut envelope = Envelope::new();
        envelope.write_sustain_release(0xF0);
        envelope.set_gate(true);
        cycles_until(&mut envelope, PEAK, 10_000);
        envelope.set_gate(false);
        for _ in 0..100_000 {
            envelope.clock();
        }
        assert_eq!(envelope.level(), 0);
    }

    #[test]
    fn a_shorter_period_written_behind_the_divider_waits_a_full_turn() {
        // The divider runs towards the slowest period, 31,251 cycles.
        let mut envelope = Envelope::new();
        envelope.write_attack_decay(0xF0);
        envelope.write_sustain_release(0xF0);
        envelope.set_gate(true);
        for _ in 0..1_000 {
            envelope.clock();
        }
        assert_eq!(envelope.level(), 0);
        // The fastest attack, 9 cycles a step, is now behind the divider:
        // the first step comes only after the divider has gone all the way
        // round, 32,767 states less the 1,000 passed, plus the period.
        envelope.write_attack_decay(0x00);
        let delay = cycles_until(&mut envelope, 1, 40_000);
        assert_eq!(delay, u32::from(RATE_STATES) - 1_000 + u32::from(RATE_PERIODS[0]));
        // After it, the steps come at the period.
        assert_eq!(cycles_until(&mut envelope, 2, 100), u32::from(RATE_PERIODS[0]));
    }

    #[test]
    fn a_longer_period_takes_effect_at_once() {
        let mut envelope = Envelope::new();
        envelope.write_sustain_release(0xF0);
        envelope.set_gate(true);
        for _ in 0..5 {
            envelope.clock();
        }
        envelope.write_attack_decay(0x20);
        assert_eq!(cycles_until(&mut envelope, 1, 100), u32::from(RATE_PERIODS[2]) - 5);
    }

    #[test]
    fn an_attack_from_the_peak_wraps_to_zero() {
        let mut envelope = Envelope::new();
        envelope.write_sustain_release(0xF0);
        envelope.set_gate(true);
        cycles_until(&mut envelope, PEAK, 10_000);
        envelope.set_gate(false);
        envelope.set_gate(true);
        assert!(cycles_until(&mut envelope, 0, 100) <= u32::from(RATE_PERIODS[0]));
        // It then rises again as an attack does.
        assert_eq!(cycles_until(&mut envelope, PEAK, 10_000), 255 * u32::from(RATE_PERIODS[0]));
    }
}
