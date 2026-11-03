//! The Sixel image writer, for terminals that draw Sixel graphics
//! (REQ-RND-029, decision E.37 / DEC-561), as DEC's VT330/VT340 Programmer
//! Reference Manual, chapter 14 ("Sixel Graphics"), defines them.
//!
//! The image is the palette indices the row renderer drew. Each index is
//! its own colour register, as DEC-561 asks, so a page of up to 144 colours
//! needs a terminal with that many registers (xterm has 1,024 by default;
//! a VT340 has 16). The writer emits only the fixed sequences below around
//! data it encoded itself (REQ-SEC-010).

use crate::Sink;
use crate::palette::Rgba;

/// Device Control String introducer, then the Sixel parameters: aspect
/// ratio 0 (pixels 2:1, overridden by the raster attributes), background
/// 1 (pixels not written keep their colour), horizontal grid 0; then `q`,
/// the Sixel final character (VT340 manual, section 14.2).
const START: &[u8] = b"\x1bP0;1;0q";
/// String Terminator: the end of the Device Control String.
const END: &[u8] = b"\x1b\\";
/// Raster attributes: pixel aspect ratio 1:1, then the width and height
/// (section 14.3.2, `"Pan;Pad;Ph;Pv`).
const RASTER_ATTRIBUTES: &[u8] = b"\"1;1;";
/// A colour introducer (section 14.3.4, `#`).
const COLOUR: u8 = b'#';
/// The RGB colour coordinate system, in a colour definition.
const RGB_SYSTEM: &[u8] = b";2;";
/// Graphics carriage return: back to the band's first column.
const CARRIAGE_RETURN: u8 = b'$';
/// Graphics new line: the next band of six rows.
const NEW_LINE: u8 = b'-';
/// A repeat introducer: `!` count, then the sixel to repeat.
const REPEAT: u8 = b'!';
/// Rows of pixels in one band: a sixel is six pixels high.
const BAND_ROWS: usize = 6;
/// A sixel's character is 0x3F plus its six bits, top pixel least
/// significant (section 14.3.1).
const SIXEL_BASE: u8 = 0x3F;
/// Colour components in a definition run from 0 to 100 per cent.
const PERCENT: u32 = 100;
/// The largest 8-bit component.
const COMPONENT_MAX: u32 = 255;
/// Runs of at least this many equal sixels are written with `!`, shorter
/// ones as they are: `!4x` is no longer than `xxxx`.
const REPEAT_FROM: usize = 4;
/// The most palette entries a raster has: the palette mode's 144
/// (REQ-RND-030).
const MAX_REGISTERS: usize = 256;

/// Writes a decimal number.
fn decimal<S: Sink>(sink: &mut S, value: usize) -> Result<(), S::Error> {
    let mut digits = [0u8; 20];
    let mut position = digits.len();
    let mut rest = value;
    loop {
        position = position.saturating_sub(1);
        if let Some(slot) = digits.get_mut(position) {
            *slot = b'0'.saturating_add((rest % 10) as u8);
        }
        rest /= 10;
        if rest == 0 || position == 0 {
            break;
        }
    }
    sink.write_all(digits.get(position..).unwrap_or(&[]))
}

/// An 8-bit component as a percentage, rounded.
fn percent(component: u8) -> usize {
    let scaled = u32::from(component).saturating_mul(PERCENT).saturating_add(COMPONENT_MAX / 2) / COMPONENT_MAX;
    usize::try_from(scaled).unwrap_or(0)
}

