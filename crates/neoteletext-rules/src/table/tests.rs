//! The RULES record's body (REQ-6526): every shape round-trips, and every
//! limit and fault of RULE-70 is refused whole.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing and arithmetic on data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::limits::*;
use std::boxed::Box;
use std::vec::Vec;

const PAGE: u16 = 0x101;

fn send() -> Action {
    Action::Send(None)
}

/// A rule of `trigger` with one `send`.
fn rule(trigger: Trigger) -> Rule {
    let mut rule = Rule::new(trigger);
    rule.actions.push(send()).unwrap();
    rule
}

fn keys(list: &[Key]) -> KeySet {
    let mut set = any_key();
    for key in list {
        set.push(*key).unwrap();
    }
    set
}

fn condition(nodes: &[Node]) -> Condition {
    let mut condition = no_condition();
    for node in nodes {
        condition.push(*node).unwrap();
    }
    condition
}

fn compare(value: Value, constant: i32) -> Node {
    Node::Compare { value, operator: Operator::Equal, constant }
}

fn members(list: &[i32]) -> SetMembers {
    let mut members = SetMembers::new(0);
    for member in list {
        members.push(*member).unwrap();
    }
    members
}

fn animate(target: Target, kind: OperationKind) -> Action {
    Action::Animate(Operation { target, kind })
}

fn bytes_of(table: &Table) -> Vec<u8> {
    let mut out = [0_u8; MAX_TABLE_BYTES];
    let length = encode(PAGE, 7, Some(table), &mut out).unwrap();
    out[..length].to_vec()
}

fn refused(table: &Table) -> Fault {
    let mut out = [0_u8; 2 * MAX_TABLE_BYTES];
    encode(PAGE, 7, Some(table), &mut out).unwrap_err()
}

/// A table that uses every trigger, node and action kind once.
fn every_shape() -> Box<Table> {
    let mut table = Box::new(Table::new());
    table.variables.push(Variable { min: -5, max: 5, initial: 0 }).unwrap();
    table.variables.push(Variable { min: i32::MIN, max: i32::MAX, initial: 9 }).unwrap();
    table.timers.push(true).unwrap();
    table.timers.push(false).unwrap();
    table.rects.push(Rect { x: 40, y: 300, width: 200, height: 100 }).unwrap();
    let a = Key { kind: KeyKind::Physical, code: u32::from('a') };
    let enter = Key { kind: KeyKind::Named, code: 13 };
    let e_acute = Key { kind: KeyKind::Character, code: u32::from('é') };
    let object = Target::object(301);
    let triggers = [
        Trigger::Key { keys: keys(&[a, enter, e_acute]), repeat: true, mods_mask: 5, mods_value: 1 },
        Trigger::KeyUp { keys: any_key(), mods_mask: 0, mods_value: 0 },
        Trigger::Press { target: Target::control(3), button: 0 },
        Trigger::Release { target: Target::ANY_OBJECT, button: ANY_BUTTON },
        Trigger::Enter { target: Target::rect(0) },
        Trigger::Leave { target: Target::ANY_CONTROL },
        Trigger::Open,
        Trigger::Close,
        Trigger::Every { timer: PAGE_TIMER, ticks: MAX_TIMER_TICKS },
        Trigger::After { timer: 1, ticks: 1 },
        Trigger::Text,
        Trigger::Move { target: Target::PAGE, held: true },
        Trigger::Wheel { target: object },
        Trigger::Focus,
        Trigger::Blur,
    ];
    for trigger in triggers {
        table.rules.push(rule(trigger)).unwrap();
    }
    let mut becomes = Rule::new(Trigger::Becomes);
    becomes.enabled = false;
    becomes.echo = true;
    becomes.condition = condition(&[
        Node::And(2),
        Node::Or(3),
        compare(Value::variable(1), -7),
        Node::InRange { value: Value::timer(PAGE_TIMER, TIMER_ELAPSED), low: -1, high: 600 },
        Node::InSet { value: Value::sound(3, SOUND_LEVEL), members: members(&[1, 2, i32::MIN]) },
        Node::Not,
        Node::And(4),
        Node::Overlaps(object, Target::PAGE),
        Node::Inside(Target::control(32), Target::rect(0)),
        compare(Value::synth(0, 14, 1, 15, CLASS_FIELD, 255), 0),
        compare(Value::key(INPUT_PHYSICAL, u32::from('q')), 1),
    ]);
    table.rules.push(becomes).unwrap();
    let mut actions = Rule::new(Trigger::Open);
    for action in [
        Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 },
        Action::Play { channel: 1, handle: 255, generation: 65_535, gain: 100, fade_ms: 10_000, looped: true },
        Action::Stop { channel: 2, fade_ms: 0 },
        Action::Gain { channel: 3, gain: 0, fade_ms: 5 },
        animate(Target::control(1), OperationKind::Show),
        animate(object, OperationKind::Hide),
        animate(object, OperationKind::Look(3)),
        Action::Set { variable: 0, value: -3 },
        Action::Add { variable: 1, amount: i32::MIN },
        Action::StartTimer(1),
        Action::StopTimer(0),
        Action::Enable(1 << 16),
        Action::Disable(1),
        send(),
        Action::Send(Some(Value::target(TargetKind::Object, 301, TARGET_INSIDE))),
        animate(object, OperationKind::Move { relative: true, x: -8, y: 32_767 }),
    ] {
        actions.actions.push(action).unwrap();
    }
    table.rules.push(actions).unwrap();
    let mut more = Rule::new(Trigger::Close);
    for action in [
        animate(object, OperationKind::Order { layer: KEEP_LAYER, order: -128 }),
        animate(object, OperationKind::Frame(15)),
        animate(object, OperationKind::Cycle { first: 0, last: 15, period: 3600, once: true }),
        animate(object, OperationKind::StopCycle),
    ] {
        more.actions.push(action).unwrap();
    }
    table.rules.push(more).unwrap();
    table.subpages.push(SubpageMask { sub: 0x0002, off: 0b101 }).unwrap();
    table.subpages.push(SubpageMask { sub: LAST_SUBCODE, off: 1 << 17 }).unwrap();
    table
}

