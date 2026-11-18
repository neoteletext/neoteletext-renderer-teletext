//! Levels 1 and 1.5 (EN 300 706 clause 12.2): the eight colours of CLUT 0,
//! every change a spacing attribute that takes a cell. Graphics colour is
//! set-after; New Background and Black Background are set-at. The cell an
//! attribute takes shows the background, or with Hold Mosaics the row's last
//! mosaic (clause 12.2, Table 26).
//!
//! Along each row a dynamic programme chooses, cell by cell, a mosaic or an
//! attribute, keeping for every foreground, background and held pattern
//! the least error so far.

use neoteletext_core::consts::{
    BLACK_BACKGROUND, CLUT_ENTRIES, COLOUR_BLACK, COLOUR_WHITE, COLUMNS, HOLD_MOSAICS, MOSAIC_BLACK, NEW_BACKGROUND,
    PRIMARY_FULL, ROWS, SPACE,
};

use crate::Page;
use crate::page::{PATTERNS, PIXELS, Picture, Rgb, SIXELS_PER_CELL, distance, first_column, sextant_code};
use crate::tti;

/// A cost no path reaches.
const INFINITY: i64 = 1 << 60;
const BLACK: usize = COLOUR_BLACK as usize;
const WHITE: usize = COLOUR_WHITE as usize;

/// One cell as encoded: its code, and the pattern and colours it shows.
struct Cell {
    code: u8,
    pattern: usize,
    foreground: usize,
    background: usize,
}

/// What a cell was in the best path to a state.
#[derive(Clone, Copy)]
enum Action {
    Mosaic,
    Graphics,
    NewBackground,
    BlackBackground,
}

/// The step back from a state: the action, and the state before it.
#[derive(Clone, Copy)]
struct Pointer {
    action: Action,
    foreground: usize,
    background: usize,
    held: usize,
}

/// The colour of CLUT 0 entry `entry`: bit 0 red, bit 1 green, bit 2 blue
/// (Table 26), each primary full or off.
pub fn clut_0(entry: usize) -> Rgb {
    let primary = |bit: usize| if entry >> bit & 1 == 1 { i64::from(PRIMARY_FULL) } else { 0 };
    [primary(0), primary(1), primary(2)]
}

