//! Events handled at once, within each tick's budgets, and replayed from
//! a recording (DEC-6635 items 1, 4 and 5; REQ-6526, REQ-6613; RULE-5).

use super::*;
use crate::command::field;
use crate::limits::{MAX_EVENT_COMMANDS_PER_TICK, MAX_EVENT_REPORTS_PER_TICK, MAX_EVENTS_PER_TICK};
use crate::table::RowRun;
use crate::testing::control;

fn inc(variable: u8) -> Built {
    built(Command::Variable { operation: VariableOperation::Inc, variable, operand: Operand::None })
}

/// RULE-5 (REQ-6608, DEC-6635 item 4): `MAX_EVENTS_PER_TICK` events are
/// handled at once in one tick; the next waits, and so does every event
/// after it while it waits, of any kind; they are handled, in order and
/// stamped with the next tick, right after the next tick step; none is
/// lost.
#[test]
fn an_event_over_the_ticks_budget_waits_for_the_next() {
    let mut table = empty();
    let counting = group(&mut table, &[inc(0), report_variable(0)]);
    rule(&mut table, on_key('a'), counting);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    let budget = u32::try_from(MAX_EVENTS_PER_TICK).unwrap();
    for sequence in 0..=budget {
        assert!(give(&mut evaluator, &mut world, press_key(sequence, 'a')));
    }
    // A command of the server's behind it waits too, in arrival order.
    assert!(send(&mut evaluator, &mut world, report_variable(0).command()));
    assert_eq!(evaluator.variables()[0], i32::try_from(budget).unwrap(), "the budget's events, at once");
    assert_eq!((evaluator.counters().events_waited, evaluator.waiting()), (2, 2));
    assert!(world.reports().iter().all(|report| report.tick == 0));
    evaluator.tick(&mut world);
    let last: Vec<(u32, u8, i32)> = world.reports()[usize::try_from(budget).unwrap()..]
        .iter()
        .map(|report| match report.what {
            Reported::Value { reading, .. } => (report.tick, report.rule, reading),
            other => panic!("{other:?}"),
        })
        .collect();
    let next = i32::try_from(budget).unwrap() + 1;
    assert_eq!(last, [(1, 0, next), (1, NO_RULE, next)], "after the tick step, in order, in tick 1");
    assert_eq!(evaluator.counters().events_dropped, 0);
}

/// RULE-5 (REQ-6608): the budgets of commands and of reports. An event
/// whose runs would pass `MAX_EVENT_REPORTS_PER_TICK` or
/// `MAX_EVENT_COMMANDS_PER_TICK` waits, and nothing of its decide phase is
/// kept; the first event of a tick is handled whatever its counts.
#[test]
fn an_event_whose_runs_pass_a_budget_waits() {
    // A group of 32 reports: one key's run is the whole report budget.
    let reports = |table: &mut Table| {
        let all: Vec<Built> = (0..MAX_EVENT_REPORTS_PER_TICK).map(|_| built(Command::Report { value: None })).collect();
        group(table, &all)
    };
    let mut table = empty();
    let loud = reports(&mut table);
    let small = group(&mut table, &[built(Command::Report { value: None })]);
    rule(&mut table, on_key('a'), loud);
    rule(&mut table, on_key('b'), small);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    assert!(give(&mut evaluator, &mut world, press_key(1, 'b')));
    assert!(give(&mut evaluator, &mut world, press_key(2, 'a')), "it waits");
    assert_eq!((world.reports().len(), evaluator.waiting()), (1, 1));
    evaluator.tick(&mut world);
    assert_eq!(world.reports().len(), 1 + MAX_EVENT_REPORTS_PER_TICK, "the first of tick 1, whatever its counts");
    // The commands: four keys of 128 commands each fill 512; sixteen fill
    // 2,048; the first event of a tick goes whatever its counts.
    let mut table = empty();
    let leaf = group(&mut table, &[set(0, 1)]);
    let mut nested = leaf;
    for _ in 0..MAX_GROUP_DEPTH - 1 {
        let runs: Vec<Built> = (0..3).map(|_| built(Command::Run { group: nested })).collect();
        nested = group(&mut table, &runs);
    }
    let cost = table.run_commands(nested);
    assert!(cost * MAX_EVENTS_PER_TICK > MAX_EVENT_COMMANDS_PER_TICK / 64, "{cost}");
    rule(&mut table, Trigger::Text, nested);
    let per_tick = MAX_EVENT_COMMANDS_PER_TICK / cost;
    let mut evaluator = super::evaluator(&table);
    evaluator.tick(&mut world);
    for sequence in 0..=u32::try_from(per_tick).unwrap() {
        give(&mut evaluator, &mut world, InputEvent { sequence, input: Input::Text });
    }
    let handled = usize::try_from(evaluator.counters().events_handled).unwrap();
    assert_eq!(handled, per_tick.min(MAX_EVENTS_PER_TICK), "{cost} commands a run");
}

