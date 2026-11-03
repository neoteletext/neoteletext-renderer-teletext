//! Levels 2.5 and 3.5 (EN 300 706 clause 12.4, Tables 4, 10, 27 and 30).
//!
//! The colours are redefined: by X/28/0 format 1 (CLUTs 2 and 3) and, at
//! Level 3.5, X/28/4 (CLUTs 0 and 1); CLUT 1 entry 0 stays transparent. At
//! Level 2.5 the sixteen colours of CLUTs 0 and 1 stay as Table 30 gives
//! them and sixteen are fitted; at Level 3.5 CLUT 0 entries 0 and 7 stay
//! black and white for the header, and the other 29 are fitted.
//!
//! Colour changes are non-spacing X/26 attributes, so every column is
//! picture: the first column holds Graphics colour with an X/26 mosaic over
//! it, and each row starts on its Full Row Colour, with Black Background
//! Colour Substitution set. Row 0 is reached by Address Display Row 0
//! (Table 28).
//!
//! The palette is fitted to the two-colour rule: encode, move each fitted
//! colour to the mean of the sixels drawn in it, rounded to the twelve-bit
//! colours, and repeat until nothing changes. It starts from three
//! reductions of the picture and keeps the best; after each fit a colour
//! split from the worst is added while that lowers the error. Without
//! objects the palette is refitted at the change cost the page's own 208
//! triplets allow.

use neoteletext_core::consts::{
    CLUT_ENTRIES, COLOUR_BLACK, COLOUR_MAP_DEFAULTS, COLOUR_MAP_ENTRIES, COLOUR_PRIMARY_BITS, COLOUR_TRANSPARENT,
    COLOUR_WHITE, COLUMNS, MODE_ADDRESS_ROW_0, MODE_BACKGROUND_COLOUR, MODE_FOREGROUND_COLOUR, MODE_FULL_ROW_COLOUR,
    MODE_G1, MOSAIC_BLACK, PRIMARY_4_TO_8, ROW_ADDRESS_LAST, ROW_ADDRESS_ROW_24, ROWS,
};

use crate::error::Error;
use crate::options::{Level, Options};
use crate::page::{PIXELS, Picture, Rgb, distance, first_column, sextant_code};
use crate::quant::{self, Px};
use crate::tti::{self, Enhanced as Lines, LOCAL_DATA_TRIPLETS};
use crate::{Page, raster::real};

/// The highest level of a primary in a colour word: four bits.
const LEVEL_MAX: i64 = (1 << COLOUR_PRIMARY_BITS) - 1;
/// One step of a twelve-bit primary in eight bits: 17, so 15 is 255.
const LEVEL_STEP: i64 = PRIMARY_4_TO_8 as i64;
const BLACK: usize = COLOUR_BLACK as usize;
const WHITE: usize = COLOUR_WHITE as usize;
const TRANSPARENT: usize = COLOUR_TRANSPARENT as usize;
/// The cost of one colour change in the first fits: the least, so the
/// fit is led by the picture.
const CHANGE_COST: i64 = 1;
/// Fit rounds before the palette is taken as it stands; measured on the
/// test pictures, no fit took more than a dozen.
const MAX_ROUNDS: usize = 20;
/// Colours split from the worst before refining stops.
const MAX_SPLITS: usize = 8;
/// Refits at the page's triplet budget.
const BUDGET_ROUNDS: usize = 4;
/// The highest change cost the budget's search tries: above any total
/// error a row can have.
const MAX_CHANGE_COST: i64 = 1 << 24;

/// The twelve-bit primary nearest `value`, in eight bits.
#[allow(clippy::cast_possible_truncation, reason = "the level is clamped to 0 to 15 first")]
fn to_12_bit_channel(value: f64) -> i64 {
    ((value / real(LEVEL_STEP as usize)).round_ties_even().clamp(0.0, real(LEVEL_MAX as usize)) as i64)
        .saturating_mul(LEVEL_STEP)
}

fn to_12_bit(colour: [f64; 3]) -> Rgb {
    colour.map(to_12_bit_channel)
}

fn unique(mut colours: Vec<Rgb>) -> Vec<Rgb> {
    colours.sort_unstable();
    colours.dedup();
    colours
}

