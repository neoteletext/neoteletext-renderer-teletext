//! An eight-bit RGB picture, and its scaling by a separable filter in fixed
//! point: a box filter, whose output dot is the mean of the area it
//! covers, or a Lanczos filter of three lobes. The arithmetic follows a
//! widely used picture library's resampler step for step (`CREDITS.md`), so
//! its rounding, and which pass runs first, are that library's.
//!
//! A picture's sizes come from the input, so nothing here indexes or does
//! arithmetic that could panic: every access is checked.

use crate::options::Smooth;

/// Bytes of a pixel: red, green, blue.
pub const CHANNELS: usize = 3;
/// The fixed-point fraction bits of the filter weights: 32 bits less eight
/// for the sample and two for the sum's headroom.
const PRECISION_BITS: u32 = 32 - 8 - 2;
/// The reach of the box filter, in output dots, either side of the centre.
const BOX_SUPPORT: f64 = 0.5;
/// The lobes of the Lanczos filter, and so its reach.
const LANCZOS_LOBES: f64 = 3.0;
/// Half a sample, the offset from a sample's index to its centre.
const HALF: f64 = 0.5;

/// An eight-bit RGB picture, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raster {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl Raster {
    /// A picture of `data`, or `None` when its length is not
    /// `width × height × 3` or a side is 0.
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Option<Self> {
        let length = width.checked_mul(height)?.checked_mul(CHANNELS)?;
        (width > 0 && height > 0 && data.len() == length).then_some(Self { width, height, data })
    }

    /// A picture of one colour.
    pub fn filled(width: usize, height: usize, colour: [u8; CHANNELS]) -> Self {
        let length = width.saturating_mul(height).saturating_mul(CHANNELS);
        Self { width, height, data: colour.iter().copied().cycle().take(length).collect() }
    }

    /// The width in pixels.
    pub const fn width(&self) -> usize {
        self.width
    }

    /// The height in pixels.
    pub const fn height(&self) -> usize {
        self.height
    }

    /// The bytes of row `y`, or none past the last.
    pub fn row(&self, y: usize) -> &[u8] {
        let length = self.width.saturating_mul(CHANNELS);
        y.checked_mul(length).and_then(|start| self.data.get(start..start.saturating_add(length))).unwrap_or_default()
    }

    /// The colour at `x`, `y`.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; CHANNELS]> {
        let start = x.checked_mul(CHANNELS)?;
        let bytes = self.row(y).get(start..start.checked_add(CHANNELS)?)?;
        bytes.try_into().ok()
    }

    /// The region at `x`, `y` of `width` by `height`, or `None` when it is
    /// empty or falls outside.
    pub fn cropped(&self, x: usize, y: usize, width: usize, height: usize) -> Option<Self> {
        if x.checked_add(width)? > self.width || y.checked_add(height)? > self.height {
            return None;
        }
        let mut data = Vec::with_capacity(width.checked_mul(height)?.checked_mul(CHANNELS)?);
        let start = x.checked_mul(CHANNELS)?;
        let end = start.checked_add(width.checked_mul(CHANNELS)?)?;
        for row in y..y.checked_add(height)? {
            data.extend_from_slice(self.row(row).get(start..end)?);
        }
        Self::new(width, height, data)
    }

    /// Copies `part` onto this picture with its corner at `x`, `y`; what
    /// would fall outside is left out.
    pub fn paste(&mut self, part: &Self, x: usize, y: usize) {
        let row_bytes = self.width.saturating_mul(CHANNELS);
        let left = x.saturating_mul(CHANNELS);
        for (offset, source) in (0..part.height).map(|row| part.row(row)).enumerate() {
            let start = y.saturating_add(offset).saturating_mul(row_bytes).saturating_add(left);
            let room = row_bytes.saturating_sub(left).min(source.len());
            if let (Some(target), Some(source)) =
                (self.data.get_mut(start..start.saturating_add(room)), source.get(..room))
            {
                target.copy_from_slice(source);
            }
        }
    }
}

/// A size as a real number: sides are at most `MAX_SIDE`, so exact.
#[allow(clippy::cast_precision_loss, reason = "sizes are at most MAX_SIDE, 32,768, which a double holds exactly")]
pub const fn real(size: usize) -> f64 {
    size as f64
}

/// A real number rounded half to even, then held to `low..=high`.
#[allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "the value is clamped to a non-negative range of sizes first"
)]
pub fn whole(value: f64, low: usize, high: usize) -> usize {
    (value.round_ties_even().clamp(real(low), real(high))) as usize
}

/// The box filter: 1 within half a dot of the centre.
fn box_filter(x: f64) -> f64 {
    if x > -BOX_SUPPORT && x <= BOX_SUPPORT { 1.0 } else { 0.0 }
}

/// The normalised sinc function.
fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * core::f64::consts::PI;
    x.sin() / x
}

/// The Lanczos filter of three lobes.
fn lanczos_filter(x: f64) -> f64 {
    if (-LANCZOS_LOBES..LANCZOS_LOBES).contains(&x) { sinc(x) * sinc(x / LANCZOS_LOBES) } else { 0.0 }
}

/// One output dot's inputs: the first, and the fixed-point weight of each.
struct Tap {
    first: usize,
    weights: Vec<i32>,
}

