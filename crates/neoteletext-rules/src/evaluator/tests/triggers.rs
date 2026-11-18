//! The triggers (REQ-6526, REQ-6613; RULE-9 to RULE-15, RULE-105,
//! RULE-107, RULE-109, RULE-116; DEC-6635).

use super::*;
use crate::limits::{MAX_KEYS_DOWN, MAX_POINTER_OBJECTS};
use crate::table::{INPUT_PHYSICAL, Rect, RowRun, SOUND_LEVEL};

/// A table with rectangle 0 at (10, 10), 20 by 20, and one group that
/// reports the firing; `triggers` are one rule's.
fn reporting(triggers: &[Trigger]) -> Box<Table> {
    let mut table = empty();
    table.rects.push(Rect { x: 10, y: 10, width: 20, height: 20 }).unwrap();
    let report = group(&mut table, &[built(Command::Report { value: None })]);
    let mut all = triggers.iter();
    let first = rule(&mut table, *all.next().unwrap(), report);
    for trigger in all {
        table.rules.as_mut_slice()[first].triggers.push(*trigger).unwrap();
    }
    table
}

/// How many times the one rule of `triggers` fires for `inputs`, all in
/// one tick after page tick 0.
fn fires(triggers: &[Trigger], inputs: &[Input]) -> usize {
    let mut evaluator = evaluator(&reporting(triggers));
    let mut world = Script::default();
    evaluator.tick(&mut world);
    for (input, sequence) in inputs.iter().zip(1..) {
        evaluator.handle(Event::Input(InputEvent { sequence, input: *input }), &mut world);
    }
    evaluator.tick(&mut world);
    world.reports().len()
}

const RECT: TriggerTarget = TriggerTarget::of(Target::rect(0));
const INSIDE: (u16, u16) = (15, 15);
const OUTSIDE: (u16, u16) = (5, 5);

fn at(action: PointerAction, button: u8, (x, y): (u16, u16)) -> Input {
    Input::Pointer { action, button, x, y, control: 0 }
}

fn typed(action: KeyAction, character: char) -> Input {
    Input::Key { action, key: KeyIdentity::Character(character as u32), base: None, mods: 0 }
}

