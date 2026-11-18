//! A page's rule table as `neottx view` and `neottx keyboard` hold it
//! (neoteletext-common `docs/page-rules.md`, DEC-6522, DEC-6587,
//! REQ-6523): under push protocol 10 a viewer's input reaches a channel
//! only inside a `RULE_EVENT` of a rule whose group reported it (RULE-46),
//! so a receiver keeps the table of the page on screen, evaluates it once
//! a tick (RULE-4, RULE-5) and sends what its groups' `REPORT` commands
//! report. A page with no table sends nothing (RULE-50). A page is a page
//! number and a sub-code together (RULE-112): another sub-code is another
//! page, with a table and sixteen variables of its own.
//!
//! Nothing here reads a clock or a socket: the caller says the time in
//! microseconds, gives the channel's records and the viewer's input, and
//! writes what [`PageRules::tick`] gives it through its input session.
//!
//! A terminal has no pointer, plays no sound and draws no object, so the
//! page the evaluator reads has no target and no sound: a command for one
//! does nothing and is counted (RULE-118), and a table's key, text and
//! focus rules, its variables, its timers, its sequences and its armed
//! runs work as they do in the browser.

use neoteletext_core::consts::{PAGE_HEIGHT, PAGE_WIDTH};
use neoteletext_input::MAX_MESSAGE;
use neoteletext_input::event::{Action, Event, Item, KeyCode, Stamp, write};
use neoteletext_input::rules::{FaultCode, PageStateEvent, RuleBody, RuleEvent, RulesFault};
use neoteletext_rules::clock::Clock;
use neoteletext_rules::command::{TYPE_RUN, TYPE_SEQUENCE, TYPE_TIMER};
use neoteletext_rules::evaluator::{
    Counters, Input, InputEvent, KeyAction, KeyIdentity, Report, Reported, SoundState, TargetState,
};
use neoteletext_rules::limits::{MAX_POINTER_OBJECTS, MAX_STATE_BYTES};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{Target, Value};
use neoteletext_rules::{Evaluator, Table, World};
use neoteletext_web::records::commands::CommandRecord;
use neoteletext_web::records::rules::{Assembled, Assembler, RulesRecord};
use neoteletext_web::records::{ChannelRecord, body_check};
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
/// The codes a `RULES_FAULT` has (RULE-48): 1 to 10, of which 2 is
/// withdrawn and never sent.
const FAULT_CODES: usize = 10;

/// What a `RULE_EVENT` to send reports (RULE-46).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Body {
    /// The rule fired, with no event.
    Fired,
    /// The viewer's input that fired the rule, as the input stream's bytes.
    Input(Vec<u8>),
    /// One value: a variable as the working state held it, any other as
    /// the snapshot did.
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
    /// A `REPORT` a run applied (RULE-22, RULE-46).
    Rule {
        /// The page on screen.
        on: Stamp,
        /// The table's generation.
        generation: u16,
        /// The rule, or 255 when the run was not made by a rule.
        rule: u8,
        /// The place of the firing row, or 255 when none.
        row: u8,
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
            Self::Rule { on, generation, rule, row, tick, target, body } => {
                let body = match body {
                    Body::Fired => RuleBody::Fired,
                    Body::Input(bytes) => RuleBody::Input(bytes),
                    Body::Value { value, reading } => RuleBody::Value { value: *value, reading: *reading },
                };
                let event = RuleEvent {
                    on: *on,
                    generation: *generation,
                    rule: *rule,
                    row: *row,
                    tick: *tick,
                    target: *target,
                    body,
                };
                send(&Item::Event(Event::RuleEvent(event)))
            }
            Self::State { on, body } => send(&Item::Event(Event::PageState(PageStateEvent { on: *on, body }))),
            Self::Fault(fault) => send(&Item::Event(Event::RulesFault(*fault))),
        }
    }
}

/// The newest table of a page.
struct Held {
    /// The page: its number and its sub-code (RULE-112).
    page: (u16, u16),
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
    /// The table has something that time alone can make happen, so every
    /// tick is evaluated when it is due and not only those with input.
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

