//! A page's rule table as `neottx view` and `neottx keyboard` hold it
//! (neoteletext-common `docs/page-rules.md`, DEC-6522, REQ-6523): under
//! push protocol 9 a viewer's input reaches a channel only inside a
//! `RULE_EVENT` of a rule that asked for it (RULE-46), so a receiver keeps
//! the table of the page on screen, evaluates it once a tick (RULE-4,
//! RULE-5) and sends what its `send` and `echo` report. A page with no
//! table sends nothing (RULE-50).
//!
//! Nothing here reads a clock or a socket: the caller says the time in
//! microseconds, gives the channel's records and the viewer's input, and
//! writes what [`PageRules::tick`] gives it through its input session.
//!
//! A terminal has no pointer, plays no sound and draws no object, so the
//! page the evaluator reads has no target and no sound: an action on one
//! is skipped and counted (RULE-20), and a table's key, text and focus
//! rules, its variables and its timers work as they do in the browser.

use neoteletext_core::consts::{PAGE_HEIGHT, PAGE_WIDTH};
use neoteletext_input::MAX_MESSAGE;
use neoteletext_input::event::{Action, Event, Item, KeyCode, Stamp, write};
use neoteletext_input::rules::{FaultCode, PageStateEvent, RuleBody, RuleEvent, RulesFault};
use neoteletext_rules::animate::Operation;
use neoteletext_rules::clock::Clock;
use neoteletext_rules::evaluator::{
    Counters, Input, InputEvent, KeyAction, KeyIdentity, Report, Reported, SoundOperation, SoundState, TargetState,
};
use neoteletext_rules::limits::{MAX_POINTER_OBJECTS, MAX_STATE_BYTES};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{Target, Trigger, Value};
use neoteletext_rules::{Evaluator, Table, World};
use neoteletext_web::records::ChannelRecord;
use neoteletext_web::records::rules::RulesRecord;
use std::collections::VecDeque;

/// The pages whose newest table is kept. A server sends a page's table
/// each time the connection selects the page, before the page (RULE-40),
/// so only the page on screen and the one being opened are ever needed;
/// the rest is room for tables that arrive while another page is drawn.
const MAX_HELD_TABLES: usize = 4;
/// The input events kept for a report to name (RULE-52 item 2).
const INPUT_RING: usize = 64;
/// The least time between two `RULES_FAULT`s of one code for a page
/// (RULE-48), and between two state queries answered (RULE-47): a second.
const ONCE_A_SECOND_US: u64 = 1_000_000;
/// The codes a `RULES_FAULT` has (RULE-48): 1 to 8.
const FAULT_CODES: usize = 8;

/// What a `RULE_EVENT` to send reports (RULE-46).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Body {
    /// The rule fired, with no event.
    Fired,
    /// The viewer's input that fired the rule, as the input stream's bytes.
    Input(Vec<u8>),
    /// One value as the snapshot held it.
    Value {
        /// The value.
        value: Value,
        /// Its reading.
        reading: i32,
    },
}

/// One message for the page's channel, to send through the input session.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Outgoing {
    /// A rule's `send` or `echo` (RULE-46).
    Rule {
        /// The page on screen.
        on: Stamp,
        /// The table's generation.
        generation: u16,
        /// The rule.
        rule: u8,
        /// The page tick at which it fired.
        tick: u32,
        /// The target the pointer was inside, or [`Target::NONE`].
        target: Target,
        /// What is reported.
        body: Body,
    },
    /// The answer to a state query (RULE-47): the bytes after its stamp.
    State {
        /// The page on screen.
        on: Stamp,
        /// The answer.
        body: Vec<u8>,
    },
    /// A fault of the page's table (RULE-48).
    Fault(RulesFault),
}

