//! Tests of the user agent driven through the in-memory replay transport
//! (REQ-WEB-001, REQ-WEB-013's exact replay), including channels
//! (REQ-WEB-018, REQ-WEB-019, REQ-WEB-024, REQ-WEB-026, REQ-WEB-027) and
//! redirects within a channel (REQ-WEB-021).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, arithmetic on small values it chose, where an overflow would also fail it and `panic!` on outcomes it rules out"
)]

mod common;

use std::string::{String, ToString};
use std::vec::Vec;

use common::{Counter, Replay, Reply, assert_profile_headers, header_value, response};
#[cfg(feature = "push")]
use common::{client_frames, server_frame};
use neoteletext_core::types::{Instant, PageAddress, SubCode};
#[cfg(feature = "push")]
use neoteletext_web::agent::InputMessage;
use neoteletext_web::agent::{Config, Input, Output, ResourceRole, Source, Storage, Subscription, UserAgent};
#[cfg(feature = "std")]
use neoteletext_web::channel::ChannelFile;
use neoteletext_web::channel::SETTINGS_FILE;
#[cfg(feature = "push")]
use neoteletext_web::consts::{
    CLOSE_BROKE_PROTOCOL, CLOSE_RESTARTING, CLOSE_RETRY_LAST, CLOSE_STOP_FIRST, CLOSE_STOP_LAST, CLOSE_WRONG_VERSION,
};
use neoteletext_web::http::MediaType;
use neoteletext_web::port::{ConnectionId, Pump, Security};
#[cfg(feature = "push")]
use neoteletext_web::records::redirect::{RedirectRecord, encode};
#[cfg(feature = "push")]
use neoteletext_web::records::write_packets;
use neoteletext_web::subpages::PartNumber;
use neoteletext_web::url::IpAddress;

const PUBLIC: IpAddress = IpAddress::V4([93, 184, 216, 34]);
const PUBLIC_2: IpAddress = IpAddress::V4([93, 184, 216, 35]);
const PRIVATE: IpAddress = IpAddress::V4([192, 168, 1, 10]);
const SECOND: u64 = 1_000_000;

const TTI: &[u8] = b"PN,10000\r\nOL,1,HELLO\r\n";

#[derive(Debug, Clone, PartialEq)]
enum Rec {
    Page { source: Source, media: MediaType, url: String, secure: bool, bytes: Vec<u8> },
    Event(String),
    Timer(Instant),
}

struct Harness {
    // Boxed, as a port keeps it: it holds many addresses of 8,000 octets
    // (REQ-1661), and a test thread has 2 MB of stack.
    agent: Box<UserAgent<'static, Counter>>,
    pump: Pump<Replay>,
    now: Instant,
    out: Vec<Rec>,
}

fn record(output: Output<'_>) -> Rec {
    match output {
        Output::PageBytes(page) => Rec::Page {
            source: page.source,
            media: page.media,
            url: page.url.to_string(),
            secure: page.secure,
            bytes: page.bytes.to_vec(),
        },
        Output::Event(event) => Rec::Event(format!("{event:?}")),
        Output::Timer { at } => Rec::Timer(at),
        other => panic!("network output reached the application: {other:?}"),
    }
}

impl Harness {
    fn new(config: Config, replay: Replay) -> Self {
        let storage: &'static mut Storage = Box::leak(Box::new(Storage::new()));
        let agent = UserAgent::new(config, storage.buffers(), Counter(0));
        Self { agent: Box::new(agent), pump: Pump::new(replay), now: Instant(1_000 * SECOND), out: Vec::new() }
    }

    fn handle(&mut self, input: Input<'_>) {
        let Self { agent, pump, out, now } = self;
        agent.handle(*now, input, &mut |output| match output.net() {
            Some(net) => pump.execute(net),
            None => out.push(record(output)),
        });
    }

    fn settle(&mut self) {
        let mut buffer = [0u8; 4096];
        for _ in 0..10_000 {
            let mut progressed = false;
            for connection in [ConnectionId::Fetch, ConnectionId::Push] {
                if let Some(input) = self.pump.poll(connection, &mut buffer) {
                    let input = copy_net(input);
                    self.handle(Input::Net(input.as_net()));
                    progressed = true;
                }
            }
            if !progressed {
                return;
            }
        }
        panic!("the exchange did not settle");
    }

    fn input(&mut self, input: Input<'_>) {
        self.handle(input);
        self.settle();
    }

    fn advance(&mut self, microseconds: u64) {
        self.now = Instant(self.now.0 + microseconds);
        self.input(Input::Tick);
    }

    fn take(&mut self) -> Vec<Rec> {
        std::mem::take(&mut self.out)
    }

    fn replay(&mut self) -> &mut Replay {
        &mut self.pump.transport
    }

    fn open(&mut self, address: &str) -> Vec<Rec> {
        self.input(Input::OpenService(address));
        self.take()
    }

    fn open_channel(&mut self, address: &str) -> Vec<Rec> {
        self.input(Input::OpenChannel(address));
        self.take()
    }

    /// Requests a page and lets the default rate limit's interval pass.
    fn fetch(&mut self, mpp: u16) -> Vec<Rec> {
        self.input(Input::FetchPage { page: PageAddress::from_mpp(mpp).unwrap(), subcode: None });
        self.advance(neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US);
        self.take()
    }
}

/// An owned copy of a network input, so the receive buffer is free while
/// the agent handles it.
struct NetCopy {
    kind: u8,
    connection: ConnectionId,
    peer: Option<IpAddress>,
    bytes: Vec<u8>,
}

fn copy_net(input: neoteletext_web::port::NetInput<'_>) -> NetCopy {
    use neoteletext_web::port::NetInput;
    match input {
        NetInput::Connected(connection, peer) => NetCopy { kind: 0, connection, peer: Some(peer), bytes: Vec::new() },
        NetInput::ConnectFailed(connection) => NetCopy { kind: 1, connection, peer: None, bytes: Vec::new() },
        NetInput::Received(connection, bytes) => NetCopy { kind: 2, connection, peer: None, bytes: bytes.to_vec() },
        NetInput::Closed(connection) => NetCopy { kind: 3, connection, peer: None, bytes: Vec::new() },
        NetInput::Unsent(connection, unsent) => {
            NetCopy { kind: 4, connection, peer: None, bytes: unsent.to_le_bytes().to_vec() }
        }
    }
}

impl NetCopy {
    fn as_net(&self) -> neoteletext_web::port::NetInput<'_> {
        use neoteletext_web::port::NetInput;
        match self.kind {
            0 => NetInput::Connected(self.connection, self.peer.unwrap()),
            1 => NetInput::ConnectFailed(self.connection),
            2 => NetInput::Received(self.connection, &self.bytes),
            3 => NetInput::Closed(self.connection),
            _ => NetInput::Unsent(self.connection, usize::from_le_bytes(self.bytes.as_slice().try_into().unwrap())),
        }
    }
}

fn events(records: &[Rec]) -> Vec<&str> {
    records.iter().filter_map(|r| if let Rec::Event(e) = r { Some(e.as_str()) } else { None }).collect()
}

fn has_event(records: &[Rec], prefix: &str) -> bool {
    events(records).iter().any(|e| e.starts_with(prefix))
}

fn pages(records: &[Rec]) -> Vec<&Rec> {
    records.iter().filter(|r| matches!(r, Rec::Page { .. })).collect()
}

fn descriptor(extra: &str) -> Vec<u8> {
    format!(
        r#"{{"name":"Test","initial_page":"100","page_template":"/p/{{mpp}}",
            "services":[{{"name":"other","display_name":"Other","descriptor":"http://other.example/d.json"}}]{extra}}}"#
    )
    .into_bytes()
}

fn json_response(body: &[u8]) -> Reply {
    Reply::Bytes(response("200 OK", &["Content-Type: application/json"], body))
}

fn tti_response(headers: &[&str]) -> Reply {
    let mut all = std::vec!["Content-Type: text/x-teletext-tti"];
    all.extend_from_slice(headers);
    Reply::Bytes(response("200 OK", &all, TTI))
}

/// The configuration of a port that follows redirects out of the service,
/// as `neottx` does for a service opened by its descriptor: the default
/// keeps them inside (REQ-SEC-020).
fn following() -> Config {
    Config { redirects_within_channel: false, ..Config::default() }
}

fn plain_service() -> Harness {
    let mut replay = Replay::new(&[("tt.example", PUBLIC), ("evil.example", PRIVATE)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    let mut harness = Harness::new(following(), replay);
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "ServiceReady { name: \"Test\""), "{records:?}");
    harness
}

/// REQ-492: a page fetched as a TTI file. REQ-493: a held validator makes the next fetch conditional, and a 304
/// marks the page current at no cost of a body.
#[test]
fn descriptor_page_and_conditional_request() {
    let mut harness = plain_service();
    harness.replay().reply(
        "tt.example:80",
        tti_response(&["ETag: \"v1\"", "Set-Cookie: session=v1-abc; Path=/; HttpOnly", "Cache-Control: no-cache"]),
    );
    harness
        .replay()
        .reply("tt.example:80", Reply::Bytes(b"HTTP/1.1 304 Not Modified\r\nETag: \"v1\"\r\n\r\n".to_vec()));
    let records = harness.fetch(0x100);
    let got = pages(&records);
    assert_eq!(got.len(), 1);
    let Rec::Page { source, media, url, secure, bytes } = got[0] else { unreachable!() };
    assert_eq!(*media, MediaType::Tti);
    assert_eq!(url, "http://tt.example/p/100");
    assert!(!secure, "came over plain HTTP: the indicator must show");
    assert_eq!(bytes, TTI);
    assert!(matches!(source, Source::Page { .. }));

    // The same page again: one conditional request, no body.
    harness.advance(SECOND);
    let records = harness.fetch(0x100);
    assert_eq!(pages(&records), Vec::<&Rec>::new());
    assert!(has_event(&records, "PageCurrent"), "{records:?}");
    let requests = harness.replay().requests_to("tt.example:80");
    let last = requests.last().unwrap();
    assert_eq!(header_value(last, "If-None-Match").as_deref(), Some("\"v1\""));
    // VS-201: the `Set-Cookie` the page came with is ignored, and no
    // request carries a `Cookie`.
    assert_eq!(header_value(last, "Cookie"), None);
    // One persistent connection served all three requests.
    assert_eq!(harness.replay().connects.len(), 1);
    assert_profile_headers(harness.replay(), &[]);
}

/// REQ-497: a connection made without encryption is reported as an event.
#[test]
fn unencrypted_connection_is_reported() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    let mut harness = Harness::new(Config::default(), replay);
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "Unencrypted { connection: Fetch, host: \"tt.example\" }"));
}

/// REQ-493: the caching headers are honoured.
#[test]
fn fresh_pages_are_not_fetched_again() {
    let mut harness = plain_service();
    harness.replay().reply("tt.example:80", tti_response(&["Cache-Control: max-age=60"]));
    harness.fetch(0x100);
    harness.advance(10 * SECOND);
    let records = harness.fetch(0x100);
    assert!(has_event(&records, "PageCurrent"));
    assert_eq!(harness.replay().requests_to("tt.example:80").len(), 2, "no request while fresh");
}

/// REQ-492: 404 is "not in this service", and other failures are reported as events.
#[test]
fn not_found_and_failures_leave_pages_alone() {
    let mut harness = plain_service();
    harness.replay().reply("tt.example:80", Reply::Bytes(response("404 Not Found", &[], b"")));
    harness.replay().reply("tt.example:80", Reply::Bytes(response("500 Oops", &["Retry-After: 30"], b"")));
    harness.replay().reply(
        "tt.example:80",
        Reply::Bytes(response("200 OK", &["Content-Type: text/x-teletext-tti", "Content-Encoding: gzip"], b"x")),
    );
    harness.replay().reply("tt.example:80", Reply::Bytes(response("200 OK", &["Content-Type: text/html"], b"<p>")));
    assert!(has_event(&harness.fetch(0x100), "PageNotInService"));
    harness.advance(SECOND);
    assert!(has_event(
        &harness.fetch(0x101),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(1) }, subcode: None, failure: Status(500)"
    ));
    // Retry-After holds the next fetch back.
    let records = harness.fetch(0x102);
    assert!(pages(&records).is_empty() && !has_event(&records, "FetchFailed"));
    assert!(
        records.contains(&Rec::Timer(Instant(harness.now.0 + 29 * SECOND)))
            || records.iter().any(|r| matches!(r, Rec::Timer(at) if at.0 >= harness.now.0 + 29 * SECOND))
    );
    harness.advance(30 * SECOND);
    assert!(has_event(
        &harness.take(),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(2) }, subcode: None, failure: Http(Compressed)"
    ));
    harness.advance(SECOND);
    assert!(has_event(
        &harness.fetch(0x103),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(3) }, subcode: None, failure: MediaType(Html)"
    ));
}

/// Decision E.103: the port is told its timer when its deadline changes
/// and after a tick, not after every input. Before this every input was
/// followed by one while any deadline stood, waking the port's loop twice
/// for each.
#[test]
fn the_timer_is_told_on_a_change_and_after_a_tick() {
    let timers = |records: &[Rec]| records.iter().filter(|record| matches!(record, Rec::Timer(_))).count();
    let mut harness = plain_service();
    harness.replay().reply("tt.example:80", Reply::Bytes(response("500 Oops", &["Retry-After: 30"], b"")));
    assert!(has_event(&harness.fetch(0x101), "FetchFailed"));
    // Retry-After holds the next fetch back: a deadline stands.
    let records = harness.fetch(0x102);
    assert!(records.iter().any(|record| matches!(record, Rec::Timer(at) if at.0 >= harness.now.0 + 29 * SECOND)));
    // Inputs that move no deadline are told no timer.
    harness.input(Input::MeasureRoundTrip(false));
    harness.input(Input::MeasureRoundTrip(false));
    assert_eq!(timers(&harness.take()), 0, "nothing changed");
    // A tick spends the deadline the port had: it is told the next, once.
    harness.advance(SECOND);
    assert_eq!(timers(&harness.take()), 1, "told again after a tick");
}

