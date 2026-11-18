//! `neottx view` (REQ-CLI-004): the interactive viewer, or its refusal in a
//! build without it.

use crate::args::{GraphicsChoice, RasterArgs};
use neoteletext_app::Failure;
use neoteletext_app::input::Options;
#[cfg(feature = "view")]
use {
    super::{display, mosaic_style, sandbox_closed, scale_for},
    crate::ansi::ColourDepth,
    crate::frame::Frame,
    crate::terminal::{self, Display},
    neoteletext_app::exit,
    neoteletext_app::input,
    neoteletext_core::consts::ROWS,
    std::io::IsTerminal,
};

/// `neottx view`.
#[cfg(feature = "view")]
pub fn view(
    options: &Options,
    raster_args: &RasterArgs,
    graphics: GraphicsChoice,
    (legacy_mosaics, colours8): (bool, bool),
    (back_key, forward_key): (Option<char>, Option<char>),
) -> Result<(), Failure> {
    use neoteletext_core::consts::PAGE_WIDTH;
    // PAR-7: a picture file is shown as `show` shows it.
    if let Some(language) = crate::picture::language_of(&options.input) {
        return super::picture::show(language, &options.input, raster_args, graphics, options.no_sandbox);
    }
    let remote_keys = crate::view::RemoteKeys::new(back_key, forward_key).map_err(|error| (exit::USAGE, error))?;
    let opening = input::opening(&options.input).map_err(|error| (exit::USAGE, error.to_string()))?;
    let (mut session, mut source) = input::open_live(options)?;
    let (display, reason) = terminal::detect(display(graphics), std::io::stdout().is_terminal());
    let area = if display == Display::Text { None } else { terminal::text_area() };
    let height = ROWS.saturating_mul(Frame::still(session.decoder().config(), false).options.row_height());
    let scale = scale_for(raster_args, area, (PAGE_WIDTH, height));
    // PAR-7, PAR-31: a picture is shown with rows 0 and 24 around it.
    let regis = crate::picture::REGIS_FRAMED_SIZE;
    let regis_scale = scale_for(raster_args, area, regis);
    let naplps_scale = scale_for(raster_args, area, crate::picture::NAPLPS_FRAMED_SIZE);
    let fonts = super::picture::read_fonts(raster_args)?;
    sandbox_closed(options.no_sandbox)?;
    let depth = if colours8 { ColourDepth::Eight } else { ColourDepth::TrueColour };
    let look = crate::view::Look {
        display,
        scale,
        depth,
        mosaics: mosaic_style(legacy_mosaics),
        text_row_pixels: area.and_then(terminal::TextArea::text_row_pixels),
        regis_scale,
        naplps_scale,
    };
    let ran = crate::view::run(&mut session, source.as_mut(), opening, look, remote_keys, fonts)
        .map_err(|error| (exit::INPUT, error.to_string()));
    // REQ-RND-029: why characters were drawn, as `show` says it, once the
    // terminal is the user's again.
    if let (Some(reason), true) = (reason, options.verbose > 0) {
        eprintln!("neottx: used text output: {reason}");
    }
    ran
}

/// `neottx view` in a build without the viewer.
#[cfg(not(feature = "view"))]
pub fn view(
    _options: &Options,
    _raster_args: &RasterArgs,
    _graphics: GraphicsChoice,
    _text: (bool, bool),
    _remote_keys: (Option<char>, Option<char>),
) -> Result<(), Failure> {
    Err(neoteletext_app::not_in_build("the interactive viewer", "view"))
}
