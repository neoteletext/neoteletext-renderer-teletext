//! Web mode against the local test server (REQ-WEB-013): the whole path
//! from descriptor to decoded page, through the fetcher process. Only a
//! build with live feeds has web mode (decision E.61).
#![cfg(feature = "live")]
#![allow(missing_docs)]

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

#[test]
fn faults_do_not_crash_the_receiver() {
    let server = server();
    for fault in ["drop", "big", "media", "loop", "ambiguous"] {
        // A descriptor address that misbehaves: neottx must fail cleanly,
        // and as an input that could not be read (REQ-CLI-005).
        let url = format!("http://127.0.0.1:{}/fault/{fault}", server.port);
        let output = Command::new(env!("CARGO_BIN_EXE_neottx"))
            .args(["text", &url, "100", "--allow-local", "127.0.0.1", "--duration", "5"])
            .output()
            .expect("run neottx");
        assert_eq!(output.status.code(), Some(3), "fault {fault}: {}", String::from_utf8_lossy(&output.stderr));
    }
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
