#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `expect` naming what it expected, indexing into data it built to a known size, arithmetic on small values it chose, where an overflow would also fail it and `panic!` on outcomes it rules out"
)]
//! The LINKS record at the bound of REQ-1661 and DEC-1668.

use neoteletext_core::consts::PACKET_BYTES;
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_formats::page::{
    LINK_LINES, LinkDestination, LinkKey, LinkLine, LinkText, NUMBER_LINKS, NumberLink, TEXT_LINKS, TextLink,
};
use neoteletext_formats::text::Text;
use neoteletext_formats::tti::link_destination;

use super::{MAX_LABEL, MAX_OTHER_LABEL, MAX_PARTS, Sending, Step, SubPageLinks, encode, read};
use crate::records::{ChannelRecord, Read, RecordFault, RecordReader};
use std::string::String;
use std::vec::Vec;

const SERVICE: u8 = 2;

fn page() -> PageAddress {
    PageAddress::from_mpp(0x100).unwrap()
}

/// An address of `length` octets, told apart from the others by `mark`.
fn address(length: usize, mark: char) -> String {
    let mut text = String::from("https://a.example/");
    text.push(mark);
    text.extend(core::iter::repeat_n('a', length - text.len()));
    text
}

fn key(key: LinkKey, destination: LinkDestination) -> LinkLine {
    let LinkDestination { service, address, subcode, url } = destination;
    LinkLine { key, service, address, subcode, url, label: Text::new(), label_truncated: false, line: 0 }
}

fn write(links: SubPageLinks<'_>) -> Result<Vec<Packet>, RecordFault> {
    let mut packets = Vec::new();
    encode(page(), SubCode::new(0), links, SERVICE, 0, &mut |packet| packets.push(packet))?;
    Ok(packets)
}

/// Every link a reader gives from `packets`, with its address.
fn read_back(packets: &[Packet]) -> Vec<(Option<LinkKey>, Vec<u8>)> {
    let mut reader = RecordReader::new();
    let mut found = Vec::new();
    for packet in packets {
        match reader.push(packet) {
            Read::Record(ChannelRecord::Links(record)) => {
                for link in record.key_links() {
                    found.push((Some(link.key), record.address(link.url).to_vec()));
                }
                for link in record.number_links() {
                    found.push((None, record.address(link.destination.url).to_vec()));
                }
                for link in record.text_links() {
                    found.push((None, record.address(link.destination.url).to_vec()));
                }
            }
            Read::Invalid(count) => panic!("{count} packets refused"),
            _ => {}
        }
    }
    found
}

/// DOC-1958: a target of 8,000 octets goes through a record.
/// REQ-1494: a key link's target goes through a record, whatever its length.
#[test]
fn a_target_of_8000_octets_travels() {
    let mut addresses = LinkText::new();
    let long = address(8000, 'x');
    let keys = [key(LinkKey::Red, link_destination(long.as_bytes(), &mut addresses).unwrap())];
    let packets = write(SubPageLinks { keys: &keys, numbers: &[], texts: &[], addresses: &addresses }).unwrap();
    assert_eq!(read_back(&packets), [(Some(LinkKey::Red), long.into_bytes())]);
}

/// REQ-1661: 8,001 octets is no target, in a TTI link or a record's.
#[test]
fn a_target_past_the_bound_is_no_link() {
    let mut addresses = LinkText::new();
    let web = std::format!("web:{}", address(8000, 'x'));
    assert!(link_destination(web.as_bytes(), &mut addresses).is_some());
    let over = std::format!("web:{}", address(8001, 'x'));
    assert!(link_destination(over.as_bytes(), &mut addresses).is_none());
}

/// REQ-1661: a sub-page with all 21 links at the bound passes through the
/// records, one record a link, and a reader gets each whole and in order.
/// REQ-1494: every kind of link at its most, read back in order.
#[test]
fn every_link_at_the_bound_travels() {
    let mut addresses = LinkText::new();
    let targets: Vec<String> = ('a'..='u').map(|mark| address(8000, mark)).collect();
    let mut destinations = targets.iter().map(|target| link_destination(target.as_bytes(), &mut addresses).unwrap());
    let colours = [LinkKey::Red, LinkKey::Green, LinkKey::Yellow, LinkKey::Cyan, LinkKey::Index];
    let keys = colours.map(|colour| key(colour, destinations.next().unwrap()));
    let numbers: Vec<NumberLink> = (0..8)
        .map(|index| NumberLink {
            number: PageAddress::from_mpp(0x150 + index).unwrap(),
            destination: destinations.next().unwrap(),
            line: 0,
        })
        .collect();
    let texts: Vec<TextLink> = (0..8)
        .map(|index| TextLink {
            row: index + 2,
            column: 0,
            length: 4,
            destination: destinations.next().unwrap(),
            line: 0,
        })
        .collect();
    let packets = write(SubPageLinks { keys: &keys, numbers: &numbers, texts: &texts, addresses: &addresses }).unwrap();
    let found = read_back(&packets);
    assert_eq!(found.len(), MAX_PARTS);
    for (link, target) in found.iter().zip(&targets) {
        assert_eq!(link.1, target.as_bytes());
    }
    // The bandwidth DEC-1668 asks for: 34 bytes of a record in each packet
    // of 42, every time the sub-page is sent.
    assert_eq!(packets.len() * PACKET_BYTES, 208_152);
}

