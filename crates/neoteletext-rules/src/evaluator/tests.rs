//! The evaluator (REQ-6526, REQ-6612, REQ-6613, REQ-6615; RULE-5 to
//! RULE-8, RULE-96 and the rules they cite).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, plain arithmetic on small numbers and `panic!` on outcomes it rules out"
)]

mod arrival;
mod commands;
mod triggers;

use std::boxed::Box;
use std::vec::Vec;

use super::*;
use crate::command::{Command, Operand, RunMode, VariableOperation};
use crate::limits::{MAX_QUEUED_COMMANDS, MAX_QUEUED_EVENTS};
use crate::table::{
    Key, KeyKind, KeyMatch, Operator, Rule, Sequence, Step, TIMER_ELAPSED, Table, TriggerTarget, condition,
};
use crate::testing::{Built, Out, PAGE, SUB, Script, body_check, built, group};

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
pub(super) fn send(evaluator: &mut Evaluator, command: Command) -> bool {
    let sent = built(command);
    evaluator.push_command(sent.kind, &sent.body)
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

/// RULE-5 (REQ-6526): the fixed order within a tick. `open` takes page
/// tick 0 to itself; in a later tick the input's rule, the hold, the
/// server's command, the armed run, the timer, the sequence's step, the
/// cue and `becomes` run in that order, then animation; and the same
/// table with the same script gives the same outputs on every run.
#[test]
fn the_order_within_a_tick_is_fixed() {
    let play = || {
        let mut table = empty();
        let groups: Vec<u8> = (2..=10).map(|number| mark(&mut table, number)).collect();
        let of = |step: usize| groups[step - 2];
        rule(&mut table, Trigger::Becomes, of(10));
        let elapsed = Value::timer(PAGE_TIMER, TIMER_ELAPSED);
        table.rules.as_mut_slice()[0].condition = condition(&[Node::compare(elapsed, Operator::Equal, 3)]);
        rule(&mut table, Trigger::Cue { channel: 0, number: ANY_CUE }, of(9));
        rule(&mut table, Trigger::After { timer: PAGE_TIMER, ticks: 3 }, of(7));
        rule(&mut table, Trigger::Held { keys: keys_of("h"), ticks: 1 }, of(4));
        rule(&mut table, on_key('a'), of(3));
        rule(&mut table, Trigger::Open, of(2));
        let mut steps = Sequence::new(100, 1);
        steps.steps.push(Step { offset: 0, group: of(8) }).unwrap();
        table.sequences.push(steps).unwrap();
        let mut evaluator = evaluator(&table);
        let mut world = Script::default();
        evaluator.tick(&mut world);
        assert_eq!(readings(&world), [2], "`open` alone, at page tick 0");
        assert_eq!(world.log.last(), Some(&Out::Cycles(0)));
        evaluator.tick(&mut world);
        // In tick 2: `h` goes down, a run is armed for the next tick and
        // the sequence is started; none of them is seen until tick 3.
        assert!(evaluator.push(press_key(1, 'h')));
        assert!(send(&mut evaluator, Command::Run { group: of(6), mode: RunMode::After(1) }));
        assert!(send(&mut evaluator, Command::Sequence { start: true, sequence: 0 }));
        evaluator.tick(&mut world);
        assert_eq!(readings(&world), [2], "nothing fires in tick 2");
        world.log.clear();
        assert!(evaluator.push(press_key(2, 'a')));
        assert!(send(&mut evaluator, set(15, 5).command()));
        assert!(send(&mut evaluator, report_variable(15).command()));
        assert!(evaluator.push_cue(0, 7));
        evaluator.tick(&mut world);
        assert_eq!(readings(&world), [3, 4, 5, 6, 7, 8, 9, 10], "steps 3 to 10, in order");
        let rules: Vec<u8> = world.reports().iter().map(|report| report.rule).collect();
        assert_eq!(rules, [4, 3, NO_RULE, NO_RULE, 2, NO_RULE, 1, 0]);
        assert!(world.reports().iter().all(|report| report.tick == 3));
        assert_eq!(world.log.last(), Some(&Out::Cycles(3)), "animation is step 11, after every rule");
        (world.log, *evaluator.counters(), *evaluator.variables())
    };
    assert_eq!(play(), play());
}

/// RULE-6 (REQ-6526): no cascade within a tick. A group sets a variable
/// and a `becomes` rule on it fires in the next tick, never the same; a
/// sequence a key starts runs its offset 0 step in the next tick; and a
/// group that stopping its own sequence still completes.
#[test]
fn nothing_a_command_does_is_seen_in_its_own_tick() {
    let mut table = empty();
    let sets = group(&mut table, &[set(0, 1), built(Command::Sequence { start: true, sequence: 0 })]);
    let became = mark(&mut table, 10);
    let stopping =
        group(&mut table, &[built(Command::Sequence { start: false, sequence: 0 }), set(15, 8), report_variable(15)]);
    rule(&mut table, on_key('a'), sets);
    let becomes = rule(&mut table, Trigger::Becomes, became);
    table.rules.as_mut_slice()[becomes].condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 1)]);
    let mut steps = Sequence::new(4, 0);
    steps.steps.push(Step { offset: 0, group: stopping }).unwrap();
    table.sequences.push(steps).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push(press_key(1, 'a'));
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), [] as [i32; 0], "tick 1: the key's group ran, and nothing saw it");
    assert_eq!(evaluator.variables()[0], 1);
    evaluator.tick(&mut world);
    let ticks: Vec<(u32, i32)> = world
        .reports()
        .iter()
        .map(|report| match report.what {
            Reported::Value { reading, .. } => (report.tick, reading),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        ticks,
        [(2, 8), (2, 10)],
        "tick 2: the sequence's step, which stopping it and completes; then `becomes`"
    );
    run(&mut evaluator, &mut world, 8);
    assert_eq!(world.reports().len(), 2, "the sequence stopped itself, and `becomes` is an edge");
}

