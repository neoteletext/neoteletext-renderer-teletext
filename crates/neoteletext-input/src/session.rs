//! A receiver's side of one input session (DOC-1921 and DOC-1943):
//! HELLO and WELCOME, the input count the channel
//! acknowledges, the rate limit, and, with RESUME, the messages kept to
//! send again after a reconnection (DOC-1923, REQ-INT-125, REQ-RCH-013). Every
//! receiver uses it alike: NeoTeletext's trusted core, `neottx view`,
//! `neottx keyboard`, a microcontroller.
//!
//! [`Session`] keeps nothing to send again and never offers RESUME, so a
//! microcontroller pays nothing for it; [`ResumableSession`] keeps up to
//! [`RESEND_BYTES`] of unacknowledged messages.

use crate::TOKEN_BYTES;
use crate::caps::{AGENT, ALL_CAPS, RESUME_CAP, caps_of};
use crate::control::{Control, Resumption};
use crate::event::{Event, InputError, Item, Resume, write};
use crate::wire::WireError;
use crate::{RESEND_BYTES, VERSION, WELCOME_TIMEOUT_MS};
mod count;
mod filter;
mod limiter;

pub use count::REMEMBERED;
pub use limiter::Limiter;

/// Where the session is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// No connection.
    Closed,
    /// HELLO sent at this time; nothing else may go yet.
    Waiting(u32),
    /// The channel answered: input goes.
    Welcomed {
        /// The capabilities both sides use.
        caps: u32,
    },
    /// The channel did not answer in time, or answered with another
    /// version: it takes no input.
    Refused,
}

/// What a WELCOME began (REQ-RCH-013).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Welcomed {
    /// A new session: the input count from HELLO, nothing kept from before.
    New,
    /// The session the reconnection asked for: what the channel had not
    /// taken is to be sent again first, by [`SessionOf::resend`].
    Resumed,
}

/// Why the channel's WELCOME was not taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WelcomeError {
    /// Not waiting for one.
    Unasked,
    /// Another version.
    Version(u32),
    /// Capabilities the receiver did not offer, or a session without
    /// RESUME, or RESUME without one.
    Caps,
    /// A resumption that cannot be: no session was asked for, or the count
    /// taken is not one the receiver sent a message up to (REQ-RCH-013). A
    /// protocol error: the receiver closes the connection, and the session
    /// is forgotten, so the next is a new one.
    Resume,
}

/// Why an item was not sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SendError {
    /// Before WELCOME, or after the channel refused input.
    NotWelcomed,
    /// A resumed session's messages are to be sent again first
    /// ([`SessionOf::resend`]), so the stream keeps its order.
    ResendFirst,
    /// The item could not be encoded; or, with [`InputError::Bare`], it is
    /// a viewer's input outside a `RULE_EVENT` (RULE-46), which since
    /// protocol version 10 a receiver never writes at the top level of the
    /// stream.
    Input(InputError),
}

impl From<InputError> for SendError {
    fn from(error: InputError) -> Self {
        Self::Input(error)
    }
}

/// One session, keeping up to `KEPT` bytes of unacknowledged messages to
/// send again after a reconnection; with `KEPT` 0 RESUME is never offered.
#[derive(Clone, Copy, Debug)]
pub struct SessionOf<const KEPT: usize> {
    state: State,
    offered: u32,
    /// The protocol version of the connection (DEC-6953 item 2): what
    /// HELLO names and WELCOME must.
    version: u32,
    /// The input count sent so far, modulo 2¹⁶ as the cursor record carries
    /// it.
    count: u16,
    /// For each unacknowledged message of input units, oldest first from
    /// `sent_first`, a ring: the count after it, its bytes in `kept` (0
    /// when nothing is kept) and whether it is a `RULE_EVENT`. The oldest
    /// goes when it is full.
    sent: [Sent; REMEMBERED],
    sent_first: usize,
    sent_length: usize,
    /// The last acknowledgement taken.
    acked: u16,
    limiter: Limiter,
    /// The token of the channel's last WELCOME with RESUME, for the next
    /// reconnection (REQ-RCH-013).
    token: Option<[u8; TOKEN_BYTES]>,
    /// The unacknowledged messages' bytes, oldest first, `kept_length` of
    /// them.
    kept: [u8; KEPT],
    kept_length: usize,
    /// Every unacknowledged message is in `kept`: none was too large or
    /// pushed out of the ring, so a reconnection may resume.
    kept_whole: bool,
    /// The input count before the oldest message in the ring.
    kept_from: u16,
    /// The open connection's first message asked to resume.
    asked: bool,
    /// After a resumed WELCOME, the count after which every message is to
    /// be sent again before anything else.
    resend_from: Option<u16>,
    /// Set by [`SessionOf::discard_rule_events`]: every `RULE_EVENT` that
    /// ends at or before this count and that the channel had not taken
    /// when the session is resumed is dropped, never sent again (RULE-46,
    /// RULE-111 case 2).
    discard_through: Option<u16>,
}

