//! The viewer machine of REQ-FSM-005 as one pure transition function
//! (REQ-FSM-001): the machine's own state record, its inputs with every
//! fact a key needs already resolved, its actions, and the names the
//! transition trace gives them (REQ-FSM-010).
//!
//! [`Viewer`](super::Viewer) resolves a key's facts from its
//! [`KeyContext`](super::KeyContext) (the link, whether its prompt is
//! shown, whether its service is confirmed, the stored neighbour, the
//! history's next entry), calls [`viewer_transition`] and applies the
//! action, so the machine can be tested without a decoder or a store.

use crate::nav::{EntryResult, HistoryEntry, LinkTarget, PageEntry, SubPageEntry, SubPageEntryResult};
use crate::types::{Instant, PageAddress, SubCode};

use super::{ViewerEvent, ViewerState};

/// The viewer machine's own state record (REQ-FSM-001): the state, and the
/// digits being entered, which decide where the next digit leads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewerMachine {
    /// The state of the table.
    pub state: ViewerState,
    /// The page number's digits entered so far (REQ-NAV-001).
    pub entry: PageEntry,
    /// The sub-code's digits entered so far (REQ-NAV-005, DEC-2363).
    pub subpage_entry: SubPageEntry,
    /// The state sub-page entry began in and returns to: Displaying or
    /// Searching.
    pub resume: ViewerState,
}

impl ViewerMachine {
    /// The machine before the decoder starts.
    pub const START: Self = Self {
        state: ViewerState::Starting,
        entry: PageEntry::EMPTY,
        subpage_entry: SubPageEntry::EMPTY,
        resume: ViewerState::Displaying,
    };

    const fn with_state(self, state: ViewerState) -> Self {
        Self { state, ..self }
    }
}

/// How the Reveal key acts, from the configuration (REQ-NAV-007).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevealInput {
    /// Each press turns reveal on or off.
    Latched,
    /// Reveal is on until the given time.
    Momentary(Instant),
}

/// A coloured key and what its link is (REQ-NAV-002, REQ-NAV-012,
/// REQ-SEC-024).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColourInput {
    /// Red 0, green 1, yellow 2, cyan 3.
    pub key: u8,
    /// The page's link for the key, if any.
    pub target: Option<LinkTarget>,
    /// The key's prompt is shown whole in row 24 (REQ-SEC-024).
    pub prompt_shown: bool,
    /// The link leads to another service already confirmed this session.
    pub confirmed: bool,
}

/// One input of the viewer machine: a decoder event, a key with its facts
/// resolved, or the port's choice of a sub-page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewerInput {
    /// An event from the decoder.
    Event(ViewerEvent),
    /// A digit key 0 to 9.
    Digit(u8),
    /// The sub-page key (DEC-2363).
    SubPageKey,
    /// The Hold key.
    Hold,
    /// The Reveal key.
    Reveal(RevealInput),
    /// The Mix key.
    Mix,
    /// The page zoom key.
    Zoom,
    /// A coloured key.
    Colour(ColourInput),
    /// The Index key, with the page it leads to, resolved by the rules of
    /// REQ-NAV-003 and DEC-2198, or `None` when it does nothing.
    Index(Option<(PageAddress, SubCode)>),
    /// The Home key, with the initial page (REQ-NAV-008).
    Home(PageAddress),
    /// Next or previous page, with the page it leads to in the configured
    /// step order.
    StepPage(PageAddress),
    /// Next or previous sub-page, with the stored neighbour, if any.
    StepSubPage(Option<SubCode>),
    /// Back, with the history's entry in that direction, if any.
    Back(Option<HistoryEntry>),
    /// Forward, with the history's entry in that direction, if any.
    Forward(Option<HistoryEntry>),
    /// A sub-page chosen by the port (`show_subpage`).
    ChooseSubPage(SubCode),
    /// A page the channel's program sent the viewer to (`go_to`; DOC-1960,
    /// REQ-1502).
    Redirect(PageAddress, SubCode),
}

