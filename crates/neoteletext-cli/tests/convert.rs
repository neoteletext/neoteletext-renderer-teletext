//! `neottx convert --page` (REQ-CLI-003, REQ-INP-014, REQ-INP-018): a
//! selection writes that page, and a sub-code that one sub-page.
#![allow(clippy::unwrap_used, missing_docs, reason = "a test")]

mod common;
use common::{folder, neottx};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// The status of a page or sub-page not found (REQ-CLI-005).
const NOT_FOUND_STATUS: i32 = 1;

/// Page 100 with sub-pages 0000 and 0001, and page 200.
const PAGES: &str =
    "PN,10000\nSC,0000\nOL,1,SUB ZERO\nPN,10001\nSC,0001\nOL,1,SUB ONE\nPN,20000\nSC,0000\nOL,1,OTHER\n";

/// Converts `PAGES` selecting `page`, and returns the sub-page's rows the
/// output holds, by listing it back as text.
fn converted(name: &str, to: &str, page: &str) -> Vec<u8> {
    let folder = folder(name);
    let input = folder.join("in.tti");
    fs::write(&input, PAGES).unwrap();
    let output = folder.join(format!("out.{to}"));
    let run = neottx(&["convert", input.to_str().unwrap(), "--to", to, "--page", page, "-o", output.to_str().unwrap()]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    fs::read(output).unwrap()
}

fn holds(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|window| window == text.as_bytes())
}

/// The text of `page` of a converted file.
fn text_of(file: &Path, page: &str) -> String {
    let run = neottx(&["text", file.to_str().unwrap(), page]);
    String::from_utf8_lossy(&run.stdout).into_owned()
}

// Verifies REQ-88.
#[test]
fn a_sub_code_selects_one_sub_page_in_tti() {
    let written = converted("convert-sub-page-tti", "tti", "100/0001");
    assert!(holds(&written, "SUB ONE"));
    assert!(!holds(&written, "SUB ZERO"), "the other sub-page");
    assert!(!holds(&written, "OTHER"), "the other page");
}

// Verifies REQ-88.
#[test]
fn a_sub_code_selects_one_sub_page_in_t42() {
    let written = converted("convert-sub-page-t42", "t42", "100/0001");
    let file = Path::new(env!("CARGO_TARGET_TMPDIR")).join("convert-sub-page-t42/out.t42");
    assert_eq!(written.len() % 42, 0);
    let text = text_of(&file, "100");
    assert!(text.contains("SUB ONE"), "{text}");
    let other = neottx(&["text", file.to_str().unwrap(), "100/0000"]);
    assert_eq!(other.status.code(), Some(NOT_FOUND_STATUS), "the other sub-page");
}

// Verifies REQ-88: a page with no sub-code keeps every sub-page.
#[test]
fn a_page_alone_selects_all_its_sub_pages() {
    let written = converted("convert-page", "tti", "100");
    assert!(holds(&written, "SUB ONE") && holds(&written, "SUB ZERO"));
    assert!(!holds(&written, "OTHER"));
}

// Verifies REQ-86: convert writes edit.tf addresses, one page a line, which
// read back as the same pages.
#[test]
fn a_page_converts_to_an_edittf_address_that_reads_back_the_same() {
    let written = converted("convert-edittf", "edittf", "200");
    let text = String::from_utf8(written).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.starts_with("https://edit.tf/#"), "{text}");
    let file = Path::new(env!("CARGO_TARGET_TMPDIR")).join("convert-edittf/out.edittf");
    let original = Path::new(env!("CARGO_TARGET_TMPDIR")).join("convert-edittf/in.tti");
    let back = neottx(&["text", file.to_str().unwrap(), "200", "--format", "edittf"]);
    assert!(back.status.success(), "{back:?}");
    assert_eq!(String::from_utf8_lossy(&back.stdout), text_of(&original, "200"));
}