/// RULE-8 (REQ-6526): an event that waits keeps nothing of its decide
/// phase: its key is not down until it is handled.
#[test]
fn an_event_that_waits_has_not_changed_the_input_state() {
    let mut table = empty();
    let reads = group(&mut table, &[built(Command::Report { value: Some(Value::keys_down()) })]);
    rule(&mut table, Trigger::Text, reads);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    for sequence in 0..u32::try_from(MAX_EVENTS_PER_TICK).unwrap() {
        give(&mut evaluator, &mut world, InputEvent { sequence, input: Input::Text });
    }
    give(&mut evaluator, &mut world, press_key(99, 'a'));
    evaluator.tick(&mut world);
    give(&mut evaluator, &mut world, InputEvent { sequence: 100, input: Input::Text });
    let read: Vec<i32> = readings(&world);
    assert_eq!(read.len(), MAX_EVENTS_PER_TICK + 1);
    assert!(read[..MAX_EVENTS_PER_TICK].iter().all(|down| *down == 0), "the waiting key was never down then");
    assert_eq!(read[MAX_EVENTS_PER_TICK], 1, "and is down once handled");
}

/// RULE-109, DEC-6635 item 1: a cue's rules fire at once, the cue's number
/// read as the firing's, and a cue is an event counted in the budget.
#[test]
fn a_cue_fires_its_rules_at_once() {
    let mut table = empty();
    let heard = group(&mut table, &[built(Command::Report { value: Some(Value::fired(FIRED_CUE)) })]);
    rule(&mut table, Trigger::Cue { channel: 1, number: ANY_CUE }, heard);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    assert!(evaluator.handle(Event::Cue { channel: 0, number: 3 }, &mut world));
    assert_eq!(readings(&world), [] as [i32; 0], "another audio channel");
    for number in 0..u8::try_from(MAX_EVENTS_PER_TICK).unwrap() {
        assert!(evaluator.handle(Event::Cue { channel: 1, number }, &mut world));
    }
    let handled = i32::try_from(MAX_EVENTS_PER_TICK).unwrap() - 1;
    assert_eq!(readings(&world), (0..handled).collect::<Vec<i32>>(), "the cue of channel 0 took one of the budget");
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), (0..=handled).collect::<Vec<i32>>());
    assert_eq!(evaluator.counters().cues, u32::try_from(MAX_EVENTS_PER_TICK).unwrap() + 1);
}

