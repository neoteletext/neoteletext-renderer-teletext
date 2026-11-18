//! Tests of damage tracking and the damage repaint (animation.md ANIM-4,
//! REQ-6529).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use crate::render::*;

use crate::config::{CellMode, Config};
use crate::consts::{COLUMNS, PAGE_HEIGHT, PAGE_WIDTH, ROWS};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, CellAttributes, FlashRate, Grid, Size, Width};
use crate::render::test_grids::*;
use crate::types::CellRect;
use proptest::prelude::*;
use std::vec;
use std::vec::Vec;

fn resolved(unit: u8, reveal: bool) -> RenderOptions {
    RenderOptions::resolved(&Config::default(), reveal, FlashPhase::at_unit(unit))
}

fn palette(reveal: bool) -> RenderOptions {
    RenderOptions::from_config(&Config::default(), reveal)
}

fn rect(row: u8, column: u8, rows: u8, columns: u8) -> CellRect {
    CellRect { row, column, rows, columns }
}

fn whole(grid: &Grid, options: RenderOptions) -> Vec<u8> {
    let mut pixels = vec![0u8; PAGE_WIDTH * options.row_height() * ROWS];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
    render_screen(grid, ALL_ROWS, &GlyphSet::noto_teletext(), options, &mut buffer).unwrap();
    pixels
}

/// ANIM-4 (REQ-6529): the rectangle is the union, in whole cells, of the
/// cells that changed; nothing when nothing did; the whole page on first
/// use and when the page-wide state changes.
#[test]
fn the_damage_is_the_union_of_the_cells_that_changed() {
    let mut grid = Grid::EMPTY;
    let mut tracker = DamageTracker::new();
    let options = resolved(0, false);
    assert_eq!(tracker.damage(&grid, options, None), Some(CellRect::PAGE), "first use");
    assert_eq!(tracker.damage(&grid, options, None), None);
    grid.cells[3][7].glyph = text(b'X');
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(3, 7, 1, 1)));
    grid.cells[3][7].foreground = RED;
    grid.cells[10][2].background = BLUE;
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(3, 2, 8, 6)));
    assert_eq!(tracker.damage(&grid, options, None), None);
    grid.row_colours[20] = RED;
    assert_eq!(
        tracker.damage(&grid, options, None),
        Some(rect(20, 0, 1, COLUMNS as u8)),
        "a row colour is its whole row"
    );
    grid.screen_colour = BLUE;
    assert_eq!(tracker.damage(&grid, options, None), Some(CellRect::PAGE), "the screen colour");
    grid.mix = true;
    assert_eq!(tracker.damage(&grid, options, None), Some(CellRect::PAGE), "mix");
    assert_eq!(tracker.damage(&grid, RenderOptions { rounding: false, ..options }, None), Some(CellRect::PAGE));
    assert_eq!(tracker.damage(&grid, RenderOptions { rounding: false, ..options }, None), None);
    tracker.invalidate();
    assert_eq!(tracker.damage(&grid, options, None), Some(CellRect::PAGE), "invalidated");
}

/// ANIM-4: a changed double-height or double-width character changes the
/// cell that shows its other part.
#[test]
fn an_enlarged_character_damages_the_cells_it_reaches() {
    let mut grid = Grid::EMPTY;
    let mut tracker = DamageTracker::new();
    let options = resolved(0, false);
    let _ = tracker.damage(&grid, options, None);
    grid.cells[4][4] = Cell { glyph: text(b'A'), size: Size::DoubleTop, ..Cell::BLANK };
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(4, 4, 2, 1)));
    grid.cells[4][4] = Cell::BLANK;
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(4, 4, 2, 1)), "the character it was");
    grid.cells[8][8].attributes = CellAttributes::DEFAULT.with_width(Width::LeftHalf);
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(8, 8, 1, 2)));
    grid.cells[24][39] =
        Cell { size: Size::DoubleTop, attributes: CellAttributes::DEFAULT.with_width(Width::LeftHalf), ..Cell::BLANK };
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(24, 39, 1, 1)), "nothing is past the page's last cell");
}

