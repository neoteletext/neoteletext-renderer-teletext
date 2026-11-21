//! The end of a painting, the `painted` trigger's event (DEC-7031 item 4,
//! REQ-7026): handled at once like a cue, under the same budgets, local
//! unless a rule reports it, and caused by no command.

use super::*;
use crate::command::TYPE_ANIMATE;
use crate::limits::MAX_EVENTS_PER_TICK;
use crate::table::{FIRED_P, FIRED_TARGET, RowRun, TargetKind};

fn painted(target: Target) -> Event<'static> {
    Event::Painted { target }
}

fn on_painted(target: Target) -> Trigger {
    Trigger::Painted { target: TriggerTarget::of(target) }
}

fn on_painted_row(constant: i16) -> Trigger {
    Trigger::Painted { target: TriggerTarget::row(TargetKind::Object, constant) }
}

/// A group that reports the firing.
fn reporting(table: &mut Table) -> u8 {
    group(table, &[built(Command::Report { value: None })])
}

/// A group that reports the firing's `property`.
fn reporting_fired(table: &mut Table, property: u16) -> u8 {
    group(table, &[built(Command::Report { value: Some(Value::fired(property)) })])
}

/// An evaluator of `table` past its tick step 0, and its world.
fn started(table: &Table) -> (Box<Evaluator>, Script) {
    let mut evaluator = evaluator(table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    world.log.clear();
    (evaluator, world)
}

/// DEC-7031 item 4: a `painted` rule fires once for each event that names
/// its target, at once, in the tick the event arrives in, and again for
/// the next; the picture's rule only for the picture.
#[test]
fn a_painted_rule_fires_once_per_event_and_again_on_the_next() {
    let mut table = empty();
    let drawing = mark(&mut table, 5);
    let picture = mark(&mut table, 1);
    rule(&mut table, on_painted(Target::object(5)), drawing);
    rule(&mut table, on_painted(Target::PAGE), picture);
    let (mut evaluator, mut world) = started(&table);
    assert!(evaluator.handle(painted(Target::object(5)), &mut world));
    assert_eq!(readings(&world), [5], "at once, before any tick step");
    assert!(evaluator.handle(painted(Target::object(5)), &mut world));
    assert_eq!(readings(&world), [5, 5], "again, in the same tick");
    evaluator.tick(&mut world);
    assert!(evaluator.handle(painted(Target::PAGE), &mut world));
    assert!(evaluator.handle(painted(Target::object(5)), &mut world));
    assert_eq!(readings(&world), [5, 5, 1, 5]);
    let ticks: Vec<u32> = world.reports().iter().map(|report| report.tick).collect();
    assert_eq!(ticks, [0, 0, 1, 1], "each in the tick it arrived in");
    assert_eq!((evaluator.counters().fired, evaluator.counters().painted), (4, 4));
}

/// DEC-7031 item 4: a painting no rule names fires nothing, but is an
/// event handled and counted.
#[test]
fn a_painting_nothing_names_fires_nothing_but_is_handled() {
    let mut table = empty();
    let drawing = mark(&mut table, 5);
    rule(&mut table, on_painted(Target::object(5)), drawing);
    rule(&mut table, Trigger::Cue { channel: 0, number: ANY_CUE }, drawing);
    let (mut evaluator, mut world) = started(&table);
    assert!(evaluator.handle(painted(Target::object(6)), &mut world));
    assert!(evaluator.handle(painted(Target::PAGE), &mut world));
    assert_eq!(world.log, []);
    let counters = evaluator.counters();
    assert_eq!((counters.painted, counters.fired, counters.cues, counters.groups), (2, 0, 0, 0));
}

/// DEC-7031 item 4, RULE-105: a constant target names its drawing for
/// every row of its rule, which fires each row; a target from the row's
/// `p` fires only the rows whose `p` gives the drawing.
#[test]
fn a_constant_target_fires_every_row_and_a_row_target_only_its_rows() {
    let mut table = empty();
    for p in [10, 20, 30] {
        table.rows.push(p).unwrap();
    }
    let rows = Some(RowRun { first: 0, count: 3 });
    let p = reporting_fired(&mut table, FIRED_P);
    let every = rule(&mut table, on_painted(Target::object(7)), p);
    table.rules.as_mut_slice()[every].rows = rows;
    let own = rule(&mut table, on_painted_row(0), p);
    table.rules.as_mut_slice()[own].rows = rows;
    let (mut evaluator, mut world) = started(&table);
    evaluator.handle(painted(Target::object(7)), &mut world);
    let fired: Vec<(u8, u8)> = world.reports().iter().map(|report| (report.rule, report.row)).collect();
    assert_eq!(fired, [(0, 0), (0, 1), (0, 2)], "every row of the constant's rule, and none of the other");
    assert_eq!(readings(&world), [10, 20, 30]);
    world.log.clear();
    evaluator.handle(painted(Target::object(20)), &mut world);
    let fired: Vec<(u8, u8)> = world.reports().iter().map(|report| (report.rule, report.row)).collect();
    assert_eq!(fired, [(1, 1)], "the row whose `p` is 20 alone");
    assert_eq!(readings(&world), [20]);
    world.log.clear();
    evaluator.handle(painted(Target::object(25)), &mut world);
    assert_eq!(world.log, [], "no row's `p` is 25");
}

/// RULE-116, DEC-7031 item 4: the firing's target is the drawing painted
/// and its `p` the firing row's, in a condition and in a command.
#[test]
fn the_firings_target_and_p_are_read_in_a_condition_and_a_command() {
    let mut table = empty();
    table.rows.push(10).unwrap();
    table.rows.push(20).unwrap();
    let target = reporting_fired(&mut table, FIRED_TARGET);
    // Drawings 15 and 25, by rows; only the row whose `p` is 20.
    let by_p = rule(&mut table, on_painted_row(5), target);
    table.rules.as_mut_slice()[by_p].rows = Some(RowRun { first: 0, count: 2 });
    table.rules.as_mut_slice()[by_p].condition =
        condition(&[Node::compare(Value::fired(FIRED_P), Operator::Equal, 20)]);
    // Drawing 40, read as the firing's target in the condition.
    let by_target = rule(&mut table, on_painted(Target::object(40)), target);
    table.rules.as_mut_slice()[by_target].condition =
        condition(&[Node::compare(Value::fired(FIRED_TARGET), Operator::Equal, 40)]);
    let (mut evaluator, mut world) = started(&table);
    for drawing in [15, 25, 40] {
        evaluator.handle(painted(Target::object(drawing)), &mut world);
    }
    assert_eq!(readings(&world), [25, 40], "15 is the row whose `p` is 10");
    let rows: Vec<(u8, u8)> = world.reports().iter().map(|report| (report.rule, report.row)).collect();
    assert_eq!(rows, [(0, 1), (1, NO_RULE)]);
}

/// DEC-7031 item 4: a rule fires once for one handling, however many of
/// its triggers name what happened; its other trigger fires it on its own
/// event.
#[test]
fn a_rule_of_painted_and_another_trigger_fires_once_per_handling() {
    let mut table = empty();
    let report = reporting(&mut table);
    let both = rule(&mut table, on_painted(Target::object(3)), report);
    table.rules.as_mut_slice()[both].triggers.push(on_painted(Target::object(3))).unwrap();
    table.rules.as_mut_slice()[both].triggers.push(Trigger::Cue { channel: 1, number: ANY_CUE }).unwrap();
    let (mut evaluator, mut world) = started(&table);
    evaluator.handle(painted(Target::object(3)), &mut world);
    assert_eq!(world.reports().len(), 1, "once, for two triggers naming drawing 3");
    evaluator.handle(Event::Cue { channel: 1, number: 2 }, &mut world);
    assert_eq!(world.reports().len(), 2, "and once for the cue");
    assert_eq!(evaluator.counters().fired, 2);
}

/// RULE-5, DEC-7031 item 4: before tick step 0 a painting waits, and is
/// handled right after it, belonging to tick 0.
#[test]
fn before_tick_0_a_painting_waits() {
    let mut table = empty();
    let report = reporting(&mut table);
    rule(&mut table, on_painted(Target::PAGE), report);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    assert!(evaluator.handle(painted(Target::PAGE), &mut world));
    assert_eq!(world.log, [], "it waits");
    assert_eq!(evaluator.counters().painted, 0);
    evaluator.tick(&mut world);
    let ticks: Vec<u32> = world.reports().iter().map(|report| report.tick).collect();
    assert_eq!(ticks, [0]);
    assert_eq!(evaluator.counters().painted, 1);
}

/// RULE-5 (REQ-6608), DEC-7031 item 4: a painting is one of the
/// `MAX_EVENTS_PER_TICK` events of a tick; the next waits for the next
/// tick step.
#[test]
fn the_painting_after_a_ticks_budget_waits_for_the_next_tick() {
    let mut table = empty();
    let target = reporting_fired(&mut table, FIRED_TARGET);
    rule(&mut table, on_painted_row(0), target);
    // One row for each drawing, 1 to one more than the budget.
    let budget = u16::try_from(MAX_EVENTS_PER_TICK).unwrap();
    for drawing in 1..=budget + 1 {
        table.rows.push(i32::from(drawing)).unwrap();
    }
    table.rules.as_mut_slice()[0].rows = Some(RowRun { first: 0, count: u8::try_from(budget + 1).unwrap() });
    let (mut evaluator, mut world) = started(&table);
    for drawing in 1..=budget + 1 {
        assert!(evaluator.handle(painted(Target::object(drawing)), &mut world), "{drawing}");
    }
    let handled: Vec<i32> = (1..=i32::from(budget)).collect();
    assert_eq!(readings(&world), handled, "the last waits");
    assert_eq!(evaluator.counters().painted, u32::from(budget));
    evaluator.tick(&mut world);
    let reports = world.reports();
    assert_eq!((reports.len(), reports[MAX_EVENTS_PER_TICK].tick), (MAX_EVENTS_PER_TICK + 1, 1));
    assert_eq!(readings(&world)[MAX_EVENTS_PER_TICK], i32::from(budget + 1));
}

/// RULE-5, RULE-57 (REQ-6608), DEC-7031 item 4: paintings wait in the one
/// queue of `MAX_QUEUED_EVENTS`; one more is dropped and counted.
#[test]
fn a_painting_over_a_full_queue_is_dropped_and_counted() {
    let mut table = empty();
    let report = reporting(&mut table);
    rule(&mut table, on_painted(Target::PAGE), report);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    for waiting in 0..MAX_QUEUED_EVENTS {
        assert!(evaluator.handle(painted(Target::PAGE), &mut world), "{waiting}");
    }
    assert!(!evaluator.handle(painted(Target::PAGE), &mut world));
    assert_eq!(evaluator.counters().events_dropped, 1);
    assert_eq!(MAX_QUEUED_EVENTS, 64);
}

/// RULE-22, RULE-46, REQ-6523, DEC-7031 item 4: a `REPORT` in a painted
/// rule reports a firing with no event and no target, never an input;
/// a painted rule without a `REPORT` reports nothing.
#[test]
fn a_painted_rules_report_is_a_bare_firing_and_nothing_else_is_sent() {
    let mut table = empty();
    let report = reporting(&mut table);
    let quiet = group(&mut table, &[set(3, 9)]);
    rule(&mut table, on_painted(Target::object(12)), report);
    rule(&mut table, on_painted(Target::PAGE), quiet);
    let (mut evaluator, mut world) = started(&table);
    evaluator.handle(painted(Target::object(12)), &mut world);
    let reports = world.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        (reports[0].rule, reports[0].row, reports[0].target, reports[0].what),
        (0, NO_RULE, Target::NONE, Reported::Fired),
        "not the drawing as its target, and no input"
    );
    world.log.clear();
    evaluator.handle(painted(Target::PAGE), &mut world);
    assert_eq!(world.reports(), [], "nothing is reported without a `REPORT`");
    assert_eq!(evaluator.variables()[3], 9, "but the rule fired");
}

