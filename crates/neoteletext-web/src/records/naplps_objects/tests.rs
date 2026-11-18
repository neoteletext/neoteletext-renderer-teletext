//! The `NAPLPS_OBJECTS` record (REQ-6529, ANIM-11).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, and indexing and arithmetic on data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use std::borrow::ToOwned;
use std::string::String;
use std::vec;
use std::vec::Vec;

fn place() -> (PageAddress, SubCode, u16) {
    (PageAddress::from_mpp(0x160).unwrap(), SubCode::new(0x0001), 9)
}

fn macro_object(number: u16, frames: &[u8]) -> NaplpsObject<'_> {
    NaplpsObject {
        number,
        kind: ObjectKind::Macro,
        shown: true,
        detectable: true,
        layer: 2,
        order: -3,
        x: -10,
        y: 300,
        width: 640,
        height: 480,
        origin: (5, -6),
        cover: 0,
        looks: [0x21, 0, 0x7F],
        frames,
    }
}

fn region(number: u16) -> NaplpsObject<'static> {
    NaplpsObject {
        number,
        kind: ObjectKind::Region,
        shown: false,
        detectable: false,
        layer: 1,
        order: 0,
        x: 0,
        y: 0,
        width: 1,
        height: 1,
        origin: (0, 0),
        cover: 255,
        looks: [0, 0, 0],
        frames: &[],
    }
}

/// ANIM-11 (REQ-6529): the body's layout, byte for byte, for a macro
/// object and a region.
#[test]
fn a_naplps_objects_record_is_its_bytes() {
    let objects = [macro_object(301, &[0x20, 0x7F]), region(65_535)];
    let mut reader = RecordReader::new();
    let mut seen = 0;
    encode(place(), &objects, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::NaplpsObjects(record)) = reader.push(&packet) {
            assert_eq!((record.page, record.subcode, record.generation, record.count), (place().0, place().1, 9, 2));
            assert_eq!(record.objects().collect::<Vec<_>>(), objects);
            seen += 1;
        }
    })
    .unwrap();
    assert_eq!(seen, 1);
    #[rustfmt::skip]
    let body: &[u8] = &[
        0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 1, 0,
        // Object 301: a macro object, shown and detectable, layer 2,
        // order −3, at (−10, 300), 640 by 480, origin (5, −6), two
        // frames, no cover, looks 2/1, none and 7/15, reserved.
        0x01, 0x2D, 1, 3, 2, 0xFD, 0xFF, 0xF6, 0x01, 0x2C, 0x02, 0x80, 0x01, 0xE0, 0x00, 0x05, 0xFF, 0xFA,
        2, 0, 0x21, 0, 0x7F, 0,
        0x20, 0x7F,
    ];
    assert_eq!(read(body).unwrap().objects().collect::<Vec<_>>(), [objects[0]]);
    assert_eq!(read(&[0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 0, 0]).unwrap().objects().count(), 0);
}

