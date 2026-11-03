//! Tests of the viewer machine's transition function, row by row
//! (REQ-FSM-005, REQ-FSM-009), and of its trace (REQ-FSM-010).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use super::*;
use crate::nav::HistoryEntry;
use crate::trace::{Machine, TraceEntry};
use crate::viewer::{Action, Viewer};
use ViewerAction as A;
use ViewerEvent as E;
use ViewerInput as I;
use ViewerState as S;
use std::vec::Vec;

fn page(mpp: u16) -> PageAddress {
    PageAddress::from_mpp(mpp).unwrap()
}

fn at(state: ViewerState) -> ViewerMachine {
    ViewerMachine { state, ..ViewerMachine::START }
}

/// Feeds `inputs` from the start and returns the machine reached.
fn after(inputs: &[ViewerInput]) -> ViewerMachine {
    inputs.iter().fold(ViewerMachine::START, |machine, &input| viewer_transition(machine, input).0)
}

#[track_caller]
fn row(from: ViewerMachine, input: ViewerInput, action: ViewerAction, to: ViewerState) {
    let (next, done) = viewer_transition(from, input);
    assert_eq!((done, next.state), (action, to), "from {:?} on {input:?}", from.state);
}

fn colour(key: u8, service: Option<u8>, prompt_shown: bool, confirmed: bool) -> ViewerInput {
    let target = LinkTarget { address: page(0x400), subcode: SubCode::ANY, service };
    I::Colour(ColourInput { key, target: Some(target), prompt_shown, confirmed })
}

fn target(service: Option<u8>) -> LinkTarget {
    LinkTarget { address: page(0x400), subcode: SubCode::ANY, service }
}

const SHOWN: [ViewerState; 2] = [S::Displaying, S::Searching];

/// REQ-FSM-009: each row of REQ-FSM-005's table (REQ-137 version 2), in
/// the table's order.
#[test]
fn viewer_table_rows() {
    // Starting, decoder started.
    row(at(S::Starting), I::Event(E::Started), A::RequestInitial, S::Searching);

    // Any state except Holding, sub-page entry and a question: first digit.
    for state in [S::Starting, S::Searching, S::Displaying] {
        row(at(state), I::Digit(1), A::ShowDigit, S::Entering1);
        row(at(state), I::Digit(8), A::ShowDigit, S::Entering1);
    }
    // Entering 1 digit, then 2.
    let one = after(&[I::Event(E::Started), I::Digit(2)]);
    row(one, I::Digit(0), A::ShowDigit, S::Entering2);
    let two = after(&[I::Event(E::Started), I::Digit(2), I::Digit(0)]);
    row(two, I::Digit(5), A::Select(page(0x205), SubCode::ANY), S::Searching);
    // A digit that cannot make a page: rejected, the digits kept.
    row(at(S::Displaying), I::Digit(0), A::RejectDigit, S::Displaying);
    row(at(S::Displaying), I::Digit(9), A::RejectDigit, S::Displaying);
    let (kept, _) = viewer_transition(one, I::Digit(10));
    assert_eq!((kept.state, kept.entry), (S::Entering1, one.entry), "the first digit is kept");
    row(two, I::Digit(10), A::RejectDigit, S::Entering2);

    // The sub-page key, and its four digits (DEC-2363).
    for state in SHOWN {
        let (entering, action) = viewer_transition(at(state), I::SubPageKey);
        assert_eq!((entering.state, entering.resume, action), (S::EnteringSubPage, state, A::BeginSubPageEntry));
        row(entering, I::Digit(0), A::ShowDigit, S::EnteringSubPage);
        row(entering, I::Digit(4), A::RejectDigit, S::EnteringSubPage);
        let three = [0, 0, 0].iter().fold(entering, |machine, &digit| viewer_transition(machine, I::Digit(digit)).0);
        row(three, I::Digit(0), A::RejectDigit, S::EnteringSubPage);
        row(three, I::Digit(7), A::ShowSubPage(SubCode::new(0x0007)), state);
        for other in [I::Hold, I::Mix, colour(0, None, true, false), I::Home(page(0x100)), I::SubPageKey] {
            row(entering, other, A::None, state);
        }
    }

    // A sub-page chosen by the port.
    for state in SHOWN {
        row(at(state), I::ChooseSubPage(SubCode::new(2)), A::ShowSubPage(SubCode::new(2)), state);
    }

    // Searching: page present, header received.
    row(at(S::Searching), I::Event(E::PagePresent), A::ShowPage, S::Displaying);
    row(at(S::Searching), I::Event(E::HeaderReceived), A::RollHeader, S::Searching);

    // Displaying: page updated, page stale.
    row(at(S::Displaying), I::Event(E::PageUpdated), A::ClearStale, S::Displaying);
    row(at(S::Displaying), I::Event(E::PageStale), A::MarkStale, S::Displaying);

    // Hold.
    row(at(S::Displaying), I::Hold, A::StopUpdates, S::Holding);
    row(at(S::Holding), I::Hold, A::ApplyUpdates, S::Displaying);

    // A coloured key in the same service whose prompt is shown, Index,
    // Home, next or previous page.
    for state in SHOWN {
        row(at(state), colour(1, None, true, false), A::Select(page(0x400), SubCode::ANY), S::Searching);
        let index = I::Index(Some((page(0x199), SubCode::ANY)));
        row(at(state), index, A::Select(page(0x199), SubCode::ANY), S::Searching);
        row(at(state), I::Home(page(0x100)), A::Select(page(0x100), SubCode::ANY), S::Searching);
        row(at(state), I::StepPage(page(0x101)), A::Select(page(0x101), SubCode::ANY), S::Searching);
    }

    // A coloured key to another service already confirmed.
    for state in SHOWN {
        row(at(state), colour(2, Some(1), true, true), A::FollowService(target(Some(1))), S::Searching);
    }
    // To an unconfirmed one: asked while Displaying, nothing while Searching.
    row(at(S::Displaying), colour(2, Some(1), true, false), A::ConfirmService(target(Some(1))), S::ConfirmingLink(2));
    row(at(S::Searching), colour(2, Some(1), true, false), A::None, S::Searching);

    // Confirming link: the same key, any other key.
    row(at(S::ConfirmingLink(2)), colour(2, Some(1), true, false), A::FollowService(target(Some(1))), S::Searching);
    for other in [colour(1, Some(1), true, false), I::Digit(1), I::Hold, I::Back(None), I::Mix, I::SubPageKey] {
        row(at(S::ConfirmingLink(2)), other, A::RestoreRow24, S::Displaying);
    }

    // Displaying: next or previous sub-page.
    row(at(S::Displaying), I::StepSubPage(Some(SubCode::new(3))), A::ShowSubPage(SubCode::new(3)), S::Displaying);

    // Back or Forward, with and without an entry in that direction.
    let entry = HistoryEntry { service: 0, address: page(0x150), subcode: SubCode::ANY };
    for state in SHOWN {
        row(at(state), I::Back(Some(entry)), A::MoveInHistory { forward: false, entry }, S::Searching);
        row(at(state), I::Forward(Some(entry)), A::MoveInHistory { forward: true, entry }, S::Searching);
        row(at(state), I::Back(None), A::None, state);
        row(at(state), I::Forward(None), A::None, state);
    }
}

