//! Tests of the G3 set (Table 47).
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

use crate::charset::code_table::base_unicode;
use crate::charset::mosaics::FULL_BLOCK;
use crate::charset::test_tables::*;
use std::collections::BTreeSet;
use std::vec::Vec;

/// REQ-CHR-011: every G3 position against the mapping of L2/19-025 and
/// `TELTXTG3.TXT`, as `research/notes-level25-drcs.md` section 9.3
/// lists it; 5/F and the unassigned positions are spaces.
/// REQ-76, REQ-79: every G3 position's Unicode value, and the unassigned positions are spaces.
#[test]
fn g3_positions() {
    const MAPPING: [u32; 96] = [
        0x1FB3C, 0x1FB3D, 0x1FB3E, 0x1FB3F, 0x1FB40, 0x25E3, 0x1FB41, 0x1FB42, // 2/0
        0x1FB43, 0x1FB44, 0x1FB45, 0x1FB46, 0x1FB68, 0x1FB69, 0x1FB70, 0x2592, // 2/8
        0x1FB47, 0x1FB48, 0x1FB49, 0x1FB4A, 0x1FB4B, 0x25E2, 0x1FB4C, 0x1FB4D, // 3/0
        0x1FB4E, 0x1FB4F, 0x1FB50, 0x1FB51, 0x1FB6A, 0x1FB6B, 0x1FB75, 0x2588, // 3/8
        0x2537, 0x252F, 0x251D, 0x2525, 0x1FBA4, 0x1FBA5, 0x1FBA6, 0x1FBA7, // 4/0
        0x1FBA0, 0x1FBA1, 0x1FBA2, 0x1FBA3, 0x253F, 0x2022, 0x25CF, 0x25CB, // 4/8
        0x2502, 0x2500, 0x250C, 0x2510, 0x2514, 0x2518, 0x251C, 0x2524, // 5/0
        0x252C, 0x2534, 0x253C, 0x2B62, 0x2B60, 0x2B61, 0x2B63, 0x0020, // 5/8
        0x1FB52, 0x1FB53, 0x1FB54, 0x1FB55, 0x1FB56, 0x25E5, 0x1FB57, 0x1FB58, // 6/0
        0x1FB59, 0x1FB5A, 0x1FB5B, 0x1FB5C, 0x1FB6C, 0x1FB6D, 0x0020, 0x0020, // 6/8
        0x1FB5D, 0x1FB5E, 0x1FB5F, 0x1FB60, 0x1FB61, 0x25E4, 0x1FB62, 0x1FB63, // 7/0
        0x1FB64, 0x1FB65, 0x1FB66, 0x1FB67, 0x1FB6E, 0x1FB6F, 0x0020, 0x0020, // 7/8
    ];
    for (code, &expected) in ALL_CODES.zip(MAPPING.iter()) {
        assert_eq!(u32::from(base_unicode(CharSet::G3, code)), expected, "G3 {code:#04x}");
    }
    assert_eq!(base_unicode(CharSet::G3, 0x3F), FULL_BLOCK);
    // Every assigned position is distinct.
    let shapes: Vec<char> = ALL_CODES
        .map(|code| base_unicode(CharSet::G3, code))
        .filter(|&found| found != ' ')
        .collect();
    let distinct: BTreeSet<char> = shapes.iter().copied().collect();
    assert_eq!(shapes.len(), 91);
    assert_eq!(distinct.len(), shapes.len());
}
