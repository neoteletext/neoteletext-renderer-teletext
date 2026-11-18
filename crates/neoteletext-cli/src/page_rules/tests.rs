#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use neoteletext_input::event::{Key, read_message};
use neoteletext_rules::limits::MAX_TABLE_BYTES;
use neoteletext_rules::table::Rule;
use neoteletext_web::records::rules::{read, read_query};

/// The page on screen in these tests: 100, sub-code 0000.
const ON: Stamp = Stamp { page: 0x100, sub: 0 };
/// One tick of the presentation clock and a little, in microseconds.
const A_TICK_US: u64 = 17_000;

/// The body of a RULES record for `page`.
fn body(page: u16, generation: u16, table: Option<&Table>) -> Vec<u8> {
    let mut out = vec![0_u8; MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(page, generation, table, &mut out).unwrap();
    out.truncate(length);
    out
}

/// The default table of a channel that sends every input (RULE-53).
fn send_all() -> Box<Table> {
    let mut table = Box::new(Table::new());
    neoteletext_rules::default_send_all(false, &mut table);
    table
}

/// Gives `rules` the RULES record of `body`.
fn give(rules: &mut PageRules, body: &[u8], now_us: u64) {
    rules.record(&ChannelRecord::Rules(read(body).unwrap()), now_us);
}

/// Rules with page 100 on screen and the default table open at time 0.
fn opened() -> PageRules {
    let mut rules = PageRules::new();
    give(&mut rules, &body(ON.page, 1, Some(&send_all())), 0);
    assert!(rules.show(Some(ON), 0));
    rules
}

fn press(character: char) -> Item<'static> {
    let key = KeyCode::Character(character);
    Item::Event(Event::Key(Key { on: ON, action: Action::Press, key, mods: 0, text: None, base: None, lost: false }))
}

fn ticked(rules: &mut PageRules, now_us: u64) -> Vec<Outgoing> {
    let mut out = Vec::new();
    rules.tick(now_us, &mut out);
    out
}

fn bytes_of(item: &Item<'_>) -> Vec<u8> {
    let mut out = [0_u8; MAX_MESSAGE];
    let length = write(item, &mut out).unwrap();
    out[..length].to_vec()
}

/// RULE-50 (REQ-6523, REQ-6528): a page with no table sends nothing,
/// whatever the viewer does: with no record at all, and with a record
/// that says the page has no table.
#[test]
fn a_page_with_no_table_sends_nothing() {
    let mut rules = PageRules::new();
    assert!(!rules.show(Some(ON), 0));
    rules.input(&press('a'));
    rules.input(&Item::Text("hi"));
    rules.input(&Item::Event(Event::FocusOut));
    assert_eq!(ticked(&mut rules, A_TICK_US), []);
    assert_eq!(rules.wait(A_TICK_US), None, "nothing to evaluate");

    give(&mut rules, &body(ON.page, 1, None), A_TICK_US);
    assert!(!rules.is_open());
    rules.input(&press('a'));
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), []);
}

/// RULE-46 (REQ-6528): a key reaches the channel inside the `RULE_EVENT`
/// of the rule that asked for it, byte for byte as the viewer made it,
/// and only once the tick that evaluates it has run.
#[test]
fn a_key_goes_inside_the_rule_event_of_the_rule_that_asked() {
    let mut rules = opened();
    rules.input(&press('a'));
    assert!(rules.wait(0).is_some(), "the key waits for its tick");
    let out = ticked(&mut rules, 0);
    let key = bytes_of(&press('a'));
    // Rule 0 of the default table is `keys` (RULE-53); page tick 0.
    let expected =
        Outgoing::Rule { on: ON, generation: 1, rule: 0, tick: 0, target: Target::NONE, body: Body::Input(key) };
    assert_eq!(out, core::slice::from_ref(&expected));
    // As the input stream has it: a server's reader takes the message and
    // finds the key inside.
    let message = expected.with_item(bytes_of);
    assert_eq!(message[..2], [0xFF, neoteletext_input::event::RULE_EVENT]);
    let mut read = Vec::new();
    read_message(&message, &mut |item| {
        let Item::Event(Event::RuleEvent(event)) = item else { panic!("not a RULE_EVENT: {item:?}") };
        read.push((event.rule, event.tick, event.input().map(|(item, _)| item == press('a'))));
    })
    .unwrap();
    assert_eq!(read, [(0, 0, Some(true))]);
    assert_eq!(rules.wait(A_TICK_US), None, "the default table has no timed rule");
    assert_eq!(ticked(&mut rules, A_TICK_US), [], "the key is reported once");
}

