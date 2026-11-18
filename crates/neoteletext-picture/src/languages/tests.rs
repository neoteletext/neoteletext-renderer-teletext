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

// PAR-9: colours outside 2 to 16 are refused.
#[test]
fn colours_outside_two_to_sixteen_are_refused() {
    for colours in [0, 1, 17] {
        let options = PictureOptions { colours, ..PictureOptions::default() };
        assert!(matches!(to_regis(&quarters(4, 4), &options), Err(Error::Conflict(_))));
    }
}