/// RULE-3, RULE-9, RULE-18, RULE-20, RULE-41 to RULE-45 (REQ-6526): every
/// shape round-trips, and the body is the one encoding of its table.
#[test]
fn every_shape_round_trips() {
    let table = every_shape();
    let body = bytes_of(&table);
    let mut back = Box::new(Table::new());
    let header = decode(&body, &mut back).unwrap();
    assert_eq!(header, Header { page: PAGE, generation: 7, has_table: true });
    assert_eq!(*back, *table);
    assert_eq!(bytes_of(&back), body);
    assert_eq!(check(&body), Ok(header));
}

/// RULE-41 (REQ-6526): the header's twelve bytes, and the body of a page
/// with no table.
#[test]
fn the_header_is_twelve_bytes() {
    let mut out = [0_u8; 64];
    let length = encode(0x8FF, 1, None, &mut out).unwrap();
    assert_eq!(out[..length], [0x08, 0xFF, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0]);
    let mut table = Table::new();
    assert_eq!(decode(&out[..length], &mut table), Ok(Header { page: 0x8FF, generation: 1, has_table: false }));
    // A table with no rules is still a table (RULE-54: a page that takes
    // no input).
    let length = encode(0x100, 65_535, Some(&Table::new()), &mut out).unwrap();
    assert_eq!(out[..length], [0x01, 0x00, 0xFF, 0xFF, 1, 1, 0, 0, 0, 0, 0, 0]);
    for (at, bad, fault) in [
        (0, 0x00, Fault::Range("page")),
        (0, 0x09, Fault::Range("page")),
        (4, 2, Fault::Range("format")),
        (5, 3, Fault::Reserved("header flags")),
        (11, 1, Fault::Reserved("header")),
    ] {
        let mut body = out;
        body[at] = bad;
        assert_eq!(check(&body[..length]), Err(fault), "byte {at}");
    }
    let mut zero = out;
    (zero[2], zero[3]) = (0, 0);
    assert_eq!(check(&zero[..length]), Err(Fault::Range("generation")));
    // Without the flag every count is 0 and the body ends at byte 11.
    let mut counted = out;
    (counted[5], counted[7]) = (0, 1);
    assert_eq!(check(&counted[..length]), Err(Fault::Range("counts without a table")));
    assert_eq!(check(&out[..length - 1]), Err(Fault::Short));
    let mut longer = out[..length].to_vec();
    longer.push(0);
    assert_eq!(check(&longer), Err(Fault::Trailing));
}

/// RULE-43 to RULE-45 (REQ-6526): a rule's bytes, field by field.
#[test]
fn a_rule_is_its_bytes() {
    let mut table = Table::new();
    table.variables.push(Variable { min: 0, max: 8, initial: 4 }).unwrap();
    let mut rule = Rule::new(Trigger::Key {
        keys: keys(&[Key { kind: KeyKind::Physical, code: u32::from('a') }]),
        repeat: false,
        mods_mask: 0,
        mods_value: 0,
    });
    rule.condition = condition(&[Node::Compare { value: Value::variable(0), operator: Operator::Less, constant: 8 }]);
    rule.actions.push(Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 }).unwrap();
    rule.actions.push(Action::Add { variable: 0, amount: -1 }).unwrap();
    table.rules.push(rule).unwrap();
    let body = bytes_of(&table);
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0x01, 0x01, 0x00, 0x07, 1, 1, 1, 0, 0, 1, 0, 0,
        // The variable: min, max, initial.
        0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 4,
        // Flags (enabled), key trigger, flags, mask, value, one key:
        // physical 'a'.
        0x01, 1, 0, 0, 0, 1, 3, 0, 0, 0, 0x61,
        // One node: compare, variable 0, operator <, 8.
        1, 4, 1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 8,
        // Two actions: midi; add variable 0, −1.
        2, 1, 0, 0x90, 60, 100, 9, 0, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    assert_eq!(body, expected);
}

