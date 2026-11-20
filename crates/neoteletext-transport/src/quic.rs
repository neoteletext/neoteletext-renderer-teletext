//! A client's HTTP/3 connection over QUIC (REQ-6985), on which each
//! WebSocket is opened as a stream by extended CONNECT (RFC 9220), so that the tabs
//! of one channel share one connection (neoteletext-common's DEC-6979).
//!
//! QUIC is `quinn-proto`'s state machine, which does no input or output;
//! two threads carry its datagrams. One waits on the UDP socket and passes
//! on what arrives. The other, the driver, owns the connection: it takes
//! datagrams and the owners' requests from one queue, waits no longer than
//! the connection's next timer, and writes what the connection has to
//! send. HTTP/3 is `neoteletext-http3`: this module moves its bytes between
//! QUIC's streams and the owners.
//!
//! An owner's side never waits on the network: a stream's bytes are taken
//! from and given to memory both sides share, within the bounds below, and
//! each stream wakes its owner when something has come for it.
//!
//! Everything a server sends is untrusted. What it can make this side hold
//! is bounded by QUIC's flow control ([`STREAM_RECEIVE_WINDOW`],
//! [`CONNECTION_RECEIVE_WINDOW`]), by the streams it may open
//! ([`MAX_PEER_UNIDIRECTIONAL_STREAMS`]) and by the bounds of
//! `neoteletext_http3::limits`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use neoteletext_http3::client::{self, Answer, Exchange, ExchangeEvent, Incoming, IncomingEvent, WebSocketRequest};
use neoteletext_http3::error::{Code, Error, Scope};
use neoteletext_http3::stream::{self, Opened, Role};
use neoteletext_web::port::Received;
use quinn_proto::crypto::rustls::QuicClientConfig;
use quinn_proto::{
    ClientConfig, ConnectionHandle, DatagramEvent, Dir, Endpoint, EndpointConfig, Event, IdleTimeout, ReadError,
    StreamEvent, StreamId, TransportConfig, VarInt, WriteError,
};

/// How long the connection may take from its first datagram to the
/// server's SETTINGS: the QUIC handshake and the first of HTTP/3. After it
/// the connection has failed, and its owners fall back (DEC-6979 item 3).
/// A handshake on a path that carries UDP takes two round trips, well
/// under a second; five seconds is a path that does not.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the connection lasts with no packet from the peer (RFC 9000
/// section 10.1).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// How often a quiet connection sends a packet that asks for an answer:
/// under a third of [`IDLE_TIMEOUT`], so two may be lost before the
/// connection is, and a dead path is found within the timeout.
pub const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const _: () = assert!(KEEP_ALIVE_INTERVAL.as_millis() * 3 <= IDLE_TIMEOUT.as_millis());
/// The unidirectional streams the server may have open: its control stream
/// and its two QPACK streams (RFC 9114 section 6.2), with room for streams
/// of the reserved types a server opens for exercise (section 6.2.3).
pub const MAX_PEER_UNIDIRECTIONAL_STREAMS: u32 = 8;
/// The bidirectional streams the server may open: none. HTTP/3 has no use
/// for one from a server (RFC 9114 section 6.1).
const MAX_PEER_BIDIRECTIONAL_STREAMS: u32 = 0;
/// QUIC's flow-control window of one stream (RFC 9000 section 4.1): the
/// most a server can send on a stream ahead of what was read. A megabyte
/// holds several of the largest page messages.
pub const STREAM_RECEIVE_WINDOW: u32 = 1024 * 1024;
/// QUIC's flow-control window of the connection: the most a server can
/// send on all streams together ahead of what was read.
pub const CONNECTION_RECEIVE_WINDOW: u32 = 8 * STREAM_RECEIVE_WINDOW;
/// The content of one stream read from QUIC and not yet taken by its
/// owner. At it the stream is read no further, so QUIC's flow control
/// holds the server.
pub const STREAM_PENDING_BYTES: usize = 256 * 1024;
/// The most streams, each a WebSocket, open on one connection: above the browser's most
/// tabs.
pub const MAX_STREAMS: usize = 64;
/// Datagrams and requests waiting for the driver at most. At it the
/// socket's thread waits, and the operating system drops what its own
/// buffer cannot hold, as it does for any slow reader of UDP.
pub const DATAGRAMS_AHEAD: usize = 64;
/// The largest UDP payload this side says it takes (RFC 9000 section
/// 18.2, `max_udp_payload_size`): what fits Ethernet's 1,500 bytes after
/// the IPv4 and UDP headers.
pub const MAX_UDP_PAYLOAD_BYTES: u16 = 1472;
/// The buffer a datagram is read into: above [`MAX_UDP_PAYLOAD_BYTES`], so
/// a longer one is seen to be longer and not taken for one that fits.
pub const RECEIVE_DATAGRAM_BYTES: usize = 2048;
const _: () = assert!(RECEIVE_DATAGRAM_BYTES > MAX_UDP_PAYLOAD_BYTES as usize);
/// How long a program's last sends may take to leave when it closes the
/// connection.
pub const CLOSE_WAIT: Duration = Duration::from_millis(200);
/// Bytes of a server's unidirectional stream read at a time: its control
/// stream's frames are a few tens of bytes.
const INCOMING_READ_BYTES: usize = 1024;
/// The ALPN protocol identifier of HTTP/3 (RFC 9114 section 3.1).
const ALPN_HTTP3: &[u8] = b"h3";

