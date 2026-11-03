#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size, slicing strings it wrote itself and `panic!` on outcomes it rules out"
)]
//! The tests of a font library's index, `fonts.txt` (REQ-124), and of a
//! font's checksum (REQ-123).

use super::*;
use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

/// The set names these tests know, standing in for the core's list.
fn known(name: &str) -> bool {
    ["G0-Latin", "G0-Latin-English", "G2-Latin", "G0-Greek"].contains(&name)
}

/// Sixty-four hexadecimal digits, all `digit`.
fn hash(digit: char) -> String {
    core::iter::repeat_n(digit, crate::consts::SHA256_HEX_DIGITS).collect()
}

fn all(text: &str) -> Vec<Result<Entry<'_>, LineFault>> {
    read(text.as_bytes(), known).unwrap().collect()
}

fn faults(text: &str) -> Vec<(usize, Fault)> {
    all(text).into_iter().filter_map(Result::err).map(|fault| (fault.line, fault.fault)).collect()
}

fn fonts(text: &str) -> Vec<(Location<'_>, &str)> {
    all(text)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(Entry::Font(font)) => Some((font.location, font.title)),
            _ => None,
        })
        .collect()
}

// Verifies REQ-124: a library with fonts in its folder and elsewhere, with
// comments and empty lines, read in order with their titles and hashes.
#[test]
fn fonts_in_the_folder_and_elsewhere() {
    let (a, b) = (hash('a'), hash('B'));
    let text = format!(
        "# a library\r\n\nf\tenglish.bdf\tMode 7 English\t{a}\r\nf\thttps://fonts.example/g/greek.bdf\tGreek elsewhere\t{b}\n"
    );
    let entries = all(&text);
    assert_eq!(entries.len(), 2);
    let Ok(Entry::Font(first)) = &entries[0] else { panic!("{entries:?}") };
    assert_eq!(
        (first.location, first.title, first.sha256),
        (Location::File("english.bdf"), "Mode 7 English", [0xAA; 32])
    );
    let Ok(Entry::Font(second)) = &entries[1] else { panic!("{entries:?}") };
    assert_eq!(
        (second.location, second.title, second.sha256),
        (Location::Address("https://fonts.example/g/greek.bdf"), "Greek elsewhere", [0xBB; 32])
    );
    // A file name is fetched from the library's folder, an address as it is.
    let folder = Url::parse("https://lib.example/fonts/").unwrap();
    assert_eq!(first.location.address(&folder).unwrap().as_str(), "https://lib.example/fonts/english.bdf");
    assert_eq!(second.location.address(&folder).unwrap().as_str(), "https://fonts.example/g/greek.bdf");
}

// Verifies REQ-124: each fault of an `f` line makes it malformed, and the
// reading goes on; an unknown kind is ignored and is not malformed.
#[test]
fn each_fault_of_an_f_line() {
    let a = hash('a');
    let long_title = "T".repeat(MAX_TITLE + 1);
    let longest = "T".repeat(MAX_TITLE);
    let text = [
        String::from("z\tlater\tkind"),
        String::from("f\tone.bdf\tOne"),
        format!("f\tone.bdf\tOne\t{a}\textra"),
        format!("f\tsub/one.bdf\tPath\t{a}"),
        format!("f\t..\tDots\t{a}"),
        format!("f\twss://fonts.example/a.bdf\tSocket\t{a}"),
        format!("f\thttps://user@fonts.example/a.bdf\tUser\t{a}"),
        format!("f\tone.bdf\t\t{a}"),
        format!("f\tone.bdf\t{long_title}\t{a}"),
        format!("f\tone.bdf\tShort\t{}", &a[1..]),
        format!("f\tone.bdf\tNot hex\t{}g", &a[1..]),
        format!("f\tone.bdf\tCaf\u{e9}\t{a}"),
        format!("f\tok.bdf\t{longest}\t{a}"),
        format!("f\tok.bdf\tAgain\t{a}"),
    ]
    .join("\n");
    assert_eq!(
        faults(&text),
        [
            (1, Fault::UnknownKind),
            (2, Fault::Fields),
            (3, Fault::Fields),
            (4, Fault::Location),
            (5, Fault::Location),
            (6, Fault::Location),
            (7, Fault::Location),
            (8, Fault::Title),
            (9, Fault::Title),
            (10, Fault::Checksum),
            (11, Fault::Checksum),
            (12, Fault::Character),
            (14, Fault::DuplicateFont),
        ]
    );
    assert_eq!(fonts(&text), [(Location::File("ok.bdf"), longest.as_str())]);
    assert!(!Fault::UnknownKind.is_malformed() && Fault::DuplicateFont.is_malformed());
}

