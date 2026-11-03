#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::terminal_io::{Wakened, next_event};
use super::typing::{Typing, input_of};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use neoteletext_input::event::Stamp;
use std::sync::mpsc;
use std::time::{Duration, Instant as StdInstant};

#[test]
fn a_page_from_the_source_ends_the_wait_at_once() {
    // The viewer looked at the source only between 40 ms waits for the
    // terminal, so a page the fetcher had in 0.3 ms waited up to 40 ms
    // more to be drawn (REQ-INT-008).
    const LONG: Duration = Duration::from_secs(5);
    let (woken, wakes) = mpsc::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        let _ = woken.send(Wakened::Source);
    });
    let started = StdInstant::now();
    assert!(next_event(&wakes, LONG).is_none());
    assert!(started.elapsed() < LONG / 10, "waited {:?}", started.elapsed());
}

/// The page on screen in these tests: 100, sub-code 0000.
const ON: Stamp = Stamp { page: 0x100, sub: 0 };

fn sent(code: KeyCode, modifiers: KeyModifiers) -> Option<Vec<u8>> {
    let mut buffer = [0_u8; 4];
    let item = input_of(KeyEvent::new(code, modifiers), ON, &mut buffer)?;
    let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
    let length = neoteletext_input::event::write(&item, &mut out).unwrap();
    Some(out[..length].to_vec())
}

/// REQ-INT-104 (amended by decision E.125): every key as KEY, a character
/// key unshifted with its text as typed, a Control combination and a named
/// key with no text; REQ-INT-116: a non-United-States layout's letters in
/// any script, and AltGr as text with no modifiers.
#[test]
fn a_terminal_key_is_sent_as_the_input_stream_has_it() {
    let none = KeyModifiers::NONE;
    assert_eq!(sent(KeyCode::Char('a'), none), Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x10, b'a', 1, b'a']));
    assert_eq!(
        sent(KeyCode::Char('A'), KeyModifiers::SHIFT),
        Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x18, b'a', 0x01, 1, b'A']),
        "the key unshifted, Shift in mods, the text as typed"
    );
    assert_eq!(
        sent(KeyCode::Char('c'), KeyModifiers::CONTROL),
        Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x08, b'c', 0x04])
    );
    assert_eq!(sent(KeyCode::Enter, none), Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x04, 0x02]));
    assert_eq!(sent(KeyCode::Up, KeyModifiers::SHIFT), Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x0C, 0x09, 0x01]));
    assert_eq!(sent(KeyCode::BackTab, none), Some(vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x0C, 0x03, 0x01]), "Shift-Tab");
    // `key` is the code point as a varint, `text` the UTF-8 with its length.
    let typed = |character: char| {
        let mut bytes = vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x10];
        let mut point = u32::from(character);
        while point >= 0x80 {
            bytes.push(u8::try_from(point & 0x7F).unwrap() | 0x80);
            point >>= 7;
        }
        bytes.push(u8::try_from(point).unwrap());
        bytes.push(u8::try_from(character.len_utf8()).unwrap());
        bytes.extend(character.to_string().as_bytes());
        bytes
    };
    assert_eq!(sent(KeyCode::Char('й'), none), Some(typed('й')), "Russian");
    assert_eq!(sent(KeyCode::Char('ש'), none), Some(typed('ש')), "Hebrew");
    assert_eq!(sent(KeyCode::Char('é'), none), Some(typed('é')), "French, composed by the terminal");
    let alt_gr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    assert_eq!(sent(KeyCode::Char('@'), alt_gr), Some(typed('@')), "German AltGr-Q, as Windows reports it");
    assert_eq!(sent(KeyCode::Char('\u{1b}'), none), None, "a control character is not text");
}

/// `docs/user-guide.md`, "The viewer": with the Kitty keyboard protocol a key's repeat
/// types its text again and its release is a KEY with no text.
#[test]
fn repeats_and_releases_where_the_terminal_reports_them() {
    let event = |kind| KeyEvent::new_with_kind(KeyCode::Char('a'), KeyModifiers::NONE, kind);
    let encode = |key: KeyEvent| {
        let mut buffer = [0_u8; 4];
        let item = input_of(key, ON, &mut buffer).unwrap();
        let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
        let length = neoteletext_input::event::write(&item, &mut out).unwrap();
        out[..length].to_vec()
    };
    assert_eq!(encode(event(KeyEventKind::Repeat)), vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x11, b'a', 1, b'a']);
    assert_eq!(
        encode(event(KeyEventKind::Release)),
        vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x02, b'a'],
        "a release, no text"
    );
    let up = KeyEvent::new_with_kind(KeyCode::Up, KeyModifiers::NONE, KeyEventKind::Release);
    assert_eq!(encode(up), vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x06, 0x09]);
}

