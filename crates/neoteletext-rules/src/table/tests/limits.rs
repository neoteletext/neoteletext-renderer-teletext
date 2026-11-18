//! Every limit of RULE-57 that a table's image is held to (REQ-6608;
//! RULE-57, RULE-70, RULE-120): a table exactly at the limit is accepted,
//! and one over is refused whole with that limit's name.

use super::*;
use crate::command::RunMode;
use crate::limits::{
    MAX_AND_OR_CHILDREN, MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES, MAX_CONDITION_DEPTH, MAX_CONDITION_NODES,
    MAX_CONDITION_TERMS, MAX_CUE, MAX_DOUBLE_TICKS, MAX_GROUP_COMMANDS, MAX_GROUP_DEPTH, MAX_GROUPS, MAX_GROUPS_BYTES,
    MAX_HOLD_TICKS, MAX_KEYS_PER_TRIGGER, MAX_RECTS, MAX_ROWS, MAX_RULE_ROWS, MAX_RULES, MAX_RUN_COMMANDS,
    MAX_SEQUENCE_REPEAT, MAX_SEQUENCE_STEPS, MAX_SEQUENCE_TICKS, MAX_SEQUENCES, MAX_SET_MEMBERS, MAX_TABLE_TARGETS,
    MAX_TABLE_VALUES, MAX_TIMER_TICKS, MAX_TRIGGERS_PER_RULE, MAX_WATCHED, value_of,
};
use crate::testing::Built;

fn table() -> Box<Table> {
    Box::new(Table::new(PAGE, SUB))
}

fn checked(table: &Table) -> Result<(), Fault> {
    table.check(body_check)
}

/// The fault of the limit named `name`, which the crate knows the value
/// of (RULE-57: a refusal names the constant and its value).
fn over(name: &'static str) -> Result<(), Fault> {
    assert!(value_of(name).is_some(), "{name} is a limit of RULE-57");
    Err(Fault::Limit(name))
}

/// A table of one group, a `REPORT`, and what `fill` adds.
fn with(fill: &dyn Fn(&mut Table)) -> Box<Table> {
    let mut table = table();
    report_group(&mut table);
    fill(&mut table);
    table
}

/// The image of `table` with the byte at `at` set to `byte`, decoded.
fn with_byte(table: &Table, at: usize, byte: u8) -> Result<(), Fault> {
    let mut bytes = image(table);
    bytes[at] = byte;
    decoded(&bytes).map(|_| ())
}

fn count_of(limit: usize) -> u8 {
    u8::try_from(limit).unwrap()
}

/// RULE-57 (REQ-6608): the five lists of a table: `MAX_RECTS`,
/// `MAX_ROWS`, `MAX_RULES`, `MAX_GROUPS` and `MAX_SEQUENCES` of each are
/// accepted, and a header that counts one more is refused by that name.
#[test]
fn the_tables_lists_are_held_to_their_limits() {
    let full = with(&|table| {
        for _ in 1..MAX_GROUPS {
            report_group(table);
        }
        for _ in 0..MAX_RECTS {
            table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
        }
        for _ in 0..MAX_ROWS {
            table.rows.push([1, 2, 3, 4]).unwrap();
        }
        for _ in 0..MAX_RULES {
            table.rules.push(Rule::new(Trigger::Open, 0)).unwrap();
        }
        for _ in 0..MAX_SEQUENCES {
            let mut sequence = Sequence::new(1, MAX_SEQUENCE_REPEAT);
            sequence.steps.push(Step { offset: 0, group: 0 }).unwrap();
            table.sequences.push(sequence).unwrap();
        }
    });
    assert_eq!(checked(&full), Ok(()));
    assert_eq!(*decoded(&image(&full)).unwrap(), *full);
    // The header's counts: rectangles, rows, rules, groups, sequences.
    let counts = [
        (2, MAX_RECTS, "MAX_RECTS"),
        (3, MAX_ROWS, "MAX_ROWS"),
        (4, MAX_RULES, "MAX_RULES"),
        (5, MAX_GROUPS, "MAX_GROUPS"),
        (6, MAX_SEQUENCES, "MAX_SEQUENCES"),
    ];
    for (at, limit, name) in counts {
        assert_eq!(image(&full)[at], count_of(limit), "{name}");
        assert_eq!(with_byte(&full, at, count_of(limit + 1)), over(name));
    }
    let mut more = full.clone();
    assert!(more.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).is_err());
    assert!(more.rows.push([0; 4]).is_err());
    assert!(more.rules.push(Rule::new(Trigger::Open, 0)).is_err());
    assert!(more.sequences.push(Sequence::new(1, 1)).is_err());
    assert_eq!(more.push_group(&[]).err(), Some(Fault::Range("group with no command")));
    let body = [1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let one = [Framed { kind: TYPE_REPORT, body: &body, bindings: &[] }];
    assert_eq!(more.push_group(&one), over("MAX_GROUPS").map(|()| 0));
}

