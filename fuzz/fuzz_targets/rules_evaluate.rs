//! The rule evaluator (RULE-71, RULE-83; REQ-6526, REQ-TST-040): any valid
//! table with any script of events, cues, server commands, metered values
//! and world answers runs any number of ticks without a panic, never
//! applies more commands in one run than the table's constant, never
//! reports an input event it was not given, and gives the same outputs
//! when run twice.
//!
//! The crate cannot allocate: it is `no_std` and does not link `alloc`, so
//! that property holds by construction and needs no counting allocator,
//! which this fuzzing crate's ban on unsafe code would not let it define.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::evaluator::{Counters, Reported};
use neoteletext_rules::limits::{MAX_GROUP_COMMANDS, MAX_QUEUED_COMMANDS, MAX_RUN_COMMANDS};
use rules_world::{Given, Step, Way, run, script, split, table_of};

fuzz_target!(|data: &[u8]| {
    let Some((image, rest)) = split(data) else { return };
    let Some(table) = table_of(image) else { return };
    let steps = script(rest);
    let given_events = steps.iter().filter(|step| matches!(step, Step::Event(_))).count();
    // RULE-120: the most one run of any group applies is a constant of
    // the table, within `MAX_RUN_COMMANDS`.
    let most = (0..table.groups.len())
        .map(|group| table.run_commands(u8::try_from(group).expect("at most 64 groups")))
        .max()
        .unwrap_or(0);
    assert!(most <= MAX_RUN_COMMANDS);
    for way in [Way::AtTheTick, Way::OnArrival, Way::Mixed] {
        let mut before = Counters::default();
        let mut each = |evaluator: &neoteletext_rules::Evaluator, tick: &[Given]| {
            let now = *evaluator.counters();
            // Every command applied is one of a group run, or one the
            // server sent; a run applies no more than its group holds.
            let groups = usize::try_from(now.groups - before.groups).expect("a count");
            let commands = usize::try_from(now.commands - before.commands).expect("a count");
            assert!(commands <= groups * MAX_GROUP_COMMANDS + MAX_QUEUED_COMMANDS, "{commands} commands, {groups} groups");
            let fired = usize::try_from(now.fired - before.fired).expect("a count");
            assert!(commands <= (groups.max(fired)) * most.max(1) + MAX_QUEUED_COMMANDS);
            before = now;
            for given in tick {
                if let Given::Report(report) = given
                    && let Reported::Input { sequence } = report.what
                {
                    let known = usize::try_from(sequence).is_ok_and(|sequence| (1..=given_events).contains(&sequence));
                    assert!(known, "event {sequence} was never given");
                }
            }
        };
        let first = run(&table, rest, &steps, way, &mut each);
        assert_eq!(first, run(&table, rest, &steps, way, &mut |_, _| {}), "the same script, the same outputs: {way:?}");
    }
});
