#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::panic,
    reason = "a test panics on what it rules out and indexes data it built to a known size"
)]

use super::*;
use crate::options::{Fit, Smooth};
use std::io::Cursor;

/// A PNG of `width` by `height`, each pixel `colour(x, y)`.
fn png(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let picture = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(colour(x, y)));
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const QUARTERS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];

/// Four coloured quarters, half the canvas on each side, so that the area
/// filter's exact doubling keeps every edge sharp.
fn quarters(width: u32, height: u32) -> Vec<u8> {
    png(width, height, |x, y| QUARTERS[usize::from(x >= width / 2) + 2 * usize::from(y >= height / 2)])
}

fn stretched(colours: u8) -> PictureOptions {
    PictureOptions { options: Options { fit: Fit::Stretch, smooth: Smooth::Area, ..Options::default() }, colours }
}

const NAPLPS_WIDEN: f64 = NAPLPS_DOT_WIDE / NAPLPS_DOT_HIGH;

// PAR-9: four coloured quarters give exactly four rectangles in each
// language, and each draws back to the same pixels.
#[test]
fn four_quarters_are_four_rectangles_in_each_language() {
    let regis_canvas = indexed(&quarters(400, 240), &stretched(4), REGIS_WIDTH, REGIS_HEIGHT, 1.0).unwrap();
    assert_eq!(rectangles(&regis_canvas).len(), 4);
    let regis = to_regis(&quarters(400, 240), &stretched(4)).unwrap();
    assert!(regis_draws(&regis, &regis_canvas));
    let regis_text = String::from_utf8_lossy(&regis);
    assert_eq!(regis_text.matches("F(").count(), 4, "{regis_text}");
    let naplps_canvas = indexed(&quarters(240, 231), &stretched(4), NAPLPS_WIDTH, NAPLPS_HEIGHT, NAPLPS_WIDEN).unwrap();
    assert_eq!(rectangles(&naplps_canvas).len(), 4);
    let naplps = to_naplps(&quarters(240, 231), &stretched(4)).unwrap();
    assert!(naplps_draws(&naplps, &naplps_canvas));
    let mut palette = naplps_canvas.palette.clone();
    palette.sort_unstable();
    let mut expected = QUARTERS.to_vec();
    expected.sort_unstable();
    assert_eq!(palette, expected, "the colours drawn are the quarters'");
}

// PAR-9: a rectangle one pixel across or down draws too.
#[test]
fn thin_rectangles_draw() {
    let options = stretched(2);
    let lines = png(800, 480, |x, y| if x == 10 || y == 20 { [255, 255, 255] } else { [0, 0, 0] });
    let canvas = indexed(&lines, &options, REGIS_WIDTH, REGIS_HEIGHT, 1.0).unwrap();
    assert!(regis_draws(&regis_of(&canvas), &canvas));
    let lines = png(480, 462, |x, y| if x == 10 || y == 20 { [255, 255, 255] } else { [0, 0, 0] });
    let canvas = indexed(&lines, &options, NAPLPS_WIDTH, NAPLPS_HEIGHT, NAPLPS_WIDEN).unwrap();
    assert!(naplps_draws(&naplps_of(&canvas), &canvas));
}

/// A picture with the shapes of a photograph: a sky's gradient, a sun,
/// and hills.
fn photograph() -> Vec<u8> {
    png(640, 480, |x, y| {
        let (dx, dy) = (i64::from(x) - 480, i64::from(y) - 120);
        let hill = 300 + (i64::from(x) - 320).abs() / 4;
        if dx * dx + dy * dy < 60 * 60 {
            [255, 220, 80]
        } else if i64::from(y) > hill {
            [40, (120 + y / 8) as u8, 40]
        } else {
            [(80 + y / 4) as u8, (140 + y / 6) as u8, 230]
        }
    })
}