impl Outgoing {
    /// Gives `send` the message as an item of the input stream.
    pub(crate) fn with_item<R>(&self, send: impl FnOnce(&Item<'_>) -> R) -> R {
        match self {
            Self::Rule { on, generation, rule, tick, target, body } => {
                let body = match body {
                    Body::Fired => RuleBody::Fired,
                    Body::Input(bytes) => RuleBody::Input(bytes),
                    Body::Value { value, reading } => RuleBody::Value { value: *value, reading: *reading },
                };
                let event =
                    RuleEvent { on: *on, generation: *generation, rule: *rule, tick: *tick, target: *target, body };
                send(&Item::Event(Event::RuleEvent(event)))
            }
            Self::State { on, body } => send(&Item::Event(Event::PageState(PageStateEvent { on: *on, body }))),
            Self::Fault(fault) => send(&Item::Event(Event::RulesFault(*fault))),
        }
    }
}

/// The newest RULES record of a page.
struct Held {
    page: u16,
    generation: u16,
    /// The table, or `None` when the record said the page has none, or
    /// could not be decoded.
    table: Option<Box<Table>>,
    /// The record was refused whole, and fault 1 has not yet been
    /// reported (RULE-48, RULE-70).
    refused: bool,
}

/// The table being evaluated: that of the page on screen.
struct Open {
    generation: u16,
    // A decoded table's evaluator is tens of kilobytes.
    evaluator: Box<Evaluator>,
    clock: Clock,
    /// When the page was opened, which its clock counts from (RULE-4).
    opened_us: u64,
    /// The table has a rule that time alone can fire, so every tick is
    /// evaluated when it is due and not only those with input.
    timed: bool,
    /// Input or a query waits for the next tick.
    pending: bool,
    /// The counts already turned into faults.
    reported: Counters,
}

/// How many times a fault happened since it was last reported, and when
/// that was.
#[derive(Clone, Copy, Default)]
struct Tally {
    count: u16,
    sent_us: Option<u64>,
}

/// What the evaluator reads of a terminal's page, and where its reports
/// go: no target, no sound, no animation.
#[derive(Default)]
struct Terminal {
    reports: Vec<Report>,
    state: Option<PageState>,
}

impl World for Terminal {
    fn picture(&self) -> (u16, u16) {
        (u16::try_from(PAGE_WIDTH).unwrap_or(u16::MAX), u16::try_from(PAGE_HEIGHT).unwrap_or(u16::MAX))
    }

    fn target(&self, _target: Target) -> Option<TargetState> {
        None
    }

    fn objects_at(&self, _x: u16, _y: u16, _objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize {
        0
    }

    fn sound(&self, _channel: u8) -> SoundState {
        SoundState::default()
    }

    fn synth(&self, _channel: u8, _part: u8, _class: u8, _index: u16) -> i32 {
        0
    }

    fn sound_operation(&mut self, _operation: SoundOperation) -> bool {
        false
    }

    fn animate(&mut self, _operation: Operation, _tick: u32) -> bool {
        false
    }

    fn report(&mut self, report: Report) {
        self.reports.push(report);
    }

    fn advance_cycles(&mut self, _tick: u32) {}

    fn page_state(&mut self, state: &PageState) {
        self.state = Some(*state);
    }
}

/// The viewer's input as the evaluator takes it, or `None` for what a
/// terminal never makes and the evaluator is not given.
fn input_of(item: &Item<'_>) -> Option<Input> {
    let event = match item {
        Item::Text(_) => return Some(Input::Text),
        Item::Event(event) => event,
    };
    match event {
        Event::Key(key) => Some(Input::Key {
            action: match key.action {
                Action::Press => KeyAction::Press,
                Action::Repeat => KeyAction::Repeat,
                Action::Release => KeyAction::Release,
            },
            key: match key.key {
                KeyCode::Character(character) => KeyIdentity::Character(u32::from(character)),
                KeyCode::Functional(code) => KeyIdentity::Named(code),
            },
            base: key.base.map(u32::from),
            mods: key.mods,
        }),
        Event::Paste { .. } | Event::Compose { .. } => Some(Input::Text),
        Event::FocusIn => Some(Input::Focus),
        Event::FocusOut => Some(Input::Blur),
        _ => None,
    }
}

/// The rule tables of a channel's pages, and the evaluator of the page on
/// screen.
#[derive(Default)]
pub(crate) struct PageRules {
    held: VecDeque<Held>,
    open: Option<Open>,
    /// The page on screen.
    on: Option<Stamp>,
    /// The last input event's number.
    sequence: u32,
    /// The last [`INPUT_RING`] input events given to the evaluator, by
    /// number, as the input stream's bytes.
    ring: VecDeque<(u32, Vec<u8>)>,
    /// Messages made outside a tick, which the next takes first.
    out: Vec<Outgoing>,
    /// A state query of a page with no table, answered at the next tick.
    query_without_table: Option<u32>,
    /// When the last state query was taken.
    queried_us: Option<u64>,
    faults: [Tally; FAULT_CODES],
    world: Terminal,
}

impl PageRules {
    /// No table and no page.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the page on screen has a table being evaluated.
    #[cfg(test)]
    pub(crate) const fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Takes one of the channel's records at `now_us`: a page's table, or
    /// a query of its state. Every other record is another reader's.
    pub(crate) fn record(&mut self, record: &ChannelRecord<'_>, now_us: u64) {
        match record {
            ChannelRecord::Rules(rules) => self.table(rules, now_us),
            ChannelRecord::RulesQuery(query) => {
                // RULE-47: a query of a page that is not open is ignored.
                if self.on.map(|on| on.page) != Some(query.page.mpp()) {
                    return;
                }
                let early = self.queried_us.is_some_and(|last| now_us.saturating_sub(last) < ONCE_A_SECOND_US);
                if early {
                    self.count(FaultCode::QueryDropped, 1);
                    return;
                }
                self.queried_us = Some(now_us);
                match self.open.as_mut() {
                    Some(open) => {
                        open.evaluator.query(query.identifier);
                        open.pending = true;
                    }
                    None => self.query_without_table = Some(query.identifier),
                }
            }
            _ => {}
        }
    }

