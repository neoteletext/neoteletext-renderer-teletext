//! Snapshot mode (REQ-6880, REQ-6879 item 4; neoteletext-common
//! `docs/tab-thumbnails.md` section 4), driven through the replay
//! transport: one page as a stranger sees it, with nothing that identifies
//! the viewer, then one end.

use super::*;
use neoteletext_input::caps::{NAPLPS_CAP, REGIS_CAP, SELECT_CAP, SNAPSHOT_OFFER};
use neoteletext_input::control::{Control, packet_of};
use neoteletext_web::agent::{SnapshotEnd, SnapshotTarget};
use neoteletext_web::consts::{CLOSE_NORMAL, SNAPSHOT_TIME_LIMIT_US};
use neoteletext_web::records::denied::{self, DeniedRecord};
use neoteletext_web::records::state::{StateChallenge, encode_challenge};

/// The page the snapshots ask for: 100 as a whole.
fn whole_page() -> SnapshotTarget {
    SnapshotTarget { page: PageAddress::from_mpp(0x100).unwrap(), sub: SubCode::ANY }
}

/// The snapshot request's bytes for `target`, by the engine's writer.
fn request(target: SnapshotTarget) -> Vec<u8> {
    let mut out = [0_u8; 64];
    let length = neoteletext_input::snapshot::write_request(target.page.mpp(), target.sub.value(), &mut out).unwrap();
    out[..length].to_vec()
}

/// A WELCOME packet agreeing `caps`.
fn welcome(caps: u32) -> Vec<u8> {
    let welcome = Control::Welcome { version: neoteletext_input::VERSION, caps, resume: None };
    packet_of(&welcome, 0, 0).unwrap().bytes.to_vec()
}

/// A page: its header, then a row.
fn a_page() -> Vec<u8> {
    [packet_at(1, 0), packet_at(1, 1)].concat()
}

/// A server close frame with `code`.
fn close(code: u16) -> Vec<u8> {
    server_frame(0x8, &code.to_be_bytes())
}

/// A live channel at `http://site.example/live/` whose push connection
/// answers the upgrade with `frames`, opened by an agent in snapshot mode.
fn live_snapshot(target: SnapshotTarget, frames: Vec<u8>) -> (Harness, Vec<Rec>) {
    let mut harness = site_harness(Config { snapshot: Some(target), ..Config::default() });
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", Reply::WebSocket(frames));
    let records = harness.open_channel("http://site.example/live/");
    (harness, records)
}

/// The `SnapshotEnded` events among `records`.
fn ends(records: &[Rec]) -> Vec<&str> {
    events(records).into_iter().filter(|event| event.starts_with("SnapshotEnded")).collect()
}

/// The one end of a snapshot, as it is written.
fn ended(end: SnapshotEnd) -> String {
    format!("{:?}", neoteletext_web::agent::Event::SnapshotEnded(end))
}

/// How many push connections the agent opened.
fn push_connects(harness: &mut Harness) -> usize {
    harness.replay().connects.iter().filter(|(connection, ..)| *connection == ConnectionId::Push).count()
}

/// No request the agent sent to `host` carries anything that could identify
/// the viewer or tie the request to an earlier one (REQ-6880 item 4).
fn assert_anonymous(harness: &mut Harness, host: &str) {
    for request in harness.replay().requests_to(host) {
        for name in ["Cookie", "Authorization", "Referer", "If-None-Match", "If-Modified-Since"] {
            assert_eq!(header_value(&request, name), None, "{name} in {}", String::from_utf8_lossy(&request));
        }
    }
}

