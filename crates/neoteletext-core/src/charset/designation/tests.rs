//! Tests of designation: Tables 32, 33 and 34, and the fallback of a
//! reserved entry.
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

use crate::charset::code_table::ENGLISH_DESIGNATION;
use crate::charset::test_tables::*;
use std::vec::Vec;

fn whole_g0(set: G0Set) -> G0 {
    G0 { set, national: None }
}

/// Every defined entry of Table 32: (group, index, G0, G2).
fn table_32_defined() -> Vec<(u8, u8, G0, G2Set)> {
    use NationalOption as N;
    let l = |group, index, national| (group, index, latin_g0(national), G2Set::Latin);
    std::vec![
        l(0b0000, 0, N::English),
        l(0b0000, 1, N::German),
        l(0b0000, 2, N::SwedishFinnishHungarian),
        l(0b0000, 3, N::Italian),
        l(0b0000, 4, N::French),
        l(0b0000, 5, N::PortugueseSpanish),
        l(0b0000, 6, N::CzechSlovak),
        l(0b0001, 0, N::Polish),
        l(0b0001, 1, N::German),
        l(0b0001, 2, N::SwedishFinnishHungarian),
        l(0b0001, 3, N::Italian),
        l(0b0001, 4, N::French),
        l(0b0001, 6, N::CzechSlovak),
        l(0b0010, 0, N::English),
        l(0b0010, 1, N::German),
        l(0b0010, 2, N::SwedishFinnishHungarian),
        l(0b0010, 3, N::Italian),
        l(0b0010, 4, N::French),
        l(0b0010, 5, N::PortugueseSpanish),
        l(0b0010, 6, N::Turkish),
        l(0b0011, 5, N::SerbianCroatianSlovenian),
        l(0b0011, 7, N::Rumanian),
        (0b0100, 0, whole_g0(G0Set::Cyrillic1), G2Set::Cyrillic),
        l(0b0100, 1, N::German),
        l(0b0100, 2, N::Estonian),
        l(0b0100, 3, N::LettishLithuanian),
        (0b0100, 4, whole_g0(G0Set::Cyrillic2), G2Set::Cyrillic),
        (0b0100, 5, whole_g0(G0Set::Cyrillic3), G2Set::Cyrillic),
        l(0b0100, 6, N::CzechSlovak),
        l(0b0110, 6, N::Turkish),
        (0b0110, 7, whole_g0(G0Set::Greek), G2Set::Greek),
        (0b1000, 0, latin_g0(N::English), G2Set::Arabic),
        (0b1000, 4, latin_g0(N::French), G2Set::Arabic),
        (0b1000, 7, whole_g0(G0Set::Arabic), G2Set::Arabic),
        (0b1010, 5, whole_g0(G0Set::Hebrew), G2Set::Arabic),
        (0b1010, 7, whole_g0(G0Set::Arabic), G2Set::Arabic),
    ]
}

/// REQ-CHR-012's table, written out again from the requirement.
fn extended_defined() -> Vec<(u8, u8, G0, G2Set)> {
    use NationalOption as N;
    if !cfg!(feature = "extended-sets") {
        return Vec::new();
    }
    let l = |index, national| (0b0101, index, latin_g0(national), G2Set::Latin);
    std::vec![
        l(0, N::HausaFula),
        l(1, N::WestAfrican),
        l(2, N::Ewe),
        l(3, N::KabyleTamazight),
        l(4, N::Azerbaijani),
        l(5, N::KanuriMossi),
        (0b0101, 6, whole_g0(G0Set::Igbo), G2Set::Latin),
        (0b0101, 7, whole_g0(G0Set::Vietnamese), G2Set::Vietnamese),
        (0b0111, 0, whole_g0(G0Set::Ukrainian), G2Set::Cyrillic),
        (0b0111, 1, whole_g0(G0Set::Belarusian), G2Set::Cyrillic),
        (0b0111, 2, whole_g0(G0Set::Macedonian), G2Set::Cyrillic),
        (0b0111, 3, whole_g0(G0Set::Serbian), G2Set::Cyrillic),
        (0b0111, 5, whole_g0(G0Set::KyrgyzMongolian), G2Set::Cyrillic),
        (0b0111, 6, whole_g0(G0Set::Tatar), G2Set::Cyrillic),
        (0b0111, 7, whole_g0(G0Set::Tajik), G2Set::Cyrillic),
        (0b1001, 0, whole_g0(G0Set::Persian), G2Set::Arabic),
        (0b1001, 1, whole_g0(G0Set::Urdu), G2Set::Arabic),
        (0b1001, 2, whole_g0(G0Set::Pashto), G2Set::Arabic),
        (0b1001, 3, whole_g0(G0Set::Sindhi), G2Set::Arabic),
        (0b1001, 4, whole_g0(G0Set::Sorani), G2Set::Arabic),
        (0b1001, 5, whole_g0(G0Set::Uyghur), G2Set::Arabic),
        (0b1001, 6, whole_g0(G0Set::Balochi), G2Set::Arabic),
        (0b1001, 7, whole_g0(G0Set::ArabicPlain), G2Set::Arabic),
        (0b1011, 0, whole_g0(G0Set::Georgian), G2Set::Latin),
        (0b1011, 1, whole_g0(G0Set::Armenian), G2Set::Armenian),
        (0b1011, 2, whole_g0(G0Set::Tifinagh), G2Set::Latin),
        (0b1101, 0, latin_g0(N::JapaneseRomaji), G2Set::Latin),
        (0b1101, 1, whole_g0(G0Set::Hiragana), G2Set::Latin),
        (0b1101, 2, whole_g0(G0Set::Katakana), G2Set::Latin),
    ]
}

