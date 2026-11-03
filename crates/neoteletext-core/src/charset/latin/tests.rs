//! Tests of Latin G0 and its national option sub-sets (Tables 35 and 36).
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
use crate::consts::{SPACE, NATIONAL_POSITIONS};

/// Each option's 13 characters, transcribed again as one string in the
/// position order 2/3 2/4 4/0 5/B 5/C 5/D 5/E 5/F 6/0 7/B 7/C 7/D 7/E.
const TABLE_36: [(NationalOption, &str); 13] = [
    (NationalOption::CzechSlovak, "#ůčťžýířéáěúš"),
    (NationalOption::English, "£$@←½→↑#—¼‖¾÷"),
    (NationalOption::Estonian, "#õŠÄÖŽÜÕšäöžü"),
    (NationalOption::French, "éïàëêùî#èâôûç"),
    (NationalOption::German, "#$§ÄÖÜ^_°äöüß"),
    (NationalOption::Italian, "£$é°ç→↑#ùàòèì"),
    (NationalOption::LettishLithuanian, "#$ŠėęŽčūšąųžį"),
    (NationalOption::Polish, "#ńąƵŚŁćóężśłź"),
    (NationalOption::PortugueseSpanish, "ç$¡áéíóú¿üñèà"),
    (NationalOption::Rumanian, "#¤ŢÂŞĂÎıţâşăî"),
    (NationalOption::SerbianCroatianSlovenian, "#ËČĆŽĐŠëčćžđš"),
    (NationalOption::SwedishFinnishHungarian, "#¤ÉÄÖÅÜ_éäöåü"),
    (NationalOption::Turkish, "₺ğİŞÖÇÜĞışöçü"),
];

/// The 13 positions of Table 36 are exactly the codes where some
/// national option differs from another; the parity bit is ignored.
#[test]
fn national_positions_are_where_the_options_differ() {
    let mut count = 0;
    for code in SPACE..=CODE_MASK_7BIT {
        let first = to_unicode(CharRef::new(CharSet::G0(latin_g0(NationalOption::ALL[0])), code)).base;
        let varies = NationalOption::ALL
            .iter()
            .any(|&option| to_unicode(CharRef::new(CharSet::G0(latin_g0(option)), code)).base != first);
        assert_eq!(is_national_position(code), varies, "code {code:#04x}");
        assert_eq!(is_national_position(code | !CODE_MASK_7BIT), varies, "code {code:#04x} with bit 8 set");
        count += usize::from(varies);
    }
    assert_eq!(count, 13);
}

/// REQ-SVC-005: 2/3 and 2/4 are Latin G0's "#" and currency sign, not
/// a national option's characters, and control codes show as spaces.
#[test]
fn latin_text_has_no_national_option() {
    let text: [char; 8] = latin_text(b"BBC1 #$\x07");
    assert_eq!(text, ['B', 'B', 'C', '1', ' ', '#', '\u{A4}', ' ']);
}

/// REQ-71: the 13 national option subsets of Table 36.
#[test]
fn every_national_option_matches_table_36() {
    assert_eq!(TABLE_36.len(), NationalOption::ALL.len());
    for (option, expected) in TABLE_36 {
        let actual: std::string::String = NATIONAL_POSITIONS
            .iter()
            .map(|&code| base_unicode(latin_set(Some(option)), code))
            .collect();
        assert_eq!(actual, expected, "{option:?}");
    }
}

/// REQ-71: the four glyphs the standard draws too small to identify, and the Turkish lira sign.
#[test]
fn project_decided_national_glyphs() {
    let english = latin_set(Some(NationalOption::English));
    let polish = latin_set(Some(NationalOption::Polish));
    assert_eq!(base_unicode(polish, 0x5B), '\u{01B5}');
    assert_eq!(base_unicode(polish, 0x7D), '\u{0142}');
    assert_eq!(base_unicode(english, 0x60), '\u{2014}');
    assert_eq!(base_unicode(english, 0x7C), '\u{2016}');
    assert_eq!(base_unicode(latin_set(Some(NationalOption::Turkish)), 0x23), '\u{20BA}');
}

/// REQ-71: a national option changes only its 13 positions of Table 35.
#[test]
fn national_option_leaves_other_positions_alone() {
    for option in NationalOption::ALL {
        for code in ALL_CODES.filter(|code| !NATIONAL_POSITIONS.contains(code)) {
            assert_eq!(
                base_unicode(latin_set(Some(option)), code),
                base_unicode(latin_set(None), code),
                "{option:?} {code:#04x}"
            );
        }
    }
}

/// REQ-71: the base forms of Table 35, used when packet X/26 refers to a Latin G0 character.
#[test]
fn latin_g0_base_forms() {
    let base: std::string::String =
        NATIONAL_POSITIONS.iter().map(|&code| base_unicode(latin_set(None), code)).collect();
    assert_eq!(base, "#¤@[\\]^_`{|}~");
    assert_eq!(row(latin_set(None), 0x40), "@ABCDEFGHIJKLMNO");
    assert_eq!(row(latin_set(None), 0x70), "pqrstuvwxyz{|}~■");
    assert_eq!(base_unicode(latin_set(None), 0x20), ' ');
}
