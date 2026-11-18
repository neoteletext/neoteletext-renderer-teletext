//! The evaluator (REQ-6526): the tick's fixed order and every rule of
//! RULE-4 to RULE-28 it holds, against a scripted world.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing and arithmetic on data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use crate::animate::OperationKind;
use crate::limits::{ANY_BUTTON, MAX_KEYS_DOWN};
use crate::table::{
    CLASS_NOTES, INPUT_NAMED, KeyKind, Operator, POINTER_OVER, POINTER_X, Rule, SubpageMask, TARGET_INSIDE, Variable,
    any_key, no_condition,
};
use std::boxed::Box;
use std::collections::BTreeMap;
use std::vec::Vec;

/// What the scripted world was given, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Out {
    Sound(SoundOperation),
    Animate(Operation, u32),
    Report(Report),
    Cycles(u32),
    State(Box<PageState>),
}

/// A world whose answers a test sets, and which keeps what it is given.
#[derive(Default)]
struct Script {
    targets: BTreeMap<(u8, u16), TargetState>,
    sounds: [SoundState; AUDIO_CHANNELS],
    synth: BTreeMap<(u8, u8, u8, u16), i32>,
    /// The handles a `play` finds.
    handles: Vec<u8>,
    log: Vec<Out>,
}

fn shown_at(left: i32, top: i32, right: i32, bottom: i32) -> TargetState {
    TargetState {
        shown: true,
        cycling: false,
        frame: 0,
        look: 0,
        layer: 1,
        order: 0,
        x: i16::try_from(left).unwrap(),
        y: i16::try_from(top).unwrap(),
        bounds: Some(Bounds { left, top, right, bottom }),
    }
}

impl Script {
    fn with(mut self, target: Target, state: TargetState) -> Self {
        self.targets.insert((target.kind.code(), target.number), state);
        self
    }

    fn state(&self, target: Target) -> TargetState {
        self.targets[&(target.kind.code(), target.number)]
    }

    /// The reports given, as rule and what.
    fn reports(&self) -> Vec<(u8, Reported)> {
        self.log
            .iter()
            .filter_map(|out| if let Out::Report(report) = out { Some((report.rule, report.what)) } else { None })
            .collect()
    }

    /// The rules that reported, in order.
    fn rules(&self) -> Vec<u8> {
        self.reports().iter().map(|(rule, _)| *rule).collect()
    }

    fn take_rules(&mut self) -> Vec<u8> {
        let rules = self.rules();
        self.log.clear();
        rules
    }
}

impl World for Script {
    fn picture(&self) -> (u16, u16) {
        (800, 480)
    }

    fn target(&self, target: Target) -> Option<TargetState> {
        self.targets.get(&(target.kind.code(), target.number)).copied()
    }