/// Wakes a stream's owner.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Why a connection is of no use.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lost {
    /// It never became ready: the handshake failed, a certificate was
    /// refused, or nothing answered within [`HANDSHAKE_TIMEOUT`].
    Unreachable,
    /// The server's SETTINGS did not offer extended CONNECT (RFC 9220
    /// section 3), so no WebSocket can be opened on it.
    NoExtendedConnect,
    /// It was ready and has ended: closed by the server, timed out, or
    /// broken.
    Ended,
}

/// A connection as its owners see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectionState {
    /// The handshake, or the wait for the server's SETTINGS.
    Connecting,
    /// A WebSocket may be opened.
    Ready,
    /// No use, and why.
    Lost(Lost),
}

/// A stream as its owner sees it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StreamState {
    /// The request has gone, or waits for the connection, and no answer
    /// has come.
    Opening,
    /// The server answered: its status, and the subprotocol it chose. The
    /// stream carries a WebSocket only if the owner accepts the answer.
    Answered {
        /// The answer's status.
        status: u16,
        /// The answer's one `sec-websocket-protocol`.
        subprotocol: Option<Vec<u8>>,
    },
    /// The stream ended before an answer, or its connection was lost.
    Ended,
}

/// The TLS of a QUIC connection, made once and used for every connection.
#[derive(Clone)]
pub struct QuicTls(ClientConfig);

/// TLS as a QUIC connection has it: see [`quic_tls`].
fn tls_config(roots: rustls::RootCertStore) -> rustls::ClientConfig {
    let mut tls = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![ALPN_HTTP3.to_vec()];
    tls.resumption = rustls::client::Resumption::default();
    tls.enable_early_data = false;
    tls
}

/// The TLS configuration of every QUIC connection (REQ-6985 item 2): TLS
/// 1.3, which is all QUIC has (RFC 9001 section 4.2), the ALPN protocol
/// `h3` alone, `roots` as the TCP path trusts them, resumption as there,
/// and no early data (0-RTT), which could replay a WebSocket's first
/// messages.
///
/// # Errors
/// When the TLS library has no cipher suite QUIC's first packets need,
/// which the `ring` provider always has.
pub fn quic_tls(roots: rustls::RootCertStore) -> io::Result<QuicTls> {
    let tls = tls_config(roots);
    let crypto = QuicClientConfig::try_from(tls).map_err(|error| io::Error::new(ErrorKind::Unsupported, error))?;
    let mut transport = TransportConfig::default();
    transport
        .max_concurrent_uni_streams(VarInt::from_u32(MAX_PEER_UNIDIRECTIONAL_STREAMS))
        .max_concurrent_bidi_streams(VarInt::from_u32(MAX_PEER_BIDIRECTIONAL_STREAMS))
        .stream_receive_window(VarInt::from_u32(STREAM_RECEIVE_WINDOW))
        .receive_window(VarInt::from_u32(CONNECTION_RECEIVE_WINDOW))
        .max_idle_timeout(IdleTimeout::try_from(IDLE_TIMEOUT).ok())
        .keep_alive_interval(Some(KEEP_ALIVE_INTERVAL))
        // No application datagrams: HTTP/3 here uses streams alone.
        .datagram_receive_buffer_size(None)
        // One datagram a send: the socket is the standard library's.
        .enable_segmentation_offload(false);
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(transport));
    Ok(QuicTls(config))
}

/// What a WebSocket's request names, and how its owner is woken.
pub struct StreamRequest<'request> {
    /// `:authority`: the host, with its port when the address names one.
    pub authority: &'request [u8],
    /// `:path`: the path and query.
    pub path: &'request [u8],
    /// The subprotocol offered.
    pub subprotocol: &'request [u8],
    /// What the client names itself as.
    pub user_agent: &'request [u8],
    /// Called when the stream's answer, bytes or end arrive, and when its
    /// unsent bytes fall under `wake_under`.
    pub wake: Wake,
    /// See `wake`.
    pub wake_under: usize,
    /// The most bytes the stream holds given and not yet taken by QUIC; a
    /// send past it fails.
    pub max_unsent: usize,
}

/// A stream's side shared with its owner.
struct StreamShared {
    answer: Option<Answer>,
    /// Content read and not yet taken.
    pending: VecDeque<u8>,
    ended: bool,
    /// Bytes given to send and not yet taken by QUIC.
    unsent: usize,
    /// The driver stopped reading at [`STREAM_PENDING_BYTES`].
    stalled: bool,
    wake: Wake,
    wake_under: usize,
    max_unsent: usize,
}

struct State {
    phase: ConnectionState,
    streams: HashMap<u64, StreamShared>,
}

