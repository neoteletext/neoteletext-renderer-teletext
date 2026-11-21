//! The page rule table's messages from the receiver to the channel's
//! server (neoteletext-common `docs/page-rules.md`, DEC-6522, DEC-6587; RULE-46 to
//! RULE-48): `RULE_EVENT`, `PAGE_STATE` and `RULES_FAULT`, carried as
//! extension events of the input stream ([`crate::event::RULE_EVENT`],
//! [`crate::event::PAGE_STATE`], [`crate::event::RULES_FAULT`]).
//!
//! Since push protocol 10 a `RULE_EVENT` is the only way a viewer's input
//! reaches a channel (REQ-6523): KEY, MOUSE, WHEEL, PASTE, COMPOSE, TOUCH,
//! `FOCUS_IN`, `FOCUS_OUT`, HOVER, PICK, `DRAWING_HOVER` and raw text are
//! carried inside one, exactly as push protocol 8 encoded them, and are a
//! protocol error at the top level of the stream
//! ([`crate::event::is_page_input`]).
//!
//! Each payload begins with the stamp, as every stamped event does
//! (DOC-1934); the fields after it are big-endian and of fixed length. A
//! target and a value are the rule table's own, read by
//! [`neoteletext_rules`], so the receiver, the server and the table agree
//! by construction.

use crate::event::{Event, InputError, Item, Stamp};
use crate::wire::WireError;
use neoteletext_rules::limits::{MAX_CONTROL, MAX_RECTS, MAX_ROWS, MAX_RULES, MAX_SEQUENCES, MAX_STATE_BYTES, NO_RULE};
use neoteletext_rules::state::PageState;
use neoteletext_rules::table::{TARGET_BYTES, Target, TargetKind, VALUE_BYTES, Value};

/// The bytes of a `RULE_EVENT` between its stamp and its body (RULE-46):
/// the generation, the rule, the row, the tick, the kind and the target.
pub const RULE_EVENT_FIXED: usize = 2 + 1 + 1 + 4 + 1 + TARGET_BYTES;
/// The bytes of a kind 2 body: a value and its reading.
pub const VALUE_BODY_BYTES: usize = VALUE_BYTES + 4;
/// The bytes of a `RULES_FAULT` after its stamp (RULE-48): the generation,
/// the code and the count.
pub const RULES_FAULT_FIXED: usize = 2 + 1 + 2;

/// `RULE_EVENT`'s kind: the rule fired, with no event.
const KIND_FIRED: u8 = 0;
/// `RULE_EVENT`'s kind: the viewer's input event that fired it.
const KIND_INPUT: u8 = 1;
/// `RULE_EVENT`'s kind: a value and its reading.
const KIND_VALUE: u8 = 2;

/// What a `RULE_EVENT` reports (RULE-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleBody<'a> {
    /// Kind 0: the rule fired, with no event (`open`, `close`, `becomes`,
    /// `every`, `after`).
    Fired,
    /// Kind 1: the input that fired the rule, as bytes of the input
    /// stream: one event from its `0xFF` (KEY, MOUSE, WHEEL, TOUCH, PASTE,
    /// COMPOSE, `FOCUS_IN`, `FOCUS_OUT`, HOVER or `DRAWING_HOVER`), with
    /// its PICK after a MOUSE, TOUCH or WHEEL on a page the ReGIS renderer
    /// draws; or one run of raw text. [`RuleEvent::input`] reads them.
    Input(&'a [u8]),
    /// Kind 2: one value as the snapshot held it.
    Value {
        /// The value.
        value: Value,
        /// Its reading.
        reading: i32,
    },
}

/// A `RULE_EVENT` (RULE-46): a rule's `send` or `echo`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RuleEvent<'a> {
    /// The page on screen.
    pub on: Stamp,
    /// The table's generation, 1 to 65,535.
    pub generation: u16,
    /// The rule, 0 to 63; 255 when the run was not made by a rule.
    pub rule: u8,
    /// The place of the firing row within the rule's rows, 0 to 63; 255
    /// when none.
    pub row: u8,
    /// The page tick at which the rule fired.
    pub tick: u32,
    /// The control, object or rectangle the pointer was inside for a rule
    /// whose pointer trigger named one; else [`Target::NONE`].
    pub target: Target,
    /// What is reported.
    pub body: RuleBody<'a>,
}

