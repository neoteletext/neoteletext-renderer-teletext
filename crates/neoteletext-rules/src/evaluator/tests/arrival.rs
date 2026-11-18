//! Evaluation on arrival (REQ-6526, DEC-6580 item 1; RULE-5, RULE-7 and
//! RULE-96): an event given by `push_now` has its rules' sound operations
//! at once and everything else at its tick, and both ways of giving an
//! event have the same result.

use super::*;
use crate::limits::{MAX_ACTIONS_PER_TICK, MAX_EVENTS_PER_TICK, MAX_QUEUED_EVENTS};
use crate::table::{INPUT_CHARACTER, TIMER_ELAPSED};

const NOTE_ON: Action = Action::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 };
const NOTE_ON_SOUND: SoundOperation = SoundOperation::Midi { channel: 0, status: 0x90, data1: 60, data2: 100 };

/// A world with a local sound on audio channel 0.
fn with_local_sound() -> Script {
    let mut world = Script::default();
    world.sounds[0] = SoundState { playing: true, local: true, handle: 1, gain: 100, family: 14, variant: 0 };
    world
}

/// RULE-96, RULE-5 (REQ-6526): the four sound actions of a rule fired by
/// an event given on arrival reach the world before the tick, in written
/// order; its variable, its animation operation, its `send` and its `echo`
/// wait for the tick and leave in order, without the sound operations;
/// and the tick does not evaluate the event again.
#[test]
fn sound_actions_leave_on_arrival_and_the_rest_at_the_tick() {
    let object = Target::object(5);
    let moved = Operation { target: object, kind: OperationKind::Move { relative: true, x: 8, y: 0 } };
    let mut table = Table::new();
    let count = variable(&mut table, 0, 9, 0);
    let play = Action::Play { channel: 1, handle: 5, generation: 2, gain: 80, fade_ms: 5, looped: false };
    let mut key = rule(
        on_key('a'),
        &[
            SEND,
            NOTE_ON,
            Action::Animate(moved),
            play,
            Action::Add { variable: count, amount: 1 },
            Action::Stop { channel: 0, fade_ms: 20 },
            Action::Gain { channel: 0, gain: 10, fade_ms: 0 },
        ],
    );
    key.echo = true;
    table.rules.push(key).unwrap();
    table.rules.push(rule(on_key('v'), &[Action::Send(Some(Value::variable(count)))])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound().with(object, shown_at(0, 0, 10, 10));
    world.handles.push(5);
    evaluator.tick(&mut world);
    world.log.clear();

    assert!(evaluator.push_now(press_key(77, 'a'), &mut world));
    assert_eq!(
        world.log,
        std::vec![
            Out::Sound(NOTE_ON_SOUND),
            Out::Sound(SoundOperation::Play {
                channel: 1,
                handle: 5,
                generation: 2,
                gain: 80,
                fade_ms: 5,
                looped: false
            }),
            Out::Sound(SoundOperation::Stop { channel: 0, fade_ms: 20 }),
            Out::Sound(SoundOperation::Gain { channel: 0, gain: 10, fade_ms: 0 }),
        ],
        "only the sound operations, at once"
    );
    assert_eq!(world.state(object).x, 0, "the animation operation waits for the tick");
    assert_eq!(evaluator.page_tick(), 1, "no tick has run");
    world.log.clear();

    evaluator.tick(&mut world);
    let report = |what| Out::Report(Report { rule: 0, tick: 1, target: Target::NONE, what });
    assert_eq!(
        world.log,
        std::vec![
            Out::Cycles(1),
            report(Reported::Input { sequence: 77 }),
            Out::Animate(moved, 1),
            report(Reported::Input { sequence: 77 }),
        ],
        "the rest at the tick, in order, and no sound operation a second time"
    );
    assert_eq!(evaluator.counters().fired, 1, "the event is evaluated once");
    assert_eq!(evaluator.counters().actions, 8);
    world.log.clear();
    evaluator.push_now(press_key(78, 'v'), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(world.reports(), [(1, Reported::Value { value: Value::variable(count), reading: 1 })]);
}

/// RULE-7 as revised, RULE-6 (REQ-6526): a condition evaluated on arrival
/// reads the variables and timers as the last completed tick left them;
/// what an earlier event of the same interval wrote is not seen until the
/// next tick, whichever way the events are given.
#[test]
fn an_arrival_reads_what_the_last_completed_tick_left() {
    let mut table = Table::new();
    table.timers.push(false).unwrap();
    let armed = variable(&mut table, 0, 1, 0);
    table.rules.push(rule(on_key('s'), &[Action::Set { variable: armed, value: 1 }, Action::StartTimer(0)])).unwrap();
    table.rules.push(when(rule(on_key('a'), &[NOTE_ON]), &[is(Value::variable(armed), 1)])).unwrap();
    table.rules.push(when(rule(on_key('t'), &[NOTE_ON]), &[is(Value::timer(0, TIMER_ELAPSED), 1)])).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound();
    evaluator.tick(&mut world);
    // Tick 1's interval: `s` arms, and `a` after it in the same interval
    // still reads the variable as tick 0 left it.
    evaluator.push_now(press_key(1, 's'), &mut world);
    evaluator.push_now(press_key(2, 'a'), &mut world);
    assert_eq!(world.log.iter().filter(|out| matches!(out, Out::Sound(_))).count(), 0);
    evaluator.tick(&mut world);
    // Tick 2's interval: tick 1 is complete, so `a` sounds at once; the
    // timer started in tick 1 runs since tick 2 and reads 0 there.
    world.log.clear();
    evaluator.push_now(press_key(3, 'a'), &mut world);
    evaluator.push_now(press_key(4, 't'), &mut world);
    assert_eq!(world.log, [Out::Sound(NOTE_ON_SOUND)]);
    evaluator.tick(&mut world);
    world.log.clear();
    evaluator.push_now(press_key(5, 't'), &mut world);
    assert_eq!(world.log, [Out::Sound(NOTE_ON_SOUND)], "one tick elapsed at tick 3");
}

/// RULE-8, RULE-96 (REQ-6526): key state on arrival is as the events that
/// arrived before left it, in arrival order: a chord given in one interval
/// fires at once, and not when the keys arrive the other way round.
#[test]
fn key_state_on_arrival_follows_arrival_order() {
    let held = Value::key(INPUT_CHARACTER, u32::from('z'));
    let table = table_of(&[when(rule(on_key('a'), &[NOTE_ON]), &[is(held, 1)])]);
    let mut world = with_local_sound();
    let mut first = evaluator(&table);
    first.push_now(press_key(1, 'z'), &mut world);
    first.push_now(press_key(2, 'a'), &mut world);
    assert_eq!(world.log, [Out::Sound(NOTE_ON_SOUND)]);
    world.log.clear();
    let mut second = evaluator(&table);
    second.push_now(press_key(1, 'a'), &mut world);
    second.push_now(press_key(2, 'z'), &mut world);
    assert_eq!(world.log, []);
}

/// RULE-7 as revised (REQ-6526): the snapshot of a tick is taken once,
/// when the first event of its interval arrives; a metered report, a
/// sound started and a sub-page shown after it are seen by the next tick,
/// by the tick's own `becomes` rules as by later arrivals.
#[test]
fn the_world_after_the_snapshot_is_seen_by_the_next_tick() {
    let level = Value::sound(0, SOUND_LEVEL);
    let mut table = Table::new();
    table.rules.push(when(rule(on_key('a'), &[NOTE_ON]), &[above(level, 100)])).unwrap();
    table.rules.push(when(rule(Trigger::Becomes, &[SEND]), &[above(level, 100)])).unwrap();
    table.rules.push(rule(on_key('b'), &[SEND])).unwrap();
    table.subpages.push(SubpageMask { sub: 0x0002, off: 0b100 }).unwrap();
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound();
    evaluator.tick(&mut world);
    world.log.clear();
    evaluator.push_now(press_key(1, 'b'), &mut world);
    // After the snapshot of tick 1.
    evaluator.set_metered(&[200]);
    evaluator.set_subpage(0x0002);
    evaluator.push_now(press_key(2, 'a'), &mut world);
    evaluator.push_now(press_key(3, 'b'), &mut world);
    evaluator.tick(&mut world);
    assert_eq!(world.log.iter().filter(|out| matches!(out, Out::Sound(_))).count(), 0);
    assert_eq!(world.take_rules(), [2, 2], "tick 1 read the level and the sub-page of its snapshot");
    evaluator.push_now(press_key(4, 'a'), &mut world);
    evaluator.push_now(press_key(5, 'b'), &mut world);
    assert_eq!(world.log, [Out::Sound(NOTE_ON_SOUND)]);
    evaluator.tick(&mut world);
    assert_eq!(world.take_rules(), [1], "tick 2 sees both: the level, and rule 2 off");
}

/// RULE-5 step 2, RULE-96 (REQ-6526): events are handled in arrival
/// order whichever way each is given: one waiting from `push` is handled
/// before a later one given on arrival; the 17th of an interval waits for
/// the next tick, where its sound operation leaves with the tick; and the
/// 65th of an interval is dropped and counted, as with `push` alone.
#[test]
fn arrivals_keep_the_order_and_the_bounds_of_the_tick() {
    let table = table_of(&[rule(any_key_down(), &[SEND, NOTE_ON])]);
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound();
    let sequences = |world: &Script| -> Vec<u32> {
        world
            .reports()
            .iter()
            .map(|(_, what)| if let Reported::Input { sequence } = what { *sequence } else { panic!("{what:?}") })
            .collect()
    };
    let sounds = |world: &Script| world.log.iter().filter(|out| matches!(out, Out::Sound(_))).count();
    assert!(evaluator.push(press_key(0, 'a')));
    assert!(evaluator.push_now(press_key(1, 'a'), &mut world));
    assert_eq!(sounds(&world), 2, "the one that waited is handled first, and both sound");
    for sequence in 2..64 {
        assert!(evaluator.push_now(press_key(sequence, 'a'), &mut world));
    }
    assert_eq!(sounds(&world), MAX_EVENTS_PER_TICK, "sixteen events a tick, on arrival as at the tick");
    assert!(!evaluator.push_now(press_key(64, 'a'), &mut world), "the 65th waiting is dropped");
    assert!(!evaluator.push(press_key(65, 'a')));
    assert_eq!(evaluator.counters().events_dropped, 2);
    evaluator.tick(&mut world);
    assert_eq!(sequences(&world), (0..16).collect::<Vec<_>>());
    assert_eq!(sounds(&world), MAX_EVENTS_PER_TICK);
    // The next tick takes the next sixteen from the queue itself.
    evaluator.tick(&mut world);
    assert_eq!(sequences(&world), (0..32).collect::<Vec<_>>());
    assert_eq!(sounds(&world), 2 * MAX_EVENTS_PER_TICK);
    // An arrival now is behind the 32 that wait: the next sixteen are
    // handled at once, and it waits its turn.
    assert!(evaluator.push_now(press_key(66, 'a'), &mut world));
    assert_eq!(sounds(&world), 3 * MAX_EVENTS_PER_TICK);
    for _ in 0..3 {
        evaluator.tick(&mut world);
    }
    let mut expected: Vec<u32> = (0..64).collect();
    expected.push(66);
    assert_eq!(sequences(&world), expected);
    assert_eq!(sounds(&world), MAX_QUEUED_EVENTS + 1);
}

/// RULE-6, RULE-96 (REQ-6526): a firing on arrival counts against its
/// tick's 256 actions; a firing past the cap is dropped whole, its sound
/// operations with it, and the tick's own rules find the cap already
/// spent.
#[test]
fn arrivals_count_against_the_ticks_actions() {
    let mut loud = rule(any_key_down(), &[NOTE_ON; MAX_ACTIONS]);
    loud.echo = false;
    let mut rules = std::vec![loud; 2];
    rules.push(rule(Trigger::Every { timer: PAGE_TIMER, ticks: 1 }, &[SEND]));
    let table = table_of(&rules);
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound();
    evaluator.tick(&mut world);
    world.log.clear();
    // Each event fires two rules of 16 actions: eight events are 256.
    for sequence in 0..10 {
        evaluator.push_now(press_key(sequence, 'a'), &mut world);
    }
    assert_eq!(world.log.len(), MAX_ACTIONS_PER_TICK, "no sound operation past the cap");
    assert_eq!(evaluator.counters().overruns, 4);
    evaluator.tick(&mut world);
    assert_eq!(world.reports(), [], "the timer's rule is dropped: the tick's actions were spent on arrival");
    assert_eq!(evaluator.counters().overruns, 5);
    world.log.clear();
    evaluator.tick(&mut world);
    assert_eq!(world.rules(), [2], "the next tick has its own 256");
}

/// RULE-96 (REQ-6526): the page closing after an arrival and before its
/// tick runs that tick first, so that what the event's rules wrote and
/// reported is not lost when their sound has already left.
#[test]
fn closing_completes_the_tick_of_an_arrival() {
    let table = table_of(&[rule(on_key('a'), &[NOTE_ON, SEND]), rule(Trigger::Close, &[SEND])]);
    let mut evaluator = evaluator(&table);
    let mut world = with_local_sound();
    evaluator.push_now(press_key(9, 'a'), &mut world);
    evaluator.close(&mut world);
    assert_eq!(world.reports(), [(0, Reported::Input { sequence: 9 }), (1, Reported::Fired)]);
    assert_eq!(evaluator.counters().ticks, 1);
    assert!(!evaluator.push_now(press_key(10, 'a'), &mut world) || evaluator.is_closed());
    assert_eq!(world.log.iter().filter(|out| matches!(out, Out::Sound(_))).count(), 1, "nothing after the close");
}

/// A small generator of numbers for the property test: xorshift, 64 bits
/// (Marsaglia, "Xorshift RNGs", 2003, the 13, 7, 17 triple).
struct Numbers(u64);

impl Numbers {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// A number below `bound`.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn byte(&mut self, bound: u8) -> u8 {
        u8::try_from(self.below(u64::from(bound))).unwrap()
    }

    fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[usize::try_from(self.below(from.len() as u64)).unwrap()]
    }
}

