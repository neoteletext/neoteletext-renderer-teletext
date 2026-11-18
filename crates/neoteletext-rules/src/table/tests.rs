//! The table's image (REQ-6526, REQ-6612, REQ-6613; RULE-41 to RULE-45,
//! RULE-53, RULE-70).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, plain arithmetic on small numbers and `panic!` on outcomes it rules out; and a test that holds one requirement at each of its bounds is one long list"
)]

mod limits;

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::command::{Binding, Command, Operand, Source, StartMode, TYPE_REPORT, VariableOperation, field};
use crate::limits::{MAX_TABLE_BYTES, PAGE_TIMER};
use crate::testing::{PAGE, SUB, animated, body_check, built, control, group};

pub(super) fn image(table: &Table) -> Vec<u8> {
    let mut out = std::vec![0_u8; MAX_TABLE_BYTES];
    let length = encode(table, body_check, &mut out).unwrap();
    out.truncate(length);
    out
}

pub(super) fn decoded(image: &[u8]) -> Result<Box<Table>, Fault> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    decode(image, PAGE, SUB, body_check, &mut table).map(|()| table)
}

/// A group of one `REPORT` of the firing.
pub(super) fn report_group(table: &mut Table) -> u8 {
    group(table, &[built(Command::Report { value: None })])
}

/// A table with every part: rectangles, rows, a rule of every trigger
/// kind, a condition of every node, groups of every command with
/// bindings, and sequences.
pub(super) fn everything() -> Box<Table> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    table.rects.push(Rect { x: 40, y: 300, width: 200, height: 100 }).unwrap();
    table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
    table.rows.push(60).unwrap();
    table.rows.push(62).unwrap();
    let note = group(
        &mut table,
        &[
            built(Command::LocalMidi { channel: 0, status: 0x90, data1: 0, data2: 100 })
                .bound(Binding::of(field::MIDI_DATA1, Source::Parameter)),
            animated(&[Operation { target: Target::object(41), kind: OperationKind::Look(2) }])
                .bound(Binding::of_operation(0, field::TARGET, Source::Parameter)),
            control(1, 3, 0, 50, 100).bound(Binding::of(field::CONTROL_GAIN, Source::Variable(4))),
        ],
    );
    let housekeeping = group(
        &mut table,
        &[
            built(Command::Variable {
                operation: VariableOperation::Set,
                variable: 5,
                operand: Operand::Fired { constant: 0, property: 0 },
            }),
            built(Command::Timer { start: true, timer: 3 }),
            built(Command::Report { value: Some(Value::variable(5)) }),
            built(Command::Run { group: 0 }),
            built(Command::Sequence { start: true, sequence: 0, mode: StartMode::Beat { timer: 3, ticks: 8 } }),
            built(Command::Sequence { start: true, sequence: 0, mode: StartMode::At(90) }),
            built(Command::Sequence { start: false, sequence: 255, mode: StartMode::Next }),
        ],
    );
    let report = report_group(&mut table);
    let keys = KeyMatch {
        keys: {
            let mut keys = any_key();
            keys.push(Key::of(KeyKind::Character, 'a' as u32)).unwrap();
            keys.push(Key::of(KeyKind::Named, 57_376)).unwrap();
            keys.push(Key::of(KeyKind::Physical, 'q' as u32)).unwrap();
            keys
        },
        mods_mask: 1,
        mods_value: 1,
        by_row: false,
    };
    let object = TriggerTarget::of(Target::object(301));
    let triggers = [
        Trigger::Key { keys, repeat: true },
        Trigger::KeyUp { keys: KeyMatch::any() },
        Trigger::Press { target: TriggerTarget::of(Target::rect(0)), button: 0 },
        Trigger::Release { target: TriggerTarget::of(Target::control(32)), button: 2 },
        Trigger::Enter { target: TriggerTarget::of(Target::ANY_CONTROL) },
        Trigger::Leave { target: TriggerTarget::of(Target::ANY_OBJECT) },
        Trigger::Open,
        Trigger::Close,
        Trigger::Every { timer: PAGE_TIMER, ticks: 30 },
        Trigger::After { timer: 7, ticks: 216_000 },
        Trigger::Text,
        Trigger::Move { target: TriggerTarget::PAGE, held: true },
        Trigger::Wheel { target: object },
        Trigger::Focus,
        Trigger::Blur,
        Trigger::Cue { channel: 3, number: 255 },
    ];
    for trigger in triggers {
        table.rules.push(Rule::new(trigger, report)).unwrap();
    }
    // Several triggers and rows: a key of each row by place, or a press on
    // drawing 41 plus the row's `p`.
    let by_row = KeyMatch::by_row(&[Key::of(KeyKind::Physical, 'a' as u32), Key::of(KeyKind::Physical, 's' as u32)]);
    let mut both = Rule::new(Trigger::Key { keys: by_row, repeat: false }, note);
    both.triggers.push(Trigger::Press { target: TriggerTarget::row(TargetKind::Object, 41), button: 0 }).unwrap();
    both.rows = Some(RowRun { first: 0, count: 2 });
    both.condition =
        condition(&[Node::Compare { value: Value::variable(0), operator: Operator::Less, constant: 10, plus_p: true }]);
    table.rules.push(both).unwrap();
    // `becomes`, with a condition of every node.
    let mut becomes = Rule::new(Trigger::Becomes, housekeeping);
    let mut members = SetMembers::new(0);
    members.push(-1).unwrap();
    members.push(7).unwrap();
    becomes.condition = condition(&[
        Node::And(3),
        Node::Or(2),
        Node::compare(Value::sound(0, SOUND_LEVEL), Operator::GreaterOrEqual, 128),
        Node::InRange { value: Value::timer(PAGE_TIMER, TIMER_ELAPSED), low: 0, high: 600 },
        Node::Not,
        Node::InSet { value: Value::sequence(0, SEQUENCE_PASSES), members },
        Node::And(2),
        Node::Overlaps(Target::object(301), Target::rect(1)),
        Node::Inside(Target::control(1), Target::PAGE),
    ]);
    table.rules.push(becomes).unwrap();
    let mut pulse = Sequence::new(30, 0);
    pulse.steps.push(Step { offset: 0, group: note }).unwrap();
    pulse.steps.push(Step { offset: 15, group: report }).unwrap();
    pulse.steps.push(Step { offset: 15, group: note }).unwrap();
    table.sequences.push(pulse).unwrap();
    table
}

