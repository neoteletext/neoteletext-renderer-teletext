#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out and indexing into data it built to a known size"
)]

use super::*;
use crate::MAX_PER_SECOND;
use crate::caps::{CURSOR_CAP, RESUME_CAP, SELECT_CAP, TOUCH_CAP};
use crate::control::Resumption;
use crate::event::{self, Action, Device, Mouse, MouseAction, Stamp, Touch};
use crate::event::{Key, KeyCode};
use crate::keys;
use crate::rules::{FaultCode, PageStateEvent, RuleBody, RuleEvent, RulesFault};
use neoteletext_rules::table::Target;

/// The page on screen in these tests.
const ON: Stamp = Stamp { page: 0x100, sub: 0 };

fn welcomed(caps: u32) -> Session {
    let mut session = Session::new(caps);
    session.open(0, None, &mut [0; crate::MAX_MESSAGE]).unwrap();
    session.welcome(VERSION, caps, None).unwrap();
    session
}

/// The first message `open` writes, read back.
fn opening<const N: usize>(session: &mut SessionOf<N>, now: u32, agent: Option<&str>) -> std::vec::Vec<Event<'static>> {
    let mut out = [0; crate::MAX_MESSAGE];
    let length = session.open(now, agent, &mut out).unwrap();
    let mut events = std::vec::Vec::new();
    event::read_message(&out[..length], &mut |item| match item {
        Item::Event(Event::Hello { version, caps, agent: None }) => {
            events.push(Event::Hello { version, caps, agent: None });
        }
        Item::Event(Event::Hello { version, caps, agent: Some(_) }) => {
            events.push(Event::Hello { version, caps, agent: Some("agent") });
        }
        Item::Event(Event::Resume(resume)) => events.push(Event::Resume(resume)),
        other => panic!("{other:?}"),
    })
    .unwrap();
    events
}

// REQ-INT-106.
#[test]
fn nothing_goes_before_welcome_and_a_late_one_refuses() {
    let mut session = Session::new(SELECT_CAP | CURSOR_CAP);
    let hello = opening(&mut session, 1_000, None);
    assert_eq!(hello, [Event::Hello { version: VERSION, caps: SELECT_CAP | CURSOR_CAP, agent: None }]);
    assert!(!session.welcomed());
    assert_eq!(session.welcome_due(), Some(1_000 + WELCOME_TIMEOUT_MS));
    session.tick(1_000 + WELCOME_TIMEOUT_MS - 1);
    assert!(matches!(session.state(), State::Waiting(_)));
    session.tick(1_000 + WELCOME_TIMEOUT_MS);
    assert_eq!(session.state(), State::Refused);
    assert_eq!(session.welcome(VERSION, SELECT_CAP, None), Err(WelcomeError::Unasked));
}

// REQ-INT-106.
#[test]
fn hello_carries_the_agent_only_with_its_capability() {
    let mut session = Session::new(SELECT_CAP | AGENT);
    assert_eq!(opening(&mut session, 0, None), [Event::Hello { version: VERSION, caps: SELECT_CAP, agent: None }]);
    let agent = Some("NeoTeletext/0.1 (macOS)");
    assert_eq!(
        opening(&mut session, 0, agent),
        [Event::Hello { version: VERSION, caps: SELECT_CAP | AGENT, agent: Some("agent") }]
    );
}

// REQ-INT-106.
#[test]
fn welcome_must_be_this_version_and_a_subset() {
    let mut session = Session::new(SELECT_CAP);
    opening(&mut session, 0, None);
    assert_eq!(
        session.welcome(VERSION, SELECT_CAP | CURSOR_CAP, None),
        Err(WelcomeError::Caps),
        "a capability not offered"
    );
    opening(&mut session, 0, None);
    assert_eq!(session.welcome(1, SELECT_CAP, None), Err(WelcomeError::Version(1)));
    opening(&mut session, 0, None);
    assert_eq!(session.welcome(VERSION, SELECT_CAP, None), Ok(Welcomed::New));
    assert_eq!(session.caps(), SELECT_CAP);
}

