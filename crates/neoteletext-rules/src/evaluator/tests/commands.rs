//! The commands a run applies (REQ-6612, REQ-6613; RULE-99, RULE-100,
//! RULE-102, RULE-103, RULE-106, RULE-117, RULE-118, RULE-120).

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::command::{Binding, Source, TYPE_ANIMATE, TYPE_LOCAL_MIDI, field};
use crate::limits::{MAX_ARM_TICKS, MAX_ARMED_RUNS, MAX_RUN_COMMANDS};
use crate::table::{RowRun, SEQUENCE_PASSES, SEQUENCE_POSITION, SEQUENCE_RUNNING};
use crate::testing::{animated, control};

fn variable(operation: VariableOperation, variable: u8, operand: Operand) -> Command {
    Command::Variable { operation, variable, operand }
}

/// RULE-14, RULE-99 (REQ-6612): every operation, each sum saturating at
/// the 32-bit bounds; all sixteen variables 0 at opening; `wrap` at,
/// above and below its range; an operand that is a constant plus a
/// variable, and one that is a value.
#[test]
fn each_variable_operation_saturates() {
    use VariableOperation::{Add, Clear, Dec, Inc, Max, Min, Set, Sub, Toggle, Wrap};
    let mut evaluator = evaluator(&empty());
    let mut world = Script::default();
    assert_eq!(*evaluator.variables(), [0; 16]);
    // Each step: the command, then `v0` after the tick that applies it.
    let (least, most) = (i32::MIN, i32::MAX);
    let constant = Operand::Constant;
    let steps = [
        (variable(Set, 0, constant(most)), most),
        (variable(Inc, 0, Operand::None), most),
        (variable(Add, 0, constant(most)), most),
        (variable(Sub, 0, constant(least)), most),
        (variable(Add, 0, constant(least)), -1),
        (variable(Add, 0, constant(least)), least),
        (variable(Dec, 0, Operand::None), least),
        (variable(Sub, 0, constant(1)), least),
        (variable(Add, 0, constant(-1)), least),
        (variable(Sub, 0, constant(least)), 0),
        (variable(Sub, 0, constant(least)), most),
        (variable(Toggle, 0, Operand::None), 0),
        (variable(Toggle, 0, Operand::None), 1),
        (variable(Toggle, 0, Operand::None), 0),
        (variable(Dec, 0, Operand::None), -1),
        (variable(Toggle, 0, Operand::None), 0),
        (variable(Set, 0, constant(9)), 9),
        (variable(Min, 0, constant(12)), 9),
        (variable(Min, 0, constant(7)), 7),
        (variable(Max, 0, constant(3)), 7),
        (variable(Max, 0, constant(8)), 8),
        // `wrap` within 2 to 8: at its greatest, above it, and below.
        (variable(Wrap, 0, Operand::Range { low: 2, high: 8 }), 8),
        (variable(Inc, 0, Operand::None), 9),
        (variable(Wrap, 0, Operand::Range { low: 2, high: 8 }), 2),
        (variable(Wrap, 0, Operand::Range { low: 2, high: 8 }), 2),
        (variable(Dec, 0, Operand::None), 1),
        (variable(Wrap, 0, Operand::Range { low: 2, high: 8 }), 8),
        // A constant plus a variable; the firing's values read 0 in a
        // command the server sends; a value of the snapshot.
        (variable(Set, 1, constant(most)), 8),
        (variable(Set, 0, Operand::Variable { constant: 5, variable: 1 }), most),
        (variable(Set, 0, Operand::Fired { constant: -4, property: 0 }), -4),
        (variable(Set, 0, Operand::Value(Value::timer(PAGE_TIMER, TIMER_ELAPSED))), 30),
        (variable(Clear, 0, Operand::None), 0),
    ];
    for (place, (command, expected)) in steps.into_iter().enumerate() {
        assert!(send(&mut evaluator, command));
        evaluator.tick(&mut world);
        assert_eq!(evaluator.variables()[0], expected, "step {place}: {command:?}");
    }
    send(&mut evaluator, variable(Set, 15, constant(4)));
    send(&mut evaluator, variable(Clear, 255, Operand::None));
    evaluator.tick(&mut world);
    assert_eq!(*evaluator.variables(), [0; 16], "`clear` with all sixteen");
}