/// REQ-L25-012: every row of Table 34, and its reserved values.
#[test]
fn table_34_modified_designation() {
    use G0Set as S;
    use G2Set::{Arabic, Cyrillic, Greek, Latin};
    let mut expected = Vec::new();
    for group in 0..4 {
        for low in 0..8 {
            expected.push((value(group, low), (S::Latin, Latin)));
        }
    }
    expected.extend([
        (value(0b0100, 0), (S::Cyrillic1, Cyrillic)),
        (value(0b0100, 1), (S::Latin, Latin)),
        (value(0b0100, 2), (S::Latin, Latin)),
        (value(0b0100, 3), (S::Latin, Latin)),
        (value(0b0100, 4), (S::Cyrillic2, Cyrillic)),
        (value(0b0100, 5), (S::Cyrillic3, Cyrillic)),
        (value(0b0100, 6), (S::Latin, Latin)),
        (value(0b0110, 6), (S::Latin, Latin)),
        (value(0b0110, 7), (S::Greek, Greek)),
        (value(0b1000, 0), (S::Latin, Arabic)),
        (value(0b1000, 4), (S::Latin, Arabic)),
        (value(0b1000, 7), (S::Arabic, Arabic)),
        (value(0b1010, 5), (S::Hebrew, Arabic)),
        (value(0b1010, 7), (S::Arabic, Arabic)),
    ]);
    #[cfg(feature = "extended-sets")]
    {
        use G2Set::{Armenian, Vietnamese};
        for low in 0..=5 {
            expected.push((value(0b0101, low), (S::Latin, Latin)));
        }
        expected.extend([
            (value(0b0101, 6), (S::Igbo, Latin)),
            (value(0b0101, 7), (S::Vietnamese, Vietnamese)),
            (value(0b0111, 0), (S::Ukrainian, Cyrillic)),
            (value(0b0111, 1), (S::Belarusian, Cyrillic)),
            (value(0b0111, 2), (S::Macedonian, Cyrillic)),
            (value(0b0111, 3), (S::Serbian, Cyrillic)),
            (value(0b0111, 5), (S::KyrgyzMongolian, Cyrillic)),
            (value(0b0111, 6), (S::Tatar, Cyrillic)),
            (value(0b0111, 7), (S::Tajik, Cyrillic)),
            (value(0b1001, 0), (S::Persian, Arabic)),
            (value(0b1001, 1), (S::Urdu, Arabic)),
            (value(0b1001, 2), (S::Pashto, Arabic)),
            (value(0b1001, 3), (S::Sindhi, Arabic)),
            (value(0b1001, 4), (S::Sorani, Arabic)),
            (value(0b1001, 5), (S::Uyghur, Arabic)),
            (value(0b1001, 6), (S::Balochi, Arabic)),
            (value(0b1001, 7), (S::ArabicPlain, Arabic)),
            (value(0b1101, 0), (S::Latin, Latin)),
            (value(0b1101, 1), (S::Hiragana, Latin)),
            (value(0b1101, 2), (S::Katakana, Latin)),
            (value(0b1011, 0), (S::Georgian, Latin)),
            (value(0b1011, 1), (S::Armenian, Armenian)),
            (value(0b1011, 2), (S::Tifinagh, Latin)),
        ]);
    }
    for data in 0..=0x7F {
        let want = expected.iter().find(|(entry, _)| *entry == data).map(|(_, sets)| *sets);
        assert_eq!(modified_designation(data), want, "value {data:#04x}");
    }
    assert_eq!(modified_designation(value(0b0100, 7)), None, "0100111 is blank, so reserved");
}

/// REQ-CHR-003: a defined Table 32 entry is used as it is.
#[test]
fn defined_designation_needs_no_fallback() {
    let (found, fallback) = designation_with_fallback(value(0b0011, 0b111));
    assert_eq!(found, Designation { g0: latin_g0(NationalOption::Rumanian), g2: G2Set::Latin });
    assert!(!fallback);
}