/// REQ-497: a page response over its limit is abandoned with a diagnostic. REQ-1904: a reader refuses a file
/// past its bound.
#[test]
fn size_limit_abandons_the_response() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    replay.reply(
        "tt.example:80",
        Reply::Bytes(response("200 OK", &["Content-Type: text/x-teletext-tti"], &[b'x'; 2000])),
    );
    let config = Config { page_limit: 1000, ..Config::default() };
    let mut harness = Harness::new(config, replay);
    harness.open("http://tt.example/service.json");
    assert!(has_event(
        &harness.fetch(0x100),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(0) }, subcode: None, failure: Http(BodyTooLarge)"
    ));
}

#[test]
fn time_limit() {
    let mut harness = plain_service();
    harness.replay().reply("tt.example:80", Reply::Silent);
    let records = harness.fetch(0x100);
    assert!(records.iter().any(|r| matches!(r, Rec::Timer(_))));
    harness.advance(31 * SECOND);
    assert!(has_event(
        &harness.take(),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(0) }, subcode: None, failure: Timeout"
    ));
}

/// REQ-493: fetches are held to the configured rate.
#[test]
fn rate_limit() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    replay.reply("tt.example:80", tti_response(&[]));
    replay.reply("tt.example:80", tti_response(&[]));
    let config = Config { fetch_interval_us: SECOND, ..Config::default() };
    let mut harness = Harness::new(config, replay);
    harness.open("http://tt.example/service.json");
    harness.advance(SECOND);
    harness.input(Input::FetchPage { page: PageAddress::from_mpp(0x100).unwrap(), subcode: None });
    harness.input(Input::FetchPage { page: PageAddress::from_mpp(0x101).unwrap(), subcode: None });
    let records = harness.take();
    assert_eq!(pages(&records).len(), 1, "the second waits");
    assert!(records.contains(&Rec::Timer(Instant(harness.now.0 + SECOND))));
    harness.advance(SECOND / 2);
    assert_eq!(pages(&harness.take()), Vec::<&Rec>::new());
    harness.advance(SECOND / 2);
    assert_eq!(pages(&harness.take()).len(), 1);
}

/// REQ-492: at most five redirects are followed.
#[test]
fn redirects_are_followed_up_to_five() {
    let mut harness = plain_service();
    for (status, location) in [("301 Moved", "/a"), ("302 Found", "http://tt.example/b"), ("307 Temporary", "c")] {
        harness
            .replay()
            .reply("tt.example:80", Reply::Bytes(response(status, &[&format!("Location: {location}")], b"moved")));
    }
    harness.replay().reply("tt.example:80", tti_response(&[]));
    let records = harness.fetch(0x100);
    let Rec::Page { url, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!(url, "http://tt.example/c");

    for hop in 0..6 {
        let location = format!("Location: /loop{hop}");
        harness.replay().reply("tt.example:80", Reply::Bytes(response("302 Found", &[&location], b"")));
    }
    harness.advance(SECOND);
    assert!(has_event(
        &harness.fetch(0x101),
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(1) }, subcode: None, failure: TooManyRedirects"
    ));
}

/// Finding F-44 (decision E.114 item 4; the review's probe B): a
/// `Location` that is empty, is not UTF-8, or names the request's own
/// address with the method unchanged leads nowhere, and is a fault after
/// one request. It resolved to the request's own address and spent six
/// requests, outside the fetch interval.
/// REQ-492: such a redirect is a fault after that one request.
#[test]
fn a_redirect_to_nowhere_is_a_fault_after_one_request() {
    let empty = b"HTTP/1.1 302 Found\r\nLocation: \r\nContent-Length: 0\r\n\r\n".to_vec();
    let not_utf8 = b"HTTP/1.1 302 Found\r\nLocation: /p/\xff\r\nContent-Length: 0\r\n\r\n".to_vec();
    let itself = response("302 Found", &["Location: http://tt.example/p/100"], b"");
    let relative_itself = response("307 Temporary Redirect", &["Location: /p/100"], b"");
    let see_itself = response("303 See Other", &["Location: /p/100"], b"");
    for reply in [empty, not_utf8, itself, relative_itself, see_itself] {
        let mut harness = plain_service();
        for _ in 0..6 {
            harness.replay().reply("tt.example:80", Reply::Bytes(reply.clone()));
        }
        let records = harness.fetch(0x100);
        let shown = String::from_utf8_lossy(&reply).into_owned();
        assert!(has_event(&records, "FetchFailed {"), "{shown}: {records:?}");
        assert!(
            records.iter().any(|record| format!("{record:?}").contains("failure: BadRedirect")),
            "{shown}: {records:?}"
        );
        assert_eq!(harness.replay().requests_to("tt.example:80").len(), 2, "{shown}: the descriptor and one request");
    }
}

/// Finding F-75 (RFC 9110 section 15.2; the review's probe D): interim
/// 1xx heads before the final one are read past, on the fetch and the
/// push connections. Every 1xx but 101 failed the exchange, so a server
/// sending `103 Early Hints` was unusable.
#[test]
fn interim_responses_before_the_final_one_are_read_past() {
    let mut harness = plain_service();
    let mut reply =
        b"HTTP/1.1 103 Early Hints\r\nLink: </style>; rel=preload\r\n\r\nHTTP/1.1 100 Continue\r\n\r\n".to_vec();
    reply.extend(response("200 OK", &["Content-Type: text/x-teletext-tti"], TTI));
    harness.replay().reply("tt.example:80", Reply::Bytes(reply));
    let records = harness.fetch(0x100);
    assert_eq!(pages(&records).len(), 1, "{records:?}");
    assert!(!has_event(&records, "FetchFailed"), "{records:?}");
}

/// Finding F-75: the same on a push connection's handshake.
#[test]
#[cfg(feature = "push")]
fn interim_responses_before_a_push_handshake_are_read_past() {
    let mut harness = push_service("ws://tt.example/push");
    let interim = b"HTTP/1.1 103 Early Hints\r\n\r\n".to_vec();
    harness.replay().reply("tt.example:80", Reply::WebSocketWith { interim, headers: String::new() });
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "PushConnected"), "{records:?}");
}

#[test]
fn redirects_to_blocked_addresses_are_refused() {
    let mut harness = plain_service();
    // A name that resolves to a private address: checked on the peer.
    harness
        .replay()
        .reply("tt.example:80", Reply::Bytes(response("302 Found", &["Location: http://evil.example/x"], b"")));
    let records = harness.fetch(0x100);
    assert!(has_event(&records, "Blocked { connection: Fetch, host: \"evil.example\" }"), "{records:?}");
    assert!(has_event(
        &records,
        "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(0) }, subcode: None, failure: Blocked"
    ));
    assert!(harness.replay().requests_to("evil.example:80").is_empty(), "no request reached it");
    // Address literals are refused before connecting.
    for literal in [
        "http://127.0.0.1/x",
        "http://10.1.2.3/",
        "http://[::1]/",
        "http://[::ffff:192.168.0.1]/",
        "http://169.254.169.254/",
    ] {
        harness.advance(SECOND);
        harness
            .replay()
            .reply("tt.example:80", Reply::Bytes(response("302 Found", &[&format!("Location: {literal}")], b"")));
        let records = harness.fetch(0x101);
        assert!(has_event(&records, "Blocked"), "{literal} {records:?}");
        harness.input(Input::Tick);
    }
    assert!(!harness.replay().connects.iter().any(|(_, host, _, _)| host == "127.0.0.1" || host == "10.1.2.3"));
}

#[test]
fn allowed_local_service() {
    let mut replay = Replay::new(&[("tt.local", PRIVATE)]);
    replay.reply("tt.local:80", json_response(&descriptor("")));
    let mut config = Config::default();
    config.allowed_local[0] = Some(PRIVATE);
    let mut harness = Harness::new(config, replay);
    assert!(has_event(&harness.open("http://tt.local/service.json"), "ServiceReady"));
    let mut replay = Replay::new(&[("tt.local", PRIVATE)]);
    replay.reply("tt.local:80", json_response(&descriptor("")));
    let mut harness = Harness::new(Config::default(), replay);
    assert!(has_event(&harness.open("http://tt.local/service.json"), "DescriptorFailed(Blocked)"));
}

fn secure_service() -> Harness {
    let mut replay = Replay::new(&[("secure.example", PUBLIC)]);
    let json = br#"{"name":"S","initial_page":"100","page_template":"/p/{mpp}"}"#;
    replay.reply("secure.example:443", json_response(json));
    let mut harness = Harness::new(Config::default(), replay);
    let records = harness.open("https://secure.example/d.json");
    assert!(has_event(&records, "ServiceReady { name: \"S\""), "{records:?}");
    assert_eq!(harness.replay().connects[0].3, Security::Tls);
    harness
}

#[test]
fn no_downgrade_after_https() {
    let mut harness = secure_service();
    // A redirect from HTTPS to plain HTTP, even on another host.
    harness
        .replay()
        .reply("secure.example:443", Reply::Bytes(response("301 Moved", &["Location: http://tt.example/p"], b"")));
    let records = harness.fetch(0x100);
    assert!(has_event(&records, "DowngradeRefused { host: \"tt.example\" }"), "{records:?}");
    // The host itself, over plain HTTP, for the rest of the session.
    harness.advance(SECOND);
    let records = harness.open("http://secure.example/d.json");
    assert!(has_event(&records, "DowngradeRefused { host: \"secure.example\" }"));
    assert!(has_event(&records, "DescriptorFailed(Downgrade)"));
    assert!(!harness.replay().connects.iter().any(|(_, _, port, _)| *port == 80));
}

/// REQ-420 (REQ-SEC-007), finding F-43 (decision E.114 item 3): when the memory of hosts reached
/// over HTTPS is full, the host least recently seen is forgotten, so the
/// rule against downgrade holds for the hosts in use. The memory filled
/// silently, and the 33rd host was never remembered.
#[test]
fn the_secure_host_memory_forgets_the_host_least_recently_seen() {
    let hosts: Vec<String> =
        (0..=neoteletext_web::consts::SECURE_HOSTS).map(|index| format!("h{index}.example")).collect();
    let resolve: Vec<(&str, IpAddress)> = hosts.iter().map(|host| (host.as_str(), PUBLIC)).collect();
    let mut harness = Harness::new(Config::default(), Replay::new(&resolve));
    let json = br#"{"name":"S","initial_page":"100","page_template":"/p/{mpp}"}"#;
    for host in &hosts {
        harness.replay().reply(&format!("{host}:443"), json_response(json));
        let records = harness.open(&format!("https://{host}/d.json"));
        assert!(has_event(&records, "ServiceReady"), "{host}: {records:?}");
        harness.advance(SECOND);
    }
    let newest = hosts.last().unwrap();
    let records = harness.open(&format!("http://{newest}/d.json"));
    assert!(has_event(&records, "DowngradeRefused"), "the newest host is remembered: {records:?}");
    harness.advance(SECOND);
    let oldest = &hosts[0];
    harness.replay().reply(&format!("{oldest}:80"), json_response(json));
    let records = harness.open(&format!("http://{oldest}/d.json"));
    assert!(!has_event(&records, "DowngradeRefused"), "the oldest host is forgotten: {records:?}");
}

/// REQ-497: without TLS an HTTPS address is unreachable.
#[test]
fn tls_unavailable() {
    let config = Config { tls_available: false, ..Config::default() };
    let mut harness = Harness::new(config, Replay::new(&[("secure.example", PUBLIC)]));
    assert!(has_event(&harness.open("https://secure.example/d.json"), "DescriptorFailed(TlsUnavailable)"));
    assert_eq!(harness.replay().connects, []);
}