/// One unacknowledged message of input units, as the ring keeps it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Sent {
    /// The input count after it.
    after: u16,
    /// Its bytes in `kept`: 0 when nothing is kept.
    length: u16,
    /// Whether it is a `RULE_EVENT`, which a connection's end discards
    /// (RULE-111 case 2).
    rule_event: bool,
}

/// An empty place of the ring.
const UNSENT: Sent = Sent { after: 0, length: 0, rule_event: false };

/// A session that keeps nothing to send again: a reconnection is always a
/// new session.
pub type Session = SessionOf<0>;
/// A session that offers RESUME and keeps [`RESEND_BYTES`] of
/// unacknowledged messages (REQ-RCH-013).
pub type ResumableSession = SessionOf<RESEND_BYTES>;

/// The input count after HELLO, which counts one in a new session
/// (DOC-1943).
const AFTER_HELLO: u16 = 1;

impl<const KEPT: usize> SessionOf<KEPT> {
    /// A closed session that will offer `caps`, and RESUME exactly when it
    /// keeps messages to send again.
    #[must_use]
    pub const fn new(caps: u32) -> Self {
        let offered = if KEPT == 0 { caps & ALL_CAPS & !RESUME_CAP } else { (caps | RESUME_CAP) & ALL_CAPS };
        Self {
            state: State::Closed,
            offered,
            version: VERSION,
            count: 0,
            sent: [UNSENT; REMEMBERED],
            sent_first: 0,
            sent_length: 0,
            acked: 0,
            limiter: Limiter::new(),
            token: None,
            kept: [0; KEPT],
            kept_length: 0,
            kept_whole: true,
            kept_from: 0,
            asked: false,
            resend_from: None,
            discard_through: None,
        }
    }

    /// A new session's start: the count from HELLO, nothing kept.
    fn begin(&mut self) {
        let version = self.version;
        *self = Self::new(self.offered);
        self.version = version;
        self.count = AFTER_HELLO;
        self.kept_from = AFTER_HELLO;
    }

    /// The protocol version of the connection about to open (DEC-6953
    /// items 1 and 2): the one the channel's WebSocket subprotocol named.
    /// The next HELLO names it and offers only the capabilities that
    /// version defines, and the WELCOME that answers must name it. A
    /// session not told speaks [`VERSION`]. Returns whether `version` is
    /// one this crate speaks ([`crate::speaks`]); another changes nothing.
    pub const fn set_version(&mut self, version: u32) -> bool {
        let spoken = crate::speaks(version);
        if spoken {
            self.version = version;
        }
        spoken
    }

    /// The protocol version the session's connection speaks
    /// ([`SessionOf::set_version`]).
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// The capabilities the session offers on its connection: those it was
    /// made with that the connection's version defines.
    const fn offering(&self) -> u32 {
        self.offered & caps_of(self.version)
    }

    /// Where it is.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Whether input may go.
    #[must_use]
    pub const fn welcomed(&self) -> bool {
        matches!(self.state, State::Welcomed { .. })
    }

    /// The capabilities both sides use, once welcomed.
    #[must_use]
    pub const fn caps(&self) -> u32 {
        match self.state {
            State::Welcomed { caps } => caps,
            _ => 0,
        }
    }

    /// Whether the next connection's first message asks to resume this
    /// session: the channel gave a token and every unacknowledged message
    /// is kept.
    #[must_use]
    pub const fn resumable(&self) -> bool {
        self.token.is_some() && self.kept_whole
    }

