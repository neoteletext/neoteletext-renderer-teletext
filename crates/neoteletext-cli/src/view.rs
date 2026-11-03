//! The interactive viewer, `neottx view` (REQ-CLI-004, decision 19 of
//! IMPLEMENTATION-LOG.md): the core viewer machine driven by terminal keys,
//! with the TTI Link Extension (Appendix E, REQ-NAV-011 and 012) and the
//! security confirmations of area SEC.

use crate::ansi::{ColourDepth, MosaicStyle, ansi};
use crate::frame::Frame;
use crate::image::{
    Raster, Scale, iterm2_inline, kitty_free, kitty_hide, kitty_place, kitty_transmit, png, raster, sixel,
};
use crate::terminal::Display;
use crossterm::event::{
    self, EnableBracketedPaste, EnableFocusChange, KeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{cursor, execute, queue, terminal};
use neoteletext_app::extras::PageExtras;
use neoteletext_app::input::Opening;
use neoteletext_app::session::Session;
use neoteletext_app::source::Source;
use neoteletext_core::acq::{EnhancementReport, PageBuild, StoredPage};
use neoteletext_core::config::Config;
use neoteletext_core::consts::COLUMNS;
use neoteletext_core::level1::{Grid, Zoom};
use neoteletext_core::render::ZOOM_ROWS;
use neoteletext_core::service::initial_page;
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_core::viewer::{Action, Viewer, ViewerEvent};
use neoteletext_image::palette::Rgba;
use std::collections::HashSet;
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant as StdInstant;
use terminal_io::{RawGuard, Wakened};
use typing::Typing;

mod input;
mod keys;
mod notices;
mod page;
mod terminal_io;
mod turn;
mod typing;

/// What the viewer keeps from one turn of its loop to the next.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact of the viewer's session, set and read apart"
)]
struct ViewState {
    start: StdInstant,
    initial: PageAddress,
    // The address opened named a page, which the channel's initial page,
    // when it comes, does not replace (REQ-1693 item 1).
    named: bool,
    // The channel's descriptor gave the initial page, which then stands
    // over packet 8/30's (DEC-2410).
    initial_from_channel: bool,
    viewer: Viewer,
    messages: Vec<String>,
    live: bool,
    grid: Grid,
    last_drawn: Option<turn::DrawnState>,
    // The next drawing sends the whole page: the terminal was resized.
    repaint_whole: bool,
    action: Action,
    prompt: Option<[u8; COLUMNS]>,
    // The origins confirmed this session (REQ-288), kept across services.
    confirmed_origins: HashSet<String>,
    typing: Typing,
    // The reception of the page whose walk was last traced (REQ-FSM-010).
    traced: Option<(neoteletext_core::fsm::PageKey, Instant)>,
    // The walk's trace entries of this turn, written with the decoder's.
    walk_trace: Vec<neoteletext_core::acq::Event>,
    // The trace could not be written, as standard error is the terminal.
    trace_withheld: bool,
    // The report of the last grid built. The page and store stay borrowed
    // until the key is handled, so it is recorded at the start of the next
    // pass, when nothing borrows the decoder.
    unrecorded: Option<EnhancementReport>,
    // A sub-page step waiting for a part of the page's list: forward or
    // back (REQ-1888).
    pending_step: Option<bool>,
    // The build of the last grid built and whether its designation fell
    // back (REQ-72), recorded with the report above.
    unrecorded_designation: Option<(PageBuild, bool)>,
}

/// What a turn knows about the page it shows, for the steps after the
/// page is found.
struct Shown<'a> {
    page: Option<&'a StoredPage>,
    extras: Option<&'a PageExtras>,
    config: &'a Config,
    now: Instant,
}

impl ViewState {
    /// The viewer at the start of `session`'s service, started at `start`.
    fn new(session: &Session, start: StdInstant, opening: Opening) -> Self {
        let (initial, subcode) = opening.page.unwrap_or((initial_page(session.service().as_ref()), SubCode::ANY));
        let mut viewer = Viewer::new(initial).with_trace(session.decoder().config().trace);
        let mut action = viewer.event(ViewerEvent::Started);
        if subcode != SubCode::ANY {
            action = viewer.show_subpage(subcode);
        }
        let mut typing = Typing::new();
        typing.opened_with((initial.mpp(), subcode.value()), opening.parameters);
        Self {
            start,
            initial,
            named: opening.page.is_some(),
            initial_from_channel: false,
            viewer,
            messages: Vec::new(),
            live: true,
            grid: Grid::EMPTY,
            last_drawn: None,
            repaint_whole: true,
            action,
            prompt: None,
            confirmed_origins: HashSet::new(),
            typing,
            traced: None,
            walk_trace: Vec::new(),
            trace_withheld: false,
            unrecorded: None,
            pending_step: None,
            unrecorded_designation: None,
        }
    }

