//! From the decoded picture to the page's sixels: the crop region, the fit
//! to the page's shape, and each sixel the mean of the area it covers as
//! the engine draws a cell.
//!
//! A picture's sizes come from the input, so nothing here indexes or does
//! arithmetic that could panic.

use neoteletext_core::consts::{ASPECT_DENOMINATOR, ASPECT_NUMERATOR, CELL_HEIGHT, COLUMNS, ROWS};

use crate::error::Error;
use crate::options::{Align, Display, Fit, Focus, Options};
use crate::page::{HEIGHT, PIXELS, Rgb, SIXEL_DOTS, SIXEL_LINES, SIXELS_DOWN, WIDTH};
use crate::raster::{CHANNELS, Raster, real, resample, whole};

/// Dots across the page's raster as the engine draws it.
const RASTER_WIDTH: usize = COLUMNS * SIXEL_DOTS * crate::page::SIXELS_ACROSS;
/// The fewest rows of sixels a picture may have to be taken as drawn
/// without asking: a page of 24 rows, the most a picture leaves to the
/// header.
const SHORTEST_AS_DRAWN: usize = (ROWS - 1) * SIXELS_DOWN;
/// Weights of red, green and blue in brightness, in thousandths (ITU-R
/// BT.601).
const LUMA_WEIGHTS: [i64; CHANNELS] = [299, 587, 114];

/// The picture as the page's sixels, row 0 or 1 to row 24.
pub fn sixels(picture: Raster, options: &Options) -> Result<crate::page::Picture, Error> {
    let first_row = usize::from(options.keep_header);
    let mut picture = picture;
    if let Some(region) = options.crop {
        picture = picture
            .cropped(region.x, region.y, region.width, region.height)
            .ok_or(Error::CropOutside { width: picture.width(), height: picture.height() })?;
    }
    let tallest = ROWS.saturating_sub(first_row).saturating_mul(SIXELS_DOWN);
    let pixels = if options.pixel_art {
        if picture.width() > WIDTH || picture.height() > tallest {
            return Err(Error::PixelArtTooLarge {
                width: picture.width(),
                height: picture.height(),
                page_width: WIDTH,
                page_height: tallest,
            });
        }
        as_drawn(&picture, options, tallest, options.align.unwrap_or(Align::CENTRE))
    } else if picture.width() == WIDTH && (SHORTEST_AS_DRAWN..=tallest).contains(&picture.height()) {
        // A picture of exactly the page's sixels was drawn for it: taken as
        // drawn, its spare rows at the top.
        as_drawn(&picture, options, tallest, options.align.unwrap_or(Align::BOTTOM))
    } else {
        let raster_height = ROWS.saturating_sub(first_row).saturating_mul(CELL_HEIGHT);
        averaged(&fitted(&picture, options, raster_height), first_row)
    };
    Ok(crate::page::Picture::new(pixels, first_row))
}

/// How much wider a raster dot is than tall on the display.
fn widen(display: Display) -> f64 {
    match display {
        Display::Square => 1.0,
        Display::Television => real(ASPECT_NUMERATOR) / real(ASPECT_DENOMINATOR),
    }
}

/// Brightness at `x`, `y` in thousandths of a level.
fn luma(picture: &Raster, x: usize, y: usize) -> i64 {
    picture.pixel(x, y).map_or(0, |pixel| {
        pixel.iter().zip(LUMA_WEIGHTS).map(|(&sample, weight)| i64::from(sample).saturating_mul(weight)).sum()
    })
}

/// Where a window `keep` long along an axis holds the most detail: the sum
/// of brightness differences between neighbours, nearest the centre on
/// equal totals.
fn busiest_window(picture: &Raster, keep: usize, across: bool) -> usize {
    let (length, other) =
        if across { (picture.width(), picture.height()) } else { (picture.height(), picture.width()) };
    let energy: Vec<i64> = (0..length)
        .map(|position| {
            (0..other)
                .map(|step| {
                    let (x, y) = if across { (position, step) } else { (step, position) };
                    let here = luma(picture, x, y);
                    let right = if x.saturating_add(1) < picture.width() {
                        here.saturating_sub(luma(picture, x.saturating_add(1), y)).saturating_abs()
                    } else {
                        0
                    };
                    let below = if y.saturating_add(1) < picture.height() {
                        here.saturating_sub(luma(picture, x, y.saturating_add(1))).saturating_abs()
                    } else {
                        0
                    };
                    right.saturating_add(below)
                })
                .fold(0, i64::saturating_add)
        })
        .collect();
    let middle = length.saturating_sub(keep) / 2;
    let mut running: i64 = energy.iter().take(keep).copied().fold(0, i64::saturating_add);
    let (mut best, mut best_total) = (0usize, running);
    for start in 1..=length.saturating_sub(keep) {
        let entering = energy.get(start.saturating_add(keep).saturating_sub(1)).copied().unwrap_or(0);
        let leaving = energy.get(start.saturating_sub(1)).copied().unwrap_or(0);
        running = running.saturating_add(entering).saturating_sub(leaving);
        let closer = start.abs_diff(middle) < best.abs_diff(middle);
        if running > best_total || (running == best_total && closer) {
            best = start;
            best_total = running;
        }
    }
    best
}

