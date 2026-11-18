//! The SHEET record (REQ-6529, ANIM-17).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};

fn record(kind: SheetKind, offset: u32, data: &[u8]) -> SheetRecord<'_> {
    SheetRecord { page: PageAddress::from_mpp(0x123).unwrap(), kind, generation: 7, offset, data }
}

/// ANIM-17 (REQ-6529): a SHEET record of any length up to the frame's
/// reaches a receiver's reader whole, for either renderer.
#[test]
fn a_sheet_record_reaches_a_receiver() {
    let long = [b'V'; MAX_DATA];
    for (kind, data) in
        [(SheetKind::Regis, b"".as_slice()), (SheetKind::Naplps, b"\x1b\x22\x46"), (SheetKind::Regis, &long)]
    {
        let sent = record(kind, 0x0102, data);
        let mut reader = RecordReader::new();
        let mut seen = false;
        encode(sent, 3, 7, &mut |packet| {
            if let Read::Record(ChannelRecord::Sheet(read)) = reader.push(&packet) {
                assert_eq!(read, sent);
                seen = true;
            }
        })
        .unwrap();
        assert!(seen);
    }
    assert_eq!(MAX_DATA, 8174);
    assert_eq!(MAX_SHEET_BYTES, 65_536);
    let body = [0x01, 0x23, 2, 0, 0x00, 0x07, 0x00, 0x00, 0x01, 0x02, b'V', b'1'];
    assert_eq!(read(&body), Ok(record(SheetKind::Naplps, 0x0102, b"V1")));
}

/// ANIM-17, ANIM-16 (REQ-6529): what the reader refuses: a kind not
/// listed, the reserved byte, and a stream past `MAX_SHEET_BYTES`.
#[test]
fn anything_else_is_refused() {
    let good = [0x01, 0x23, 1, 0, 0x00, 0x07, 0x00, 0x00, 0xFF, 0xFE, b'V', b'1'];
    assert!(read(&good).is_ok(), "ends at byte 65,536");
    for (at, value) in [(0, 0x00), (2, 0), (2, 3), (3, 1), (9, 0xFF), (7, 0x01)] {
        let mut body = good;
        body[at] = value;
        assert!(read(&body).is_err(), "byte {at} as {value}");
    }
    assert_eq!(read(&good[..9]), Err(RecordFault::Length));
    let over = [0_u8; MAX_DATA + 1];
    assert!(encode(record(SheetKind::Regis, 0, &over), 3, 7, &mut |_| {}).is_err());
}