/// What a transition does, applied by [`Viewer`](super::Viewer).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewerAction {
    /// Nothing.
    None,
    /// Request the initial page (REQ-SVC-004).
    RequestInitial,
    /// Show the page found, the row 0 marker white (decision E.10).
    ShowPage,
    /// The port rolls the header and clock (decision E.11).
    RollHeader,
    /// Show "not recently received" (REQ-NAV-006).
    MarkStale,
    /// Clear "not recently received"; the port marks the changed rows
    /// (REQ-RND-019).
    ClearStale,
    /// Show the digit entered in the status area.
    ShowDigit,
    /// Refuse the digit, keeping those entered so far.
    RejectDigit,
    /// Request a page and record it in the history (REQ-NAV-010).
    Select(PageAddress, SubCode),
    /// Move in the history and request its entry (REQ-NAV-010).
    MoveInHistory {
        /// Forward, else back.
        forward: bool,
        /// The entry moved to.
        entry: HistoryEntry,
    },
    /// Request that sub-page of the page shown (REQ-NAV-005).
    ShowSubPage(SubCode),
    /// Show "P/----" in the status area (DEC-2363).
    BeginSubPageEntry,
    /// Stop applying updates (Hold).
    StopUpdates,
    /// Apply the updates received meanwhile.
    ApplyUpdates,
    /// Turn reveal on or off.
    ToggleReveal,
    /// Reveal until the given time.
    RevealUntil(Instant),
    /// Turn mix on or off.
    ToggleMix,
    /// Step page zoom: off, top half, bottom half.
    CycleZoom,
    /// Follow a link to another service already confirmed (REQ-NAV-012).
    FollowService(LinkTarget),
    /// Ask whether to follow a link to another service (REQ-NAV-012).
    ConfirmService(LinkTarget),
    /// The question is withdrawn and row 24 restored.
    RestoreRow24,
}