// Verifies REQ-6880 item 2 and REQ-6879 item 3: on its push connection the
// agent sends the snapshot request, HELLO with SNAPSHOT_OFFER and no agent
// then SELECT of the page, and nothing else but the close that answers the
// server's; the page is given on and the snapshot is answered.
#[test]
fn a_snapshot_of_a_live_channel_sends_hello_and_select_and_nothing_more() {
    let target = whole_page();
    let mut frames = server_frame(0x2, &welcome(SNAPSHOT_OFFER));
    frames.extend(server_frame(0x2, &a_page()));
    frames.extend(close(CLOSE_NORMAL));
    let (mut harness, records) = live_snapshot(target, frames);
    assert_eq!(
        push_frames(&mut harness),
        [(0x2, request(target)), (0x8, CLOSE_NORMAL.to_be_bytes().to_vec())],
        "the request, then the close answered"
    );
    // HELLO, then SELECT 100: the wire vector `input hello_snapshot`.
    assert_eq!(request(target), [0xFF, 0x09, 0x0B, 0x80, 0x80, 0x80, 0x1E, 0xFF, 0x40, 3, b'1', b'0', b'0']);
    let delivered = pages(&records);
    assert_eq!(delivered.len(), 2, "{records:?}");
    assert!(matches!(delivered[1], Rec::Page { source: Source::Push, bytes, .. } if *bytes == a_page()));
    assert_eq!(ends(&records), [ended(SnapshotEnd::Answered).as_str()]);
    assert!(!has_event(&records, "PushLost"), "{records:?}");
    assert_eq!(paths(&mut harness, "site.example:80"), [format!("/live/{SETTINGS_FILE}"), "/live/push".to_owned()]);
    assert_anonymous(&mut harness, "site.example:80");
}

// Verifies REQ-6880 item 2 and REQ-6879 item 4: a WELCOME that does not agree
// SNAPSHOT closes the connection at once with a normal close, and nothing
// after it, the page included, is given on.
#[test]
fn a_snapshot_closes_when_welcome_does_not_agree_it() {
    let mut frames = server_frame(0x2, &welcome(SELECT_CAP | REGIS_CAP | NAPLPS_CAP));
    frames.extend(server_frame(0x2, &a_page()));
    let (mut harness, records) = live_snapshot(whole_page(), frames);
    assert_eq!(ends(&records), [ended(SnapshotEnd::NotOffered).as_str()]);
    assert!(pages(&records).is_empty(), "{records:?}");
    assert_eq!(push_frames(&mut harness), [(0x2, request(whole_page())), (0x8, CLOSE_NORMAL.to_be_bytes().to_vec())]);
    assert!(!harness.replay().is_connected(ConnectionId::Push), "closed");
}

// Verifies REQ-6880 item 2: a snapshot never reconnects. A connection that
// fails, or closes before a page, is a failure, and time passing opens no
// other.
#[test]
fn a_snapshot_never_reconnects() {
    let mut harness = site_harness(Config { snapshot: Some(whole_page()), ..Config::default() });
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", Reply::Close);
    harness.replay().reply("site.example:80", Reply::WebSocket(server_frame(0x2, &welcome(SNAPSHOT_OFFER))));
    let mut records = harness.open_channel("http://site.example/live/");
    for _ in 0..10 {
        harness.advance(60 * SECOND);
        records.extend(harness.take());
    }
    assert_eq!(push_connects(&mut harness), 1);
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Network)).as_str()]);
    assert!(!has_event(&records, "PushLost"), "{records:?}");

    // A close with code 1000 before any page is no answer either.
    let mut frames = server_frame(0x2, &welcome(SNAPSHOT_OFFER));
    frames.extend(close(CLOSE_NORMAL));
    let (_, records) = live_snapshot(whole_page(), frames);
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Network)).as_str()]);
}

// Verifies REQ-6880 item 4: every input message the caller gives, a viewer
// state answer among them, is refused and nothing is sent, before the
// connection opens and while it is open.
#[test]
fn a_snapshot_refuses_its_callers_input_and_state_answers() {
    use neoteletext_input::event::{Event as InputEvent, Item, write};
    use neoteletext_input::state::NoneReason;
    let mut answer = [0_u8; 16];
    let length = write(&Item::Event(InputEvent::StateNone(NoneReason::Stateless)), &mut answer).unwrap();
    let mut harness = site_harness(Config { snapshot: Some(whole_page()), ..Config::default() });
    harness.input(Input::Send(InputMessage::alone(&answer[..length])));
    assert!(has_event(&harness.take(), "InputDropped"));
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", Reply::WebSocket(server_frame(0x2, &welcome(SNAPSHOT_OFFER))));
    let records = harness.open_channel("http://site.example/live/");
    assert_eq!(ends(&records), Vec::<&str>::new(), "still open: {records:?}");
    harness.input(Input::Send(InputMessage::alone(&answer[..length])));
    harness.input(Input::Send(InputMessage::alone(b"hello")));
    harness.input(Input::Subscribe(Subscription::All));
    harness.input(Input::MeasureRoundTrip(true));
    harness.advance(SECOND);
    let records = harness.take();
    assert_eq!(events(&records).iter().filter(|event| **event == "InputDropped").count(), 2, "{records:?}");
    assert_eq!(push_frames(&mut harness), [(0x2, request(whole_page()))], "the request alone");
}

