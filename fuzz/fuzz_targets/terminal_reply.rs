//! The terminal's replies to `neottx`'s graphics queries (REQ-382,
//! REQ-SEC-010, REQ-TST-040): any bytes, as a terminal might send them. No
//! input may panic; a version kept is printable and bounded, and graphics
//! are found only in the bytes within the bound.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_app::terminal_reply::{MAX_REPLY_BYTES, MAX_VERSION_BYTES, parse};

fuzz_target!(|data: &[u8]| {
    let replies = parse(data);
    if let Some(version) = &replies.version {
        assert!(!version.is_empty() && version.len() <= MAX_VERSION_BYTES);
        assert!(version.bytes().all(|byte| (b' '..=b'~').contains(&byte)));
    }
    let bounded = &data[..data.len().min(MAX_REPLY_BYTES)];
    assert_eq!(parse(bounded), replies, "nothing past the bound counts");
    if replies.sixel {
        assert!(replies.complete, "Sixel is known only from the attributes reply");
    }
});
