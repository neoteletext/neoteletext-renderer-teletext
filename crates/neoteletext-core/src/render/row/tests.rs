//! Tests of the row renderer: rows drawn alike in any order, fields, zoom
//! and bounds, and the index formats it refuses.
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

use crate::charset::{CharRef, CharSet, G0, G0Set, NationalOption};
use crate::config::CellMode;
use crate::consts::{CELL_HEIGHT, PAGE_WIDTH};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, CellAttributes, FlashMode, FlashRate, Glyph, Grid, Zoom};
use crate::render::test_grids::*;
use std::vec;

/// A case the palette mode must refuse: its name and how to make it.
type RefusalCase = (&'static str, fn(&mut Grid));

/// REQ-RND-021's test harness: rows drawn alone, in reverse order and
/// line by line match the whole-screen loop of REQ-397 (REQ-RND-022).
#[test]
fn rows_render_identically_in_any_order() {
    let mut grid = Grid::EMPTY;
    let g0 = CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::English) });
    grid.cells[2][3].glyph = Glyph::Char(CharRef::new(g0, b'A'));
    grid.cells[2][4].glyph = Glyph::Mosaic { pattern: 0x15, separated: false };
    let glyphs = GlyphSet::noto_teletext();
    let mut whole = vec![0u8; PAGE_WIDTH * 500];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut whole, stride: PAGE_WIDTH };
    render_screen(&grid, ALL_ROWS, &glyphs, default_options(), &mut buffer).unwrap();
    for row in (0..ROWS).rev() {
        let mut single = vec![0u8; PAGE_WIDTH * 20];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut single, stride: PAGE_WIDTH };
        render_row(&grid, row, Field::Both, &glyphs, default_options(), &mut buffer).unwrap();
        assert_eq!(&whole[row * 20 * PAGE_WIDTH..(row + 1) * 20 * PAGE_WIDTH], &single[..]);
    }
    for y in 0..500 {
        let mut line = vec![0u8; PAGE_WIDTH];
        render_line(&grid, y, Zoom::Off, &glyphs, default_options(), IndexFormat::Bits8, &mut line).unwrap();
        assert_eq!(&whole[y * PAGE_WIDTH..(y + 1) * PAGE_WIDTH], &line[..]);
    }
}

/// REQ-RND-017, REQ-L1-020: one field draws the even or the odd lines of
/// a row; zoom doubles each source line of rows 0 to 11 or 12 to 23; a
/// line beyond the last row, directly or through zoom, is out of range; a
/// framebuffer too small for the rows asked for is refused.
#[test]
fn fields_zoom_and_bounds_of_the_row_renderer() {
    let mut grid = Grid::EMPTY;
    let g0 = CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::English) });
    for row in [0, 5, 12] {
        grid.cells[row][1].glyph = Glyph::Char(CharRef::new(g0, b'A' + row as u8));
        grid.cells[row][2].glyph = Glyph::Mosaic { pattern: 0x2A, separated: false };
    }
    let glyphs = GlyphSet::noto_teletext();
    let lines = 20;
    let row = |field| {
        let mut out = vec![0u8; PAGE_WIDTH * lines];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut out, stride: PAGE_WIDTH };
        render_row(&grid, 5, field, &glyphs, default_options(), &mut buffer).unwrap();
        out
    };
    let both = row(Field::Both);
    let (first, second) = (row(Field::First), row(Field::Second));
    for line in 0..lines / 2 {
        let line_of = |pixels: &[u8], line: usize| pixels[line * PAGE_WIDTH..(line + 1) * PAGE_WIDTH].to_vec();
        assert_eq!(line_of(&first, line), line_of(&both, 2 * line), "first field, line {line}");
        assert_eq!(line_of(&second, line), line_of(&both, 2 * line + 1), "second field, line {line}");
    }
    let line = |y, zoom| {
        let mut out = vec![0u8; PAGE_WIDTH];
        render_line(&grid, y, zoom, &glyphs, default_options(), IndexFormat::Bits8, &mut out).map(|()| out)
    };
    for y in [0, 7, 39, 105] {
        assert_eq!(line(2 * y, Zoom::Top), line(y, Zoom::Off), "top, line {y}");
        assert_eq!(line(2 * y + 1, Zoom::Top), line(y, Zoom::Off), "top, line {y} doubled");
        assert_eq!(line(2 * y, Zoom::Bottom), line(12 * lines + y, Zoom::Off), "bottom, line {y}");
    }
    // A line beyond the last row, zoomed or not.
    assert_eq!(line(2 * (ROWS - 12) * lines, Zoom::Bottom), Err(RenderError::OutOfRange));
    assert_eq!(line(ROWS * lines, Zoom::Off), Err(RenderError::OutOfRange));
    let mut out = vec![0u8; PAGE_WIDTH * lines];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut out, stride: PAGE_WIDTH };
    assert_eq!(
        render_row(&grid, ROWS, Field::Both, &glyphs, default_options(), &mut buffer),
        Err(RenderError::OutOfRange)
    );
    // Room for rows 0 and 1 only: drawing row 2 is refused.
    let mut small = vec![0u8; PAGE_WIDTH * lines * 2];
    let mut framebuffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut small, stride: PAGE_WIDTH };
    assert_eq!(render_screen(&grid, 0b11, &glyphs, default_options(), &mut framebuffer), Ok(()));
    assert_eq!(
        render_screen(&grid, 0b111, &glyphs, default_options(), &mut framebuffer),
        Err(RenderError::BufferTooSmall)
    );
}

