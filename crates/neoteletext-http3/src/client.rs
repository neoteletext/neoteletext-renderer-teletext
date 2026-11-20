//! The client's side of an HTTP/3 connection (REQ-6986): the request that
//! opens a WebSocket by extended CONNECT (RFC 9220), the streams a server
//! opens, read from their first byte, and the stream of the WebSocket once
//! it is opened. As the rest of the crate it carries no bytes itself: the
//! host gives it what each QUIC stream received and sends what it writes.

use alloc::vec::Vec;

use crate::error::{Code, Error};
use crate::frame;
use crate::limits::{MAX_FIELD_SECTION_BYTES, MAX_INTERIM_RESPONSES};
use crate::message::{self, AUTHORITY, CONNECT, METHOD, PATH, PROTOCOL, SCHEME};
use crate::qpack;
use crate::settings::Settings;
use crate::stream::{Control, ControlEvent, Message, MessageEvent, Opened, Role, Unidirectional};
use crate::varint;

/// What an extended CONNECT asks to speak for a WebSocket (RFC 8441
/// section 4, RFC 9220 section 3).
pub const WEBSOCKET: &[u8] = b"websocket";
/// The scheme of every request here: HTTP/3 runs over QUIC, which is
/// always TLS.
const HTTPS: &[u8] = b"https";
/// `sec-websocket-version` and the one version there is (RFC 6455 section
/// 4.1), which RFC 8441 section 5 keeps.
const WEBSOCKET_VERSION: &[u8] = b"sec-websocket-version";
const WEBSOCKET_VERSION_13: &[u8] = b"13";
/// `sec-websocket-protocol` (RFC 6455 section 11.3.4).
pub const WEBSOCKET_PROTOCOL: &[u8] = b"sec-websocket-protocol";
/// `user-agent` (RFC 9110 section 10.1.5).
const USER_AGENT: &[u8] = b"user-agent";
/// The first and last interim status codes (RFC 9110 section 15.2).
const INTERIM_FIRST: u16 = 100;
const INTERIM_LAST: u16 = 199;
/// The first byte of every path (RFC 9114 section 4.3.1).
const PATH_START: u8 = b'/';

/// What an extended CONNECT for a WebSocket names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WebSocketRequest<'request> {
    /// `:authority`: the host, with its port when it is not 443.
    pub authority: &'request [u8],
    /// `:path`: the path and query, from its `/`.
    pub path: &'request [u8],
    /// The one subprotocol offered.
    pub subprotocol: &'request [u8],
    /// What the client names itself as.
    pub user_agent: &'request [u8],
}

/// A request that cannot be written: one of its values is empty, has a
/// byte no field value may carry (RFC 9114 section 4.2), or is a path that
/// does not start with `/`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unwritable;

/// Appends the HEADERS frame of the extended CONNECT that opens a
/// WebSocket (RFC 9220 section 3, RFC 8441 sections 4 and 5): the first
/// bytes of a request stream. No `origin` is sent, and no
/// `sec-websocket-key`, which RFC 8441 section 5 drops.
///
/// # Errors
/// [`Unwritable`], with nothing appended.
pub fn open_websocket(request: &WebSocketRequest<'_>, out: &mut Vec<u8>) -> Result<(), Unwritable> {
    let values = [request.authority, request.path, request.subprotocol, request.user_agent];
    if values.iter().any(|value| value.is_empty() || !message::writable(value))
        || request.path.first() != Some(&PATH_START)
    {
        return Err(Unwritable);
    }
    let fields: [(&[u8], &[u8]); 8] = [
        (METHOD, CONNECT),
        (PROTOCOL, WEBSOCKET),
        (SCHEME, HTTPS),
        (AUTHORITY, request.authority),
        (PATH, request.path),
        (WEBSOCKET_VERSION, WEBSOCKET_VERSION_13),
        (WEBSOCKET_PROTOCOL, request.subprotocol),
        (USER_AGENT, request.user_agent),
    ];
    let mut section = Vec::new();
    qpack::encode(fields, &mut section);
    frame::write(frame::HEADERS, &section, out);
    Ok(())
}

/// Appends `payload` as one DATA frame: a WebSocket's bytes on its stream
/// (RFC 8441 section 5). Nothing is appended for an empty payload, which
/// would carry nothing.
pub fn data(payload: &[u8], out: &mut Vec<u8>) {
    if !payload.is_empty() {
        frame::write(frame::DATA, payload, out);
    }
}

/// What a unidirectional stream the server opened said.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IncomingEvent {
    /// The server's settings, its control stream's first frame. Extended
    /// CONNECT may be sent only when [`Settings::connect_protocol`] is set
    /// (REQ-6986 item 5).
    Settings(Settings),
    /// GOAWAY (RFC 9114 section 5.2): the server is closing the
    /// connection, and no new request may be made on it.
    GoAway(u64),
    /// The stream's type is unknown: stop reading it with
    /// `H3_STREAM_CREATION_ERROR` (RFC 9114 section 6.2.3). The connection
    /// goes on, and whatever else the stream carries is passed over.
    Refuse,
}

#[derive(Clone, Debug)]
enum Kind {
    /// Reading the stream's type.
    Type(varint::Reader),
    Control(Control),
    /// A QPACK stream, which with no dynamic table carries nothing kept.
    Drain,
    Refused,
}

/// A unidirectional stream the server opened, read from its first byte
/// (REQ-6986 item 4).
#[derive(Clone, Debug)]
pub struct Incoming {
    kind: Kind,
}

impl Default for Incoming {
    fn default() -> Self {
        Self { kind: Kind::Type(varint::Reader::default()) }
    }
}

