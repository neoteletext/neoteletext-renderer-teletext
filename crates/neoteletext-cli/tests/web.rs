//! Web mode against the local test server (REQ-WEB-013): the whole path
//! from descriptor to decoded page, through the fetcher process. Only a
//! build with live feeds has web mode (decision E.61).
#![cfg(feature = "live")]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The test server, which needs `python3`; a test fails without it.
fn server() -> Server {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server/server.py");
    let mut child = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .arg("0")
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 runs the test server");
    let mut line = String::new();
    BufReader::new(child.stdout.take().expect("the server's output"))
        .read_line(&mut line)
        .expect("the server prints its port");
    let port = line.trim().parse().expect("the server's port");
    Server { child, port }
}

fn neottx(arguments: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_neottx")).args(arguments).output().expect("run neottx");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn page_is_fetched_from_a_web_service() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/service.json", server.port);
    let (ok, text, errors) = neottx(&["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"]);
    assert!(ok, "neottx failed: {errors}");
    assert!(text.contains("NEOTELETEXT TEST SERVICE"), "{text}");
    assert!(text.contains("News ....."), "{text}");
    // The TTI Link Extension row drawn from the LN lines (REQ-NAV-011).
    let row24 = text.lines().nth(24).unwrap_or_default();
    assert!(row24.contains("Weather") && row24.contains("News"), "row 24: {row24:?}");
}

/// REQ-WEB-002: a channel's `region` gives the group of a page that names
/// none, here Greek, whose option the page's header holds; the user's own
/// `--region` stands above it.
#[test]
fn a_channels_region_is_the_group_of_its_pages() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/regional/", server.port);
    let page = ["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"];
    let (ok, text, errors) = neottx(&page);
    assert!(ok, "neottx failed: {errors}");
    assert!(text.contains("\u{03B1}"), "group 6 reads a as alpha: {text}");
    let (ok, text, errors) = neottx(&[&page[..], &["--region", "0"]].concat());
    assert!(ok, "neottx failed: {errors}");
    assert!(!text.contains("\u{03B1}"), "the user's region stands: {text}");
}

// Verifies REQ-492: a page served as `application/octet-stream` or
// `text/plain` is read by the detection of REQ-INP-005: these bytes are T42.
#[test]
fn a_page_of_undeclared_format_is_detected() {
    let server = server();
    for kind in ["octet", "plain"] {
        let url = format!("http://127.0.0.1:{}/undeclared/{kind}/service.json", server.port);
        let (ok, text, errors) = neottx(&["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"]);
        assert!(ok, "{kind}: {errors}");
        assert!(text.contains("NEOTELETEXT TEST SERVICE"), "{kind}: {text}");
    }
}

/// The status line and `ETag` of a GET of `path` from the test server, sent
/// with `If-None-Match: etag` when given.
fn get_head(port: u16, path: &str, etag: Option<&str>) -> (String, Option<String>) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the test server");
    let condition = etag.map(|etag| format!("If-None-Match: {etag}\r\n")).unwrap_or_default();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{condition}Connection: close\r\n\r\n").unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).unwrap();
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let status = answer.lines().next().unwrap_or_default().to_owned();
    let tag = answer.lines().find_map(|line| line.strip_prefix("ETag: ")).map(str::to_owned);
    (status, tag)
}

// Verifies REQ-502 (REQ-WEB-013) and REQ-501 (REQ-WEB-012): the test server
// serves T42 pages with validators over HTTP, and an ETag on every page,
// the regional channel's too; a request naming the validator is answered
// 304; and neottx reads the service whose pages are T42.
#[test]
fn the_test_server_serves_t42_pages_with_validators() {
    let server = server();
    for path in ["/pages/100.t42", "/pages/100.tti", "/regional/100.tti"] {
        let (status, etag) = get_head(server.port, path, None);
        assert!(status.contains(" 200 "), "{path}: {status}");
        let etag = etag.unwrap_or_else(|| panic!("{path} has an ETag"));
        let (again, _) = get_head(server.port, path, Some(&etag));
        assert!(again.contains(" 304 "), "{path}: {again}");
    }
    let url = format!("http://127.0.0.1:{}/t42.json", server.port);
    let (ok, text, errors) = neottx(&["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"]);
    assert!(ok, "{errors}");
    assert!(text.contains("NEOTELETEXT TEST SERVICE"), "{text}");
}

