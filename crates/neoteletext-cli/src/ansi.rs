//! Plain text (REQ-RND-008) and ANSI terminal output (REQ-RND-009), at
//! every level (REQ-RND-041): G3 characters convert as REQ-CHR-011 says,
//! and colours come from the page's Colour Map, except in the cells the
//! receiver draws, which keep Table 30's colours (DEC-2464).
//!
//! Every character written passes the allow-list of REQ-SEC-010, and the
//! only escape sequences are the fixed ones built here.

use neoteletext_app::present::cell_text;
pub use neoteletext_app::present::{MosaicStyle, plain};
use neoteletext_core::colour::{ColourIndex, ColourMap};
use neoteletext_core::consts::{COLOURS, PRIMARY_FULL};
use neoteletext_core::glyphs::{CharClass, class};
use neoteletext_core::level1::{Cell, Glyph, Grid};
use std::fmt::Write as _;

/// Terminal colour depth (REQ-RND-009).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColourDepth {
    /// 24-bit colour.
    TrueColour,
    /// The eight ANSI colours, whose numbering equals teletext's.
    Eight,
}

/// The nearest of the eight Level 1 colours to a colour (REQ-RND-041): each
/// primary on when it is at least half intensity, which is the nearest by
/// distance in each primary.
fn nearest_of_eight([red, green, blue]: [u8; 3]) -> u8 {
    const HALF: u8 = PRIMARY_FULL / 2;
    u8::from(red > HALF) | u8::from(green > HALF) << 1 | u8::from(blue > HALF) << 2
}

/// A colour as the terminal is told it: an RGB triple, or the terminal's
/// default for the picture.
fn colour_of(map: &ColourMap, index: ColourIndex, screen: ColourIndex) -> Option<[u8; 3]> {
    // Transparent cells are written in the screen colour (REQ-RND-041).
    let index = if ColourMap::is_transparent(index) { screen } else { index };
    (!ColourMap::is_transparent(index)).then(|| map.rgb888(index))
}

/// The colour of a cell the receiver draws (`Grid::is_receiver_cell`):
/// Table 30's CLUT 0, which no page can redefine (REQ-SEC-005,
/// REQ-SEC-024, DEC-2464), as the row renderer's receiver colours are.
/// A transparent one shows the terminal's default, as the row renderer
/// shows the picture there.
fn receiver_colour_of(index: ColourIndex) -> Option<[u8; 3]> {
    const CLUT0_LAST: u8 = (COLOURS - 1) as u8;
    (!ColourMap::is_transparent(index)).then(|| ColourMap::DEFAULT.rgb888(index & CLUT0_LAST))
}

/// What a cell's escape sequence depends on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Style {
    foreground: Option<[u8; 3]>,
    background: Option<[u8; 3]>,
    flash: bool,
    underline: bool,
}

impl Style {
    /// The style of the cell at `row` and `column`. Invert swaps the
    /// colours (decision E.47).
    fn of(cell: &Cell, grid: &Grid, row: usize, column: usize) -> Self {
        let screen = grid.screen_colour;
        let (foreground, background) = if cell.attributes.invert() {
            (cell.background, cell.foreground)
        } else {
            (cell.foreground, cell.background)
        };
        let picture = grid.mix || (grid.boxed_mode && !cell.boxed);
        let text = matches!(cell.glyph, Glyph::Char(character) if class(character) == CharClass::Text);
        let colour = |index| {
            if grid.is_receiver_cell(row, column) {
                receiver_colour_of(index)
            } else {
                colour_of(&grid.colour_map, index, screen)
            }
        };
        Self {
            foreground: colour(foreground),
            background: if picture { None } else { colour(background) },
            flash: cell.flash,
            underline: text && cell.attributes.underline(),
        }
    }

    /// Writes the selection. The sequences come from a fixed set: SGR 0, 4,
    /// 5, 38;2, 48;2, 30 to 37, 39, 40 to 47 and 49.
    fn write(self, depth: ColourDepth, out: &mut String) {
        out.push_str("\x1b[0");
        for (on, code) in [(self.underline, ";4"), (self.flash, ";5")] {
            if on {
                out.push_str(code);
            }
        }
        match (depth, self.foreground) {
            (_, None) => out.push_str(";39"),
            (ColourDepth::TrueColour, Some([r, g, b])) => {
                let _ = write!(out, ";38;2;{r};{g};{b}");
            }
            (ColourDepth::Eight, Some(rgb)) => {
                let _ = write!(out, ";{}", 30 + u16::from(nearest_of_eight(rgb)));
            }
        }
        match (depth, self.background) {
            (_, None) => out.push_str(";49"),
            (ColourDepth::TrueColour, Some([r, g, b])) => {
                let _ = write!(out, ";48;2;{r};{g};{b}");
            }
            (ColourDepth::Eight, Some(rgb)) => {
                let _ = write!(out, ";{}", 40 + u16::from(nearest_of_eight(rgb)));
            }
        }
        out.push('m');
    }
}