/// RULE-114, DEC-6635 item 1: a command the server sends directly is
/// applied at once, with no firing: a `REPORT` of the firing reports a
/// bare firing with no rule; a `RUN` runs its group with `p` 0.
#[test]
fn a_servers_command_is_applied_at_once() {
    let mut table = empty();
    let p = group(
        &mut table,
        &[built(Command::Variable {
            operation: VariableOperation::Set,
            variable: 6,
            operand: Operand::Fired { constant: 9, property: 4 },
        })],
    );
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    assert!(send(&mut evaluator, &mut world, set(4, 12).command()));
    assert_eq!(evaluator.variables()[4], 12);
    assert!(send(&mut evaluator, &mut world, Command::Report { value: None }));
    assert_eq!(world.reports()[0].rule, NO_RULE);
    assert_eq!(world.reports()[0].what, Reported::Fired);
    assert!(send(&mut evaluator, &mut world, Command::Run { group: p }));
    assert_eq!(evaluator.variables()[6], 9);
}

/// A sequence of numbers from a seed, for the replay test.
struct Numbers(u64);

impl Numbers {
    fn next(&mut self, below: u32) -> u32 {
        // A linear congruential step (Knuth's MMIX constants); the high
        // bits are the better ones.
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        u32::try_from(self.0 >> 33).unwrap() % below
    }
}

/// A table that uses every kind of command and trigger an input can
/// reach: bound notes, variables, a nested group, an armed run, a timer,
/// a sequence, rows, a pointer rule on any object and a `becomes` rule.
fn busy_table() -> Box<Table> {
    let mut table = empty();
    table.rows.push(3).unwrap();
    table.rows.push(5).unwrap();
    let by_row = Binding::of(field::MIDI_DATA1, Source::Parameter);
    let moved =
        animated(&[Operation { target: Target::object(5), kind: OperationKind::Move { relative: true, x: 0, y: 0 } }])
            .bound(Binding::of_operation(0, field::X, Source::Variable(1)));
    let note_by_v0 = note(60).bound(Binding::of(field::MIDI_DATA1, Source::Variable(0)));
    let inner = group(&mut table, &[inc(1), note_by_v0.clone(), report_variable(1)]);
    let keyed = group(
        &mut table,
        &[
            built(Command::Variable {
                operation: VariableOperation::Add,
                variable: 0,
                operand: Operand::Fired { constant: 0, property: 4 },
            }),
            note(40).bound(by_row),
            built(Command::Run { group: inner }),
            control(2, 3, 0, 10, 0).bound(Binding::of(field::CONTROL_GAIN, Source::Variable(1))),
            moved,
            built(Command::Report { value: None }),
            built(Command::Variable {
                operation: VariableOperation::Wrap,
                variable: 0,
                operand: Operand::Range { low: 0, high: 40 },
            }),
        ],
    );
    let timed = group(
        &mut table,
        &[
            built(Command::Timer { start: true, timer: 1 }),
            built(Command::Sequence { start: true, sequence: 0, mode: StartMode::Next }),
            built(Command::Sequence { start: true, sequence: 0, mode: StartMode::At(3) }),
            note_by_v0,
        ],
    );
    let pointed = group(&mut table, &[note(20).bound(Binding::of(field::MIDI_DATA1, Source::Fired(1))), inc(2)]);
    let by_key = KeyMatch::by_row(&[Key::of(KeyKind::Character, 'a' as u32), Key::of(KeyKind::Character, 'b' as u32)]);
    let first = rule(&mut table, Trigger::Key { keys: by_key, repeat: true }, keyed);
    table.rules.as_mut_slice()[first].rows = Some(RowRun { first: 0, count: 2 });
    rule(&mut table, Trigger::KeyUp { keys: keys_of("c") }, timed);
    let odd = rule(&mut table, on_key('c'), inner);
    table.rules.as_mut_slice()[odd].condition = condition(&[Node::compare(Value::variable(1), Operator::Greater, 2)]);
    rule(&mut table, Trigger::Press { target: TriggerTarget::of(Target::ANY_OBJECT), button: 255 }, pointed);
    rule(&mut table, Trigger::After { timer: 1, ticks: 2 }, inner);
    let high = rule(&mut table, Trigger::Becomes, timed);
    table.rules.as_mut_slice()[high].condition = condition(&[Node::compare(Value::variable(2), Operator::Greater, 3)]);
    let mut steps = Sequence::new(5, 2);
    steps.steps.push(Step { offset: 1, group: pointed }).unwrap();
    table.sequences.push(steps).unwrap();
    table
}

