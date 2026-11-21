//! The PAINT record, type 35, and the line's constants (DEC-7031 items 1
//! and 7; REQ-7024, REQ-7025).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use std::format;
use std::vec::Vec;

use super::*;
use crate::consts::{LINE_BITS_PER_BYTE, LINE_PACE_TICKS, PUSH_SUBPROTOCOL, line_bytes};
use crate::records::{ChannelRecord, Read, RecordReader};
use crate::wire_vectors::{bytes_of, vectors};

/// The page of these tests, and its sub-code.
const PAGE: u16 = 0x101;
const SUB: u16 = 0x0001;
/// The language bytes (the SHEET record's kinds).
const REGIS: u8 = 1;
const NAPLPS: u8 = 2;

/// A PAINT body of these fields, in the record's order.
fn body(page: u16, sub: u16, language: u8, reserved: u8, speed: u32, rate: u16) -> [u8; BYTES] {
    let mut bytes = [0_u8; BYTES];
    bytes[0..2].copy_from_slice(&page.to_be_bytes());
    bytes[2..4].copy_from_slice(&sub.to_be_bytes());
    bytes[4] = language;
    bytes[5] = reserved;
    bytes[6..8].copy_from_slice(&0x0102_u16.to_be_bytes());
    bytes[8..12].copy_from_slice(&speed.to_be_bytes());
    bytes[12..14].copy_from_slice(&rate.to_be_bytes());
    bytes
}

fn record(language: SheetKind, line_speed: Option<u32>, default_rate: Option<u16>) -> PaintRecord {
    PaintRecord {
        page: PageAddress::from_mpp(PAGE).unwrap(),
        subcode: SubCode::new(SUB),
        language,
        generation: 0x0102,
        line_speed,
        default_rate,
    }
}

/// What a receiver's reader gives for the packets `record` is written as.
fn through_a_reader(record: PaintRecord) -> (usize, Vec<PaintRecord>) {
    let mut reader = RecordReader::new();
    let mut seen = Vec::new();
    let packets = encode(record, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::Paint(read)) = reader.push(&packet) {
            seen.push(read);
        }
    })
    .unwrap();
    (packets, seen)
}

/// DEC-7031 item 1: a PAINT record of each language, at the ends of each
/// range, reaches a receiver's reader whole as the record written, in one
/// packet; and its fourteen bytes read as it.
#[test]
fn a_paint_record_round_trips_through_a_receivers_reader() {
    assert_eq!(BYTES, 14);
    for sent in [
        record(SheetKind::Regis, Some(1_200), Some(20)),
        record(SheetKind::Regis, None, Some(1)),
        record(SheetKind::Regis, Some(MIN_LINE_SPEED), Some(MAX_DEFAULT_PAINT_RATE)),
        record(SheetKind::Regis, Some(MAX_LINE_SPEED), None),
        record(SheetKind::Naplps, Some(MIN_LINE_SPEED), None),
        record(SheetKind::Naplps, Some(MAX_LINE_SPEED), None),
    ] {
        assert_eq!(through_a_reader(sent), (1, std::vec![sent]), "{sent:?}");
    }
    assert_eq!(read(&body(PAGE, SUB, REGIS, 0, 1_200, 20)), Ok(record(SheetKind::Regis, Some(1_200), Some(20))));
    assert_eq!(read(&body(PAGE, SUB, NAPLPS, 0, 300, 0)), Ok(record(SheetKind::Naplps, Some(300), None)));
}

/// DEC-7031 item 1: every other shape of the body is refused whole, each
/// with its own fault.
#[test]
fn every_other_paint_record_is_refused_with_its_fault() {
    let good = body(PAGE, SUB, REGIS, 0, 1_200, 20);
    assert_eq!(read(&good[..13]), Err(RecordFault::Length));
    assert_eq!(read(&[good.as_slice(), &[0]].concat()), Err(RecordFault::Length));
    let refused = |body: [u8; BYTES]| read(&body).err();
    let bad = |what| Some(RecordFault::BadValue(what));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 1, 1_200, 20)), bad("paint reserved byte"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0x80, 1_200, 20)), bad("paint reserved byte"));
    assert_eq!(refused(body(PAGE, SUB, 0, 0, 1_200, 0)), bad("paint language"));
    assert_eq!(refused(body(PAGE, SUB, 3, 0, 1_200, 0)), bad("paint language"));
    assert_eq!(refused(body(PAGE, SUB, NAPLPS, 0, 299, 0)), bad("paint line speed"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0, 115_201, 20)), bad("paint line speed"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0, u32::MAX, 20)), bad("paint line speed"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0, 0, 32_768)), bad("paint default rate"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0, 0, u16::MAX)), bad("paint default rate"));
    assert_eq!(refused(body(PAGE, SUB, NAPLPS, 0, 1_200, 1)), bad("paint rate for NAPLPS"));
    assert_eq!(refused(body(PAGE, SUB, NAPLPS, 0, 0, 20)), bad("paint rate for NAPLPS"));
    assert_eq!(refused(body(PAGE, SUB, REGIS, 0, 0, 0)), bad("paint with no value"));
    assert_eq!(refused(body(PAGE, SUB, NAPLPS, 0, 0, 0)), bad("paint with no value"));
    assert_eq!(refused(body(0x900, SUB, REGIS, 0, 1_200, 20)), bad("page"));
    assert_eq!(refused(body(0x0FF, SUB, REGIS, 0, 1_200, 20)), bad("page"));
    assert_eq!(refused(body(PAGE, 0x3F7F, REGIS, 0, 1_200, 20)), bad("sub-code"));
    assert_eq!(refused(body(PAGE, 0x3F7E, REGIS, 0, 1_200, 20)), None, "the last sub-code is taken");
}