/// REQ-INT-113: a REDIRECT record names the page to open, whether or not
/// keys go to the channel, once.
/// REQ-1502: a program's page redirect selects the page it names.
#[test]
fn a_redirect_names_the_page_to_open() {
    let mut typing = Typing::new();
    assert!(!typing.on());
    typing.redirected(0x301, 0x0002, None);
    assert_eq!(typing.take_redirect(), Some((0x301, 0x0002)));
    assert_eq!(typing.take_redirect(), None);
}

/// DOC-1954 and DEC-1689: a cursor record never moves a viewer,
/// whatever page it names, so one still in flight when the viewer selects
/// another page cannot take them back (TODO-1656).
/// REQ-1502: only a selection or a redirect moves a connection's viewer.
#[test]
fn a_cursor_record_never_moves_a_viewer() {
    let mut source = Recorded::default();
    let mut typing = welcomed(&mut source);
    typing.tell_selection(&mut source, 0x200, 0x3F7F, 0);
    let late = neoteletext_input::cursor::Cursor { page: 0x100, acked: 0, place: None, flags: 0 }.packet(3, 1).unwrap();
    typing.records(&mut source, [late].into_iter(), 0);
    assert_eq!(typing.take_redirect(), None);
}

/// REQ-1693 item 6: the address's page parameters go with the selection it
/// named, and any other selection leaves them behind for good.
#[test]
fn page_parameters_go_with_the_selection_alone() {
    use neoteletext_input::event::{Event, Item, Select, read_message};
    let selections = |source: &Recorded| {
        let mut found = Vec::new();
        for message in &source.0 {
            read_message(message, &mut |item| {
                if let Item::Event(Event::Select(select, parameters)) = item {
                    found.push((select.page, select.sub, parameters.to_owned()));
                }
            })
            .unwrap();
        }
        found
    };
    let any = Select::new(0x302, 0x3F7F).unwrap().sub;
    let mut source = Recorded::default();
    let mut typing = welcomed(&mut source);
    typing.opened_with((0x302, any), "a=1&b=2".to_owned());
    typing.tell_selection(&mut source, 0x302, any, 0);
    typing.tell_selection(&mut source, 0x303, any, 0);
    typing.tell_selection(&mut source, 0x302, any, 0);
    assert_eq!(
        selections(&source),
        [(0x302, any, "a=1&b=2".to_owned()), (0x303, any, String::new()), (0x302, any, String::new())]
    );
}

/// REQ-INT-110 as amended by REQ-RCH-012 (decision E.133): the channel's
/// acknowledgement is taken where it comes, in the answer's own frame
/// among the page's packets, and alone.
#[test]
fn an_acknowledgement_is_taken_in_the_answers_frame_and_alone() {
    use neoteletext_core::packet::{PacketMeta, encode};
    use neoteletext_core::types::Magazine;
    use neoteletext_input::cursor::{Cursor, from_packet};
    use neoteletext_input::event::{Event, Item};
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    session.keep(neoteletext_input::is_input_packet);
    let cursor = |acked| Cursor { page: 0x100, acked, place: None, flags: 0 }.packet(3, 1).unwrap();
    let magazine = Magazine::new(1).unwrap();
    // The answer: the page's header and a row, and the acknowledgement in
    // the same message.
    for packet in [encode::address(magazine, 0), encode::row(magazine, 1, &[0x20; 40]), cursor(2)] {
        session.push(&packet, &PacketMeta::default());
    }
    // A channel that published nothing: the acknowledgement alone.
    session.push(&cursor(3), &PacketMeta::default());
    let mut input = neoteletext_input::session::Session::new(0);
    let mut out = [0_u8; neoteletext_input::MAX_MESSAGE];
    input.open(0, None, &mut out).unwrap();
    input.welcome(neoteletext_input::VERSION, 0, None).unwrap();
    input.send(&Item::Event(Event::FocusIn), 0, &mut out).unwrap();
    input.send(&Item::Event(Event::FocusOut), 0, &mut out).unwrap();
    let mut taken = Vec::new();
    for packet in session.take_kept() {
        let acked = from_packet(&packet).unwrap().unwrap().acked;
        taken.push((acked, input.acknowledged(acked)));
    }
    assert_eq!(taken, [(2, true), (3, true)], "both kept and taken, and nothing else of the answer");
    assert_eq!(input.acked(), 3);
}

