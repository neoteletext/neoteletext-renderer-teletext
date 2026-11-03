#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "a test")]
use super::*;
use neoteletext_core::limits::is_sub_code;
use neoteletext_formats::item::{Item, Reader};
use neoteletext_formats::tti::TtiReader;
use neoteletext_web::subpages::ROWS_PER_PART;
use std::fmt::Write as _;

/// Sub-pages of page 100 beyond what one part of its list holds.
const SUB_PAGES: usize = ROWS_PER_PART + 6;

/// The first `count` sub-codes a page may have, from 0001.
fn subcodes(count: usize) -> Vec<u16> {
    (1u16..).filter(|&value| is_sub_code(value)).take(count).collect()
}

/// Pages read from TTI text.
fn pages_of(tti: &str) -> Vec<PageFile> {
    let mut pages = Vec::new();
    let mut reader = TtiReader::new();
    let mut each = |item: Item<'_>| {
        if let Item::Page(page) = item {
            pages.push(page.clone());
        }
    };
    reader.feed(tti.as_bytes(), &mut each);
    reader.finish(&mut each);
    pages
}

/// Page 100 with [`SUB_PAGES`] sub-pages, page 200 alone, and an object
/// page 1FE of two sub-pages.
fn input() -> String {
    let mut tti = String::new();
    for subcode in subcodes(SUB_PAGES) {
        let _ = write!(
            tti,
            "PN,100{:02X}
SC,{subcode:04X}
OL,1,Sub-page {subcode:04X}
",
            subcode & 0xFF
        );
    }
    tti.push_str("PN,20000\r\nSC,0000\r\nOL,1,Alone\r\n");
    tti.push_str("PN,1FE00\r\nSC,0000\r\nPF,4,0\r\nPN,1FE01\r\nSC,0001\r\nPF,4,0\r\n");
    tti
}

// Verifies REQ-1887 and REQ-1888 through DEC-2413: a page of more sub-pages
// than one part holds is written as its page file, a file for each sub-page
// and two parts of its list; a page of one sub-page as its page file alone;
// a page for further processing whole; and the bundle.
#[test]
fn a_channel_is_laid_out_as_its_files() {
    let files = channel_files(&pages_of(&input())).unwrap();
    let names: std::collections::BTreeSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    let mut expected: std::collections::BTreeSet<String> =
        ["100.t42", "100/0.sub", "100/1.sub", "200.t42", "1FE.t42", "index.t42"].map(String::from).into();
    for subcode in subcodes(SUB_PAGES) {
        expected.insert(format!("100/{subcode:04X}.t42"));
    }
    assert_eq!(names, expected.iter().map(String::as_str).collect());
}

// Verifies REQ-1888: the list's rows name every sub-page in ascending order.
#[test]
fn the_lists_rows_name_every_sub_page() {
    let files = channel_files(&pages_of(&input())).unwrap();
    let mut listed = Vec::new();
    for part in ["100/0.sub", "100/1.sub"] {
        let bytes = &files.iter().find(|(name, _)| name == part).unwrap().1;
        let text = String::from_utf8_lossy(bytes);
        listed.extend(text.lines().skip(1).map(|row| u16::from_str_radix(row.get(..4).unwrap(), 16).unwrap()));
    }
    assert_eq!(listed, subcodes(SUB_PAGES));
}

// Verifies DEC-2413 item 1: a name that would leave the folder is refused.
#[test]
fn a_name_outside_the_folder_is_refused() {
    let folder = Path::new("channel");
    assert!(inside(folder, "100/0001.t42").is_some());
    for name in ["../100.t42", "/etc/100.t42", "", "100/../../x"] {
        assert!(inside(folder, name).is_none(), "{name}");
    }
}
