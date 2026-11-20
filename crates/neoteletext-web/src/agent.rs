//! The user agent (ARCHITECTURE.md layer L7): one sans-IO state machine
//! that ties the address parser, HTTP, WebSocket and the descriptor
//! together. It keeps no cookies: a channel's viewer state is a key the
//! port holds, proven on the push connection (the viewer-state design,
//! VS-201), and the agent only carries its records and messages.
//!
//! The application calls [`UserAgent::handle`] with an [`Input`]: a user
//! request, a network event from the transport, or a timer tick. The agent
//! answers through a sink with [`Output`]s: connections to open, bytes to
//! send, connections to close, the next timer, page bytes (TTI or T42) for
//! the format readers, and events for the viewer. Data in an output is
//! borrowed from the agent's buffers and is valid only during the call.
//!
//! Two connections are used: [`ConnectionId::Fetch`](crate::port::ConnectionId::Fetch) carries the
//! descriptor and page requests one at a time (REQ-WEB-014), and
//! [`ConnectionId::Push`](crate::port::ConnectionId::Push) carries the push stream (REQ-WEB-005).
//!
//! Push is a compile-time feature (decision E.61). The agent's
//! types are the same in every build. A build without a feature answers
//! whatever needs it with a value that says so, and every branch that would
//! reach the missing part tests a constant, so the optimiser and the linker
//! drop that part's code.

use crate::user_agent::UserAgentString;
use neoteletext_core::types::{Instant, PageAddress, SubCode};

use crate::bytes::TextBuf;
use crate::consts::{
    CACHE_ENTRIES, MAX_HOST_TEXT_LENGTH, MAX_SUBSCRIBED_PAGES, MAX_VALIDATOR_LENGTH, PAGE_QUEUE, PUSH_RETRY_FIRST_US,
    SECURE_HOSTS,
};
use crate::descriptor::Descriptor;
use crate::http::{BodyDecoder, HeadCollector, Method, ResponseHead};
use crate::port::RandomSource;
use crate::subpages::PartNumber;
use crate::url::Url;
use crate::ws::{FrameReader, HandshakeKey};

mod config;
mod events;
mod fetch;
mod input;
mod page;
mod push;
mod push_data;
mod push_messages;
mod request;
mod resource;
mod response;
mod secure;
mod service;
mod snapshot;

pub use config::{Buffers, Config, SnapshotTarget, Storage};
pub use events::{
    Event, Failure, Input, InputMessage, Output, PageBytes, PushProblem, PushStop, PushStream, ResourceRole, Sink,
    SnapshotEnd, Source, Subscription,
};

/// The bytes of one T42 packet (REQ-INP-002).
const T42_PACKET: usize = neoteletext_core::consts::PACKET_BYTES;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Link {
    Closed,
    Connecting,
    Open,
}

/// Which file a page request asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PageFile {
    /// The descriptor's page address template (REQ-WEB-003).
    Template,
    /// A channel's page file (REQ-WEB-019): T42 or TTI, and whether it is
    /// the second of the two tried.
    Channel { t42: bool, second: bool },
    /// A channel's sub-page file, `NNN/SSSS.t42` or `NNN/SSSS.tti`, of a page
    /// whose list says its sub-pages are files of their own (REQ-1887): which
    /// of the two, and whether it is the second tried.
    SubPage { t42: bool, second: bool },
}

/// What a queued request is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Want {
    /// A page, or a sub-page of it.
    Page,
    /// A part of a page's list of sub-pages, `NNN/P.sub` (REQ-1888).
    SubPages(PartNumber),
}

/// A request waiting for the fetch connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Queued {
    page: PageAddress,
    subcode: Option<SubCode>,
    want: Want,
}

/// Which file a cache entry's validators are for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cached {
    /// The page's own file, or the descriptor's template.
    Page,
    /// A sub-page's file, `NNN/SSSS.t42` or `NNN/SSSS.tti`.
    SubPage,
    /// A part of the page's list, `NNN/P.sub`.
    SubPages(PartNumber),
}

/// Page numbers of every magazine, 100 to 8FF, as bits from 100: the pages
/// whose lists say their sub-pages are files of their own.
const PAGE_SLOTS: usize = neoteletext_core::limits::PAGE_NUMBERS;
/// The first page number, 100.
const FIRST_PAGE: u16 = neoteletext_core::limits::FIRST_PAGE_ADDRESS;
/// Bits in one word of a set of pages.
const WORD_BITS: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Purpose {
    Descriptor {
        channel: bool,
    },
    FrontDoor,
    Page {
        page: PageAddress,
        subcode: Option<SubCode>,
        file: PageFile,
    },
    Contents,
    /// A part of a page's list of sub-pages (REQ-1888).
    SubPages {
        page: PageAddress,
        part: PartNumber,
    },
    /// One resource the port asked for outside any channel (REQ-2476,
    /// REQ-123): nothing of a channel is touched by its answer.
    Resource(ResourceRole),
}