impl Incoming {
    /// A stream at its first byte.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the connection needs this stream: the control stream or a
    /// QPACK stream, whose end is the connection error
    /// `H3_CLOSED_CRITICAL_STREAM` (RFC 9114 section 6.2.1, RFC 9204
    /// section 4.2).
    #[must_use]
    pub const fn critical(&self) -> bool {
        matches!(self.kind, Kind::Control(_) | Kind::Drain)
    }

    /// Reads from the start of `input`: how many bytes it took and what
    /// they said, if anything. Call it again with the rest while it takes
    /// bytes or gives events. `opened` is the connection's record of the
    /// streams the server has opened, one for every stream of it.
    ///
    /// # Errors
    /// A connection error: what [`Opened::open`] finds in the stream's
    /// type, and what [`Control::read`] finds on the control stream.
    pub fn read(&mut self, opened: &mut Opened, input: &[u8]) -> Result<(usize, Option<IncomingEvent>), Error> {
        let mut taken = 0_usize;
        loop {
            let rest = input.get(taken..).unwrap_or(&[]);
            match &mut self.kind {
                Kind::Type(reader) => {
                    let (used, kind) = reader.read(rest);
                    taken = taken.saturating_add(used);
                    let Some(kind) = kind else { return Ok((taken, None)) };
                    match opened.open(kind)? {
                        Unidirectional::Control => self.kind = Kind::Control(Control::new(Role::Client)),
                        Unidirectional::Drain => self.kind = Kind::Drain,
                        Unidirectional::Refuse => {
                            self.kind = Kind::Refused;
                            return Ok((taken, Some(IncomingEvent::Refuse)));
                        }
                    }
                }
                Kind::Control(control) => {
                    let (used, event) = control.read(rest)?;
                    let event = event.map(|event| match event {
                        ControlEvent::Settings(settings) => IncomingEvent::Settings(settings),
                        ControlEvent::GoAway(identifier) => IncomingEvent::GoAway(identifier),
                    });
                    return Ok((taken.saturating_add(used), event));
                }
                Kind::Drain | Kind::Refused => return Ok((input.len(), None)),
            }
        }
    }
}

/// A server's final answer to an extended CONNECT.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Answer {
    /// Its status. A WebSocket is open when it is 2xx (RFC 8441 section
    /// 5); 200 is what a server sends.
    pub status: u16,
    /// The subprotocol the server chose: the value of its one
    /// `sec-websocket-protocol` field. `None` when it sent none, or more
    /// than one, which chooses nothing.
    pub subprotocol: Option<Vec<u8>>,
}

/// What a WebSocket's stream carried from the server.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExchangeEvent<'input> {
    /// The final answer, once.
    Answered(Answer),
    /// Part of the content after it, never empty: the WebSocket's bytes
    /// when the answer opened one.
    Data(&'input [u8]),
    /// A trailer section, passed over, after which only the stream's end
    /// may come.
    Trailers,
}

/// The server's side of the stream of an extended CONNECT (REQ-6986 items
/// 2 and 6): interim responses passed over, the final answer, then the
/// content as it comes.
#[derive(Clone, Debug, Default)]
pub struct Exchange {
    message: Message,
    interims: usize,
    answered: bool,
}

impl Exchange {
    /// A stream on which the request has gone and nothing has come.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the stream may end here: after the final answer, between
    /// frames. A stream that ends sooner is `H3_REQUEST_INCOMPLETE` (RFC
    /// 9114 section 4.1.2), or inside a frame `H3_FRAME_ERROR` (section
    /// 7.1).
    #[must_use]
    pub fn may_end(&self) -> bool {
        self.answered && self.message.may_end()
    }

    /// Reads from the start of `input`: how many bytes it took and what
    /// they carried, if anything. Call it again with the rest while it
    /// takes bytes or gives events.
    ///
    /// # Errors
    /// What [`Message::read`], [`qpack::decode`] and [`message::response`]
    /// find; and `H3_EXCESSIVE_LOAD`, a stream error, at more than
    /// [`MAX_INTERIM_RESPONSES`] interim responses.
    pub fn read<'input>(&mut self, input: &'input [u8]) -> Result<(usize, Option<ExchangeEvent<'input>>), Error> {
        let mut taken = 0_usize;
        loop {
            let (used, event) = self.message.read(input.get(taken..).unwrap_or(&[]))?;
            taken = taken.saturating_add(used);
            let section = match event {
                None => return Ok((taken, None)),
                Some(MessageEvent::Data(part)) => return Ok((taken, Some(ExchangeEvent::Data(part)))),
                Some(MessageEvent::Trailers(_)) => return Ok((taken, Some(ExchangeEvent::Trailers))),
                Some(MessageEvent::Head(section)) => section,
            };
            let response = message::response(qpack::decode(&section, u64::from(MAX_FIELD_SECTION_BYTES))?)?;
            if (INTERIM_FIRST..=INTERIM_LAST).contains(&response.status) {
                // RFC 9114 section 4.1: the final response follows.
                self.interims = self.interims.saturating_add(1);
                if self.interims > MAX_INTERIM_RESPONSES {
                    return Err(Error::stream(Code::ExcessiveLoad));
                }
                self.message.interim();
                continue;
            }
            self.answered = true;
            let mut chosen = response.fields.iter().filter(|field| field.name == WEBSOCKET_PROTOCOL);
            let subprotocol = match (chosen.next(), chosen.next()) {
                (Some(one), None) => Some(one.value.clone()),
                _ => None,
            };
            return Ok((taken, Some(ExchangeEvent::Answered(Answer { status: response.status, subprotocol }))));
        }
    }
}

#[cfg(test)]
mod tests;
