//! The reordering tests of REQ-FSM-009 (REQ-141): pages from the encoder at
//! Levels 1.5, 2.5 and 3.5, object pages and the Magazine Organization
//! Table among them, and pages of the real-broadcast corpus (REQ-TST-022),
//! fed in each non-standard order of REQ-FSM-008 (REQ-140) that
//! `common::reorder` generates, give the same stored page and the same cell
//! grid as standard order.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

mod common;

use common::reorder::{interleaved, page_orders, service_orders, with_gaps};
use common::{
    DISPLAY, Mot, ObjectDef, ObjectLayout, Receiver, background, colours, display_page, end, foreground, g0, invoke,
    link, local_data, mag, mot_packets, null_link, object_link, object_page, page_grid, public_number, set_position,
    worst_case_page, x26, x27, x28,
};
use neoteletext_core::acq::StoredPage;
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{DATA_BYTES, LINK_FUNCTION_POP, PACKET_BYTES};
use neoteletext_core::ecc::{hamming84, with_odd_parity};
use neoteletext_core::level1::Grid;
use neoteletext_core::packet::{
    DefaultObject, MotPageEntry, ObjectKind, ObjectSource, Packet, TripletFunction, decode_address, encode,
};
use neoteletext_core::types::{ControlBits, FallbackFields, Instant, PageAddress, SubCode};

#[path = "../../../tests/corpus_files.rs"]
mod corpus_files;

/// The Public Object Page of the Level 2.5 and 3.5 services.
const POP: u16 = 0x1A0;
/// The other magazine's page of the interleaving test.
const OTHER: u16 = 0x200;

/// The stored page `mpp` and its grid at `config`'s level after `packets`.
/// The times of its first and last packet are left out: they are where the
/// page's packets fell in the stream, which another order moves, and not
/// what was received.
fn received(packets: &[Packet], config: Config, mpp: u16) -> (StoredPage, Box<Grid>) {
    Receiver::new().run(packets, config, |decoder| {
        let address = PageAddress::from_mpp(mpp).unwrap();
        let mut page = *decoder.store().get(address, SubCode::ANY).expect("the page is stored");
        page.first_received = Instant(0);
        page.last_received = Instant(0);
        (page, page_grid(decoder, mpp, &config).0)
    })
}

/// Checks that each of `orders` gives page `mpp` as `standard` does.
#[track_caller]
fn every_order_gives_the_same_page(standard: &[Packet], orders: &[(String, Vec<Packet>)], level: PresentationLevel) {
    let config = Config { level, ..Config::default() };
    let expected = received(standard, config, DISPLAY);
    assert_ne!(orders.len(), 0, "orders to feed");
    for (name, order) in orders {
        let mut sorted_order = order.clone();
        let mut sorted_standard = standard.to_vec();
        sorted_order.sort_by_key(|packet| packet.bytes);
        sorted_standard.sort_by_key(|packet| packet.bytes);
        assert_eq!(sorted_order, sorted_standard, "{name}: the same packets");
        let got = received(order, config, DISPLAY);
        assert_eq!(got.0, expected.0, "{name}: the stored page at {level:?}");
        assert_eq!(got.1, expected.1, "{name}: the grid at {level:?}");
    }
}

/// Page 100 at Level 1.5: rows, page enhancement in X/28/0, and local
/// enhancement data over two packets X/26 that place G0 characters with
/// diacritical marks, a G2 and a G3 character and move between rows.
fn level_15_page() -> Vec<Packet> {
    let diacritic = |column, code, mark| TripletFunction::G0Diacritic { column, code, diacritic: mark };
    let mut local = vec![set_position(1, 0)];
    local.extend((0..8).map(|index| diacritic(index * 2, b'a' + index, index + 1)));
    local.push(set_position(3, 0));
    local.extend([
        TripletFunction::G2Character { column: 4, code: 0x41 },
        TripletFunction::G3Character { column: 6, code: 0x51, level_15_form: true },
    ]);
    local.push(set_position(20, 0));
    local.extend((0..6).map(|index| diacritic(10 + index, b'A' + index, 2)));
    let others = [vec![x28(0, &colours(0, 0, false, 0))], local_data(&local)].concat();
    assert!(others.len() >= 3, "two packets X/26 and X/28");
    display_page(
        0,
        &[(1, b"LEVEL ONE POINT FIVE"), (3, b"\x11\x7f\x7f MOSAIC"), (20, b"ROW TWENTY"), (23, b"END")],
        others,
    )
}

