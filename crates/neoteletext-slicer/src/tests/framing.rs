//! REQ-SLC-008 and decision E.24: framing code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::similar_names
)]

use super::*;

#[test]
fn every_single_bit_framing_error() {
    for bit in 0..BITS_PER_BYTE {
        let mut bytes = line_bytes(&known_packet());
        bytes[FRAMING_INDEX] ^= 1 << bit;
        let samples = line_with(&bytes, &nominal(), RATE);
        let mut slicer = expecting(Config::new(), RATE);
        let sliced = slice_expected(&mut slicer, &samples, RATE).unwrap_or_else(|e| panic!("bit {bit}: {e:?}"));
        assert_eq!(sliced.packet, known_packet(), "bit {bit}");
        assert!(sliced.meta.quality.unwrap().framing_corrected, "bit {bit}");
        // The strict option refuses it.
        let mut strict = expecting(Config { framing_tolerance: false, ..Config::new() }, RATE);
        assert_eq!(slice_expected(&mut strict, &samples, RATE), Err(Rejection::NoFramingCode), "bit {bit}");
    }
}

#[test]
fn corrected_framing_needs_a_valid_address() {
    let mut bytes = line_bytes(&known_packet());
    bytes[FRAMING_INDEX] ^= 0x10;
    bytes[ADDRESS_INDEX] ^= 0x0C;
    let samples = line_with(&bytes, &nominal(), RATE);
    let mut slicer = expecting(Config::new(), RATE);
    assert_eq!(slice_expected(&mut slicer, &samples, RATE), Err(Rejection::BadAddress));
}

/// The line with framing code bit 3 wrong.
fn one_bit_wrong() -> Vec<u8> {
    let mut bytes = line_bytes(&known_packet());
    bytes[FRAMING_INDEX] ^= 0x08;
    line_with(&bytes, &nominal(), RATE)
}

/// REQ-447, DEC-1092: on a line where no teletext was seen, or too little
/// (fewer exact packets than `EXPECTED_LINE_EXACT_PACKETS`), a framing
/// code with one wrong bit is refused, and so is a line the slicer cannot
/// place.
#[test]
fn one_wrong_framing_bit_is_refused_where_no_teletext_was_seen() {
    let samples = one_bit_wrong();
    let mut fresh = Slicer::new(Config::new());
    assert_eq!(fresh.slice(&placed(&samples, RATE, TELETEXT_LINE)), Err(Rejection::NoFramingCode));
    let exact = line_with(&line_bytes(&known_packet()), &nominal(), RATE);
    for _ in 1..crate::consts::EXPECTED_LINE_EXACT_PACKETS {
        assert!(fresh.slice(&placed(&exact, RATE, TELETEXT_LINE)).is_ok());
    }
    assert_eq!(fresh.slice(&placed(&samples, RATE, TELETEXT_LINE)), Err(Rejection::NoFramingCode), "one too few");
    let mut slicer = expecting(Config::new(), RATE);
    assert_eq!(slicer.slice(&placed(&samples, RATE, TELETEXT_LINE + 1)), Err(Rejection::NoFramingCode), "another line");
    let unplaced = Line::new(&samples, SampleFormat::U8, RATE);
    assert_eq!(slicer.slice(&unplaced), Err(Rejection::NoFramingCode), "a line of unknown place");
    let second = Line { second_field: Some(true), ..placed(&samples, RATE, TELETEXT_LINE) };
    assert_eq!(slicer.slice(&second), Err(Rejection::NoFramingCode), "the same place in the other field");
}

/// REQ-447, DEC-1092: after a packet with an exact framing code on a line,
/// one wrong bit is accepted on that line for `EXPECTED_LINE_FIELDS`
/// fields, and refused after; a packet accepted through the tolerance does
/// not renew the expectation, and a line known by its number is tracked by
/// it.
#[test]
fn one_wrong_framing_bit_is_accepted_only_while_the_line_carries_teletext() {
    let wrong = one_bit_wrong();
    let empty = line_with(&[0; 45], &nominal(), RATE);
    let mut slicer = expecting(Config::new(), RATE);
    assert!(slice_expected(&mut slicer, &wrong, RATE).is_ok(), "the field after");
    // Fields with nothing on the line: the corrected packet above does not
    // renew the expectation, so it ends EXPECTED_LINE_FIELDS fields after
    // the exact packet.
    for _ in 2..crate::consts::EXPECTED_LINE_FIELDS {
        assert!(slice_expected(&mut slicer, &empty, RATE).is_err());
    }
    assert!(slice_expected(&mut slicer, &wrong, RATE).is_ok(), "the last field it is expected");
    assert_eq!(slice_expected(&mut slicer, &wrong, RATE), Err(Rejection::NoFramingCode), "a field later");
    // Known by number: line 335 is in the second field.
    let exact = line_with(&line_bytes(&known_packet()), &nominal(), RATE);
    let numbered = |samples| Line { line: Some(335), ..Line::new(samples, SampleFormat::U8, RATE) };
    assert_eq!(slicer.slice(&numbered(&wrong)), Err(Rejection::NoFramingCode));
    for _ in 0..crate::consts::EXPECTED_LINE_EXACT_PACKETS {
        assert!(slicer.slice(&numbered(&exact)).is_ok());
    }
    assert!(slicer.slice(&numbered(&wrong)).is_ok());
}
