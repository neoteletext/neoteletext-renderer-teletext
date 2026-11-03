//! The viewer (area NAV, layer L6): the viewer state machine of
//! REQ-FSM-005, the receiver-owned status area (decision E.10,
//! REQ-SEC-005), and composing the grid shown on screen.

use crate::acq::{EnhancementLookup, MagazineData, PageStore};
use crate::config::Config;
use crate::consts::{FASTEXT_KEYS, HEADER_DISPLAY_BYTES};
use crate::level1::Zoom;
use crate::nav::{History, HistoryEntry, LinkTarget, PageLinks};
use crate::trace::{Machine, TraceEntry};
use crate::types::{FixedVec, Instant, PageAddress, SubCode};

mod keys;
mod machine;
mod screen;
mod status;

pub use machine::{
    ColourInput, RevealInput, ViewerAction, ViewerInput, ViewerMachine, viewer_action_name, viewer_input_name,
    viewer_state_name, viewer_transition,
};
pub use screen::Composed;

/// Viewer keys: the numbered functions of area NAV.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// A digit 0 to 9.
    Digit(u8),
    /// The red Fastext key.
    Red,
    /// The green Fastext key.
    Green,
    /// The yellow Fastext key.
    Yellow,
    /// The cyan Fastext key.
    Cyan,
    /// Index.
    Index,
    /// Home: the initial page (REQ-NAV-008).
    Home,
    /// Next page.
    NextPage,
    /// Previous page.
    PreviousPage,
    /// Next stored sub-page.
    NextSubPage,
    /// Previous stored sub-page.
    PreviousSubPage,
    /// Reveal.
    Reveal,
    /// Hold.
    Hold,
    /// Mix.
    Mix,
    /// Page zoom.
    Zoom,
    /// Sub-page entry: the next four digits are a sub-code (REQ-NAV-005,
    /// REQ-NUM-017, DEC-2363).
    SubPage,
    /// Back in history.
    Back,
    /// Forward in history.
    Forward,
}

/// States of the viewer machine (REQ-FSM-005).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewerState {
    /// Before the decoder starts.
    Starting,
    /// One digit entered.
    Entering1,
    /// Two digits entered.
    Entering2,
    /// Sub-page digits are being entered for the page shown (REQ-NAV-005,
    /// DEC-2363).
    EnteringSubPage,
    /// The requested page is not yet stored.
    Searching,
    /// The requested page is shown.
    Displaying,
    /// Updates are not applied.
    Holding,
    /// Waiting for the same coloured key to confirm a link to another
    /// service (REQ-NAV-012).
    ConfirmingLink(u8),
}

/// What the application must do after a viewer transition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Nothing beyond redrawing.
    None,
    /// Request a page from the current service.
    Request(PageAddress, SubCode),
    /// Follow a link to another service, already confirmed (REQ-NAV-012).
    FollowService(LinkTarget),
    /// Ask for confirmation of a link to another service.
    ConfirmService(LinkTarget),
}

/// Events from the decoder that drive the viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewerEvent {
    /// The decoder started.
    Started,
    /// The requested page is now in the store.
    PagePresent,
    /// A header arrived.
    HeaderReceived,
    /// The displayed page changed.
    PageUpdated,
    /// The displayed page is stale.
    PageStale,
}

/// The security indicator shown in the status area (REQ-SEC-005).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecurityIndicator {
    /// No indicator: broadcast or encrypted source.
    None,
    /// The page on screen came over plain HTTP (REQ-SEC-008).
    Unencrypted,
}

/// What a key press needs to know about its surroundings.
#[derive(Clone, Copy)]
pub struct KeyContext<'a> {
    /// The links of the page shown (from X/27/0 or the TTI Link Extension).
    pub links: &'a PageLinks,
    /// The page store.
    pub store: &'a PageStore<'a>,
    /// The initial page (REQ-SVC-004).
    pub initial: PageAddress,
    /// The configuration.
    pub config: &'a Config,
    /// The current time.
    pub now: Instant,
    /// Whether another service's origin was confirmed this session.
    pub confirmed: &'a dyn Fn(u8) -> bool,
    /// Which of the coloured keys' prompts are on screen, in the order red,
    /// green, yellow, cyan ([`Grid::prompts_shown`](crate::level1::Grid::prompts_shown)):
    /// a key whose prompt is not does nothing (REQ-SEC-024).
    pub prompts_shown: [bool; FASTEXT_KEYS],
}