/// RULE-41 to RULE-45 (REQ-6526): a table of every part round-trips byte
/// for byte, its header is the twelve bytes of RULE-41, and the image
/// holds neither the page nor the sub-code.
#[test]
fn a_table_round_trips() {
    let table = everything();
    let bytes = image(&table);
    assert_eq!(*decoded(&bytes).unwrap(), *table);
    assert_eq!(image(&decoded(&bytes).unwrap()), bytes);
    let groups = u16::try_from(table.groups.as_bytes().len()).unwrap().to_be_bytes();
    assert_eq!(bytes[..HEADER_BYTES], [2, 0, 2, 2, 18, 3, 1, 0, groups[0], groups[1], 0, 0]);
    assert_eq!(check(&bytes, PAGE, SUB, body_check), Ok(()));
    // The same image for another page is another table's: its commands
    // name this page, so it is refused there (RULE-119).
    assert_eq!(check(&bytes, PAGE, SUB + 1, body_check), Err(Fault::Command("names another page")));
    assert_eq!(check(&bytes, PAGE + 1, SUB, body_check), Err(Fault::Command("names another page")));
    // What it reads and names (RULE-47).
    let index = Index::of(&table).unwrap();
    assert_eq!(index.targets.as_slice(), [Target::control(32), Target::object(301), Target::control(1)]);
    assert_eq!(index.values.len(), 5);
    assert_eq!(index.watch.as_slice(), [WatchSlot { channel: 0, kind: WATCH_LEVEL, part: 0 }]);
    assert_eq!(
        index.interest,
        INTEREST_KEYS | INTEREST_TEXT | INTEREST_POINTER_BUTTONS | INTEREST_MOVES | INTEREST_WHEEL | INTEREST_FOCUS
    );
    assert_eq!(table.run_commands(1), 7 + 3);
    assert_eq!((table.run_reports(1), table.run_reports(2)), (1, 1), "its own report; the RUN of group 0 holds none");
}