/// The levels, 0 to 15, of Colour Map entry `slot` as Table 30 defines it.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "slots within the Colour Map, and shifts of under twelve bits"
)]
fn default_levels(slot: usize) -> Rgb {
    let word = i64::from(COLOUR_MAP_DEFAULTS[slot]);
    let primary = |place: u32| word >> (place * COLOUR_PRIMARY_BITS) & LEVEL_MAX;
    [primary(2), primary(1), primary(0)]
}

/// The palette: fixed and fitted colours, and the Colour Map slot each
/// takes.
#[derive(Clone)]
struct Palette {
    /// The fixed colours: each slot and its levels.
    fixed: Vec<(usize, Rgb)>,
    /// The slots fitted colours may take.
    redefinable: Vec<usize>,
    fitted: Vec<Rgb>,
    /// The fixed colours, then the fitted, in eight bits.
    colours: Vec<Rgb>,
    /// Each colour's slot, by colour.
    slot: Vec<usize>,
    /// The colours that hold a slot, in the order they took it.
    entries: Vec<usize>,
    /// Whether a colour's slot is in CLUT 0 but black, so a Graphics
    /// colour code can name it.
    is_free: Vec<bool>,
    /// The colour a row's Graphics colour code names when its own cannot.
    first_free: usize,
    /// White, the colour a row starts in.
    start: usize,
}

impl Palette {
    #[allow(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "colours and slots within the Colour Map"
    )]
    fn new(fixed: &[(usize, Rgb)], fitted: Vec<Rgb>, redefinable: &[usize], use_counts: Option<&[i64]>) -> Self {
        let mut colours: Vec<Rgb> = fixed.iter().map(|(_, levels)| levels.map(|v| v * LEVEL_STEP)).collect();
        colours.extend_from_slice(&fitted);
        let count = colours.len();
        let mut slot = vec![usize::MAX; count];
        let mut entries: Vec<usize> = Vec::with_capacity(count);
        for (entry, (s, _)) in fixed.iter().enumerate() {
            slot[entry] = *s;
            entries.push(entry);
        }
        let mut order: Vec<usize> = (0..count).collect();
        if let Some(use_counts) = use_counts {
            order.sort_by_key(|&p| -use_counts[p]);
        }
        order.retain(|&p| p >= fixed.len());
        // The most used fitted colours take white's slot and the Graphics
        // colour slots first, so a row's first column needs no triplet.
        let mut wanted: Vec<usize> = Vec::with_capacity(redefinable.len());
        for s in [WHITE].into_iter().chain(1..CLUT_ENTRIES).chain([BLACK]).chain(redefinable.iter().copied()) {
            if redefinable.contains(&s) && !wanted.contains(&s) {
                wanted.push(s);
            }
        }
        for (&entry, &s) in order.iter().zip(wanted.iter()) {
            slot[entry] = s;
            entries.push(entry);
        }
        let free: Vec<usize> = entries.iter().copied().filter(|&p| (1..CLUT_ENTRIES).contains(&slot[p])).collect();
        let mut is_free = vec![false; count];
        for &p in &free {
            is_free[p] = true;
        }
        let start = entries.iter().copied().find(|&p| slot[p] == WHITE).unwrap_or(0);
        Self {
            fixed: fixed.to_vec(),
            redefinable: redefinable.to_vec(),
            fitted,
            colours,
            slot,
            entries,
            is_free,
            first_free: free.first().copied().unwrap_or(start),
            start,
        }
    }

    fn with_use(&self, use_counts: &[i64]) -> Self {
        Self::new(&self.fixed, self.fitted.clone(), &self.redefinable, Some(use_counts))
    }

    fn with_fitted(&self, fitted: Vec<Rgb>) -> Self {
        Self::new(&self.fixed, fitted, &self.redefinable, None)
    }

    /// The Colour Map's levels.
    #[allow(clippy::indexing_slicing, reason = "colours and slots within the Colour Map")]
    fn colour_map(&self) -> [Rgb; COLOUR_MAP_ENTRIES] {
        let mut map = [[0; 3]; COLOUR_MAP_ENTRIES];
        for &entry in &self.entries {
            map[self.slot[entry]] = self.colours[entry].map(|v| v / LEVEL_STEP);
        }
        map
    }
}

