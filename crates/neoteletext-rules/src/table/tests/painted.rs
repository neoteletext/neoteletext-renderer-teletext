//! The `painted` trigger, code 22, in the table's image (DEC-7031 item 4,
//! REQ-7026): its three forms, the targets it is refused for, the
//! drawings a table may name, and what the index makes of it.

use super::*;
use crate::evaluator::Evaluator;
use crate::limits::{MAX_PAINT_WATCHED, MAX_TABLE_TARGETS, MAX_TRIGGERS_PER_RULE, value_of};

/// A table of one group, a `REPORT`, and the rows `rows`.
fn with_rows(rows: &[Row]) -> Box<Table> {
    let mut table = Box::new(Table::new(PAGE, SUB));
    report_group(&mut table);
    for row in rows {
        table.rows.push(*row).unwrap();
    }
    table
}

fn painted(target: TriggerTarget) -> Trigger {
    Trigger::Painted { target }
}

fn painted_object(number: u16) -> Trigger {
    painted(TriggerTarget::of(Target::object(number)))
}

/// A rule of `triggers`, running group 0, over rows `rows` when given.
fn rule_of(triggers: &[Trigger], rows: Option<RowRun>) -> Rule {
    let mut rule = Rule::new(triggers[0], 0);
    for trigger in &triggers[1..] {
        rule.triggers.push(*trigger).unwrap();
    }
    rule.rows = rows;
    rule
}

/// Rules that name each of `objects` in a `painted` trigger by a constant,
/// as many to a rule as a rule holds.
fn naming_painted(table: &mut Table, objects: &[u16]) {
    for chunk in objects.chunks(MAX_TRIGGERS_PER_RULE) {
        let triggers: Vec<Trigger> = chunk.iter().map(|object| painted_object(*object)).collect();
        table.rules.push(rule_of(&triggers, None)).unwrap();
    }
}

/// The image of `table` with its last `painted` trigger's target bytes
/// `from` written as `to`: the encoder refuses what the checks refuse, so
/// an image a writer could not make is made from one it could.
fn patched(table: &Table, from: [u8; 3], to: [u8; 3]) -> Vec<u8> {
    let mut bytes = image(table);
    let trigger = [trigger_code::PAINTED, from[0], from[1], from[2]];
    let at = bytes.windows(4).rposition(|window| window == trigger).unwrap();
    bytes[at + 1..at + 4].copy_from_slice(&to);
    bytes
}

/// DEC-7031 item 4 (REQ-7026): the whole picture, a drawing by a constant
/// and a drawing by the row's `p` are each code 22 and the target's three
/// bytes, and each table decodes to itself.
#[test]
fn a_painted_trigger_round_trips_in_its_page_object_and_row_forms() {
    let forms: [(Trigger, &[Row], [u8; 4]); 3] = [
        (painted(TriggerTarget::PAGE), &[], [22, 0, 0, 0]),
        (painted_object(301), &[], [22, 2, 0x01, 0x2D]),
        // Bit 7 of the kind byte: the row's `p` plus -20 (RULE-43).
        (painted(TriggerTarget::row(TargetKind::Object, -20)), &[61], [22, 0x82, 0xFF, 0xEC]),
    ];
    for (trigger, rows, bytes) in forms {
        let mut table = with_rows(rows);
        let run = (!rows.is_empty()).then_some(RowRun { first: 0, count: 1 });
        table.rules.push(rule_of(&[trigger], run)).unwrap();
        assert_eq!(table.check(body_check), Ok(()), "{trigger:?}");
        let image = image(&table);
        // The rule follows the header and the rows: its flags, group,
        // rows when it names them, one trigger, then the trigger.
        let trigger_at = HEADER_BYTES + ROW_BYTES * rows.len() + if rows.is_empty() { 3 } else { 5 };
        assert_eq!(image[trigger_at..trigger_at + 4], bytes, "{trigger:?}");
        assert_eq!(*decoded(&image).unwrap(), *table, "{trigger:?}");
        assert_eq!(decoded(&image).unwrap().rules[0].triggers[0].painted(), Some(trigger.painted().unwrap()));
    }
}

