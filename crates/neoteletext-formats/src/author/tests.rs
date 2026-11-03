//! Builder tests (REQ-INP-025): the builder reproduces the worked encodings
//! of `research/notes-level25-objects.md` sections 13.2 to 13.4 and
//! `research/notes-level25-drcs.md` sections 3.5, 4, 5 and 11, and what it
//! builds decodes back to the same fields.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    clippy::cast_possible_truncation
)]

use super::*;
use crate::item::Reader;
use crate::page::RawBody;
use neoteletext_core::consts::PACKET_LINKS;
use neoteletext_core::ecc::{hamming84_encode, hamming2418_encode};
use neoteletext_core::packet::{
    Decoded, DefaultObject, MotPageEntry, ObjectKind, ObjectPacket, ObjectSource, ObjectUsage, TripletFunction, decode,
    mot_object_link,
};
use neoteletext_core::types::Magazine;
use std::vec::Vec;

mod round_trip;

const BOTH: ObjectUsage = ObjectUsage(0b11);
/// Every style of Font Style: proportional spacing, bold and italic (data
/// bits D0 to D2, Table 29).
const EVERY_STYLE: FontStyle = FontStyle { proportional: true, bold: true, italics: true };

fn values(triplets: &Triplets) -> Vec<u32> {
    let resolve = |_: LocalObject, _: ObjectKind| -> Result<Triplet> { Err(AuthorError::UnknownLocalObject) };
    triplets.resolved(&resolve).unwrap().iter().map(|triplet| triplet.value()).collect()
}

fn one(build: impl FnOnce(&mut Triplets)) -> u32 {
    let mut triplets = Triplets::new();
    build(&mut triplets);
    values(&triplets)[0]
}

fn raw_triplets(page: &PageFile, number: u8, designation: u8) -> [Triplet; 13] {
    let raw = page.raw_packets.as_slice().iter().find(|raw| raw.number == number && raw.designation == designation);
    let Some(RawPacket { body: RawBody::Triplets(triplets), .. }) = raw else { panic!("no {number}/{designation}") };
    *triplets
}

pub(super) fn packets_of(page: &PageFile) -> Vec<Packet> {
    let mut packets = Vec::new();
    encode_page(page, &mut |packet| packets.push(packet));
    packets
}

/// REQ-INP-025: the triplet encodings of objects section 13.2.
#[test]
fn object_triplets_match_the_worked_encodings() {
    let mut pop = ObjectPage::new(address(0x2E3).unwrap(), false);
    let c3 = pop.define_at(2, ObjectKind::Active, BOTH, 3, Triplets::new()).unwrap();
    assert_eq!(c3, ObjectRef { source: ObjectSource::Public, kind: ObjectKind::Active, number: public_number(2, 3) });
    let invocation = c3.invocation().to_triplet().value();
    assert_eq!(invocation, 0x19470);
    assert_eq!(hamming2418_encode(invocation), [0x81, 0x47, 0x32]);
    let definition = TripletFunction::ObjectDefinition { usage: BOTH, kind: ObjectKind::Active, number: c3.number };
    assert_eq!(definition.to_triplet().value(), 0x19578);
    assert_eq!(hamming2418_encode(0x19578), [0x43, 0x57, 0x32]);
    assert_eq!(
        one(|t| {
            t.origin(2, 56);
        }),
        0x1C42A
    );
    assert_eq!(hamming2418_encode(0x1C42A), [0xDB, 0x42, 0x38]);
    assert_eq!(
        one(|t| {
            t.terminate(crate::consts::TERMINATION_LOCAL_DATA_END);
        }),
        0x03FFF
    );
    assert_eq!(
        one(|t| {
            t.terminate(crate::consts::TERMINATION_FINAL_MORE);
        }),
        0x017FF
    );
    assert_eq!(hamming2418_encode(0x017FF), [0x76, 0x7F, 0x02]);
}

