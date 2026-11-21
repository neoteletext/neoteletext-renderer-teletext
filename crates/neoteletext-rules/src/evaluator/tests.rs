//! The evaluator (REQ-6526, REQ-6612, REQ-6613, REQ-6615; RULE-5 to
//! RULE-8, RULE-96 and the rules they cite).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, plain arithmetic on small numbers and `panic!` on outcomes it rules out"
)]

mod commands;
mod handling;
mod painted;
mod triggers;

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::command::{Command, Operand, StartMode, VariableOperation};
use crate::limits::MAX_QUEUED_EVENTS;
use crate::state::{SequenceState, TimerState};
use crate::table::{
    Key, KeyKind, KeyMatch, Operator, Rule, Sequence, Step, TIMER_ELAPSED, Table, TriggerTarget, condition,
};
use crate::testing::{Built, Out, PAGE, SUB, Script, animated, body_check, built, group};

pub(super) fn evaluator(table: &Table) -> Box<Evaluator> {
    Box::new(Evaluator::new(table, 1, body_check).unwrap())
}

pub(super) fn key(sequence: u32, action: KeyAction, character: char) -> InputEvent {
    let key = KeyIdentity::Character(character as u32);
    InputEvent { sequence, input: Input::Key { action, key, base: None, mods: 0 } }
}

pub(super) fn press_key(sequence: u32, character: char) -> InputEvent {
    key(sequence, KeyAction::Press, character)
}

pub(super) fn keys_of(characters: &str) -> KeyMatch {
    let keys: Vec<Key> = characters.chars().map(|character| Key::of(KeyKind::Character, character as u32)).collect();
    KeyMatch::of(&keys)
}

pub(super) fn on_key(character: char) -> Trigger {
    Trigger::Key { keys: keys_of(character.encode_utf8(&mut [0; 4])), repeat: false }
}

/// A table for the tests' page with nothing in it.
pub(super) fn empty() -> Box<Table> {
    Box::new(Table::new(PAGE, SUB))
}

/// Adds a rule of `trigger` that runs `group`, and gives its number.
pub(super) fn rule(table: &mut Table, trigger: Trigger, group: u8) -> usize {
    table.rules.push(Rule::new(trigger, group)).unwrap()
}

/// Adds a group that marks its run: it sets `v15` to `mark` and reports
/// it, so that the readings reported say which groups ran, in order.
pub(super) fn mark(table: &mut Table, mark: i32) -> u8 {
    group(table, &[set(15, mark), report_variable(15)])
}

/// Gives the evaluator a command as the server sends it directly.
pub(super) fn send(evaluator: &mut Evaluator, world: &mut Script, command: Command) -> bool {
    let sent = built(command);
    evaluator.handle(Event::Command { kind: sent.kind, body: &sent.body }, world)
}

/// Gives the evaluator an input event.
pub(super) fn give(evaluator: &mut Evaluator, world: &mut Script, event: InputEvent) -> bool {
    evaluator.handle(Event::Input(event), world)
}

/// Runs `ticks` ticks.
pub(super) fn run(evaluator: &mut Evaluator, world: &mut Script, ticks: u32) {
    for _ in 0..ticks {
        evaluator.tick(world);
    }
}

/// A pointer event at a pixel.
pub(super) fn pointer(sequence: u32, action: PointerAction, button: u8, x: u16, y: u16) -> InputEvent {
    InputEvent { sequence, input: Input::Pointer { action, button, x, y, control: 0 } }
}

/// A target shown at a rectangle of the picture.
pub(super) fn shown_at(left: i32, top: i32, width: i32, height: i32) -> TargetState {
    TargetState {
        shown: true,
        cycling: false,
        frame: 0,
        look: 0,
        layer: 1,
        order: 0,
        x: 0,
        y: 0,
        bounds: Some(Bounds { left, top, right: left + width, bottom: top + height }),
    }
}

/// A `LOCAL_MIDI` note on of `key` on audio channel 0.
pub(super) fn note(key: u8) -> Built {
    built(Command::LocalMidi { channel: 0, status: 0x90, data1: key, data2: 100 })
}

/// The keys of the notes the world was given, in order.
pub(super) fn notes(world: &Script) -> Vec<u8> {
    world.commands().iter().filter(|(kind, _)| *kind == command::TYPE_LOCAL_MIDI).map(|(_, body)| body[6]).collect()
}

