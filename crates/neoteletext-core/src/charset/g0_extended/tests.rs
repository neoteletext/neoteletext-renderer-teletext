//! Tests of neoteletext's own G0 sets (REQ-CHR-013).
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

/// REQ-2188 item 1, DEC-2187 item 3: the kana tables. The 75 kana both
/// scripts share are the same sound at the same code; the digits are at
/// 3/0 to 3/9; hiragana keeps ! + - : ? at their ASCII codes; both put
/// 、 and 。 at the comma's and the full stop's.
#[cfg(feature = "extended-sets")]
#[test]
fn kana_tables() {
    let hiragana = CharSet::G0(G0 { set: G0Set::Hiragana, national: None });
    let katakana = CharSet::G0(G0 { set: G0Set::Katakana, national: None });
    let at = |set, code: u8| to_unicode(CharRef::new(set, code)).base;
    let mut shared = 0;
    for code in 0x21..=0x7E_u8 {
        let (h, k) = (at(hiragana, code), at(katakana, code));
        if ('\u{3041}'..='\u{3096}').contains(&h) && ('\u{30A1}'..='\u{30F6}').contains(&k) {
            assert_eq!(u32::from(k) - u32::from(h), 0x60, "{code:#04x}: {h} and {k} are one sound");
            shared += 1;
        }
    }
    assert_eq!(shared, 75);
    for digit in 0..10_u8 {
        assert_eq!(at(hiragana, 0x30 + digit), char::from(b'0' + digit));
        assert_eq!(at(katakana, 0x30 + digit), char::from(b'0' + digit));
    }
    let hiragana_own = [(0x21, '!'), (0x28, '「'), (0x29, '」'), (0x2B, '+'), (0x2D, '-'), (0x3A, ':'), (0x3F, '?')];
    for (code, expected) in hiragana_own {
        assert_eq!(at(hiragana, code), expected);
    }
    let katakana_own =
        [(0x21, 'ァ'), (0x28, 'ィ'), (0x29, 'ゥ'), (0x2B, 'ェ'), (0x2D, 'ー'), (0x3A, 'ォ'), (0x3F, 'ヴ')];
    for (code, expected) in katakana_own {
        assert_eq!(at(katakana, code), expected);
    }
    for set in [hiragana, katakana] {
        assert_eq!((at(set, 0x2C), at(set, 0x2E), at(set, 0x7F)), ('、', '。', '■'));
    }
    assert_eq!((at(hiragana, 0x40), at(katakana, 0x40)), ('あ', 'ア'));
    let romaji = CharSet::G0(G0 { set: G0Set::Latin, national: Some(NationalOption::JapaneseRomaji) });
    let options: std::string::String = [0x23, 0x24, 0x40, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x7B, 0x7C, 0x7D, 0x7E]
        .iter()
        .map(|&code| at(romaji, code))
        .collect();
    assert_eq!(options, "¥$ĀĒĪŌŪ#āēīōū");
}

/// DEC-2240, DEC-2244: the Igbo set keeps ị ọ ụ and ṅ, and the four
/// positions that held Yoruba's Ẹ ẹ Ṣ ṣ are unassigned.
#[test]
fn the_igbo_set_holds_no_yoruba_only_letters() {
    let igbo = |code| to_unicode(CharRef::new(CharSet::G0(G0 { set: G0Set::Igbo, national: None }), code)).base;
    assert_eq!([igbo(0x40), igbo(0x5C), igbo(0x60), igbo(0x7C)], [' '; 4]);
    assert_eq!(
        [igbo(0x5B), igbo(0x5D), igbo(0x5E), igbo(0x7B), igbo(0x7D), igbo(0x7E)],
        ['Ọ', 'Ị', 'Ụ', 'ọ', 'ị', 'ụ']
    );
}

/// REQ-82 (TODO-1164): the Tatar set holds Щ at 2/7 and щ at 3/E, in
/// place of Table 39's apostrophe and greater-than sign.
#[cfg(feature = "extended-sets")]
#[test]
fn tatar_holds_shcha_at_2_7_and_3_e() {
    let tatar = CharSet::G0(G0 { set: G0Set::Tatar, national: None });
    assert_eq!((to_unicode(CharRef::new(tatar, 0x27)).base, to_unicode(CharRef::new(tatar, 0x3E)).base), ('Щ', 'щ'));
}
