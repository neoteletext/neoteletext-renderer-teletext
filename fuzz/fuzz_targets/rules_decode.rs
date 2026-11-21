//! A table's image (RULE-70, RULE-83; REQ-6526, REQ-6608, REQ-TST-040):
//! any bytes decode to a table within every limit or are refused whole;
//! what decodes encodes to the same bytes and decodes to the same table;
//! and no accepted table has a ring of groups, a run over
//! `MAX_RUN_COMMANDS` commands or a command RULE-119 forbids. A `painted`
//! trigger (code 22, DEC-7031 item 4) of an accepted table names the
//! page's whole picture or one drawing for every row, and the table's
//! index holds every drawing so named, at most `MAX_PAINT_WATCHED`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::command;
use neoteletext_rules::limits::{
    MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES, MAX_CONDITION_NODES, MAX_GROUP_COMMANDS, MAX_GROUPS, MAX_GROUPS_BYTES,
    MAX_KEYS_PER_TRIGGER, MAX_PAINT_WATCHED, MAX_RECTS, MAX_ROWS, MAX_RULE_ROWS, MAX_RULES, MAX_RUN_COMMANDS,
    MAX_SEQUENCE_STEPS, MAX_SEQUENCES, MAX_TABLE_BYTES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES, MAX_TRIGGERS_PER_RULE,
    MAX_WATCHED,
};
use neoteletext_rules::table::{Index, Table, Target, TargetKind, Trigger, check, check_condition, decode, encode};
use neoteletext_web::records::body_check;

/// The page the image is decoded for.
const PAGE: u16 = 0x101;
const SUB: u16 = 0x0001;

fuzz_target!(|data: &[u8]| {
    let mut table = Box::new(Table::new(PAGE, SUB));
    if decode(data, PAGE, SUB, body_check, &mut table).is_err() {
        assert_eq!(*table, Table::new(PAGE, SUB), "nothing of a refused table is kept");
        assert!(check(data, PAGE, SUB, body_check).is_err());
        return;
    }
    assert_eq!(check(data, PAGE, SUB, body_check), Ok(()));
    assert!(data.len() <= MAX_TABLE_BYTES);
    assert!(table.rects.len() <= MAX_RECTS && table.rows.len() <= MAX_ROWS && table.rules.len() <= MAX_RULES);
    assert!(table.groups.len() <= MAX_GROUPS && table.sequences.len() <= MAX_SEQUENCES);
    assert!(table.groups.as_bytes().len() <= MAX_GROUPS_BYTES);
    let mut rule_rows = 0;
    for rule in table.rules.iter() {
        assert!((1..=MAX_TRIGGERS_PER_RULE).contains(&rule.triggers.len()));
        assert!(rule.condition.len() <= MAX_CONDITION_NODES && usize::from(rule.group) < table.groups.len());
        check_condition(&rule.condition).expect("a decoded condition is one tree within its limits");
        for trigger in rule.triggers.iter() {
            assert!(trigger.keys().is_none_or(|keys| keys.keys.len() <= MAX_KEYS_PER_TRIGGER));
        }
        rule_rows += rule.row_count();
    }
    assert!(rule_rows <= MAX_RULE_ROWS);
    for group in 0..table.groups.len() {
        let commands = table.groups.command_count(group);
        assert!((1..=MAX_GROUP_COMMANDS).contains(&commands));
        // RULE-120: a run's size is a constant of the table, at least the
        // group's own commands; a ring or too deep a nest would give 0.
        let run = table.run_commands(u8::try_from(group).expect("at most 64 groups"));
        assert!((commands..=MAX_RUN_COMMANDS).contains(&run), "group {group} runs {run} commands");
        for command in table.groups.commands(group) {
            assert!(command::is_allowed(command.kind), "type {} in a group", command.kind);
            assert!((1..=MAX_COMMAND_BYTES).contains(&command.body.len()));
            assert!(command.binding_count() <= MAX_COMMAND_BINDINGS);
        }
    }
    for sequence in table.sequences.iter() {
        assert!((1..=MAX_SEQUENCE_STEPS).contains(&sequence.steps.len()));
    }
    let index = Index::of(&table).expect("a decoded table is within the limits on what it reads");
    assert!(index.values.len() <= MAX_TABLE_VALUES && index.targets.len() <= MAX_TABLE_TARGETS);
    assert!(index.watch.len() <= MAX_WATCHED);
    // DEC-7031 item 4: what each `painted` trigger names for each row is
    // the page's whole picture or one drawing, and the index holds it.
    assert!(index.painted.len() <= MAX_PAINT_WATCHED && !index.painted.contains(&0));
    let mut painted = 0;
    for rule in table.rules.iter() {
        for target in rule.triggers.iter().filter_map(Trigger::painted) {
            assert!(!target.from_row || (rule.rows.is_some() && target.kind == TargetKind::Object));
            for place in 0..rule.row_count() {
                let named = target.resolve(&table.row_of(rule, place)).expect("a row gives a number");
                match named.kind {
                    TargetKind::Page => assert!(named == Target::PAGE && index.painted_page),
                    TargetKind::Object => assert!(named.number >= 1 && index.painted.contains(&named.number)),
                    TargetKind::Control | TargetKind::Rect => panic!("a painted trigger names {named:?}"),
                }
                painted += 1;
            }
        }
    }
    assert!(painted > 0 || (index.painted.is_empty() && !index.painted_page), "nothing painted is named unasked");
    // The encoding is the one encoding of its table: the bytes back.
    let mut out = vec![0_u8; MAX_TABLE_BYTES];
    let length = encode(&table, body_check, &mut out).expect("what was decoded is encoded");
    assert_eq!(&out[..length], data);
    let mut again = Box::new(Table::new(PAGE, SUB));
    assert_eq!(decode(&out[..length], PAGE, SUB, body_check, &mut again), Ok(()));
    assert_eq!(again, table);
});
