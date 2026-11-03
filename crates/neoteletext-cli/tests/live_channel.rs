//! `neottx` against a live channel of this test, one whose `channel.ttx`
//! names a push connection and which serves nothing else (REQ-507, DEC-2445):
//! the one-shot commands select their page on the connection, `neottx list`
//! gives the channel's index from its index records, a sub-page's links come
//! from its LINKS record, and `neottx view` steps by the SUBPAGES record.
#![cfg(feature = "live")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `expect` naming what it expected and indexing into data it built to a known size"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_formats::encode::encode_page;
use neoteletext_formats::item::{Item as ReadItem, Reader};
use neoteletext_formats::page::PageFile;
use neoteletext_formats::tti::TtiReader;
use neoteletext_input::control::{Control, packet_of};
use neoteletext_input::event::{Event, Item, read_message};
use neoteletext_web::channel::SETTINGS_FILE;
use neoteletext_web::consts::PUSH_SUBPROTOCOL;
use neoteletext_web::records::index::{self, IndexMagazine, IndexPage};
use neoteletext_web::records::links;
use neoteletext_web::records::subpages::{self, SubPageEntry};
use neoteletext_web::ws::accept_for;

/// RFC 6455 section 5.2.
const FIN: u8 = 0x80;
const BINARY: u8 = 0x2;
const OPCODE: u8 = 0x0F;
const MASK_BYTES: usize = 4;
const MASKED_LENGTH: u8 = 0x7F;
const LENGTH_16: u8 = 126;
/// The audio service the channel's records go on (DOC-1952), and the
/// selection of no particular sub-page (clause 9.6.1).
const RECORD_SERVICE: u8 = 3;
const ANY: u16 = neoteletext_core::limits::SUBCODE_ANY;
/// The sub-pages of page 100.
const SUB_PAGES: u16 = 3;

/// Sub-page `subcode` of page 100, its row 1 saying which, as TTI with its
/// Link Extension line.
fn tti(subcode: u16) -> String {
    format!("PN,100{subcode:02}\r\nSC,{subcode:04}\r\nPS,8000\r\nOL,1,SUB {subcode:04}\r\nLN,R,200,Weather\r\n")
}

/// The page file of a sub-page.
fn page_file(subcode: u16) -> PageFile {
    let mut page = None;
    let mut reader = TtiReader::new();
    let mut each = |item: ReadItem<'_>| {
        if let ReadItem::Page(file) = item {
            page = Some(file.clone());
        }
    };
    reader.feed(tti(subcode).as_bytes(), &mut each);
    reader.finish(&mut each);
    page.expect("a page")
}

/// What the channel sends for a selection of sub-page `subcode`: the
/// sub-page, its LINKS record, and the page's SUBPAGES records; at the first
/// selection the index before them (DOC-1950).
fn answer(subcode: u16, first: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut out = |packet: neoteletext_core::packet::Packet| bytes.extend_from_slice(&packet.bytes);
    if first {
        let pages = [
            IndexPage { page: 0x100, subpages: SUB_PAGES, title: "News", section: "" },
            IndexPage { page: 0x150, subpages: 1, title: "Weather", section: "" },
        ];
        index::encode(1, &[IndexMagazine { magazine: 1, name: "", pages: &pages }], RECORD_SERVICE, 1, &mut out)
            .unwrap();
    }
    let page = page_file(subcode);
    encode_page(&page, &mut out);
    links::encode_page(&page, RECORD_SERVICE, 2, &mut out).unwrap();
    let entries: Vec<SubPageEntry<'_>> =
        (1..=SUB_PAGES).map(|code| SubPageEntry { subcode: SubCode::new(code), title: "" }).collect();
    subpages::encode(PageAddress::from_mpp(0x100).unwrap(), 1, &entries, RECORD_SERVICE, 3, &mut out).unwrap();
    bytes
}

fn frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
    let mut bytes = vec![FIN | opcode];
    if payload.len() < usize::from(LENGTH_16) {
        bytes.push(u8::try_from(payload.len()).unwrap());
    } else {
        bytes.push(LENGTH_16);
        bytes.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_be_bytes());
    }
    bytes.extend_from_slice(payload);
    let _ = stream.write_all(&bytes);
}

/// One client frame's opcode and payload.
fn read_frame(reader: &mut BufReader<TcpStream>) -> Option<(u8, Vec<u8>)> {
    let mut head = [0_u8; 2];
    reader.read_exact(&mut head).ok()?;
    let length = match head[1] & MASKED_LENGTH {
        LENGTH_16 => {
            let mut bytes = [0_u8; 2];
            reader.read_exact(&mut bytes).ok()?;
            usize::from(u16::from_be_bytes(bytes))
        }
        short => usize::from(short),
    };
    let mut mask = [0_u8; MASK_BYTES];
    reader.read_exact(&mut mask).ok()?;
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).ok()?;
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % MASK_BYTES];
    }
    Some((head[0] & OPCODE, payload))
}