/// An encoding: each row's foreground and background per cell from its
/// first column, and whether the first cell holds a mosaic.
#[derive(Clone)]
struct Encoded {
    rows: Vec<(Vec<(usize, usize)>, bool)>,
    distance: Vec<i64>,
    count: usize,
}

#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "states and cells within the fixed page and a palette of at most 32 colours; one dynamic programme"
)]
fn encode_rows(picture: &Picture, palette: &Palette, change_cost: i64) -> Encoded {
    let colours = &palette.colours;
    let count = colours.len();
    let mut cost_of = vec![0i64; PIXELS * count];
    for pixel in 0..PIXELS {
        for (colour, &rgb) in colours.iter().enumerate() {
            cost_of[pixel * count + colour] = distance(picture.pixels[pixel], rgb) * picture.weights[pixel];
        }
    }
    let mut cost = vec![0i64; ROWS * COLUMNS * count * count];
    for cell in 0..ROWS * COLUMNS {
        let base = cell * count * count;
        let pixels = &picture.cells[cell];
        for f in 0..count {
            for b in 0..count {
                cost[base + f * count + b] =
                    pixels.iter().map(|&pixel| cost_of[pixel * count + f].min(cost_of[pixel * count + b])).sum();
            }
        }
    }
    let mut rows = Vec::with_capacity(ROWS);
    for row in picture.first_row..ROWS {
        let start = first_column(row);
        let cost_at =
            |column: usize, f: usize, b: usize| cost[(row * COLUMNS + column) * count * count + f * count + b];
        let mut space = vec![0i64; count];
        for &pixel in picture.cell(row, start) {
            for (b, total) in space.iter_mut().enumerate() {
                *total += cost_of[pixel * count + b];
            }
        }
        let mut current = vec![0i64; count * count];
        for f in 0..count {
            let start_cost = if f == palette.start { 0 } else { change_cost };
            for b in 0..count {
                current[f * count + b] = (cost_at(start, f, b) + change_cost).min(space[b]) + start_cost;
            }
        }
        let mut back: Vec<(Vec<usize>, Vec<usize>)> = Vec::with_capacity(COLUMNS);
        let mut best_for_b = vec![0i64; count];
        let mut from_f = vec![0usize; count];
        let mut best_for_f = vec![0i64; count];
        let mut from_b = vec![0usize; count];
        for column in start + 1..COLUMNS {
            best_for_b.fill(i64::MAX);
            best_for_f.fill(i64::MAX);
            let (mut best, mut best_pair) = (i64::MAX, (0, 0));
            for f in 0..count {
                for b in 0..count {
                    let value = current[f * count + b];
                    if value < best_for_b[b] {
                        best_for_b[b] = value;
                        from_f[b] = f;
                    }
                    if value < best_for_f[f] {
                        best_for_f[f] = value;
                        from_b[f] = b;
                    }
                    if value < best {
                        best = value;
                        best_pair = (f, b);
                    }
                }
            }
            let mut previous_f = vec![0usize; count * count];
            let mut previous_b = vec![0usize; count * count];
            let mut next = vec![0i64; count * count];
            for f in 0..count {
                for b in 0..count {
                    let at = f * count + b;
                    let value;
                    if column == start + 1 {
                        // The second column's foreground is the Graphics
                        // colour code's, free when its slot is in CLUT 0.
                        let set_f = if palette.is_free[f] { 0 } else { change_cost };
                        let first = best_for_b[b] + set_f;
                        let second = best + set_f + change_cost;
                        if second < first {
                            (previous_f[at], previous_b[at]) = best_pair;
                            value = second;
                        } else {
                            (previous_f[at], previous_b[at]) = (from_f[b], b);
                            value = first;
                        }
                    } else {
                        let options = [
                            (current[at], f, b),
                            (best_for_b[b] + change_cost, from_f[b], b),
                            (best_for_f[f] + change_cost, f, from_b[f]),
                            (best + 2 * change_cost, best_pair.0, best_pair.1),
                        ];
                        let mut chosen = options[0];
                        for option in &options[1..] {
                            if option.0 < chosen.0 {
                                chosen = *option;
                            }
                        }
                        (value, previous_f[at], previous_b[at]) = chosen;
                    }
                    next[at] = value + cost_at(column, f, b);
                }
            }
            back.push((previous_f, previous_b));
            current = next;
        }
        let mut best = 0;
        for index in 1..current.len() {
            if current[index] < current[best] {
                best = index;
            }
        }
        let (mut f, mut b) = (best / count, best % count);
        let mut states = vec![(f, b)];
        for (previous_f, previous_b) in back.iter().rev() {
            let at = f * count + b;
            (f, b) = (previous_f[at], previous_b[at]);
            states.push((f, b));
        }
        states.reverse();
        let (f0, b0) = states[0];
        let character = cost_at(start, f0, b0) + change_cost < space[b0];
        rows.push((states, character));
    }
    Encoded { rows, distance: cost_of, count }
}