#[test]
fn descriptor_rejections() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC), ("secure.example", PUBLIC_2)]);
    replay.reply("tt.example:80", json_response(br#"{"name":"S","initial_page":"100","page_template":"/{page}"}"#));
    replay.reply("tt.example:80", Reply::Bytes(response("200 OK", &["Content-Type: text/html"], b"{}")));
    replay.reply(
        "secure.example:443",
        json_response(br#"{"name":"S","initial_page":"100","page_template":"http://secure.example/{mpp}"}"#),
    );
    let mut harness = Harness::new(Config::default(), replay);
    assert!(has_event(&harness.open("http://tt.example/a.json"), "DescriptorRejected(Placeholder)"));
    harness.advance(SECOND);
    assert!(has_event(&harness.open("http://tt.example/b.json"), "DescriptorFailed(MediaType(Html))"));
    harness.advance(SECOND);
    assert!(has_event(&harness.open("https://secure.example/c.json"), "DescriptorRejected(Downgrade(PageTemplate))"));
    assert!(has_event(&harness.open("ftp://x.example/"), "DescriptorFailed(Address(Scheme))"));
}

#[test]
fn links_to_other_services() {
    let mut harness = plain_service();
    harness.input(Input::FollowLink { service: "nowhere", page: PageAddress::from_mpp(0x100).unwrap(), subcode: None });
    assert!(has_event(&harness.take(), "LinkUnknown"));
    harness.replay().resolve.insert("other.example".to_string(), PUBLIC_2);
    harness.replay().reply(
        "other.example:80",
        json_response(br#"{"name":"Other","initial_page":"200","page_template":"/x/{mpp}/{sub}"}"#),
    );
    harness.replay().reply("other.example:80", tti_response(&[]));
    harness.input(Input::FollowLink {
        service: "other",
        page: PageAddress::from_mpp(0x234).unwrap(),
        subcode: Some(SubCode::new(2)),
    });
    harness.advance(SECOND);
    let records = harness.take();
    assert!(has_event(&records, "ServiceReady { name: \"Other\""), "{records:?}");
    let Rec::Page { url, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!(url, "http://other.example/x/234/0002");
}

#[cfg(feature = "push")]
fn push_service(address: &str) -> Harness {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    let extra = format!(r#","push":{{"address":"{address}"}}"#);
    replay.reply("tt.example:80", json_response(&descriptor(&extra)));
    Harness::new(Config::default(), replay)
}

#[test]
#[cfg(feature = "push")]
fn websocket_push() {
    let mut harness = push_service("ws://tt.example/push");
    let packet = [0x15u8; 42];
    let mut frames = server_frame(0x2, &[packet, packet].concat());
    frames.extend(server_frame(0x1, TTI));
    frames.extend(server_frame(0x9, b"hi"));
    harness.replay().reply("tt.example:80", Reply::WebSocket(frames));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "PushConnected"), "{records:?}");
    let delivered = pages(&records);
    // DEC-1459, REQ-1462: the text message is never read as a page; it is
    // discarded and counted as one invalid packet.
    assert_eq!(delivered.len(), 1);
    assert!(
        matches!(delivered[0], Rec::Page { source: Source::Push, media: MediaType::T42, bytes, .. } if bytes.len() == 84)
    );
    assert!(has_event(&records, "InvalidPackets { total: 1 }"), "{records:?}");
    let push_frames: Vec<_> = harness
        .replay()
        .sent
        .iter()
        .filter(|(c, _)| *c == ConnectionId::Push)
        .skip(1)
        .flat_map(|(_, f)| client_frames(f))
        .collect();
    assert_eq!(push_frames[0], (0x1, b"subscribe all".to_vec()));
    assert_eq!(push_frames[1], (0xA, b"hi".to_vec()), "ping answered with pong");
    let handshake = &harness.replay().requests_to("tt.example:80")[1];
    assert_eq!(header_value(handshake, "Sec-WebSocket-Version").as_deref(), Some("13"));
    // Changing the subscription.
    let wanted = [PageAddress::from_mpp(0x100).unwrap(), PageAddress::from_mpp(0x1A0).unwrap()];
    harness.input(Input::Subscribe(Subscription::Pages(&wanted)));
    let (_, last) = harness.replay().sent.last().unwrap().clone();
    assert_eq!(client_frames(&last).last(), Some(&(0x1, b"subscribe 100 1A0".to_vec())));
    assert_profile_headers(harness.replay(), &[]);
}

/// DOC-1960, REQ-1688: a REDIRECT record on the push connection
/// reaches the application, in either form, and the agent moves nothing.
#[test]
#[cfg(feature = "push")]
fn a_channels_redirect_reaches_the_application() {
    let mut harness = push_service("ws://tt.example/push");
    let page = PageAddress::from_mpp(0x1A0).unwrap();
    let mut message = Vec::new();
    let mut packet = |packet: neoteletext_core::packet::Packet| message.extend_from_slice(&packet.bytes);
    encode(RedirectRecord::Page { page, subcode: SubCode::ANY, parameters: None }, 3, 1, &mut packet).unwrap();
    encode(RedirectRecord::Address("https://elsewhere.example/channel/302"), 3, 2, &mut packet).unwrap();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "Redirect(Page {"), "{records:?}");
    assert!(has_event(&records, "Redirect(Address(\"https://elsewhere.example/channel/302\"))"), "{records:?}");
    assert!(!has_event(&records, "InvalidPackets"), "{records:?}");
}

/// The delay each `PushLost` among `records` gave.
#[cfg(feature = "push")]
fn retries(records: &[Rec]) -> Vec<u64> {
    let lost = events(records);
    lost.iter()
        .filter_map(|event| event.strip_prefix("PushLost { retry_in_us: "))
        .map(|rest| rest.split_once(',').unwrap().0.parse().unwrap())
        .collect()
}

/// Whether a `PushLost` among `records` names `problem`.
#[cfg(feature = "push")]
fn lost_for(records: &[Rec], problem: &str) -> bool {
    events(records).iter().any(|event| event.starts_with("PushLost") && event.contains(problem))
}

/// DOC-1920, DEC-1759 lesson 10: each delay is in the upper
/// half of a ceiling that starts at a second and doubles to a minute.
#[test]
#[cfg(feature = "push")]
fn websocket_push_reconnects_at_random_under_a_doubling_ceiling() {
    let mut harness = push_service("ws://tt.example/push");
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x8, &1000u16.to_be_bytes())));
    harness.replay().reply("tt.example:80", Reply::Close);
    harness.replay().reply("tt.example:80", Reply::Close);
    let records = harness.open("http://tt.example/service.json");
    let within = |delay: u64, ceiling: u64| {
        let range = ceiling * SECOND / 2..=ceiling * SECOND;
        assert!(range.contains(&delay), "{delay} outside {range:?}");
    };
    let delays = retries(&records);
    assert_eq!(delays.len(), 1, "{records:?}");
    within(delays[0], 1);
    let (_, close) = harness.replay().sent.last().unwrap().clone();
    assert_eq!(client_frames(&close).last(), Some(&(0x8, 1000u16.to_be_bytes().to_vec())), "close answered");
    harness.advance(SECOND);
    let second = retries(&harness.take());
    assert_eq!(second.len(), 1);
    within(second[0], 2);
    harness.advance(2 * SECOND);
    let third = retries(&harness.take());
    assert_eq!(third.len(), 1);
    within(third[0], 4);
    assert_ne!(third[0], 4 * SECOND / 2, "drawn, not the bottom of the range");
    for _ in 0..8 {
        harness.replay().reply("tt.example:80", Reply::Close);
        harness.advance(60 * SECOND);
    }
    let last = *retries(&harness.take()).last().unwrap();
    within(last, 60);
}

/// DOC-1920, DEC-1759 lesson 10: a stop code ends the
/// connection for good, and a retry code, or a code nobody defined outside
/// the stop range, is tried again.
#[test]
#[cfg(feature = "push")]
fn a_stop_code_stops_and_a_retry_code_retries() {
    for stop in [CLOSE_BROKE_PROTOCOL, CLOSE_WRONG_VERSION, CLOSE_STOP_LAST] {
        let mut harness = push_service("ws://tt.example/push");
        harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x8, &stop.to_be_bytes())));
        let records = harness.open("http://tt.example/service.json");
        assert!(has_event(&records, &format!("PushStopped(Closed({stop}))")), "{records:?}");
        assert!(!has_event(&records, "PushLost"), "{records:?}");
        let connects = harness.replay().connects.len();
        harness.advance(120 * SECOND);
        assert_eq!(harness.replay().connects.len(), connects, "no reconnection after {stop}");
    }
    for retry in [CLOSE_RESTARTING, CLOSE_RETRY_LAST, 1000, 1012, 1006, CLOSE_STOP_FIRST - 1, 4200, 4999] {
        let mut harness = push_service("ws://tt.example/push");
        harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x8, &retry.to_be_bytes())));
        harness.replay().reply("tt.example:80", Reply::Close);
        let records = harness.open("http://tt.example/service.json");
        assert!(has_event(&records, "PushLost"), "{retry}: {records:?}");
        assert!(!has_event(&records, "PushStopped"), "{retry}: {records:?}");
        let connects = harness.replay().connects.len();
        harness.advance(2 * SECOND);
        assert_eq!(harness.replay().connects.len(), connects + 1, "reconnected after {retry}");
    }
}

/// DEC-1759 lesson 14: a server that answers with no subprotocol, or
/// another's, is the wrong server or the wrong version, and refuses are
/// final; a limit or a server's own fault may pass, so they are tried again.
#[test]
#[cfg(feature = "push")]
fn a_handshake_refused_stops_and_a_passing_fault_retries() {
    for named in [None, Some("neottx.6")] {
        let mut harness = push_service("ws://tt.example/push");
        harness.replay().reply("tt.example:80", Reply::WebSocketNamed(named));
        let records = harness.open("http://tt.example/service.json");
        assert!(has_event(&records, "PushStopped(WrongProtocol)"), "{records:?}");
    }
    for status in [400_u16, 403, 404, 426] {
        let mut harness = push_service("ws://tt.example/push");
        let response = format!("HTTP/1.1 {status} No\r\nContent-Length: 0\r\n\r\n").into_bytes();
        harness.replay().reply("tt.example:80", Reply::Bytes(response));
        let records = harness.open("http://tt.example/service.json");
        assert!(has_event(&records, &format!("PushStopped(Refused({status}))")), "{records:?}");
    }
    for status in [429_u16, 500, 503] {
        let mut harness = push_service("ws://tt.example/push");
        let response = format!("HTTP/1.1 {status} No\r\nContent-Length: 0\r\n\r\n").into_bytes();
        harness.replay().reply("tt.example:80", Reply::Bytes(response));
        let records = harness.open("http://tt.example/service.json");
        assert!(has_event(&records, "PushLost"), "{status}: {records:?}");
        assert!(!has_event(&records, "PushStopped"), "{status}: {records:?}");
    }
}

#[test]
#[cfg(feature = "push")]
fn websocket_push_protocol_errors() {
    let mut harness = push_service("ws://tt.example/push");
    let mut frames = server_frame(0x2, &[0u8; 41]);
    frames.extend([0x82, 0x85, 0, 0, 0, 0, 1, 2, 3, 4, 5]); // masked server frame
    harness.replay().reply("tt.example:80", Reply::WebSocket(frames));
    let records = harness.open("http://tt.example/service.json");
    // REQ-1462: a ragged message is discarded and counted, ⌈41 ÷ 42⌉ = 1.
    assert!(has_event(&records, "InvalidPackets { total: 1 }"), "{records:?}");
    assert!(lost_for(&records, "Some(WebSocket(Protocol))"), "{records:?}");
    let (_, close) = harness.replay().sent.last().unwrap().clone();
    assert_eq!(client_frames(&close).last(), Some(&(0x8, 1002u16.to_be_bytes().to_vec())));
    // A wrong accept value fails the handshake.
    harness.replay().reply("tt.example:80", Reply::BadWebSocket);
    harness.advance(SECOND);
    assert!(lost_for(&harness.take(), "Some(WebSocket(Handshake))"));
}

/// DEC-1759 lesson 16, DOC-1952: a record of a type nobody
/// defined yet is skipped: not counted as invalid, and the connection stays,
/// so the next record is read.
#[test]
#[cfg(feature = "push")]
fn an_unknown_record_type_is_skipped() {
    const UNKNOWN_RECORD_TYPE: u8 = 200;
    let mut harness = push_service("ws://tt.example/push");
    let mut message = Vec::new();
    let mut packet = |packet: neoteletext_core::packet::Packet| message.extend_from_slice(&packet.bytes);
    write_packets(UNKNOWN_RECORD_TYPE, &[1, 2, 3], 3, 1, &mut packet).unwrap();
    encode(RedirectRecord::Address("https://elsewhere.example/channel/302"), 3, 2, &mut packet).unwrap();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "Redirect(Address(\"https://elsewhere.example/channel/302\"))"), "{records:?}");
    assert!(!has_event(&records, "InvalidPackets"), "{records:?}");
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

/// REQ-497: a push message over its limit is abandoned.
#[test]
#[cfg(feature = "push")]
fn websocket_push_oversize_message() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor(r#","push":{"address":"ws://tt.example/push"}"#)));
    replay.reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &[0u8; 420])));
    let config = Config { push_limit: 200, ..Config::default() };
    let mut harness = Harness::new(config, replay);
    let records = harness.open("http://tt.example/service.json");
    assert!(lost_for(&records, "Some(WebSocket(TooBig))"), "{records:?}");
}

#[test]
#[cfg(feature = "push")]
fn push_to_blocked_peer_is_refused() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC), ("push.example", PRIVATE)]);
    replay.reply("tt.example:80", json_response(&descriptor(r#","push":{"address":"ws://push.example/p"}"#)));
    let mut harness = Harness::new(Config::default(), replay);
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "Blocked { connection: Push, host: \"push.example\" }"), "{records:?}");
    assert_eq!(harness.replay().requests_to("push.example:80"), Vec::<Vec<u8>>::new());
}

/// VS-201: no cookie goes with the WebSocket upgrade, whatever the
/// channel's responses set (REQ-495 withdrawn with cookies).
#[test]
#[cfg(feature = "push")]
fn no_cookie_goes_with_the_websocket_upgrade() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    let json = descriptor(r#","push":{"address":"ws://tt.example/push"}"#);
    replay.reply(
        "tt.example:80",
        Reply::Bytes(response("200 OK", &["Content-Type: application/json", "Set-Cookie: s=v1; Path=/"], &json)),
    );
    replay.reply("tt.example:80", Reply::WebSocket(Vec::new()));
    let mut harness = Harness::new(Config::default(), replay);
    assert!(has_event(&harness.open("http://tt.example/service.json"), "PushConnected"));
    let handshake = &harness.replay().requests_to("tt.example:80")[1];
    assert_eq!(header_value(handshake, "Upgrade").as_deref(), Some("websocket"));
    assert_eq!(header_value(handshake, "Cookie"), None);
}

