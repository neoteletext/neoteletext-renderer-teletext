//! The SUBPAGES record (DOC-1959, REQ-1503, REQ-480, REQ-TST-040): every
//! body the reader accepts, written again by the encoder, reads back
//! unchanged; and the one reassembly of a sending, given any sequence of
//! records, never panics and calls a sending complete only when it is a
//! whole list that keeps its totals.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::records::subpages::{Sending, Step, SubPageEntry, encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

/// A service any record may be written on.
const SERVICE: u8 = 3;
/// What separates the records of a sending in the input.
const SEPARATOR: u8 = 0xFF;

fuzz_target!(|data: &[u8]| {
    // One record, written again.
    if let Ok(record) = read(data) {
        let entries: Vec<SubPageEntry<'_>> = record.entries().collect();
        let mut packets = Vec::new();
        encode(record.page, record.version, &entries, SERVICE, 0, &mut |packet| packets.push(packet))
            .expect("entries the reader took are a list the encoder takes");
        let mut reader = RecordReader::new();
        let mut back = Vec::new();
        for packet in &packets {
            if let Read::Record(ChannelRecord::SubPages(again)) = reader.push(packet) {
                assert_eq!((again.page, again.version), (record.page, record.version));
                back.extend(again.entries().map(|entry| (entry.subcode.value(), entry.title.to_owned())));
            }
        }
        let first: Vec<(u16, String)> =
            entries.iter().map(|entry| (entry.subcode.value(), entry.title.to_owned())).collect();
        assert_eq!(back, first);
    }
    // A sequence of records: a sending is complete only as a whole list.
    let mut sending = Sending::new();
    let mut held: Vec<(u16, String)> = Vec::new();
    for body in data.split(|&byte| byte == SEPARATOR).filter(|body| !body.is_empty()) {
        let Ok(record) = read(body) else { continue };
        if record.part == 0 {
            held.clear();
        }
        let entries: Vec<(u16, String)> =
            record.entries().map(|entry| (entry.subcode.value(), entry.title.to_owned())).collect();
        match sending.note(&record, 1) {
            Step::Partial => held.extend(entries),
            Step::Complete => {
                held.extend(entries);
                assert_eq!(held.len(), usize::from(record.count));
                assert!(held.windows(2).all(|pair| pair[0].0 < pair[1].0), "ascending, each once");
                held.clear();
            }
            Step::Refused { packets } => {
                assert!(packets >= 1);
                held.clear();
            }
        }
    }
});
