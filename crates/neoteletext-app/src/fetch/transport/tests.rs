//! The transport through a proxy played on this computer (REQ-WEB-032).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::Duration;

/// A proxy played on this computer: it reads the client's handshake,
/// answers with `answer`, then sends `HOST`, as the host would.
fn played_proxy(kind: neoteletext_web::proxy::ProxyKind, answer: Vec<u8>) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut heard = Vec::new();
        let mut byte = [0_u8; 1];
        let wanted = |heard: &[u8]| match kind {
            neoteletext_web::proxy::ProxyKind::Http => heard.ends_with(b"\r\n\r\n"),
            // The greeting, then the request for a 12-byte name.
            neoteletext_web::proxy::ProxyKind::Socks5 => heard.len() == 3 + 5 + 12 + 2,
        };
        while !wanted(&heard) {
            if kind == neoteletext_web::proxy::ProxyKind::Socks5 && heard.len() == 3 {
                stream.write_all(&[5, 0]).unwrap();
            }
            stream.read_exact(&mut byte).unwrap();
            heard.push(byte[0]);
        }
        // The answer and the host's first bytes in one write, as a proxy
        // relaying a server that speaks at once may send them.
        stream.write_all(&[answer.as_slice(), b"HOST"].concat()).unwrap();
        heard
    });
    (port, handle)
}

fn through(kind: neoteletext_web::proxy::ProxyKind, port: u16) -> StdTransport {
    let (wake, _woken) = mpsc::channel();
    StdTransport::new(&[], wake, Some(Proxy { kind, host: "127.0.0.1".into(), port }))
}

/// REQ-WEB-032: an HTTP proxy by CONNECT and a SOCKS5 proxy with the
/// host's name, on this computer, which the user chose; the tunnel's
/// first bytes are the host's.
#[test]
fn connections_go_through_the_proxy() {
    use neoteletext_web::proxy::ProxyKind;
    let (port, proxy) = played_proxy(ProxyKind::Http, b"HTTP/1.1 200 Connection established\r\n\r\n".to_vec());
    // What the proxy's answer brought, then what the socket gives.
    let first_four = |mut socket: TcpStream, early: Vec<u8>| {
        let mut host = early;
        let mut rest = vec![0_u8; b"HOST".len().saturating_sub(host.len())];
        socket.read_exact(&mut rest).unwrap();
        host.extend(rest);
        host
    };
    let (socket, peer, early) = through(ProxyKind::Http, port).opener.reach("news.example", 443).unwrap();
    assert_eq!(peer, IpAddr::from([127, 0, 0, 1]), "the peer is the proxy");
    assert_eq!(first_four(socket, early), b"HOST");
    assert!(proxy.join().unwrap().starts_with(b"CONNECT news.example:443 HTTP/1.1\r\n"));
    let (port, proxy) = played_proxy(ProxyKind::Socks5, vec![5, 0, 0, 1, 10, 0, 0, 1, 0, 80]);
    let (socket, _, early) = through(ProxyKind::Socks5, port).opener.reach("news.example", 80).unwrap();
    assert!(early.is_empty(), "a SOCKS5 answer is read to its length");
    assert_eq!(first_four(socket, early), b"HOST");
    assert_eq!(&proxy.join().unwrap()[3..8], &[5, 1, 0, 3, 12], "the name for the proxy to resolve");
}

/// REQ-WEB-032: the host's first bytes that came with the proxy's
/// answer reach the connection's reader before the socket's next.
#[test]
fn the_tunnels_first_bytes_reach_the_connection() {
    use neoteletext_web::proxy::ProxyKind;
    let (port, proxy) = played_proxy(ProxyKind::Http, b"HTTP/1.1 200 Connection established\r\n\r\n".to_vec());
    let transport = through(ProxyKind::Http, port).opener;
    let (mut connection, _) = transport.open("news.example", 80, Security::Plain).unwrap();
    proxy.join().unwrap();
    let mut seen = Vec::new();
    let mut buffer = [0_u8; 16];
    let started = std::time::Instant::now();
    while seen.len() < b"HOST".len() && started.elapsed() < Duration::from_secs(5) {
        match connection.receive(&mut buffer) {
            Received::Data(count) => seen.extend_from_slice(&buffer[..count]),
            Received::Nothing => std::thread::yield_now(),
            Received::Closed => break,
        }
    }
    assert_eq!(seen, b"HOST");
}

/// REQ-WEB-032: a refusing proxy fails the connection; nothing goes
/// direct.
#[test]
fn a_refusing_proxy_fails_the_connection() {
    use neoteletext_web::proxy::ProxyKind;
    let (port, proxy) = played_proxy(ProxyKind::Http, b"HTTP/1.1 403 Forbidden\r\n\r\n".to_vec());
    let error = through(ProxyKind::Http, port).opener.reach("news.example", 443).unwrap_err();
    assert!(error.contains("the proxy refused the connection to news.example"), "{error}");
    proxy.join().unwrap();
    assert!(Proxy::parse("ftp://p.example:21").is_err());
}