/// An `ANIMATE` command for the tests' page whose body is exactly
/// `length` bytes: moves of nine bytes, and shorter operations to make
/// up the rest.
fn animate_of_length(length: usize) -> Built {
    let object = Target::object(1);
    let on = |kind| Operation { target: object, kind };
    let (four, five, six) =
        (on(OperationKind::Show), on(OperationKind::Look(0)), on(OperationKind::Order { layer: 0, order: 0 }));
    let nine = on(OperationKind::Move { relative: false, x: 0, y: 0 });
    let operations = length - crate::animate::RECORD_FIXED;
    // What is left over nines, made of fours, fives and sixes; some take
    // a nine back.
    let (nines, rest): (usize, &[Operation]) = match operations % 9 {
        0 => (operations / 9, &[]),
        1 => (operations / 9 - 1, &[five, five]),
        2 => (operations / 9 - 1, &[five, six]),
        3 => (operations / 9 - 1, &[six, six]),
        4 => (operations / 9, &[four]),
        5 => (operations / 9, &[five]),
        6 => (operations / 9, &[six]),
        7 => (operations / 9 - 1, &[five, five, six]),
        _ => (operations / 9, &[four, four]),
    };
    let all: Vec<Operation> = core::iter::repeat_n(nine, nines).chain(rest.iter().copied()).collect();
    let built = animated(&all);
    assert_eq!(built.body.len(), length);
    built
}

fn framed(commands: &[Built]) -> Vec<Framed<'_>> {
    commands
        .iter()
        .map(|command| Framed { kind: command.kind, body: &command.body, bindings: &command.bindings })
        .collect()
}

