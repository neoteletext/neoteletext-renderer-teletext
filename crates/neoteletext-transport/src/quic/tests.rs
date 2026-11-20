//! The HTTP/3 connection against a QUIC and HTTP/3 server played on this
//! computer with `quinn-proto` and `neoteletext-http3` themselves
//! (REQ-6985): streams apart on one connection, the TLS it offers, and
//! each way a connection or a stream fails.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `expect` naming what it expected, indexing into data it built to a known size, `panic!` on outcomes it rules out and arithmetic on small values it chose, where an overflow would also fail it"
)]

use super::*;
use crate::mozilla_roots;
use neoteletext_http3::stream::{Message as RequestStream, MessageEvent};
use neoteletext_http3::{frame, message, qpack};
use quinn_proto::crypto::rustls::QuicServerConfig;
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// How long a test gives what should happen at once.
const LIMIT: Duration = Duration::from_secs(5);
/// How often the played server looks at its socket.
const SERVER_TICK: Duration = Duration::from_millis(2);
/// What the played server sends when asked to flood: more than a stream's
/// flow-control window and its owner's pending bound together.
const FLOOD_BYTES: usize = 2 * STREAM_RECEIVE_WINDOW as usize;
/// The content that asks the played server to flood, and to close.
const FLOOD: &[u8] = b"flood";
const CLOSE: &[u8] = b"close";

/// How the played server behaves.
#[derive(Clone, Copy)]
struct Play {
    /// Its SETTINGS offer extended CONNECT.
    connect_protocol: bool,
    /// The status of its answer to every request.
    status: &'static [u8],
    /// The subprotocol its answer names.
    subprotocol: Option<&'static [u8]>,
}

const WELL: Play = Play { connect_protocol: true, status: b"200", subprotocol: Some(b"neoteletext") };

/// What the played server saw.
#[derive(Default)]
struct Seen {
    connections: AtomicUsize,
    requests: Mutex<Vec<(Vec<u8>, Vec<u8>)>>,
    /// Content received, every stream's together, in the order it came.
    content: Mutex<Vec<u8>>,
    flooded: AtomicUsize,
    stop: AtomicBool,
}

struct Server {
    address: SocketAddr,
    roots: rustls::RootCertStore,
    seen: Arc<Seen>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.seen.stop.store(true, Ordering::SeqCst);
    }
}

/// One request stream as the played server holds it.
struct Served {
    reader: RequestStream,
    out: VecDeque<u8>,
}

/// Starts a QUIC and HTTP/3 server on this computer, with the certificate
/// for `localhost` the TLS tests use.
fn server(play: Play) -> Server {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let certificate = CertificateDer::from(include_bytes!("../testdata/localhost-certificate.der").to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(include_bytes!("../testdata/localhost-key.der").to_vec()));
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate.clone()).unwrap();
    let mut tls = rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)
        .unwrap();
    tls.alpn_protocols = vec![ALPN_HTTP3.to_vec()];
    let config = quinn_proto::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls).unwrap()));
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.set_read_timeout(Some(SERVER_TICK)).unwrap();
    let address = socket.local_addr().unwrap();
    let seen = Arc::new(Seen::default());
    let theirs = Arc::clone(&seen);
    std::thread::spawn(move || serve(&socket, config, play, &theirs));
    Server { address, roots, seen }
}