/// RULE-53 (REQ-6526): the default table, with and without `wheel`, byte
/// for byte.
#[test]
fn the_default_table_is_byte_exact() {
    let mut table = Table::new();
    default_send_all(false, &mut table);
    assert_eq!(table.rules.len(), 12);
    #[rustfmt::skip]
    let twelve: &[u8] = &[
        0x01, 0x01, 0x00, 0x07, 1, 1, 0, 0, 0, 12, 0, 0,
        // keys: key, repeat, any key; no condition; one action, send.
        1, 1, 1, 0, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // keys_up.
        1, 2, 0, 0, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // text.
        1, 12, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // press and release on the page, any button.
        1, 3, 0, 0, 0, 255, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 4, 0, 0, 0, 255, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // move on the page, not held.
        1, 13, 0, 0, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // hover_in and hover_out: enter and leave, any control.
        1, 5, 1, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 6, 1, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // drawing_in and drawing_out: enter and leave, any object.
        1, 5, 2, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 6, 2, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        // focus and blur.
        1, 15, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        1, 16, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(bytes_of(&table), twelve);
    default_send_all(true, &mut table);
    let mut thirteen = twelve.to_vec();
    thirteen[9] = 13;
    // wheel on the page.
    thirteen.extend([1, 14, 0, 0, 0, 0, 1, 14, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes_of(&table), thirteen);
    let index = Index::of(&table).unwrap();
    let all =
        INTEREST_KEYS | INTEREST_TEXT | INTEREST_POINTER_BUTTONS | INTEREST_MOVES | INTEREST_WHEEL | INTEREST_FOCUS;
    assert_eq!((index.interest, index.values.len(), index.targets.len(), index.watch.len()), (all, 0, 0, 0));
}

/// A body of `count` of something in its header and nothing after: the
/// count is refused before anything is read.
fn header_with(at: usize, count: u8) -> [u8; HEADER_BYTES] {
    let mut header = [0x01, 0x01, 0x00, 0x07, 1, 1, 0, 0, 0, 0, 0, 0];
    header[at] = count;
    header
}

/// RULE-57 (REQ-6526): the counts of the header, each at its limit and
/// one over.
#[test]
fn the_declaration_limits_hold() {
    for (at, limit, name) in [
        (6, MAX_VARIABLES, "MAX_VARIABLES"),
        (7, MAX_TIMERS, "MAX_TIMERS"),
        (8, MAX_RECTS, "MAX_RECTS"),
        (9, MAX_RULES, "MAX_RULES"),
        (10, MAX_SUBPAGE_MASKS, "MAX_SUBPAGE_MASKS"),
    ] {
        let over = u8::try_from(limit + 1).unwrap();
        assert_eq!(check(&header_with(at, over)), Err(Fault::Limit(name)));
    }
    let mut table = Box::new(Table::new());
    for _ in 0..MAX_VARIABLES {
        table.variables.push(Variable { min: 0, max: 1, initial: 0 }).unwrap();
    }
    for _ in 0..MAX_TIMERS {
        table.timers.push(true).unwrap();
    }
    for _ in 0..MAX_RECTS {
        table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
    }
    for _ in 0..MAX_RULES {
        table.rules.push(rule(Trigger::Open)).unwrap();
    }
    for sub in 0..MAX_SUBPAGE_MASKS {
        table.subpages.push(SubpageMask { sub: u16::try_from(sub).unwrap(), off: u64::MAX }).unwrap();
    }
    let body = bytes_of(&table);
    assert!(check(&body).unwrap().has_table);
    assert!(table.rules.push(rule(Trigger::Open)).is_err(), "a 65th rule has no place");
}

/// RULE-42 (REQ-6526): each declaration's bounds.
#[test]
fn declarations_are_bounded() {
    let with = |change: &dyn Fn(&mut Table)| {
        let mut table = Box::new(Table::new());
        table.rules.push(rule(Trigger::Open)).unwrap();
        change(&mut table);
        refused(&table)
    };
    for bad in [Variable { min: 1, max: 0, initial: 0 }, Variable { min: 0, max: 5, initial: 6 }] {
        assert_eq!(with(&|table| _ = table.variables.push(bad)), Fault::Range("variable"));
    }
    for bad in [Rect { x: 0, y: 0, width: 0, height: 1 }, Rect { x: 0, y: 0, width: 1, height: 0 }] {
        assert_eq!(with(&|table| _ = table.rects.push(bad)), Fault::Range("rectangle"));
    }
    let mask = |sub, off| move |table: &mut Table| _ = table.subpages.push(SubpageMask { sub, off });
    assert_eq!(with(&mask(0x3F7F, 0)), Fault::Range("sub-code"));
    assert_eq!(with(&mask(0x0080, 0)), Fault::Range("sub-code"));
    assert_eq!(with(&mask(0, 0b10)), Fault::Reference("rule in a mask"));
    assert_eq!(
        with(&|table| {
            mask(5, 1)(table);
            mask(5, 0)(table);
        }),
        Fault::Range("sub-code repeated")
    );
    // A timer's reserved bits, which no decoded table can hold.
    let mut body = bytes_of(&{
        let mut table = Table::new();
        table.timers.push(true).unwrap();
        table
    });
    body[HEADER_BYTES] = 3;
    assert_eq!(check(&body), Err(Fault::Reserved("timer flags")));
}

/// RULE-3, RULE-43 (REQ-6526): a rule's shape and its trigger's bounds.
#[test]
fn rules_and_triggers_are_bounded() {
    let one = |rule: Rule| {
        let mut table = Box::new(Table::new());
        table.timers.push(false).unwrap();
        table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
        table.rules.push(rule).unwrap();
        let mut out = [0_u8; MAX_TABLE_BYTES];
        encode(PAGE, 7, Some(&table), &mut out).map(|_| ())
    };
    assert_eq!(one(Rule::new(Trigger::Open)), Err(Fault::Shape("no actions and no echo")));
    let mut echo = Rule::new(Trigger::Open);
    echo.echo = true;
    assert_eq!(one(echo), Ok(()));
    assert_eq!(one(rule(Trigger::Becomes)), Err(Fault::Shape("becomes without a condition")));
    let timer = |timer, ticks| one(rule(Trigger::After { timer, ticks }));
    assert_eq!(timer(0, MAX_TIMER_TICKS), Ok(()));
    assert_eq!(timer(PAGE_TIMER, 1), Ok(()));
    assert_eq!(timer(0, MAX_TIMER_TICKS + 1), Err(Fault::Limit("MAX_TIMER_TICKS")));
    assert_eq!(timer(0, 0), Err(Fault::Limit("MAX_TIMER_TICKS")));
    assert_eq!(timer(1, 1), Err(Fault::Reference("trigger's timer")));
    let press = |target, button| one(rule(Trigger::Press { target, button }));
    assert_eq!(press(Target::PAGE, 2), Ok(()));
    assert_eq!(press(Target::PAGE, 3), Err(Fault::Range("button")));
    assert_eq!(press(Target::control(32), 0), Ok(()));
    assert_eq!(press(Target::control(33), 0), Err(Fault::Range("target")));
    assert_eq!(press(Target { kind: TargetKind::Page, number: 1 }, 0), Err(Fault::Range("target")));
    assert_eq!(press(Target::rect(0), 0), Ok(()));
    assert_eq!(press(Target::rect(1), 0), Err(Fault::Reference("rectangle")));
    assert_eq!(press(Target::object(65_535), ANY_BUTTON), Ok(()));
    // RULE-10: a set of 16 keys is accepted; a 17th has no place, and a
    // count of 17 in the record is refused.
    let full: Vec<Key> = (0..16).map(|code| Key { kind: KeyKind::Named, code }).collect();
    let mut set = keys(&full);
    assert!(set.push(full[0]).is_err());
    let mut table = Table::new();
    table.rules.push(rule(Trigger::KeyUp { keys: set, mods_mask: 0, mods_value: 0 })).unwrap();
    let mut body = bytes_of(&table);
    // Flags, kind, flags, mask, value, then the key count.
    body[HEADER_BYTES + 5] = 17;
    assert_eq!(check(&body), Err(Fault::Limit("MAX_KEYS_PER_TRIGGER")));
    let surrogate = Key { kind: KeyKind::Character, code: 0xD800 };
    let bad = Trigger::Key { keys: keys(&[surrogate]), repeat: false, mods_mask: 0, mods_value: 0 };
    assert_eq!(one(rule(bad)), Err(Fault::Range("key")));
    // Every reserved bit of a rule and of its trigger.
    let table = {
        let mut table = Table::new();
        table.rules.push(rule(Trigger::KeyUp { keys: any_key(), mods_mask: 0, mods_value: 0 })).unwrap();
        table.rules.push(rule(Trigger::Move { target: Target::PAGE, held: false })).unwrap();
        table
    };
    let body = bytes_of(&table);
    for (at, fault) in [
        (HEADER_BYTES, Fault::Reserved("rule flags")),
        (HEADER_BYTES + 2, Fault::Reserved("key_up flags")),
        (HEADER_BYTES + 18 + 5, Fault::Reserved("move flags")),
    ] {
        let mut bad = body.clone();
        bad[at] |= 0x80;
        assert_eq!(check(&bad), Err(fault));
    }
    let mut unknown = body;
    unknown[HEADER_BYTES + 1] = 17;
    assert_eq!(check(&unknown), Err(Fault::Range("trigger kind")));
}

/// A `becomes` rule of `nodes`, encoded.
fn condition_result(nodes: &[Node]) -> Result<(), Fault> {
    let mut table = Box::new(Table::new());
    table.variables.push(Variable { min: 0, max: 1, initial: 0 }).unwrap();
    table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
    let mut rule = rule(Trigger::Becomes);
    rule.condition = condition(nodes);
    table.rules.push(rule).unwrap();
    let mut out = [0_u8; MAX_TABLE_BYTES];
    encode(PAGE, 7, Some(&table), &mut out).map(|_| ())
}

/// RULE-18, RULE-44 (REQ-6526): trees at each limit; a ninth term, a 17th
/// node, a fourth level, and a count that leaves nodes over or runs short
/// are each refused whole.
#[test]
fn conditions_are_one_tree_within_their_limits() {
    let term = compare(Value::variable(0), 1);
    assert_eq!(condition_result(&[term]), Ok(()));
    // Eight terms under one `and`; and a term directly under three
    // operators.
    let mut eight = std::vec![Node::And(8)];
    eight.extend([term; 8]);
    assert_eq!(condition_result(&eight), Ok(()));
    assert_eq!(condition_result(&[Node::Not, Node::Not, Node::Not, term]), Ok(()));
    assert_eq!(
        condition_result(&[Node::Not, Node::Not, Node::Not, Node::Not, term]),
        Err(Fault::Limit("MAX_CONDITION_DEPTH"))
    );
    // Sixteen nodes: two `or`s of four terms under an `and`, with five
    // of the terms under a `not`.
    let sixteen = [
        Node::And(2),
        Node::Or(4),
        Node::Not,
        term,
        Node::Not,
        term,
        Node::Not,
        term,
        Node::Not,
        term,
        Node::Or(4),
        Node::Not,
        term,
        term,
        term,
        term,
    ];
    assert_eq!(sixteen.len(), MAX_CONDITION_NODES);
    assert_eq!(condition_result(&sixteen), Ok(()));
    // A ninth term.
    let nine = [Node::And(2), Node::Or(8), term, term, term, term, term, term, term, term, term];
    assert_eq!(condition_result(&nine), Err(Fault::Limit("MAX_CONDITION_TERMS")));
    // Nodes left over, and nodes that run short.
    assert_eq!(condition_result(&[term, term]), Err(Fault::Tree));
    assert_eq!(condition_result(&[Node::And(2), term]), Err(Fault::Tree));
    assert_eq!(condition_result(&[Node::Not]), Err(Fault::Tree));
    // A 17th node has no place in the list; its count in a record is
    // refused.
    let mut table = Table::new();
    table.variables.push(Variable { min: 0, max: 1, initial: 0 }).unwrap();
    let mut becomes = rule(Trigger::Becomes);
    becomes.condition = condition(&[term]);
    table.rules.push(becomes).unwrap();
    let mut body = bytes_of(&table);
    // Twelve of declaration, then flags and kind, then the node count.
    body[HEADER_BYTES + 12 + 2] = 17;
    assert_eq!(check(&body), Err(Fault::Limit("MAX_CONDITION_NODES")));
    // `and` and `or` take 2 to 8 children.
    assert_eq!(condition_result(&[Node::Or(1), term]), Err(Fault::Range("children of and or or")));
    let mut nine_children = std::vec![Node::Or(9)];
    nine_children.extend([term; 8]);
    assert_eq!(condition_result(&nine_children), Err(Fault::Limit("MAX_AND_OR_CHILDREN")));
    // Terms' own bounds.
    let range = |low, high| condition_result(&[Node::InRange { value: Value::variable(0), low, high }]);
    assert_eq!(range(3, 3), Ok(()));
    assert_eq!(range(4, 3), Err(Fault::Range("range")));
    let set = |list: &[i32]| condition_result(&[Node::InSet { value: Value::variable(0), members: members(list) }]);
    assert_eq!(set(&[1, 2, 3, 4, 5, 6, 7, 8]), Ok(()));
    assert_eq!(set(&[]), Err(Fault::Range("empty set")));
    // RULE-19: the first operand a control, an object or a rectangle,
    // the second the page too; never "any".
    let overlaps = |first, second| condition_result(&[Node::Overlaps(first, second)]);
    assert_eq!(overlaps(Target::object(1), Target::PAGE), Ok(()));
    assert_eq!(overlaps(Target::rect(0), Target::control(1)), Ok(()));
    assert_eq!(overlaps(Target::PAGE, Target::object(1)), Err(Fault::Range("target")));
    assert_eq!(overlaps(Target::ANY_OBJECT, Target::object(1)), Err(Fault::Range("target")));
    assert_eq!(overlaps(Target::object(1), Target::ANY_CONTROL), Err(Fault::Range("target")));
    assert_eq!(overlaps(Target::object(1), Target::rect(1)), Err(Fault::Reference("rectangle")));
}

/// RULE-23 to RULE-28, RULE-44 (REQ-6526): each space's ranges, and every
/// unused field 0.
#[test]
fn values_are_checked_against_their_spaces() {
    let ok = |value: Value| value.check(2, 1);
    assert_eq!(ok(Value::variable(1)), Ok(()));
    assert_eq!(ok(Value::variable(2)), Err(Fault::Reference("value's variable")));
    assert_eq!(ok(Value { b: 1, ..Value::variable(0) }), Err(Fault::Range("value's unused field")));
    for good in [
        Value::key(INPUT_CHARACTER, 0x1F600),
        Value::key(INPUT_NAMED, 0xFFFF_FFFF),
        Value::button(2),
        Value::pointer(POINTER_Y),
        Value::keys_down(),
        Value::timer(0, TIMER_ELAPSED),
        Value::timer(PAGE_TIMER, TIMER_RUNNING),
        Value::target(TargetKind::Control, 32, TARGET_INSIDE),
        Value::target(TargetKind::Object, 65_535, TARGET_EXISTS),
        Value::sound(3, SOUND_LOCAL),
        Value::synth(3, 12, 2, 15, CLASS_BEND, 0),
        Value::synth(0, 18, 0, 0, CLASS_NOTE, 127),
        Value::synth(0, 13, 0, 0, CLASS_FIELD, 255),
    ] {
        assert_eq!(ok(good), Ok(()), "{good:?}");
    }
    for bad in [
        Value::NONE,
        Value { space: 7, ..Value::variable(0) },
        Value::key(INPUT_PHYSICAL, 0x0011_0000),
        Value::button(3),
        Value::pointer(3),
        Value { b: 1, ..Value::keys_down() },
        Value { a: 7, ..Value::keys_down() },
        Value::timer(0, 2),
        Value::target(TargetKind::Control, 33, TARGET_SHOWN),
        Value::target(TargetKind::Control, 0, TARGET_SHOWN),
        Value::target(TargetKind::Page, 0, TARGET_SHOWN),
        Value::target(TargetKind::Rect, 1, TARGET_SHOWN),
        Value::target(TargetKind::Object, 1, 10),
        Value::sound(4, SOUND_PLAYING),
        Value::sound(0, 5),
        Value::synth(4, 14, 0, 0, CLASS_NOTES, 0),
        Value::synth(0, 11, 0, 0, CLASS_NOTES, 0),
        Value::synth(0, 19, 0, 0, CLASS_NOTES, 0),
        Value::synth(0, 14, 3, 0, CLASS_NOTES, 0),
        Value::synth(0, 14, 0, 16, CLASS_NOTES, 0),
        Value::synth(0, 14, 0, 0, 10, 0),
        Value::synth(0, 14, 0, 0, CLASS_NOTE, 128),
        Value::synth(0, 14, 0, 0, CLASS_FIELD, 256),
        Value::synth(0, 14, 0, 0, CLASS_PROGRAM, 1),
    ] {
        assert!(matches!(ok(bad), Err(Fault::Range(_))), "{bad:?}");
    }
    assert_eq!(ok(Value::timer(1, TIMER_RUNNING)), Err(Fault::Reference("value's timer")));
    let value = Value::synth(2, 14, 1, 9, CLASS_VELOCITY, 60);
    assert_eq!(value.to_bytes(), [6, 2, 14, 1, 9, 3, 0, 60]);
    assert_eq!(Value::from_bytes(&value.to_bytes()), Ok(value));
    assert_eq!((value.family(), value.variant(), value.part(), value.class()), (14, 1, 9, CLASS_VELOCITY));
    assert_eq!(Value::key(INPUT_CHARACTER, 0x0001_F600).to_bytes(), [2, 1, 0, 1, 0xF6, 0, 0, 0]);
}

/// A table of one `open` rule with `action`, encoded.
fn action_result(action: Action) -> Result<(), Fault> {
    let mut table = Box::new(Table::new());
    table.variables.push(Variable { min: 0, max: 1, initial: 0 }).unwrap();
    table.timers.push(false).unwrap();
    let mut rule = Rule::new(Trigger::Open);
    rule.actions.push(action).unwrap();
    table.rules.push(rule).unwrap();
    let mut out = [0_u8; MAX_TABLE_BYTES];
    encode(PAGE, 7, Some(&table), &mut out).map(|_| ())
}

/// RULE-20, RULE-21, RULE-45 (REQ-6526): each action's fields at and
/// beyond their bounds.
#[test]
fn actions_are_bounded() {
    let midi = |status, data1, data2| action_result(Action::Midi { channel: 0, status, data1, data2 });
    for status in [0x80, 0x8F, 0x90, 0xB3, 0xE0] {
        assert_eq!(midi(status, 127, 127), Ok(()), "{status:#x}");
    }
    assert_eq!(midi(0xC0, 127, 0), Ok(()));
    assert_eq!(midi(0xC0, 1, 1), Err(Fault::Range("midi data")), "a program change's second byte is 0");
    assert_eq!(midi(0x90, 128, 0), Err(Fault::Range("midi data")));
    assert_eq!(midi(0x90, 0, 128), Err(Fault::Range("midi data")));
    // Polyphonic and channel pressure, system exclusive, system common
    // and real-time, and a data byte in place of a status.
    for status in [0xA0, 0xD0, 0xF0, 0xF1, 0xF8, 0xFF, 0x00, 0x7F] {
        assert_eq!(midi(status, 0, 0), Err(Fault::Range("midi status")), "{status:#x}");
    }
    assert_eq!(
        action_result(Action::Midi { channel: 4, status: 0x90, data1: 0, data2: 0 }),
        Err(Fault::Range("audio channel"))
    );
    let play = |gain, fade_ms| {
        action_result(Action::Play { channel: 3, handle: 0, generation: 0, gain, fade_ms, looped: false })
    };
    assert_eq!(play(100, 10_000), Ok(()));
    assert_eq!(play(101, 0), Err(Fault::Range("gain")));
    assert_eq!(play(0, 10_001), Err(Fault::Range("fade")));
    assert_eq!(action_result(Action::Stop { channel: 0, fade_ms: 10_001 }), Err(Fault::Range("fade")));
    assert_eq!(action_result(Action::Gain { channel: 0, gain: 101, fade_ms: 0 }), Err(Fault::Range("gain")));
    assert_eq!(action_result(Action::Set { variable: 0, value: i32::MAX }), Ok(()));
    assert_eq!(action_result(Action::Set { variable: 1, value: 0 }), Err(Fault::Reference("action's variable")));
    assert_eq!(action_result(Action::Add { variable: 1, amount: 0 }), Err(Fault::Reference("action's variable")));
    assert_eq!(action_result(Action::StartTimer(0)), Ok(()));
    assert_eq!(action_result(Action::StartTimer(1)), Err(Fault::Reference("action's timer")));
    assert_eq!(action_result(Action::StopTimer(PAGE_TIMER)), Err(Fault::Reference("action's timer")));
    assert_eq!(action_result(Action::Enable(1)), Ok(()));
    assert_eq!(action_result(Action::Enable(0b10)), Err(Fault::Reference("rule in a mask")));
    assert_eq!(action_result(Action::Disable(1 << 63)), Err(Fault::Reference("rule in a mask")));
    assert_eq!(action_result(Action::Send(Some(Value::variable(0)))), Ok(()));
    assert!(action_result(Action::Send(Some(Value::variable(1)))).is_err());
    // Actions on controls and objects: show, hide and look take either;
    // the rest an object; never the page, a rectangle or "any".
    let on = |target, kind| action_result(animate(target, kind));
    let cycle = |first, last, period| OperationKind::Cycle { first, last, period, once: false };
    assert_eq!(on(Target::control(32), OperationKind::Look(3)), Ok(()));
    assert_eq!(on(Target::control(1), OperationKind::Look(4)), Err(Fault::Range("operation's field")));
    assert_eq!(on(Target::control(1), OperationKind::Frame(0)), Err(Fault::Range("operation's target")));
    for target in [Target::PAGE, Target::rect(0), Target::ANY_OBJECT, Target::ANY_CONTROL, Target::control(33)] {
        assert_eq!(on(target, OperationKind::Show), Err(Fault::Range("operation's target")));
    }
    let object = Target::object(9);
    assert_eq!(on(object, OperationKind::Frame(MAX_FRAME)), Ok(()));
    assert_eq!(on(object, OperationKind::Frame(MAX_FRAME + 1)), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, cycle(0, 15, MAX_CYCLE_PERIOD)), Ok(()));
    assert_eq!(on(object, cycle(0, 15, MAX_CYCLE_PERIOD + 1)), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, cycle(0, 15, 0)), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, cycle(3, 2, 1)), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, cycle(0, 16, 1)), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, OperationKind::Order { layer: 3, order: 127 }), Ok(()));
    assert_eq!(on(object, OperationKind::Order { layer: 4, order: 0 }), Err(Fault::Range("operation's field")));
    assert_eq!(on(object, OperationKind::Reset), Err(Fault::Range("operation's field")), "reset is a record's");
    // A 17th action, an unknown action and `send`'s reserved bytes.
    let mut table = Table::new();
    table.rules.push(rule(Trigger::Open)).unwrap();
    let body = bytes_of(&table);
    let action_count = HEADER_BYTES + 3;
    let mut seventeen = body.clone();
    seventeen[action_count] = 17;
    assert_eq!(check(&seventeen), Err(Fault::Limit("MAX_ACTIONS")));
    for code in [0, 21, 255] {
        let mut unknown = body.clone();
        unknown[action_count + 1] = code;
        assert_eq!(check(&unknown), Err(Fault::Range("action kind")), "{code}");
    }
    let mut what = body.clone();
    what[action_count + 2] = 2;
    assert_eq!(check(&what), Err(Fault::Range("send's what")));
    let mut value = body;
    value[action_count + 3] = 1;
    assert_eq!(check(&value), Err(Fault::Reserved("send's value")));
}