/// RULE-42, RULE-43, RULE-45 (REQ-6526): the bytes of a rectangle, a
/// row, a rule with rows and two triggers, a group's frame with a
/// binding, and a sequence.
#[test]
fn the_parts_are_their_bytes() {
    let mut table = Table::new(PAGE, SUB);
    table.rects.push(Rect { x: 40, y: 300, width: 200, height: 100 }).unwrap();
    table.rows.push(61).unwrap();
    let note = group(
        &mut table,
        &[built(Command::LocalMidi { channel: 0, status: 0x90, data1: 0, data2: 100 })
            .bound(Binding::of(field::MIDI_DATA1, Source::Parameter))],
    );
    let keys = KeyMatch::by_row(&[Key::of(KeyKind::Physical, 'a' as u32)]);
    let mut rule = Rule::new(Trigger::Key { keys, repeat: true }, note);
    rule.triggers.push(Trigger::Press { target: TriggerTarget::row(TargetKind::Object, -20), button: 0 }).unwrap();
    rule.rows = Some(RowRun { first: 0, count: 1 });
    table.rules.push(rule).unwrap();
    let mut sequence = Sequence::new(12, 2);
    sequence.steps.push(Step { offset: 4, group: note }).unwrap();
    table.sequences.push(sequence).unwrap();
    let expected: Vec<u8> = [
        // The header: format 2, one of each, sixteen bytes of groups.
        &[2, 0, 1, 1, 1, 1, 1, 0, 0, 16, 0, 0][..],
        // The rectangle.
        &[0, 40, 0x01, 0x2C, 0, 200, 0, 100],
        // The row: its one parameter `p`, 61 (DEC-6635 item 8).
        &[0, 0, 0, 61],
        // The rule: it names rows; group 0; row 0, one row; two triggers.
        &[1, 0, 0, 1, 2],
        // `key`: repeat and one key a row (bit 1); no modifiers; one
        // physical key, `a`.
        &[1, 3, 0, 0, 1, 3, 0, 0, 0, 97],
        // `press`: an object, -20 plus the row's `p`; button 0.
        &[3, 0x82, 0xFF, 0xEC, 0],
        // No condition.
        &[0],
        // The group: one command; type 28, one binding, eight bytes.
        &[1, 28, 1, 0, 8, 0x01, 0x01, 0x00, 0x01, 0, 0x90, 0, 100],
        // The binding: item 0, data 1, the row's `p`.
        &[0, 1, 0x20],
        // The sequence: two passes, one step, twelve ticks; offset 4,
        // group 0.
        &[2, 1, 0, 0, 0, 12, 0, 0, 0, 4, 0],
    ]
    .concat();
    assert_eq!(image(&table), expected);
    assert_eq!(*decoded(&expected).unwrap(), table);
}

