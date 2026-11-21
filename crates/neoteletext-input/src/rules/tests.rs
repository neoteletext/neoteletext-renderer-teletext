//! The rule table's messages (REQ-6528; RULE-46 to RULE-49).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use crate::event::{
    Action, Device, Key, KeyCode, Mouse, MouseAction, PAGE_STATE, Pick, Point, RULE_EVENT, RULES_FAULT, StreamDecoder,
    Wheel, read, read_items, read_message, write,
};
use neoteletext_rules::state::TableState;
use neoteletext_rules::table::{SOUND_LEVEL, TARGET_SHOWN};
use std::vec::Vec;

const ON: Stamp = Stamp { page: 0x101, sub: 0x0001 };

fn bytes(item: &Item<'_>) -> Vec<u8> {
    let mut out = [0_u8; crate::MAX_MESSAGE];
    let length = write(item, &mut out).unwrap();
    out[..length].to_vec()
}

fn report(target: Target, body: RuleBody<'_>) -> Item<'_> {
    Item::Event(Event::RuleEvent(RuleEvent { on: ON, generation: 3, rule: 5, row: 2, tick: 412, target, body }))
}

fn key_a() -> Item<'static> {
    Item::Event(Event::Key(Key {
        on: ON,
        action: Action::Press,
        key: KeyCode::Character('a'),
        mods: 0,
        text: Some("a"),
        base: None,
        lost: false,
    }))
}

fn press() -> Item<'static> {
    let at = Point { column: 3, row: 4, pixels: Some((40, 90)), mods: 0 };
    Item::Event(Event::Mouse(Mouse { on: ON, action: MouseAction::Press, button: 0, at, device: Device::Mouse }))
}

fn pick() -> Item<'static> {
    Item::Event(Event::Pick(Pick { on: ON, drawing: 301, x: 2, y: -3, picture_x: 40, picture_y: 90, commits: 9 }))
}

/// RULE-46 (REQ-6528): a `RULE_EVENT` of each kind is its bytes, which
/// read back as it.
#[test]
fn a_rule_event_of_each_kind_round_trips() {
    // The stamp is the page and the sub-code as two varints: 0x101 is
    // 0x81 0x02, and 1 is 0x01.
    let head = [0xFF, RULE_EVENT];
    // Then the generation, the rule, the row and the tick.
    let after_stamp = [0x81, 0x02, 0x01, 0, 3, 5, 2, 0, 0, 0x01, 0x9C];
    let fired = bytes(&report(Target::NONE, RuleBody::Fired));
    assert_eq!(fired, [&head[..], &[15], &after_stamp, &[0, 0, 0, 0]].concat());
    let key = bytes(&key_a());
    let input = bytes(&report(Target::NONE, RuleBody::Input(&key)));
    assert_eq!(
        input,
        [&head[..], &[15 + u8::try_from(key.len()).unwrap()], &after_stamp, &[1, 0, 0, 0], &key].concat()
    );
    let level = Value::sound(0, SOUND_LEVEL);
    let value = bytes(&report(Target::NONE, RuleBody::Value { value: level, reading: -2 }));
    let body = [5, 0, 0, 1, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFE];
    assert_eq!(value, [&head[..], &[27], &after_stamp, &[2, 0, 0, 0], &body].concat());
    // A pointer event names the target it was inside, and carries its
    // PICK on a page the ReGIS renderer draws.
    let both = [bytes(&press()), bytes(&pick())].concat();
    let inside = report(Target::object(301), RuleBody::Input(&both));
    let message = bytes(&inside);
    let mut back = Vec::new();
    read_message(&message, &mut |item| back.push(item)).unwrap();
    assert_eq!(back, [inside]);
    let Item::Event(Event::RuleEvent(carrier)) = back[0] else { panic!() };
    let Item::Event(Event::Pick(picked)) = pick() else { panic!() };
    assert_eq!(carrier.input(), Some((press(), Some(picked))));
    assert_eq!(carrier.target, Target::object(301));
    // Text that came from no key is one run in place of an event.
    let text = report(Target::NONE, RuleBody::Input("né".as_bytes()));
    let message = bytes(&text);
    let Some((Item::Event(Event::RuleEvent(carrier)), _)) = read(&message).unwrap() else { panic!() };
    assert_eq!(carrier.input(), Some((Item::Text("né"), None)));
    // The other kinds carry no input.
    let Some((Item::Event(Event::RuleEvent(carrier)), _)) = read(&fired).unwrap() else { panic!() };
    assert_eq!(carrier.input(), None);
}