/// REQ-RND-031, decision E.55: the palette mode is refused for each case
/// that needs the resolved mode, by every drawing call.
/// REQ-398: the palette mode is refused for what only the resolved mode can draw.
#[test]
fn palette_mode_is_refused_where_it_cannot_draw() {
    let glyphs = GlyphSet::noto_teletext();
    let palette = RenderOptions {
        mode: CellMode::Saa5050,
        rounding: true,
        reveal: false,
        colour: ColourMode::Palette,
        underline: true,
    };
    let refused = |grid: &Grid| {
        let mut out = vec![0u8; PAGE_WIDTH * CELL_HEIGHT];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut out, stride: PAGE_WIDTH };
        let row = render_row(grid, 2, Field::Both, &glyphs, palette, &mut buffer);
        let line = render_line(grid, 2 * CELL_HEIGHT, Zoom::Off, &glyphs, palette, IndexFormat::Bits8, &mut out);
        assert_eq!(row, line);
        assert_eq!(row.is_err(), !palette_mode_allowed(grid));
        row == Err(RenderError::NeedsResolvedMode)
    };
    assert!(!refused(&level1_grid()), "a Level 1 page draws in the palette mode");
    let cases: [RefusalCase; 6] = [
        ("foreground outside CLUT 0", |grid| grid.cells[2][5].foreground = 9),
        ("background outside CLUT 0", |grid| grid.cells[2][5].background = 31),
        ("fast flash", |grid| {
            grid.cells[2][5].flash = true;
            grid.cells[2][5].attributes = CellAttributes::DEFAULT.with_flash_rate(FlashRate::FastPhase2);
        }),
        ("inverted flash", |grid| {
            grid.cells[2][5].flash = true;
            grid.cells[2][5].attributes = CellAttributes::DEFAULT.with_flash_mode(FlashMode::InvertPhase);
        }),
        ("the Additional Flash Functions' slow flash, timed by units", |grid| {
            grid.cells[2][5].flash = true;
            grid.cells[2][5].attributes = CellAttributes::DEFAULT.with_flash_rate(FlashRate::Slow);
        }),
        ("adjacent-CLUT flash", |grid| {
            grid.cells[2][5].flash = true;
            grid.cells[2][5].attributes = CellAttributes::DEFAULT.with_flash_mode(FlashMode::AdjacentClut);
        }),
    ];
    for (name, case) in cases {
        let mut grid = level1_grid();
        case(&mut grid);
        assert!(refused(&grid), "{name}");
    }
    // Flash with the default slow normal attributes is Level 1 flash.
    let mut grid = level1_grid();
    grid.cells[2][5].flash = true;
    assert!(!refused(&grid));
}