    fn objects_at(&self, x: u16, y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize {
        let pixel = Bounds { left: i32::from(x), top: i32::from(y), right: i32::from(x) + 1, bottom: i32::from(y) + 1 };
        let mut count = 0;
        for ((kind, number), state) in &self.targets {
            let inside = state.shown && state.bounds.is_some_and(|bounds| pixel.within(&bounds));
            if *kind == TargetKind::Object.code() && inside && count < MAX_POINTER_OBJECTS {
                objects[count] = *number;
                count += 1;
            }
        }
        count
    }

    fn sound(&self, channel: u8) -> SoundState {
        self.sounds[usize::from(channel)]
    }

    fn synth(&self, channel: u8, part: u8, class: u8, index: u16) -> i32 {
        self.synth.get(&(channel, part, class, index)).copied().unwrap_or(0)
    }

    fn sound_operation(&mut self, operation: SoundOperation) -> bool {
        self.log.push(Out::Sound(operation));
        match operation {
            SoundOperation::Play { handle, .. } => self.handles.contains(&handle),
            _ => true,
        }
    }

    /// The effect a renderer's animation state would have, so that the
    /// next tick's snapshot shows it.
    fn animate(&mut self, operation: Operation, tick: u32) -> bool {
        self.log.push(Out::Animate(operation, tick));
        let Some(state) = self.targets.get_mut(&(operation.target.kind.code(), operation.target.number)) else {
            return false;
        };
        match operation.kind {
            OperationKind::Show => state.shown = true,
            OperationKind::Hide => state.shown = false,
            OperationKind::Look(look) => state.look = look,
            OperationKind::Move { relative, x, y } => {
                let (dx, dy) = if relative { (x, y) } else { (x - state.x, y - state.y) };
                (state.x, state.y) = (state.x + dx, state.y + dy);
                if let Some(bounds) = &mut state.bounds {
                    bounds.left += i32::from(dx);
                    bounds.right += i32::from(dx);
                    bounds.top += i32::from(dy);
                    bounds.bottom += i32::from(dy);
                }
            }
            OperationKind::Order { layer, order } => {
                if layer != crate::limits::KEEP_LAYER {
                    state.layer = layer;
                }
                state.order = order;
            }
            OperationKind::Frame(frame) => (state.frame, state.cycling) = (frame, false),
            OperationKind::Cycle { first, .. } => (state.frame, state.cycling) = (first, true),
            OperationKind::StopCycle => state.cycling = false,
            OperationKind::Reset => {}
        }
        true
    }

    fn report(&mut self, report: Report) {
        self.log.push(Out::Report(report));
    }

    fn advance_cycles(&mut self, tick: u32) {
        self.log.push(Out::Cycles(tick));
    }

    fn page_state(&mut self, state: &PageState) {
        self.log.push(Out::State(Box::new(*state)));
    }
}

fn rule(trigger: Trigger, actions: &[Action]) -> Rule {
    let mut rule = Rule::new(trigger);
    for action in actions {
        rule.actions.push(*action).unwrap();
    }
    rule
}

fn when(mut rule: Rule, nodes: &[Node]) -> Rule {
    rule.condition = no_condition();
    for node in nodes {
        rule.condition.push(*node).unwrap();
    }
    rule
}

fn is(value: Value, constant: i32) -> Node {
    Node::Compare { value, operator: Operator::Equal, constant }
}

fn above(value: Value, constant: i32) -> Node {
    Node::Compare { value, operator: Operator::Greater, constant }
}

const SEND: Action = Action::Send(None);

fn table_of(rules: &[Rule]) -> Box<Table> {
    let mut table = Box::new(Table::new());
    for rule in rules {
        table.rules.push(*rule).unwrap();
    }
    table
}

/// An evaluator for `table`, which the codec must accept.
fn evaluator(table: &Table) -> Box<Evaluator> {
    let mut out = [0_u8; crate::limits::MAX_TABLE_BYTES];
    crate::encode(0x101, 1, Some(table), &mut out).unwrap();
    Box::new(Evaluator::new(table, 1, 0x0001).unwrap())
}

fn character(code: char) -> KeyIdentity {
    KeyIdentity::Character(u32::from(code))
}

fn key_event(sequence: u32, action: KeyAction, key: KeyIdentity) -> InputEvent {
    InputEvent { sequence, input: Input::Key { action, key, base: None, mods: 0 } }
}

fn press_key(sequence: u32, code: char) -> InputEvent {
    key_event(sequence, KeyAction::Press, character(code))
}

fn release_key(sequence: u32, code: char) -> InputEvent {
    key_event(sequence, KeyAction::Release, character(code))
}

fn pointer(sequence: u32, action: PointerAction, button: u8, x: u16, y: u16, control: u16) -> InputEvent {
    InputEvent { sequence, input: Input::Pointer { action, button, x, y, control } }
}

fn on_key(code: char) -> Trigger {
    let mut keys = any_key();
    keys.push(crate::table::Key { kind: KeyKind::Character, code: u32::from(code) }).unwrap();
    Trigger::Key { keys, repeat: false, mods_mask: 0, mods_value: 0 }
}

fn any_key_down() -> Trigger {
    Trigger::Key { keys: any_key(), repeat: false, mods_mask: 0, mods_value: 0 }
}

fn variable(table: &mut Table, min: i32, max: i32, initial: i32) -> u8 {
    u8::try_from(table.variables.push(Variable { min, max, initial }).unwrap()).unwrap()
}

/// RULE-5 (REQ-6526): one table with a rule of every trigger kind, each
/// sending; the order of the sends in a tick is exactly steps 2 to 5,
/// whatever order the rules are written in, and animation is advanced
/// before the outputs leave.
#[test]
fn the_tick_runs_in_its_fixed_order() {
    let mut table = Table::new();
    table.timers.push(true).unwrap();
    let flag = variable(&mut table, 0, 1, 0);
    // Written in the reverse of the order they must fire in.
    let rules = [
        when(rule(Trigger::Becomes, &[SEND]), &[is(Value::variable(flag), 1)]),
        when(rule(Trigger::Becomes, &[SEND]), &[is(Value::variable(flag), 0)]),
        rule(Trigger::Open, &[SEND, Action::Set { variable: flag, value: 1 }]),
        rule(Trigger::Every { timer: 0, ticks: 1 }, &[SEND]),
        rule(Trigger::After { timer: PAGE_TIMER, ticks: 1 }, &[SEND]),
        rule(Trigger::Blur, &[SEND]),
        rule(Trigger::Text, &[SEND]),
        rule(any_key_down(), &[SEND]),
        rule(Trigger::Close, &[SEND]),
    ];
    for rule in rules {
        table.rules.push(rule).unwrap();
    }
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    // Tick 0: two events in arrival order, then `open`, then `becomes`.
    evaluator.push(press_key(1, 'a'));
    evaluator.push(InputEvent { sequence: 2, input: Input::Text });
    evaluator.tick(&mut world);
    assert_eq!(world.rules(), [7, 6, 2, 1]);
    assert_eq!(world.log[0], Out::Cycles(0), "step 6 is before step 7's outputs");
    world.log.clear();
    // Tick 1: the events (text before blur, as they arrived, though blur
    // is the earlier rule), the page timer's rule before timer 0's, then
    // `becomes`. `open` does not fire again.
    evaluator.push(InputEvent { sequence: 3, input: Input::Text });
    evaluator.push(InputEvent { sequence: 4, input: Input::Blur });
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [6, 5, 4, 3, 0]);
    // The final tick: `close` alone, with no event, timer or `becomes`.
    evaluator.push(InputEvent { sequence: 5, input: Input::Text });
    evaluator.close(&mut world);
    assert_eq!(world.reports(), [(8, Reported::Fired)]);
    assert!(evaluator.is_closed());
    world.log.clear();
    evaluator.tick(&mut world);
    assert!(world.log.is_empty(), "nothing is evaluated after the final tick");
}

/// RULE-5 (REQ-6526): the same table and the same event script give the
/// same outputs on every run.
#[test]
fn the_same_script_gives_the_same_outputs() {
    let mut table = Table::new();
    let count = variable(&mut table, 0, 9, 0);
    table.rules.push(rule(any_key_down(), &[Action::Add { variable: count, amount: 1 }])).unwrap();
    table
        .rules
        .push(when(
            rule(Trigger::Becomes, &[Action::Send(Some(Value::variable(count)))]),
            &[above(Value::variable(count), 2)],
        ))
        .unwrap();
    table.rules.push(rule(Trigger::Every { timer: PAGE_TIMER, ticks: 3 }, &[SEND])).unwrap();
    let run = || {
        let mut evaluator = evaluator(&table);
        let mut world = Script::default();
        for tick in 0..20_u32 {
            if tick % 2 == 0 {
                evaluator.push(press_key(tick, 'x'));
                evaluator.push(release_key(tick, 'x'));
            }
            evaluator.tick(&mut world);
        }
        (world.log, *evaluator.counters())
    };
    let first = run();
    assert_eq!(first, run());
    assert!(first.0.iter().any(|out| matches!(out, Out::Report(report) if report.rule == 1)));
}

