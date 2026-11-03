#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `expect` naming what it expected, indexing into data it built to a known size, arithmetic on small values it chose, where an overflow would also fail it and `panic!` on outcomes it rules out"
)]
//! The SUBPAGES record at its bounds, through packets and a reader.

use super::{MAX_TITLE_CHARACTERS, Sending, Step, SubPageEntry, encode, read};
use crate::records::{ChannelRecord, Read, RecordFault, RecordReader, TYPE_SUBPAGES, write_packets};
use neoteletext_core::limits::{LAST_SUBPAGE_CODE, MAX_SUBPAGES, SUBCODE_ANY, SUBCODE_MASK};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use std::string::{String, ToString};
use std::vec::Vec;

const SERVICE: u8 = 2;
const VERSION: u16 = 9;
const PAGE: u16 = 0x100;

fn page() -> PageAddress {
    PageAddress::from_mpp(PAGE).unwrap()
}

/// The first `count` sub-codes in ascending order.
fn codes(count: usize) -> Vec<u16> {
    (0..=LAST_SUBPAGE_CODE).filter(|code| code & !SUBCODE_MASK == 0).take(count).collect()
}

fn entries(codes: &[u16]) -> Vec<SubPageEntry<'static>> {
    codes.iter().map(|&code| SubPageEntry { subcode: SubCode::new(code), title: "" }).collect()
}