/// RULE-118, RULE-119 (REQ-6612): each command of a group is handed on,
/// or carried out, exactly as the same record sent directly is: the
/// world is given the record's own type and body, in written order; and
/// a command that does nothing is counted and never stops the run.
#[test]
fn a_run_applies_each_command_as_its_record() {
    let mut table = empty();
    let show = animated(&[Operation { target: Target::object(7), kind: OperationKind::Show }]);
    let sprites = Built { kind: command::TYPE_SPRITES, body: std::vec![1, 1, 0, 1, 1, 0], bindings: Vec::new() };
    let objects = Built { kind: command::TYPE_NAPLPS_OBJECTS, ..sprites.clone() };
    let all = [control(1, 1, 5, 80, 250), note(60), show, sprites, objects, set(0, 3), report_variable(0)];
    let every = group(&mut table, &all);
    rule(&mut table, Trigger::Open, every);
    let mut evaluator = evaluator(&table);
    let mut world = Script { refuses: std::vec![TYPE_LOCAL_MIDI, TYPE_ANIMATE], ..Script::default() };
    evaluator.tick(&mut world);
    let given: Vec<(u8, Vec<u8>)> = all[..5].iter().map(|command| (command.kind, command.body.clone())).collect();
    assert_eq!(world.commands(), given, "byte for byte, in written order");
    assert_eq!(readings(&world), [3], "the run went on past the commands that did nothing");
    let counters = evaluator.counters();
    assert_eq!((counters.groups, counters.commands, counters.skipped, counters.reports), (1, 7, 2, 1));
    assert_eq!(table.run_commands(every), 7);
}

/// RULE-106 (REQ-6612): a command's amounts are resolved when it runs:
/// a `CONTROL`'s gain from a variable, a `LOCAL_MIDI`'s key from the
/// firing's key, and an `ANIMATE`'s target from the firing's target and
/// its position from a row parameter; a bound target that names nothing
/// leaves its one operation out, counted.
#[test]
fn amounts_are_resolved_when_a_command_runs() {
    let mut table = empty();
    table.rows.push([0, 0, 40, -3]).unwrap();
    let moved = animated(&[
        Operation { target: Target::object(1), kind: OperationKind::Move { relative: false, x: 100, y: 0 } },
        Operation { target: Target::control(1), kind: OperationKind::Show },
    ])
    .bound(Binding::of_operation(0, field::TARGET, Source::Fired(1)))
    .bound(Binding::of_operation(0, field::X, Source::Parameter(3)))
    .bound(Binding::of_operation(1, field::TARGET, Source::Parameter(2)));
    let commands = [
        set(2, 30),
        control(0, 3, 0, 50, 0).bound(Binding::of(field::CONTROL_GAIN, Source::Variable(2))),
        note(0).bound(Binding::of(field::MIDI_DATA1, Source::Fired(0))),
        moved,
    ];
    let bound = group(&mut table, &commands);
    let any = Trigger::Press { target: TriggerTarget::of(Target::ANY_OBJECT), button: 0 };
    let pressed = rule(&mut table, any, bound);
    table.rules.as_mut_slice()[pressed].triggers.push(Trigger::Key { keys: KeyMatch::any(), repeat: false }).unwrap();
    table.rules.as_mut_slice()[pressed].rows = Some(RowRun { first: 0, count: 1 });
    let mut evaluator = evaluator(&table);
    let mut world = Script { under: std::vec![300], ..Script::default() };
    evaluator.tick(&mut world);
    evaluator.push(pointer(1, PointerAction::Press, 0, 2, 2));
    evaluator.tick(&mut world);
    let given = world.commands();
    assert_eq!(given[0].1[command::CONTROL_GAIN_AT], 80, "50 plus v2, which its own group set to 30");
    assert_eq!(given[1].1[6], 0, "no key fired the rule");
    // Object 1 plus the firing's target, 300; x 100 plus `p3`; and
    // control 1 plus `p2`, which is control 41: no such control.
    assert_eq!(given[2], (TYPE_ANIMATE, std::vec![1, 1, 0, 1, 1, 0, 15, 2, 0x01, 0x2D, 0, 0, 97, 0, 0]));
    assert_eq!(evaluator.counters().skipped, 1);
    world.log.clear();
    evaluator.push(press_key(2, 'C'));
    evaluator.tick(&mut world);
    assert_eq!(world.commands()[1].1[6], 67, "the key's code, held to a MIDI key's range");
    assert_eq!(world.commands()[2].1[9], 1, "object 1 plus no target");
}

