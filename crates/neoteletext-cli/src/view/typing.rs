//! Typing into a channel that takes input (area INT): a terminal key as the
//! input stream has it, and the one input session with the channel. Under
//! push protocol 9 a key goes to the channel only inside a `RULE_EVENT` of
//! a rule of the page's table that asked for it (page-rules.md RULE-46,
//! REQ-6523), so the keys are given to the table's evaluator
//! ([`PageRules`]) and what it reports is sent.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use neoteletext_app::source::Source;
use neoteletext_core::consts::COLUMNS;
use neoteletext_core::packet::Packet;
use neoteletext_input::event::Stamp;
use neoteletext_input::held::{Held, MAX_HELD_KEYS, Released, lost_key};
use neoteletext_web::records::{Read, RecordReader};
use std::time::Duration;

use crate::page_rules::PageRules;

/// What `neottx view` offers in HELLO (DOC-1922): it shows
/// pages and says which with SELECT; it draws no cursor and no echo.
const OFFERED: u32 = neoteletext_input::caps::SELECT_CAP;
/// What it offers besides where it shows pictures (renderer parity PAR-1,
/// PAR-7): it draws the ReGIS and NAPLPS pictures it is sent. A viewer of
/// characters alone cannot show a picture, so it does not offer them and is
/// sent the server's mosaic page instead (PAR-25).
const PICTURES_OFFERED: u32 = neoteletext_input::caps::REGIS_CAP | neoteletext_input::caps::NAPLPS_CAP;

/// What the viewer offers in HELLO: [`OFFERED`], and [`PICTURES_OFFERED`]
/// when it shows pictures.
pub(super) const fn offered(pictures: bool) -> u32 {
    if pictures { OFFERED | PICTURES_OFFERED } else { OFFERED }
}

/// Whether the viewer keeps `packet` of the channel's stream for
/// [`Typing::records`]: every fragment of the carriage its records come
/// on, since a page's rule table is a record of many fragments (RULE-41)
/// and only its first says what it is.
pub(super) fn is_channel_packet(packet: &Packet) -> bool {
    neoteletext_web::records::is_record_packet(packet)
}

/// A terminal key as the input stream has it (DOC-1930 and DOC-1935):
/// every key as KEY, stamped with the page on screen, a
/// character key's text with it unless Control or Alt is held or it is a
/// release. `None` for keys a terminal cannot name.
pub(super) fn input_of(key: KeyEvent, on: Stamp, buffer: &mut [u8; 4]) -> Option<neoteletext_input::event::Item<'_>> {
    use neoteletext_input::event::{ALT, Action, CONTROL, Event, Item, Key, KeyCode as InputKey, SHIFT, printable};
    // A terminal with the Kitty keyboard protocol reports repeats and
    // releases; any other reports every key once, as a press.
    let action = match key.kind {
        KeyEventKind::Press => Action::Press,
        KeyEventKind::Repeat => Action::Repeat,
        KeyEventKind::Release => Action::Release,
    };
    let bit = |set: bool, value: u8| if set { value } else { 0 };
    let mut control = key.modifiers.contains(KeyModifiers::CONTROL);
    let mut alt = key.modifiers.contains(KeyModifiers::ALT);
    // AltGr is a level shift, not a modifier (DOC-1937,
    // REQ-INT-116): a terminal on Windows reports it as Control and Alt
    // held, and a character typed so is text.
    if control && alt && matches!(key.code, KeyCode::Char(character) if printable(character)) {
        (control, alt) = (false, false);
    }
    let mut mods = bit(key.modifiers.contains(KeyModifiers::SHIFT), SHIFT) | bit(control, CONTROL) | bit(alt, ALT);
    let functional = |name: &str| neoteletext_input::keys::code(name).map(InputKey::Functional);
    let (code, text) = match key.code {
        KeyCode::Char(character) if !printable(character) => return None,
        KeyCode::Char(character) => {
            let text: &str = character.encode_utf8(buffer);
            // A terminal gives the character typed; Shift is in `mods`.
            let unshifted = character.to_lowercase().next().unwrap_or(character);
            (InputKey::Character(unshifted), (!control && !alt && action != Action::Release).then_some(text))
        }
        KeyCode::BackTab => {
            mods |= SHIFT;
            (functional("Tab")?, None)
        }
        KeyCode::F(number) => (functional(&format!("F{number}"))?, None),
        KeyCode::Enter => (functional("Enter")?, None),
        KeyCode::Backspace => (functional("Backspace")?, None),
        KeyCode::Tab => (functional("Tab")?, None),
        KeyCode::Esc => (functional("Escape")?, None),
        KeyCode::Left => (functional("ArrowLeft")?, None),
        KeyCode::Right => (functional("ArrowRight")?, None),
        KeyCode::Up => (functional("ArrowUp")?, None),
        KeyCode::Down => (functional("ArrowDown")?, None),
        KeyCode::Home => (functional("Home")?, None),
        KeyCode::End => (functional("End")?, None),
        KeyCode::PageUp => (functional("PageUp")?, None),
        KeyCode::PageDown => (functional("PageDown")?, None),
        KeyCode::Delete => (functional("Delete")?, None),
        KeyCode::Insert => (functional("Insert")?, None),
        _ => return None,
    };
    Some(Item::Event(Event::Key(Key { on, action, key: code, mods, text, base: None, lost: false })))
}

