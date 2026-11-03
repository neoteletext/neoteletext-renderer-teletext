//! Pixel-format helpers (REQ-RND-018): a row of palette indices to RGB565,
//! RGB888, RGBA8888, 8-bit grey or 1-bit, in either byte order, at any
//! stride, rotated by 0, 90, 180 or 270 degrees.

use crate::palette::Rgba;
use neoteletext_core::render::IndexFormat;

/// Bits in a byte, and in a 4-bit index's nibble.
const BYTE_BITS: usize = 8;
const NIBBLE_BITS: u32 = 4;
/// The low nibble: the right pixel of a byte at 4 bits per pixel.
const LOW_NIBBLE: u8 = 0x0F;

/// Output pixel formats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    /// 16-bit 5-6-5.
    Rgb565,
    /// 24-bit.
    Rgb888,
    /// 32-bit with alpha.
    Rgba8888,
    /// 8-bit grey.
    Grey8,
    /// 1-bit black and white, most significant bit leftmost.
    Mono1,
}

impl PixelFormat {
    /// Bits per pixel.
    #[must_use]
    pub const fn bits(self) -> usize {
        match self {
            Self::Rgb565 => 16,
            Self::Rgb888 => 24,
            Self::Rgba8888 => 32,
            Self::Grey8 => 8,
            Self::Mono1 => 1,
        }
    }
}

/// Byte order of multi-byte pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ByteOrder {
    /// Least significant byte first; for RGB888 and RGBA8888, red first.
    Little,
    /// Most significant byte first; for RGB888 and RGBA8888, the reverse.
    Big,
}

/// Rotation of the output image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rotation {
    /// None.
    None,
    /// 90 degrees clockwise.
    Clockwise90,
    /// 180 degrees.
    Half,
    /// 270 degrees clockwise.
    Clockwise270,
}

/// Errors from conversion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConvertError {
    /// The target buffer is too small.
    BufferTooSmall,
}

/// Luma weights of ITU-R BT.601 in parts of 256, for grey output.
const LUMA_R: u32 = 77;
const LUMA_G: u32 = 150;
const LUMA_B: u32 = 29;
/// Grey level at or above which a 1-bit pixel is white.
const MONO_THRESHOLD: u8 = 0x80;

fn grey(colour: Rgba) -> u8 {
    let weighted = LUMA_R
        .saturating_mul(u32::from(colour.r))
        .saturating_add(LUMA_G.saturating_mul(u32::from(colour.g)))
        .saturating_add(LUMA_B.saturating_mul(u32::from(colour.b)));
    (weighted >> 8).min(0xFF) as u8
}

/// Writes one pixel's bytes into `out` at bit offset `bit` (bytes are
/// byte-aligned except for 1-bit).
fn put(out: &mut [u8], bit: usize, colour: Rgba, format: PixelFormat, order: ByteOrder) -> Result<(), ConvertError> {
    let byte = bit / BYTE_BITS;
    let mut write = |offset: usize, value: u8| -> Result<(), ConvertError> {
        let slot = out.get_mut(byte.saturating_add(offset)).ok_or(ConvertError::BufferTooSmall)?;
        *slot = value;
        Ok(())
    };
    match format {
        PixelFormat::Rgb565 => {
            let value = (u16::from(colour.r >> 3) << 11) | (u16::from(colour.g >> 2) << 5) | u16::from(colour.b >> 3);
            let bytes = match order {
                ByteOrder::Little => value.to_le_bytes(),
                ByteOrder::Big => value.to_be_bytes(),
            };
            for (offset, value) in bytes.into_iter().enumerate() {
                write(offset, value)?;
            }
        }
        PixelFormat::Rgb888 => {
            let bytes = match order {
                ByteOrder::Little => [colour.r, colour.g, colour.b],
                ByteOrder::Big => [colour.b, colour.g, colour.r],
            };
            for (offset, value) in bytes.into_iter().enumerate() {
                write(offset, value)?;
            }
        }
        PixelFormat::Rgba8888 => {
            let bytes = match order {
                ByteOrder::Little => [colour.r, colour.g, colour.b, colour.a],
                ByteOrder::Big => [colour.a, colour.b, colour.g, colour.r],
            };
            for (offset, value) in bytes.into_iter().enumerate() {
                write(offset, value)?;
            }
        }
        PixelFormat::Grey8 => write(0, grey(colour))?,
        PixelFormat::Mono1 => {
            let slot = out.get_mut(byte).ok_or(ConvertError::BufferTooSmall)?;
            let mask = 0x80u8 >> (bit % BYTE_BITS);
            if grey(colour) >= MONO_THRESHOLD { *slot |= mask } else { *slot &= !mask }
        }
    }
    Ok(())
}