/// The transition function of the viewer machine (REQ-FSM-005), one arm
/// for each row of its table; pairs the table does not list are explicit
/// arms that change nothing (REQ-FSM-001).
#[allow(
    clippy::match_same_arms,
    clippy::too_many_lines,
    reason = "arms with equal bodies are kept apart so each matches one row of the viewer's table (REQ-FSM-001), and one arm per row makes the function as long as the table"
)]
#[must_use]
pub fn viewer_transition(machine: ViewerMachine, input: ViewerInput) -> (ViewerMachine, ViewerAction) {
    use ViewerAction as A;
    use ViewerEvent as E;
    use ViewerInput as I;
    use ViewerState as S;
    let unchanged = (machine, A::None);
    match (machine.state, input) {
        // Decoder events.
        (S::Starting, I::Event(E::Started)) => (machine.with_state(S::Searching), A::RequestInitial),
        (S::Searching, I::Event(E::PagePresent)) => (machine.with_state(S::Displaying), A::ShowPage),
        (S::Searching, I::Event(E::HeaderReceived)) => (machine, A::RollHeader),
        (S::Displaying, I::Event(E::PageUpdated)) => (machine, A::ClearStale),
        (S::Displaying, I::Event(E::PageStale)) => (machine, A::MarkStale),
        (S::Starting, I::Event(E::PagePresent | E::HeaderReceived | E::PageUpdated | E::PageStale))
        | (S::Searching, I::Event(E::Started | E::PageUpdated | E::PageStale))
        | (S::Displaying, I::Event(E::Started | E::PagePresent | E::HeaderReceived))
        | (
            S::Entering1 | S::Entering2 | S::EnteringSubPage | S::Holding | S::ConfirmingLink(_),
            I::Event(E::Started | E::PagePresent | E::HeaderReceived | E::PageUpdated | E::PageStale),
        ) => unchanged,

        // Page number entry (REQ-NAV-001): any state but Holding, sub-page
        // entry and a question.
        (S::Starting | S::Entering1 | S::Entering2 | S::Searching | S::Displaying, I::Digit(digit)) => {
            let mut entry = machine.entry;
            match entry.push(digit) {
                EntryResult::Rejected => (ViewerMachine { entry, ..machine }, A::RejectDigit),
                EntryResult::Partial => {
                    let state = if entry.count() == 1 { S::Entering1 } else { S::Entering2 };
                    (ViewerMachine { state, entry, ..machine }, A::ShowDigit)
                }
                EntryResult::Complete(address) => {
                    (ViewerMachine { state: S::Searching, entry, ..machine }, A::Select(address, SubCode::ANY))
                }
            }
        }
        (S::Holding, I::Digit(_)) => unchanged,

        // Sub-page entry (DEC-2363).
        (S::Displaying | S::Searching, I::SubPageKey) => (
            ViewerMachine {
                state: S::EnteringSubPage,
                subpage_entry: SubPageEntry::EMPTY,
                resume: machine.state,
                ..machine
            },
            A::BeginSubPageEntry,
        ),
        (S::EnteringSubPage, I::Digit(digit)) => {
            let mut subpage_entry = machine.subpage_entry;
            match subpage_entry.push(digit) {
                SubPageEntryResult::Rejected => (ViewerMachine { subpage_entry, ..machine }, A::RejectDigit),
                SubPageEntryResult::Partial => (ViewerMachine { subpage_entry, ..machine }, A::ShowDigit),
                // REQ-328: the viewer searches until the store holds the
                // sub-page, as it does for a page.
                SubPageEntryResult::Complete(subcode) => {
                    (ViewerMachine { state: S::Searching, subpage_entry, ..machine }, A::ShowSubPage(subcode))
                }
            }
        }
        (
            S::EnteringSubPage,
            I::SubPageKey
            | I::Hold
            | I::Reveal(_)
            | I::Mix
            | I::Zoom
            | I::Colour(_)
            | I::Index(_)
            | I::Home(_)
            | I::StepPage(_)
            | I::StepSubPage(_)
            | I::Back(_)
            | I::Forward(_),
        ) => (machine.with_state(machine.resume), A::None),
        (S::Starting | S::Entering1 | S::Entering2 | S::Holding, I::SubPageKey) => unchanged,

        // A sub-page chosen by the port.
        // REQ-328: a sub-page the store may not hold is searched for until
        // it comes, as a page is.
        (S::Displaying | S::Searching, I::ChooseSubPage(subcode)) => {
            (machine.with_state(S::Searching), A::ShowSubPage(subcode))
        }
        (
            S::Starting | S::Entering1 | S::Entering2 | S::EnteringSubPage | S::Holding | S::ConfirmingLink(_),
            I::ChooseSubPage(_),
        ) => unchanged,

        // A page the channel's program sends the viewer to (DOC-1960): from
        // any state, as the viewer's own selection would be, a visit of the
        // history (REQ-NAV-010), ending any entry or question.
        (
            S::Starting
            | S::Entering1
            | S::Entering2
            | S::EnteringSubPage
            | S::Searching
            | S::Displaying
            | S::Holding
            | S::ConfirmingLink(_),
            I::Redirect(address, subcode),
        ) => (
            ViewerMachine {
                state: S::Searching,
                entry: PageEntry::EMPTY,
                subpage_entry: SubPageEntry::EMPTY,
                ..machine
            },
            A::Select(address, subcode),
        ),

        // Hold.
        (S::Displaying, I::Hold) => (machine.with_state(S::Holding), A::StopUpdates),
        (S::Holding, I::Hold) => (machine.with_state(S::Displaying), A::ApplyUpdates),
        (S::Starting | S::Entering1 | S::Entering2 | S::Searching, I::Hold) => unchanged,

        // Reveal, mix and zoom are viewer settings, not states.
        (S::Starting | S::Entering1 | S::Entering2 | S::Searching | S::Displaying | S::Holding, I::Reveal(reveal)) => {
            match reveal {
                RevealInput::Latched => (machine, A::ToggleReveal),
                RevealInput::Momentary(until) => (machine, A::RevealUntil(until)),
            }
        }
        (S::Starting | S::Entering1 | S::Entering2 | S::Searching | S::Displaying | S::Holding, I::Mix) => {
            (machine, A::ToggleMix)
        }
        (S::Starting | S::Entering1 | S::Entering2 | S::Searching | S::Displaying | S::Holding, I::Zoom) => {
            (machine, A::CycleZoom)
        }

        // Coloured keys (REQ-NAV-002, REQ-NAV-012, REQ-SEC-024).
        (S::Displaying | S::Searching, I::Colour(colour)) => match (colour.prompt_shown, colour.target) {
            (false, _) | (true, None) => unchanged,
            (true, Some(target)) => match (target.service, colour.confirmed, machine.state) {
                (None, _, _) => (machine.with_state(S::Searching), A::Select(target.address, target.subcode)),
                (Some(_), true, _) => (machine.with_state(S::Searching), A::FollowService(target)),
                (Some(_), false, S::Displaying) => {
                    (machine.with_state(S::ConfirmingLink(colour.key)), A::ConfirmService(target))
                }
                (
                    Some(_),
                    false,
                    S::Starting
                    | S::Entering1
                    | S::Entering2
                    | S::EnteringSubPage
                    | S::Searching
                    | S::Holding
                    | S::ConfirmingLink(_),
                ) => unchanged,
            },
        },
        (S::ConfirmingLink(waiting), I::Colour(colour)) => match colour.target {
            Some(target) if colour.key == waiting => (machine.with_state(S::Searching), A::FollowService(target)),
            Some(_) | None => (machine.with_state(S::Displaying), A::RestoreRow24),
        },
        (
            S::ConfirmingLink(_),
            I::Digit(_)
            | I::SubPageKey
            | I::Hold
            | I::Reveal(_)
            | I::Mix
            | I::Zoom
            | I::Index(_)
            | I::Home(_)
            | I::StepPage(_)
            | I::StepSubPage(_)
            | I::Back(_)
            | I::Forward(_),
        ) => (machine.with_state(S::Displaying), A::RestoreRow24),

        // Index, Home and page stepping (REQ-NAV-003, REQ-NAV-008).
        (S::Displaying | S::Searching, I::Index(Some((address, subcode)))) => {
            (machine.with_state(S::Searching), A::Select(address, subcode))
        }
        (S::Displaying | S::Searching, I::Index(None)) => unchanged,
        (S::Displaying | S::Searching, I::Home(address) | I::StepPage(address)) => {
            (machine.with_state(S::Searching), A::Select(address, SubCode::ANY))
        }

        // Stored sub-pages (REQ-NAV-005).
        (S::Displaying, I::StepSubPage(Some(subcode))) => (machine, A::ShowSubPage(subcode)),
        (S::Displaying, I::StepSubPage(None)) | (S::Searching, I::StepSubPage(_)) => unchanged,

        // History (REQ-NAV-010).
        (S::Displaying | S::Searching, I::Back(Some(entry))) => {
            (machine.with_state(S::Searching), A::MoveInHistory { forward: false, entry })
        }
        (S::Displaying | S::Searching, I::Forward(Some(entry))) => {
            (machine.with_state(S::Searching), A::MoveInHistory { forward: true, entry })
        }
        (S::Displaying | S::Searching, I::Back(None) | I::Forward(None)) => unchanged,

        // Navigation keys do nothing before a page is requested, during
        // page number entry, or while holding.
        (
            S::Starting | S::Entering1 | S::Entering2 | S::Holding,
            I::Colour(_) | I::Index(_) | I::Home(_) | I::StepPage(_) | I::StepSubPage(_) | I::Back(_) | I::Forward(_),
        ) => unchanged,
    }
}

