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

/// A grid showing `text` on row `row`.
fn grid_showing(row: usize, text: &[u8]) -> super::Grid {
    use neoteletext_core::charset::{CharRef, CharSet, G0, G0Set, NationalOption};
    use neoteletext_core::level1::Glyph;
    let mut grid = super::Grid::EMPTY;
    let g0 = CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::English) });
    for (cell, &byte) in grid.cells[row].iter_mut().zip(text) {
        cell.glyph = Glyph::Char(CharRef::new(g0, byte));
    }
    grid
}

/// The height a Sixel image declares in its raster attributes, `"1;1;W;H`.
fn sixel_heights(output: &[u8]) -> Vec<usize> {
    let text = String::from_utf8_lossy(output);
    text.split("\"1;1;")
        .skip(1)
        .map(|rest| {
            let height: String = rest.split(';').nth(1).unwrap().chars().take_while(char::is_ascii_digit).collect();
            height.parse().unwrap()
        })
        .collect()
}

/// REQ-382 (REQ-RND-029 item 4, TODO-2070): with Sixel, a change to one row
/// of the page sends an image of that row's text rows only, placed at its
/// text row, where the whole page was sent before; a page that did not
/// change sends nothing; and a whole redraw, as after a resize, sends the
/// page.
#[test]
fn sixel_sends_only_the_rows_that_changed() {
    use super::{Drawer, Look, Scale};
    use crate::ansi::{ColourDepth, MosaicStyle};
    use crate::frame::Frame;
    use crate::terminal::Display;
    // Text rows of 16 pixels; at scale 1 a teletext row is 20.
    const TEXT_ROW_PIXELS: usize = 16;
    let look = Look {
        display: Display::Sixel,
        scale: Scale { x: 1, y: 1, aspect: false },
        depth: ColourDepth::TrueColour,
        mosaics: MosaicStyle::Unicode16,
        text_row_pixels: Some(TEXT_ROW_PIXELS),
    };
    let mut drawer = Drawer::new(look);
    let frame = Frame::still(&neoteletext_core::config::Config::default(), false);
    let mut first = Vec::new();
    drawer.draw(&mut first, &grid_showing(5, b"ALPHA"), frame, true).unwrap();
    let page_height = sixel_heights(&first)[0];
    assert_eq!(page_height, 500, "the whole page first");

    let mut changed = Vec::new();
    drawer.draw(&mut changed, &grid_showing(5, b"OMEGA"), frame, false).unwrap();
    let heights = sixel_heights(&changed);
    assert_eq!(heights.len(), 1);
    // Teletext row 5 is pixel rows 100 to 119; its first changed pixel
    // row lies in text row 6 (pixels 96 to 111), where the image starts,
    // and it ends with the Sixel band holding the last changed pixel row,
    // at most row 119: at most four bands of six.
    let height = heights[0];
    assert!((1..=24).contains(&height), "{height}: {:?}", String::from_utf8_lossy(&changed));
    assert!(String::from_utf8_lossy(&changed).contains("\u{1b}[7;1H"), "placed at text row 6");

    let mut unchanged = Vec::new();
    drawer.draw(&mut unchanged, &grid_showing(5, b"OMEGA"), frame, false).unwrap();
    assert!(sixel_heights(&unchanged).is_empty(), "nothing changed, nothing sent");

    let mut whole = Vec::new();
    drawer.draw(&mut whole, &grid_showing(5, b"OMEGA"), frame, true).unwrap();
    assert_eq!(sixel_heights(&whole), [page_height]);
}

/// REQ-382: every picture protocol draws the page again when only the
/// flash phase changes, as it draws flash itself; text does not, as the
/// terminal's blink flashes it.
#[test]
fn a_picture_is_drawn_again_at_each_flash_phase() {
    use super::turn::drawn_state;
    use crate::frame::Frame;
    use crate::terminal::Display;
    let on = Frame::still(&neoteletext_core::config::Config::default(), false);
    let off = Frame { flash_visible: false, ..on };
    let grid = super::Grid::EMPTY;
    for display in [Display::Kitty, Display::Sixel, Display::Iterm2] {
        assert_ne!(drawn_state(&grid, display, on, false), drawn_state(&grid, display, off, false), "{display:?}");
    }
    assert_eq!(drawn_state(&grid, Display::Text, on, false), drawn_state(&grid, Display::Text, off, false));
}

