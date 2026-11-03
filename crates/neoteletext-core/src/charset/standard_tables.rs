//! REQ-464 (REQ-TST-011): the standard's G0 and G2 sets, transcribed once
//! from the tables of EN 300 706 V1.2.1 (clause 15.6, pages 106 to 117 of
//! the ETSI edition), position by position, and compared whole with the
//! core's mapping. The transcription was made from the standard's pages,
//! not from the code, and every position must agree. When this module was
//! written, on 4 November 2026, no position differed; a difference found
//! later is fixed in the core, or, if the core is right, explained here
//! beside the position.
//!
//! Tables 36 (the national options), 47 (G1) and 48 (G3) are compared
//! whole by `every_national_option_matches_table_36` in `latin/tests.rs`,
//! the mosaic tests of `mosaics/tests.rs` and `g3_positions` in
//! `g3/tests.rs`. Tables 44 and 45, the Arabic sets, are compared here
//! only where their positions are Latin or punctuation: their Arabic
//! letters are drawn as presentation forms the table's glyphs do not name
//! one by one, and are checked by `arabic_spot_checks` in
//! `g0_standard/tests.rs`.
//!
//! How a position is written: 2/0 is the space of each table's note 1; 7/F,
//! the rectangle of the background colour notes call it, is `■`, as the
//! core shows it; a position the table leaves empty, or says should not be
//! transmitted, is a space (REQ-CHR-006). The marks of column 4 of the G2
//! sets are written as their spacing forms; 4/9, the dot below, has no
//! spacing character in Unicode, and a full stop stands for it.
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test indexes the 96 positions of a table it built to that size, and adds small column and row numbers it chose, where an overflow would also fail it"
)]

extern crate std;

use std::format;
use std::string::String;
use std::vec::Vec;

use crate::charset::code_table::base_unicode;
use crate::charset::{CharSet, G0, G0Set, G2Set};