/// REQ-INP-025: a Local Object's number is the place of its definition;
/// with 17 triplets of local data and the marker, a Passive object lands
/// at X/26/1 triplet 5 (objects section 13.2: value 0x0ACE8).
#[test]
fn local_object_numbers_come_from_the_layout() {
    let mut page = DisplayPage::new(address(0x100).unwrap(), SubCode::new(0));
    let mut body = Triplets::new();
    body.character(0, b'P');
    let passive = page.local_object(ObjectKind::Passive, BOTH, body);
    page.local().position(1, 0).unwrap();
    page.local().pad_to(16, TripletFunction::SetActivePosition { address: 41, column: 0 });
    page.local().invoke_local(passive);
    let built = page.build().unwrap();
    let triplets: Vec<Triplet> = built.enhancements.as_slice().iter().flat_map(|packet| packet.triplets).collect();
    assert_eq!(triplets[16].value(), 0x0ACE8);
    assert_eq!(hamming2418_encode(0x0ACE8), [0xC9, 0x4E, 0x15]);
    assert_eq!(
        triplets[17],
        TripletFunction::Termination { kind: crate::consts::TERMINATION_LOCAL_DATA_OBJECTS_FOLLOW }.to_triplet()
    );
    let TripletFunction::ObjectDefinition { kind, number, .. } = triplets[18].function() else { panic!() };
    assert_eq!((kind, number.designation(), number.triplet()), (ObjectKind::Passive, 1, 5));
    assert_eq!(triplets[19].function(), TripletFunction::G0Diacritic { column: 0, code: b'P', diacritic: 0 });
    let last = TripletFunction::Termination { kind: crate::consts::TERMINATION_LOCAL_OBJECT_LAST }.to_triplet();
    assert!(triplets[20..].iter().all(|&triplet| triplet == last), "padded with the last marker");
    assert_eq!(triplets.len(), 26);
}

/// REQ-INP-025: a page with no local enhancement data sends no packet
/// X/26. Clause 13.8 makes any X/26 cancel the default objects, so an empty
/// one would change what the page shows.
#[test]
fn no_local_data_sends_no_packet_26() {
    let page = DisplayPage::new(address(0x400).unwrap(), SubCode::new(0));
    assert_eq!(page.build().unwrap().enhancements.as_slice(), []);
}

/// REQ-INP-025: the pointer table of objects section 13.2: C2 unused and
/// C3 at absolute triplet 30, which is packet 5 triplet 4.
#[test]
fn pointer_tables_are_computed() {
    let mut pop = ObjectPage::new(address(0x2E3).unwrap(), false);
    let mut long = Triplets::new();
    long.pad_to(
        28,
        TripletFunction::ForegroundColour {
            column: 0,
            colour: neoteletext_core::packet::ColourData { index: 1, high: 0 },
        },
    );
    pop.define_at(0, ObjectKind::Active, BOTH, 0, long).unwrap();
    let mut body = Triplets::new();
    body.character(3, b'X');
    pop.define_at(0, ObjectKind::Active, BOTH, 3, body).unwrap();
    let pages = pop.build().unwrap();
    assert_eq!(pages.len(), 1);
    let page = &pages[0];
    let packets = packets_of(page);
    let row = |number: u8| {
        packets
            .iter()
            .find_map(|packet| match decode(packet) {
                Ok(Decoded::Row(row)) if row.row == number => Some(row.bytes),
                _ => None,
            })
            .unwrap()
    };
    let pointers = ObjectPacket::new(&row(1));
    assert!(pointers.carries_pointers());
    assert_eq!(pointers.triplets[4].unwrap().value(), 0x03DFF);
    assert_eq!(hamming2418_encode(0x03DFF), [0xFE, 0x5F, 0x87]);
    assert_eq!(pointers.pointer(1, false), 0, "C0 at absolute triplet 0");
    assert_eq!(pointers.pointer(1, true), 0x1FF, "C1 unused");
    let definitions = ObjectPacket::new(&row(5));
    assert!(!definitions.carries_pointers());
    let TripletFunction::ObjectDefinition { number, .. } = definitions.triplets[4].unwrap().function() else {
        panic!()
    };
    assert_eq!(number, public_number(0, 3));
    assert!(
        packets.iter().all(|packet| !matches!(decode(packet), Ok(Decoded::Row(row)) if row.row == 2)),
        "packet 2 unused"
    );
    // Annex A.1: the last packet sent is X/5, S1 = 0.
    assert_eq!(page.subcode.value(), 5 << 8);
    assert_eq!(page.function, Some(crate::page::PageFunction { function: 3, coding: 2 }));
}