/// ANIM-11, ANIM-16 (REQ-6529): what the reader refuses, and a 65th
/// object.
#[test]
fn anything_else_is_refused() {
    let (page, subcode, generation) = place();
    let refused = |object: NaplpsObject<'_>| encode((page, subcode, generation), &[object], 3, 7, &mut |_| {}).is_err();
    let good = macro_object(1, &[0x40]);
    assert!(!refused(good));
    assert!(!refused(region(1)));
    let sixteen = [0x40; MAX_OBJECT_FRAMES];
    let seventeen = [0x40; MAX_OBJECT_FRAMES + 1];
    assert!(!refused(macro_object(1, &sixteen)));
    for (bad, what) in [
        (NaplpsObject { number: 0, ..good }, "number 0"),
        (NaplpsObject { layer: 0, ..good }, "layer 0"),
        (NaplpsObject { layer: 4, ..good }, "layer 4"),
        (NaplpsObject { width: 0, ..good }, "no width"),
        (NaplpsObject { width: 641, ..good }, "wider than the picture"),
        (NaplpsObject { height: 0, ..good }, "no height"),
        (NaplpsObject { height: 481, ..good }, "taller than the picture"),
        (NaplpsObject { cover: 1, ..good }, "a macro object with a cover"),
        (NaplpsObject { looks: [0x1F, 0, 0], ..good }, "a look below 2/0"),
        (NaplpsObject { looks: [0, 0, 0x80], ..good }, "a look above 7/15"),
        (macro_object(1, &[]), "a macro object with no frame"),
        (macro_object(1, &seventeen), "17 frames"),
        (macro_object(1, &[0x1F]), "a frame that is no macro's code"),
        (NaplpsObject { frames: &[0x40], ..region(1) }, "a region with a frame"),
        (NaplpsObject { origin: (1, 0), ..region(1) }, "a region with an origin"),
    ] {
        assert!(refused(bad), "{what}");
    }
    assert_eq!(
        encode((page, subcode, generation), &[region(4), good, region(4)], 3, 7, &mut |_| {}),
        Err(RecordFault::BadValue("naplps object number repeated"))
    );
    let all: Vec<NaplpsObject<'_>> = (1..=65).map(region).collect();
    assert!(encode((page, subcode, generation), &all[..MAX_OBJECTS], 3, 7, &mut |_| {}).is_ok());
    assert!(encode((page, subcode, generation), &all, 3, 7, &mut |_| {}).is_err(), "a 65th object");
    // Reserved bytes and bits, a kind not listed, and the body's length.
    let mut body = Vec::new();
    encode((page, subcode, generation), &[good], 3, 0, &mut |_| {}).unwrap();
    body.extend([0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 1, 0]);
    body.extend([0, 1, 1, 3, 2, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0x40]);
    assert!(read(&body).is_ok());
    for (at, value, what) in [
        (6, 65, "65 objects"),
        (7, 4, "a reserved flag of the record"),
        (7, ONE | TITLED, "one object redefined with a title"),
        (10, 0, "kind 0"),
        (10, 3, "kind 3"),
        (11, 4, "a reserved flag"),
        (31, 1, "a label longer than the bytes left"),
    ] {
        let mut bad = body.clone();
        bad[at] = value;
        assert!(read(&bad).is_err(), "{what}");
    }
    assert_eq!(read(&body[..body.len() - 1]), Err(RecordFault::Length));
    body.push(0);
    assert_eq!(read(&body), Err(RecordFault::Length));
    assert_eq!(MAX_BYTES, 8 + 64 * (24 + 16 + 64) + 1 + 64);
}

/// ANIM-11, RULE-119 (REQ-6529, REQ-6612): the body alone, as a group of
/// a rule table holds it, is the bytes the reader reads, byte for byte,
/// and the group's body check accepts it for its page and sub-code; a
/// buffer one byte short is refused.
#[test]
fn a_naplps_objects_body_is_written_alone() {
    let objects = [macro_object(301, &[0x20, 0x7F])];
    let mut body = [0_u8; MAX_BYTES];
    let length = write_body(place(), &objects, &mut body).unwrap();
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 1, 0,
        0x01, 0x2D, 1, 3, 2, 0xFD, 0xFF, 0xF6, 0x01, 0x2C, 0x02, 0x80, 0x01, 0xE0, 0x00, 0x05, 0xFF, 0xFA,
        2, 0, 0x21, 0, 0x7F, 0,
        0x20, 0x7F,
    ];
    assert_eq!(&body[..length], expected);
    assert_eq!(read(&body[..length]).unwrap().objects().collect::<Vec<_>>(), objects);
    let facts = crate::records::body_check(TYPE_NAPLPS_OBJECTS, &body[..length]).unwrap();
    assert_eq!((facts.page, facts.sub), (Some(0x160), Some(0x0001)));
    let mut short = [0_u8; 33];
    assert_eq!(write_body(place(), &objects, &mut short), Err(RecordFault::Length));
}

fn labelled<'a>(object: NaplpsObject<'a>, label: &'a str) -> LabelledObject<'a> {
    LabelledObject { object, label }
}

/// TODO-6657 (DEC-6638 items 3 and 6, REQ-6643, REQ-6644): each object's
/// label follows its frame bytes, its length in byte 23, and the
/// picture's title follows the last object when the flag TITLED is set;
/// both read back, through the packets too, and a record without them
/// reads as before, with no label and no title.
#[test]
fn labels_and_a_title_are_their_bytes() {
    let objects = [labelled(macro_object(301, &[0x20]), "Play"), labelled(region(7), "")];
    let contents = Contents::Whole { objects: &objects, title: Some("Écran") };
    let mut body = [0_u8; MAX_BYTES];
    let length = write_contents(place(), &contents, &mut body).unwrap();
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 2, TITLED,
        0x01, 0x2D, 1, 3, 2, 0xFD, 0xFF, 0xF6, 0x01, 0x2C, 0x02, 0x80, 0x01, 0xE0, 0x00, 0x05, 0xFF, 0xFA,
        1, 0, 0x21, 0, 0x7F, 4,
        0x20, b'P', b'l', b'a', b'y',
        0x00, 0x07, 2, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0,
        6, 0xC3, 0x89, b'c', b'r', b'a', b'n',
    ];
    assert_eq!(&body[..length], expected);
    let record = read(expected).unwrap();
    assert_eq!((record.scope, record.title, record.count), (Scope::Whole, Some("Écran"), 2));
    assert_eq!(record.labelled_objects().collect::<Vec<_>>(), objects);
    assert_eq!(record.objects().collect::<Vec<_>>(), [objects[0].object, objects[1].object]);
    let mut reader = RecordReader::new();
    let mut seen = None;
    encode_contents(place(), &contents, 3, 7, &mut |packet| {
        if let Read::Record(ChannelRecord::NaplpsObjects(record)) = reader.push(&packet) {
            seen = Some((
                record.title.map(str::to_owned),
                record.labelled_objects().map(|o| o.label.to_owned()).collect::<Vec<_>>(),
            ));
        }
    })
    .unwrap();
    assert_eq!(seen, Some((Some("Écran".to_owned()), vec!["Play".to_owned(), String::new()])));
    // The writer without labels writes what it wrote before.
    let plain = read(&[0x01, 0x60, 0x00, 0x01, 0x00, 0x09, 0, 0]).unwrap();
    assert_eq!((plain.scope, plain.title), (Scope::Whole, None));
}