/// RULE-53: text and the focus are sent by their rules of the default
/// table, a release by `keys_up`, in the order the viewer made them.
#[test]
fn the_default_table_sends_text_focus_and_releases() {
    let mut rules = opened();
    assert_eq!(ticked(&mut rules, 0), []);
    let release = Item::Event(Event::Key(Key {
        on: ON,
        action: Action::Release,
        key: KeyCode::Character('a'),
        mods: 0,
        text: None,
        base: None,
        lost: true,
    }));
    let given = [Item::Event(Event::FocusIn), Item::Text("hi"), release, Item::Event(Event::FocusOut)];
    for item in &given {
        rules.input(item);
    }
    let sent: Vec<(u8, Vec<u8>)> = ticked(&mut rules, A_TICK_US)
        .into_iter()
        .map(|outgoing| match outgoing {
            Outgoing::Rule { rule, tick: 1, body: Body::Input(bytes), .. } => (rule, bytes),
            other => panic!("{other:?}"),
        })
        .collect();
    // `focus` is rule 10, `text` 2, `keys_up` 1 and `blur` 11.
    let rules_in_order = [10, 2, 1, 11];
    let expected: Vec<(u8, Vec<u8>)> = rules_in_order.into_iter().zip(given.iter().map(bytes_of)).collect();
    assert_eq!(sent, expected);
}

/// RULE-2, RULE-41: a record of another generation replaces the page's
/// table and opens it afresh; one of the same generation changes nothing;
/// one that says "no table" leaves the page sending nothing.
#[test]
fn a_new_generation_replaces_the_table() {
    let mut rules = opened();
    assert_eq!(ticked(&mut rules, 0), []);
    give(&mut rules, &body(ON.page, 1, None), A_TICK_US);
    assert!(rules.is_open(), "the same generation is the table already held");
    give(&mut rules, &body(ON.page, 2, Some(&send_all())), A_TICK_US);
    rules.input(&press('b'));
    let out = ticked(&mut rules, A_TICK_US);
    assert!(
        matches!(out.as_slice(), [Outgoing::Rule { generation: 2, rule: 0, tick: 0, .. }]),
        "the new table's page tick 0: {out:?}"
    );
    give(&mut rules, &body(ON.page, 3, None), 2 * A_TICK_US);
    assert!(!rules.is_open());
    rules.input(&press('c'));
    assert_eq!(ticked(&mut rules, 3 * A_TICK_US), []);
}

/// RULE-70, RULE-48: a table the decoder refuses is refused whole, the
/// page has no table, and fault 1 is reported once, when the page is on
/// screen, with the record's generation.
#[test]
fn a_refused_table_is_reported_and_sends_nothing() {
    let mut rules = PageRules::new();
    let mut refused = body(ON.page, 7, Some(&send_all()));
    refused.push(0);
    give(&mut rules, &refused, 0);
    assert_eq!(ticked(&mut rules, 0), [], "no page is on screen to stamp it");
    assert!(!rules.show(Some(ON), 0));
    rules.input(&press('a'));
    let fault = RulesFault { on: ON, generation: 7, fault: FaultCode::Undecodable, count: 1 };
    assert_eq!(ticked(&mut rules, A_TICK_US), [Outgoing::Fault(fault)]);
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), []);
}

