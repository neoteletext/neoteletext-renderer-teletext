//! Tests of the colour indices the renderer gives each pixel: the palette
//! mode's flash, conceal and reveal ranges, the resolved mode's Colour Map
//! indices, the display planes of Table C.1, the border and the receiver's
//! own colours.
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
use crate::consts::{
    ADJACENT_CLUT_FLIP, COLOUR_BLUE_BIT, COLOUR_RED_BIT, COLOUR_TRANSPARENT, COLUMNS, PAGE_WIDTH, PALETTE_CONCEAL_BASE,
    PALETTE_FLASH_BASE, PALETTE_SIZE, PALETTE_TRANSPARENT, RECEIVER_COLUMNS,
};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, CellAttributes, FlashMode, FlashRate, Grid};
use crate::render::test_grids::*;
use std::vec;
use std::vec::Vec;

/// The index of the first pixel of `column` in the first line of `row`,
/// drawn in the resolved mode.
fn resolved_index(grid: &Grid, row: usize, column: usize) -> u8 {
    let options = RenderOptions { colour: ColourMode::Resolved(FlashPhase::STEADY), ..default_options() };
    let mut pixels = vec![0u8; PAGE_WIDTH * options.row_height()];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
    render_row(grid, row, Field::Both, &GlyphSet::noto_teletext(), options, &mut buffer).unwrap();
    pixels[column * crate::consts::CELL_WIDTH]
}

const CYAN: u8 = 6;

const T: u8 = COLOUR_TRANSPARENT;

/// Invert, flash mode, conceal, reveal and flash unit, with the dots' and
/// gaps' indices they give.
type ColourCase = (bool, Option<FlashMode>, bool, bool, u8, (u8, u8));

/// REQ-398: the flashing and concealed ranges map to f or b by the flash phase and the reveal setting.
#[test]
fn palette_flash_and_reveal() {
    let mut palette = [0u8; PALETTE_SIZE];
    let base = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    fill_palette(&mut palette, &base, true, false);
    let flash = usize::from(PALETTE_FLASH_BASE) + 8 * 3 + 4;
    let conceal = usize::from(PALETTE_CONCEAL_BASE) + 8 * 3 + 4;
    assert_eq!((palette[flash], palette[conceal]), (3, 4));
    fill_palette(&mut palette, &base, false, true);
    assert_eq!((palette[flash], palette[conceal]), (4, 3));
    assert_eq!(palette[usize::from(PALETTE_TRANSPARENT)], 8);
}

/// REQ-L1-009, REQ-L1-010, REQ-L1-011: what the flash, conceal and box
/// marks of a cell do to its pixels.
/// REQ-398: a flashing or concealed cell's pixels take their index in those ranges. REQ-378: outside a box on a
/// boxed page both index the picture.
#[test]
fn flash_conceal_and_box_marks_drive_the_palette() {
    // Colours 1 and 4 are the single-gun colours.
    let (red, blue) = (COLOUR_RED_BIT, COLOUR_BLUE_BIT);
    let base = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    let mut grid = Grid::EMPTY;
    let plain = Cell { foreground: red, background: blue, ..Cell::BLANK };
    // Flash: foreground pixels take the background colour in the hidden
    // phase.
    let index = usize::from(foreground_index(&Cell { flash: true, ..plain }, &grid, false));
    for (visible, colour) in [(true, red), (false, blue)] {
        let mut palette = [0u8; PALETTE_SIZE];
        fill_palette(&mut palette, &base, visible, false);
        assert_eq!(palette[index], colour, "flash visible {visible}");
    }
    // Conceal: shown as a space on its background unless revealed.
    let index = usize::from(foreground_index(&Cell { conceal: true, ..plain }, &grid, false));
    for (reveal, colour) in [(false, blue), (true, red)] {
        let mut palette = [0u8; PALETTE_SIZE];
        fill_palette(&mut palette, &base, true, reveal);
        assert_eq!(palette[index], colour, "reveal {reveal}");
    }
    // Box membership changes nothing outside boxed mode (decision E.22).
    let boxed = Cell { boxed: true, ..plain };
    assert_eq!((foreground_index(&plain, &grid, false), background_index(&plain, &grid)), (red, blue));
    assert_eq!((foreground_index(&boxed, &grid, false), background_index(&boxed, &grid)), (red, blue));
    grid.boxed_mode = true;
    let picture = (PALETTE_TRANSPARENT, PALETTE_TRANSPARENT);
    assert_eq!((foreground_index(&plain, &grid, false), background_index(&plain, &grid)), picture);
    assert_eq!((foreground_index(&boxed, &grid, false), background_index(&boxed, &grid)), (red, blue));
}

