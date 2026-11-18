//! The RULES and `RULES_QUERY` records (REQ-6526; RULE-41, RULE-47).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use neoteletext_rules::table::default_send_all;
use std::boxed::Box;
use std::vec::Vec;

fn table_body(wheel: bool) -> Vec<u8> {
    let mut table = Box::new(Table::new());
    default_send_all(wheel, &mut table);
    let mut out = [0_u8; MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(0x101, 7, Some(&table), &mut out).unwrap();
    out[..length].to_vec()
}

/// RULE-41 (REQ-6526): a RULES record reaches a receiver's reader whole,
/// in as many fragments as it needs, and decodes to the table sent.
#[test]
fn a_rules_record_reaches_a_receiver_and_decodes() {
    let body = table_body(true);
    let mut reader = RecordReader::new();
    let mut seen = 0;
    let packets = encode(&body, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::Rules(record)) = reader.push(&packet) {
            seen += 1;
            assert_eq!((record.page.mpp(), record.generation, record.body), (0x101, 7, &body[..]));
            let mut table = Box::new(Table::new());
            let header = record.decode(&mut table).unwrap();
            assert!(header.has_table);
            assert_eq!(table.rules.len(), 13);
        }
    })
    .unwrap();
    assert_eq!(seen, 1);
    assert_eq!(packets, crate::records::fragments(body.len()));
    assert!(packets > 1, "the default table takes more than one fragment");
    // The body that says a page has no table.
    let mut none = [0_u8; HEADER_BYTES];
    let length = neoteletext_rules::encode(0x8FF, 65_535, None, &mut none).unwrap();
    let record = read(&none[..length]).unwrap();
    assert_eq!((record.page.mpp(), record.generation), (0x8FF, 65_535));
    assert!(!record.decode(&mut Table::new()).unwrap().has_table);
}

/// RULE-41, RULE-59, RULE-70 (REQ-6526): the reader takes the page and the
/// generation and leaves the table to the rule table's decoder, so that a
/// receiver can report a table it refuses; the writer sends nothing that
/// decoder refuses.
#[test]
fn a_faulty_table_is_not_written_and_is_refused_by_its_decoder() {
    let mut body = table_body(false);
    // A reserved bit of the first rule's flags.
    body[HEADER_BYTES] |= 0x80;
    assert_eq!(encode(&body, 3, 7, &mut |_| {}), Err(RecordFault::BadValue("rules table")));
    let record = read(&body).unwrap();
    assert_eq!(record.generation, 7, "the generation a RULES_FAULT names");
    let mut table = Box::new(Table::new());
    assert!(record.decode(&mut table).is_err());
    assert_eq!(*table, Table::new(), "refused whole");
    // What the reader itself refuses.
    assert_eq!(read(&body[..HEADER_BYTES - 1]), Err(RecordFault::Length));
    assert_eq!(read(&[0_u8; MAX_TABLE_BYTES + 1]), Err(RecordFault::Length));
    let mut page = table_body(false);
    page[0] = 0x09;
    assert_eq!(read(&page), Err(RecordFault::BadValue("rules page")));
    let mut generation = table_body(false);
    (generation[2], generation[3]) = (0, 0);
    assert_eq!(read(&generation), Err(RecordFault::BadValue("rules generation")));
}

/// RULE-47 (REQ-6526): a `RULES_QUERY` record is its six bytes, in one
/// packet.
#[test]
fn a_query_record_is_its_six_bytes() {
    let page = PageAddress::from_mpp(0x101).unwrap();
    let record = RulesQueryRecord { page, identifier: 0x0102_0304 };
    assert_eq!(read_query(&[0x01, 0x01, 1, 2, 3, 4]), Ok(record));
    let mut reader = RecordReader::new();
    let mut seen = None;
    let packets = encode_query(record, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::RulesQuery(read)) = reader.push(&packet) {
            seen = Some(read);
        }
    })
    .unwrap();
    assert_eq!((packets, seen), (1, Some(record)));
    assert_eq!(read_query(&[0x01, 0x01, 1, 2, 3]), Err(RecordFault::Length));
    assert_eq!(read_query(&[0x01, 0x01, 1, 2, 3, 4, 5]), Err(RecordFault::Length));
    assert!(read_query(&[0x00, 0xFF, 1, 2, 3, 4]).is_err());
}