struct Shared {
    state: Mutex<State>,
}

impl Shared {
    /// The state as it stands; a thread that panicked holding it changed
    /// nothing that matters here.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the driver is given.
enum Message {
    /// A datagram from the peer.
    Datagram(Vec<u8>),
    /// The socket said the peer refused a datagram (an ICMP answer).
    Refused,
    /// The socket was silent for [`IDLE_TIMEOUT`], or failed.
    SocketEnded,
    /// Open a WebSocket: its number and its request's HEADERS frame.
    Open(u64, Vec<u8>),
    /// Send these bytes, a whole DATA frame, on a stream.
    Send(u64, Vec<u8>),
    /// The owner took content of a stream the driver had stopped reading.
    Taken(u64),
    /// The owner closed a stream.
    Close(u64),
    /// Close the connection once what was given has left, within
    /// [`CLOSE_WAIT`].
    Shutdown,
}

/// Ends the driver when the last handle of the connection goes.
struct Closer {
    commands: SyncSender<Message>,
    driver: Mutex<Option<JoinHandle<()>>>,
}

impl Closer {
    /// Asks the driver to close and waits for it, at most [`CLOSE_WAIT`]
    /// and the time of one turn.
    fn close(&self) {
        let _ = self.commands.send(Message::Shutdown);
        let driver = self.driver.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(driver) = driver {
            let _ = driver.join();
        }
    }
}

impl Drop for Closer {
    fn drop(&mut self) {
        let _ = self.commands.send(Message::Shutdown);
    }
}

/// A client's HTTP/3 connection. Its handles are cheap copies of one
/// connection; the connection closes when [`Http3Connection::close`] is
/// called or the last handle is dropped.
#[derive(Clone)]
pub struct Http3Connection {
    commands: SyncSender<Message>,
    shared: Arc<Shared>,
    peer: SocketAddr,
    next_stream: Arc<AtomicU64>,
    closer: Arc<Closer>,
}

impl Http3Connection {
    /// Starts a connection to `peer`, whose certificate must be good for
    /// `host`, and returns at once; [`Http3Connection::state`] says how it
    /// goes, and `wake` is called when it is ready or lost. `peer` is an
    /// address the caller resolved and checked by its own rule (REQ-6985
    /// item 3); the socket takes datagrams from it alone.
    ///
    /// # Errors
    /// When no UDP socket can be made or pointed at `peer`, or `host` is
    /// not a name a certificate can be checked against.
    pub fn connect(tls: &QuicTls, peer: SocketAddr, host: &str, wake: Wake) -> io::Result<Self> {
        let local: SocketAddr = match peer {
            SocketAddr::V4(_) => (Ipv4Addr::UNSPECIFIED, 0).into(),
            SocketAddr::V6(_) => (Ipv6Addr::UNSPECIFIED, 0).into(),
        };
        let socket = UdpSocket::bind(local)?;
        // The kernel then passes on datagrams from this peer alone.
        socket.connect(peer)?;
        // A socket silent for the idle timeout is a connection lost, so
        // its thread ends then, and within that of the driver's end.
        socket.set_read_timeout(Some(IDLE_TIMEOUT))?;
        let mut endpoint_config = EndpointConfig::default();
        endpoint_config
            .max_udp_payload_size(MAX_UDP_PAYLOAD_BYTES)
            .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
        // No path MTU discovery: it needs the don't-fragment bit, which the
        // standard library's socket cannot set.
        let mut endpoint = Endpoint::new(Arc::new(endpoint_config), None, false, None);
        let (handle, connection) = endpoint
            .connect(Instant::now(), tls.0.clone(), peer, host)
            .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))?;
        let (commands, inbox) = mpsc::sync_channel(DATAGRAMS_AHEAD);
        let shared = Arc::new(Shared {
            state: Mutex::new(State { phase: ConnectionState::Connecting, streams: HashMap::new() }),
        });
        let reader = socket.try_clone()?;
        let datagrams = commands.clone();
        std::thread::spawn(move || read_datagrams(&reader, &datagrams));
        let started = Instant::now();
        let mut driver = Driver {
            socket,
            endpoint,
            handle,
            connection,
            shared: Arc::clone(&shared),
            peer,
            wake,
            streams: HashMap::new(),
            by_id: HashMap::new(),
            finishing: HashSet::new(),
            incoming: HashMap::new(),
            opened: Opened::new(Role::Client),
            control: VecDeque::new(),
            control_id: None,
            ready: false,
            lost: false,
            // A clock that cannot be read that far ahead leaves no time.
            deadline: Some(started.checked_add(HANDSHAKE_TIMEOUT).unwrap_or(started)),
            closing: None,
            buffer: Vec::new(),
        };
        let thread = std::thread::spawn(move || driver.run(&inbox));
        let closer = Arc::new(Closer { commands: commands.clone(), driver: Mutex::new(Some(thread)) });
        Ok(Self { commands, shared, peer, next_stream: Arc::new(AtomicU64::new(0)), closer })
    }

    /// How the connection stands.
    #[must_use]
    pub fn state(&self) -> ConnectionState {
        self.shared.lock().phase
    }

    /// The address the connection was made to.
    #[must_use]
    pub const fn peer(&self) -> SocketAddr {
        self.peer
    }

    /// Whether `other` is a handle of this same connection.
    #[must_use]
    pub fn is(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Asks for a WebSocket on the connection (RFC 9220 section 3). The
    /// request goes when the connection is ready, and at once when it is
    /// already. `None` when the connection is lost, holds [`MAX_STREAMS`],
    /// or the request cannot be written (a value no field may carry).
    #[must_use]
    pub fn open_websocket(&self, request: &StreamRequest<'_>) -> Option<WebSocketStream> {
        let mut head = Vec::new();
        let named = WebSocketRequest {
            authority: request.authority,
            path: request.path,
            subprotocol: request.subprotocol,
            user_agent: request.user_agent,
        };
        client::open_websocket(&named, &mut head).ok()?;
        let key = self.next_stream.fetch_add(1, Ordering::Relaxed);
        {
            let mut state = self.shared.lock();
            if matches!(state.phase, ConnectionState::Lost(_)) || state.streams.len() >= MAX_STREAMS {
                return None;
            }
            state.streams.insert(
                key,
                StreamShared {
                    answer: None,
                    pending: VecDeque::new(),
                    ended: false,
                    unsent: 0,
                    stalled: false,
                    wake: Arc::clone(&request.wake),
                    wake_under: request.wake_under,
                    max_unsent: request.max_unsent,
                },
            );
        }
        let stream = WebSocketStream { key, commands: self.commands.clone(), shared: Arc::clone(&self.shared) };
        // The stream's drop takes its entry away again.
        self.commands.send(Message::Open(key, head)).ok().map(|()| stream)
    }

    /// Closes the connection: what its streams were given leaves first,
    /// for at most [`CLOSE_WAIT`], and the server is told. Waits for that.
    pub fn close(&self) {
        self.closer.close();
    }
}