/// RULE-9 (REQ-6526): each kind of input trigger: the event that fires
/// it, and the nearest events that must not.
#[test]
fn each_input_trigger_fires_on_its_event_and_no_other() {
    use KeyAction::{Press as Down, Release as Up, Repeat};
    use PointerAction::{Gone, Move, Press, Release};
    let key = |repeat| Trigger::Key { keys: keys_of("a"), repeat };
    // `key`: a press; a repeat only with `repeat`; never a release or
    // another key.
    assert_eq!(fires(&[key(false)], &[typed(Down, 'a')]), 1);
    assert_eq!(fires(&[key(false)], &[typed(Repeat, 'a'), typed(Up, 'a'), typed(Down, 'b')]), 0);
    assert_eq!(fires(&[key(true)], &[typed(Down, 'a'), typed(Repeat, 'a'), typed(Repeat, 'a')]), 3);
    // `key_up`: a release, of that key.
    let key_up = Trigger::KeyUp { keys: keys_of("a") };
    assert_eq!(fires(&[key_up], &[typed(Down, 'a'), typed(Up, 'a')]), 1);
    assert_eq!(fires(&[key_up], &[typed(Down, 'a'), typed(Up, 'b')]), 0);
    // The modifier mask and value: Shift (1) down, Control (4) not.
    let shifted = Trigger::Key { keys: KeyMatch { mods_mask: 5, mods_value: 1, ..keys_of("a") }, repeat: false };
    let with = |mods| Input::Key { action: Down, key: KeyIdentity::Character('a' as u32), base: None, mods };
    assert_eq!(fires(&[shifted], &[with(1), with(3)]), 2);
    assert_eq!(fires(&[shifted], &[with(0), with(5), with(4)]), 0);
    // `press`: a button going down inside the target; the button named.
    let press = |button| Trigger::Press { target: RECT, button };
    assert_eq!(fires(&[press(0)], &[at(Press, 0, INSIDE)]), 1);
    assert_eq!(fires(&[press(0)], &[at(Press, 0, OUTSIDE), at(Press, 2, INSIDE), at(Move, 1, INSIDE)]), 0);
    assert_eq!(fires(&[press(255)], &[at(Press, 2, INSIDE)]), 1);
    // The rectangle's last pixel is inside, the next is not (RULE-13).
    assert_eq!(fires(&[press(0)], &[at(Press, 0, (29, 29))]), 1);
    assert_eq!(fires(&[press(0)], &[at(Press, 0, (30, 29))]), 0);
    // `release`: where the press was, wherever the pointer then is.
    let release = Trigger::Release { target: RECT, button: 0 };
    assert_eq!(fires(&[release], &[at(Press, 0, INSIDE), at(Release, 0, OUTSIDE)]), 1);
    assert_eq!(fires(&[release], &[at(Press, 0, OUTSIDE), at(Release, 0, INSIDE)]), 0);
    // `enter` and `leave`: when the pointer comes inside, goes outside or
    // leaves the page; not while it stays.
    let (enter, leave) = (Trigger::Enter { target: RECT }, Trigger::Leave { target: RECT });
    assert_eq!(fires(&[enter], &[at(Move, 0, OUTSIDE), at(Move, 0, INSIDE), at(Move, 0, (16, 16))]), 1);
    assert_eq!(fires(&[enter], &[at(Press, 0, INSIDE)]), 1, "any event that carries a position");
    assert_eq!(fires(&[leave], &[at(Move, 0, INSIDE), at(Move, 0, OUTSIDE)]), 1);
    assert_eq!(fires(&[leave], &[at(Move, 0, INSIDE), at(Gone, 0, (0, 0))]), 1);
    assert_eq!(fires(&[leave], &[at(Move, 0, OUTSIDE), at(Move, 0, (6, 6))]), 0);
    // `move`: to another pixel inside, never a press; `held` only while
    // a button is down.
    let moved = |held| Trigger::Move { target: RECT, held };
    assert_eq!(fires(&[moved(false)], &[at(Move, 0, INSIDE), at(Move, 0, INSIDE), at(Move, 0, (16, 15))]), 2);
    assert_eq!(fires(&[moved(false)], &[at(Press, 0, INSIDE), at(Move, 0, OUTSIDE)]), 0);
    assert_eq!(fires(&[moved(true)], &[at(Move, 0, INSIDE), at(Move, 1, (16, 15))]), 1);
    assert_eq!(fires(&[moved(true)], &[at(Press, 0, INSIDE), at(Move, 0, (16, 15))]), 1, "a tracked button");
    // `wheel`, `text`, `focus` and `blur`.
    let wheel = |(x, y): (u16, u16)| Input::Wheel { x, y, control: 0 };
    assert_eq!(fires(&[Trigger::Wheel { target: RECT }], &[wheel(INSIDE), wheel(OUTSIDE)]), 1);
    assert_eq!(fires(&[Trigger::Text], &[Input::Text, typed(Down, 'a')]), 1);
    assert_eq!(fires(&[Trigger::Focus], &[Input::Focus, Input::Blur]), 1);
    assert_eq!(fires(&[Trigger::Blur], &[Input::Focus, Input::Blur, Input::Text]), 1);
}

/// RULE-107 (REQ-6613): several triggers on one rule: any of them fires
/// it, an event that matches two fires it once, and triggers of
/// different steps each fire it at their own step.
#[test]
fn any_of_a_rules_triggers_fires_it_once_an_event() {
    let key = Trigger::Key { keys: keys_of("a"), repeat: false };
    let any = Trigger::Key { keys: KeyMatch::any(), repeat: false };
    let press = Trigger::Press { target: RECT, button: 0 };
    let down = typed(KeyAction::Press, 'a');
    assert_eq!(fires(&[key, press], &[down]), 1);
    assert_eq!(fires(&[key, press], &[at(PointerAction::Press, 0, INSIDE)]), 1);
    assert_eq!(fires(&[key, press], &[down, at(PointerAction::Press, 0, INSIDE)]), 2);
    assert_eq!(fires(&[key, any, any, key], &[down]), 1, "one event fires a rule once");
    let every = Trigger::Every { timer: PAGE_TIMER, ticks: 1 };
    assert_eq!(fires(&[key, every], &[down]), 2, "once at step 3 and once at step 7");
}