/// Page 100 at Levels 2.5 and 3.5, with its Public Object Page and the
/// Magazine Organization Table: colours in X/28/0, links in X/27/4, local
/// colour and character triplets, a Passive object of the POP invoked at
/// row 2, and the POP's Active object as the page's default object.
fn enhanced_service(level_35: bool) -> Vec<Vec<Packet>> {
    let local = [
        set_position(2, 0),
        invoke(ObjectSource::Public, ObjectKind::Passive, public_number(0, 0)),
        set_position(5, 0),
        foreground(0, 3),
        background(1, 4),
        g0(2, b'X'),
        end(),
    ];
    let links = x27(
        4,
        [
            null_link(0),
            link(LINK_FUNCTION_POP, 0b11, POP),
            null_link(0b10),
            null_link(0b11),
            null_link(0),
            null_link(0),
        ],
    );
    let page = display_page(
        0,
        &[(2, b"HELLO"), (5, b"COLOURS"), (9, b"\x12\x7f\x13\x7f")],
        vec![x28(0, &colours(1, 2, true, 0)), links, x26(0, &local)],
    );
    let objects = object_page(
        POP,
        0,
        &[
            ObjectDef::new(ObjectKind::Passive, 0, vec![g0(0, b'a')]),
            ObjectDef::new(ObjectKind::Active, 0, vec![set_position(7, 0), g0(3, b'd')]),
        ],
    );
    let mut pops = [None; 7];
    let default = DefaultObject { kind: ObjectKind::Active, number: 0 };
    pops[0] = Some(object_link(POP, FallbackFields::default(), [Some(default), None]));
    let table = Mot {
        entries: vec![(0x00, MotPageEntry { global_objects: false, pop_link: 1 })],
        pops,
        level_25: true,
        level_35,
        ..Mot::default()
    };
    vec![page, objects, mot_packets(&table)]
}

/// REQ-141, REQ-140: a Level 1.5 page in every order of the generator gives
/// the page and grid of standard order, at Level 1.5 and at Level 1.
#[test]
fn every_order_of_a_level_15_page() {
    let page = level_15_page();
    for level in [PresentationLevel::Level15, PresentationLevel::Level1] {
        every_order_gives_the_same_page(&page, &page_orders(&page), level);
    }
}

/// REQ-141, REQ-140: designation codes with gaps (0, 2, 4 …) in any order
/// give the page that the same gapped codes give in standard order.
#[test]
fn designation_codes_with_gaps_in_every_order() {
    let gapped = with_gaps(&level_15_page()).expect("few enough packets X/26 to space out");
    assert_ne!(gapped, level_15_page());
    every_order_gives_the_same_page(&gapped, &page_orders(&gapped), PresentationLevel::Level15);
}

/// The page 200 of magazine 2: a header and three rows.
fn other_magazine_page() -> Vec<Packet> {
    let address = PageAddress::from_mpp(OTHER).unwrap();
    let header =
        encode::header(address.magazine, address.page, SubCode::new(0), ControlBits::default(), &[0x20; 32]).unwrap();
    let row = |number: u8, text: &[u8]| {
        let mut bytes = [with_odd_parity(b' '); DATA_BYTES];
        for (slot, &code) in bytes.iter_mut().zip(text) {
            *slot = with_odd_parity(code);
        }
        encode::row(mag(2), number, &bytes)
    };
    vec![header, row(1, b"MAGAZINE TWO"), row(4, b"OTHER PAGE"), row(22, b"BOTTOM")]
}

/// REQ-141, REQ-140: two magazines in parallel mode interleaved packet by
/// packet, each page in each of its orders, give both pages as standard
/// order does, one magazine after the other.
#[test]
fn interleaved_magazines_in_every_order() {
    let (first, second) = (level_15_page(), other_magazine_page());
    let standard = [first.clone(), second.clone()].concat();
    let config = Config { level: PresentationLevel::Level15, ..Config::default() };
    let expected = (received(&standard, config, DISPLAY), received(&standard, config, OTHER));
    let (first_orders, second_orders) = (page_orders(&first), page_orders(&second));
    for (index, (name, order)) in first_orders.iter().enumerate() {
        let (other_name, other_order) = &second_orders[index % second_orders.len()];
        for stream in [interleaved(order, other_order), interleaved(other_order, order)] {
            let got = (received(&stream, config, DISPLAY), received(&stream, config, OTHER));
            assert_eq!(got.0, expected.0, "page 100 {name} with page 200 {other_name}");
            assert_eq!(got.1, expected.1, "page 200 {other_name} with page 100 {name}");
        }
    }
}