/// One WebSocket's stream. Dropping it closes the stream: what it was
/// given is sent, then its sending side finished and its receiving side
/// stopped (REQ-6985 item 6).
pub struct WebSocketStream {
    key: u64,
    commands: SyncSender<Message>,
    shared: Arc<Shared>,
}

impl WebSocketStream {
    /// How the stream stands.
    #[must_use]
    pub fn state(&self) -> StreamState {
        let state = self.shared.lock();
        let Some(stream) = state.streams.get(&self.key) else { return StreamState::Ended };
        match &stream.answer {
            Some(answer) => StreamState::Answered { status: answer.status, subprotocol: answer.subprotocol.clone() },
            None if stream.ended || matches!(state.phase, ConnectionState::Lost(_)) => StreamState::Ended,
            None => StreamState::Opening,
        }
    }

    /// Gives `bytes` to send as one DATA frame, after those given before;
    /// false when the stream is not answered, has ended, or would hold
    /// more than its owner's bound, and nothing is then sent.
    #[must_use]
    pub fn send(&self, bytes: &[u8]) -> bool {
        let mut frame = Vec::new();
        client::data(bytes, &mut frame);
        if frame.is_empty() {
            return true;
        }
        {
            let mut state = self.shared.lock();
            let Some(stream) = state.streams.get_mut(&self.key) else { return false };
            let held = stream.unsent.saturating_add(frame.len());
            if stream.ended || stream.answer.is_none() || held > stream.max_unsent {
                return false;
            }
            // Counted before it is queued, so the driver never takes off
            // what was not yet added.
            stream.unsent = held;
        }
        self.commands.send(Message::Send(self.key, frame)).is_ok()
    }

    /// The bytes given and not yet taken by QUIC, DATA frame headers
    /// among them.
    #[must_use]
    pub fn unsent(&self) -> usize {
        self.shared.lock().streams.get(&self.key).map_or(0, |stream| stream.unsent)
    }

    /// Takes content that has arrived into `buffer`, without waiting.
    pub fn receive(&self, buffer: &mut [u8]) -> Received {
        let resume = {
            let mut state = self.shared.lock();
            let Some(stream) = state.streams.get_mut(&self.key) else { return Received::Closed };
            if stream.pending.is_empty() {
                return if stream.ended { Received::Closed } else { Received::Nothing };
            }
            let mut taken = 0_usize;
            for slot in buffer.iter_mut() {
                let Some(byte) = stream.pending.pop_front() else { break };
                *slot = byte;
                taken = taken.saturating_add(1);
            }
            let resume = stream.stalled && stream.pending.len() < STREAM_PENDING_BYTES;
            if resume {
                stream.stalled = false;
            }
            (taken, resume)
        };
        if resume.1 {
            let _ = self.commands.send(Message::Taken(self.key));
        }
        Received::Data(resume.0)
    }
}

impl Drop for WebSocketStream {
    fn drop(&mut self) {
        self.shared.lock().streams.remove(&self.key);
        let _ = self.commands.send(Message::Close(self.key));
    }
}

