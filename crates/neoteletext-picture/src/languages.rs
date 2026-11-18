//! Pictures to ReGIS and NAPLPS (renderer parity PAR-9, REQ-6763 item 5).
//!
//! A picture file is decoded within the converter's bounds, fitted to the
//! language's canvas with the converter's own fit, reduced to at most
//! sixteen colours by its median-cut quantiser, and written as filled
//! rectangles: each row's runs of one colour, merged downwards with the run
//! beneath of the same colour and extent. ReGIS is then shortened by the
//! ReGIS repository's minifier; both outputs are checked by drawing them
//! with the project's own renderer, and must draw exactly the colour of
//! each pixel the reduction chose.

use crate::decode;
use crate::error::Error;
use crate::fit::fitted;
use crate::options::Options;
use crate::quant::{Px, median_cut};
use crate::raster::Raster;

/// The most bytes a picture's ReGIS or NAPLPS may take (PAR-9).
pub const MAX_PICTURE_OUTPUT_BYTES: usize = 262_144;
/// The fewest colours a picture may be reduced to.
pub const MIN_PICTURE_COLOURS: u8 = 2;
/// The most: the VT340's sixteen colour-map entries (the manual's chapter
/// 2), and the sixteen entries of a NAPLPS colour map of four bits, which
/// NeoTeletext's receivers give the renderer.
pub const MAX_PICTURE_COLOURS: u8 = 16;

/// The ReGIS canvas: the VT340's screen, square pixels.
const REGIS_WIDTH: usize = 800;
const REGIS_HEIGHT: usize = 480;
const _: () = assert!(REGIS_WIDTH == regis::SCREEN_WIDTH as usize && REGIS_HEIGHT == regis::SCREEN_HEIGHT as usize);
/// The NAPLPS canvas: the 480 dots and 462 lines of the teletext raster a
/// NAPLPS picture is drawn on (DEC-6667, REQ-5819), each dot 16/13 as wide
/// as it is high (DEC-2049).
const NAPLPS_WIDTH: usize = 480;
const NAPLPS_HEIGHT: usize = 462;
const NAPLPS_DOT_WIDE: f64 = 16.0;
const NAPLPS_DOT_HIGH: f64 = 13.0;
/// The NAPLPS colour map the output is checked with: four bits, sixteen
/// entries.
const NAPLPS_PALETTE_BITS: u32 = 4;
const _: () = assert!(1 << NAPLPS_PALETTE_BITS == MAX_PICTURE_COLOURS as usize);

/// The choices of a conversion to a picture language: the converter's own
/// (the fit, the filter, the alignment, the background, orientation, crop,
/// enlarging and focus; the level and the page's rows do not apply) and the
/// colours to reduce to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureOptions {
    /// The converter's options.
    pub options: Options,
    /// The colours, [`MIN_PICTURE_COLOURS`] to [`MAX_PICTURE_COLOURS`].
    pub colours: u8,
}

impl Default for PictureOptions {
    fn default() -> Self {
        Self { options: Options::default(), colours: MAX_PICTURE_COLOURS }
    }
}

/// One filled rectangle of the picture: its colour's index and its pixels,
/// `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rectangle {
    colour: u8,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

/// A picture reduced to indexed colours on a canvas.
struct Indexed {
    width: usize,
    height: usize,
    /// Each pixel's colour index, row by row.
    indices: Vec<u8>,
    /// The colours, as red, green and blue.
    palette: Vec<[u8; 3]>,
}