// REQ-INT-111, decision E.125: an older channel's mode records change
// nothing, before WELCOME or after.
#[test]
fn the_withdrawn_mode_records_change_nothing() {
    let mut session = Session::new(SELECT_CAP);
    opening(&mut session, 0, None);
    assert_eq!(session.control(&Control::Withdrawn(0x02)), Ok(None));
    session.control(&Control::Welcome { version: VERSION, caps: SELECT_CAP, resume: None }).unwrap();
    assert_eq!(session.control(&Control::Withdrawn(0x04)), Ok(None));
    assert!(session.welcomed());
}

// REQ-INT-106.
#[test]
fn a_reconnection_is_a_new_session() {
    let mut session = welcomed(SELECT_CAP);
    sent_item(&mut session, &Item::Text("abc"));
    session.close();
    opening(&mut session, 10, None);
    assert_eq!(session.count(), 1, "the count from zero, HELLO counted");
    assert_eq!(session.acked(), 0);
    assert!(!session.welcomed());
}

// REQ-INT-110; REQ-INT-137 as amended by REQ-RCH-016 (decision E.133):
// the round trip is no longer measured from acknowledgements.
#[test]
fn the_input_count_and_its_acknowledgement() {
    let mut session = welcomed(0);
    sent_item(&mut session, &Item::Text("ab"));
    assert_eq!(session.count(), 2, "HELLO, then one RULE_EVENT whatever text it carries (RULE-46)");
    sent_item(&mut session, &Item::Event(Event::Paste { on: ON, text: "a long paste" }));
    assert_eq!(session.count(), 3, "a paste counts one");
    sent_item(&mut session, &Item::Event(Event::FocusIn));
    assert_eq!(session.count(), 4);
    assert!(session.acknowledged(2));
    assert!(!session.acknowledged(1), "an older acknowledgement changes nothing");
    assert!(session.acknowledged(4));
    assert_eq!(session.acked(), 4);
    assert!(!session.acknowledged(5), "beyond what was sent: refused");
    assert_eq!(session.acked(), 4);
}

// REQ-INT-110.
#[test]
fn the_count_wraps_and_is_compared_as_a_serial_number() {
    let mut session = welcomed(0);
    for at in 0..u32::from(u16::MAX) {
        session.record(&Item::Event(Event::FocusIn), &[]);
        assert!(session.acknowledged(session.count()), "{at}");
    }
    assert_eq!(session.count(), 0, "65 536 units: wrapped");
    session.record(&Item::Text("ab"), &[]);
    assert!(session.acknowledged(2), "2 is after 0 across the wrap");
    assert!(!session.acknowledged(u16::MAX), "65 535 is before 2");
}

// RULE-46 (REQ-6528): a RULE_EVENT counts as one message for
// acknowledgement, whatever it carries, a move and a hover among them; the
// session's own events and the rule table's other two messages, which are
// no input, count nothing and are never acknowledged (REQ-INT-126).
#[test]
fn a_rule_event_counts_one_and_what_is_no_input_counts_nothing() {
    let mut session = welcomed(TOUCH_CAP | SELECT_CAP);
    let at = event::Point { column: 2, row: 3, pixels: None, mods: 0 };
    let mouse = |action| Item::Event(Event::Mouse(Mouse { on: ON, action, button: 0, at, device: Device::Touch }));
    let mut count = 1;
    for item in [
        mouse(MouseAction::Move),
        mouse(MouseAction::Press),
        Item::Event(Event::Hover { on: ON, leave: false, column: 2, row: 3 }),
        Item::Event(Event::Compose { on: ON, cursor: 0, text: "か" }),
        Item::Text("many characters"),
    ] {
        assert!(sent_item(&mut session, &item).is_some(), "{item:?} goes");
        count += 1;
        assert_eq!(session.count(), count, "{item:?} counts one");
    }
    let fired = RuleEvent { on: ON, generation: 1, rule: 63, tick: 7, target: Target::NONE, body: RuleBody::Fired };
    assert!(sent_item(&mut session, &Item::Event(Event::RuleEvent(fired))).is_some());
    assert_eq!(session.count(), count + 1, "a firing with no event counts one too");
    let fault = RulesFault { on: ON, generation: 1, fault: FaultCode::TickOverrun, count: 12 };
    let state = PageStateEvent { on: ON, body: &[0, 0, 0, 42, 0, 0, 0, 0, 0, 0] };
    for item in [
        Item::Event(Event::RulesFault(fault)),
        Item::Event(Event::PageState(state)),
        Item::Event(Event::Select(event::Select::new(0x200, 0x3F7F).unwrap(), "")),
    ] {
        assert!(sent_item(&mut session, &item).is_some(), "{item:?} goes");
        assert_eq!(session.count(), count + 1, "{item:?} counts nothing");
    }
}