/// RULE-10 (REQ-6526): a physical key is the key that carries the
/// character on a United States layout, whatever it types; `any` matches
/// every key, a modifier alone among them; and a key-state value names
/// keys the same way (RULE-24).
#[test]
fn keys_are_named_three_ways() {
    let physical = Trigger::Key { keys: KeyMatch::of(&[Key::of(KeyKind::Physical, 'q' as u32)]), repeat: false };
    // On a French layout the key that types `a` is the one that carries
    // `q` on a United States layout.
    let french = |typed: char, base: char| Input::Key {
        action: KeyAction::Press,
        key: KeyIdentity::Character(typed as u32),
        base: Some(base as u32),
        mods: 0,
    };
    assert_eq!(fires(&[physical], &[french('a', 'q')]), 1);
    assert_eq!(fires(&[physical], &[french('q', 'a')]), 0);
    assert_eq!(fires(&[physical], &[typed(KeyAction::Press, 'q')]), 1, "no base: the key's own character");
    let named = |code| Input::Key { action: KeyAction::Press, key: KeyIdentity::Named(code), base: None, mods: 0 };
    let enter = Trigger::Key { keys: KeyMatch::of(&[Key::of(KeyKind::Named, 57_345)]), repeat: false };
    assert_eq!(fires(&[enter], &[named(57_345), named(57_346), typed(KeyAction::Press, 'a')]), 1);
    let any = Trigger::Key { keys: KeyMatch::any(), repeat: false };
    assert_eq!(fires(&[any], &[named(57_441), typed(KeyAction::Press, 'a')]), 2, "a modifier alone is a key");

    // A condition on the physical key's state.
    let mut table = reporting(&[Trigger::Text]);
    let down = Value::key(INPUT_PHYSICAL, 'q' as u32);
    table.rules.as_mut_slice()[0].condition = condition(&[Node::compare(down, Operator::Equal, 1)]);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    for (input, sequence) in [Input::Text, french('a', 'q'), Input::Text].into_iter().zip(1..) {
        evaluator.handle(Event::Input(InputEvent { sequence, input }), &mut world);
    }
    evaluator.tick(&mut world);
    assert_eq!(world.reports().len(), 1, "text after the key went down, not before");
}

/// RULE-12 (REQ-6526): "any object" and "any control" fire once for each
/// one concerned, and the report names which; the page names none. The
/// pointer is followed inside at most `MAX_POINTER_OBJECTS` objects.
#[test]
fn any_fires_once_for_each_thing_and_names_it() {
    let targets = |trigger: Trigger, under: &[u16], control: u16| {
        let mut evaluator = evaluator(&reporting(&[trigger]));
        let mut world = Script { under: under.to_vec(), ..Script::default() };
        evaluator.tick(&mut world);
        let input = Input::Pointer { action: PointerAction::Press, button: 0, x: 15, y: 15, control };
        evaluator.handle(Event::Input(InputEvent { sequence: 1, input }), &mut world);
        evaluator.tick(&mut world);
        world.reports().iter().map(|report| report.target).collect::<Vec<Target>>()
    };
    let press = |target| Trigger::Press { target: TriggerTarget::of(target), button: 0 };
    assert_eq!(targets(press(Target::ANY_OBJECT), &[5, 9], 0), [Target::object(5), Target::object(9)]);
    assert_eq!(targets(press(Target::ANY_OBJECT), &[], 3), []);
    assert_eq!(targets(press(Target::ANY_CONTROL), &[5], 3), [Target::control(3)]);
    assert_eq!(targets(press(Target::object(9)), &[5, 9], 0), [Target::object(9)]);
    assert_eq!(targets(press(Target::object(7)), &[5, 9], 0), []);
    assert_eq!(targets(press(Target::control(3)), &[], 3), [Target::control(3)]);
    assert_eq!(targets(press(Target::PAGE), &[5], 3), [Target::NONE]);
    assert_eq!(targets(press(Target::rect(0)), &[], 0), [Target::rect(0)]);
    let many: Vec<u16> = (1..=20).collect();
    assert_eq!(targets(press(Target::ANY_OBJECT), &many, 0).len(), MAX_POINTER_OBJECTS);
}

