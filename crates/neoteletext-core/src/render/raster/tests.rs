//! Tests of the cell raster (REQ-RND-016 and the areas it draws): rounding,
//! mosaic geometry, double width and size, underline and the G3 shapes. The
//! tests of drawing loaded fonts are in tests/fonts.rs.
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

use crate::charset::{CharRef, CharSet};
use crate::config::CellMode;
use crate::consts::{CELL_WIDTH, G3_CODES, G3_FIRST_CODE};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, CellAttributes, Glyph, Grid, Size, Width};
use crate::render::raster::{CELL_MASK, SEPARATED_COLUMNS, font_line_20, mosaic_block, mosaic_line};
use crate::render::test_grids::*;
use std::vec;
use std::vec::Vec;

/// The normal-width mask of a cell on a line.
fn normal_mask(grid: &Grid, cell: Cell, line: usize, options: RenderOptions) -> u16 {
    let plain = Cell { size: Size::Normal, attributes: cell.attributes.with_width(Width::Normal), ..cell };
    line_of(grid, &plain, 5, 5, line, options).mask
}

/// Decision E.13 as a doubled dot-matrix font gets it (REQ-FNT-008).
/// REQ-385: decision E.13's rule on a doubled dot-matrix font.
#[test]
fn rounding_matches_decision_e13() {
    // A diagonal of doubled dots: line 0 has its dot at the right, line 1
    // one dot to the left.
    let rows = [0b0011, 0b1100, 0, 0, 0, 0, 0, 0, 0, 0];
    let lower = font_line_20(&rows, 1, true);
    assert_eq!(lower, rows[0] | (rows[0] << 1) & rows[1] & !(rows[1] << 1));
    // Upper sub-line of the first line has no neighbour above: no rounding.
    assert_eq!(font_line_20(&rows, 0, true), rows[0]);
    assert_eq!(font_line_20(&rows, 1, false), rows[0]);
    // Nothing is drawn beyond the glyph's lines.
    assert_eq!(font_line_20(&rows, 19, true), 0);
}

/// REQ-RND-013: decision E.14's geometry in both cell sizes.
#[test]
fn separated_mosaic_geometry_matches_e14() {
    let full = 0x3F;
    let blank_lines: Vec<usize> = (0..20).filter(|&l| mosaic_line(full, true, l, 20) == 0).collect();
    assert_eq!(blank_lines, vec![4, 5, 12, 13, 18, 19]);
    assert_eq!(mosaic_line(full, true, 0, 20), !SEPARATED_COLUMNS & CELL_MASK);
    let small: Vec<usize> = (0..10).filter(|&l| mosaic_line(full, true, l, 10) == 0).collect();
    assert_eq!(small, vec![2, 6, 9]);
    // Contiguous blocks: 6, 8 and 6 lines.
    assert_eq!(mosaic_block(5, 20).0, 0);
    assert_eq!(mosaic_block(6, 20).0, 1);
    assert_eq!(mosaic_block(13, 20).0, 1);
    assert_eq!(mosaic_block(14, 20).0, 2);
}

/// REQ-RND-034: the left and right halves of a double-width character
/// show the normal-width line, each half-dot drawn twice, for text with
/// rounding, mosaics and G3; double size adds the vertical halves.
#[test]
fn double_width_and_double_size() {
    let grid = Grid::EMPTY;
    let glyphs = [text(b'W'), Glyph::Mosaic { pattern: 0x2D, separated: true }, g3(0x22)];
    for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
        let options = options_with(mode, 0, false);
        let height = options.row_height();
        for glyph in glyphs {
            for (size, vertical) in [(Size::Normal, 0), (Size::DoubleTop, 1), (Size::DoubleBottom, 2)] {
                let half =
                    |width| Cell { glyph, size, attributes: CellAttributes::DEFAULT.with_width(width), ..Cell::BLANK };
                for line in 0..height {
                    let source = match vertical {
                        0 => line,
                        1 => line / 2,
                        _ => height / 2 + line / 2,
                    };
                    let normal = normal_mask(&grid, half(Width::Normal), source, options);
                    let left = line_of(&grid, &half(Width::LeftHalf), 5, 5, line, options).mask;
                    let right = line_of(&grid, &half(Width::RightHalf), 5, 6, line, options).mask;
                    let joined = (u32::from(left) << CELL_WIDTH) | u32::from(right);
                    assert_eq!(joined, doubled(normal), "{glyph:?} {size:?} {mode:?} line {line}");
                }
            }
        }
    }
}

