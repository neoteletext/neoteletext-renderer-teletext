//! Tests of the code tables as a whole: every set's space and block,
//! codes outside the table, distinct letters and private use code points.
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

use crate::charset::code_table::{NATIONAL_BITS_MASK, base_unicode};
use crate::charset::test_tables::*;
use crate::consts::{SPACE, DESIGNATION_GROUP_SHIFT as GROUP_SHIFT, NATIONAL_INDEX_MASK};
use std::collections::BTreeSet;
use std::vec::Vec;

fn every_set() -> Vec<CharSet> {
    let mut sets = std::vec![latin_set(None)];
    sets.extend(NationalOption::ALL.iter().map(|&option| latin_set(Some(option))));
    sets.extend(
        [
            G0Set::Cyrillic1,
            G0Set::Cyrillic2,
            G0Set::Cyrillic3,
            G0Set::Greek,
            G0Set::Arabic,
            G0Set::Hebrew,
        ]
        .map(g0),
    );
    sets.extend(
        [G2Set::Latin, G2Set::Cyrillic, G2Set::Greek, G2Set::Arabic].map(CharSet::G2),
    );
    sets.push(CharSet::G3);
    sets
}

/// REQ-2166, DEC-2241: the private-use range the withdrawn mark pairs used,
/// U+F0100 to U+F017F, is retired: no allocation of the core lies in it.
#[cfg(feature = "extended-sets")]
#[test]
fn no_private_use_allocation_lies_in_the_retired_range() {
    const RETIRED: core::ops::RangeInclusive<char> = '\u{F0100}'..='\u{F017F}';
    assert!(joined_forms().iter().all(|&(_, _, point)| !RETIRED.contains(&point)));
    for code in 0x20u8..=0x7F {
        for set in [G2Set::Latin, G2Set::Cyrillic, G2Set::Greek, G2Set::Arabic, G2Set::Vietnamese, G2Set::Armenian] {
            assert!(!RETIRED.contains(&glyph_key(CharRef::new(CharSet::G2(set), code))));
        }
    }
}

/// REQ-2166 item 4, REQ-84: no private-use code point leaves the core as
/// text. Every code of every set fonts draw, in every joined form and with
/// every diacritical mark, gives `to_unicode` a base and a mark outside
/// Supplementary Private Use Area-A, where the core's joined forms are.
#[cfg(all(feature = "extended-sets", feature = "render"))]
#[test]
fn to_unicode_never_gives_a_private_use_code_point() {
    use crate::consts::DIACRITIC_HOOK_ABOVE;
    const PRIVATE_USE: core::ops::RangeInclusive<char> = '\u{F0000}'..='\u{FFFFD}';
    let forms = [JoinForm::Isolated, JoinForm::Final, JoinForm::Initial, JoinForm::Medial];
    assert!(joined_forms().iter().any(|&(_, _, point)| PRIVATE_USE.contains(&point)), "some form is private-use");
    for &(name, set) in &crate::glyphs::conformance::FONT_SETS {
        for code in 0x20u8..=0x7F {
            for form in forms {
                for diacritic in 0..=DIACRITIC_HOOK_ABOVE {
                    let character = CharRef { set, code, diacritic, form };
                    let text = to_unicode(character);
                    assert!(
                        !PRIVATE_USE.contains(&text.base)
                            && !text.combining.is_some_and(|mark| PRIVATE_USE.contains(&mark)),
                        "{name} {code:#04x} {form:?} mark {diacritic}: {text:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn national_bits_mask_is_the_index_mask() {
    assert_eq!(NATIONAL_BITS_MASK, NATIONAL_INDEX_MASK);
    assert_eq!(1_u8 << GROUP_SHIFT, NATIONAL_BITS_MASK + 1);
}

/// REQ-79: every code of every set converts to a scalar value, never the replacement character or a control.
#[test]
fn no_replacement_characters_in_any_set() {
    for set in every_set() {
        for code in ALL_CODES {
            let found = base_unicode(set, code);
            assert_ne!(found, char::REPLACEMENT_CHARACTER, "{set:?} {code:#04x}");
            assert!(!found.is_control(), "{set:?} {code:#04x}");
        }
    }
}

#[test]
fn codes_outside_the_table_are_spaces() {
    for set in every_set() {
        for code in (0..0x20).chain(0x80..=0xFF) {
            assert_eq!(base_unicode(set, code), ' ', "{set:?} {code:#04x}");
        }
    }
}

#[test]
fn space_is_space_in_every_alphanumeric_set() {
    // G3 2/0 is a smooth mosaic, not a space.
    for set in every_set().into_iter().filter(|&set| set != CharSet::G3) {
        assert_eq!(base_unicode(set, SPACE), ' ', "{set:?}");
    }
}

#[test]
fn letters_are_distinct_within_each_g0_set() {
    for set in [G0Set::Cyrillic1, G0Set::Cyrillic2, G0Set::Cyrillic3, G0Set::Greek]
        .map(g0)
    {
        let letters: Vec<char> = (0x40..0x7F).map(|code| base_unicode(set, code)).collect();
        let distinct: BTreeSet<char> = letters.iter().copied().collect();
        assert_eq!(distinct.len(), letters.len(), "{set:?}");
    }
}

/// REQ-82: every G0 set, neoteletext's own among them in a build with
/// `extended-sets`, has the space at 2/0 and the block at 7/F. The list of sets
/// is the font's, so the test needs `render`.
#[cfg(feature = "render")]
#[test]
fn every_g0_set_has_the_space_and_the_block() {
    const SOLID_BLOCK: char = '\u{25A0}';
    for (name, set) in crate::glyphs::conformance::FONT_SETS {
        if matches!(set, CharSet::G0(_)) {
            assert_eq!(base_unicode(set, 0x20), ' ', "{name}");
            assert_eq!(base_unicode(set, 0x7F), SOLID_BLOCK, "{name}");
        }
    }
}