/// RULE-118, RULE-120 (REQ-6608): `MAX_GROUP_COMMANDS` commands a group,
/// `MAX_COMMAND_BYTES` of body a command, `MAX_COMMAND_BINDINGS` bindings
/// a command and `MAX_GROUPS_BYTES` of groups a table, each accepted at
/// its bound and refused one over.
#[test]
fn a_groups_commands_bytes_and_bindings_are_held_to_their_limits() {
    let report = built(Command::Report { value: None });
    let most = std::vec![report.clone(); MAX_GROUP_COMMANDS];
    let mut table = table();
    assert_eq!(table.push_group(&framed(&most)), Ok(0));
    assert_eq!(checked(&table), Ok(()));
    let one_more = std::vec![report; MAX_GROUP_COMMANDS + 1];
    assert_eq!(table.push_group(&framed(&one_more)), over("MAX_GROUP_COMMANDS").map(|()| 0));
    // In an image, the group's count byte is the first of its bytes.
    let rules = image(&table).len() - table.groups.as_bytes().len();
    assert_eq!(with_byte(&table, rules, count_of(MAX_GROUP_COMMANDS + 1)), over("MAX_GROUP_COMMANDS"));

    let longest = animate_of_length(MAX_COMMAND_BYTES);
    let mut table = self::table();
    assert_eq!(table.push_group(&framed(core::slice::from_ref(&longest))), Ok(0));
    assert_eq!(checked(&table), Ok(()));
    let mut longer = longest.clone();
    longer.body.push(0);
    assert_eq!(table.push_group(&framed(&[longer])), over("MAX_COMMAND_BYTES").map(|()| 0));
    // The frame's length field, 16 bits after the group's count, the
    // type and the count of bindings: 256 is 0x0100, and its low byte
    // made 1 is one byte more.
    let low = image(&table).len() - table.groups.as_bytes().len() + 4;
    assert_eq!(with_byte(&table, low, 1), over("MAX_COMMAND_BYTES"));

    let moves = [Operation { target: Target::object(1), kind: OperationKind::Move { relative: false, x: 0, y: 0 } }; 5];
    let bound = |bindings: usize| {
        let mut command = animated(&moves);
        for binding in 0..bindings {
            let item = u8::try_from(binding / 2).unwrap();
            let code = if binding % 2 == 0 { field::X } else { field::Y };
            command.bindings.push(Binding::of_operation(item, code, Source::Variable(0)));
        }
        command
    };
    let mut table = self::table();
    assert_eq!(table.push_group(&framed(&[bound(MAX_COMMAND_BINDINGS)])), Ok(0));
    assert_eq!(checked(&table), Ok(()));
    assert_eq!(table.push_group(&framed(&[bound(MAX_COMMAND_BINDINGS + 1)])), over("MAX_COMMAND_BINDINGS").map(|()| 0));
    let count = image(&table).len() - table.groups.as_bytes().len() + 2;
    assert_eq!(with_byte(&table, count, count_of(MAX_COMMAND_BINDINGS + 1)), over("MAX_COMMAND_BINDINGS"));

    // All the groups together: one group of 32 commands of the longest
    // body, and a second that brings the bytes to the limit exactly.
    let frame = |body: usize| 4 + body;
    let first = 1 + MAX_GROUP_COMMANDS * frame(MAX_COMMAND_BYTES);
    let left = MAX_GROUPS_BYTES - first - 1 - (MAX_GROUP_COMMANDS - 2) * frame(MAX_COMMAND_BYTES);
    let last = (left - 2 * frame(0)) / 2;
    assert_eq!(left, 2 * frame(last), "the two last bodies are of one length");
    let exactly = |extra: usize| {
        let mut table = self::table();
        table.push_group(&framed(&std::vec![longest.clone(); MAX_GROUP_COMMANDS])).unwrap();
        let mut second = std::vec![longest.clone(); MAX_GROUP_COMMANDS - 2];
        second.push(animate_of_length(last));
        second.push(animate_of_length(last + extra));
        let pushed = table.push_group(&framed(&second));
        (table, pushed)
    };
    let (full, pushed) = exactly(0);
    assert_eq!((pushed, full.groups.as_bytes().len()), (Ok(1), MAX_GROUPS_BYTES));
    assert_eq!(checked(&full), Ok(()));
    assert_eq!(*decoded(&image(&full)).unwrap(), *full);
    assert_eq!(exactly(1).1, over("MAX_GROUPS_BYTES").map(|()| 0));
    // The header's count of the groups' bytes, one more than they are.
    let mut bytes = image(&full);
    bytes[8..10].copy_from_slice(&u16::try_from(MAX_GROUPS_BYTES + 1).unwrap().to_be_bytes());
    assert_eq!(decoded(&bytes).err(), Some(Fault::Limit("MAX_GROUPS_BYTES")));
}