/// REQ-INP-025: objects of index 8 and above need further pointer
/// packets, and definitions then start after them (clause 10.5.1.2).
#[test]
fn pointer_packets_extend_for_high_indices() {
    let mut gpop = ObjectPage::new(address(0x1E0).unwrap(), true);
    let passive = gpop.define_at(1, ObjectKind::Passive, BOTH, 25, Triplets::new()).unwrap();
    assert_eq!(passive.source, ObjectSource::Global);
    assert_eq!(pointer_place(ObjectKind::Passive, 25), (4, 3, true));
    let pages = gpop.build().unwrap();
    let packets = packets_of(&pages[0]);
    let rows: Vec<u8> = packets
        .iter()
        .filter_map(|packet| if let Ok(Decoded::Row(row)) = decode(packet) { Some(row.row) } else { None })
        .collect();
    assert_eq!(rows, [1, 2, 3, 4, 5]);
    let Ok(Decoded::Row(row)) =
        decode(packets.iter().find(|p| matches!(decode(p), Ok(Decoded::Row(r)) if r.row == 4)).unwrap())
    else {
        panic!()
    };
    let table = ObjectPacket::new(&row.bytes);
    // Packet 5 triplet 0 is absolute triplet 26.
    assert_eq!(table.pointer(3, true), 26);
    assert_eq!(pages[0].subcode.value(), (5 << 8) | 1);
}

/// REQ-INP-025: the X/27/4 link of objects section 13.2.
#[test]
fn compositional_link_matches_the_worked_encoding() {
    let target = address(0x2E3).unwrap();
    let pop = link(LinkFunction::Pop, VALID_BOTH, target, 0b11);
    let (first, second) = pop.to_values(Magazine::new(1).unwrap());
    assert_eq!((first, second), (0x39CCD, 0x0000C));
    assert_eq!(hamming2418_encode(first), [0x66, 0x4C, 0xF3]);
    assert_eq!(hamming2418_encode(second), [0xEA, 0x80, 0x80]);
    let mut page = DisplayPage::new(address(0x100).unwrap(), SubCode::new(0));
    page.link_level_25(LinkFunction::Pop, target, 0b11).unwrap();
    let built = page.build().unwrap();
    let triplets = raw_triplets(&built, PACKET_LINKS, 4);
    assert_eq!((triplets[2].value(), triplets[3].value()), (0x39CCD, 0x0000C));
    let Some(Ok(Decoded::CompositionalLinks { links, .. })) =
        packets_of(&built).iter().map(decode).find(|item| matches!(item, Ok(Decoded::CompositionalLinks { .. })))
    else {
        panic!()
    };
    assert_eq!(links[1], Some(pop));
    assert!(links[0].unwrap().is_null() && links[2].unwrap().is_null() && links[5].unwrap().is_null());
}

/// REQ-INP-025: the table positions of objects sections 7.2 and 13.3, the
/// Hamming 8/4 values of section 13.2 and the default objects of 13.4.
#[test]
fn magazine_organization_table_places_pairs_and_links() {
    let magazine = Magazine::new(1).unwrap();
    let mut mot = Mot::new(magazine);
    let entry = MotPageEntry { global_objects: true, pop_link: 1 };
    for page in [0x23, 0x4F, 0xB7, 0xFE] {
        mot.page(PageNumber(page), entry);
    }
    let mut pop_link = object_link(address(0x1A0).unwrap(), 2);
    pop_link.default_objects = [
        Some(DefaultObject { kind: ObjectKind::Active, number: 0x32 }),
        Some(DefaultObject { kind: ObjectKind::Adaptive, number: 0x32 }),
    ];
    mot.object_link(MotLevel::Level25, 1, pop_link).unwrap();
    let page = mot.build().unwrap();
    let packets = packets_of(&page);
    let row = |number: u8| {
        packets.iter().find_map(|packet| match decode(packet) {
            Ok(Decoded::Row(row)) if row.row == number => Some(row.bytes),
            _ => None,
        })
    };
    let pair = |number: u8, byte: usize| {
        let bytes = row(number).unwrap();
        (bytes[byte - 6], bytes[byte - 5])
    };
    // The second byte of a pair names no DRCS page (decision E.95).
    let expected = (hamming84_encode(0x8 | 1), hamming84_encode(0));
    assert_eq!(pair(2, 12), expected, "x23");
    assert_eq!(pair(10, 28), expected, "x4F");
    assert_eq!(pair(6, 40), expected, "xB7");
    assert_eq!(pair(14, 14), expected, "xFE");
    let hamming: Vec<u8> = (0..16).map(hamming84_encode).collect();
    assert_eq!(
        hamming,
        [0x15, 0x02, 0x49, 0x5E, 0x64, 0x73, 0x38, 0x2F, 0xD0, 0xC7, 0x8C, 0x9B, 0xA1, 0xB6, 0xFD, 0xEA]
    );
    let links = row(19).unwrap();
    assert_eq!(mot_object_link(&links, 1), Some(pop_link));
    let raw = encode::mot_object_link(&pop_link);
    assert_eq!(&raw[5..8], &[0x9, 0x2, 0x3]);
    assert_eq!(mot_object_link(&links, 0).unwrap().page.0, 0xFF, "the GPOP link is not used");
    assert!(row(20).is_some() && row(21).is_some() && row(22).is_none(), "only the Level 2.5 packets");
    // X/21's links to pattern pages are written unused: magazine 1, page
    // FF, no sub-pages (Table 23, decision E.95).
    let unused = [1, 0xF, 0xF, 0].map(hamming84_encode);
    assert!(row(21).unwrap()[..32].as_chunks::<4>().0.iter().all(|link| *link == unused));
    assert_eq!(page.subcode.value(), 21 << 8);
}