impl<'a> RuleEvent<'a> {
    /// The input a kind 1 body carries: its event or its run of text, and
    /// the PICK that follows a pointer event on a page the ReGIS renderer
    /// draws. `None` for the other kinds.
    #[must_use]
    pub fn input(&self) -> Option<(Item<'a>, Option<crate::event::Pick>)> {
        let RuleBody::Input(bytes) = self.body else { return None };
        read_input(bytes).ok()
    }
}

/// Whether `event` may be carried inside a `RULE_EVENT` as its first
/// event (RULE-46).
const fn is_carried(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Key(_)
            | Event::Mouse(_)
            | Event::Wheel(_)
            | Event::Touch(_)
            | Event::Paste { .. }
            | Event::Compose { .. }
            | Event::FocusIn
            | Event::FocusOut
            | Event::Hover { .. }
            | Event::DrawingHover { .. }
    )
}

/// Reads a kind 1 body: one event and at most its PICK, or one run of
/// text, and nothing after.
fn read_input(bytes: &[u8]) -> Result<(Item<'_>, Option<crate::event::Pick>), InputError> {
    let refused = InputError::BadValue("rule event input");
    // The body is whole: what it cuts short is its fault, never a wait for
    // more of the stream.
    let whole = |error| match error {
        InputError::Wire(WireError::Incomplete) => refused,
        other => other,
    };
    let (first, used) = crate::event::read(bytes).map_err(whole)?.ok_or(refused)?;
    let rest = bytes.get(used..).unwrap_or(&[]);
    let carried = match &first {
        Item::Text(_) => true,
        Item::Event(event) => is_carried(event),
    };
    if !carried {
        return Err(refused);
    }
    if rest.is_empty() {
        return Ok((first, None));
    }
    let pointed = matches!(first, Item::Event(Event::Mouse(_) | Event::Touch(_) | Event::Wheel(_)));
    match crate::event::read(rest).map_err(whole)? {
        Some((Item::Event(Event::Pick(pick)), used)) if pointed && used == rest.len() => Ok((first, Some(pick))),
        _ => Err(refused),
    }
}

/// A target as a `RULE_EVENT` may name it (RULE-46): all zero, or one
/// control, object or declared rectangle.
fn check_target(target: Target) -> bool {
    match target.kind {
        TargetKind::Page => target.number == 0,
        TargetKind::Control => (1..=MAX_CONTROL).contains(&target.number),
        TargetKind::Object => target.number != 0,
        TargetKind::Rect => usize::from(target.number) < MAX_RECTS,
    }
}

/// A big-endian `u16` at the front of `bytes`, and what follows it.
fn take_u16(bytes: &[u8]) -> Option<(u16, &[u8])> {
    let (head, rest) = bytes.split_first_chunk::<2>()?;
    Some((u16::from_be_bytes(*head), rest))
}

/// A big-endian `u32` at the front of `bytes`, and what follows it.
fn take_u32(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let (head, rest) = bytes.split_first_chunk::<4>()?;
    Some((u32::from_be_bytes(*head), rest))
}