fn write(entries: &[SubPageEntry<'_>]) -> Result<Vec<Packet>, RecordFault> {
    let mut packets = Vec::new();
    encode(page(), VERSION, entries, SERVICE, 0, &mut |packet| packets.push(packet))?;
    Ok(packets)
}

/// A record as read: its part, its parts, its count and its entries.
type ReadRecord = (u16, u16, u16, Vec<(u16, String)>);

/// Each record a reader takes from `packets`.
fn read_back(packets: &[Packet]) -> Vec<ReadRecord> {
    let mut reader = RecordReader::new();
    let mut found = Vec::new();
    for packet in packets {
        match reader.push(packet) {
            Read::Record(ChannelRecord::SubPages(record)) => {
                assert_eq!((record.page, record.version), (page(), VERSION));
                let list = record.entries().map(|entry| (entry.subcode.value(), entry.title.to_string())).collect();
                found.push((record.part, record.parts, record.count, list));
            }
            Read::Invalid(count) => panic!("{count} packets refused"),
            _ => {}
        }
    }
    found
}

/// A SUBPAGES body with the fixed fields of one record of one sub-page list.
fn body(count: u16, codes: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [PAGE, VERSION, 0, 1, count] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    for code in codes {
        bytes.extend_from_slice(&code.to_be_bytes());
        bytes.push(0);
    }
    bytes
}

/// REQ-1503: a list of one sub-page and one of 8,190, split into records,
/// reads back whole, each part numbered and every sub-code in its order.
#[test]
fn a_list_of_one_and_of_8190_sub_pages_reads_back_whole() {
    for count in [1, MAX_SUBPAGES] {
        let codes = codes(count);
        let found = read_back(&write(&entries(&codes)).unwrap());
        let parts = found.len();
        assert_eq!(parts > 1, count > 1, "a long list is split, a short one is not");
        let mut listed = Vec::new();
        for (index, (part, of, total, list)) in found.into_iter().enumerate() {
            assert_eq!((usize::from(part), usize::from(of), usize::from(total)), (index, parts, count));
            listed.extend(list.into_iter().map(|(code, _)| code));
        }
        assert_eq!(listed, codes);
    }
}

/// REQ-1503: every sub-code is a peer, so a list may hold 0000 beside the
/// others, and it stays first.
#[test]
fn a_list_may_hold_0000_beside_other_sub_codes() {
    let found = read_back(&write(&entries(&[0x0000, 0x0001, 0x0002])).unwrap());
    let list: Vec<u16> = found[0].3.iter().map(|(code, _)| *code).collect();
    assert_eq!(list, [0x0000, 0x0001, 0x0002]);
}

/// REQ-1503: a title of 40 characters travels, and one of 41 does not.
#[test]
fn a_title_is_at_most_a_row() {
    let longest: String = core::iter::repeat_n('x', MAX_TITLE_CHARACTERS).collect();
    let entry = SubPageEntry { subcode: SubCode::new(1), title: &longest };
    assert_eq!(read_back(&write(&[entry]).unwrap())[0].3, [(1, longest.clone())]);
    let over = longest + "x";
    let entry = SubPageEntry { subcode: SubCode::new(1), title: &over };
    assert!(write(&[entry]).is_err());
}

/// REQ-1503: a list out of ascending order or with a sub-code repeated is
/// not written, and a reader refuses one.
#[test]
fn a_list_out_of_order_or_repeated_is_refused() {
    for bad in [[0x0002, 0x0001], [0x0001, 0x0001]] {
        assert_eq!(write(&entries(&bad)), Err(RecordFault::BadValue("sub-codes not ascending")), "{bad:04X?}");
        assert_eq!(read(&body(2, &bad)).err(), Some(RecordFault::BadValue("sub-codes not ascending")), "{bad:04X?}");
    }
    assert!(read(&body(2, &[0x0001, 0x0002])).is_ok());
}

/// REQ-1503: each bound of the record, and one past it.
#[test]
fn a_record_past_a_bound_is_refused() {
    let most = u16::try_from(MAX_SUBPAGES).unwrap();
    assert!(read(&body(most, &[0x0001])).is_ok());
    assert!(read(&body(most + 1, &[0x0001])).is_err(), "more sub-pages than a page may have");
    assert!(read(&body(0, &[0x0001])).is_err(), "no sub-pages");
    assert!(read(&body(1, &[])).is_err(), "no entries");
    assert!(read(&body(1, &[0x0001, 0x0002])).is_err(), "more entries than the count");
    assert!(read(&body(1, &[LAST_SUBPAGE_CODE])).is_ok());
    assert!(read(&body(1, &[SUBCODE_ANY])).is_err(), "3F7F names no sub-page");
    assert!(write(&[]).is_err(), "an empty list");
    assert!(write(&entries(&[0x0001; MAX_SUBPAGES + 1])).is_err(), "more sub-pages than a page may have");
}

/// REQ-1503, REQ-1462: a reader discards a record that breaks its layout and
/// counts its packets as invalid.
#[test]
fn a_malformed_record_is_discarded_and_counted() {
    let mut packets = Vec::new();
    write_packets(TYPE_SUBPAGES, &body(2, &[0x0002, 0x0001]), SERVICE, 0, &mut |packet| packets.push(packet)).unwrap();
    let mut reader = RecordReader::new();
    let invalid: usize = packets
        .iter()
        .map(|packet| match reader.push(packet) {
            Read::Invalid(count) => count,
            Read::Record(_) => panic!("a malformed record was taken"),
            Read::Nothing => 0,
        })
        .sum();
    assert_eq!(invalid, packets.len());
}

/// A SUBPAGES body of any part of any sending.
fn part_body(version: u16, part: u16, parts: u16, count: u16, codes: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [PAGE, version, part, parts, count] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    for code in codes {
        bytes.extend_from_slice(&code.to_be_bytes());
        bytes.push(0);
    }
    bytes
}

/// What the one reassembly makes of each body in turn, each taking one
/// packet more than the one before so that the counts tell them apart.
fn steps(bodies: &[Vec<u8>]) -> Vec<Step> {
    let mut sending = Sending::new();
    bodies
        .iter()
        .enumerate()
        .map(|(index, body)| sending.note(&read(body).expect("each part is valid alone"), index + 1))
        .collect()
}

/// REQ-1503, DOC-1959: a list of 8,190 sub-pages written by `encode` is put
/// together by the one reassembly, partial until its last part and then
/// complete, with every sub-code in order.
#[test]
fn the_reassembly_takes_a_whole_sending() {
    let codes = codes(MAX_SUBPAGES);
    let packets = write(&entries(&codes)).unwrap();
    let mut reader = RecordReader::new();
    let mut sending = Sending::new();
    let mut results = Vec::new();
    let mut listed = Vec::new();
    for packet in &packets {
        let (read, packets) = reader.push_counted(packet);
        if let Read::Record(ChannelRecord::SubPages(record)) = read {
            listed.extend(record.entries().map(|entry| entry.subcode.value()));
            results.push(sending.note(&record, packets));
        }
    }
    let (last, earlier) = results.split_last().unwrap();
    assert_eq!(*last, Step::Complete);
    assert!(earlier.iter().all(|step| *step == Step::Partial), "{earlier:?}");
    assert_eq!(listed, codes);
}

/// REQ-1503, REQ-1462: a sending that breaks a total over its parts is
/// refused, every packet of it counted: a `count` that differs from part
/// 0's, a sub-code repeated or out of order across parts, more entries than
/// the count, and a last part leaving fewer.
#[test]
fn a_sending_that_breaks_its_totals_is_refused() {
    for (what, first, second) in [
        ("count differs", part_body(1, 0, 2, 4, &[1, 2]), part_body(1, 1, 2, 5, &[3, 4])),
        ("repeated across parts", part_body(1, 0, 2, 4, &[1, 2]), part_body(1, 1, 2, 4, &[2, 3])),
        ("out of order across parts", part_body(1, 0, 2, 4, &[5, 6]), part_body(1, 1, 2, 4, &[3, 4])),
        ("more than the count", part_body(1, 0, 2, 3, &[1, 2]), part_body(1, 1, 2, 3, &[3, 4])),
        ("fewer than the count", part_body(1, 0, 2, 5, &[1, 2]), part_body(1, 1, 2, 5, &[3, 4])),
    ] {
        assert_eq!(steps(&[first, second]), [Step::Partial, Step::Refused { packets: 3 }], "{what}");
    }
}

/// DOC-1959: a record of another version begins a new sending, so the
/// partial one is discarded and a later part of it refused alone; a part
/// out of turn breaks its sending; and a partial sending is never complete.
#[test]
fn a_new_version_begins_again() {
    let old_first = part_body(1, 0, 2, 4, &[1, 2]);
    let new_first = part_body(2, 0, 2, 4, &[1, 3]);
    let old_second = part_body(1, 1, 2, 4, &[3, 4]);
    let new_second = part_body(2, 1, 2, 4, &[5, 6]);
    assert_eq!(
        steps(&[old_first.clone(), new_first.clone(), old_second, new_second.clone()]),
        [Step::Partial, Step::Partial, Step::Refused { packets: 3 }, Step::Refused { packets: 4 }],
        "the old part breaks the new sending in progress, which ends it"
    );
    assert_eq!(steps(&[old_first.clone(), new_first, new_second]), [Step::Partial, Step::Partial, Step::Complete]);
    assert_eq!(steps(&[part_body(1, 1, 2, 4, &[3, 4])]), [Step::Refused { packets: 1 }], "no part 0");
    assert_eq!(steps(&[old_first]), [Step::Partial], "a partial sending is not complete");
}

/// REQ-1503: a title of 40 characters of four-byte UTF-8, 160 bytes,
/// travels; one more character is refused by `encode` and by `read`; a
/// title with a control character is refused by both.
#[test]
fn a_titles_bytes_and_characters_keep_their_bounds() {
    let longest: String = core::iter::repeat_n('\u{1F600}', MAX_TITLE_CHARACTERS).collect();
    assert_eq!(longest.len(), super::MAX_TITLE_BYTES);
    let over = [longest.as_str(), "a"].concat();
    assert_eq!(
        read_back(&write(&[SubPageEntry { subcode: SubCode::new(1), title: &longest }]).unwrap())[0].3,
        [(1, longest.clone())]
    );
    assert!(write(&[SubPageEntry { subcode: SubCode::new(1), title: &over }]).is_err(), "161 bytes and 41 characters");
    assert!(write(&[SubPageEntry { subcode: SubCode::new(1), title: "line\nbreak" }]).is_err(), "a control character");
    for title in [over.as_bytes(), b"line\nbreak".as_slice()] {
        let mut raw = part_body(1, 0, 1, 1, &[]);
        raw.extend_from_slice(&1_u16.to_be_bytes());
        raw.push(u8::try_from(title.len()).unwrap());
        raw.extend_from_slice(title);
        assert!(read(&raw).is_err(), "read refuses {title:?}");
    }
}

/// REQ-1503: `read` refuses a part equal to its parts, and a sub-code with a
/// bit outside the thirteen a sub-code has.
#[test]
fn a_part_or_sub_code_past_its_field_is_refused() {
    assert!(read(&part_body(1, 0, 1, 1, &[1])).is_ok());
    assert!(read(&part_body(1, 1, 1, 1, &[1])).is_err(), "part equal to parts");
    // The lowest bit no sub-code has: below 0x3F7E, so only the mask
    // refuses it.
    let bit = 1_u16 << (!SUBCODE_MASK).trailing_zeros();
    assert!(bit < LAST_SUBPAGE_CODE);
    assert!(read(&part_body(1, 0, 1, 1, &[bit])).is_err(), "sub-code {bit:#06x}");
    // `encode` takes a `SubCode`, which holds the thirteen bits alone, so
    // only a record from outside can carry such a sub-code.
    assert_eq!(SubCode::new(bit).value() & bit, 0);
}

/// REQ-1503, DOC-1959 (splitting): 8,190 entries with the longest titles
/// are split with at least six entries in every record but the last, and
/// none cut: every entry reads back whole.
#[test]
fn a_split_puts_six_entries_in_a_record_and_cuts_none() {
    let title: String = core::iter::repeat_n('\u{1F600}', MAX_TITLE_CHARACTERS).collect();
    let codes = codes(MAX_SUBPAGES);
    let list: Vec<SubPageEntry<'_>> =
        codes.iter().map(|&code| SubPageEntry { subcode: SubCode::new(code), title: &title }).collect();
    let found = read_back(&write(&list).unwrap());
    let (_, earlier) = found.split_last().unwrap();
    assert!(earlier.iter().all(|record| record.3.len() >= 6), "six or more in each record");
    let all: Vec<(u16, String)> = found.into_iter().flat_map(|record| record.3).collect();
    assert_eq!(all.len(), MAX_SUBPAGES);
    assert!(all.iter().zip(&codes).all(|(entry, &code)| entry.0 == code && entry.1 == title));
}
