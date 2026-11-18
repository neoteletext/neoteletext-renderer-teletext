//! Damage-only repaint (animation.md ANIM-4, REQ-6529): which rectangle of
//! a page changed since it was last rendered, and the rendering of that
//! rectangle alone.

use super::changes::flash_changes;
use super::options::{ColourMode, RenderOptions};
use super::row::{check_mode, write_cell};
use super::{IndexBuffer, RenderError, cell_line};
use crate::consts::{COLUMNS, ROWS};
use crate::glyphs::GlyphSet;
use crate::level1::{Cell, Grid, Size, Width};
use crate::types::CellRect;

/// Damage tracking (ANIM-4): the rectangle of a page that changed since
/// the caller last asked, in whole cells. It keeps the grid it last saw,
/// so a cell is compared with itself and never with a fingerprint: what
/// [`render_rect`] draws of the rectangle it reports is, dot for dot, what
/// rendering the whole page gives.
///
/// EN 300 706 says nothing of redrawing; like [`ChangeTracker`](super::ChangeTracker),
/// which reports whole rows from 200 bytes of state, this is the port's
/// choice (DEC-553): it costs a grid of memory and reports a rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DamageTracker {
    last: Grid,
    options: Option<RenderOptions>,
    /// The cells the sprites covered when last asked.
    sprites: Option<CellRect>,
}

impl Default for DamageTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// True when a cell reaches into the row below or the column to its
/// right: a changed double-height or double-width character changes the
/// cells that show its other parts.
const fn enlarged(cell: &Cell) -> (bool, bool) {
    (matches!(cell.size, Size::DoubleTop), matches!(cell.attributes.width(), Width::LeftHalf))
}

impl DamageTracker {
    /// A tracker that reports the whole page on first use.
    #[must_use]
    pub const fn new() -> Self {
        Self { last: Grid::EMPTY, options: None, sprites: None }
    }

    /// Forces the whole page to be reported on the next call.
    pub const fn invalidate(&mut self) {
        self.options = None;
    }

    /// The rectangle that changed since the last call, for drawing with
    /// `options`; `None` when nothing did. It is the union, in whole
    /// cells, of:
    ///
    /// - the cells whose character, attributes or enhancement changed, and
    ///   the cell below or to the right that shows the other part of a
    ///   changed double-height or double-width character;
    /// - the rows whose full row colour changed;
    /// - the cells whose flash phase changed (in the resolved mode, the
    ///   flashing cells that look different in the two phases, as
    ///   REQ-RND-032 has it) and the concealed cells when reveal changed;
    /// - the cells any sprite covered at the last call or covers now:
    ///   `sprites` is `compose::SpritesDrawn::covered`, or `None` for a page
    ///   without sprites.
    ///
    /// It is the whole page on first use, and when the page-wide display
    /// state (boxed mode, mix, the Colour Map, the screen colour, the
    /// receiver's colours and row 24) or the way of drawing (cell mode,
    /// rounding, underline, colour mode) changed.
    pub fn damage(&mut self, grid: &Grid, options: RenderOptions, sprites: Option<CellRect>) -> Option<CellRect> {
        let damage = self.compare(grid, options);
        let damage = CellRect::join(CellRect::join(damage, self.sprites), sprites);
        self.last = *grid;
        self.options = Some(options);
        self.sprites = sprites;
        damage
    }