/// Names for the transition trace (REQ-FSM-010).
#[must_use]
pub const fn viewer_state_name(state: ViewerState) -> &'static str {
    match state {
        ViewerState::Starting => "Starting",
        ViewerState::Entering1 => "Entering 1 digit",
        ViewerState::Entering2 => "Entering 2 digits",
        ViewerState::EnteringSubPage => "Entering sub-page",
        ViewerState::Searching => "Searching",
        ViewerState::Displaying => "Displaying",
        ViewerState::Holding => "Holding",
        ViewerState::ConfirmingLink(_) => "Confirming link",
    }
}

/// The names of the inputs for the transition trace, by [`input_code`].
const INPUT_NAMES: [&str; INPUT_KINDS] = [
    "decoder started",
    "page present",
    "header received",
    "page updated",
    "page stale",
    "digit",
    "sub-page key",
    "hold key",
    "reveal key",
    "mix key",
    "zoom key",
    "coloured key",
    "index key",
    "home key",
    "next or previous page",
    "next or previous sub-page",
    "back",
    "forward",
    "sub-page chosen",
    "page redirect",
];
/// How many kinds of input the machine has.
const INPUT_KINDS: usize = 20;

/// The names of the actions for the transition trace, by [`action_code`].
const ACTION_NAMES: [&str; ACTION_KINDS] = [
    "none",
    "request initial page",
    "show page",
    "roll header",
    "mark not recently received",
    "clear not recently received",
    "show digit",
    "reject digit",
    "request page",
    "move in history and request",
    "request sub-page",
    "begin sub-page entry",
    "stop applying updates",
    "apply updates",
    "toggle reveal",
    "reveal for a while",
    "toggle mix",
    "step zoom",
    "follow link to other service",
    "ask to confirm other service",
    "restore row 24",
];
/// How many kinds of action the machine has.
const ACTION_KINDS: usize = 21;