/// RULE-57 (REQ-6526): the limits on what a whole table reads and names,
/// each at its limit and one over; and `MAX_TABLE_BYTES`.
#[test]
fn the_table_wide_limits_hold() {
    // Each rule holds 16 `send`s of a value: four rules read 64 values.
    let reading = |count: u16, value: &dyn Fn(u16) -> Value| {
        let mut table = Box::new(Table::new());
        let mut rule = Rule::new(Trigger::Open);
        for number in 0..count {
            if rule.actions.len() == MAX_ACTIONS {
                table.rules.push(rule).unwrap();
                rule = Rule::new(Trigger::Open);
            }
            rule.actions.push(Action::Send(Some(value(number)))).unwrap();
        }
        table.rules.push(rule).unwrap();
        let mut out = [0_u8; MAX_TABLE_BYTES];
        encode(PAGE, 7, Some(&table), &mut out).map(|_| Index::of(&table).unwrap())
    };
    let note = |key: u16| Value::synth(0, 14, 0, 0, CLASS_NOTE, key);
    assert_eq!(reading(64, &note).unwrap().values.len(), MAX_TABLE_VALUES);
    assert_eq!(reading(65, &note), Err(Fault::Limit("MAX_TABLE_VALUES")));
    // The same value read twice is one value.
    assert_eq!(reading(80, &|_| note(1)).unwrap().values.len(), 1);
    let shown = |number: u16| Value::target(TargetKind::Object, number + 1, TARGET_SHOWN);
    assert_eq!(reading(64, &shown).unwrap().targets.len(), MAX_TABLE_TARGETS);
    assert_eq!(reading(65, &shown), Err(Fault::Limit("MAX_TABLE_TARGETS")));
    // Metered values: 16 parts' envelope stages and levels are 32 slots.
    let metered = |number: u16| {
        let part = u8::try_from(number / 2).unwrap();
        Value::synth(
            1,
            14,
            0,
            part,
            if number.is_multiple_of(2) { CLASS_ENVELOPE_STAGE } else { CLASS_ENVELOPE_LEVEL },
            0,
        )
    };
    let index = reading(32, &metered).unwrap();
    assert_eq!(index.watch.len(), MAX_WATCHED);
    assert_eq!(index.watch[1], WatchSlot { channel: 1, kind: WATCH_ENVELOPE_LEVEL, part: 0 });
    let one_more = |number: u16| if number < 32 { metered(number) } else { Value::sound(0, SOUND_LEVEL) };
    assert_eq!(reading(33, &one_more), Err(Fault::Limit("MAX_WATCHED")));
    // Two families' readings of one part's voices share its slot.
    let families = |number: u16| Value::synth(1, 14 + u8::try_from(number).unwrap(), 0, 0, CLASS_VOICES, 0);
    assert_eq!(reading(2, &families).unwrap().watch.len(), 1);
    // A body of 8,185 bytes is refused before it is read, and so is a
    // table that would encode to more than 8,184.
    assert_eq!(check(&[0_u8; MAX_TABLE_BYTES + 1]), Err(Fault::Limit("MAX_TABLE_BYTES")));
    let mut big = Box::new(Table::new());
    let mut heavy = Rule::new(Trigger::Open);
    for _ in 0..MAX_ACTIONS {
        heavy.actions.push(Action::Enable(1)).unwrap();
    }
    for _ in 0..MAX_RULES {
        big.rules.push(heavy).unwrap();
    }
    // 64 rules of 16 nine-byte actions pass 8,184 bytes.
    assert_eq!(refused(&big), Fault::Limit("MAX_TABLE_BYTES"));
}