    /// The messages the source gave, for after the terminal is restored.
    fn into_messages(mut self) -> Vec<String> {
        if self.trace_withheld {
            self.messages.push(
                "the trace of -vv was not written while the viewer held the terminal; \
                 redirect standard error, as in `neottx view -vv INPUT 2>trace.txt`"
                    .to_owned(),
            );
        }
        self.messages
    }

    /// Writes the trace kept this turn, the decoder's, the walk's and the
    /// viewer's, as the other commands print it with `-vv` (REQ-FSM-010): to `out`,
    /// standard error, when it is not the terminal the viewer draws on,
    /// where its lines would spoil the page; otherwise it is dropped, and
    /// the viewer says so when it ends. Nothing is kept from turn to turn.
    fn write_trace(&mut self, session: &mut Session, out: &mut dyn Write, out_is_terminal: bool) {
        session.trace.append(&mut self.walk_trace);
        let now = Instant(self.start.elapsed().as_micros().try_into().unwrap_or(u64::MAX));
        let lost = self.viewer.take_trace(now, &mut |entry| {
            session.trace.push(neoteletext_core::acq::Event::Trace(entry));
        });
        if session.trace.is_empty() {
            return;
        }
        if out_is_terminal {
            self.trace_withheld = true;
            session.trace.clear();
            return;
        }
        for event in session.trace.drain(..) {
            let _ = writeln!(out, "neottx: {event:?}");
        }
        if lost > 0 {
            let _ = writeln!(out, "neottx: {lost} viewer transitions came between two passes and were not kept");
        }
    }
}

/// How the viewer draws pages: the protocol, the scale and the text
/// output's colours and mosaics, as the command chose them.
#[derive(Clone, Copy)]
pub struct Look {
    /// The graphics protocol, or characters.
    pub display: Display,
    /// The scale of a picture.
    pub scale: Scale,
    /// The colours of the text output.
    pub depth: ColourDepth,
    /// The mosaics of the text output.
    pub mosaics: MosaicStyle,
    /// The height of a text row in pixels, when the terminal reports its
    /// size in pixels and the rows divide it exactly; a Sixel redraw of
    /// the changed rows alone needs it (REQ-RND-029).
    pub text_row_pixels: Option<usize>,
}

/// Runs the viewer until the user quits.
pub fn run(session: &mut Session, source: &mut dyn Source, opening: Opening, look: Look) -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    // Releases and repeats where the terminal has the Kitty keyboard
    // protocol, a paste as one event, and the focus (`docs/user-guide.md`, "The viewer").
    let enhanced = terminal::supports_keyboard_enhancement().unwrap_or(false);
    let guard = RawGuard::new(enhanced);
    let mut out = std::io::stdout();
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide, EnableBracketedPaste, EnableFocusChange)?;
    if enhanced {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
        execute!(out, PushKeyboardEnhancementFlags(flags))?;
    }

    let start = StdInstant::now();
    let mut state = ViewState::new(session, start, opening);
    session.keep(neoteletext_input::is_input_packet);
    // The viewer waits for the terminal and the source together: a key,
    // and a page the source has for it, are taken at once; the tick is
    // only the longest wait, for flash and the ticker (REQ-INT-008).
    let (woken, wakes) = mpsc::channel();
    let terminal = woken.clone();
    // Held for as long as the viewer runs, so the wait never ends early for
    // want of a sender: a file source drops the wake it is given.
    let _held = woken.clone();
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if terminal.send(Wakened::Terminal(event)).is_err() {
                return;
            }
        }
    });
    source.set_wake(Arc::new(move || {
        let _ = woken.send(Wakened::Source);
    }));
    let display = look.display;
    let mut drawer = Drawer::new(look);
    state.run(session, source, &wakes, display, &mut |grid, frame, whole| drawer.draw(&mut out, grid, frame, whole))?;
    drawer.kitty.free_all(&mut out)?;
    drop(guard);
    for message in state.into_messages() {
        eprintln!("neottx: {message}");
    }
    Ok(())
}