/// The page of `picture` at Level 1, with Hold Mosaics when `hold`.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    clippy::similar_names,
    reason = "states and cells within the fixed page and the eight colours; one dynamic programme"
)]
pub fn encode(picture: &Picture, hold: bool) -> Page {
    let palette: Vec<Rgb> = (0..CLUT_ENTRIES).map(clut_0).collect();
    let count = palette.len();
    let held = if hold { PATTERNS } else { 1 };
    let at = |f: usize, b: usize, h: usize| (f * count + b) * held + h;
    let mut cost = vec![0i64; PIXELS * count];
    for pixel in 0..PIXELS {
        for (colour, &rgb) in palette.iter().enumerate() {
            cost[pixel * count + colour] = distance(picture.pixels[pixel], rgb) * picture.weights[pixel];
        }
    }
    let mut rows: Vec<Vec<Cell>> = Vec::with_capacity(ROWS);
    for row in picture.first_row..picture.end_row {
        let start = first_column(row);
        // The row opens on Graphics colour, and with Hold Mosaics on that
        // attribute first: those cells show black.
        let first_free = start + if hold { 2 } else { 1 };
        let lead: i64 = (start..first_free)
            .map(|column| picture.cell(row, column).iter().map(|&p| cost[p * count + BLACK]).sum::<i64>())
            .sum();
        let mut current = vec![INFINITY; count * count * held];
        for colour in 1..CLUT_ENTRIES {
            current[at(colour, BLACK, 0)] = lead;
        }
        let mut back: Vec<(Vec<Pointer>, Vec<usize>)> = Vec::new();
        for column in first_free..COLUMNS {
            let cell = picture.cell(row, column);
            let d = |s: usize, colour: usize| cost[cell[s] * count + colour];
            let mut mosaic_cost = vec![0i64; count * count];
            let mut mosaic_pattern = vec![0usize; count * count];
            for f in 0..count {
                for b in 0..count {
                    for s in 0..SIXELS_PER_CELL {
                        mosaic_cost[f * count + b] += d(s, f).min(d(s, b));
                        if d(s, f) < d(s, b) {
                            mosaic_pattern[f * count + b] |= 1 << s;
                        }
                    }
                }
            }
            let shown = |f: usize, b: usize, h: usize| -> i64 {
                (0..SIXELS_PER_CELL).map(|s| if h >> s & 1 == 1 { d(s, f) } else { d(s, b) }).sum()
            };
            let mut new = vec![INFINITY; count * count * held];
            let start_pointer = Pointer { action: Action::Mosaic, foreground: 0, background: 0, held: 0 };
            let mut pointer = vec![start_pointer; count * count * held];
            // A mosaic: any held pattern before, this cell's best after.
            for f in 0..count {
                for b in 0..count {
                    let (mut best, mut from) = (current[at(f, b, 0)], 0);
                    for h in 1..held {
                        if current[at(f, b, h)] < best {
                            best = current[at(f, b, h)];
                            from = h;
                        }
                    }
                    let h = if hold { mosaic_pattern[f * count + b] } else { 0 };
                    let value = best + mosaic_cost[f * count + b];
                    if value < new[at(f, b, h)] {
                        new[at(f, b, h)] = value;
                        pointer[at(f, b, h)] =
                            Pointer { action: Action::Mosaic, foreground: f, background: b, held: from };
                    }
                }
            }
            // Graphics colour c, set-after: the cell shows the old colours.
            for b in 0..count {
                for h in 0..held {
                    let (mut best, mut from) = (INFINITY + INFINITY, 0);
                    for f in 0..count {
                        let value = current[at(f, b, h)] + shown(f, b, h);
                        if value < best {
                            best = value;
                            from = f;
                        }
                    }
                    for c in 1..CLUT_ENTRIES {
                        if best < new[at(c, b, h)] {
                            new[at(c, b, h)] = best;
                            pointer[at(c, b, h)] =
                                Pointer { action: Action::Graphics, foreground: from, background: b, held: h };
                        }
                    }
                }
            }
            // New Background, set-at: the background becomes the
            // foreground; then Black Background, set-at.
            for (action, black) in [(Action::NewBackground, false), (Action::BlackBackground, true)] {
                for f in 0..count {
                    let target = if black { BLACK } else { f };
                    for h in 0..held {
                        let (mut best, mut from) = (INFINITY + INFINITY, 0);
                        let extra = shown(f, target, h);
                        for b in 0..count {
                            let value = current[at(f, b, h)] + extra;
                            if value < best {
                                best = value;
                                from = b;
                            }
                        }
                        if best < new[at(f, target, h)] {
                            new[at(f, target, h)] = best;
                            pointer[at(f, target, h)] = Pointer { action, foreground: f, background: from, held: h };
                        }
                    }
                }
            }
            back.push((pointer, mosaic_pattern));
            current = new;
        }
        let mut best = 0;
        for index in 1..current.len() {
            if current[index] < current[best] {
                best = index;
            }
        }
        let (mut f, mut b, mut h) = (best / held / count, best / held % count, best % held);
        let mut out: Vec<Cell> = Vec::new();
        for (pointer, mosaic_pattern) in back.iter().rev() {
            let step = pointer[at(f, b, h)];
            out.push(match step.action {
                Action::Mosaic => {
                    let pattern = mosaic_pattern[f * count + b];
                    Cell { code: sextant_code(pattern), pattern, foreground: f, background: b }
                }
                Action::Graphics => Cell {
                    code: MOSAIC_BLACK + f as u8,
                    pattern: step.held,
                    foreground: step.foreground,
                    background: step.background,
                },
                Action::NewBackground => {
                    Cell { code: NEW_BACKGROUND, pattern: step.held, foreground: f, background: f }
                }
                Action::BlackBackground => {
                    Cell { code: BLACK_BACKGROUND, pattern: step.held, foreground: f, background: BLACK }
                }
            });
            (f, b, h) = (step.foreground, step.background, step.held);
        }
        let mut cells = Vec::with_capacity(COLUMNS);
        if hold {
            cells.push(Cell { code: HOLD_MOSAICS, pattern: 0, foreground: WHITE, background: BLACK });
        }
        cells.push(Cell { code: MOSAIC_BLACK + f as u8, pattern: 0, foreground: WHITE, background: BLACK });
        cells.extend(out.into_iter().rev());
        rows.push(cells);
    }
    let codes: Vec<Vec<u8>> = rows.iter().map(|cells| cells.iter().map(|cell| cell.code).collect()).collect();
    let mut shown = vec![[0i64; 3]; PIXELS];
    for (index, cells) in rows.iter().enumerate() {
        let row = index + picture.first_row;
        for (offset, cell) in cells.iter().enumerate() {
            for (k, &pixel) in picture.cell(row, first_column(row) + offset).iter().enumerate() {
                let foreground = cell.pattern >> k & 1 == 1;
                shown[pixel] = palette[if foreground { cell.foreground } else { cell.background }];
            }
        }
    }
    let attributes = rows.iter().flatten().filter(|cell| cell.code < SPACE).count();
    let (pixels, drawn) = picture.views(&shown);
    Page {
        tti: tti::text(&tti::level_1_lines(&codes, picture.first_row)),
        summary: format!("{CLUT_ENTRIES} colours, {attributes} attribute cells"),
        mean_squared_error: crate::enhanced::mean_squared_error(picture, &shown),
        picture: pixels,
        drawn,
    }
}
