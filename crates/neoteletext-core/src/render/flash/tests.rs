//! Tests of the flash timing: the Level 1 flash, the rates and phases of
//! decision E.48 and the flash unit from the time.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use crate::render::*;

use crate::config::{CellMode, Config};
use crate::consts::FLASH_UNITS;
use crate::level1::{Cell, CellAttributes, FlashRate, Grid};
use crate::render::test_grids::*;
use crate::types::Instant;
use std::vec;
use std::vec::Vec;

const WHITE: u8 = 7;

/// The off units of each rate, as decision E.48 lists them.
const OFF_UNITS: [(FlashRate, [u8; 4]); 3] = [
    (FlashRate::FastPhase1, [0, 1, 6, 7]),
    (FlashRate::FastPhase2, [2, 3, 8, 9]),
    (FlashRate::FastPhase3, [4, 5, 10, 11]),
];

#[test]
fn flash_timing_is_three_to_one() {
    let config = Config::default();
    assert!(flash_visible(Instant(0), &config));
    assert!(flash_visible(Instant(959_999), &config));
    assert!(!flash_visible(Instant(960_000), &config));
    assert!(flash_visible(Instant(1_280_000), &config));
    // REQ-GEN-006: a configuration with no flash period has nothing to
    // divide by, and shows flashing characters steadily.
    let steady = Config { flash_shown_us: 0, flash_hidden_us: 0, ..Config::default() };
    assert!([0, 1, 960_000, u64::MAX].iter().all(|&time| flash_visible(Instant(time), &steady)));
}

/// REQ-RND-040, decision E.48: every rate and phase in every unit.
#[test]
fn flash_rates_follow_decision_e48_in_every_unit() {
    for unit in 0..FLASH_UNITS {
        assert_eq!(flash_on(FlashRate::Slow, unit), unit <= 8, "slow, unit {unit}");
        for (rate, off) in OFF_UNITS {
            assert_eq!(flash_on(rate, unit), !off.contains(&unit), "{rate:?}, unit {unit}");
        }
        let phase = FlashPhase::at_unit(unit);
        for rate in [FlashRate::Slow, FlashRate::FastPhase1, FlashRate::FastPhase2, FlashRate::FastPhase3] {
            assert_eq!(phase.is_on(rate), flash_on(rate, unit));
            assert!(FlashPhase::STEADY.is_on(rate));
        }
    }
    // Units wrap modulo 12.
    assert_eq!(FlashPhase::at_unit(12), FlashPhase::at_unit(0));
}

/// REQ-RND-040: the caller's time to the flash unit, computed once per
/// frame.
#[test]
fn flash_unit_from_time() {
    let unit = |us| flash_unit(Instant(us));
    assert_eq!((unit(0), unit(83_333), unit(83_334)), (0, 0, 1));
    assert_eq!((unit(999_999), unit(1_000_000), unit(1_500_000)), (11, 0, 6));
    assert_eq!(unit(u64::MAX), unit(u64::MAX % 1_000_000));
    assert_eq!(FlashPhase::at(Instant(250_000), &Config::default()), FlashPhase::at_unit(3));
}

/// REQ-RND-040, REQ-L25-027: an incremental run of phases 1, 2, 3, 1, ...
/// makes the dark cell step rightwards every two units; a decremental run
/// (1, 3, 2, ...) steps leftwards.
#[test]
fn incremental_and_decremental_runs() {
    let phases = [FlashRate::FastPhase1, FlashRate::FastPhase2, FlashRate::FastPhase3];
    let mut grid = Grid::EMPTY;
    for column in 0..6 {
        let up = phases[column % 3];
        let down = phases[(3 - column % 3) % 3];
        for (row, rate) in [(1, up), (2, down)] {
            let cell = &mut grid.cells[row][column];
            *cell = Cell { glyph: g3(0x3F), foreground: WHITE, background: BLUE, flash: true, ..Cell::BLANK };
            cell.attributes = CellAttributes::DEFAULT.with_flash_rate(rate);
        }
    }
    let dark = |grid: &Grid, row: usize, unit: u8| -> Vec<usize> {
        (0..6)
            .filter(|&column| line_of(grid, &grid.cells[row][column], row, column, 0, large(unit)).foreground == BLUE)
            .collect()
    };
    for (pair, expected) in [(0u8, 0usize), (1, 1), (2, 2)] {
        for unit in [2 * pair, 2 * pair + 1, 2 * pair + 6, 2 * pair + 7] {
            assert_eq!(dark(&grid, 1, unit), vec![expected, expected + 3], "incremental, unit {unit}");
            let leftwards = (3 - expected) % 3;
            assert_eq!(dark(&grid, 2, unit), vec![leftwards, leftwards + 3], "decremental, unit {unit}");
        }
    }
}

/// Decisions E.12 and E.48: the Level 1 Flash code keeps the SAA5050's
/// timing at every level, 960 ms shown and 320 ms hidden, and the phase
/// changes for it at those moments, not with the units.
#[test]
fn the_level_1_flash_follows_the_saa5050_timing_at_every_level() {
    let config = Config::default();
    for (us, shown) in
        [(0, true), (800_000, true), (959_999, true), (960_000, false), (1_279_999, false), (1_280_000, true)]
    {
        let phase = FlashPhase::at(Instant(us), &config);
        assert_eq!(phase.is_on(FlashRate::Level1), shown, "{us} microseconds");
    }
    assert!(!FlashPhase::at(Instant(800_000), &config).is_on(FlashRate::Slow), "unit 9 still times the slow rate");
    let mut grid = Grid::EMPTY;
    grid.cells[3][0] = Cell { glyph: text(b'X'), foreground: RED, background: BLUE, flash: true, ..Cell::BLANK };
    let at = |us| RenderOptions {
        mode: CellMode::Saa5050,
        rounding: true,
        reveal: false,
        colour: ColourMode::Resolved(FlashPhase::at(Instant(us), &config)),
        underline: true,
    };
    let mut tracker = ChangeTracker::new();
    tracker.changed_rows(&grid, at(0));
    assert_eq!(tracker.changed_rows(&grid, at(800_000)), 0, "750 ms is not an edge of this flash");
    assert_eq!(tracker.changed_rows(&grid, at(960_000)), 1 << 3, "hidden");
    assert_eq!(tracker.changed_rows(&grid, at(1_280_000)), 1 << 3, "shown");
}
