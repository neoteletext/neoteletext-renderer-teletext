//! Rectangles of cells and of dots: what a renderer says changed
//! (animation.md ANIM-4) and where an object lies (ANIM-13).

use crate::consts::{CELL_HEIGHT, CELL_WIDTH, COLUMNS, ROWS};

/// A rectangle of whole cells on the page, never empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct CellRect {
    /// Its top row, 0 to 24.
    pub row: u8,
    /// Its first column, 0 to 39.
    pub column: u8,
    /// Rows down, at least 1.
    pub rows: u8,
    /// Cells across, at least 1.
    pub columns: u8,
}

impl CellRect {
    /// The whole page.
    pub const PAGE: Self = Self { row: 0, column: 0, rows: ROWS as u8, columns: COLUMNS as u8 };

    /// One cell.
    #[must_use]
    pub const fn cell(row: u8, column: u8) -> Self {
        Self { row, column, rows: 1, columns: 1 }
    }

    /// The cells of `first..=last` rows and columns that lie on the page,
    /// given as signed positions so that a rectangle partly off the page
    /// is cut to it; `None` when none of it is on the page.
    #[must_use]
    pub fn cut(top: i32, left: i32, bottom: i32, right: i32) -> Option<Self> {
        Self::cut_to(top, left, bottom, right, Self::PAGE)
    }

    /// As [`CellRect::cut`], cut to `bounds` in place of the page.
    #[must_use]
    pub fn cut_to(top: i32, left: i32, bottom: i32, right: i32, bounds: Self) -> Option<Self> {
        let top = top.max(i32::from(bounds.row));
        let left = left.max(i32::from(bounds.column));
        let bottom = bottom.min(i32::from(bounds.last_row()));
        let right = right.min(i32::from(bounds.last_column()));
        if top > bottom || left > right {
            return None;
        }
        Some(Self {
            row: u8::try_from(top).ok()?,
            column: u8::try_from(left).ok()?,
            rows: u8::try_from(bottom.saturating_sub(top).saturating_add(1)).ok()?,
            columns: u8::try_from(right.saturating_sub(left).saturating_add(1)).ok()?,
        })
    }

    /// Its bottom row.
    #[must_use]
    pub const fn last_row(self) -> u8 {
        self.row.saturating_add(self.rows.saturating_sub(1))
    }

    /// Its last column.
    #[must_use]
    pub const fn last_column(self) -> u8 {
        self.column.saturating_add(self.columns.saturating_sub(1))
    }

    /// Whether the cell at `row`, `column` is one of its cells.
    #[must_use]
    pub const fn contains(self, row: usize, column: usize) -> bool {
        row >= self.row as usize
            && row <= self.last_row() as usize
            && column >= self.column as usize
            && column <= self.last_column() as usize
    }

    /// The smallest rectangle holding this one and `other`.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        let row = self.row.min(other.row);
        let column = self.column.min(other.column);
        let last_row = self.last_row().max(other.last_row());
        let last_column = self.last_column().max(other.last_column());
        Self {
            row,
            column,
            rows: last_row.saturating_sub(row).saturating_add(1),
            columns: last_column.saturating_sub(column).saturating_add(1),
        }
    }

    /// The union of two rectangles either of which may be absent.
    #[must_use]
    pub fn join(one: Option<Self>, other: Option<Self>) -> Option<Self> {
        match (one, other) {
            (Some(one), Some(other)) => Some(one.union(other)),
            (one, other) => one.or(other),
        }
    }

    /// Its cells.
    #[must_use]
    pub const fn area(self) -> u32 {
        (self.rows as u32).saturating_mul(self.columns as u32)
    }

    /// The rectangle in dots, 12 by 20 a cell (ANIM-8).
    #[must_use]
    pub const fn dots(self) -> DotRect {
        self.dots_of(CELL_HEIGHT)
    }

    /// The rectangle in dots for cells `cell_height` lines tall: the
    /// renderer's row height in the cell mode it draws
    /// (`RenderOptions::row_height`).
    #[must_use]
    pub const fn dots_of(self, cell_height: usize) -> DotRect {
        let height = cell_height as u16;
        DotRect {
            x: (self.column as u16).saturating_mul(CELL_WIDTH as u16),
            y: (self.row as u16).saturating_mul(height),
            width: (self.columns as u16).saturating_mul(CELL_WIDTH as u16),
            height: (self.rows as u16).saturating_mul(height),
        }
    }
}

/// A rectangle of the page's picture in dots, the origin at the top left
/// and y down: what collision and region tests are made on (ANIM-13).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct DotRect {
    /// Its left edge.
    pub x: u16,
    /// Its top edge.
    pub y: u16,
    /// Dots across.
    pub width: u16,
    /// Lines down.
    pub height: u16,
}

impl DotRect {
    /// Its dots.
    #[must_use]
    pub const fn area(self) -> u32 {
        (self.width as u32).saturating_mul(self.height as u32)
    }

    /// Whether the dot at `x`, `y` is inside it.
    #[must_use]
    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && y >= self.y && x.wrapping_sub(self.x) < self.width && y.wrapping_sub(self.y) < self.height
    }

    /// Whether it and `other` share a dot.
    #[must_use]
    pub const fn overlaps(self, other: Self) -> bool {
        self.x < other.x.saturating_add(other.width)
            && other.x < self.x.saturating_add(self.width)
            && self.y < other.y.saturating_add(other.height)
            && other.y < self.y.saturating_add(self.height)
    }
}