/// The taps of scaling `in_size` dots to `out_size`.
#[allow(clippy::cast_possible_truncation, reason = "positions are within the input's size, at most MAX_SIDE")]
fn taps(in_size: usize, out_size: usize, smooth: Smooth) -> Vec<Tap> {
    let (filter, filter_support): (fn(f64) -> f64, f64) = match smooth {
        Smooth::Area => (box_filter, BOX_SUPPORT),
        Smooth::Lanczos => (lanczos_filter, LANCZOS_LOBES),
    };
    // The scale is taken through single precision, as the resampler this
    // follows passes its box as single-precision numbers.
    #[allow(clippy::cast_possible_truncation, reason = "the size is at most MAX_SIDE, exact in single precision")]
    let scale = f64::from(real(in_size) as f32) / real(out_size);
    let filter_scale = scale.max(1.0);
    let support = filter_support * filter_scale;
    let inverse = 1.0 / filter_scale;
    let one = f64::from(1u32 << PRECISION_BITS);
    let last = i64::try_from(in_size).unwrap_or(i64::MAX);
    (0..out_size)
        .map(|out| {
            let centre = (real(out) + HALF) * scale;
            let first = ((centre - support + HALF) as i64).max(0);
            let end = ((centre + support + HALF) as i64).min(last);
            let raw: Vec<f64> = (first..end.max(first))
                .map(|x| filter((real(usize::try_from(x).unwrap_or(0)) - centre + HALF) * inverse))
                .collect();
            let total: f64 = raw.iter().sum();
            let weights = raw
                .iter()
                .map(|weight| {
                    let value = if total == 0.0 { *weight } else { weight / total };
                    if value < 0.0 { (-HALF + value * one) as i32 } else { (HALF + value * one) as i32 }
                })
                .collect();
            Tap { first: usize::try_from(first).unwrap_or(0), weights }
        })
        .collect()
}

/// A fixed-point sum to a sample, held to 0 to 255.
fn clip(total: i32) -> u8 {
    u8::try_from(total.wrapping_shr(PRECISION_BITS).clamp(0, i32::from(u8::MAX))).unwrap_or(u8::MAX)
}

/// The rounding term every sum starts from: half of one.
const ROUNDING: i32 = 1 << (PRECISION_BITS - 1);

/// Scales each of `height` rows from row `offset` across to the taps.
fn across(input: &Raster, offset: usize, height: usize, taps: &[Tap]) -> Raster {
    let width = taps.len();
    let mut data = Vec::with_capacity(width.saturating_mul(height).saturating_mul(CHANNELS));
    for y in offset..offset.saturating_add(height) {
        let line = input.row(y);
        for tap in taps {
            let start = tap.first.saturating_mul(CHANNELS);
            let pixels = line.get(start..).unwrap_or_default().as_chunks::<CHANNELS>().0;
            let mut totals = [ROUNDING; CHANNELS];
            for (pixel, weight) in pixels.iter().zip(&tap.weights) {
                for (total, &sample) in totals.iter_mut().zip(pixel) {
                    *total = total.wrapping_add(i32::from(sample).wrapping_mul(*weight));
                }
            }
            data.extend(totals.map(clip));
        }
    }
    Raster { width, height, data }
}

/// Scales every column of `input` down to the taps.
fn down(input: &Raster, taps: &[Tap]) -> Raster {
    let width = input.width;
    let row_bytes = width.saturating_mul(CHANNELS);
    let mut data = Vec::with_capacity(row_bytes.saturating_mul(taps.len()));
    for tap in taps {
        let rows: Vec<&[u8]> = (0..tap.weights.len()).map(|y| input.row(tap.first.saturating_add(y))).collect();
        for byte in 0..row_bytes {
            let mut total = ROUNDING;
            for (row, weight) in rows.iter().zip(&tap.weights) {
                let sample = row.get(byte).copied().unwrap_or(0);
                total = total.wrapping_add(i32::from(sample).wrapping_mul(*weight));
            }
            data.push(clip(total));
        }
    }
    Raster { width, height: taps.len(), data }
}

/// `input` scaled to `width` by `height`, both at least 1.
pub fn resample(input: &Raster, width: usize, height: usize, smooth: Smooth) -> Raster {
    let (width, height) = (width.max(1), height.max(1));
    let horizontal = width != input.width;
    let vertical = height != input.height;
    if !horizontal && !vertical {
        return input.clone();
    }
    // Which pass runs first changes the rounding, so it is the resampler's
    // own rule: down first only when it shrinks by more than twice across.
    let shrink_down = input.height.saturating_sub(height);
    let shrink_across = input.width.saturating_sub(width);
    let across_first = !(shrink_down > 0 && shrink_down > shrink_across.saturating_mul(2));
    let mut taps_down = taps(input.height, height, smooth);
    let taps_across = taps(input.width, width, smooth);
    if across_first {
        let mut picture = None;
        if horizontal {
            // Only the rows the vertical pass reads are scaled across.
            let first = taps_down.first().map_or(0, |tap| tap.first);
            let end = taps_down.last().map_or(0, |tap| tap.first.saturating_add(tap.weights.len()));
            for tap in &mut taps_down {
                tap.first = tap.first.saturating_sub(first);
            }
            picture = Some(across(input, first, end.saturating_sub(first), &taps_across));
        }
        let source = picture.as_ref().unwrap_or(input);
        if vertical { down(source, &taps_down) } else { source.clone() }
    } else {
        let middle = down(input, &taps_down);
        if horizontal { across(&middle, 0, height, &taps_across) } else { middle }
    }
}

#[cfg(test)]
mod tests;
