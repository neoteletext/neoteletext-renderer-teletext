//! The rule evaluator (RULE-71, RULE-83; REQ-6526, REQ-TST-040): any valid
//! table with any script of events, metered values and world answers runs
//! any number of ticks without a panic, never runs more than
//! `MAX_ACTIONS_PER_TICK` actions in a tick, never reports an input event
//! it was not given, and gives the same outputs when run twice. It holds
//! whether the events are handled at the tick or on arrival (RULE-96,
//! DEC-6580), and both ways give the same sound operations, the same
//! other outputs and the same counts at every tick.
//!
//! The crate cannot allocate: it is `no_std` and does not link `alloc`, so
//! that property holds by construction and needs no counting allocator,
//! which this fuzzing crate's ban on unsafe code would not let it define.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::evaluator::{Counters, Evaluator, Reported};
use neoteletext_rules::limits::MAX_ACTIONS_PER_TICK;
use neoteletext_rules::table::{Table, decode};
use rules_world::{FuzzWorld, Given, Step, Way, script, settled, split};

/// The most ticks one input runs, so that a run stays short.
const MAX_TICKS: usize = 200;

/// What one tick and the interval before it gave the world: the sound
/// operations in order, everything else in order, and the counts after.
type Interval = (Vec<Given>, Vec<Given>, Counters);

/// Runs the script once, its events given `way`, and gives what the world
/// was given, whole and tick by tick.
fn run(table: &Table, generation: u16, seed: &[u8], steps: &[Step], way: Way) -> (Vec<Given>, Vec<Interval>) {
    let mut evaluator = Box::new(Evaluator::new(table, generation, 1).expect("a decoded table is evaluated"));
    let mut world = FuzzWorld::new(seed);
    let mut given_events = 0_u32;
    let mut ticks = 0_usize;
    // Where the interval of the next tick began: an event handled on
    // arrival counts against that tick (RULE-96).
    let mut before = 0_usize;
    let mut actions_before = 0_u32;
    let mut intervals = Vec::new();
    for step in steps {
        match step {
            Step::Event(event) => {
                way.give(&mut evaluator, *event, &mut world);
                given_events = event.sequence;
            }
            Step::Metered(values) => evaluator.set_metered(values),
            Step::Subpage(sub) => evaluator.set_subpage(*sub),
            Step::Tick if ticks < MAX_TICKS => {
                ticks += 1;
                if ticks.is_multiple_of(5) {
                    evaluator.query(u32::try_from(ticks).unwrap_or(0));
                }
                evaluator.tick(&mut world);
                let run = evaluator.counters().actions - actions_before;
                assert!(usize::try_from(run).is_ok_and(|run| run <= MAX_ACTIONS_PER_TICK), "{run} actions in a tick");
                let outputs =
                    world.given[before..].iter().filter(|given| !matches!(given, Given::Cycles(_) | Given::State(_)));
                assert!(outputs.count() <= MAX_ACTIONS_PER_TICK);
                for given in &world.given[before..] {
                    if let Given::Report(report) = given
                        && let Reported::Input { sequence } = report.what
                    {
                        assert!((1..=given_events).contains(&sequence), "event {sequence} was never given");
                    }
                }
                let (sounds, others) =
                    world.given[before..].iter().cloned().partition(|given| matches!(given, Given::Sound(_)));
                intervals.push((sounds, others, *evaluator.counters()));
                before = world.given.len();
                actions_before = evaluator.counters().actions;
            }
            Step::Tick => {}
        }
    }
    evaluator.close(&mut world);
    evaluator.tick(&mut world);
    (world.given, intervals)
}

fuzz_target!(|data: &[u8]| {
    let Some((body, rest)) = split(data) else { return };
    let mut table = Box::new(Table::new());
    let Ok(header) = decode(body, &mut table) else { return };
    if !header.has_table {
        return;
    }
    let steps = script(rest);
    let run = |steps: &[Step], way: Way| run(&table, header.generation, rest, steps, way);
    for way in [Way::AtTheTick, Way::OnArrival] {
        assert_eq!(run(&steps, way), run(&steps, way), "the same script gives the same outputs, {way:?}");
    }
    // RULE-96: only the moment a sound operation is given differs.
    let steps = settled(&steps);
    assert_eq!(run(&steps, Way::AtTheTick).1, run(&steps, Way::OnArrival).1, "both ways give the same at every tick");
});