/// ANIM-4: the cells whose flash phase changed are damaged, and no other;
/// a change of reveal damages the concealed cells. In the palette mode
/// flash and reveal are the palette's, and only a flashing concealed cell
/// is drawn again (REQ-RND-023).
#[test]
fn flash_and_reveal_damage_their_own_cells() {
    let mut grid = Grid::EMPTY;
    let flashing = Cell { glyph: text(b'F'), foreground: RED, background: BLUE, flash: true, ..Cell::BLANK };
    grid.cells[2][5] = Cell { attributes: CellAttributes::DEFAULT.with_flash_rate(FlashRate::Slow), ..flashing };
    grid.cells[6][9] = Cell { attributes: CellAttributes::DEFAULT.with_flash_rate(FlashRate::FastPhase1), ..flashing };
    grid.cells[9][1] = Cell { glyph: text(b'C'), conceal: true, ..Cell::BLANK };
    grid.cells[12][30] = Cell { conceal: true, ..flashing };
    // A flashing space shows nothing in either phase.
    grid.cells[15][15] = Cell { flash: true, ..Cell::BLANK };
    let mut tracker = DamageTracker::new();
    let _ = tracker.damage(&grid, resolved(2, false), None);
    assert_eq!(tracker.damage(&grid, resolved(3, false), None), None, "units 2 and 3 show alike");
    assert_eq!(
        tracker.damage(&grid, resolved(9, false), None),
        Some(rect(2, 5, 1, 1)),
        "slow flash goes off at unit 9"
    );
    assert_eq!(tracker.damage(&grid, resolved(0, false), None), Some(rect(2, 5, 5, 5)), "slow on, fast phase 1 off");
    assert_eq!(tracker.damage(&grid, resolved(0, true), None), Some(rect(9, 1, 4, 30)), "reveal: the concealed cells");
    let level_1 =
        RenderOptions { colour: ColourMode::Resolved(FlashPhase::at_unit(0).with_level_1(false)), ..resolved(0, true) };
    assert_eq!(tracker.damage(&grid, level_1, None), Some(rect(12, 30, 1, 1)), "the Level 1 flash");

    let mut tracker = DamageTracker::new();
    let _ = tracker.damage(&grid, palette(false), None);
    assert_eq!(tracker.damage(&grid, palette(true), None), Some(rect(12, 30, 1, 1)), "the palette mode");
    assert_eq!(tracker.damage(&grid, resolved(0, true), None), Some(CellRect::PAGE), "another colour mode");
}

/// ANIM-4: the cells any sprite covered before or covers now are in the
/// rectangle.
#[test]
fn the_cells_sprites_covered_and_cover_are_damaged() {
    let grid = Grid::EMPTY;
    let mut tracker = DamageTracker::new();
    let options = resolved(0, false);
    let _ = tracker.damage(&grid, options, None);
    assert_eq!(tracker.damage(&grid, options, Some(rect(5, 5, 2, 2))), Some(rect(5, 5, 2, 2)), "covers now");
    assert_eq!(
        tracker.damage(&grid, options, Some(rect(8, 9, 1, 1))),
        Some(rect(5, 5, 4, 5)),
        "covered before and covers now"
    );
    assert_eq!(tracker.damage(&grid, options, None), Some(rect(8, 9, 1, 1)), "covered before");
    assert_eq!(tracker.damage(&grid, options, None), None);
}

