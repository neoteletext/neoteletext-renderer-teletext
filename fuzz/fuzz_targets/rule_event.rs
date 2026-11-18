//! `RULE_EVENT` and `RULES_FAULT` (RULE-46, RULE-48, RULE-83; REQ-6528,
//! REQ-TST-040): the readers never panic; what they accept writes back to
//! the same bytes; a `RULE_EVENT`'s input is one carried event with at
//! most its PICK, or one run of text; and at the top level of the stream
//! a viewer's input is refused wherever it stands.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_input::MAX_MESSAGE;
use neoteletext_input::event::{Event, InputError, Item, is_page_input, read, read_items, read_message, write};
use neoteletext_input::rules::RuleBody;

fuzz_target!(|data: &[u8]| {
    // The top level: every item given is one the session may hold, and
    // the lexical reader, which has no such rule, finds a viewer's input
    // exactly when the top level refuses the message for one.
    let mut top = 0_usize;
    let at_top = read_message(data, &mut |item| {
        assert!(!is_page_input(&item), "a viewer's input was read at the top level");
        top += 1;
    });
    let mut items = 0_usize;
    let mut bare = false;
    let lexical = read_items(data, &mut |item| {
        items += 1;
        bare |= is_page_input(&item);
    });
    match at_top {
        Ok(()) => assert!(lexical.is_ok() && !bare && top == items),
        Err(InputError::Bare) => assert!(bare),
        Err(_) => assert!(lexical.is_err() || bare),
    }
    // One item: a rule table's message writes back to the bytes read.
    let Ok(Some((item, used))) = read(data) else { return };
    let Item::Event(event @ (Event::RuleEvent(_) | Event::RulesFault(_))) = item else { return };
    let mut out = [0_u8; MAX_MESSAGE];
    let length = write(&Item::Event(event), &mut out).expect("what was read is written");
    assert_eq!(&out[..length], &data[..used]);
    if let Event::RuleEvent(report) = event {
        match (report.body, report.input()) {
            (RuleBody::Input(bytes), Some((first, pick))) => {
                let mut carried = 0_usize;
                read_items(bytes, &mut |_| carried += 1).expect("what a RULE_EVENT carries is whole items");
                assert_eq!(carried, 1 + usize::from(pick.is_some()));
                assert!(is_page_input(&first), "a RULE_EVENT carries a viewer's input and nothing else");
            }
            (RuleBody::Input(_), None) => panic!("an input body that does not read"),
            (_, input) => assert!(input.is_none()),
        }
    }
});