/// DEC-7031 item 4: no command of types 28 to 33, nor its run, makes a
/// painting: only the receiver gives one.
#[test]
fn no_command_causes_a_painting() {
    let mut table = empty();
    let painting = mark(&mut table, 1);
    rule(&mut table, on_painted(Target::PAGE), painting);
    rule(&mut table, on_painted(Target::object(1)), painting);
    let shows = group(&mut table, &[animated(&[Operation { target: Target::object(1), kind: OperationKind::Show }])]);
    let mut steps = Sequence::new(2, 1);
    steps.steps.push(Step { offset: 0, group: shows }).unwrap();
    table.sequences.push(steps).unwrap();
    let (mut evaluator, mut world) = started(&table);
    for command in [
        Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 },
        Command::Variable { operation: VariableOperation::Set, variable: 0, operand: Operand::Constant(1) },
        Command::Timer { start: true, timer: 0 },
        Command::Report { value: None },
        Command::Run { group: shows },
        Command::Sequence { start: true, sequence: 0, mode: StartMode::Next },
    ] {
        send(&mut evaluator, &mut world, command);
    }
    run(&mut evaluator, &mut world, 5);
    assert_eq!(readings(&world), [] as [i32; 0], "no painted rule fired");
    assert_eq!((evaluator.counters().painted, evaluator.counters().fired), (0, 0));
    assert!(world.commands().iter().any(|(kind, _)| *kind == TYPE_ANIMATE), "the commands ran");
}