const KEYS: [char; 4] = ['a', 'b', 'c', 'd'];
const THINGS: [Target; 3] = [Target::object(1), Target::object(2), Target::control(3)];
const VARIABLES: u8 = 3;
const TIMERS: u8 = 2;
const RULES: u8 = 12;

fn random_value(numbers: &mut Numbers) -> Value {
    match numbers.below(8) {
        0 | 1 => Value::variable(numbers.byte(VARIABLES)),
        2 => Value::key(INPUT_CHARACTER, u32::from(numbers.pick(&KEYS))),
        3 => Value::keys_down(),
        4 => Value::timer(numbers.byte(TIMERS), TIMER_ELAPSED),
        5 => Value::sound(numbers.byte(2), numbers.pick(&[SOUND_LEVEL, SOUND_PLAYING, SOUND_LOCAL])),
        6 => {
            let thing = numbers.pick(&THINGS);
            Value::target(thing.kind, thing.number, numbers.pick(&[TARGET_SHOWN, TARGET_X, TARGET_INSIDE]))
        }
        _ => Value::pointer(POINTER_OVER),
    }
}

fn random_term(numbers: &mut Numbers) -> Node {
    let operator = numbers.pick(&[Operator::Equal, Operator::NotEqual, Operator::Less, Operator::Greater]);
    Node::Compare { value: random_value(numbers), operator, constant: i32::from(numbers.byte(4)) }
}