/// What a channel is sent.
#[derive(Default)]
struct Recorded(Vec<Vec<u8>>);

impl neoteletext_app::source::Source for Recorded {
    fn poll(
        &mut self,
        _session: &mut neoteletext_app::session::Session,
        _now: neoteletext_core::types::Instant,
    ) -> bool {
        false
    }
    fn send_input(&mut self, message: &[u8]) {
        self.0.push(message.to_vec());
    }
}

/// A session the channel welcomed, what it was sent so far forgotten.
fn welcomed(source: &mut Recorded) -> Typing {
    use neoteletext_input::caps::SELECT_CAP;
    use neoteletext_input::control::{Control, packet_of};
    let mut typing = Typing::new();
    typing.connection(source, true, 0);
    let welcome = Control::Welcome { version: neoteletext_input::VERSION, caps: SELECT_CAP, resume: None };
    let packet = packet_of(&welcome, 1, 1).unwrap();
    assert!(!typing.records(source, std::iter::once(packet), 0));
    source.0.clear();
    typing
}

/// `character` as a press stamped with what is drawn.
fn press(typing: &mut Typing, source: &mut Recorded, character: char, kind: KeyEventKind) {
    let on = typing.stamp().unwrap();
    let mut buffer = [0_u8; 4];
    let mut key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE);
    key.kind = kind;
    typing.send(source, &input_of(key, on, &mut buffer).unwrap(), 0);
}

/// REQ-1556, REQ-1090: opening a page sends SELECT, never 0x0A, with 3F7F
/// for the page alone and the sub-code for a sub-page, once a change.
/// REQ-1502: a page alone goes as 3F7F's text, and a sub-page with its sub-code.
#[test]
fn opening_a_page_sends_select_and_never_page() {
    let mut source = Recorded::default();
    let mut typing = welcomed(&mut source);
    typing.tell_selection(&mut source, 0x100, 0x3F7F, 0);
    typing.tell_selection(&mut source, 0x100, 0x3F7F, 0);
    typing.tell_selection(&mut source, 0x100, 0x0002, 0);
    assert_eq!(
        source.0,
        [[&[0xFF, 0x40, 3][..], b"100"].concat(), [&[0xFF, 0x40, 8][..], b"100/0002"].concat()],
        "the page, then its sub-page, and no message of type 0x0A"
    );
}

/// REQ-1555, REQ-1554: a key is stamped with the page drawn, whatever was
/// selected since, and no key goes while nothing is drawn.
#[test]
fn a_key_is_stamped_with_the_page_drawn() {
    let mut source = Recorded::default();
    let mut typing = welcomed(&mut source);
    typing.tell_selection(&mut source, 0x100, 0x3F7F, 0);
    assert_eq!(typing.stamp(), None, "nothing drawn yet");
    typing.set_drawn(Some(ON));
    typing.tell_selection(&mut source, 0x200, 0x3F7F, 0);
    source.0.clear();
    press(&mut typing, &mut source, 'a', KeyEventKind::Press);
    assert_eq!(source.0[0][2..5], [0x80, 0x02, 0x00], "page 100 is still on screen");
}

/// An ordinary terminal's `Control-]` is the byte 0x1D, which reaches the
/// viewer as Control and 5; a viewer it did not free could not leave at all.
#[test]
fn control_right_bracket_leaves_as_any_terminal_reports_it() {
    use super::keys::leaves_typing;
    assert!(leaves_typing(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)));
    assert!(leaves_typing(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::CONTROL)));
    assert!(!leaves_typing(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE)));
    assert!(!leaves_typing(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)));
    let release = KeyEvent::new_with_kind(KeyCode::Char(']'), KeyModifiers::CONTROL, KeyEventKind::Release);
    assert!(!leaves_typing(release));
}

