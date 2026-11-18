#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::*;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_input::event::{Key, read_message};
use neoteletext_rules::command::{Command, Operand, VariableOperation};
use neoteletext_rules::limits::{MAX_TABLE_BYTES, NO_RULE, PAGE_TIMER};
use neoteletext_rules::table::{Framed, Rule, Trigger};
use neoteletext_web::records::commands;
use neoteletext_web::records::rules::{Fragment, RulesQueryRecord};

/// The page on screen in these tests: 100, sub-code 0001.
const ON: Stamp = Stamp { page: 0x100, sub: 1 };
/// One tick of the presentation clock and a little, in microseconds.
const A_TICK_US: u64 = 17_000;

fn place(on: Stamp) -> (PageAddress, SubCode) {
    (PageAddress::from_mpp(on.page).unwrap(), SubCode::new(on.sub))
}

/// Gives `rules` the RULES record of `table` for the page `on`, or the
/// record that says the page has none.
fn give(rules: &mut PageRules, on: Stamp, generation: u16, table: Option<&Table>, now_us: u64) {
    let (page, subcode) = place(on);
    let mut image = vec![0_u8; MAX_TABLE_BYTES];
    let length = table.map_or(0, |table| neoteletext_rules::encode(table, body_check, &mut image).unwrap());
    let fragment = table.map(|_| Fragment { total: u16::try_from(length).unwrap(), offset: 0, data: &image[..length] });
    rules.record(&ChannelRecord::Rules(RulesRecord { page, subcode, generation, fragment }), now_us);
}

/// The default table of a channel that sends every input (RULE-53).
fn send_all(on: Stamp) -> Box<Table> {
    let mut table = Box::new(Table::new(on.page, on.sub));
    neoteletext_rules::default_send_all(false, on.page, on.sub, &mut table);
    table
}

/// A command's body for the page `on`.
fn body_of(command: &Command, on: Stamp) -> Vec<u8> {
    let (page, subcode) = place(on);
    let mut out = [0_u8; neoteletext_rules::command::MAX_BODY_BYTES];
    let length = commands::body(command, page, subcode, &mut out).unwrap();
    out[..length].to_vec()
}

/// Adds to `table` a group of `commands`, and gives its number.
fn group(table: &mut Table, commands: &[Command]) -> u8 {
    let on = Stamp { page: table.page, sub: table.sub };
    let bodies: Vec<(u8, Vec<u8>)> = commands.iter().map(|command| (command.kind(), body_of(command, on))).collect();
    let framed: Vec<Framed<'_>> =
        bodies.iter().map(|(kind, body)| Framed { kind: *kind, body, bindings: &[] }).collect();
    table.push_group(&framed).unwrap()
}

/// Rules with the page on screen and the default table open at time 0,
/// its page tick 0 run.
fn opened() -> PageRules {
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&send_all(ON)), 0);
    assert!(rules.show(Some(ON), 0));
    assert_eq!(ticked(&mut rules, 0), [], "page tick 0: the default table has no `open` rule");
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

    give(&mut rules, ON, 1, None, A_TICK_US);
    assert!(!rules.is_open());
    rules.input(&press('a'));
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), []);
}

/// RULE-46, RULE-5 (REQ-6528): a key reaches the channel inside the
/// `RULE_EVENT` of the rule whose group reported it, byte for byte as the
/// viewer made it, and only once the tick that evaluates it has run. A
/// key that arrives before page tick 0 has run belongs to page tick 1.
#[test]
fn a_key_goes_inside_the_rule_event_of_the_rule_that_asked() {
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&send_all(ON)), 0);
    assert!(rules.show(Some(ON), 0));
    rules.input(&press('a'));
    assert!(rules.wait(0).is_some(), "the key waits for its tick");
    assert_eq!(ticked(&mut rules, 0), [], "no input belongs to page tick 0");
    assert!(rules.wait(0).is_some(), "the key still waits");
    let out = ticked(&mut rules, A_TICK_US);
    let key = bytes_of(&press('a'));
    // Rule 0 of the default table is `keys` (RULE-53); it has no rows.
    let expected = Outgoing::Rule {
        on: ON,
        generation: 1,
        rule: 0,
        row: NO_RULE,
        tick: 1,
        target: Target::NONE,
        body: Body::Input(key),
    };
    assert_eq!(out, core::slice::from_ref(&expected));
    // As the input stream has it: a server's reader takes the message and
    // finds the key inside.
    let message = expected.with_item(bytes_of);
    assert_eq!(message[..2], [0xFF, neoteletext_input::event::RULE_EVENT]);
    let mut read = Vec::new();
    read_message(&message, &mut |item| {
        let Item::Event(Event::RuleEvent(event)) = item else { panic!("not a RULE_EVENT: {item:?}") };
        read.push((event.rule, event.row, event.tick, event.input().map(|(item, _)| item == press('a'))));
    })
    .unwrap();
    assert_eq!(read, [(0, NO_RULE, 1, Some(true))]);
    assert_eq!(rules.wait(2 * A_TICK_US), None, "the default table has nothing time alone fires");
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), [], "the key is reported once");
}

