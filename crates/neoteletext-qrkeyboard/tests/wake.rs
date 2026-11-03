//! Tests of the screen's wait (REQ-INT-009, decision E.106): it ends the moment the relay sends
//! something or a waker wakes it, and otherwise sleeps until its limit,
//! against a relay played here: its instance answer and its WebSocket.
//!
//! The tests the gate runs assert what ends the wait, with each bound a
//! ceiling a loaded machine stays far inside (`DEVELOPMENT.md` section 6:
//! a timeout is a ceiling, not a wait); a wake that is missed runs the wait
//! to its limit. How soon the wait ends is measured by the ignored tests,
//! run alone on an idle machine, since at load averages of 9 to 28 a gate
//! measured waits of 166 to 437 ms that took under a millisecond alone.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, reason = "a test")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use neoteletext_qrkeyboard::config::{Layout, ScreenConfig};
use neoteletext_qrkeyboard::screen::{Event, Screen};
use neoteletext_web::ws::accept_for;

/// RFC 6455 section 5.2: an unmasked final text frame of a short payload.
const FIN_TEXT: u8 = 0x81;
/// The measured tests: a wait that should end at once ends well inside
/// this. The old screen's wait could not be ended at all, and polled every
/// 50 ms.
const AT_ONCE: Duration = Duration::from_millis(40);
/// The limit of a wait something should end; reaching it means the wake was
/// missed.
const LIMIT: Duration = Duration::from_secs(30);
/// A wait nothing ends lasts its limit.
const QUIET: Duration = Duration::from_millis(300);
/// How far past its limit a wait nothing ends may run on a loaded machine
/// before it counts as not ending at its limit.
const OVERRUN: Duration = Duration::from_secs(10);
/// How long the wakers wait before they act, so the screen is asleep.
const ASLEEP: Duration = Duration::from_millis(150);
/// The join code the played relay hands out: 16 base-32 symbols.
const CODE: &str = "ABCDEFGHIJKLMN27";

/// Reads one request head; its lines, lower-cased names.
fn read_head(reader: &mut BufReader<TcpStream>) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" || line.is_empty() {
            return lines;
        }
        lines.push(line.trim_end().to_owned());
    }
}

/// Answers the screen's `POST /i` with an instance on the relay's own
/// WebSocket.
fn answer_instance(listener: &TcpListener) {
    let port = listener.local_addr().unwrap().port();
    let (stream, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let head = read_head(&mut reader);
    let length: usize = head
        .iter()
        .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|value| value.trim().to_owned()))
        .unwrap()
        .parse()
        .unwrap();
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).unwrap();
    let answer = format!(r#"{{"code":"{CODE}","ownerToken":"owner","relay":"ws://127.0.0.1:{port}/s"}}"#);
    let mut stream = stream;
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}", answer.len()).unwrap();
}

/// Accepts the screen's WebSocket and answers its handshake; the frames the
/// screen sends (its auth) are read and dropped by a thread of their own.
fn accept_websocket(listener: &TcpListener) -> TcpStream {
    let (stream, _) = listener.accept().unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let key = read_head(&mut reader)
        .iter()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("sec-websocket-key").then(|| value.trim().to_owned())
        })
        .unwrap();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        String::from_utf8_lossy(&accept_for(key.as_bytes()))
    )
    .unwrap();
    thread::spawn(move || {
        let mut sink = [0_u8; 1024];
        while reader.read(&mut sink).is_ok_and(|count| count > 0) {}
    });
    stream
}

/// The relay: answers `POST /i` with an instance on its own WebSocket,
/// accepts that WebSocket, and then sends each text it is given.
fn relay(listener: &TcpListener, texts: &mpsc::Receiver<String>) {
    answer_instance(listener);
    let mut stream = accept_websocket(listener);
    for text in texts {
        let mut frame = vec![FIN_TEXT, u8::try_from(text.len()).unwrap()];
        frame.extend_from_slice(text.as_bytes());
        stream.write_all(&frame).unwrap();
    }
}

/// A relay that drops the screen's WebSocket as soon as it is open, then
/// accepts the screen's next connection and never answers its handshake,
/// as a relay that is restarting or overloaded may. It reports when it holds
/// that connection, and holds it until `release` ends.
fn dropping_relay(listener: &TcpListener, holding: &mpsc::Sender<()>, release: &mpsc::Receiver<()>) {
    answer_instance(listener);
    let first = accept_websocket(listener);
    first.shutdown(Shutdown::Both).unwrap();
    drop(first);
    let (second, _) = listener.accept().unwrap();
    holding.send(()).unwrap();
    let _ = release.recv();
    drop(second);
}

