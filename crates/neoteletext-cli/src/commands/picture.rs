//! `neottx render`, `show`, `view` and `check` of a ReGIS or NAPLPS file
//! (renderer parity PAR-7, REQ-6763 item 1). The files are read within
//! their bounds before the sandbox closes, and drawn after it.

use super::{ImageFormat, display, io_failure, sandbox_closed, sandbox_writable, scale_for};
use crate::args::{GraphicsChoice, RasterArgs};
use crate::image::{Scale, iterm2_inline, kitty_transmit, png, ppm, sixel};
use crate::picture::{Fonts, Language, PictureSource, draw, draw_source, framed};
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

/// `raster` enlarged by `scale`.
fn scaled(raster: &Raster, scale: Scale) -> Raster {
    crate::picture::scaled(raster, scale.x, scale.y)
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
) -> Result<Option<PictureSource>, Failure> {
    let path = Path::new(input);
    let is_t42 = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("t42"));
    if !is_t42 || !path.is_file() {
        return Ok(None);
    }
    let bytes = read_bounded(path, MAX_PICTURE_PAGE_FILE_BYTES)?;
    Ok(picture_of(crate::picture::pictures_in_t42(&bytes), page, subcode))
}

/// The picture of `page`, `subcode` among `pictures`: the sub-page's own,
/// or the page's only one.
pub fn picture_of(
    mut pictures: std::collections::BTreeMap<(neoteletext_core::types::PageAddress, u16), PictureSource>,
    page: neoteletext_core::types::PageAddress,
    subcode: neoteletext_core::types::SubCode,
) -> Option<PictureSource> {
    if let Some(own) = pictures.remove(&(page, subcode.value())) {
        return Some(own);
    }
    let mut on_page = pictures.into_iter().filter(|((address, _), _)| *address == page);
    match (on_page.next(), on_page.next()) {
        (Some((_, only)), None) => Some(only),
        _ => None,
    }
}

/// `neottx render` of a sub-page's picture: one image of it, its NAPLPS
/// objects composed.
pub fn render_source(
    source: &PictureSource,
    output: &Path,
    raster_args: &RasterArgs,
    image: ImageFormat,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let fonts = read_fonts(raster_args)?;
    sandbox_writable(Some(&output_folder(output)), no_sandbox)?;
    let (raster, _, _) = draw_source(source, &fonts).map_err(|why| (exit::INPUT, why))?;
    write_raster(&raster, output, raster_args, image)
}

/// The folder an output file is written in.
fn output_folder(output: &Path) -> PathBuf {
    output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// The fonts `--regis-font` and `--naplps-font` name.
pub fn read_fonts(raster_args: &RasterArgs) -> Result<Fonts, Failure> {
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
    sandbox_writable(Some(&output_folder(output)), no_sandbox)?;
    let (raster, _) = drawn(language, bytes, fonts)?;
    write_raster(&raster, output, raster_args, image)
}

/// Writes a drawn picture as an image file at `--scale`.
fn write_raster(raster: &Raster, output: &Path, raster_args: &RasterArgs, image: ImageFormat) -> Result<(), Failure> {
    let scale = Scale { x: raster_args.scale.max(1), y: raster_args.scale.max(1), aspect: false };
    let raster = scaled(raster, scale);
    let black = Rgba::opaque(0, 0, 0);
    let encoded = match image {
        ImageFormat::Png => png(&raster),
        ImageFormat::Ppm => ppm(&raster, black),
    }
    .map_err(|error| (exit::INPUT, error.to_string()))?;
    std::fs::write(output, encoded).map_err(|error| io_failure(output, error))
}

/// `neottx show` (and `view`) of a picture file: the picture as a kitty,
/// Sixel or iTerm2 image; a terminal of text alone cannot show one. A
/// picture file has no teletext page beneath, so it is shown alone.
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
    show_raster(&raster, raster_args, area, shown)
}

/// What `show` of a `.t42` sub-page's picture finds before the sandbox
/// closes: the fonts, the display and the terminal's size.
pub struct ShowSetup {
    fonts: Fonts,
    shown: Display,
    area: Option<terminal::TextArea>,
}

/// Reads the fonts and measures the terminal, before the sandbox.
pub fn show_setup(raster_args: &RasterArgs, graphics: GraphicsChoice) -> Result<ShowSetup, Failure> {
    let fonts = read_fonts(raster_args)?;
    let is_terminal = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let (shown, _) = terminal::detect(display(graphics), is_terminal);
    let area = if shown == Display::Text { None } else { terminal::text_area() };
    Ok(ShowSetup { fonts, shown, area })
}