/// DEC-7031 item 4: every target but the page and one drawing is refused,
/// each with its own fault, in a table checked and in its image decoded.
#[test]
fn a_painted_trigger_is_refused_for_every_other_target() {
    let one_row = Some(RowRun { first: 0, count: 1 });
    let painted_target = Fault::Range("painted target");
    let row_number = Fault::Range("row's parameter as a target's number");
    let cases: [(&str, Trigger, Option<RowRun>, Fault); 9] = [
        ("any object", painted(TriggerTarget::of(Target::ANY_OBJECT)), None, painted_target),
        ("a control", painted(TriggerTarget::of(Target::control(1))), None, painted_target),
        ("any control", painted(TriggerTarget::of(Target::ANY_CONTROL)), None, painted_target),
        ("a rectangle", painted(TriggerTarget::of(Target::rect(0))), None, painted_target),
        (
            "the page numbered",
            painted(TriggerTarget::of(Target { kind: TargetKind::Page, number: 5 })),
            None,
            painted_target,
        ),
        ("the page from a row", painted(TriggerTarget::row(TargetKind::Page, 0)), one_row, painted_target),
        ("a control from a row", painted(TriggerTarget::row(TargetKind::Control, 0)), one_row, painted_target),
        ("a row giving 0", painted(TriggerTarget::row(TargetKind::Object, -60)), one_row, row_number),
        ("a row giving below 0", painted(TriggerTarget::row(TargetKind::Object, -61)), one_row, row_number),
    ];
    let table_of = |trigger: Trigger, rows: Option<RowRun>| {
        let mut table = with_rows(&[60]);
        table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
        table.rules.push(rule_of(&[trigger], rows)).unwrap();
        table
    };
    for (name, trigger, rows, fault) in cases {
        assert_eq!(table_of(trigger, rows).check(body_check), Err(fault), "{name}");
        // The same table's image, made from one naming drawing 1 or the
        // row's `p` plus 0.
        let target = trigger.painted().unwrap();
        let good = if target.from_row {
            TriggerTarget::row(TargetKind::Object, 0)
        } else {
            TriggerTarget::of(Target::object(1))
        };
        let from = if target.from_row {
            [FROM_ROW | TargetKind::Object.code(), 0, 0]
        } else {
            [TargetKind::Object.code(), 0, 1]
        };
        let number = target.number.to_be_bytes();
        let kind = target.kind.code() | if target.from_row { FROM_ROW } else { 0 };
        let bytes = patched(&table_of(painted(good), rows), from, [kind, number[0], number[1]]);
        assert_eq!(decoded(&bytes).err(), Some(fault), "{name}: its image");
    }
    // A row's `p` in a rule that names no rows.
    let without_rows = Fault::Shape("row parameter in a rule without rows");
    let by_row = painted(TriggerTarget::row(TargetKind::Object, 1));
    assert_eq!(table_of(by_row, None).check(body_check), Err(without_rows));
    let bytes = patched(
        &table_of(painted_object(1), None),
        [TargetKind::Object.code(), 0, 1],
        [FROM_ROW | TargetKind::Object.code(), 0, 1],
    );
    assert_eq!(decoded(&bytes).err(), Some(without_rows), "its image");
    // The greatest drawing, by a constant and by a row, is taken.
    let mut table = with_rows(&[65_534]);
    table.rules.push(rule_of(&[painted_object(u16::MAX)], None)).unwrap();
    table.rules.push(rule_of(&[painted(TriggerTarget::row(TargetKind::Object, 1))], one_row)).unwrap();
    assert_eq!(table.check(body_check), Ok(()));
}

/// DEC-7031 item 4 (RULE-57): a table's `painted` triggers name at most
/// `MAX_PAINT_WATCHED` distinct drawings, by constants or by rows; one
/// more is refused by that name, in the table and in its image.
#[test]
fn a_table_names_at_most_max_paint_watched_drawings() {
    assert_eq!(MAX_PAINT_WATCHED, 32);
    assert_eq!(value_of("MAX_PAINT_WATCHED"), Some(32), "a limit of RULE-57");
    let by_constant = |count: u16| {
        let mut table = with_rows(&[]);
        naming_painted(&mut table, &(1..=count).collect::<Vec<u16>>());
        table
    };
    let watched = u16::try_from(MAX_PAINT_WATCHED).unwrap();
    let full = by_constant(watched);
    assert_eq!(full.check(body_check), Ok(()));
    assert_eq!(Index::of(&full).unwrap().painted.as_slice(), (1..=watched).collect::<Vec<u16>>());
    assert_eq!(decoded(&image(&full)).map(|_| ()), Ok(()));
    let over = by_constant(watched + 1);
    assert_eq!(over.check(body_check), Err(Fault::Limit("MAX_PAINT_WATCHED")));
    // Its image, made from one whose last trigger names drawing 1 again.
    let mut again = by_constant(watched);
    again.rules.push(rule_of(&[painted_object(1)], None)).unwrap();
    let number = (watched + 1).to_be_bytes();
    let bytes = patched(&again, [2, 0, 1], [2, number[0], number[1]]);
    assert_eq!(decoded(&bytes).err(), Some(Fault::Limit("MAX_PAINT_WATCHED")));
    assert!(Evaluator::new(&over, 1, body_check).is_err(), "an evaluator refuses it too");

    // By rows: one rule over rows whose `p` is each drawing's number.
    let by_rows = |count: u16| {
        let rows: Vec<Row> = (1..=i32::from(count)).collect();
        let mut table = with_rows(&rows);
        let run = RowRun { first: 0, count: u8::try_from(count).unwrap() };
        table.rules.push(rule_of(&[painted(TriggerTarget::row(TargetKind::Object, 0))], Some(run))).unwrap();
        table
    };
    assert_eq!(by_rows(watched).check(body_check), Ok(()));
    assert_eq!(by_rows(watched + 1).check(body_check), Err(Fault::Limit("MAX_PAINT_WATCHED")));
    // The whole picture is no drawing, and takes no place among them.
    let mut with_page = by_constant(watched);
    with_page.rules.push(rule_of(&[painted(TriggerTarget::PAGE)], None)).unwrap();
    assert_eq!(with_page.check(body_check), Ok(()));
}