// Verifies REQ-188: page files converted to T42 go in the transmission
// order of Annex B.7, object pages before the pages that invoke them,
// whatever their order in the file.
#[test]
fn page_files_convert_to_t42_in_transmission_order() {
    let folder = folder("convert-b7-order");
    let input = folder.join("in.tti");
    fs::write(&input, "PN,10000\nOL,1,DISPLAY\nPN,1A000\nPF,3,2\nOL,1,OBJECTS\n").unwrap();
    let output = folder.join("out.t42");
    let run = neottx(&["convert", input.to_str().unwrap(), "--to", "t42", "-o", output.to_str().unwrap()]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let written = fs::read(&output).unwrap();
    // The page number of each header, packet 0, in the order written.
    let decode = |byte: u8| neoteletext_core::ecc::hamming84_decode(byte).0;
    let headers: Vec<u8> = written
        .chunks(42)
        .filter(|packet| (decode(packet[0]) >> 3) | (decode(packet[1]) << 1) == 0)
        .map(|packet| (decode(packet[3]) << 4) | decode(packet[2]))
        .collect();
    assert_eq!(headers, [0xA0, 0x00], "the object page comes first");
}

/// Sub-pages of page 100 in the channel test: more than one part of a page's
/// list holds (`neoteletext_web::subpages::ROWS_PER_PART`, 64).
const CHANNEL_SUB_PAGES: u16 = 70;

/// Page 100 of [`CHANNEL_SUB_PAGES`] sub-pages, 0001 up, each saying its
/// sub-code, and page 200 alone.
fn channel_input() -> String {
    let mut pages = String::new();
    for subcode in 1..=CHANNEL_SUB_PAGES {
        let _ = write!(
            pages,
            "PN,100{:02}
SC,{subcode:04}
OL,1,SUB {subcode:04}
",
            subcode % 100
        );
    }
    pages + "PN,20000\nSC,0000\nOL,1,OTHER\n"
}

// Verifies REQ-1887 and REQ-1888 through DEC-2413: `convert --to channel`
// writes a channel's folder whose page of more sub-pages than one part holds
// has a file for each sub-page and its list in two parts, and the engine's
// web agent reads a sub-page listed in the second part back from it.
#[cfg(feature = "live")]
#[test]
fn a_channel_is_written_as_a_folder_and_read_back() {
    let folder = folder("convert-channel");
    let input = folder.join("in.tti");
    fs::write(&input, channel_input()).unwrap();
    let channel = folder.join("channel");
    let run = neottx(&["convert", input.to_str().unwrap(), "--to", "channel", "-o", channel.to_str().unwrap()]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    for name in ["100.t42", "100/0.sub", "100/1.sub", "100/0001.t42", "100/0070.t42", "200.t42", "index.t42"] {
        assert!(channel.join(name).is_file(), "{name}");
    }
    assert!(text_of(&channel.join("100/0067.t42"), "100").contains("SUB 0067"));
    let (port, asked) = common::serve_folder(channel);
    let probe = neottx(&[
        "probe",
        &format!("http://127.0.0.1:{port}/ch/"),
        "--allow-local",
        "127.0.0.1",
        "--no-sandbox",
        "--select",
        "100/0067",
    ]);
    let said = String::from_utf8_lossy(&probe.stdout);
    let asked = asked.lock().unwrap();
    assert!(said.contains("selection   100/0067: a page in"), "{said}: {asked:?}");
    assert!(asked.iter().any(|target| target == "/ch/100/0067.t42"), "{asked:?}");
}

// Verifies DEC-2413 item 1: a file already in the folder is never written
// over, and the conversion fails saying which.
#[test]
fn a_channel_never_overwrites_a_file() {
    let folder = folder("convert-channel-existing");
    let input = folder.join("in.tti");
    fs::write(&input, channel_input()).unwrap();
    let channel = folder.join("channel");
    fs::create_dir_all(&channel).unwrap();
    fs::write(channel.join("200.t42"), "kept").unwrap();
    let run = neottx(&["convert", input.to_str().unwrap(), "--to", "channel", "-o", channel.to_str().unwrap()]);
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("200.t42"), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(fs::read(channel.join("200.t42")).unwrap(), b"kept");
}

// Verifies REQ-328, REQ-290 and REQ-1888: `neottx view` on a channel written
// by `convert --to channel`, whose page holds more sub-pages than one part
// of its list, steps with Right from the first through every sub-page in
// ascending order, across the boundary between parts, wrapping to the first,
// and Left from the first wraps to the last; the store held none of them
// before it was asked for, and each sub-page's file is fetched once.
#[cfg(all(feature = "live", feature = "view"))]
#[test]
fn the_viewer_steps_through_every_sub_page_a_channel_lists() {
    let folder = folder("convert-channel-view");
    let input = folder.join("in.tti");
    fs::write(&input, channel_input()).unwrap();
    let channel = folder.join("channel");
    let run = neottx(&["convert", input.to_str().unwrap(), "--to", "channel", "-o", channel.to_str().unwrap()]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let (port, asked) = common::serve_folder(channel);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server/drive_subpages.py");
    let output = std::process::Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_neottx"))
        .arg(port.to_string())
        .arg(CHANNEL_SUB_PAGES.to_string())
        .output()
        .expect("python3 runs the driver");
    let asked = asked.lock().unwrap();
    assert!(output.status.success(), "{}: {asked:?}", String::from_utf8_lossy(&output.stdout));
    for subcode in 2..=CHANNEL_SUB_PAGES {
        let file = format!("/ch/100/{subcode:04}.t42");
        assert_eq!(asked.iter().filter(|target| **target == file).count(), 1, "{file}: {asked:?}");
    }
}