/// A screen on a played relay, and the sender of what the relay says.
fn screen() -> (Screen, mpsc::Sender<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (texts, said) = mpsc::channel();
    thread::spawn(move || relay(&listener, &said));
    let config = ScreenConfig { name: "wake test".into(), layout: Layout::Terminal, pin: false, rejoin_seconds: None };
    (Screen::create(&format!("http://127.0.0.1:{port}"), config, None).unwrap(), texts)
}

/// Wakes a sleeping screen with its waker; what the wait reported, and how
/// long after the wake it ended.
fn woken_by_the_waker() -> (Vec<Event>, Duration) {
    let (mut screen, _texts) = screen();
    let waker = screen.waker();
    let woke = Instant::now();
    thread::spawn(move || {
        thread::sleep(ASLEEP);
        waker.wake();
    });
    let events = screen.wait(Some(LIMIT)).unwrap();
    let late = woke.elapsed().checked_sub(ASLEEP).unwrap();
    println!("woken {late:?} after the wake");
    (events, late)
}

/// Has the relay send a sleeping screen a message; what the wait reported,
/// and how long after the message went it ended.
fn woken_by_the_relay() -> (Vec<Event>, Duration) {
    let (mut screen, texts) = screen();
    let sent = Instant::now();
    thread::spawn(move || {
        thread::sleep(ASLEEP);
        texts.send(r#"{"type":"ready"}"#.to_owned()).unwrap();
        // Kept open: the relay does not close.
        thread::sleep(LIMIT);
    });
    let events = screen.wait(Some(LIMIT)).unwrap();
    let late = sent.elapsed().checked_sub(ASLEEP).unwrap();
    println!("the relay's message reported {late:?} after it went");
    (events, late)
}

#[test]
fn a_waker_ends_the_wait() {
    let (events, late) = woken_by_the_waker();
    assert!(events.is_empty(), "{events:?}");
    assert!(late + ASLEEP < LIMIT, "the wait ran to its limit: the wake was missed");
}

#[test]
fn the_relays_message_ends_the_wait() {
    let (events, late) = woken_by_the_relay();
    assert_eq!(events, [Event::Scan]);
    assert!(late + ASLEEP < LIMIT, "the wait ran to its limit: the message was missed");
}

#[test]
#[ignore = "measurement; run alone on an idle machine"]
fn a_waker_ends_the_wait_at_once() {
    let (events, late) = woken_by_the_waker();
    assert!(events.is_empty(), "{events:?}");
    assert!(late < AT_ONCE, "the wait went on {late:?} after the wake");
}

#[test]
#[ignore = "measurement; run alone on an idle machine"]
fn the_relays_message_ends_the_wait_at_once() {
    let (events, late) = woken_by_the_relay();
    assert_eq!(events, [Event::Scan]);
    assert!(late < AT_ONCE, "the message waited {late:?}");
}

#[test]
fn a_wait_nothing_ends_lasts_its_limit_and_reports_nothing() {
    let (mut screen, _texts) = screen();
    let started = Instant::now();
    let events = screen.wait(Some(QUIET)).unwrap();
    let took = started.elapsed();
    assert!(events.is_empty(), "{events:?}");
    assert!(took >= QUIET && took < QUIET + OVERRUN, "{took:?}");
}

// REQ-199 (REQ-INT-009): a wake ends the wait at once even while the screen
// is reconnecting to a relay that dropped it and does not answer the new
// handshake. The opening of the connection must not hold up the wait: before
// the fix it blocked the waiting thread for the handshake's 15-second limit.
#[test]
fn a_waker_ends_the_wait_while_the_screen_reconnects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (holding, held) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    thread::spawn(move || dropping_relay(&listener, &holding, &released));
    let config =
        ScreenConfig { name: "reconnect test".into(), layout: Layout::Terminal, pin: false, rejoin_seconds: None };
    let mut screen = Screen::create(&format!("http://127.0.0.1:{port}"), config, None).unwrap();
    let waker = screen.waker();
    let (woke_at, woke) = mpsc::channel();
    thread::spawn(move || {
        // The relay holds the screen's second connection: the screen is
        // reconnecting, and the handshake will never be answered.
        held.recv().unwrap();
        thread::sleep(ASLEEP);
        woke_at.send(Instant::now()).unwrap();
        waker.wake();
    });
    let events = screen.wait(Some(LIMIT)).unwrap();
    let ended = Instant::now();
    let woken = woke.recv_timeout(LIMIT).expect("the relay never saw the screen reconnect");
    let late = ended.saturating_duration_since(woken);
    println!("woken {late:?} after the wake, during a reconnection");
    drop(release);
    assert!(events.is_empty(), "{events:?}");
    assert!(late < OVERRUN, "the wait went on {late:?} after the wake: the reconnection held it up");
}