/// RULE-120 (REQ-6608, REQ-6612): groups run other groups to a depth of
/// `MAX_GROUP_DEPTH` and no deeper; a group that runs itself, and two
/// that run each other, are refused; and a run whose expansion is
/// `MAX_RUN_COMMANDS` commands is accepted and one more refused.
#[test]
fn nesting_is_held_to_its_depth_its_size_and_no_ring() {
    let run = |group: u8| built(Command::Run { group, mode: RunMode::Now });
    let leaf = built(Command::Timer { start: true, timer: 0 });
    // A chain: group `n` runs group `n + 1`, and the last runs nothing.
    let chain = |length: usize| {
        let mut table = table();
        for number in 0..length {
            let next = u8::try_from(number + 1).unwrap();
            let commands = if number + 1 < length { std::vec![run(next)] } else { std::vec![leaf.clone()] };
            group(&mut table, &commands);
        }
        table
    };
    assert_eq!(checked(&chain(MAX_GROUP_DEPTH)), Ok(()));
    assert_eq!(chain(MAX_GROUP_DEPTH).run_commands(0), MAX_GROUP_DEPTH);
    assert_eq!(checked(&chain(MAX_GROUP_DEPTH + 1)), over("MAX_GROUP_DEPTH"));
    // Deep from wherever it is entered: group 1 of the longer chain is a
    // chain of four.
    assert_eq!(chain(MAX_GROUP_DEPTH + 1).run_commands(1), MAX_GROUP_DEPTH);
    assert_eq!(chain(MAX_GROUP_DEPTH + 1).run_commands(0), 0, "what would be refused has no run");

    let mut itself = table();
    group(&mut itself, &[run(0)]);
    assert_eq!(checked(&itself), Err(Fault::Ring));
    let mut two = table();
    group(&mut two, &[leaf.clone(), run(1)]);
    group(&mut two, &[run(0)]);
    assert_eq!(checked(&two), Err(Fault::Ring));
    let mut three = table();
    group(&mut three, &[run(1)]);
    group(&mut three, &[run(2)]);
    group(&mut three, &[leaf.clone(), run(0)]);
    assert_eq!(checked(&three), Err(Fault::Ring));
    // A `RUN` that arms a run is no nesting: a group may arm itself.
    let mut arms = table();
    group(&mut arms, &[built(Command::Run { group: 0, mode: RunMode::After(1) })]);
    assert_eq!(checked(&arms), Ok(()));

    // The size of a run: a group of 32 commands, three of which run a
    // group of 32, applies 32 + 3 × 32 commands.
    let sized = |extra: bool| {
        let mut table = table();
        group(&mut table, &std::vec![leaf.clone(); MAX_GROUP_COMMANDS]);
        group(&mut table, core::slice::from_ref(&leaf));
        let mut top = std::vec![run(0), run(0), run(0)];
        top.extend(extra.then(|| run(1)));
        top.resize(MAX_GROUP_COMMANDS, leaf.clone());
        group(&mut table, &top);
        table
    };
    assert_eq!(sized(false).run_commands(2), MAX_RUN_COMMANDS);
    assert_eq!(checked(&sized(false)), Ok(()));
    assert_eq!(checked(&sized(true)), over("MAX_RUN_COMMANDS"));
}

/// A trigger made of its ticks, the most ticks it may have, and the name
/// of that limit.
type Timed<'a> = (&'a dyn Fn(u32) -> Trigger, u32, &'static str);

