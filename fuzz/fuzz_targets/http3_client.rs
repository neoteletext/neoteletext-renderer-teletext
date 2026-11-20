//! The HTTP/3 client's readers with any bytes, in pieces the input chooses
//! (REQ-6986): the stream of an extended CONNECT, whose answer is decoded
//! and read as a response, a unidirectional stream a server opened, read
//! from its first byte, and the response reader on the bytes as a field
//! section. None may panic, each must end, an answer's status is three
//! digits' worth, and content is never empty.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_http3::client::{Exchange, ExchangeEvent, Incoming};
use neoteletext_http3::limits::MAX_FIELD_SECTION_BYTES;
use neoteletext_http3::stream::{Opened, Role};
use neoteletext_http3::{message, qpack};

fuzz_target!(|data: &[u8]| {
    let Some((&split, rest)) = data.split_first() else { return };
    let piece = usize::from(split).max(1);

    let mut exchange = Exchange::new();
    let mut opened = Opened::new(Role::Client);
    let mut incoming = Incoming::new();
    let (mut exchanging, mut reading) = (true, true);
    for chunk in rest.chunks(piece) {
        let mut at = 0;
        while exchanging && at < chunk.len() {
            match exchange.read(&chunk[at..]) {
                Ok((taken, event)) => {
                    at += taken;
                    match event {
                        Some(ExchangeEvent::Answered(answer)) => {
                            assert!((200..=999).contains(&answer.status), "a final status");
                        }
                        Some(ExchangeEvent::Data(part)) => assert!(!part.is_empty(), "content is never empty"),
                        None if taken == 0 => break,
                        Some(ExchangeEvent::Trailers) | None => {}
                    }
                }
                Err(_) => exchanging = false,
            }
        }
        let _ = exchange.may_end();
        let mut at = 0;
        while reading && at < chunk.len() {
            match incoming.read(&mut opened, &chunk[at..]) {
                Ok((taken, event)) => {
                    at += taken;
                    if taken == 0 && event.is_none() {
                        break;
                    }
                }
                Err(_) => reading = false,
            }
        }
        let _ = incoming.critical();
    }

    if let Ok(fields) = qpack::decode(rest, u64::from(MAX_FIELD_SECTION_BYTES))
        && let Ok(response) = message::response(fields)
    {
        assert!((100..=999).contains(&response.status) && response.status != 101);
    }
});
