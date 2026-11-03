#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
//! The index record at its bounds (REQ-1477, REQ-1480, DOC-1955), through
//! packets, the reader and the sending's progress.

use neoteletext_core::limits::{
    FIRST_PAGE_ADDRESS, LAST_PAGE_ADDRESS, MAGAZINES, MAX_SUBPAGES, MPP_MAGAZINE_SHIFT, USABLE_PAGE_NUMBERS,
    is_reserved_page,
};
use neoteletext_core::packet::Packet;

use super::{IndexError, IndexMagazine, IndexPage, MAX_TEXT_BYTES, MAX_TEXT_CHARACTERS, Progress, Step, encode, read};
use crate::records::{ChannelRecord, Read, RecordReader};
use std::string::String;
use std::vec::Vec;

const SERVICE: u8 = 1;
const GENERATION: u16 = 7;

/// Text of `characters` four-byte characters.
fn wide(characters: usize) -> String {
    core::iter::repeat_n('\u{1F4FA}', characters).collect()
}

/// Every page number of `magazine` an index may list, ascending.
fn usable(magazine: u8) -> Vec<u16> {
    let first = u16::from(magazine) << MPP_MAGAZINE_SHIFT;
    (0..=u8::MAX)
        .filter(|&page| !is_reserved_page(magazine, page))
        .map(|page| first | u16::from(page))
        .filter(|page| (FIRST_PAGE_ADDRESS..=LAST_PAGE_ADDRESS).contains(page))
        .collect()
}