/// REQ-FSM-001, REQ-137: any event or key the table does not list for a
/// state leaves the state unchanged and does nothing; reveal, mix and zoom
/// are settings, not states.
#[test]
fn viewer_unlisted_pairs_change_nothing() {
    row(at(S::Holding), I::Digit(1), A::None, S::Holding);
    for input in [I::Event(E::PagePresent), I::Event(E::PageStale), I::Event(E::HeaderReceived)] {
        row(at(S::Starting), input, A::None, S::Starting);
    }
    row(at(S::Displaying), I::Event(E::Started), A::None, S::Displaying);
    row(at(S::Searching), I::Event(E::PageStale), A::None, S::Searching);
    for state in [S::Starting, S::Entering1, S::Entering2, S::Holding] {
        for input in [colour(0, None, true, false), I::Index(Some((page(0x100), SubCode::ANY))), I::Back(None)] {
            row(at(state), input, A::None, state);
        }
        row(at(state), I::ChooseSubPage(SubCode::new(1)), A::None, state);
        row(at(state), I::Mix, A::ToggleMix, state);
        row(at(state), I::Zoom, A::CycleZoom, state);
        row(at(state), I::Reveal(RevealInput::Latched), A::ToggleReveal, state);
    }
    // REQ-SEC-024: a key whose prompt is not shown does nothing.
    row(at(S::Displaying), colour(0, None, false, false), A::None, S::Displaying);
    row(
        at(S::Displaying),
        I::Colour(ColourInput { key: 0, target: None, prompt_shown: true, confirmed: false }),
        A::None,
        S::Displaying,
    );
    // Index with nowhere to go; sub-page stepping while searching.
    row(at(S::Displaying), I::Index(None), A::None, S::Displaying);
    row(at(S::Searching), I::StepSubPage(Some(SubCode::new(3))), A::None, S::Searching);
    row(at(S::Displaying), I::StepSubPage(None), A::None, S::Displaying);
    // Hold outside Displaying and Holding.
    row(at(S::Searching), I::Hold, A::None, S::Searching);
    row(at(S::Displaying), I::Reveal(RevealInput::Momentary(Instant(5))), A::RevealUntil(Instant(5)), S::Displaying);
}