/// TODO-6657 (REQ-6643, REQ-6644): a label or a title of 65 bytes, bytes
/// that are not UTF-8, a control character and an empty title are each
/// refused, and the whole record with them; 64 bytes is taken.
#[test]
fn a_label_or_a_title_out_of_bounds_is_refused() {
    let most = "x".repeat(MAX_LABEL_BYTES);
    let over = "x".repeat(MAX_LABEL_BYTES + 1);
    let mut body = [0_u8; MAX_BYTES];
    let object = macro_object(1, &[0x40]);
    let whole = |objects: &[LabelledObject<'_>], title, body: &mut [u8]| {
        write_contents(place(), &Contents::Whole { objects, title }, body)
    };
    assert!(whole(&[labelled(object, &most)], Some(&most), &mut body).is_ok());
    assert!(whole(&[labelled(object, &over)], None, &mut body).is_err(), "a 65-byte label");
    assert!(whole(&[], Some(&over), &mut body).is_err(), "a 65-byte title");
    assert!(whole(&[labelled(object, "a\nb")], None, &mut body).is_err(), "a control character in a label");
    assert!(whole(&[], Some("a\u{7}"), &mut body).is_err(), "a control character in a title");
    assert!(whole(&[], Some(""), &mut body).is_err(), "an empty title");
    // Bytes that are not UTF-8, and a control character, as a channel
    // could send them.
    let length = whole(&[labelled(object, "ab")], Some("cd"), &mut body).unwrap();
    let good = body[..length].to_vec();
    assert!(read(&good).is_ok());
    let label_at = good.len() - 3 - 2;
    for (at, value, what) in [
        (label_at, 0xFF, "a label not UTF-8"),
        (label_at, 0x1B, "an escape in a label"),
        (good.len() - 1, 0x80, "a title not UTF-8"),
        (good.len() - 1, 0x7F, "a delete in a title"),
        (good.len() - 3, 0, "a title of no bytes"),
    ] {
        let mut bad = good.clone();
        bad[at] = value;
        assert!(read(&bad).is_err(), "{what}");
    }
    let mut over_label = good.clone();
    over_label[8 + 23] = 65;
    assert!(read(&over_label).is_err(), "a label length of 65");
}

/// TODO-6657 (DEC-6638 item 3, REQ-6645): a record with the flag ONE
/// redefines one object: it holds exactly one, never a title, and reads
/// back as [`Scope::One`].
#[test]
fn one_object_is_redefined_alone() {
    let object = labelled(macro_object(301, &[0x20]), "Stop");
    let mut body = [0_u8; MAX_BYTES];
    let length = write_contents(place(), &Contents::One(object), &mut body).unwrap();
    assert_eq!(body[6..8], [1, ONE]);
    let record = read(&body[..length]).unwrap();
    assert_eq!((record.scope, record.title, record.count), (Scope::One, None, 1));
    assert_eq!(record.labelled_objects().collect::<Vec<_>>(), [object]);
    let facts = crate::records::body_check(TYPE_NAPLPS_OBJECTS, &body[..length]).unwrap();
    assert_eq!((facts.page, facts.sub), (Some(0x160), Some(0x0001)), "a group may hold it");
    let mut none = body[..length].to_vec();
    none[6] = 0;
    none.truncate(8);
    assert!(read(&none).is_err(), "ONE with no object");
    let two = [labelled(region(1), ""), labelled(region(2), "")];
    let length = write_contents(place(), &Contents::Whole { objects: &two, title: None }, &mut body).unwrap();
    let mut both = body[..length].to_vec();
    both[7] = ONE;
    assert!(read(&both).is_err(), "ONE with two objects");
}
