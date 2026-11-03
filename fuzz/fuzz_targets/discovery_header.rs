//! Discovery's reader of `Link` header fields (REQ-2476, REQ-TST-040):
//! any bytes after a status line, read as a response head; at most
//! `MAX_HEADER_POINTERS` pointers come out, and each target is checked and
//! resolved against the answering address.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::discovery::{self, MAX_HEADER_POINTERS};
use neoteletext_web::url::Url;

/// The status line before the fuzzer's bytes, so that its fields are read.
const STATUS_LINE: &[u8] = b"HTTP/1.1 200 OK\r\n";

fuzz_target!(|data: &[u8]| {
    let Ok(answered) = Url::parse("https://example.org/news/") else { return };
    let mut head = Vec::with_capacity(STATUS_LINE.len() + data.len());
    head.extend_from_slice(STATUS_LINE);
    head.extend_from_slice(data);
    let mut count = 0_usize;
    discovery::read_headers(&head, &mut |pointer| {
        count += 1;
        let _ = core::hint::black_box(discovery::resolve(&pointer, &answered));
    });
    assert!(count <= MAX_HEADER_POINTERS, "{count} pointers from the headers");
});