/// Writes a `width` by `height` image of palette indices as one Sixel
/// image. A transparent palette entry, or an index beyond the palette,
/// takes `fill`, as a format with no transparency does (REQ-RND-006).
///
/// # Errors
/// The sink's error, when it cannot take the bytes.
pub fn write<S: Sink>(
    sink: &mut S,
    indices: &[u8],
    width: usize,
    height: usize,
    palette: &[Rgba],
    fill: Rgba,
) -> Result<(), S::Error> {
    sink.write_all(START)?;
    sink.write_all(RASTER_ATTRIBUTES)?;
    decimal(sink, width)?;
    sink.write_all(b";")?;
    decimal(sink, height)?;
    // Every register an index can name, defined once.
    let mut used = [false; MAX_REGISTERS];
    for &index in indices.iter().take(width.saturating_mul(height)) {
        if let Some(slot) = used.get_mut(usize::from(index)) {
            *slot = true;
        }
    }
    for (register, _) in used.iter().enumerate().filter(|&(_, &used)| used) {
        let colour = palette.get(register).copied().filter(|colour| colour.a != 0).unwrap_or(fill);
        sink.write_all(&[COLOUR])?;
        decimal(sink, register)?;
        sink.write_all(RGB_SYSTEM)?;
        decimal(sink, percent(colour.r))?;
        sink.write_all(b";")?;
        decimal(sink, percent(colour.g))?;
        sink.write_all(b";")?;
        decimal(sink, percent(colour.b))?;
    }
    let pixel = |x: usize, y: usize| indices.get(y.saturating_mul(width).saturating_add(x)).copied();
    let mut top = 0;
    while top < height {
        let rows = (height.saturating_sub(top)).min(BAND_ROWS);
        let mut first_colour = true;
        for (register, _) in used.iter().enumerate().filter(|&(_, &used)| used) {
            let sixel_at = |x: usize| {
                let mut bits = 0u8;
                for row in 0..rows {
                    if pixel(x, top.saturating_add(row)).is_some_and(|index| usize::from(index) == register) {
                        bits |= 1 << row;
                    }
                }
                bits
            };
            if (0..width).all(|x| sixel_at(x) == 0) {
                continue;
            }
            if !first_colour {
                sink.write_all(&[CARRIAGE_RETURN])?;
            }
            first_colour = false;
            sink.write_all(&[COLOUR])?;
            decimal(sink, register)?;
            let mut x = 0;
            while x < width {
                let bits = sixel_at(x);
                let mut run = 1;
                while x.saturating_add(run) < width && sixel_at(x.saturating_add(run)) == bits {
                    run = run.saturating_add(1);
                }
                let character = SIXEL_BASE.saturating_add(bits);
                if run >= REPEAT_FROM {
                    sink.write_all(&[REPEAT])?;
                    decimal(sink, run)?;
                    sink.write_all(&[character])?;
                } else {
                    for _ in 0..run {
                        sink.write_all(&[character])?;
                    }
                }
                x = x.saturating_add(run);
            }
        }
        top = top.saturating_add(BAND_ROWS);
        if top < height {
            sink.write_all(&[NEW_LINE])?;
        }
    }
    sink.write_all(END)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]
mod tests {
    use super::*;
    use crate::SliceSink;

    const RED: Rgba = Rgba::opaque(255, 0, 0);
    const BLUE: Rgba = Rgba::opaque(0, 0, 255);

    /// REQ-382 (REQ-RND-029): a 3 by 7 image, red above blue, written as
    /// the VT340 manual's chapter 14 defines, every byte as expected: the
    /// raster attributes, the registers by palette index, two bands, and
    /// runs.
    #[test]
    fn a_small_image_is_written_exactly() {
        let mut indices = [0u8; 21];
        for (position, index) in indices.iter_mut().enumerate() {
            // Rows 0 to 3 red (index 1), rows 4 to 6 blue (index 4).
            *index = if position / 3 < 4 { 1 } else { 4 };
        }
        let palette = [Rgba::TRANSPARENT, RED, Rgba::TRANSPARENT, Rgba::TRANSPARENT, BLUE];
        let mut buffer = [0u8; 256];
        let mut sink = SliceSink::new(&mut buffer);
        write(&mut sink, &indices, 3, 7, &palette, Rgba::opaque(0, 0, 0)).unwrap();
        // Band 1 (rows 0 to 5): red has bits 0 to 3 (0x0F + 0x3F = 'N'),
        // blue bits 4 and 5 (0x30 + 0x3F = 'o'). Band 2 (row 6): blue bit 0
        // ('@').
        assert_eq!(sink.written(), b"\x1bP0;1;0q\"1;1;3;7#1;2;100;0;0#4;2;0;0;100#1NNN$#4ooo-#4@@@\x1b\\");
    }

    /// Runs of four or more are written with the repeat introducer, and a
    /// transparent entry takes the fill colour.
    #[test]
    fn long_runs_repeat_and_transparent_takes_the_fill() {
        let indices = [0u8; 10];
        let mut buffer = [0u8; 128];
        let mut sink = SliceSink::new(&mut buffer);
        write(&mut sink, &indices, 10, 1, &[Rgba::TRANSPARENT], Rgba::opaque(255, 255, 255)).unwrap();
        assert_eq!(sink.written(), b"\x1bP0;1;0q\"1;1;10;1#0;2;100;100;100#0!10@\x1b\\");
    }
}
