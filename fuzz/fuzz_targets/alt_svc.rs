//! The `Alt-Svc` reader with any bytes (REQ-6983): it may not panic, a
//! value over its bound names nothing, and what it names is a port of 1 to
//! 65535. The response head parser reads the same field when a head
//! carries it, so the bytes are also read as a head's one field.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::alt_svc::{self, AltSvc};
use neoteletext_web::consts::MAX_ALT_SVC_BYTES;

fuzz_target!(|data: &[u8]| {
    let named = alt_svc::read(data);
    if data.len() > MAX_ALT_SVC_BYTES {
        assert_eq!(named, AltSvc::None, "a value over the bound names nothing");
    }
    if let AltSvc::Http3 { port, .. } = named {
        assert_ne!(port, 0, "a port is 1 to 65535");
    }
    let mut head = b"HTTP/1.1 200 OK\r\nAlt-Svc: ".to_vec();
    head.extend_from_slice(data);
    head.extend_from_slice(b"\r\n\r\n");
    if let Ok(read) = neoteletext_web::http::parse_head(&head)
        && let Some(AltSvc::Http3 { port, .. }) = read.alt_svc
    {
        assert_ne!(port, 0);
    }
});