/// REQ-RND-035, decision E.47: underline is the last line of the cell,
/// full width, on G0 and G2 characters only, and on the bottom half of a
/// double-height character; the underline bit on a mosaic means separated
/// mosaics and draws no line.
#[test]
fn underline_on_the_last_line() {
    let grid = Grid::EMPTY;
    let underlined = CellAttributes::DEFAULT.with_underline(true);
    let g2 = CharSet::G2(crate::charset::G2Set::Latin);
    for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
        let options = options_with(mode, 0, false);
        let last = options.row_height() - 1;
        for glyph in [text(b'g'), text(b' '), Glyph::Char(CharRef::new(g2, 0x23))] {
            let cell = Cell { glyph, attributes: underlined, ..Cell::BLANK };
            assert_eq!(line_of(&grid, &cell, 1, 1, last, options).mask, CELL_MASK, "{glyph:?} {mode:?}");
            let plain = Cell { attributes: CellAttributes::DEFAULT, ..cell };
            for line in 0..last {
                assert_eq!(line_of(&grid, &cell, 1, 1, line, options), line_of(&grid, &plain, 1, 1, line, options));
            }
            let top = Cell { size: Size::DoubleTop, ..cell };
            let bottom = Cell { size: Size::DoubleBottom, ..cell };
            let plain_top = Cell { attributes: CellAttributes::DEFAULT, ..top };
            for line in 0..=last {
                assert_eq!(
                    line_of(&grid, &top, 1, 1, line, options),
                    line_of(&grid, &plain_top, 1, 1, line, options),
                    "top half"
                );
            }
            assert_eq!(line_of(&grid, &bottom, 2, 1, last, options).mask, CELL_MASK);
            assert_eq!(line_of(&grid, &bottom, 2, 1, last - 1, options).mask, CELL_MASK, "the enlarged underline");
        }
        // Never on G3, a mosaic or an empty cell.
        for glyph in [g3(0x20), Glyph::Space] {
            let cell = Cell { glyph, attributes: underlined, ..Cell::BLANK };
            let plain = Cell { attributes: CellAttributes::DEFAULT, ..cell };
            assert_eq!(line_of(&grid, &cell, 1, 1, last, options), line_of(&grid, &plain, 1, 1, last, options));
        }
        let separated =
            Cell { glyph: Glyph::Mosaic { pattern: 0x3F, separated: true }, attributes: underlined, ..Cell::BLANK };
        assert_eq!(line_of(&grid, &separated, 1, 1, last, options).mask, 0, "separated mosaics, no underline");
    }
}

