//! Evaluation on arrival (REQ-6526, DEC-6580 item 1; RULE-5, RULE-7 and
//! RULE-96): an event given by `push_now` has the sound commands of the
//! groups it fires at once and everything else at its tick, and both
//! ways of giving an event have the same result.

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::command::{Binding, Source, TYPE_ANIMATE, TYPE_CONTROL, TYPE_LOCAL_MIDI, field};
use crate::limits::MAX_EVENTS_PER_TICK;
use crate::table::{RowRun, SOUND_LEVEL};
use crate::testing::{animated, control};

fn inc(variable: u8) -> Built {
    built(Command::Variable { operation: VariableOperation::Inc, variable, operand: Operand::None })
}

/// A note whose key is 60 plus `v0`.
fn note_by_v0() -> Built {
    note(60).bound(Binding::of(field::MIDI_DATA1, Source::Variable(0)))
}

/// RULE-96, RULE-5 (REQ-6526): the sound commands of a group fired by an
/// event given on arrival reach the world before the tick, in written
/// order, those of a group it runs in place among them; its `ANIMATE`,
/// its variable and its report wait for the tick and are applied in
/// order, without the sound commands a second time.
#[test]
fn sound_commands_leave_on_arrival_and_the_rest_at_the_tick() {
    let mut table = empty();
    let moved =
        animated(&[Operation { target: Target::object(5), kind: OperationKind::Move { relative: true, x: 8, y: 0 } }]);
    let inner = group(&mut table, &[control(1, 2, 0, 0, 20), set(3, 9)]);
    let all = [
        built(Command::Report { value: None }),
        note(60),
        moved.clone(),
        control(1, 1, 5, 80, 5),
        inc(0),
        built(Command::Run { group: inner, mode: RunMode::Now }),
        built(Command::Timer { start: true, timer: 0 }),
        report_variable(3),
    ];
    let every = group(&mut table, &all);
    rule(&mut table, on_key('a'), every);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    world.log.clear();

    assert!(evaluator.push_now(press_key(77, 'a'), &mut world));
    let sounds = [note(60), control(1, 1, 5, 80, 5), control(1, 2, 0, 0, 20)];
    let expected: Vec<Out> = sounds.iter().map(|sound| Out::Command(sound.kind, sound.body.clone(), 1)).collect();
    assert_eq!(world.log, expected, "only the sound commands, at once, stamped with the coming tick");
    assert_eq!(evaluator.page_tick(), 1, "no tick has run");
    assert_eq!(*evaluator.variables(), [0; 16], "the real writes wait for the tick");
    world.log.clear();

    evaluator.tick(&mut world);
    let report = |what| Out::Report(Report { rule: 0, row: NO_RULE, tick: 1, target: Target::NONE, what });
    assert_eq!(
        world.log,
        [
            report(Reported::Input { sequence: 77 }),
            Out::Command(TYPE_ANIMATE, moved.body.clone(), 1),
            report(Reported::Value { value: Value::variable(3), reading: 9 }),
            Out::Cycles(1),
        ],
        "everything else, in order, and no sound command again"
    );
    assert_eq!((evaluator.variables()[0], evaluator.variables()[3]), (1, 9));
    let counters = evaluator.counters();
    assert_eq!((counters.fired, counters.groups, counters.commands, counters.reports), (1, 2, 10, 2));
    world.log.clear();
    evaluator.tick(&mut world);
    assert_eq!(world.log, [Out::Cycles(2)], "the event is not decided a second time");
}