/// REQ-CHR-003 step 1: a reserved entry retries the same national
/// option bits in group 0000.
#[test]
fn reserved_designation_retries_group_0000() {
    // Group 0011 defines only indexes 101 and 111.
    let (found, fallback) = designation_with_fallback(value(0b0011, 0b110));
    assert_eq!(found, Designation { g0: latin_g0(NationalOption::CzechSlovak), g2: G2Set::Latin });
    assert!(fallback);
    // Group 1110 is wholly reserved.
    let (found, fallback) = designation_with_fallback(value(0b1110, 0b001));
    assert_eq!(found.g0, latin_g0(NationalOption::German));
    assert!(fallback);
}

/// REQ-CHR-003 step 2: when group 0000 is reserved too, English.
#[test]
fn doubly_reserved_designation_is_english() {
    let english = Designation { g0: latin_g0(NationalOption::English), g2: G2Set::Latin };
    for reserved in [value(0b1101, 0b111), value(0b1111, 0b111), value(0b0000, 0b111)] {
        assert_eq!(designation(reserved), None);
        assert_eq!(designation_with_fallback(reserved), (english, true), "value {reserved:#09b}");
    }
}

/// REQ-CHR-012: Table 34 gives neoteletext's own values' sets, and each
/// implies Latin English as the second set; the standard's none.
#[test]
fn extended_values_in_tables_33_and_34() {
    for (group, index, g0, g2) in extended_defined() {
        assert_eq!(modified_designation(value(group, index)), Some((g0.set, g2)));
        let second = match (group, index) {
            (0b1101, 1) => whole_g0(G0Set::Katakana),
            (0b1101, 2) => whole_g0(G0Set::Hiragana),
            _ => ENGLISH_DESIGNATION.g0,
        };
        assert_eq!(default_second_g0(value(group, index)), Some(second), "{group:04b} {index:03b}");
    }
    for (group, index, _, _) in table_32_defined() {
        assert_eq!(default_second_g0(value(group, index)), None);
    }
    let reserved = value(0b0111, 0b100);
    assert_eq!((designation(reserved), modified_designation(reserved), default_second_g0(reserved)), (None, None, None));
    // DEC-2239: group 1100's sets were withdrawn, so all of its values are
    // reserved again and fall back as any reserved value does (REQ-70).
    for index in 0..8 {
        let withdrawn = value(0b1100, index);
        assert_eq!((designation(withdrawn), modified_designation(withdrawn), default_second_g0(withdrawn)), (None, None, None));
    }
}

#[test]
fn table_32_every_entry() {
    let mut defined = table_32_defined();
    defined.extend(extended_defined());
    let mut seen = 0;
    for candidate in 0..=0x7F_u8 {
        let expected = defined
            .iter()
            .find(|(group, index, _, _)| value(*group, *index) == candidate)
            .map(|&(_, _, g0, g2)| Designation { g0, g2 });
        assert_eq!(designation(candidate), expected, "value {candidate:#09b}");
        seen += usize::from(expected.is_some());
    }
    assert_eq!(seen, defined.len());
}

#[test]
fn table_32_decision_e21_examples() {
    // France 2: C12 = 1, C13 = 0, C14 = 0 gives index 4.
    let french = designation(0b100).unwrap();
    assert_eq!(french.g0, latin_g0(NationalOption::French));
    // Italian: C12 = 0, C13 = 1, C14 = 1 gives index 3.
    let italian = designation(0b011).unwrap();
    assert_eq!(italian.g0, latin_g0(NationalOption::Italian));
    assert_eq!(designation(0).unwrap(), ENGLISH_DESIGNATION);
    assert_eq!(designation(1).unwrap().g0, latin_g0(NationalOption::German));
    assert_eq!(designation(value(0b0000, 0b111)), None);
    assert_eq!(designation(value(0b1110, 0b000)), None);
    assert_eq!(designation(0x7F), None);
}

#[test]
fn table_33_spot_checks() {
    assert_eq!(second_g0(0x7F), None);
    assert_eq!(second_g0(value(0b0100, 0b000)), Some(whole_g0(G0Set::Cyrillic1)));
    assert_eq!(second_g0(value(0b0100, 0b100)), Some(whole_g0(G0Set::Cyrillic2)));
    assert_eq!(second_g0(value(0b0110, 0b111)), Some(whole_g0(G0Set::Greek)));
    assert_eq!(second_g0(value(0b1000, 0b111)), Some(whole_g0(G0Set::Arabic)));
    assert_eq!(second_g0(value(0b1010, 0b101)), Some(whole_g0(G0Set::Hebrew)));
    assert_eq!(second_g0(value(0b0001, 0b000)), Some(latin_g0(NationalOption::Polish)));
    assert_eq!(second_g0(value(0b0010, 0b110)), Some(latin_g0(NationalOption::Turkish)));
    assert_eq!(second_g0(value(0b1101, 0b011)), None);
    assert_eq!(second_g0(value(0b0011, 0b000)), None);
    for (group, index, g0, _) in table_32_defined().into_iter().chain(extended_defined()) {
        assert_eq!(second_g0(value(group, index)), Some(g0));
    }
}