/// What composing the screen needs besides the page.
#[derive(Clone, Copy)]
pub struct ComposeContext<'a> {
    /// The magazine data of the page's magazine.
    pub magazine: &'a MagazineData,
    /// The configuration.
    pub config: &'a Config,
    /// Header columns 8 to 39 rolled from incoming headers.
    pub rolling_header: &'a [u8; HEADER_DISPLAY_BYTES],
    /// The page store.
    pub store: &'a PageStore<'a>,
    /// A receiver-drawn row 24 (REQ-NAV-011).
    pub row24: Option<[u8; crate::consts::COLUMNS]>,
    /// The stored object pages, other enhancement pages and Magazine
    /// Organization Tables, for Levels 2.5 and 3.5 (REQ-ACQ-020).
    pub lookup: EnhancementLookup<'a>,
}

/// History capacity (REQ-NAV-010 default).
pub const HISTORY_ENTRIES: usize = 32;

/// The viewer's transitions kept for the trace between two takes by the
/// port (REQ-FSM-010): a port takes them every pass, and a pass sees a few
/// keys at most; when more come, the oldest are dropped and counted.
pub const VIEWER_TRACE_ENTRIES: usize = 8;

/// One viewer transition kept for the trace, stamped when the port takes it.
/// It keeps the kinds of input and action as a byte each and is named only
/// when taken, so it costs a few bytes of the viewer, and a port that never
/// takes the trace links none of the names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Transition {
    from: ViewerState,
    input: u8,
    action: u8,
    to: ViewerState,
}

const NO_TRANSITION: Transition =
    Transition { from: ViewerState::Starting, input: 0, action: 0, to: ViewerState::Starting };

/// The viewer.
// Reveal, mix, boxed display and staleness are separate settings and facts
// of the viewer (REQ-NAV-007, REQ-L1-020), each set and read alone.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug)]
pub struct Viewer {
    machine: ViewerMachine,
    requested: PageAddress,
    subcode: SubCode,
    history: History<HISTORY_ENTRIES>,
    reveal: bool,
    reveal_until: Option<Instant>,
    mix: bool,
    boxed: bool,
    zoom: Zoom,
    stale: bool,
    indicator: SecurityIndicator,
    service: u8,
    tracing: bool,
    transitions: FixedVec<Transition, VIEWER_TRACE_ENTRIES>,
    transitions_lost: usize,
}

impl Viewer {
    /// A viewer that will start on `initial`.
    #[must_use]
    pub fn new(initial: PageAddress) -> Self {
        Self {
            machine: ViewerMachine::START,
            requested: initial,
            subcode: SubCode::ANY,
            history: History::new(),
            reveal: false,
            reveal_until: None,
            mix: false,
            boxed: false,
            zoom: Zoom::Off,
            stale: false,
            indicator: SecurityIndicator::None,
            service: 0,
            tracing: false,
            transitions: FixedVec::new(NO_TRANSITION),
            transitions_lost: 0,
        }
    }

    /// The same viewer with its transition trace on or off, as the
    /// configuration's `trace` says (REQ-FSM-010).
    #[must_use]
    pub const fn with_trace(mut self, on: bool) -> Self {
        self.tracing = on;
        self
    }

    /// Whether the viewer keeps its transitions for the trace.
    #[must_use]
    pub const fn traces(&self) -> bool {
        self.tracing
    }

    /// Hands the transitions kept since the last take to `sink`, oldest
    /// first, stamped with `time`, and returns how many were dropped
    /// because more came between two takes than are kept.
    pub fn take_trace(&mut self, time: Instant, sink: &mut dyn FnMut(TraceEntry)) -> usize {
        for transition in self.transitions.as_slice() {
            sink(TraceEntry {
                machine: Machine::Viewer,
                time,
                from: viewer_state_name(transition.from),
                event: machine::input_code_name(transition.input),
                action: machine::action_code_name(transition.action),
                to: viewer_state_name(transition.to),
            });
        }
        self.transitions.clear();
        core::mem::take(&mut self.transitions_lost)
    }

    /// The machine state.
    #[must_use]
    pub const fn state(&self) -> ViewerState {
        self.machine.state
    }

    /// The requested page and sub-code.
    #[must_use]
    pub const fn requested(&self) -> (PageAddress, SubCode) {
        (self.requested, self.subcode)
    }

    /// Reveal is on.
    #[must_use]
    pub fn reveal(&self, now: Instant) -> bool {
        self.reveal || self.reveal_until.is_some_and(|until| now < until)
    }

    /// Page zoom.
    #[must_use]
    pub const fn zoom(&self) -> Zoom {
        self.zoom
    }

    /// Mix mode.
    #[must_use]
    pub const fn mix(&self) -> bool {
        self.mix
    }

    /// Boxed display (REQ-L1-020): only the page's boxed areas are shown,
    /// as on a newsflash or subtitle page, whatever the page's C5 and C6
    /// say; a port turns it on to show subtitles over a picture.
    #[must_use]
    pub const fn boxed(&self) -> bool {
        self.boxed
    }

    /// Turns boxed display on or off.
    pub fn set_boxed(&mut self, boxed: bool) {
        self.boxed = boxed;
    }