/// RULE-9, RULE-10, RULE-43, RULE-107 (REQ-6608): a rule's triggers, a
/// trigger's keys, the ticks of each timed trigger, a cue's number and
/// the rule-and-row pairs of a table, each at its bound and one over.
#[test]
fn triggers_are_held_to_their_limits() {
    let rule_at = HEADER_BYTES;
    let of = |triggers: &[Trigger]| {
        with(&|table| {
            let mut rule = Rule::new(triggers[0], 0);
            for trigger in &triggers[1..] {
                rule.triggers.push(*trigger).unwrap();
            }
            table.rules.push(rule).unwrap();
        })
    };
    let four = of(&[Trigger::Open; MAX_TRIGGERS_PER_RULE]);
    assert_eq!(checked(&four), Ok(()));
    // The rule: its flags, its group, then its count of triggers.
    assert_eq!(with_byte(&four, rule_at + 2, count_of(MAX_TRIGGERS_PER_RULE + 1)), over("MAX_TRIGGERS_PER_RULE"));

    let keys = KeyMatch::of(&[Key::of(KeyKind::Character, 'a' as u32); MAX_KEYS_PER_TRIGGER]);
    let sixteen = of(&[Trigger::Key { keys, repeat: false }]);
    assert_eq!(checked(&sixteen), Ok(()));
    // The `key` trigger: its code, its flags, its two modifier bytes,
    // then its count of keys.
    assert_eq!(with_byte(&sixteen, rule_at + 3 + 4, count_of(MAX_KEYS_PER_TRIGGER + 1)), over("MAX_KEYS_PER_TRIGGER"));

    let target = TriggerTarget::PAGE;
    let any = KeyMatch::any();
    let timed: [Timed<'_>; 6] = [
        (&|ticks| Trigger::Every { timer: 0, ticks }, MAX_TIMER_TICKS, "MAX_TIMER_TICKS"),
        (&|ticks| Trigger::After { timer: 255, ticks }, MAX_TIMER_TICKS, "MAX_TIMER_TICKS"),
        (
            &|ticks| Trigger::Held { keys: any, ticks: u16::try_from(ticks).unwrap() },
            u32::from(MAX_HOLD_TICKS),
            "MAX_HOLD_TICKS",
        ),
        (
            &|ticks| Trigger::PressHeld { target, button: 0, ticks: u16::try_from(ticks).unwrap() },
            u32::from(MAX_HOLD_TICKS),
            "MAX_HOLD_TICKS",
        ),
        (
            &|ticks| Trigger::Double { keys: any, ticks: u16::try_from(ticks).unwrap() },
            u32::from(MAX_DOUBLE_TICKS),
            "MAX_DOUBLE_TICKS",
        ),
        (
            &|ticks| Trigger::DoublePress { target, button: 0, ticks: u16::try_from(ticks).unwrap() },
            u32::from(MAX_DOUBLE_TICKS),
            "MAX_DOUBLE_TICKS",
        ),
    ];
    for (trigger, limit, name) in timed {
        assert_eq!(checked(&of(&[trigger(limit)])), Ok(()), "{name}");
        assert_eq!(checked(&of(&[trigger(1)])), Ok(()), "{name}");
        assert_eq!(checked(&of(&[trigger(limit + 1)])), over(name));
        assert_eq!(checked(&of(&[trigger(0)])), over(name));
    }
    assert_eq!(checked(&of(&[Trigger::Cue { channel: 3, number: MAX_CUE }])), Ok(()));
    assert_eq!(checked(&of(&[Trigger::Cue { channel: 3, number: 255 }])), Ok(()));
    assert_eq!(checked(&of(&[Trigger::Cue { channel: 3, number: MAX_CUE + 1 }])), Err(Fault::Range("cue")));
    assert_eq!(checked(&of(&[Trigger::Cue { channel: 4, number: 0 }])), Err(Fault::Range("cue")));
    assert_eq!(checked(&of(&[Trigger::Every { timer: 8, ticks: 1 }])), Err(Fault::Range("trigger's timer")));

    // Two rules of 64 rows each are 128 rule-and-row pairs; a rule
    // without rows is one more.
    let rowed = |extra: bool| {
        with(&|table| {
            for _ in 0..MAX_ROWS {
                table.rows.push([0; 4]).unwrap();
            }
            let mut rule = Rule::new(Trigger::Open, 0);
            rule.rows = Some(RowRun { first: 0, count: count_of(MAX_ROWS) });
            for _ in 0..MAX_RULE_ROWS / MAX_ROWS {
                table.rules.push(rule).unwrap();
            }
            if extra {
                table.rules.push(Rule::new(Trigger::Open, 0)).unwrap();
            }
        })
    };
    assert_eq!(checked(&rowed(false)), Ok(()));
    assert_eq!(checked(&rowed(true)), over("MAX_RULE_ROWS"));
}