/// REQ-INP-025: the DRCS triplets of DRCS sections 5 and 11, sent as
/// written though not acted on (decision E.95), and other worked
/// encodings, with their TTI characters.
#[test]
fn drcs_and_character_triplets_match_the_worked_encodings() {
    let cases = [
        (
            one(|t| {
                t.function(TripletFunction::ReservedRow { mode: 0b11000, address: 41, data: 0x70 });
            }),
            0x38629,
            [0x69, 0x58, 0x78],
        ),
        (
            one(|t| {
                t.function(TripletFunction::ReservedRow { mode: 0b11000, address: 41, data: 0x13 });
            }),
            0x09E29,
            [0x69, 0x78, 0x49],
        ),
        (
            one(|t| {
                t.function(TripletFunction::UnsupportedColumn { column: 2, mode: 0b01101, data: 0x40 });
            }),
            0x20342,
            [0x42, 0x4D, 0x60],
        ),
        (
            one(|t| {
                t.function(TripletFunction::UnsupportedColumn { column: 10, mode: 0b01101, data: 47 });
            }),
            0x17B4A,
            [0x4A, 0x6D, 0x57],
        ),
        (
            one(|t| {
                t.g3(0, 0x20);
            }),
            0x102C0,
            [0x40, 0x4B, 0x50],
        ),
        (
            one(|t| {
                t.g3_level_15(5, 0x5B);
            }),
            0x2D885,
            [0x45, 0x62, 0x6D],
        ),
        (
            one(|t| {
                // REQ-189: Font Style by name, for two more rows (D6 to D4).
                t.font_style(4, EVERY_STYLE, 2).unwrap();
            }),
            0x13B84,
            [0x44, 0x6E, 0x53],
        ),
        (
            one(|t| {
                t.terminate(7);
            }),
            0x03FFF,
            [0x7F, 0x7F, 0x43],
        ),
    ];
    for (value, expected, characters) in cases {
        assert_eq!(value, expected, "{expected:#x}");
        let mut triplets = [Triplet::default(); 13];
        triplets[0] = Triplet::from_value(value);
        assert_eq!(&crate::encode::pack_triplets(&triplets)[..3], &characters, "{expected:#x}");
    }
}

/// Decision E.127: the control triplet is written as DOC-1844
/// gives it, the bytes of its worked example, and a width that does not
/// fit is refused.
#[test]
fn controls_are_written_as_input_md_gives_them() {
    let mut page = DisplayPage::new(address(0x128).unwrap(), SubCode::new(0));
    page.local().position(20, 0).unwrap();
    for column in [1, 11, 21, 31] {
        page.local().control(column, 9, false).unwrap();
    }
    let built = page.build().unwrap();
    let triplets: Vec<Triplet> = built.enhancements.as_slice().iter().flat_map(|packet| packet.triplets).collect();
    let values: Vec<u32> = triplets[..5].iter().map(|triplet| triplet.value()).collect();
    assert_eq!(values, [0x0013C, 0x04941, 0x0494B, 0x04955, 0x0495F]);
    assert_eq!(hamming2418_encode(0x04941), [0x8F, 0x94, 0x89]);
    assert_eq!(hamming2418_encode(0x2A942), [0x98, 0x94, 0xD5], "column 2, width 21, joined");
    assert_eq!(page.local().control(32, 9, false).err(), Some(AuthorError::BadPosition), "past column 39");
    assert_eq!(page.local().control(0, 0, false).err(), Some(AuthorError::BadPosition), "no width");
}

