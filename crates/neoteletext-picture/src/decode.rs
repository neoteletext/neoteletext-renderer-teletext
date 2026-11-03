//! Decoding within fixed bounds: the five formats of DEC-2114, the
//! orientation tag, and transparency laid over the background.

use std::io::Cursor;

use image::error::ImageError;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};

use crate::error::Error;
use crate::options::Options;
use crate::raster::{CHANNELS, Raster};

/// The largest picture file accepted, in bytes: far above any photograph a
/// camera writes, and a bound on what a caller must hold before decoding.
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// The longest side accepted, in pixels, after which a file is refused
/// before its pixels are decoded.
pub const MAX_SIDE: u32 = 32_768;
/// The most memory a decoder may allocate, in bytes: the `image` crate's
/// own default, named here so the bound is ours. A 48-megapixel photograph
/// with transparency, 8,000 by 6,000 at four bytes a pixel, needs 192 MiB.
pub const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;

/// Full opacity of an eight-bit alpha channel.
const OPAQUE: u32 = 255;
/// The alpha channel's place in an RGBA pixel.
const ALPHA: usize = 3;
/// Bytes of an RGBA pixel.
const RGBA_BYTES: usize = 4;

/// The picture in `bytes`, upright unless the options say otherwise, with
/// any transparency over the background colour. An animation gives its
/// first frame.
pub fn decode(bytes: &[u8], options: &Options) -> Result<Raster, Error> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(Error::InputTooLarge { bytes: bytes.len() });
    }
    let mut reader =
        ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|error| Error::Decode(error.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(refusal)?;
    // The decoder's own limit is not enough: a buffer the system hands out
    // in pages it has not yet backed passes it, and the picture is then
    // decoded into it. Its size is known from the header, so it is refused
    // here, before a byte of it is decoded.
    if decoder.total_bytes() > MAX_DECODE_BYTES {
        return Err(Error::PictureTooLarge);
    }
    // A broken orientation tag is not worth refusing a picture for: it is
    // shown as stored.
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).map_err(refusal)?;
    if options.auto_orient
        && let Some(orientation) = orientation
    {
        picture.apply_orientation(orientation);
    }
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let data = if picture.color().has_alpha() {
        over_background(picture.into_rgba8().as_raw(), options.background)
    } else {
        picture.into_rgb8().into_raw()
    };
    Raster::new(width, height, data).ok_or(Error::PictureTooLarge)
}

/// The error a decoder's refusal becomes.
fn refusal(error: ImageError) -> Error {
    match error {
        ImageError::Limits(_) => Error::PictureTooLarge,
        other => Error::Decode(other.to_string()),
    }
}

/// RGBA pixels laid over `background`, rounded to the nearest level.
fn over_background(rgba: &[u8], background: [u8; CHANNELS]) -> Vec<u8> {
    let mut out = Vec::with_capacity((rgba.len() / RGBA_BYTES).saturating_mul(CHANNELS));
    for pixel in rgba.as_chunks::<RGBA_BYTES>().0 {
        let alpha = pixel.get(ALPHA).copied().map_or(OPAQUE, u32::from);
        for (&channel, &under) in pixel.iter().zip(background.iter()) {
            out.push(blend(u32::from(channel), u32::from(under), alpha));
        }
    }
    out
}

/// `over` at opacity `alpha` out of 255 on `under`, to the nearest level.
fn blend(over: u32, under: u32, alpha: u32) -> u8 {
    let total = over.saturating_mul(alpha).saturating_add(under.saturating_mul(OPAQUE.saturating_sub(alpha)));
    let rounded = total.saturating_add(OPAQUE / 2).checked_div(OPAQUE).unwrap_or(0);
    u8::try_from(rounded).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEC-2325 item 3: half-transparent white over black is mid grey, and
    /// fully transparent shows the background, not the hidden colour.
    #[test]
    fn transparency_is_laid_over_the_background() {
        assert_eq!(over_background(&[255, 255, 255, 128, 200, 10, 30, 0], [0, 0, 255]), [128, 128, 255, 0, 0, 255]);
    }
}