fn random_condition(numbers: &mut Numbers) -> Vec<Node> {
    match numbers.below(5) {
        0 => Vec::new(),
        1 | 2 => std::vec![random_term(numbers)],
        3 => std::vec![Node::And(2), random_term(numbers), random_term(numbers)],
        _ => std::vec![Node::Or(2), Node::Not, random_term(numbers), random_term(numbers)],
    }
}

fn random_trigger(numbers: &mut Numbers) -> Trigger {
    let key = numbers.pick(&KEYS);
    let target = numbers.pick(&[THINGS[0], THINGS[1], THINGS[2], Target::PAGE]);
    match numbers.below(14) {
        0..=3 => on_key(key),
        4 => any_key_down(),
        5 => {
            let Trigger::Key { keys, .. } = on_key(key) else { panic!() };
            Trigger::KeyUp { keys, mods_mask: 0, mods_value: 0 }
        }
        6 => Trigger::Press { target, button: ANY_BUTTON },
        7 => Trigger::Release { target, button: ANY_BUTTON },
        8 => Trigger::Enter { target },
        9 => Trigger::Text,
        10 => Trigger::Every { timer: numbers.pick(&[PAGE_TIMER, 0, 1]), ticks: 1 + u32::from(numbers.byte(4)) },
        11 => Trigger::After { timer: numbers.pick(&[PAGE_TIMER, 0, 1]), ticks: 1 + u32::from(numbers.byte(6)) },
        12 => Trigger::Open,
        _ => Trigger::Becomes,
    }
}

