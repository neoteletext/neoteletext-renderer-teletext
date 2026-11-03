//! Tests of composing a character with a diacritical mark (clause 15.6.2,
//! REQ-CHR-014): the Vietnamese tones, the composition table and its marks.
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
use crate::charset::composition::{COMPOSITIONS, EXTENDED_COMPOSITIONS, search};
use crate::charset::diacritics::{COMBINING_MARKS, diacritic_combining};
use crate::charset::test_tables::*;
use crate::consts::SPACE;
use std::collections::BTreeSet;

/// The code of `letter` in `set`, for the tests that draw.
#[cfg(feature = "render")]
fn code_of(set: CharSet, letter: char) -> u8 {
    (SPACE..0x7F)
        .find(|&code| to_unicode(CharRef::new(set, code)).base == letter)
        .unwrap_or_else(|| panic!("{letter} is in {set:?}"))
}

/// REQ-CHR-014: every tone on every Vietnamese vowel, placed by X/26 on
/// the Vietnamese G0 set with marks from the Vietnamese G2 set,
/// composes to its precomposed letter. Tones: grave 4/1, acute 4/2,
/// tilde 4/4, dot below 4/9, hook above 4/C.
/// REQ-82: the Vietnamese set is a whole set, so that X/26 marks find its letters as bases.
#[test]
fn every_vietnamese_tone_composes() {
    const TONES: [u8; 5] = [0x1, 0x2, 0x4, 0x9, 0xC];
    let expected = [
        ('a', "àáãạả"),
        ('ă', "ằắẵặẳ"),
        ('â', "ầấẫậẩ"),
        ('e', "èéẽẹẻ"),
        ('ê', "ềếễệể"),
        ('i', "ìíĩịỉ"),
        ('o', "òóõọỏ"),
        ('ô', "ồốỗộổ"),
        ('ơ', "ờớỡợở"),
        ('u', "ùúũụủ"),
        ('ư', "ừứữựử"),
        ('y', "ỳýỹỵỷ"),
        ('A', "ÀÁÃẠẢ"),
        ('Ă', "ẰẮẴẶẲ"),
        ('Â', "ẦẤẪẬẨ"),
        ('E', "ÈÉẼẸẺ"),
        ('Ê', "ỀẾỄỆỂ"),
        ('I', "ÌÍĨỊỈ"),
        ('O', "ÒÓÕỌỎ"),
        ('Ô', "ỒỐỖỘỔ"),
        ('Ơ', "ỜỚỠỢỞ"),
        ('U', "ÙÚŨỤỦ"),
        ('Ư', "ỪỨỮỰỬ"),
        ('Y', "ỲÝỸỴỶ"),
    ];
    let set = CharSet::G0(G0 { set: G0Set::Vietnamese, national: None });
    for (vowel, toned) in expected {
        let code = (SPACE..0x7F)
            .find(|&code| to_unicode(CharRef::new(set, code)).base == vowel)
            .unwrap_or_else(|| panic!("{vowel} is in the Vietnamese set"));
        for (tone, want) in TONES.into_iter().zip(toned.chars()) {
            let mut character = CharRef::new(set, code);
            character.diacritic = page_mark(G2Set::Vietnamese, tone);
            let unicode = to_unicode(character);
            assert_eq!(compose(unicode.base, unicode.combining.unwrap()), Some(want), "{vowel} tone {tone:X}");
        }
    }
}

/// REQ-CHR-014: 4/C is the hook above only in the Vietnamese G2 set;
/// pages with the standard's sets keep the low line.
#[test]
fn the_hook_above_is_vietnamese_only() {
    let hook = to_unicode(CharRef {
        diacritic: page_mark(G2Set::Vietnamese, 0xC),
        ..CharRef::new(CharSet::G0(G0 { set: G0Set::Latin, national: None }), b'a')
    });
    assert_eq!(hook.combining, Some('\u{0309}'));
    for g2 in [G2Set::Latin, G2Set::Cyrillic, G2Set::Greek, G2Set::Arabic, G2Set::Armenian] {
        for mark in 0..=0xF {
            assert_eq!(page_mark(g2, mark), mark);
        }
    }
    for mark in (0..=0xF).filter(|&mark| mark != 0xC) {
        assert_eq!(page_mark(G2Set::Vietnamese, mark), mark);
    }
}