/// One event of a recording of paintings: the tick steps run before it,
/// and what was painted.
type Recorded = (u32, Target);

/// Runs `table` through `recording`, and gives the world's log.
fn replayed(table: &Table, recording: &[Recorded], ticks: u32) -> Vec<Out> {
    let mut evaluator = evaluator(table);
    let mut world = Script::default();
    let mut events = recording.iter().peekable();
    for _ in 0..ticks {
        while let Some((_, target)) = events.next_if(|(at, _)| *at == evaluator.counters().ticks) {
            evaluator.handle(painted(*target), &mut world);
        }
        evaluator.tick(&mut world);
    }
    world.log
}

/// DEC-6635 item 5, DEC-7031 item 4: the same table given the same
/// paintings at the same ticks gives the same outputs.
#[test]
fn replaying_a_recording_of_paintings_gives_the_same_outputs() {
    let mut table = empty();
    table.rows.push(2).unwrap();
    table.rows.push(4).unwrap();
    let target = reporting_fired(&mut table, FIRED_TARGET);
    let counted = group(
        &mut table,
        &[
            built(Command::Variable { operation: VariableOperation::Inc, variable: 1, operand: Operand::None }),
            report_variable(1),
        ],
    );
    let by_row = rule(&mut table, on_painted_row(1), target);
    table.rules.as_mut_slice()[by_row].rows = Some(RowRun { first: 0, count: 2 });
    rule(&mut table, on_painted(Target::PAGE), counted);
    let recording: Vec<Recorded> = (0..40_u16)
        .map(|at| {
            let target = if at % 3 == 0 { Target::PAGE } else { Target::object(1 + 2 * (at % 4)) };
            (u32::from(at / 2), target)
        })
        .collect();
    let first = replayed(&table, &recording, 30);
    assert!(first.len() > 20, "the recording fires rules: {}", first.len());
    assert_eq!(replayed(&table, &recording, 30), first);
}