// RULE-46, RULE-49 (REQ-6528): under protocol version 9 a receiver never
// writes a viewer's input at the top level of the stream.
#[test]
fn a_viewers_input_is_never_sent_bare() {
    let mut session = welcomed(TOUCH_CAP | SELECT_CAP);
    let at = event::Point { column: 2, row: 3, pixels: None, mods: 0 };
    let mut out = [0; crate::MAX_MESSAGE];
    for item in [
        Item::Text("a"),
        key(Action::Press, KeyCode::Character('a')),
        Item::Event(Event::Mouse(Mouse { on: ON, action: MouseAction::Move, button: 0, at, device: Device::Mouse })),
        Item::Event(Event::Wheel(event::Wheel { on: ON, dx: 0, dy: 1, at })),
        Item::Event(Event::Paste { on: ON, text: "p" }),
        Item::Event(Event::Compose { on: ON, cursor: 0, text: "" }),
        Item::Event(Event::Touch(Touch { on: ON, action: MouseAction::Press, id: 1, column: 2, row: 3, pixels: None })),
        Item::Event(Event::FocusIn),
        Item::Event(Event::FocusOut),
        Item::Event(Event::Hover { on: ON, leave: true, column: 2, row: 3 }),
        Item::Event(Event::DrawingHover { on: ON, leave: false, drawing: 4, commits: 0 }),
        Item::Event(Event::Pick(event::Pick {
            on: ON,
            drawing: 0,
            x: 0,
            y: 0,
            picture_x: 1,
            picture_y: 1,
            commits: 0,
        })),
    ] {
        assert!(event::is_page_input(&item), "{item:?}");
        assert_eq!(session.send(&item, 0, &mut out), Err(SendError::Input(InputError::Bare)), "{item:?}");
    }
    assert_eq!(session.count(), 1, "nothing went");
}

// REQ-INT-109, amended by decision E.133: a button's release and a contact's
// up always go too, and COMPOSE is held to the rate.
#[test]
fn the_rate_is_120_units_a_second_and_releases_always_go() {
    let mut session = Session::new(0);
    assert_eq!(session.allow(&Item::Text("ab"), 5), 2);
    let mut allowed = 0;
    for _ in 0..119 {
        allowed += session.allow(&Item::Text("a"), 5);
    }
    assert_eq!(allowed, 118, "120 units in all");
    let release = Item::Event(Event::Key(Key {
        on: ON,
        lost: false,
        action: Action::Release,
        key: KeyCode::Character('a'),
        mods: 0,
        text: None,
        base: None,
    }));
    assert_eq!(session.allow(&release, 5), 1, "a release beyond the rate");
    let at = event::Point { column: 1, row: 1, pixels: Some((12, 20)), mods: 0 };
    let mouse = |action, button| Item::Event(Event::Mouse(Mouse { on: ON, action, button, at, device: Device::Mouse }));
    assert_eq!(session.allow(&mouse(MouseAction::Release, 0), 5), 1, "a button's release beyond the rate");
    let up = Item::Event(Event::Touch(Touch {
        on: ON,
        action: MouseAction::Release,
        id: 1,
        column: 1,
        row: 1,
        pixels: None,
    }));
    assert_eq!(session.allow(&up, 5), 1, "a contact's up beyond the rate");
    assert_eq!(session.allow(&mouse(MouseAction::Press, 0), 5), 0, "a press is held to it");
    let compose = Item::Event(Event::Compose { on: ON, cursor: 0, text: "" });
    assert_eq!(session.allow(&compose, 5), 0, "so is COMPOSE, which nothing else bounds");
    assert_eq!(session.allow(&Item::Text("a"), 5 + 1_000), 1, "a second later");
    let mut limiter = Limiter::new();
    for _ in 0..MAX_PER_SECOND {
        assert!(limiter.allow(u32::MAX - 10, false));
    }
    assert!(!limiter.allow(u32::MAX, false), "the 121st unit");
    assert!(limiter.allow(u32::MAX, true), "a release across the clock's wrap");
    assert!(limiter.allow(1_000 - 11, false), "a second after, past the wrap");
    // RULE-46: the rate looks at what a RULE_EVENT carries, so a release
    // inside one always goes, and a firing with no event is held to it.
    let mut session = welcomed(0);
    for _ in 0..MAX_PER_SECOND {
        assert!(sent_item(&mut session, &key(Action::Press, KeyCode::Character('a'))).is_some());
    }
    assert_eq!(sent_item(&mut session, &key(Action::Press, KeyCode::Character('a'))), None, "the 121st unit");
    assert!(sent_item(&mut session, &key(Action::Release, KeyCode::Character('a'))).is_some(), "its release");
    let fired = RuleEvent { on: ON, generation: 1, rule: 0, tick: 0, target: Target::NONE, body: RuleBody::Fired };
    assert_eq!(sent_item(&mut session, &Item::Event(Event::RuleEvent(fired))), None, "a firing over the rate");
}