/// RULE-7 (REQ-6526): conditions read the snapshot; commands write, and
/// read variables from, the working state. A `LOCAL_MIDI` after `inc v0`
/// whose key is `60 + v0` plays the new key; a condition on `v0` in the
/// same tick reads the old value.
#[test]
fn commands_read_the_working_variables_and_conditions_the_snapshot() {
    let mut table = empty();
    let inc = built(Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None });
    let bound = note(60).bound(Binding::of(command::field::MIDI_DATA1, Source::Variable(0)));
    let plays = group(&mut table, &[inc, bound]);
    let old = mark(&mut table, 1);
    rule(&mut table, on_key('a'), plays);
    let second = rule(&mut table, on_key('a'), old);
    table.rules.as_mut_slice()[second].condition = condition(&[Node::compare(Value::variable(0), Operator::Equal, 0)]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push(press_key(1, 'a'));
    evaluator.tick(&mut world);
    assert_eq!(notes(&world), [61], "the amount reads the variable its own group just changed");
    assert_eq!(readings(&world), [1], "the second rule's condition read v0 as the last tick left it");
    world.log.clear();
    evaluator.push(press_key(2, 'a'));
    evaluator.tick(&mut world);
    assert_eq!((notes(&world), readings(&world)), (std::vec![62], std::vec![]), "and now it reads 1");
}

/// RULE-8 (REQ-6526): input state follows the events. Shift and A in one
/// tick fire the rule on A-with-Shift; A before Shift does not; and the
/// releases the receiver gives when the page loses the keyboard fire the
/// `key_up` rules in the order given.
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
    evaluator.push(press_key(1, 'a'));
    evaluator.push(shift(2, KeyAction::Press));
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), [] as [i32; 0], "A arrived before Shift");
    evaluator.push(key(3, KeyAction::Release, 'a'));
    evaluator.push(shift(4, KeyAction::Release));
    evaluator.push(shift(5, KeyAction::Press));
    evaluator.push(press_key(6, 'a'));
    evaluator.tick(&mut world);
    assert_eq!(readings(&world), [1], "Shift, then A, in one tick");
    world.log.clear();
    // The page loses the keyboard: a release for each key still down.
    evaluator.push(shift(7, KeyAction::Release));
    evaluator.push(key(8, KeyAction::Release, 'a'));
    evaluator.tick(&mut world);
    let released: Vec<Reported> = world.reports().iter().map(|report| report.what).collect();
    assert_eq!(released, [Reported::Input { sequence: 7 }, Reported::Input { sequence: 8 }]);
}