/// The socket's thread: passes on each datagram until the socket is silent
/// for its read timeout, fails, or nothing takes them.
fn read_datagrams(socket: &UdpSocket, inbox: &SyncSender<Message>) {
    let mut buffer = [0_u8; RECEIVE_DATAGRAM_BYTES];
    loop {
        let message = match socket.recv(&mut buffer) {
            // A datagram that filled the buffer was longer than this side
            // said it takes, and is dropped.
            Ok(length) if length >= buffer.len() => continue,
            Ok(length) => Message::Datagram(buffer.get(..length).unwrap_or(&[]).to_vec()),
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            // A connected socket reports the peer's ICMP refusal of an
            // earlier datagram on the next call; the socket goes on.
            Err(error) if error.kind() == ErrorKind::ConnectionRefused => Message::Refused,
            Err(_) => {
                let _ = inbox.send(Message::SocketEnded);
                return;
            }
        };
        if inbox.send(message).is_err() {
            return;
        }
    }
}

/// A stream as the driver holds it.
struct StreamDriver {
    /// Its QUIC stream, once the connection was ready to open it.
    id: Option<StreamId>,
    /// What waits to be written: the request's HEADERS frame first.
    outgoing: VecDeque<u8>,
    /// How many of `outgoing`'s first bytes are the request's, which the
    /// owner's count of unsent bytes leaves out.
    head_left: usize,
    exchange: Exchange,
    /// The owner closed it: finish once `outgoing` has gone.
    closed: bool,
    /// Its content is not read: the owner holds [`STREAM_PENDING_BYTES`].
    stalled: bool,
    /// Its receiving side ended.
    read_done: bool,
}

struct Driver {
    socket: UdpSocket,
    endpoint: Endpoint,
    handle: ConnectionHandle,
    connection: quinn_proto::Connection,
    shared: Arc<Shared>,
    peer: SocketAddr,
    /// The connection's own wake: ready or lost.
    wake: Wake,
    streams: HashMap<u64, StreamDriver>,
    by_id: HashMap<StreamId, u64>,
    /// The streams finished at their owners' closing whose last bytes the
    /// server has not yet acknowledged: a closing connection waits for
    /// them, within [`CLOSE_WAIT`].
    finishing: HashSet<StreamId>,
    /// The unidirectional streams the server opened.
    incoming: HashMap<StreamId, Incoming>,
    opened: Opened,
    /// This side's control stream's bytes not yet written.
    control: VecDeque<u8>,
    control_id: Option<StreamId>,
    ready: bool,
    lost: bool,
    /// When the handshake and the server's SETTINGS must have come.
    deadline: Option<Instant>,
    /// When a closing connection stops waiting for its last sends.
    closing: Option<Instant>,
    buffer: Vec<u8>,
}

/// The HTTP/3 error code as QUIC carries it.
fn code(code: Code) -> VarInt {
    VarInt::from_u32(code.value())
}

