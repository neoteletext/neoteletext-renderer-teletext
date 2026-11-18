//! The ANIMATE record (animation.md ANIM-6, ANIM-23; REQ-6529,
//! REQ-TST-040): the reader never panics, holds a record to 64 operations,
//! and accepts only what its writer can write.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::animate::Operation;
use neoteletext_rules::limits::MAX_ANIMATE_OPERATIONS;
use neoteletext_web::records::animate::{encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    let operations: Vec<Operation> = record.operations().collect();
    assert!((1..=MAX_ANIMATE_OPERATIONS).contains(&operations.len()));
    assert_eq!(operations.len(), usize::from(record.count));
    let mut reader = RecordReader::new();
    let mut back = false;
    encode(record.page, record.subcode, &operations, 0, 0, &mut |packet| {
        if let Read::Record(ChannelRecord::Animate(again)) = reader.push(&packet) {
            back = again == record;
        }
    })
    .expect("what was read is written");
    assert!(back, "what was written reads back as it was read");
});