/// REQ-77, DEC-2350 (TODO-1164): ё and Ё are reached on a Cyrillic page
/// that lacks them as е and Е with the diaeresis placed by X/26, on the
/// standard's Russian set and on NeoTeletext's Kyrgyz set alike, and are
/// drawn from the font's precomposed glyphs.
#[cfg(all(feature = "extended-sets", feature = "render"))]
#[test]
fn yo_is_e_with_a_diaeresis_on_a_cyrillic_page() {
    const DIAERESIS: u8 = 8;
    let glyphs = crate::glyphs::GlyphSet::noto_teletext();
    for set in [G0Set::Cyrillic2, G0Set::KyrgyzMongolian] {
        let set = CharSet::G0(G0 { set, national: None });
        for (plain, with_diaeresis) in [('е', 'ё'), ('Е', 'Ё')] {
            let character = CharRef { diacritic: DIAERESIS, ..CharRef::new(set, code_of(set, plain)) };
            let unicode = to_unicode(character);
            assert_eq!(compose(unicode.base, unicode.combining.unwrap()), Some(with_diaeresis), "{set:?}");
            assert_eq!(Some(&glyphs.glyph(character)), glyphs.lookup(with_diaeresis), "{set:?} draws {with_diaeresis}");
        }
    }
}

/// DEC-2370 (TODO-1165): Greek Ό, the one capital with tonos the
/// standard's Greek set lacks, is reached on a Greek page as Ο with the
/// acute placed by X/26, and drawn from the font's precomposed glyph.
#[cfg(feature = "render")]
#[test]
fn greek_omicron_with_tonos_is_omicron_with_the_acute() {
    const ACUTE: u8 = 2;
    let greek = CharSet::G0(G0 { set: G0Set::Greek, national: None });
    let character = CharRef { diacritic: ACUTE, ..CharRef::new(greek, code_of(greek, 'Ο')) };
    let unicode = to_unicode(character);
    assert_eq!(compose(unicode.base, unicode.combining.unwrap()), Some('Ό'));
    let glyphs = crate::glyphs::GlyphSet::noto_teletext();
    assert_eq!(Some(&glyphs.glyph(character)), glyphs.lookup('Ό'));
}

#[test]
fn compose_spot_checks() {
    let acute = diacritic_combining(2).unwrap();
    let dot_above = diacritic_combining(7).unwrap();
    let caron = diacritic_combining(15).unwrap();
    assert_eq!(compose('A', acute), Some('Á'));
    assert_eq!(compose('c', caron), Some('č'));
    assert_eq!(compose('Z', dot_above), Some('Ż'));
    assert_eq!(compose('x', acute), None);
    assert_eq!(compose('o', diacritic_combining(13).unwrap()), Some('ő'));
    assert_eq!(compose('s', diacritic_combining(11).unwrap()), Some('ş'));
    assert_eq!(compose('e', diacritic_combining(14).unwrap()), Some('ę'));
    assert_eq!(compose('\u{0131}', acute), None);
    assert_eq!(compose('A', 'B'), None);
}

#[test]
fn composition_table_is_sorted_and_uses_known_marks() {
    for pair in COMPOSITIONS.windows(2) {
        assert!((pair[0].0, pair[0].1) < (pair[1].0, pair[1].1), "{pair:?}");
    }
    let marks: BTreeSet<char> = COMBINING_MARKS.iter().flatten().copied().collect();
    for (base, mark, composed) in COMPOSITIONS {
        assert!(base.is_ascii_alphabetic());
        assert!(marks.contains(&mark));
        assert_eq!(compose(base, mark), Some(composed));
        // Composition keeps the case of the base letter.
        assert_eq!(composed.is_uppercase(), base.is_uppercase(), "{composed}");
    }
    for pair in EXTENDED_COMPOSITIONS.windows(2) {
        assert!((pair[0].0, pair[0].1) < (pair[1].0, pair[1].1), "{pair:?}");
    }
    for (base, mark, composed) in EXTENDED_COMPOSITIONS {
        assert!(marks.contains(&mark));
        assert_eq!(search(&COMPOSITIONS, base, mark), None, "{composed} is in one table only");
        assert_eq!(compose(base, mark), Some(composed));
        assert_eq!(composed.is_uppercase(), base.is_uppercase(), "{composed}");
    }
}

#[test]
fn every_latin_g0_letter_with_every_mark() {
    // Each Latin G0 letter with each mark either has a precomposed form
    // or none; the pairs national option sub-sets need must compose.
    let needed = [
        ('u', 10, 'ů'),
        ('c', 15, 'č'),
        ('t', 15, 'ť'),
        ('o', 4, 'õ'),
        ('e', 7, 'ė'),
        ('a', 14, 'ą'),
        ('u', 5, 'ū'),
        ('n', 2, 'ń'),
        ('T', 11, 'Ţ'),
        ('g', 6, 'ğ'),
        ('I', 7, 'İ'),
        ('u', 13, 'ű'),
    ];
    for (base, mark, composed) in needed {
        assert_eq!(compose(base, diacritic_combining(mark).unwrap()), Some(composed));
    }
    let mut count = 0;
    for code in (0x41..=0x5A).chain(0x61..=0x7A) {
        let base = base_unicode(latin_set(None), code);
        for mark in 1..=15 {
            count += usize::from(compose(base, diacritic_combining(mark).unwrap()).is_some());
        }
    }
    assert_eq!(count, COMPOSITIONS.len());
}