/// RULE-5, RULE-8: another page closes the open one: input still waiting
/// is evaluated first, then the `close` rules fire, all stamped with the
/// page that closed; and the page opened has its own table or none.
#[test]
fn another_page_closes_the_open_one() {
    let mut table = send_all();
    let mut close = Rule::new(Trigger::Close);
    close.echo = true;
    let closing = u8::try_from(table.rules.push(close).unwrap()).unwrap();
    let mut rules = PageRules::new();
    give(&mut rules, &body(ON.page, 1, Some(&table)), 0);
    assert!(rules.show(Some(ON), 0));
    assert_eq!(ticked(&mut rules, 0), []);
    rules.input(&press('a'));
    let other = Stamp { page: 0x200, sub: 0 };
    assert!(!rules.show(Some(other), A_TICK_US), "page 200 has no table");
    let out = ticked(&mut rules, A_TICK_US);
    let fired: Vec<(Stamp, u8, bool)> = out
        .iter()
        .map(|outgoing| match outgoing {
            Outgoing::Rule { on, rule, body, .. } => (*on, *rule, *body == Body::Fired),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(fired, [(ON, 0, false), (ON, closing, true)]);
    rules.input(&press('b'));
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), [], "page 200 sends nothing");
}

/// RULE-47 (REQ-6528): a state query of the open page is answered once,
/// at the end of the next tick; a second within a second is dropped and
/// reported as fault 6; a query of another page is ignored; a page with
/// no table answers with generation 0.
#[test]
fn a_state_query_is_answered_once_a_second() {
    let query = |page: u16, identifier: u32| {
        let bytes = [&page.to_be_bytes()[..], &identifier.to_be_bytes()].concat();
        read_query(&bytes).unwrap()
    };
    let mut rules = opened();
    assert_eq!(ticked(&mut rules, 0), []);
    rules.record(&ChannelRecord::RulesQuery(query(0x200, 9)), A_TICK_US);
    assert_eq!(rules.wait(A_TICK_US), None, "a query of a page that is not open is ignored");
    rules.record(&ChannelRecord::RulesQuery(query(ON.page, 42)), A_TICK_US);
    rules.record(&ChannelRecord::RulesQuery(query(ON.page, 43)), A_TICK_US);
    let out = ticked(&mut rules, A_TICK_US);
    let [Outgoing::State { on: ON, body }, Outgoing::Fault(fault)] = out.as_slice() else { panic!("{out:?}") };
    let state = neoteletext_rules::state::decode(body).unwrap();
    let table = state.table.unwrap();
    assert_eq!((state.identifier, table.generation, table.tick), (42, 1, 1));
    assert_eq!(*fault, RulesFault { on: ON, generation: 1, fault: FaultCode::QueryDropped, count: 1 });

    let mut bare = PageRules::new();
    assert!(!bare.show(Some(ON), 0));
    bare.record(&ChannelRecord::RulesQuery(query(ON.page, 5)), 0);
    assert_eq!(bare.wait(0), Some(0));
    let out = ticked(&mut bare, 0);
    let [Outgoing::State { body, .. }] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(neoteletext_rules::state::decode(body).unwrap(), PageState { identifier: 5, table: None });
}

/// RULE-4: a table with a timed rule is evaluated at every tick due, and
/// its timer's reports name the page ticks they fired at.
#[test]
fn a_timed_rule_fires_on_its_ticks() {
    let mut table = Box::new(Table::new());
    let mut every = Rule::new(Trigger::Every { timer: neoteletext_rules::limits::PAGE_TIMER, ticks: 2 });
    every.echo = true;
    table.rules.push(every).unwrap();
    let mut rules = PageRules::new();
    give(&mut rules, &body(ON.page, 1, Some(&table)), 0);
    assert!(rules.show(Some(ON), 0));
    let mut fired = Vec::new();
    for turn in 0..7_u64 {
        let now_us = turn.saturating_mul(A_TICK_US);
        assert!(rules.wait(now_us).is_some(), "a timed table always has a next tick");
        for outgoing in ticked(&mut rules, now_us) {
            let Outgoing::Rule { tick, body: Body::Fired, .. } = outgoing else { panic!("{outgoing:?}") };
            fired.push(tick);
        }
    }
    assert_eq!(fired, [2, 4, 6]);
}