/// The played server's loop: one connection at a time is enough here.
fn serve(socket: &UdpSocket, config: quinn_proto::ServerConfig, play: Play, seen: &Seen) {
    let mut endpoint = Endpoint::new(Arc::new(EndpointConfig::default()), Some(Arc::new(config)), false, None);
    let mut connections: HashMap<ConnectionHandle, (quinn_proto::Connection, SocketAddr, HashMap<StreamId, Served>)> =
        HashMap::new();
    let mut datagram = [0_u8; RECEIVE_DATAGRAM_BYTES];
    let mut buffer = Vec::new();
    while !seen.stop.load(Ordering::SeqCst) {
        if let Ok((length, from)) = socket.recv_from(&mut datagram) {
            let now = Instant::now();
            match endpoint.handle(now, from, None, None, BytesMut::from(&datagram[..length]), &mut buffer) {
                Some(DatagramEvent::ConnectionEvent(handle, event)) => {
                    if let Some((connection, ..)) = connections.get_mut(&handle) {
                        connection.handle_event(event);
                    }
                }
                Some(DatagramEvent::NewConnection(incoming)) => {
                    if let Ok((handle, connection)) = endpoint.accept(incoming, now, &mut buffer, None) {
                        seen.connections.fetch_add(1, Ordering::SeqCst);
                        connections.insert(handle, (connection, from, HashMap::new()));
                    }
                }
                Some(DatagramEvent::Response(transmit)) => {
                    let _ = socket.send_to(&buffer[..transmit.size], transmit.destination);
                }
                None => {}
            }
            buffer.clear();
        }
        let now = Instant::now();
        for (&handle, (connection, from, streams)) in &mut connections {
            if connection.poll_timeout().is_some_and(|at| at <= now) {
                connection.handle_timeout(now);
            }
            while let Some(event) = connection.poll_endpoint_events() {
                if let Some(event) = endpoint.handle_event(handle, event) {
                    connection.handle_event(event);
                }
            }
            while let Some(event) = connection.poll() {
                if matches!(event, Event::Connected) {
                    let control = connection.streams().open(Dir::Uni).unwrap();
                    let mut opening = Vec::new();
                    if play.connect_protocol {
                        stream::open_control(&mut opening);
                    } else {
                        // A client's SETTINGS leave the setting out, as a
                        // server without extended CONNECT does.
                        stream::open_client_control(&mut opening);
                    }
                    connection.send_stream(control).write(&opening).unwrap();
                }
            }
            while let Some(id) = connection.streams().accept(Dir::Bi) {
                streams.insert(id, Served { reader: RequestStream::new(), out: VecDeque::new() });
            }
            // The client's control stream is accepted and left unread.
            while connection.streams().accept(Dir::Uni).is_some() {}
            let mut close = false;
            for (&id, served) in streams.iter_mut() {
                serve_stream(connection, id, served, play, seen, &mut close);
            }
            if close {
                connection.close(now, VarInt::from_u32(Code::NoError.value()), Bytes::new());
            }
            loop {
                buffer.clear();
                let Some(transmit) = connection.poll_transmit(Instant::now(), 1, &mut buffer) else { break };
                let _ = socket.send_to(&buffer[..transmit.size], *from);
            }
        }
        connections.retain(|_, (connection, ..)| !connection.is_drained());
    }
}

/// Reads a request stream's bytes, answers its request, echoes its
/// content, and writes what it can of what waits.
fn serve_stream(
    connection: &mut quinn_proto::Connection,
    id: StreamId,
    served: &mut Served,
    play: Play,
    seen: &Seen,
    close: &mut bool,
) {
    let mut arrived = Vec::new();
    if let Ok(mut chunks) = connection.recv_stream(id).read(true) {
        while let Ok(Some(chunk)) = chunks.next(usize::MAX) {
            arrived.extend_from_slice(&chunk.bytes);
        }
        let _ = chunks.finalize();
    }
    let mut rest = arrived.as_slice();
    while let Ok((taken, event)) = served.reader.read(rest) {
        rest = &rest[taken..];
        match event {
            Some(MessageEvent::Head(section)) => {
                let request = message::request(qpack::decode(&section, u64::MAX).unwrap()).unwrap();
                seen.requests.lock().unwrap().push((request.authority.unwrap(), request.path.unwrap()));
                let mut fields: Vec<(&[u8], &[u8])> = vec![(b":status", play.status)];
                if let Some(subprotocol) = play.subprotocol {
                    fields.push((b"sec-websocket-protocol", subprotocol));
                }
                let mut section = Vec::new();
                qpack::encode(fields, &mut section);
                let mut head = Vec::new();
                frame::write(frame::HEADERS, &section, &mut head);
                served.out.extend(head);
            }
            Some(MessageEvent::Data(part)) if part == FLOOD => {
                let mut flood = Vec::new();
                client::data(&vec![0x55; FLOOD_BYTES], &mut flood);
                served.out.extend(flood);
            }
            Some(MessageEvent::Data(part)) if part == CLOSE => *close = true,
            Some(MessageEvent::Data(part)) => {
                seen.content.lock().unwrap().extend_from_slice(part);
                let mut echo = Vec::new();
                client::data(part, &mut echo);
                served.out.extend(echo);
            }
            None if taken == 0 => break,
            Some(MessageEvent::Trailers(_)) | None => {}
        }
    }
    loop {
        let (first, _) = served.out.as_slices();
        if first.is_empty() {
            break;
        }
        match connection.send_stream(id).write(first) {
            Ok(written) if written > 0 => {
                seen.flooded.fetch_add(written, Ordering::SeqCst);
                served.out.drain(..written);
            }
            _ => break,
        }
    }
}