    /// Sets the security indicator (from the web module's events).
    pub fn set_indicator(&mut self, indicator: SecurityIndicator) {
        self.indicator = indicator;
    }

    /// A link to another service waits for confirmation, whose question
    /// takes the status area for as long as it is asked. It lives in the
    /// state and not in the indicator: the page on screen is the same page
    /// while it is asked, and what is known of it stands (REQ-SEC-008).
    const fn confirming(&self) -> bool {
        matches!(self.machine.state, ViewerState::ConfirmingLink(_))
    }

    /// Sets the service the viewer is showing, for history entries.
    pub fn set_service(&mut self, service: u8) {
        self.service = service;
    }

    /// The history.
    #[must_use]
    pub const fn history(&self) -> &History<HISTORY_ENTRIES> {
        &self.history
    }

    /// Wipes the history (REQ-NAV-010).
    pub fn wipe_history(&mut self) {
        self.history.wipe();
    }

    /// Shows a sub-page of the page shown, entered directly (REQ-NAV-005):
    /// one of the 3,199 decimal sub-codes ([`crate::nav::SubPageEntry`];
    /// REQ-NUM-017), or `SubCode::ANY` to return to the rotating display.
    /// Ignored unless a page is shown or searched for.
    pub fn show_subpage(&mut self, subcode: SubCode) -> Action {
        self.run(ViewerInput::ChooseSubPage(subcode))
    }

    /// Handles a decoder event (REQ-FSM-005).
    pub fn event(&mut self, event: ViewerEvent) -> Action {
        self.run(ViewerInput::Event(event))
    }

    /// Makes one transition of the machine, keeps it for the trace and
    /// applies its action.
    fn run(&mut self, input: ViewerInput) -> Action {
        let before = self.machine;
        let (after, action) = viewer_transition(before, input);
        self.machine = after;
        if self.tracing && (after.state != before.state || action != ViewerAction::None) {
            self.keep_transition(Transition {
                from: before.state,
                input: machine::input_code(input),
                action: machine::action_code(action),
                to: after.state,
            });
        }
        self.apply(action)
    }

    fn keep_transition(&mut self, transition: Transition) {
        if self.transitions.is_full() {
            let _ = self.transitions.remove(0);
            self.transitions_lost = self.transitions_lost.saturating_add(1);
        }
        let _ = self.transitions.push(transition);
    }

    /// Carries out a transition's action on the viewer's settings and
    /// navigation, and says what the application must do.
    fn apply(&mut self, action: ViewerAction) -> Action {
        match action {
            ViewerAction::None
            | ViewerAction::ShowPage
            | ViewerAction::RollHeader
            | ViewerAction::ShowDigit
            | ViewerAction::RejectDigit
            | ViewerAction::BeginSubPageEntry
            | ViewerAction::StopUpdates
            | ViewerAction::ApplyUpdates
            | ViewerAction::RestoreRow24 => Action::None,
            ViewerAction::RequestInitial => self.select(self.requested, SubCode::ANY, true),
            ViewerAction::MarkStale => {
                self.stale = true;
                Action::None
            }
            ViewerAction::ClearStale => {
                self.stale = false;
                Action::None
            }
            ViewerAction::Select(address, subcode) => self.select(address, subcode, true),
            ViewerAction::MoveInHistory { forward, entry } => {
                let _ = if forward { self.history.forward() } else { self.history.back() };
                self.select(entry.address, entry.subcode, false)
            }
            ViewerAction::ShowSubPage(subcode) => {
                self.subcode = subcode;
                Action::Request(self.requested, subcode)
            }
            ViewerAction::ToggleReveal => {
                self.reveal = !self.reveal;
                Action::None
            }
            ViewerAction::RevealUntil(until) => {
                self.reveal_until = Some(until);
                Action::None
            }
            ViewerAction::ToggleMix => {
                self.mix = !self.mix;
                Action::None
            }
            ViewerAction::CycleZoom => {
                self.zoom = match self.zoom {
                    Zoom::Off => Zoom::Top,
                    Zoom::Top => Zoom::Bottom,
                    Zoom::Bottom => Zoom::Off,
                };
                Action::None
            }
            ViewerAction::FollowService(target) => Action::FollowService(target),
            ViewerAction::ConfirmService(target) => Action::ConfirmService(target),
        }
    }

    /// Requests a page; the machine has already moved to Searching.
    fn select(&mut self, address: PageAddress, subcode: SubCode, record: bool) -> Action {
        if address != self.requested {
            // REQ-NAV-007: reveal clears when a different page is selected.
            self.reveal = false;
            self.reveal_until = None;
        }
        self.requested = address;
        self.subcode = subcode;
        self.stale = false;
        if record {
            self.history.visit(HistoryEntry { service: self.service, address, subcode });
        }
        Action::Request(address, subcode)
    }
}

#[cfg(test)]
mod tests;