/// DEC-7031 item 1: the writer refuses what its reader would not give
/// back, a value of `Some(0)` among them, and writes nothing.
#[test]
fn the_writer_refuses_a_value_of_some_zero() {
    for refused in [
        record(SheetKind::Regis, Some(0), Some(20)),
        record(SheetKind::Regis, Some(1_200), Some(0)),
        record(SheetKind::Regis, Some(0), Some(0)),
        record(SheetKind::Regis, None, None),
        record(SheetKind::Naplps, Some(1_200), Some(20)),
        record(SheetKind::Regis, Some(299), None),
    ] {
        let mut written = 0;
        assert!(encode(refused, 3, 7, &mut |_| written += 1).is_err(), "{refused:?}");
        assert_eq!(written, 0, "{refused:?}");
    }
    let mut written = 0;
    assert_eq!(
        encode(record(SheetKind::Regis, Some(0), Some(20)), 3, 7, &mut |_| written += 1),
        Err(RecordFault::BadValue("does not read back"))
    );
    assert_eq!(written, 0);
}

/// DEC-7031 items 1 and 7: the wire vector `paint` is the packet the
/// writer makes of page 101/0001's ReGIS stream of generation 1 at 1,200
/// bits a second and rate 20, and a receiver's reader gives that record.
#[test]
fn the_paint_wire_vector_is_this_record() {
    let vector = vectors().find(|vector| vector.kind == "records" && vector.name == "paint").unwrap();
    let expected = PaintRecord {
        page: PageAddress::from_mpp(0x101).unwrap(),
        subcode: SubCode::new(0x0001),
        language: SheetKind::Regis,
        generation: 1,
        line_speed: Some(1_200),
        default_rate: Some(20),
    };
    let (bytes, length) = bytes_of(vector.value).unwrap();
    assert_eq!(length, neoteletext_core::consts::PACKET_BYTES, "one packet");
    let packet = Packet { bytes: bytes[..length].try_into().unwrap() };
    let mut reader = RecordReader::new();
    let Read::Record(ChannelRecord::Paint(read)) = reader.push(&packet) else { panic!("{}", vector.value) };
    assert_eq!(read, expected);
    // The writer, as the engine's `cargo xtask wire-vectors` calls it.
    let mut written = Vec::new();
    encode(expected, 0, 0, &mut |packet| written.push(packet.bytes)).unwrap();
    assert_eq!(written, [packet.bytes]);
}

/// REQ-7024, DEC-7031 item 1: a line delivers a byte for each ten bits,
/// so its bytes by a tick are its speed times the ticks over 600: at
/// 1,200 bits a second, 2 bytes a tick and 120 after 60 ticks; at 300, a
/// byte every second tick; and at the greatest tick no wrap.
#[test]
fn the_lines_bytes_follow_its_speed() {
    assert_eq!((LINE_BITS_PER_BYTE, LINE_PACE_TICKS), (10, 600));
    assert_eq!((MIN_LINE_SPEED, MAX_LINE_SPEED, MAX_DEFAULT_PAINT_RATE), (300, 115_200, 32_767));
    for tick in 0..=60 {
        assert_eq!(line_bytes(tick, 1_200), 2 * tick, "tick {tick}");
    }
    assert_eq!(line_bytes(60, 1_200), 120);
    let at_300: Vec<u64> = (0..=6).map(|tick| line_bytes(tick, 300)).collect();
    assert_eq!(at_300, [0, 0, 1, 1, 2, 2, 3]);
    assert_eq!(line_bytes(60, MAX_LINE_SPEED), 11_520, "a second of the fastest line");
    assert_eq!(line_bytes(u64::MAX, MAX_LINE_SPEED), u64::MAX / 600, "it stops, never wraps");
    assert!(line_bytes(u64::MAX, MAX_LINE_SPEED) >= line_bytes(u64::MAX / 2, MAX_LINE_SPEED));
    assert_eq!(line_bytes(u64::MAX, 0), 0);
}

/// DEC-7031 item 7: painting's trigger and record came with a push
/// protocol after 10, whose receivers refuse a table with trigger 22, and
/// the subprotocol's name says the input protocol's version.
#[test]
fn painting_needs_a_push_protocol_after_10() {
    let version = neoteletext_input::VERSION;
    assert!(version > 10, "version {version}");
    assert_eq!(PUSH_SUBPROTOCOL, format!("neottx.{version}"));
}