/// DEC-7031 item 4: a drawing named by a constant and again by a row, and
/// twice by constants, is one drawing of `MAX_PAINT_WATCHED`.
#[test]
fn the_same_drawing_by_a_constant_and_by_a_row_is_counted_once() {
    let watched = u16::try_from(MAX_PAINT_WATCHED).unwrap();
    // Drawings 1 to 32 by constants; 5 again by a constant, and 5 and 32
    // again by the rows 4 and 31 plus 1.
    let mut table = with_rows(&[4, 31]);
    naming_painted(&mut table, &(1..=watched).collect::<Vec<u16>>());
    table.rules.push(rule_of(&[painted_object(5)], None)).unwrap();
    let run = Some(RowRun { first: 0, count: 2 });
    table.rules.push(rule_of(&[painted(TriggerTarget::row(TargetKind::Object, 1))], run)).unwrap();
    assert_eq!(table.check(body_check), Ok(()));
    assert_eq!(Index::of(&table).unwrap().painted.as_slice(), (1..=watched).collect::<Vec<u16>>());
}

/// DEC-7031 item 4: the index lists the drawings in the order first named,
/// row by row, and says whether the whole picture is named.
#[test]
fn the_index_lists_the_painted_drawings_in_order_and_the_page() {
    let mut table = with_rows(&[40, 10]);
    table.rules.push(rule_of(&[painted_object(7), painted_object(3)], None)).unwrap();
    let run = Some(RowRun { first: 0, count: 2 });
    table.rules.push(rule_of(&[painted(TriggerTarget::row(TargetKind::Object, 2))], run)).unwrap();
    table.rules.push(rule_of(&[painted_object(7), painted_object(9)], None)).unwrap();
    let index = Index::of(&table).unwrap();
    assert_eq!(index.painted.as_slice(), [7, 3, 42, 12, 9]);
    assert!(!index.painted_page);
    table.rules.push(rule_of(&[painted(TriggerTarget::PAGE)], None)).unwrap();
    let index = Index::of(&table).unwrap();
    assert_eq!(index.painted.as_slice(), [7, 3, 42, 12, 9], "the picture is not a drawing");
    assert!(index.painted_page);
    // A table without the trigger watches nothing.
    let mut other = with_rows(&[]);
    other.rules.push(rule_of(&[Trigger::Open], None)).unwrap();
    let index = Index::of(&other).unwrap();
    assert_eq!((index.painted.as_slice(), index.painted_page), (&[][..], false));
}

/// DEC-7031 item 4: a painting's end is no input, so a table of `painted`
/// triggers asks for no class of input.
#[test]
fn a_painted_trigger_is_no_input_and_asks_for_none() {
    for trigger in [painted(TriggerTarget::PAGE), painted_object(4)] {
        assert!(!trigger.is_input(), "{trigger:?}");
        assert_eq!(trigger.code(), trigger_code::PAINTED);
    }
    assert_eq!(trigger_code::PAINTED, 22);
    let mut table = with_rows(&[]);
    table.rules.push(rule_of(&[painted(TriggerTarget::PAGE), painted_object(4)], None)).unwrap();
    assert_eq!(Index::of(&table).unwrap().interest, 0);
}

/// DEC-7031 item 4 (RULE-57): a drawing a `painted` trigger names by a
/// constant is one of the table's `MAX_TABLE_TARGETS` targets; one named
/// by a row is not.
#[test]
fn a_constant_painted_drawing_counts_toward_max_table_targets() {
    let pressing = |count: usize| {
        let mut table = with_rows(&[1000]);
        let objects: Vec<u16> = (1..=u16::try_from(count).unwrap()).collect();
        for chunk in objects.chunks(MAX_TRIGGERS_PER_RULE) {
            let triggers: Vec<Trigger> = chunk
                .iter()
                .map(|object| Trigger::Press { target: TriggerTarget::of(Target::object(*object)), button: 0 })
                .collect();
            table.rules.push(rule_of(&triggers, None)).unwrap();
        }
        table
    };
    let mut full = pressing(MAX_TABLE_TARGETS - 1);
    full.rules.push(rule_of(&[painted_object(500)], None)).unwrap();
    assert_eq!(full.check(body_check), Ok(()));
    assert_eq!(Index::of(&full).unwrap().targets.as_slice().last(), Some(&Target::object(500)));
    let mut over = pressing(MAX_TABLE_TARGETS);
    over.rules.push(rule_of(&[painted_object(500)], None)).unwrap();
    assert_eq!(over.check(body_check), Err(Fault::Limit("MAX_TABLE_TARGETS")));
    // By a row it names no target of the list.
    let mut by_row = pressing(MAX_TABLE_TARGETS);
    let run = Some(RowRun { first: 0, count: 1 });
    by_row.rules.push(rule_of(&[painted(TriggerTarget::row(TargetKind::Object, 0))], run)).unwrap();
    assert_eq!(by_row.check(body_check), Ok(()));
    assert_eq!(Index::of(&by_row).unwrap().painted.as_slice(), [1000]);
}
