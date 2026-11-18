#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::event::{EVENT, Event, Item, STATE_NONE, STATE_OFFER, STATE_PROOF, read, units, write};
use std::vec::Vec;

/// A public key and a signature of recognisable bytes.
const KEY: [u8; PUBLIC_KEY_BYTES] = [0xA5; PUBLIC_KEY_BYTES];
const SIGNATURE: [u8; SIGNATURE_BYTES] = [0x5A; SIGNATURE_BYTES];

fn encoded(event: Event<'_>) -> Vec<u8> {
    let mut out = [0_u8; 256];
    let length = write(&Item::Event(event), &mut out).unwrap();
    out[..length].to_vec()
}

/// VS-204: `STATE_PROOF` is `0xFF`, its type, the length 98 and the payload
/// of algorithm, lifetime, public key and signature, and reads back.
#[test]
fn a_state_proof_is_its_98_bytes_and_reads_back() {
    let proof = StateProof {
        algorithm: Algorithm::Ed25519,
        lifetime: Lifetime::Remembered,
        public_key: &KEY,
        signature: &SIGNATURE,
    };
    let bytes = encoded(Event::StateProof(proof));
    let mut expected = std::vec![EVENT, STATE_PROOF, 98, 1, 1];
    expected.extend_from_slice(&KEY);
    expected.extend_from_slice(&SIGNATURE);
    assert_eq!(bytes, expected);
    let (item, used) = read(&bytes).unwrap().unwrap();
    assert_eq!((item, used), (Item::Event(Event::StateProof(proof)), bytes.len()));
}

/// VS-204: `STATE_NONE` carries each reason as its byte, and `STATE_OFFER`
/// nothing.
#[test]
fn state_none_and_state_offer_are_their_bytes() {
    let reasons =
        [NoneReason::NoKey, NoneReason::Declined, NoneReason::Stateless, NoneReason::Forgotten, NoneReason::NoGesture];
    for (code, reason) in (0_u8..).zip(reasons) {
        let bytes = encoded(Event::StateNone(reason));
        assert_eq!(bytes, [EVENT, STATE_NONE, 1, code]);
        assert_eq!(read(&bytes).unwrap().unwrap().0, Item::Event(Event::StateNone(reason)));
    }
    let bytes = encoded(Event::StateOffer);
    assert_eq!(bytes, [EVENT, STATE_OFFER, 0]);
    assert_eq!(read(&bytes).unwrap().unwrap().0, Item::Event(Event::StateOffer));
}

/// VS-204: a message of any other length, or with a value not listed, is a
/// protocol error.
#[test]
fn a_state_message_of_another_length_or_value_is_refused() {
    let mut proof = std::vec![EVENT, STATE_PROOF, 98, 1, 0];
    proof.extend_from_slice(&KEY);
    proof.extend_from_slice(&SIGNATURE);
    assert!(read(&proof).is_ok(), "the good one");
    let mut short = proof.clone();
    short[2] = 97;
    short.pop();
    let mut long = proof.clone();
    long[2] = 99;
    long.push(0);
    let mut algorithm = proof.clone();
    algorithm[3] = 2;
    let mut zero_algorithm = proof.clone();
    zero_algorithm[3] = 0;
    let mut lifetime = proof;
    lifetime[4] = 2;
    for bad in [short, long, algorithm, zero_algorithm, lifetime] {
        assert!(read(&bad).is_err(), "{bad:02x?}");
    }
    for bad in [
        std::vec![EVENT, STATE_NONE, 1, 5],
        std::vec![EVENT, STATE_NONE, 1, 0xFF],
        std::vec![EVENT, STATE_NONE, 0],
        std::vec![EVENT, STATE_NONE, 2, 0, 0],
        std::vec![EVENT, STATE_OFFER, 1, 0],
    ] {
        assert!(read(&bad).is_err(), "{bad:02x?}");
    }
}

/// VS-204, VS-212: viewer state's messages are no input, and count
/// nothing.
#[test]
fn state_messages_count_nothing() {
    let proof = StateProof {
        algorithm: Algorithm::Ed25519,
        lifetime: Lifetime::Visit,
        public_key: &KEY,
        signature: &SIGNATURE,
    };
    for event in [Event::StateProof(proof), Event::StateNone(NoneReason::Stateless), Event::StateOffer] {
        assert_eq!(units(&Item::Event(event)), 0, "{event:?}");
    }
}

/// Each code reads back to its value, and every other byte to none.
#[test]
fn every_code_reads_back_and_no_other() {
    for code in 0..=u8::MAX {
        assert_eq!(Algorithm::from_code(code).map(Algorithm::code), (code == 1).then_some(code));
        assert_eq!(Lifetime::from_code(code).map(Lifetime::code), (code <= 1).then_some(code));
        assert_eq!(NoneReason::from_code(code).map(NoneReason::code), (code <= 4).then_some(code));
    }
}
