//! Tests of change tracking: the rows a change of cells, reveal or flash
//! phase redraws.
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

use crate::config::CellMode;
use crate::consts::FLASH_UNITS;
use crate::level1::{Cell, CellAttributes, FlashMode, FlashRate, Glyph, Grid};
use crate::render::test_grids::*;

#[test]
fn change_tracking_reports_only_changed_rows() {
    let mut tracker = ChangeTracker::new();
    let mut grid = Grid::EMPTY;
    assert_eq!(tracker.changed_rows(&grid, default_options()), ALL_ROWS);
    assert_eq!(tracker.changed_rows(&grid, default_options()), 0);
    grid.cells[5][0].foreground = 3;
    assert_eq!(tracker.changed_rows(&grid, default_options()), 1 << 5);
    grid.cells[7][0].flash = true;
    grid.cells[7][0].conceal = true;
    tracker.changed_rows(&grid, default_options());
    let revealed = RenderOptions { reveal: true, ..default_options() };
    assert_eq!(tracker.changed_rows(&grid, revealed), 1 << 7, "reveal redraws flashing concealed rows only");
}

/// REQ-RND-019: a new tracker, however made, reports every row; after an
/// invalidation every row is reported once more.
#[test]
fn change_tracking_starts_and_restarts_with_every_row() {
    assert_eq!(ChangeTracker::default(), ChangeTracker::new());
    let grid = Grid::EMPTY;
    let mut tracker = ChangeTracker::default();
    assert_eq!(tracker.changed_rows(&grid, default_options()), ALL_ROWS);
    assert_eq!(tracker.changed_rows(&grid, default_options()), 0);
    tracker.invalidate();
    assert_eq!(tracker.changed_rows(&grid, default_options()), ALL_ROWS);
    assert_eq!(tracker.changed_rows(&grid, default_options()), 0);
}

/// REQ-RND-032: a flash phase change reports a row only for a flashing
/// cell that can show dots: a mosaic with at least one block set, and any
/// G3 character, whose shapes are drawn whatever the code; a mosaic with
/// no block set looks the same in both phases.
#[test]
fn flash_changes_follow_what_a_cell_can_show() {
    let mut grid = Grid::EMPTY;
    let attributes = CellAttributes::DEFAULT.with_flash_rate(FlashRate::Slow);
    let flashing = Cell { foreground: RED, background: BLUE, flash: true, attributes, ..Cell::BLANK };
    grid.cells[3][0] = Cell { glyph: Glyph::Mosaic { pattern: 0x15, separated: false }, ..flashing };
    grid.cells[5][0] = Cell { glyph: Glyph::Mosaic { pattern: 0, separated: true }, ..flashing };
    grid.cells[7][0] = Cell { glyph: g3(0x20), ..flashing };
    let mut tracker = ChangeTracker::new();
    tracker.changed_rows(&grid, large(8));
    // Slow flash is on in unit 8 and off in unit 9 (decision E.48).
    assert!(flash_on(FlashRate::Slow, 8) && !flash_on(FlashRate::Slow, 9));
    assert_eq!(tracker.changed_rows(&grid, large(9)), 1 << 3 | 1 << 7);
}

/// REQ-RND-032 (REQ-TST-018 for the counting): moving from each unit to
/// the next reports exactly the rows whose flashing cells change; a reveal
/// change reports the rows with concealed cells; a changed cell or a
/// rebuilt row reports only its own row.
#[test]
fn change_tracking_in_the_resolved_mode() {
    let mut grid = Grid::EMPTY;
    let rates = [
        (2, FlashRate::Slow, FlashMode::Normal),
        (4, FlashRate::FastPhase1, FlashMode::Normal),
        (6, FlashRate::FastPhase2, FlashMode::InvertPhase),
        (8, FlashRate::FastPhase3, FlashMode::AdjacentClut),
    ];
    for (row, rate, mode) in rates {
        grid.cells[row][10] = Cell {
            glyph: text(b'X'),
            foreground: RED,
            background: BLUE,
            flash: true,
            attributes: CellAttributes::DEFAULT.with_flash_rate(rate).with_flash_mode(mode),
            ..Cell::BLANK
        };
    }
    // Flashing cells that look the same in both phases: a space, and red
    // on red.
    grid.cells[12][0] = Cell { flash: true, ..Cell::BLANK };
    grid.cells[13][0] = Cell { glyph: text(b'X'), foreground: RED, background: RED, flash: true, ..Cell::BLANK };
    // A flashing concealed cell changes nothing until revealed.
    let attributes = CellAttributes::DEFAULT.with_flash_rate(FlashRate::Slow);
    grid.cells[14][0] =
        Cell { glyph: text(b'X'), foreground: RED, flash: true, conceal: true, attributes, ..Cell::BLANK };
    grid.cells[16][3] = Cell { glyph: text(b'X'), conceal: true, ..Cell::BLANK };
    let mut tracker = ChangeTracker::new();
    assert_eq!(tracker.changed_rows(&grid, large(11)), ALL_ROWS);
    for unit in 0..FLASH_UNITS {
        let previous = (unit + FLASH_UNITS - 1) % FLASH_UNITS;
        let expected = rates.iter().fold(0u32, |bits, &(row, rate, _)| {
            if flash_on(rate, unit) == flash_on(rate, previous) { bits } else { bits | 1 << row }
        });
        assert_eq!(tracker.changed_rows(&grid, large(unit)), expected, "unit {previous} to {unit}");
    }
    assert_eq!(tracker.changed_rows(&grid, large(11)), 0, "same unit, nothing");
    tracker.changed_rows(&grid, large(0));
    // Reveal: the rows with concealed cells.
    let revealed = options_with(CellMode::Saa5050, 0, true);
    assert_eq!(tracker.changed_rows(&grid, revealed), 1 << 14 | 1 << 16);
    // A flashing concealed cell flashes once revealed.
    assert_eq!(tracker.changed_rows(&grid, options_with(CellMode::Saa5050, 9, true)) & 1 << 14, 1 << 14);
    tracker.changed_rows(&grid, large(0));
    // A changed cell changes only its row.
    grid.cells[20][5].glyph = text(b'D');
    assert_eq!(tracker.changed_rows(&grid, large(0)), 1 << 20, "the cell changed");
    // A row colour belongs to its row.
    grid.row_colours[11] = 5;
    assert_eq!(tracker.changed_rows(&grid, large(0)), 1 << 11);
}
