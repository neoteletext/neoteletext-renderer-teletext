//! The command records and the body check (REQ-6612; RULE-21, RULE-22,
//! RULE-56, RULE-99, RULE-100, RULE-102, RULE-103, RULE-119).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};
use neoteletext_audio::record::{Control, Operation};
use neoteletext_rules::command::{
    Binding, CONTROL_FADE_AT, CONTROL_GAIN_AT, Operand, Source, StartMode, VariableOperation, apply_bindings, field,
};
use neoteletext_rules::limits::MAX_COMMAND_BYTES;
use neoteletext_rules::table::{Framed, Rule, Table, Trigger, Value};
use std::boxed::Box;
use std::vec::Vec;

const PAGE: u16 = 0x101;
const SUB: u16 = 0x0001;

fn place() -> (PageAddress, SubCode) {
    (PageAddress::from_mpp(PAGE).unwrap(), SubCode::new(SUB))
}

/// RULE-21 to RULE-103 (REQ-6612): one record of each of the types 28 to
/// 33 reaches a receiver's reader as its command, with the body a group
/// holds, in one packet; and the six types are a channel's records.
#[test]
fn each_command_record_reaches_a_receiver() {
    let (page, subcode) = place();
    let commands = [
        Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 },
        Command::Variable {
            operation: VariableOperation::Wrap,
            variable: 0,
            operand: Operand::Range { low: 0, high: 3 },
        },
        Command::Timer { start: true, timer: 0 },
        Command::Report { value: Some(Value::variable(0)) },
        Command::Run { group: 3 },
        Command::Sequence { start: true, sequence: 2, mode: StartMode::Beat { timer: 255, ticks: 30 } },
    ];
    for (command, kind) in commands.into_iter().zip(TYPE_LOCAL_MIDI..=TYPE_SEQUENCE) {
        assert_eq!(command.kind(), kind);
        let mut bytes = [0_u8; MAX_BODY_BYTES];
        let length = body(&command, page, subcode, &mut bytes).unwrap();
        let expected = CommandRecord { kind, page, subcode, command, body: &bytes[..length] };
        assert_eq!(read(kind, &bytes[..length]), Ok(expected));
        let mut reader = RecordReader::new();
        let mut seen = false;
        let packets = encode(&command, page, subcode, 3, 7, &mut |packet| {
            let Read::Record(record) = reader.push(&packet) else { panic!("{command:?} is not read") };
            let carried = match record {
                ChannelRecord::LocalMidi(carried)
                | ChannelRecord::Variable(carried)
                | ChannelRecord::Timer(carried)
                | ChannelRecord::Report(carried)
                | ChannelRecord::Run(carried)
                | ChannelRecord::Sequence(carried) => carried,
                other => panic!("{other:?}"),
            };
            assert_eq!(carried, expected);
            seen = true;
        })
        .unwrap();
        assert!(seen);
        assert_eq!(packets, 1);
    }
    // Another type, another record's body, and a page out of range.
    assert!(read(27, &[1, 1, 0, 1, 1, 0]).is_err());
    assert!(read(TYPE_TIMER, &[1, 1, 0, 1, 1, 0, 0]).is_err());
    assert!(read(TYPE_TIMER, &[0, 0xFF, 0, 1, 1, 0]).is_err());
    assert!(read(TYPE_TIMER, &[1, 1, 0x3F, 0x7F, 1, 0]).is_err());
}

/// The body of the audio profile's CONTROL record for `control`
/// (REQ-AUD-006), which the profile's own reader reads back as it.
fn control_body(control: &Control) -> Vec<u8> {
    let operation = match control.operation {
        Operation::Play => 1,
        Operation::Stop => 2,
        Operation::Gain => 3,
    };
    let mut body = Vec::from([control.channel]);
    body.extend(control.sequence.to_be_bytes());
    body.extend([operation, control.handle]);
    body.extend(control.generation.to_be_bytes());
    body.push(control.gain);
    body.extend(control.fade_ms.to_be_bytes());
    body.extend([control.flags, control.duck]);
    body.extend(control.page.unwrap_or(neoteletext_audio::SERVICE_SCOPE).to_be_bytes());
    let mut record = [0_u8; 64];
    let length = neoteletext_audio::record::write(TYPE_CONTROL, &body, &mut record).unwrap();
    assert_eq!(neoteletext_audio::record::parse(&record[..length]), Ok(Record::Control(*control)));
    body
}