/// RULE-46 (REQ-6528): what a `RULE_EVENT` may not be.
#[test]
fn a_rule_event_out_of_range_is_refused() {
    let refused = |item: &Item<'_>| write(item, &mut [0_u8; crate::MAX_MESSAGE]).is_err();
    let with = |change: &dyn Fn(&mut RuleEvent<'_>)| {
        let mut event = RuleEvent {
            on: ON,
            generation: 3,
            rule: 5,
            row: 255,
            tick: 0,
            target: Target::NONE,
            body: RuleBody::Fired,
        };
        change(&mut event);
        refused(&Item::Event(Event::RuleEvent(event)))
    };
    assert!(!with(&|_| {}));
    assert!(with(&|event| event.generation = 0));
    assert!(!with(&|event| event.rule = 63));
    assert!(with(&|event| event.rule = 64));
    // 255 is a run no rule made, and no row; a row is a rule's.
    assert!(!with(&|event| event.rule = 255));
    assert!(!with(&|event| event.row = 63));
    assert!(with(&|event| event.row = 64));
    assert!(with(&|event| (event.rule, event.row) = (255, 0)));
    let of_sequence = Value::sequence(8, 0);
    assert!(with(&|event| event.body = RuleBody::Value { value: of_sequence, reading: 0 }));
    let fired = Value::fired(0);
    assert!(!with(&|event| event.body = RuleBody::Value { value: fired, reading: 97 }));
    // A firing with no event and a value name no target.
    assert!(with(&|event| event.target = Target::object(1)));
    let unchecked = Value::target(TargetKind::Control, 33, TARGET_SHOWN);
    assert!(with(&|event| event.body = RuleBody::Value { value: unchecked, reading: 0 }));
    assert!(with(&|event| event.body = RuleBody::Value { value: Value::NONE, reading: 0 }));
    let key = bytes(&key_a());
    let input = |body: &[u8], target| refused(&report(target, RuleBody::Input(body)));
    assert!(!input(&key, Target::NONE));
    assert!(!input(&key, Target::control(32)));
    assert!(!input(&key, Target::rect(15)));
    assert!(input(&key, Target::control(33)));
    assert!(input(&key, Target::control(0)));
    assert!(input(&key, Target::object(0)));
    assert!(input(&key, Target::rect(16)));
    assert!(input(&key, Target { kind: TargetKind::Page, number: 1 }));
    // Nothing; an event cut short; two events that are not a pointer
    // event and its PICK; a PICK alone; three events; text then an event;
    // and the events that are the session's.
    assert!(input(&[], Target::NONE));
    assert!(input(&key[..key.len() - 1], Target::NONE));
    assert!(input(&[key.clone(), key.clone()].concat(), Target::NONE));
    assert!(input(&[key.clone(), bytes(&pick())].concat(), Target::NONE));
    assert!(input(&bytes(&pick()), Target::NONE));
    assert!(input(&[bytes(&press()), bytes(&pick()), bytes(&pick())].concat(), Target::NONE));
    assert!(input(&[b"a".to_vec(), key.clone()].concat(), Target::NONE));
    let select = bytes(&Item::Event(Event::Select(crate::event::Select::new(0x200, 0x3F7F).unwrap(), "")));
    assert!(input(&select, Target::NONE));
    let nested = bytes(&report(Target::NONE, RuleBody::Input(&key)));
    assert!(input(&nested, Target::NONE), "a RULE_EVENT carries no RULE_EVENT");
    // A wheel carries its PICK too.
    let at = Point { column: 3, row: 4, pixels: None, mods: 0 };
    let wheel = bytes(&Item::Event(Event::Wheel(Wheel { on: ON, dx: 0, dy: 120, at })));
    assert!(!input(&[wheel, bytes(&pick())].concat(), Target::object(9)));
    // A kind not listed, a body after a firing, and a payload cut short.
    let fired = bytes(&report(Target::NONE, RuleBody::Fired));
    let mut kind = fired.clone();
    kind[14] = 3;
    assert!(read(&kind).is_err());
    let mut longer = fired.clone();
    longer[2] += 1;
    longer.push(0);
    assert!(read(&longer).is_err());
    let mut shorter = fired;
    shorter[2] -= 1;
    shorter.pop();
    assert_eq!(read(&shorter), Err(InputError::BadValue("rule event")));
}

/// RULE-46, RULE-49 (REQ-6528): from protocol version 10 on (now 11,
/// DEC-7031 item 7) a viewer's input
/// at the top level of the stream is a protocol fault; inside a
/// `RULE_EVENT` it is read; the session's own events stay at the top
/// level.
#[test]
fn a_bare_input_event_is_a_protocol_fault() {
    assert_eq!(crate::VERSION, 11);
    let key = bytes(&key_a());
    for bare in [key.clone(), bytes(&press()), bytes(&pick()), b"text".to_vec(), bytes(&Item::Event(Event::FocusIn))] {
        assert_eq!(read_message(&bare, &mut |_| {}), Err(InputError::Bare), "{bare:?}");
        let mut decoder = StreamDecoder::<{ crate::event::MAX_STREAM_ITEM }>::default();
        assert_eq!(decoder.push(&bare, &mut |_| {}), Err(InputError::Bare), "{bare:?}");
        // What a RULE_EVENT carried, taken out, reads as before.
        assert!(read_items(&bare, &mut |_| {}).is_ok());
    }
    let hello = Item::Event(Event::Hello { version: crate::VERSION, caps: 0, agent: None });
    let select = Item::Event(Event::Select(crate::event::Select::new(0x200, 0x3F7F).unwrap(), ""));
    let resize = Item::Event(Event::Resize { columns: 40, rows: 25, width: 0, height: 0 });
    let wrapped = report(Target::NONE, RuleBody::Input(&key));
    let message = [bytes(&hello), bytes(&select), bytes(&resize), bytes(&wrapped)].concat();
    let mut seen = 0;
    read_message(&message, &mut |_| seen += 1).unwrap();
    assert_eq!(seen, 4);
    // Nothing after the fault is read.
    let mut seen = 0;
    let faulty = [bytes(&select), key, bytes(&resize)].concat();
    assert_eq!(read_message(&faulty, &mut |_| seen += 1), Err(InputError::Bare));
    assert_eq!(seen, 1);
}

