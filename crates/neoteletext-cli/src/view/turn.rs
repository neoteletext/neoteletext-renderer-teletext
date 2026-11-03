//! One turn of the viewer's loop, in the order a turn takes its steps:
//! the source's notices, the action the viewer asked for, the channel's
//! typing, the page shown, the drawing, then the terminal's
//! event and the keys.

use super::terminal_io::{TICK, Wakened, next_event};
use super::{Shown, ViewState};
use crate::clock::now_ms;
use crate::frame::Frame;
use crate::terminal::Display;
use crossterm::event::Event;
use neoteletext_app::interact::{leave_prompt, message_row};
use neoteletext_app::session::Session;
use neoteletext_app::source::Source;
use neoteletext_core::config::Config;
use neoteletext_core::level1::Grid;
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_core::viewer::Action;
use neoteletext_input::event::Stamp;
use std::io::IsTerminal;
use std::sync::mpsc;

/// What a drawing of the page depends on: the grid, the frame where the
/// display is a picture (every picture protocol draws the flash phase
/// itself, REQ-RND-029; text flashes by the terminal's blink), reveal and
/// zoom. The page is drawn again when it changes.
pub(super) fn drawn_state(grid: &Grid, display: Display, frame: Frame, reveal: bool) -> DrawnState {
    (*grid, (display != Display::Text).then_some(frame), reveal, frame.zoom)
}

/// See [`drawn_state`].
pub(super) type DrawnState = (Grid, Option<Frame>, bool, neoteletext_core::level1::Zoom);

impl ViewState {
    /// Runs turns until the user quits. `paint` draws a grid at a flash
    /// phase, the whole page when its third argument is true and otherwise
    /// what changed where the protocol allows; a failure to draw ends the
    /// loop with the error.
    pub(super) fn run(
        &mut self,
        session: &mut Session,
        source: &mut dyn Source,
        wakes: &mpsc::Receiver<Wakened>,
        display: Display,
        paint: &mut dyn FnMut(&Grid, Frame, bool) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        loop {
            if let Some(report) = self.unrecorded.take() {
                session.decoder_mut().record_enhancement(report.build, &report.faults);
            }
            if let Some((build, fell_back)) = self.unrecorded_designation.take() {
                session.decoder_mut().record_designation(build, fell_back);
            }
            let now = Instant(self.start.elapsed().as_micros().try_into().unwrap_or(u64::MAX));
            if self.live {
                self.live = source.poll(session, now);
            }
            let stderr = std::io::stderr();
            let to_terminal = stderr.is_terminal();
            self.write_trace(session, &mut stderr.lock(), to_terminal);
            for notice in source.notices() {
                self.take_notice(notice, source);
            }
            self.follow_service_initial(session);
            let config: Config = *session.decoder().config();
            self.act(session, source);
            let (requested, subcode) = self.viewer.requested();
            if self.typing_turn(session, source, requested, subcode) {
                continue;
            }
            let session: &Session = session;
            let page = self.page_shown(session, requested, subcode);
            self.mark_security(page.and_then(|page| source.came_unencrypted(page.key.address)));
            self.typing.set_drawn(page.and_then(|page| Stamp::new(page.key.address.mpp(), page.key.subcode.value())));
            let page_extras = page.and_then(|page| source.extras(page.key.address, page.key.subcode)).cloned();
            let shown = Shown { page, extras: page_extras.as_ref(), config: &config, now };
            if let Some(forward) = self.pending_step.take() {
                self.step_subpage(forward, source, session, &shown);
            }
            self.compose(session, requested, &shown);
            let reveal = self.viewer.reveal(now);
            let frame = Frame { zoom: self.viewer.zoom(), ..Frame::at(&config, reveal, now) };
            let state = drawn_state(&self.grid, display, frame, reveal);
            if self.last_drawn.as_ref() != Some(&state) {
                paint(&self.grid, frame, core::mem::take(&mut self.repaint_whole))?;
                self.last_drawn = Some(state);
            }
            let woke = next_event(wakes, TICK);
            // A resized terminal may have cleared or moved what was drawn.
            if matches!(woke, Some(Event::Resize(..))) {
                self.last_drawn = None;
                self.repaint_whole = true;
            }
            let clock_ms = now_ms(self.start);
            self.paste_or_focus(woke.as_ref(), source, clock_ms);
            if let Some(Event::Key(key)) = woke
                && self.key(key, source, session, &shown, clock_ms).is_break()
            {
                return Ok(());
            }
        }
    }

    /// Carries out the action the viewer asked for last, and clears it.
    pub(super) fn act(&mut self, session: &mut Session, source: &mut dyn Source) {
        match self.action {
            Action::Request(address, subcode) => {
                session.decoder_mut().set_requested(Some(address));
                source.request(address, subcode);
                self.prompt = None;
            }
            Action::FollowService(target) => {
                self.prompt = None;
                if let Some(service) = target.service {
                    if let Some(linked) = source.linked_service(service).filter(|linked| !linked.origin.is_empty()) {
                        self.confirmed_origins.insert(linked.origin);
                    }
                    // REQ-295: the viewer goes on in the service followed,
                    // its history keeping the pages before the link.
                    match source.follow(service, target.address, target.subcode) {
                        Some(number) => self.viewer.followed(number, target.address, target.subcode),
                        None => self.prompt = Some(message_row("This service cannot be opened")),
                    }
                }
            }
            // REQ-295: Back or Forward reached a page of another service of
            // the session. Every one was opened by the user or by a link
            // whose origin was confirmed, and an origin confirmed stays so
            // for the session (REQ-288, DEC-2454), so it is opened again
            // without asking.
            Action::ReturnTo(entry) => {
                source.reopen(entry.service, entry.address, entry.subcode);
                self.prompt = None;
            }
            Action::ConfirmService(target) => {
                let linked = target.service.and_then(|service| source.linked_service(service)).unwrap_or_default();
                let name = if linked.display_name.is_empty() { &linked.short_name } else { &linked.display_name };
                self.prompt = Some(leave_prompt(name, &linked.origin));
            }
            Action::None => {}
        }
        self.action = Action::None;
    }

    /// The channel's side of the turn: its records taken, its welcome
    /// shown, a page it sends the viewer to, and the selection told to it.
    /// True when the channel sent the viewer to another page, which the
    /// next turn requests.
    pub(super) fn typing_turn(
        &mut self,
        session: &mut Session,
        source: &mut dyn Source,
        requested: PageAddress,
        subcode: SubCode,
    ) -> bool {
        let clock_ms = now_ms(self.start);
        let was_on = self.typing.on();
        if self.typing.records(source, session.take_kept().into_iter(), clock_ms) {
            self.prompt = Some(message_row("This channel did not welcome input"));
        }
        if self.typing.on() && !was_on {
            self.prompt = Some(message_row("Keys go to this channel. Ctrl-] leaves"));
        }
        if let Some((mpp, sub)) = self.typing.take_redirect()
            && let Some(address) = PageAddress::from_mpp(mpp)
            && (address, SubCode::new(sub)) != (requested, subcode)
        {
            // A page the channel's program sends the viewer to is a visit
            // of the history, as a link is (REQ-295).
            self.action = self.viewer.go_to(address, SubCode::new(sub));
            return true;
        }
        self.typing.tell_selection(source, requested.mpp(), subcode.value(), clock_ms);
        false
    }
}