    /// Takes a page's RULES record (RULE-41): the table replaces the one
    /// held for the page when its generation differs, and the page on
    /// screen is opened afresh with it (RULE-2).
    fn table(&mut self, record: &RulesRecord<'_>, now_us: u64) {
        let page = record.page.mpp();
        if self.held.iter().any(|held| held.page == page && held.generation == record.generation) {
            return;
        }
        let mut table = Box::new(Table::new());
        // RULE-70: a table the decoder refuses is refused whole, and the
        // page then has no table.
        let (table, refused) = match record.decode(&mut table) {
            Ok(header) => (header.has_table.then_some(table), false),
            Err(_) => (None, true),
        };
        self.held.retain(|held| held.page != page);
        if self.held.len() >= MAX_HELD_TABLES {
            // The oldest that is not the page on screen's.
            let on = self.on.map(|on| on.page);
            if let Some(oldest) = self.held.iter().position(|held| Some(held.page) != on) {
                self.held.remove(oldest);
            }
        }
        self.held.push_back(Held { page, generation: record.generation, table, refused });
        if self.on.map(|on| on.page) == Some(page) {
            // The table it replaces ends without its `close` rules: the
            // page is still shown.
            self.open = None;
            self.open_page(now_us);
        }
    }

    /// The page on screen is `on` at `now_us`: another page closes the one
    /// that was open (RULE-5) and opens this one's table at page tick 0
    /// (RULE-4); another sub-page of the same page switches its list of
    /// rules (RULE-17). True when a table was opened afresh.
    pub(crate) fn show(&mut self, on: Option<Stamp>, now_us: u64) -> bool {
        if self.on.map(|on| on.page) == on.map(|on| on.page) {
            if let (Some(open), Some(on)) = (self.open.as_mut(), on) {
                open.evaluator.set_subpage(on.sub);
            }
            self.on = on;
            return false;
        }
        self.close(now_us);
        self.on = on;
        self.query_without_table = None;
        self.queried_us = None;
        self.open_page(now_us)
    }

    /// Opens the table held for the page on screen, if there is one, and
    /// reports a table that was refused.
    fn open_page(&mut self, now_us: u64) -> bool {
        let Some(on) = self.on else { return false };
        let Some(held) = self.held.iter_mut().find(|held| held.page == on.page) else { return false };
        let generation = held.generation;
        if core::mem::take(&mut held.refused) {
            self.out.push(Outgoing::Fault(RulesFault { on, generation, fault: FaultCode::Undecodable, count: 1 }));
            return false;
        }
        let Some(table) = held.table.as_deref() else { return false };
        // A decoded table is never refused here.
        let Ok(evaluator) = Evaluator::new(table, generation, on.sub) else { return false };
        let timed = table
            .rules
            .as_slice()
            .iter()
            .any(|rule| matches!(rule.trigger, Trigger::Becomes | Trigger::Every { .. } | Trigger::After { .. }));
        self.ring.clear();
        self.open = Some(Open {
            generation,
            evaluator: Box::new(evaluator),
            clock: Clock::new(),
            opened_us: now_us,
            timed,
            // Page tick 0 runs the table's `open` rules.
            pending: true,
            reported: Counters::default(),
        });
        true
    }

    /// Closes the open page (RULE-5): input still waiting is evaluated in
    /// one tick, so that the releases of what was held fire their rules
    /// (RULE-8), then the final tick runs the `close` rules.
    fn close(&mut self, now_us: u64) {
        let Some(mut open) = self.open.take() else { return };
        let Some(on) = self.on else { return };
        if open.pending {
            open.evaluator.tick(&mut self.world);
            self.collect(on, open.generation);
        }
        open.evaluator.close(&mut self.world);
        self.collect(on, open.generation);
        self.count_from(&mut open);
        self.flush_faults(on, open.generation, now_us);
        self.faults = [Tally::default(); FAULT_CODES];
    }

    /// Gives the evaluator one thing the viewer did, already as it goes
    /// under the session's capabilities. With no table open it is dropped:
    /// such a page sends no input (RULE-50).
    pub(crate) fn input(&mut self, item: &Item<'_>) {
        let Some(open) = self.open.as_mut() else { return };
        let Some(input) = input_of(item) else { return };
        let mut bytes = [0_u8; MAX_MESSAGE];
        let Ok(length) = write(item, &mut bytes) else { return };
        let Some(bytes) = bytes.get(..length) else { return };
        self.sequence = self.sequence.wrapping_add(1);
        if self.ring.len() >= INPUT_RING {
            self.ring.pop_front();
        }
        self.ring.push_back((self.sequence, bytes.to_vec()));
        // A full queue drops the event and counts it (RULE-5).
        let _ = open.evaluator.push(InputEvent { sequence: self.sequence, input });
        open.pending = true;
    }