/// RULE-47 (REQ-6528): a `PAGE_STATE` carries the answer after its stamp,
/// checked by the rule table's own reader.
#[test]
fn a_page_state_round_trips_and_is_checked() {
    let mut table = TableState::new(3, 9120);
    table.variables[0] = 5;
    table.values.push((Value::sound(0, SOUND_LEVEL), 41)).unwrap();
    let state = PageState { identifier: 42, table: Some(table) };
    let mut body = [0_u8; neoteletext_rules::state::MAX_PAYLOAD_BYTES];
    let length = neoteletext_rules::state::encode(&state, &mut body).unwrap();
    let event = PageStateEvent { on: ON, body: &body[..length] };
    let message = bytes(&Item::Event(Event::PageState(event)));
    // The payload's length is a varint of two bytes: the sixteen
    // variables and the eight timers alone pass 127 bytes.
    let payload = 3 + length;
    let varint = [u8::try_from(payload & 0x7F).unwrap() | 0x80, u8::try_from(payload >> 7).unwrap()];
    assert_eq!(message[..7], [0xFF, PAGE_STATE, varint[0], varint[1], 0x81, 0x02, 0x01]);
    let Some((Item::Event(Event::PageState(back)), used)) = read(&message).unwrap() else { panic!() };
    assert_eq!((used, back, back.state()), (message.len(), event, Some(state)));
    // A page with no table.
    let none = PageStateEvent { on: ON, body: &[0, 0, 0, 42, 0, 0, 0, 0, 0, 0] };
    assert_eq!(none.state(), Some(PageState { identifier: 42, table: None }));
    assert_eq!(bytes(&Item::Event(Event::PageState(none))).len(), 2 + 1 + 3 + 10);
    // A body the rule table's reader refuses, and one cut short.
    let refused = |body: &[u8]| {
        write(&Item::Event(Event::PageState(PageStateEvent { on: ON, body })), &mut [0_u8; crate::MAX_MESSAGE])
    };
    assert_eq!(refused(&[0, 0, 0, 42, 0, 0, 0, 0, 0, 1]), Err(InputError::BadValue("page state")));
    assert_eq!(refused(&body[..length - 1]), Err(InputError::BadValue("page state")));
    assert_eq!(refused(&[0; MAX_STATE_BYTES]), Err(InputError::BadValue("page state")));
    assert_eq!(crate::event::units(&Item::Event(Event::PageState(event))), 0);
}

/// RULE-48 (REQ-6528): a `RULES_FAULT` of each code is its bytes, and a
/// code not listed is refused: code 2, the first form's `tick_overrun`,
/// is withdrawn and never read.
#[test]
fn a_rules_fault_round_trips_and_is_checked() {
    let names = [
        (1, "undecodable"),
        (3, "reports_dropped"),
        (4, "events_dropped"),
        (5, "commands_skipped"),
        (6, "query_dropped"),
        (7, "local_overrun"),
        (8, "animation_slowed"),
        (9, "evaluation_late"),
        (10, "queue_full"),
    ];
    assert_eq!(names.len(), FaultCode::ALL.len());
    for (code, name) in names {
        let fault = FaultCode::from_code(code).unwrap();
        assert_eq!((fault.code(), fault.name()), (code, name));
        let event = RulesFault { on: ON, generation: 3, fault, count: 12 };
        let message = bytes(&Item::Event(Event::RulesFault(event)));
        assert_eq!(message, [0xFF, RULES_FAULT, 8, 0x81, 0x02, 0x01, 0, 3, code, 0, 12]);
        assert_eq!(read(&message), Ok(Some((Item::Event(Event::RulesFault(event)), message.len()))));
    }
    for withdrawn_or_none in [0, 2, 11, 255] {
        assert_eq!(FaultCode::from_code(withdrawn_or_none), None);
    }
    for bad in [
        &[0xFF, RULES_FAULT, 8, 0x81, 0x02, 0x01, 0, 3, 11, 0, 12][..],
        &[0xFF, RULES_FAULT, 8, 0x81, 0x02, 0x01, 0, 3, 0, 0, 12],
        &[0xFF, RULES_FAULT, 8, 0x81, 0x02, 0x01, 0, 3, 2, 0, 12],
        &[0xFF, RULES_FAULT, 7, 0x81, 0x02, 0x01, 0, 3, 1, 0],
        &[0xFF, RULES_FAULT, 9, 0x81, 0x02, 0x01, 0, 3, 1, 0, 12, 0],
    ] {
        assert_eq!(read(bad), Err(InputError::BadValue("rules fault")), "{bad:?}");
    }
}