/// RULE-11 (REQ-6526): `becomes` is an edge. A level crossing a
/// threshold fires once and not again while above; falling and rising
/// fires again; a condition that already holds at page tick 0 fires at
/// tick 0; and eight rows of one rule with eight thresholds fire
/// independently.
#[test]
fn becomes_fires_on_the_edge() {
    let level = Value::sound(0, SOUND_LEVEL);
    let mut table = empty();
    let report = group(&mut table, &[built(Command::Report { value: None })]);
    let loud = rule(&mut table, Trigger::Becomes, report);
    table.rules.as_mut_slice()[loud].condition = condition(&[Node::compare(level, Operator::GreaterOrEqual, 128)]);
    let fired_at = |table: &Table, levels: &[u8]| {
        let mut evaluator = evaluator(table);
        let mut world = Script::default();
        for level in levels {
            evaluator.set_metered(&[*level]);
            evaluator.tick(&mut world);
        }
        world.reports().iter().map(|report| (report.tick, report.row)).collect::<Vec<(u32, u8)>>()
    };
    assert_eq!(fired_at(&table, &[0, 200, 210, 128, 50, 127, 130, 255]), [(1, NO_RULE), (6, NO_RULE)]);
    assert_eq!(fired_at(&table, &[200, 200, 0, 200]), [(0, NO_RULE), (3, NO_RULE)], "true at opening fires at tick 0");

    // Eight rows: the threshold is 0 plus each row's `p`.
    let mut table = empty();
    let report = group(&mut table, &[built(Command::Report { value: None })]);
    for threshold in (1..=8).map(|row| row * 30) {
        table.rows.push(threshold).unwrap();
    }
    let meter = rule(&mut table, Trigger::Becomes, report);
    let over = Node::Compare { value: level, operator: Operator::GreaterOrEqual, constant: 0, plus_p: true };
    table.rules.as_mut_slice()[meter].condition = condition(&[over]);
    table.rules.as_mut_slice()[meter].rows = Some(RowRun { first: 0, count: 8 });
    assert_eq!(
        fired_at(&table, &[0, 95, 95, 250, 100, 125]),
        [(1, 0), (1, 1), (1, 2), (3, 3), (3, 4), (3, 5), (3, 6), (3, 7), (5, 3)]
    );
}

/// RULE-15 (REQ-6526): `after 1` fires one tick after the start was
/// applied... counted from the tick the timer runs since; restarting
/// replaces; a stopped timer fires nothing; and `every 30` on the page
/// timer fires at ticks 30, 60 and 90.
#[test]
fn timers_fire_after_and_every() {
    let mut table = empty();
    let start = group(&mut table, &[built(Command::Timer { start: true, timer: 0 })]);
    let stop = group(&mut table, &[built(Command::Timer { start: false, timer: 0 })]);
    let after = mark(&mut table, 1);
    let every = mark(&mut table, 30);
    rule(&mut table, on_key('s'), start);
    rule(&mut table, on_key('x'), stop);
    rule(&mut table, Trigger::After { timer: 0, ticks: 3 }, after);
    rule(&mut table, Trigger::Every { timer: PAGE_TIMER, ticks: 30 }, every);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    let fired = |world: &Script, mark: i32| -> Vec<u32> {
        world
            .reports()
            .iter()
            .filter(|report| matches!(report.what, Reported::Value { reading, .. } if reading == mark))
            .map(|report| report.tick)
            .collect()
    };
    evaluator.tick(&mut world);
    evaluator.handle(Event::Input(press_key(1, 's')), &mut world);
    // Started before tick 1 is evaluated: the timer runs from tick 1.
    run(&mut evaluator, &mut world, 3);
    evaluator.handle(Event::Input(press_key(2, 's')), &mut world);
    // Restarted before tick 4: it runs from tick 4, and fires at tick 7.
    run(&mut evaluator, &mut world, 6);
    assert_eq!(fired(&world, 1), [7], "the first start was replaced before its third tick");
    evaluator.handle(Event::Input(press_key(3, 's')), &mut world);
    evaluator.tick(&mut world);
    evaluator.handle(Event::Input(press_key(4, 'x')), &mut world);
    run(&mut evaluator, &mut world, 85);
    assert_eq!(fired(&world, 1), [7], "stopped before it fired");
    assert_eq!(fired(&world, 30), [30, 60, 90]);
    assert_eq!(evaluator.page_tick(), 95, "the tick of the last tick step run");
}