/// REQ-398: the index of a foreground pixel: a flashing cell's and a
/// concealed cell's depend on neither the flash phase nor the reveal setting,
/// and a cell both flashing and concealed uses the concealed range while
/// reveal is off and the flashing range while it is on.
#[test]
fn a_flashing_concealed_cell_changes_range_with_reveal() {
    const FOREGROUND: u8 = 3;
    const BACKGROUND: u8 = 4;
    let grid = Grid::EMPTY;
    let pair = FOREGROUND * 8 + BACKGROUND;
    let plain = Cell { foreground: FOREGROUND, background: BACKGROUND, ..Cell::BLANK };
    let flashing = Cell { flash: true, ..plain };
    let concealed = Cell { conceal: true, ..plain };
    let both = Cell { flash: true, conceal: true, ..plain };
    for reveal in [false, true] {
        assert_eq!(foreground_index(&flashing, &grid, reveal), PALETTE_FLASH_BASE + pair);
        assert_eq!(foreground_index(&concealed, &grid, reveal), PALETTE_CONCEAL_BASE + pair);
    }
    assert_eq!(foreground_index(&both, &grid, false), PALETTE_CONCEAL_BASE + pair);
    assert_eq!(foreground_index(&both, &grid, true), PALETTE_FLASH_BASE + pair);
}

/// REQ-SEC-005, REQ-SEC-024, DEC-2464: at Level 3.5 a page can redefine
/// CLUT 0, so the status area and a receiver-drawn row 24 are drawn in the
/// receiver's own colour indices, 32 to 39, which no page redefines; the
/// page's own cells keep their Colour Map indices. Before, the status area
/// drew index 1 for red, whatever colour the page had made it.
#[test]
fn the_receivers_cells_are_drawn_in_its_own_colours() {
    use crate::consts::{RECEIVER_COLOUR_BASE, RESOLVED_COLOURS};
    let red = COLOUR_RED_BIT;
    let mut grid = Grid::EMPTY;
    for row in [0, 1, ROWS - 1] {
        grid.cells[row][2].background = red;
    }
    // The page makes red black: the receiver's cells do not follow it.
    grid.colour_map.entries[usize::from(red)] = grid.colour_map.entries[0];
    assert_eq!(resolved_index(&grid, 0, 2), RECEIVER_COLOUR_BASE + red, "the status area");
    assert_eq!(resolved_index(&grid, 1, 2), red, "the page's own row");
    assert_eq!(resolved_index(&grid, ROWS - 1, 2), red, "the page's own row 24");
    grid.receiver_row_24 = true;
    assert_eq!(resolved_index(&grid, ROWS - 1, 2), RECEIVER_COLOUR_BASE + red, "a receiver row 24");
    assert!(usize::from(RECEIVER_COLOUR_BASE + red) < RESOLVED_COLOURS);
}

/// REQ-RND-031: on Level 1 pages the resolved mode gives exactly the
/// colours of the palette mode, at every flash unit and both reveal
/// settings, with the slow rate standing in for the timing of decision
/// E.12; also on a boxed page.
#[test]
fn resolved_mode_matches_palette_mode_on_level1_pages() {
    let identity = [0u8, 1, 2, 3, 4, 5, 6, 7, T];
    for boxed_mode in [false, true] {
        let mut grid = level1_grid();
        grid.boxed_mode = boxed_mode;
        for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
            for reveal in [false, true] {
                let palette_options =
                    RenderOptions { mode, rounding: true, reveal, colour: ColourMode::Palette, underline: true };
                let indices = screen(&grid, palette_options);
                for shown in [true, false] {
                    let mut palette = [0u8; PALETTE_SIZE];
                    fill_palette(&mut palette, &identity, shown, reveal);
                    let expected: Vec<u8> = indices.iter().map(|&index| palette[usize::from(index)]).collect();
                    let colour = ColourMode::Resolved(FlashPhase::STEADY.with_level_1(shown));
                    // The status area is drawn in the receiver's own
                    // indices (DEC-2464), which are Table 30's CLUT 0 as the
                    // palette mode's are: compare the colours they stand for.
                    let resolved: Vec<u8> = screen(&grid, RenderOptions { colour, ..palette_options })
                        .iter()
                        .map(|&index| index.checked_sub(crate::consts::RECEIVER_COLOUR_BASE).unwrap_or(index))
                        .collect();
                    assert!(expected == resolved, "boxed {boxed_mode}, {mode:?}, reveal {reveal}, shown {shown}");
                }
            }
        }
    }
}