/// How the viewer draws a page, and what it keeps of the pages drawn: the
/// kitty images, and the last Sixel picture.
struct Drawer {
    display: Display,
    scale: Scale,
    depth: ColourDepth,
    mosaics: MosaicStyle,
    kitty: KittyImages,
    sixel: SixelRows,
}

impl Drawer {
    fn new(look: Look) -> Self {
        Self {
            display: look.display,
            scale: look.scale,
            depth: look.depth,
            mosaics: look.mosaics,
            kitty: KittyImages::default(),
            sixel: SixelRows { last: None, text_row_pixels: look.text_row_pixels },
        }
    }

    /// Draws `frame` of `grid`. `whole` asks for the whole page even where
    /// only changed rows would be sent, as after the terminal was resized.
    fn draw(&mut self, out: &mut impl Write, grid: &Grid, frame: Frame, whole: bool) -> std::io::Result<()> {
        queue!(out, cursor::MoveTo(0, 0))?;
        let black = Rgba::opaque(0, 0, 0);
        match self.display {
            Display::Kitty => {
                if whole {
                    self.kitty.free_all(out)?;
                }
                self.kitty.show(out, grid, frame, |frame| png(&raster(grid, frame, self.scale, black)))?;
            }
            Display::Sixel => self.sixel.draw(out, raster(grid, frame, self.scale, black), whole, black)?,
            Display::Iterm2 => {
                let raster = raster(grid, frame, self.scale, black);
                let encoded = png(&raster)?;
                out.write_all(iterm2_inline(&encoded, raster.width, raster.height).as_bytes())?;
            }
            Display::Text => {
                let text = zoomed_text(&ansi(grid, frame.options.reveal, self.mosaics, self.depth), frame.zoom)
                    .replace('\n', "\r\n");
                out.write_all(text.as_bytes())?;
            }
        }
        out.flush()
    }
}

/// The rows of a picture that changed since the last one drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Changed {
    /// Nothing changed.
    Nothing,
    /// Pixel rows `start` to `end` (exclusive) are to be sent, from the
    /// top of text row `text_row`.
    Rows { text_row: u16, start: usize, end: usize },
}

/// Which rows of `current` differ from `previous`, widened to whole text
/// rows of `text_row_pixels` at the top, where the terminal can place an
/// image, and to whole Sixel bands at the bottom; `None` when the whole
/// picture must be sent: its size or palette changed (each palette index is
/// its own colour register, and a terminal that shares registers between
/// images would recolour the rows already drawn, VT340 manual section
/// 14.3.4).
fn changed_rows(previous: &Raster, current: &Raster, text_row_pixels: usize) -> Option<Changed> {
    use neoteletext_image::sixel::BAND_ROWS;
    if (previous.width, previous.height, &previous.palette) != (current.width, current.height, &current.palette)
        || previous.width == 0
        || text_row_pixels == 0
    {
        return None;
    }
    let rows = || previous.indices.chunks(previous.width).zip(current.indices.chunks(current.width));
    let Some(first) = rows().position(|(before, after)| before != after) else { return Some(Changed::Nothing) };
    let last = rows().rposition(|(before, after)| before != after).unwrap_or(first);
    let text_row = first.checked_div(text_row_pixels)?;
    let start = text_row.saturating_mul(text_row_pixels);
    let bands = last.saturating_add(1).saturating_sub(start).div_ceil(BAND_ROWS);
    let end = start.saturating_add(bands.saturating_mul(BAND_ROWS)).min(current.height);
    Some(Changed::Rows { text_row: u16::try_from(text_row).ok()?, start, end })
}

/// The last Sixel picture drawn, so that a change sends only the rows
/// that changed (REQ-RND-029 item 4): a flash phase, a clock or one row of
/// a page sends a band, not the page.
struct SixelRows {
    last: Option<Raster>,
    text_row_pixels: Option<usize>,
}