/// RULE-33 item 1, RULE-52 item 1 (REQ-6526): the watch list and the
/// interest mask of a table.
#[test]
fn a_table_says_what_it_watches_and_what_input_it_needs() {
    let mut table = Box::new(Table::new());
    let mut hot = rule(Trigger::Becomes);
    hot.condition = condition(&[
        Node::And(3),
        Node::Compare { value: Value::sound(0, SOUND_LEVEL), operator: Operator::Greater, constant: 200 },
        compare(Value::synth(1, 14, 0, 2, CLASS_ENVELOPE_STAGE, 0), 3),
        compare(Value::synth(1, 14, 0, 2, CLASS_NOTES, 0), 1),
    ]);
    hot.actions.push(Action::Send(Some(Value::sound(0, SOUND_LEVEL)))).unwrap();
    hot.actions.push(Action::Send(Some(Value::synth(1, 14, 0, 0, CLASS_VOICES, 0)))).unwrap();
    table.rules.push(hot).unwrap();
    let index = Index::of(&table).unwrap();
    assert_eq!(
        index.watch.as_slice(),
        [
            WatchSlot { channel: 0, kind: WATCH_LEVEL, part: 0 },
            WatchSlot { channel: 1, kind: WATCH_ENVELOPE_STAGE, part: 2 },
            WatchSlot { channel: 1, kind: WATCH_VOICES, part: 0 },
        ]
    );
    assert_eq!(index.values.len(), 4);
    assert_eq!(index.interest, 0, "nothing of the viewer's is needed");
    assert_eq!(index.watch_place(&Value::synth(1, 15, 1, 2, CLASS_ENVELOPE_STAGE, 0)), Some(1));
    // Each trigger and each input-state value asks for its class.
    let needs = |trigger: Trigger, value: Option<Value>| {
        let mut table = Box::new(Table::new());
        let mut rule = rule(trigger);
        if let Some(value) = value {
            rule.condition = condition(&[compare(value, 1)]);
        }
        table.rules.push(rule).unwrap();
        Index::of(&table).unwrap().interest
    };
    let key = Trigger::KeyUp { keys: any_key(), mods_mask: 0, mods_value: 0 };
    assert_eq!(needs(key, None), INTEREST_KEYS);
    assert_eq!(needs(Trigger::Text, None), INTEREST_TEXT);
    assert_eq!(needs(Trigger::Press { target: Target::PAGE, button: 0 }, None), INTEREST_POINTER_BUTTONS);
    assert_eq!(needs(Trigger::Enter { target: Target::PAGE }, None), INTEREST_MOVES);
    assert_eq!(needs(Trigger::Wheel { target: Target::PAGE }, None), INTEREST_WHEEL);
    assert_eq!(needs(Trigger::Blur, None), INTEREST_FOCUS);
    assert_eq!(needs(Trigger::Open, None), 0);
    assert_eq!(needs(Trigger::Open, Some(Value::keys_down())), INTEREST_KEYS);
    assert_eq!(needs(Trigger::Open, Some(Value::button(0))), INTEREST_POINTER_BUTTONS);
    assert_eq!(needs(Trigger::Open, Some(Value::pointer(POINTER_X))), INTEREST_MOVES);
    let inside = Value::target(TargetKind::Object, 5, TARGET_INSIDE);
    assert_eq!(needs(Trigger::Text, Some(inside)), INTEREST_TEXT | INTEREST_MOVES);
}