#[derive(Clone, Copy, Debug)]
struct Exchange {
    purpose: Purpose,
    url: Url,
    method: Method,
    redirects: u8,
    deadline: Instant,
    sent: bool,
    /// The request went out on a connection already used once.
    reused: bool,
    /// One retry after a kept-alive connection closed unanswered.
    retried: bool,
    /// When this request was made, for the exchange's report (REQ-WEB-033).
    started: Instant,
}

#[derive(Clone, Copy, Debug)]
struct Target {
    host: TextBuf<MAX_HOST_TEXT_LENGTH>,
    port: u16,
    secure: bool,
}

impl Target {
    fn of(url: &Url) -> Self {
        let mut host = TextBuf::new();
        // The host always fits: the address parser bounds it.
        let _ = host.set(url.host_text().as_bytes());
        Self { host, port: url.port(), secure: url.is_secure() }
    }

    fn matches(&self, url: &Url) -> bool {
        self.host.as_str() == url.host_text() && self.port == url.port() && self.secure == url.is_secure()
    }
}

#[derive(Clone, Copy, Debug)]
struct FetchConnection {
    link: Link,
    target: Option<Target>,
    deadline: Option<Instant>,
    /// A response has completed on this connection.
    used: bool,
    exchange: Option<Exchange>,
    head: HeadCollector,
    response: Option<ResponseHead>,
    body: Option<BodyDecoder>,
    body_len: usize,
    /// Response bytes have arrived for the current exchange.
    started: bool,
}

#[derive(Clone, Copy, Debug)]
struct CacheEntry {
    page: PageAddress,
    subcode: Option<SubCode>,
    etag: Option<TextBuf<MAX_VALIDATOR_LENGTH>>,
    last_modified: Option<TextBuf<MAX_VALIDATOR_LENGTH>>,
    fresh_until: Option<Instant>,
    used: u64,
    /// For a channel, the page file that answered is T42.
    t42: bool,
    /// The file the validators are for.
    file: Cached,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PushPhase {
    Idle,
    /// It ended for good: nothing reconnects (DOC-1920).
    Stopped,
    Backoff {
        until: Instant,
    },
    Connecting {
        deadline: Instant,
    },
    Handshake {
        deadline: Instant,
    },
    /// Open, last heard from at `heard`, and pinged at `pinged` with no
    /// answer yet (REQ-WEB-005's heartbeat).
    Open {
        heard: Instant,
        pinged: Option<Instant>,
    },
}

/// A pointer's move held while the push socket is congested (REQ-RCH-035).
#[derive(Clone, Copy, Debug)]
struct HeldMove {
    bytes: [u8; crate::consts::PUSH_MOVE_MAX],
    length: usize,
}

/// The push WebSocket's round trip, measured by ping and pong
/// (REQ-RCH-016).
#[derive(Clone, Copy, Debug, Default)]
struct RoundTrip {
    /// Measured: keys go to the channel.
    on: bool,
    /// The last ping's payload, one byte, counted on from ping to ping.
    sequence: u8,
    /// The ping not yet answered, and when it went.
    waiting: Option<(u8, Instant)>,
    /// When the last ping went.
    last: Option<Instant>,
}

#[derive(Clone, Copy, Debug)]
struct PushConnection {
    phase: PushPhase,
    url: Option<Url>,
    key: HandshakeKey,
    head: HeadCollector,
    frames: FrameReader,
    delay_us: u64,
    healthy: bool,
    /// Frames written into the push request buffer and not yet sent: the
    /// messages waiting at one moment, sent together (REQ-RCH-010).
    out: usize,
    /// The push socket's unsent bytes as the port last reported them
    /// (REQ-RCH-035).
    unsent: usize,
    /// Each contact's latest move, held while the socket is congested.
    held: [Option<HeldMove>; crate::consts::PUSH_MOVE_SLOTS],
    round_trip: RoundTrip,
    /// Invalid packets since the connection opened (REQ-1462).
    invalid: u64,
    /// LOG records given on since the last page frame (REQ-INP-026).
    log_lines: usize,
    /// The port asked for the push connection to be opened as a stream it
    /// presents (REQ-6984).
    over_stream: bool,
    /// The connection being opened is such a stream: the port's answer is
    /// awaited, not a TCP connection's.
    opening_stream: bool,
    /// The port on which the answer that brought the open service's
    /// descriptor named HTTP/3 on its own host, and until when that is
    /// fresh (REQ-6983): a stream is asked for only on it, and only until
    /// then.
    http3: Option<(u16, Instant)>,
}

/// The web user agent, with random numbers from `R`.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag belongs to a different part of the agent: navigation, the fetch connection, push, the channel"
)]
// Not `Debug`: REQ-GEN-016 keeps `core::fmt` out of a small build, and the
// size check found the derive's panic paths.
pub struct UserAgent<'m, R: RandomSource> {
    config: Config,
    /// The `User-Agent` sent on every request and upgrade, composed once
    /// (REQ-WEB-031).
    named_as: UserAgentString,
    buffers: Buffers<'m>,
    random: R,
    descriptor: Option<Descriptor>,
    pending_descriptor: Option<(Url, bool)>,
    /// A resource the port asked for, waiting for the fetch connection.
    pending_resource: Option<(Url, ResourceRole)>,
    /// The open channel's folder (REQ-WEB-018), when a channel is open.
    channel: Option<Url>,
    /// The folder that holds the descriptor of a service opened by its
    /// descriptor, from the moment it is asked for (REQ-507): with its
    /// origin, the service's identity for redirects, as a
    /// channel's folder is a channel's.
    service_folder: Option<Url>,
    /// The page file extension to try first in the channel: the one that
    /// last answered (REQ-WEB-019).
    t42_first: bool,
    /// The same for a sub-page's file. It starts at T42, unlike a page's,
    /// because a generator writes sub-pages as T42, so that keying a sub-code
    /// on a built channel asks for one file (REQ-1887).
    sub_page_t42_first: bool,
    /// A page whose other file is to be tried next.
    second_try: Option<(PageAddress, Option<SubCode>, PageFile)>,
    /// The channel's bundle to fetch after its settings.
    front_door: Option<Url>,
    /// The channel's contents file to fetch.
    contents: Option<Url>,
    secure_hosts: [Option<TextBuf<MAX_HOST_TEXT_LENGTH>>; SECURE_HOSTS],
    fetch: FetchConnection,
    /// Close the fetch connection when the current response completes.
    close_fetch_after: bool,
    queue: [Option<Queued>; PAGE_QUEUE],
    /// The pages of the open channel whose list last answered: their
    /// sub-pages are asked for as files of their own (REQ-WEB-019).
    catalogued: [u32; PAGE_SLOTS / WORD_BITS],
    next_fetch_at: Instant,
    /// The deadline the port was last told, so it is told only of a change.
    timer_told: Option<Instant>,
    cache: [Option<CacheEntry>; CACHE_ENTRIES],
    cache_tick: u64,
    push: PushConnection,
    /// The channel's records on the push connection (DOC-1951).
    records: crate::records::RecordReader,
    /// The index sending in progress (DOC-1955).
    index: crate::records::index::Progress,
    /// The LINKS sending in progress, held to its totals (DOC-1958).
    links: crate::records::links::Sending,
    /// The SUBPAGES sending in progress, held to its totals (DOC-1959).
    subpage_list: crate::records::subpages::Sending,
    /// The open channel is live: its `channel.ttx` names a push
    /// connection, so nothing is fetched but its settings (REQ-WEB-018).
    live: bool,
    /// A live channel whose first page has not yet arrived.
    connecting: bool,
    subscribe_all: bool,
    subscribed: [Option<PageAddress>; MAX_SUBSCRIBED_PAGES],
    /// Snapshot mode's progress (REQ-6880); idle without it.
    snapshot: snapshot::Run,
}

