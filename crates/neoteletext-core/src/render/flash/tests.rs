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
    // ANIM-1: 57 ticks shown and 19 hidden, a cycle of 76 ticks.
    assert!(flash_visible(0, &config));
    assert!(flash_visible(56, &config));
    assert!(!flash_visible(57, &config));
    assert!(!flash_visible(75, &config));
    assert!(flash_visible(76, &config));
    // REQ-GEN-006: a configuration with no flash period has nothing to
    // divide by, and shows flashing characters steadily.
    let steady = Config { flash_shown_ticks: 0, flash_hidden_ticks: 0, ..Config::default() };
    assert!([0, 1, 57, u32::MAX].iter().all(|&tick| flash_visible(tick, &steady)));
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

/// REQ-RND-040, ANIM-1 (REQ-6529): the caller's tick to the flash unit,
/// five ticks a unit, computed once per frame.
#[test]
fn flash_unit_from_the_tick() {
    assert_eq!((flash_unit(0), flash_unit(4), flash_unit(5)), (0, 0, 1));
    assert_eq!((flash_unit(59), flash_unit(60), flash_unit(90)), (11, 0, 6));
    assert_eq!(flash_unit(u32::MAX), flash_unit(u32::MAX % 60));
    assert_eq!(FlashPhase::at_tick(15, &Config::default()), FlashPhase::at_unit(3));
    for tick in 0..120 {
        assert_eq!(u32::from(flash_unit(tick)), tick % 60 / 5, "tick {tick}");
    }
}

/// ANIM-1 (REQ-6529): every flash phase by tick. Each rate of the
/// Additional Flash Functions is on in exactly the ticks of its "on"
/// units, and the Level 1 flash in ticks 0 to 56 of each 76.
#[test]
fn every_flash_phase_by_tick() {
    let config = Config::default();
    // 1,140 ticks is the least common multiple of the two cycles, 76 and 60.
    for tick in 0..2 * 1_140 {
        let phase = FlashPhase::at_tick(tick, &config);
        assert_eq!(phase.is_on(FlashRate::Level1), tick % 76 < 57, "Level 1, tick {tick}");
        let unit = (tick % 60 / 5) as u8;
        assert_eq!(phase.is_on(FlashRate::Slow), unit <= 8, "slow, tick {tick}");
        for (rate, off) in OFF_UNITS {
            assert_eq!(phase.is_on(rate), !off.contains(&unit), "{rate:?}, tick {tick}");
        }
        assert_eq!(phase, FlashPhase::at_tick(tick + 1_140, &config), "the phase is a function of the tick alone");
    }
}

/// ANIM-1 (REQ-6529): the next tick at which a page's flashing cells
/// change, so that a decoder wakes twice a cycle for a slow flash and not
/// sixty times a second.
#[test]
fn the_next_flash_tick_follows_the_rates_a_grid_uses() {
    let config = Config::default();
    let mut grid = Grid::EMPTY;
    assert!(FlashRates::of(&grid).is_empty());
    assert_eq!(next_flash_tick(0, &config, FlashRates::of(&grid)), None, "nothing flashes");
    grid.cells[1][0] = Cell { glyph: text(b'X'), flash: true, ..Cell::BLANK };
    let level_1 = FlashRates::of(&grid);
    assert!(level_1.has(FlashRate::Level1) && !level_1.has(FlashRate::Slow));
    let wakes: Vec<u32> =
        core::iter::successors(Some(0), |&tick| next_flash_tick(tick, &config, level_1)).take(5).collect();
    assert_eq!(wakes, vec![0, 57, 76, 133, 152], "twice in each cycle of 76 ticks");
    grid.cells[1][0].attributes = CellAttributes::DEFAULT.with_flash_rate(FlashRate::Slow);
    let slow = FlashRates::of(&grid);
    let wakes: Vec<u32> =
        core::iter::successors(Some(0), |&tick| next_flash_tick(tick, &config, slow)).take(5).collect();
    assert_eq!(wakes, vec![0, 45, 60, 105, 120], "units 9 and 0 of each second");
    let steady = Config { flash_shown_ticks: 0, flash_hidden_ticks: 0, ..Config::default() };
    assert_eq!(next_flash_tick(0, &steady, level_1), None, "a steady Level 1 flash never changes");
    assert_eq!(next_flash_tick(u32::MAX, &config, level_1), None, "the tick counter has ended");
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

/// Decisions E.12 and E.48, ANIM-1: the Level 1 Flash code keeps the
/// SAA5050's timing at every level, in whole ticks (57 shown, 19 hidden),
/// and the phase changes for it at those ticks, not with the units.
#[test]
fn the_level_1_flash_follows_the_saa5050_timing_at_every_level() {
    let config = Config::default();
    for (tick, shown) in [(0, true), (48, true), (56, true), (57, false), (75, false), (76, true)] {
        let phase = FlashPhase::at_tick(tick, &config);
        assert_eq!(phase.is_on(FlashRate::Level1), shown, "tick {tick}");
    }
    assert!(!FlashPhase::at_tick(48, &config).is_on(FlashRate::Slow), "unit 9 still times the slow rate");
    let mut grid = Grid::EMPTY;
    grid.cells[3][0] = Cell { glyph: text(b'X'), foreground: RED, background: BLUE, flash: true, ..Cell::BLANK };
    let at = |tick| RenderOptions {
        mode: CellMode::Saa5050,
        rounding: true,
        reveal: false,
        colour: ColourMode::Resolved(FlashPhase::at_tick(tick, &config)),
        underline: true,
    };
    let mut tracker = ChangeTracker::new();
    tracker.changed_rows(&grid, at(0));
    assert_eq!(tracker.changed_rows(&grid, at(48)), 0, "unit 9 is not an edge of this flash");
    assert_eq!(tracker.changed_rows(&grid, at(57)), 1 << 3, "hidden");
    assert_eq!(tracker.changed_rows(&grid, at(76)), 1 << 3, "shown");
}