/// REQ-382: a text row's height is known only when the rows divide the
/// reported height exactly.
#[test]
fn a_text_rows_height_needs_rows_that_divide_the_height() {
    use crate::terminal::TextArea;
    assert_eq!(TextArea { width: 800, height: 600, rows: 30 }.text_row_pixels(), Some(20));
    assert_eq!(TextArea { width: 800, height: 604, rows: 30 }.text_row_pixels(), None, "padding");
    assert_eq!(TextArea { width: 800, height: 600, rows: 0 }.text_row_pixels(), None);
}

/// REQ-295 (REQ-NAV-010), DEC-2477: the keys given for Back and Forward act
/// as Back and Forward, typed without Control or Alt, and a key the viewer
/// already uses, a key that is not printable, or one key for both is
/// refused.
#[test]
fn the_keys_given_for_back_and_forward_act_as_them() {
    use super::RemoteKeys;
    use neoteletext_core::viewer::Key;
    let keys = RemoteKeys::new(Some('b'), Some('f')).unwrap();
    let typed = |character, modifiers| keys.key(KeyEvent::new(KeyCode::Char(character), modifiers));
    assert_eq!(typed('b', KeyModifiers::NONE), Some(Key::Back));
    assert_eq!(typed('f', KeyModifiers::NONE), Some(Key::Forward));
    assert_eq!(typed('b', KeyModifiers::CONTROL), None);
    assert_eq!(typed('x', KeyModifiers::NONE), None);
    assert_eq!(
        RemoteKeys::default().key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)),
        None,
        "none by default"
    );
    for refused in [(Some('i'), None), (None, Some('q')), (Some('7'), None), (Some(' '), None), (Some('b'), Some('b'))]
    {
        assert!(RemoteKeys::new(refused.0, refused.1).is_err(), "{refused:?}");
    }
}

/// REQ-295 (REQ-NAV-010), DEC-2477: on a terminal that reports releases,
/// Index held is Back and Index pressed is Index: a repeat goes Back once,
/// a quick release is Index, a release after the hold time is Back; on a
/// terminal that reports no releases, `i` is Index at once.
#[test]
fn a_held_index_is_back_and_a_pressed_index_is_index() {
    use super::Shown;
    use crossterm::event::KeyEventKind;
    use neoteletext_core::config::Config;
    use neoteletext_core::types::{Instant, PageAddress};
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let session = neoteletext_app::session::Session::new(Config::default(), 8);
    let config = Config::default();
    let shown = Shown { page: None, extras: None, config: &config, now: Instant(0) };
    let mut source = Recorded::default();
    let mut state = view_state(&session);
    state.initial = page(0x150);
    state.releases_reported = true;
    let mut press = |state: &mut super::ViewState, character, kind| {
        let key = KeyEvent::new_with_kind(KeyCode::Char(character), KeyModifiers::NONE, kind);
        let _ = state.key(key, &mut source, &session, &shown, 0);
    };
    for digit in ['3', '0', '0'] {
        press(&mut state, digit, KeyEventKind::Press);
    }
    assert_eq!(state.viewer.requested().0, page(0x300));
    press(&mut state, 'i', KeyEventKind::Press);
    assert_eq!(state.viewer.requested().0, page(0x300), "a press waits");
    press(&mut state, 'i', KeyEventKind::Repeat);
    assert_eq!(state.viewer.requested().0, page(0x100), "held: Back");
    press(&mut state, 'i', KeyEventKind::Repeat);
    press(&mut state, 'i', KeyEventKind::Release);
    assert_eq!(state.viewer.requested().0, page(0x100), "Back once a hold");
    press(&mut state, 'i', KeyEventKind::Press);
    press(&mut state, 'i', KeyEventKind::Release);
    assert_eq!(state.viewer.requested().0, page(0x150), "pressed: Index");
    // Held past the hold time with no repeat.
    let started = StdInstant::now();
    assert_eq!(state.index_hold(KeyEventKind::Press, started), None);
    assert_eq!(
        state.index_hold(KeyEventKind::Release, started + super::keys::HOLD_FOR_BACK),
        Some(neoteletext_core::viewer::Key::Back)
    );
    // A terminal that reports no releases: Index at once.
    let mut plain = view_state(&session);
    plain.initial = page(0x150);
    for digit in ['3', '0', '0'] {
        press(&mut plain, digit, KeyEventKind::Press);
    }
    press(&mut plain, 'i', KeyEventKind::Press);
    assert_eq!(plain.viewer.requested().0, page(0x150));
}

