//! Tests of the standard's other G0 sets: Cyrillic, Greek, Hebrew and
//! Arabic.
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
fn cyrillic_rows() {
    assert_eq!(row(g0(G0Set::Cyrillic1), 0x40), "ЧАБЦДЕФГХИЈКЛМНО");
    assert_eq!(row(g0(G0Set::Cyrillic1), 0x50), "ПЌРСТУВЃЉЊЗЋЖЂШЏ");
    assert_eq!(row(g0(G0Set::Cyrillic2), 0x50), "ПЯРСТУЖВЬЪЗШЭЩЧЫ");
    assert_eq!(row(g0(G0Set::Cyrillic2), 0x70), "пярстужвьъзшэщч■");
    assert_eq!(row(g0(G0Set::Cyrillic3), 0x50), "ПЯРСТУЖВЬІЗШЄЩЧЇ");
    assert_eq!(base_unicode(g0(G0Set::Cyrillic2), 0x26), 'ы');
    assert_eq!(base_unicode(g0(G0Set::Cyrillic3), 0x26), 'ї');
    assert_eq!(base_unicode(g0(G0Set::Cyrillic1), 0x26), '&');
    // Each lower-case letter is its capital two columns to the left;
    // 7/F is the solid block, so the last capital has no partner.
    for set in [G0Set::Cyrillic1, G0Set::Cyrillic2, G0Set::Cyrillic3].map(g0) {
        for code in 0x40..0x5F {
            let upper = base_unicode(set, code);
            let lower = base_unicode(set, code + 0x20);
            assert_eq!(upper.to_lowercase().next().unwrap(), lower, "{set:?} {code:#04x}");
        }
    }
}

#[test]
fn greek_rows() {
    let greek = g0(G0Set::Greek);
    assert_eq!(row(greek, 0x40), "ΐΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟ");
    assert_eq!(row(greek, 0x50), "ΠΡ΄ΣΤΥΦΧΨΩΪΫάέήί");
    assert_eq!(row(greek, 0x60), "ΰαβγδεζηθικλμνξο");
    assert_eq!(row(greek, 0x70), "πρςστυφχψωϊϋόύώ■");
    assert_eq!(row(CharSet::G2(G2Set::Greek), 0x60), "CDFGJLQRSUVWYZΆΉ");
}

#[test]
fn hebrew_rows() {
    let hebrew = g0(G0Set::Hebrew);
    assert_eq!(row(hebrew, 0x60), "אבגדהוזחטיךכלםמן");
    assert_eq!(row(hebrew, 0x70), "נסעףפץצקרשת₪‖¾÷■");
    assert_eq!(row(hebrew, 0x50), "PQRSTUVWXYZ←½→↑#");
}

#[test]
fn arabic_spot_checks() {
    let arabic = g0(G0Set::Arabic);
    assert_eq!(base_unicode(arabic, 0x41), '\u{0621}'); // hamza
    assert_eq!(base_unicode(arabic, 0x47), '\u{0627}'); // alef
    assert_eq!(base_unicode(arabic, 0x60), '\u{0640}'); // tatweel
    assert_eq!(base_unicode(arabic, 0x7E), '\u{FEFB}'); // lam with alef
    assert_eq!(base_unicode(arabic, 0x3F), '\u{061F}'); // question mark
    assert_eq!(base_unicode(arabic, 0x5F), '#');
    let arabic_g2 = CharSet::G2(G2Set::Arabic);
    let digits: std::string::String =
        (0x30..0x3A).map(|code| base_unicode(arabic_g2, code)).collect();
    assert_eq!(digits, "٠١٢٣٤٥٦٧٨٩");
    assert_eq!(row(arabic_g2, 0x70), "pqrstuvwxyzâôûç ");
    // Every Arabic G0 letter position holds an Arabic character.
    for code in 0x40..0x7F {
        if code == 0x5F {
            continue;
        }
        let found = u32::from(base_unicode(arabic, code));
        assert!(
            (0x0600..=0x06FF).contains(&found) || (0xFB50..=0xFEFF).contains(&found),
            "{code:#04x}"
        );
    }
}