/// RULE-53: text and the focus are sent by their rules of the default
/// table, a release by `keys_up`, in the order the viewer made them.
#[test]
fn the_default_table_sends_text_focus_and_releases() {
    let mut rules = opened();
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

/// RULE-2, RULE-41: a later table replaces the page's and opens it
/// afresh, with every variable 0; a record that says "no table" leaves
/// the page sending nothing.
#[test]
fn a_later_table_replaces_the_table() {
    let mut table = send_all(ON);
    let counted = group(
        &mut table,
        &[
            Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None },
            Command::Report { value: Some(Value::variable(0)) },
        ],
    );
    table.rules.as_mut_slice()[0].group = counted;
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&table), 0);
    assert!(rules.show(Some(ON), 0));
    assert_eq!(ticked(&mut rules, 0), []);
    let count = |rules: &mut PageRules, now_us: u64| {
        rules.input(&press('b'));
        match ticked(rules, now_us).as_slice() {
            [Outgoing::Rule { generation, tick, body: Body::Value { reading, .. }, .. }] => {
                (*generation, *tick, *reading)
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(count(&mut rules, A_TICK_US), (1, 1, 1));
    assert_eq!(count(&mut rules, 2 * A_TICK_US), (1, 2, 2));
    // The same table again, at the next generation: the count starts
    // again, at the new table's page tick 1.
    give(&mut rules, ON, 2, Some(&table), 3 * A_TICK_US);
    assert!(rules.is_open());
    assert_eq!(ticked(&mut rules, 3 * A_TICK_US), []);
    assert_eq!(count(&mut rules, 4 * A_TICK_US), (2, 1, 1));
    give(&mut rules, ON, 3, None, 5 * A_TICK_US);
    assert!(!rules.is_open());
    rules.input(&press('c'));
    assert_eq!(ticked(&mut rules, 6 * A_TICK_US), []);
}

/// RULE-112, RULE-2 (REQ-6615): a sub-page is a page. A table for
/// 100/0001 is not evaluated while 100/0002 is on screen; going from one
/// to the other closes one and opens the other, each with its own table
/// and its own variables, all 0 at each opening.
#[test]
fn two_sub_codes_of_one_page_number_are_two_pages() {
    let other = Stamp { page: ON.page, sub: 2 };
    let counting = |on: Stamp, by: i32| {
        let mut table = Box::new(Table::new(on.page, on.sub));
        let counted = group(
            &mut table,
            &[
                Command::Variable { operation: VariableOperation::Add, variable: 0, operand: Operand::Constant(by) },
                Command::Report { value: Some(Value::variable(0)) },
            ],
        );
        table
            .rules
            .push(Rule::new(Trigger::Key { keys: neoteletext_rules::table::KeyMatch::any(), repeat: false }, counted))
            .unwrap();
        table
    };
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&counting(ON, 1)), 0);
    assert!(!rules.show(Some(other), 0), "the table is 100/0001's, and 100/0002 is on screen");
    rules.input(&press('a'));
    assert_eq!(ticked(&mut rules, A_TICK_US), [], "a page with no table sends nothing");
    give(&mut rules, other, 2, Some(&counting(other, 10)), A_TICK_US);
    assert!(rules.is_open());
    let pressed = |rules: &mut PageRules, now_us: u64| {
        rules.input(&press('a'));
        let out = ticked(rules, now_us);
        match out.as_slice() {
            [] => None,
            [Outgoing::Rule { on, body: Body::Value { reading, .. }, .. }] => Some((*on, *reading)),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(ticked(&mut rules, A_TICK_US), [], "page tick 0 of 100/0002");
    assert_eq!(pressed(&mut rules, 2 * A_TICK_US), Some((other, 10)));
    assert_eq!(pressed(&mut rules, 3 * A_TICK_US), Some((other, 20)));
    // To 100/0001: its own table, its own variable, from 0.
    assert!(rules.show(Some(ON), 4 * A_TICK_US));
    assert_eq!(ticked(&mut rules, 4 * A_TICK_US), []);
    assert_eq!(pressed(&mut rules, 5 * A_TICK_US), Some((ON, 1)));
    // And back: 100/0002 opens afresh, its variable 0 again.
    assert!(rules.show(Some(other), 6 * A_TICK_US));
    assert_eq!(ticked(&mut rules, 6 * A_TICK_US), []);
    assert_eq!(pressed(&mut rules, 7 * A_TICK_US), Some((other, 10)));
}

/// RULE-70, RULE-48: a table the decoder refuses is refused whole, the
/// page has no table, and fault 1 is reported once, when the page is on
/// screen, with the record's generation.
#[test]
fn a_refused_table_is_reported_and_sends_nothing() {
    let mut rules = PageRules::new();
    let (page, subcode) = place(ON);
    let mut image = vec![0_u8; MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(&send_all(ON), body_check, &mut image).unwrap();
    // A byte after the image's end.
    let total = u16::try_from(length + 1).unwrap();
    let fragment = Some(Fragment { total, offset: 0, data: &image[..=length] });
    rules.record(&ChannelRecord::Rules(RulesRecord { page, subcode, generation: 7, fragment }), 0);
    assert_eq!(ticked(&mut rules, 0), [], "no page is on screen to stamp it");
    assert!(!rules.show(Some(ON), 0));
    rules.input(&press('a'));
    let fault = RulesFault { on: ON, generation: 7, fault: FaultCode::Undecodable, count: 1 };
    assert_eq!(ticked(&mut rules, A_TICK_US), [Outgoing::Fault(fault)]);
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), []);
    // RULE-41: a fragment out of place, for the page on screen.
    let later = Some(Fragment { total, offset: 5, data: &image[5..=length] });
    rules.record(&ChannelRecord::Rules(RulesRecord { page, subcode, generation: 8, fragment: later }), 3 * A_TICK_US);
    let fault = RulesFault { on: ON, generation: 8, fault: FaultCode::Undecodable, count: 1 };
    assert_eq!(ticked(&mut rules, 3 * A_TICK_US), [Outgoing::Fault(fault)]);
}

/// RULE-5, RULE-8, RULE-111: another page closes the open one: input
/// still waiting is evaluated first, then the `close` rules fire, all
/// stamped with the page that closed; and the page opened has its own
/// table or none.
#[test]
fn another_page_closes_the_open_one() {
    let mut table = send_all(ON);
    let closing = u8::try_from(table.rules.push(Rule::new(Trigger::Close, 0)).unwrap()).unwrap();
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&table), 0);
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
/// reported as fault 6; a query of another page, or of another sub-code
/// of this page number, is ignored; a page with no table answers with
/// generation 0.
#[test]
fn a_state_query_is_answered_once_a_second() {
    let query = |on: Stamp, identifier: u32| {
        let (page, subcode) = place(on);
        ChannelRecord::RulesQuery(RulesQueryRecord { page, subcode, identifier })
    };
    let mut rules = opened();
    rules.record(&query(Stamp { page: 0x200, sub: ON.sub }, 9), A_TICK_US);
    rules.record(&query(Stamp { page: ON.page, sub: 2 }, 9), A_TICK_US);
    assert_eq!(rules.wait(A_TICK_US), Some(0), "only the tick after page tick 0 is owed");
    assert_eq!(ticked(&mut rules, A_TICK_US), [], "a query of a page that is not open is ignored");
    rules.record(&query(ON, 42), 2 * A_TICK_US);
    rules.record(&query(ON, 43), 2 * A_TICK_US);
    let out = ticked(&mut rules, 2 * A_TICK_US);
    let [Outgoing::State { on: ON, body }, Outgoing::Fault(fault)] = out.as_slice() else { panic!("{out:?}") };
    let state = neoteletext_rules::state::decode(body).unwrap();
    let table = state.table.unwrap();
    assert_eq!((state.identifier, table.generation, table.tick), (42, 1, 2));
    assert_eq!(table.variables, [0; 16]);
    assert_eq!(*fault, RulesFault { on: ON, generation: 1, fault: FaultCode::QueryDropped, count: 1 });

    let mut bare = PageRules::new();
    assert!(!bare.show(Some(ON), 0));
    bare.record(&query(ON, 5), 0);
    assert_eq!(bare.wait(0), Some(0));
    let out = ticked(&mut bare, 0);
    let [Outgoing::State { body, .. }] = out.as_slice() else { panic!("{out:?}") };
    assert_eq!(neoteletext_rules::state::decode(body).unwrap(), PageState { identifier: 5, table: None });
}

