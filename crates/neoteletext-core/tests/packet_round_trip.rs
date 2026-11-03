//! Property tests of the packet encoders (REQ-TST-001's encode-then-decode
//! round trips): every packet the encoders of `packet::encode` build decodes
//! back to the fields it was built from, for any fields the standard allows.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and `panic!` on outcomes it rules out"
)]

use neoteletext_core::consts::{
    DATA_BYTES, HEADER_DISPLAY_BYTES, LINK_NULL_PAGE, LINK_NULL_SUBCODE, PACKET_ENHANCEMENT, PACKET_ROW_25,
    TRIPLETS_PER_PACKET,
};
use neoteletext_core::limits::{NULL_ADDRESS, SUBCODE_MASK};
use neoteletext_core::packet::{Decoded, Link, Triplet, decode, decode_address, encode};
use neoteletext_core::types::{ControlBits, Magazine, PageNumber, SubCode};
use proptest::prelude::*;

/// Cases each property tries: enough to reach every field's corners within a
/// second of a debug build.
const CASES: u32 = 512;
/// The bits of the control bits C4 to C14 a header carries (clause 9.3.1.3).
const CONTROL_MASK: u16 = 0x7FF0;
/// The largest 18-bit triplet value (clause 8.3).
const TRIPLET_MAX: u32 = (1 << 18) - 1;
/// The highest packet number (clause 7.1.2).
const LAST_PACKET: u8 = 31;
/// Designation codes are four bits.
const LAST_DESIGNATION: u8 = 15;

fn magazine() -> impl Strategy<Value = Magazine> {
    (1_u8..=8).prop_map(|number| Magazine::new(number).unwrap())
}

fn subcode() -> impl Strategy<Value = u16> {
    any::<u16>().prop_map(|value| value & SUBCODE_MASK)
}

fn data<const N: usize>() -> impl Strategy<Value = [u8; N]> {
    proptest::collection::vec(any::<u8>(), N).prop_map(|bytes| bytes.try_into().unwrap())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: CASES, ..ProptestConfig::default() })]

    // REQ-460: a packet address of any magazine and number decodes back.
    #[test]
    fn an_address_round_trips(magazine in magazine(), number in 0..=LAST_PACKET) {
        let address = decode_address(&encode::address(magazine, number)).unwrap();
        prop_assert_eq!((address.magazine, address.number), (magazine, number));
    }

    // REQ-460: a header of any page, sub-code and control bits but the null
    // address decodes back field by field, its display bytes untouched.
    #[test]
    fn a_header_round_trips(
        magazine in magazine(),
        page in any::<u8>(),
        sub in subcode(),
        control in any::<u16>(),
        display in data::<HEADER_DISPLAY_BYTES>(),
    ) {
        prop_assume!((page, sub) != NULL_ADDRESS);
        let control = ControlBits(control & CONTROL_MASK);
        let packet = encode::header(magazine, PageNumber(page), SubCode::new(sub), control, &display).unwrap();
        let Ok(Decoded::Header(header)) = decode(&packet) else { panic!("not a header") };
        prop_assert_eq!(header.magazine, magazine);
        prop_assert_eq!(header.page, Some(PageNumber(page)));
        prop_assert_eq!(header.subcode, Some(SubCode::new(sub)));
        prop_assert_eq!(header.control, Some(control));
        prop_assert_eq!(header.display, display);
    }

    // REQ-460: a display row of any number and bytes decodes back.
    #[test]
    fn a_row_round_trips(magazine in magazine(), row in 1..=PACKET_ROW_25, bytes in data::<DATA_BYTES>()) {
        let Ok(Decoded::Row(decoded)) = decode(&encode::row(magazine, row, &bytes)) else { panic!("not a row") };
        prop_assert_eq!((decoded.magazine, decoded.row, decoded.bytes), (magazine, row, bytes));
    }

    // REQ-460: thirteen triplets of any 18-bit values, in an X/26 packet of
    // any designation code, decode back each one (Hamming 24/18, clause
    // 8.3).
    #[test]
    fn an_enhancement_packet_round_trips(
        magazine in magazine(),
        designation in 0..=LAST_DESIGNATION,
        values in proptest::collection::vec(0..=TRIPLET_MAX, TRIPLETS_PER_PACKET),
    ) {
        let triplets: [Triplet; TRIPLETS_PER_PACKET] =
            values.iter().map(|&value| Triplet::from_value(value)).collect::<Vec<_>>().try_into().unwrap();
        let packet = encode::triplet_packet(magazine, PACKET_ENHANCEMENT, designation, &triplets);
        let Ok(Decoded::Enhancement(enhancement)) = decode(&packet) else { panic!("not an enhancement packet") };
        prop_assert_eq!((enhancement.magazine, enhancement.designation), (magazine, designation));
        prop_assert_eq!(enhancement.triplets, triplets.map(Some));
    }

    // REQ-460: six editorial links to any magazine, page and sub-code, or
    // null links, decode back from X/27/0 with the row-24 flag (clause
    // 9.6.1).
    #[test]
    fn the_editorial_links_round_trip(
        magazine in magazine(),
        targets in proptest::collection::vec(proptest::option::of((magazine(), any::<u8>(), subcode())), 6),
        display_row_24 in any::<bool>(),
        checksum in any::<u16>(),
    ) {
        // A link to page FF sub-code 3F7F is the null link itself.
        let targets: Vec<_> = targets
            .into_iter()
            .map(|target| target.filter(|&(_, page, sub)| (page, sub) != (LINK_NULL_PAGE, LINK_NULL_SUBCODE)))
            .collect();
        let links: [Option<(Magazine, PageNumber, SubCode)>; 6] = targets
            .iter()
            .map(|target| target.map(|(to, page, sub)| (to, PageNumber(page), SubCode::new(sub))))
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let Ok(Decoded::Links(decoded)) = decode(&encode::links(magazine, &links, display_row_24, checksum)) else {
            panic!("not packet X/27/0")
        };
        let expected = links.map(|link| link.map(|(magazine, page, subcode)| Link { magazine, page, subcode }));
        prop_assert_eq!(decoded.magazine, magazine);
        prop_assert_eq!(decoded.links, expected);
        prop_assert_eq!(decoded.display_row_24, display_row_24);
    }
}
