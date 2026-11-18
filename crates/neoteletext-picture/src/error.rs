//! Why a conversion failed.

use core::fmt;

/// Why a picture could not be made a page. Every failure of untrusted input
/// is one of these, never a panic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Two options contradict each other; the text says which.
    Conflict(&'static str),
    /// The file is larger than [`MAX_INPUT_BYTES`](crate::MAX_INPUT_BYTES).
    InputTooLarge {
        /// Its length in bytes.
        bytes: usize,
    },
    /// The bytes are none of the five formats, or a format's decoder
    /// refused them; the text is the decoder's.
    Decode(String),
    /// The picture is larger than [`MAX_SIDE`](crate::MAX_SIDE) on a side,
    /// or would need more than [`MAX_DECODE_BYTES`](crate::MAX_DECODE_BYTES)
    /// to decode.
    PictureTooLarge,
    /// The crop region falls outside the picture.
    CropOutside {
        /// The picture's width, after orientation.
        width: usize,
        /// Its height.
        height: usize,
    },
    /// Pixel art larger than the sixels the page draws.
    PixelArtTooLarge {
        /// The picture's width.
        width: usize,
        /// Its height.
        height: usize,
        /// The sixels across the page.
        page_width: usize,
        /// The sixels down the rows drawn.
        page_height: usize,
    },
    /// Without objects, no change cost brings the page's triplets within
    /// the 208 of its own X/26.
    NoChangeCostFits,
    /// The rows' triplets need more object sub-pages than an object page
    /// has.
    TooManyObjectSubpages {
        /// The sub-pages needed.
        needed: usize,
    },
    /// The page written could not be encoded as T42; the text says why.
    T42(String),
    /// A picture's ReGIS or NAPLPS is longer than
    /// `MAX_PICTURE_OUTPUT_BYTES` (renderer parity PAR-9): its length.
    TooLarge {
        /// Its length in bytes.
        bytes: usize,
    },
    /// A picture's ReGIS or NAPLPS, drawn by its renderer, does not draw
    /// the picture it was written for; the text says which.
    Unchecked(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(reason) | Self::Unchecked(reason) => formatter.write_str(reason),
            Self::InputTooLarge { bytes } => {
                write!(formatter, "the file is {bytes} bytes, more than the {} accepted", crate::MAX_INPUT_BYTES)
            }
            Self::Decode(reason) => write!(formatter, "the picture could not be read: {reason}"),
            Self::PictureTooLarge => write!(
                formatter,
                "the picture is larger than {} pixels on a side or needs more than {} bytes to decode",
                crate::MAX_SIDE,
                crate::MAX_DECODE_BYTES
            ),
            Self::CropOutside { width, height } => {
                write!(formatter, "the crop falls outside the picture, which is {width} by {height}")
            }
            Self::PixelArtTooLarge { width, height, page_width, page_height } => write!(
                formatter,
                "{width} by {height} is larger than the {page_width} by {page_height} sixels pixel art is drawn in"
            ),
            Self::NoChangeCostFits => formatter.write_str("no change cost fits the page's own triplets"),
            Self::TooManyObjectSubpages { needed } => {
                write!(formatter, "{needed} object sub-pages are needed, more than an object page holds")
            }
            Self::T42(reason) => write!(formatter, "the page could not be encoded as T42: {reason}"),
            Self::TooLarge { bytes } => {
                write!(formatter, "the picture takes {bytes} bytes, more than a picture's ReGIS or NAPLPS may")
            }
        }
    }
}

impl std::error::Error for Error {}