/// RULE-5 (REQ-6526): at most 16 events a tick; later ones wait, in a
/// queue of at most 64; one more is dropped and counted.
#[test]
fn events_wait_in_a_bounded_queue() {
    let table = table_of(&[rule(any_key_down(), &[SEND])]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    for sequence in 0..64 {
        assert!(evaluator.push(press_key(sequence, 'a')));
    }
    assert!(!evaluator.push(press_key(64, 'a')));
    assert_eq!(evaluator.counters().events_dropped, 1);
    let sequences = |world: &Script| -> Vec<u32> {
        world
            .reports()
            .iter()
            .map(|(_, what)| if let Reported::Input { sequence } = what { *sequence } else { panic!("{what:?}") })
            .collect()
    };
    evaluator.tick(&mut world);
    assert_eq!(sequences(&world), (0..16).collect::<Vec<_>>());
    for _ in 0..3 {
        evaluator.tick(&mut world);
    }
    assert_eq!(sequences(&world), (0..64).collect::<Vec<_>>());
    world.log.clear();
    evaluator.tick(&mut world);
    assert_eq!(world.reports(), []);
    // The queue is a ring: it takes 64 more.
    for sequence in 0..64 {
        assert!(evaluator.push(press_key(sequence, 'a')));
    }
}

/// RULE-6 (REQ-6526): rule A sets a variable and rule B's `becomes` on it
/// fires in the next tick, never the same; a rule that disables itself
/// runs all its actions.
#[test]
fn nothing_cascades_within_a_tick() {
    let mut table = Table::new();
    let v = variable(&mut table, 0, 9, 0);
    table.rules.push(rule(on_key('a'), &[Action::Set { variable: v, value: 5 }])).unwrap();
    table.rules.push(when(rule(Trigger::Becomes, &[SEND]), &[is(Value::variable(v), 5)])).unwrap();
    table.rules.push(rule(on_key('d'), &[Action::Disable(1 << 2), SEND, SEND])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    evaluator.push(press_key(1, 'a'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [0_u8; 0], "the `becomes` rule read the tick's start");
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [1]);
    // The rule that disables itself completes, and is off from the next
    // tick.
    evaluator.push(press_key(2, 'd'));
    evaluator.push(press_key(3, 'd'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [2, 2, 2, 2], "both events of the tick still found it enabled");
    evaluator.push(press_key(4, 'd'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [0_u8; 0]);
}

/// RULE-6 (REQ-6526): a table built to fire 64 rules on each of 16 events
/// stops at 256 actions; the firings beyond are dropped whole and
/// counted.
#[test]
fn a_tick_stops_at_256_actions() {
    let mut table = Box::new(Table::new());
    let v = variable(&mut table, 0, i32::MAX, 0);
    let sixteen = [Action::Add { variable: v, amount: 1 }; MAX_ACTIONS];
    for _ in 0..MAX_RULES {
        table.rules.push(rule(any_key_down(), &sixteen)).unwrap();
    }
    // A last firing that would pass the cap is dropped whole, though some
    // of its actions would fit: 15 firings of 16, then one of 17.
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    for sequence in 0..16 {
        evaluator.push(press_key(sequence, 'a'));
    }
    evaluator.query(1);
    evaluator.tick(&mut world);
    let counters = *evaluator.counters();
    assert_eq!((counters.actions, counters.fired, counters.overruns), (256, 16, 64 * 16 - 16));
    evaluator.query(2);
    evaluator.tick(&mut world);
    let Some(Out::State(state)) = world.log.last() else { panic!("no answer") };
    assert_eq!(state.table.unwrap().variables.as_slice(), [256]);
    let mut echoing = table_of(&[rule(any_key_down(), &sixteen)]);
    echoing.variables.push(Variable { min: 0, max: i32::MAX, initial: 0 }).unwrap();
    echoing.rules.as_mut_slice()[0].echo = true;
    let mut evaluator = self::evaluator(&echoing);
    for sequence in 0..16 {
        evaluator.push(press_key(sequence, 'a'));
    }
    evaluator.tick(&mut world);
    // 17 actions a firing: fifteen fit in 256, the sixteenth does not.
    assert_eq!((evaluator.counters().actions, evaluator.counters().overruns), (255, 1));
}

/// RULE-7 (REQ-6526): `set v 5` then `add v 1` in one tick leave 6; a
/// condition and a `send` of `v` in the same tick read the old value.
#[test]
fn reads_come_from_the_snapshot_and_writes_accumulate() {
    let mut table = Table::new();
    let v = variable(&mut table, 0, 9, 0);
    let read = Action::Send(Some(Value::variable(v)));
    table
        .rules
        .push(rule(on_key('a'), &[Action::Set { variable: v, value: 5 }, Action::Add { variable: v, amount: 1 }, read]))
        .unwrap();
    table.rules.push(when(rule(on_key('a'), &[SEND]), &[is(Value::variable(v), 0)])).unwrap();
    table.rules.push(rule(on_key('b'), &[read])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.push(press_key(1, 'a'));
    evaluator.tick(&mut world);
    let value = Value::variable(v);
    assert_eq!(world.reports(), [(0, Reported::Value { value, reading: 0 }), (1, Reported::Input { sequence: 1 })]);
    world.log.clear();
    evaluator.push(press_key(2, 'b'));
    evaluator.tick(&mut world);
    assert_eq!(world.reports(), [(2, Reported::Value { value, reading: 6 })]);
}

/// RULE-8 (REQ-6526): Shift and A in one tick fire the rule on
/// A-with-Shift; a blur with two keys down fires both `key_up` rules, by
/// key arrival; a 17th key is not down for conditions, though its trigger
/// fires.
#[test]
fn input_state_follows_the_events() {
    const SHIFT: u32 = 80;
    let shift_down = Value::key(INPUT_NAMED, SHIFT);
    let key_up = |code: char| {
        let Trigger::Key { keys, .. } = on_key(code) else { panic!() };
        Trigger::KeyUp { keys, mods_mask: 0, mods_value: 0 }
    };
    let table = table_of(&[
        when(rule(on_key('a'), &[SEND]), &[is(shift_down, 1)]),
        rule(key_up('b'), &[SEND]),
        rule(key_up('a'), &[SEND]),
        when(rule(Trigger::Becomes, &[Action::Send(Some(Value::keys_down()))]), &[above(Value::keys_down(), 15)]),
        when(rule(any_key_down(), &[SEND]), &[above(Value::keys_down(), 15)]),
    ]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.push(key_event(1, KeyAction::Press, KeyIdentity::Named(SHIFT)));
    evaluator.push(press_key(2, 'a'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [0]);
    // Without Shift the rule does not fire.
    evaluator.push(key_event(3, KeyAction::Release, KeyIdentity::Named(SHIFT)));
    evaluator.push(release_key(4, 'a'));
    evaluator.push(press_key(5, 'a'));
    evaluator.push(press_key(6, 'b'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [2], "the release of a, and no press with Shift");
    // The page loses the keyboard: the receiver gives a release for each
    // key still down, a first.
    evaluator.push(release_key(7, 'a'));
    evaluator.push(release_key(8, 'b'));
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [2, 1]);
    // Sixteen keys are tracked; the 17th press, which waits for the
    // second tick, fires its trigger and reads as not down.
    for (sequence, code) in (0_u32..).zip('a'..='q') {
        evaluator.push(press_key(sequence, code));
    }
    evaluator.tick(&mut world);
    evaluator.tick(&mut world);
    let sixteen = Reported::Value { value: Value::keys_down(), reading: i32::try_from(MAX_KEYS_DOWN).unwrap() };
    assert_eq!(
        world.reports(),
        [(4, Reported::Input { sequence: 15 }), (3, sixteen), (4, Reported::Input { sequence: 16 })]
    );
}

/// RULE-9 (REQ-6526): for each kind of trigger, the event that fires it
/// and the nearest events that must not.
#[test]
fn each_trigger_fires_on_its_event_alone() {
    let mut table = Box::new(Table::new());
    table.rects.push(Rect { x: 100, y: 100, width: 10, height: 10 }).unwrap();
    let object = Target::object(10);
    let control = Target::control(3);
    let Trigger::Key { keys, .. } = on_key('a') else { panic!() };
    let rules = [
        Trigger::Key { keys, repeat: true, mods_mask: 0b101, mods_value: 0b001 },
        Trigger::KeyUp { keys, mods_mask: 0, mods_value: 0 },
        Trigger::Press { target: control, button: 0 },
        Trigger::Release { target: control, button: ANY_BUTTON },
        Trigger::Enter { target: object },
        Trigger::Leave { target: object },
        Trigger::Text,
        Trigger::Move { target: Target::PAGE, held: true },
        Trigger::Wheel { target: Target::rect(0) },
        Trigger::Focus,
        Trigger::Blur,
        on_key('z'),
    ];
    for trigger in rules {
        table.rules.push(rule(trigger, &[SEND])).unwrap();
    }
    let mut evaluator = evaluator(&table);
    let mut world = Script::default().with(object, shown_at(200, 200, 210, 210));
    let mut fired = |events: &[Input]| {
        for (sequence, input) in (0_u32..).zip(events) {
            evaluator.push(InputEvent { sequence, input: *input });
        }
        evaluator.tick(&mut world);
        world.take_rules()
    };
    let key = |action, code: char, mods| Input::Key { action, key: character(code), base: None, mods };
    // `key`: a press and a repeat with Shift and without Control; not
    // another key, not without Shift, not with Control, not a release.
    assert_eq!(fired(&[key(KeyAction::Press, 'a', 1), key(KeyAction::Repeat, 'a', 0b11)]), [0, 0]);
    assert_eq!(fired(&[key(KeyAction::Press, 'b', 1), key(KeyAction::Press, 'a', 0)]), [0_u8; 0]);
    assert_eq!(fired(&[key(KeyAction::Press, 'a', 0b101)]), [0_u8; 0]);
    // `key_up`: the release, whatever the modifiers.
    assert_eq!(fired(&[key(KeyAction::Release, 'a', 1), key(KeyAction::Release, 'b', 0)]), [1]);
    // A press of 'z' without `repeat` does not fire on a repeat.
    assert_eq!(fired(&[key(KeyAction::Repeat, 'z', 0), key(KeyAction::Press, 'z', 0)]), [11]);
    let at = |action, button, x, y, control| Input::Pointer { action, button, x, y, control };
    // `press` on control 3 with the left button; not the right, not
    // another control.
    assert_eq!(fired(&[at(PointerAction::Press, 2, 5, 5, 3), at(PointerAction::Release, 2, 5, 5, 3)]), [3]);
    assert_eq!(fired(&[at(PointerAction::Press, 0, 5, 5, 4), at(PointerAction::Release, 0, 5, 5, 4)]), [0_u8; 0]);
    // `release`: the button that went down inside the control comes up,
    // wherever the pointer then is; a release with no press is nothing.
    assert_eq!(fired(&[at(PointerAction::Press, 0, 5, 5, 3)]), [2]);
    assert_eq!(fired(&[at(PointerAction::Release, 0, 700, 400, 0)]), [3]);
    assert_eq!(fired(&[at(PointerAction::Release, 0, 5, 5, 3)]), [0_u8; 0]);
    // `enter` and `leave` of the object at 200 to 209: its edge pixels.
    assert_eq!(fired(&[at(PointerAction::Move, 0, 199, 205, 0)]), [0_u8; 0]);
    assert_eq!(fired(&[at(PointerAction::Move, 0, 200, 205, 0)]), [4]);
    assert_eq!(fired(&[at(PointerAction::Move, 0, 209, 209, 0)]), [0_u8; 0]);
    assert_eq!(fired(&[at(PointerAction::Move, 0, 210, 209, 0)]), [5]);
    // `leave` also when the pointer leaves the page.
    assert_eq!(fired(&[at(PointerAction::Move, 0, 205, 205, 0), at(PointerAction::Gone, 0, 0, 0, 0)]), [4, 5]);
    // `move` with `held`: only while a button is down, and only to
    // another pixel.
    assert_eq!(fired(&[at(PointerAction::Move, 0, 1, 1, 0)]), [0_u8; 0]);
    assert_eq!(fired(&[at(PointerAction::Move, 1, 2, 1, 0), at(PointerAction::Move, 1, 2, 1, 0)]), [7]);
    // `wheel` inside the rectangle, 100 to 109; not outside it.
    let wheel = |x, y| Input::Wheel { x, y, control: 0 };
    assert_eq!(fired(&[wheel(109, 100), wheel(110, 100), wheel(99, 100)]), [8]);
    // `text`, `focus` and `blur`.
    assert_eq!(fired(&[Input::Text, Input::Focus, Input::Blur]), [6, 9, 10]);
}

/// RULE-10 (REQ-6526): on a French layout the physical key `q` matches
/// the key that types `a`; `any` matches a modifier alone.
#[test]
fn keys_are_named_three_ways() {
    let set = |kind, code: char| {
        let mut keys = any_key();
        keys.push(crate::table::Key { kind, code: u32::from(code) }).unwrap();
        Trigger::Key { keys, repeat: false, mods_mask: 0, mods_value: 0 }
    };
    let mut named = any_key();
    named.push(crate::table::Key { kind: KeyKind::Named, code: 13 }).unwrap();
    let table = table_of(&[
        rule(set(KeyKind::Physical, 'q'), &[SEND]),
        rule(set(KeyKind::Character, 'q'), &[SEND]),
        rule(Trigger::Key { keys: named, repeat: false, mods_mask: 0, mods_value: 0 }, &[SEND]),
        rule(any_key_down(), &[SEND]),
    ]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    let mut fired = |key, base: Option<char>| {
        let input = Input::Key { action: KeyAction::Press, key, base: base.map(u32::from), mods: 0 };
        evaluator.push(InputEvent { sequence: 0, input });
        evaluator.tick(&mut world);
        world.take_rules()
    };
    // The French key that types 'a' is where 'q' is on a United States
    // layout: the physical key q, and not the character q.
    assert_eq!(fired(character('a'), Some('q')), [0, 3]);
    // The French key that types 'q' is the physical key a.
    assert_eq!(fired(character('q'), Some('a')), [1, 3]);
    // With no base, a character key is its own physical key.
    assert_eq!(fired(character('q'), None), [0, 1, 3]);
    // A named key, and a modifier alone.
    assert_eq!(fired(KeyIdentity::Named(13), None), [2, 3]);
    assert_eq!(fired(KeyIdentity::Named(80), None), [3]);
    // A character with the code of a named key is not that named key.
    assert_eq!(fired(KeyIdentity::Character(13), None), [3]);
}

/// RULE-11 (REQ-6526): a level crossing a threshold fires once and not
/// again while above; falling and rising fires again; a condition that
/// holds at opening fires at tick 0; a rule enabled while its condition
/// holds fires at the next tick.
#[test]
fn becomes_is_an_edge() {
    let level = Value::sound(0, SOUND_LEVEL);
    let mut enabled_later = when(rule(Trigger::Becomes, &[SEND]), &[above(level, 100)]);
    enabled_later.enabled = false;
    let table = table_of(&[
        when(rule(Trigger::Becomes, &[SEND]), &[above(level, 100)]),
        when(rule(Trigger::Becomes, &[SEND]), &[Node::Not, above(level, 100)]),
        enabled_later,
        rule(on_key('e'), &[Action::Enable(1 << 2)]),
    ]);
    let mut evaluator = evaluator(&table);
    assert_eq!(evaluator.index().watch.len(), 1);
    let mut world = Script::default();
    let mut fired = |level: u8, key: Option<char>| {
        evaluator.set_metered(&[level]);
        if let Some(code) = key {
            evaluator.push(press_key(0, code));
        }
        evaluator.tick(&mut world);
        world.take_rules()
    };
    assert_eq!(fired(0, None), [1], "true at opening fires at tick 0");
    assert_eq!(fired(100, None), [0_u8; 0]);
    assert_eq!(fired(101, None), [0]);
    assert_eq!(fired(255, None), [0_u8; 0]);
    assert_eq!(fired(100, None), [1]);
    assert_eq!(fired(200, Some('e')), [0]);
    assert_eq!(fired(200, None), [2], "enabled while its condition holds");
    assert_eq!(fired(200, None), [0_u8; 0]);
}

/// RULE-12, RULE-13 (REQ-6526): each kind's inside rule on an edge pixel;
/// "any" fires once for each object, which the report names; a rectangle
/// reaches the picture's last pixel.
#[test]
fn targets_are_inside_by_their_own_rules() {
    let mut table = Box::new(Table::new());
    // The last pixel of an 800 by 480 picture, and one that hangs off it.
    table.rects.push(Rect { x: 799, y: 479, width: 1, height: 1 }).unwrap();
    table.rects.push(Rect { x: 790, y: 470, width: 100, height: 100 }).unwrap();
    for target in [Target::ANY_OBJECT, Target::ANY_CONTROL, Target::rect(0), Target::rect(1), Target::PAGE] {
        table.rules.push(rule(Trigger::Press { target, button: ANY_BUTTON }, &[SEND])).unwrap();
    }
    let hidden = TargetState { shown: false, ..shown_at(0, 0, 800, 480) };
    let mut world = Script::default()
        .with(Target::object(1), shown_at(10, 10, 20, 20))
        .with(Target::object(2), shown_at(19, 19, 30, 30))
        .with(Target::object(3), hidden);
    let mut evaluator = evaluator(&table);
    let mut targets = |x, y, control| {
        evaluator.push(pointer(9, PointerAction::Press, 0, x, y, control));
        evaluator.tick(&mut world);
        let named: Vec<(u8, Target)> = world
            .log
            .iter()
            .filter_map(|out| if let Out::Report(report) = out { Some((report.rule, report.target)) } else { None })
            .collect();
        world.log.clear();
        named
    };
    // Pixel (19, 19) is the last of object 1 and the first of object 2:
    // "any object" fires once for each, naming it; the page's report
    // names nothing.
    assert_eq!(targets(19, 19, 0), [(0, Target::object(1)), (0, Target::object(2)), (4, Target::NONE)]);
    assert_eq!(targets(20, 19, 7), [(0, Target::object(2)), (1, Target::control(7)), (4, Target::NONE)]);
    assert_eq!(targets(799, 479, 0), [(2, Target::rect(0)), (3, Target::rect(1)), (4, Target::NONE)]);
    assert_eq!(targets(798, 479, 0), [(3, Target::rect(1)), (4, Target::NONE)]);
    assert_eq!(targets(789, 470, 0), [(4, Target::NONE)]);
}

/// RULE-12, RULE-20 (REQ-6526): a target that does not exist is never
/// inside, reads as not shown, and an action on it does nothing and is
/// counted.
#[test]
fn a_missing_target_is_a_counted_no_op() {
    let ghost = Target::object(77);
    let exists = Value::target(TargetKind::Object, 77, TARGET_EXISTS);
    let shown = Value::target(TargetKind::Object, 77, TARGET_SHOWN);
    let table = table_of(&[
        rule(
            Trigger::Open,
            &[
                Action::Animate(Operation { target: ghost, kind: OperationKind::Show }),
                Action::Send(Some(exists)),
                Action::Send(Some(shown)),
                Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 },
                Action::Stop { channel: 1, fade_ms: 5 },
                Action::Gain { channel: 2, gain: 50, fade_ms: 5 },
                Action::Play { channel: 3, handle: 9, generation: 1, gain: 100, fade_ms: 5, looped: false },
            ],
        ),
        rule(Trigger::Enter { target: ghost }, &[SEND]),
    ]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.push(pointer(1, PointerAction::Move, 0, 5, 5, 0));
    evaluator.tick(&mut world);
    assert_eq!(
        world.reports(),
        [(0, Reported::Value { value: exists, reading: 0 }), (0, Reported::Value { value: shown, reading: 0 })]
    );
    // The show, the midi with no local sound, the stop and the gain of a
    // silent channel, and the play of a handle with no sound: five.
    assert_eq!(evaluator.counters().skipped, 5);
    let sounds = world.log.iter().filter(|out| matches!(out, Out::Sound(_))).count();
    assert_eq!(sounds, 1, "only the play was tried");
}

/// RULE-14 (REQ-6526): saturation at both ends, including `add` of the
/// largest and smallest 32-bit constants; reset at each opening.
#[test]
fn variables_are_held_to_their_ranges() {
    let mut table = Table::new();
    let narrow = variable(&mut table, -3, 3, 1);
    let wide = variable(&mut table, i32::MIN, i32::MAX, 0);
    let add = |variable, amount| Action::Add { variable, amount };
    let set = |variable, value| Action::Set { variable, value };
    table.rules.push(rule(on_key('u'), &[add(narrow, i32::MAX), add(wide, i32::MAX), add(wide, i32::MAX)])).unwrap();
    table.rules.push(rule(on_key('d'), &[add(narrow, i32::MIN), add(wide, i32::MIN), add(wide, i32::MIN)])).unwrap();
    table.rules.push(rule(on_key('s'), &[set(narrow, 100), set(wide, -100)])).unwrap();
    table.rules.push(rule(on_key('t'), &[set(narrow, -100)])).unwrap();
    let variables = |evaluator: &mut Evaluator, key: Option<char>| {
        let mut world = Script::default();
        if let Some(key) = key {
            evaluator.push(press_key(0, key));
            evaluator.tick(&mut world);
        }
        evaluator.query(1);
        evaluator.tick(&mut world);
        let Some(Out::State(state)) = world.log.last() else { panic!("no answer") };
        state.table.unwrap().variables.to_vec()
    };
    let mut evaluator = evaluator(&table);
    assert_eq!(variables(&mut evaluator, None), [1, 0]);
    assert_eq!(variables(&mut evaluator, Some('u')), [3, i32::MAX]);
    // MAX + MIN is −1, and −1 + MIN saturates.
    assert_eq!(variables(&mut evaluator, Some('d')), [-3, i32::MIN]);
    assert_eq!(variables(&mut evaluator, Some('s')), [3, -100]);
    assert_eq!(variables(&mut evaluator, Some('t')), [-3, -100]);
    // Opening the page again starts from the declarations.
    assert_eq!(variables(&mut self::evaluator(&table), None), [1, 0]);
}

/// RULE-15, RULE-25 (REQ-6526): `every 30` on the page timer fires at
/// ticks 30, 60 and 90; a timer started in tick t runs since t + 1 and
/// its `after 1` is due one tick after that; restarting replaces; a
/// stopped timer fires nothing; its value is 1 while it runs.
#[test]
fn timers_run_from_their_start_tick() {
    let mut table = Table::new();
    table.timers.push(false).unwrap();
    table.timers.push(true).unwrap();
    let elapsed = Value::timer(0, TIMER_ELAPSED);
    let running = Value::timer(0, TIMER_RUNNING);
    table.rules.push(rule(Trigger::Every { timer: PAGE_TIMER, ticks: 30 }, &[SEND])).unwrap();
    table.rules.push(rule(Trigger::After { timer: 0, ticks: 1 }, &[Action::Send(Some(elapsed))])).unwrap();
    table.rules.push(rule(on_key('s'), &[Action::StartTimer(0)])).unwrap();
    table.rules.push(rule(on_key('x'), &[Action::StopTimer(0)])).unwrap();
    table.rules.push(rule(Trigger::After { timer: 1, ticks: 3 }, &[Action::Send(Some(running))])).unwrap();
    table.rules.push(rule(Trigger::After { timer: 0, ticks: 5 }, &[SEND])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    let mut fired_at = Vec::new();
    for tick in 0..100_u32 {
        match tick {
            10 | 40 | 41 | 50 => evaluator.push(press_key(tick, 's')),
            52 => evaluator.push(press_key(tick, 'x')),
            _ => true,
        };
        evaluator.tick(&mut world);
        for (rule, what) in world.reports() {
            fired_at.push((tick, rule, what));
        }
        world.log.clear();
    }
    let value = |value, reading| Reported::Value { value, reading };
    assert_eq!(
        fired_at,
        [
            // A timer that starts at opening runs since tick 0; timer 0 is
            // stopped then.
            (3, 4, value(running, 0)),
            // Started in tick 10: running since 11, `after 1` at 12, `after
            // 5` at 16.
            (12, 1, value(elapsed, 1)),
            (16, 5, Reported::Fired),
            (30, 0, Reported::Fired),
            // Started in 40 and again in 41: the restart replaces, so `after
            // 1` is due once, at 43.
            (43, 1, value(elapsed, 1)),
            (47, 5, Reported::Fired),
            // Started in 50, `after 1` at 52; stopped in 52, so no `after 5`.
            (52, 1, value(elapsed, 1)),
            (60, 0, Reported::Fired),
            (90, 0, Reported::Fired),
        ]
    );
}

/// RULE-16, RULE-17 (REQ-6526): a rule fires on sub-page 1 and not on
/// sub-page 2, whose list switches it off, with its `becomes` bit cleared
/// while off; a change of sub-page keeps the variables and fires no
/// `open`.
#[test]
fn a_sub_page_switches_rules_off_while_it_is_shown() {
    let mut table = Table::new();
    let v = variable(&mut table, 0, 9, 0);
    table.rules.push(rule(on_key('a'), &[SEND, Action::Add { variable: v, amount: 1 }])).unwrap();
    table.rules.push(when(rule(Trigger::Becomes, &[SEND]), &[above(Value::variable(v), 0)])).unwrap();
    table.rules.push(rule(Trigger::Open, &[SEND])).unwrap();
    table.rules.push(rule(on_key('v'), &[Action::Send(Some(Value::variable(v)))])).unwrap();
    table.subpages.push(SubpageMask { sub: 0x0002, off: 0b11 }).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    let mut fired = |evaluator: &mut Evaluator, key| {
        evaluator.push(press_key(0, key));
        evaluator.tick(&mut world);
        world.take_rules()
    };
    assert_eq!(fired(&mut evaluator, 'a'), [0, 2]);
    assert_eq!(fired(&mut evaluator, 'a'), [0, 1]);
    evaluator.set_subpage(0x0002);
    assert_eq!(fired(&mut evaluator, 'a'), [0_u8; 0], "off while sub-page 2 is shown, and no `open`");
    evaluator.set_subpage(0x0001);
    // The variable was kept; the `becomes` bit was cleared while the rule
    // was off, so its condition, still true, fires again.
    assert_eq!(fired(&mut evaluator, 'v'), [3, 1]);
    assert_eq!(evaluator.counters().sends, 6);
}

/// RULE-19 (REQ-6526): rectangles that touch at an edge do not overlap;
/// that share one pixel, do; a hidden object overlaps nothing; `inside`
/// is wholly within.
#[test]
fn collisions_are_of_bounding_rectangles() {
    let mut table = Box::new(Table::new());
    table.rects.push(Rect { x: 0, y: 0, width: 100, height: 100 }).unwrap();
    let (a, b) = (Target::object(1), Target::object(2));
    let conditions = [
        Node::Overlaps(a, b),
        Node::Inside(a, Target::rect(0)),
        Node::Overlaps(a, Target::PAGE),
        Node::Overlaps(Target::rect(0), a),
    ];
    for node in conditions {
        table.rules.push(when(rule(any_key_down(), &[SEND]), &[node])).unwrap();
    }
    let mut evaluator = evaluator(&table);
    let mut holds = |a_state: TargetState, b_state: Option<TargetState>| {
        let mut world = Script::default().with(a, a_state);
        if let Some(b_state) = b_state {
            world = world.with(b, b_state);
        }
        evaluator.push(press_key(0, 'k'));
        evaluator.tick(&mut world);
        world.rules()
    };
    // b begins where a ends: they touch at an edge.
    assert_eq!(holds(shown_at(0, 0, 10, 10), Some(shown_at(10, 0, 20, 10))), [1, 2, 3]);
    // One pixel in common, (9, 9).
    assert_eq!(holds(shown_at(0, 0, 10, 10), Some(shown_at(9, 9, 20, 20))), [0, 1, 2, 3]);
    // A hidden object, and one that does not exist, overlap nothing.
    let hidden = TargetState { shown: false, ..shown_at(0, 0, 10, 10) };
    assert_eq!(holds(shown_at(0, 0, 10, 10), Some(hidden)), [1, 2, 3]);
    assert_eq!(holds(shown_at(0, 0, 10, 10), None), [1, 2, 3]);
    assert_eq!(holds(hidden, Some(shown_at(0, 0, 10, 10))), [0_u8; 0]);
    // Wholly within the rectangle up to its last pixel, and not one
    // beyond.
    assert_eq!(holds(shown_at(90, 90, 100, 100), None), [1, 2, 3]);
    assert_eq!(holds(shown_at(91, 90, 101, 100), None), [2, 3]);
    // Off the picture altogether: it still has a rectangle, which
    // overlaps neither the page nor the rectangle.
    assert_eq!(holds(shown_at(-50, -50, 0, 0), None), [0_u8; 0]);
}

/// RULE-20 to RULE-22 (REQ-6526): each action's output, in the order the
/// actions ran, with the rule's two sends and its `echo` in order.
#[test]
fn actions_give_their_outputs_in_order() {
    let object = Target::object(5);
    let moved = Operation { target: object, kind: OperationKind::Move { relative: true, x: 8, y: 0 } };
    let look = Operation { target: Target::control(2), kind: OperationKind::Look(2) };
    let level = Value::sound(1, SOUND_LEVEL);
    let mut both = rule(
        on_key('a'),
        &[
            Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 },
            SEND,
            Action::Animate(moved),
            Action::Play { channel: 1, handle: 5, generation: 2, gain: 80, fade_ms: 5, looped: true },
            Action::Send(Some(level)),
            Action::Animate(look),
            Action::Stop { channel: 0, fade_ms: 20 },
            Action::Gain { channel: 0, gain: 10, fade_ms: 0 },
        ],
    );
    both.echo = true;
    let table = table_of(&[both]);
    let mut evaluator = evaluator(&table);
    let mut world =
        Script::default().with(object, shown_at(0, 0, 10, 10)).with(Target::control(2), shown_at(0, 0, 1, 1));
    world.sounds[0] = SoundState { playing: true, local: true, handle: 1, gain: 100, family: 14, variant: 0 };
    world.handles.push(5);
    evaluator.tick(&mut world);
    world.log.clear();
    evaluator.push(press_key(77, 'a'));
    evaluator.tick(&mut world);
    let report = |what| Out::Report(Report { rule: 0, tick: 1, target: Target::NONE, what });
    assert_eq!(
        world.log,
        std::vec![
            Out::Cycles(1),
            Out::Sound(SoundOperation::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 }),
            report(Reported::Input { sequence: 77 }),
            Out::Animate(moved, 1),
            Out::Sound(SoundOperation::Play {
                channel: 1,
                handle: 5,
                generation: 2,
                gain: 80,
                fade_ms: 5,
                looped: true
            }),
            report(Reported::Value { value: level, reading: 0 }),
            Out::Animate(look, 1),
            Out::Sound(SoundOperation::Stop { channel: 0, fade_ms: 20 }),
            Out::Sound(SoundOperation::Gain { channel: 0, gain: 10, fade_ms: 0 }),
            report(Reported::Input { sequence: 77 }),
        ]
    );
    assert_eq!(evaluator.counters().skipped, 0);
    assert_eq!(world.state(object).x, 8);
}

/// RULE-24, RULE-26 to RULE-28 (REQ-6526): what each space reads: the
/// pointer, a target one tick after an animation action, a sound, and a
/// synthesizer, whose every class reads 0 for another family's sound.
#[test]
fn values_read_what_their_spaces_say() {
    let object = Target::object(5);
    let of = |property| Value::target(TargetKind::Object, 5, property);
    let notes = Value::synth(2, 14, 1, 3, CLASS_NOTES, 0);
    let stage = Value::synth(2, 14, 1, 3, CLASS_ENVELOPE_STAGE, 0);
    let other_family = Value::synth(2, 15, 0, 3, CLASS_NOTES, 0);
    let other_stage = Value::synth(2, 15, 0, 3, CLASS_ENVELOPE_STAGE, 0);
    let values = [
        Value::pointer(POINTER_OVER),
        Value::pointer(POINTER_X),
        Value::button(0),
        of(TARGET_SHOWN),
        of(TARGET_X),
        of(TARGET_FRAME),
        of(TARGET_CYCLING),
        of(TARGET_LAYER),
        of(TARGET_ORDER),
        of(TARGET_LOOK),
        of(TARGET_INSIDE),
        Value::sound(2, SOUND_PLAYING),
        Value::sound(2, SOUND_HANDLE),
        Value::sound(2, SOUND_GAIN),
        Value::sound(2, SOUND_LOCAL),
        Value::sound(3, SOUND_HANDLE),
    ];
    let synths = [notes, stage, other_family, other_stage, Value::sound(2, SOUND_LEVEL)];
    let sends = |values: &[Value]| values.iter().map(|value| Action::Send(Some(*value))).collect::<Vec<_>>();
    let on = |kind| Action::Animate(Operation { target: object, kind });
    let table = table_of(&[
        rule(on_key('r'), &sends(&values)),
        rule(on_key('s'), &sends(&synths)),
        rule(
            on_key('m'),
            &[
                on(OperationKind::Hide),
                on(OperationKind::Move { relative: false, x: 300, y: 4 }),
                on(OperationKind::Cycle { first: 2, last: 3, period: 9, once: false }),
                on(OperationKind::Order { layer: 3, order: -4 }),
                on(OperationKind::Look(1)),
            ],
        ),
    ]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default().with(object, shown_at(10, 10, 20, 20));
    world.sounds[2] = SoundState { playing: true, local: true, handle: 9, gain: 70, family: 14, variant: 1 };
    world.synth.insert((2, 3, CLASS_NOTES, 0), 4);
    // The watch list: the stage of part 3 on channel 2, then its level.
    assert_eq!(evaluator.index().watch.len(), 2);
    evaluator.set_metered(&[3, 200]);
    let mut readings = |world: &mut Script, events: &[InputEvent]| {
        for event in events {
            evaluator.push(*event);
        }
        evaluator.tick(world);
        let read: Vec<i32> = world
            .reports()
            .iter()
            .map(|(_, what)| if let Reported::Value { reading, .. } = what { *reading } else { panic!("{what:?}") })
            .collect();
        world.log.clear();
        read
    };
    assert_eq!(readings(&mut world, &[press_key(0, 'r')]), [0, 0, 0, 1, 10, 0, 0, 1, 0, 0, 0, 1, 9, 70, 1, -1]);
    let over = [pointer(1, PointerAction::Press, 0, 15, 12, 0), press_key(2, 'm'), press_key(3, 'r')];
    // The pointer is over the object with a button down; the animation
    // actions of this tick are not yet seen.
    assert_eq!(readings(&mut world, &over), [1, 15, 1, 1, 10, 0, 0, 1, 0, 0, 1, 1, 9, 70, 1, -1]);
    // One tick later they are: hidden, at 300, frame 2, cycling, layer 3,
    // order −4, the hover look. The hidden object is no longer inside
    // the pointer once the pointer says where it is again.
    let again = [pointer(4, PointerAction::Move, 0, 15, 12, 0), press_key(5, 'r')];
    assert_eq!(readings(&mut world, &again), [1, 15, 1, 0, 300, 2, 1, 3, -4, 1, 0, 1, 9, 70, 1, -1]);
    // The synthesizer: the tracked value from the world, the metered ones
    // from the report; all 0 for the family the sound is not.
    assert_eq!(readings(&mut world, &[press_key(6, 's')]), [4, 3, 0, 0, 200]);
    world.sounds[2].playing = false;
    assert_eq!(readings(&mut world, &[press_key(7, 's')]), [0, 0, 0, 0, 200]);
}

/// RULE-47 (REQ-6526): a query is answered once, at the end of the next
/// tick, with the snapshot that tick read.
#[test]
fn a_state_query_is_answered_once() {
    let mut table = Table::new();
    table.timers.push(true).unwrap();
    table.timers.push(false).unwrap();
    let v = variable(&mut table, 0, 9, 4);
    let object = Target::object(301);
    let level = Value::sound(0, SOUND_LEVEL);
    let mut off = rule(Trigger::Text, &[SEND]);
    off.enabled = false;
    table
        .rules
        .push(when(
            rule(on_key('a'), &[Action::Add { variable: v, amount: 1 }]),
            &[Node::Or(2), above(level, 250), Node::Overlaps(object, Target::PAGE)],
        ))
        .unwrap();
    table.rules.push(off).unwrap();
    table
        .rules
        .push(rule(Trigger::Enter { target: Target::control(4) }, &[Action::Send(Some(Value::variable(v)))]))
        .unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default().with(object, TargetState { frame: 2, order: -1, ..shown_at(700, 380, 710, 390) });
    evaluator.set_metered(&[41]);
    for _ in 0..18 {
        evaluator.tick(&mut world);
    }
    world.log.clear();
    evaluator.push(press_key(1, 'a'));
    evaluator.query(42);
    evaluator.tick(&mut world);
    let Some(Out::State(state)) = world.log.last().cloned() else { panic!("no answer") };
    let answer = state.table.unwrap();
    assert_eq!((state.identifier, answer.generation, answer.tick, answer.enabled), (42, 1, 18, 0b101));
    assert_eq!(answer.variables.as_slice(), [4], "the snapshot, not what the tick's actions wrote");
    assert_eq!(
        answer.timers.as_slice(),
        [TimerState { running: true, elapsed: 18 }, TimerState { running: false, elapsed: 0 }]
    );
    let found = TargetReport {
        target: object,
        exists: true,
        shown: true,
        cycling: false,
        look: 0,
        frame: 2,
        x: 700,
        y: 380,
        layer: 1,
        order: -1,
    };
    let missing = TargetReport {
        target: Target::control(4),
        exists: false,
        shown: false,
        cycling: false,
        look: 0,
        frame: 0,
        x: 0,
        y: 0,
        layer: 0,
        order: 0,
    };
    assert_eq!(answer.targets.as_slice(), [found, missing]);
    assert_eq!(answer.values.as_slice(), [(level, 41), (Value::variable(v), 4)]);
    // The answer is within the payload's bound and reads back.
    let mut out = [0_u8; crate::state::MAX_BODY_BYTES];
    let length = crate::state::encode(&state, &mut out).unwrap();
    assert_eq!(crate::state::decode(&out[..length]), Ok(*state));
    // Once: the next tick answers nothing.
    world.log.clear();
    evaluator.tick(&mut world);
    assert!(!world.log.iter().any(|out| matches!(out, Out::State(_))));
}

/// RULE-51 item 4 (REQ-6526): the rectangles inside which an enabled
/// `wheel` rule applies.
#[test]
fn the_wheel_rules_give_their_rectangles() {
    let mut table = Box::new(Table::new());
    table.rects.push(Rect { x: 700, y: 400, width: 200, height: 200 }).unwrap();
    let object = Target::object(8);
    let mut off = rule(Trigger::Wheel { target: Target::PAGE }, &[SEND]);
    off.enabled = false;
    table.rules.push(rule(Trigger::Wheel { target: Target::rect(0) }, &[SEND])).unwrap();
    table.rules.push(rule(Trigger::Wheel { target: object }, &[SEND])).unwrap();
    table.rules.push(off).unwrap();
    table.rules.push(rule(on_key('w'), &[Action::Enable(1 << 2)])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = Script::default().with(object, shown_at(1, 2, 3, 4));
    let mut out = [Bounds { left: 0, top: 0, right: 0, bottom: 0 }; MAX_RECTS];
    evaluator.tick(&mut world);
    assert_eq!(evaluator.wheel_bounds(&mut out), 2);
    // The rectangle is cut to the 800 by 480 picture.
    assert_eq!(out[0], Bounds { left: 700, top: 400, right: 800, bottom: 480 });
    assert_eq!(out[1], Bounds { left: 1, top: 2, right: 3, bottom: 4 });
    evaluator.push(press_key(0, 'w'));
    evaluator.tick(&mut world);
    assert_eq!(evaluator.wheel_bounds(&mut out), 3);
    assert_eq!(out[2], Bounds { left: 0, top: 0, right: 800, bottom: 480 });
}

/// RULE-56 (REQ-6526): a table built by hand that the decoder would
/// refuse is refused here too.
#[test]
fn a_table_the_decoder_refuses_is_not_evaluated() {
    let table = table_of(&[when(rule(Trigger::Becomes, &[SEND]), &[Node::Not])]);
    assert_eq!(Evaluator::new(&table, 1, 0).err(), Some(Fault::Tree));
}