/// The colour each sixel is drawn in.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "cells within the fixed page")]
fn drawn(picture: &Picture, encoded: &Encoded) -> Vec<usize> {
    let count = encoded.count;
    let mut out = vec![0usize; PIXELS];
    for (index, (states, character)) in encoded.rows.iter().enumerate() {
        let row = index + picture.first_row;
        let start = first_column(row);
        for (offset, &(f, b)) in states.iter().enumerate() {
            let column = start + offset;
            for &pixel in picture.cell(row, column) {
                out[pixel] = if column == start && !character {
                    b
                } else if encoded.distance[pixel * count + f] < encoded.distance[pixel * count + b] {
                    f
                } else {
                    b
                };
            }
        }
    }
    out
}

/// How many rows use each colour as their second column's foreground.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "colours within the palette")]
fn column_1_use(encoded: &Encoded) -> Vec<i64> {
    let mut use_counts = vec![0; encoded.count];
    for (states, _) in &encoded.rows {
        if let Some(&(f, _)) = states.get(1) {
            use_counts[f] += 1;
        }
    }
    use_counts
}

/// Each row's Level 1 codes and its triplets.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "rows, columns and slots within the page and the Colour Map"
)]
fn content(picture: &Picture, palette: &Palette, encoded: &Encoded) -> (Vec<Vec<u8>>, Vec<Vec<u32>>) {
    let count = encoded.count;
    let pattern = |row: usize, column: usize| -> usize {
        let (f, b) = encoded.rows[row - picture.first_row].0[column - first_column(row)];
        picture
            .cell(row, column)
            .iter()
            .enumerate()
            .filter(|&(_, &pixel)| encoded.distance[pixel * count + f] < encoded.distance[pixel * count + b])
            .map(|(k, _)| 1 << k)
            .sum()
    };
    let slot = &palette.slot;
    let mut level_1 = Vec::with_capacity(ROWS);
    let mut row_triplets = Vec::with_capacity(ROWS);
    for (index, (states, character)) in encoded.rows.iter().enumerate() {
        let row = index + picture.first_row;
        let start = first_column(row);
        let (f0, b0) = states[0];
        let mut out = Vec::new();
        if row == 0 {
            out.push(tti::triplet(u32::from(ROW_ADDRESS_LAST), u32::from(MODE_ADDRESS_ROW_0), slot[b0] as u32));
        } else {
            // Row 24 is address 40, rows 1 to 23 are 41 to 63 (Table 28).
            let address = if row == ROWS - 1 { ROW_ADDRESS_ROW_24 } else { ROW_ADDRESS_ROW_24 + row as u8 };
            out.push(tti::triplet(u32::from(address), u32::from(MODE_FULL_ROW_COLOUR), slot[b0] as u32));
        }
        if f0 != palette.start {
            out.push(tti::triplet(start as u32, u32::from(MODE_FOREGROUND_COLOUR), slot[f0] as u32));
        }
        if *character {
            out.push(tti::triplet(start as u32, u32::from(MODE_G1), u32::from(sextant_code(pattern(row, start)))));
        }
        let mut code = 0;
        let (mut previous_f, mut previous_b) = (f0, b0);
        for column in start + 1..COLUMNS {
            let (f, b) = states[column - start];
            if column == start + 1 {
                if palette.is_free[f] {
                    code = MOSAIC_BLACK + slot[f] as u8;
                } else {
                    code = MOSAIC_BLACK + slot[palette.first_free] as u8;
                    out.push(tti::triplet(column as u32, u32::from(MODE_FOREGROUND_COLOUR), slot[f] as u32));
                }
            } else if f != previous_f {
                out.push(tti::triplet(column as u32, u32::from(MODE_FOREGROUND_COLOUR), slot[f] as u32));
            }
            if b != previous_b {
                out.push(tti::triplet(column as u32, u32::from(MODE_BACKGROUND_COLOUR), slot[b] as u32));
            }
            (previous_f, previous_b) = (f, b);
        }
        row_triplets.push(out);
        let mut codes = vec![code];
        codes.extend((start + 1..COLUMNS).map(|column| sextant_code(pattern(row, column))));
        level_1.push(codes);
    }
    (level_1, row_triplets)
}