/// REQ-141, REQ-140: a Level 2.5 and a Level 3.5 page, its Public Object
/// Page and the Magazine Organization Table, in every order of the pages
/// and each page in its own orders, give the grid and page of standard
/// order.
#[test]
fn every_order_of_an_enhanced_service() {
    for (level, level_35) in [(PresentationLevel::Level25, false), (PresentationLevel::Level35, true)] {
        let pages = enhanced_service(level_35);
        let standard = pages.concat();
        let orders = service_orders(&pages);
        every_order_gives_the_same_page(&standard, &orders, level);
    }
}

/// REQ-141, REQ-140: the worst-case Level 3.5 page of REQ-OBJ (sixteen
/// packets X/26 invoking nested objects of a Global Object Page), with its
/// GOP, in both orders of the two pages and each page in each named order.
/// The shuffles are left out: each build of this page is the slowest there
/// is, by design, and the named orders already move every packet 26 to 28.
/// Its objects share one sub-page of the GOP (`ObjectLayout::Shared`): the
/// layout of REQ-TST-019's bound, an object page sub-page for each, reads
/// three times as many triplets and would make this test take minutes.
#[test]
fn every_named_order_of_the_worst_case_page() {
    let all = worst_case_page(ObjectLayout::Shared);
    let second_header = all.iter().skip(1).position(|packet| decode_address(packet).unwrap().number == 0).unwrap() + 1;
    let pages = vec![all[..second_header].to_vec(), all[second_header..].to_vec()];
    let named: Vec<_> = service_orders(&pages).into_iter().filter(|(name, _)| !name.contains("shuffle")).collect();
    every_order_gives_the_same_page(&all, &named, PresentationLevel::Level35);
}

/// The first whole transmission of page 100 in a capture: its header and
/// the packets 1 to 28 of magazine 1 up to the next header that ends it,
/// which in parallel mode is the next of magazine 1 and in serial mode any
/// (EN 300 706 clause 7.2.1).
fn first_transmission_of_page_100(packets: &[Packet]) -> Vec<Packet> {
    // Header data byte 0 is the page units, 1 the tens, 7 C11 to C14.
    const SERIAL_BYTE: usize = 7;
    let header_of = |packet: &Packet| -> Option<(u8, u8, bool)> {
        let address = decode_address(packet).ok()?;
        (address.number == 0).then_some(())?;
        let data = packet.data();
        let page = hamming84(data[1])? << 4 | hamming84(data[0])?;
        Some((address.magazine.number(), page, hamming84(data[SERIAL_BYTE])? & 1 == 1))
    };
    let start = packets.iter().position(|packet| header_of(packet).is_some_and(|(m, p, _)| m == 1 && p == 0)).unwrap();
    let serial = header_of(&packets[start]).unwrap().2;
    let mut out = vec![packets[start]];
    for packet in &packets[start + 1..] {
        if let Some((magazine, _, _)) = header_of(packet)
            && (serial || magazine == 1)
        {
            break;
        }
        if let Ok(address) = decode_address(packet)
            && address.magazine.number() == 1
            && (1..=28).contains(&address.number)
        {
            out.push(*packet);
        }
    }
    out
}

/// REQ-141, REQ-TST-022: page 100 of a real broadcast, in every order of
/// the generator, gives the page and grid of the order it was sent in.
#[test]
#[ignore = "needs Dan Farrimond's recovery dan/bbc1_1991-10-31_Dan/bbc1_1991-10-31_v2_squashed.t42 in $NEOTELETEXT_CORPUS"]
fn every_order_of_a_broadcast_page() {
    let bytes = corpus_files::read("dan/bbc1_1991-10-31_Dan/bbc1_1991-10-31_v2_squashed.t42");
    let packets: Vec<Packet> =
        bytes.as_chunks::<PACKET_BYTES>().0.iter().map(|chunk| Packet { bytes: *chunk }).collect();
    let page = first_transmission_of_page_100(&packets);
    assert!(page.len() > 10, "a broadcast page 100 has rows");
    for level in [PresentationLevel::Level1, PresentationLevel::Level15, PresentationLevel::Level25] {
        every_order_gives_the_same_page(&page, &page_orders(&page), level);
    }
}