/// REQ-RND-017: whole screen, row by row and line by line agree, and a
/// buffer one pixel short of a row is refused.
#[test]
fn rows_render_identically_in_any_order_in_the_resolved_mode() {
    let mut grid = Grid::EMPTY;
    grid.screen_colour = 3;
    grid.cells[4][0].glyph = text(b'A');
    grid.cells[9][39].glyph = g3(0x25);
    let width = PAGE_WIDTH;
    let options = large(5);
    let whole = screen(&grid, options);
    let glyphs = GlyphSet::noto_teletext();
    for row in (0..ROWS).rev() {
        let mut single = vec![0u8; width * CELL_HEIGHT];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut single, stride: width };
        render_row(&grid, row, Field::Both, &glyphs, options, &mut buffer).unwrap();
        assert_eq!(&whole[row * CELL_HEIGHT * width..(row + 1) * CELL_HEIGHT * width], &single[..]);
    }
    for y in 0..ROWS * CELL_HEIGHT {
        let mut line = vec![0u8; width];
        render_line(&grid, y, Zoom::Off, &glyphs, options, IndexFormat::Bits8, &mut line).unwrap();
        assert_eq!(&whole[y * width..(y + 1) * width], &line[..]);
    }
    let mut narrow = vec![0u8; width - 1];
    let result = render_line(&grid, 0, Zoom::Off, &glyphs, options, IndexFormat::Bits8, &mut narrow);
    assert_eq!(result, Err(RenderError::BufferTooSmall));
}

/// REQ-RND-017: in the resolved mode the 4-bit format holds indices up to
/// 15 and refuses wider ones.
#[test]
fn four_bit_format_in_the_resolved_mode() {
    let mut grid = Grid::EMPTY;
    grid.cells[2][3] = Cell { glyph: g3(0x3F), foreground: 15, background: 9, ..Cell::BLANK };
    let glyphs = GlyphSet::noto_teletext();
    let mut out = vec![0u8; PAGE_WIDTH / 2];
    render_line(&grid, 2 * CELL_HEIGHT, Zoom::Off, &glyphs, large(0), IndexFormat::Bits4, &mut out).unwrap();
    assert_eq!(&out[3 * 6..4 * 6], &[0xFF; 6]);
    grid.cells[2][3].foreground = 16;
    let result = render_line(&grid, 2 * CELL_HEIGHT, Zoom::Off, &glyphs, large(0), IndexFormat::Bits4, &mut out);
    assert_eq!(result, Err(RenderError::IndexTooWide));
}

/// REQ-390, DEC-2129: at 4 bits the palette mode draws the eight colours
/// and refuses a flashing or concealed cell, whose indices of REQ-398 do not
/// fit, rather than draw another colour.
#[test]
fn four_bit_format_in_the_palette_mode() {
    let mut grid = Grid::EMPTY;
    grid.cells[2][3] = Cell { glyph: g3(0x3F), foreground: 7, background: 1, ..Cell::BLANK };
    let glyphs = GlyphSet::noto_teletext();
    let mut out = vec![0u8; PAGE_WIDTH / 2];
    let palette = RenderOptions { colour: ColourMode::Palette, ..large(0) };
    let draw = |grid: &Grid, out: &mut [u8]| {
        render_line(grid, 2 * CELL_HEIGHT, Zoom::Off, &glyphs, palette, IndexFormat::Bits4, out)
    };
    assert_eq!(draw(&grid, &mut out), Ok(()));
    assert_eq!(&out[3 * 6..4 * 6], &[0x77; 6], "a solid cell in its foreground");
    grid.cells[2][3].flash = true;
    assert_eq!(draw(&grid, &mut out), Err(RenderError::IndexTooWide), "flash");
    grid.cells[2][3].flash = false;
    grid.cells[2][3].conceal = true;
    assert_eq!(draw(&grid, &mut out), Err(RenderError::IndexTooWide), "conceal");
}