/// A target in a page of this channel.
fn same_channel(addresses: &mut LinkText) -> LinkDestination {
    link_destination(b"150", addresses).unwrap()
}

/// `count` number links, each with a page number of its own.
fn number_links(count: u16, addresses: &mut LinkText) -> Vec<NumberLink> {
    (0..count)
        .map(|index| NumberLink {
            number: PageAddress::from_mpp(0x150 + index).unwrap(),
            destination: same_channel(addresses),
            line: 0,
        })
        .collect()
}

fn text_link(row: u8, column: u8, length: u8, addresses: &mut LinkText) -> TextLink {
    TextLink { row, column, length, destination: same_channel(addresses), line: 0 }
}

/// A key link with a label of `length` characters.
fn labelled(colour: LinkKey, destination: LinkDestination, length: usize) -> LinkLine {
    let (label, _) = Text::from_bytes(&std::vec![b'a'; length]);
    LinkLine { label, ..key(colour, destination) }
}

/// REQ-1494: at most five key links, with four coloured keys and the Index
/// key, each key once.
#[test]
fn a_record_holds_five_key_links_each_key_once() {
    let mut addresses = LinkText::new();
    let colours = [LinkKey::Red, LinkKey::Green, LinkKey::Yellow, LinkKey::Cyan, LinkKey::Index];
    assert_eq!(colours.len(), LINK_LINES);
    let keys = colours.map(|colour| key(colour, same_channel(&mut addresses)));
    let links = |keys: &[LinkLine], addresses: &LinkText| {
        write(SubPageLinks { keys, numbers: &[], texts: &[], addresses }).map(|packets| read_back(&packets).len())
    };
    assert_eq!(links(&keys, &addresses), Ok(LINK_LINES));
    let sixth = [keys.as_slice(), &keys[..1]].concat();
    assert!(links(&sixth, &addresses).is_err(), "a sixth key link");
    let twice = [keys[0], keys[0]];
    assert!(links(&twice, &addresses).is_err(), "a key given twice");
}

/// REQ-1494: at most eight page-number links, and at most eight text links.
#[test]
fn a_record_holds_eight_number_links_and_eight_text_links() {
    let mut addresses = LinkText::new();
    let most = u16::try_from(NUMBER_LINKS).unwrap();
    let numbers = number_links(most, &mut addresses);
    let one_more = number_links(most + 1, &mut addresses);
    let write_numbers = |numbers: &[NumberLink], addresses: &LinkText| {
        write(SubPageLinks { keys: &[], numbers, texts: &[], addresses }).map(|packets| read_back(&packets).len())
    };
    assert_eq!(write_numbers(&numbers, &addresses), Ok(NUMBER_LINKS));
    assert!(write_numbers(&one_more, &addresses).is_err());
    let texts: Vec<TextLink> = (0..=TEXT_LINKS).map(|row| text_link(row as u8 + 1, 0, 1, &mut addresses)).collect();
    let write_texts = |texts: &[TextLink], addresses: &LinkText| {
        write(SubPageLinks { keys: &[], numbers: &[], texts, addresses }).map(|packets| read_back(&packets).len())
    };
    assert_eq!(write_texts(&texts[..TEXT_LINKS], &addresses), Ok(TEXT_LINKS));
    assert!(write_texts(&texts, &addresses).is_err());
}

/// REQ-1494: a label is at most 9 characters for a link in the same service
/// and 8 for one elsewhere, and the Index key has none.
#[test]
fn a_label_keeps_its_bound() {
    let mut addresses = LinkText::new();
    let here = same_channel(&mut addresses);
    let elsewhere = link_destination(b"https://a.example/", &mut addresses).unwrap();
    let write_key = |line: LinkLine, addresses: &LinkText| {
        write(SubPageLinks { keys: &[line], numbers: &[], texts: &[], addresses }).map(|_| ())
    };
    assert_eq!(write_key(labelled(LinkKey::Red, here, MAX_LABEL), &addresses), Ok(()));
    assert!(write_key(labelled(LinkKey::Red, here, MAX_LABEL + 1), &addresses).is_err());
    assert_eq!(write_key(labelled(LinkKey::Red, elsewhere, MAX_OTHER_LABEL), &addresses), Ok(()));
    assert!(write_key(labelled(LinkKey::Red, elsewhere, MAX_OTHER_LABEL + 1), &addresses).is_err());
    let write_index = |length: usize, addresses: &LinkText| {
        let keys = [labelled(LinkKey::Index, here, length)];
        write(SubPageLinks { keys: &keys, numbers: &[], texts: &[], addresses }).unwrap()
    };
    assert_eq!(write_index(1, &addresses), write_index(0, &addresses), "the Index key's label is not written");
}