/// The weighted mean of the sixels drawn in `entry`.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::needless_range_loop,
    reason = "sums of the fixed page's weighted sixels, far below 2^53"
)]
fn weighted_centre(picture: &Picture, who: &[usize], entry: usize) -> Option<[f64; 3]> {
    let mut total = [0i64; 3];
    let mut weight = 0i64;
    for pixel in 0..PIXELS {
        if who[pixel] == entry && picture.weights[pixel] > 0 {
            for i in 0..3 {
                total[i] += picture.pixels[pixel][i] * picture.weights[pixel];
            }
            weight += picture.weights[pixel];
        }
    }
    (weight > 0).then(|| total.map(|t| t as f64 / weight as f64))
}

/// The palette fitted from `start` to the two-colour rule.
fn fit(picture: &Picture, start: Palette, change_cost: i64) -> (Palette, Encoded) {
    let fixed_count = start.fixed.len();
    let mut palette = start;
    for _ in 0..MAX_ROUNDS {
        let encoded = encode_rows(picture, &palette, change_cost);
        palette = palette.with_use(&column_1_use(&encoded));
        let encoded = encode_rows(picture, &palette, change_cost);
        let who = drawn(picture, &encoded);
        let mut moved = palette.fitted.clone();
        for (index, colour) in moved.iter_mut().enumerate() {
            if let Some(centre) = weighted_centre(picture, &who, fixed_count.saturating_add(index)) {
                *colour = to_12_bit(centre);
            }
        }
        let moved = unique(moved);
        if moved == palette.fitted {
            return (palette, encoded);
        }
        palette = palette.with_fitted(moved);
    }
    let encoded = encode_rows(picture, &palette, change_cost);
    palette = palette.with_use(&column_1_use(&encoded));
    let encoded = encode_rows(picture, &palette, change_cost);
    (palette, encoded)
}

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "sixels within the fixed page")]
fn squared_error(picture: &Picture, encoded: &Encoded) -> i64 {
    drawn(picture, encoded).iter().enumerate().map(|(pixel, &who)| encoded.distance[pixel * encoded.count + who]).sum()
}

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "colours within the palette")]
fn bincount(who: &[usize], length: usize) -> Vec<i64> {
    let mut counts = vec![0i64; length];
    for &entry in who {
        counts[entry] += 1;
    }
    counts
}