/// RULE-18, RULE-44 (REQ-6608): a condition of `MAX_CONDITION_NODES`
/// nodes, `MAX_CONDITION_TERMS` terms and `MAX_CONDITION_DEPTH` levels
/// is accepted, as are `MAX_AND_OR_CHILDREN` children and
/// `MAX_SET_MEMBERS` members; one more of each is refused.
#[test]
fn a_condition_is_held_to_its_limits() {
    let term = Node::compare(Value::variable(0), Operator::Equal, 0);
    let of = |nodes: &[Node]| {
        with(&|table| {
            let mut rule = Rule::new(Trigger::Open, 0);
            rule.condition = condition(nodes);
            table.rules.push(rule).unwrap();
        })
    };
    // Eight children, seven of them a `not` of a term: sixteen nodes and
    // eight terms.
    let mut widest = std::vec![Node::And(count_of(MAX_AND_OR_CHILDREN))];
    for _ in 1..MAX_AND_OR_CHILDREN {
        widest.extend([Node::Not, term]);
    }
    widest.push(term);
    assert_eq!(
        (widest.len(), widest.iter().filter(|node| node.is_term()).count()),
        (MAX_CONDITION_NODES, MAX_CONDITION_TERMS)
    );
    let full = of(&widest);
    assert_eq!(checked(&full), Ok(()));
    assert_eq!(check_condition(&widest), Ok(()));
    // The rule: flags, group, one trigger of one byte, then the count of
    // nodes.
    let count_at = HEADER_BYTES + 4;
    assert_eq!(image(&full)[count_at], count_of(MAX_CONDITION_NODES));
    assert_eq!(with_byte(&full, count_at, count_of(MAX_CONDITION_NODES + 1)), over("MAX_CONDITION_NODES"));

    let mut nine_children = std::vec![Node::Or(count_of(MAX_AND_OR_CHILDREN + 1))];
    nine_children.resize(MAX_AND_OR_CHILDREN + 2, term);
    assert_eq!(checked(&of(&nine_children)), over("MAX_AND_OR_CHILDREN"));
    assert_eq!(checked(&of(&[Node::And(1), term])), Err(Fault::Range("children of and or or")));
    // Nine terms: an `and` of eight under an `or` with one more.
    let mut nine_terms = std::vec![Node::Or(2), Node::And(count_of(MAX_CONDITION_TERMS))];
    nine_terms.resize(2 + MAX_CONDITION_TERMS + 1, term);
    assert_eq!(check_condition(&nine_terms), over("MAX_CONDITION_TERMS"));
    assert_eq!(checked(&of(&nine_terms)), over("MAX_CONDITION_TERMS"));
    // A term under three operators, and under four.
    let deep = |levels: usize| {
        let mut nodes = std::vec![Node::Not; levels];
        nodes.push(term);
        nodes
    };
    assert_eq!(checked(&of(&deep(MAX_CONDITION_DEPTH))), Ok(()));
    assert_eq!(checked(&of(&deep(MAX_CONDITION_DEPTH + 1))), over("MAX_CONDITION_DEPTH"));
    // Not one tree: a node left over, and a child missing.
    assert_eq!(checked(&of(&[term, term])), Err(Fault::Tree));
    assert_eq!(checked(&of(&[Node::And(2), term])), Err(Fault::Tree));

    let mut members = SetMembers::new(0);
    for member in 0..MAX_SET_MEMBERS {
        members.push(i32::try_from(member).unwrap()).unwrap();
    }
    let set = of(&[Node::InSet { value: Value::variable(0), members }]);
    assert_eq!(checked(&set), Ok(()));
    // The node: its code, its value of eight bytes, then its count.
    let members_at = count_at + 1 + 1 + VALUE_BYTES;
    assert_eq!(image(&set)[members_at], count_of(MAX_SET_MEMBERS));
    assert_eq!(with_byte(&set, members_at, count_of(MAX_SET_MEMBERS + 1)), over("MAX_SET_MEMBERS"));
    assert_eq!(
        checked(&of(&[Node::InSet { value: Value::variable(0), members: SetMembers::new(0) }])),
        Err(Fault::Range("empty set"))
    );
    assert_eq!(
        checked(&of(&[Node::InRange { value: Value::variable(0), low: 2, high: 1 }])),
        Err(Fault::Range("range"))
    );
}

/// RULE-103 (REQ-6608): a sequence of `MAX_SEQUENCE_STEPS` steps and
/// `MAX_SEQUENCE_TICKS` ticks is accepted; one step or one tick more, a
/// step's offset not below the length, descending offsets and no step at
/// all are refused.
#[test]
fn a_sequence_is_held_to_its_limits() {
    let of = |length: u32, offsets: &[u32]| {
        with(&|table| {
            let mut sequence = Sequence::new(length, 0);
            for offset in offsets {
                sequence.steps.push(Step { offset: *offset, group: 0 }).unwrap();
            }
            table.sequences.push(sequence).unwrap();
        })
    };
    let most = of(MAX_SEQUENCE_TICKS, &[MAX_SEQUENCE_TICKS - 1; MAX_SEQUENCE_STEPS]);
    assert_eq!(checked(&most), Ok(()));
    assert_eq!(*decoded(&image(&most)).unwrap(), *most);
    assert_eq!(checked(&of(MAX_SEQUENCE_TICKS + 1, &[0])), over("MAX_SEQUENCE_TICKS"));
    assert_eq!(checked(&of(0, &[0])), over("MAX_SEQUENCE_TICKS"));
    // The sequence follows the one group: its repeat count, then its
    // count of steps.
    let steps_at = image(&most).len() - (2 + 4 + MAX_SEQUENCE_STEPS * 5) + 1;
    assert_eq!(image(&most)[steps_at], count_of(MAX_SEQUENCE_STEPS));
    assert_eq!(with_byte(&most, steps_at, count_of(MAX_SEQUENCE_STEPS + 1)), over("MAX_SEQUENCE_STEPS"));
    assert_eq!(checked(&of(12, &[12])), Err(Fault::Range("step's offset")));
    assert_eq!(checked(&of(12, &[4, 3])), Err(Fault::Range("step's offset")));
    assert_eq!(checked(&of(12, &[4, 4, 11])), Ok(()), "two steps of one offset run in written order");
    assert_eq!(checked(&of(12, &[])), Err(Fault::Range("sequence with no step")));
    let mut missing = of(12, &[0]);
    missing.sequences.as_mut_slice()[0].steps.as_mut_slice()[0].group = 1;
    assert_eq!(checked(&missing), Err(Fault::Reference("step's group")));
}