    /// The push connection opened at `now_ms`: writes its first message
    /// into `out` and returns its length. It is HELLO, with the AGENT
    /// capability when `agent` is given; in a new session HELLO counts
    /// one. When the last session may be resumed, RESUME follows it, with
    /// the token and the last acknowledgement (DOC-1923), and the count
    /// goes on from where it was.
    ///
    /// # Errors
    /// When `out` is too small or `agent` breaks HELLO's bounds.
    pub fn open(&mut self, now_ms: u32, agent: Option<&str>, out: &mut [u8]) -> Result<usize, InputError> {
        let resume = self.token.filter(|_| self.kept_whole).map(|token| Resume { token, acked: self.acked });
        if resume.is_none() {
            self.begin();
        }
        self.state = State::Waiting(now_ms);
        self.asked = resume.is_some();
        self.resend_from = None;
        let caps = if agent.is_some() { self.offering() | AGENT } else { self.offering() & !AGENT };
        let mut length = write(&Item::Event(Event::Hello { version: self.version, caps, agent }), out)?;
        if let Some(resume) = resume {
            let rest = out.get_mut(length..).ok_or(InputError::Wire(WireError::Full))?;
            length = length.saturating_add(write(&Item::Event(Event::Resume(resume)), rest)?);
        }
        Ok(length)
    }

    /// The connection closed: nothing goes. A session whose channel set
    /// RESUME keeps what a resumption needs, its token, its count and the
    /// messages not acknowledged; any other is forgotten.
    pub fn close(&mut self) {
        if self.resumable() {
            self.state = State::Closed;
            self.asked = false;
            self.resend_from = None;
        } else {
            self.forget();
        }
    }

    /// The push connection ended (RULE-111 case 2): every `RULE_EVENT` not
    /// acknowledged now is discarded and never sent again (RULE-46), and
    /// the session itself is kept, so that the next connection may still
    /// resume it. Call it with [`SessionOf::close`], before the next
    /// [`SessionOf::open`]; it changes nothing of a session that is not
    /// resumed.
    ///
    /// Which reports to drop cannot be settled here: the channel may have
    /// taken some of them before the break without acknowledging them, and
    /// those count in the channel's input count. So they are marked, and
    /// dropped when a resumed WELCOME says what the channel had taken:
    /// what it had taken stays (it is never sent again anyway), every
    /// marked `RULE_EVENT` after it is dropped, and the messages that are
    /// left after it, the session's own (RESIZE), are counted again from
    /// the count taken, as the channel will count them when they are sent
    /// again. The receiver's count and the channel's then agree, every
    /// acknowledgement the channel gives after the resumption names a
    /// count the receiver sent up to, and the session's token, its
    /// identity, is kept. A WELCOME that begins a new session forgets
    /// everything anyway, and a receiver selects its page afresh either
    /// way (RULE-111).
    pub fn discard_rule_events(&mut self) {
        self.discard_through = Some(self.count);
    }

    /// Forgets the session whole, as when the viewer leaves the channel:
    /// the next connection is a new session.
    pub fn forget(&mut self) {
        let version = self.version;
        *self = Self::new(self.offered);
        self.version = version;
    }

    /// Takes the channel's WELCOME: its version, the capabilities both
    /// sides use, and with RESUME the session (DOC-1923).
    ///
    /// # Errors
    /// When none was asked for, it names another version, capabilities
    /// not offered or a session against RESUME's bit (the session is then
    /// refused), or a resumption that cannot be ([`WelcomeError::Resume`]:
    /// the session is forgotten).
    pub fn welcome(&mut self, version: u32, caps: u32, resume: Option<Resumption>) -> Result<Welcomed, WelcomeError> {
        if !matches!(self.state, State::Waiting(_)) {
            return Err(WelcomeError::Unasked);
        }
        if version != self.version {
            self.state = State::Refused;
            return Err(WelcomeError::Version(version));
        }
        if caps & !self.offering() != 0 || resume.is_some() != (caps & RESUME_CAP != 0) {
            self.state = State::Refused;
            return Err(WelcomeError::Caps);
        }
        let welcomed = match resume.and_then(|resume| resume.taken) {
            Some(taken) if self.asked && self.ends_a_message(taken) => {
                self.drop_discarded(taken);
                self.resend_from = (taken != self.count).then_some(taken);
                Welcomed::Resumed
            }
            Some(_) => {
                self.forget();
                return Err(WelcomeError::Resume);
            }
            None => {
                if self.asked {
                    // The channel did not hold the session: a new one, from
                    // HELLO, with nothing of the old (REQ-INT-106).
                    self.begin();
                }
                Welcomed::New
            }
        };
        self.asked = false;
        self.token = resume.map(|resume| resume.token);
        self.state = State::Welcomed { caps };
        Ok(welcomed)
    }

