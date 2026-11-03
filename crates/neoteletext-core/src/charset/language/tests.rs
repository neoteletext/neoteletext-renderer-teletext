//! Tests of the one table from a language to its character set (REQ-2181).
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

use crate::charset::test_tables::*;

/// REQ-2181: the language table. Every entry's value resolves to a set;
/// tags match on their language and script subtags, ignoring case and
/// region; a tag naming no script takes the language's default; an
/// unknown language has no set.
#[cfg(feature = "extended-sets")]
#[test]
fn the_language_table() {
    use crate::charset::language::{LANGUAGES, language, language_value, region_for_iso_639_2};
    for entry in LANGUAGES {
        assert!(designation(entry.value).is_some(), "{}-{}", entry.language, entry.script);
        if let Some(standard) = entry.standard {
            assert!(designation(standard).is_some(), "{}-{}", entry.language, entry.script);
        }
    }
    let value_of = |tag: &str| language(tag).map(|found| found.value);
    assert_eq!(value_of("sr"), Some(value(0b0111, 0b011)), "Serbian in its Cyrillic set");
    assert_eq!(value_of("sr-Latn"), Some(value(0b0011, 0b101)));
    assert_eq!(value_of("SR-cyrl-RS"), Some(value(0b0111, 0b011)));
    assert_eq!(value_of("kk"), Some(value(0b0010, 0b110)), "Kazakh in the Turkish option, DEC-602");
    assert_eq!(value_of("en-GB"), Some(0));
    assert_eq!(value_of("ja"), Some(value(0b1101, 0b001)));
    assert_eq!(value_of("ja-Kana"), Some(value(0b1101, 0b010)));
    assert_eq!(value_of("ja-Latn"), Some(value(0b1101, 0b000)));
    assert_eq!(value_of("xx"), None);
    // REQ-2215: each entry's English name and direction, for the server's
    // list of languages.
    let entry = |tag: &str| language(tag).unwrap();
    assert_eq!((entry("ru").name, entry("sr-Latn").name), ("Russian", "Serbian (Latin script)"));
    assert!(entry("ar").right_to_left() && entry("ur").right_to_left() && entry("he").right_to_left());
    assert!(!entry("en").right_to_left() && !entry("vi").right_to_left());
    // DEC-2239: no language of the table is written in a script it dropped.
    assert_eq!((value_of("th"), value_of("lo")), (None, None));
    assert_eq!((entry("sr").tag_parts(), entry("sr-Latn").tag_parts()), (("sr", None), ("sr", Some("Latn"))));
    assert!(LANGUAGES.iter().all(|entry| !entry.name.is_empty()));
    assert_eq!(value_of("ru-Latn"), None, "no Latin Russian is written");
    // DEC-2184, DEC-2187 item 9: for Level 3.5 and below, rōmaji is Latin
    // English and kana are refused; other languages keep their sets.
    assert_eq!(language_value("ja-Latn", false), Some(0));
    assert_eq!((language_value("ja", false), language_value("ja-Kana", false)), (None, None));
    assert_eq!(language_value("ig", false), Some(value(0b0101, 0b110)));
    // DEC-2240: Yoruba is not in the table; Igbo keeps the set it shared.
    assert_eq!(value_of("yo"), None);
    // REQ-2181 item 5: the subtitle region of every code the input knew
    // before is unchanged.
    let before: [(&[u8; 3], u8); 21] = [
        (b"pol", 1),
        (b"tur", 2),
        (b"srp", 3),
        (b"scc", 3),
        (b"hrv", 3),
        (b"scr", 3),
        (b"slv", 3),
        (b"ron", 3),
        (b"RUM", 3),
        (b"rus", 4),
        (b"bul", 4),
        (b"ukr", 4),
        (b"est", 4),
        (b"lav", 4),
        (b"lit", 4),
        (b"gre", 6),
        (b"ell", 6),
        (b"ara", 8),
        (b"heb", 10),
        (b"eng", 0),
        (b"zzz", 0),
    ];
    for (code, region) in before {
        assert_eq!(region_for_iso_639_2(*code), region, "{}", core::str::from_utf8(code).unwrap_or("?"));
    }
}