/// REQ-WEB-031: the port's product names the agent on every request and
/// on the push WebSocket's upgrade; with none, the engine names itself.
#[test]
#[cfg(feature = "push")]
fn the_user_agent_goes_on_requests_and_the_upgrade() {
    use neoteletext_web::user_agent::{Product, significant_version};
    let engine = significant_version(env!("CARGO_PKG_VERSION"));
    let teefax = Product { name: "Teefax", version: "1.4.2", family: "ESP-IDF", engine_own: false };
    for (product, expected) in
        [(Some(teefax), format!("Teefax/1 (ESP-IDF) neoteletext/{engine}")), (None, format!("neoteletext/{engine}"))]
    {
        let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
        let json = descriptor(r#","push":{"address":"ws://tt.example/push"}"#);
        replay.reply("tt.example:80", json_response(&json));
        replay.reply("tt.example:80", Reply::WebSocket(Vec::new()));
        let mut harness = Harness::new(Config { product, ..Config::default() }, replay);
        assert!(has_event(&harness.open("http://tt.example/service.json"), "PushConnected"));
        let requests = harness.replay().requests_to("tt.example:80");
        assert_eq!(requests.len(), 2, "the descriptor and the upgrade");
        for request in &requests {
            assert_eq!(header_value(request, "User-Agent").as_deref(), Some(expected.as_str()));
        }
    }
}

/// REQ-WEB-032 with REQ-SEC-006: through a proxy, the peer is the proxy
/// and may be local, and a local name is refused before anything is sent.
#[test]
fn through_a_proxy_local_names_are_refused_and_the_proxy_may_be_local() {
    let proxied = Config { proxied: true, ..Config::default() };
    let replay = Replay::new(&[("printer.local", PUBLIC)]);
    let mut harness = Harness::new(proxied, replay);
    let records = harness.open("http://printer.local/service.json");
    assert!(has_event(&records, "Blocked"), "{records:?}");
    assert!(harness.replay().connects.is_empty(), "nothing was sent to the proxy");
    // Finding F-17 (decision E.113): a name of one label and a private-use
    // suffix are local names too.
    for local in ["router", "nas.lan", "x.home.arpa"] {
        let mut harness = Harness::new(proxied, Replay::new(&[(local, PUBLIC)]));
        let records = harness.open(&format!("http://{local}/service.json"));
        assert!(has_event(&records, "Blocked"), "{local}: {records:?}");
        assert!(harness.replay().connects.is_empty(), "{local}: nothing was sent to the proxy");
    }
    // The proxy on this network: the peer the transport reports.
    let mut replay = Replay::new(&[("tt.example", PRIVATE)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    let mut harness = Harness::new(proxied, replay);
    assert!(has_event(&harness.open("http://tt.example/service.json"), "ServiceReady"));
    let mut replay = Replay::new(&[("tt.example", PRIVATE)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    let mut harness = Harness::new(Config::default(), replay);
    assert!(
        has_event(&harness.open("http://tt.example/service.json"), "Blocked"),
        "without a proxy the peer is the host"
    );
}

/// REQ-WEB-033: each answered request is reported, with its method,
/// address, status and size, for a developer's view of the channel.
#[test]
fn each_exchange_is_reported() {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor("")));
    let mut harness = Harness::new(Config::default(), replay);
    let records = harness.open("http://tt.example/service.json");
    let exchanged: Vec<&str> = events(&records).into_iter().filter(|event| event.starts_with("Exchanged")).collect();
    assert_eq!(exchanged.len(), 1, "{records:?}");
    assert!(
        exchanged[0]
            .starts_with("Exchanged { method: Get, url: \"http://tt.example/service.json\", status: 200, bytes: "),
        "{exchanged:?}"
    );
    harness.replay().reply("tt.example:80", tti_response(&[]));
    let records = harness.fetch(0x100);
    assert!(
        events(&records)
            .iter()
            .any(|event| event.starts_with("Exchanged { method: Get, url: \"http://tt.example/p/100\", status: 200")),
        "{records:?}"
    );
}

#[test]
fn stale_kept_alive_connection_is_retried_once_for_a_get() {
    let mut harness = plain_service();
    harness.replay().reply("tt.example:80", Reply::Close);
    harness.replay().reply("tt.example:80", tti_response(&[]));
    let records = harness.fetch(0x100);
    assert_eq!(pages(&records).len(), 1, "{records:?}");
    assert_eq!(harness.replay().connects.len(), 2, "a fresh connection for the retry");
}

// Decision E.61: a build without a feature answers whatever needs it with a
// value that says so, and never acts as if the missing part had worked.

#[test]
#[cfg(not(feature = "push"))]
fn push_not_compiled_in_is_reported_and_never_connects() {
    const REPORTED: &str = "PushNotCompiledIn";
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    replay.reply("tt.example:80", json_response(&descriptor(r#","push":{"address":"ws://tt.example/push"}"#)));
    let mut harness = Harness::new(Config::default(), replay);
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "ServiceReady"), "{records:?}");
    assert!(has_event(&records, REPORTED), "{records:?}");
    assert_eq!(records.last(), Some(&Rec::Event(REPORTED.to_string())), "no timer for a reconnection follows");
    harness.input(Input::Subscribe(Subscription::All));
    harness.advance(120 * SECOND);
    assert!(!has_event(&harness.take(), "Push"), "nothing more about push");
    assert!(
        !harness.replay().connects.iter().any(|(connection, ..)| *connection == ConnectionId::Push),
        "no push connection is opened"
    );
}

// ---------------------------------------------------------------------
// Channels (decision E.70, REQ-WEB-018 to REQ-WEB-027)
// ---------------------------------------------------------------------

fn site_harness(config: Config) -> Harness {
    let replay = Replay::new(&[("site.example", PUBLIC), ("other.example", PUBLIC_2)]);
    Harness::new(config, replay)
}

fn not_found() -> Reply {
    Reply::Bytes(response("404 Not Found", &["Content-Type: text/html"], b"<p>no</p>"))
}

fn request_path(request: &[u8]) -> String {
    String::from_utf8_lossy(request).split(' ').nth(1).unwrap_or_default().to_string()
}

fn paths(harness: &mut Harness, host: &str) -> Vec<String> {
    harness.replay().requests_to(host).iter().map(|request| request_path(request)).collect()
}

/// One T42 packet with a valid address: row 0 of magazine 1.
fn packet() -> Vec<u8> {
    let mut packet = std::vec![0x15, 0x15];
    packet.extend_from_slice(&[0x20; 40]);
    packet
}

fn ok(media: &str, body: &[u8]) -> Reply {
    let header = format!("Content-Type: {media}");
    Reply::Bytes(response("200 OK", &[header.as_str()], body))
}

/// The path of the entry file of the channel `site_harness` serves.
fn settings_path() -> String {
    format!("/news/{SETTINGS_FILE}")
}

#[test]
fn a_channel_with_settings_opens_from_its_folder() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", json_response(br#"{"name":"Test"}"#));
    harness.replay().reply("site.example:80", not_found());
    let records = harness.open_channel("http://site.example/news/");
    assert!(has_event(&records, "ServiceReady { name: \"Test\""), "{records:?}");
    assert!(!has_event(&records, "ChannelDefaults"));
    assert!(!has_event(&records, "BundleFailed"), "a missing bundle is not a failure: {records:?}");
    assert_eq!(paths(&mut harness, "site.example:80"), [settings_path().as_str(), "/news/index.t42"]);
}

/// REQ-WEB-027 (DEC-1726): a channel's entry file opens under whatever type a
/// host gives an extension it does not know, including none, and only an
/// HTML page is refused by its type.
/// REQ-1903: the entry file under the types a host gives it.
#[test]
fn a_channels_entry_file_opens_whatever_type_a_host_gives_it() {
    let settings = br#"{"name":"Test"}"#;
    for media in [
        Some("application/json"),
        Some("application/octet-stream"),
        Some("text/plain; charset=utf-8"),
        Some("binary/octet-stream"),
        None,
    ] {
        let mut harness = site_harness(Config::default());
        let header = media.map(|media| format!("Content-Type: {media}"));
        let headers: Vec<&str> = header.iter().map(String::as_str).collect();
        harness.replay().reply("site.example:80", Reply::Bytes(response("200 OK", &headers, settings)));
        harness.replay().reply("site.example:80", not_found());
        let records = harness.open_channel("http://site.example/news/");
        assert!(has_event(&records, "ServiceReady { name: \"Test\""), "{media:?}: {records:?}");
    }
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", ok("text/html; charset=utf-8", settings));
    let records = harness.open_channel("http://site.example/news/");
    assert!(has_event(&records, "DescriptorFailed(MediaType(Html))"), "{records:?}");
    // A generic type gives no pass to what is not the channel's settings.
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", ok("application/octet-stream", b"<html></html>"));
    let records = harness.open_channel("http://site.example/news/");
    assert!(has_event(&records, "DescriptorRejected"), "{records:?}");
}

/// DEC-1726: the entry file's old name is not read: a channel is never asked
/// for it, and told to open it no channel opens.
/// REQ-1884: a reader asks for the fixed names alone.
#[test]
fn a_channels_old_entry_file_is_not_read() {
    let mut harness = site_harness(Config::default());
    let records = harness.open_channel("http://site.example/news/channel.json");
    assert!(has_event(&records, "DescriptorFailed(ChannelAddress(Channel(NotChannelFile)))"), "{records:?}");
    assert_eq!(harness.replay().requests.len(), 0);
}

/// REQ-1886: the page files in turn, the extension that last answered first, and not found after them.
/// REQ-1895: no `push` member means no request to `push`.
#[test]
fn a_channel_without_settings_uses_the_defaults_its_bundle_and_page_files() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", ok("application/octet-stream", &packet()));
    let records = harness.open_channel("http://site.example/news/");
    assert!(has_event(&records, "ChannelDefaults"), "{records:?}");
    assert!(has_event(&records, "ServiceReady { name: \"site.example/news\""), "{records:?}");
    let Rec::Page { source, url, media, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!(
        (*source, url.as_str(), *media),
        (Source::FrontDoor, "http://site.example/news/index.t42", MediaType::T42)
    );
    // A page: `NNN.tti` first, then `NNN.t42`.
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", ok("text/plain", &packet()));
    let records = harness.fetch(0x140);
    let Rec::Page { url, media, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!((url.as_str(), *media), ("http://site.example/news/140.t42", MediaType::T42));
    assert_eq!(paths(&mut harness, "site.example:80")[2..], ["/news/140.tti", "/news/140.t42"]);
    // The extension that answered is tried first next time.
    harness.replay().reply("site.example:80", ok("application/x-teletext-t42", &packet()));
    let records = harness.fetch(0x150);
    assert_eq!(pages(&records).len(), 1, "{records:?}");
    assert_eq!(paths(&mut harness, "site.example:80")[4..], ["/news/150.t42"]);
    // Neither file: not found.
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", Reply::Bytes(response("410 Gone", &[], b"")));
    let records = harness.fetch(0x160);
    assert!(has_event(&records, "PageNotInService"), "{records:?}");
}

/// REQ-1894: an invalid member refuses the file.
#[test]
fn a_broken_settings_file_is_not_replaced_by_the_defaults() {
    // Only a missing file means "no settings": a server error, an invalid
    // file, a template or an HTML page must not change how the channel is
    // read.
    let cases: [(Reply, &str); 3] = [
        (Reply::Bytes(response("500 Oops", &["Content-Type: text/plain"], b"")), "DescriptorFailed(Status(500))"),
        (json_response(br#"{"page_template":"/{mpp}.t42"}"#), "DescriptorRejected(NotForChannels(PageTemplate))"),
        (ok("text/html", br#"{"name":"Test"}"#), "DescriptorFailed(MediaType(Html))"),
    ];
    for (reply, expected) in cases {
        let mut harness = site_harness(Config::default());
        harness.replay().reply("site.example:80", reply);
        let records = harness.open_channel("http://site.example/");
        assert!(has_event(&records, expected), "{expected}: {records:?}");
        assert!(!has_event(&records, "ChannelDefaults"));
        assert!(!has_event(&records, "ServiceReady"));
    }
}

#[test]
fn settings_may_come_as_plain_text_and_link_channels() {
    let mut harness = site_harness(Config::default());
    let settings = br#"{"services":[{"name":"w","display_name":"W","channel":"http://other.example/w/"}]}"#;
    harness.replay().reply("site.example:80", ok("text/plain; charset=utf-8", settings));
    let records = harness.open_channel("http://site.example/");
    assert!(has_event(&records, "ServiceReady"), "{records:?}");
    let descriptor = harness.agent.descriptor().unwrap();
    let linked = descriptor.service("w").unwrap();
    assert!(linked.channel);
    assert_eq!(descriptor.service_address(linked), "http://other.example/w/");
}

/// REQ-1903: a channel's page files are accepted by the media types of REQ-WEB-027.
#[test]
fn page_files_must_be_what_their_names_say() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/");
    // A Template Toolkit file named 100.tti: refused, and 100.t42 is not
    // then tried.
    harness.replay().reply("site.example:80", ok("text/plain", b"[% INCLUDE header %]\n"));
    let records = harness.fetch(0x100);
    assert!(
        has_event(
            &records,
            "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(0) }, subcode: None, failure: Content }"
        ),
        "{records:?}"
    );
    assert_eq!(paths(&mut harness, "site.example:80").last().unwrap(), "/100.tti");
    // An HTML page answered with 200 for a missing file.
    harness.replay().reply("site.example:80", ok("text/html", TTI));
    let records = harness.fetch(0x110);
    assert!(
        has_event(
            &records,
            "FetchFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(16) }, subcode: None, failure: MediaType(Html) }"
        ),
        "{records:?}"
    );
    // A bundle that is not T42 is reported, and the channel works without
    // it.
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", ok("application/octet-stream", b"not packets"));
    let records = harness.open_channel("http://site.example/");
    assert!(has_event(&records, "BundleFailed(Content)"), "{records:?}");
    assert!(has_event(&records, "ServiceReady"), "{records:?}");
}

// Verifies REQ-WEB-021.
/// REQ-492: a redirect leaving the channel is reported, not followed.
#[test]
fn redirects_that_leave_the_channel_are_reported_not_followed() {
    let config = Config { redirects_within_channel: true, ..Config::default() };
    for location in [
        format!("http://other.example/news/{SETTINGS_FILE}"),
        format!("http://site.example/sport/{SETTINGS_FILE}"),
        format!("https://site.example:8443/news/{SETTINGS_FILE}"),
    ] {
        let mut harness = site_harness(config);
        let header = format!("Location: {location}");
        harness.replay().reply("site.example:80", Reply::Bytes(response("302 Found", &[header.as_str()], b"")));
        let records = harness.open_channel("http://site.example/news/");
        let expected = format!("RedirectToOtherChannel {{ location: \"{location}\" }}");
        assert!(has_event(&records, &expected), "{location}: {records:?}");
        assert!(harness.replay().requests_to("other.example:80").is_empty(), "nothing is fetched elsewhere");
        assert_eq!(harness.replay().requests.len(), 1, "{location}");
    }
}

// Verifies REQ-WEB-021.
#[test]
fn redirects_inside_the_channel_and_upgrades_are_followed() {
    let config = Config { redirects_within_channel: true, ..Config::default() };
    let mut harness = site_harness(config);
    harness.replay().reply(
        "site.example:80",
        Reply::Bytes(response(
            "301 Moved",
            &[format!("Location: https://site.example/news/{SETTINGS_FILE}").as_str()],
            b"",
        )),
    );
    harness.replay().reply("site.example:443", json_response(br#"{"name":"Secure"}"#));
    let records = harness.open_channel("http://site.example/news/");
    assert!(!has_event(&records, "RedirectToOtherChannel"), "{records:?}");
    assert!(has_event(&records, "ServiceReady { name: \"Secure\""), "{records:?}");
}

#[test]
fn neottx_still_follows_redirects_to_other_hosts() {
    let mut harness = site_harness(following());
    harness
        .replay()
        .reply("site.example:80", Reply::Bytes(response("302 Found", &["Location: http://other.example/d.json"], b"")));
    harness.replay().reply("other.example:80", json_response(&descriptor("")));
    let records = harness.open("http://site.example/d.json");
    assert!(has_event(&records, "ServiceReady"), "{records:?}");
}

#[test]
fn a_service_redirected_to_another_port_leaves_its_channel() {
    let config = Config { redirects_within_channel: true, ..Config::default() };
    let mut harness = site_harness(config);
    harness.replay().reply(
        "site.example:80",
        Reply::Bytes(response("301 Moved", &["Location: https://site.example:8443/d.json"], b"")),
    );
    let records = harness.open("http://site.example/d.json");
    assert!(has_event(&records, "RedirectToOtherChannel"), "{records:?}");
}

#[test]
fn contents_are_fetched_on_request() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    harness.replay().reply("site.example:80", ok("text/plain", b"p\t100\tIndex\n"));
    harness.input(Input::FetchContents);
    let records = harness.take();
    assert!(has_event(&records, "Contents([112, 9, 49, 48, 48"), "{records:?}");
    assert_eq!(paths(&mut harness, "site.example:80").last().unwrap(), "/news/contents.txt");
    harness.replay().reply("site.example:80", not_found());
    harness.input(Input::FetchContents);
    assert!(has_event(&harness.take(), "ContentsMissing"));
    harness.replay().reply("site.example:80", ok("text/html", b"p\t100\tIndex\n"));
    harness.input(Input::FetchContents);
    assert!(has_event(&harness.take(), "ContentsFailed(MediaType(Html))"));
}

/// Whether a file is a part of a page's list.
#[cfg(feature = "std")]
const fn is_list(file: ChannelFile) -> bool {
    matches!(file, ChannelFile::SubPages { .. })
}

/// The parts of the list of a page of the standard's 8,190 sub-pages
/// (REQ-NUM-020), each sub-page's file being `body`, as the writer lays them
/// out (REQ-1888).
#[cfg(feature = "std")]
fn full_list(body: &[u8]) -> Vec<(String, Vec<u8>)> {
    use neoteletext_web::subpages::{SubPageFile, page_files};
    let codes = (1..neoteletext_core::limits::SUBCODE_ANY)
        .filter(|&value| neoteletext_core::limits::is_sub_code(value))
        .map(SubCode::new);
    let sub_pages: Vec<SubPageFile<'_>> =
        codes.map(|subcode| SubPageFile { subcode, t42: body, title: "A story" }).collect();
    let page = PageAddress::from_mpp(0x100).unwrap();
    let files = page_files(page, &sub_pages).unwrap();
    files.into_iter().filter(|(name, _)| ChannelFile::from_name(name).is_some_and(is_list)).collect()
}

/// A part of `list`, as a host answers it.
#[cfg(feature = "std")]
fn list_reply(list: &[(String, Vec<u8>)], part: usize, validator: &str) -> Reply {
    let etag = format!("ETag: \"{validator}\"");
    Reply::Bytes(response("200 OK", &["Content-Type: text/plain", &etag], &list[part].1))
}

/// REQ-1888 (decisions E.122, DEC-2116): a part of a page's list is asked for
/// when wanted, conditionally once its validators are held, and then one
/// sub-page passes the wire when it is asked for, from its own file and
/// nothing else: keying a sub-code asks for one file. A page whose part 0 is
/// missing keeps its page file.
#[cfg(feature = "std")]
#[test]
fn a_page_of_8190_sub_pages_is_fetched_one_sub_page_at_a_time() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let interval = neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US;
    let parts = full_list(&packet());
    assert_eq!(parts.len(), neoteletext_web::subpages::MAX_PARTS);
    harness.replay().reply("site.example:80", list_reply(&parts, 0, "c1"));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(interval);
    let records = harness.take();
    let delivered = events(&records).into_iter().find(|event| event.starts_with("SubPages {")).unwrap();
    let count = neoteletext_web::subpages::read(&parts[0].1).unwrap().total;
    assert_eq!(count, 8190, "whichever part arrives first sizes the list");
    assert!(delivered.len() > parts[0].1.len(), "the whole part is delivered");
    // One sub-page, from its own file.
    harness.replay().reply("site.example:80", ok("application/octet-stream", &packet()));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(0x2345)) });
    harness.advance(interval);
    let records = harness.take();
    let Rec::Page { source, url, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!(*source, Source::Page { page, subcode: Some(SubCode::new(0x2345)) });
    assert_eq!(url, "http://site.example/news/100/2345.t42");
    // The part again: conditional, and unchanged.
    harness.replay().reply("site.example:80", Reply::Bytes(response("304 Not Modified", &["ETag: \"c1\""], b"")));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(interval);
    assert!(has_event(&harness.take(), "SubPagesCurrent"));
    let last = harness.replay().requests_to("site.example:80").last().unwrap().clone();
    assert_eq!(header_value(&last, "If-None-Match").as_deref(), Some("\"c1\""));
    // A sub-page that has gone is not looked for but in its two files.
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(0x0124)) });
    harness.advance(interval);
    assert!(has_event(&harness.take(), "PageNotInService"));
    // "Any sub-page" is the page's own file.
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::ANY) });
    harness.advance(interval);
    assert_eq!(pages(&harness.take()).len(), 1);
    // A page with no part 0: its sub-pages are in its page file.
    let other = PageAddress::from_mpp(0x200).unwrap();
    harness.replay().reply("site.example:80", not_found());
    harness.input(Input::FetchSubPages { page: other, part: PartNumber::FIRST });
    harness.advance(interval);
    assert!(has_event(&harness.take(), "SubPagesMissing"));
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.input(Input::FetchPage { page: other, subcode: Some(SubCode::new(1)) });
    harness.advance(interval);
    assert_eq!(pages(&harness.take()).len(), 1);
    assert_eq!(
        paths(&mut harness, "site.example:80"),
        [
            settings_path().as_str(),
            "/news/index.t42",
            "/news/100/0.sub",
            "/news/100/2345.t42",
            "/news/100/0.sub",
            "/news/100/0124.t42",
            "/news/100/0124.tti",
            "/news/100.tti",
            "/news/200/0.sub",
            "/news/200.tti",
        ]
    );
}