pub(super) fn set(variable: u8, value: i32) -> Built {
    built(Command::Variable { operation: VariableOperation::Set, variable, operand: Operand::Constant(value) })
}

pub(super) fn report_variable(variable: u8) -> Built {
    built(Command::Report { value: Some(Value::variable(variable)) })
}

/// The readings of the value reports logged, in order.
pub(super) fn readings(world: &Script) -> Vec<i32> {
    world
        .reports()
        .iter()
        .filter_map(|report| match report.what {
            Reported::Value { reading, .. } => Some(reading),
            _ => None,
        })
        .collect()
}

/// RULE-5 (REQ-6526, DEC-6635 item 2): the tick step decides `open` at
/// page tick 0 alone, then timers, sequences and `becomes`, in that order,
/// and the cycling of frames comes after every group; an event before tick
/// step 0 waits and is handled right after it, belonging to tick 0; and
/// the same table with the same script gives the same outputs every run.
#[test]
fn the_order_of_a_tick_step_is_fixed() {
    let play = || {
        let mut table = empty();
        let groups: Vec<u8> = (2..=10).map(|number| mark(&mut table, number)).collect();
        let of = |step: usize| groups[step - 2];
        rule(&mut table, Trigger::Becomes, of(10));
        let elapsed = Value::timer(PAGE_TIMER, TIMER_ELAPSED);
        table.rules.as_mut_slice()[0].condition = condition(&[Node::compare(elapsed, Operator::Equal, 3)]);
        rule(&mut table, Trigger::After { timer: PAGE_TIMER, ticks: 3 }, of(7));
        rule(&mut table, on_key('a'), of(3));
        rule(&mut table, Trigger::Open, of(2));
        let mut steps = Sequence::new(100, 1);
        steps.steps.push(Step { offset: 1, group: of(8) }).unwrap();
        table.sequences.push(steps).unwrap();
        let mut evaluator = evaluator(&table);
        let mut world = Script::default();
        assert!(give(&mut evaluator, &mut world, press_key(1, 'a')));
        assert_eq!(world.log, [], "before tick step 0 an event waits for it");
        evaluator.tick(&mut world);
        assert_eq!(readings(&world), [2, 3], "`open`, then the event that waited");
        assert_eq!(world.reports()[1].tick, 0, "the event belongs to tick 0");
        assert_eq!(world.log[1], Out::Cycles(0), "the waiting event is handled after the tick step");
        world.log.clear();
        // In tick 0: the sequence starts, at tick 1, and its offset 1 step
        // runs at tick 2. Tick 3 holds the timer and `becomes`.
        assert!(send(
            &mut evaluator,
            &mut world,
            Command::Sequence { start: true, sequence: 0, mode: StartMode::At(2) }
        ));
        run(&mut evaluator, &mut world, 3);
        assert_eq!(readings(&world), [7, 8, 10], "tick 3: the timer, the step (offset 1 of a start at 2), `becomes`");
        assert!(world.reports().iter().all(|report| report.tick == 3));
        assert_eq!(world.log.last(), Some(&Out::Cycles(3)), "the cycling of frames is after every group");
        (world.log, *evaluator.counters(), *evaluator.variables())
    };
    assert_eq!(play(), play());
}