// Verifies REQ-6880 item 3: a static channel's snapshot fetches the
// channel's settings and the page's file, and no bundle, contents, list or
// other page, however the caller asks.
#[test]
fn a_snapshot_of_a_static_channel_fetches_one_file_and_no_contents() {
    let mut harness = site_harness(Config { snapshot: Some(whole_page()), ..Config::default() });
    harness.replay().reply("site.example:80", json_response(br#"{"name":"Test"}"#));
    harness.replay().reply("site.example:80", tti_response(&["ETag: \"a\""]));
    let mut records = harness.open_channel("http://site.example/news/");
    harness.input(Input::FetchContents);
    harness.input(Input::FetchPage { page: PageAddress::from_mpp(0x200).unwrap(), subcode: None });
    harness.input(Input::FetchSubPages { page: PageAddress::from_mpp(0x100).unwrap(), part: PartNumber::FIRST });
    harness.advance(SECOND);
    records.extend(harness.take());
    assert_eq!(paths(&mut harness, "site.example:80"), [settings_path().as_str(), "/news/100.tti"]);
    let delivered = pages(&records);
    assert_eq!(delivered.len(), 1, "{records:?}");
    assert!(matches!(delivered[0], Rec::Page { source: Source::Page { .. }, bytes, .. } if *bytes == TTI));
    assert_eq!(ends(&records), [ended(SnapshotEnd::Answered).as_str()]);

    // A sub-page is its own file, then, when the channel has none, the
    // page's file; a page missing in every file is a 404.
    let target = SnapshotTarget { page: PageAddress::from_mpp(0x100).unwrap(), sub: SubCode::new(2) };
    let mut harness = site_harness(Config { snapshot: Some(target), ..Config::default() });
    harness.replay().reply("site.example:80", not_found());
    for _ in 0..4 {
        harness.replay().reply("site.example:80", not_found());
    }
    let mut records = harness.open_channel("http://site.example/news/");
    for _ in 0..4 {
        harness.advance(SECOND);
        records.extend(harness.take());
    }
    assert_eq!(
        paths(&mut harness, "site.example:80"),
        [settings_path().as_str(), "/news/100/0002.t42", "/news/100/0002.tti", "/news/100.tti", "/news/100.t42"]
    );
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Status(404))).as_str()]);
}