/// RULE-53 (REQ-6526): the default table is one group, a `REPORT` of the
/// firing, and twelve rules that each run it, or thirteen with the
/// wheel; its image is the same bytes every time.
#[test]
fn the_default_table_reports_every_input() {
    let mut table = Box::new(Table::new(PAGE, SUB));
    default_send_all(false, PAGE, SUB, &mut table);
    assert_eq!((table.rules.len(), table.groups.len(), table.groups.command_count(0)), (12, 1, 1));
    let command = table.groups.command(0, 0).unwrap();
    assert_eq!((command.kind, command.body), (TYPE_REPORT, &[1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0][..]));
    assert!(table.rules.iter().all(|rule| rule.group == 0 && rule.rows.is_none() && rule.condition.is_empty()));
    let codes: Vec<u8> = table.rules.iter().map(|rule| rule.triggers[0].code()).collect();
    assert_eq!(codes, [1, 2, 12, 3, 4, 13, 5, 6, 5, 6, 15, 16]);
    let without = image(&table);
    default_send_all(true, PAGE, SUB, &mut table);
    assert_eq!(table.rules.len(), 13);
    assert_eq!(table.rules[12].triggers[0], Trigger::Wheel { target: TriggerTarget::PAGE });
    let with = image(&table);
    assert_eq!(with.len(), without.len() + 8, "one rule of a wheel trigger more");
    assert_eq!(Index::of(&table).unwrap().interest, 0b11_1111);
    // A table that was something else is replaced whole.
    let mut other = everything();
    default_send_all(false, PAGE, SUB, &mut other);
    assert_eq!(image(&other), without);
}

/// RULE-70 (REQ-6526): each single-byte corruption of a valid image
/// either decodes to a table that encodes to exactly those bytes, or is
/// refused whole, leaving nothing.
#[test]
fn a_corrupted_image_is_refused_whole_or_is_another_table() {
    let good = image(&everything());
    let mut refused = 0;
    for at in 0..good.len() {
        for flip in [0x01, 0x80, 0xFF] {
            let mut bytes = good.clone();
            bytes[at] ^= flip;
            let mut table = Box::new(Table::new(PAGE, SUB));
            if decode(&bytes, PAGE, SUB, body_check, &mut table).is_ok() {
                assert_eq!(image(&table), bytes, "byte {at}");
            } else {
                refused += 1;
                assert_eq!(*table, Table::new(PAGE, SUB), "byte {at}: nothing is kept");
            }
        }
    }
    assert!(refused > good.len(), "most corruptions are refused");
    // Short anywhere, and a byte more.
    for cut in 0..good.len() {
        assert!(decoded(&good[..cut]).is_err(), "cut at {cut}");
    }
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(decoded(&longer).err(), Some(Fault::Trailing));
}