/// The palette with a colour split from the one of most error: the mean of
/// its sixels whose error is at least the median, added in a free slot or
/// in place of the least used.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::needless_range_loop,
    reason = "sixels and colours within the fixed page and the palette"
)]
fn split_worst(picture: &Picture, palette: &Palette, encoded: &Encoded, drop_least_used: bool) -> Palette {
    let count = encoded.count;
    let who = drawn(picture, encoded);
    let error: Vec<i64> = (0..PIXELS).map(|pixel| encoded.distance[pixel * count + who[pixel]]).collect();
    let mut per_colour = vec![0i64; count];
    for pixel in 0..PIXELS {
        per_colour[who[pixel]] += error[pixel];
    }
    let mut worst = 0;
    for entry in 1..count {
        if per_colour[entry] > per_colour[worst] {
            worst = entry;
        }
    }
    let mine: Vec<usize> = (0..PIXELS).filter(|&pixel| who[pixel] == worst).collect();
    let mut errors: Vec<i64> = mine.iter().map(|&pixel| error[pixel]).collect();
    errors.sort_unstable();
    let n = errors.len();
    if n == 0 {
        return palette.clone();
    }
    // Twice the median, so that the even case stays a whole number.
    let doubled_median = if n % 2 == 1 { 2 * errors[n / 2] } else { errors[n / 2 - 1] + errors[n / 2] };
    let mut total = [0i64; 3];
    let mut far = 0i64;
    for &pixel in &mine {
        if 2 * error[pixel] >= doubled_median {
            for i in 0..3 {
                total[i] += picture.pixels[pixel][i];
            }
            far += 1;
        }
    }
    let mut fitted = palette.fitted.clone();
    if drop_least_used {
        let use_counts = &bincount(&who, count)[palette.fixed.len()..];
        let mut least = 0;
        for index in 1..use_counts.len() {
            if use_counts[index] < use_counts[least] {
                least = index;
            }
        }
        if least < fitted.len() {
            fitted.remove(least);
        }
    }
    fitted.push(to_12_bit(total.map(|t| t as f64 / far.max(1) as f64)));
    palette.with_fitted(unique(fitted))
}

/// The best fit from `start`, splitting colours while that helps, and its
/// error.
#[allow(clippy::indexing_slicing, reason = "fixed colours are the palette's first")]
fn refine(picture: &Picture, start: Palette, change_cost: i64) -> ((Palette, Encoded), i64) {
    let mut best = fit(picture, start, change_cost);
    let mut best_error = squared_error(picture, &best.1);
    for _ in 0..MAX_SPLITS {
        let (mut current, mut encoded) = best.clone();
        let use_counts = bincount(&drawn(picture, &encoded), current.colours.len());
        let use_counts = &use_counts[current.fixed.len()..];
        if use_counts.contains(&0) {
            let kept: Vec<Rgb> =
                current.fitted.iter().zip(use_counts).filter(|(_, n)| **n > 0).map(|(colour, _)| *colour).collect();
            current = current.with_fitted(kept);
            encoded = encode_rows(picture, &current, change_cost);
        }
        let full = current.fitted.len() >= current.redefinable.len();
        let trial = fit(picture, split_worst(picture, &current, &encoded, full), change_cost);
        let error = squared_error(picture, &trial.1);
        if error >= best_error {
            break;
        }
        best = trial;
        best_error = error;
    }
    (best, best_error)
}

/// The least change cost whose rows fit the page's own X/26, found by
/// halving.
#[allow(clippy::arithmetic_side_effects, reason = "costs between 1 and 2^24")]
fn within_budget(picture: &Picture, palette: &Palette) -> Result<(Palette, Encoded, i64), Error> {
    let (mut low, mut high) = (CHANGE_COST, MAX_CHANGE_COST);
    let mut best = None;
    while low < high {
        let middle = low.midpoint(high);
        let encoded = encode_rows(picture, palette, middle);
        let trial = palette.with_use(&column_1_use(&encoded));
        let encoded = encode_rows(picture, &trial, middle);
        let (_, row_triplets) = content(picture, &trial, &encoded);
        if row_triplets.iter().map(Vec::len).sum::<usize>() <= LOCAL_DATA_TRIPLETS {
            high = middle;
            best = Some((trial, encoded, middle));
        } else {
            low = middle + 1;
        }
    }
    best.ok_or(Error::NoChangeCostFits)
}

fn fit_within_page(picture: &Picture, palette: &Palette) -> Result<(Palette, Encoded), Error> {
    let (mut palette, mut encoded, mut cost) = within_budget(picture, palette)?;
    let mut best_error = squared_error(picture, &encoded);
    let mut best = (palette.clone(), encoded.clone());
    for _ in 0..BUDGET_ROUNDS {
        let (refitted, _) = fit(picture, palette, cost);
        (palette, encoded, cost) = within_budget(picture, &refitted)?;
        let error = squared_error(picture, &encoded);
        if error >= best_error {
            break;
        }
        best = (palette.clone(), encoded.clone());
        best_error = error;
    }
    Ok(best)
}