fn key(action: Action, key: KeyCode) -> Item<'static> {
    Item::Event(Event::Key(Key { on: ON, lost: false, action, key, mods: 0, text: Some("a"), base: Some('q') }))
}

/// Sends `item` as a receiver under protocol version 9 does: a viewer's
/// input as the session's capabilities carry it, inside a rule's
/// `RULE_EVENT` (RULE-46), and anything else as it is. Gives the bytes of
/// the input carried, or of the message for anything else, and `None`
/// when nothing went; `message` is given the whole message.
fn sent_as<const N: usize>(
    session: &mut SessionOf<N>,
    item: &Item<'_>,
    message: &mut std::vec::Vec<u8>,
) -> Option<std::vec::Vec<u8>> {
    let mut out = [0; crate::MAX_MESSAGE];
    let mut inner = [0; crate::MAX_MESSAGE];
    let (length, carried) = if event::is_page_input(item) {
        let carried = session.carried(item)?;
        let inner_length = write(&carried, &mut inner).unwrap();
        let body = RuleBody::Input(&inner[..inner_length]);
        let report = RuleEvent { on: ON, generation: 1, rule: 0, tick: 0, target: Target::NONE, body };
        (session.send(&Item::Event(Event::RuleEvent(report)), 0, &mut out).unwrap(), Some(inner_length))
    } else {
        (session.send(item, 0, &mut out).unwrap(), None)
    };
    *message = out[..length].to_vec();
    (length > 0).then(|| carried.map_or_else(|| message.clone(), |carried| inner[..carried].to_vec()))
}

fn sent_item(session: &mut Session, item: &Item<'_>) -> Option<std::vec::Vec<u8>> {
    sent_as(session, item, &mut std::vec::Vec::new())
}

// REQ-INT-106.
#[test]
fn nothing_is_sent_before_welcome() {
    let mut session = Session::new(0);
    opening(&mut session, 0, None);
    assert_eq!(session.send(&Item::Text("a"), 0, &mut [0; 8]), Err(SendError::NotWelcomed));
}