// Verifies REQ-124: a `c` line states the sets of a listed font, with `*`
// for the SAA5050 cell profile, before or after its `f` line; an empty
// list, or none after the font's name, states no set.
#[test]
fn c_lines_state_the_sets() {
    let a = hash('a');
    let text = format!(
        "c\tone.bdf\tG0-Latin* G0-Latin-English* G2-Latin\nf\tone.bdf\tOne\t{a}\nf\ttwo.bdf\tTwo\t{a}\nc\ttwo.bdf\t\nf\tthree.bdf\tThree\t{a}\nc\tthree.bdf\n"
    );
    let entries = all(&text);
    let sets: Vec<Vec<(&str, bool)>> = entries
        .iter()
        .filter_map(|entry| match entry {
            Ok(Entry::Conformance(conformance)) => Some(conformance.sets().collect()),
            _ => None,
        })
        .collect();
    assert_eq!(sets, [vec![("G0-Latin", true), ("G0-Latin-English", true), ("G2-Latin", false)], vec![], vec![]]);
    assert_eq!(faults(&text), []);
}

// Verifies REQ-124: a `c` line for a font no `f` line lists, a second `c`
// line for a font, an unknown set name, and set names not separated by
// single spaces each make that line malformed.
#[test]
fn each_fault_of_a_c_line() {
    let a = hash('a');
    let text = [
        format!("f\tone.bdf\tOne\t{a}"),
        String::from("f\tbad.bdf\tBad\tnothex"),
        String::from("c\tnone.bdf\tG0-Latin"),
        String::from("c\tbad.bdf\tG0-Latin"),
        String::from("c\tone.bdf\tG0-Klingon"),
        String::from("c\tone.bdf\tG0-Latin  G2-Latin"),
        String::from("c\tone.bdf\tG0-Latin "),
        String::from("c\tone.bdf\tG0-Latin\textra"),
        String::from("c\tone.bdf\tG0-Greek"),
        String::from("c\tone.bdf\tG0-Latin"),
    ]
    .join("\n");
    assert_eq!(
        faults(&text),
        [
            (2, Fault::Checksum),
            (3, Fault::Unlisted),
            (4, Fault::Unlisted),
            (5, Fault::SetName),
            (6, Fault::SetName),
            (7, Fault::SetName),
            (8, Fault::Fields),
            (10, Fault::SecondConformance),
        ]
    );
}

// Verifies REQ-124: an index over 64 kilobytes or 1,000 lines is refused
// whole; one at the bounds is read.
#[test]
fn the_index_is_bounded() {
    let comment = vec![b'#'; MAX_BYTES + 1];
    assert_eq!(read(&comment, known).err(), Some(LibraryError::TooLarge));
    assert!(read(&comment[..MAX_BYTES], known).is_ok());
    let lines = "\n".repeat(MAX_LINES);
    assert!(read(lines.as_bytes(), known).is_ok());
    let over = format!("{lines}#");
    assert_eq!(read(over.as_bytes(), known).err(), Some(LibraryError::TooManyLines));
}

// Verifies REQ-123: the bytes of a font named with a SHA-256 are refused
// when their digest is any other (FIPS 180-4 Appendix B.1's "abc").
#[test]
fn a_wrong_checksum_is_refused() {
    let abc = sha256::parse_hex(b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").unwrap();
    assert_eq!(check_font(b"abc", &abc), Ok(()));
    assert_eq!(check_font(b"abd", &abc), Err(sha256::digest(b"abd")));
}