/// Typing into a channel that takes input (area INT): the viewer's keys
/// go to it as a terminal's go to its host, in one input session.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact: the socket, the viewer's choice, the terminal's focus and the session's start"
)]
pub(super) struct Typing {
    /// The channel's WebSocket is open and its descriptor declares input.
    open: bool,
    /// The viewer left the keys with Control-].
    left: bool,
    /// The terminal lost the focus, where it reports that.
    unfocused: bool,
    /// The focus the channel was last told.
    focus_told: Option<bool>,
    /// The input session (DOC-1921).
    input: neoteletext_input::session::Session,
    /// The session was welcomed and told the size and focus.
    started: bool,
    /// The selection the channel was last told: page and sub-code.
    told: Option<(u16, u16)>,
    /// The selection the address opened named, with its page parameters,
    /// until another selection leaves them behind (REQ-1693 item 6).
    parameters: Option<((u16, u16), String)>,
    /// The page and sub-code on screen, which stamp the viewer's input.
    drawn: Option<Stamp>,
    /// What the viewer holds, so a key the page loses is released.
    held: Held,
    /// The terminal reports releases: without them nothing can be held.
    releases: bool,
    /// The page and sub-code the channel's REDIRECT record named, to open
    /// (DOC-1960, REQ-INT-113), and the query it kept.
    redirect: Option<(u16, u16, Option<String>)>,
    /// The channel's records, for its pages' rule tables (RULE-41).
    records: RecordReader,
    /// The rule tables, and the evaluator of the page on screen.
    rules: PageRules,
    /// The ReGIS and NAPLPS pictures the channel sent, by sub-page (PAR-7).
    pictures: crate::picture::PictureStore,
    /// The time of the last [`Typing::tick`], in microseconds since the
    /// viewer started: when a record taken since is said to have come.
    now_us: u64,
}

impl Typing {
    /// A session not yet open; `pictures` when the viewer shows pictures,
    /// so that it offers the picture languages (PAR-7).
    pub(super) fn new(pictures: bool) -> Self {
        Self {
            open: false,
            left: false,
            unfocused: false,
            focus_told: None,
            input: neoteletext_input::session::Session::new(offered(pictures)),
            started: false,
            told: None,
            parameters: None,
            drawn: None,
            held: Held::new(),
            releases: false,
            redirect: None,
            records: RecordReader::new(),
            rules: PageRules::new(),
            pictures: crate::picture::PictureStore::new(),
            now_us: 0,
        }
    }

    /// The pictures the channel, or the file, sent (PAR-7).
    pub(super) const fn pictures(&self) -> &crate::picture::PictureStore {
        &self.pictures
    }

    /// Tells the channel the focus when it changed: away when the viewer
    /// left with Control-] or the terminal lost the focus, so the channel
    /// lets go of what is held (REQ-INT-108).
    pub(super) fn tell_focus(&mut self, source: &mut dyn Source, clock_ms: u32) {
        use neoteletext_input::event::{Event, Item};
        let focus = !(self.left || self.unfocused);
        if self.started && self.focus_told != Some(focus) {
            self.focus_told = Some(focus);
            let event = if focus { Event::FocusIn } else { Event::FocusOut };
            self.send(source, &Item::Event(event), clock_ms);
        }
    }

    pub(super) fn on(&self) -> bool {
        self.open && !self.left && self.input.welcomed()
    }