/// The picture fitted to a canvas of `width` by `height` whose dot is
/// `widen` as wide as high, reduced to `colours` colours.
fn indexed(bytes: &[u8], options: &PictureOptions, width: usize, height: usize, widen: f64) -> Result<Indexed, Error> {
    if !(MIN_PICTURE_COLOURS..=MAX_PICTURE_COLOURS).contains(&options.colours) {
        return Err(Error::Conflict("a picture has 2 to 16 colours"));
    }
    let picture = decode::decode(bytes, &options.options)?;
    let picture = match options.options.crop {
        Some(region) => picture
            .cropped(region.x, region.y, region.width, region.height)
            .ok_or(Error::CropOutside { width: picture.width(), height: picture.height() })?,
        None => picture,
    };
    let canvas: Raster = fitted(&picture, &options.options, width, height, widen);
    let pixels: Vec<Px> = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| canvas.pixel(x, y).unwrap_or_default())
        .map(|[r, g, b]| Px { r, g, b })
        .collect();
    let reduced = median_cut(&pixels, u32::from(options.colours));
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut indices = Vec::with_capacity(reduced.len());
    for pixel in reduced {
        let colour = [pixel.r, pixel.g, pixel.b];
        let index = if let Some(index) = palette.iter().position(|known| *known == colour) {
            index
        } else {
            palette.push(colour);
            palette.len().saturating_sub(1)
        };
        indices.push(u8::try_from(index).map_err(|_| Error::Conflict("the quantiser gave more colours than asked"))?);
    }
    if palette.len() > usize::from(options.colours) {
        return Err(Error::Conflict("the quantiser gave more colours than asked"));
    }
    Ok(Indexed { width, height, indices, palette })
}

/// The picture's rectangles: each row's runs of one colour, merged with the
/// open rectangle above of the same colour and extent.
fn rectangles(picture: &Indexed) -> Vec<Rectangle> {
    let mut done = Vec::new();
    let mut open: Vec<Rectangle> = Vec::new();
    for y in 0..picture.height {
        let row = picture.indices.get(y.saturating_mul(picture.width)..).unwrap_or_default();
        let row = row.get(..picture.width).unwrap_or_default();
        let mut runs = Vec::new();
        let mut x = 0;
        while let Some(&colour) = row.get(x) {
            let end = (x..row.len()).find(|&at| row.get(at) != Some(&colour)).unwrap_or(row.len());
            runs.push((colour, x, end));
            x = end;
        }
        let mut next = Vec::with_capacity(runs.len());
        for (colour, x0, x1) in runs {
            if let Some(place) = open.iter().position(|rect| rect.colour == colour && rect.x0 == x0 && rect.x1 == x1) {
                let mut rect = open.swap_remove(place);
                rect.y1 = y.saturating_add(1);
                next.push(rect);
            } else {
                next.push(Rectangle { colour, x0, y0: y, x1, y1: y.saturating_add(1) });
            }
        }
        done.append(&mut open);
        open = next;
    }
    done.append(&mut open);
    done.sort_by_key(|rect| (rect.y0, rect.x0));
    done
}

/// Refuses output over [`MAX_PICTURE_OUTPUT_BYTES`].
fn bounded(bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    if bytes.len() > MAX_PICTURE_OUTPUT_BYTES {
        return Err(Error::TooLarge { bytes: bytes.len() });
    }
    Ok(bytes)
}

// ReGIS (the VT330/VT340 Programmer Reference Manual, volume 2).

/// The device control string that starts ReGIS (chapter 1).
const REGIS_START: &[u8] = b"\x1bPp";
/// The string terminator that ends it.
const REGIS_END: &[u8] = b"\x1b\\";
/// The most an eight-bit component is.
const FULL_BYTE: i64 = 255;
/// Degrees of the hue circle, and DEC's turn of it: blue at 0, red at 120,
/// green at 240 (chapter 2, "HLS System").
const CIRCLE: f64 = 360.0;
const DEC_HUE_TURN: f64 = 120.0;
/// A sixth of the circle.
const SECTOR: f64 = 60.0;