/// RULE-3, RULE-43, RULE-44, RULE-107 (REQ-6526, REQ-6613): the shapes
/// and the reserved bits a table is refused for.
#[test]
fn shapes_and_reserved_bits_are_refused() {
    let base = || {
        let mut table = Box::new(Table::new(PAGE, SUB));
        table.rows.push(60).unwrap();
        report_group(&mut table);
        table
    };
    let refused = |change: &dyn Fn(&mut Table)| {
        let mut table = base();
        change(&mut table);
        table.check(body_check).err()
    };
    let rule = |trigger| Rule::new(trigger, 0);
    assert_eq!(
        refused(&|table| {
            table.rules.push(rule(Trigger::Becomes)).unwrap();
        }),
        Some(Fault::Shape("becomes without a condition"))
    );
    assert_eq!(
        refused(&|table| {
            let mut becomes = rule(Trigger::Becomes);
            becomes.condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 1)]);
            becomes.triggers.push(Trigger::Open).unwrap();
            table.rules.push(becomes).unwrap();
        }),
        Some(Fault::Shape("becomes beside another trigger"))
    );
    assert_eq!(
        refused(&|table| {
            let mut none = rule(Trigger::Open);
            none.triggers.clear();
            table.rules.push(none).unwrap();
        }),
        Some(Fault::Shape("rule without a trigger"))
    );
    assert_eq!(
        refused(&|table| {
            table.rules.push(Rule::new(Trigger::Open, 1)).unwrap();
        }),
        Some(Fault::Reference("rule's group"))
    );
    // A row parameter in a rule without rows: a key, a target, a
    // comparison.
    let without_rows = Some(Fault::Shape("row parameter in a rule without rows"));
    let by_row = Trigger::Key { keys: KeyMatch::by_row(&[Key::of(KeyKind::Character, 'a' as u32)]), repeat: false };
    assert_eq!(
        refused(&|table| {
            table.rules.push(rule(by_row)).unwrap();
        }),
        Some(Fault::Shape("one key a row in a rule without rows"))
    );
    // One key a row, and not one key for each row (RULE-105).
    let two = Trigger::KeyUp {
        keys: KeyMatch::by_row(&[Key::of(KeyKind::Character, 'a' as u32), Key::of(KeyKind::Character, 'b' as u32)]),
    };
    assert_eq!(
        refused(&|table| {
            let mut ruled = rule(two);
            ruled.rows = Some(RowRun { first: 0, count: 1 });
            table.rules.push(ruled).unwrap();
        }),
        Some(Fault::Shape("one key a row, and not one key for each row"))
    );
    let press_row = Trigger::Press { target: TriggerTarget::row(TargetKind::Object, 41), button: 0 };
    assert_eq!(
        refused(&|table| {
            table.rules.push(rule(press_row)).unwrap();
        }),
        without_rows
    );
    assert_eq!(
        refused(&|table| {
            let mut compared = rule(Trigger::Open);
            compared.condition = condition(&[Node::Compare {
                value: Value::variable(0),
                operator: Operator::Equal,
                constant: 0,
                plus_p: true,
            }]);
            table.rules.push(compared).unwrap();
        }),
        without_rows
    );
    // Rows the table does not have, and none.
    let with_rows = |first, count| {
        move |table: &mut Table| {
            let mut ruled = Rule::new(Trigger::Open, 0);
            ruled.rows = Some(RowRun { first, count });
            table.rules.push(ruled).unwrap();
        }
    };
    assert_eq!(refused(&with_rows(0, 2)), Some(Fault::Reference("rule's rows")));
    assert_eq!(refused(&with_rows(1, 1)), Some(Fault::Reference("rule's rows")));
    assert_eq!(refused(&with_rows(0, 0)), Some(Fault::Range("rule's row count")));
    assert_eq!(refused(&with_rows(0, 1)), None);

    // Reserved bits and bytes of the image.
    let mut table = base();
    let mut moved = rule(Trigger::Move { target: TriggerTarget::PAGE, held: false });
    moved.triggers.push(Trigger::Key { keys: KeyMatch::any(), repeat: false }).unwrap();
    moved.triggers.push(Trigger::KeyUp { keys: KeyMatch::any() }).unwrap();
    table.rules.push(moved).unwrap();
    let good = image(&table);
    let rule_at = HEADER_BYTES + ROW_BYTES;
    let with = |at: usize, byte: u8| {
        let mut bytes = good.clone();
        bytes[at] = byte;
        decoded(&bytes).err()
    };
    assert_eq!(with(0, 1), Some(Fault::Range("format")), "the first form's format is refused");
    assert_eq!(with(1, 1), Some(Fault::Reserved("header")));
    assert_eq!(with(7, 1), Some(Fault::Reserved("header")));
    assert_eq!(with(11, 1), Some(Fault::Reserved("header")));
    assert_eq!(with(rule_at, 2), Some(Fault::Reserved("rule flags")));
    assert_eq!(with(rule_at + 2, 0), Some(Fault::Shape("rule without a trigger")));
    // `move`: its code, its target of three bytes, its flags.
    assert_eq!(with(rule_at + 7, 2), Some(Fault::Reserved("move flags")));
    assert_eq!(with(rule_at + 9, 4), Some(Fault::Reserved("key flags")));
    assert_eq!(with(rule_at + 14, 1), Some(Fault::Reserved("key_up flags")));
    assert_eq!(with(rule_at + 3, 22), Some(Fault::Range("trigger kind")));
    // `held`, `double`, `press_held` and `double_press` are withdrawn
    // (DEC-6635 item 7), their codes never reused.
    for withdrawn in 17..=20 {
        assert_eq!(with(rule_at + 3, withdrawn), Some(Fault::Range("trigger kind")), "{withdrawn}");
    }
    assert_eq!(with(rule_at + 4, 4), Some(Fault::Range("target kind")));
    assert_eq!(with(rule_at + 4, 0x81), Some(Fault::Shape("row parameter in a rule without rows")));
}