/// Types `digits` into the viewer as key presses, with `extras` the page
/// shown's links.
fn type_number(
    state: &mut super::ViewState,
    source: &mut dyn neoteletext_app::source::Source,
    session: &neoteletext_app::session::Session,
    extras: Option<&neoteletext_app::extras::PageExtras>,
    keys: &[KeyCode],
) {
    let config = neoteletext_core::config::Config::default();
    let shown = super::Shown { page: None, extras, config: &config, now: neoteletext_core::types::Instant(0) };
    for &code in keys {
        let _ = state.key(KeyEvent::new(code, KeyModifiers::NONE), source, session, &shown, 0);
    }
}

/// REQ-509 (REQ-WEB-020), DEC-2480: a typed number with an alias to a
/// linked service asks, naming the service and its origin, and Enter goes
/// there; once its origin is confirmed it goes at once; an alias to
/// another channel's address is said; a number with no alias is a page of
/// this service.
#[test]
fn a_typed_number_follows_its_alias() {
    use neoteletext_core::nav::LinkTarget;
    use neoteletext_core::types::{PageAddress, SubCode};
    use neoteletext_core::viewer::Action;
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let digits = |text: &str| text.chars().map(KeyCode::Char).collect::<Vec<_>>();
    let session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut source = Linking(vec![linked("weather", "Weather", "https://weather.example")]);
    let mut state = view_state(&session);
    state.take_notice(
        neoteletext_app::pipe::Notice::Aliases(vec![
            (0x300, "weather:301".to_owned()),
            (0x400, "https://sport.example/400.t42".to_owned()),
        ]),
        &mut source,
    );
    type_number(&mut state, &mut source, &session, None, &digits("300"));
    let target = LinkTarget { address: page(0x301), subcode: SubCode::ANY, service: Some(0) };
    assert_eq!(state.pending_link, Some(target));
    assert_eq!(
        state.prompt,
        Some(neoteletext_app::interact::prompt_with("Enter: ", "Weather", "https://weather.example"))
    );
    type_number(&mut state, &mut source, &session, None, &[KeyCode::Enter]);
    assert_eq!(state.action, Action::FollowService(target));
    state.act(&mut neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8), &mut source);
    // The origin is now confirmed: the alias goes at once.
    type_number(&mut state, &mut source, &session, None, &digits("300"));
    assert_eq!(state.action, Action::FollowService(target));
    // Another key lets a question go.
    let mut asked = view_state(&session);
    asked.aliases = state.aliases.clone();
    type_number(&mut asked, &mut source, &session, None, &digits("3001"));
    assert_eq!((asked.pending_link, asked.prompt), (None, None));
    // Another channel's address is said.
    type_number(&mut state, &mut source, &session, None, &digits("400"));
    assert!(
        state.messages.iter().any(|message| message.contains("https://sport.example/400.t42")),
        "{:?}",
        state.messages
    );
    // No alias: the page of this service.
    type_number(&mut state, &mut source, &session, None, &digits("500"));
    assert_eq!(state.viewer.requested().0, page(0x500));
}