/// A colour in the HLS form the VT340's colour map takes, `(H h L l S s)`:
/// the RGB System's eight letters cannot name sixteen chosen colours.
#[allow(clippy::cast_possible_truncation, reason = "the values are rounded within 0 to 360 and 0 to 100")]
fn hls(colour: [u8; 3]) -> (i64, i64, i64) {
    let [red, green, blue] = colour.map(|part| f64::from(part) / f64::from(u8::MAX));
    let high = red.max(green).max(blue);
    let low = red.min(green).min(blue);
    let lightness = f64::midpoint(high, low);
    let chroma = high - low;
    if chroma <= 0.0 {
        return (0, (lightness * 100.0).round() as i64, 0);
    }
    let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if (high - red).abs() < f64::EPSILON {
        SECTOR * ((green - blue) / chroma).rem_euclid(6.0)
    } else if (high - green).abs() < f64::EPSILON {
        SECTOR * ((blue - red) / chroma + 2.0)
    } else {
        SECTOR * ((red - green) / chroma + 4.0)
    };
    let dec = (hue + DEC_HUE_TURN).rem_euclid(CIRCLE);
    (
        (dec.round() as i64).rem_euclid(CIRCLE as i64),
        (lightness * 100.0).round() as i64,
        (saturation * 100.0).round() as i64,
    )
}

/// The picture as ReGIS: the colour map set (`S(M…)`), then each rectangle
/// drawn in its colour (`W(I…)`) as a filled polygon (`F(V…)`, chapter 11),
/// or a vector where it is one pixel across or down, since a polygon needs
/// three different vertices.
fn regis_of(picture: &Indexed) -> Vec<u8> {
    let mut out = REGIS_START.to_vec();
    out.extend_from_slice(b"S(M");
    for (index, &colour) in picture.palette.iter().enumerate() {
        let (hue, lightness, saturation) = hls(colour);
        out.extend_from_slice(format!("{index}(H{hue}L{lightness}S{saturation})").as_bytes());
    }
    out.push(b')');
    let mut pen = None;
    for rect in rectangles(picture) {
        if pen != Some(rect.colour) {
            out.extend_from_slice(format!("W(I{})", rect.colour).as_bytes());
            pen = Some(rect.colour);
        }
        let (right, bottom) = (rect.x1.saturating_sub(1), rect.y1.saturating_sub(1));
        let (left, top) = (rect.x0, rect.y0);
        let command = if right == left || bottom == top {
            format!("P[{left},{top}]V[{right},{bottom}]")
        } else {
            format!("P[{left},{top}]F(V[{right},{top}][{right},{bottom}][{left},{bottom}][{left},{top}])")
        };
        out.extend_from_slice(command.as_bytes());
    }
    out.extend_from_slice(REGIS_END);
    out
}

/// Whether `bytes`, drawn by the ReGIS renderer, give every pixel its
/// index and every index its colour within the colour map's percentages.
fn regis_draws(bytes: &[u8], picture: &Indexed) -> bool {
    let mut screen = regis::Screen::new();
    let mut terminal = regis::Terminal::new();
    let _ = terminal.feed(&mut screen, bytes);
    let _ = terminal.finish(&mut screen);
    let shown = screen.displayed();
    let pixels_match = (0..picture.height).all(|y| {
        (0..picture.width).all(|x| {
            let at = y.saturating_mul(picture.width).saturating_add(x);
            let (Ok(column), Ok(line)) = (u32::try_from(x), u32::try_from(y)) else { return false };
            shown.pixel(column, line) == picture.indices.get(at).copied()
        })
    });
    let palette = terminal.palette();
    // A percentage is 2.55 levels of a byte; HLS's rounding loses a little
    // more, so each component is allowed a twentieth of its range.
    let tolerance = FULL_BYTE / 20;
    let colours_match = picture.palette.iter().enumerate().all(|(index, colour)| {
        let drawn = palette.colour(u8::try_from(index).unwrap_or(0));
        drawn.iter().zip(colour).all(|(&a, &b)| i64::from(a).abs_diff(i64::from(b)) <= tolerance.unsigned_abs())
    });
    pixels_match && colours_match
}