/// One table: columns 2 to 7, each of rows 0 to F.
type Table = [&'static str; 6];

/// The first code of the tables, column 2 row 0.
const FIRST: u8 = 0x20;
/// Positions in a column.
const ROWS: u8 = 16;

/// Table 35, the Latin G0 primary set, with the shaded positions (2/3, 2/4,
/// 4/0, 5/B to 5/F, 6/0, 7/B to 7/E) as a set accessed without a national
/// option shows them (note 2).
const TABLE_35: Table = [
    " !\"#¤%&'()*+,-./",
    "0123456789:;<=>?",
    "@ABCDEFGHIJKLMNO",
    "PQRSTUVWXYZ[\\]^_",
    "`abcdefghijklmno",
    "pqrstuvwxyz{|}~■",
];

/// Column 4 of the G2 sets: the marks, as spacing forms.
const MARKS: &str = " `´ˆ˜¯˘˙¨.˚¸_˝˛ˇ";

/// Table 37, the Latin G2 supplementary set. 5/9 to 5/B and 6/5 should not
/// be transmitted (note 5); 5/6 is the euro sign (note 4).
const TABLE_37: Table =
    [" ¡¢£$¥#§¤‘“«←↑→↓", "°±²³×µ¶·÷’”»¼½¾¿", MARKS, "―¹®©™♪€‰α   ⅛⅜⅝⅞", "ΩÆĐªĦ ĲĿŁØŒºÞŦŊŉ", "ĸæđðħıĳŀłøœßþŧŋ■"];

/// Table 38, Cyrillic G0 option 1, Serbian and Croatian.
const TABLE_38: Table = [
    " !\"#$%&'()*+,-./",
    "0123456789:;<=>?",
    "ЧАБЦДЕФГХИЈКЛМНО",
    "ПЌРСТУВЃЉЊЗЋЖЂШЏ",
    "чабцдефгхијклмно",
    "пќрстувѓљњзћжђш■",
];

/// Table 39, Cyrillic G0 option 2, Russian and Bulgarian.
const TABLE_39: Table = [
    " !\"#$%ы'()*+,-./",
    "0123456789:;<=>?",
    "ЮАБЦДЕФГХИЙКЛМНО",
    "ПЯРСТУЖВЬЪЗШЭЩЧЫ",
    "юабцдефгхийклмно",
    "пярстужвьъзшэщч■",
];

/// Table 40, Cyrillic G0 option 3, Ukrainian.
const TABLE_40: Table = [
    " !\"#$%ї'()*+,-./",
    "0123456789:;<=>?",
    "ЮАБЦДЕФГХИЙКЛМНО",
    "ПЯРСТУЖВЬІЗШЄЩЧЇ",
    "юабцдефгхийклмно",
    "пярстужвьізшєщч■",
];

/// Table 41, the Cyrillic G2 supplementary set. 2/6 and 2/8 should not be
/// transmitted (note 2); 5/6 is the euro sign (note 5); 7/F is "z", not the
/// rectangle.
const TABLE_41: Table =
    [" ¡¢£$¥ § ‘“«←↑→↓", "°±²³×µ¶·÷’”»¼½¾¿", MARKS, "―¹®©™♪€‰αŁłß⅛⅜⅝⅞", "DEFGIJKLNQRSUVWZ", "defgijklnqrsuvwz"];

/// Table 42, the Greek G0 primary set. 5/2 is the tonos alone.
const TABLE_42: Table = [
    " !\"#$%&'()*+,-./",
    "0123456789:;«=»?",
    "ΐΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟ",
    "ΠΡ΄ΣΤΥΦΧΨΩΪΫάέήί",
    "ΰαβγδεζηθικλμνξο",
    "πρςστυφχψωϊϋόύώ■",
];

/// Table 43, the Greek G2 supplementary set: Latin letters the Greek G0
/// set lacks, and the capitals with tonos. 5/6 is the euro sign (note 4).
/// 3/4 is the multiplication sign, drawn as Table 37's 3/4 is; only 3/F is
/// the small letter x, drawn as the table's other small letters are.
const TABLE_43: Table =
    [" ab£ehi§:‘“k←↑→↓", "°±²³×mnp÷’”t¼½¾x", MARKS, "?¹®©™♪€‰αΊΎΏ⅛⅜⅝⅞", "CDFGJLQRSUVWYZΆΉ", "cdfgjlqrsuvwyzΈ■"];

/// Table 46, the Hebrew G0 primary set. 7/B is the shekel sign (note 4).
const TABLE_46: Table = [
    " !\"£$%&'()*+,-./",
    "0123456789:;<=>?",
    "@ABCDEFGHIJKLMNO",
    "PQRSTUVWXYZ←½→↑#",
    "אבגדהוזחטיךכלםמן",
    "נסעףפץצקרשת₪‖¾÷■",
];

/// Table 45, the Arabic G2 supplementary set, columns 4 to 7: Latin
/// letters, some accented, for French text beside Arabic. 5/F is an Arabic
/// character (see the module's notes) and 7/F is empty.
const TABLE_45_LATIN: [&str; 4] = ["àABCDEFGHIJKLMNO", "PQRSTUVWXYZëêùî", "éabcdefghijklmno", "pqrstuvwxyzâôûç "];

fn g0(set: G0Set) -> CharSet {
    CharSet::G0(G0 { set, national: None })
}

/// Compares every position of `table` in `set`, and returns the
/// differences.
fn differences(name: &str, set: CharSet, table: &Table) -> Vec<(u8, char, char)> {
    let mut found = Vec::new();
    for (column, text) in (0u8..).zip(table) {
        let expected: Vec<char> = text.chars().collect();
        assert_eq!(expected.len(), usize::from(ROWS), "{name} column {}: sixteen positions", column + 2);
        for row in 0..ROWS {
            let code = FIRST + column * ROWS + row;
            let shown = base_unicode(set, code);
            let wanted = expected[usize::from(row)];
            if shown != wanted {
                found.push((code, wanted, shown));
            }
        }
    }
    found
}

fn assert_matches(name: &str, set: CharSet, table: &Table) {
    let found = differences(name, set, table);
    let listed: Vec<String> = found
        .iter()
        .map(|&(code, wanted, shown)| format!("{:X}/{:X}: table {wanted:?}, core {shown:?}", code >> 4, code & 0xF))
        .collect();
    assert!(found.is_empty(), "{name} differs at {listed:#?}");
}

#[test]
fn table_35_latin_g0() {
    assert_matches("35", g0(G0Set::Latin), &TABLE_35);
}

#[test]
fn table_37_latin_g2() {
    assert_matches("37", CharSet::G2(G2Set::Latin), &TABLE_37);
}

#[test]
fn table_38_cyrillic_option_1() {
    assert_matches("38", g0(G0Set::Cyrillic1), &TABLE_38);
}

#[test]
fn table_39_cyrillic_option_2() {
    assert_matches("39", g0(G0Set::Cyrillic2), &TABLE_39);
}

#[test]
fn table_40_cyrillic_option_3() {
    assert_matches("40", g0(G0Set::Cyrillic3), &TABLE_40);
}

#[test]
fn table_41_cyrillic_g2() {
    assert_matches("41", CharSet::G2(G2Set::Cyrillic), &TABLE_41);
}

#[test]
fn table_42_greek_g0() {
    assert_matches("42", g0(G0Set::Greek), &TABLE_42);
}

#[test]
fn table_43_greek_g2() {
    assert_matches("43", CharSet::G2(G2Set::Greek), &TABLE_43);
}

#[test]
fn table_46_hebrew_g0() {
    assert_matches("46", g0(G0Set::Hebrew), &TABLE_46);
}

#[test]
fn table_45_arabic_g2_latin_columns() {
    let set = CharSet::G2(G2Set::Arabic);
    for (column, text) in (4u8..).zip(TABLE_45_LATIN) {
        for (row, wanted) in (0u8..).zip(text.chars()) {
            let code = column * ROWS + row;
            // 5/F is an Arabic character, outside this comparison.
            if code == 0x5F {
                continue;
            }
            assert_eq!(base_unicode(set, code), wanted, "45 {column:X}/{row:X}");
        }
    }
}
