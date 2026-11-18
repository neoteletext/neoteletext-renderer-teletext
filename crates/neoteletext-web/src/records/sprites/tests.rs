//! The SPRITES record (REQ-6529, ANIM-7).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, and indexing and arithmetic on data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use std::vec::Vec;

fn place() -> (PageAddress, SubCode) {
    (PageAddress::from_mpp(0x150).unwrap(), SubCode::new(0x0001))
}

/// A local active object 5, and an adaptive object 0x1FF of the Global
/// Public Object Page.
const FRAMES: [u8; 4] = [0x0A, 0x05, 0x1D, 0xFF];

/// ANIM-7 (REQ-6529): the body's layout, byte for byte, and the fields of
/// an object reference.
#[test]
fn a_sprites_record_is_its_bytes() {
    let walker = Sprite::new(10, true, true, 2, -1, (-39, 24), (40, 25), &FRAMES);
    let still = Sprite::new(65_535, false, false, 1, 127, (0, -24), (1, 1), &FRAMES[..2]);
    let (page, subcode) = place();
    let mut reader = RecordReader::new();
    let mut seen = 0;
    encode(page, subcode, &[walker, still], 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::Sprites(record)) = reader.push(&packet) {
            assert_eq!((record.page, record.subcode, record.count), (page, subcode, 2));
            assert_eq!(record.sprites().collect::<Vec<_>>(), [walker, still]);
            seen += 1;
        }
    })
    .unwrap();
    assert_eq!(seen, 1);
    let references: Vec<ObjectReference> = walker.frames().collect();
    let fields: Vec<(u8, u8, u16)> = references.iter().map(|r| (r.source(), r.kind(), r.number())).collect();
    assert_eq!(fields, [(1, 1, 5), (3, 2, 0x1FF)]);
    assert_eq!(references[0].bits(), 0x0A05);
    #[rustfmt::skip]
    let body: &[u8] = &[
        0x01, 0x50, 0x00, 0x01, 1, 0,
        // Sprite 10: shown and detectable, layer 2, order −1, column −39,
        // row 24, 40 by 25 cells, two frames.
        0, 10, 3, 2, 0xFF, 0xD9, 24, 40, 25, 2, 0x0A, 0x05, 0x1D, 0xFF,
    ];
    assert_eq!(read(body).unwrap().sprites().collect::<Vec<_>>(), [walker]);
    // No sprite takes every sprite away.
    assert_eq!(read(&[0x01, 0x50, 0x00, 0x01, 0, 0]).unwrap().sprites().count(), 0);
}

/// ANIM-7, ANIM-16 (REQ-6529): the reader refuses a repeated number, a
/// reserved bit, a source of 0, each field beyond its bound, and a 33rd
/// sprite.
#[test]
fn anything_else_is_refused() {
    let good = [0x01, 0x50, 0x00, 0x01, 1, 0, 0, 10, 3, 2, 0xFF, 0xD9, 24, 40, 25, 1, 0x0A, 0x05];
    assert!(read(&good).is_ok());
    for (at, value, what) in [
        (5, 1, "the reserved byte"),
        (7, 0, "number 0"),
        (8, 4, "a reserved flag"),
        (9, 0, "layer 0, which is the page"),
        (9, 4, "layer 4"),
        (11, 40, "column 40"),
        (11, 0xD8, "column −40"),
        (12, 25, "row 25"),
        (12, 0xE7, "row −25"),
        (13, 0, "no width"),
        (13, 41, "41 cells wide"),
        (14, 0, "no height"),
        (14, 26, "26 cells tall"),
        (15, 0, "no frame"),
        (15, 17, "17 frames"),
        (16, 0x2A, "a reserved bit of the reference"),
        (16, 0x02, "source 0"),
        (16, 0x08, "type 0"),
    ] {
        let mut body = good;
        body[at] = value;
        assert!(read(&body).is_err(), "{what}");
    }
    assert_eq!(read(&[good.as_slice(), &[0]].concat()), Err(RecordFault::Length), "bytes after the last sprite");
    assert_eq!(read(&good[..good.len() - 1]), Err(RecordFault::Length));
    let (page, subcode) = place();
    let sprite = |number| Sprite::new(number, true, false, 1, 0, (0, 0), (1, 1), &FRAMES[..2]);
    assert_eq!(
        encode(page, subcode, &[sprite(4), sprite(4)], 3, 7, &mut |_| {}),
        Err(RecordFault::BadValue("sprite number repeated"))
    );
    let all: Vec<Sprite<'_>> = (1..=33).map(sprite).collect();
    assert!(encode(page, subcode, &all[..MAX_SPRITES], 3, 7, &mut |_| {}).is_ok());
    assert!(encode(page, subcode, &all, 3, 7, &mut |_| {}).is_err(), "a 33rd sprite");
    let mut count = good;
    count[4] = 33;
    assert_eq!(read(&count), Err(RecordFault::BadValue("sprite count")));
    // Sixteen frames are the most.
    let sixteen = [0x0A, 0x05].repeat(MAX_OBJECT_FRAMES);
    let full = Sprite::new(1, true, false, 3, 0, (0, 0), (1, 1), &sixteen);
    assert!(encode(page, subcode, &[full], 3, 7, &mut |_| {}).is_ok());
    assert_eq!(MAX_BYTES, 6 + 32 * 42);
}