/// RULE-102 (REQ-6613): each mode of `RUN`. A group run in place; a run
/// armed for a tick, after a delay and for a timer's next beat; a tick
/// that has passed runs at the next and is counted late; one too far
/// ahead is not armed; a beat of 8 on a timer started at tick 3 armed at
/// tick 13 is due at tick 20; and an armed run keeps its row's
/// parameters.
#[test]
fn a_run_is_armed_for_its_tick() {
    let mut table = empty();
    table.rows.push([0, 77, 0, 0]).unwrap();
    let marked = mark(&mut table, 1);
    let parameter = group(
        &mut table,
        &[
            built(variable(VariableOperation::Set, 14, Operand::Fired { constant: 0, property: 5 })),
            report_variable(14),
        ],
    );
    let arms = group(&mut table, &[built(Command::Run { group: parameter, mode: RunMode::After(2) })]);
    let keyed = rule(&mut table, on_key('a'), arms);
    table.rules.as_mut_slice()[keyed].rows = Some(RowRun { first: 0, count: 1 });
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    let ran_at = |world: &Script| world.reports().iter().map(|report| (report.tick, report.rule)).collect::<Vec<_>>();
    let run_of = |mode| Command::Run { group: marked, mode };
    // Sent before tick 0, so applied in tick 0.
    send(&mut evaluator, run_of(RunMode::Now));
    send(&mut evaluator, run_of(RunMode::At(4)));
    send(&mut evaluator, run_of(RunMode::After(2)));
    send(&mut evaluator, run_of(RunMode::At(0)));
    send(&mut evaluator, run_of(RunMode::At(MAX_ARM_TICKS + 1)));
    send(&mut evaluator, run_of(RunMode::At(MAX_ARM_TICKS)));
    run(&mut evaluator, &mut world, 6);
    assert_eq!(ran_at(&world), [(0, NO_RULE), (1, NO_RULE), (2, NO_RULE), (4, NO_RULE)]);
    let counters = *evaluator.counters();
    assert_eq!((counters.armed, counters.armed_late, counters.skipped), (4, 1, 1));
    world.log.clear();
    // The beat: `t0` is started in tick 6, so it runs since tick 7... the
    // document's example has it started at tick 3 and armed at tick 13.
    let mut beat = evaluator_at(&table, 3);
    send(&mut beat, Command::Timer { start: true, timer: 0 });
    run(&mut beat, &mut world, 10);
    send(&mut beat, run_of(RunMode::Beat { timer: 0, ticks: 8 }));
    send(&mut beat, run_of(RunMode::Beat { timer: 1, ticks: 8 }));
    send(&mut beat, run_of(RunMode::Beat { timer: PAGE_TIMER, ticks: 5 }));
    run(&mut beat, &mut world, 10);
    assert_eq!(ran_at(&world), [(14, NO_RULE), (15, NO_RULE), (20, NO_RULE)], "a stopped timer: the next tick");
    world.log.clear();
    // The row's parameters are held for the armed run.
    evaluator.push(press_key(1, 'a'));
    run(&mut evaluator, &mut world, 4);
    assert_eq!((readings(&world), ran_at(&world)), (std::vec![77], std::vec![(8, NO_RULE)]));
    // One over `MAX_ARMED_RUNS` is not armed, and is counted.
    let waiting = evaluator.counters().armed;
    for _ in 0..=MAX_ARMED_RUNS {
        send(&mut evaluator, run_of(RunMode::After(1000)));
    }
    evaluator.tick(&mut world);
    let counters = evaluator.counters();
    // The one armed for an hour ahead still waits among them.
    assert_eq!((counters.armed - waiting, counters.not_armed), (u32::try_from(MAX_ARMED_RUNS).unwrap() - 1, 2));
}

