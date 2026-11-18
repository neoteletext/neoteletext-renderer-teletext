//! The per-tick budgets and the replay of a recording (DEC-6635 items 4
//! and 5, RULE-5, RULE-83; REQ-6526, REQ-TST-040): for any table and any
//! script of events, cues, server commands, queries and metered values,
//! no tick handles more than `MAX_EVENTS_PER_TICK` events; every event is
//! handled, waits, or is dropped at a full queue and counted; and a
//! recording of every step with the place among the tick steps it was
//! given, replayed in that order, gives the world the same outputs in the
//! same order and leaves the same counters and variables after every tick.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::limits::MAX_EVENTS_PER_TICK;
use neoteletext_rules::table::Table;
use rules_world::{FuzzWorld, Interval, MAX_TICKS, Step, evaluator_of, give, run, script, split, table_of};

/// Replays a recording made by `run`: each step given, in its order,
/// after as many tick steps as had run when it was recorded, with the
/// same ticks and the same close.
fn replay(table: &Table, seed: &[u8], recording: &[(u32, Step)], ticks: usize) -> Vec<Interval> {
    let mut evaluator = evaluator_of(table);
    let mut world = FuzzWorld::new(seed);
    let mut recorded = recording.iter().peekable();
    let mut before = 0_usize;
    let mut intervals = Vec::new();
    for run in 0..ticks {
        let run = u32::try_from(run).unwrap_or(u32::MAX);
        while let Some((_, step)) = recorded.next_if(|(at, _)| *at == run) {
            give(&mut evaluator, &mut world, step);
        }
        evaluator.tick(&mut world);
        intervals.push((world.given[before..].to_vec(), *evaluator.counters(), *evaluator.variables()));
        before = world.given.len();
    }
    for (_, step) in recorded {
        give(&mut evaluator, &mut world, step);
    }
    evaluator.close(&mut world);
    evaluator.tick(&mut world);
    intervals.push((world.given[before..].to_vec(), *evaluator.counters(), *evaluator.variables()));
    intervals
}

/// Gives each step and runs each tick, holding the events of every tick
/// to `MAX_EVENTS_PER_TICK` and every event accounted for.
fn budgets(table: &Table, seed: &[u8], steps: &[Step]) {
    let mut evaluator = evaluator_of(table);
    let mut world = FuzzWorld::new(seed);
    let budget = u32::try_from(MAX_EVENTS_PER_TICK).expect("a small number");
    let (mut events, mut this_tick, mut handled, mut ticks) = (0_u32, 0_u32, 0_u32, 0_usize);
    for step in steps {
        if matches!(step, Step::Tick) {
            if ticks >= MAX_TICKS {
                continue;
            }
            ticks += 1;
            evaluator.tick(&mut world);
            // The events that waited, handled after the tick step, are the
            // new tick's first.
            this_tick = evaluator.counters().events_handled - handled;
        } else {
            give(&mut evaluator, &mut world, step);
            events += u32::from(!matches!(step, Step::Metered(_)));
            this_tick += evaluator.counters().events_handled - handled;
        }
        handled = evaluator.counters().events_handled;
        assert!(this_tick <= budget, "{this_tick} events in one tick");
        let counters = evaluator.counters();
        let waiting = u32::try_from(evaluator.waiting()).expect("a small number");
        assert_eq!(counters.events_handled + counters.events_dropped + waiting, events, "every event accounted for");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((image, rest)) = split(data) else { return };
    let Some(table) = table_of(image) else { return };
    let steps = script(rest);
    budgets(&table, rest, &steps);
    let mut recording: Vec<(u32, Step)> = Vec::new();
    let live = run(&table, rest, &steps, &mut |_, _| {}, &mut |ticks, step| recording.push((ticks, step)));
    // The live run's tick steps, less the final one after the close.
    let ticks = live.len() - 1;
    let replayed = replay(&table, rest, &recording, ticks);
    assert_eq!(live.len(), replayed.len());
    for (tick, (expected, given)) in live.iter().zip(&replayed).enumerate() {
        assert_eq!(expected, given, "interval {tick}");
    }
});