// PAR-9: a photograph's output is within MAX_PICTURE_OUTPUT_BYTES and
// draws (each language's converter checks that by drawing); its length is
// printed as the measured baseline.
#[test]
fn a_photograph_fits_the_output_bound_in_both_languages() {
    let options = PictureOptions::default();
    for (name, bytes) in [("regis", to_regis(&photograph(), &options)), ("naplps", to_naplps(&photograph(), &options))]
    {
        let bytes = bytes.unwrap();
        assert!(bytes.len() <= MAX_PICTURE_OUTPUT_BYTES);
        println!("{name}: {} bytes", bytes.len());
    }
}

/// The mean squared error, per colour component on 0 to 255, of the
/// photograph's ReGIS and NAPLPS against the picture as fitted, before its
/// colours were reduced (PAR-9): measured on 19 November 2026 with the
/// default options and sixteen colours, rounded up to a whole level. The
/// error is the colour reduction's, since each output draws exactly the
/// reduced picture; a change of the quantiser or the fit that makes it
/// worse fails here, and one that makes it better lowers the baseline.
const REGIS_PHOTOGRAPH_MSE: u64 = 54;
/// See [`REGIS_PHOTOGRAPH_MSE`].
const NAPLPS_PHOTOGRAPH_MSE: u64 = 74;

/// The mean squared error, per component, of `drawn`'s colours against
/// `fitted`'s, rounded up.
fn mean_squared_error(fitted: &Raster, width: usize, height: usize, drawn: impl Fn(usize, usize) -> [u8; 3]) -> u64 {
    let mut sum = 0_u64;
    for y in 0..height {
        for x in 0..width {
            let expected = fitted.pixel(x, y).unwrap();
            for (a, b) in drawn(x, y).iter().zip(expected) {
                sum += u64::from(a.abs_diff(b)).pow(2);
            }
        }
    }
    sum.div_ceil((width * height * 3) as u64)
}

// PAR-9: the photograph's output, drawn by each renderer, is measured
// against the picture as fitted; the mean squared error is the recorded
// baseline and must not grow.
#[test]
fn a_photograph_draws_within_its_measured_error() {
    let options = PictureOptions::default();
    let photograph = photograph();
    let picture = decode::decode(&photograph, &options.options).unwrap();
    let regis_fitted = fitted(&picture, &options.options, REGIS_WIDTH, REGIS_HEIGHT, 1.0);
    let regis = to_regis(&photograph, &options).unwrap();
    let mut screen = regis::Screen::new();
    let mut terminal = regis::Terminal::new();
    let _ = terminal.feed(&mut screen, &regis);
    let _ = terminal.finish(&mut screen);
    let shown = screen.displayed();
    let regis_error = mean_squared_error(&regis_fitted, REGIS_WIDTH, REGIS_HEIGHT, |x, y| {
        terminal.palette().colour(shown.pixel(x as u32, y as u32).unwrap())
    });
    let naplps_fitted = fitted(&picture, &options.options, NAPLPS_WIDTH, NAPLPS_HEIGHT, NAPLPS_WIDEN);
    let naplps = to_naplps(&photograph, &options).unwrap();
    let mut canvas = naplps::Canvas::new(NAPLPS_WIDTH as u32, NAPLPS_HEIGHT as u32).unwrap();
    let mut terminal = naplps::Terminal::new(NAPLPS_PALETTE_BITS);
    terminal.feed(&mut canvas, &naplps);
    terminal.finish(&mut canvas);
    let naplps_error = mean_squared_error(&naplps_fitted, NAPLPS_WIDTH, NAPLPS_HEIGHT, |x, y| {
        terminal.palette().colour(canvas.pixels()[y * NAPLPS_WIDTH + x])
    });
    println!("mean squared error: regis {regis_error}, naplps {naplps_error}");
    assert!(regis_error <= REGIS_PHOTOGRAPH_MSE, "regis {regis_error}");
    assert!(naplps_error <= NAPLPS_PHOTOGRAPH_MSE, "naplps {naplps_error}");
}

// PAR-9: colours outside 2 to 16 are refused.
#[test]
fn colours_outside_two_to_sixteen_are_refused() {
    for colours in [0, 1, 17] {
        let options = PictureOptions { colours, ..PictureOptions::default() };
        assert!(matches!(to_regis(&quarters(4, 4), &options), Err(Error::Conflict(_))));
    }
}