    /// Evaluates each tick due at `now_us` (RULE-4) and gives `out` what
    /// is to be sent, in order.
    pub(crate) fn tick(&mut self, now_us: u64, out: &mut Vec<Outgoing>) {
        out.append(&mut self.out);
        let Some(on) = self.on else { return };
        if let Some(mut open) = self.open.take() {
            let due = open.clock.wake(now_us.saturating_sub(open.opened_us));
            for _ in 0..due {
                open.evaluator.tick(&mut self.world);
                self.collect(on, open.generation);
                open.pending = false;
            }
            self.count_from(&mut open);
            self.flush_faults(on, open.generation, now_us);
            self.open = Some(open);
        } else {
            if let Some(identifier) = self.query_without_table.take() {
                // RULE-47: generation 0 and tick 0 say the page has no table.
                self.state(on, &PageState { identifier, table: None });
            }
            let generation = self.held.iter().find(|held| held.page == on.page).map_or(0, |held| held.generation);
            self.flush_faults(on, generation, now_us);
        }
        out.append(&mut self.out);
    }

    /// The microseconds from `now_us` until [`PageRules::tick`] has work:
    /// `None` when it has none until something arrives.
    pub(crate) fn wait(&self, now_us: u64) -> Option<u64> {
        if !self.out.is_empty() || self.query_without_table.is_some() {
            return Some(0);
        }
        let open = self.open.as_ref()?;
        if !(open.pending || open.timed) {
            return None;
        }
        Some(open.opened_us.saturating_add(open.clock.next_due()).saturating_sub(now_us))
    }

    /// Turns what the evaluator reported in its last tick into messages.
    fn collect(&mut self, on: Stamp, generation: u16) {
        let reports = core::mem::take(&mut self.world.reports);
        for report in &reports {
            let body = match report.what {
                Reported::Fired => Body::Fired,
                Reported::Value { value, reading } => Body::Value { value, reading },
                // RULE-52 item 3: an event no longer kept is not reported.
                Reported::Input { sequence } => match self.ring.iter().find(|(number, _)| *number == sequence) {
                    Some((_, bytes)) => Body::Input(bytes.clone()),
                    None => continue,
                },
            };
            self.out.push(Outgoing::Rule {
                on,
                generation,
                rule: report.rule,
                tick: report.tick,
                target: report.target,
                body,
            });
        }
        if let Some(state) = self.world.state.take() {
            self.state(on, &state);
        }
    }

    /// Queues a `PAGE_STATE`.
    fn state(&mut self, on: Stamp, state: &PageState) {
        let mut bytes = [0_u8; MAX_STATE_BYTES];
        if let Ok(length) = neoteletext_rules::state::encode(state, &mut bytes)
            && let Some(body) = bytes.get(..length)
        {
            self.out.push(Outgoing::State { on, body: body.to_vec() });
        }
    }

    fn count(&mut self, fault: FaultCode, times: u32) {
        let place = usize::from(fault.code()).saturating_sub(1);
        if let Some(tally) = self.faults.get_mut(place) {
            tally.count = tally.count.saturating_add(u16::try_from(times).unwrap_or(u16::MAX));
        }
    }

    /// Counts what the evaluator's counters gained since they were last
    /// read as the faults of RULE-48.
    fn count_from(&mut self, open: &mut Open) {
        let now = *open.evaluator.counters();
        let before = core::mem::replace(&mut open.reported, now);
        self.count(FaultCode::TickOverrun, now.overruns.saturating_sub(before.overruns));
        self.count(FaultCode::EventsDropped, now.events_dropped.saturating_sub(before.events_dropped));
        self.count(FaultCode::ActionsSkipped, now.skipped.saturating_sub(before.skipped));
    }

    /// Reports each fault counted, at most once a second for each code
    /// (RULE-48).
    fn flush_faults(&mut self, on: Stamp, generation: u16, now_us: u64) {
        for (tally, code) in self.faults.iter_mut().zip(1_u8..) {
            let due = tally.sent_us.is_none_or(|sent| now_us.saturating_sub(sent) >= ONCE_A_SECOND_US);
            if tally.count == 0 || !due {
                continue;
            }
            let Some(fault) = FaultCode::from_code(code) else { continue };
            self.out.push(Outgoing::Fault(RulesFault { on, generation, fault, count: tally.count }));
            *tally = Tally { count: 0, sent_us: Some(now_us) };
        }
    }
}

#[cfg(test)]
mod tests;
