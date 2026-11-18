//! Flash (decisions E.12 and E.48, REQ-RND-040): the phase of the flash
//! cycle, and whether a flashing cell is shown. A phase is a function of
//! the presentation clock's tick alone (animation.md ANIM-1, REQ-6529), so
//! every page a caller shows flashes in step; the caller keeps the clock.

use crate::config::Config;
use crate::consts::{
    FLASH_FAST_HALF_UNITS, FLASH_FAST_OFF_UNITS, FLASH_SLOW_ON_UNITS, FLASH_UNIT_TICKS, FLASH_UNITS,
    PRESENTATION_TICKS_PER_SECOND,
};
use crate::level1::{Cell, FlashMode, FlashRate, Grid};

/// Which flash rates show their "on" phase at one moment (decision E.48,
/// REQ-RND-040). It is computed once per frame, from the flash unit, so the
/// cell raster only tests a bit (REQ-GEN-016).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FlashPhase {
    /// One bit for each [`FlashRate`], set while it is on.
    on: u8,
}

/// The bit of a flash rate in [`FlashPhase`].
const fn rate_bit(rate: FlashRate) -> u8 {
    match rate {
        FlashRate::Slow => 0b0_0001,
        FlashRate::FastPhase1 => 0b0_0010,
        FlashRate::FastPhase2 => 0b0_0100,
        FlashRate::FastPhase3 => 0b0_1000,
        FlashRate::Level1 => 0b1_0000,
    }
}

impl FlashPhase {
    /// Every rate on: flashing characters in their "on" phase, for still
    /// images.
    pub const STEADY: Self = Self {
        on: rate_bit(FlashRate::Level1)
            | rate_bit(FlashRate::Slow)
            | rate_bit(FlashRate::FastPhase1)
            | rate_bit(FlashRate::FastPhase2)
            | rate_bit(FlashRate::FastPhase3),
    };

    /// The phase of flash unit `unit`, 0 to 11 (taken modulo 12), with the
    /// Level 1 flash, which units do not time, in its "on" phase.
    #[must_use]
    pub const fn at_unit(unit: u8) -> Self {
        let rates =
            [FlashRate::Level1, FlashRate::Slow, FlashRate::FastPhase1, FlashRate::FastPhase2, FlashRate::FastPhase3];
        let mut on = 0;
        let mut index = 0;
        while index < rates.len() {
            #[allow(
                clippy::indexing_slicing,
                reason = "the loop condition keeps `index` below the array's constant length"
            )]
            let rate = rates[index];
            if flash_on(rate, unit) {
                on |= rate_bit(rate);
            }
            index = index.wrapping_add(1);
        }
        Self { on }
    }

    /// The same phase with the Level 1 flash shown or hidden.
    #[must_use]
    pub const fn with_level_1(self, shown: bool) -> Self {
        let bit = rate_bit(FlashRate::Level1);
        Self { on: if shown { self.on | bit } else { self.on & !bit } }
    }

    /// The phase at a tick of the caller's presentation clock (REQ-GEN-005,
    /// ANIM-1): the Level 1 flash follows the timing of `config` at every
    /// level (decision E.12), and the units time only the Additional Flash
    /// Functions.
    #[must_use]
    pub fn at_tick(tick: u32, config: &Config) -> Self {
        Self::at_unit(flash_unit(tick)).with_level_1(flash_visible(tick, config))
    }

    /// True when a cell flashing at one of `rates` is drawn differently in
    /// this phase and in `other`.
    #[must_use]
    pub const fn differs_for(self, other: Self, rates: FlashRates) -> bool {
        (self.on ^ other.on) & rates.0 != 0
    }

    /// True while a character of this rate shows its "on" phase.
    #[must_use]
    pub const fn is_on(self, rate: FlashRate) -> bool {
        self.on & rate_bit(rate) != 0
    }
}

/// The flash rates the cells of a grid use (ANIM-1): what a caller needs
/// to wake only for the ticks at which a page it shows changes phase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FlashRates(u8);

impl FlashRates {
    /// No flashing cell.
    pub const NONE: Self = Self(0);