/// A wake that counts its calls.
fn counter() -> (Wake, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    (
        Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }),
        count,
    )
}

/// Waits until `met` holds, at most [`LIMIT`] and `longer`.
fn until(longer: Duration, mut met: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < LIMIT + longer {
        if met() {
            return true;
        }
        std::thread::sleep(SERVER_TICK);
    }
    false
}

fn connect(server: &Server) -> Http3Connection {
    let tls = quic_tls(server.roots.clone()).unwrap();
    Http3Connection::connect(&tls, server.address, "localhost", Arc::new(|| {})).unwrap()
}

fn request(path: &'static [u8], wake: Wake) -> StreamRequest<'static> {
    StreamRequest {
        authority: b"localhost",
        path,
        subprotocol: b"neoteletext",
        user_agent: b"test",
        wake,
        wake_under: 1024,
        max_unsent: 64 * 1024,
    }
}

/// Opens a WebSocket and waits for its answer.
fn opened(connection: &Http3Connection, path: &'static [u8]) -> WebSocketStream {
    let stream = connection.open_websocket(&request(path, Arc::new(|| {}))).unwrap();
    assert!(until(Duration::ZERO, || stream.state() != StreamState::Opening), "answered");
    stream
}

/// Takes `wanted` bytes of a stream's content, or what came before it
/// ended or the limit passed.
fn take(stream: &WebSocketStream, wanted: usize) -> Vec<u8> {
    let mut content = Vec::new();
    let mut buffer = [0_u8; 4096];
    until(Duration::ZERO, || {
        loop {
            match stream.receive(&mut buffer) {
                Received::Data(count) => content.extend_from_slice(&buffer[..count]),
                Received::Nothing => return content.len() >= wanted,
                Received::Closed => return true,
            }
        }
    });
    content
}

/// REQ-6985 items 1, 5 and 6, DEC-6979 items 1 and 2: two streams, each a
/// WebSocket, open on one connection, are each answered, and each carries
/// its own bytes both ways.
#[test]
fn two_websockets_share_one_connection_and_carry_their_own_bytes() {
    let server = server(WELL);
    let connection = connect(&server);
    let first = opened(&connection, b"/ch/push");
    let second = opened(&connection, b"/ch/push?second");
    assert_eq!(connection.state(), ConnectionState::Ready);
    for stream in [&first, &second] {
        assert_eq!(stream.state(), StreamState::Answered { status: 200, subprotocol: Some(b"neoteletext".to_vec()) });
    }
    assert!(first.send(b"from the first"));
    assert!(second.send(b"and from the second"));
    assert_eq!(take(&first, 14), b"from the first");
    assert_eq!(take(&second, 19), b"and from the second");
    assert!(first.send(b" again"));
    assert_eq!(take(&first, 6), b" again");
    assert_eq!(second.receive(&mut [0_u8; 8]), Received::Nothing, "nothing of the first's");
    assert_eq!(server.seen.connections.load(Ordering::SeqCst), 1, "one QUIC connection");
    let requests = server.seen.requests.lock().unwrap().clone();
    assert_eq!(
        requests,
        [(b"localhost".to_vec(), b"/ch/push".to_vec()), (b"localhost".to_vec(), b"/ch/push?second".to_vec())]
    );
    assert_eq!(connection.peer(), server.address);
}

/// REQ-6985 item 2: the ALPN protocol `h3` alone, and no early data. That
/// the versions offered are TLS 1.3 alone is the builder's argument, which
/// the configuration does not show; QUIC carries no other (RFC 9001
/// section 4.2).
#[test]
fn the_configuration_offers_h3_over_tls_13_and_no_early_data() {
    let config = tls_config(mozilla_roots());
    assert_eq!(config.alpn_protocols, [b"h3".to_vec()]);
    assert!(!config.enable_early_data, "no 0-RTT");
}

/// REQ-6985 item 2: a certificate the roots do not hold fails the
/// connection, and a stream asked for on it ends.
#[test]
fn a_certificate_the_roots_do_not_hold_fails_the_connection() {
    let server = server(WELL);
    let tls = quic_tls(mozilla_roots()).unwrap();
    let (wake, woken) = counter();
    let connection = Http3Connection::connect(&tls, server.address, "localhost", wake).unwrap();
    let stream = connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).unwrap();
    assert!(until(Duration::ZERO, || connection.state() == ConnectionState::Lost(Lost::Unreachable)));
    assert_eq!(stream.state(), StreamState::Ended);
    assert!(woken.load(Ordering::SeqCst) >= 1, "the connection's owner is woken");
    assert!(server.seen.requests.lock().unwrap().is_empty(), "no request reached the server");
    assert!(connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).is_none(), "none on a lost connection");
}

