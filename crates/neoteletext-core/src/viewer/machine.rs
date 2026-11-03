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
// Arms with equal bodies are kept apart so each matches one table row.
#[allow(clippy::match_same_arms, clippy::too_many_lines)]
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
                SubPageEntryResult::Complete(subcode) => {
                    (ViewerMachine { state: machine.resume, subpage_entry, ..machine }, A::ShowSubPage(subcode))
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
        (S::Displaying | S::Searching, I::ChooseSubPage(subcode)) => (machine, A::ShowSubPage(subcode)),
        (
            S::Starting | S::Entering1 | S::Entering2 | S::EnteringSubPage | S::Holding | S::ConfirmingLink(_),
            I::ChooseSubPage(_),
        ) => unchanged,

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

/// Names for the transition trace.
#[must_use]
pub const fn viewer_input_name(input: ViewerInput) -> &'static str {
    match input {
        ViewerInput::Event(ViewerEvent::Started) => "decoder started",
        ViewerInput::Event(ViewerEvent::PagePresent) => "page present",
        ViewerInput::Event(ViewerEvent::HeaderReceived) => "header received",
        ViewerInput::Event(ViewerEvent::PageUpdated) => "page updated",
        ViewerInput::Event(ViewerEvent::PageStale) => "page stale",
        ViewerInput::Digit(_) => "digit",
        ViewerInput::SubPageKey => "sub-page key",
        ViewerInput::Hold => "hold key",
        ViewerInput::Reveal(_) => "reveal key",
        ViewerInput::Mix => "mix key",
        ViewerInput::Zoom => "zoom key",
        ViewerInput::Colour(_) => "coloured key",
        ViewerInput::Index(_) => "index key",
        ViewerInput::Home(_) => "home key",
        ViewerInput::StepPage(_) => "next or previous page",
        ViewerInput::StepSubPage(_) => "next or previous sub-page",
        ViewerInput::Back(_) => "back",
        ViewerInput::Forward(_) => "forward",
        ViewerInput::ChooseSubPage(_) => "sub-page chosen",
    }
}

/// Names for the transition trace.
#[must_use]
pub const fn viewer_action_name(action: ViewerAction) -> &'static str {
    match action {
        ViewerAction::None => "none",
        ViewerAction::RequestInitial => "request initial page",
        ViewerAction::ShowPage => "show page",
        ViewerAction::RollHeader => "roll header",
        ViewerAction::MarkStale => "mark not recently received",
        ViewerAction::ClearStale => "clear not recently received",
        ViewerAction::ShowDigit => "show digit",
        ViewerAction::RejectDigit => "reject digit",
        ViewerAction::Select(..) => "request page",
        ViewerAction::MoveInHistory { .. } => "move in history and request",
        ViewerAction::ShowSubPage(_) => "request sub-page",
        ViewerAction::BeginSubPageEntry => "begin sub-page entry",
        ViewerAction::StopUpdates => "stop applying updates",
        ViewerAction::ApplyUpdates => "apply updates",
        ViewerAction::ToggleReveal => "toggle reveal",
        ViewerAction::RevealUntil(_) => "reveal for a while",
        ViewerAction::ToggleMix => "toggle mix",
        ViewerAction::CycleZoom => "step zoom",
        ViewerAction::FollowService(_) => "follow link to other service",
        ViewerAction::ConfirmService(_) => "ask to confirm other service",
        ViewerAction::RestoreRow24 => "restore row 24",
    }
}

#[cfg(test)]
mod tests;
