//! The frames between `neottx`'s fetcher and decoder processes, from
//! arbitrary bytes (REQ-431, REQ-SEC-018; REQ-TST-040): the decoder reads
//! what a compromised fetcher may send, so every frame is read strictly and
//! without a panic, and whatever is read is written back and read again the
//! same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_app::pipe::{read, write};

fuzz_target!(|data: &[u8]| {
    let mut input = data;
    while let Ok(Some(frame)) = read(&mut input) {
        let mut written = Vec::new();
        write(&mut written, &frame).expect("a frame read is written");
        let mut again = written.as_slice();
        assert_eq!(read(&mut again).expect("and read again").as_ref(), Some(&frame));
        assert!(again.is_empty(), "one frame, whole");
    }
});