/// REQ-1888: with the 128 parts of 8,190 sub-pages, showing rows 4,020 to
/// 4,042 asks for parts 62 and 63 alone, each with its own validators, and a
/// missing later part says nothing of the page's folder.
#[cfg(feature = "std")]
#[test]
fn showing_some_rows_asks_for_the_parts_they_fall_in_alone() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let interval = neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US;
    let list = full_list(&packet());
    let parts: Vec<PartNumber> = neoteletext_web::subpages::parts_of_rows(4020, 23).collect();
    for &part in &parts {
        harness.replay().reply("site.example:80", list_reply(&list, usize::from(part.number()), "c"));
        harness.input(Input::FetchSubPages { page, part });
        harness.advance(interval);
    }
    let records = harness.take();
    let delivered = events(&records).into_iter().filter(|event| event.starts_with("SubPages {")).count();
    assert_eq!(delivered, 2);
    assert_eq!(paths(&mut harness, "site.example:80")[2..], ["/news/100/62.sub", "/news/100/63.sub"]);
    // Part 0 was never asked for, and a part past the page's end leaves it
    // with its folder: a sub-page still comes from its own file.
    harness.replay().reply("site.example:80", not_found());
    harness.input(Input::FetchSubPages { page, part: PartNumber::new(127).unwrap() });
    harness.advance(interval);
    assert!(has_event(&harness.take(), "SubPagesMissing"));
    harness.replay().reply("site.example:80", ok("application/octet-stream", &packet()));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(1)) });
    harness.advance(interval);
    assert_eq!(paths(&mut harness, "site.example:80").last().unwrap(), "/news/100/0001.t42");
}

/// REQ-1887, DEC-2116: a page's sub-pages may be TTI files in its folder. The
/// extension that last answered is tried first, and the result does not change.
#[test]
fn a_sub_page_may_be_a_tti_file_in_its_pages_folder() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let interval = neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US;
    harness.replay().reply("site.example:80", ok("text/plain", b"2\tv\n0001\tv\n0002\tv\n"));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(interval);
    harness.take();
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(1)) });
    harness.advance(interval);
    let records = harness.take();
    let Rec::Page { url, media, .. } = pages(&records)[0] else { unreachable!() };
    assert_eq!(url, "http://site.example/news/100/0001.tti");
    assert_eq!(*media, MediaType::Tti);
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(2)) });
    harness.advance(interval);
    assert_eq!(pages(&harness.take()).len(), 1);
    assert_eq!(
        paths(&mut harness, "site.example:80")[2..],
        ["/news/100/0.sub", "/news/100/0001.t42", "/news/100/0001.tti", "/news/100/0002.tti"]
    );
}

/// REQ-1904: a sub-page's T42 file at its bound is read, and one packet past
/// it is abandoned whole.
#[test]
fn a_sub_pages_file_has_its_bound() {
    use neoteletext_web::subpages::MAX_SUBPAGE_FILE;
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let interval = neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US;
    harness.replay().reply("site.example:80", ok("text/plain", b"2\tv\n0001\tv\n0002\tv\n"));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(interval);
    harness.take();
    let packet = packet();
    let full = packet.repeat(MAX_SUBPAGE_FILE / packet.len());
    assert_eq!(full.len(), MAX_SUBPAGE_FILE);
    harness.replay().reply("site.example:80", ok("application/octet-stream", &full));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(1)) });
    harness.advance(interval);
    assert_eq!(pages(&harness.take()).len(), 1);
    let over = packet.repeat(MAX_SUBPAGE_FILE / packet.len() + 1);
    harness.replay().reply("site.example:80", ok("application/octet-stream", &over));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(2)) });
    harness.advance(interval);
    let records = harness.take();
    assert!(pages(&records).is_empty(), "nothing of it is delivered");
    assert!(has_event(&records, "FetchFailed"));
}

/// REQ-1896: the file is the truth. A sub-page whose file answers with a
/// validator other than its list's is used, and a listed sub-page whose files
/// answer 404 is not found.
#[test]
fn a_sub_pages_file_is_the_truth_when_it_disagrees_with_its_list() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let interval = neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US;
    harness.replay().reply("site.example:80", ok("text/plain", b"2\tlisted\n0001\tlisted\n0002\tlisted\n"));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(interval);
    harness.take();
    let reply = response("200 OK", &["Content-Type: application/octet-stream", "ETag: \"other\""], &packet());
    harness.replay().reply("site.example:80", Reply::Bytes(reply));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(1)) });
    harness.advance(interval);
    assert_eq!(pages(&harness.take()).len(), 1, "the file answers, and is used");
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(2)) });
    harness.advance(interval);
    assert!(has_event(&harness.take(), "PageNotInService"), "listed, and its files say it does not exist");
}

/// REQ-WEB-027: a part of a list is text; anything else is refused, and the
/// page's sub-pages stay in its page file.
/// REQ-1903: a part of a list is text.
#[test]
fn a_part_of_a_list_must_be_text() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    harness.replay().reply("site.example:80", ok("text/html", b"1\tv\n0001\tv\n"));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US);
    assert!(has_event(
        &harness.take(),
        "SubPagesFailed { page: PageAddress { magazine: Magazine(1), page: PageNumber(0) }, part: PartNumber(0), failure: MediaType(Html) }"
    ));
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.input(Input::FetchPage { page, subcode: Some(SubCode::new(1)) });
    harness.advance(neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US);
    assert_eq!(paths(&mut harness, "site.example:80").last().unwrap(), "/100.tti");
}

/// REQ-1904: a part larger than its bound is abandoned whole, and the agent
/// never holds more of it than that bound.
#[test]
fn a_part_of_a_list_past_its_bound_is_refused() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/");
    let page = PageAddress::from_mpp(0x100).unwrap();
    let over = std::vec![b'x'; neoteletext_web::subpages::MAX_BYTES + 1];
    harness.replay().reply("site.example:80", ok("text/plain", &over));
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.advance(neoteletext_web::consts::DEFAULT_FETCH_INTERVAL_US);
    assert!(has_event(&harness.take(), "SubPagesFailed"));
}

/// The target of the push connection of the channel at `address`.
#[cfg(feature = "push")]
fn push_target(address: &str) -> String {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel(address);
    let last = harness.replay().requests_to("site.example:80").last().unwrap().clone();
    request_path(&last)
}