fn random_action(numbers: &mut Numbers) -> Action {
    let channel = numbers.byte(2);
    let thing = numbers.pick(&THINGS);
    let mask = 1_u64 << numbers.below(u64::from(RULES));
    match numbers.below(16) {
        0..=2 => Action::Midi { channel, status: 0x90, data1: 60 + numbers.byte(12), data2: 100 },
        3 => Action::Play { channel, handle: numbers.byte(4), generation: 1, gain: 80, fade_ms: 5, looped: false },
        4 => Action::Stop { channel, fade_ms: 5 },
        5 => Action::Gain { channel, gain: numbers.byte(101), fade_ms: 5 },
        6 => Action::Set { variable: numbers.byte(VARIABLES), value: i32::from(numbers.byte(4)) },
        7 | 8 => Action::Add { variable: numbers.byte(VARIABLES), amount: i32::from(numbers.byte(3)) - 1 },
        9 => Action::StartTimer(numbers.byte(TIMERS)),
        10 => Action::StopTimer(numbers.byte(TIMERS)),
        11 => Action::Enable(mask),
        12 => Action::Disable(mask),
        13 => Action::Send(Some(random_value(numbers))),
        // A control takes `show`, `hide` and `look` alone (ANIM-6).
        14 if thing.kind == TargetKind::Control => Action::Animate(Operation {
            target: thing,
            kind: numbers.pick(&[OperationKind::Show, OperationKind::Hide]),
        }),
        14 => Action::Animate(Operation {
            target: thing,
            kind: numbers.pick(&[
                OperationKind::Show,
                OperationKind::Hide,
                OperationKind::Move { relative: true, x: 40, y: 0 },
            ]),
        }),
        _ => SEND,
    }
}

