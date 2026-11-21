//! The push connection opened as a stream the port presents (REQ-6984),
//! and what a descriptor's answer says in `Alt-Svc` (REQ-6983), driven
//! through the replay transport. The harness stands for a port: where the
//! agent asks for a stream it opens the replay's push connection with no
//! handshake on it, and answers as the test scripted.

use super::*;
use neoteletext_web::consts::{
    PUSH_PING_AFTER_US, PUSH_PONG_WITHIN_US, PUSH_RETRY_MAX_US, PUSH_SUBPROTOCOL, PUSH_SUBPROTOCOLS,
};

/// The secure host the live channel is on, as the replay keys its replies.
const SECURE: &str = "site.example:443";

/// The server's answer that opens the stream: 200 with the push
/// subprotocol.
fn accepted() -> (u16, Option<Vec<u8>>) {
    (200, Some(PUSH_SUBPROTOCOL.as_bytes().to_vec()))
}

/// The field with which the settings' answer names HTTP/3 on the channel's
/// own port.
const NAMED: &str = "Alt-Svc: h3=\":443\"";

/// A harness whose live channel at `https://site.example/live/` answers its
/// settings with `headers`, with streams asked for or not, answered with
/// `answer`.
fn secure_live(headers: &[&str], streams: bool, answer: Option<(u16, Option<Vec<u8>>)>) -> Harness {
    let mut harness = site_harness(Config::default());
    let mut all = std::vec!["Content-Type: application/json"];
    all.extend_from_slice(headers);
    harness.replay().reply(SECURE, Reply::Bytes(response("200 OK", &all, br#"{"push":{}}"#)));
    harness.stream_answer = answer;
    if streams {
        harness.input(Input::PushOverStream(true));
    }
    harness
}

/// The requests for the push connection's HTTP/1.1 handshake seen so far.
fn upgrades(harness: &mut Harness) -> usize {
    paths(harness, SECURE).iter().filter(|path| path.as_str() == "/live/push").count()
}

/// The frames sent on the push connection, every send's.
fn stream_frames(harness: &mut Harness) -> Vec<(u8, Vec<u8>)> {
    harness.replay().sent.iter().filter(|(c, _)| *c == ConnectionId::Push).flat_map(|(_, f)| client_frames(f)).collect()
}

/// The `OpenPushStream` outputs among `records`.
fn asked(records: &[Rec]) -> Vec<&str> {
    events(records).into_iter().filter(|event| event.starts_with("OpenPushStream")).collect()
}

/// REQ-6983: the answer that brought a channel's settings names HTTP/3 in
/// `Alt-Svc`, and the agent reports it once, before the channel is open.
#[test]
fn a_settings_answers_alt_svc_is_reported_before_the_channel_opens() {
    let mut harness = secure_live(&["Alt-Svc: h3=\":443\"; ma=3600", "Alt-Svc: h3=\":444\""], false, None);
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    let records = harness.open_channel("https://site.example/live/");
    let events = events(&records);
    let named: Vec<_> = events.iter().filter(|event| event.starts_with("AlternativeService")).collect();
    assert_eq!(named, [&"AlternativeService(Http3 { port: 443, max_age_seconds: 3600 })"], "the first field, once");
    let at = |prefix: &str| events.iter().position(|event| event.starts_with(prefix)).unwrap();
    assert!(at("AlternativeService") < at("ServiceReady"), "{events:?}");
    // The port has not said it can present a stream: the push connection
    // is the HTTP/1.1 one.
    assert!(asked(&records).is_empty() && upgrades(&mut harness) == 1, "{records:?}");
}

/// REQ-6983: an answer without the field reports nothing, and one whose
/// field is malformed reports that it named nothing.
#[test]
fn no_alt_svc_reports_nothing_and_a_malformed_one_names_nothing() {
    let mut harness = secure_live(&[], false, None);
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    let records = harness.open_channel("https://site.example/live/");
    assert!(!has_event(&records, "AlternativeService"), "{records:?}");

    let mut harness = secure_live(&["Alt-Svc: h3=:443"], false, None);
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    let records = harness.open_channel("https://site.example/live/");
    assert!(has_event(&records, "AlternativeService(None)"), "{records:?}");
}

/// REQ-6984 items 2, 3 and 5: with streams on, the push connection is asked
/// for as a stream, with what the port needs for the request and no
/// HTTP/1.1 handshake; an accepted answer opens it, the subscription goes
/// as a masked frame, and pages arrive on it.
#[test]
fn a_stream_is_asked_for_and_an_accepted_answer_opens_the_push_connection() {
    let mut harness = secure_live(&[NAMED], true, Some(accepted()));
    let records = harness.open_channel("https://site.example/live/");
    let user_agent =
        String::from_utf8_lossy(neoteletext_web::user_agent::UserAgentString::of(None).as_bytes()).to_string();
    assert_eq!(
        asked(&records),
        [format!(
            "OpenPushStream {{ host: site.example, port: 443, authority: site.example, path: /live/push, subprotocol: {PUSH_SUBPROTOCOLS}, user_agent: {user_agent} }}"
        )
        .as_str()]
    );
    assert_eq!(upgrades(&mut harness), 0, "no HTTP/1.1 handshake");
    assert!(has_event(&records, "PushConnected"), "{records:?}");
    let first =
        harness.replay().sent.iter().find(|(c, _)| *c == ConnectionId::Push).map(|(_, bytes)| bytes.clone()).unwrap();
    assert!(!first.starts_with(b"GET "), "the stream's first bytes are a frame");
    // RFC 6455 section 5.1, which RFC 8441 keeps: a client's frames are masked.
    assert_eq!(first[1] & 0x80, 0x80, "masked");
    assert!(!stream_frames(&mut harness).is_empty(), "the subscription");
    harness.replay().inject(ConnectionId::Push, &server_frame(0x2, &packet_at(1, 0)));
    harness.settle();
    assert!(has_event(&harness.take(), "ChannelLive"));
}

/// REQ-6984 item 5: after the handshake the stream is the push connection
/// as over HTTP/1.1: a silence is pinged, an unanswered ping loses it, and
/// the retry asks for a stream again.
#[test]
fn the_heartbeat_and_the_retry_run_on_a_stream_as_over_http_1() {
    let mut harness = secure_live(&[NAMED], true, Some(accepted()));
    harness.open_channel("https://site.example/live/");
    let before = stream_frames(&mut harness).len();
    harness.advance(PUSH_PING_AFTER_US);
    let frames = stream_frames(&mut harness);
    assert_eq!(frames.len(), before + 1);
    assert_eq!(frames.last().unwrap().0, 0x9, "a ping after the silence");
    harness.take();
    harness.advance(PUSH_PONG_WITHIN_US);
    let lost = harness.take();
    assert!(has_event(&lost, "PushLost"), "{lost:?}");
    assert!(!harness.replay().is_connected(ConnectionId::Push), "the stream is closed");
    harness.advance(PUSH_RETRY_MAX_US);
    let again = harness.take();
    assert_eq!(asked(&again).len(), 1, "{again:?}");
    assert!(has_event(&again, "PushConnected"), "{again:?}");
    assert_eq!(upgrades(&mut harness), 0);
}

/// REQ-6984 item 3, REQ-SEC-006: the address the stream's connection
/// reached is held to the rule of a TCP peer.
#[test]
fn a_stream_to_a_local_peer_is_refused() {
    let mut harness = secure_live(&[NAMED], true, Some(accepted()));
    harness.open_channel("https://site.example/live/");
    // The settings and the first stream came from a public address; the
    // next stream's connection reaches a private one.
    harness.replay().resolve.insert(String::from("site.example"), PRIVATE);
    harness.replay().drop_connection(ConnectionId::Push);
    harness.settle();
    harness.take();
    harness.advance(PUSH_RETRY_MAX_US);
    let records = harness.take();
    assert_eq!(asked(&records).len(), 1, "{records:?}");
    assert!(has_event(&records, "Blocked { connection: Push, host: \"site.example\" }"), "{records:?}");
    assert!(lost_for(&records, "Failure(Blocked)"), "{records:?}");
    assert!(!has_event(&records, "PushConnected"), "{records:?}");
}

/// REQ-6984 item 3, DOC-1920: any other answer is treated as the same
/// answer to an HTTP/1.1 handshake.
#[test]
fn other_answers_stop_or_retry_as_an_http_1_handshake_does() {
    let outcome = |answer: (u16, Option<&[u8]>)| {
        let mut harness = secure_live(&[NAMED], true, Some((answer.0, answer.1.map(<[u8]>::to_vec))));
        let records = harness.open_channel("https://site.example/live/");
        assert!(!has_event(&records, "PushConnected"), "{answer:?}: {records:?}");
        assert!(!harness.replay().is_connected(ConnectionId::Push), "{answer:?}: the stream is closed");
        records
    };
    let subprotocol = Some(PUSH_SUBPROTOCOL.as_bytes());
    assert!(has_event(&outcome((403, None)), "PushStopped(Refused(403))"));
    assert!(has_event(&outcome((404, subprotocol)), "PushStopped(Refused(404))"));
    assert!(has_event(&outcome((200, None)), "PushStopped(WrongProtocol)"));
    assert!(has_event(&outcome((200, Some(b"other"))), "PushStopped(WrongProtocol)"));
    for retried in [429, 500, 503, 204, 301] {
        let records = outcome((retried, subprotocol));
        assert!(has_event(&records, "PushLost"), "{retried}: {records:?}");
        assert!(!has_event(&records, "PushStopped"), "{retried}: {records:?}");
    }
}

/// REQ-6984 item 4: when no stream can be opened, the push connection is
/// opened over HTTP/1.1 at once, with no wait and no loss reported, and no
/// stream is asked for again until the port says so.
#[test]
fn an_unavailable_stream_falls_back_to_http_1_at_once() {
    let mut harness = secure_live(&[NAMED], true, None);
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    let records = harness.open_channel("https://site.example/live/");
    assert_eq!(asked(&records).len(), 1, "{records:?}");
    assert!(!has_event(&records, "PushLost"), "nothing counted against the channel: {records:?}");
    assert!(has_event(&records, "PushConnected"), "with no time passed: {records:?}");
    assert_eq!(upgrades(&mut harness), 1, "the HTTP/1.1 handshake");
    // A later loss reconnects over HTTP/1.1 too.
    harness.replay().drop_connection(ConnectionId::Push);
    harness.settle();
    harness.advance(PUSH_RETRY_MAX_US);
    let later = harness.take();
    assert!(asked(&later).is_empty(), "{later:?}");
    assert_eq!(upgrades(&mut harness), 2);
    // Until the port turns streams on again.
    harness.stream_answer = Some(accepted());
    harness.input(Input::PushOverStream(true));
    harness.replay().drop_connection(ConnectionId::Push);
    harness.settle();
    harness.advance(PUSH_RETRY_MAX_US);
    assert_eq!(asked(&harness.take()).len(), 1);
}

/// REQ-6984 item 2: HTTP/3 has no plain form, so a push address that is
/// not secure is never asked for as a stream.
#[test]
fn a_plain_push_address_is_never_a_stream() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", Reply::WebSocket(Vec::new()));
    harness.stream_answer = Some(accepted());
    harness.input(Input::PushOverStream(true));
    let records = harness.open_channel("http://site.example/live/");
    assert!(asked(&records).is_empty(), "{records:?}");
    assert!(has_event(&records, "PushConnected"), "{records:?}");
    assert_eq!(paths(&mut harness, "site.example:80"), ["/live/channel.ttx", "/live/push"]);
}

/// REQ-6984 item 3: an answer nobody asked for changes nothing.
#[test]
fn an_answer_to_no_request_is_ignored() {
    let mut harness = secure_live(&[], false, None);
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    harness.open_channel("https://site.example/live/");
    let subprotocol = PUSH_SUBPROTOCOL.as_bytes();
    harness.input(Input::PushStream(neoteletext_web::agent::PushStream::Unavailable));
    harness.input(Input::PushStream(neoteletext_web::agent::PushStream::Answered {
        peer: PUBLIC,
        status: 200,
        subprotocol: Some(subprotocol),
    }));
    let records = harness.take();
    assert!(events(&records).is_empty(), "{records:?}");
    assert!(harness.replay().is_connected(ConnectionId::Push), "the HTTP/1.1 connection stands");
    assert_eq!(upgrades(&mut harness), 1);
}

/// REQ-6984 item 2, DEC-6979 item 3: a stream is asked for only when the
/// server named HTTP/3 on the push address's own port with its settings.
/// With no field, another port, another host, `clear` or a malformed
/// field, the push connection is the HTTP/1.1 one, though the port can
/// present a stream.
#[test]
fn a_stream_is_asked_for_only_where_the_server_named_http_3_on_its_own_port() {
    for headers in [
        &[][..],
        &["Alt-Svc: h3=\":8443\""],
        &["Alt-Svc: h3=\"other.example:443\""],
        &["Alt-Svc: clear"],
        &["Alt-Svc: h2=\":443\""],
        &["Alt-Svc: h3=:443"],
        &["Alt-Svc: h3=\":443\"; ma=0"],
    ] {
        let mut harness = secure_live(headers, true, Some(accepted()));
        harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
        let records = harness.open_channel("https://site.example/live/");
        assert!(asked(&records).is_empty(), "{headers:?}: {records:?}");
        assert!(has_event(&records, "PushConnected"), "{headers:?}: {records:?}");
        assert_eq!(upgrades(&mut harness), 1, "{headers:?}");
    }
}

/// REQ-6983, REQ-6984 item 2: what the server named is fresh for as long
/// as it said (RFC 7838 section 3.1); a reconnection after that is over
/// HTTP/1.1.
#[test]
fn a_named_http_3_is_used_only_while_it_is_fresh() {
    let mut harness = secure_live(&["Alt-Svc: h3=\":443\"; ma=30"], true, Some(accepted()));
    harness.replay().reply(SECURE, Reply::WebSocket(Vec::new()));
    let records = harness.open_channel("https://site.example/live/");
    assert_eq!(asked(&records).len(), 1, "{records:?}");
    // Lost within the thirty seconds: a stream again.
    harness.replay().drop_connection(ConnectionId::Push);
    harness.settle();
    harness.advance(2 * SECOND);
    assert_eq!(asked(&harness.take()).len(), 1);
    // Lost after them: the HTTP/1.1 handshake.
    harness.advance(30 * SECOND);
    harness.replay().inject(ConnectionId::Push, &server_frame(0xA, b""));
    harness.replay().drop_connection(ConnectionId::Push);
    harness.settle();
    harness.take();
    harness.advance(PUSH_RETRY_MAX_US);
    let later = harness.take();
    assert!(asked(&later).is_empty(), "{later:?}");
    assert_eq!(upgrades(&mut harness), 1);
}