/// The kind of an input, without its facts, as the viewer keeps it for the
/// trace: one byte, named only when the trace is taken.
#[must_use]
pub(super) const fn input_code(input: ViewerInput) -> u8 {
    match input {
        ViewerInput::Event(ViewerEvent::Started) => 0,
        ViewerInput::Event(ViewerEvent::PagePresent) => 1,
        ViewerInput::Event(ViewerEvent::HeaderReceived) => 2,
        ViewerInput::Event(ViewerEvent::PageUpdated) => 3,
        ViewerInput::Event(ViewerEvent::PageStale) => 4,
        ViewerInput::Digit(_) => 5,
        ViewerInput::SubPageKey => 6,
        ViewerInput::Hold => 7,
        ViewerInput::Reveal(_) => 8,
        ViewerInput::Mix => 9,
        ViewerInput::Zoom => 10,
        ViewerInput::Colour(_) => 11,
        ViewerInput::Index(_) => 12,
        ViewerInput::Home(_) => 13,
        ViewerInput::StepPage(_) => 14,
        ViewerInput::StepSubPage(_) => 15,
        ViewerInput::Back(_) => 16,
        ViewerInput::Forward(_) => 17,
        ViewerInput::ChooseSubPage(_) => 18,
        ViewerInput::Redirect(..) => 19,
    }
}

/// The kind of an action, without its data, as the viewer keeps it.
#[must_use]
pub(super) const fn action_code(action: ViewerAction) -> u8 {
    match action {
        ViewerAction::None => 0,
        ViewerAction::RequestInitial => 1,
        ViewerAction::ShowPage => 2,
        ViewerAction::RollHeader => 3,
        ViewerAction::MarkStale => 4,
        ViewerAction::ClearStale => 5,
        ViewerAction::ShowDigit => 6,
        ViewerAction::RejectDigit => 7,
        ViewerAction::Select(..) => 8,
        ViewerAction::MoveInHistory { .. } => 9,
        ViewerAction::ShowSubPage(_) => 10,
        ViewerAction::BeginSubPageEntry => 11,
        ViewerAction::StopUpdates => 12,
        ViewerAction::ApplyUpdates => 13,
        ViewerAction::ToggleReveal => 14,
        ViewerAction::RevealUntil(_) => 15,
        ViewerAction::ToggleMix => 16,
        ViewerAction::CycleZoom => 17,
        ViewerAction::FollowService(_) => 18,
        ViewerAction::ConfirmService(_) => 19,
        ViewerAction::RestoreRow24 => 20,
    }
}

/// The name of the input kind `code`.
pub(super) fn input_code_name(code: u8) -> &'static str {
    INPUT_NAMES.get(usize::from(code)).copied().unwrap_or("")
}

/// The name of the action kind `code`.
pub(super) fn action_code_name(code: u8) -> &'static str {
    ACTION_NAMES.get(usize::from(code)).copied().unwrap_or("")
}

/// Names for the transition trace.
#[must_use]
pub fn viewer_input_name(input: ViewerInput) -> &'static str {
    input_code_name(input_code(input))
}

/// Names for the transition trace.
#[must_use]
pub fn viewer_action_name(action: ViewerAction) -> &'static str {
    action_code_name(action_code(action))
}

#[cfg(test)]
mod tests;
