//! The qrkeyboard client's JSON reader, from arbitrary bytes (REQ-416,
//! REQ-SEC-003; REQ-TST-040): the relay and the phone send JSON, so it is
//! read with bounded nesting and items and without a panic, and whatever is
//! read is written back canonically and read again the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_qrkeyboard::json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = Value::parse(data) {
        let canonical = value.canonical();
        let again = Value::parse(canonical.as_bytes()).expect("the canonical text is read again");
        assert_eq!(again.canonical(), canonical);
    }
});
