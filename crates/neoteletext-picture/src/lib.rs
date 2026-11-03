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
mod level1;
mod options;
mod page;
mod quant;
mod raster;
mod tti;

pub use decode::{MAX_DECODE_BYTES, MAX_INPUT_BYTES, MAX_SIDE};
pub use error::Error;
pub use options::{Align, Display, Fit, Focus, Level, Options, Place, Region, Rgb8, Smooth, parse_colour};
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
}

impl Page {
    /// The page as T42 packets, in the order of Annex B of EN 300 706: the
    /// TTI text read back by the engine's own TTI reader and encoded by its
    /// T42 writer, so the packets are the ones any engine program would send.
    pub fn t42(&self) -> Result<Vec<u8>, Error> {
        tti::to_t42(&self.tti)
    }
}

/// Converts the picture in `bytes` to a teletext page.
///
/// The options are checked first ([`Options::check`]); then the picture is
/// decoded, fitted to the page and encoded.
pub fn convert(bytes: &[u8], options: &Options) -> Result<Page, Error> {
    options.check()?;
    let picture = decode::decode(bytes, options)?;
    let sixels = fit::sixels(picture, options)?;
    if options.level.is_enhanced() {
        enhanced::encode(&sixels, options)
    } else {
        Ok(level1::encode(&sixels, options.hold_mosaics))
    }
}