/// REQ-1693 item 4: a fragment the map does not list is refused, never
/// ignored, so nothing is fetched for the address.
#[test]
fn an_address_with_a_refused_fragment_opens_nothing() {
    for address in ["http://site.example/a/#302", "http://site.example/a/302.tti#303", "http://site.example/a/#wall"] {
        let mut harness = site_harness(Config::default());
        assert!(has_event(&harness.open_channel(address), "DescriptorFailed(ChannelAddress(Place))"), "{address}");
        assert!(harness.replay().requests_to("site.example:80").is_empty(), "{address}");
    }
}

/// DEC-1730: a query goes with the selection, in SELECT, and never on the
/// push connection, whose address is the same for every page of a channel.
/// REQ-1895: the live connection is the folder's `push`.
#[test]
#[cfg(feature = "push")]
fn the_push_connection_carries_no_query_and_no_page() {
    for address in [
        "http://site.example/a/",
        "http://site.example/a/?id=7",
        "http://site.example/a/302",
        "http://site.example/a/302?page=1",
        "http://site.example/a/302/0002",
    ] {
        assert_eq!(push_target(address), "/a/push", "{address}");
    }
}

#[cfg(feature = "push")]
fn input_service(input: bool) -> Harness {
    let mut replay = Replay::new(&[("tt.example", PUBLIC)]);
    let extra = format!(r#","push":{{"address":"ws://tt.example/push","input":{input}}}"#);
    replay.reply("tt.example:80", json_response(&descriptor(&extra)));
    replay.reply("tt.example:80", Reply::WebSocket(Vec::new()));
    Harness::new(Config::default(), replay)
}

/// The push connection's frames sent after the handshake.
#[cfg(feature = "push")]
fn push_frames(harness: &mut Harness) -> Vec<(u8, Vec<u8>)> {
    harness
        .replay()
        .sent
        .iter()
        .filter(|(c, _)| *c == ConnectionId::Push)
        .skip(1)
        .flat_map(|(_, f)| client_frames(f))
        .collect()
}

/// The push connection's sends after the handshake, each as its frames.
#[cfg(feature = "push")]
fn push_sends(harness: &mut Harness) -> Vec<Vec<(u8, Vec<u8>)>> {
    harness
        .replay()
        .sent
        .iter()
        .filter(|(c, _)| *c == ConnectionId::Push)
        .skip(1)
        .map(|(_, f)| client_frames(f))
        .collect()
}

/// Decision E.90, and page-rules.md RULE-49 (REQ-6528): a message of the
/// input profile goes as one binary message on the push WebSocket of the
/// descriptor's own origin, whatever the withdrawn `push.input` says, and
/// nowhere else (finding F-90).
#[test]
#[cfg(feature = "push")]
fn input_goes_on_the_push_websocket_of_its_own_origin_only() {
    let record = [1u8, 1, 0, 1, 0, 0, 0, 9, 0, 0x61, 0, 0, 0, 0];
    let mut harness = input_service(true);
    assert!(has_event(&harness.open("http://tt.example/service.json"), "PushConnected"));
    harness.input(Input::Send(InputMessage::alone(&record)));
    assert_eq!(push_frames(&mut harness).last(), Some(&(0x2, record.to_vec())), "binary (REQ-RCH-017)");
    assert!(!has_event(&harness.take(), "InputDropped"));
    // Too long for the input profile.
    harness.input(Input::Send(InputMessage::alone(&[0; neoteletext_web::consts::INPUT_MESSAGE_MAX + 1])));
    assert!(has_event(&harness.take(), "InputDropped"));
    // The withdrawn `push.input` means nothing: a page's rule table is the
    // only thing that asks for input, and the session's messages go.
    let mut harness = input_service(false);
    harness.open("http://tt.example/service.json");
    harness.input(Input::Send(InputMessage::alone(&record)));
    assert_eq!(push_frames(&mut harness).last(), Some(&(0x2, record.to_vec())));
    assert!(!has_event(&harness.take(), "InputDropped"));
    // A push on another origin is sent nothing.
    let mut replay = Replay::new(&[("tt.example", PUBLIC), ("keys.example", PUBLIC)]);
    let extra = r#","push":{"address":"ws://keys.example/push","input":true}"#;
    replay.reply("tt.example:80", json_response(&descriptor(extra)));
    replay.reply("keys.example:80", Reply::WebSocket(Vec::new()));
    let mut harness = Harness::new(Config::default(), replay);
    assert!(has_event(&harness.open("http://tt.example/service.json"), "PushConnected"));
    let before = push_frames(&mut harness).len();
    harness.input(Input::Send(InputMessage::alone(&record)));
    assert!(has_event(&harness.take(), "InputDropped"));
    assert_eq!(push_frames(&mut harness).len(), before);
}

/// REQ-WEB-005's heartbeat: a silent WebSocket is pinged after 25 seconds,
/// and lost when no pong comes within 10 more.
#[test]
#[cfg(feature = "push")]
fn a_silent_websocket_is_pinged_then_given_up() {
    let mut harness = input_service(false);
    harness.open("http://tt.example/service.json");
    harness.advance(24 * SECOND);
    assert!(push_frames(&mut harness).iter().all(|(opcode, _)| *opcode != 0x9), "no ping yet");
    harness.advance(SECOND);
    assert_eq!(push_frames(&mut harness).last(), Some(&(0x9, Vec::new())), "pinged");
    harness.advance(9 * SECOND);
    assert!(!has_event(&harness.take(), "PushLost"));
    harness.advance(SECOND);
    assert!(lost_for(&harness.take(), "Some(Failure(Timeout))"));
}

/// REQ-WEB-035, REQ-RCH-010, decision E.133: messages given together leave in one send,
/// one write and one TLS record, in order, each its own WebSocket message;
/// a lone one is written before `handle` returns, with no tick.
#[test]
#[cfg(feature = "push")]
fn messages_waiting_together_leave_in_one_write() {
    let mut harness = input_service(true);
    harness.open("http://tt.example/service.json");
    let before = push_sends(&mut harness).len();
    let (left, entered) = ([0xFF, 0x43, 3, 1, 2, 20], [0xFF, 0x43, 3, 0, 11, 20]);
    harness.handle(Input::Send(InputMessage { bytes: &left, move_of: None, more: true }));
    assert_eq!(push_sends(&mut harness).len(), before, "nothing yet: more follows at once");
    harness.handle(Input::Send(InputMessage { bytes: &entered, move_of: None, more: false }));
    let sends = push_sends(&mut harness);
    assert_eq!(sends.len(), before + 1, "one send");
    assert_eq!(sends.last().unwrap(), &[(0x2, left.to_vec()), (0x2, entered.to_vec())], "two messages, in order");
    harness.handle(Input::Send(InputMessage::alone(b"a")));
    assert_eq!(push_sends(&mut harness).last().unwrap(), &[(0x2, b"a".to_vec())], "a lone event at once");
}

/// REQ-WEB-035, REQ-RCH-035, decision E.133: against a stalled socket a move waits in
/// its contact's slot, the latest replacing it; any other event writes it
/// first, then itself; once the socket drains, the latest held is written,
/// one move and no backlog.
#[test]
#[cfg(feature = "push")]
fn moves_wait_while_the_socket_is_stalled_and_only_the_latest_goes() {
    use neoteletext_web::consts::PUSH_UNSENT_BOUND;
    let mouse_move = |x: u8| [0xFF, 0x02, 0x06, 0x00, 0x01, 0x05, x, 0x64];
    let touch_move = |x: u8| [0xFF, 0x44, 0x06, 0x06, 0x02, 0x01, 0x05, x, 0x64];
    let send_move = |harness: &mut Harness, bytes: &[u8], contact: u8| {
        harness.input(Input::Send(InputMessage { bytes, move_of: Some(contact), more: false }));
    };
    let mut harness = input_service(true);
    harness.open("http://tt.example/service.json");
    send_move(&mut harness, &mouse_move(1), 0);
    assert_eq!(push_frames(&mut harness).last(), Some(&(0x2, mouse_move(1).to_vec())), "a free socket: at once");
    // The socket stalls: the port reports its unsent bytes over the bound.
    harness.replay().unsent = PUSH_UNSENT_BOUND;
    harness.input(Input::Tick);
    let before = push_sends(&mut harness).len();
    for x in 2..=9 {
        send_move(&mut harness, &mouse_move(x), 0);
        send_move(&mut harness, &touch_move(x), 2);
    }
    assert_eq!(push_sends(&mut harness).len(), before, "nothing written behind the network");
    // A key: the moves held first, then the key, together.
    let key = [0xFF, 0x01, 0x10, b'a', 1, b'a'];
    harness.input(Input::Send(InputMessage::alone(&key)));
    assert_eq!(
        push_sends(&mut harness).last().unwrap(),
        &[(0x2, mouse_move(9).to_vec()), (0x2, touch_move(9).to_vec()), (0x2, key.to_vec())],
        "a release never waits behind a move, nor comes before it"
    );
    for x in 10..=20 {
        send_move(&mut harness, &mouse_move(x), 0);
    }
    let before = push_sends(&mut harness).len();
    harness.replay().unsent = 0;
    harness.input(Input::Tick);
    let sends = push_sends(&mut harness);
    assert_eq!(sends.len(), before + 1);
    assert_eq!(sends.last().unwrap(), &[(0x2, mouse_move(20).to_vec())], "after the stall, one move, the latest");
    // A move longer than a slot, or of no contact, is dropped.
    harness.replay().unsent = PUSH_UNSENT_BOUND;
    harness.input(Input::Tick);
    harness.take();
    send_move(&mut harness, &[0xFF; 40], 0);
    send_move(&mut harness, &mouse_move(1), 10);
    let dropped = harness.take().iter().filter(|record| matches!(record, Rec::Event(e) if e == "InputDropped")).count();
    assert_eq!(dropped, 2);
}

/// REQ-WEB-035, REQ-RCH-016, decision E.133: while keys go to the channel the round trip
/// is measured by a ping of one byte, at most once a second, and reported
/// when its pong comes; the heartbeat's empty pong is not taken for it.
#[test]
#[cfg(feature = "push")]
fn the_round_trip_is_measured_by_ping_and_pong() {
    let mut harness = input_service(true);
    harness.open("http://tt.example/service.json");
    harness.input(Input::MeasureRoundTrip(true));
    harness.input(Input::Tick);
    let ping = push_frames(&mut harness).last().cloned().unwrap();
    assert_eq!(ping.0, 0x9, "a ping");
    assert_eq!(ping.1.len(), 1, "of one byte");
    harness.replay().inject(ConnectionId::Push, &server_frame(0xA, b""));
    harness.advance(3_000);
    assert!(!has_event(&harness.take(), "RoundTrip"), "an empty pong is the heartbeat's");
    harness.replay().inject(ConnectionId::Push, &server_frame(0xA, &ping.1));
    harness.advance(2_000);
    assert!(has_event(&harness.take(), "RoundTrip { microseconds: 5000 }"));
    let pings = |harness: &mut Harness| push_frames(harness).iter().filter(|(opcode, _)| *opcode == 0x9).count();
    let sent = pings(&mut harness);
    harness.advance(SECOND / 2);
    assert_eq!(pings(&mut harness), sent, "not within the second");
    harness.advance(SECOND / 2);
    assert_eq!(pings(&mut harness), sent + 1, "a second after the last");
    harness.input(Input::MeasureRoundTrip(false));
    harness.advance(5 * SECOND);
    assert_eq!(pings(&mut harness), sent + 1, "off: no more");
}

/// REQ-WEB-035, REQ-RCH-015, REQ-RCH-017, decision E.133: the push WebSocket is a
/// connection of its own, an HTTP/1.1 upgrade asking for no extension, so
/// permessage-deflate is never offered (and refused when a server answers
/// with it: `ws::tests`).
#[test]
#[cfg(feature = "push")]
fn the_push_socket_is_a_connection_of_its_own() {
    let mut harness = input_service(true);
    harness.open("http://tt.example/service.json");
    let connects: Vec<_> = harness.replay().connects.iter().map(|(connection, ..)| *connection).collect();
    assert_eq!(connects, [ConnectionId::Fetch, ConnectionId::Push], "the descriptor's fetch, then push alone");
    let upgrade = harness.replay().requests_to("tt.example:80")[1].clone();
    assert!(upgrade.starts_with(b"GET /push HTTP/1.1\r\n"));
    assert_eq!(header_value(&upgrade, "Sec-WebSocket-Extensions"), None, "no compression asked for");
    harness.fetch(0x100);
    let sent = &harness.replay().sent;
    let upgrades: Vec<_> = sent.iter().filter(|(_, bytes)| bytes.starts_with(b"GET /push")).collect();
    assert_eq!(upgrades.len(), 1);
    assert_eq!(upgrades[0].0, ConnectionId::Push);
    assert!(
        sent.iter()
            .filter(|(connection, _)| *connection == ConnectionId::Push)
            .skip(1)
            .all(|(_, bytes)| bytes[0] & 0x80 != 0),
        "after the upgrade, only WebSocket frames on the push socket: never a fetch"
    );
}

// Verifies REQ-503: a response outside the profile is never partly
// interpreted: a 101 on the fetch connection, and one whose framing is
// refused, leave no cookie, as they set none.
#[test]
fn a_rejected_response_sets_no_cookie() {
    for head in [
        "HTTP/1.1 101 Switching Protocols\r\nSet-Cookie: id=1; Path=/\r\n\r\n".to_owned(),
        "HTTP/1.1 200 OK\r\nContent-Type: text/x-teletext-tti\r\nContent-Length: 1000000000\r\nSet-Cookie: id=1; Path=/\r\n\r\n"
            .to_owned(),
    ] {
        let mut harness = plain_service();
        harness.replay().reply("tt.example:80", Reply::Bytes(head.clone().into_bytes()));
        let _ = harness.fetch(0x100);
        // The next request carries no cookie.
        harness.advance(SECOND);
        harness.replay().reply("tt.example:80", tti_response(&[]));
        let _ = harness.fetch(0x101);
        let last = harness.replay().requests_to("tt.example:80").pop().unwrap();
        assert_eq!(header_value(&last, "Cookie"), None, "{head}");
    }
}

/// A packet with the address of `magazine`/`number`, the rest of it spaces.
#[cfg(feature = "push")]
fn packet_at(magazine: u8, number: u8) -> [u8; 42] {
    let mut packet =
        neoteletext_core::packet::encode::address(neoteletext_core::types::Magazine::from_bits(magazine % 8), number);
    packet.bytes[2..].fill(0x20);
    packet.bytes
}

// Verifies REQ-1462 item 3: an undecodable address, packet 8/31 and packet 31
// of another data channel are each discarded and counted, nothing of them
// applied, and the connection is kept.
#[test]
#[cfg(feature = "push")]
fn each_invalid_kind_is_discarded_and_counted() {
    let mut harness = push_service("ws://tt.example/push");
    let valid = packet_at(1, 1);
    let mut undecodable = valid;
    undecodable[0] ^= 0b11;
    let message = [undecodable, valid, packet_at(8, 31), packet_at(1, 31)].concat();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "InvalidPackets { total: 3 }"), "{records:?}");
    let delivered = pages(&records);
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0], Rec::Page { bytes, .. } if *bytes == valid[..]), "only the valid packet");
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

