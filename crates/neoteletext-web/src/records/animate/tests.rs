//! The ANIMATE record (REQ-6529, ANIM-6).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use neoteletext_rules::animate::OperationKind;
use neoteletext_rules::table::Target;
use std::vec::Vec;

fn page() -> PageAddress {
    PageAddress::from_mpp(0x150).unwrap()
}

fn on(target: Target, kind: OperationKind) -> Operation {
    Operation { target, kind }
}

/// ANIM-6 (REQ-6529): the body's layout, byte for byte, with every
/// operation; it reaches a receiver's reader with its operations in
/// order.
#[test]
fn an_animate_record_is_its_bytes() {
    let object = Target::object(10);
    let operations = [
        on(Target::control(3), OperationKind::Show),
        on(object, OperationKind::Hide),
        on(object, OperationKind::Look(1)),
        on(object, OperationKind::Move { relative: true, x: -8, y: 0 }),
        on(object, OperationKind::Order { layer: 2, order: -1 }),
        on(object, OperationKind::Frame(3)),
        on(object, OperationKind::Cycle { first: 0, last: 1, period: 30, once: false }),
        on(object, OperationKind::StopCycle),
        on(object, OperationKind::Reset),
    ];
    let mut reader = RecordReader::new();
    let mut seen = Vec::new();
    encode(page(), Some(SubCode::new(0x0002)), &operations, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::Animate(record)) = reader.push(&packet) {
            assert_eq!((record.page, record.subcode, record.count), (page(), Some(SubCode::new(0x0002)), 9));
            seen = record.operations().collect();
        }
    })
    .unwrap();
    assert_eq!(seen, operations);
    #[rustfmt::skip]
    let body: &[u8] = &[
        0x01, 0x50, 0x3F, 0x7F, 2, 0,
        // move object 10 by (−8, 0); reset control 3.
        15, 2, 0, 10, 1, 0xFF, 0xF8, 0, 0,
        20, 1, 0, 3,
    ];
    let record = read(body).unwrap();
    assert_eq!(record.subcode, None, "whichever sub-page is shown");
    assert_eq!(
        record.operations().collect::<Vec<_>>(),
        [on(object, OperationKind::Move { relative: true, x: -8, y: 0 }), on(Target::control(3), OperationKind::Reset)]
    );
}

/// ANIM-6, ANIM-16 (REQ-6529): what the reader refuses, and the writer
/// with it.
#[test]
fn anything_else_is_refused() {
    let show = on(Target::object(1), OperationKind::Show);
    assert_eq!(encode(page(), None, &[], 3, 7, &mut |_| {}), Err(RecordFault::BadValue("animate count")));
    assert!(encode(page(), None, &[show; MAX_ANIMATE_OPERATIONS], 3, 7, &mut |_| {}).is_ok());
    assert!(encode(page(), None, &[show; MAX_ANIMATE_OPERATIONS + 1], 3, 7, &mut |_| {}).is_err());
    let bad = on(Target::object(1), OperationKind::Frame(16));
    assert_eq!(encode(page(), None, &[bad], 3, 7, &mut |_| {}), Err(RecordFault::BadValue("animate operation")));
    let good = [0x01, 0x50, 0x00, 0x02, 1, 0, 5, 2, 0, 1];
    assert!(read(&good).is_ok());
    for (at, value, what) in [
        (0, 0x00, "a page that does not exist"),
        (2, 0x40, "a sub-code with a bit a sub-code has not"),
        (4, 0, "no operation"),
        (4, 2, "an operation short"),
        (4, 65, "more than 64"),
        (5, 1, "the reserved byte"),
        (6, 4, "a code that is no operation's"),
        (7, 0, "the page as a target"),
        (9, 0, "any object"),
    ] {
        let mut body = good;
        body[at] = value;
        assert!(read(&body).is_err(), "{what}");
    }
    assert_eq!(read(&[good.as_slice(), &[0]].concat()), Err(RecordFault::Length), "bytes after the last operation");
    assert_eq!(read(&good[..5]), Err(RecordFault::Length));
}