/// Converts the picture in `bytes` to ReGIS (PAR-9): fitted to the VT340's
/// 800 by 480 screen, reduced to `options.colours` colours of the colour
/// map, written as filled rectangles, shortened by the ReGIS repository's
/// minifier and checked by drawing; the minified form is kept only when it
/// draws the same.
///
/// # Errors
///
/// The converter's errors of decoding and fitting; [`Error::Conflict`] for
/// colours outside 2 to 16; [`Error::Unchecked`] when the ReGIS does not
/// draw the picture; [`Error::TooLarge`] past
/// [`MAX_PICTURE_OUTPUT_BYTES`].
pub fn to_regis(bytes: &[u8], options: &PictureOptions) -> Result<Vec<u8>, Error> {
    options.options.check()?;
    let picture = indexed(bytes, options, REGIS_WIDTH, REGIS_HEIGHT, 1.0)?;
    let written = regis_of(&picture);
    let minified = regis_minify::minify(&written, regis_minify::Level::Exact);
    if regis_draws(&minified, &picture) {
        return bounded(minified);
    }
    if regis_draws(&written, &picture) {
        return bounded(written);
    }
    Err(Error::Unchecked("the ReGIS does not draw the picture it was written for"))
}

// NAPLPS (ANSI X3.110-1983).

/// SO: the PDI set into the in-use table (section 4.3).
const SHIFT_OUT: u8 = 0x0E;
/// DOMAIN (section 5.3.2.2), and its fixed-format byte: one-byte single
/// values, four-byte multi-values, two dimensions (sections 5.3.2.2.2 to
/// 5.3.2.2.4).
const DOMAIN: u8 = 0x21;
const MULTI_BYTES: usize = 4;
const DOMAIN_FOUR_BYTES: u8 = DATA_BIT | ((MULTI_BYTES as u8 - 1) << 2);
/// SET COLOR and SELECT COLOR (sections 5.3.2.5 and 5.3.2.6).
const SET_COLOR: u8 = 0x3C;
const SELECT_COLOR: u8 = 0x3E;
/// RECTANGLE (SET, FILLED) (section 5.3.3.4): a start point, then a size.
const RECT_SET_FILLED: u8 = 0x33;
/// Bit 7 marks an operand byte as data (section 5.3.1).
const DATA_BIT: u8 = 0x40;
/// Bits of a coordinate in a two-dimensional multi-value operand of
/// `MULTI_BYTES` bytes, three a byte, the first the sign; and of a colour
/// primary, two a byte (section 5.3.2.2.5, 5.3.2.5.1).
const COORDINATE_BITS: u32 = 3 * MULTI_BYTES as u32;
const PRIMARY_BITS: u32 = 2 * MULTI_BYTES as u32;
const _: () = assert!(PRIMARY_BITS == u8::BITS, "four-byte colours carry each primary exactly");
/// The unit screen's height, 25/32 of its width, as the NAPLPS renderer
/// maps it to its canvas.
const UNIT_HEIGHT_NUMERATOR: i64 = 25;
const UNIT_HEIGHT_DENOMINATOR: i64 = 32;
/// SELECT COLOR's one-byte operand names entry `n` of sixteen as `n` in its
/// top four of six bits (section 5.3.2.6).
const ENTRY_SHIFT: u32 = 2;

/// A unit-screen value of `numerator / denominator` as a two's complement
/// fraction of `COORDINATE_BITS` bits, one the sign.
fn fraction(numerator: i64, denominator: i64) -> i64 {
    let scale = 1_i64 << (COORDINATE_BITS - 1);
    let rounded = numerator
        .saturating_mul(scale)
        .saturating_mul(2)
        .saturating_add(denominator)
        .div_euclid(denominator.saturating_mul(2).max(1));
    rounded & ((1_i64 << COORDINATE_BITS) - 1)
}

/// A two-dimensional multi-value operand of `MULTI_BYTES` bytes.
fn point(x: i64, y: i64, out: &mut Vec<u8>) {
    for byte in (0..MULTI_BYTES).rev() {
        let shift = u32::try_from(byte).unwrap_or(0).saturating_mul(3);
        let part = |value: i64| u8::try_from((value >> shift) & 0b111).unwrap_or(0);
        out.push(DATA_BIT | (part(x) << 3) | part(y));
    }
}