/// An evaluator for `table` with `ticks` ticks already run.
fn evaluator_at(table: &Table, ticks: u32) -> Box<Evaluator> {
    let mut evaluator = evaluator(table);
    run(&mut evaluator, &mut Script::default(), ticks);
    evaluator
}

/// RULE-103, RULE-117 (REQ-6613): a sequence of three steps and length
/// 12 repeated twice, started in tick `t`, runs its groups at ticks
/// `t+1`, `t+5`, `t+9`, `t+13`, `t+17` and `t+21` and stops; two steps
/// of one offset run in written order; restarting replaces; "until
/// stopped" runs past any count; and its values are read.
#[test]
fn a_sequence_plays_its_steps() {
    let mut table = empty();
    let first = mark(&mut table, 1);
    let second = mark(&mut table, 2);
    let third = mark(&mut table, 3);
    let mut twice = Sequence::new(12, 2);
    for (offset, group) in [(0, first), (4, second), (8, third), (8, first)] {
        twice.steps.push(Step { offset, group }).unwrap();
    }
    table.sequences.push(twice).unwrap();
    let mut forever = Sequence::new(1, 0);
    forever.steps.push(Step { offset: 0, group: first }).unwrap();
    table.sequences.push(forever).unwrap();
    let state = |sequence| {
        [SEQUENCE_RUNNING, SEQUENCE_PASSES, SEQUENCE_POSITION]
            .map(|property| built(Command::Report { value: Some(Value::sequence(sequence, property)) }))
    };
    let reads = group(&mut table, &state(0));
    rule(&mut table, on_key('r'), reads);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    run(&mut evaluator, &mut world, 2);
    let start = |evaluator: &mut Evaluator, sequence| send(evaluator, Command::Sequence { start: true, sequence });
    let played = |world: &Script| -> Vec<(u32, i32)> {
        world
            .reports()
            .iter()
            .filter_map(|report| match report.what {
                Reported::Value { reading, .. } => Some((report.tick, reading)),
                _ => None,
            })
            .collect()
    };
    // Applied in tick 2: t is 2.
    start(&mut evaluator, 0);
    run(&mut evaluator, &mut world, 30);
    assert_eq!(
        played(&world),
        [(3, 1), (7, 2), (11, 3), (11, 1), (15, 1), (19, 2), (23, 3), (23, 1)],
        "two passes, then it stops"
    );
    world.log.clear();
    // Tick 32: started; restarted in tick 38, at position 5 of its pass.
    start(&mut evaluator, 0);
    run(&mut evaluator, &mut world, 6);
    start(&mut evaluator, 0);
    evaluator.push(press_key(1, 'r'));
    evaluator.tick(&mut world);
    assert_eq!(played(&world), [(33, 1), (37, 2), (38, 1), (38, 0), (38, 5)], "running, no pass yet, position 5");
    world.log.clear();
    run(&mut evaluator, &mut world, 9);
    assert_eq!(played(&world), [(39, 1), (43, 2), (47, 3), (47, 1)], "from its start again");
    send(&mut evaluator, Command::Sequence { start: false, sequence: 0 });
    world.log.clear();
    run(&mut evaluator, &mut world, 20);
    assert_eq!(played(&world), [], "stopped where it was");
    // Until stopped: a pass of one tick, more than any stated count of
    // repeats; then every sequence is stopped at once.
    start(&mut evaluator, 1);
    run(&mut evaluator, &mut world, 300);
    assert_eq!(played(&world).len(), 299);
    send(&mut evaluator, Command::Sequence { start: false, sequence: 255 });
    evaluator.tick(&mut world);
    world.log.clear();
    run(&mut evaluator, &mut world, 5);
    assert_eq!(played(&world), []);
    // A sequence the table does not have does nothing, and is counted.
    let skipped = evaluator.counters().skipped;
    start(&mut evaluator, 2);
    evaluator.tick(&mut world);
    assert_eq!(evaluator.counters().skipped, skipped + 1);
}