fn play() -> Control {
    Control {
        channel: 1,
        sequence: 0,
        operation: Operation::Play,
        handle: 5,
        generation: 2,
        gain: 80,
        fade_ms: 500,
        flags: neoteletext_audio::record::FLAG_LOOP,
        duck: 0,
        page: Some(PAGE),
        music: None,
    }
}

/// RULE-56, RULE-119 (REQ-6612): the body check reads a `CONTROL`, a
/// `SPRITES` and a `NAPLPS_OBJECTS` body with its record's own reader and
/// says what the rule table's crate needs of it; a body that reader
/// refuses, and another type, give nothing.
#[test]
fn the_body_check_is_the_records_own_readers() {
    let body = control_body(&play());
    let facts = body_check(TYPE_CONTROL, &body).unwrap();
    assert_eq!(facts, BodyFacts { page: Some(PAGE), sub: None, sequence: 0, after_current: false });
    let service = control_body(&Control { page: None, sequence: 9, ..play() });
    assert_eq!(body_check(TYPE_CONTROL, &service).map(|facts| (facts.page, facts.sequence)), Some((None, 9)));
    let after = control_body(&Control { flags: FLAG_AFTER_CURRENT, ..play() });
    assert_eq!(body_check(TYPE_CONTROL, &after).map(|facts| facts.after_current), Some(true));
    let mut gain = body.clone();
    gain[CONTROL_GAIN_AT] = 101;
    assert_eq!(body_check(TYPE_CONTROL, &gain), None);
    assert_eq!(body_check(TYPE_CONTROL, &body[..body.len() - 1]), None);
    assert_eq!(body_check(TYPE_CONTROL, &[&body[..], &[0]].concat()), None);

    let (page, subcode) = place();
    let mut sprites_body = Vec::new();
    let frames = [0x0A, 0x05];
    let sprite = sprites::Sprite::new(10, true, true, 2, 0, (5, 6), (3, 2), &frames);
    sprites::encode(page, subcode, &[sprite], 0, 0, &mut |packet| sprites_body.push(packet)).unwrap();
    let mut reader = RecordReader::new();
    let Read::Record(ChannelRecord::Sprites(_)) = reader.push(&sprites_body[0]) else { panic!() };
    let none = [0x01, 0x01, 0x00, 0x01, 0, 0];
    let placed = Some(BodyFacts { page: Some(PAGE), sub: Some(SUB), sequence: 0, after_current: false });
    assert_eq!(body_check(TYPE_SPRITES, &none), placed, "a SPRITES record that takes every sprite away");
    assert_eq!(body_check(TYPE_SPRITES, &[0x01, 0x01, 0x00, 0x01, 1, 0]), None, "a sprite cut short");
    assert_eq!(body_check(TYPE_NAPLPS_OBJECTS, &[0x01, 0x01, 0x00, 0x01, 0, 1, 0, 0]), placed);
    assert_eq!(body_check(TYPE_NAPLPS_OBJECTS, &none), None);
    for kind in [0, 1, 2, 23, 28, 33] {
        assert_eq!(body_check(kind, &body), None, "type {kind} is not the body check's");
    }
    // Renderer parity PAR-20: a whole sending of drawing links, and a
    // cursor, read by their records' own readers.
    let links = [0x01, 0x01, 0x00, 0x01, 0, 1, 1, 0x00, 0x07, 0x00, 0x03, b'3', b'0', b'0'];
    assert_eq!(body_check(TYPE_DRAWING_LINKS, &links), placed, "one link to page 300");
    assert_eq!(body_check(TYPE_DRAWING_LINKS, &[0x01, 0x01, 0x00, 0x01, 0, 1, 0]), placed, "no links");
    assert_eq!(body_check(TYPE_DRAWING_LINKS, &[0x01, 0x01, 0x00, 0x01, 0, 2, 0]), None, "part of a longer sending");
    assert_eq!(body_check(TYPE_DRAWING_LINKS, &links[..links.len() - 1]), None, "a target cut short");
    assert_eq!(body_check(TYPE_REGIS_CURSOR, &[0x01, 0x01, 0x00, 0x01, 0x00, 0x07]), placed);
    assert_eq!(body_check(TYPE_REGIS_CURSOR, &[0x01, 0x01, 0x00, 0x01, 0x00]), None, "cut short");
    assert_eq!(body_check(TYPE_REGIS_CURSOR, &[0x01, 0x01, 0x3F, 0x7F, 0x00, 0x07]), None, "sub-code 3F7F");
}