/// REQ-1494: a text link stands on rows 1 to 23, and its run is inside the
/// 40 columns.
#[test]
fn a_text_link_stays_on_the_page() {
    let mut addresses = LinkText::new();
    let place = |row: u8, column: u8, length: u8, addresses: &mut LinkText| {
        let texts = [text_link(row, column, length, addresses)];
        write(SubPageLinks { keys: &[], numbers: &[], texts: &texts, addresses }).is_ok()
    };
    for (row, column, length, fits) in [
        (1, 0, 1, true),
        (23, 39, 1, true),
        (1, 0, 40, true),
        (0, 0, 1, false),
        (24, 0, 1, false),
        (1, 0, 0, false),
        (1, 39, 2, false),
    ] {
        assert_eq!(place(row, column, length, &mut addresses), fits, "row {row} column {column} length {length}");
    }
}

/// A short link costs what it did: one record, not the bound's.
#[test]
fn short_links_stay_in_one_record() {
    let mut addresses = LinkText::new();
    let keys = [key(LinkKey::Red, link_destination(b"https://a.example/", &mut addresses).unwrap())];
    let packets = write(SubPageLinks { keys: &keys, numbers: &[], texts: &[], addresses: &addresses }).unwrap();
    assert_eq!(packets.len(), 2);
}

/// A LINKS record's body written by hand, so that each part of a sending
/// can be valid alone: `keys` as key letters with page targets, `numbers`
/// as page numbers with page targets, and `texts` text links on rows from
/// 1.
fn body(subcode: u16, part: u8, parts: u8, keys: &[u8], numbers: &[u16], texts: u8) -> Vec<u8> {
    let target = b"150";
    let mut body = Vec::new();
    body.extend_from_slice(&0x100_u16.to_be_bytes());
    body.extend_from_slice(&subcode.to_be_bytes());
    body.extend_from_slice(&[part, parts, u8::try_from(keys.len()).unwrap()]);
    for &letter in keys {
        body.push(letter);
        body.extend_from_slice(&u16::try_from(target.len()).unwrap().to_be_bytes());
        body.extend_from_slice(target);
        body.push(0);
    }
    body.push(u8::try_from(numbers.len()).unwrap());
    for &number in numbers {
        body.extend_from_slice(&number.to_be_bytes());
        body.extend_from_slice(&u16::try_from(target.len()).unwrap().to_be_bytes());
        body.extend_from_slice(target);
    }
    body.push(texts);
    for row in 1..=texts {
        body.extend_from_slice(&[row, 0, 1]);
        body.extend_from_slice(&u16::try_from(target.len()).unwrap().to_be_bytes());
        body.extend_from_slice(target);
    }
    body
}

/// What the tracker makes of each body in turn, each taking one packet more
/// than the one before so that the counts tell them apart.
fn steps(bodies: &[Vec<u8>]) -> Vec<Step> {
    let mut sending = Sending::new();
    bodies
        .iter()
        .enumerate()
        .map(|(index, body)| sending.note(&read(body).expect("each part is valid alone"), index + 1))
        .collect()
}

/// DOC-1958, REQ-1494: a sending within its totals is taken part by part,
/// and complete at its last part; a new part 0 begins again.
#[test]
fn a_sending_within_its_totals_is_taken() {
    let bodies = [
        body(0, 0, 3, b"RG", &[0x101], 1),
        body(0, 1, 3, b"YC", &[0x102], 1),
        body(0, 2, 3, b"I", &[0x103], 1),
        body(0, 0, 1, b"R", &[], 0),
    ];
    assert_eq!(steps(&bodies), [Step::Partial, Step::Partial, Step::Complete, Step::Complete]);
}

/// DOC-1958 (bounds across the parts), REQ-1462: a key given in two parts,
/// a page number given in two parts, a ninth page-number link and a ninth
/// text link each break the sending, every packet of it counted.
#[test]
fn a_sending_that_breaks_a_total_across_parts_is_refused() {
    let numbers: Vec<u16> = (0x101..=0x108).collect();
    for (what, second) in [
        ("a key twice", body(0, 1, 2, b"R", &[], 0)),
        ("a page number twice", body(0, 1, 2, b"", &[0x101], 0)),
        ("a ninth page-number link", body(0, 1, 2, b"", &[0x109], 0)),
        ("a ninth text link", body(0, 1, 2, b"", &[], 1)),
    ] {
        let first = if what.starts_with("a ninth") {
            body(0, 0, 2, b"R", &numbers, 8)
        } else {
            body(0, 0, 2, b"R", &[0x101], 0)
        };
        assert_eq!(steps(&[first, second]), [Step::Partial, Step::Refused { packets: 3 }], "{what}");
    }
}