/// The page with ANSI colour (REQ-RND-009, REQ-RND-041). Double height and
/// double width are approximated: top and left halves as normal
/// characters, bottom and right halves as background.
#[must_use]
pub fn ansi(grid: &Grid, reveal: bool, mosaics: MosaicStyle, depth: ColourDepth) -> String {
    let mut out = String::new();
    for (row, cells) in grid.cells.iter().enumerate() {
        let mut previous: Option<Style> = None;
        for (column, cell) in cells.iter().enumerate() {
            let style = Style::of(cell, grid, row, column);
            if previous != Some(style) {
                style.write(depth, &mut out);
                previous = Some(style);
            }
            cell_text(cell, grid, reveal, mosaics, &mut out);
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "a test")]
mod tests {
    use super::*;
    use neoteletext_core::charset::{CharRef, CharSet, G0, G0Set, NationalOption};
    use neoteletext_core::colour::colour_index;
    use neoteletext_core::consts::COLOUR_TRANSPARENT;
    use neoteletext_core::level1::{CellAttributes, Size};
    use neoteletext_core::policy::is_safe_output_char;
    use neoteletext_core::types::Rgb12;

    fn grid_with(text: &str) -> Grid {
        let mut grid = Grid::EMPTY;
        let g0 = CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::English) });
        for (cell, byte) in grid.cells[1].iter_mut().zip(text.bytes()) {
            cell.glyph = Glyph::Char(CharRef::new(g0, byte));
        }
        grid
    }

    /// REQ-381: flash uses the terminal's blink attribute, and a bottom half
    /// is a blank cell in the background colour.
    #[test]
    fn flash_blinks_and_a_bottom_half_is_blank() {
        let mut grid = grid_with("AB");
        grid.cells[1][0].flash = true;
        let text = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::Eight);
        assert!(text.lines().nth(1).unwrap().starts_with("\x1b[0;5;"), "{text:?}");
        grid.cells[1][0].flash = false;
        grid.cells[1][0].background = 4;
        grid.cells[1][0].size = Size::DoubleBottom;
        let text = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::Eight);
        assert!(text.lines().nth(1).unwrap().starts_with("\x1b[0;37;44m "), "{text:?}");
    }

    /// REQ-381, REQ-423: every code of every character set: the terminal is
    /// given no escape sequence but the renderer's own, and no control.
    #[test]
    fn every_code_of_every_set_reaches_the_terminal_harmless() {
        for (name, set) in neoteletext_core::glyphs::conformance::FONT_SETS {
            for codes in (0..=u8::MAX).collect::<Vec<u8>>().chunks(40) {
                let mut grid = Grid::EMPTY;
                for (cell, &code) in grid.cells[1].iter_mut().zip(codes) {
                    cell.glyph = Glyph::Char(CharRef::new(set, code));
                }
                let text = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::TrueColour);
                let mut rest = text.as_str();
                while let Some(at) = rest.find('\u{1B}') {
                    let (before, sequence) = rest.split_at(at);
                    assert!(before.chars().all(|c| c == '\n' || is_safe_output_char(c)), "{name}: {before:?}");
                    let (body, after) = sequence.split_once('m').unwrap();
                    let parameters = body.strip_prefix("\u{1B}[").unwrap();
                    assert!(parameters.chars().all(|c| c.is_ascii_digit() || c == ';'), "{name}: {body:?}");
                    rest = after;
                }
                assert!(rest.chars().all(|c| c == '\n' || is_safe_output_char(c)), "{name}: {rest:?}");
            }
        }
    }

    /// REQ-381, REQ-423: a page's control codes never reach the terminal, and only the renderer's own escape
    /// sequences do.
    #[test]
    fn page_control_codes_never_reach_the_terminal() {
        // Every control code a page can carry, in every cell.
        let hostile: String = (0u8..0x20).map(char::from).chain("\u{7f}".chars()).collect();
        let text = ansi(&grid_with(&hostile), false, MosaicStyle::Unicode16, ColourDepth::Eight);
        let bytes = text.as_bytes();
        for (index, &byte) in bytes.iter().enumerate() {
            if byte == 0x1B {
                assert_eq!(bytes.get(index + 1), Some(&b'['), "only our own CSI sequences");
            } else {
                assert!(byte >= 0x20 || byte == b'\n', "control byte {byte:#x} written");
            }
        }
    }

    /// REQ-RND-041: 24-bit colours come from the page's Colour Map, the
    /// 8-colour mode takes the nearest Level 1 colour, transparent cells
    /// take the screen colour, invert swaps the colours, and underline uses
    /// its SGR code on text.
    /// REQ-381: 24-bit colour and 8-colour modes.
    #[test]
    fn terminal_colours_and_attributes() {
        let mut grid = grid_with("A");
        grid.colour_map.entries[usize::from(colour_index(2, 3))] = Rgb12::new(15, 8, 1);
        grid.screen_colour = colour_index(3, 2);
        let cell = &mut grid.cells[1][0];
        cell.foreground = colour_index(2, 3);
        cell.background = COLOUR_TRANSPARENT;
        let screen = grid.colour_map.rgb888(colour_index(3, 2));
        let text = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::TrueColour);
        let line = text.lines().nth(1).unwrap();
        let expected = format!("\x1b[0;38;2;255;136;17;48;2;{};{};{}mA", screen[0], screen[1], screen[2]);
        assert!(line.starts_with(&expected), "{line:?}");
        let eight = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::Eight);
        let screen_eight = 40 + u16::from(nearest_of_eight(screen));
        assert!(eight.lines().nth(1).unwrap().starts_with(&format!("\x1b[0;33;{screen_eight}mA")));
        // Invert and underline.
        let cell = &mut grid.cells[1][0];
        cell.foreground = 1;
        cell.background = 4;
        cell.attributes = CellAttributes::DEFAULT.with_invert(true).with_underline(true);
        let text = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::Eight);
        assert!(text.lines().nth(1).unwrap().starts_with("\x1b[0;4;34;41mA"));
    }

    /// REQ-418 (REQ-SEC-005), REQ-437 (REQ-SEC-024), DEC-2464: a page that
    /// redefines CLUT 0 (X/28/4, M/29/4) cannot recolour the receiver's
    /// cells in a terminal: the status area and a receiver-drawn row 24 keep
    /// the escape sequences of Table 30's colours, while the page's own
    /// cells take its Colour Map.
    #[test]
    fn a_redefined_clut_0_leaves_the_receivers_cells_alone() {
        let mut grid = grid_with("A");
        let last = neoteletext_core::consts::ROWS - 1;
        for (row, column) in [(0, 0), (0, 7), (0, 8), (1, 0), (last, 0)] {
            grid.cells[row][column].glyph = grid.cells[1][0].glyph;
            grid.cells[row][column].foreground = 2;
            grid.cells[row][column].background = 0;
        }
        grid.receiver_row_24 = true;
        let before = ansi(&grid, false, MosaicStyle::Unicode16, ColourDepth::TrueColour);
        let mut hostile = grid;
        // The page makes green and black the same colour.
        hostile.colour_map.entries[2] = hostile.colour_map.entries[0];
        let after = ansi(&hostile, false, MosaicStyle::Unicode16, ColourDepth::TrueColour);
        let (before_lines, after_lines): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
        // Row 0 draws "A" at columns 0, 7 and 8: the text before the
        // second "A" is the status area's, the text before the third
        // column 8's.
        let parts = |line: &str| line.split('A').map(str::to_owned).collect::<Vec<_>>();
        let (status_before, status_after) = (parts(before_lines[0]), parts(after_lines[0]));
        assert_eq!(status_before[..2], status_after[..2], "the status area keeps its colours");
        assert!(status_after[2].ends_with("38;2;0;0;0;48;2;0;0;0m"), "column 8 is the page's: {:?}", status_after[2]);
        assert_eq!(before_lines[last], after_lines[last], "a receiver-drawn row 24 keeps its colours");
        assert_ne!(before_lines[1], after_lines[1], "the page's own cells follow its Colour Map");
    }

    /// The nearest Level 1 colour of every Table 30 entry of CLUT 0 is the
    /// entry itself.
    #[test]
    fn nearest_colour_keeps_clut_0() {
        for index in 0..8u8 {
            assert_eq!(nearest_of_eight(ColourMap::DEFAULT.rgb888(index)), index);
        }
        assert_eq!(nearest_of_eight(ColourMap::DEFAULT.rgb888(colour_index(1, 1))), 0, "half red is dark");
    }
}