/// REQ-509 (REQ-WEB-020): a page's own `LK` link for a typed number wins
/// over the service's alias for it, and leads to a page of this service as
/// one visit, so Back returns to the page before.
#[test]
fn a_pages_own_number_link_wins_over_the_alias() {
    use neoteletext_app::extras::{Destination, PageExtras};
    use neoteletext_core::nav::LinkTarget;
    use neoteletext_core::types::{PageAddress, SubCode};
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut source = Linking(vec![linked("weather", "Weather", "https://weather.example")]);
    let mut state = view_state(&session);
    state.aliases = vec![(0x300, "weather:300".to_owned())];
    let extras = PageExtras {
        links: None,
        row24: None,
        key_addresses: Default::default(),
        number_links: vec![(
            page(0x300),
            Destination::Page(LinkTarget { address: page(0x350), subcode: SubCode::ANY, service: None }),
        )],
        text_links: Vec::new(),
    };
    let keys: Vec<KeyCode> = "300".chars().map(KeyCode::Char).collect();
    type_number(&mut state, &mut source, &session, Some(&extras), &keys);
    assert_eq!(state.viewer.requested().0, page(0x350));
    assert_eq!(state.pending_link, None);
    type_number(&mut state, &mut source, &session, Some(&extras), &[KeyCode::Backspace]);
    assert_eq!(state.viewer.requested().0, page(0x100), "one visit: Back goes to the page before");
}

/// REQ-89 (REQ-CLI-004): the interactive viewer takes a key for each
/// function the requirement lists: page number entry, next and previous
/// page, the four Fastext keys on F1 to F4 and on r, g, y and c, Index,
/// reveal, hold, mix, zoom and sub-page stepping. Quitting (`q` or Escape)
/// is the loop's own key, read before this mapping (`input.rs`).
#[test]
fn every_function_of_the_interactive_viewer_has_its_key() {
    use super::keys::map_key;
    use neoteletext_core::viewer::Key;
    let key = |code: KeyCode| map_key(KeyEvent::new(code, KeyModifiers::NONE));
    for digit in 0..=9u8 {
        assert_eq!(key(KeyCode::Char(char::from(b'0' + digit))), Some(Key::Digit(digit)));
    }
    assert_eq!(key(KeyCode::Up), Some(Key::NextPage));
    assert_eq!(key(KeyCode::Down), Some(Key::PreviousPage));
    let fastext = [(1, 'r', Key::Red), (2, 'g', Key::Green), (3, 'y', Key::Yellow), (4, 'c', Key::Cyan)];
    for (function, letter, wanted) in fastext {
        assert_eq!(key(KeyCode::F(function)), Some(wanted), "F{function}");
        assert_eq!(key(KeyCode::Char(letter)), Some(wanted), "{letter}");
    }
    assert_eq!(key(KeyCode::Char('i')), Some(Key::Index));
    assert_eq!(key(KeyCode::Char('?')), Some(Key::Reveal));
    assert_eq!(key(KeyCode::Char('H')), Some(Key::Hold));
    assert_eq!(key(KeyCode::Char('m')), Some(Key::Mix));
    assert_eq!(key(KeyCode::Char('z')), Some(Key::Zoom));
    assert_eq!(key(KeyCode::Right), Some(Key::NextSubPage));
    assert_eq!(key(KeyCode::Left), Some(Key::PreviousSubPage));
}

/// A source of a service linking to others that records the links followed
/// and the services reopened, numbering the services as `Live` does.
#[derive(Default)]
struct Travelling {
    linked: Vec<neoteletext_app::pipe::ServiceEntry>,
    followed: Vec<(u8, u16)>,
    reopened: Vec<(u8, u16)>,
    prefetched: Vec<u16>,
}

impl neoteletext_app::source::Source for Travelling {
    fn poll(
        &mut self,
        _session: &mut neoteletext_app::session::Session,
        _now: neoteletext_core::types::Instant,
    ) -> bool {
        true
    }

    fn linked_service(&self, service: u8) -> Option<neoteletext_app::pipe::ServiceEntry> {
        self.linked.get(usize::from(service)).cloned()
    }

