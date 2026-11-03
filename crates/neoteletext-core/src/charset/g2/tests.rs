//! Tests of the G2 sets (Tables 37 to 41).
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
use crate::charset::test_tables::*;

#[test]
fn latin_g2_rows() {
    let set = CharSet::G2(G2Set::Latin);
    assert_eq!(row(set, 0x20), " ¡¢£$¥#§¤‘“«←↑→↓");
    assert_eq!(row(set, 0x30), "°±²³×µ¶·÷’”»¼½¾¿");
    assert_eq!(row(set, 0x50), "―¹®©™♪€‰α   ⅛⅜⅝⅞");
    assert_eq!(row(set, 0x60), "ΩÆĐªĦ ĲĿŁØŒºÞŦŊŉ");
    assert_eq!(row(set, 0x70), "ĸæđðħıĳŀłøœßþŧŋ■");
    // REQ-CHR-006: undefined positions are spaces.
    for code in [0x59, 0x5A, 0x5B, 0x65] {
        assert_eq!(base_unicode(set, code), ' ');
    }
}

#[test]
fn g2_column_4_is_the_diacritic_column() {
    let spacing = " `´ˆ˜¯˘˙¨.˚¸_˝˛ˇ";
    for set in [G2Set::Latin, G2Set::Cyrillic, G2Set::Greek] {
        assert_eq!(row(CharSet::G2(set), 0x40), spacing, "{set:?}");
    }
    assert_eq!(row(CharSet::G2(G2Set::Arabic), 0x40), "àABCDEFGHIJKLMNO");
}

#[test]
fn cyrillic_g2_rows() {
    let set = CharSet::G2(G2Set::Cyrillic);
    assert_eq!(base_unicode(set, 0x26), ' ');
    assert_eq!(base_unicode(set, 0x28), ' ');
    assert_eq!(row(set, 0x50), "―¹®©™♪€‰αŁłß⅛⅜⅝⅞");
    assert_eq!(row(set, 0x60), "DEFGIJKLNQRSUVWZ");
    assert_eq!(row(set, 0x70), "defgijklnqrsuvwz");
}
