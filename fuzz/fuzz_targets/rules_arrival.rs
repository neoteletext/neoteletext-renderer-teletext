//! Evaluation on arrival (RULE-96, RULE-83; REQ-6526, DEC-6580,
//! REQ-TST-040): for any table and events, the sound commands delivered
//! on arrival are exactly those the same events give when every command
//! is run at the tick, in the same order, with the same amounts and the
//! same page ticks; and every other output, every counter and the sixteen
//! variables are the same at the end of every tick, whether the events
//! are decided at the tick, on arrival, or some each way.
#![no_main]

#[path = "rules_world.rs"]
mod rules_world;

use libfuzzer_sys::fuzz_target;
use rules_world::{Way, run, script, settled, split, table_of};

fuzz_target!(|data: &[u8]| {
    let Some((image, rest)) = split(data) else { return };
    let Some(table) = table_of(image) else { return };
    // RULE-7: the tick's one snapshot is taken at the interval's first
    // arrival, so the interval's metered reports, cues and commands of
    // the server come before its events.
    let steps = settled(&script(rest));
    let at_the_tick = run(&table, rest, &steps, Way::AtTheTick, &mut |_, _| {});
    let on_arrival = run(&table, rest, &steps, Way::OnArrival, &mut |_, _| {});
    for (tick, (expected, given)) in at_the_tick.iter().zip(&on_arrival).enumerate() {
        assert_eq!(expected.0, given.0, "tick {tick}: the sound commands");
        assert_eq!(expected.1, given.1, "tick {tick}: every other output");
        assert_eq!((expected.2, expected.3), (given.2, given.3), "tick {tick}: the counters and the variables");
    }
    assert_eq!(at_the_tick.len(), on_arrival.len());
    assert_eq!(run(&table, rest, &steps, Way::Mixed, &mut |_, _| {}), at_the_tick, "mixed");
});