    fn follow(
        &mut self,
        service: u8,
        page: neoteletext_core::types::PageAddress,
        _subcode: neoteletext_core::types::SubCode,
    ) -> Option<u8> {
        self.followed.push((service, page.mpp()));
        u8::try_from(self.followed.len()).ok()
    }

    fn reopen(
        &mut self,
        number: u8,
        page: neoteletext_core::types::PageAddress,
        _subcode: neoteletext_core::types::SubCode,
    ) {
        self.reopened.push((number, page.mpp()));
    }

    fn prefetch(&mut self, page: neoteletext_core::types::PageAddress) {
        self.prefetched.push(page.mpp());
    }
}

/// REQ-295 (REQ-NAV-010): following a link to another service keeps the
/// viewer and its history; Back then returns to the first service and its
/// page, by the number the session gave it, and Forward goes on again to
/// the service followed, both without asking (REQ-288); a page the
/// channel's program sends the viewer to is a visit Back returns from.
#[test]
fn back_returns_across_a_link_to_another_service_and_forward_goes_on() {
    use neoteletext_core::nav::{HistoryEntry, LinkTarget};
    use neoteletext_core::types::{PageAddress, SubCode};
    use neoteletext_core::viewer::Action;
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let mut session = neoteletext_app::session::Session::new(neoteletext_core::config::Config::default(), 8);
    let mut source =
        Travelling { linked: vec![linked("weather", "Weather", "https://weather.example")], ..Travelling::default() };
    let mut state = view_state(&session);
    state.act(&mut session, &mut source);
    type_number(&mut state, &mut source, &session, None, &"200".chars().map(KeyCode::Char).collect::<Vec<_>>());
    state.act(&mut session, &mut source);
    state.action = Action::FollowService(LinkTarget { address: page(0x301), subcode: SubCode::ANY, service: Some(0) });
    state.act(&mut session, &mut source);
    assert_eq!(source.followed, [(0, 0x301)]);
    assert_eq!((state.viewer.service(), state.viewer.requested().0), (1, page(0x301)));
    // The channel's program sends the viewer to 302 of the service followed.
    state.typing.redirected(0x302, SubCode::ANY.value(), None);
    let (requested, subcode) = state.viewer.requested();
    assert!(state.typing_turn(&mut session, &mut source, requested, subcode));
    assert_eq!(state.viewer.requested().0, page(0x302));
    let config = neoteletext_core::config::Config::default();
    let shown = super::Shown { page: None, extras: None, config: &config, now: neoteletext_core::types::Instant(0) };
    let mut press = |state: &mut super::ViewState, code, modifiers| {
        let _ = state.key(KeyEvent::new(code, modifiers), &mut source, &session, &shown, 0);
        let action = state.action;
        state.act(&mut neoteletext_app::session::Session::new(config, 8), &mut source);
        action
    };
    assert_eq!(press(&mut state, KeyCode::Backspace, KeyModifiers::NONE), Action::Request(page(0x301), SubCode::ANY));
    let first = HistoryEntry { service: 0, address: page(0x200), subcode: SubCode::ANY };
    assert_eq!(press(&mut state, KeyCode::Backspace, KeyModifiers::NONE), Action::ReturnTo(first));
    assert_eq!(state.prompt, None, "no question");
    let again = HistoryEntry { service: 1, address: page(0x301), subcode: SubCode::ANY };
    assert_eq!(press(&mut state, KeyCode::Right, KeyModifiers::ALT), Action::ReturnTo(again));
    assert_eq!(source.reopened, [(0, 0x200), (1, 0x301)]);
    assert_eq!((state.viewer.service(), state.viewer.requested().0), (1, page(0x301)));
}

/// Pushes the header of page `mpp` of magazine 1, at `subcode`.
fn push_header(session: &mut neoteletext_app::session::Session, mpp: u16, subcode: u16) {
    use neoteletext_core::packet::{PacketMeta, encode};
    use neoteletext_core::types::{ControlBits, PageAddress, SubCode};
    let address = PageAddress::from_mpp(mpp).unwrap();
    let display = [b' '; neoteletext_core::consts::HEADER_DISPLAY_BYTES];
    let header = encode::header(address.magazine, address.page, SubCode::new(subcode), ControlBits(0), &display);
    session.push(&header.unwrap(), &PacketMeta::default());
}