/// RULE-6, RULE-96 (REQ-6526, DEC-6635 items 1 and 3): an event's groups
/// are applied at once, before `handle` returns: the variable, the report,
/// the sound and the animation. A command fires no rule: a `becomes` rule
/// on the variable fires at the next tick step's check, while a later
/// event's condition sees the change at once; a sequence a key starts runs
/// its offset 0 step at the next tick step; a group that stops its own
/// sequence still completes.
#[test]
fn an_events_groups_are_applied_at_once_and_cascade_nothing() {
    let mut table = empty();
    let shown = animated(&[Operation { target: Target::object(5), kind: OperationKind::Show }]);
    let start = built(Command::Sequence { start: true, sequence: 0, mode: StartMode::Next });
    let sets = group(&mut table, &[set(0, 1), report_variable(0), note(60), shown, start]);
    let became = mark(&mut table, 10);
    let seen = mark(&mut table, 20);
    let stopping = group(
        &mut table,
        &[
            built(Command::Sequence { start: false, sequence: 0, mode: StartMode::Next }),
            set(15, 8),
            report_variable(15),
        ],
    );
    rule(&mut table, on_key('a'), sets);
    let becomes = rule(&mut table, Trigger::Becomes, became);
    table.rules.as_mut_slice()[becomes].condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 1)]);
    let later = rule(&mut table, on_key('b'), seen);
    table.rules.as_mut_slice()[later].condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 1)]);
    let mut steps = Sequence::new(4, 0);
    steps.steps.push(Step { offset: 0, group: stopping }).unwrap();
    table.sequences.push(steps).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    world.log.clear();
    assert!(give(&mut evaluator, &mut world, press_key(1, 'a')));
    assert_eq!(evaluator.variables()[0], 1);
    assert_eq!(readings(&world), [1], "the report left at once");
    assert_eq!(notes(&world), [60], "the note left at once");
    let kinds: Vec<u8> = world.commands().iter().map(|(kind, _)| *kind).collect();
    assert_eq!(kinds, [command::TYPE_LOCAL_MIDI, command::TYPE_ANIMATE], "in the group's order");
    assert!(world.reports().iter().all(|report| report.tick == 0), "it belongs to the present tick");
    assert!(give(&mut evaluator, &mut world, press_key(2, 'b')));
    assert_eq!(readings(&world), [1, 20], "a later event's condition reads the change at once");
    evaluator.tick(&mut world);
    assert_eq!(
        readings(&world),
        [1, 20, 8, 10],
        "tick 1: the sequence's step, which stops it and completes; then `becomes`"
    );
    run(&mut evaluator, &mut world, 8);
    assert_eq!(readings(&world).len(), 4, "the sequence stopped itself, and `becomes` is an edge");
}

/// RULE-7 (REQ-6526, DEC-6635 item 1): there is one state. `set v0 5`
/// then `add v0 1` leave 6; a `LOCAL_MIDI` after `inc v0` whose key is
/// `60 + v0` plays the new key; the conditions of one event's rules are
/// each read before any of its groups runs, so a second rule of the same
/// event whose condition is on `v0` reads it as it was; the next event
/// reads the new value; and a metered report given between two keys is
/// read by the second key's rules.
#[test]
fn commands_read_the_state_as_it_stands_and_an_events_conditions_as_it_was() {
    let mut table = empty();
    let inc = built(Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None });
    let add =
        built(Command::Variable { operation: VariableOperation::Add, variable: 1, operand: Operand::Constant(1) });
    let bound = note(60).bound(Binding::of(command::field::MIDI_DATA1, Source::Variable(0)));
    let plays = group(&mut table, &[inc, set(1, 5), add, bound]);
    let old = mark(&mut table, 1);
    let loud = mark(&mut table, 2);
    rule(&mut table, on_key('a'), plays);
    let second = rule(&mut table, on_key('a'), old);
    table.rules.as_mut_slice()[second].condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 0)]);
    let metered = rule(&mut table, on_key('m'), loud);
    let level = Value::sound(0, SOUND_LEVEL);
    table.rules.as_mut_slice()[metered].condition = condition(&[Node::compare(level, Operator::Greater, 100)]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    give(&mut evaluator, &mut world, press_key(1, 'a'));
    assert_eq!(notes(&world), [61], "the amount reads the variable its own group just changed");
    assert_eq!(evaluator.variables()[1], 6);
    assert_eq!(readings(&world), [1], "the second rule's condition read v0 before the event's groups ran");
    world.log.clear();
    give(&mut evaluator, &mut world, press_key(2, 'a'));
    assert_eq!((notes(&world), readings(&world)), (std::vec![62], std::vec![]), "and now it reads 1");
    give(&mut evaluator, &mut world, press_key(3, 'm'));
    evaluator.set_metered(&[200]);
    give(&mut evaluator, &mut world, press_key(4, 'm'));
    assert_eq!(readings(&world), [2], "the report between the keys is read by the second");
}