/// REQ-189: Font Style's row count has three bits, so eight more rows are
/// refused, and a style left clear is written as a cancel (Table 29).
#[test]
fn font_style_refuses_more_than_seven_rows() {
    let mut triplets = Triplets::new();
    assert_eq!(triplets.font_style(0, FontStyle::default(), 8).err(), Some(AuthorError::BadPosition));
    let bold = FontStyle { bold: true, ..FontStyle::default() };
    assert_eq!(
        one(|t| {
            t.font_style(5, bold, 7).unwrap();
        }) >> 11,
        0b111_0010
    );
}

/// REQ-320 (REQ-NUM-022), REQ-324 (REQ-NUM-026): the MIP writer gives page
/// mFD the sub-code nnXs with S1 = 0 and the update indicator, each page
/// its code, and each counted code its thirteen-bit count; the engine's
/// reader reads them all back.
#[test]
fn a_magazine_inventory_page_is_written_and_read_back() {
    use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementStore, EnhancementSubPage, StoredPage};
    use neoteletext_core::config::Config;
    use neoteletext_core::packet::{PacketMeta, PageCode};
    use neoteletext_core::types::{Instant, PageAddress, PageNumber};
    let magazine = Magazine::new(2).unwrap();
    let mut mip = Mip::new(magazine);
    mip.update_indicator(3);
    let pages = [(0x23, 0x79, None), (0x40, 0x50, Some(300)), (0x41, 0x51, Some(5000)), (0x42, 0x0A, None)];
    for (page, code, count) in pages {
        mip.page(PageNumber(page), PageCode(code), count).unwrap();
    }
    assert!(mip.page(PageNumber(0x43), PageCode(0x50), Some(5000)).is_err(), "50 counts below 2^12");
    assert!(mip.page(PageNumber(0x43), PageCode(0x01), Some(1)).is_err(), "01 carries no count");
    let page = mip.build().unwrap();
    assert_eq!(page.address, PageAddress::from_mpp(0x2FD).unwrap());
    let subcode = page.subcode.value();
    assert_eq!(neoteletext_core::limits::enhancement_sub_page(subcode), 0);
    assert_eq!(neoteletext_core::limits::enhancement_update(subcode), 3);
    let mut slots = std::vec![StoredPage::EMPTY; 4];
    let mut sub_pages = std::vec![EnhancementSubPage::EMPTY; 8];
    let mut packets = std::vec![EnhancementPacket::EMPTY; 100];
    let store = EnhancementStore::new(&mut sub_pages, &mut packets);
    let mut decoder = Decoder::with_enhancement_store(&mut slots, store, Config::default());
    let mut stream = packets_of(&page);
    stream.push(
        encode::header(magazine, PageNumber(0xFF), SubCode::new(0), ControlBits::default(), &[0x20; 32]).unwrap(),
    );
    for (index, packet) in stream.iter().enumerate() {
        decoder.push(packet, &PacketMeta::default(), Instant(index as u64 * 1000), &mut |_| {});
    }
    for (page, code, count) in pages {
        let address = PageAddress { magazine, page: PageNumber(page) };
        assert_eq!(decoder.inventory_code(address), Some(PageCode(code)), "{page:02X}");
        let expected = count.or_else(|| PageCode(code).sub_pages_in_code());
        assert_eq!(decoder.inventory_sub_pages(address), expected, "{page:02X}");
    }
}

/// REQ-320 (REQ-NUM-022), EN 300 706 Annex B.6, Table B.1: the update
/// indicator S2 of an enhancement page goes from N to N + 1 at each change,
/// modulo 8 since S2 has three bits; the writers carry it so for the MOT
/// and the MIP alike.
#[test]
fn the_update_indicator_counts_modulo_8() {
    use neoteletext_core::limits::enhancement_update;
    let magazine = Magazine::new(1).unwrap();
    for change in 0..20u8 {
        let mut mot = Mot::new(magazine);
        mot.update_indicator(change);
        let mut mip = Mip::new(magazine);
        mip.update_indicator(change);
        let expected = change % 8;
        assert_eq!(enhancement_update(mot.build().unwrap().subcode.value()), expected, "MOT {change}");
        assert_eq!(enhancement_update(mip.build().unwrap().subcode.value()), expected, "MIP {change}");
    }
}
