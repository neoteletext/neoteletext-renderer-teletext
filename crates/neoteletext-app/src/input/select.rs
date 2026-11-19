//! A one-shot command's selection on a live channel (REQ-507, DEC-2445):
//! a live channel's pages come only on its push connection, for the page
//! the connection selects, so a command that reads one says HELLO when the
//! connection opens (DOC-1921) and, once welcomed, selects the page it
//! wants, or none, which the channel answers with its initial page
//! (DOC-1753). A channel that is not live never opens the connection for
//! input, and nothing here acts.

use crate::pipe::Notice;
use crate::session::Session;
use crate::source::Source;
use neoteletext_core::limits::SUBCODE_ANY;
use neoteletext_core::types::PageAddress;
use neoteletext_input::caps::SELECT_CAP;
use neoteletext_input::control::{FRAGMENT_DATA, from_packet};
use neoteletext_input::event::{Event, Item, Select};
use neoteletext_input::session::Session as InputSession;
use std::time::Instant;

/// The page a selection names when a command wants none in particular: no
/// page, which a connection's first selection takes as the channel's
/// initial page (DOC-1753).
const NO_PAGE: u16 = 0;

/// One command's input session on a live channel, which only ever selects.
pub(super) struct Selecting {
    input: InputSession,
    select: Option<Select>,
    sent: bool,
    start: Instant,
}

impl Selecting {
    /// The session for a command that wants `wanted`, any sub-page, or the
    /// channel's initial page.
    pub(super) fn new(wanted: Option<PageAddress>) -> Self {
        let page = wanted.map_or(NO_PAGE, PageAddress::mpp);
        Self {
            input: InputSession::new(SELECT_CAP),
            select: Select::new(page, SUBCODE_ANY),
            sent: false,
            start: Instant::now(),
        }
    }

    /// Milliseconds since the command began, the input session's clock.
    fn clock_ms(&self) -> u32 {
        u32::try_from(self.start.elapsed().as_millis()).unwrap_or(u32::MAX)
    }

    /// The push connection opened, and the channel takes input: HELLO
    /// goes first (REQ-205); or it closed, and a new one is a new session.
    pub(super) fn notice(&mut self, notice: &Notice, source: &mut dyn Source) {
        match notice {
            Notice::Input(true) => {
                self.sent = false;
                let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
                if let Ok(length) = self.input.open(self.clock_ms(), None, &mut out)
                    && let Some(message) = out.get(..length)
                {
                    source.send_input(message);
                }
            }
            Notice::PushVersion(version) => {
                // A version not spoken changes nothing: the channel's
                // WELCOME then names another and is refused.
                self.input.set_version(*version);
            }
            Notice::Input(false) => {
                self.sent = false;
                self.input.close();
            }
            _ => {}
        }
    }

    /// Takes the channel's records for the session and, once it is
    /// welcomed, sends the selection, once a connection.
    pub(super) fn turn(&mut self, session: &mut Session, source: &mut dyn Source) {
        for packet in session.take_kept() {
            let mut buffer = [0; FRAGMENT_DATA];
            if let Some(Ok(control)) = from_packet(&packet, &mut buffer) {
                // A refused WELCOME leaves the session refused: nothing goes.
                let _ = self.input.control(&control);
            }
        }
        let now = self.clock_ms();
        self.input.tick(now);
        if self.sent || !self.input.welcomed() {
            return;
        }
        let Some(select) = self.select else { return };
        self.sent = true;
        let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
        if let Ok(length @ 1..) = self.input.send(&Item::Event(Event::Select(select, "")), now, &mut out)
            && let Some(message) = out.get(..length)
        {
            source.send_input(message);
        }
    }
}