/// RULE-4: a table with a timed rule is evaluated at every tick due, and
/// its timer's reports name the page ticks they fired at.
#[test]
fn a_timed_rule_fires_on_its_ticks() {
    let mut table = Box::new(Table::new(ON.page, ON.sub));
    let report = group(&mut table, &[Command::Report { value: None }]);
    table.rules.push(Rule::new(Trigger::Every { timer: PAGE_TIMER, ticks: 2 }, report)).unwrap();
    let mut rules = PageRules::new();
    give(&mut rules, ON, 1, Some(&table), 0);
    assert!(rules.show(Some(ON), 0));
    let mut fired = Vec::new();
    for turn in 0..7_u64 {
        let now_us = turn.saturating_mul(A_TICK_US);
        assert!(rules.wait(now_us).is_some(), "a timed table always has a next tick");
        for outgoing in ticked(&mut rules, now_us) {
            let Outgoing::Rule { tick, rule: 0, row: NO_RULE, body: Body::Fired, .. } = outgoing else {
                panic!("{outgoing:?}")
            };
            fired.push(tick);
        }
    }
    assert_eq!(fired, [2, 4, 6]);
}

/// RULE-114, RULE-50 (REQ-6528): a command the server sends directly is
/// applied by the open page's evaluator at its next tick; for a page that
/// is not the open one, or has no table, it is dropped and counted, and
/// the count is reported as fault 5.
#[test]
fn a_command_the_server_sends_is_applied_to_the_open_page() {
    let command = |on: Stamp, command: Command| {
        let body = body_of(&command, on);
        let (page, subcode) = place(on);
        (command, page, subcode, body)
    };
    let nine = Command::Variable { operation: VariableOperation::Set, variable: 3, operand: Operand::Constant(9) };
    let report = Command::Report { value: Some(Value::variable(3)) };
    let mut rules = opened();
    for given in [nine, report] {
        let (given, page, subcode, body) = command(ON, given);
        let record = CommandRecord { kind: given.kind(), page, subcode, command: given, body: &body };
        let record = if given.kind() == neoteletext_rules::command::TYPE_VARIABLE {
            ChannelRecord::Variable(record)
        } else {
            ChannelRecord::Report(record)
        };
        rules.record(&record, A_TICK_US);
    }
    let out = ticked(&mut rules, A_TICK_US);
    // A run no rule made: rule 255.
    let [Outgoing::Rule { rule: NO_RULE, row: NO_RULE, tick: 1, body: Body::Value { reading: 9, .. }, .. }] =
        out.as_slice()
    else {
        panic!("{out:?}")
    };
    // For another sub-code of the page number: dropped and counted.
    let elsewhere = Stamp { page: ON.page, sub: 2 };
    let (given, page, subcode, body) = command(elsewhere, nine);
    let record = CommandRecord { kind: given.kind(), page, subcode, command: given, body: &body };
    rules.record(&ChannelRecord::Variable(record), 2 * A_TICK_US);
    let fault = RulesFault { on: ON, generation: 1, fault: FaultCode::CommandsSkipped, count: 1 };
    assert_eq!(ticked(&mut rules, 2 * A_TICK_US), [Outgoing::Fault(fault)]);
}