/// REQ-389 (REQ-RND-016), the cell raster primitive, and REQ-RND-036,
/// decisions E.47 and E.48: invert, then flash, then conceal,
/// for every combination, in both phases. Red on blue; the slow rate is on
/// in unit 0 and off in unit 9.
#[test]
fn invert_flash_and_conceal_combinations() {
    const ON: u8 = 0;
    const OFF: u8 = 9;
    let flip = |colour: u8| colour ^ ADJACENT_CLUT_FLIP;
    // (invert, flash mode, conceal, reveal, unit) -> (dots, gaps)
    let cases: [ColourCase; 26] = [
        (false, None, false, false, ON, (RED, BLUE)),
        (true, None, false, false, ON, (BLUE, RED)),
        (false, Some(FlashMode::Normal), false, false, ON, (RED, BLUE)),
        (false, Some(FlashMode::Normal), false, false, OFF, (BLUE, BLUE)),
        (false, Some(FlashMode::InvertPhase), false, false, ON, (BLUE, BLUE)),
        (false, Some(FlashMode::InvertPhase), false, false, OFF, (RED, BLUE)),
        (false, Some(FlashMode::AdjacentClut), false, false, ON, (RED, BLUE)),
        (false, Some(FlashMode::AdjacentClut), false, false, OFF, (flip(RED), BLUE)),
        (true, Some(FlashMode::Normal), false, false, ON, (BLUE, RED)),
        (true, Some(FlashMode::Normal), false, false, OFF, (RED, RED)),
        (true, Some(FlashMode::InvertPhase), false, false, ON, (RED, RED)),
        (true, Some(FlashMode::InvertPhase), false, false, OFF, (BLUE, RED)),
        (true, Some(FlashMode::AdjacentClut), false, false, ON, (BLUE, RED)),
        (true, Some(FlashMode::AdjacentClut), false, false, OFF, (flip(BLUE), RED)),
        (false, None, true, false, ON, (BLUE, BLUE)),
        (false, None, true, true, ON, (RED, BLUE)),
        (true, None, true, false, ON, (RED, RED)),
        (true, None, true, true, ON, (BLUE, RED)),
        (false, Some(FlashMode::Normal), true, false, ON, (BLUE, BLUE)),
        (false, Some(FlashMode::Normal), true, true, ON, (RED, BLUE)),
        (false, Some(FlashMode::Normal), true, true, OFF, (BLUE, BLUE)),
        (false, Some(FlashMode::AdjacentClut), true, false, OFF, (BLUE, BLUE)),
        (false, Some(FlashMode::AdjacentClut), true, true, OFF, (flip(RED), BLUE)),
        (true, Some(FlashMode::InvertPhase), true, false, OFF, (RED, RED)),
        (true, Some(FlashMode::InvertPhase), true, true, OFF, (BLUE, RED)),
        (true, Some(FlashMode::AdjacentClut), true, true, OFF, (flip(BLUE), RED)),
    ];
    let grid = Grid::EMPTY;
    for (invert, flash, conceal, reveal, unit, expected) in cases {
        let attributes = CellAttributes::DEFAULT
            .with_invert(invert)
            .with_flash_mode(flash.unwrap_or(FlashMode::Normal))
            .with_flash_rate(FlashRate::Slow);
        let cell = Cell {
            glyph: text(b'W'),
            foreground: RED,
            background: BLUE,
            flash: flash.is_some(),
            conceal,
            attributes,
            ..Cell::BLANK
        };
        let raster = line_of(&grid, &cell, 4, 4, 8, options_with(CellMode::Saa5050, unit, reveal));
        assert_eq!((raster.foreground, raster.background), expected, "{invert} {flash:?} {conceal} {reveal} {unit}");
        assert_ne!(raster.mask, 0);
    }
}