fn event(sink: &mut Sink<'_>, event: Event<'_>) {
    sink(Output::Event(event));
}

impl<'m, R: RandomSource> UserAgent<'m, R> {
    /// A new agent with no service open.
    pub fn new(config: Config, buffers: Buffers<'m>, random: R) -> Self {
        Self {
            named_as: UserAgentString::of(config.product.as_ref()),
            config,
            buffers,
            random,
            descriptor: None,
            pending_descriptor: None,
            pending_resource: None,
            channel: None,
            service_folder: None,
            t42_first: false,
            sub_page_t42_first: true,
            second_try: None,
            front_door: None,
            contents: None,
            secure_hosts: [None; SECURE_HOSTS],
            fetch: FetchConnection {
                link: Link::Closed,
                target: None,
                deadline: None,
                used: false,
                exchange: None,
                head: HeadCollector::default(),
                response: None,
                body: None,
                body_len: 0,
                started: false,
            },
            close_fetch_after: false,
            queue: [None; PAGE_QUEUE],
            catalogued: [0; PAGE_SLOTS / WORD_BITS],
            next_fetch_at: Instant(0),
            timer_told: None,
            cache: [None; CACHE_ENTRIES],
            cache_tick: 0,
            push: PushConnection {
                phase: PushPhase::Idle,
                url: None,
                key: [0; crate::consts::WEBSOCKET_KEY_TEXT],
                head: HeadCollector::default(),
                frames: FrameReader::new(),
                delay_us: PUSH_RETRY_FIRST_US,
                healthy: false,
                out: 0,
                unsent: 0,
                held: [None; crate::consts::PUSH_MOVE_SLOTS],
                round_trip: RoundTrip::default(),
                invalid: 0,
                log_lines: 0,
                over_stream: false,
                opening_stream: false,
                http3: None,
            },
            records: crate::records::RecordReader::new(),
            index: crate::records::index::Progress::new(),
            links: crate::records::links::Sending::new(),
            subpage_list: crate::records::subpages::Sending::new(),
            live: false,
            connecting: false,
            subscribe_all: true,
            subscribed: [None; MAX_SUBSCRIBED_PAGES],
            snapshot: snapshot::Run::new(),
        }
    }

    /// The open service's descriptor.
    #[must_use]
    pub const fn descriptor(&self) -> Option<&Descriptor> {
        self.descriptor.as_ref()
    }
}