    /// The rates of the flashing cells of `grid`.
    #[must_use]
    pub fn of(grid: &Grid) -> Self {
        let cells = grid.cells.iter().flatten().filter(|cell| cell.flash);
        Self(cells.fold(0, |rates, cell| rates | rate_bit(cell.attributes.flash_rate())))
    }

    /// True when no cell flashes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when a cell flashes at `rate`.
    #[must_use]
    pub const fn has(self, rate: FlashRate) -> bool {
        self.0 & rate_bit(rate) != 0
    }
}

/// The first tick after `tick` at which a cell flashing at one of `rates`
/// changes phase (ANIM-1: the decoder wakes for the next flash phase a
/// shown page uses, and not sixty times a second); `None` when no rate is
/// used, the configuration shows the rate steadily, or the tick counter
/// ends first. The search is bounded by one cycle of the Level 1 flash and
/// one second of the units, whichever is longer.
#[must_use]
pub fn next_flash_tick(tick: u32, config: &Config, rates: FlashRates) -> Option<u32> {
    if rates.is_empty() {
        return None;
    }
    let now = FlashPhase::at_tick(tick, config);
    let cycle = config.flash_shown_ticks.saturating_add(config.flash_hidden_ticks).max(PRESENTATION_TICKS_PER_SECOND);
    (1..=cycle)
        .map_while(|ahead| tick.checked_add(ahead))
        .find(|&later| FlashPhase::at_tick(later, config).differs_for(now, rates))
}

/// The flash unit, 0 to 11, of a tick of the presentation clock: the
/// twelfth of the one-second flash cycle it falls in, each unit
/// [`FLASH_UNIT_TICKS`] long (decision E.48, REQ-RND-040, ANIM-1).
#[must_use]
pub const fn flash_unit(tick: u32) -> u8 {
    (tick % PRESENTATION_TICKS_PER_SECOND / FLASH_UNIT_TICKS) as u8
}

/// True when a character of `rate` is in its "on" phase in flash unit
/// `unit`, 0 to [`FLASH_UNITS`] − 1, a larger value taken modulo
/// [`FLASH_UNITS`] (decision E.48, REQ-RND-040):
///
/// - the Level 1 flash is timed by [`flash_visible`], not by units, so it
///   counts as on;
/// - slow flash (1 hertz) is on in units 0 to 8 and off in 9 to 11;
/// - fast flash (2 hertz) with phase p is off in units 2(p − 1) and 2p − 1
///   of each half-second of six units, and on otherwise.
///
/// Invert phase flash swaps the phases; that is the cell raster's business.
#[must_use]
pub const fn flash_on(rate: FlashRate, unit: u8) -> bool {
    let unit = unit % FLASH_UNITS;
    let off_pair = match rate {
        FlashRate::Level1 => return true,
        FlashRate::Slow => return unit < FLASH_SLOW_ON_UNITS,
        FlashRate::FastPhase1 => 0,
        FlashRate::FastPhase2 => 1,
        FlashRate::FastPhase3 => 2,
    };
    (unit % FLASH_FAST_HALF_UNITS) / FLASH_FAST_OFF_UNITS != off_pair
}

/// True when the cell is in its flash "on" phase: the rate's phase,
/// swapped for inverted flash (decision E.48).
pub(super) fn flash_shown(cell: &Cell, flash: FlashPhase) -> bool {
    let attributes = cell.attributes;
    flash.is_on(attributes.flash_rate()) != (attributes.flash_mode() == FlashMode::InvertPhase)
}

/// The Level 1 flash at a tick of the presentation clock, at every level
/// (decision E.12, ANIM-1): true while shown. [`FlashPhase::at_tick`]
/// carries it into the resolved mode.
#[must_use]
pub fn flash_visible(tick: u32, config: &Config) -> bool {
    let period = config.flash_shown_ticks.saturating_add(config.flash_hidden_ticks);
    if period == 0 {
        return true;
    }
    tick.checked_rem(period).unwrap_or(0) < config.flash_shown_ticks
}

#[cfg(test)]
mod tests;
