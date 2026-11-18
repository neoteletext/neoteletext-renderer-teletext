//! The SHEET record (animation.md ANIM-17, ANIM-23; REQ-6529,
//! REQ-TST-040): the reader never panics, holds a sheet's stream to
//! `MAX_SHEET_BYTES`, and accepts only what its writer can write.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::consts::MAX_SHEET_BYTES;
use neoteletext_web::records::sheet::{encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    let end = usize::try_from(record.offset).expect("an offset within a sheet") + record.data.len();
    assert!(end <= MAX_SHEET_BYTES);
    let mut reader = RecordReader::new();
    let mut back = false;
    encode(record, 0, 0, &mut |packet| {
        if let Read::Record(ChannelRecord::Sheet(again)) = reader.push(&packet) {
            back = again == record;
        }
    })
    .expect("what was read is written");
    assert!(back, "what was written reads back as it was read");
});