/// REQ-L25-029, Table C.1 of Annex C.1 (REQ-TST-016): all sixteen rows,
/// for the gaps and the dots of a cell, and mix mode.
#[test]
fn display_planes_follow_table_c1() {
    const OPAQUE: u8 = 2;
    const ROW: u8 = 5;
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Shows {
        Colour,
        Video,
        Row,
    }
    use Shows::{Colour, Row, Video};
    // (F/B transparent, row colour transparent, boxed, newsflash) -> result.
    let table = [
        (false, false, false, false, Colour),
        (false, false, false, true, Video),
        (false, false, true, false, Colour),
        (false, false, true, true, Colour),
        (false, true, false, false, Colour),
        (false, true, false, true, Video),
        (false, true, true, false, Colour),
        (false, true, true, true, Colour),
        (true, false, false, false, Row),
        (true, false, false, true, Video),
        (true, false, true, false, Video),
        (true, false, true, true, Row),
        (true, true, false, false, Video),
        (true, true, false, true, Video),
        (true, true, true, false, Video),
        (true, true, true, true, Video),
    ];
    for (fb_transparent, row_transparent, boxed, newsflash, result) in table {
        let mut grid = Grid::EMPTY;
        grid.boxed_mode = newsflash;
        grid.row_colours[6] = if row_transparent { T } else { ROW };
        let colour = if fb_transparent { T } else { OPAQUE };
        let cell = Cell { glyph: g3(0x3F), foreground: colour, background: colour, boxed, ..Cell::BLANK };
        let expected = match result {
            Colour => OPAQUE,
            Video => T,
            Row => ROW,
        };
        let raster = line_of(&grid, &cell, 6, 20, 3, large(0));
        assert_eq!(
            (raster.foreground, raster.background),
            (expected, expected),
            "{fb_transparent} {row_transparent} {boxed} {newsflash}"
        );
    }
    // Mix mode makes backgrounds the picture, not the dots.
    let mut grid = Grid::EMPTY;
    grid.mix = true;
    let cell = Cell { glyph: g3(0x3F), foreground: RED, background: BLUE, ..Cell::BLANK };
    let raster = line_of(&grid, &cell, 6, 20, 3, large(0));
    assert_eq!((raster.foreground, raster.background), (RED, T));
}

/// Decision E.45, REQ-SEC-005: the row colour never shows in row 0,
/// columns 0 to 7, even where the cell's colour is transparent.
#[test]
fn row_colour_never_colours_the_status_area() {
    let mut grid = Grid::EMPTY;
    grid.row_colours[0] = CYAN;
    let cell = Cell { background: T, ..Cell::BLANK };
    for column in 0..COLUMNS {
        let expected = if column < RECEIVER_COLUMNS { T } else { CYAN };
        assert_eq!(line_of(&grid, &cell, 0, column, 0, large(0)).background, expected, "column {column}");
    }
}

/// REQ-RND-030: indices above 31 keep their low five bits, so the resolved
/// mode always writes Colour Map indices.
#[test]
fn resolved_indices_are_colour_map_indices() {
    let grid = Grid::EMPTY;
    let cell = Cell { glyph: g3(0x3F), foreground: 0xFF, background: 0x3F, ..Cell::BLANK };
    let raster = line_of(&grid, &cell, 2, 2, 0, large(0));
    assert_eq!((raster.foreground, raster.background), (31, 31));
}

/// REQ-278, decision E.45: the screen colour is the picture on a boxed page
/// and in mix mode.
#[test]
fn border_index_follows_the_screen_colour() {
    let mut grid = Grid::EMPTY;
    grid.screen_colour = 0x2A;
    assert_eq!(border_index(&grid), 0x0A);
    grid.mix = true;
    assert_eq!(border_index(&grid), T);
    grid.mix = false;
    grid.boxed_mode = true;
    assert_eq!(border_index(&grid), T);
}
