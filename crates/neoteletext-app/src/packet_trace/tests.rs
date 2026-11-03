#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "a test")]

use super::*;
use neoteletext_core::consts::{ADDRESS_BYTE_0, DATA_BYTES, TRIPLETS_PER_PACKET};
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::{Triplet, encode};
use neoteletext_core::types::{ControlBits, SubCode};

fn magazine(number: u8) -> Magazine {
    Magazine::new(number).unwrap()
}

/// REQ-87 (REQ-CLI-002): a header's line names its magazine and packet
/// number and the page, sub-code and control bits read from it; a clean
/// packet says so.
#[test]
fn a_headers_fields_are_traced() {
    let header = encode::header(
        magazine(1),
        PageNumber(0x23),
        SubCode::new(0x0001),
        ControlBits(0),
        &[with_odd_parity(b' '); 32],
    )
    .unwrap();
    assert_eq!(
        line(&header),
        "packet 1/0: header: page 123, sub-code 0001, control bits 0x0000, 0 parity errors; protection clean"
    );
}

/// REQ-87: a display row's parity failures, a damaged designation's
/// Hamming failure, and a damaged address are each marked.
#[test]
fn errors_are_marked() {
    let mut bytes = [with_odd_parity(b'A'); DATA_BYTES];
    // Even parity: 0x41 has two bits set, and 0x00 none.
    bytes[3] = b'A';
    bytes[4] = 0x00;
    let row = encode::row(magazine(2), 5, &bytes);
    assert_eq!(line(&row), "packet 2/5: display row 5: 2 parity errors; protection clean");

    let enhancement = encode::triplet_packet(magazine(1), 26, 0, &[Triplet::default(); TRIPLETS_PER_PACKET]);
    assert!(line(&enhancement).starts_with("packet 1/26/0: enhancement data"), "{}", line(&enhancement));
    let mut damaged = enhancement;
    // Two bit errors in the designation code: Hamming 8/4 cannot correct them.
    damaged.bytes[DATA_START] ^= 0b0000_0110;
    assert_eq!(
        line(&damaged),
        "packet 1/26: designation code uncorrectable; Hamming 8/4: 0 corrected, 1 uncorrectable"
    );

    let mut lost = enhancement;
    lost.bytes[ADDRESS_BYTE_0] ^= 0b0000_0110;
    assert_eq!(line(&lost), "packet ?/?: address uncorrectable; Hamming 8/4: 0 corrected, 1 uncorrectable");
}