    /// Whether Control-] may leave the keys, or take them back: the
    /// channel's connection is open and its session welcomed.
    pub(super) fn may_leave(&self) -> bool {
        self.open && self.input.welcomed()
    }

    /// Leaves the keys with Control-], or takes them back, and tells the
    /// channel the focus. The selection stays as it was: focus never
    /// changes it (REQ-1554). Returns whether the keys are now left.
    pub(super) fn leave_or_return(&mut self, source: &mut dyn Source, clock_ms: u32) -> bool {
        self.left = !self.left;
        self.tell_focus(source, clock_ms);
        self.left
    }

    /// The terminal gained or lost the focus.
    pub(super) fn focus_changed(&mut self, unfocused: bool, source: &mut dyn Source, clock_ms: u32) {
        self.unfocused = unfocused;
        self.tell_focus(source, clock_ms);
    }

    /// The channel's REDIRECT record named a page of the channel, to open.
    /// It needs no keys to go to the channel: a selected page is active
    /// whatever its focus (DOC-1960).
    pub(super) fn redirected(&mut self, page: u16, sub: u16, query: Option<String>) {
        self.redirect = Some((page, sub, query));
    }

    /// The page and sub-code a REDIRECT record named, to open, once. Its
    /// query goes with that selection, as an opened address's does.
    pub(super) fn take_redirect(&mut self) -> Option<(u16, u16)> {
        let (page, sub, query) = self.redirect.take()?;
        self.opened_with((page, sub), query.unwrap_or_default());
        Some((page, sub))
    }

    /// The address opened named `selection` and these page parameters.
    pub(super) fn opened_with(&mut self, selection: (u16, u16), parameters: String) {
        self.parameters = (!parameters.is_empty()).then_some((selection, parameters));
    }

    /// The channel is told the page and sub-page selected (DOC-1753),
    /// when they changed, whatever the focus (REQ-1090): 3F7F for a
    /// page alone. What the channel answers 3F7F with stays the selection,
    /// so no second SELECT goes for it. The address's page parameters go
    /// with the selection it named, and any other leaves them behind
    /// (REQ-1693 item 6); a new connection to the same selection sends them
    /// again, as they stay in the address.
    pub(super) fn tell_selection(&mut self, source: &mut dyn Source, page: u16, sub: u16, clock_ms: u32) {
        use neoteletext_input::event::{Event, Item, Select};
        if self.started && self.told != Some((page, sub)) {
            self.told = Some((page, sub));
            self.parameters.take_if(|(selection, _)| *selection != (page, sub));
            let parameters = self.parameters.as_ref().map_or_else(String::new, |(_, parameters)| parameters.clone());
            if let Some(select) = Select::new(page, sub) {
                self.send(source, &Item::Event(Event::Select(select, &parameters)), clock_ms);
            }
        }
    }

    /// What is on screen: `None` while nothing of the selected page is
    /// drawn, when no input goes (REQ-1554).
    pub(super) const fn set_drawn(&mut self, on: Option<Stamp>) {
        self.drawn = on;
    }

    /// The stamp for input now, while keys go and the page is drawn.
    pub(super) fn stamp(&self) -> Option<Stamp> {
        self.drawn.filter(|_| self.on())
    }

    /// The input protocol version the push connection speaks (DEC-6953
    /// item 2), told before the connection is said to be open.
    pub(super) fn speak(&mut self, version: u32) {
        // A version not spoken changes nothing: the channel's WELCOME
        // then names another and is refused.
        self.input.set_version(version);
    }

    /// The push connection opened or closed: a new session (REQ-INT-106).
    pub(super) fn connection(&mut self, source: &mut dyn Source, open: bool, clock_ms: u32) {
        self.open = open;
        self.started = false;
        self.told = None;
        self.drawn = None;
        self.held = Held::new();
        self.focus_told = None;
        // A new connection's server sends the tables again (RULE-40).
        self.records.clear();
        self.rules = PageRules::new();
        self.pictures.clear();
        if open {
            let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
            if let Ok(length) = self.input.open(clock_ms, None, &mut out)
                && let Some(message) = out.get(..length)
            {
                source.send_input(message);
            }
        } else {
            self.input.close();
        }
    }

