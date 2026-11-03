//! The builders the renderer's tests share: grids, cells, options and
//! screens made by hand, so that each test depends on nothing but the grid
//! (REQ-RND-001).
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
use crate::consts::{CELL_WIDTH, PAGE_WIDTH};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, Glyph, Grid, Size};
use std::vec;
use std::vec::Vec;

pub(super) fn default_options() -> RenderOptions {
    RenderOptions {
        mode: CellMode::Saa5050,
        rounding: true,
        reveal: false,
        colour: ColourMode::Palette,
        underline: true,
    }
}

pub(super) const RED: u8 = 1;

pub(super) const BLUE: u8 = 4;

pub(super) fn latin() -> CharSet {
    CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::English) })
}

pub(super) fn text(code: u8) -> Glyph {
    Glyph::Char(CharRef::new(latin(), code))
}

pub(super) fn g3(code: u8) -> Glyph {
    Glyph::Char(CharRef::new(CharSet::G3, code))
}

pub(super) fn options_with(mode: CellMode, unit: u8, reveal: bool) -> RenderOptions {
    RenderOptions {
        mode,
        rounding: true,
        reveal,
        colour: ColourMode::Resolved(FlashPhase::at_unit(unit)),
        underline: true,
    }
}

pub(super) fn large(unit: u8) -> RenderOptions {
    options_with(CellMode::Saa5050, unit, false)
}

pub(super) fn line_of(
    grid: &Grid,
    cell: &Cell,
    row: usize,
    column: usize,
    line: usize,
    options: RenderOptions,
) -> CellLine {
    cell_line(cell, grid, row, column, line, &GlyphSet::noto_teletext(), options)
}

/// A whole screen at the grid's row width.
pub(super) fn screen(grid: &Grid, options: RenderOptions) -> Vec<u8> {
    let width = PAGE_WIDTH;
    let mut out = vec![0u8; width * ROWS * options.row_height()];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut out, stride: width };
    render_screen(grid, ALL_ROWS, &GlyphSet::noto_teletext(), options, &mut buffer).unwrap();
    out
}

/// A grid that only Level 1 could have made: CLUT 0 colours, normal slow
/// flash, conceal, boxes, mosaics, text and double height.
pub(super) fn level1_grid() -> Grid {
    let mut grid = Grid::EMPTY;
    for (column, cell) in grid.cells[3].iter_mut().enumerate() {
        let code = 0x41 + (column % 26) as u8;
        *cell = Cell {
            glyph: if column % 3 == 0 {
                Glyph::Mosaic { pattern: code & 0x3F, separated: column % 2 == 0 }
            } else {
                text(code)
            },
            foreground: (column % 8) as u8,
            background: ((column + 3) % 8) as u8,
            flash: column % 4 == 1,
            conceal: column % 5 == 2,
            boxed: column % 6 == 0,
            ..Cell::BLANK
        };
    }
    grid.cells[5][4] = Cell { glyph: text(b'H'), size: Size::DoubleTop, ..Cell::BLANK };
    grid.cells[6][4] = Cell { glyph: text(b'H'), size: Size::DoubleBottom, ..Cell::BLANK };
    grid.cells[7][1] = Cell { glyph: text(b'Q'), flash: true, conceal: true, foreground: RED, ..Cell::BLANK };
    grid
}

/// Each half-dot of a normal-width line, drawn twice, across 24 dots.
pub(super) fn doubled(mask: u16) -> u32 {
    (0..CELL_WIDTH).fold(0u32, |acc, dot| if (mask >> dot) & 1 == 1 { acc | (0b11 << (2 * dot)) } else { acc })
}