/// Reads a `RULE_EVENT`'s payload after its stamp.
///
/// # Errors
/// [`InputError::BadValue`] for a payload cut short, a generation of 0, a
/// rule above 63, a kind not listed, a target that is not one thing or is
/// named where no pointer was, a value outside its space's ranges, or a
/// kind 1 body that is not exactly one carried event with at most its
/// PICK, or one run of text.
pub fn read_rule_event(on: Stamp, payload: &[u8]) -> Result<RuleEvent<'_>, InputError> {
    let refused = InputError::BadValue("rule event");
    let (generation, rest) = take_u16(payload).ok_or(refused)?;
    let (&[rule, row], rest) = rest.split_first_chunk::<2>().ok_or(refused)?;
    let (tick, rest) = take_u32(rest).ok_or(refused)?;
    let (&kind, rest) = rest.split_first().ok_or(refused)?;
    let (&[target_kind, high, low], body) = rest.split_first_chunk::<TARGET_BYTES>().ok_or(refused)?;
    let target =
        Target { kind: TargetKind::from_code(target_kind).ok_or(refused)?, number: u16::from_be_bytes([high, low]) };
    // RULE-46: 255 for a run no rule made, and for no row; a row is a
    // rule's.
    let ruled = usize::from(rule) < MAX_RULES || rule == NO_RULE;
    let rowed = row == NO_RULE || (usize::from(row) < MAX_ROWS && rule != NO_RULE);
    if generation == 0 || !ruled || !rowed || !check_target(target) {
        return Err(refused);
    }
    let body = match kind {
        KIND_FIRED if body.is_empty() => RuleBody::Fired,
        KIND_INPUT => {
            read_input(body)?;
            RuleBody::Input(body)
        }
        KIND_VALUE if body.len() == VALUE_BODY_BYTES => {
            let value = Value::from_bytes(body).map_err(|_| refused)?;
            // Whichever table it was, a value names a sequence below the
            // most a table has.
            value.check(MAX_SEQUENCES).map_err(|_| refused)?;
            let (reading, _) = take_u32(body.get(VALUE_BYTES..).unwrap_or(&[])).ok_or(refused)?;
            RuleBody::Value { value, reading: reading.cast_signed() }
        }
        _ => return Err(refused),
    };
    // Only an input event has a pointer that was inside something.
    if !matches!(body, RuleBody::Input(_)) && target != Target::NONE {
        return Err(refused);
    }
    Ok(RuleEvent { on, generation, rule, row, tick, target, body })
}

/// A `RULE_EVENT`'s fixed bytes after its stamp, and its kind 2 body when
/// it has one: what goes before a kind 1 body's bytes.
#[must_use]
pub fn rule_event_head(event: &RuleEvent<'_>) -> ([u8; RULE_EVENT_FIXED + VALUE_BODY_BYTES], usize) {
    let mut head = [0_u8; RULE_EVENT_FIXED + VALUE_BODY_BYTES];
    let kind = match event.body {
        RuleBody::Fired => KIND_FIRED,
        RuleBody::Input(_) => KIND_INPUT,
        RuleBody::Value { .. } => KIND_VALUE,
    };
    let fixed = event
        .generation
        .to_be_bytes()
        .into_iter()
        .chain([event.rule, event.row])
        .chain(event.tick.to_be_bytes())
        .chain([kind, event.target.kind.code()])
        .chain(event.target.number.to_be_bytes());
    let value = match event.body {
        RuleBody::Value { value, reading } => Some(value.to_bytes().into_iter().chain(reading.to_be_bytes())),
        _ => None,
    };
    let mut length = 0_usize;
    for (slot, byte) in head.iter_mut().zip(fixed.chain(value.into_iter().flatten())) {
        *slot = byte;
        length = length.saturating_add(1);
    }
    (head, length)
}

/// A `PAGE_STATE` (RULE-47): a page's local state, in answer to a state
/// query. It borrows the bytes after its stamp, which
/// [`neoteletext_rules::state`] reads and writes, so that an
/// [`Event`] stays small.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageStateEvent<'a> {
    /// The page on screen.
    pub on: Stamp,
    /// The answer after its stamp: at most `MAX_STATE_BYTES` with it.
    pub body: &'a [u8],
}

impl PageStateEvent<'_> {
    /// The answer, decoded.
    #[must_use]
    pub fn state(&self) -> Option<PageState> {
        neoteletext_rules::state::decode(self.body).ok()
    }
}

/// Reads a `PAGE_STATE`'s payload after its stamp, `stamp_bytes` long.
///
/// # Errors
/// [`InputError::BadValue`] for an answer over `MAX_STATE_BYTES` with its
/// stamp, or one [`neoteletext_rules::state::decode`] refuses.
pub fn read_page_state(on: Stamp, stamp_bytes: usize, body: &[u8]) -> Result<PageStateEvent<'_>, InputError> {
    let refused = InputError::BadValue("page state");
    if stamp_bytes.saturating_add(body.len()) > MAX_STATE_BYTES {
        return Err(refused);
    }
    neoteletext_rules::state::decode(body).map_err(|_| refused)?;
    Ok(PageStateEvent { on, body })
}

