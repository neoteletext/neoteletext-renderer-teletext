//! The web agent's push connection (REQ-1462, REQ-480, REQ-TST-040): any
//! WebSocket messages a server sends after the handshake, the agent never
//! panics, and every byte range it delivers as pushed page bytes is whole
//! 42-byte packets, each one a stream may carry.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::consts::PACKET_BYTES;
use neoteletext_core::packet::{Packet, is_valid_stream_packet};
use neoteletext_core::types::Instant;
use neoteletext_web::agent::{Config, Input, Output, Source, Storage, UserAgent};
use neoteletext_web::consts::PUSH_SUBPROTOCOL;
use neoteletext_web::port::{ConnectionId, NetInput, NetOutput, RandomSource};
use neoteletext_web::url::IpAddress;

/// The service's address, and the one a name resolves to.
const SERVICE: &str = "http://tt.example/service.json";
const PEER: IpAddress = IpAddress::V4([93, 184, 216, 34]);
/// The service's descriptor: its push connection on the same host.
const DESCRIPTOR: &str =
    r#"{"name":"Fuzz","initial_page":"100","page_template":"/p/{mpp}","push":{"address":"ws://tt.example/push"}}"#;

/// Random bytes that are not random: the fuzzer's input is what varies.
struct Counter(u8);

impl RandomSource for Counter {
    fn fill(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            *byte = self.0;
            self.0 = self.0.wrapping_add(1);
        }
    }
}

/// A server's frame (RFC 6455 section 5.2): final, unmasked.
fn frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode];
    // The inputs are at most 381 bytes a message: a length byte of 127,
    // three bytes each.
    let length = u16::try_from(payload.len()).expect("a message of the fuzzer's");
    match u8::try_from(length) {
        Ok(short) if short <= 125 => out.push(short),
        _ => {
            out.push(126);
            out.extend_from_slice(&length.to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

/// The value of header `name` in a request's head.
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

fuzz_target!(|data: &[u8]| {
    // Boxed, as a port keeps it, and dropped with the agent each run.
    let mut storage: Box<Storage> = Box::new(Storage::new());
    let mut agent: Box<UserAgent<'_, Counter, 16, 4096>> =
        Box::new(UserAgent::new(Config::default(), storage.buffers(), Counter(0)));
    let now = Instant(1_000_000_000);
    // The fuzzer's bytes become the server's messages: a length byte, then
    // that many bytes, binary unless the length's top bit asks for text.
    let mut messages = Vec::new();
    let mut rest = data;
    while let Some((&length, after)) = rest.split_first() {
        let take = usize::from(length & 0x7F) * 3;
        let (payload, after) = after.split_at(take.min(after.len()));
        messages.extend(frame(if length & 0x80 == 0 { 0x2 } else { 0x1 }, payload));
        rest = after;
    }
    let mut inputs: Vec<(u8, ConnectionId, Vec<u8>)> = Vec::new();
    let mut sent_messages = false;
    let mut first = true;
    for _ in 0..64 {
        let mut outputs: Vec<(u8, ConnectionId, Vec<u8>)> = Vec::new();
        let mut sink = |output: Output<'_>| {
            if let Some(net) = output.net() {
                match net {
                    NetOutput::Connect { connection, .. } => outputs.push((0, connection, Vec::new())),
                    NetOutput::Send(connection, bytes) => outputs.push((1, connection, bytes.to_vec())),
                    NetOutput::Close(connection) => outputs.push((2, connection, Vec::new())),
                }
                return;
            }
            if let Output::PageBytes(page) = output
                && page.source == Source::Push
            {
                let (packets, rest) = page.bytes.as_chunks::<PACKET_BYTES>();
                assert!(rest.is_empty(), "whole packets");
                for &bytes in packets {
                    assert!(
                        is_valid_stream_packet(&Packet { bytes }),
                        "a packet the stream may not carry was delivered"
                    );
                }
            }
        };
        if first {
            agent.handle(now, Input::OpenService(SERVICE), &mut sink);
            first = false;
        }
        for (kind, connection, bytes) in inputs.drain(..) {
            let input = match kind {
                0 => NetInput::Connected(connection, PEER),
                _ => NetInput::Received(connection, &bytes),
            };
            agent.handle(now, Input::Net(input), &mut sink);
        }
        if outputs.is_empty() {
            break;
        }
        for (kind, connection, bytes) in outputs {
            match (kind, connection) {
                (0, _) => inputs.push((0, connection, Vec::new())),
                (1, ConnectionId::Fetch) if bytes.windows(4).any(|window| window == b"\r\n\r\n") => {
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{DESCRIPTOR}",
                        DESCRIPTOR.len()
                    );
                    inputs.push((1, connection, answer.into_bytes()));
                }
                (1, ConnectionId::Push) if !sent_messages && bytes.starts_with(b"GET ") => {
                    let head = String::from_utf8_lossy(&bytes).into_owned();
                    let Some(key) = header(&head, "Sec-WebSocket-Key") else { return };
                    let accept = neoteletext_web::ws::accept_for(key.as_bytes());
                    let mut answer = format!(
                        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
                         Sec-WebSocket-Protocol: {PUSH_SUBPROTOCOL}\r\nSec-WebSocket-Accept: {}\r\n\r\n",
                        String::from_utf8_lossy(&accept)
                    )
                    .into_bytes();
                    answer.extend_from_slice(&messages);
                    inputs.push((1, connection, answer));
                    sent_messages = true;
                }
                _ => {}
            }
        }
    }
});
