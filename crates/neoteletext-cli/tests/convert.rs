//! `neottx convert --page` (REQ-CLI-003, REQ-INP-014, REQ-INP-018): a
//! selection writes that page, and a sub-code that one sub-page.
#![allow(clippy::unwrap_used, missing_docs, reason = "a test")]

mod common;
use common::{folder, neottx};
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