/// A table of twelve rules the codec accepts.
fn random_table(numbers: &mut Numbers) -> Box<Table> {
    let mut table = Box::new(Table::new());
    for _ in 0..VARIABLES {
        variable(&mut table, 0, 3, 0);
    }
    for _ in 0..TIMERS {
        table.timers.push(numbers.below(2) == 0).unwrap();
    }
    table.subpages.push(SubpageMask { sub: 0x0002, off: numbers.below(1 << RULES) }).unwrap();
    for _ in 0..RULES {
        let trigger = random_trigger(numbers);
        let mut actions = Vec::new();
        for _ in 0..=numbers.below(5) {
            actions.push(random_action(numbers));
        }
        let mut made = when(rule(trigger, &actions), &random_condition(numbers));
        if trigger == Trigger::Becomes && made.condition.is_empty() {
            made.condition.push(random_term(numbers)).unwrap();
        }
        made.echo = numbers.below(3) == 0;
        made.enabled = numbers.below(6) != 0;
        table.rules.push(made).unwrap();
    }
    table
}

fn random_event(numbers: &mut Numbers, sequence: u32) -> InputEvent {
    let key = numbers.pick(&KEYS);
    let x = u16::try_from(numbers.below(300)).unwrap();
    match numbers.below(10) {
        0..=3 => press_key(sequence, key),
        4 | 5 => release_key(sequence, key),
        6 => pointer(sequence, PointerAction::Press, 0, x, 20, 3),
        7 => pointer(sequence, PointerAction::Release, 0, x, 20, 0),
        8 => pointer(sequence, PointerAction::Move, 0, x, 20, 0),
        _ => InputEvent { sequence, input: Input::Text },
    }
}

/// What one interval and its tick gave the world: the sound operations in
/// order, everything else in order, and the counts after the tick.
type Interval = (Vec<SoundOperation>, Vec<Out>, Counters);

/// How one event is given.
#[derive(Clone, Copy, PartialEq)]
enum Way {
    Queued,
    OnArrival,
}

/// One interval's script: the world's changes when it begins, then its
/// events.
struct Step {
    sounds: [SoundState; AUDIO_CHANNELS],
    metered: [u8; 2],
    sub: u16,
    events: Vec<(InputEvent, Way)>,
}

