//! The page as the encoder sees it: 80 by 75 sixels, two across and three
//! down in each of the 40 by 25 cells, each with its colour and the weight
//! its error counts by.
//!
//! The encoder's indices all come from this fixed grid and from palettes of
//! at most 32 colours, the Colour Map, never from the input, so the
//! modules that index it (`page`, `level1`, `enhanced`, `tti`) allow
//! indexing and arithmetic function by function, each with that reason. The
//! picture's own sizes, which do come from the input, are handled in
//! `decode`, `raster` and `fit` without either.

use neoteletext_core::consts::{
    CELL_HEIGHT, CELL_WIDTH, COLUMNS, MOSAIC_BIT5, MOSAIC_BIT6, MOSAIC_ROW_TENTHS, RECEIVER_COLUMNS, ROWS,
};

/// Sixels across a mosaic cell (the G1 set of EN 300 706: two across,
/// three down).
pub const SIXELS_ACROSS: usize = 2;
/// Sixels down a mosaic cell: one for each band of the core's mosaic rows.
pub const SIXELS_DOWN: usize = MOSAIC_ROW_TENTHS.len();
/// Sixels in a cell.
pub const SIXELS_PER_CELL: usize = SIXELS_ACROSS * SIXELS_DOWN;
/// Mosaic patterns of a cell, one bit a sixel.
pub const PATTERNS: usize = 1 << SIXELS_PER_CELL;
/// Sixels across the page.
pub const WIDTH: usize = COLUMNS * SIXELS_ACROSS;
/// Sixels down the page.
pub const HEIGHT: usize = ROWS * SIXELS_DOWN;
/// Sixels on the page.
pub const PIXELS: usize = WIDTH * HEIGHT;
/// Dots across a sixel as the engine draws a cell.
pub const SIXEL_DOTS: usize = CELL_WIDTH / SIXELS_ACROSS;
/// Tenths in a whole, the unit of [`MOSAIC_ROW_TENTHS`].
const TENTHS: usize = 10;
/// Lines of each band of sixels as the engine draws a 20-line cell: 6, 8
/// and 6.
#[allow(clippy::indexing_slicing, reason = "a constant, checked when it is compiled")]
pub const SIXEL_LINES: [usize; SIXELS_DOWN] = {
    let mut lines = [0; SIXELS_DOWN];
    let mut band = 0;
    while band < SIXELS_DOWN {
        lines[band] = MOSAIC_ROW_TENTHS[band] * CELL_HEIGHT / TENTHS;
        band += 1;
    }
    lines
};
const _: () = assert!(SIXEL_LINES[0] + SIXEL_LINES[1] + SIXEL_LINES[2] == CELL_HEIGHT);
/// The mosaic code's bit of each sixel, left to right and top to bottom:
/// bits 0 to 4, then bit 6, since bit 5 marks every mosaic code.
pub const SIXEL_BITS: [u8; SIXELS_PER_CELL] = [1, 1 << 1, 1 << 2, 1 << 3, 1 << 4, MOSAIC_BIT6];
const _: () = assert!(SIXEL_BITS[SIXELS_PER_CELL - 2] << 1 == MOSAIC_BIT5);

/// A colour, each primary 0 to 255, signed so that differences need no
/// casts.
pub type Rgb = [i64; 3];

/// The picture as sixels, with the weights and cells the encoder reads.
pub struct Picture {
    /// 0, or 1 when row 0 is left to the page's header.
    pub first_row: usize,
    /// One past the last row drawn: 25, or 24 when row 24 is left to the
    /// page (PAR-11).
    pub end_row: usize,
    /// Each sixel's colour, row by row.
    pub pixels: Vec<Rgb>,
    /// How much each sixel's error counts: its height in lines, and nothing
    /// for a sixel the page does not draw.
    pub weights: Vec<i64>,
    /// Each cell's six sixels, as indices into `pixels`.
    pub cells: Vec<[usize; SIXELS_PER_CELL]>,
}

impl Picture {
    /// The picture of `pixels`, which hold [`PIXELS`] colours, drawn on
    /// rows `first_row` to `end_row` (exclusive).
    pub fn with_rows(pixels: Vec<Rgb>, first_row: usize, end_row: usize) -> Self {
        Self { first_row, end_row, pixels, weights: weights(first_row, end_row), cells: cell_pixels() }
    }

    /// The sixels of the cell at `row` and `column`.
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "row and column are within the page grid"
    )]
    pub fn cell(&self, row: usize, column: usize) -> &[usize; SIXELS_PER_CELL] {
        &self.cells[row * COLUMNS + column]
    }
}

/// The first column the page may draw in `row`: row 0's first eight belong
/// to the receiver (EN 300 706 clause 9.3.1).
pub const fn first_column(row: usize) -> usize {
    if row == 0 { RECEIVER_COLUMNS } else { 0 }
}

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "indices within the fixed sixel grid")]
fn weights(first_row: usize, end_row: usize) -> Vec<i64> {
    let mut weights = vec![0; PIXELS];
    for (index, weight) in weights.iter_mut().enumerate() {
        let drawn = index / WIDTH / SIXELS_DOWN < end_row;
        *weight = if drawn { i64::try_from(SIXEL_LINES[index / WIDTH % SIXELS_DOWN]).unwrap_or(0) } else { 0 };
    }
    let reserved = if first_row == 0 { RECEIVER_COLUMNS * SIXELS_ACROSS } else { WIDTH };
    for line in 0..SIXELS_DOWN {
        for x in 0..reserved {
            weights[line * WIDTH + x] = 0;
        }
    }
    weights
}

#[allow(clippy::arithmetic_side_effects, reason = "indices within the fixed sixel grid")]
fn cell_pixels() -> Vec<[usize; SIXELS_PER_CELL]> {
    let mut cells = Vec::with_capacity(ROWS * COLUMNS);
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            cells.push(core::array::from_fn(|k| {
                (row * SIXELS_DOWN + k / SIXELS_ACROSS) * WIDTH + column * SIXELS_ACROSS + k % SIXELS_ACROSS
            }));
        }
    }
    cells
}

/// The mosaic code of a pattern, bit k set for sixel k.
pub fn sextant_code(pattern: usize) -> u8 {
    SIXEL_BITS.iter().enumerate().filter(|&(k, _)| pattern >> k & 1 == 1).fold(MOSAIC_BIT5, |code, (_, bit)| code | bit)
}

/// A colour as eight bits a primary.
pub fn to_rgb8(colour: Rgb) -> crate::Rgb8 {
    colour.map(|primary| u8::try_from(primary.clamp(0, i64::from(u8::MAX))).unwrap_or(u8::MAX))
}

impl Picture {
    /// The page's two views of the picture, as [`crate::Page`] holds them:
    /// the sixels aimed at, and those drawn, black where nothing is drawn.
    pub fn views(&self, shown: &[Rgb]) -> (Vec<crate::Rgb8>, Vec<crate::Rgb8>) {
        let picture = self.pixels.iter().map(|&colour| to_rgb8(colour)).collect();
        let drawn = shown
            .iter()
            .zip(&self.weights)
            .map(|(&colour, &weight)| if weight > 0 { to_rgb8(colour) } else { [0; 3] })
            .collect();
        (picture, drawn)
    }
}

/// The squared distance between two colours.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "primaries are 0 to 255, so squares sum far below the integer's range"
)]
pub fn distance(first: Rgb, second: Rgb) -> i64 {
    first.iter().zip(second).map(|(a, b)| (a - b) * (a - b)).sum()
}
