//! Pictures to teletext pages of mosaic graphics (DEC-2142, DEC-2325).
//!
//! [`convert`] takes the bytes of a picture file, one of the five formats of
//! DEC-2114 (PNG, JPEG, GIF, BMP and WebP), and returns a teletext page as a
//! TTI page file that draws the picture in sixels, two across and three down
//! in each cell, at the presentation level asked for. Every choice is a
//! field of [`Options`]. The library reads no file and opens nothing: the
//! caller supplies the bytes and stores the page (REQ-SEC-016), so a program
//! can run it in a confined process.
//!
//! The path from bytes to page:
//! 1. `decode`: the picture is decoded within fixed bounds
//!    on its size and memory, turned upright from its orientation tag, and
//!    any transparency is laid over the background colour.
//! 2. `fit`: the picture is cropped, scaled or placed whole on the page's
//!    raster, then averaged into the 80 by 75 sixels of the page, each sixel
//!    the area it covers as a cell is drawn.
//! 3. `level1` or `enhanced`: the encoder chooses each cell's two colours
//!    and the attributes that set them, by a dynamic programme that weighs
//!    the colour error against what each change costs at the level.
//! 4. `tti`: the page file's lines.
//!
//! All input is untrusted: decoding is bounded by [`MAX_INPUT_BYTES`],
//! [`MAX_SIDE`] and [`MAX_DECODE_BYTES`], and every failure is an [`Error`],
//! never a panic.
// REQ-SEC-015: only the C interface crate may contain unsafe code.
#![forbid(unsafe_code)]
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

mod decode;
mod enhanced;
mod error;
mod fit;
#[cfg(feature = "picture-languages")]
mod languages;
mod level1;
mod options;
mod page;
mod quant;
mod raster;
mod tti;

pub use decode::{MAX_DECODE_BYTES, MAX_INPUT_BYTES, MAX_SIDE};
pub use error::Error;
#[cfg(feature = "picture-languages")]
pub use languages::{
    MAX_PICTURE_COLOURS, MAX_PICTURE_OUTPUT_BYTES, MIN_PICTURE_COLOURS, PictureOptions, to_naplps, to_regis,
};
pub use options::{Align, Display, Fit, Focus, Level, Options, Place, Region, Rgb8, Smooth, parse_colour};
pub use raster::Raster;
pub use tti::PAGE_NUMBER;

/// A converted page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// The page as a TTI page file: page 100, and at Levels 2.5 and 3.5 with
    /// objects, the object page 1A0 after it.
    pub tti: String,
    /// What the encoder used, for a person to read: the colours, and the
    /// attribute cells or triplets.
    pub summary: String,
    /// The mean squared error of the page as drawn against the picture's
    /// sixels, each sixel weighted by its height; lower is closer.
    pub mean_squared_error: u64,
    /// The picture as the page's 80 by 75 sixels, row by row from the top
    /// left: what the encoder aimed at.
    pub picture: Vec<Rgb8>,
    /// Each sixel as a receiver draws the page, in the same order, so that
    /// a program can check what it shows (REQ-2335). A sixel the page does
    /// not draw is black: the receiver's eight columns of row 0, and the
    /// whole of row 0 when the header is kept.
    pub drawn: Vec<Rgb8>,
}

impl Page {
    /// The page as T42 packets, in the order of Annex B of EN 300 706: the
    /// TTI text read back by the engine's own TTI reader and encoded by its
    /// T42 writer, so the packets are the ones any engine program would send.
    ///
    /// # Errors
    ///
    /// [`Error::T42`], with the reason, when the engine's TTI reader or T42
    /// writer refuses the page.
    pub fn t42(&self) -> Result<Vec<u8>, Error> {
        tti::to_t42(&self.tti)
    }
}

/// Converts the picture in `bytes` to a teletext page.
///
/// The options are checked first ([`Options::check`]); then the picture is
/// decoded, fitted to the page and encoded.
///
/// # Errors
///
/// - [`Error::Conflict`] when two options contradict each other;
/// - [`Error::InputTooLarge`] when the file is larger than
///   [`MAX_INPUT_BYTES`];
/// - [`Error::Decode`] when the bytes are none of the five formats, or their
///   decoder refuses them;
/// - [`Error::PictureTooLarge`] when the picture passes [`MAX_SIDE`] on a
///   side or would need more than [`MAX_DECODE_BYTES`] to decode;
/// - [`Error::CropOutside`] when the crop region falls outside the picture;
/// - [`Error::PixelArtTooLarge`] when pixel art is larger than the sixels
///   the page draws;
/// - [`Error::NoChangeCostFits`] and [`Error::TooManyObjectSubpages`] when,
///   at Levels 2.5 and 3.5, the page's enhancement triplets cannot be made to
///   fit.
#[cfg(feature = "decode")]
pub fn convert(bytes: &[u8], options: &Options) -> Result<Page, Error> {
    options.check()?;
    convert_raster(decode::decode(bytes, options)?, options)
}

/// Converts a picture already in pixels to a teletext page: [`convert`]'s
/// path after decoding (renderer parity PAR-11), for a program that holds
/// the pixels, such as the server drawing a picture's mosaic fallback. The
/// orientation and transparency options, which belong to decoding, do not
/// apply.
///
/// # Errors
///
/// As [`convert`], but for the errors of decoding.
pub fn convert_raster(raster: Raster, options: &Options) -> Result<Page, Error> {
    options.check()?;
    let sixels = fit::sixels(raster, options)?;
    if options.level.is_enhanced() {
        enhanced::encode(&sixels, options)
    } else {
        Ok(level1::encode(&sixels, options.hold_mosaics))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test panics on what it rules out")]
    use super::*;

    // Renderer parity PAR-11: the server's fallback, a raster at Level 1
    // contained in rows 1 to 23, writes nothing on rows 0 and 24; and it
    // builds without the decoders.
    #[test]
    fn the_fallback_draws_rows_1_to_23_only() {
        let raster = Raster::filled(800, 480, [255, 0, 0]);
        let options = Options {
            level: Level::One,
            fit: Fit::Contain,
            keep_header: true,
            keep_footer: true,
            ..Options::default()
        };
        let page = convert_raster(raster, &options).unwrap();
        let rows: Vec<&str> = page.tti.lines().filter(|line| line.starts_with("OL,")).collect();
        assert_eq!(rows.len(), 23, "{rows:?}");
        assert!(!rows.iter().any(|line| line.starts_with("OL,0,") || line.starts_with("OL,24,")));
        assert!(page.drawn.contains(&[255, 0, 0]), "the picture is drawn in red");
        let enhanced = Options { keep_footer: true, ..Options::default() };
        assert!(matches!(convert_raster(Raster::filled(8, 8, [0; 3]), &enhanced), Err(Error::Conflict(_))));
    }
}