/// `neottx show` of a `.t42` sub-page's picture, under the sandbox: the
/// picture with row 0 of the sub-page's teletext page, `page`, drawn whole,
/// above it and row 24 beneath (PAR-7, PAR-31).
pub fn show_framed(
    source: &PictureSource,
    setup: &ShowSetup,
    page: &Raster,
    raster_args: &RasterArgs,
) -> Result<(), Failure> {
    let (raster, _, _) = draw_source(source, &setup.fonts).map_err(|why| (exit::INPUT, why))?;
    let language = source.language.unwrap_or(Language::Regis);
    show_raster(&framed(language, &raster, page), raster_args, setup.area, setup.shown)
}

/// Writes a picture to the terminal as an image, scaled to fit `area`.
fn show_raster(
    raster: &Raster,
    raster_args: &RasterArgs,
    area: Option<terminal::TextArea>,
    shown: Display,
) -> Result<(), Failure> {
    let raster = scaled(raster, scale_for(raster_args, area, (raster.width, raster.height)));
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
/// (PAR-10), its characters no set shows as stand-ins; with `--to channel`
/// (`channel` the page and sub-code), a static channel's sub-page in the
/// folder `output` that carries it, as `convert --to channel` writes one
/// (PAR-8).
pub fn write_picture(
    picture: super::PictureChoice,
    language: Option<&str>,
    channel: Option<&str>,
    input: &Path,
    output: &Path,
    no_sandbox: bool,
) -> Result<(), Failure> {
    let bytes = read_bounded(input, TEXT_MAX_FILE_BYTES)?;
    let text = String::from_utf8(bytes).map_err(|_| io_failure(input, "not UTF-8"))?;
    let place = channel.map(channel_place).transpose()?;
    let mut out = if place.is_some() {
        std::fs::create_dir_all(output).map_err(|error| io_failure(output, error))?;
        sandbox_writable(Some(output), no_sandbox)?;
        None
    } else {
        let file = std::fs::File::create(output).map_err(|error| io_failure(output, error))?;
        sandbox_closed(no_sandbox)?;
        Some(file)
    };
    let rows = crate::picture::wrap(&text, language).map_err(|why| io_failure(input, why))?;
    let (written, kind) = match picture {
        super::PictureChoice::Regis => (crate::picture::regis_page(&rows), Language::Regis),
        super::PictureChoice::Naplps => (crate::picture::naplps_page(&rows), Language::Naplps),
    };
    let written = written.map_err(|why| io_failure(input, why))?;
    if let Some(out) = out.as_mut() {
        out.write_all(&written).and_then(|()| out.flush()).map_err(|error| io_failure(output, error))?;
    }
    if let Some((page, subcode)) = place {
        let mut sub_page = neoteletext_formats::page::PageFile::new();
        sub_page.address = page;
        sub_page.subcode = subcode;
        neoteletext_app::convert::write_channel(output, &[sub_page])?;
        let packets = crate::picture::picture_packets(page, subcode, kind, &written, None, &[], None)
            .map_err(|why| (exit::INPUT, why))?;
        let file = super::convert::sub_page_file(output, page, subcode);
        let mut sub_page_file =
            std::fs::OpenOptions::new().append(true).open(&file).map_err(|error| io_failure(&file, error))?;
        sub_page_file
            .write_all(&packets)
            .and_then(|()| sub_page_file.flush())
            .map_err(|error| io_failure(&file, error))?;
        eprintln!("{}: {} rows, a picture of {} bytes", file.display(), rows.len(), written.len());
        return Ok(());
    }
    eprintln!("{}: {} rows, {} bytes", output.display(), rows.len(), written.len());
    Ok(())
}

/// The page and sub-code `--page` names for a channel's sub-page: `200`
/// for sub-code 0000, or `200/0001`.
fn channel_place(
    text: &str,
) -> Result<(neoteletext_core::types::PageAddress, neoteletext_core::types::SubCode), Failure> {
    match super::parse_selection(text)? {
        super::Selection::Page(page, subcode) if subcode.is_any() => {
            Ok((page, neoteletext_core::types::SubCode::new(0)))
        }
        super::Selection::Page(page, subcode) => Ok((page, subcode)),
        super::Selection::All => Err((exit::USAGE, "--page names one page, e.g. 200/0001".to_owned())),
    }
}