/// ANIM-4: the damage repaint writes the rectangle's cells and no other
/// pixel, in both pixel formats, and refuses a rectangle off the page.
#[test]
fn the_damage_repaint_writes_only_its_rectangle() {
    let mut grid = Grid::EMPTY;
    for (row, cells) in grid.cells.iter_mut().enumerate() {
        for (column, cell) in cells.iter_mut().enumerate() {
            *cell = Cell {
                glyph: text(b'A' + ((row + column) % 26) as u8),
                foreground: RED,
                background: BLUE,
                ..Cell::BLANK
            };
        }
    }
    let glyphs = GlyphSet::noto_teletext();
    for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
        let options = RenderOptions { mode, ..resolved(0, false) };
        let expected = whole(&grid, options);
        let area = CellRect { row: 3, column: 5, rows: 4, columns: 7 };
        let mut pixels = vec![0xEEu8; expected.len()];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
        render_rect(&grid, area, &glyphs, options, &mut buffer).unwrap();
        let dots = area.dots_of(options.row_height());
        for (index, (&drawn, &wanted)) in pixels.iter().zip(&expected).enumerate() {
            let inside = dots.contains((index % PAGE_WIDTH) as u16, (index / PAGE_WIDTH) as u16);
            assert_eq!(drawn, if inside { wanted } else { 0xEE }, "{mode:?}: pixel {index}");
        }
    }
    let options = palette(false);
    let mut packed = vec![0u8; PAGE_WIDTH / 2 * PAGE_HEIGHT];
    let mut all = IndexBuffer { format: IndexFormat::Bits4, data: &mut packed, stride: PAGE_WIDTH / 2 };
    render_screen(&grid, ALL_ROWS, &glyphs, options, &mut all).unwrap();
    let mut part = vec![0u8; packed.len()];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits4, data: &mut part, stride: PAGE_WIDTH / 2 };
    render_rect(&grid, CellRect::PAGE, &glyphs, options, &mut buffer).unwrap();
    assert!(part == packed, "the whole page as a rectangle, four bits a pixel");

    let mut short = vec![0u8; PAGE_WIDTH * 40];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut short, stride: PAGE_WIDTH };
    let options = resolved(0, false);
    assert_eq!(render_rect(&grid, CellRect::cell(1, 0), &glyphs, options, &mut buffer), Ok(()));
    assert_eq!(
        render_rect(&grid, CellRect::cell(2, 0), &glyphs, options, &mut buffer),
        Err(RenderError::BufferTooSmall)
    );
    let off = CellRect { row: 24, column: 39, rows: 2, columns: 1 };
    assert_eq!(render_rect(&grid, off, &glyphs, options, &mut buffer), Err(RenderError::OutOfRange));
    let off = CellRect { row: 0, column: 39, rows: 1, columns: 2 };
    assert_eq!(render_rect(&grid, off, &glyphs, options, &mut buffer), Err(RenderError::OutOfRange));
}

/// One random change to a grid: a cell's character, colours, flash,
/// conceal or size, or a row's colour.
fn change() -> impl Strategy<Value = (usize, usize, u8, u8)> {
    (0..ROWS, 0..COLUMNS, 0u8..8, any::<u8>())
}

fn apply(grid: &mut Grid, (row, column, kind, value): (usize, usize, u8, u8)) {
    let cell = &mut grid.cells[row][column];
    match kind {
        0 => cell.glyph = text(b' ' + value % 0x5F),
        1 => cell.glyph = g3(0x20 + value % 0x5F),
        2 => cell.foreground = value % 32,
        3 => cell.background = value % 32,
        4 => {
            cell.flash = value & 1 == 1;
            let rates = [
                FlashRate::Level1,
                FlashRate::Slow,
                FlashRate::FastPhase1,
                FlashRate::FastPhase2,
                FlashRate::FastPhase3,
            ];
            cell.attributes = cell.attributes.with_flash_rate(rates[usize::from(value >> 1) % rates.len()]);
        }
        5 => cell.conceal = value & 1 == 1,
        6 => cell.size = [Size::Normal, Size::DoubleTop, Size::DoubleBottom][usize::from(value % 3)],
        _ => grid.row_colours[row] = value % 32,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// ANIM-4 (REQ-6529): for random sequences of changes, the damaged
    /// rendering equals the whole rendering, so no dot outside the
    /// reported rectangle differs.
    #[test]
    fn damaged_rendering_equals_whole_rendering(
        steps in prop::collection::vec((prop::collection::vec(change(), 0..4), 0u8..12, any::<bool>(), any::<bool>()), 1..12)
    ) {
        let glyphs = GlyphSet::noto_teletext();
        let mut grid = Grid::EMPTY;
        let mut tracker = DamageTracker::new();
        let mut pixels = vec![0xEEu8; PAGE_WIDTH * PAGE_HEIGHT];
        for (changes, unit, level_1, reveal) in steps {
            for one in changes {
                apply(&mut grid, one);
            }
            let phase = FlashPhase::at_unit(unit).with_level_1(level_1);
            let options = RenderOptions { colour: ColourMode::Resolved(phase), ..resolved(0, reveal) };
            if let Some(damage) = tracker.damage(&grid, options, None) {
                let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
                render_rect(&grid, damage, &glyphs, options, &mut buffer).unwrap();
            }
            prop_assert!(pixels == whole(&grid, options));
        }
    }
}
