//! Timing (decisions E.12 and E.19, REQ-NAV-007), the presentation clock's
//! tick (animation.md ANIM-1, REQ-6529) and the flash units of the
//! Additional Flash Functions (decision E.48, Table 29 mode 00111).

/// Microseconds per field at 50 fields per second. It times acquisition
/// and timeouts only: nothing shown is timed in fields (ANIM-1).
pub const FIELD_PERIOD_US: u64 = 20_000;

// ---------------------------------------------------------------------------
// The presentation clock (animation.md ANIM-1, REQ-6529): everything shown
// that depends on time is a function of a tick the caller gives; the core
// keeps no clock
// ---------------------------------------------------------------------------

/// Ticks of the presentation clock in a second (ANIM-1).
pub const PRESENTATION_TICKS_PER_SECOND: u32 = 60;
/// Microseconds in a second, for [`presentation_tick_due_us`].
const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
/// [`PRESENTATION_TICKS_PER_SECOND`] as wide as a time in microseconds.
const TICKS_PER_SECOND_WIDE: u64 = PRESENTATION_TICKS_PER_SECOND as u64;
/// Ticks a character with the Level 1 Flash attribute is shown (EN 300 706
/// clause 12.2, Table 26; decision E.12; ANIM-1): 950 ms, for the SAA5050's
/// 960 ms, because a tick is one sixtieth of a second.
pub const FLASH_SHOWN_TICKS: u32 = 57;
/// Ticks it is hidden (ANIM-1): 317 ms for the SAA5050's 320 ms. Shown to
/// hidden stays exactly three to one, the ratio decision E.12 measured.
pub const FLASH_HIDDEN_TICKS: u32 = 19;
const _: () = assert!(FLASH_SHOWN_TICKS == 3 * FLASH_HIDDEN_TICKS);

/// The microseconds after the clock's start at which tick `tick` is due:
/// `tick` × 50,000 ÷ 3, rounded down (ANIM-1), so ticks never drift against
/// real time.
#[must_use]
pub const fn presentation_tick_due_us(tick: u32) -> u64 {
    // A u32 times 1,000,000 fits a u64.
    (tick as u64).saturating_mul(MICROSECONDS_PER_SECOND) / TICKS_PER_SECOND_WIDE
}

/// The last tick due at `elapsed_us` microseconds after the clock's start,
/// stopping at the largest tick: the inverse of
/// [`presentation_tick_due_us`], for a caller that has a clock.
#[must_use]
pub const fn presentation_tick_at_us(elapsed_us: u64) -> u32 {
    // Tick n is due at n × 1,000,000 ÷ 60 rounded down, so it is due by
    // `elapsed_us` exactly when n × 1,000,000 < (elapsed_us + 1) × 60: the
    // answer is that quotient rounded up, less one. The whole seconds and
    // the rest are taken apart so that nothing overflows.
    let after = elapsed_us.saturating_add(1);
    let rest =
        (after % MICROSECONDS_PER_SECOND).saturating_mul(TICKS_PER_SECOND_WIDE).div_ceil(MICROSECONDS_PER_SECOND);
    let ticks =
        (after / MICROSECONDS_PER_SECOND).saturating_mul(TICKS_PER_SECOND_WIDE).saturating_add(rest).saturating_sub(1);
    if ticks > u32::MAX as u64 { u32::MAX } else { ticks as u32 }
}

/// Page completion timeout (decision E.19, Annex B.9).
pub const PAGE_COMPLETION_TIMEOUT_US: u64 = 100_000;
/// Momentary reveal timeout (REQ-NAV-007).
pub const MOMENTARY_REVEAL_US: u64 = 160_000;

// ---------------------------------------------------------------------------
// The Additional Flash Functions (decision E.48, Table 29 mode 00111); the
// Level 1 Flash code keeps the timing of decision E.12 at every level
// ---------------------------------------------------------------------------

/// One cycle of the Additional Flash Functions lasts one second (decision
/// E.48). The renderer times the cycle in ticks ([`FLASH_UNIT_TICKS`]);
/// this is the same second in microseconds, for a port's frame pacing.
pub const FLASH_CYCLE_US: u64 = MICROSECONDS_PER_SECOND;
/// The cycle is divided into twelve units of one twelfth of a second,
/// numbered 0 to 11: twelve divides both rates and the three fast phases
/// evenly (decision E.48).
pub const FLASH_UNITS: u8 = 12;
/// Ticks of the presentation clock in one flash unit (ANIM-1): exactly one
/// twelfth of a second.
pub const FLASH_UNIT_TICKS: u32 = 5;
const _: () = assert!(FLASH_UNIT_TICKS * FLASH_UNITS as u32 == PRESENTATION_TICKS_PER_SECOND);
/// Slow flash (1 hertz) is on during units 0 to 8 and off during units 9
/// to 11, the 3 to 1 ratio of decision E.12.
pub const FLASH_SLOW_ON_UNITS: u8 = 9;
/// Fast flash (2 hertz) treats each half-second of six units alike.
pub const FLASH_FAST_HALF_UNITS: u8 = FLASH_UNITS / 2;
/// The fast flash phases, 1 to 3.
pub const FLASH_PHASES: u8 = 3;
/// A fast flash character with phase p is off during the two units
/// 2(p − 1) and 2p − 1 of each half-second.
pub const FLASH_FAST_OFF_UNITS: u8 = 2;
