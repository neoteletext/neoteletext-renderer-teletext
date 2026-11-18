//! The RULES and `RULES_QUERY` records (REQ-6526, REQ-6615; RULE-41,
//! RULE-47, RULE-112).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, `panic!` on outcomes it rules out and plain arithmetic on small numbers"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use neoteletext_rules::command::{Command, TYPE_REPORT};
use neoteletext_rules::table::{Framed, Key, KeyKind, KeyMatch, Rule, Table, Trigger, default_send_all};
use std::boxed::Box;
use std::vec::Vec;

const PAGE: u16 = 0x101;
const SUB: u16 = 0x0001;

fn place() -> (PageAddress, SubCode) {
    (PageAddress::from_mpp(PAGE).unwrap(), SubCode::new(SUB))
}

fn image_of(table: &Table) -> Vec<u8> {
    let mut out = std::vec![0_u8; MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(table, body_check, &mut out).unwrap();
    out.truncate(length);
    out
}

fn default_image(wheel: bool) -> Vec<u8> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    default_send_all(wheel, PAGE, SUB, &mut table);
    image_of(&table)
}

/// A table whose image is `groups` groups of 32 `REPORT` commands each,
/// about 550 bytes a group, and `rules` rules of four triggers of sixteen
/// keys each, about 350 bytes a rule.
fn large_image(groups: usize, rules: usize) -> Vec<u8> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    let mut body = [0_u8; neoteletext_rules::command::REPORT_BYTES];
    let length = neoteletext_rules::command::encode(&Command::Report { value: None }, PAGE, SUB, &mut body).unwrap();
    let commands = [Framed { kind: TYPE_REPORT, body: &body[..length], bindings: &[] }; 32];
    for _ in 0..groups {
        table.push_group(&commands).unwrap();
    }
    let keys = KeyMatch::of(&[Key::of(KeyKind::Character, 'a' as u32); 16]);
    let mut rule = Rule::new(Trigger::Key { keys, repeat: false }, 0);
    for _ in 0..3 {
        rule.triggers.push(Trigger::KeyUp { keys }).unwrap();
    }
    for _ in 0..rules {
        table.rules.push(rule).unwrap();
    }
    image_of(&table)
}

/// The records an image travels in, read back by a receiver's reader and
/// given to an assembler: what each gave.
fn travel(image: &[u8], assembler: &mut Assembler) -> (usize, Vec<Vec<u8>>, usize) {
    let (page, subcode) = place();
    let mut reader = RecordReader::new();
    let (mut records, mut whole, mut incomplete) = (0, Vec::new(), 0);
    encode_image(page, subcode, 7, image, 3, &mut |number| u8::try_from(number).unwrap(), &mut |packet| {
        if let Read::Record(ChannelRecord::Rules(record)) = reader.push(&packet) {
            records += 1;
            assert_eq!((record.page, record.subcode, record.generation), (page, subcode, 7));
            match assembler.push(&record) {
                Assembled::Whole { image, generation, .. } => {
                    assert_eq!(generation, 7);
                    whole.push(image.to_vec());
                }
                Assembled::Incomplete => incomplete += 1,
                other => panic!("{other:?}"),
            }
        }
    })
    .unwrap();
    (records, whole, incomplete)
}

/// RULE-41 (REQ-6526): a table in one, two and four records reaches a
/// receiver whole and decodes to the table sent; until the last fragment
/// the image is incomplete.
#[test]
fn a_table_travels_in_one_two_and_four_records() {
    let mut assembler = Box::new(Assembler::new());
    for (image, expected) in [(default_image(true), 1), (large_image(20, 1), 2), (large_image(30, 30), 4)] {
        assert_eq!(records_for(image.len()), expected, "{} bytes", image.len());
        let (records, whole, incomplete) = travel(&image, &mut assembler);
        assert_eq!((records, incomplete), (expected, expected - 1));
        assert_eq!(whole, core::slice::from_ref(&image));
        let mut table = Box::new(Table::new(PAGE, SUB));
        neoteletext_rules::decode(&whole[0], PAGE, SUB, body_check, &mut table).unwrap();
        assert_eq!(image_of(&table), image);
    }
    assert_eq!(records_for(MAX_TABLE_FRAGMENT), 1);
    assert_eq!(records_for(MAX_TABLE_FRAGMENT + 1), 2);
    assert_eq!(records_for(MAX_TABLE_BYTES), 5);
}