fn random_script(numbers: &mut Numbers) -> Vec<Step> {
    let mut sequence = 0_u32;
    let mut steps = Vec::new();
    for _ in 0..40 {
        let mut sounds = [SoundState::default(); AUDIO_CHANNELS];
        for sound in sounds.iter_mut().take(2) {
            let local = numbers.below(3) != 0;
            *sound = SoundState { playing: numbers.below(4) != 0, local, handle: 1, gain: 100, family: 14, variant: 0 };
        }
        // Mostly a few events; sometimes more than a tick handles, and
        // sometimes more than the queue holds.
        let count = match numbers.below(20) {
            0 => 70,
            1 | 2 => 20,
            3..=8 => 0,
            _ => numbers.below(5),
        };
        let mut events = Vec::new();
        for _ in 0..count {
            sequence += 1;
            let way = if numbers.below(4) == 0 { Way::Queued } else { Way::OnArrival };
            events.push((random_event(numbers, sequence), way));
        }
        steps.push(Step {
            sounds,
            metered: [numbers.byte(4), numbers.byte(4)],
            sub: 1 + u16::from(numbers.byte(2)),
            events,
        });
    }
    steps
}

/// Runs a script, each event given as its step says or, with `all`, every
/// event that way.
fn run_script(table: &Table, steps: &[Step], all: Option<Way>) -> Vec<Interval> {
    let mut evaluator = Box::new(Evaluator::new(table, 1, 0x0001).unwrap());
    let mut world = Script::default()
        .with(THINGS[0], shown_at(0, 0, 100, 100))
        .with(THINGS[1], shown_at(50, 0, 200, 100))
        .with(THINGS[2], shown_at(0, 0, 40, 40));
    world.handles.extend([1, 2]);
    let mut intervals = Vec::new();
    for (step, number) in steps.iter().zip(0_u32..) {
        world.sounds = step.sounds;
        evaluator.set_metered(&step.metered);
        evaluator.set_subpage(step.sub);
        evaluator.query(number);
        for (event, way) in &step.events {
            match all.unwrap_or(*way) {
                Way::Queued => evaluator.push(*event),
                Way::OnArrival => evaluator.push_now(*event, &mut world),
            };
        }
        evaluator.tick(&mut world);
        let (mut sounds, mut others) = (Vec::new(), Vec::new());
        for out in world.log.drain(..) {
            if let Out::Sound(operation) = out { sounds.push(operation) } else { others.push(out) }
        }
        intervals.push((sounds, others, *evaluator.counters()));
    }
    evaluator.close(&mut world);
    intervals.push((Vec::new(), world.log, *evaluator.counters()));
    intervals
}

/// RULE-96, RULE-5 (REQ-6526, DEC-6580 item 1): for random tables and
/// random events at random places, giving every event at the tick, every
/// event on arrival, or each either way gives the world the same sound
/// operations in the same order, the same animation operations, reports
/// and state answers in the same order, and the same counts, at every
/// tick: only the moment a sound operation is given differs.
#[test]
fn both_ways_of_giving_an_event_have_the_same_result() {
    const TABLES: u64 = 300;
    let mut early_sounds = 0_usize;
    let mut fired = 0_u32;
    for seed in 1..=TABLES {
        // The multiplier spreads consecutive seeds over the 64 bits
        // (Knuth's MMIX linear congruential multiplier).
        let mut numbers = Numbers(seed.wrapping_mul(6_364_136_223_846_793_005) | 1);
        let table = random_table(&mut numbers);
        let mut encoded = [0_u8; crate::limits::MAX_TABLE_BYTES];
        crate::encode(0x101, 1, Some(&table), &mut encoded).unwrap();
        let steps = random_script(&mut numbers);
        let at_the_tick = run_script(&table, &steps, Some(Way::Queued));
        let on_arrival = run_script(&table, &steps, Some(Way::OnArrival));
        let mixed = run_script(&table, &steps, None);
        for (((queued, arrived), mixed), tick) in at_the_tick.iter().zip(&on_arrival).zip(&mixed).zip(0_u32..) {
            assert_eq!(queued, arrived, "table {seed}, tick {tick}: on arrival");
            assert_eq!(queued, mixed, "table {seed}, tick {tick}: either way");
        }
        assert_eq!(at_the_tick.len(), on_arrival.len());
        early_sounds += at_the_tick.iter().map(|(sounds, ..)| sounds.len()).sum::<usize>();
        fired += at_the_tick.last().unwrap().2.fired;
    }
    // The instrument exercises what it claims to.
    assert!(early_sounds > 1000, "{early_sounds} sound operations");
    assert!(fired > 10_000, "{fired} firings");
}