/// REQ-6985 item 2: the certificate is checked against the host given.
#[test]
fn a_certificate_for_another_name_fails_the_connection() {
    let server = server(WELL);
    let tls = quic_tls(server.roots.clone()).unwrap();
    let connection = Http3Connection::connect(&tls, server.address, "other.example", Arc::new(|| {})).unwrap();
    assert!(until(Duration::ZERO, || connection.state() == ConnectionState::Lost(Lost::Unreachable)));
}

/// REQ-6985 item 5, REQ-6986 item 5: a server whose SETTINGS do not offer
/// extended CONNECT is of no use, and no request is sent to it.
#[test]
fn a_server_without_extended_connect_is_refused() {
    let server = server(Play { connect_protocol: false, ..WELL });
    let connection = connect(&server);
    let stream = connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).unwrap();
    assert!(until(Duration::ZERO, || connection.state() == ConnectionState::Lost(Lost::NoExtendedConnect)));
    assert_eq!(stream.state(), StreamState::Ended);
    assert!(server.seen.requests.lock().unwrap().is_empty(), "no extended CONNECT was sent");
}

/// REQ-6985: a peer that never answers fails the connection at the
/// handshake timeout, and not before.
#[test]
fn a_silent_peer_fails_the_connection_at_the_handshake_timeout() {
    let silent = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let tls = quic_tls(mozilla_roots()).unwrap();
    let started = Instant::now();
    let connection =
        Http3Connection::connect(&tls, silent.local_addr().unwrap(), "localhost", Arc::new(|| {})).unwrap();
    assert!(until(HANDSHAKE_TIMEOUT, || connection.state() == ConnectionState::Lost(Lost::Unreachable)));
    let took = started.elapsed();
    assert!(took >= HANDSHAKE_TIMEOUT, "not before the timeout: {took:?}");
    assert!(took < HANDSHAKE_TIMEOUT + Duration::from_secs(1), "and at it: {took:?}");
}

/// REQ-6985 item 7: when the server closes the connection every stream
/// ends, each owner is woken, and no more is sent.
#[test]
fn a_servers_close_ends_every_stream_and_wakes_each_owner() {
    let server = server(WELL);
    let connection = connect(&server);
    let (first_wake, first_woken) = counter();
    let (second_wake, second_woken) = counter();
    let first = connection.open_websocket(&request(b"/a", first_wake)).unwrap();
    let second = connection.open_websocket(&request(b"/b", second_wake)).unwrap();
    assert!(until(Duration::ZERO, || {
        first.state() != StreamState::Opening && second.state() != StreamState::Opening
    }));
    let (before_first, before_second) = (first_woken.load(Ordering::SeqCst), second_woken.load(Ordering::SeqCst));
    assert!(first.send(CLOSE));
    assert!(until(Duration::ZERO, || connection.state() == ConnectionState::Lost(Lost::Ended)));
    assert!(first_woken.load(Ordering::SeqCst) > before_first && second_woken.load(Ordering::SeqCst) > before_second);
    for stream in [&first, &second] {
        assert_eq!(stream.receive(&mut [0_u8; 8]), Received::Closed);
        assert!(!stream.send(b"more"), "nothing is sent on a lost connection");
    }
}