/// One event of a recording: how many tick steps had run when it was
/// given, and the event, in the order given (DEC-6635 item 5).
type Recorded = (u32, InputEvent);

/// What a run gave: everything the world was given, the counters and the
/// variables after each tick.
type Outcome = Vec<(Vec<Out>, Counters, [i32; 16])>;

/// A live run of a random script: up to twenty events between ticks, so
/// that some pass the tick's budget; recorded as it goes.
fn live(seed: u64) -> (Outcome, Vec<Recorded>) {
    let mut numbers = Numbers(seed);
    let mut evaluator = evaluator(&busy_table());
    let mut world = Script { under: std::vec![7, 9], ..Script::default() };
    let (mut outcome, mut recording, mut sequence) = (Vec::new(), Vec::new(), 0);
    for interval in 0..64 {
        let busy = if numbers.next(4) == 0 { numbers.next(21) } else { numbers.next(4) };
        let events = if interval < 60 { busy } else { 0 };
        for _ in 0..events {
            sequence += 1;
            let key = ['a', 'b', 'c'][usize::try_from(numbers.next(3)).unwrap()];
            let event = match numbers.next(6) {
                0 | 1 => press_key(sequence, key),
                2 => key_action(sequence, KeyAction::Release, key),
                3 => key_action(sequence, KeyAction::Repeat, key),
                4 => pointer(sequence, PointerAction::Press, 0, 3, 3),
                _ => pointer(sequence, PointerAction::Release, 0, 3, 3),
            };
            // The tick steps run so far say where the event fell: its tick, and
            // before tick step 0 or after it.
            recording.push((evaluator.counters().ticks, event));
            evaluator.handle(Event::Input(event), &mut world);
        }
        evaluator.tick(&mut world);
        outcome.push((core::mem::take(&mut world.log), *evaluator.counters(), *evaluator.variables()));
    }
    (outcome, recording)
}

/// A replay of a recording: each event given in its order, at the place
/// among the tick steps it fell.
fn replay(recording: &[Recorded]) -> Outcome {
    let mut evaluator = evaluator(&busy_table());
    let mut world = Script { under: std::vec![7, 9], ..Script::default() };
    let mut outcome = Vec::new();
    let mut events = recording.iter().peekable();
    for tick in 0..64 {
        while let Some((_, event)) = events.next_if(|(at, _)| *at == tick) {
            evaluator.handle(Event::Input(*event), &mut world);
        }
        evaluator.tick(&mut world);
        outcome.push((core::mem::take(&mut world.log), *evaluator.counters(), *evaluator.variables()));
    }
    outcome
}

fn key_action(sequence: u32, action: KeyAction, character: char) -> InputEvent {
    key(sequence, action, character)
}

/// DEC-6635 item 5 (REQ-6526): a recording of the events, each with the
/// page tick it belonged to and its place among that tick's events,
/// replayed in that order with the ticks between, gives the same outputs
/// in the same order, the same counters and the same variables after
/// every tick; and the same script gives the same outputs on every run.
#[test]
fn a_recorded_run_replays_to_the_same_outputs() {
    let mut fired = 0;
    for seed in 1..=40 {
        let (outcome, recording) = live(seed);
        let replayed = replay(&recording);
        for (tick, (expected, given)) in outcome.iter().zip(&replayed).enumerate() {
            assert_eq!(expected, given, "seed {seed}, tick {tick}");
        }
        assert_eq!(live(seed).0, outcome, "seed {seed}: the same script again");
        fired += outcome.last().map_or(0, |(_, counters, _)| counters.fired);
    }
    assert!(fired > 1000, "the scripts fire rules: {fired}");
}