/// Decision E.88: with the underline option off, no G0 or G2 character is
/// underlined, in either half of double height, and separated mosaics,
/// which share the attribute bit, are drawn exactly as before.
#[test]
fn the_underline_option_turns_underline_off_and_leaves_separated_mosaics() {
    let grid = Grid::EMPTY;
    let underlined = CellAttributes::DEFAULT.with_underline(true);
    let g2 = CharSet::G2(crate::charset::G2Set::Latin);
    for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
        let on = options_with(mode, 0, false);
        let off = on.with_underline(false);
        assert!(on.underline && !off.underline);
        let last = on.row_height() - 1;
        for glyph in [text(b'g'), text(b' '), Glyph::Char(CharRef::new(g2, 0x23))] {
            for size in [Size::Normal, Size::DoubleTop, Size::DoubleBottom] {
                let cell = Cell { glyph, size, attributes: underlined, ..Cell::BLANK };
                let plain = Cell { attributes: CellAttributes::DEFAULT, ..cell };
                let row = if size == Size::DoubleBottom { 2 } else { 1 };
                for line in 0..=last {
                    assert_eq!(
                        line_of(&grid, &cell, row, 1, line, off),
                        line_of(&grid, &plain, row, 1, line, off),
                        "{glyph:?} {size:?} {mode:?} line {line}"
                    );
                }
            }
        }
        for separated in [false, true] {
            let mosaic = Glyph::Mosaic { pattern: 0x3F, separated };
            let cell = Cell { glyph: mosaic, attributes: underlined, ..Cell::BLANK };
            for line in 0..=last {
                assert_eq!(
                    line_of(&grid, &cell, 1, 1, line, off),
                    line_of(&grid, &cell, 1, 1, line, on),
                    "mosaic line {line}"
                );
            }
        }
    }
}

/// REQ-RND-037: every G3 position draws in both cell sizes, from the
/// generated data, without rounding and without panicking; the undefined
/// positions and 5/F are spaces.
/// REQ-76: every defined G3 position has a glyph.
/// REQ-385: rounding on or off draws a G3 character the same.
#[test]
fn every_g3_position_draws() {
    let grid = Grid::EMPTY;
    let glyphs = GlyphSet::noto_teletext();
    for offset in 0..G3_CODES {
        let code = G3_FIRST_CODE + offset as u8;
        let data = glyphs.g3(code);
        for mode in [CellMode::Saa5050, CellMode::Saa5246a] {
            for rounding in [false, true] {
                let options = RenderOptions { rounding, ..options_with(mode, 0, false) };
                let cell = Cell { glyph: g3(code), ..Cell::BLANK };
                for line in 0..options.row_height() {
                    let expected = match mode {
                        CellMode::Saa5050 => data.large[line],
                        CellMode::Saa5246a => data.small[line],
                    };
                    assert_eq!(line_of(&grid, &cell, 1, 1, line, options).mask, expected, "{code:#04x} {mode:?}");
                }
            }
        }
    }
    for code in [0x5F, 0x6E, 0x6F, 0x7E, 0x7F, 0x00, 0x80, 0xFF] {
        assert_eq!(*glyphs.g3(code), crate::glyphs::G3Glyph::BLANK, "{code:#04x}");
    }
}

/// REQ-RND-037: shapes checked against Table 48's figure. 2/5 and 6/5 are
/// the two halves of the cell cut by its diagonal; 3/F is the full block;
/// 5/0 and 5/1 are the light lines through the centre; 5/D points up;
/// 4/E is a disc.
/// REQ-76: G3 glyphs drawn as REQ-RND-037 describes.
#[test]
fn g3_shapes_match_the_figure() {
    let glyphs = GlyphSet::noto_teletext();
    let small = |code| glyphs.g3(code).small;
    let lower_left = small(0x25);
    assert_eq!(lower_left[9] & 0x800, 0x800, "2/5 fills the bottom-left corner");
    assert_eq!(lower_left[0] & 0x001, 0, "and not the top-right");
    for line in 0..10 {
        assert_eq!(small(0x25)[line] | small(0x65)[line], CELL_MASK, "2/5 and 6/5 tile the cell");
        assert_eq!(small(0x25)[line] & small(0x65)[line], 0);
    }
    assert_eq!(small(0x3F), [CELL_MASK; 10]);
    assert_eq!(small(0x50), [0b0000_0110_0000; 10]);
    assert_eq!(small(0x51)[4..6], [CELL_MASK, CELL_MASK]);
    let up = small(0x5D);
    assert!(up[0].count_ones() < up[3].count_ones(), "5/D widens downwards from its point");
    let disc = small(0x4E);
    assert!(disc[0] == 0 && disc[5] == CELL_MASK && disc[1].count_ones() < disc[4].count_ones());
}

// Loaded fonts fall back to the shipped glyph set.
mod fonts;