/// REQ-6985: a stream its owner does not read is read no further than its
/// pending bound, so QUIC's flow control holds the server; once the owner
/// takes it, all of it comes.
#[test]
fn a_stream_not_read_stops_at_its_pending_bound() {
    let server = server(WELL);
    let connection = connect(&server);
    let stream = opened(&connection, b"/ch/push");
    assert!(stream.send(FLOOD));
    // The server writes until the stream's window is full, and no more.
    let sent = || server.seen.flooded.load(Ordering::SeqCst);
    assert!(until(Duration::ZERO, || sent() >= STREAM_PENDING_BYTES));
    std::thread::sleep(Duration::from_millis(300));
    let held = connection.shared.lock().streams.get(&stream.key).unwrap().pending.len();
    assert_eq!(held, STREAM_PENDING_BYTES, "held at the bound");
    let window = STREAM_PENDING_BYTES + STREAM_RECEIVE_WINDOW as usize;
    assert!(sent() <= window + 64 * 1024, "the server was held near {window}: {}", sent());
    assert!(sent() < FLOOD_BYTES, "and has not sent it all");
    let content = take(&stream, FLOOD_BYTES);
    assert_eq!(content.len(), FLOOD_BYTES);
    assert!(content.iter().all(|&byte| byte == 0x55));
}

/// REQ-6985 item 6: a status other than 200 is reported with its number,
/// and the subprotocol only when the server named one.
#[test]
fn another_status_is_reported_with_its_number() {
    let server = server(Play { status: b"403", subprotocol: None, ..WELL });
    let connection = connect(&server);
    let stream = opened(&connection, b"/ch/push");
    assert_eq!(stream.state(), StreamState::Answered { status: 403, subprotocol: None });
}

/// REQ-6985 item 6: a send is one DATA frame after those before it, the
/// unsent bytes are counted and fall as QUIC takes them, a send past the
/// owner's bound fails, and what was given before a stream is dropped and
/// the connection closed still reaches the server.
#[test]
fn sends_are_counted_bounded_and_leave_before_the_connection_closes() {
    let server = server(WELL);
    let connection = connect(&server);
    let (wake, _) = counter();
    let stream = connection.open_websocket(&StreamRequest { max_unsent: 4096, ..request(b"/ch/push", wake) }).unwrap();
    assert!(until(Duration::ZERO, || stream.state() != StreamState::Opening));
    assert!(!stream.send(&[0_u8; 4096]), "with its frame header it is past the bound");
    assert!(stream.send(b"one "));
    assert!(stream.send(b"two "));
    assert!(until(Duration::ZERO, || stream.unsent() == 0), "QUIC took them");
    assert!(stream.send(b"last"));
    drop(stream);
    let started = Instant::now();
    connection.close();
    assert!(started.elapsed() < CLOSE_WAIT + Duration::from_millis(300), "within the close wait");
    assert!(until(Duration::ZERO, || server.seen.content.lock().unwrap().as_slice() == b"one two last"));
    assert_eq!(connection.state(), ConnectionState::Lost(Lost::Ended));
}

/// REQ-6985: a request that cannot be carried, and a stream past the
/// connection's bound, are refused.
#[test]
fn a_request_that_cannot_be_written_and_a_stream_past_the_bound_are_refused() {
    let server = server(WELL);
    let connection = connect(&server);
    assert!(connection.open_websocket(&request(b"no-slash", Arc::new(|| {}))).is_none());
    assert!(
        connection.open_websocket(&StreamRequest { authority: b"a\nb", ..request(b"/", Arc::new(|| {})) }).is_none()
    );
    let streams: Vec<_> =
        (0..MAX_STREAMS).map(|_| connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).unwrap()).collect();
    assert!(connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).is_none(), "one past the bound");
    drop(streams);
    assert!(until(Duration::ZERO, || connection.open_websocket(&request(b"/ch/push", Arc::new(|| {}))).is_some()));
}