impl SixelRows {
    /// Draws `picture`, from the cursor at the top left when it is sent
    /// whole; `fill` takes the transparent pixels.
    fn draw(&mut self, out: &mut impl Write, picture: Raster, whole: bool, fill: Rgba) -> std::io::Result<()> {
        let changed = match (&self.last, self.text_row_pixels) {
            (Some(last), Some(row_pixels)) if !whole => changed_rows(last, &picture, row_pixels),
            _ => None,
        };
        match changed {
            Some(Changed::Nothing) => {}
            Some(Changed::Rows { text_row, start, end }) => {
                let band = Raster {
                    width: picture.width,
                    height: end.saturating_sub(start),
                    indices: picture
                        .indices
                        .get(start.saturating_mul(picture.width)..end.saturating_mul(picture.width))
                        .unwrap_or_default()
                        .to_vec(),
                    palette: picture.palette.clone(),
                };
                queue!(out, cursor::MoveTo(0, text_row))?;
                out.write_all(&sixel(&band, fill)?)?;
            }
            None => out.write_all(&sixel(&picture, fill)?)?,
        }
        self.last = Some(picture);
        Ok(())
    }
}

/// The most frames of one page kept in the terminal as kitty images: the
/// flash phases of the palette mode (decision E.12) and of the resolved
/// mode (decision E.48) with reveal and zoom, with room to spare.
const KITTY_FRAMES_KEPT: usize = 8;

/// The kitty images of the page shown, one per frame (REQ-RND-029, item
/// 5 of TODO-2070): each frame is sent once, and a flash or reveal that
/// returns to a frame already sent only places it again, without sending
/// the page.
#[derive(Default)]
struct KittyImages {
    grid: Option<Grid>,
    sent: Vec<(Frame, u32)>,
    shown: Option<u32>,
    last_id: u32,
}

impl KittyImages {
    /// Shows `frame` of `grid`, sending it first if it has not been; `png`
    /// draws a frame.
    fn show(
        &mut self,
        out: &mut dyn Write,
        grid: &Grid,
        frame: Frame,
        png: impl Fn(Frame) -> std::io::Result<Vec<u8>>,
    ) -> std::io::Result<()> {
        if self.grid.as_ref() != Some(grid) {
            self.free_all(out)?;
            self.grid = Some(*grid);
        }
        let id = if let Some(&(_, id)) = self.sent.iter().find(|(sent, _)| *sent == frame) {
            id
        } else {
            if self.sent.len() >= KITTY_FRAMES_KEPT
                && let Some(position) = self.sent.iter().position(|&(_, id)| Some(id) != self.shown)
            {
                let (_, old) = self.sent.remove(position);
                out.write_all(kitty_free(old).as_bytes())?;
            }
            // Image ids start at 1: 0 is no id to the protocol.
            self.last_id = self.last_id.checked_add(1).unwrap_or(1);
            out.write_all(kitty_transmit(&png(frame)?, self.last_id, false).as_bytes())?;
            self.sent.push((frame, self.last_id));
            self.last_id
        };
        if self.shown != Some(id) {
            if let Some(old) = self.shown {
                out.write_all(kitty_hide(old).as_bytes())?;
            }
            out.write_all(kitty_place(id).as_bytes())?;
            self.shown = Some(id);
        }
        Ok(())
    }

    /// Frees every image sent.
    fn free_all(&mut self, out: &mut dyn Write) -> std::io::Result<()> {
        for (_, id) in self.sent.drain(..) {
            out.write_all(kitty_free(id).as_bytes())?;
        }
        self.shown = None;
        self.grid = None;
        Ok(())
    }
}

/// The terminal text of a page at a page zoom (REQ-L1-020), row by row as
/// `render_line` draws the picture: each row of the half shown on two
/// lines, since a text cell cannot be split, then the first row after the
/// half once, where the picture shows its upper half.
fn zoomed_text(text: &str, zoom: Zoom) -> String {
    let first = match zoom {
        Zoom::Off => return text.to_owned(),
        Zoom::Top => 0,
        Zoom::Bottom => ZOOM_ROWS,
    };
    let rows: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for row in rows.iter().skip(first).take(ZOOM_ROWS) {
        out.push_str(row);
        out.push_str(row);
    }
    if let Some(row) = rows.get(first.saturating_add(ZOOM_ROWS)) {
        out.push_str(row);
    }
    out
}

#[cfg(test)]
mod tests;