/// RULE-70 (REQ-6526): each single-byte corruption of a valid table
/// either decodes to a valid table, whose encoding is those bytes, or is
/// refused whole, leaving nothing.
#[test]
fn a_corrupted_table_is_valid_or_refused_whole() {
    let body = bytes_of(&every_shape());
    let mut table = Box::new(Table::new());
    let mut refused = 0_usize;
    for at in 0..body.len() {
        for change in [0x01, 0x80, 0xFF] {
            let mut bad = body.clone();
            bad[at] ^= change;
            if let Ok(header) = decode(&bad, &mut table) {
                let mut out = [0_u8; MAX_TABLE_BYTES];
                let length = encode(header.page, header.generation, Some(&table), &mut out).unwrap();
                assert_eq!(out[..length], bad[..], "byte {at}");
            } else {
                refused += 1;
                assert_eq!(*table, Table::new(), "nothing of a refused table is kept");
            }
        }
    }
    assert!(refused > body.len(), "most corruptions are refused: {refused}");
    // Every truncation is refused.
    for length in 0..body.len() {
        assert!(check(&body[..length]).is_err(), "{length} bytes");
    }
}

/// RULE-17 (REQ-6526): the rules a sub-page switches off, and the rules
/// enabled at opening.
#[test]
fn a_table_gives_its_masks() {
    let table = every_shape();
    assert_eq!(table.off_for(0x0002), 0b101);
    assert_eq!(table.off_for(0x0003), 0);
    // Rule 15, the `becomes` rule, is declared disabled.
    assert_eq!(table.enabled_at_opening(), (1 << 18) - 1 - (1 << 15));
}
