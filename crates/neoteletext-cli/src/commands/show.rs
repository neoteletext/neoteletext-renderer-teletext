//! `neottx show` (REQ-CLI-001): one page in the terminal, as a kitty,
//! Sixel or iTerm2 image or as characters (REQ-RND-029).

use super::{Selection, display, grid_of, mosaic_style, parse_selection, sandbox_closed, scale_for, selected};
use crate::ansi::{ColourDepth, ansi, plain};
use crate::args::{GraphicsChoice, RasterArgs};
use crate::frame::Frame;
use crate::image::{iterm2_inline, kitty_transmit, png, raster, sixel};
use crate::terminal::{self, Display};
use neoteletext_app::input::{self, Options};
use neoteletext_app::{Failure, exit};
use neoteletext_core::consts::{PAGE_WIDTH, ROWS};
use neoteletext_image::palette::Rgba;
use std::io::{IsTerminal, Write};

/// The kitty image id of the page `show` draws: one image, left on the
/// screen when the program ends.
const SHOWN_IMAGE: u32 = 1;

/// `neottx show`.
pub fn show(
    options: &Options,
    page: &str,
    raster_args: &RasterArgs,
    graphics: GraphicsChoice,
    legacy_mosaics: bool,
    colours8: bool,
) -> Result<(), Failure> {
    if let Some(language) = crate::picture::language_of(&options.input) {
        return super::picture::show(language, &options.input, raster_args, graphics, options.no_sandbox);
    }
    let selection = parse_selection(page)?;
    if selection == Selection::All {
        return Err((exit::USAGE, "show displays one page; use text or render for all".into()));
    }
    let is_terminal = std::io::stdout().is_terminal();
    let (display, reason) = terminal::detect(display(graphics), is_terminal);
    if let (Some(reason), true) = (reason, options.verbose > 0) {
        eprintln!("neottx: using text output: {reason}");
    }
    // Measure the terminal before the sandbox closes /dev/tty.
    let area = if display == Display::Text { None } else { terminal::text_area() };
    let mut session = input::load_sandboxed(options, selection.page(), || sandbox_closed(options.no_sandbox))?;
    let frame = Frame::still(session.decoder().config(), raster_args.reveal);
    let key = *selected(&session, selection)?.first().ok_or((exit::NOT_FOUND, "no page found".to_owned()))?;
    let grid = grid_of(&mut session, key)?;
    let mut out = std::io::stdout().lock();
    let black = Rgba::opaque(0, 0, 0);
    let image = || {
        let scale = scale_for(raster_args, area, (PAGE_WIDTH, ROWS.saturating_mul(frame.options.row_height())));
        raster(&grid, frame, scale, black)
    };
    let failed = |error: std::io::Error| (exit::INPUT, error.to_string());
    match display {
        Display::Kitty => {
            let encoded = png(&image()).map_err(failed)?;
            let _ = out.write_all(kitty_transmit(&encoded, SHOWN_IMAGE, true).as_bytes());
            let _ = out.write_all(b"\n");
        }
        Display::Sixel => {
            let _ = out.write_all(&sixel(&image(), black).map_err(failed)?);
            let _ = out.write_all(b"\n");
        }
        Display::Iterm2 => {
            let image = image();
            let encoded = png(&image).map_err(failed)?;
            let _ = out.write_all(iterm2_inline(&encoded, image.width, image.height).as_bytes());
            let _ = out.write_all(b"\n");
        }
        Display::Text => {
            let depth = if colours8 { ColourDepth::Eight } else { ColourDepth::TrueColour };
            let text = if is_terminal || graphics == GraphicsChoice::Text {
                ansi(&grid, raster_args.reveal, mosaic_style(legacy_mosaics), depth)
            } else {
                plain(&grid, raster_args.reveal, mosaic_style(legacy_mosaics))
            };
            let _ = out.write_all(text.as_bytes());
        }
    }
    Ok(())
}