// REQ-INT-104, decision E.125: every press, repeat and release goes whole,
// with its text and its base; a base the same as its key is left out.
#[test]
fn every_key_goes_whole() {
    let mut session = welcomed(SELECT_CAP);
    let press = key(Action::Press, KeyCode::Character('a'));
    let release = key(Action::Release, KeyCode::Character('a'));
    assert_eq!(
        sent_item(&mut session, &press),
        Some(std::vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x30, b'a', 1, b'a', b'q']),
        "text and base"
    );
    assert!(sent_item(&mut session, &release).is_some(), "a release");
    assert_eq!(sent_item(&mut session, &Item::Text("é")), Some("é".as_bytes().to_vec()), "an input method's text");
    let same = Item::Event(Event::Key(Key {
        on: ON,
        lost: false,
        action: Action::Press,
        key: KeyCode::Character('q'),
        mods: 0,
        text: Some("q"),
        base: Some('q'),
    }));
    assert_eq!(
        sent_item(&mut session, &same),
        Some(std::vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x10, b'q', 1, b'q']),
        "no base the same as the key"
    );
}

// REQ-INT-116, decision E.125: a dead key and a key that only modifies go
// as every key does.
#[test]
fn a_dead_key_and_a_modifier_alone_go() {
    let functional = |name: &str| key(Action::Press, KeyCode::Functional(keys::code(name).unwrap()));
    let mut session = welcomed(SELECT_CAP);
    for name in ["Dead", "ShiftLeft", "AltGraph", "CapsLock", "Enter"] {
        assert!(sent_item(&mut session, &functional(name)).is_some(), "{name}");
    }
}

// REQ-INT-115, amended 27 October 2026 (decision E.125): every press,
// release and move, and the wheel, go with the cell and the dot.
#[test]
fn the_mouse_and_the_wheel_always_go_with_their_dot() {
    let mut session = welcomed(SELECT_CAP);
    let at = event::Point { column: 10, row: 5, pixels: Some((120, 100)), mods: 0 };
    let click =
        Item::Event(Event::Mouse(Mouse { on: ON, action: MouseAction::Press, button: 0, at, device: Device::Mouse }));
    let wheel = Item::Event(Event::Wheel(event::Wheel { on: ON, dx: 0, dy: 120, at }));
    let drag =
        Item::Event(Event::Mouse(Mouse { on: ON, action: MouseAction::Move, button: 1, at, device: Device::Mouse }));
    assert_eq!(
        sent_item(&mut session, &click),
        Some(std::vec![0xFF, 0x02, 0x80, 0x02, 0x00, 0x04, 0x00, 0x0A, 0x05, 0x78, 0x64]),
        "the cell and the dot"
    );
    assert_eq!(sent_item(&mut session, &wheel).map(|bytes| bytes.len()), Some(13), "the wheel, with its dot");
    assert!(sent_item(&mut session, &drag).is_some(), "a drag");
}

// REQ-205, DOC-1922, DOC-1753: SELECT goes only when both sides set it. A
// receiver that did not is shown the program's answer for the channel's
// initial page, so it has nothing to say.
#[test]
fn select_goes_only_when_both_sides_set_it() {
    let select = Item::Event(Event::Select(event::Select::new(0x200, 0x3F7F).unwrap(), ""));
    assert_eq!(sent_item(&mut welcomed(0), &select), None, "agreed by neither side");
    assert_eq!(sent_item(&mut welcomed(TOUCH_CAP), &select), None, "only another capability agreed");
    let mut offered_and_not_agreed = Session::new(SELECT_CAP);
    offered_and_not_agreed.open(0, None, &mut [0; crate::MAX_MESSAGE]).unwrap();
    offered_and_not_agreed.welcome(VERSION, 0, None).unwrap();
    assert_eq!(sent_item(&mut offered_and_not_agreed, &select), None, "offered, and the channel did not agree");
    assert!(sent_item(&mut welcomed(SELECT_CAP), &select).is_some(), "agreed by both");
}

// REQ-INT-106, decision E.125: COMPOSE and the rest always go, with or
// without a capability.
#[test]
fn compose_always_goes() {
    let compose = Item::Event(Event::Compose { on: ON, cursor: 1, text: "か" });
    let mut session = welcomed(0);
    assert!(sent_item(&mut session, &compose).is_some());
    assert!(sent_item(&mut session, &Item::Event(Event::FocusOut)).is_some());
}

