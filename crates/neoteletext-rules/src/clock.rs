//! The presentation clock's arithmetic (RULE-4, ANIM-1): sixty ticks a
//! second that never drift against real time, and what a late receiver
//! does. The crate has no clock of its own: the receiver says how long
//! ago the clock started.

use crate::limits::{MAX_CATCH_UP_TICKS, TICK_SPAN_MICROSECONDS, TICKS_PER_TICK_SPAN};

/// Which ticks of a presentation clock are due.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Clock {
    /// The clock tick of the next evaluation, counted from the clock's
    /// first start: the ticks run and the ticks a late wake passed over.
    position: u64,
}

impl Clock {
    /// A clock whose tick 0 is due at its start.
    #[must_use]
    pub const fn new() -> Self {
        Self { position: 0 }
    }

    /// The microseconds after the clock's start at which its tick `tick`
    /// is due (RULE-4): `tick × 50,000 ÷ 3`, rounded down.
    #[must_use]
    pub const fn due_at(tick: u64) -> u64 {
        tick.saturating_mul(TICK_SPAN_MICROSECONDS) / TICKS_PER_TICK_SPAN
    }

    /// The microseconds after the clock's start at which the next tick is
    /// due: how long a receiver with nothing else to do may sleep.
    #[must_use]
    pub const fn next_due(&self) -> u64 {
        Self::due_at(self.position)
    }

    /// How many ticks to evaluate now, `elapsed` microseconds after the
    /// clock's start: each tick that is due, in order, at most
    /// `MAX_CATCH_UP_TICKS` in one wake. When more are due the clock's
    /// start is moved later by those not run, so that page time slows and
    /// no page tick is ever skipped (RULE-4).
    pub fn wake(&mut self, elapsed: u64) -> u32 {
        // The last tick due: the greatest `n` with `due_at(n) <= elapsed`.
        let scaled = elapsed.saturating_add(1).saturating_mul(TICKS_PER_TICK_SPAN).saturating_sub(1);
        let last = scaled / TICK_SPAN_MICROSECONDS;
        let Some(behind) = last.checked_sub(self.position) else { return 0 };
        let due = behind.saturating_add(1);
        // Passing over the ticks not run is moving the start later by them.
        self.position = self.position.saturating_add(due);
        u32::try_from(due).unwrap_or(u32::MAX).min(MAX_CATCH_UP_TICKS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RULE-4 (REQ-6526): 3,600 ticks are due in exactly 60 seconds, and
    /// the next at 60 seconds.
    #[test]
    fn sixty_seconds_hold_3600_ticks() {
        let mut clock = Clock::new();
        let mut ticks = 0_u32;
        for elapsed in (0..60_000_000_u64).step_by(1000) {
            ticks += clock.wake(elapsed);
        }
        assert_eq!(ticks, 3600);
        assert_eq!(clock.next_due(), 60_000_000);
        assert_eq!(clock.wake(59_999_999), 0);
        assert_eq!(clock.wake(60_000_000), 1);
        // The ticks of a second: 0, 16,666, 33,333, 50,000 microseconds.
        assert_eq!([0, 1, 2, 3].map(Clock::due_at), [0, 16_666, 33_333, 50_000]);
    }

    /// RULE-4 (REQ-6526): seven ticks overdue evaluate six, and the start
    /// moves later by the one not run, so the next tick is one period on.
    #[test]
    fn a_late_wake_runs_six_and_moves_the_start() {
        let mut clock = Clock::new();
        assert_eq!(clock.wake(0), 1);
        // Ticks 1 to 7 are due at 116,666 microseconds.
        assert_eq!(clock.wake(Clock::due_at(7)), MAX_CATCH_UP_TICKS);
        assert_eq!(clock.wake(Clock::due_at(7)), 0);
        assert_eq!(clock.next_due(), Clock::due_at(8));
        assert_eq!(clock.wake(Clock::due_at(8)), 1);
    }
}