/// RULE-33, RULE-47 (REQ-6608): a table reads at most `MAX_TABLE_VALUES`
/// distinct values, names at most `MAX_TABLE_TARGETS` distinct controls
/// and objects by a constant, and meters at most `MAX_WATCHED` values;
/// one more of each is refused.
#[test]
fn what_a_table_reads_and_names_is_held_to_its_limits() {
    // Each rule reads eight values, the terms of one `or`.
    let reading = |values: &[Value]| {
        with(&|table| {
            for chunk in values.chunks(MAX_CONDITION_TERMS) {
                let mut nodes = std::vec![Node::Or(count_of(chunk.len().max(2)))];
                nodes.extend(chunk.iter().map(|value| Node::compare(*value, Operator::Equal, 1)));
                if chunk.len() == 1 {
                    nodes.push(Node::compare(chunk[0], Operator::Equal, 2));
                }
                let mut rule = Rule::new(Trigger::Open, 0);
                rule.condition = condition(&nodes);
                table.rules.push(rule).unwrap();
            }
        })
    };
    let notes = |count: usize| -> Vec<Value> {
        (0..u16::try_from(count).unwrap()).map(|key| Value::synth(0, 13, 0, 0, CLASS_NOTE, key)).collect()
    };
    assert_eq!(checked(&reading(&notes(MAX_TABLE_VALUES))), Ok(()));
    assert_eq!(Index::of(&reading(&notes(MAX_TABLE_VALUES))).unwrap().values.len(), MAX_TABLE_VALUES);
    assert_eq!(checked(&reading(&notes(MAX_TABLE_VALUES + 1))), over("MAX_TABLE_VALUES"));
    // A value read twice is one value.
    let twice: Vec<Value> = notes(MAX_TABLE_VALUES).into_iter().chain(notes(MAX_TABLE_VALUES)).collect();
    assert_eq!(checked(&reading(&twice[..96])), Ok(()));

    // A part's envelope level, on each audio channel and part.
    let levels = |count: usize| -> Vec<Value> {
        (0..u8::try_from(count).unwrap())
            .map(|slot| Value::synth(slot % 4, 13, 0, slot / 4, CLASS_ENVELOPE_LEVEL, 0))
            .collect()
    };
    assert_eq!(checked(&reading(&levels(MAX_WATCHED))), Ok(()));
    assert_eq!(Index::of(&reading(&levels(MAX_WATCHED))).unwrap().watch.len(), MAX_WATCHED);
    assert_eq!(checked(&reading(&levels(MAX_WATCHED + 1))), over("MAX_WATCHED"));

    // Each rule's four triggers name four objects.
    let naming = |count: usize| {
        with(&|table| {
            let objects: Vec<u16> = (1..=u16::try_from(count).unwrap()).collect();
            for chunk in objects.chunks(MAX_TRIGGERS_PER_RULE) {
                let press =
                    |object: u16| Trigger::Press { target: TriggerTarget::of(Target::object(object)), button: 0 };
                let mut rule = Rule::new(press(chunk[0]), 0);
                for object in &chunk[1..] {
                    rule.triggers.push(press(*object)).unwrap();
                }
                table.rules.push(rule).unwrap();
            }
        })
    };
    assert_eq!(checked(&naming(MAX_TABLE_TARGETS)), Ok(()));
    assert_eq!(Index::of(&naming(MAX_TABLE_TARGETS)).unwrap().targets.len(), MAX_TABLE_TARGETS);
    assert_eq!(checked(&naming(MAX_TABLE_TARGETS + 1)), over("MAX_TABLE_TARGETS"));
}