/// RULE-106 (REQ-6612): the places the rule table's crate fills a
/// `CONTROL`'s gain and fade at are where the audio profile's reader
/// reads them, so a bound `CONTROL` is applied with the value bound.
#[test]
fn a_bound_control_is_read_with_its_amounts() {
    let body = control_body(&play());
    let bindings =
        [Binding::of(field::CONTROL_GAIN, Source::Variable(0)), Binding::of(field::CONTROL_FADE, Source::Parameter)];
    let mut out = [0_u8; MAX_COMMAND_BYTES];
    let bound = apply_bindings(
        TYPE_CONTROL,
        &body,
        &bindings,
        &|source| if source == Source::Variable(0) { 15 } else { 1500 },
        &mut out,
    );
    let mut record = [0_u8; 64];
    let length = neoteletext_audio::record::write(TYPE_CONTROL, &out[..bound.length], &mut record).unwrap();
    let Ok(Record::Control(control)) = neoteletext_audio::record::parse(&record[..length]) else { panic!() };
    assert_eq!(control, Control { gain: 95, fade_ms: 2000, ..play() });
    assert_eq!((CONTROL_GAIN_AT, CONTROL_FADE_AT), (7, 8));

    // And a table that holds it is one the decoder accepts with this
    // crate's body check, and refuses for another page's sound.
    let mut table = Box::new(Table::new(PAGE, SUB));
    table.push_group(&[Framed { kind: TYPE_CONTROL, body: &body, bindings: &bindings[..1] }]).unwrap();
    table.rules.push(Rule::new(Trigger::Open, 0)).unwrap();
    assert_eq!(table.check(body_check), Ok(()));
    let other = control_body(&Control { page: Some(0x102), ..play() });
    let mut table = Box::new(Table::new(PAGE, SUB));
    table.push_group(&[Framed { kind: TYPE_CONTROL, body: &other, bindings: &[] }]).unwrap();
    assert_eq!(table.check(body_check), Err(neoteletext_rules::Fault::Command("names another page")));
}

/// Renderer parity PAR-20 (TODO-6794): a group may hold `DRAWING_LINKS`
/// and `REGIS_CURSOR`, so that a rule table can give a picture's links and
/// cursor (`naplps_links`, `naplps_cursor`, `regis_links`, `regis_cursor`
/// as group commands); one naming another page is refused, as every
/// command is, and neither takes a binding.
#[test]
fn a_group_may_hold_drawing_links_and_a_cursor_for_its_own_page() {
    let links: &[u8] = &[0x01, 0x01, 0x00, 0x01, 0, 1, 1, 0x00, 0x07, 0x00, 0x03, b'3', b'0', b'0'];
    let cursor: &[u8] = &[0x01, 0x01, 0x00, 0x01, 0x00, 0x07];
    let mut table = Box::new(Table::new(PAGE, SUB));
    table
        .push_group(&[
            Framed { kind: TYPE_DRAWING_LINKS, body: links, bindings: &[] },
            Framed { kind: TYPE_REGIS_CURSOR, body: cursor, bindings: &[] },
        ])
        .unwrap();
    table.rules.push(Rule::new(Trigger::Open, 0)).unwrap();
    assert_eq!(table.check(body_check), Ok(()));
    for (kind, body) in [
        (TYPE_DRAWING_LINKS, [&[0x01, 0x02], &links[2..]].concat()),
        (TYPE_REGIS_CURSOR, [&[0x01, 0x01, 0x00, 0x02], &cursor[4..]].concat()),
    ] {
        let mut table = Box::new(Table::new(PAGE, SUB));
        table.push_group(&[Framed { kind, body: &body, bindings: &[] }]).unwrap();
        assert_eq!(table.check(body_check), Err(neoteletext_rules::Fault::Command("names another page")), "{kind}");
    }
    let bound = [Binding::of(field::CONTROL_GAIN, Source::Parameter)];
    let mut table = Box::new(Table::new(PAGE, SUB));
    table.push_group(&[Framed { kind: TYPE_REGIS_CURSOR, body: cursor, bindings: &bound }]).unwrap();
    assert!(table.check(body_check).is_err(), "a cursor takes no binding");
}