/// A colour operand of `MULTI_BYTES` bytes: green, red, blue, two bits of
/// each a byte, high bits first (section 5.3.2.5.1).
fn colour_operand([red, green, blue]: [u8; 3], out: &mut Vec<u8>) {
    for byte in (0..MULTI_BYTES).rev() {
        let shift = u32::try_from(byte).unwrap_or(0).saturating_mul(2);
        let pair = |value: u8| (value >> shift) & 0b11;
        let (g, r, b) = (pair(green), pair(red), pair(blue));
        out.push(
            DATA_BIT | ((g >> 1) << 5) | ((r >> 1) << 4) | ((b >> 1) << 3) | ((g & 1) << 2) | ((r & 1) << 1) | (b & 1),
        );
    }
}

/// The unit-screen point of the centre of pixel `x`, `y` (from the top
/// left) of a canvas `width` by `height`: x across the unit width, y up the
/// unit height from the bottom line.
fn centre(x: usize, y: usize, width: usize, height: usize) -> (i64, i64) {
    let to = |value: usize| i64::try_from(value).unwrap_or(i64::MAX);
    let across = fraction(to(x), to(width));
    let up = to(height).saturating_sub(1).saturating_sub(to(y));
    let up = fraction(up.saturating_mul(UNIT_HEIGHT_NUMERATOR), to(height).saturating_mul(UNIT_HEIGHT_DENOMINATOR));
    (across, up)
}

/// The picture as NAPLPS: SO, a four-byte domain, the colour map set from
/// entry 0 in colour mode 1 (SELECT COLOR then one SET COLOR), and each
/// rectangle filled in its entry from the centre of its bottom-left pixel
/// to that of its top-right.
fn naplps_of(picture: &Indexed) -> Vec<u8> {
    let mut out = vec![SHIFT_OUT, DOMAIN, DOMAIN_FOUR_BYTES, SELECT_COLOR, DATA_BIT, SET_COLOR];
    for &colour in &picture.palette {
        colour_operand(colour, &mut out);
    }
    let mut pen = None;
    let mask = (1_i64 << COORDINATE_BITS) - 1;
    for rect in rectangles(picture) {
        if pen != Some(rect.colour) {
            out.extend_from_slice(&[SELECT_COLOR, DATA_BIT | (rect.colour << ENTRY_SHIFT)]);
            pen = Some(rect.colour);
        }
        let (left, bottom) = centre(rect.x0, rect.y1.saturating_sub(1), picture.width, picture.height);
        let (right, top) = centre(rect.x1.saturating_sub(1), rect.y0, picture.width, picture.height);
        out.push(RECT_SET_FILLED);
        point(left, bottom, &mut out);
        point(right.wrapping_sub(left) & mask, top.wrapping_sub(bottom) & mask, &mut out);
    }
    out
}

/// Whether `bytes`, drawn by the NAPLPS renderer on the picture's canvas,
/// give every pixel its index.
fn naplps_draws(bytes: &[u8], picture: &Indexed) -> bool {
    let (Ok(width), Ok(height)) = (u32::try_from(picture.width), u32::try_from(picture.height)) else { return false };
    let Ok(mut canvas) = naplps::Canvas::new(width, height) else { return false };
    let mut terminal = naplps::Terminal::new(NAPLPS_PALETTE_BITS);
    terminal.feed(&mut canvas, bytes);
    terminal.finish(&mut canvas);
    canvas.pixels() == picture.indices.as_slice()
}

/// Converts the picture in `bytes` to NAPLPS (PAR-9): fitted to the 480 by
/// 462 teletext raster a NAPLPS picture is drawn on, whose dot is 16/13 as
/// wide as it is high, reduced to `options.colours` entries of colour mode
/// 1, written as filled rectangles and checked by drawing.
///
/// # Errors
///
/// As [`to_regis`].
pub fn to_naplps(bytes: &[u8], options: &PictureOptions) -> Result<Vec<u8>, Error> {
    options.options.check()?;
    let picture = indexed(bytes, options, NAPLPS_WIDTH, NAPLPS_HEIGHT, NAPLPS_DOT_WIDE / NAPLPS_DOT_HIGH)?;
    let written = naplps_of(&picture);
    if !naplps_draws(&written, &picture) {
        return Err(Error::Unchecked("the NAPLPS does not draw the picture it was written for"));
    }
    bounded(written)
}

#[cfg(test)]
mod tests;