/// REQ-FSM-010: every state, input and action has its own trace name.
#[test]
fn viewer_trace_names_are_distinct() {
    let states = [
        S::Starting,
        S::Entering1,
        S::Entering2,
        S::EnteringSubPage,
        S::Searching,
        S::Displaying,
        S::Holding,
        S::ConfirmingLink(0),
    ];
    let entry = HistoryEntry { service: 0, address: page(0x100), subcode: SubCode::ANY };
    let inputs = [
        I::Event(E::Started),
        I::Event(E::PagePresent),
        I::Event(E::HeaderReceived),
        I::Event(E::PageUpdated),
        I::Event(E::PageStale),
        I::Digit(0),
        I::SubPageKey,
        I::Hold,
        I::Reveal(RevealInput::Latched),
        I::Mix,
        I::Zoom,
        colour(0, None, true, false),
        I::Index(None),
        I::Home(page(0x100)),
        I::StepPage(page(0x100)),
        I::StepSubPage(None),
        I::Back(None),
        I::Forward(None),
        I::ChooseSubPage(SubCode::ANY),
    ];
    let actions = [
        A::None,
        A::RequestInitial,
        A::ShowPage,
        A::RollHeader,
        A::MarkStale,
        A::ClearStale,
        A::ShowDigit,
        A::RejectDigit,
        A::Select(page(0x100), SubCode::ANY),
        A::MoveInHistory { forward: false, entry },
        A::ShowSubPage(SubCode::ANY),
        A::BeginSubPageEntry,
        A::StopUpdates,
        A::ApplyUpdates,
        A::ToggleReveal,
        A::RevealUntil(Instant(0)),
        A::ToggleMix,
        A::CycleZoom,
        A::FollowService(target(None)),
        A::ConfirmService(target(None)),
        A::RestoreRow24,
    ];
    let distinct = |names: Vec<&'static str>| {
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "{names:?}");
    };
    distinct(states.iter().map(|&state| viewer_state_name(state)).collect());
    distinct(inputs.iter().map(|&input| viewer_input_name(input)).collect());
    distinct(actions.iter().map(|&action| viewer_action_name(action)).collect());
}

/// REQ-FSM-010: the viewer reports each transition, with its state before,
/// input, action and state after, when its trace is on, and nothing when
/// it is off.
#[test]
fn the_viewer_is_traced() {
    let mut viewer = Viewer::new(page(0x100)).with_trace(true);
    assert_eq!(viewer.event(E::Started), Action::Request(page(0x100), SubCode::ANY));
    let _ = viewer.event(E::PagePresent);
    let _ = viewer.event(E::Started); // Changes nothing: not traced.
    let _ = viewer.show_subpage(SubCode::new(2));
    let mut entries = Vec::new();
    assert_eq!(viewer.take_trace(Instant(7), &mut |entry| entries.push(entry)), 0);
    let entry =
        |from, event, action, to| TraceEntry { machine: Machine::Viewer, time: Instant(7), from, event, action, to };
    assert_eq!(
        entries,
        [
            entry("Starting", "decoder started", "request initial page", "Searching"),
            entry("Searching", "page present", "show page", "Displaying"),
            entry("Displaying", "sub-page chosen", "request sub-page", "Displaying"),
        ]
    );
    let mut again = Vec::new();
    assert_eq!(viewer.take_trace(Instant(8), &mut |entry| again.push(entry)), 0);
    assert!(again.is_empty(), "a take empties the trace");

    // More transitions than are kept between two takes: the oldest dropped
    // and counted.
    for _ in 0..crate::viewer::VIEWER_TRACE_ENTRIES + 3 {
        let _ = viewer.event(E::PageStale);
    }
    let mut kept = Vec::new();
    assert_eq!(viewer.take_trace(Instant(9), &mut |entry| kept.push(entry)), 3);
    assert_eq!(kept.len(), crate::viewer::VIEWER_TRACE_ENTRIES);

    let mut quiet = Viewer::new(page(0x100));
    let _ = quiet.event(E::Started);
    let mut none = Vec::new();
    assert_eq!(quiet.take_trace(Instant(7), &mut |entry| none.push(entry)), 0);
    assert!(none.is_empty(), "the trace is off unless asked for");
}