/// The three reductions of the picture to `count` colours, each as its set
/// of twelve-bit colours.
fn starting_palettes(picture: &Picture, count: u32) -> Vec<Vec<Rgb>> {
    let byte = |value: i64| u8::try_from(value).unwrap_or(u8::MAX);
    let pixels: Vec<Px> = picture.pixels.iter().map(|&[r, g, b]| Px { r: byte(r), g: byte(g), b: byte(b) }).collect();
    [quant::median_cut(&pixels, count), quant::max_coverage(&pixels, count), quant::octree(&pixels, count)]
        .iter()
        .map(|mapped| {
            unique(mapped.iter().map(|p| to_12_bit([f64::from(p.r), f64::from(p.g), f64::from(p.b)])).collect())
        })
        .collect()
}

/// The fixed colours and the redefinable slots of a level.
fn colours_of(level: Level) -> (Vec<(usize, Rgb)>, Vec<usize>) {
    if level == Level::TwoPointFive {
        // CLUTs 0 and 1 as Table 30 gives them, but transparent.
        let fixed = (0..2 * CLUT_ENTRIES).filter(|&slot| slot != TRANSPARENT).map(|slot| (slot, default_levels(slot)));
        (fixed.collect(), (2 * CLUT_ENTRIES..COLOUR_MAP_ENTRIES).collect())
    } else {
        (
            vec![(BLACK, default_levels(BLACK)), (WHITE, default_levels(WHITE))],
            (0..COLOUR_MAP_ENTRIES).filter(|s| ![TRANSPARENT, BLACK, WHITE].contains(s)).collect(),
        )
    }
}

/// The mean squared error of the sixels as `shown` against the picture,
/// each weighted by its height, rounded.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::needless_range_loop,
    reason = "sums over the fixed page, far below 2^53, and a non-negative mean"
)]
pub fn mean_squared_error(picture: &Picture, shown: &[Rgb]) -> u64 {
    let (mut total, mut weight) = (0i64, 0i64);
    for pixel in 0..PIXELS {
        total += distance(picture.pixels[pixel], shown[pixel]) * picture.weights[pixel];
        weight += picture.weights[pixel];
    }
    (total as f64 / weight.max(1) as f64).round() as u64
}

/// The page of `picture` at Level 2.5 or 3.5.
#[allow(clippy::indexing_slicing, clippy::cast_possible_truncation, reason = "colours drawn are within the palette")]
pub fn encode(picture: &Picture, options: &Options) -> Result<Page, Error> {
    let (fixed, redefinable) = colours_of(options.level);
    let results: Vec<((Palette, Encoded), i64)> = starting_palettes(picture, redefinable.len() as u32)
        .into_iter()
        .map(|start| refine(picture, Palette::new(&fixed, start, &redefinable, None), CHANGE_COST))
        .collect();
    // The first of equal errors.
    let mut best = 0;
    for (index, result) in results.iter().enumerate() {
        if result.1 < results[best].1 {
            best = index;
        }
    }
    let ((mut palette, mut encoded), _) = results.into_iter().nth(best).ok_or(Error::NoChangeCostFits)?;
    if !options.objects {
        (palette, encoded) = fit_within_page(picture, &palette)?;
    }
    let (level_1, row_triplets) = content(picture, &palette, &encoded);
    let colour_map = palette.colour_map();
    let (lines, objects) = tti::enhanced_lines(&Lines {
        level_35: options.level == Level::ThreePointFive,
        colour_map: &colour_map,
        level_1: &level_1,
        row_triplets: &row_triplets,
        objects: options.objects,
        first_row: picture.first_row,
    })?;
    let shown: Vec<Rgb> = drawn(picture, &encoded).iter().map(|&entry| palette.colours[entry]).collect();
    let place = if objects == 0 { "in the page's X/26".to_string() } else { format!("in {objects} objects") };
    let triplets: usize = row_triplets.iter().map(Vec::len).sum();
    let (pixels, drawn) = picture.views(&shown);
    Ok(Page {
        tti: tti::text(&lines),
        summary: format!("{} colours, {triplets} triplets {place}", palette.colours.len()),
        mean_squared_error: mean_squared_error(picture, &shown),
        picture: pixels,
        drawn,
    })
}