// Verifies REQ-1462 item 4: the count is gathered for 20 seconds, so a command
// shorter than that writes no line at the first rise.
#[test]
fn an_invalid_packet_is_not_reported_at_once() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/invalid.json", server.port);
    let (ok, _, errors) = neottx(&["text", &url, "200", "--allow-local", "127.0.0.1", "--duration", "6"]);
    assert!(ok, "neottx failed: {errors}");
    assert!(!errors.contains("invalid packets"), "{errors}");
}

#[test]
fn local_addresses_are_blocked_unless_named() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/service.json", server.port);
    let (ok, _, errors) = neottx(&["text", &url, "100", "--duration", "5"]);
    assert!(!ok);
    assert!(errors.contains("Blocked"), "REQ-SEC-006: {errors}");
}

// Verifies REQ-502 (REQ-WEB-013): each fault the test server injects ends
// neottx cleanly, as an input that could not be read (REQ-CLI-005), with the
// diagnostic that names the fault.
#[test]
fn faults_do_not_crash_the_receiver() {
    let server = server();
    let expected = [
        ("drop", "DescriptorFailed(Http(Truncated))"),
        ("big", "DescriptorFailed(Http(BodyTooLarge))"),
        ("media", "DescriptorFailed(MediaType(Html))"),
        ("loop", "DescriptorFailed(BadRedirect)"),
        ("ambiguous", "DescriptorFailed(Http(AmbiguousFraming))"),
    ];
    for (fault, diagnostic) in expected {
        let url = format!("http://127.0.0.1:{}/fault/{fault}", server.port);
        let output = Command::new(env!("CARGO_BIN_EXE_neottx"))
            .args(["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "5"])
            .output()
            .expect("run neottx");
        let errors = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(3), "fault {fault}: {errors}");
        assert!(errors.contains(diagnostic), "fault {fault}: {errors}");
    }
}

// Verifies REQ-502 (REQ-WEB-013): a server slow to answer, here by two
// seconds the test gives it, is waited for within the response limit.
#[test]
fn a_slow_server_is_waited_for() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/fault/slow?seconds=2", server.port);
    let (ok, text, errors) = neottx(&["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"]);
    assert!(ok, "neottx failed: {errors}");
    assert!(text.contains("NEOTELETEXT TEST SERVICE"), "{text}");
}

/// A GET of `path` from the test server with `Accept: accept`: the whole
/// answer.
fn get_accepting(port: u16, path: &str, accept: &str) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the test server");
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: {accept}\r\nConnection: close\r\n\r\n").unwrap();
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).unwrap();
    answer
}