/// RULE-41 (REQ-6526, REQ-6615): the record's bytes, field by field; the
/// record that says a page has no table is its twelve bytes; and what the
/// reader refuses.
#[test]
fn a_rules_record_is_its_bytes() {
    let image = default_image(false);
    let total = u16::try_from(image.len()).unwrap().to_be_bytes();
    let body: Vec<u8> = [&[0x01, 0x01, 0x00, 0x01, 0, 7, 1, 0, total[0], total[1], 0, 0][..], &image].concat();
    let (page, subcode) = place();
    let fragment = Fragment { total: u16::from_be_bytes(total), offset: 0, data: &image };
    assert_eq!(read(&body), Ok(RulesRecord { page, subcode, generation: 7, fragment: Some(fragment) }));
    let none = [0x08, 0xFF, 0x3F, 0x7E, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
    let record = read(&none).unwrap();
    assert_eq!(
        (record.page.mpp(), record.subcode.value(), record.generation, record.fragment),
        (0x8FF, 0x3F7E, 65_535, None)
    );
    let mut packets = Vec::new();
    assert_eq!(encode(&record, 3, 7, &mut |packet| packets.push(packet)), Ok(1));

    let with = |at: usize, byte: u8| {
        let mut bytes = body.clone();
        bytes[at] = byte;
        read(&bytes).map(|record| record.subcode.value())
    };
    assert_eq!(with(0, 0x09), Err(RecordFault::BadValue("page")));
    assert_eq!(with(2, 0x40), Err(RecordFault::BadValue("sub-code")));
    assert_eq!(with(3, 0x7F), Ok(0x007F));
    let mut any = body.clone();
    any[2..4].copy_from_slice(&[0x3F, 0x7F]);
    assert_eq!(read(&any), Err(RecordFault::BadValue("sub-code")), "no table is for whichever sub-page is shown");
    let mut generation = body.clone();
    generation[4..6].copy_from_slice(&[0, 0]);
    assert_eq!(read(&generation), Err(RecordFault::BadValue("rules generation")));
    assert_eq!(with(6, 3), Err(RecordFault::BadValue("rules reserved bits")));
    assert_eq!(with(7, 1), Err(RecordFault::BadValue("rules reserved bits")));
    // A total below the image's header, an offset past the total, no
    // data, and a table's fields without a table.
    let mut short = body.clone();
    short[8..10].copy_from_slice(&[0, 11]);
    assert_eq!(read(&short), Err(RecordFault::BadValue("rules total")));
    assert_eq!(with(11, 1), Err(RecordFault::BadValue("rules offset")));
    assert_eq!(read(&body[..RULES_RECORD_FIXED]), Err(RecordFault::Length));
    assert_eq!(read(&body[..RULES_RECORD_FIXED - 1]), Err(RecordFault::Length));
    assert_eq!(with(6, 0), Err(RecordFault::BadValue("rules total without a table")));
    let mut trailing = none.to_vec();
    trailing.push(0);
    assert_eq!(read(&trailing), Err(RecordFault::Length));
}

/// RULE-41, RULE-48 (REQ-6526): a fragment out of place discards the
/// incomplete image, and the receiver is told the generation to report;
/// a fragment with offset 0 begins again; a record with no table removes
/// the page's table at once.
#[test]
fn a_fragment_out_of_place_discards_the_image() {
    let image = large_image(30, 30);
    let (page, subcode) = place();
    let total = u16::try_from(image.len()).unwrap();
    let chunks: Vec<&[u8]> = image.chunks(MAX_TABLE_FRAGMENT).collect();
    let fragment = |generation: u16, number: usize, total: u16| RulesRecord {
        page,
        subcode,
        generation,
        fragment: Some(Fragment {
            total,
            offset: u16::try_from(number * MAX_TABLE_FRAGMENT).unwrap(),
            data: chunks[number],
        }),
    };
    let out_of_place = |generation| Assembled::OutOfPlace { page, subcode, generation };
    let mut assembler = Box::new(Assembler::new());
    // A later fragment with nothing begun.
    assert_eq!(assembler.push(&fragment(7, 1, total)), out_of_place(7));
    // One skipped.
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    assert_eq!(assembler.push(&fragment(7, 2, total)), out_of_place(7));
    assert_eq!(assembler.push(&fragment(7, 1, total)), out_of_place(7), "the image was discarded");
    // Another generation, another total, another sub-code.
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    assert_eq!(assembler.push(&fragment(8, 1, total)), out_of_place(8));
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    assert_eq!(assembler.push(&fragment(7, 1, total - 1)), out_of_place(7));
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    let other = RulesRecord { subcode: SubCode::new(2), ..fragment(7, 1, total) };
    assert_eq!(assembler.push(&other), Assembled::OutOfPlace { page, subcode: SubCode::new(2), generation: 7 });
    // Offset 0 begins again, and the whole image arrives.
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    assert_eq!(assembler.push(&fragment(9, 0, total)), Assembled::Incomplete);
    for number in 1..chunks.len() - 1 {
        assert_eq!(assembler.push(&fragment(9, number, total)), Assembled::Incomplete);
    }
    let Assembled::Whole { image: whole, generation: 9, .. } = assembler.push(&fragment(9, chunks.len() - 1, total))
    else {
        panic!()
    };
    assert_eq!(whole, image);
    // No table: at once, and an image in flight is dropped.
    assert_eq!(assembler.push(&fragment(7, 0, total)), Assembled::Incomplete);
    let none = RulesRecord { page, subcode, generation: 10, fragment: None };
    assert_eq!(assembler.push(&none), Assembled::Removed { page, subcode, generation: 10 });
    assert_eq!(assembler.push(&fragment(7, 1, total)), out_of_place(7));
}

/// RULE-59, RULE-70 (REQ-6526): the writer sends nothing the rule table's
/// decoder refuses, and nothing for another page than the image's
/// commands name.
#[test]
fn a_faulty_image_is_not_written() {
    let (page, subcode) = place();
    let mut image = default_image(false);
    let written = |image: &[u8], page, subcode| encode_image(page, subcode, 7, image, 3, &mut |_| 0, &mut |_| {});
    assert_eq!(written(&image, page, subcode), Ok(crate::records::fragments(RULES_RECORD_FIXED + image.len())));
    assert_eq!(written(&image, page, SubCode::new(2)), Err(RecordFault::BadValue("rules table")));
    // A reserved bit of the first rule's flags.
    image[HEADER_BYTES] |= 0x80;
    assert_eq!(written(&image, page, subcode), Err(RecordFault::BadValue("rules table")));
}

/// RULE-47 (REQ-6526, REQ-6615): a `RULES_QUERY` record is its eight
/// bytes, the page, the sub-code and the identifier, in one packet.
#[test]
fn a_query_record_is_its_eight_bytes() {
    let (page, subcode) = place();
    let record = RulesQueryRecord { page, subcode, identifier: 0x0102_0304 };
    assert_eq!(read_query(&[0x01, 0x01, 0x00, 0x01, 1, 2, 3, 4]), Ok(record));
    let mut reader = RecordReader::new();
    let mut seen = None;
    let packets = encode_query(record, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::RulesQuery(read)) = reader.push(&packet) {
            seen = Some(read);
        }
    })
    .unwrap();
    assert_eq!((packets, seen), (1, Some(record)));
    assert_eq!(QUERY_BYTES, 8);
    // The first form's six bytes, and nine.
    assert_eq!(read_query(&[0x01, 0x01, 1, 2, 3, 4]), Err(RecordFault::Length));
    assert_eq!(read_query(&[0x01, 0x01, 0, 1, 1, 2, 3, 4, 5]), Err(RecordFault::Length));
    assert!(read_query(&[0x00, 0xFF, 0, 1, 1, 2, 3, 4]).is_err());
    assert!(read_query(&[0x01, 0x01, 0x3F, 0x7F, 1, 2, 3, 4]).is_err());
}