/// RULE-103, RULE-116 (REQ-6613): a sequence holds the row parameters in
/// force when it was started, which its steps' groups run with; the
/// firing's key, target, row and cue read 0 in them, and their reports
/// name no rule.
#[test]
fn a_sequence_runs_with_the_parameters_it_was_started_with() {
    let mut table = empty();
    table.rows.push(['a' as i32, 64, 0, 0]).unwrap();
    table.rows.push(['b' as i32, 67, 0, 0]).unwrap();
    let step = group(
        &mut table,
        &[
            note(0).bound(Binding::of(field::MIDI_DATA1, Source::Parameter(1))),
            note(1).bound(Binding::of(field::MIDI_DATA1, Source::Fired(0))),
            built(Command::Report { value: None }),
        ],
    );
    let starts = group(&mut table, &[built(Command::Sequence { start: true, sequence: 0 })]);
    let mut pulse = Sequence::new(3, 2);
    pulse.steps.push(Step { offset: 0, group: step }).unwrap();
    table.sequences.push(pulse).unwrap();
    let by_key = KeyMatch::of(&[Key::row(KeyKind::Character, 0)]);
    let keyed = rule(&mut table, Trigger::Key { keys: by_key, repeat: false }, starts);
    table.rules.as_mut_slice()[keyed].rows = Some(RowRun { first: 0, count: 2 });
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push(press_key(1, 'b'));
    run(&mut evaluator, &mut world, 8);
    assert_eq!(notes(&world), [67, 1, 67, 1], "the second row's note, twice; and no key");
    let reports = world.reports();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|report| (report.rule, report.row, report.what) == (NO_RULE, NO_RULE, Reported::Fired)));
    assert_eq!((reports[0].tick, reports[1].tick), (2, 5));
}

/// RULE-120 (REQ-6612): a run applies the commands of its group and of
/// every group it runs in place, in written order; the count is a
/// constant of the table.
#[test]
fn a_nested_run_is_replaced_by_its_groups_commands() {
    let mut table = empty();
    let innermost = group(&mut table, &[set(15, 4), report_variable(15)]);
    let inner = group(
        &mut table,
        &[set(15, 3), report_variable(15), built(Command::Run { group: innermost, mode: RunMode::Now })],
    );
    let middle = group(
        &mut table,
        &[
            set(15, 2),
            report_variable(15),
            built(Command::Run { group: inner, mode: RunMode::Now }),
            set(15, 5),
            report_variable(15),
        ],
    );
    let outer = group(
        &mut table,
        &[
            set(15, 1),
            report_variable(15),
            built(Command::Run { group: middle, mode: RunMode::Now }),
            built(Command::Run { group: innermost, mode: RunMode::Now }),
        ],
    );
    rule(&mut table, Trigger::Open, outer);
    assert_eq!(table.run_commands(outer), 4 + 5 + 3 + 2 + 2);
    assert!(table.run_commands(outer) <= MAX_RUN_COMMANDS);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), [1, 2, 3, 4, 5, 4]);
    let counters = evaluator.counters();
    assert_eq!((counters.groups, counters.commands), (5, 16));
    // A group the table does not have, run by a record sent directly.
    send(&mut evaluator, Command::Run { group: 9, mode: RunMode::Now });
    send(&mut evaluator, Command::Run { group: 9, mode: RunMode::After(1) });
    evaluator.tick(&mut world);
    assert_eq!(evaluator.counters().skipped, 2);
}