/// REQ-WEB-035, REQ-RCH-014, decision E.133: one TLS configuration, shared by every
/// connection, keeps sessions to resume and never sends early data.
#[test]
fn tls_resumes_and_sends_no_early_data() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (wake, _) = mpsc::channel();
    let transport = StdTransport::new(&[], wake, None);
    assert!(!transport.opener.tls.enable_early_data, "no 0-RTT: an input message could be replayed");
    let shared = Arc::clone(&transport.opener.tls);
    assert!(Arc::ptr_eq(&shared, &transport.opener.tls), "one configuration, so one store of sessions");
    assert!(
        format!("{:?}", transport.opener.tls.resumption).contains("MemoryCache"),
        "sessions kept in memory to resume: {:?}",
        transport.opener.tls.resumption
    );
}

/// How long the test of a silent server allows `connect` to return; the
/// opening it starts would wait for TLS's answer for the connection's limit
/// of ten seconds.
const AT_ONCE: Duration = Duration::from_secs(2);

// REQ-490: connecting does not block the loop. A server that accepts the
// connection and never answers TLS's handshake leaves the connection
// connecting, and `connect` returns at once; before, it waited for the
// handshake's limit. Closing it abandons the opening.
#[test]
fn connecting_returns_at_once_while_the_server_is_silent() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (accepted, silent) = mpsc::channel();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        accepted.send(()).unwrap();
        // Held, unanswered, until the test ends.
        std::thread::sleep(Duration::from_secs(30));
        drop(stream);
    });
    let (wake, _woken) = mpsc::channel();
    let mut transport = StdTransport::new(&[IpAddr::from([127, 0, 0, 1])], wake, None);
    let started = std::time::Instant::now();
    transport.connect(ConnectionId::Fetch, "127.0.0.1", port, Security::Tls).unwrap();
    let took = started.elapsed();
    assert!(took < AT_ONCE, "connect blocked for {took:?}");
    silent.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(transport.state(ConnectionId::Fetch), LinkState::Connecting);
    transport.close(ConnectionId::Fetch);
    assert_eq!(transport.state(ConnectionId::Fetch), LinkState::Closed);
}

// REQ-490: a connection opened on its thread is taken up as open, and its
// peer is the address reached.
#[test]
fn a_connection_opened_on_its_thread_is_taken_up() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_secs(5));
        drop(stream);
    });
    let (wake, woken) = mpsc::channel();
    let mut transport = StdTransport::new(&[IpAddr::from([127, 0, 0, 1])], wake, None);
    transport.connect(ConnectionId::Push, "127.0.0.1", port, Security::Plain).unwrap();
    assert!(matches!(woken.recv_timeout(Duration::from_secs(10)), Ok(Wake::Network)), "the opening woke nobody");
    assert_eq!(transport.state(ConnectionId::Push), LinkState::Open(web_address(IpAddr::from([127, 0, 0, 1]))));
}

/// What one send of the test of a peer that stops reading hands over: the
/// longest input message.
const PIECE: usize = neoteletext_web::consts::INPUT_MESSAGE_MAX;
/// The most bytes that test sends before it must have seen the writer
/// thread hold some: 64 MiB, beyond what the operating system buffers for a
/// connection on this computer (a few MiB on Linux and macOS).
const STALL_WITHIN: usize = 64 << 20;

// REQ-490, REQ-RCH-035: a send does not block the loop. A peer that accepts
// the connection and never reads fills the socket's buffers; each send still
// returns at once, the bytes the writer thread holds are counted by
// `unsent`, and a send past the writer's bound fails rather than waits.
// Before the writer thread, the send that found the buffer full blocked for
// the write limit of ten seconds, and `unsent` was always 0.
#[test]
fn a_send_returns_at_once_while_the_peer_does_not_read() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        // Held, never read, until the test ends.
        std::thread::sleep(Duration::from_secs(60));
        drop(stream);
    });
    let (wake, woken) = mpsc::channel();
    let mut transport = StdTransport::new(&[IpAddr::from([127, 0, 0, 1])], wake, None);
    transport.connect(ConnectionId::Push, "127.0.0.1", port, Security::Plain).unwrap();
    assert!(matches!(woken.recv_timeout(Duration::from_secs(10)), Ok(Wake::Network)), "the opening woke nobody");
    assert!(matches!(transport.state(ConnectionId::Push), LinkState::Open(_)));
    let piece = vec![0_u8; PIECE];
    let (mut held, mut refused) = (0, false);
    for _ in 0..STALL_WITHIN / PIECE {
        let started = std::time::Instant::now();
        let sent = transport.send(ConnectionId::Push, &piece);
        let took = started.elapsed();
        assert!(took < AT_ONCE, "a send blocked for {took:?}");
        held = held.max(transport.unsent(ConnectionId::Push));
        if sent.is_err() {
            refused = true;
            break;
        }
    }
    assert!(held > 0, "the writer thread never held a byte: the peer's buffers took {STALL_WITHIN} bytes");
    assert!(refused, "a writer holding {held} bytes took more past its bound");
    assert!(held <= writer::MAX_UNSENT, "held {held}");
}
