//! No input unless asked (REQ-6523; RULE-22, RULE-83; REQ-6526,
//! REQ-TST-040): for any table and events, every report of an input event
//! is of an event that was given, in the tick the event belongs to, made
//! by a rule one of whose triggers takes that kind of event and whose
//! group, with the groups it runs in place, holds a `REPORT` of the
//! firing; and a run not made by an input's rule reports no event. A
//! table with no `REPORT` reports nothing at all, but for what the server
//! itself asks.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::command::{self, Command, TYPE_REPORT, TYPE_RUN};
use neoteletext_rules::evaluator::{Input, PointerAction, Reported};
use neoteletext_rules::limits::{MAX_GROUP_DEPTH, NO_RULE};
use neoteletext_rules::table::{Table, Trigger};
use rules_world::{Given, Step, run, script, split, table_of};

/// How many `REPORT`s of the firing one run of `group` applies, with the
/// groups it runs in place (RULE-22, RULE-102).
fn reports_of_the_firing(table: &Table, group: usize, depth: usize) -> usize {
    if depth >= MAX_GROUP_DEPTH {
        return 0;
    }
    table
        .groups
        .commands(group)
        .map(|held| match command::decode(held.kind, held.body) {
            Ok((_, _, Command::Report { value: None })) if held.kind == TYPE_REPORT => 1,
            Ok((_, _, Command::Run { group })) if held.kind == TYPE_RUN => {
                reports_of_the_firing(table, usize::from(group), depth + 1)
            }
            _ => 0,
        })
        .sum()
}

/// Whether an event of `input` is of the kind `trigger` takes (RULE-9).
fn takes(trigger: &Trigger, input: &Input) -> bool {
    match (trigger, input) {
        (Trigger::Key { .. } | Trigger::KeyUp { .. }, Input::Key { .. })
        | (Trigger::Text, Input::Text)
        | (Trigger::Focus, Input::Focus)
        | (Trigger::Blur, Input::Blur)
        | (Trigger::Wheel { .. }, Input::Wheel { .. })
        | (Trigger::Enter { .. } | Trigger::Leave { .. }, Input::Pointer { .. } | Input::Wheel { .. }) => true,
        (Trigger::Press { .. }, Input::Pointer { action, .. }) => *action == PointerAction::Press,
        (Trigger::Release { .. }, Input::Pointer { action, .. }) => *action == PointerAction::Release,
        (Trigger::Move { .. }, Input::Pointer { action, .. }) => *action == PointerAction::Move,
        _ => false,
    }
}

/// Runs the script, and holds the property.
fn check(table: &Table, rest: &[u8], steps: &[Step]) {
    // Sequence numbers count from 1 in order, so an event's place is its
    // number less one.
    let events: Vec<Input> =
        steps.iter().filter_map(|step| if let Step::Event(event) = step { Some(event.input) } else { None }).collect();
    let mut reported = 0_usize;
    let mut each = |evaluator: &neoteletext_rules::Evaluator, tick: &[Given]| {
        // The tick just run, or the final tick of a closed page.
        let ran = evaluator.page_tick().saturating_sub(u32::from(!evaluator.is_closed()));
        // An event of the interval after it belongs to the coming tick.
        let ran = ran.saturating_add(1);
        for given in tick {
            let Given::Report(report) = given else { continue };
            reported += 1;
            let Reported::Input { sequence } = report.what else { continue };
            assert!(report.tick <= ran, "a report of tick {} in tick {ran}", report.tick);
            let place = usize::try_from(sequence).ok().and_then(|sequence| sequence.checked_sub(1));
            let input = place.and_then(|place| events.get(place)).expect("the event reported was given");
            assert_ne!(report.rule, NO_RULE, "a run no rule made reported an event");
            let rule = table.rules.get(usize::from(report.rule)).expect("a report names a rule of the table");
            assert!(
                reports_of_the_firing(table, usize::from(rule.group), 0) > 0,
                "rule {} asks for no event",
                report.rule
            );
            assert!(
                rule.triggers.iter().any(|trigger| takes(trigger, input)),
                "rule {} does not take {input:?}",
                report.rule
            );
        }
    };
    run(table, rest, steps, &mut each, &mut |_, _| {});
    let asks = (0..table.groups.len()).any(|group| table.groups.commands(group).any(|held| held.kind == TYPE_REPORT));
    let server_asks = steps.iter().any(|step| matches!(step, Step::Command(Command::Report { .. })));
    if !asks && !server_asks {
        assert_eq!(reported, 0, "nothing asked, nothing sent");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((image, rest)) = split(data) else { return };
    let Some(table) = table_of(image) else { return };
    let steps = script(rest);
    check(&table, rest, &steps);
});