/// RULE-105, RULE-116 (REQ-6613): binding rows. Thirteen rows fire the
/// right row for each key and none for another; the row's parameters
/// stand in the group's commands; two rules share the rows; and the
/// report names the row.
#[test]
fn a_rule_with_rows_is_matched_once_for_each_row() {
    let mut table = empty();
    let keys = "awsedftgyhujk";
    for number in 0..13 {
        table.rows.push(60 + number).unwrap();
    }
    let by_row = Binding::of(command::field::MIDI_DATA1, Source::Parameter);
    let on = group(&mut table, &[note(0).bound(by_row), built(Command::Report { value: None })]);
    let off_note = built(Command::LocalMidi { channel: 0, status: 0x80, data1: 0, data2: 64 }).bound(by_row);
    let off = group(&mut table, &[off_note]);
    let rows = Some(RowRun { first: 0, count: 13 });
    let row_keys: Vec<Key> = keys.chars().map(|key| Key::of(KeyKind::Physical, key as u32)).collect();
    let by_key = KeyMatch::by_row(&row_keys);
    let down = rule(&mut table, Trigger::Key { keys: by_key, repeat: false }, on);
    // Drawing 40 plus the row's `p`: 100 to 112.
    let object = TriggerTarget::row(TargetKind::Object, 40);
    table.rules.as_mut_slice()[down].triggers.push(Trigger::Press { target: object, button: 0 }).unwrap();
    table.rules.as_mut_slice()[down].rows = rows;
    let up = rule(&mut table, Trigger::KeyUp { keys: by_key }, off);
    table.rules.as_mut_slice()[up].rows = rows;
    let mut evaluator = evaluator(&table);
    let mut world = Script { under: std::vec![105], ..Script::default() };
    evaluator.tick(&mut world);
    for (key, sequence) in keys.chars().zip(1..) {
        evaluator.handle(Event::Input(press_key(sequence, key)), &mut world);
    }
    evaluator.handle(Event::Input(press_key(50, 'z')), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(notes(&world), (60..73).collect::<Vec<u8>>(), "each key its own note, and `z` none");
    let rows: Vec<(u8, u8)> = world.reports().iter().map(|report| (report.rule, report.row)).collect();
    assert_eq!(rows, (0..13).map(|row| (0, row)).collect::<Vec<(u8, u8)>>());
    world.log.clear();
    // The other rule shares the rows; and a press on object 105, which
    // is row 5's `p2`, plays row 5's note.
    evaluator.handle(Event::Input(key(60, KeyAction::Release, 'e')), &mut world);
    evaluator.handle(Event::Input(pointer(61, PointerAction::Press, 0, 1, 1)), &mut world);
    evaluator.tick(&mut world);
    let played: Vec<(u8, u8)> = world.commands().iter().map(|(_, body)| (body[5], body[6])).collect();
    assert_eq!(played, [(0x80, 63), (0x90, 65)]);
    assert_eq!(world.reports()[0].target, Target::object(105));
    assert_eq!(world.reports()[0].row, 5);
}

/// RULE-8, RULE-57 (REQ-6608): `MAX_KEYS_DOWN` keys are tracked as
/// down, and one more is not down for conditions though its `key` rule
/// fires.
#[test]
fn keys_down_are_held_to_their_limit() {
    let mut table = reporting(&[Trigger::Key { keys: KeyMatch::any(), repeat: false }]);
    let count = group(&mut table, &[built(Command::Report { value: Some(Value::keys_down()) })]);
    rule(&mut table, Trigger::Text, count);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    let keys: Vec<char> = ('a'..='z').take(MAX_KEYS_DOWN + 1).collect();
    for (sequence, key) in (1..).zip(keys.iter()) {
        evaluator.handle(Event::Input(press_key(sequence, *key)), &mut world);
        evaluator.tick(&mut world);
    }
    evaluator.handle(Event::Input(InputEvent { sequence: 41, input: Input::Text }), &mut world);
    assert_eq!(
        world.reports().iter().filter(|report| report.rule == 0).count(),
        MAX_KEYS_DOWN + 1,
        "every press fired"
    );
    let down = i32::try_from(MAX_KEYS_DOWN).unwrap();
    assert_eq!(readings(&world), [down], "one over the limit is not down");
}

/// RULE-109, RULE-116 (REQ-6613): a cue fires, at once, the `cue` rules
/// of its audio channel that name its number or any, with the number as
/// the firing's `cue`; a cue over a full queue is dropped and counted.
#[test]
fn a_cue_fires_its_rules() {
    let mut table = empty();
    let read = group(
        &mut table,
        &[
            built(Command::Variable {
                operation: VariableOperation::Set,
                variable: 0,
                operand: Operand::Fired { constant: 100, property: 3 },
            }),
            report_variable(0),
        ],
    );
    rule(&mut table, Trigger::Cue { channel: 1, number: ANY_CUE }, read);
    let seventh = mark(&mut table, 7);
    rule(&mut table, Trigger::Cue { channel: 1, number: 7 }, seventh);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    evaluator.tick(&mut world);
    assert!(evaluator.handle(Event::Cue { channel: 1, number: 3 }, &mut world));
    assert!(evaluator.handle(Event::Cue { channel: 0, number: 7 }, &mut world), "another audio channel's");
    assert!(evaluator.handle(Event::Cue { channel: 1, number: 7 }, &mut world));
    assert_eq!(readings(&world), [103, 107, 7]);
    // Before page tick 0 cues wait, with the events, in a queue that holds
    // `MAX_QUEUED_EVENTS`.
    let mut waiting = super::evaluator(&table);
    for _ in 0..MAX_QUEUED_EVENTS {
        assert!(waiting.handle(Event::Cue { channel: 1, number: 1 }, &mut world));
    }
    assert!(!waiting.handle(Event::Cue { channel: 1, number: 1 }, &mut world));
    assert_eq!(waiting.counters().events_dropped, 1);
}

/// RULE-116 (REQ-6612): the values of the firing, in a condition and in
/// a command: the key, the target, the row and the parameters; a nested
/// run reads its caller's.
#[test]
fn the_firings_values_are_read_in_conditions_and_commands() {
    let mut table = empty();
    table.rows.push(40).unwrap();
    table.rows.push(41).unwrap();
    let fired = |variable: u8, property: u8| {
        built(Command::Variable {
            operation: VariableOperation::Set,
            variable,
            operand: Operand::Fired { constant: 0, property },
        })
    };
    let inner = group(&mut table, &[fired(4, 0), fired(5, 2), fired(6, 4), fired(7, 1)]);
    let outer = group(&mut table, &[built(Command::Run { group: inner })]);
    let keyed = rule(&mut table, Trigger::Key { keys: KeyMatch::any(), repeat: false }, outer);
    table.rules.as_mut_slice()[keyed].rows = Some(RowRun { first: 0, count: 2 });
    // Only the key `b`, and only the second row.
    table.rules.as_mut_slice()[keyed].condition = condition(&[
        Node::And(2),
        Node::compare(Value::fired(0), Operator::Equal, 'b' as i32),
        Node::compare(Value::fired(2), Operator::Equal, 1),
    ]);
    let pressed = rule(&mut table, Trigger::Press { target: TriggerTarget::of(Target::ANY_OBJECT), button: 0 }, inner);
    table.rules.as_mut_slice()[pressed].condition = condition(&[Node::compare(Value::fired(1), Operator::Greater, 8)]);
    let mut evaluator = evaluator(&table);
    let mut world = Script { under: std::vec![5, 9], ..Script::default() };
    evaluator.tick(&mut world);
    evaluator.handle(Event::Input(press_key(1, 'a')), &mut world);
    evaluator.handle(Event::Input(press_key(2, 'b')), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(evaluator.variables()[4..8], ['b' as i32, 1, 41, 0], "the key, the row, `p3`, and no target");
    assert_eq!(evaluator.counters().fired, 1);
    evaluator.handle(Event::Input(pointer(3, PointerAction::Press, 0, 1, 1)), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(evaluator.variables()[4..8], [0, 0, 0, 9], "object 9 alone, with no key, row or parameter");
}

/// RULE-25 (REQ-6613, DEC-6635 item 6): a timer's phase in a period. On a
/// timer started in tick 3, so running from tick 4, the phase in a period
/// of 8 reads 0 at ticks 12 and 20 and 7 at tick 19, and 0 while stopped
/// and before its start tick.
#[test]
fn a_timers_phase_counts_within_its_period() {
    let mut table = empty();
    let phase = Value::timer_phase(0, 8);
    let reads = group(&mut table, &[built(Command::Report { value: Some(phase) })]);
    rule(&mut table, Trigger::Every { timer: PAGE_TIMER, ticks: 1 }, reads);
    let mut evaluator = evaluator(&table);
    let mut world = Script::default();
    run(&mut evaluator, &mut world, 4);
    send(&mut evaluator, &mut world, Command::Timer { start: true, timer: 0 });
    run(&mut evaluator, &mut world, 17);
    let at: Vec<(u32, i32)> = world
        .reports()
        .iter()
        .filter_map(|report| match report.what {
            Reported::Value { reading, .. } => Some((report.tick, reading)),
            _ => None,
        })
        .collect();
    let phase_at = |tick: u32| at.iter().find(|(held, _)| *held == tick).map(|(_, reading)| *reading);
    assert_eq!(
        (phase_at(3), phase_at(4), phase_at(12), phase_at(19), phase_at(20)),
        (Some(0), Some(0), Some(0), Some(7), Some(0))
    );
    assert_eq!(phase_at(5), Some(1));
}
