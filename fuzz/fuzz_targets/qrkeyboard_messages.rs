//! A phone's messages as the screen reads them once a frame is opened, from
//! arbitrary bytes (REQ-416, REQ-SEC-003; REQ-TST-040): a hostile phone or
//! relay controls the plaintext, so it is read without a panic, and a
//! message read has a well-formed id.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_qrkeyboard::message::{is_id, read};

fuzz_target!(|data: &[u8]| {
    if let Some(message) = read(data) {
        assert!(is_id(&message.id));
    }
});