    /// The rectangle of the cells that differ from the grid last seen.
    fn compare(&self, grid: &Grid, options: RenderOptions) -> Option<CellRect> {
        let Some(before) = self.options else { return Some(CellRect::PAGE) };
        let last = &self.last;
        let same_drawing = (before.mode, before.rounding, before.underline)
            == (options.mode, options.rounding, options.underline)
            && matches!(before.colour, ColourMode::Palette) == matches!(options.colour, ColourMode::Palette);
        let same_page = (last.boxed_mode, last.mix, last.receiver_row_24)
            == (grid.boxed_mode, grid.mix, grid.receiver_row_24)
            && last.colour_map == grid.colour_map
            && last.receiver_colours == grid.receiver_colours
            && last.screen_colour == grid.screen_colour;
        if !same_drawing || !same_page {
            return Some(CellRect::PAGE);
        }
        let reveal_changed = before.reveal != options.reveal;
        let phases = match (before.colour, options.colour) {
            (ColourMode::Resolved(old), ColourMode::Resolved(new)) if old != new => Some((old, new)),
            _ => None,
        };
        let resolved = !matches!(options.colour, ColourMode::Palette);
        let mut damage = None;
        let mut add = |row: usize, column: usize| {
            let (Ok(row), Ok(column)) = (u8::try_from(row), u8::try_from(column)) else { return };
            if usize::from(row) < ROWS && usize::from(column) < COLUMNS {
                damage = CellRect::join(damage, Some(CellRect::cell(row, column)));
            }
        };
        for (row, (cells, old_cells)) in grid.cells.iter().zip(&last.cells).enumerate() {
            if grid.row_colours.get(row) != last.row_colours.get(row) {
                add(row, 0);
                add(row, COLUMNS.saturating_sub(1));
            }
            for (column, (cell, old)) in cells.iter().zip(old_cells).enumerate() {
                if cell != old {
                    add(row, column);
                    for (down, right) in [enlarged(cell), enlarged(old)] {
                        if down {
                            add(row.saturating_add(1), column);
                        }
                        if right {
                            add(row, column.saturating_add(1));
                        }
                    }
                    continue;
                }
                // The palette mode shows flash and reveal by its palette;
                // only a flashing concealed cell is drawn again (REQ-RND-023).
                let revealed = reveal_changed && cell.conceal && (resolved || cell.flash);
                let flashed = phases.is_some_and(|phases| {
                    flash_changes(cell, grid, row, column, phases, (options.reveal, options.underline))
                });
                if revealed || flashed {
                    add(row, column);
                }
            }
        }
        damage
    }
}

/// Renders the cells of `rect` alone into a whole-screen framebuffer laid
/// out as [`render_screen`](super::render_screen) lays it out: the damage
/// repaint of ANIM-4. Every other pixel of the framebuffer is left as it
/// is, so a framebuffer that held the page as last rendered holds, after
/// the rectangle [`DamageTracker::damage`] reported is drawn, exactly the
/// whole rendering of the page as it is now.
///
/// # Errors
/// [`RenderError::OutOfRange`] when `rect` reaches off the page,
/// [`RenderError::BufferTooSmall`] when the framebuffer does not hold the
/// rectangle's rows at its stride, and [`render_row`](super::render_row)'s
/// other errors for a row; rows before it are drawn, the rest are not.
pub fn render_rect(
    grid: &Grid,
    rect: CellRect,
    glyphs: &GlyphSet,
    options: RenderOptions,
    framebuffer: &mut IndexBuffer<'_>,
) -> Result<(), RenderError> {
    if usize::from(rect.last_row()) >= ROWS || usize::from(rect.last_column()) >= COLUMNS {
        return Err(RenderError::OutOfRange);
    }
    let height = options.row_height();
    for row in usize::from(rect.row)..=usize::from(rect.last_row()) {
        check_mode(grid, row, options)?;
        let cells = grid.cells.get(row).ok_or(RenderError::OutOfRange)?;
        for line in 0..height {
            let start = row.saturating_mul(height).saturating_add(line).saturating_mul(framebuffer.stride);
            let out = framebuffer.data.get_mut(start..).ok_or(RenderError::BufferTooSmall)?;
            let cells = cells.iter().enumerate().skip(usize::from(rect.column)).take(usize::from(rect.columns));
            for (column, cell) in cells {
                let raster = cell_line(cell, grid, row, column, line, glyphs, options);
                write_cell(out, column, raster, framebuffer.format)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