fn write(magazines: &[IndexMagazine<'_>]) -> Result<Vec<Packet>, IndexError> {
    let mut packets = Vec::new();
    encode(GENERATION, magazines, SERVICE, 0, &mut |packet| packets.push(packet))?;
    Ok(packets)
}

/// A page read back: magazine, number, sub-pages, title and section.
type ReadPage = (u8, u16, u16, String, String);

/// Every page and magazine name a reader takes from `packets`, and the
/// progress of the sending after each record.
fn read_back(packets: &[Packet]) -> (Vec<ReadPage>, Vec<(u8, String)>, Vec<Step>) {
    let mut reader = RecordReader::new();
    let mut progress = Progress::new();
    let (mut pages, mut names, mut steps) = (Vec::new(), Vec::new(), Vec::new());
    for packet in packets {
        match reader.push(packet) {
            Read::Record(ChannelRecord::Index(record)) => {
                if !record.name.is_empty() {
                    names.push((record.magazine, String::from(record.name)));
                }
                for page in record.pages() {
                    let IndexPage { page, subpages, title, section } = page;
                    pages.push((record.magazine, page, subpages, String::from(title), String::from(section)));
                }
                steps.push(progress.note(&record));
            }
            Read::Invalid(count) => panic!("{count} packets refused"),
            _ => {}
        }
    }
    (pages, names, steps)
}

/// REQ-1477, REQ-1480: an index at every bound, all eight magazines with
/// every page they may hold (253 in a magazine, 251 in magazine 1, 2,022
/// in all), names, titles and sections of 120 four-byte characters and
/// 8,190 sub-pages, reads back equal, complete only at its last part.
#[test]
fn an_index_at_every_bound_reads_back_whole() {
    let text = wide(MAX_TEXT_CHARACTERS);
    assert_eq!(text.len(), MAX_TEXT_BYTES);
    let numbers: Vec<Vec<u16>> = (1..=MAGAZINES as u8).map(usable).collect();
    assert_eq!(numbers[0].len(), 251);
    assert!(numbers[1..].iter().all(|pages| pages.len() == 253));
    assert_eq!(numbers.iter().map(Vec::len).sum::<usize>(), USABLE_PAGE_NUMBERS);
    let pages: Vec<Vec<IndexPage<'_>>> = numbers
        .iter()
        .map(|numbers| {
            numbers
                .iter()
                .map(|&page| IndexPage { page, subpages: MAX_SUBPAGES as u16, title: &text, section: &text })
                .collect()
        })
        .collect();
    let magazines: Vec<IndexMagazine<'_>> = pages
        .iter()
        .enumerate()
        .map(|(index, pages)| IndexMagazine { magazine: index as u8 + 1, name: &text, pages })
        .collect();
    let (read, names, steps) = read_back(&write(&magazines).unwrap());
    let expected: Vec<ReadPage> = magazines
        .iter()
        .flat_map(|magazine| {
            magazine.pages.iter().map(|page| {
                (magazine.magazine, page.page, page.subpages, String::from(page.title), String::from(page.section))
            })
        })
        .collect();
    assert_eq!(read, expected);
    assert_eq!(names, (1..=MAGAZINES as u8).map(|magazine| (magazine, text.clone())).collect::<Vec<_>>());
    let (last, earlier) = steps.split_last().unwrap();
    assert_eq!(*last, Step::Complete);
    assert!(earlier.iter().all(|step| *step == Step::Partial));
}

/// REQ-1477: one past each bound, and each rule of order, repetition,
/// reserved pages and magazines, is refused by `encode` before anything is
/// written.
#[test]
fn an_index_past_a_bound_is_refused_by_encode() {
    let title = "T";
    let page = |page: u16| IndexPage { page, subpages: 1, title, section: "" };
    let one = |magazine: u8, pages: &[IndexPage<'_>]| write(&[IndexMagazine { magazine, name: "", pages }]).map(|_| ());
    let over = wide(MAX_TEXT_CHARACTERS + 1);
    assert_eq!(one(1, &[page(0x100)]), Ok(()));
    assert_eq!(one(0, &[]), Err(IndexError::Magazine), "magazine 0");
    assert_eq!(one(9, &[]), Err(IndexError::Magazine), "magazine 9");
    let twice =
        [IndexMagazine { magazine: 2, name: "", pages: &[] }, IndexMagazine { magazine: 1, name: "", pages: &[] }];
    assert_eq!(write(&twice).map(|_| ()), Err(IndexError::Magazine), "out of order");
    assert_eq!(one(1, &[page(0x101), page(0x100)]), Err(IndexError::Page), "pages out of order");
    assert_eq!(one(1, &[page(0x100), page(0x100)]), Err(IndexError::Page), "a page twice");
    assert_eq!(one(1, &[page(0x200)]), Err(IndexError::Page), "a page of another magazine");
    assert_eq!(one(1, &[page(0x1FF)]), Err(IndexError::Page), "a reserved page");
    assert_eq!(one(1, &[IndexPage { subpages: 0, ..page(0x100) }]), Err(IndexError::SubPages));
    assert_eq!(one(1, &[IndexPage { subpages: MAX_SUBPAGES as u16 + 1, ..page(0x100) }]), Err(IndexError::SubPages));
    assert_eq!(one(1, &[IndexPage { title: "", ..page(0x100) }]), Err(IndexError::Text), "an empty title");
    assert_eq!(one(1, &[IndexPage { title: &over, ..page(0x100) }]), Err(IndexError::Text), "121 characters");
    assert_eq!(one(1, &[IndexPage { section: "a\u{7}", ..page(0x100) }]), Err(IndexError::Text), "a control");
    assert_eq!(write(&[IndexMagazine { magazine: 1, name: &over, pages: &[] }]).map(|_| ()), Err(IndexError::Text));
}

/// An index record's body written by hand.
fn body(part: u16, parts: u16, magazine: u8, pages: &[(u16, u16, &str)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for field in [GENERATION, part, parts] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes.push(magazine);
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    for &(page, subpages, title) in pages {
        bytes.extend_from_slice(&page.to_be_bytes());
        bytes.extend_from_slice(&subpages.to_be_bytes());
        bytes.extend_from_slice(&u16::try_from(title.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(title.as_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
    }
    bytes
}

/// REQ-1477, DOC-1955: `read` refuses what `encode` would never write: a
/// part not below its parts, no parts, too many parts, a magazine outside
/// 1 to 8, a page of another magazine, reserved, repeated or out of order,
/// a sub-page count outside 1 to 8,190, a text past its bound, and bytes
/// cut short.
#[test]
fn an_index_record_past_a_bound_is_refused_by_read() {
    assert!(read(&body(0, 1, 1, &[(0x100, 1, "T")])).is_ok());
    let over = wide(MAX_TEXT_CHARACTERS + 1);
    for (what, bytes) in [
        ("part equal to parts", body(1, 1, 1, &[])),
        ("no parts", body(0, 0, 1, &[])),
        ("too many parts", body(0, super::MAX_PARTS as u16 + 1, 1, &[])),
        ("magazine 0", body(0, 1, 0, &[])),
        ("magazine 9", body(0, 1, 9, &[])),
        ("another magazine's page", body(0, 1, 1, &[(0x200, 1, "T")])),
        ("a reserved page", body(0, 1, 1, &[(0x1FF, 1, "T")])),
        ("a page twice", body(0, 1, 1, &[(0x100, 1, "T"), (0x100, 1, "T")])),
        ("out of order", body(0, 1, 1, &[(0x101, 1, "T"), (0x100, 1, "T")])),
        ("no sub-pages", body(0, 1, 1, &[(0x100, 0, "T")])),
        ("8,191 sub-pages", body(0, 1, 1, &[(0x100, MAX_SUBPAGES as u16 + 1, "T")])),
        ("an empty title", body(0, 1, 1, &[(0x100, 1, "")])),
        ("121 characters", body(0, 1, 1, &[(0x100, 1, &over)])),
    ] {
        assert!(read(&bytes).is_err(), "{what}");
    }
    let whole = body(0, 1, 1, &[(0x100, 1, "T")]);
    assert!(read(&whole[..whole.len() - 1]).is_err(), "cut short");
}

/// DOC-1955: a sending's parts may come in any order and once each; a part
/// that names other parts than the sending's first is refused, and a new
/// generation begins again.
#[test]
fn the_progress_of_a_sending() {
    let parts = |part: u16, parts: u16| read_owned(body(part, parts, 1, &[]));
    let mut progress = Progress::new();
    assert_eq!(parts(1, 3).with(|record| progress.note(record)), Step::Partial);
    assert_eq!(parts(1, 3).with(|record| progress.note(record)), Step::Partial, "a part again counts once");
    assert_eq!(parts(0, 4).with(|record| progress.note(record)), Step::Refused);
    assert_eq!(parts(0, 3).with(|record| progress.note(record)), Step::Partial);
    assert_eq!(parts(2, 3).with(|record| progress.note(record)), Step::Complete);
}

/// A body kept beside the record read from it.
struct Owned(Vec<u8>);

fn read_owned(bytes: Vec<u8>) -> Owned {
    Owned(bytes)
}

impl Owned {
    fn with<T>(&self, each: impl FnOnce(&super::IndexRecord<'_>) -> T) -> T {
        each(&read(&self.0).unwrap())
    }
}

/// REQ-WEB-018 (TODO-1528), DOC-1955: an index record's entries are what a
/// `contents.txt` with the same sections and pages gives through
/// `contents::read`, so a port shows a live channel's contents and a static
/// one's by one path.
#[test]
fn an_index_records_entries_are_contents_txts() {
    let pages = [
        IndexPage { page: 0x100, subpages: 1, title: "Index", section: "" },
        IndexPage { page: 0x101, subpages: 3, title: "Headlines", section: "News" },
        IndexPage { page: 0x102, subpages: 1, title: "World", section: "News" },
        IndexPage { page: 0x150, subpages: 1, title: "Weather", section: "Weather" },
    ];
    let packets = write(&[IndexMagazine { magazine: 1, name: "", pages: &pages }]).unwrap();
    let contents = b"p\t100\tIndex\ns\tNews\np\t101\tHeadlines\np\t102\tWorld\ns\tWeather\np\t150\tWeather\n";
    let from_file: Vec<_> = crate::contents::read(contents).unwrap().map(Result::unwrap).collect();
    let mut reader = RecordReader::new();
    let mut from_record = Vec::new();
    for packet in &packets {
        if let Read::Record(ChannelRecord::Index(record)) = reader.push(packet) {
            from_record.extend(record.entries().map(|entry| std::format!("{entry:?}")));
        }
    }
    let from_file: Vec<_> = from_file.iter().map(|entry| std::format!("{entry:?}")).collect();
    assert_eq!(from_record, from_file);
}