// Verifies REQ-6880 item 4: no request of a snapshot, the settings', the
// page's or the upgrade, carries a validator, a cookie, a referrer or
// credentials, though the server gave validators before.
#[test]
fn a_snapshot_request_carries_no_conditional_header() {
    let target = SnapshotTarget { page: PageAddress::from_mpp(0x100).unwrap(), sub: SubCode::new(1) };
    let mut harness = site_harness(Config { snapshot: Some(target), ..Config::default() });
    let validated = ["ETag: \"v1\"", "Last-Modified: Wed, 18 Nov 2026 10:00:00 GMT"];
    harness.replay().reply("site.example:80", json_response(br#"{"name":"Test"}"#));
    // The sub-page's T42 file is missing with validators, then its TTI.
    harness.replay().reply(
        "site.example:80",
        Reply::Bytes(response("404 Not Found", &["Content-Type: text/html", validated[0], validated[1]], b"no")),
    );
    harness.replay().reply("site.example:80", tti_response(&validated));
    let mut records = harness.open_channel("http://site.example/news/");
    harness.advance(SECOND);
    records.extend(harness.take());
    assert_eq!(ends(&records), [ended(SnapshotEnd::Answered).as_str()]);
    assert_anonymous(&mut harness, "site.example:80");
    let (mut live, _) = live_snapshot(whole_page(), server_frame(0x2, &welcome(SNAPSHOT_OFFER)));
    assert_anonymous(&mut live, "site.example:80");
    let upgrade = &live.replay().requests_to("site.example:80")[1];
    assert!(header_value(upgrade, "User-Agent").is_some(), "it names its product, as every request does");
}

// Verifies REQ-6880 item 5: a server that never answers holds the snapshot
// no longer than SNAPSHOT_TIME_LIMIT_US from the open; then everything
// closes, with a normal close on the push connection, and the snapshot has
// failed with the timeout. No heartbeat goes meanwhile.
#[test]
fn a_snapshot_ends_at_its_time_limit() {
    assert_eq!(SNAPSHOT_TIME_LIMIT_US, 15_000_000);
    let (mut harness, records) = live_snapshot(whole_page(), server_frame(0x2, &welcome(SNAPSHOT_OFFER)));
    assert_eq!(ends(&records), Vec::<&str>::new());
    harness.advance(SNAPSHOT_TIME_LIMIT_US - 1);
    assert_eq!(ends(&harness.take()), Vec::<&str>::new());
    harness.advance(1);
    let records = harness.take();
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Timeout)).as_str()]);
    assert_eq!(
        push_frames(&mut harness),
        [(0x2, request(whole_page())), (0x8, CLOSE_NORMAL.to_be_bytes().to_vec())],
        "no ping, then the close"
    );
    assert!(!harness.replay().is_connected(ConnectionId::Push));
    harness.advance(60 * SECOND);
    assert!(harness.take().iter().all(|record| matches!(record, Rec::Timer(_))), "nothing after the end");
}

// Verifies REQ-6880 item 2: a STATE_CHALLENGE on a snapshot connection is the
// server breaking the protocol: it is neither reported nor answered, nothing
// after it is given on, and the snapshot fails.
#[test]
fn a_challenge_on_a_snapshot_connection_fails_it() {
    let challenge = packets_of(|out| {
        encode_challenge(StateChallenge { challenge: [7; 32], create: true }, 0, 1, out).unwrap();
    });
    let mut frames = server_frame(0x2, &welcome(SNAPSHOT_OFFER));
    frames.extend(server_frame(0x2, &[challenge, a_page()].concat()));
    let (mut harness, records) = live_snapshot(whole_page(), frames);
    assert!(!has_event(&records, "StateChallenge"), "{records:?}");
    assert_eq!(pages(&records).len(), 1, "the WELCOME alone: {records:?}");
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Content)).as_str()]);
    assert_eq!(push_frames(&mut harness), [(0x2, request(whole_page())), (0x8, CLOSE_NORMAL.to_be_bytes().to_vec())]);
}

// Verifies REQ-6880 item 2: a DENIED record is given on and ends the
// snapshot with its status; packets before WELCOME fail it.
#[test]
fn a_denied_snapshot_ends_with_its_status() {
    let refused = packets_of(|out| {
        denied::encode(DeniedRecord { status: 404, place: "100" }, 0, 1, out).unwrap();
    });
    let mut frames = server_frame(0x2, &welcome(SNAPSHOT_OFFER));
    frames.extend(server_frame(0x2, &refused));
    frames.extend(close(CLOSE_NORMAL));
    let (_, records) = live_snapshot(whole_page(), frames);
    assert!(has_event(&records, "Denied(DeniedRecord { status: 404"), "{records:?}");
    assert_eq!(ends(&records), [ended(SnapshotEnd::Denied(404)).as_str()]);

    let (_, records) = live_snapshot(whole_page(), server_frame(0x2, &a_page()));
    assert!(pages(&records).is_empty(), "{records:?}");
    assert_eq!(ends(&records), [ended(SnapshotEnd::Failed(neoteletext_web::agent::Failure::Content)).as_str()]);
}