    fn command(&mut self, _kind: u8, _body: &[u8], _tick: u32) -> bool {
        // No sound, no object: a command for one does nothing (RULE-118).
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

/// Whether time alone can make something of `table` happen: a rule with
/// a trigger no input makes, a sequence, or a command that starts a timer
/// or a sequence or arms a run. A table without any is evaluated only at
/// the ticks that have input (RULE-58: no wake without work).
fn is_timed(table: &Table) -> bool {
    let triggered = table.rules.iter().any(|rule| rule.triggers.iter().any(|trigger| !trigger.is_input()));
    let commanded = (0..table.groups.len()).any(|group| {
        table.groups.commands(group).any(|command| matches!(command.kind, TYPE_TIMER | TYPE_RUN | TYPE_SEQUENCE))
    });
    triggered || commanded || !table.sequences.is_empty()
}

/// The rule tables of a channel's pages, and the evaluator of the page on
/// screen.
pub(crate) struct PageRules {
    held: VecDeque<Held>,
    /// The image in flight of a table that travels in several records
    /// (RULE-41).
    assembler: Box<Assembler>,
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

impl Default for PageRules {
    fn default() -> Self {
        Self {
            held: VecDeque::new(),
            assembler: Box::new(Assembler::new()),
            open: None,
            on: None,
            sequence: 0,
            ring: VecDeque::new(),
            out: Vec::new(),
            query_without_table: None,
            queried_us: None,
            faults: [Tally::default(); FAULT_CODES],
            world: Terminal::default(),
        }
    }
}

/// The page a stamp names: its number and its sub-code together
/// (RULE-112).
const fn page_of(on: Stamp) -> (u16, u16) {
    (on.page, on.sub)
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

    /// Takes one of the channel's records at `now_us`: a fragment of a
    /// page's table, a query of its state, or a command the server sent
    /// directly. Every other record is another reader's.
    pub(crate) fn record(&mut self, record: &ChannelRecord<'_>, now_us: u64) {
        let on = self.on.map(page_of);
        match record {
            ChannelRecord::Rules(rules) => self.fragment(rules, now_us),
            ChannelRecord::RulesQuery(query) => {
                // RULE-47: a query of a page that is not open is ignored.
                if on != Some((query.page.mpp(), query.subcode.value())) {
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
            ChannelRecord::Variable(command)
            | ChannelRecord::Timer(command)
            | ChannelRecord::Report(command)
            | ChannelRecord::Run(command)
            | ChannelRecord::Sequence(command) => self.command(command),
            // A terminal plays no local sound: the message does nothing,
            // and is counted (RULE-21).
            ChannelRecord::LocalMidi(command) if on == Some((command.page.mpp(), command.subcode.value())) => {
                self.count(FaultCode::CommandsSkipped, 1);
            }
            _ => {}
        }
    }

    /// Takes a command the server sent directly (RULE-114): the open
    /// page's evaluator applies it at step 5 of its next tick. For a page
    /// with no table, or one that is not the open page, it is dropped and
    /// counted (RULE-50).
    fn command(&mut self, command: &CommandRecord<'_>) {
        let ours = self.on.map(page_of) == Some((command.page.mpp(), command.subcode.value()));
        match self.open.as_mut().filter(|_| ours) {
            Some(open) => {
                // What the evaluator refuses it counts itself.
                let _ = open.evaluator.push_command(command.kind, command.body);
                open.pending = true;
            }
            None => self.count(FaultCode::CommandsSkipped, 1),
        }
    }

    /// Takes one RULES record (RULE-41): a fragment of a page's table
    /// image, or the word that the page has no table. A whole image is
    /// decoded (RULE-70) and replaces the table held for the page, and the
    /// page on screen is opened afresh with it (RULE-2).
    fn fragment(&mut self, record: &RulesRecord<'_>, now_us: u64) {
        let (page, generation, table, refused) = match self.assembler.push(record) {
            Assembled::Incomplete => return,
            Assembled::Removed { page, subcode, generation } => {
                ((page.mpp(), subcode.value()), generation, None, false)
            }
            // RULE-41, RULE-48: the incomplete image is discarded and the
            // page keeps the table it had; the fault is reported.
            Assembled::OutOfPlace { page, subcode, generation } => {
                if let Some(on) = self.on.filter(|on| page_of(*on) == (page.mpp(), subcode.value())) {
                    let fault = RulesFault { on, generation, fault: FaultCode::Undecodable, count: 1 };
                    self.out.push(Outgoing::Fault(fault));
                }
                return;
            }
            Assembled::Whole { page, subcode, generation, image } => {
                let page = (page.mpp(), subcode.value());
                let mut table = Box::new(Table::new(page.0, page.1));
                // RULE-70: a table the decoder refuses is refused whole,
                // and the page then has no table.
                match neoteletext_rules::decode(image, page.0, page.1, body_check, &mut table) {
                    Ok(()) => (page, generation, Some(table), false),
                    Err(_) => (page, generation, None, true),
                }
            }
        };
        self.held.retain(|held| held.page != page);
        if self.held.len() >= MAX_HELD_TABLES {
            // The oldest that is not the page on screen's.
            let on = self.on.map(page_of);
            if let Some(oldest) = self.held.iter().position(|held| Some(held.page) != on) {
                self.held.remove(oldest);
            }
        }
        self.held.push_back(Held { page, generation, table, refused });
        if self.on.map(page_of) == Some(page) {
            // The table it replaces ends without its `close` rules: the
            // page is still shown. Its variables, timers, sequences and
            // armed runs go with it (RULE-2).
            self.open = None;
            self.open_page(now_us);
        }
    }

    /// The page on screen is `on` at `now_us`: another page, which is
    /// another page number or another sub-code (RULE-112), closes the one
    /// that was open (RULE-5, RULE-111) and opens this one's table at page
    /// tick 0 (RULE-4) with every variable 0. True when a table was opened
    /// afresh.
    pub(crate) fn show(&mut self, on: Option<Stamp>, now_us: u64) -> bool {
        if self.on.map(page_of) == on.map(page_of) {
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
        let Some(held) = self.held.iter_mut().find(|held| held.page == page_of(on)) else { return false };
        let generation = held.generation;
        if core::mem::take(&mut held.refused) {
            self.out.push(Outgoing::Fault(RulesFault { on, generation, fault: FaultCode::Undecodable, count: 1 }));
            return false;
        }
        let Some(table) = held.table.as_deref() else { return false };
        // A decoded table is never refused here.
        let Ok(evaluator) = Evaluator::new(table, generation, body_check) else { return false };
        let timed = is_timed(table);
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
                // RULE-5 step 2: no input belongs to page tick 0, so what
                // waits then still waits after it.
                let first = open.evaluator.page_tick() == 0;
                open.evaluator.tick(&mut self.world);
                self.collect(on, open.generation);
                open.pending = first;
            }
            self.count_from(&mut open);
            self.flush_faults(on, open.generation, now_us);
            self.open = Some(open);
        } else {
            if let Some(identifier) = self.query_without_table.take() {
                // RULE-47: generation 0 and tick 0 say the page has no table.
                self.state(on, &PageState { identifier, table: None });
            }
            let generation = self.held.iter().find(|held| held.page == page_of(on)).map_or(0, |held| held.generation);
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
                row: report.row,
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
        self.count(FaultCode::EventsDropped, now.events_dropped.saturating_sub(before.events_dropped));
        self.count(FaultCode::CommandsSkipped, now.skipped.saturating_sub(before.skipped));
        // RULE-48, code 10: an armed run not armed, and a command the
        // server sent or a cue dropped at a full queue.
        let full = |counters: &Counters| {
            counters.not_armed.saturating_add(counters.commands_dropped).saturating_add(counters.cues_dropped)
        };
        self.count(FaultCode::QueueFull, full(&now).saturating_sub(full(&before)));
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