/// A fault of a page's table (RULE-48), its code on the wire. Code 2,
/// the first form's `tick_overrun`, is withdrawn: it is never sent and
/// never reused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaultCode {
    /// The table image could not be assembled or decoded and was refused
    /// whole: 1.
    Undecodable,
    /// Reports dropped by the trusted core's budgets (RULE-52): 3.
    ReportsDropped,
    /// Input events dropped from a full queue (RULE-5): 4.
    EventsDropped,
    /// Commands that did nothing for a missing target or sound
    /// (RULE-118): 5.
    CommandsSkipped,
    /// A state query dropped: 6.
    QueryDropped,
    /// A local sound stopped for overrunning its time (RULE-62): 7.
    LocalOverrun,
    /// Animation slowed by its budget (ANIM-16): 8.
    AnimationSlowed,
    /// The evaluation of the page's ticks took longer than the ticks' own
    /// time, so page time slowed (RULE-4): 9.
    EvaluationLate,
    /// An armed run not armed, a command the server sent, a cue or a
    /// sound command dropped at a full queue or budget: 10.
    QueueFull,
}

impl FaultCode {
    /// Every fault, in code order.
    pub const ALL: [Self; 9] = [
        Self::Undecodable,
        Self::ReportsDropped,
        Self::EventsDropped,
        Self::CommandsSkipped,
        Self::QueryDropped,
        Self::LocalOverrun,
        Self::AnimationSlowed,
        Self::EvaluationLate,
        Self::QueueFull,
    ];

    /// The fault's code on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Undecodable => 1,
            Self::ReportsDropped => 3,
            Self::EventsDropped => 4,
            Self::CommandsSkipped => 5,
            Self::QueryDropped => 6,
            Self::LocalOverrun => 7,
            Self::AnimationSlowed => 8,
            Self::EvaluationLate => 9,
            Self::QueueFull => 10,
        }
    }

    /// The fault of `code`, or `None` for a code no fault has.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|fault| fault.code() == code)
    }

    /// The fault's name in the program protocol (RULE-48).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Undecodable => "undecodable",
            Self::ReportsDropped => "reports_dropped",
            Self::EventsDropped => "events_dropped",
            Self::CommandsSkipped => "commands_skipped",
            Self::QueryDropped => "query_dropped",
            Self::LocalOverrun => "local_overrun",
            Self::AnimationSlowed => "animation_slowed",
            Self::EvaluationLate => "evaluation_late",
            Self::QueueFull => "queue_full",
        }
    }
}

/// A `RULES_FAULT` (RULE-48).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RulesFault {
    /// The page on screen.
    pub on: Stamp,
    /// The table's generation; that of the record refused for
    /// [`FaultCode::Undecodable`], or 0 when it could not be read.
    pub generation: u16,
    /// Which fault.
    pub fault: FaultCode,
    /// How many times since the last report.
    pub count: u16,
}

/// Reads a `RULES_FAULT`'s payload after its stamp: exactly
/// [`RULES_FAULT_FIXED`] bytes, a code listed.
///
/// # Errors
/// [`InputError::BadValue`] for a payload of another length or a code not
/// listed.
pub fn read_rules_fault(on: Stamp, payload: &[u8]) -> Result<RulesFault, InputError> {
    let refused = InputError::BadValue("rules fault");
    let &[generation_high, generation_low, code, count_high, count_low] = payload else { return Err(refused) };
    Ok(RulesFault {
        on,
        generation: u16::from_be_bytes([generation_high, generation_low]),
        fault: FaultCode::from_code(code).ok_or(refused)?,
        count: u16::from_be_bytes([count_high, count_low]),
    })
}

/// A `RULES_FAULT`'s bytes after its stamp.
#[must_use]
pub fn rules_fault_bytes(fault: &RulesFault) -> [u8; RULES_FAULT_FIXED] {
    let [generation_high, generation_low] = fault.generation.to_be_bytes();
    let [count_high, count_low] = fault.count.to_be_bytes();
    [generation_high, generation_low, fault.fault.code(), count_high, count_low]
}

#[cfg(test)]
mod tests;