/// The connection: WELCOME for HELLO, and each selection answered; the
/// selections are noted as `page/sub`.
fn serve_socket(stream: &mut TcpStream, reader: &mut BufReader<TcpStream>, selected: &Mutex<Vec<String>>) {
    let mut first = true;
    while let Some((opcode, payload)) = read_frame(reader) {
        if opcode != BINARY {
            continue;
        }
        let mut answers = Vec::new();
        let _ = read_message(&payload, &mut |item| match item {
            Item::Event(Event::Hello { version, caps, .. }) => {
                let control = Control::Welcome { version, caps, resume: None };
                answers.push(packet_of(&control, 0, 0).unwrap().bytes.to_vec());
            }
            Item::Event(Event::Select(select, _)) => {
                selected.lock().unwrap().push(format!("{:03X}/{:04X}", select.page, select.sub));
                // No page, or page 100 with any sub-page: its first.
                let subcode = if select.sub == ANY { 1 } else { select.sub };
                if matches!(select.page, 0 | 0x100) && (1..=SUB_PAGES).contains(&subcode) {
                    answers.push(answer(subcode, first));
                    first = false;
                }
            }
            _ => {}
        });
        for bytes in answers {
            frame(stream, BINARY, &bytes);
        }
    }
}

/// The channel: its entry file names a push connection, and every other
/// path but the connection answers 404, as a live server's do.
fn serve(listener: &TcpListener, selected: &Arc<Mutex<Vec<String>>>) {
    for stream in listener.incoming().flatten() {
        let selected = Arc::clone(selected);
        std::thread::spawn(move || {
            let mut stream = stream;
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
            let mut key = None;
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("sec-websocket-key")
                {
                    key = Some(value.trim().to_owned());
                }
            }
            if path.ends_with(SETTINGS_FILE) {
                let body = r#"{"name":"Live","initial_page":"100","push":{"input":true}}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.shutdown(Shutdown::Write);
            } else if let (true, Some(key)) = (path.ends_with("/push"), key) {
                let accept = accept_for(key.as_bytes());
                let _ = write!(
                    stream,
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Protocol: {PUSH_SUBPROTOCOL}\r\nSec-WebSocket-Accept: {}\r\n\r\n",
                    String::from_utf8_lossy(&accept)
                );
                serve_socket(&mut stream, &mut reader, &selected);
            } else {
                let _ = write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
    }
}

/// The channel on a local port, and the selections it was sent.
fn channel() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
    let port = listener.local_addr().expect("its address").port();
    let selected = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&selected);
    std::thread::spawn(move || serve(&listener, &noted));
    (port, selected)
}

fn neottx(port: u16, arguments: &[&str]) -> Output {
    let address = format!("http://127.0.0.1:{port}/ch/");
    Command::new(env!("CARGO_BIN_EXE_neottx"))
        .arg(arguments[0])
        .arg(&address)
        .args(&arguments[1..])
        .args(["--allow-local", "127.0.0.1", "--no-sandbox", "--duration", "10"])
        .output()
        .expect("run neottx")
}

// Verifies REQ-507 through DEC-2445: `neottx text` of a live channel says
// HELLO, selects the page asked for on the push connection and shows it,
// with row 24 drawn from the sub-page's LINKS record (REQ-1494).
#[test]
fn a_one_shot_command_selects_its_page_on_a_live_channel() {
    let (port, selected) = channel();
    let output = neottx(port, &["text", "100"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("SUB 0001"), "{text}");
    assert_eq!(*selected.lock().unwrap(), ["100/3F7F"]);
    let row24 = text.lines().nth(24).unwrap_or_default();
    assert!(row24.contains("Weather"), "row 24 from the LINKS record: {row24:?}");
}

// Verifies REQ-507 and DOC-1955 through DEC-2445: `neottx list` of a live
// channel selects no page, which the channel answers with its initial page,
// and lists the channel's index from its index records.
#[test]
fn neottx_list_gives_a_live_channels_index() {
    let (port, selected) = channel();
    let output = neottx(port, &["list"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(*selected.lock().unwrap(), ["000/3F7F"]);
    assert!(text.contains("100  sub-pages    3  News"), "{text}");
    assert!(text.contains("150  sub-pages    1  Weather"), "{text}");
}

// Verifies REQ-1503 and REQ-328: `neottx view` steps through a live page's
// sub-pages by its SUBPAGES record, selecting each, and wraps.
#[cfg(feature = "view")]
#[test]
fn the_viewer_steps_through_a_live_pages_list() {
    let (port, selected) = channel();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server/drive_subpages.py");
    let output = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_neottx"))
        .arg(port.to_string())
        .arg(SUB_PAGES.to_string())
        .arg("leave")
        .output()
        .expect("python3 runs the driver");
    let selected = selected.lock().unwrap();
    assert!(output.status.success(), "{}: {selected:?}", String::from_utf8_lossy(&output.stdout));
    for subcode in 2..=SUB_PAGES {
        assert!(selected.contains(&format!("100/{subcode:04X}")), "{selected:?}");
    }
}
