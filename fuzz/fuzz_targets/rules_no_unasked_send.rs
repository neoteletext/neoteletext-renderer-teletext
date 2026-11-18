//! No input unless asked (REQ-6523; RULE-83; REQ-6526, REQ-TST-040): for
//! any table and events, every report of an input event is of an event
//! that was given, that is of the kind its rule's trigger takes, and whose
//! rule holds `send` or `echo` and was enabled in the tick that reported
//! it. A table with no rule that sends reports nothing at all. It holds
//! whether the events are handled at the tick or on arrival (RULE-96).
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::evaluator::{Evaluator, Input, PointerAction, Reported};
use neoteletext_rules::table::{Action, Rule, Table, Trigger, decode};
use rules_world::{FuzzWorld, Given, Step, Way, script, settled, split};

/// The most ticks one input runs, so that a run stays short.
const MAX_TICKS: usize = 200;

/// Whether `rule` reports its trigger's own event (RULE-22).
fn sends_its_event(rule: &Rule) -> bool {
    rule.echo || rule.actions.iter().any(|action| matches!(action, Action::Send(None)))
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

/// Runs the script with its events given `way`, and holds the property.
fn check(table: &Table, generation: u16, rest: &[u8], steps: &[Step], way: Way) {
    let mut evaluator = Box::new(Evaluator::new(table, generation, 1).expect("a decoded table is evaluated"));
    let mut world = FuzzWorld::new(rest);
    let mut events: Vec<Input> = Vec::new();
    let mut sub = 1_u16;
    let mut ticks = 0_usize;
    for step in steps {
        match step {
            Step::Event(event) => {
                // Sequence numbers count from 1 in order, so an event's
                // place is its number less one.
                events.push(event.input);
                way.give(&mut evaluator, *event, &mut world);
            }
            Step::Metered(values) => evaluator.set_metered(values),
            Step::Subpage(shown) => {
                sub = *shown;
                evaluator.set_subpage(sub);
            }
            Step::Tick if ticks < MAX_TICKS => {
                ticks += 1;
                // The answer to a state query says which rules the tick's
                // snapshot had enabled.
                evaluator.query(1);
                let before = world.given.len();
                evaluator.tick(&mut world);
                let tick = &world.given[before..];
                let enabled = tick
                    .iter()
                    .find_map(|given| {
                        if let Given::State(state) = given { state.table.map(|table| table.enabled) } else { None }
                    })
                    .expect("a query is answered at the end of the next tick");
                let on = enabled & !table.off_for(sub);
                for given in tick {
                    let Given::Report(report) = given else { continue };
                    let rule = table.rules.get(usize::from(report.rule)).expect("a report names a rule of the table");
                    assert!(on & (1_u64 << report.rule) != 0, "rule {} was not enabled", report.rule);
                    if let Reported::Input { sequence } = report.what {
                        let place = usize::try_from(sequence).ok().and_then(|sequence| sequence.checked_sub(1));
                        let input = place.and_then(|place| events.get(place)).expect("the event reported was given");
                        assert!(sends_its_event(rule), "rule {} asks for no event", report.rule);
                        assert!(takes(&rule.trigger, input), "rule {} does not take {input:?}", report.rule);
                    }
                }
            }
            Step::Tick => {}
        }
    }
    if !table.rules.iter().any(|rule| rule.echo || rule.actions.iter().any(|action| matches!(action, Action::Send(_))))
    {
        assert!(!world.given.iter().any(|given| matches!(given, Given::Report(_))), "nothing asked, nothing sent");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((body, rest)) = split(data) else { return };
    let mut table = Box::new(Table::new());
    let Ok(header) = decode(body, &mut table) else { return };
    if !header.has_table {
        return;
    }
    let steps = script(rest);
    check(&table, header.generation, rest, &steps, Way::AtTheTick);
    // On arrival a tick's snapshot, the sub-page shown with it, is taken
    // at the interval's first event (RULE-7), so the sub-page a tick's
    // rules were enabled for is the one shown before the events.
    check(&table, header.generation, rest, &settled(&steps), Way::OnArrival);
});