// REQ-INT-124, REQ-RCH-041, REQ-RCH-042 (decision E.133): without TOUCH a channel sees
// nothing a mouse would not send: no TOUCH, and MOUSE's device as the
// mouse's; with it, both go.
#[test]
fn touch_goes_only_when_both_sides_set_it() {
    let at = event::Point { column: 10, row: 5, pixels: None, mods: 0 };
    let tap =
        Item::Event(Event::Mouse(Mouse { on: ON, action: MouseAction::Press, button: 0, at, device: Device::Touch }));
    let second = Item::Event(Event::Touch(Touch {
        on: ON,
        action: MouseAction::Press,
        id: 1,
        column: 10,
        row: 5,
        pixels: None,
    }));
    let mut plain = welcomed(0);
    assert_eq!(
        sent_item(&mut plain, &tap),
        Some(std::vec![0xFF, 0x02, 0x80, 0x02, 0x00, 0x00, 0x00, 0x0A, 0x05]),
        "as the mouse"
    );
    assert_eq!(sent_item(&mut plain, &second), None, "no TOUCH");
    let mut touch = welcomed(TOUCH_CAP);
    assert_eq!(
        sent_item(&mut touch, &tap),
        Some(std::vec![0xFF, 0x02, 0x80, 0x02, 0x00, 0x10, 0x00, 0x0A, 0x05]),
        "device 1"
    );
    assert_eq!(
        sent_item(&mut touch, &second),
        Some(std::vec![0xFF, 0x44, 0x07, 0x80, 0x02, 0x00, 0x00, 0x01, 0x0A, 0x05])
    );
}

const TOKEN: [u8; TOKEN_BYTES] = [0x5A; TOKEN_BYTES];

/// A resumable session welcomed as a new one with RESUME and `TOKEN`.
fn resumable() -> ResumableSession {
    let mut session = ResumableSession::new(SELECT_CAP);
    let first = opening(&mut session, 0, None);
    assert_eq!(
        first,
        [Event::Hello { version: VERSION, caps: SELECT_CAP | RESUME_CAP, agent: None }],
        "RESUME offered"
    );
    let resume = Some(Resumption { token: TOKEN, taken: None });
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Ok(Welcomed::New));
    session
}

/// Sends `text` inside a rule's `RULE_EVENT`, and gives the message.
fn text(session: &mut ResumableSession, text: &str) -> std::vec::Vec<u8> {
    let mut message = std::vec::Vec::new();
    sent_as(session, &Item::Text(text), &mut message).unwrap();
    message
}

fn resent(session: &mut ResumableSession) -> std::vec::Vec<std::vec::Vec<u8>> {
    let mut messages = std::vec::Vec::new();
    session.resend(&mut |message| messages.push(message.to_vec()));
    messages
}

// REQ-INT-125, REQ-RCH-013 (decision E.133): after a reconnection the session is asked
// for with its token and the last acknowledgement; a channel that held it
// says what it had taken, and the receiver sends again, in order, only
// what came after, before anything else; the count goes on.
#[test]
fn a_resumed_session_sends_again_only_what_the_channel_had_not_taken() {
    let mut session = resumable();
    text(&mut session, "a"); // 2
    let second = text(&mut session, "bc"); // 3
    let third = text(&mut session, "d"); // 4
    let select = Item::Event(Event::Select(event::Select::new(0x200, 0x3F7F).unwrap(), ""));
    assert!(sent_any(&mut session, &select), "what counts nothing goes, and is never kept");
    assert!(session.acknowledged(2));
    session.close();
    assert!(session.resumable());
    let first = opening(&mut session, 100, None);
    assert_eq!(
        first,
        [
            Event::Hello { version: VERSION, caps: SELECT_CAP | RESUME_CAP, agent: None },
            Event::Resume(Resume { token: TOKEN, acked: 2 }),
        ]
    );
    let next = [0x77; TOKEN_BYTES];
    let resume = Some(Resumption { token: next, taken: Some(3) });
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Ok(Welcomed::Resumed));
    assert_eq!(session.send(&select, 0, &mut [0; 8]), Err(SendError::ResendFirst), "the order kept");
    assert_eq!(resent(&mut session), core::slice::from_ref(&third), "only what came after 3");
    assert_eq!(session.count(), 4, "the count goes on");
    let fourth = text(&mut session, "e");
    assert_eq!(session.count(), 5);
    // Taken from what was acknowledged: everything kept goes again.
    session.close();
    opening(&mut session, 200, None);
    let resume = Some(Resumption { token: next, taken: Some(2) });
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Ok(Welcomed::Resumed));
    assert_eq!(resent(&mut session), [second, third, fourth]);
}

