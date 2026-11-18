//! `neottx render`, `show`, `view` and `check` of a ReGIS or NAPLPS file
//! (renderer parity PAR-7, REQ-6763 item 1). The files are read within
//! their bounds before the sandbox closes, and drawn after it.

use super::{ImageFormat, display, io_failure, sandbox_closed, sandbox_writable, scale_for};
use crate::args::{GraphicsChoice, RasterArgs};
use crate::image::{Scale, iterm2_inline, kitty_transmit, png, ppm, sixel};
use crate::picture::{Fonts, Language, draw};
use crate::terminal::{self, Display};
use neoteletext_app::{Failure, exit};
use neoteletext_formats::picture_text::MAX_PICTURE_STREAM_BYTES;
use neoteletext_image::palette::Rgba;
use neoteletext_image::raster::Raster;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// The kitty image id of a picture `show` draws.
const SHOWN_IMAGE: u32 = 1;

/// A file's bytes, refused past `bound`.
fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>, Failure> {
    let file = std::fs::File::open(path).map_err(|error| io_failure(path, error))?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(bound).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut bytes).map_err(|error| io_failure(path, error))?;
    if bytes.len() > bound {
        return Err(io_failure(path, format!("larger than the {bound} bytes accepted")));
    }
    Ok(bytes)
}

/// A font file's text, within the renderer's bound for it.
fn read_font_text(path: &Path, bound: usize) -> Result<String, Failure> {
    String::from_utf8(read_bounded(path, bound)?).map_err(|_| io_failure(path, "a font file is text in UTF-8"))
}

/// The picture's bytes and the fonts `--regis-font` and `--naplps-font`
/// name, read before the sandbox. A picture file is bounded as a static
/// channel's picture is (PAR-8): `MAX_PICTURE_STREAM_BYTES`.
pub fn read_picture(input: &str, raster_args: &RasterArgs) -> Result<(Vec<u8>, Fonts), Failure> {
    let bytes = read_bounded(Path::new(input), MAX_PICTURE_STREAM_BYTES)?;
    let fonts = read_fonts(raster_args)?;
    Ok((bytes, fonts))
}

/// `raster` enlarged `scale` times across and down.
fn scaled(raster: &Raster, scale: Scale) -> Raster {
    let (across, down) = (scale.x.max(1), scale.y.max(1));
    let width = raster.width.saturating_mul(across);
    let mut indices = Vec::with_capacity(width.saturating_mul(raster.height).saturating_mul(down));
    for row in raster.indices.chunks(raster.width.max(1)) {
        let line: Vec<u8> = row.iter().flat_map(|&index| core::iter::repeat_n(index, across)).collect();
        for _ in 0..down {
            indices.extend_from_slice(&line);
        }
    }
    Raster { width, height: raster.height.saturating_mul(down), indices, palette: raster.palette.clone() }
}

/// The picture drawn, refusing what the renderers refuse to set up.
fn drawn(language: Language, bytes: &[u8], fonts: &Fonts) -> Result<(Raster, crate::picture::Findings), Failure> {
    draw(language, bytes, fonts).map_err(|why| (exit::INPUT, why))
}

/// The most bytes of a `.t42` file read for the pictures it carries: a
/// static sub-page's file holds its page and its picture's records, and a
/// stream of `MAX_PICTURE_STREAM_BYTES` takes a packet of `PACKET_BYTES`
/// for every `FRAGMENT_DATA` of it; twice that leaves room for the
/// records' framing, the links, the cursor and the page (PAR-8).
const MAX_PICTURE_PAGE_FILE_BYTES: usize = MAX_PICTURE_STREAM_BYTES
    .div_ceil(neoteletext_web::records::FRAGMENT_DATA)
    .saturating_mul(neoteletext_core::consts::PACKET_BYTES)
    .saturating_mul(2);

/// The picture a `.t42` file carries for `page`, `subcode` (PAR-7, PAR-8):
/// the sub-page's own, or the page's only one; `None` when the input is no
/// `.t42` file or carries none for it. Read before the sandbox.
pub fn t42_picture(
    input: &str,
    page: neoteletext_core::types::PageAddress,
    subcode: neoteletext_core::types::SubCode,
) -> Result<Option<(Language, Vec<u8>)>, Failure> {
    let path = Path::new(input);
    let is_t42 = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("t42"));
    if !is_t42 || !path.is_file() {
        return Ok(None);
    }
    let bytes = read_bounded(path, MAX_PICTURE_PAGE_FILE_BYTES)?;
    let pictures = crate::picture::pictures_in_t42(&bytes);
    let mut on_page = pictures.iter().filter(|((address, _), _)| *address == page);
    let chosen = pictures.get(&(page, subcode.value())).or_else(|| match (on_page.next(), on_page.next()) {
        (Some((_, only)), None) => Some(only),
        _ => None,
    });
    Ok(chosen.and_then(|source| source.language.map(|language| (language, source.stream.clone()))))
}

/// `neottx render` of a picture's bytes: one image of it.
pub fn render_bytes(
    language: Language,
    bytes: &[u8],
    output: &Path,
    raster_args: &RasterArgs,
    image: ImageFormat,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let fonts = read_fonts(raster_args)?;
    write_image(language, bytes, &fonts, output, raster_args, image, no_sandbox)
}

