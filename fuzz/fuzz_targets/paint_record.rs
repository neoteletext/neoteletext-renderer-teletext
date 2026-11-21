//! The PAINT record (DEC-7031 item 1; REQ-7024, REQ-7025, REQ-TST-040):
//! the reader never panics, accepts a body of exactly its fourteen bytes
//! with every value in its range, at least one of the two stated and no
//! rate for NAPLPS, and accepts only what its writer can write: what was
//! read is written to the same bytes and reads back as it was.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::packet::Packet;
use neoteletext_web::consts::{MAX_DEFAULT_PAINT_RATE, MAX_LINE_SPEED, MIN_LINE_SPEED, line_bytes};
use neoteletext_web::records::paint::{BYTES, encode, read};
use neoteletext_web::records::sheet::SheetKind;
use neoteletext_web::records::{ChannelRecord, Read, RecordReader, TYPE_PAINT, write_packets};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    assert_eq!(data.len(), BYTES);
    assert!(record.line_speed.is_some() || record.default_rate.is_some());
    assert!(record.line_speed.is_none_or(|speed| (MIN_LINE_SPEED..=MAX_LINE_SPEED).contains(&speed)));
    assert!(record.default_rate.is_none_or(|rate| (1..=MAX_DEFAULT_PAINT_RATE).contains(&rate)));
    assert!(record.default_rate.is_none() || record.language == SheetKind::Regis);
    // A line never delivers more bytes in a tick than its bits a second.
    if let Some(speed) = record.line_speed {
        let ticks = u64::from(record.generation);
        assert!(line_bytes(ticks, speed) <= ticks.saturating_mul(u64::from(speed)));
        assert!(line_bytes(ticks, speed) <= line_bytes(ticks + 1, speed));
    }
    let mut written: Vec<Packet> = Vec::new();
    encode(record, 0, 0, &mut |packet| written.push(packet)).expect("what was read is written");
    // The writer's packets are those of the bytes read: one encoding.
    let mut same: Vec<Packet> = Vec::new();
    write_packets(TYPE_PAINT, data, 0, 0, &mut |packet| same.push(packet)).expect("fourteen bytes fit a record");
    assert!(written == same, "what was read is written to the same bytes");
    let mut reader = RecordReader::new();
    let mut back = false;
    for packet in &written {
        if let Read::Record(ChannelRecord::Paint(again)) = reader.push(packet) {
            back = again == record;
        }
    }
    assert!(back, "what was written reads back as it was read");
});