    /// Takes a control message: WELCOME; a withdrawn mode record or an
    /// extension changes nothing.
    ///
    /// # Errors
    /// As [`SessionOf::welcome`].
    pub fn control(&mut self, control: &Control<'_>) -> Result<Option<Welcomed>, WelcomeError> {
        match *control {
            Control::Welcome { version, caps, resume } => self.welcome(version, caps, resume).map(Some),
            Control::Withdrawn(_) | Control::Extension { .. } => Ok(None),
        }
    }

    /// After a resumed WELCOME, gives each message the channel had not
    /// taken to `each`, in the order first sent, to be written before
    /// anything else (REQ-RCH-013); until then [`SessionOf::send`] refuses.
    /// They stay kept until acknowledged.
    pub fn resend(&mut self, each: &mut dyn FnMut(&[u8])) {
        let Some(from) = self.resend_from.take() else { return };
        let mut offset = 0_usize;
        for index in 0..self.sent_length {
            let Some(&Sent { after, length, .. }) = self.sent.get(self.ring(index)) else { break };
            let end = offset.saturating_add(usize::from(length));
            if count::serial_after(after, from)
                && let Some(message) = self.kept.get(offset..end)
            {
                each(message);
            }
            offset = end;
        }
    }

    /// At `now_ms`: refused when WELCOME is overdue (DOC-1921).
    pub fn tick(&mut self, now_ms: u32) {
        if let State::Waiting(since) = self.state
            && now_ms.wrapping_sub(since) >= WELCOME_TIMEOUT_MS
        {
            self.state = State::Refused;
        }
    }

    /// When WELCOME is due, while waiting for it.
    #[must_use]
    pub const fn welcome_due(&self) -> Option<u32> {
        match self.state {
            State::Waiting(since) => Some(since.wrapping_add(WELCOME_TIMEOUT_MS)),
            _ => None,
        }
    }

    /// Encodes `item` into `out` as DOC-1922 and DOC-1943 say, at `now_ms`, and
    /// counts it: a `RULE_EVENT` and the session's own events go, but an
    /// event whose capability is not agreed (SELECT) and an event over the
    /// rate limit, which is dropped. Returns how many bytes to write, 0
    /// for none.
    ///
    /// # Errors
    /// Before WELCOME, before a resumed session's messages are sent again,
    /// when the item cannot be encoded, or when it is a viewer's input
    /// outside a `RULE_EVENT` ([`InputError::Bare`], RULE-46).
    pub fn send(&mut self, item: &Item<'_>, now_ms: u32, out: &mut [u8]) -> Result<usize, SendError> {
        if !self.welcomed() {
            return Err(SendError::NotWelcomed);
        }
        if self.resend_from.is_some() {
            return Err(SendError::ResendFirst);
        }
        // RULE-46: a viewer's input travels only inside a RULE_EVENT.
        if crate::event::is_page_input(item) {
            return Err(SendError::Input(InputError::Bare));
        }
        let Some(item) = self.carried(item) else { return Ok(0) };
        let Some(item) = self.within_rate(item, now_ms) else { return Ok(0) };
        let length = write(&item, out)?;
        self.record(&item, out.get(..length).unwrap_or(&[]));
        Ok(length)
    }

    /// The input count sent so far.
    #[must_use]
    pub const fn count(&self) -> u16 {
        self.count
    }

    /// The last acknowledgement taken.
    #[must_use]
    pub const fn acked(&self) -> u16 {
        self.acked
    }

    /// The ring's slot of the `index`th unacknowledged message.
    fn ring(&self, index: usize) -> usize {
        self.sent_first.saturating_add(index).checked_rem(REMEMBERED).unwrap_or(0)
    }

    /// Whether `taken` is the count after a message the receiver sent, or
    /// before the oldest it still keeps: where a resumed session may go on.
    fn ends_a_message(&self, taken: u16) -> bool {
        taken == self.count
            || taken == self.kept_from
            || (0..self.sent_length)
                .any(|index| self.sent.get(self.ring(index)).is_some_and(|sent| sent.after == taken))
    }
}

#[cfg(test)]
mod tests;