/// RULE-96, RULE-7 (REQ-6526): the arrival copy. A group `inc v0`,
/// `LOCAL_MIDI 60 + v0` delivers key 61 on arrival and leaves `v0` at 1
/// after the tick; two keys in one interval read each other's writes in
/// arrival order; and a condition on the variable reads the last
/// completed tick's value, so the second key's rule on it does not fire
/// until the next interval.
#[test]
fn an_amount_on_arrival_reads_the_arrival_copy() {
    let mut table = empty();
    let plays = group(&mut table, &[inc(0), note_by_v0()]);
    rule(&mut table, on_key('a'), plays);
    let seen = group(&mut table, &[note(1)]);
    let after = rule(&mut table, on_key('b'), seen);
    table.rules.as_mut_slice()[after].condition = condition(&[Node::compare(Value::variable(0), Operator::Greater, 0)]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    world.log.clear();
    evaluator.push_now(press_key(1, 'a'), &mut world);
    assert_eq!(notes(&world), [61]);
    evaluator.push_now(press_key(2, 'b'), &mut world);
    evaluator.push_now(press_key(3, 'a'), &mut world);
    assert_eq!(notes(&world), [61, 62], "the second key reads the first's write; `b` reads the snapshot");
    assert_eq!(evaluator.variables()[0], 0);
    evaluator.tick(&mut world);
    assert_eq!(evaluator.variables()[0], 2, "the real writes, at step 3 of the tick");
    assert_eq!(notes(&world), [61, 62], "and nothing delivered twice");
    evaluator.push_now(press_key(4, 'b'), &mut world);
    evaluator.push_now(press_key(5, 'a'), &mut world);
    assert_eq!(notes(&world), [61, 62, 1, 63], "in the next interval the condition holds");
}

/// RULE-96, RULE-5 (REQ-6526, REQ-6608): the first event over
/// `MAX_EVENTS_PER_TICK` in an interval waits, and is decided as soon as
/// the tick before its own has completed; and an event that arrives
/// before page tick 0 has been evaluated belongs to page tick 1.
#[test]
fn an_event_over_the_ticks_events_is_decided_after_the_tick() {
    let mut table = empty();
    let plays = group(&mut table, &[inc(0), note_by_v0(), built(Command::Report { value: None })]);
    rule(&mut table, on_key('a'), plays);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    assert!(evaluator.push_now(press_key(1, 'a'), &mut world));
    assert_eq!(world.log, [], "before page tick 0 nothing is decided");
    evaluator.tick(&mut world);
    assert_eq!(world.log, [Out::Cycles(0), Out::Command(TYPE_LOCAL_MIDI, note(61).body, 1)], "then at once");
    evaluator.tick(&mut world);
    assert_eq!(world.reports()[0].tick, 1, "it belongs to page tick 1");
    world.log.clear();

    for sequence in 0..=u32::try_from(MAX_EVENTS_PER_TICK).unwrap() {
        assert!(evaluator.push_now(press_key(10 + sequence, 'a'), &mut world));
    }
    assert_eq!(notes(&world).len(), MAX_EVENTS_PER_TICK, "one waits");
    evaluator.tick(&mut world);
    let after_tick: Vec<&Out> = world.log.iter().skip_while(|out| **out != Out::Cycles(2)).collect();
    // v0 was 1; sixteen keys of tick 2 make it 17, and the one that waited
    // plays 60 plus 18, stamped with tick 3.
    assert_eq!(after_tick, [&Out::Cycles(2), &Out::Command(TYPE_LOCAL_MIDI, note(78).body, 3)]);
    assert_eq!(world.reports().len(), MAX_EVENTS_PER_TICK);
    evaluator.tick(&mut world);
    assert_eq!(world.reports().len(), MAX_EVENTS_PER_TICK + 1);
    assert_eq!(
        world.reports().last().map(|report| (report.tick, report.what)),
        Some((3, Reported::Input { sequence: 26 }))
    );
    assert_eq!(evaluator.variables()[0], 18);
}

/// RULE-96 (REQ-6526): when the page closes with events decided on
/// arrival whose tick has not run, that tick is run before the final
/// tick, so what their groups wrote and reported is not lost when their
/// sound has left.
#[test]
fn close_runs_the_tick_of_events_decided_on_arrival() {
    let mut table = empty();
    let plays = group(&mut table, &[note(60), built(Command::Report { value: None })]);
    rule(&mut table, on_key('a'), plays);
    let closing = mark(&mut table, 8);
    rule(&mut table, Trigger::Close, closing);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push_now(press_key(5, 'a'), &mut world);
    world.log.clear();
    evaluator.close(&mut world);
    let reports: Vec<(u32, Reported)> = world.reports().iter().map(|report| (report.tick, report.what)).collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0], (1, Reported::Input { sequence: 5 }));
    assert_eq!(reports[1].0, 2, "then the final tick's `close` rule");
    assert_eq!(notes(&world), [] as [u8; 0], "the note left on arrival, once");
}

/// RULE-7 (REQ-6526): a snapshot an arrival took is the tick's one
/// snapshot for every value but the meters: a metered report after the
/// interval's first arrival is not seen by its later arrivals, and is
/// seen by the tick's `becomes` rule.
#[test]
fn a_meter_is_never_a_tick_late_because_a_key_arrived() {
    let level = Value::sound(0, SOUND_LEVEL);
    let loud = condition(&[Node::compare(level, Operator::GreaterOrEqual, 128)]);
    let mut table = empty();
    let keyed = mark(&mut table, 3);
    let became = mark(&mut table, 10);
    let first = rule(&mut table, on_key('a'), keyed);
    let second = rule(&mut table, Trigger::Becomes, became);
    table.rules.as_mut_slice()[first].condition = loud;
    table.rules.as_mut_slice()[second].condition = loud;
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push_now(press_key(1, 'a'), &mut world);
    evaluator.set_metered(&[200]);
    evaluator.push_now(press_key(2, 'a'), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), [10], "the keys read the level of the first arrival; `becomes` the newest");
}