/// RULE-41 (REQ-6608): an image of exactly `MAX_TABLE_BYTES` is accepted
/// and decodes to its table; a table whose image would be one byte more
/// is refused, and so are that many bytes given to the decoder.
#[test]
fn an_image_is_held_to_its_limit() {
    let keys = KeyMatch::of(&[Key::of(KeyKind::Character, 'a' as u32); MAX_KEYS_PER_TRIGGER]);
    // The groups at their limit, as above, then rules of four triggers
    // of sixteen keys each until the image is nearly full.
    let report = built(Command::Report { value: None });
    let filled = |rules: usize, rects: usize, last_keys: usize| {
        let mut table = table();
        let longest = animate_of_length(MAX_COMMAND_BYTES);
        table.push_group(&framed(&std::vec![longest.clone(); MAX_GROUP_COMMANDS])).unwrap();
        table.push_group(&framed(&std::vec![longest; MAX_GROUP_COMMANDS - 2])).unwrap();
        table.push_group(&framed(core::slice::from_ref(&report))).unwrap();
        let mut rule = Rule::new(Trigger::Key { keys, repeat: false }, 2);
        for _ in 1..MAX_TRIGGERS_PER_RULE {
            rule.triggers.push(Trigger::KeyUp { keys }).unwrap();
        }
        for _ in 0..rules {
            table.rules.push(rule).unwrap();
        }
        for _ in 0..rects {
            table.rects.push(Rect { x: 0, y: 0, width: 1, height: 1 }).unwrap();
        }
        let few = KeyMatch::of(&[Key::of(KeyKind::Character, 'a' as u32); MAX_KEYS_PER_TRIGGER][..last_keys]);
        table.rules.push(Rule::new(Trigger::KeyUp { keys: few }, 2)).unwrap();
        table
    };
    let mut out = std::vec![0_u8; MAX_TABLE_BYTES + 64];
    let mut size =
        |rules: usize, rects: usize, last_keys: usize| encode(&filled(rules, rects, last_keys), body_check, &mut out);
    // A rule of the four full triggers, a rectangle and a key each add a
    // fixed number of bytes: as many rules as fit, then the rectangles
    // and the keys of the last rule that bring the image to the limit
    // exactly.
    let empty = size(0, 0, 0).unwrap();
    let a_rule = size(1, 0, 0).unwrap() - empty;
    let a_key = size(0, 0, 1).unwrap() - empty;
    let fit = (0..MAX_RULES)
        .rev()
        .flat_map(|rules| (0..=MAX_RECTS).map(move |rects| (rules, rects)))
        .flat_map(|(rules, rects)| (0..=MAX_KEYS_PER_TRIGGER).map(move |keys| (rules, rects, keys)))
        .find(|(rules, rects, keys)| empty + rules * a_rule + rects * RECT_BYTES + keys * a_key == MAX_TABLE_BYTES);
    let (rules, rects, last_keys) = fit.unwrap_or_else(|| panic!("no table of {MAX_TABLE_BYTES} bytes"));
    assert_eq!(size(rules, rects, last_keys), Ok(MAX_TABLE_BYTES));
    let full = filled(rules, rects, last_keys);
    assert_eq!(*decoded(&image(&full)).unwrap(), *full);
    // One key more is five bytes more, and the table is refused.
    assert!(last_keys < MAX_KEYS_PER_TRIGGER, "the last rule has room for a key more");
    assert_eq!(size(rules, rects, last_keys + 1), over("MAX_TABLE_BYTES").map(|()| 0));
    assert_eq!(decoded(&std::vec![2_u8; MAX_TABLE_BYTES + 1]).err(), Some(Fault::Limit("MAX_TABLE_BYTES")));
}