// Verifies REQ-1462 item 3: a ragged message is discarded whole, counts as its
// length in packets rounded up, and a page then arrives on the same connection.
#[test]
#[cfg(feature = "push")]
fn a_ragged_message_leaves_the_connection_open() {
    let mut harness = push_service("ws://tt.example/push");
    let valid = packet_at(1, 1);
    let mut frames = server_frame(0x2, &[0x15_u8; 50]);
    frames.extend(server_frame(0x2, &valid));
    harness.replay().reply("tt.example:80", Reply::WebSocket(frames));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "InvalidPackets { total: 2 }"), "{records:?}");
    assert_eq!(pages(&records).len(), 1, "{records:?}");
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

/// The packets `write` gives, joined as one message's bytes.
#[cfg(feature = "push")]
fn packets_of(write: impl FnOnce(&mut dyn FnMut(neoteletext_core::packet::Packet))) -> Vec<u8> {
    let mut bytes = Vec::new();
    write(&mut |packet| bytes.extend_from_slice(&packet.bytes));
    bytes
}

/// A LINKS record's body for sub-page 100/0000: part `part` of `parts`,
/// with one red key link to page 150.
#[cfg(feature = "push")]
fn links_body(part: u8, parts: u8, key: u8) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&0x100_u16.to_be_bytes());
    body.extend_from_slice(&0_u16.to_be_bytes());
    body.extend_from_slice(&[part, parts, 1, key, 0, 3]);
    body.extend_from_slice(b"150");
    body.extend_from_slice(&[0, 0, 0]);
    body
}

// Verifies DOC-1958's bounds across the parts (REQ-1494) and REQ-1462: a
// LINKS sending whose second part repeats the first part's key is refused
// in the agent; the port is told to drop the sub-page's links, and both
// records' packets are counted invalid.
#[test]
#[cfg(feature = "push")]
fn a_links_sending_that_breaks_its_totals_is_refused() {
    use neoteletext_web::records::{TYPE_LINKS, fragments};
    let mut harness = push_service("ws://tt.example/push");
    let (first, second) = (links_body(0, 2, b'R'), links_body(1, 2, b'R'));
    let message = packets_of(|out| {
        write_packets(TYPE_LINKS, &first, 3, 1, out).unwrap();
        write_packets(TYPE_LINKS, &second, 3, 2, out).unwrap();
    });
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert_eq!(events(&records).iter().filter(|event| event.starts_with("Links(")).count(), 1, "{records:?}");
    assert!(has_event(&records, "LinksRefused { page: PageAddress"), "{records:?}");
    let total = fragments(first.len()) + fragments(second.len());
    assert!(has_event(&records, &format!("InvalidPackets {{ total: {total} }}")), "{records:?}");
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

/// A SUBPAGES record's body for page 100, version 1.
#[cfg(feature = "push")]
fn subpages_body(part: u16, parts: u16, count: u16, codes: &[u16]) -> Vec<u8> {
    let mut body = Vec::new();
    for field in [0x100_u16, 1, part, parts, count] {
        body.extend_from_slice(&field.to_be_bytes());
    }
    for code in codes {
        body.extend_from_slice(&code.to_be_bytes());
        body.push(0);
    }
    body
}

// Verifies REQ-1503 and DOC-1959 through the agent: a whole sending's parts
// reach the port and then its completion; a sending whose sub-codes repeat
// across parts is refused, the port told, and its packets counted.
#[test]
#[cfg(feature = "push")]
fn the_agent_reassembles_a_list_of_sub_pages() {
    use neoteletext_web::records::{TYPE_SUBPAGES, fragments};
    let mut harness = push_service("ws://tt.example/push");
    let whole = [subpages_body(0, 2, 4, &[1, 2]), subpages_body(1, 2, 4, &[3, 4])];
    let broken = [subpages_body(0, 2, 4, &[1, 2]), subpages_body(1, 2, 4, &[2, 3])];
    let message = packets_of(|out| {
        for (id, body) in whole.iter().enumerate() {
            write_packets(TYPE_SUBPAGES, body, 3, id as u8, out).unwrap();
        }
    });
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    let names: Vec<&str> = events(&records)
        .into_iter()
        .filter(|event| event.starts_with("SubPageList"))
        .map(|event| event.split([' ', '(']).next().unwrap())
        .collect();
    assert_eq!(names, ["SubPageList", "SubPageList", "SubPageListComplete"], "{records:?}");
    assert!(!has_event(&records, "InvalidPackets"), "{records:?}");

    let mut harness = push_service("ws://tt.example/push");
    let message = packets_of(|out| {
        for (id, body) in broken.iter().enumerate() {
            write_packets(TYPE_SUBPAGES, body, 3, id as u8, out).unwrap();
        }
    });
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "SubPageListRefused { page: PageAddress"), "{records:?}");
    assert!(!has_event(&records, "SubPageListComplete"), "{records:?}");
    let total = fragments(broken[0].len()) + fragments(broken[1].len());
    assert!(has_event(&records, &format!("InvalidPackets {{ total: {total} }}")), "{records:?}");
}

// Verifies REQ-INP-026 and DOC-1956 through the agent: at most 16 LOG lines
// reach the port before the first page frame, and 16 for each page frame.
#[test]
#[cfg(feature = "push")]
fn log_lines_are_limited_for_each_page_frame() {
    use neoteletext_formats::log::{LogLevel, MAX_LOG_LINES_PER_PAGE};
    use neoteletext_web::records::log;
    let mut harness = push_service("ws://tt.example/push");
    let lines = |first_id: u8| {
        packets_of(|out| {
            for index in 0..MAX_LOG_LINES_PER_PAGE + 4 {
                let id = first_id.wrapping_add(index as u8);
                log::encode(LogLevel::Information, None, "line", 3, id, out).unwrap();
            }
        })
    };
    let header = packet_at(1, 0);
    let message = [lines(0).as_slice(), &header, &lines(100)].concat();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    let logged = events(&records).iter().filter(|event| event.starts_with("Log {")).count();
    assert_eq!(logged, 2 * MAX_LOG_LINES_PER_PAGE, "{records:?}");
}

// Verifies VS-204 through the agent: viewer state's records reach the port
// as events, in the order sent, and the agent answers none of them itself:
// the key, and so the answer, is the port's (VS-101).
#[test]
#[cfg(feature = "push")]
fn viewer_state_records_reach_the_port() {
    use neoteletext_web::records::state::{StateChallenge, StateEnd, encode_challenge, encode_end, encode_keep_offer};
    let mut harness = push_service("ws://tt.example/push");
    let challenge = StateChallenge { challenge: [7; 32], create: true };
    let end = StateEnd { key_id: [9; 32] };
    let message = packets_of(|out| {
        encode_challenge(challenge, 0, 1, out).unwrap();
        encode_end(end, 0, 2, out).unwrap();
        encode_keep_offer(0, 3, out).unwrap();
    });
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    let state: Vec<&str> = events(&records).into_iter().filter(|event| event.starts_with("State")).collect();
    assert_eq!(
        state,
        [
            format!("{:?}", neoteletext_web::agent::Event::StateChallenge(challenge)).as_str(),
            format!("{:?}", neoteletext_web::agent::Event::StateEnd(end)).as_str(),
            "StateKeepOffer",
        ]
    );
    assert!(!has_event(&records, "InvalidPackets"), "{records:?}");
    assert!(push_frames(&mut harness).iter().all(|(opcode, _)| *opcode != 0x2), "the agent sent no answer");
}

// Verifies REQ-1462 item 3: packet 1/30 among good packets is discarded and
// counted alone, the good ones delivered in order, the connection kept.
#[test]
#[cfg(feature = "push")]
fn packet_30_of_magazine_1_is_discarded_and_counted() {
    let mut harness = push_service("ws://tt.example/push");
    let (first, second) = (packet_at(1, 1), packet_at(1, 2));
    let message = [first, packet_at(1, 30), second].concat();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    assert_eq!(events(&records).iter().filter(|event| event.starts_with("InvalidPackets")).count(), 1);
    assert!(has_event(&records, "InvalidPackets { total: 1 }"), "{records:?}");
    let delivered = pages(&records);
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0], Rec::Page { bytes, .. } if *bytes == [first, second].concat()));
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

// Verifies REQ-1462 item 3, DOC-1952: a channel record that breaks its
// layout, arriving among page packets, is discarded and each of its
// fragments counted; the page packets are delivered and the connection
// kept.
#[test]
#[cfg(feature = "push")]
fn a_malformed_record_among_page_packets_counts_its_fragments() {
    use neoteletext_web::records::{TYPE_SUBPAGES, fragments};
    let mut harness = push_service("ws://tt.example/push");
    // A SUBPAGES body whose count is 0: the frame is whole, the layout not.
    let mut body = Vec::new();
    for field in [0x100_u16, 1, 0, 1, 0] {
        body.extend_from_slice(&field.to_be_bytes());
    }
    body.extend_from_slice(&[0, 1, 0]);
    let record = packets_of(|out| {
        write_packets(TYPE_SUBPAGES, &body, 3, 1, out).unwrap();
    });
    let page = packet_at(1, 1);
    let message = [page.as_slice(), &record, &page].concat();
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &message)));
    let records = harness.open("http://tt.example/service.json");
    let expected = format!("InvalidPackets {{ total: {} }}", fragments(body.len()));
    assert!(has_event(&records, &expected), "{records:?}");
    assert!(!has_event(&records, "SubPageList"), "{records:?}");
    assert!(!has_event(&records, "PushLost"), "{records:?}");
}

// Verifies REQ-1462 item 4's count, since the push connection opened: after
// the connection is lost and opened again, the total starts again from the
// new connection's packets.
#[test]
#[cfg(feature = "push")]
fn the_invalid_count_restarts_when_the_connection_reopens() {
    let mut harness = push_service("ws://tt.example/push");
    let mut first = server_frame(0x2, &[packet_at(8, 31), packet_at(8, 31)].concat());
    first.extend(server_frame(0x8, &1000u16.to_be_bytes()));
    harness.replay().reply("tt.example:80", Reply::WebSocket(first));
    let records = harness.open("http://tt.example/service.json");
    assert!(has_event(&records, "InvalidPackets { total: 2 }"), "{records:?}");
    assert!(has_event(&records, "PushLost"), "{records:?}");
    harness.replay().reply("tt.example:80", Reply::WebSocket(server_frame(0x2, &packet_at(8, 31))));
    harness.advance(SECOND);
    let records = harness.take();
    assert!(has_event(&records, "PushConnected"), "{records:?}");
    assert!(has_event(&records, "InvalidPackets { total: 1 }"), "{records:?}");
}

/// The live channel `site_harness` serves at `/live/`: its settings name a
/// push connection, and the connection sends `frames`.
#[cfg(feature = "push")]
fn live_channel(frames: Vec<u8>) -> (Harness, Vec<Rec>) {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", json_response(br#"{"push":{}}"#));
    harness.replay().reply("site.example:80", Reply::WebSocket(frames));
    let records = harness.open_channel("http://site.example/live/");
    (harness, records)
}

// Verifies REQ-WEB-018 (REQ-507), TODO-1528 items 1 and 2: a live channel
// makes no request after its entry file but its push connection, even when
// the port asks for its initial page, a part of a page's list of sub-pages
// and its contents; it reports connecting at once and never a page not
// found, and is live when the first page frame arrives.
#[test]
#[cfg(feature = "push")]
fn a_live_channel_opens_by_its_connection_alone() {
    let (mut harness, records) = live_channel(Vec::new());
    assert!(has_event(&records, "ChannelConnecting"), "{records:?}");
    assert!(has_event(&records, "PushConnected"), "{records:?}");
    let page = PageAddress::from_mpp(0x100).unwrap();
    harness.input(Input::FetchPage { page, subcode: None });
    harness.input(Input::FetchSubPages { page, part: PartNumber::FIRST });
    harness.input(Input::FetchContents);
    harness.advance(10 * SECOND);
    let later = harness.take();
    let paths = paths(&mut harness, "site.example:80");
    assert_eq!(paths, ["/live/channel.ttx", "/live/push"], "the entry file, then the push connection's handshake");
    assert!(!has_event(&later, "PageNotInService") && !has_event(&records, "PageNotInService"), "{later:?}");
    assert!(!has_event(&later, "ChannelLive"), "no page yet: {later:?}");
    // The first page frame makes it live.
    harness.replay().inject(ConnectionId::Push, &server_frame(0x2, &packet_at(1, 0)));
    harness.settle();
    let live = harness.take();
    assert!(has_event(&live, "ChannelLive"), "{live:?}");
}

