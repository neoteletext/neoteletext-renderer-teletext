//! Tests of the diacritical marks of G2 column 4 and their combining forms.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use crate::charset::diacritics::diacritic_combining;

#[test]
fn diacritic_combining_marks() {
    let expected = [
        0x0300, 0x0301, 0x0302, 0x0303, 0x0304, 0x0306, 0x0307, 0x0308, 0x0323, 0x030A,
        0x0327, 0x0332, 0x030B, 0x0328, 0x030C, 0x0309,
    ];
    assert_eq!(diacritic_combining(0), None);
    for (mark, scalar) in (1..=16).zip(expected) {
        assert_eq!(diacritic_combining(mark), char::from_u32(scalar), "mark {mark}");
    }
    for mark in 17..=0xFF {
        assert_eq!(diacritic_combining(mark), None);
    }
}