/// REQ-1554, REQ-207: after `Control-]` no key goes, `FOCUS_OUT` has gone, and
/// the selection stays; a held key goes first, lost, where the terminal
/// reports releases.
#[test]
fn after_leaving_keys_stop_and_focus_goes() {
    let mut source = Recorded::default();
    let mut typing = welcomed(&mut source);
    typing.set_drawn(Some(ON));
    typing.tell_selection(&mut source, 0x100, 0x3F7F, 0);
    press(&mut typing, &mut source, 'a', KeyEventKind::Press);
    press(&mut typing, &mut source, 'a', KeyEventKind::Release);
    press(&mut typing, &mut source, 'b', KeyEventKind::Press);
    source.0.clear();
    assert!(typing.leave_or_return(&mut source, 0));
    assert_eq!(
        source.0,
        [vec![0xFF, 0x01, 0x80, 0x02, 0x00, 0x42, b'b'], vec![0xFF, 0x06]],
        "the held key released as lost, then FOCUS_OUT, and no SELECT"
    );
    assert_eq!(typing.stamp(), None, "no key goes");
}

/// REQ-290, DEC-2363: `s` is the sub-page key, and digits stay page digits.
#[test]
fn s_is_the_sub_page_key() {
    use neoteletext_core::viewer::Key;
    let key = |code| super::keys::map_key(KeyEvent::new(code, KeyModifiers::NONE));
    assert_eq!(key(KeyCode::Char('s')), Some(Key::SubPage));
    assert_eq!(key(KeyCode::Char('7')), Some(Key::Digit(7)));
}

/// A packet 8/30 format 1 naming `mpp` as the initial page: it shares
/// X/27/0's page reference (Figure 12), with an absolute magazine.
fn service_packet(mpp: u16) -> neoteletext_core::packet::Packet {
    use neoteletext_core::consts::{PACKET_SERVICE, SERVICE_MAGAZINE};
    use neoteletext_core::packet::encode;
    use neoteletext_core::types::{Magazine, PageAddress, SubCode};
    let target = PageAddress::from_mpp(mpp).unwrap();
    let here = Magazine::new(SERVICE_MAGAZINE).unwrap();
    // Link 0's magazine is written relative to the packet's, which 8/30
    // reads as absolute: give the bits the target's magazine has.
    let relative = Magazine::from_bits(target.magazine.bits() ^ here.bits());
    let links = [Some((relative, target.page, SubCode::ANY)), None, None, None, None, None];
    let mut packet = encode::links(Magazine::from_bits(0), &links, true, 0);
    packet.bytes[..2].copy_from_slice(&encode::address(here, PACKET_SERVICE).bytes[..2]);
    packet
}

fn view_state(session: &neoteletext_app::session::Session) -> super::ViewState {
    let opening = neoteletext_app::input::Opening { page: None, parameters: String::new() };
    super::ViewState::new(session, StdInstant::now(), opening)
}

/// REQ-458 (REQ-SVC-004), DEC-2410: an 8/30 that arrives on a live source
/// after the view started moves an untouched viewer to its initial page
/// and makes it the Index and Home keys' target; a viewer the user has
/// moved stays, with only the target changed; a channel's own initial
/// page stands over 8/30.
#[test]
fn a_late_8_30_gives_the_initial_page() {
    use neoteletext_core::packet::PacketMeta;
    use neoteletext_core::types::{PageAddress, SubCode};
    use neoteletext_core::viewer::{Viewer, ViewerEvent};
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut state = view_state(&session);
    assert_eq!((state.initial, state.viewer.requested().0), (page(0x100), page(0x100)));
    session.push(&service_packet(0x200), &PacketMeta::default());
    state.follow_service_initial(&session);
    assert_eq!((state.initial, state.viewer.requested()), (page(0x200), (page(0x200), SubCode::ANY)));
    // The user has moved: only the target changes.
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut state = view_state(&session);
    state.viewer = Viewer::new(page(0x300));
    let _ = state.viewer.event(ViewerEvent::Started);
    session.push(&service_packet(0x200), &PacketMeta::default());
    state.follow_service_initial(&session);
    assert_eq!((state.initial, state.viewer.requested().0), (page(0x200), page(0x300)));
    // A channel's descriptor named its initial page: 8/30 does not move it.
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut state = view_state(&session);
    let mut recorded = Recorded::default();
    state.take_notice(neoteletext_app::pipe::Notice::InitialPage(0x150), &mut recorded);
    session.push(&service_packet(0x200), &PacketMeta::default());
    state.follow_service_initial(&session);
    assert_eq!((state.initial, state.viewer.requested().0), (page(0x150), page(0x150)));
}