/// The picture on the page's raster of `RASTER_WIDTH` by `raster_height`.
fn fitted(picture: &Raster, options: &Options, raster_height: usize) -> Raster {
    let (width, height) = (picture.width(), picture.height());
    let widen = widen(options.display);
    let align = options.align.unwrap_or(Align::CENTRE);
    if options.fit == Fit::Stretch {
        return resample(picture, RASTER_WIDTH, raster_height, options.smooth);
    }
    if options.fit == Fit::Crop && options.upscale {
        let target = real(RASTER_WIDTH) * widen / real(raster_height);
        let detail = options.focus == Focus::Detail;
        let window = if real(width) / real(height) > target {
            let keep = whole(real(height) * target, 1, width);
            let x = if detail {
                busiest_window(picture, keep, true)
            } else {
                align.across.offset(width.saturating_sub(keep))
            };
            picture.cropped(x, 0, keep, height)
        } else {
            let keep = whole(real(width) / target, 1, height);
            let y = if detail {
                busiest_window(picture, keep, false)
            } else {
                align.down.offset(height.saturating_sub(keep))
            };
            picture.cropped(0, y, width, keep)
        };
        return resample(window.as_ref().unwrap_or(picture), RASTER_WIDTH, raster_height, options.smooth);
    }
    // Contain, or either fit that must not enlarge: one scale for the
    // picture as shown, where a raster dot is `widen` wide and a line high.
    let scale_across = real(RASTER_WIDTH) * widen / real(width);
    let scale_down = real(raster_height) / real(height);
    let mut scale =
        if options.fit == Fit::Contain { scale_across.min(scale_down) } else { scale_across.max(scale_down) };
    if !options.upscale {
        scale = scale.min(1.0);
    }
    let window_width = whole((real(RASTER_WIDTH) * widen / scale).round(), 1, width);
    let window_height = whole((real(raster_height) / scale).round(), 1, height);
    let window = picture.cropped(
        align.across.offset(width.saturating_sub(window_width)),
        align.down.offset(height.saturating_sub(window_height)),
        window_width,
        window_height,
    );
    let out_width = whole((real(window_width) * scale / widen).round(), 1, RASTER_WIDTH);
    let out_height = whole((real(window_height) * scale).round(), 1, raster_height);
    let scaled = resample(window.as_ref().unwrap_or(picture), out_width, out_height, options.smooth);
    let mut raster = Raster::filled(RASTER_WIDTH, raster_height, options.background);
    raster.paste(
        &scaled,
        align.across.offset(RASTER_WIDTH.saturating_sub(out_width)),
        align.down.offset(raster_height.saturating_sub(out_height)),
    );
    raster
}

/// The colour of a raster or picture pixel, in the encoder's form.
fn colour(pixel: [u8; CHANNELS]) -> Rgb {
    pixel.map(i64::from)
}

/// A picture taken sixel for sixel, enlarged by the largest whole number
/// that fits unless enlarging is off, on bars of the background.
fn as_drawn(picture: &Raster, options: &Options, tallest: usize, align: Align) -> Vec<Rgb> {
    let factor = if options.upscale {
        (WIDTH.checked_div(picture.width()).unwrap_or(1)).min(tallest.checked_div(picture.height()).unwrap_or(1)).max(1)
    } else {
        1
    };
    let (width, height) = (picture.width().saturating_mul(factor), picture.height().saturating_mul(factor));
    let mut out = vec![colour(options.background); PIXELS];
    let left = align.across.offset(WIDTH.saturating_sub(width));
    let top = HEIGHT.saturating_sub(tallest).saturating_add(align.down.offset(tallest.saturating_sub(height)));
    for y in 0..height {
        for x in 0..width {
            let source = picture.pixel(x.checked_div(factor).unwrap_or(0), y.checked_div(factor).unwrap_or(0));
            let at = top.saturating_add(y).saturating_mul(WIDTH).saturating_add(left).saturating_add(x);
            if let (Some(source), Some(target)) = (source, out.get_mut(at)) {
                *target = colour(source);
            }
        }
    }
    out
}

/// Each sixel of rows `first_row` to 24 the mean of the raster area it
/// covers: 6 dots by 6, 8 or 6 lines.
fn averaged(raster: &Raster, first_row: usize) -> Vec<Rgb> {
    let mut out = vec![[0; CHANNELS]; PIXELS];
    let mut line = 0usize;
    for row in first_row..ROWS {
        for (band, &lines) in SIXEL_LINES.iter().enumerate() {
            let count = real(lines.saturating_mul(SIXEL_DOTS));
            for x in 0..WIDTH {
                let mut totals = [0u32; CHANNELS];
                for y in line..line.saturating_add(lines) {
                    for dot in x.saturating_mul(SIXEL_DOTS)..x.saturating_add(1).saturating_mul(SIXEL_DOTS) {
                        let pixel = raster.pixel(dot, y).unwrap_or_default();
                        for (total, sample) in totals.iter_mut().zip(pixel) {
                            *total = total.saturating_add(u32::from(sample));
                        }
                    }
                }
                let at = row.saturating_mul(SIXELS_DOWN).saturating_add(band).saturating_mul(WIDTH).saturating_add(x);
                if let Some(target) = out.get_mut(at) {
                    *target = totals
                        .map(|total| i64::try_from(whole(f64::from(total) / count, 0, u8::MAX.into())).unwrap_or(0));
                }
            }
            line = line.saturating_add(lines);
        }
    }
    out
}