/// DOC-1958, DOC-1952: a part out of turn breaks its sending: a later part
/// with no part 0, a part skipped, a part of another sub-page, a part
/// again, and a part that names other parts than its part 0.
#[test]
fn a_part_out_of_turn_is_refused() {
    let first = body(0, 0, 3, b"R", &[], 0);
    assert_eq!(steps(&[body(0, 1, 3, b"G", &[], 0)]), [Step::Refused { packets: 1 }], "no part 0");
    for (what, later) in [
        ("a part skipped", body(0, 2, 3, b"G", &[], 0)),
        ("another sub-page", body(1, 1, 3, b"G", &[], 0)),
        ("other parts", body(0, 1, 4, b"G", &[], 0)),
    ] {
        assert_eq!(steps(&[first.clone(), later]), [Step::Partial, Step::Refused { packets: 2 }], "{what}");
    }
    let again = steps(&[first.clone(), body(0, 1, 3, b"G", &[], 0), body(0, 1, 3, b"Y", &[], 0)]);
    assert_eq!(again, [Step::Partial, Step::Partial, Step::Refused { packets: 3 }], "a part again");
}

/// The one page of a TTI file, as the TTI reader gives it.
fn page_file(tti: &[u8]) -> neoteletext_formats::page::PageFile {
    let mut pages = Vec::new();
    let mut reader = neoteletext_formats::tti::TtiReader::new();
    let mut keep = |item: neoteletext_formats::Item<'_>| {
        if let neoteletext_formats::Item::Page(page) = item {
            pages.push(page.clone());
        }
    };
    neoteletext_formats::Reader::feed(&mut reader, tti, &mut keep);
    neoteletext_formats::Reader::finish(&mut reader, &mut keep);
    pages.remove(0)
}

/// REQ-1494 verification item 1: a record built from a TTI page decodes to
/// the same links as the TTI reader gives for that page, key links,
/// page-number links and text links, with their addresses.
#[test]
fn a_record_from_a_page_gives_the_pages_links() {
    let page = page_file(
        b"PN,10000\nOL,1,NEWS INDEX\nLN,R,101,News\nLN,G,sport:140,Sport\nLN,Y,https://w.example/,Web\n\
LN,I,100,\nLK,300,https://weather.example/300.t42\nLK,400,sport:100\nLT,4,0,7,http://w.example/\nLT,5,2,3,150\n",
    );
    assert!(page.links_valid, "the page's key links are valid");
    let mut packets = Vec::new();
    super::encode_page(&page, SERVICE, 0, &mut |packet| packets.push(packet)).unwrap();
    let mut reader = RecordReader::new();
    let mut seen = 0;
    for packet in &packets {
        if let Read::Record(ChannelRecord::Links(record)) = reader.push(packet) {
            seen += 1;
            let keys: Vec<_> = record.key_links().collect();
            assert_eq!(keys.len(), page.links.as_slice().len());
            for (sent, held) in keys.iter().zip(page.links.as_slice()) {
                assert_eq!(
                    (sent.key, sent.service, sent.address, sent.subcode),
                    (held.key, held.service, held.address, held.subcode)
                );
                assert_eq!(sent.label.as_bytes(), held.label.as_bytes());
                assert_eq!(record.address(sent.url), page.link_text.address(held.url));
            }
            let numbers: Vec<_> = record.number_links().collect();
            assert_eq!(numbers.len(), page.number_links.as_slice().len());
            for (sent, held) in numbers.iter().zip(page.number_links.as_slice()) {
                assert_eq!(sent.number, held.number);
                assert_eq!(
                    (sent.destination.service, sent.destination.address, sent.destination.subcode),
                    (held.destination.service, held.destination.address, held.destination.subcode)
                );
                assert_eq!(record.address(sent.destination.url), page.link_text.address(held.destination.url));
            }
            let texts: Vec<_> = record.text_links().collect();
            assert_eq!(texts.len(), page.text_links.as_slice().len());
            for (sent, held) in texts.iter().zip(page.text_links.as_slice()) {
                assert_eq!((sent.row, sent.column, sent.length), (held.row, held.column, held.length));
                assert_eq!(record.address(sent.destination.url), page.link_text.address(held.destination.url));
            }
        }
    }
    assert_eq!(seen, 1, "the links fit one record");
}