/// REQ-245, REQ-292: the terminal text zooms as the picture does: the top
/// half's rows 0 to 11 each on two lines then row 12 once, the bottom
/// half's rows 12 to 23 then row 24, which leaves row 24 off the screen at
/// the top half; with no zoom the text is as it was.
#[test]
fn the_text_output_zooms_as_the_picture_does() {
    use neoteletext_core::level1::Zoom;
    let text: String = (0..25).map(|row| format!("row {row}")).collect::<Vec<_>>().join("\n") + "\n";
    let lines = |zoom| super::zoomed_text(&text, zoom).lines().map(str::to_owned).collect::<Vec<_>>();
    let expect = |first: usize| {
        let mut rows: Vec<String> =
            (first..first + 12).flat_map(|row| [format!("row {row}"), format!("row {row}")]).collect();
        rows.push(format!("row {}", first + 12));
        rows
    };
    assert_eq!(lines(Zoom::Top), expect(0));
    assert_eq!(lines(Zoom::Bottom), expect(12));
    assert!(!lines(Zoom::Top).contains(&"row 24".to_owned()), "row 24 is off the screen");
    assert_eq!(super::zoomed_text(&text, Zoom::Off), text);
}

/// REQ-142 (REQ-FSM-010), TODO-1111: with `-vv` the viewer writes the
/// machines' transitions to standard error when it is not the terminal,
/// keeps none from turn to turn, and when it is the terminal drops them
/// and says so at the end.
#[test]
fn the_viewer_writes_the_trace_where_it_does_not_spoil_the_page() {
    use neoteletext_core::config::Config;
    use neoteletext_core::packet::{PacketMeta, encode};
    use neoteletext_core::types::{ControlBits, Magazine, PageNumber, SubCode};
    let header =
        encode::header(Magazine::new(1).unwrap(), PageNumber(0), SubCode::new(0), ControlBits(0), &[0x20; 32]).unwrap();
    let traced = Config { trace: true, ..Config::default() };
    let mut session = neoteletext_app::session::Session::new(traced, 8);
    let mut state = view_state(&session);
    session.push(&header, &PacketMeta::default());
    let mut written = Vec::new();
    state.write_trace(&mut session, &mut written, false);
    let text = String::from_utf8(written).unwrap();
    assert!(text.lines().any(|line| line.starts_with("neottx: Trace(")), "{text}");
    // The viewer's own machine is in it: its start, from the state's making.
    assert!(
        text.lines().any(|line| line.contains("machine: Viewer") && line.contains("event: \"decoder started\"")),
        "{text}"
    );
    assert!(session.trace.is_empty(), "nothing is kept");
    session.push(&header, &PacketMeta::default());
    let mut written = Vec::new();
    state.write_trace(&mut session, &mut written, true);
    assert_eq!((written.len(), session.trace.len()), (0, 0));
    assert!(state.into_messages().iter().any(|message| message.contains("2>trace.txt")));
    // Without -vv nothing is kept or written.
    let mut session = neoteletext_app::session::Session::new(Config::default(), 8);
    let mut state = view_state(&session);
    session.push(&header, &PacketMeta::default());
    let mut written = Vec::new();
    state.write_trace(&mut session, &mut written, false);
    assert_eq!(written, b"");
}

/// A source of a service linking to others, as a descriptor lists them.
#[derive(Default)]
struct Linking(Vec<neoteletext_app::pipe::ServiceEntry>);

impl neoteletext_app::source::Source for Linking {
    fn poll(
        &mut self,
        _session: &mut neoteletext_app::session::Session,
        _now: neoteletext_core::types::Instant,
    ) -> bool {
        true
    }

    fn linked_service(&self, service: u8) -> Option<neoteletext_app::pipe::ServiceEntry> {
        self.0.get(usize::from(service)).cloned()
    }
}