/// The index at column `x` of the row starting at byte `start`, packed as
/// `format` packs it (REQ-RND-017: at 4 bits the left pixel is the high
/// nibble); 0 beyond the buffer.
fn index_at(indices: &[u8], start: usize, x: usize, format: IndexFormat) -> u8 {
    match format {
        IndexFormat::Bits8 => indices.get(start.saturating_add(x)).copied().unwrap_or(0),
        IndexFormat::Bits4 => {
            let byte = indices.get(start.saturating_add(x / 2)).copied().unwrap_or(0);
            if x.is_multiple_of(2) { byte >> NIBBLE_BITS } else { byte & LOW_NIBBLE }
        }
    }
}

/// Converts a `width` by `height` image of palette indices (`stride` bytes
/// per row, packed at 8 or 4 bits per pixel as the row renderer writes them
/// in `index_format`) into `out` (`out_stride` bytes per output row),
/// rotated. `palette` is the 144 colours of the palette mode or the 32 of a
/// Colour Map (REQ-RND-030); an index beyond it is transparent.
///
/// # Errors
/// [`ConvertError::BufferTooSmall`] when `out` is too short for the
/// geometry and `out_stride` given. An index beyond the end of `indices`
/// reads as 0, as an index beyond `palette` is transparent.
#[allow(clippy::too_many_arguments)] // Each argument is a separate geometry or format parameter of REQ-RND-018.
pub fn convert(
    indices: &[u8],
    index_format: IndexFormat,
    width: usize,
    height: usize,
    stride: usize,
    palette: &[Rgba],
    format: PixelFormat,
    order: ByteOrder,
    rotation: Rotation,
    out: &mut [u8],
    out_stride: usize,
) -> Result<(), ConvertError> {
    for y in 0..height {
        for x in 0..width {
            let index = index_at(indices, y.saturating_mul(stride), x, index_format);
            let colour = palette.get(usize::from(index)).copied().unwrap_or(Rgba::TRANSPARENT);
            let (ox, oy) = match rotation {
                Rotation::None => (x, y),
                Rotation::Clockwise90 => (height.saturating_sub(1).saturating_sub(y), x),
                Rotation::Half => {
                    (width.saturating_sub(1).saturating_sub(x), height.saturating_sub(1).saturating_sub(y))
                }
                Rotation::Clockwise270 => (y, width.saturating_sub(1).saturating_sub(x)),
            };
            let bit = oy
                .saturating_mul(out_stride)
                .saturating_mul(BYTE_BITS)
                .saturating_add(ox.saturating_mul(format.bits()));
            put(out, bit, colour, format, order)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;
    use crate::palette::{CLUT0, palette};
    use std::vec;
    use std::vec::Vec;

    #[test]
    fn formats_and_rotation() {
        let colours = palette(&CLUT0, Rgba::TRANSPARENT, true, false);
        // Two pixels: red then white.
        let indices = [1u8, 7];
        let mut out = [0u8; 8];
        convert(
            &indices,
            IndexFormat::Bits8,
            2,
            1,
            2,
            &colours,
            PixelFormat::Rgb565,
            ByteOrder::Big,
            Rotation::None,
            &mut out,
            4,
        )
        .unwrap();
        assert_eq!(&out[..4], &[0xF8, 0x00, 0xFF, 0xFF]);
        let mut rotated = [0u8; 6];
        convert(
            &indices,
            IndexFormat::Bits8,
            2,
            1,
            2,
            &colours,
            PixelFormat::Rgb888,
            ByteOrder::Little,
            Rotation::Clockwise90,
            &mut rotated,
            3,
        )
        .unwrap();
        assert_eq!(&rotated, &[255, 0, 0, 255, 255, 255]);
        let mut mono = [0u8; 1];
        convert(
            &indices,
            IndexFormat::Bits8,
            2,
            1,
            2,
            &colours,
            PixelFormat::Mono1,
            ByteOrder::Little,
            Rotation::None,
            &mut mono,
            1,
        )
        .unwrap();
        assert_eq!(mono[0], 0b0100_0000);
        let mut small = [0u8; 2];
        assert!(
            convert(
                &indices,
                IndexFormat::Bits8,
                2,
                1,
                2,
                &colours,
                PixelFormat::Rgba8888,
                ByteOrder::Little,
                Rotation::None,
                &mut small,
                8
            )
            .is_err()
        );
    }

    /// The bytes of one pixel in `format` and `order`, written out from the
    /// formats' definitions for the test's model.
    fn model_pixel(colour: Rgba, format: PixelFormat, order: ByteOrder) -> Vec<u8> {
        let (r, g, b, a) = (colour.r, colour.g, colour.b, colour.a);
        let grey = ((77 * u32::from(r) + 150 * u32::from(g) + 29 * u32::from(b)) >> 8) as u8;
        match (format, order) {
            (PixelFormat::Rgb565, _) => {
                let value = (u16::from(r) >> 3) << 11 | (u16::from(g) >> 2) << 5 | u16::from(b) >> 3;
                if order == ByteOrder::Little { value.to_le_bytes().to_vec() } else { value.to_be_bytes().to_vec() }
            }
            (PixelFormat::Rgb888, ByteOrder::Little) => vec![r, g, b],
            (PixelFormat::Rgb888, ByteOrder::Big) => vec![b, g, r],
            (PixelFormat::Rgba8888, ByteOrder::Little) => vec![r, g, b, a],
            (PixelFormat::Rgba8888, ByteOrder::Big) => vec![a, b, g, r],
            (PixelFormat::Grey8, _) => vec![grey],
            (PixelFormat::Mono1, _) => vec![u8::from(grey >= 0x80)],
        }
    }

    /// REQ-394 (REQ-RND-018): every output format, both byte orders and all
    /// four rotations, from indices at 8 and at 4 bits per pixel, against a
    /// model that walks the output and finds each pixel's source by the
    /// inverse of the rotation.
    #[test]
    fn every_format_order_rotation_and_index_width() {
        let colours = palette(&CLUT0, Rgba::TRANSPARENT, true, false);
        let (width, height) = (3usize, 2usize);
        let source = [[1u8, 2, 3], [4, 5, 6]];
        let bits8: Vec<u8> = source.iter().flatten().copied().collect();
        // At 4 bits, two indices a byte, the left in the high nibble, each
        // row padded to whole bytes.
        let stride4 = width.div_ceil(2);
        let mut bits4 = vec![0u8; stride4 * height];
        for (y, row) in source.iter().enumerate() {
            for (x, &index) in row.iter().enumerate() {
                bits4[y * stride4 + x / 2] |= if x % 2 == 0 { index << 4 } else { index };
            }
        }
        let formats =
            [PixelFormat::Rgb565, PixelFormat::Rgb888, PixelFormat::Rgba8888, PixelFormat::Grey8, PixelFormat::Mono1];
        let rotations = [Rotation::None, Rotation::Clockwise90, Rotation::Half, Rotation::Clockwise270];
        for format in formats {
            for order in [ByteOrder::Little, ByteOrder::Big] {
                for rotation in rotations {
                    let turned = matches!(rotation, Rotation::Clockwise90 | Rotation::Clockwise270);
                    let (out_width, out_height) = if turned { (height, width) } else { (width, height) };
                    let out_stride = (out_width * format.bits()).div_ceil(8);
                    let mut expected = vec![0u8; out_stride * out_height];
                    for oy in 0..out_height {
                        for ox in 0..out_width {
                            let (x, y) = match rotation {
                                Rotation::None => (ox, oy),
                                Rotation::Clockwise90 => (oy, height - 1 - ox),
                                Rotation::Half => (width - 1 - ox, height - 1 - oy),
                                Rotation::Clockwise270 => (width - 1 - oy, ox),
                            };
                            let pixel = model_pixel(colours[usize::from(source[y][x])], format, order);
                            if format == PixelFormat::Mono1 {
                                expected[oy * out_stride + ox / 8] |= pixel[0] << (7 - ox % 8);
                            } else {
                                let at = oy * out_stride + ox * format.bits() / 8;
                                expected[at..at + pixel.len()].copy_from_slice(&pixel);
                            }
                        }
                    }
                    for (index_format, indices, stride) in
                        [(IndexFormat::Bits8, &bits8, width), (IndexFormat::Bits4, &bits4, stride4)]
                    {
                        let mut out = vec![0u8; expected.len()];
                        convert(
                            indices,
                            index_format,
                            width,
                            height,
                            stride,
                            &colours,
                            format,
                            order,
                            rotation,
                            &mut out,
                            out_stride,
                        )
                        .unwrap();
                        assert_eq!(out, expected, "{format:?} {order:?} {rotation:?} {index_format:?}");
                    }
                }
            }
        }
    }
}