fn sent_any(session: &mut ResumableSession, item: &Item<'_>) -> bool {
    sent_as(session, item, &mut std::vec::Vec::new()).is_some()
}

// REQ-INT-125, REQ-RCH-013, REQ-INT-106: a channel that did not hold the session, or
// that does not set RESUME, begins a new one: the count from HELLO and
// nothing sent again; a count taken that no message ends at is refused,
// and the session forgotten.
#[test]
fn a_session_not_held_is_a_new_one_and_a_false_count_is_refused() {
    let mut session = resumable();
    text(&mut session, "ab");
    session.close();
    opening(&mut session, 10, None);
    let resume = Some(Resumption { token: [1; TOKEN_BYTES], taken: None });
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Ok(Welcomed::New));
    assert_eq!(session.count(), 1, "from HELLO");
    assert_eq!(resent(&mut session).len(), 0, "nothing sent again");
    text(&mut session, "ab");
    session.close();
    opening(&mut session, 20, None);
    assert_eq!(session.welcome(VERSION, SELECT_CAP, None), Ok(Welcomed::New), "a channel without RESUME");
    assert_eq!(session.count(), 1);
    session.close();
    assert!(!session.resumable(), "no token: forgotten on close");
    let mut session = resumable();
    text(&mut session, "ab"); // 2
    session.close();
    opening(&mut session, 30, None);
    let resume = Some(Resumption { token: TOKEN, taken: Some(3) });
    assert_eq!(
        session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume),
        Err(WelcomeError::Resume),
        "a count no message ends at"
    );
    assert!(!session.resumable());
    let mut session = resumable();
    let resume = Some(Resumption { token: TOKEN, taken: Some(1) });
    session.close();
    opening(&mut session, 40, None);
    session.forget();
    opening(&mut session, 41, None);
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Err(WelcomeError::Resume), "not asked");
}

// REQ-INT-125, REQ-RCH-013: what is kept is bounded; a message that does not fit, or
// more unacknowledged messages than are remembered, end the session's
// chance to resume, and the next connection is a new session.
#[test]
fn what_is_kept_is_bounded() {
    let mut session = resumable();
    let paste: std::string::String = core::iter::repeat_n('x', crate::MAX_PASTE).collect();
    for _ in 0..RESEND_BYTES / crate::MAX_PASTE - 1 {
        assert!(sent_any(&mut session, &Item::Event(Event::Paste { on: ON, text: &paste })));
        assert!(session.resumable());
    }
    assert!(sent_any(&mut session, &Item::Event(Event::Paste { on: ON, text: &paste })));
    assert!(sent_any(&mut session, &Item::Event(Event::Paste { on: ON, text: &paste })), "one too many to keep");
    assert!(!session.resumable());
    session.close();
    assert_eq!(opening(&mut session, 0, None).len(), 1, "HELLO alone: a new session");
    let mut session = resumable();
    for _ in 0..REMEMBERED {
        session.record(&Item::Event(Event::FocusIn), &[0xFF, 0x05]);
    }
    assert!(session.resumable());
    session.record(&Item::Event(Event::FocusIn), &[0xFF, 0x05]);
    assert!(!session.resumable(), "past the ring");
}

// REQ-INT-125, REQ-RCH-013: a plain session never offers RESUME.
#[test]
fn a_session_that_keeps_nothing_never_offers_resume() {
    let mut session = Session::new(SELECT_CAP | RESUME_CAP);
    assert_eq!(opening(&mut session, 0, None), [Event::Hello { version: VERSION, caps: SELECT_CAP, agent: None }]);
    let resume = Some(Resumption { token: TOKEN, taken: None });
    assert_eq!(session.welcome(VERSION, SELECT_CAP | RESUME_CAP, resume), Err(WelcomeError::Caps));
}