// Verifies VS-201 (the viewer-state design, which withdrew REQ-502's
// cookie): the session cookie the test server sets never comes back on
// neottx's next requests, which carry no cookie at all.
#[test]
fn the_session_cookie_never_comes_back() {
    let server = server();
    let count = || {
        let answer = get_accepting(server.port, "/sessions", "text/plain");
        let text = String::from_utf8_lossy(&answer).into_owned();
        text.rsplit("\r\n\r\n").next().unwrap_or_default().trim().parse::<u32>().expect("a count")
    };
    let before = count();
    let url = format!("http://127.0.0.1:{}/service.json", server.port);
    let (ok, _, errors) = neottx(&["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "15"]);
    assert!(ok, "neottx failed: {errors}");
    assert_eq!(count(), before, "a request brought the session cookie back");
}

// Verifies REQ-501 (REQ-WEB-012) and REQ-2476: the test server shows a
// publisher how to point from the web to a channel: a web page with a `Link`
// header field and a `<link>` element in its head, read by the engine's
// discovery reader as the channel's folder, and a `303 See Other` to the
// channel for a receiver that asks for teletext, both varying by `Accept`.
#[test]
fn the_test_server_points_from_the_web_to_its_channel() {
    use neoteletext_web::discovery::{self, Found, PointerMedia};
    use neoteletext_web::url::Url;
    let server = server();
    let answer = get_accepting(server.port, "/web/", "text/html");
    let split = answer.windows(4).position(|window| window == b"\r\n\r\n").expect("a head") + 4;
    let (head, body) = answer.split_at(split);
    let head_text = String::from_utf8_lossy(head);
    assert!(head_text.starts_with("HTTP/1.1 200 "), "{head_text}");
    assert!(head_text.contains("Vary: Accept"), "{head_text}");
    // The pointers are relative, so they are resolved against a public
    // name standing for the site: a literal local address is refused, as
    // discovery refuses it.
    let page = Url::parse("http://site.example/web/").unwrap();
    let mut found = Vec::new();
    let mut take = |pointer: discovery::Pointer<'_>| {
        let target = discovery::resolve(&pointer, &page).expect("a pointer that checks");
        found.push((target.as_str().to_owned(), pointer.media, pointer.found));
    };
    discovery::read_headers(head, &mut take);
    discovery::read_elements(body, &mut take);
    let channel = "http://site.example/regional/".to_owned();
    assert_eq!(
        found,
        [
            (channel.clone(), PointerMedia::Channel, Found::Header),
            (channel, PointerMedia::Channel, Found::Element { after_base: false }),
        ]
    );
    let answer = get_accepting(server.port, "/web/", "application/vnd.neoteletext+json");
    let text = String::from_utf8_lossy(&answer);
    assert!(text.starts_with("HTTP/1.1 303 "), "{text}");
    assert!(text.contains("Location: /regional/") && text.contains("Vary: Accept"), "{text}");
}

// Verifies REQ-502 (REQ-WEB-013), a behaviour only a real socket shows: a
// server that closes each kept-alive connection after its answer without
// saying so is met with a new connection for the next request, and the page
// arrives.
#[test]
fn a_server_that_closes_its_kept_alive_connection_is_reconnected_to() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/closing/service.json", server.port);
    let (ok, text, errors) = neottx(&["text", &url, "200", "--allow-local", "127.0.0.1", "--duration", "15"]);
    assert!(ok, "neottx failed: {errors}");
    assert!(text.contains("Sunny, 21 degrees."), "{text}");
    let asked = get_accepting(server.port, "/requests", "text/plain");
    let asked = String::from_utf8_lossy(&asked).into_owned();
    let asked: Vec<&str> = asked.rsplit("\r\n\r\n").next().unwrap_or_default().lines().collect();
    assert_eq!(asked, ["/closing/service.json", "/closing/pages/200.tti"]);
}

/// The status line, the head's fields and the body of a GET of `path` from
/// the test server.
fn get(port: u16, path: &str) -> (String, Vec<(String, String)>, Vec<u8>) {
    let answer = get_accepting(port, path, "*/*");
    let split = answer.windows(4).position(|window| window == b"\r\n\r\n").expect("a head") + 4;
    let (head, body) = answer.split_at(split);
    let head = String::from_utf8_lossy(head).into_owned();
    let mut lines = head.lines();
    let status = lines.next().unwrap_or_default().to_owned();
    let fields = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    (status, fields, body.to_vec())
}

fn field<'a>(fields: &'a [(String, String)], name: &str) -> Option<&'a str> {
    fields.iter().find(|(field, _)| field == name).map(|(_, value)| value.as_str())
}

