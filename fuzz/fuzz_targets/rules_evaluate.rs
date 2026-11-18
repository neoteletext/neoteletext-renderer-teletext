//! The rule evaluator (RULE-71, RULE-83; REQ-6526, REQ-TST-040): any valid
//! table with any script of events, metered values and world answers runs
//! any number of ticks without a panic, never runs more than
//! `MAX_ACTIONS_PER_TICK` actions in a tick, never reports an input event
//! it was not given, and gives the same outputs when run twice.
//!
//! The crate cannot allocate: it is `no_std` and does not link `alloc`, so
//! that property holds by construction and needs no counting allocator,
//! which this fuzzing crate's ban on unsafe code would not let it define.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::evaluator::{Evaluator, Reported};
use neoteletext_rules::limits::MAX_ACTIONS_PER_TICK;
use neoteletext_rules::table::{Table, decode};
use rules_world::{FuzzWorld, Given, Step, script, split};

/// The most ticks one input runs, so that a run stays short.
const MAX_TICKS: usize = 200;

/// Runs the script once, and gives what the world was given.
fn run(table: &Table, generation: u16, seed: &[u8], steps: &[Step]) -> Vec<Given> {
    let mut evaluator = Box::new(Evaluator::new(table, generation, 1).expect("a decoded table is evaluated"));
    let mut world = FuzzWorld::new(seed);
    let mut given_events = 0_u32;
    let mut ticks = 0_usize;
    for step in steps {
        match step {
            Step::Event(event) => {
                evaluator.push(*event);
                given_events = event.sequence;
            }
            Step::Metered(values) => evaluator.set_metered(values),
            Step::Subpage(sub) => evaluator.set_subpage(*sub),
            Step::Tick if ticks < MAX_TICKS => {
                ticks += 1;
                let before = world.given.len();
                let actions_before = evaluator.counters().actions;
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
            }
            Step::Tick => {}
        }
    }
    evaluator.close(&mut world);
    evaluator.tick(&mut world);
    world.given
}

fuzz_target!(|data: &[u8]| {
    let Some((body, rest)) = split(data) else { return };
    let mut table = Box::new(Table::new());
    let Ok(header) = decode(body, &mut table) else { return };
    if !header.has_table {
        return;
    }
    let steps = script(rest);
    let first = run(&table, header.generation, rest, &steps);
    assert_eq!(first, run(&table, header.generation, rest, &steps), "the same script gives the same outputs");
});