/// RULE-8 (REQ-6526): input state follows the events. Shift then A fire
/// the rule on A-with-Shift; A before Shift does not; and the releases
/// the receiver gives when the page loses the keyboard fire the `key_up`
/// rules in the order given, whatever the budget.
#[test]
fn input_state_follows_the_events() {
    // The functional code of a key, a plain number here (RULE-56).
    const SHIFT: u32 = 57_441;
    let shift = |sequence, action| InputEvent {
        sequence,
        input: Input::Key { action, key: KeyIdentity::Named(SHIFT), base: None, mods: 0 },
    };
    let mut table = empty();
    let chord = mark(&mut table, 1);
    let up = group(&mut table, &[built(Command::Report { value: None })]);
    let with_shift = rule(&mut table, on_key('a'), chord);
    let down = Value::key(crate::table::INPUT_NAMED, SHIFT);
    table.rules.as_mut_slice()[with_shift].condition = condition(&[Node::compare(down, Operator::Equal, 1)]);
    rule(&mut table, Trigger::KeyUp { keys: KeyMatch::any() }, up);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    give(&mut evaluator, &mut world, press_key(1, 'a'));
    give(&mut evaluator, &mut world, shift(2, KeyAction::Press));
    assert_eq!(readings(&world), [] as [i32; 0], "A arrived before Shift");
    give(&mut evaluator, &mut world, key(3, KeyAction::Release, 'a'));
    give(&mut evaluator, &mut world, shift(4, KeyAction::Release));
    give(&mut evaluator, &mut world, shift(5, KeyAction::Press));
    give(&mut evaluator, &mut world, press_key(6, 'a'));
    assert_eq!(readings(&world), [1], "Shift, then A");
    world.log.clear();
    // Spend the tick's budget, then lose the keyboard: a release for each
    // key still down, handled at once all the same.
    for sequence in 10..10 + u32::try_from(MAX_EVENTS_PER_TICK).unwrap() {
        give(&mut evaluator, &mut world, InputEvent { sequence, input: Input::Text });
    }
    assert!(evaluator.handle(Event::Lost(shift(7, KeyAction::Release)), &mut world));
    assert!(evaluator.handle(Event::Lost(key(8, KeyAction::Release, 'a')), &mut world));
    let released: Vec<Reported> = world.reports().iter().map(|report| report.what).collect();
    assert_eq!(released, [Reported::Input { sequence: 7 }, Reported::Input { sequence: 8 }]);
}

/// RULE-47 (REQ-6526): a state query is an event, answered at once with
/// the state as it stands: the sixteen variables, the eight timers, the
/// table's sequences with their waiting flag, and each target and value
/// the table names, at the page tick it is handled in.
#[test]
fn a_state_query_is_answered_at_once() {
    let mut table = empty();
    let reads = group(&mut table, &[built(Command::Report { value: Some(Value::sound(2, SOUND_GAIN)) })]);
    let shown = rule(&mut table, Trigger::Becomes, reads);
    let object = Target::object(301);
    let is_shown = Value::target(TargetKind::Object, 301, TARGET_SHOWN);
    table.rules.as_mut_slice()[shown].condition = condition(&[Node::compare(is_shown, Operator::Equal, 7)]);
    table.sequences.push(Sequence::new(10, 0)).unwrap();
    table.sequences.as_mut_slice()[0].steps.push(Step { offset: 9, group: reads }).unwrap();
    table.sequences.push(Sequence::new(10, 0)).unwrap();
    table.sequences.as_mut_slice()[1].steps.push(Step { offset: 9, group: reads }).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    world.targets.push((object, TargetState { frame: 3, x: 700, y: -380, ..shown_at(0, 0, 4, 4) }));
    world.sounds[2].gain = 55;
    // Before tick step 0 the server's commands wait for it, and are
    // handled in tick 0.
    send(&mut evaluator, &mut world, set(3, -9).command());
    send(&mut evaluator, &mut world, Command::Timer { start: true, timer: 7 });
    send(&mut evaluator, &mut world, Command::Sequence { start: true, sequence: 0, mode: StartMode::Next });
    send(&mut evaluator, &mut world, Command::Sequence { start: true, sequence: 1, mode: StartMode::At(100) });
    run(&mut evaluator, &mut world, 4);
    world.log.clear();
    assert!(evaluator.handle(Event::Query(42), &mut world));
    let Some(Out::State(state)) = world.log.last() else { panic!("{:?}", world.log.last()) };
    let answered = state.table.unwrap();
    assert_eq!((state.identifier, answered.generation, answered.tick), (42, 1, 3));
    assert_eq!(answered.variables[3], -9);
    assert_eq!(answered.variables.iter().filter(|value| **value != 0).count(), 1);
    // Started in tick 0, running from tick 1: two ticks by tick 3.
    assert_eq!(answered.timers[7], TimerState { running: true, elapsed: 2 });
    assert_eq!(answered.timers[..7], [TimerState::default(); 7]);
    assert_eq!(
        answered.sequences.as_slice(),
        [
            SequenceState { running: true, passes: 0, position: 3, waiting: false },
            SequenceState { running: false, passes: 0, position: 0, waiting: true }
        ]
    );
    assert_eq!(answered.targets.len(), 1);
    let target = answered.targets[0];
    assert_eq!(
        (target.target, target.exists, target.shown, target.frame, target.x, target.y),
        (object, true, true, 3, 700, -380)
    );
    assert_eq!(answered.values.as_slice(), [(is_shown, 1), (Value::sound(2, SOUND_GAIN), 55)]);
}