// Verifies REQ-501 (REQ-WEB-012) and REQ-502 (REQ-WEB-013): the test server
// publishes a channel as a folder, as the guidance tells a publisher to:
// each file under its fixed name with its media type and a validator; a
// contents file and a page's list of sub-pages in parts that the engine's
// readers take without a fault, the list's validators those of the files it
// names; 404 for a file the channel does not have; and a moved page's old
// name redirected inside the folder.
#[test]
fn the_test_server_publishes_a_channel_as_a_folder() {
    use neoteletext_web::subpages::{self, Validator};
    let server = server();
    let t42 = "application/x-teletext-t42";
    let files = [
        ("channel.ttx", "application/json"),
        ("index.t42", t42),
        ("contents.txt", "text/plain"),
        ("100.t42", t42),
        ("200.t42", t42),
        ("400.t42", t42),
        ("400/0.sub", "text/plain"),
        ("400/0001.t42", t42),
        ("400/0002.t42", t42),
        ("400/0003.t42", t42),
    ];
    for (name, media) in files {
        let (status, fields, _) = get(server.port, &format!("/channel/{name}"));
        assert!(status.contains(" 200 "), "{name}: {status}");
        assert_eq!(field(&fields, "content-type"), Some(media), "{name}");
        assert!(field(&fields, "etag").is_some(), "{name} has no validator");
    }
    let (_, _, contents) = get(server.port, "/channel/contents.txt");
    let entries: Vec<_> = neoteletext_web::contents::read(&contents).expect("a contents file").collect();
    assert!(entries.iter().all(Result::is_ok), "{entries:?}");
    let pages = entries
        .iter()
        .filter_map(|entry| match entry {
            Ok(neoteletext_web::contents::Entry::Page { page, .. }) => Some(page.mpp()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(pages, [0x100, 0x200, 0x300, 0x400]);
    let (_, _, list) = get(server.port, "/channel/400/0.sub");
    let part = subpages::read(&list).expect("a part of a list");
    let sub_pages: Vec<Vec<u8>> =
        (1..=3).map(|subcode| get(server.port, &format!("/channel/400/{subcode:04X}.t42")).2).collect();
    assert_eq!(part.total, 3);
    assert_eq!(part.version, Validator::of_page(sub_pages.iter().map(Vec::as_slice)));
    let rows: Vec<_> = part.entries.map(|row| row.expect("a good row")).collect();
    let listed: Vec<_> = rows.iter().map(|row| (row.subcode.value(), row.validator)).collect();
    let expected: Vec<_> = (1..=3_u16).zip(sub_pages.iter().map(|file| Validator::of(file))).collect();
    assert_eq!(listed, expected);
    for missing in ["500.t42", "400/0009.t42", "400/1.sub", "nothing"] {
        let (status, _, _) = get(server.port, &format!("/channel/{missing}"));
        assert!(status.contains(" 404 "), "{missing}: {status}");
    }
    let (status, fields, _) = get(server.port, "/channel/300.t42");
    assert!(status.contains(" 302 "), "{status}");
    assert_eq!(field(&fields, "location"), Some("/channel/moved/300.t42"));
}

// Verifies REQ-502 (REQ-WEB-013) and REQ-501: neottx opens the test server's
// folder channel: a page from its bundle, a page whose old name redirects
// inside the folder, a missing page answered 404, and a sub-page found
// through the page's list and fetched from its own file.
#[test]
fn neottx_reads_the_test_servers_folder_channel() {
    let server = server();
    let url = format!("http://127.0.0.1:{}/channel/", server.port);
    let text = |page: &str| neottx(&["text", &url, page, "--allow-local", "127.0.0.1", "--duration", "15"]);
    let (ok, page, errors) = text("100");
    assert!(ok, "{errors}");
    assert!(page.contains("NEOTELETEXT TEST SERVICE"), "{page}");
    let (ok, page, errors) = text("300");
    assert!(ok, "{errors}");
    assert!(page.contains("TEST NEWS"), "the moved page: {page}");
    let (ok, _, errors) = text("500");
    assert!(!ok);
    assert!(errors.contains("page 500 not found"), "{errors}");
    let (_, said, errors) =
        neottx(&["probe", &url, "--allow-local", "127.0.0.1", "--no-sandbox", "--select", "400/0002"]);
    assert!(said.contains("selection   400/0002: a page in"), "{said}{errors}");
    let asked = get_accepting(server.port, "/requests", "text/plain");
    let asked = String::from_utf8_lossy(&asked).into_owned();
    let asked: Vec<&str> = asked.rsplit("\r\n\r\n").next().unwrap_or_default().lines().collect();
    for path in ["/channel/channel.ttx", "/channel/index.t42", "/channel/moved/300.t42", "/channel/400/0.sub"] {
        assert!(asked.contains(&path), "{path} was not asked for: {asked:?}");
    }
    assert!(asked.contains(&"/channel/400/0002.t42"), "the sub-page's own file: {asked:?}");
}

#[cfg(feature = "view")]
#[test]
fn viewer_shows_pages_and_follows_links() {
    let server = server();
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server/drive_viewer.py");
    let output = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_neottx"))
        .arg(server.port.to_string())
        .output()
        .expect("run the viewer driver");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{report}");
}

/// REQ-WEB-009: the driver starts and stops its own server, so that the
/// network goes away under a running viewer.
#[cfg(feature = "view")]
#[test]
fn viewer_works_offline_and_recovers() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server");
    let output = Command::new("python3")
        .arg("-I")
        .arg(directory.join("drive_offline.py"))
        .arg(env!("CARGO_BIN_EXE_neottx"))
        .arg(directory.join("server.py"))
        .output()
        .expect("python3 runs the offline driver");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{report}");
}
