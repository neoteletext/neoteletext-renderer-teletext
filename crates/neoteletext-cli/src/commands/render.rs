//! `neottx render` (REQ-RND-007): one page, or all, as image files; and
//! with `--frames`, one page played into a sequence of images
//! (animation.md ANIM-18, REQ-6529).

use super::font::{parse_sha256, parse_table, read_font};
use super::io_failure;
use super::sequence;
use super::{ImageFormat, Selection, grid_of, page_file_name, parse_selection, sandbox_writable, scale_for, selected};
use crate::args::{RasterArgs, SequenceArgs};
use crate::frame::Frame;
use crate::image::{png, ppm, raster_in};
use crate::script::{self, MAX_SCRIPT_BYTES, Step};
use neoteletext_app::input::{self, Options};
use neoteletext_app::{Failure, exit};
use neoteletext_core::glyphs::conformance::FONT_SET_COUNT;
use neoteletext_core::glyphs::{GlyphSet, LoadedFont};
use neoteletext_core::level1::Grid;
use neoteletext_image::palette::Rgba;
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// The digits of a tick in an image's name (ANIM-18): five, which hold the
/// last tick of the longest sequence.
const TICK_DIGITS: usize = 5;
/// The script path that reads standard input.
const STANDARD_INPUT: &str = "-";

/// Reads a script, a file or standard input, within its bound, and parses
/// it (ANIM-18). It is read before the sandbox closes, as every input is.
fn read_script(path: &Path) -> Result<Vec<Step>, Failure> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_SCRIPT_BYTES).unwrap_or(u64::MAX).saturating_add(1);
    let read = if path.as_os_str() == STANDARD_INPUT {
        std::io::stdin().lock().take(limit).read_to_end(&mut bytes)
    } else {
        std::fs::File::open(path).and_then(|file| file.take(limit).read_to_end(&mut bytes))
    };
    read.map_err(|error| io_failure(path, error))?;
    if bytes.len() > MAX_SCRIPT_BYTES {
        return Err(io_failure(path, format!("a script is at most {MAX_SCRIPT_BYTES} bytes")));
    }
    let text = String::from_utf8(bytes).map_err(|_| io_failure(path, "a script is text in UTF-8"))?;
    script::parse(&text).map_err(|why| io_failure(path, why))
}

/// The file of the image at `tick` of a sequence written to `output`:
/// `-NNNNN`, the tick in five digits, before the extension (ANIM-18).
fn sequence_file(output: &Path, tick: u32) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let mut name = format!("{stem}-{tick:0TICK_DIGITS$}");
    if let Some(extension) = output.extension() {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    output.with_file_name(name)
}

/// `neottx render`.
pub fn render(
    options: &Options,
    page: &str,
    output: &Path,
    raster_args: &RasterArgs,
    image: ImageFormat,
    sequence_args: &SequenceArgs,
) -> Result<(), Failure> {
    let selection = parse_selection(page)?;
    if sequence_args.frames.is_some() && selection == Selection::All {
        return Err((exit::USAGE, "--frames plays one page: name it, not `all`".to_owned()));
    }
    let script = sequence_args.script.as_deref().map(read_script).transpose()?.unwrap_or_default();
    let expected = raster_args.font_sha256.as_deref().map(parse_sha256).transpose()?;
    let directory: PathBuf = if selection == Selection::All {
        std::fs::create_dir_all(output).map_err(|error| io_failure(output, error))?;
        output.to_path_buf()
    } else {
        output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    };
    // The font file is read, within its bound, or fetched by the fetcher
    // process, before the sandbox, and parsed after it with the input
    // (REQ-SEC-017, REQ-123).
    let font_file = raster_args
        .font
        .as_deref()
        .map(|path| read_font(path, expected.as_ref(), options).map(|bytes| (path, bytes)))
        .transpose()?;
    let mut session =
        input::load_sandboxed(options, selection.page(), || sandbox_writable(Some(&directory), options.no_sandbox))?;
    let frame = Frame::still(session.decoder().config(), raster_args.reveal);
    let scale = scale_for(raster_args, None, (0, 0));
    let table = font_file.map(|(path, bytes)| parse_table(path, &bytes, expected.as_ref())).transpose()?;
    let loaded =
        table.as_deref().map(LoadedFont::new).transpose().map_err(|error| (exit::INPUT, format!("{error:?}")))?;
    let glyphs = GlyphSet::noto_teletext().with_fonts(loaded, [None; FONT_SET_COUNT]);
    let pages = selected(&session, selection)?;
    let extension = match image {
        ImageFormat::Png => "png",
        ImageFormat::Ppm => "ppm",
    };
    let write = |grid: &Grid, frame: Frame, target: &Path| {
        let picture = match image {
            ImageFormat::Png => Rgba::TRANSPARENT,
            ImageFormat::Ppm => Rgba::opaque(0, 0, 0),
        };
        let image_data = raster_in(grid, frame, scale, picture, &glyphs);
        let bytes = match image {
            ImageFormat::Png => png(&image_data),
            ImageFormat::Ppm => ppm(&image_data, Rgba::opaque(0, 0, 0)),
        }
        .map_err(|error| (exit::INPUT, error.to_string()))?;
        std::fs::write(target, bytes).map_err(|error| io_failure(target, error))
    };
    for key in pages {
        if let Some(ticks) = sequence_args.frames {
            // ANIM-18: each image shows the page at its tick, so flash is
            // in the phase of that tick (ANIM-1), not held on as a still's.
            let config = *session.decoder().config();
            sequence::play(&mut session, key, &script, (ticks, sequence_args.frame_step), |tick, grid| {
                let frame = Frame::at_tick(&config, raster_args.reveal, tick);
                write(grid, frame, &sequence_file(output, tick))
            })?;
            continue;
        }
        let grid = grid_of(&mut session, key)?;
        let target = if selection == Selection::All {
            directory.join(page_file_name(key, extension))
        } else {
            output.to_path_buf()
        };
        write(&grid, frame, &target)?;
    }
    Ok(())
}
