//! The interactive viewer, `neottx view` (REQ-CLI-004, decision 19 of
//! IMPLEMENTATION-LOG.md): the core viewer machine driven by terminal keys,
//! with the TTI Link Extension (Appendix E, REQ-NAV-011 and 012) and the
//! security confirmations of area SEC.

use crate::ansi::{ColourDepth, MosaicStyle, ansi};
use crate::frame::Frame;
use crate::image::{Scale, iterm2_inline, kitty_free, kitty_hide, kitty_place, kitty_transmit, png, raster, sixel};
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
    last_drawn: Option<(Grid, Option<Frame>, bool, Zoom)>,
    action: Action,
    prompt: Option<[u8; COLUMNS]>,
    confirmed_services: HashSet<u8>,
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
        let mut viewer = Viewer::new(initial);
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
            action,
            prompt: None,
            confirmed_services: HashSet::new(),
            typing,
            traced: None,
            walk_trace: Vec::new(),
            trace_withheld: false,
            unrecorded: None,
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

    /// Writes the trace kept this turn, the decoder's and the walk's, as
    /// the other commands print it with `-vv` (REQ-FSM-010): to `out`,
    /// standard error, when it is not the terminal the viewer draws on,
    /// where its lines would spoil the page; otherwise it is dropped, and
    /// the viewer says so when it ends. Nothing is kept from turn to turn.
    fn write_trace(&mut self, session: &mut Session, out: &mut dyn Write, out_is_terminal: bool) {
        session.trace.append(&mut self.walk_trace);
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
    }
}

/// Runs the viewer until the user quits.
pub fn run(
    session: &mut Session,
    source: &mut dyn Source,
    opening: Opening,
    display: Display,
    scale: Scale,
    depth: ColourDepth,
    mosaics: MosaicStyle,
) -> std::io::Result<()> {
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
    let mut drawer = Drawer { display, scale, depth, mosaics, kitty: KittyImages::default() };
    state.run(session, source, &wakes, display, &mut |grid, frame| drawer.draw(&mut out, grid, frame))?;
    drawer.kitty.free_all(&mut out)?;
    drop(guard);
    for message in state.into_messages() {
        eprintln!("neottx: {message}");
    }
    Ok(())
}

/// How the viewer draws a page, and the kitty images it holds.
struct Drawer {
    display: Display,
    scale: Scale,
    depth: ColourDepth,
    mosaics: MosaicStyle,
    kitty: KittyImages,
}

impl Drawer {
    fn draw(&mut self, out: &mut std::io::Stdout, grid: &Grid, frame: Frame) -> std::io::Result<()> {
        queue!(out, cursor::MoveTo(0, 0))?;
        let black = Rgba::opaque(0, 0, 0);
        match self.display {
            Display::Kitty => {
                self.kitty.show(out, grid, frame, |frame| png(&raster(grid, frame, self.scale, black)))?;
            }
            Display::Sixel => out.write_all(&sixel(&raster(grid, frame, self.scale, black), black)?)?,
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