// Verifies REQ-WEB-018, DOC-1955, TODO-1528 item 3: index records on a live
// channel's connection reach the port as `Index`, and `IndexComplete`
// follows the last part only.
#[test]
#[cfg(feature = "push")]
fn a_live_channels_index_reaches_the_port() {
    use neoteletext_web::records::index::{IndexMagazine, IndexPage, encode};
    let pages = [IndexPage { page: 0x100, subpages: 1, title: "Index", section: "" }];
    let other = [IndexPage { page: 0x200, subpages: 2, title: "Sport", section: "Sport" }];
    let magazines = [
        IndexMagazine { magazine: 1, name: "One", pages: &pages },
        IndexMagazine { magazine: 2, name: "", pages: &other },
    ];
    let message = packets_of(|out| {
        encode(4, &magazines, 3, 1, out).unwrap();
    });
    let (_, records) = live_channel(server_frame(0x2, &message));
    let order: Vec<&str> = events(&records)
        .into_iter()
        .filter(|event| event.starts_with("Index"))
        .map(|event| event.split([' ', '(']).next().unwrap())
        .collect();
    assert_eq!(order, ["Index", "Index", "IndexComplete"], "{records:?}");
    assert!(has_event(&records, "IndexComplete { generation: 4 }"), "{records:?}");
}

// Verifies REQ-WEB-018, TODO-1528 items 4 and 5: a static channel still asks
// for its entry file, then its bundle, and then the page asked for; and a
// channel opened after a live one is fetched again.
#[test]
#[cfg(feature = "push")]
fn a_static_channel_after_a_live_one_makes_its_requests() {
    let (mut harness, _) = live_channel(Vec::new());
    harness.replay().reply("site.example:80", json_response(br#"{"name":"Static"}"#));
    harness.replay().reply("site.example:80", not_found());
    let records = harness.open_channel("http://site.example/news/");
    assert!(!has_event(&records, "ChannelConnecting"), "{records:?}");
    harness.replay().reply("site.example:80", tti_response(&[]));
    harness.fetch(0x100);
    let paths = paths(&mut harness, "site.example:80");
    assert_eq!(&paths[2..], ["/news/channel.ttx", "/news/index.t42", "/news/100.tti"]);
}

// Verifies REQ-508 (REQ-WEB-019) and RFC 9110 section 8.8: when the file a
// page answered from has gone and the other is tried, the request for the
// other file carries none of the first file's validators.
#[test]
fn the_second_file_of_a_page_carries_no_validators_of_the_first() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", not_found());
    harness.open_channel("http://site.example/news/");
    harness
        .replay()
        .reply("site.example:80", tti_response(&["ETag: \"tti-1\"", "Last-Modified: Mon, 02 Nov 2026 10:00:00 GMT"]));
    harness.fetch(0x100);
    harness.replay().reply("site.example:80", not_found());
    harness.replay().reply("site.example:80", ok("application/x-teletext-t42", &packet()));
    harness.advance(60 * SECOND);
    harness.fetch(0x100);
    let requests = harness.replay().requests_to("site.example:80");
    let [.., tti_again, t42] = requests.as_slice() else { panic!("{} requests", requests.len()) };
    assert_eq!(request_path(tti_again), "/news/100.tti");
    assert_eq!(header_value(tti_again, "If-None-Match").as_deref(), Some("\"tti-1\""), "the file's own validator");
    assert_eq!(request_path(t42), "/news/100.t42");
    assert_eq!(header_value(t42, "If-None-Match"), None);
    assert_eq!(header_value(t42, "If-Modified-Since"), None);
}

// Verifies REQ-512 (REQ-WEB-023): a channel's address that breaks a rule is
// refused with the rule it breaks, and nothing is fetched.
#[test]
fn a_channel_address_that_breaks_a_rule_names_it() {
    for (address, rule) in [
        ("http://site.example/a//", "EmptySegment"),
        ("http://site.example/a/%41/", "Character"),
        ("ws://site.example/a/", "Scheme"),
    ] {
        let mut harness = site_harness(Config::default());
        let records = harness.open_channel(address);
        assert!(
            has_event(&records, &format!("DescriptorFailed(ChannelAddress(Channel({rule})))")),
            "{address}: {records:?}"
        );
        assert!(harness.replay().requests_to("site.example:80").is_empty(), "{address}");
    }
}

// Verifies REQ-433 (REQ-SEC-020): every security setting of the web
// agent's configuration defaults to the safer choice: no local-network
// address allowed, redirects kept inside the channel, the connection direct rather than through a proxy, and TLS
// offered, so that HTTPS is used where an address asks for it.
#[test]
fn the_default_configuration_is_the_safer_choice() {
    let config = Config::default();
    assert!(config.allowed_local.iter().all(Option::is_none), "no local address is allowed");
    assert!(config.redirects_within_channel, "redirects stay inside the channel");
    assert!(!config.proxied, "no proxy");
    assert!(config.tls_available, "TLS is offered");
}

/// The events of a resource fetched, without the exchange's report.
fn outcome(records: &[Rec]) -> Vec<&str> {
    events(records).into_iter().filter(|event| !event.starts_with("Exchanged")).collect()
}

/// A page with pointers for discovery: one in a `Link` field, one in a
/// `<link>` element, one hidden in a comment, and one after the prefix.
fn discovery_page(padding: usize) -> Vec<u8> {
    let mut body = String::from(
        "<!DOCTYPE html><html><head><!-- <link rel=alternate type=text/x-teletext-tti href=/hidden/> -->\
         <link rel=\"alternate\" type=\"application/vnd.neoteletext+json\" href=\"/teletext/\">\
         <link rel=alternate type=text/x-teletext-tti href=\"/a&amp;b/\">",
    );
    body.push_str(&" ".repeat(padding));
    body.push_str("<link rel=alternate type=text/x-teletext-tti href=/far/></head><body></body></html>");
    body.into_bytes()
}

fn discovery_answer(padding: usize) -> Reply {
    Reply::Bytes(response(
        "200 OK",
        &[
            "Content-Type: text/html; charset=utf-8",
            "Set-Cookie: planted=1; Path=/",
            "Link: <https://secure.example/tv/>; rel=\"alternate\"; type=\"application/vnd.neoteletext+json\"",
        ],
        &discovery_page(padding),
    ))
}

// REQ-2476: discovery reads the pointers of the headers and then the elements in the
// first 8,192 bytes, refuses a target with `&`, and closes its connection.
#[test]
fn discovery_reads_pointers() {
    let mut harness = secure_service();
    harness.replay().reply("secure.example:443", tti_response(&[]));
    harness.fetch(0x100);
    harness.replay().reply("secure.example:443", discovery_answer(64 * 1024));
    let connects = harness.replay().connects.len();
    harness.input(Input::FetchResource { role: ResourceRole::Discovery, address: "https://secure.example/news/" });
    let records = harness.take();
    assert_eq!(
        outcome(&records),
        [
            "Pointer { target: \"https://secure.example/tv/\", media: Channel, found: Header }",
            "Pointer { target: \"https://secure.example/teletext/\", media: Channel, found: Element { after_base: false } }",
            "DiscoveryDone { answered: \"https://secure.example/news/\", refused: 1 }",
        ]
    );
    let request = harness.replay().requests_to("secure.example:443").pop().unwrap();
    assert_eq!(header_value(&request, "Cookie"), None, "a cookie went with discovery");
    assert_eq!(header_value(&request, "Accept").as_deref(), Some("text/html"));
    assert_eq!(harness.replay().connects.len(), connects + 1, "discovery opened a connection of its own");
    // The next page request opens another, and carries no cookie: the
    // `Set-Cookie` discovery's answer came with was never kept (VS-201).
    harness.advance(SECOND);
    harness.replay().reply("secure.example:443", tti_response(&[]));
    harness.fetch(0x101);
    assert_eq!(harness.replay().connects.len(), connects + 2, "discovery's connection was used again");
    let last = harness.replay().requests_to("secure.example:443").pop().unwrap();
    assert_eq!(header_value(&last, "Cookie"), None);
    assert_profile_headers(harness.replay(), &[]);
}

// REQ-2476: a body of any length is read only to its prefix, without being
// refused for its size.
#[test]
fn discovery_reads_a_long_page_only_to_its_prefix() {
    let mut harness = secure_service();
    harness.replay().reply("secure.example:443", discovery_answer(4 * 1024 * 1024));
    harness.input(Input::FetchResource { role: ResourceRole::Discovery, address: "https://secure.example/" });
    let found = outcome(&harness.take()).into_iter().filter(|event| event.starts_with("Pointer")).count();
    assert_eq!(found, 2, "the pointer past the prefix was read");
}

// REQ-2476: a redirect to the local network or from https to http ends
// discovery; any status but 200 and any type but HTML find nothing.
#[test]
fn discovery_refuses_what_it_may_not_read() {
    let mut replay = Replay::new(&[("secure.example", PUBLIC), ("evil.example", PRIVATE)]);
    replay.reply("secure.example:443", Reply::Bytes(response("302 Found", &["Location: http://secure.example/"], b"")));
    replay.reply("secure.example:443", Reply::Bytes(response("302 Found", &["Location: https://evil.example/"], b"")));
    replay.reply("secure.example:443", Reply::Bytes(response("404 Not Found", &["Content-Type: text/html"], b"")));
    replay.reply("secure.example:443", tti_response(&[]));
    let mut harness = Harness::new(Config::default(), replay);
    let expected = [
        "ResourceFailed { role: Discovery, failure: Downgrade }",
        "ResourceFailed { role: Discovery, failure: Blocked }",
        "ResourceFailed { role: Discovery, failure: Status(404) }",
        "ResourceFailed { role: Discovery, failure: MediaType(Tti) }",
    ];
    for expected in expected {
        harness.input(Input::FetchResource { role: ResourceRole::Discovery, address: "https://secure.example/" });
        let records = harness.take();
        assert!(outcome(&records).contains(&expected), "{expected}: {records:?}");
    }
}

/// A font file's bytes, as a host would serve them; their shape is the
/// drawing process's to check.
const FONT: &[u8] = b"STARTFONT 2.1\nENDFONT\n";

// REQ-123: a font named by its address is fetched without a cookie and
// handed to the port with its final address, only as a font's media type
// and within the core's bound.
#[test]
fn a_font_is_fetched_by_its_address() {
    let mut harness = secure_service();
    harness
        .replay()
        .reply("secure.example:443", Reply::Bytes(response("200 OK", &["Content-Type: application/x-font-bdf"], FONT)));
    harness.input(Input::FetchResource { role: ResourceRole::Font, address: "https://secure.example/fonts/a.bdf" });
    let records = harness.take();
    let expected = format!("Font {{ address: \"https://secure.example/fonts/a.bdf\", bytes: {FONT:?} }}");
    assert_eq!(outcome(&records), [expected.as_str()]);
    let request = harness.replay().requests_to("secure.example:443").pop().unwrap();
    assert_eq!(header_value(&request, "Cookie"), None);
    assert_eq!(header_value(&request, "Accept").as_deref(), Some("application/x-font-bdf, application/octet-stream"));

    harness.replay().reply("secure.example:443", Reply::Bytes(response("200 OK", &["Content-Type: text/html"], FONT)));
    harness.input(Input::FetchResource { role: ResourceRole::Font, address: "https://secure.example/fonts/a.bdf" });
    assert_eq!(outcome(&harness.take()), ["ResourceFailed { role: Font, failure: MediaType(Html) }"]);

    let large = std::vec![b'x'; neoteletext_core::consts::FONT_MAX_FILE_BYTES + 1];
    harness.replay().reply(
        "secure.example:443",
        Reply::Bytes(response("200 OK", &["Content-Type: application/octet-stream"], &large)),
    );
    harness.input(Input::FetchResource { role: ResourceRole::Font, address: "https://secure.example/fonts/a.bdf" });
    assert_eq!(outcome(&harness.take()), ["ResourceFailed { role: Font, failure: Http(BodyTooLarge) }"]);

    harness.input(Input::FetchResource { role: ResourceRole::Font, address: "https://192.168.1.10/a.bdf" });
    assert!(outcome(&harness.take()).contains(&"ResourceFailed { role: Font, failure: Blocked }"));
}

// REQ-123: the Accept header a font is asked for with names the media
// types a font is taken as, which are defined once in the media constants.
#[test]
fn a_fonts_accept_header_names_the_font_media_types() {
    use neoteletext_web::consts::{ACCEPT_FONT, media};
    let accept = std::str::from_utf8(ACCEPT_FONT).unwrap();
    let names: Vec<&str> = accept.split(", ").collect();
    assert_eq!(
        names,
        [std::str::from_utf8(media::FONT_BDF).unwrap(), std::str::from_utf8(media::OCTET_STREAM).unwrap()]
    );
}

/// REQ-507, REQ-WEB-021: once a service's descriptor is asked for, a
/// redirect out of the folder that holds it leaves the service, and is
/// reported, not followed; one inside the folder is followed.
#[test]
fn a_redirect_out_of_a_descriptors_folder_leaves_the_service() {
    let mut harness = site_harness(Config::default());
    harness.replay().reply(
        "site.example:80",
        Reply::Bytes(response("302 Found", &["Location: http://site.example/two/service.json"], b"")),
    );
    let records = harness.open("http://site.example/one/service.json");
    assert!(has_event(&records, "RedirectToOtherChannel"), "{records:?}");
    assert!(!has_event(&records, "ServiceReady"), "{records:?}");
    let mut inside = site_harness(Config::default());
    inside.replay().reply(
        "site.example:80",
        Reply::Bytes(response("302 Found", &["Location: http://site.example/one/v2/service.json"], b"")),
    );
    inside.replay().reply(
        "site.example:80",
        json_response(br#"{"name":"Moved","initial_page":"100","page_template":"/one/{mpp}.tti"}"#),
    );
    let records = inside.open("http://site.example/one/service.json");
    assert!(has_event(&records, "ServiceReady { name: \"Moved\""), "{records:?}");
}