    /// Takes the channel's records; once welcomed, says the size and the
    /// focus (DOC-1921). Returns whether the channel refused input.
    pub(super) fn records(
        &mut self,
        source: &mut dyn Source,
        packets: impl Iterator<Item = Packet>,
        clock_ms: u32,
    ) -> bool {
        use neoteletext_input::event::{Event, Item};
        let waiting = matches!(self.input.state(), neoteletext_input::session::State::Waiting(_));
        for packet in packets {
            let mut buffer = [0; neoteletext_input::control::FRAGMENT_DATA];
            if let Some(Ok(control)) = neoteletext_input::control::from_packet(&packet, &mut buffer) {
                // A refused WELCOME leaves the session refused, said below.
                let _ = self.input.control(&control);
            } else if let Some(Ok(cursor)) = neoteletext_input::cursor::from_packet(&packet) {
                // A cursor record never moves a viewer, whatever page it
                // names (DOC-1954).
                self.input.acknowledged(cursor.acked);
            } else if let Read::Record(record) = self.records.push(&packet) {
                self.pictures.record(&record);
                self.rules.record(&record, self.now_us);
            }
        }
        self.input.tick(clock_ms);
        if self.input.welcomed() && !self.started {
            self.started = true;
            let columns = u32::try_from(COLUMNS).unwrap_or(0);
            let rows = neoteletext_input::MAX_ROWS;
            self.send(source, &Item::Event(Event::Resize { columns, rows, width: 0, height: 0 }), clock_ms);
            self.tell_focus(source, clock_ms);
        }
        waiting && self.input.state() == neoteletext_input::session::State::Refused
    }

    /// Writes `item` at once, one message an item (REQ-INT-112). The event
    /// that ends the input goes after a lost release of each key held
    /// (DOC-1944), on a terminal that reports releases.
    pub(super) fn send(&mut self, source: &mut dyn Source, item: &neoteletext_input::event::Item<'_>, clock_ms: u32) {
        use neoteletext_input::event::{Action, Event, Item};
        if matches!(item, Item::Event(Event::Key(key)) if key.action == Action::Release) {
            self.releases = true;
        }
        let mut lost = [None; MAX_HELD_KEYS];
        let mut count = 0;
        self.held.take(item, &mut |released, on| {
            if let (Released::Key(key), Some(slot)) = (released, lost.get_mut(count)) {
                *slot = Some(lost_key(key, on));
                count = count.saturating_add(1);
            }
        });
        if self.releases {
            for key in lost.into_iter().flatten() {
                self.write(source, &Item::Event(Event::Key(key)), clock_ms);
            }
        }
        self.write(source, item, clock_ms);
    }

    /// Evaluates the ticks of the page's table due at `now_us`,
    /// microseconds since the viewer started, and sends what its rules
    /// report (RULE-4, RULE-46). The table evaluated is that of the page
    /// on screen, once the channel welcomed the session; a page opened
    /// afresh is told the focus, since it has gained or lacks the keyboard.
    pub(super) fn tick(&mut self, source: &mut dyn Source, now_us: u64, clock_ms: u32) {
        self.now_us = now_us;
        let shown = self.drawn.filter(|_| self.started);
        if self.rules.show(shown, now_us) {
            self.focus_told = None;
            self.tell_focus(source, clock_ms);
        }
        let mut reports = Vec::new();
        self.rules.tick(now_us, &mut reports);
        for report in &reports {
            report.with_item(|item| self.write(source, item, clock_ms));
        }
    }

    /// How long the viewer may wait at `now_us` before the page's table
    /// has a tick to evaluate: `None` when it has none until a key or a
    /// record comes.
    pub(super) fn wait(&self, now_us: u64) -> Option<Duration> {
        self.rules.wait(now_us).map(Duration::from_micros)
    }

    /// Writes `item` to the channel, or, when it is the viewer's input to
    /// the page, gives it to the page's table: only a rule's report of it
    /// reaches the channel (RULE-46), and with no table nothing does
    /// (RULE-50).
    fn write(&mut self, source: &mut dyn Source, item: &neoteletext_input::event::Item<'_>, clock_ms: u32) {
        if neoteletext_input::event::is_page_input(item) {
            crate::nfc::with_nfc(item, |item| {
                if let Some(carried) = self.input.carried(item) {
                    self.rules.input(&carried);
                }
            });
            return;
        }
        let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
        let sent = crate::nfc::with_nfc(item, |item| self.input.send(item, clock_ms, &mut out));
        if let Ok(length @ 1..) = sent
            && let Some(message) = out.get(..length)
        {
            source.send_input(message);
        }
    }
}