fn linked(short_name: &str, display_name: &str, origin: &str) -> neoteletext_app::pipe::ServiceEntry {
    neoteletext_app::pipe::ServiceEntry {
        short_name: short_name.into(),
        display_name: display_name.into(),
        origin: origin.into(),
    }
}

// Verifies REQ-288: the question names the service as its descriptor gives
// it and its origin, with the whole instruction; following the link
// confirms the origin for the session, so a new service's notice keeps it,
// and another origin under the same short name in the next descriptor is
// asked about again.
#[test]
fn a_confirmed_origin_lasts_the_session_and_another_is_asked() {
    use neoteletext_core::nav::LinkTarget;
    use neoteletext_core::types::{PageAddress, SubCode};
    use neoteletext_core::viewer::Action;
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut state = view_state(&session);
    let mut source = Linking(vec![linked("weather", "The Weather Channel", "https://weather.example")]);
    let target = LinkTarget { address: PageAddress::from_mpp(0x100).unwrap(), subcode: SubCode::ANY, service: Some(0) };
    state.action = Action::ConfirmService(target);
    state.act(&mut session, &mut source);
    assert_eq!(
        state.prompt,
        Some(neoteletext_app::interact::leave_prompt("The Weather Channel", "https://weather.example"))
    );
    assert!(!state.origin_confirmed(&source, 0));
    state.action = Action::FollowService(target);
    state.act(&mut session, &mut source);
    assert!(state.origin_confirmed(&source, 0));
    let mut recorded = Recorded::default();
    state.take_notice(neoteletext_app::pipe::Notice::Service(source.0.clone()), &mut recorded);
    assert!(state.origin_confirmed(&source, 0), "kept across a new service");
    let other = Linking(vec![linked("weather", "Other weather", "https://other.example")]);
    assert!(!state.origin_confirmed(&other, 0), "the same short name at another origin");
}

// Verifies REQ-508: a bundle's first page is shown while the viewer still
// searches for the page it started on, and not once the address opened
// named a page or the page asked for is shown.
#[test]
fn a_bundles_first_page_is_shown_while_the_viewer_has_not_moved() {
    use neoteletext_app::pipe::Notice;
    use neoteletext_core::types::PageAddress;
    use neoteletext_core::viewer::ViewerEvent;
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut recorded = Recorded::default();
    let mut state = view_state(&session);
    state.take_notice(Notice::Bundle(0x199), &mut recorded);
    assert_eq!(state.viewer.requested().0, page(0x199));
    let mut state = view_state(&session);
    state.named = true;
    state.take_notice(Notice::Bundle(0x199), &mut recorded);
    assert_eq!(state.viewer.requested().0, page(0x100), "the address named its page");
    let mut state = view_state(&session);
    let _ = state.viewer.event(ViewerEvent::PagePresent);
    state.take_notice(Notice::Bundle(0x199), &mut recorded);
    assert_eq!(state.viewer.requested().0, page(0x100), "the page asked for is shown");
}

/// REQ-382 (TODO-2070 item 5): with kitty, each frame of a page is sent
/// once; flashing back and forth between two frames afterwards only hides
/// one image and places the other, and a new page frees the old images.
#[test]
fn kitty_sends_each_flash_phase_once_and_switches_between_them() {
    use super::{Frame, Grid, KittyImages};
    let config = neoteletext_core::config::Config::default();
    let on = Frame::still(&config, false);
    let off = Frame { flash_visible: false, ..on };
    let mut images = KittyImages::default();
    let mut out = Vec::new();
    let png = |frame: Frame| Ok(if frame.flash_visible { b"ON".to_vec() } else { b"OFF".to_vec() });
    for frame in [on, off, on, off, on] {
        images.show(&mut out, &Grid::EMPTY, frame, png).unwrap();
    }
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("a=t,").count(), 2, "two transmissions: {text:?}");
    assert_eq!(text.matches("a=p,").count(), 5, "a placement each frame");
    assert_eq!(text.matches("a=d,d=i,").count(), 4, "the other frame hidden each switch");
    let mut out = Vec::new();
    let mut other = Grid::EMPTY;
    other.boxed_mode = true;
    images.show(&mut out, &other, on, png).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.matches("a=d,d=I,").count(), 2, "both old images freed: {text:?}");
    assert_eq!(text.matches("a=t,").count(), 1);
}