/// RULE-47 (REQ-6526): a state query is answered once, at the end of the
/// next tick, with the snapshot that tick read: the sixteen variables,
/// the eight timers, the table's sequences, the armed runs waiting, and
/// each target and value the table names.
#[test]
fn a_state_query_is_answered_with_the_snapshot() {
    let mut table = empty();
    let reads = group(&mut table, &[built(Command::Report { value: Some(Value::sound(2, SOUND_GAIN)) })]);
    let shown = rule(&mut table, Trigger::Becomes, reads);
    let object = Target::object(301);
    let is_shown = Value::target(TargetKind::Object, 301, TARGET_SHOWN);
    table.rules.as_mut_slice()[shown].condition = condition(&[Node::compare(is_shown, Operator::Equal, 7)]);
    table.sequences.push(Sequence::new(10, 0)).unwrap();
    table.sequences.as_mut_slice()[0].steps.push(Step { offset: 9, group: reads }).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    world.targets.push((object, TargetState { frame: 3, x: 700, y: -380, ..shown_at(0, 0, 4, 4) }));
    world.sounds[2].gain = 55;
    send(&mut evaluator, set(3, -9).command());
    send(&mut evaluator, Command::Timer { start: true, timer: 7 });
    send(&mut evaluator, Command::Sequence { start: true, sequence: 0 });
    send(&mut evaluator, Command::Run { group: reads, mode: RunMode::After(100) });
    run(&mut evaluator, &mut world, 3);
    evaluator.query(42);
    evaluator.tick(&mut world);
    let Some(Out::State(state)) = world.log.last() else { panic!("{:?}", world.log.last()) };
    let answered = state.table.unwrap();
    assert_eq!((state.identifier, answered.generation, answered.tick), (42, 1, 3));
    assert_eq!(answered.variables[3], -9);
    assert_eq!(answered.variables.iter().filter(|value| **value != 0).count(), 1);
    // Started in tick 0, running since tick 1: two ticks by tick 3.
    assert_eq!(answered.timers[7], TimerState { running: true, elapsed: 2 });
    assert_eq!(answered.timers[..7], [TimerState::default(); 7]);
    assert_eq!(answered.sequences.as_slice(), [SequenceState { running: true, passes: 0, position: 2 }]);
    assert_eq!(answered.armed, 1);
    assert_eq!(answered.targets.len(), 1);
    let target = answered.targets[0];
    assert_eq!(
        (target.target, target.exists, target.shown, target.frame, target.x, target.y),
        (object, true, true, 3, 700, -380)
    );
    assert_eq!(answered.values.as_slice(), [(is_shown, 1), (Value::sound(2, SOUND_GAIN), 55)]);
    world.log.clear();
    evaluator.tick(&mut world);
    assert_eq!(world.log, [Out::Cycles(4)], "answered once");
}

/// RULE-5, RULE-111 (REQ-6526, REQ-6615): a page closed while its
/// connection stands runs one final tick, in which only the `close`
/// rules fire; nothing is evaluated after it.
#[test]
fn close_runs_the_close_rules_in_one_final_tick() {
    let mut table = empty();
    let closing = mark(&mut table, 8);
    let ticking = mark(&mut table, 7);
    rule(&mut table, Trigger::Close, closing);
    rule(&mut table, Trigger::Every { timer: PAGE_TIMER, ticks: 1 }, ticking);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    run(&mut evaluator, &mut world, 3);
    assert_eq!(readings(&world), [7, 7]);
    world.log.clear();
    evaluator.close(&mut world);
    assert_eq!(readings(&world), [8], "the `close` rules alone: no timer, no animation");
    assert_eq!(world.reports()[0].tick, 3);
    assert!(evaluator.is_closed());
    world.log.clear();
    evaluator.tick(&mut world);
    evaluator.close(&mut world);
    assert!(!evaluator.push_now(press_key(1, 'a'), &mut world) || world.log.is_empty());
    assert_eq!(world.log, []);
}

/// RULE-5, RULE-57 (REQ-6608): the queues are held to their limits: one
/// input event over `MAX_QUEUED_EVENTS` and one command of the server
/// over `MAX_QUEUED_COMMANDS` are dropped and counted, and a command for
/// another page, or one its reader refuses, is skipped and counted.
#[test]
fn the_queues_are_held_to_their_limits() {
    let mut evaluator = evaluator(&empty());
    for sequence in 0..MAX_QUEUED_EVENTS {
        assert!(evaluator.push(press_key(u32::try_from(sequence).unwrap(), 'a')), "{sequence}");
    }
    assert!(!evaluator.push(press_key(99, 'a')));
    assert_eq!(evaluator.counters().events_dropped, 1);
    for _ in 0..MAX_QUEUED_COMMANDS {
        assert!(send(&mut evaluator, set(0, 1).command()));
    }
    assert!(!send(&mut evaluator, set(0, 1).command()));
    assert_eq!(evaluator.counters().commands_dropped, 1);
    let mut elsewhere = set(0, 1);
    elsewhere.body[3] = 2;
    assert!(!evaluator.push_command(elsewhere.kind, &elsewhere.body), "another sub-code is another page");
    assert!(!evaluator.push_command(TYPE_VARIABLE, &[1, 1, 0, 1, 99, 0, 0]), "no such operation");
    let midi = note(60);
    assert!(!evaluator.push_command(midi.kind, &midi.body), "LOCAL_MIDI is the receiver's to apply on arrival");
    assert_eq!(evaluator.counters().skipped, 3);
}