/// RULE-5, RULE-111 (REQ-6526, REQ-6615): a page closed while its
/// connection stands runs one `close` handling, in which only the `close`
/// rules fire; the events still waiting are dropped and counted; nothing
/// is evaluated or handled after it.
#[test]
fn close_runs_the_close_rules_once() {
    let mut table = empty();
    let closing = mark(&mut table, 8);
    let ticking = mark(&mut table, 7);
    rule(&mut table, Trigger::Close, closing);
    rule(&mut table, Trigger::Every { timer: PAGE_TIMER, ticks: 1 }, ticking);
    rule(&mut table, on_key('a'), ticking);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    run(&mut evaluator, &mut world, 3);
    assert_eq!(readings(&world), [7, 7]);
    world.log.clear();
    for sequence in 0..=u32::try_from(MAX_EVENTS_PER_TICK).unwrap() {
        give(&mut evaluator, &mut world, press_key(sequence, 'z'));
    }
    assert_eq!(evaluator.waiting(), 1);
    evaluator.close(&mut world);
    assert_eq!(readings(&world), [8], "the `close` rules alone: no timer, no animation");
    assert_eq!(world.reports()[0].tick, 2);
    assert_eq!(evaluator.counters().events_dropped, 1, "the event that waited");
    assert!(evaluator.is_closed());
    world.log.clear();
    evaluator.tick(&mut world);
    evaluator.close(&mut world);
    assert!(!give(&mut evaluator, &mut world, press_key(1, 'a')));
    assert!(!send(&mut evaluator, &mut world, set(0, 1).command()));
    assert_eq!(world.log, []);
}

fn command(built: &Built) -> Event<'_> {
    Event::Command { kind: built.kind, body: &built.body }
}

/// RULE-5, RULE-57 (REQ-6608): one queue for every kind of event: before
/// tick step 0, `MAX_QUEUED_EVENTS` events of every kind wait and one more
/// is dropped and counted; a record of the server's for another page, one
/// its reader refuses and one the evaluator does not carry out are
/// refused and counted as skipped.
#[test]
fn the_queue_is_held_to_its_limit() {
    let mut evaluator = evaluator(&empty());
    let mut world = Script::default();
    for sequence in 0..MAX_QUEUED_EVENTS {
        let event = match sequence % 3 {
            0 => Event::Input(press_key(u32::try_from(sequence).unwrap(), 'a')),
            1 => Event::Cue { channel: 0, number: 1 },
            _ => Event::Query(7),
        };
        assert!(evaluator.handle(event, &mut world), "{sequence}");
    }
    assert!(!send(&mut evaluator, &mut world, set(0, 1).command()));
    assert_eq!(evaluator.counters().events_dropped, 1);
    let mut elsewhere = set(0, 1);
    elsewhere.body[3] = 2;
    assert!(!evaluator.handle(command(&elsewhere), &mut world), "another sub-code is another page");
    let refused = Event::Command { kind: TYPE_VARIABLE, body: &[1, 1, 0, 1, 99, 0, 0] };
    assert!(!evaluator.handle(refused, &mut world), "no such operation");
    let midi = note(60);
    assert!(!evaluator.handle(command(&midi), &mut world), "LOCAL_MIDI is the receiver's to apply");
    assert_eq!(evaluator.counters().skipped, 3);
}
