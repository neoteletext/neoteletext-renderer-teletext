//! Tests of the G1 mosaics and their Unicode (REQ-CHR-005).
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
use crate::charset::*;

use crate::charset::mosaics::{MOSAIC_LEFT_COLUMN, MOSAIC_RIGHT_COLUMN, mosaic_unicode};
use crate::consts::{SPACE, MOSAIC_FULL};
use std::collections::BTreeSet;
use std::vec::Vec;

/// REQ-CHR-005: G1 codes through the pattern to Unicode.
/// REQ-79: a mosaic, which is no letter, converts through its pattern.
#[test]
fn mosaic_codes_to_unicode() {
    // 2/0 has no blocks; 7/F has all six.
    assert_eq!(mosaic_to_unicode(mosaic_pattern(SPACE), false), ' ');
    assert_eq!(mosaic_to_unicode(mosaic_pattern(CODE_MASK_7BIT), false), '\u{2588}');
    // 3/5: blocks 1, 3 and 5, the left column: LEFT HALF BLOCK.
    assert_eq!(mosaic_to_unicode(mosaic_pattern(b'5'), false), '\u{258C}');
    // 6/A: blocks 2, 4 and 6, the right column: RIGHT HALF BLOCK.
    assert_eq!(mosaic_to_unicode(mosaic_pattern(b'j'), false), '\u{2590}');
    // Separated mosaics are U+1CE50 plus the pattern.
    assert_eq!(mosaic_to_unicode(mosaic_pattern(b'5'), true), '\u{1CE65}');
    assert_eq!(mosaic_to_unicode(mosaic_pattern(SPACE), true), ' ');
}

#[test]
fn mosaic_fixed_points() {
    assert_eq!(MOSAIC_LEFT_COLUMN | MOSAIC_RIGHT_COLUMN, MOSAIC_FULL);
    assert_eq!(MOSAIC_LEFT_COLUMN, 21);
    assert_eq!(MOSAIC_RIGHT_COLUMN, 42);
    assert_eq!(mosaic_unicode(0, false), ' ');
    assert_eq!(mosaic_unicode(21, false), '\u{258C}');
    assert_eq!(mosaic_unicode(42, false), '\u{2590}');
    assert_eq!(mosaic_unicode(63, false), '\u{2588}');
    assert_eq!(mosaic_unicode(1, false), '\u{1FB00}');
    assert_eq!(mosaic_unicode(20, false), '\u{1FB13}');
    assert_eq!(mosaic_unicode(22, false), '\u{1FB14}');
    assert_eq!(mosaic_unicode(62, false), '\u{1FB3B}');
    assert_eq!(mosaic_unicode(0, true), ' ');
    assert_eq!(mosaic_unicode(1, true), '\u{1CE51}');
    assert_eq!(mosaic_unicode(63, true), '\u{1CE8F}');
}

#[test]
fn all_64_mosaics_contiguous() {
    let fixed = [0, 21, 42, 63];
    let sextants: Vec<u32> = (0..64_u8)
        .filter(|n| !fixed.contains(n))
        .map(|n| u32::from(mosaic_unicode(n, false)))
        .collect();
    assert_eq!(sextants.len(), 60);
    assert_eq!(sextants.first(), Some(&0x1FB00));
    assert_eq!(sextants.last(), Some(&0x1FB3B));
    // Strictly increasing by one: monotonic, distinct and gap-free.
    for pair in sextants.windows(2) {
        assert_eq!(pair[1], pair[0] + 1);
    }
    let all: BTreeSet<char> = (0..64).map(|n| mosaic_unicode(n, false)).collect();
    assert_eq!(all.len(), 64);
}

#[test]
fn all_64_mosaics_separated() {
    for n in 1..64_u8 {
        assert_eq!(u32::from(mosaic_unicode(n, true)), 0x1CE50 + u32::from(n));
    }
    let all: BTreeSet<char> = (0..64).map(|n| mosaic_unicode(n, true)).collect();
    assert_eq!(all.len(), 64);
}

#[test]
fn mosaic_pattern_ignores_high_bits() {
    for n in 0..64_u8 {
        assert_eq!(mosaic_unicode(n | 0xC0, false), mosaic_unicode(n, false));
        assert_eq!(mosaic_unicode(n | 0xC0, true), mosaic_unicode(n, true));
    }
}