impl Driver {
    fn run(&mut self, inbox: &Receiver<Message>) {
        loop {
            self.turn();
            if self.connection.is_drained() || (self.lost && self.closing.is_none()) {
                self.transmit();
                return;
            }
            let now = Instant::now();
            if self.closing.is_some_and(|until| now >= until) || (self.closing.is_some() && self.all_sent()) {
                self.connection.close(now, code(Code::NoError), Bytes::new());
                self.transmit();
                self.fail(Lost::Ended);
                return;
            }
            let until = [self.connection.poll_timeout(), self.deadline, self.closing].into_iter().flatten().min();
            let first = match until {
                Some(until) => match inbox.recv_timeout(until.saturating_duration_since(now)) {
                    Ok(message) => Some(message),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Message::Shutdown),
                },
                None => Some(inbox.recv().unwrap_or(Message::Shutdown)),
            };
            // Everything already here is taken before the connection is
            // asked what it has to send, so datagrams that came together
            // are answered together.
            for message in first.into_iter().chain(std::iter::from_fn(|| inbox.try_recv().ok()).take(DATAGRAMS_AHEAD)) {
                self.take(message);
            }
            let now = Instant::now();
            if self.connection.poll_timeout().is_some_and(|at| at <= now) {
                self.connection.handle_timeout(now);
            }
            if !self.ready && self.deadline.is_some_and(|at| at <= now) {
                self.give_up(Lost::Unreachable, Code::NoError);
            }
        }
    }

    /// Whether everything the owners gave has left: every stream was
    /// finished, and the server acknowledged each one's last bytes.
    fn all_sent(&self) -> bool {
        self.streams.is_empty() && self.finishing.is_empty()
    }

    fn take(&mut self, message: Message) {
        match message {
            Message::Datagram(data) => {
                let event = self.endpoint.handle(
                    Instant::now(),
                    self.peer,
                    None,
                    None,
                    BytesMut::from(data.as_slice()),
                    &mut self.buffer,
                );
                match event {
                    Some(DatagramEvent::ConnectionEvent(handle, event)) if handle == self.handle => {
                        self.connection.handle_event(event);
                    }
                    // A stateless answer of the endpoint's own, a version
                    // negotiation or a reset.
                    Some(DatagramEvent::Response(transmit)) => {
                        self.send_datagram(transmit.size, transmit.destination);
                    }
                    // No server here, and no other connection.
                    Some(DatagramEvent::ConnectionEvent(..) | DatagramEvent::NewConnection(_)) | None => {}
                }
                self.buffer.clear();
            }
            // Before the handshake a refusal is a server with no QUIC
            // listener, known at once; after it, one lost datagram.
            Message::Refused => {
                if !self.ready {
                    self.give_up(Lost::Unreachable, Code::NoError);
                }
            }
            Message::SocketEnded => {
                self.give_up(if self.ready { Lost::Ended } else { Lost::Unreachable }, Code::NoError);
            }
            Message::Open(key, head) => {
                let head_left = head.len();
                self.streams.insert(
                    key,
                    StreamDriver {
                        id: None,
                        outgoing: head.into(),
                        head_left,
                        exchange: Exchange::new(),
                        closed: false,
                        stalled: false,
                        read_done: false,
                    },
                );
            }
            Message::Send(key, frame) => {
                if let Some(stream) = self.streams.get_mut(&key) {
                    stream.outgoing.extend(frame);
                }
            }
            Message::Taken(key) => {
                if let Some(stream) = self.streams.get_mut(&key) {
                    stream.stalled = false;
                }
            }
            Message::Close(key) => {
                if let Some(stream) = self.streams.get_mut(&key) {
                    stream.closed = true;
                }
            }
            Message::Shutdown => {
                for stream in self.streams.values_mut() {
                    stream.closed = true;
                }
                let now = Instant::now();
                self.closing.get_or_insert(now.checked_add(CLOSE_WAIT).unwrap_or(now));
            }
        }
    }

    /// One turn: what the connection reports, the streams read and
    /// written, and what it has to send, until nothing more comes of it.
    fn turn(&mut self) {
        // Each pass that changes something is followed by another; a
        // pass's work is bounded by what the windows and bounds let in.
        loop {
            let mut progressed = false;
            while let Some(event) = self.connection.poll_endpoint_events() {
                if let Some(event) = self.endpoint.handle_event(self.handle, event) {
                    self.connection.handle_event(event);
                }
            }
            while let Some(event) = self.connection.poll() {
                progressed = true;
                self.event(&event);
            }
            if !self.lost {
                progressed |= self.accept_streams();
                progressed |= self.read_incoming();
                progressed |= self.open_streams();
                progressed |= self.write_streams();
                progressed |= self.read_streams();
            }
            self.transmit();
            if !progressed {
                return;
            }
        }
    }

    fn event(&mut self, event: &Event) {
        match *event {
            Event::Connected => {
                // RFC 9114 section 6.2.1: each side opens its control
                // stream and sends its SETTINGS first.
                self.control_id = self.connection.streams().open(Dir::Uni);
                let mut opening = Vec::new();
                stream::open_client_control(&mut opening);
                self.control = opening.into();
                if self.control_id.is_none() {
                    self.give_up(Lost::Unreachable, Code::StreamCreationError);
                }
            }
            Event::ConnectionLost { .. } => self.fail(if self.ready { Lost::Ended } else { Lost::Unreachable }),
            // The server will take no more of a stream: it is over.
            Event::Stream(StreamEvent::Stopped { id, .. }) => {
                if let Some(key) = self.by_id.get(&id).copied() {
                    self.end_stream(key);
                }
            }
            Event::Stream(StreamEvent::Finished { id }) => {
                self.finishing.remove(&id);
            }
            // Streams are looked at every turn, whatever became possible.
            Event::Stream(_) | Event::HandshakeDataReady | Event::DatagramReceived | Event::DatagramsUnblocked => {}
        }
    }

    /// Takes up the unidirectional streams the server opened.
    fn accept_streams(&mut self) -> bool {
        let mut accepted = false;
        while let Some(id) = self.connection.streams().accept(Dir::Uni) {
            self.incoming.insert(id, Incoming::new());
            accepted = true;
        }
        accepted
    }

    /// Reads the server's unidirectional streams (REQ-6986 item 4).
    fn read_incoming(&mut self) -> bool {
        let mut progressed = false;
        let ids: Vec<StreamId> = self.incoming.keys().copied().collect();
        for id in ids {
            let mut ended = false;
            let mut refused = false;
            let mut failure = None;
            let mut settings = None;
            let mut going_away = false;
            {
                let Self { connection, incoming, opened, .. } = self;
                let Some(reader) = incoming.get_mut(&id) else { continue };
                let mut receiving = connection.recv_stream(id);
                let Ok(mut chunks) = receiving.read(true) else { continue };
                loop {
                    match chunks.next(INCOMING_READ_BYTES) {
                        Ok(Some(chunk)) => {
                            progressed = true;
                            let mut rest: &[u8] = &chunk.bytes;
                            while failure.is_none() {
                                match reader.read(opened, rest) {
                                    Ok((taken, event)) => {
                                        rest = rest.get(taken..).unwrap_or(&[]);
                                        match event {
                                            Some(IncomingEvent::Settings(read)) => settings = Some(read),
                                            Some(IncomingEvent::GoAway(_)) => going_away = true,
                                            Some(IncomingEvent::Refuse) => refused = true,
                                            None if taken == 0 => break,
                                            None => {}
                                        }
                                    }
                                    Err(error) => failure = Some(error),
                                }
                            }
                            if failure.is_some() {
                                break;
                            }
                        }
                        Ok(None) | Err(ReadError::Reset(_)) => {
                            ended = true;
                            break;
                        }
                        Err(ReadError::Blocked) => break,
                    }
                }
                let _ = chunks.finalize();
                if refused {
                    // RFC 9114 section 6.2.3.
                    let _ = receiving.stop(code(Code::StreamCreationError));
                }
                // RFC 9114 section 6.2.1, RFC 9204 section 4.2.
                if ended && reader.critical() {
                    failure = failure.or(Some(Error::connection(Code::ClosedCriticalStream)));
                }
            }
            if refused || ended {
                self.incoming.remove(&id);
            }
            if let Some(error) = failure {
                self.give_up(if self.ready { Lost::Ended } else { Lost::Unreachable }, error.code);
                return true;
            }
            if let Some(settings) = settings {
                // REQ-6986 item 5: no extended CONNECT without the
                // server's word.
                if settings.connect_protocol {
                    self.ready = true;
                    self.deadline = None;
                    self.shared.lock().phase = ConnectionState::Ready;
                    (self.wake)();
                } else {
                    self.give_up(Lost::NoExtendedConnect, Code::NoError);
                    return true;
                }
            }
            if going_away {
                // RFC 9114 section 5.2: no new request may be made; the
                // owners move to a new connection together.
                self.give_up(Lost::Ended, Code::NoError);
                return true;
            }
        }
        progressed
    }

    /// Opens the QUIC stream of each WebSocket asked for, once the
    /// connection is ready.
    fn open_streams(&mut self) -> bool {
        if !self.ready {
            return false;
        }
        let mut opened = false;
        for (&key, stream) in &mut self.streams {
            if stream.id.is_none() {
                // None while the server's limit on streams is reached; a
                // later turn tries again.
                let Some(id) = self.connection.streams().open(Dir::Bi) else { break };
                stream.id = Some(id);
                self.by_id.insert(id, key);
                opened = true;
            }
        }
        opened
    }

    /// Writes what waits on this side's control stream and on each
    /// WebSocket, and finishes the streams their owners closed.
    fn write_streams(&mut self) -> bool {
        let mut progressed = false;
        if let Some(id) = self.control_id {
            let (first, _) = self.control.as_slices();
            if !first.is_empty()
                && let Ok(written) = self.connection.send_stream(id).write(first)
            {
                self.control.drain(..written.min(self.control.len()));
                progressed |= written > 0;
            }
        }
        let mut over = Vec::new();
        let mut woken = Vec::new();
        for (&key, stream) in &mut self.streams {
            let Some(id) = stream.id else {
                // Closed before it was ever opened: nothing went.
                if stream.closed {
                    over.push(key);
                }
                continue;
            };
            loop {
                let (first, _) = stream.outgoing.as_slices();
                if first.is_empty() {
                    break;
                }
                match self.connection.send_stream(id).write(first) {
                    Ok(0) | Err(WriteError::Blocked) => break,
                    Ok(written) => {
                        progressed = true;
                        stream.outgoing.drain(..written.min(stream.outgoing.len()));
                        let of_head = written.min(stream.head_left);
                        stream.head_left = stream.head_left.saturating_sub(of_head);
                        let counted = written.saturating_sub(of_head);
                        if counted > 0
                            && let Some(shared) = self.shared.lock().streams.get_mut(&key)
                        {
                            let before = shared.unsent;
                            shared.unsent = before.saturating_sub(counted);
                            if before >= shared.wake_under && shared.unsent < shared.wake_under {
                                woken.push(Arc::clone(&shared.wake));
                            }
                        }
                    }
                    Err(WriteError::Stopped(_) | WriteError::ClosedStream) => {
                        over.push(key);
                        break;
                    }
                }
            }
            if stream.closed && stream.outgoing.is_empty() {
                // REQ-6985 item 6: an orderly end of this side's bytes, and
                // no more of the server's wanted.
                if self.connection.send_stream(id).finish().is_ok() {
                    self.finishing.insert(id);
                }
                let _ = self.connection.recv_stream(id).stop(code(Code::RequestCancelled));
                over.push(key);
                progressed = true;
            }
        }
        for key in over {
            self.end_stream(key);
        }
        for wake in woken {
            wake();
        }
        progressed
    }

    /// Reads each WebSocket's stream: its answer, then its content, as far
    /// as its owner has room.
    fn read_streams(&mut self) -> bool {
        let mut progressed = false;
        let keys: Vec<u64> = self.streams.keys().copied().collect();
        for key in keys {
            let mut outcome = Ok(false);
            {
                let Self { connection, streams, shared, .. } = self;
                let Some(stream) = streams.get_mut(&key) else { continue };
                let Some(id) = stream.id else { continue };
                if stream.stalled || stream.read_done || stream.closed {
                    continue;
                }
                let mut receiving = connection.recv_stream(id);
                let Ok(mut chunks) = receiving.read(true) else { continue };
                loop {
                    let room = STREAM_PENDING_BYTES
                        .saturating_sub(shared.lock().streams.get(&key).map_or(0, |held| held.pending.len()));
                    if room == 0 {
                        stream.stalled = true;
                        if let Some(held) = shared.lock().streams.get_mut(&key) {
                            held.stalled = true;
                        }
                        break;
                    }
                    match chunks.next(room) {
                        Ok(Some(chunk)) => {
                            progressed = true;
                            if let Err(error) = feed(&mut stream.exchange, shared, key, &chunk.bytes) {
                                outcome = Err(error);
                                break;
                            }
                        }
                        Ok(None) | Err(ReadError::Reset(_)) => {
                            stream.read_done = true;
                            outcome = Ok(true);
                            break;
                        }
                        Err(ReadError::Blocked) => break,
                    }
                }
                let _ = chunks.finalize();
            }
            match outcome {
                Ok(false) => {}
                Ok(true) => self.end_stream(key),
                Err(error) if error.scope == Scope::Connection => {
                    self.give_up(Lost::Ended, error.code);
                    return true;
                }
                Err(error) => {
                    if let Some(id) = self.streams.get(&key).and_then(|stream| stream.id) {
                        let _ = self.connection.send_stream(id).reset(code(error.code));
                        let _ = self.connection.recv_stream(id).stop(code(error.code));
                    }
                    self.end_stream(key);
                }
            }
        }
        progressed
    }

    /// A stream is over: its owner sees its end once it has taken what
    /// came, and the driver forgets it.
    fn end_stream(&mut self, key: u64) {
        if let Some(stream) = self.streams.remove(&key)
            && let Some(id) = stream.id
        {
            self.by_id.remove(&id);
            if !stream.closed {
                let _ = self.connection.send_stream(id).finish();
            }
        }
        let wake = self.shared.lock().streams.get_mut(&key).map(|shared| {
            shared.ended = true;
            shared.unsent = 0;
            Arc::clone(&shared.wake)
        });
        if let Some(wake) = wake {
            wake();
        }
    }

    /// Closes the connection with `error` and marks it lost.
    fn give_up(&mut self, lost: Lost, error: Code) {
        if !self.lost {
            self.connection.close(Instant::now(), code(error), Bytes::new());
            self.transmit();
            self.fail(lost);
        }
    }

    /// The connection is lost: every stream ends, and every owner is
    /// woken (REQ-6985 item 7).
    fn fail(&mut self, lost: Lost) {
        if self.lost {
            return;
        }
        self.lost = true;
        self.streams.clear();
        self.by_id.clear();
        self.finishing.clear();
        let wakes: Vec<Wake> = {
            let mut state = self.shared.lock();
            state.phase = ConnectionState::Lost(lost);
            state
                .streams
                .values_mut()
                .map(|stream| {
                    stream.ended = true;
                    stream.unsent = 0;
                    Arc::clone(&stream.wake)
                })
                .collect()
        };
        (self.wake)();
        for wake in wakes {
            wake();
        }
    }

    /// Sends what the connection has to send.
    fn transmit(&mut self) {
        loop {
            self.buffer.clear();
            let Some(transmit) = self.connection.poll_transmit(Instant::now(), 1, &mut self.buffer) else { return };
            self.send_datagram(transmit.size, transmit.destination);
        }
    }

    /// Sends the first `size` bytes of the buffer, to the peer alone: the
    /// connection never follows it to another address (REQ-6985 item 3).
    fn send_datagram(&self, size: usize, destination: SocketAddr) {
        if destination == self.peer
            && let Some(datagram) = self.buffer.get(..size)
        {
            // A datagram the network does not take is a datagram lost,
            // which QUIC sends again.
            let _ = self.socket.send(datagram);
        }
    }
}

/// Gives a WebSocket's stream bytes to its exchange, and what they carried
/// to its owner.
fn feed(exchange: &mut Exchange, shared: &Shared, key: u64, mut bytes: &[u8]) -> Result<(), Error> {
    loop {
        let (taken, event) = exchange.read(bytes)?;
        bytes = bytes.get(taken..).unwrap_or(&[]);
        let wake = {
            let mut state = shared.lock();
            let Some(stream) = state.streams.get_mut(&key) else { return Ok(()) };
            match event {
                Some(ExchangeEvent::Answered(answer)) => stream.answer = Some(answer),
                Some(ExchangeEvent::Data(part)) => stream.pending.extend(part),
                Some(ExchangeEvent::Trailers) => {}
                None if taken == 0 => return Ok(()),
                None => continue,
            }
            Arc::clone(&stream.wake)
        };
        wake();
    }
}

#[cfg(test)]
mod tests;