/// REQ-511 (REQ-WEB-022), decision E.63 item 5: a page shown at Level 2.5
/// asks its service once for its magazine's Organization Table, then once
/// for the object pages the table names for it; a page already stored is
/// not asked for, and nothing is asked at Level 1.5.
#[test]
fn a_level_2_5_page_asks_for_its_table_then_its_object_pages_once() {
    use neoteletext_core::config::{Config, PresentationLevel};
    use neoteletext_core::packet::{MotObjectLink, MotPageEntry, PacketMeta, encode, mot_position};
    use neoteletext_core::types::{FallbackFields, Magazine, PageAddress, PageNumber, SubCode};
    let config = Config { level: PresentationLevel::Level25, ..Config::default() };
    let mut session = neoteletext_app::session::Session::new(config, 8);
    push_header(&mut session, 0x100, 0);
    push_header(&mut session, 0x101, 0);
    let page = PageAddress::from_mpp(0x100).unwrap();
    let mut state = view_state(&session);
    let mut source = Travelling::default();
    let ask = |state: &mut super::ViewState, session: &neoteletext_app::session::Session, source: &mut Travelling| {
        state.ask_enhancements(session, source, session.page(page, SubCode::ANY), &config);
    };
    ask(&mut state, &session, &mut source);
    ask(&mut state, &session, &mut source);
    assert_eq!(source.prefetched, [0x1FE], "the table, once");
    // The table: GPOP 1A0, POP link 1 to 1A1, and page 00 using POP link 1.
    let magazine = Magazine::new(1).unwrap();
    let link = |target: u8| MotObjectLink {
        magazine,
        x27_flag: false,
        page: PageNumber(target),
        subpage_count: 0,
        fallback: FallbackFields::default(),
        default_objects: [None; 2],
    };
    let mut links = [0u8; neoteletext_core::consts::DATA_BYTES];
    let (gpop, pop) = links.split_at_mut(neoteletext_core::consts::MOT_OBJECT_LINK_BYTES);
    gpop.copy_from_slice(&encode::mot_object_link(&link(0xA0)));
    pop[..neoteletext_core::consts::MOT_OBJECT_LINK_BYTES].copy_from_slice(&encode::mot_object_link(&link(0xA1)));
    let position = mot_position(PageNumber(0x00));
    let mut entries = [0u8; neoteletext_core::consts::DATA_BYTES];
    entries[position.index..position.index + 2]
        .copy_from_slice(&encode::mot_page_entry(MotPageEntry { global_objects: true, pop_link: 1 }));
    // S4 S3 name the table's last packet, X/19 (clause 10.6).
    push_header(&mut session, 0x1FE, 19 << 8);
    session.push(&encode::nibble_packet(magazine, position.packet, &entries), &PacketMeta::default());
    session.push(&encode::nibble_packet(magazine, 19, &links), &PacketMeta::default());
    push_header(&mut session, 0x101, 0);
    assert!(session.decoder().enhancement_lookup().magazine_organization_table(magazine).is_some());
    ask(&mut state, &session, &mut source);
    ask(&mut state, &session, &mut source);
    assert_eq!(source.prefetched, [0x1FE, 0x1A0, 0x1A1], "then its object pages, once");
    // A viewer that has asked nothing asks only for what is not stored.
    let mut fresh = view_state(&session);
    let mut other = Travelling::default();
    ask(&mut fresh, &session, &mut other);
    assert_eq!(other.prefetched, [0x1A0, 0x1A1]);
    // Level 1.5 uses no enhancement page.
    let mut plain = view_state(&session);
    plain.ask_enhancements(
        &session,
        &mut other,
        session.page(page, SubCode::ANY),
        &Config { level: PresentationLevel::Level15, ..Config::default() },
    );
    assert_eq!(other.prefetched, [0x1A0, 0x1A1]);
}