/// A small generator of numbers, the same on every run.
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
    table.rows.push(['a' as i32, 3, 0, 0]).unwrap();
    table.rows.push(['b' as i32, 5, 0, 0]).unwrap();
    let by_row = Binding::of(field::MIDI_DATA1, Source::Parameter(1));
    let moved =
        animated(&[Operation { target: Target::object(5), kind: OperationKind::Move { relative: true, x: 0, y: 0 } }])
            .bound(Binding::of_operation(0, field::X, Source::Variable(1)));
    let inner = group(&mut table, &[inc(1), note_by_v0(), report_variable(1)]);
    let keyed = group(
        &mut table,
        &[
            built(Command::Variable {
                operation: VariableOperation::Add,
                variable: 0,
                operand: Operand::Fired { constant: 0, property: 5 },
            }),
            note(40).bound(by_row),
            built(Command::Run { group: inner, mode: RunMode::Now }),
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
            built(Command::Sequence { start: true, sequence: 0 }),
            built(Command::Run { group: inner, mode: RunMode::After(3) }),
            note_by_v0(),
        ],
    );
    let pointed = group(&mut table, &[note(20).bound(Binding::of(field::MIDI_DATA1, Source::Fired(1))), inc(2)]);
    let by_key = KeyMatch::of(&[Key::row(KeyKind::Character, 0)]);
    let first = rule(&mut table, Trigger::Key { keys: by_key, repeat: true }, keyed);
    table.rules.as_mut_slice()[first].rows = Some(RowRun { first: 0, count: 2 });
    rule(&mut table, Trigger::KeyUp { keys: keys_of("c") }, timed);
    let odd = rule(&mut table, on_key('c'), inner);
    table.rules.as_mut_slice()[odd].condition = condition(&[Node::compare(Value::variable(1), Operator::Greater, 2)]);
    rule(&mut table, Trigger::Press { target: TriggerTarget::of(Target::ANY_OBJECT), button: 255 }, pointed);
    rule(&mut table, Trigger::After { timer: 1, ticks: 2 }, inner);
    rule(&mut table, Trigger::Double { keys: keys_of("ab"), ticks: 4 }, pointed);
    let high = rule(&mut table, Trigger::Becomes, timed);
    table.rules.as_mut_slice()[high].condition = condition(&[Node::compare(Value::variable(2), Operator::Greater, 3)]);
    let mut steps = Sequence::new(5, 2);
    steps.steps.push(Step { offset: 1, group: pointed }).unwrap();
    table.sequences.push(steps).unwrap();
    table
}

/// How an event of the property test is given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Way {
    /// Every event by `push`: decided at its tick.
    AtTheTick,
    /// Every event by `push_now`: decided on arrival.
    OnArrival,
    /// Some one way and some the other, freely mixed.
    Mixed,
}

/// What a run of the property test gave: for each tick, the reports, the
/// commands that are not sound commands, the counters and the variables;
/// and for the whole run the sound commands in order.
type Outcome = (Vec<(Vec<Out>, Counters, [i32; 16])>, Vec<Out>);

fn play(seed: u64, way: Way) -> Outcome {
    let mut numbers = Numbers(seed);
    let mut choice = Numbers(seed ^ 0x5DEE_CE66);
    let mut evaluator = evaluator(&busy_table());
    let mut world = Script { under: std::vec![7, 9], ..Script::default() };
    let (mut ticks, mut sounds, mut sequence) = (Vec::new(), Vec::new(), 0);
    for interval in 0..64 {
        // Up to twenty events an interval, so that some intervals pass
        // the sixteen a tick handles; none in the last four, so that
        // every event that waited has had its tick.
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
            let now = match way {
                Way::AtTheTick => false,
                Way::OnArrival => true,
                Way::Mixed => choice.next(2) == 0,
            };
            if now {
                evaluator.push_now(event, &mut world);
            } else {
                evaluator.push(event);
            }
        }
        evaluator.tick(&mut world);
        let is_sound =
            |out: &Out| matches!(out, Out::Command(kind, ..) if [TYPE_LOCAL_MIDI, TYPE_CONTROL].contains(kind));
        let (sound, rest): (Vec<Out>, Vec<Out>) = world.log.drain(..).partition(is_sound);
        sounds.extend(sound);
        ticks.push((rest, *evaluator.counters(), *evaluator.variables()));
    }
    (ticks, sounds)
}

fn key_action(sequence: u32, action: KeyAction, character: char) -> InputEvent {
    key(sequence, action, character)
}

/// RULE-96 (REQ-6526): for the same table and the same events in the
/// same arrival order within the same tick intervals, deciding at the
/// tick, deciding on arrival and any mixture of the two produce the same
/// firings, the same reports with the same page ticks, the same commands
/// in the same order, the same counters and the same rule state at the
/// end of every tick; the sound commands are the same, with the same
/// amounts and the same page ticks, and only the moment they leave
/// differs. A replay of the same script gives the same outputs.
#[test]
fn deciding_at_the_tick_and_on_arrival_agree() {
    let mut fired = 0;
    for seed in 1..=40 {
        let at_the_tick = play(seed, Way::AtTheTick);
        let on_arrival = play(seed, Way::OnArrival);
        for (tick, (expected, given)) in at_the_tick.0.iter().zip(&on_arrival.0).enumerate() {
            assert_eq!(expected, given, "seed {seed}, tick {tick}");
        }
        assert_eq!(at_the_tick.1, on_arrival.1, "seed {seed}: the sound commands");
        assert_eq!(play(seed, Way::Mixed), at_the_tick, "seed {seed}: mixed");
        assert_eq!(play(seed, Way::OnArrival), on_arrival, "seed {seed}: a replay");
        fired += at_the_tick.0.last().map_or(0, |(_, counters, _)| counters.fired);
    }
    assert!(fired > 1000, "the scripts fire rules: {fired}");
}