/// The fonts `--regis-font` and `--naplps-font` name.
fn read_fonts(raster_args: &RasterArgs) -> Result<Fonts, Failure> {
    Ok(Fonts {
        regis: raster_args
            .regis_font
            .as_deref()
            .map(|path| read_font_text(path, regis::limits::MAX_FONT_BYTES))
            .transpose()?,
        naplps: raster_args
            .naplps_font
            .as_deref()
            .map(|path| read_font_text(path, naplps::limits::MAX_STROKE_FONT_BYTES))
            .transpose()?,
    })
}

/// `neottx render` of a picture file: one image of it.
pub fn render(
    language: Language,
    input: &str,
    output: &Path,
    raster_args: &RasterArgs,
    image: ImageFormat,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let (bytes, fonts) = read_picture(input, raster_args)?;
    write_image(language, &bytes, &fonts, output, raster_args, image, no_sandbox)
}

/// Draws a picture and writes it as an image file, the sandbox closed but
/// for the output's folder.
fn write_image(
    language: Language,
    bytes: &[u8],
    fonts: &Fonts,
    output: &Path,
    raster_args: &RasterArgs,
    image: ImageFormat,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let directory: PathBuf = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    sandbox_writable(Some(&directory), no_sandbox)?;
    let (raster, _) = drawn(language, bytes, fonts)?;
    let scale = Scale { x: raster_args.scale.max(1), y: raster_args.scale.max(1), aspect: false };
    let raster = scaled(&raster, scale);
    let black = Rgba::opaque(0, 0, 0);
    let encoded = match image {
        ImageFormat::Png => png(&raster),
        ImageFormat::Ppm => ppm(&raster, black),
    }
    .map_err(|error| (exit::INPUT, error.to_string()))?;
    std::fs::write(output, encoded).map_err(|error| io_failure(output, error))
}

/// `neottx show` (and `view`) of a picture file: the picture as a kitty,
/// Sixel or iTerm2 image; a terminal of text alone cannot show one.
pub fn show(
    language: Language,
    input: &str,
    raster_args: &RasterArgs,
    graphics: GraphicsChoice,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let (bytes, fonts) = read_picture(input, raster_args)?;
    let is_terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let (shown, _) = terminal::detect(display(graphics), is_terminal);
    let area = if shown == Display::Text { None } else { terminal::text_area() };
    sandbox_closed(no_sandbox)?;
    let (raster, _) = drawn(language, &bytes, &fonts)?;
    let raster = scaled(&raster, scale_for(raster_args, area, (raster.width, raster.height)));
    let failed = |error: std::io::Error| (exit::INPUT, error.to_string());
    let black = Rgba::opaque(0, 0, 0);
    let mut out = std::io::stdout().lock();
    match shown {
        Display::Kitty => {
            let encoded = png(&raster).map_err(failed)?;
            let _ = out.write_all(kitty_transmit(&encoded, SHOWN_IMAGE, true).as_bytes());
        }
        Display::Sixel => {
            let _ = out.write_all(&sixel(&raster, black).map_err(failed)?);
        }
        Display::Iterm2 => {
            let encoded = png(&raster).map_err(failed)?;
            let _ = out.write_all(iterm2_inline(&encoded, raster.width, raster.height).as_bytes());
        }
        Display::Text => {
            return Err((exit::USAGE, "a picture needs a terminal that shows images (kitty, Sixel or iTerm2)".into()));
        }
    }
    let _ = out.write_all(b"\n");
    Ok(())
}

/// A picture file's bytes for `neottx check`, read before the sandbox.
pub fn read_checked(path: &Path) -> Result<Vec<u8>, Failure> {
    read_bounded(path, MAX_PICTURE_STREAM_BYTES)
}

/// `neottx check` of a picture file's bytes: what the server's checks
/// report (PAR-23), each finding a line; `true` when one is a refusal.
pub fn check(language: Language, path: &Path, bytes: &[u8]) -> Result<bool, Failure> {
    let (_, findings) = drawn(language, bytes, &Fonts::default())?;
    for line in &findings.lines {
        println!("{}: {line}", path.display());
    }
    Ok(findings.refused)
}

/// The largest text file `write-text --picture` reads: as for a page
/// (23 rows of 40 cells at most six UTF-8 bytes a character, with room for
/// long lines that are refused).
const TEXT_MAX_FILE_BYTES: usize = 16 * 1024;

/// `neottx write-text --picture`: a ReGIS or NAPLPS picture of the text
/// (PAR-10), its characters no set shows as stand-ins.
pub fn write_picture(
    picture: super::PictureChoice,
    language: Option<&str>,
    input: &Path,
    output: &Path,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let bytes = read_bounded(input, TEXT_MAX_FILE_BYTES)?;
    let text = String::from_utf8(bytes).map_err(|_| io_failure(input, "not UTF-8"))?;
    let mut out = std::fs::File::create(output).map_err(|error| io_failure(output, error))?;
    sandbox_closed(no_sandbox)?;
    let rows = crate::picture::wrap(&text, language).map_err(|why| io_failure(input, why))?;
    let written = match picture {
        super::PictureChoice::Regis => crate::picture::regis_page(&rows),
        super::PictureChoice::Naplps => crate::picture::naplps_page(&rows),
    }
    .map_err(|why| io_failure(input, why))?;
    out.write_all(&written).and_then(|()| out.flush()).map_err(|error| io_failure(output, error))?;
    eprintln!("{}: {} rows, {} bytes", output.display(), rows.len(), written.len());
    Ok(())
}
