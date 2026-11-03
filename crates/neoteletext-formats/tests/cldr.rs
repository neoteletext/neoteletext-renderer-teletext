//! Every language's letters against Unicode CLDR (REQ-82, DEC-2411): each
//! letter of CLDR 48.0.0's main exemplar characters for a language of the
//! core's table (REQ-2181), small and capital, is written alone by the page
//! writer for that language, with every means a page has (the set's codes,
//! ESC to the second set, G2 characters and diacritical marks by X/26, row
//! set triplets). A letter the writer refuses must be listed in
//! `cldr-refusals.txt` beside this file with the record that explains it;
//! an unlisted refusal fails, and so does a listed letter that is no longer
//! refused, so the list cannot go stale.
//!
//! CLDR's files are fetched at test time into `$NEOTELETEXT_CORPUS` by
//! `cargo xtask corpus` and checked against the SHA-256 pinned in
//! `tests/corpus-manifest.txt` (REQ-TST-022); they are never committed. So
//! the check is an ignored test, which the push gate runs with the files.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::expect_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out and `expect` naming what it expected"
)]

use std::collections::BTreeSet;

use neoteletext_core::charset::language::LANGUAGES;
use neoteletext_formats::write_text::{Choice, Level, WriteError, write_text};

#[path = "../../../tests/corpus_files.rs"]
mod corpus_files;

/// The CLDR release the manifest pins.
const CLDR_RELEASE: &str = "cldr-48.0.0";
/// The page every letter is written on; any page would do.
const PAGE: u16 = 0x100;
/// The expected refusals, one per line: the language's tag, the letter and
/// the record that explains it, separated by tabs.
const REFUSALS: &str = include_str!("cldr-refusals.txt");

/// How each entry of the core's table of languages is checked: the CLDR
/// locale whose exemplar characters it is checked against, or why it is
/// not. Every entry of `LANGUAGES` is here, so that a language added to the
/// table must be added here too.
const LOCALES: &[(&str, Result<&str, &str>)] = &[
    ("en", Ok("en")),
    ("id", Ok("id")),
    ("sw", Ok("sw")),
    ("de", Ok("de")),
    ("sv", Ok("sv")),
    ("fi", Ok("fi")),
    ("hu", Ok("hu")),
    ("it", Ok("it")),
    ("fr", Ok("fr")),
    ("pt", Ok("pt")),
    ("es", Ok("es")),
    ("cs", Ok("cs")),
    ("sk", Ok("sk")),
    ("pl", Ok("pl")),
    ("tr", Ok("tr")),
    ("kk", Err("CLDR's Kazakh is the Cyrillic alphabet; NeoTeletext writes the Latin one (DEC-602 item 1)")),
    ("hr", Ok("hr")),
    ("sl", Ok("sl")),
    ("sr-Latn", Ok("sr-Latn")),
    ("ro", Ok("ro")),
    ("et", Ok("et")),
    ("lv", Ok("lv")),
    ("lt", Ok("lt")),
    ("ru", Ok("ru")),
    ("bg", Ok("bg")),
    ("el", Ok("el")),
    ("he", Ok("he")),
    ("uk", Ok("uk")),
    ("sr", Ok("sr")),
    ("ar", Ok("ar")),
    ("ha", Ok("ha")),
    ("ff", Ok("ff")),
    ("ak", Ok("ak")),
    ("bm", Ok("bm")),
    ("dyu", Err("CLDR 48 has no Dyula")),
    ("din", Err("CLDR 48 has no Dinka")),
    ("fon", Err("CLDR 48 has no Fon")),
    ("kri", Err("CLDR 48 has no Krio")),
    ("ln", Ok("ln")),
    ("mnk", Err("CLDR 48 has no Mandinka")),
    ("men", Err("CLDR 48 has no Mende")),
    ("nus", Ok("nus")),
    ("ee", Ok("ee")),
    ("kab", Ok("kab")),
    ("tzm", Ok("tzm")),
    ("az", Ok("az")),
    ("kr", Err("CLDR 48 has no Kanuri")),
    ("mos", Err("CLDR 48 has no Mossi")),
    ("ig", Ok("ig")),
    ("vi", Ok("vi")),
    ("be", Ok("be")),
    ("mk", Ok("mk")),
    ("ky", Ok("ky")),
    ("mn", Ok("mn")),
    ("tt", Ok("tt")),
    ("tg", Ok("tg")),
    ("fa", Ok("fa")),
    ("prs", Ok("fa-AF")),
    ("ur", Ok("ur")),
    ("ps", Ok("ps")),
    ("sd", Ok("sd")),
    ("ckb", Ok("ckb")),
    ("ug", Ok("ug")),
    ("bal", Ok("bal")),
    ("ka", Ok("ka")),
    ("hy", Ok("hy")),
    ("zgh", Ok("zgh")),
    ("tzm-Tfng", Err("CLDR 48 has Central Atlas Tamazight in the Latin script only")),
    ("ja", Ok("ja")),
    ("ja-Hira", Err("the same letters as `ja`, whose kana are checked")),
    ("ja-Hrkt", Err("the same letters as `ja`, whose kana are checked")),
    ("ja-Kana", Err("the same letters as `ja`, whose kana are checked")),
    ("ja-Latn", Err("rōmaji is English's set with marks; CLDR has no exemplar list for it")),
];

/// The tag that names a table entry exactly.
fn tag_of(language: &neoteletext_core::charset::language::LanguageSet) -> String {
    match language.tag_parts() {
        (subtag, None) => subtag.to_string(),
        (subtag, Some(script)) => format!("{subtag}-{script}"),
    }
}

/// The value of the JSON string member `name` in `json`, unescaped. CLDR's
/// files are machine-written JSON whose strings use only `\\`, `\"` and
/// `\uXXXX` escapes.
fn json_string(json: &str, name: &str) -> String {
    let key = format!("\"{name}\": \"");
    let start = json.find(&key).unwrap_or_else(|| panic!("no {name}")) + key.len();
    let mut out = String::new();
    let mut characters = json[start..].chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => return out,
            '\\' => match characters.next().expect("an escape") {
                'u' => {
                    let hex: String = characters.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).unwrap()).unwrap());
                }
                'n' => out.push('\n'),
                other => out.push(other),
            },
            other => out.push(other),
        }
    }
    panic!("{name} is not terminated")
}

/// The elements of a CLDR exemplar set, written as a Unicode set (UTS 35
/// part 1 section 5.3.3): single characters, ranges `a-c`, and sequences
/// `{ch}`, with `\` escaping the next character and spaces between.
fn exemplars(set: &str) -> Vec<String> {
    let inner = set.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')).expect("a bracketed set");
    let mut elements = Vec::new();
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            ' ' => {}
            '{' => elements.push(characters.by_ref().take_while(|&c| c != '}').collect()),
            '-' => {
                let first = elements.pop().and_then(|text: String| text.chars().next()).expect("a range's start");
                let last = characters.next().expect("a range's end");
                elements.extend((first..=last).map(String::from));
            }
            '\\' => elements.push(characters.next().expect("an escaped character").to_string()),
            other => {
                // A combining mark belongs to the letter before it.
                let mut element = other.to_string();
                while let Some(&mark) = characters.peek() {
                    if !is_combining(mark) {
                        break;
                    }
                    element.push(mark);
                    characters.next();
                }
                elements.push(element);
            }
        }
    }
    elements
}

/// Combining marks (Unicode's blocks of them), which CLDR writes after the
/// letter they belong to.
fn is_combining(character: char) -> bool {
    matches!(u32::from(character), 0x0300..=0x036F | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x1DC0..=0x1DFF)
}

/// Han ideographs, which no NeoTeletext set holds: Japanese pages are
/// written in kana (DEC-2184, DEC-2187), and downloaded glyphs are banned,
/// so the kanji of CLDR's Japanese list are left out of the check.
fn is_han(text: &str) -> bool {
    text.chars().any(|c| matches!(u32::from(c), 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3005))
}

/// What the writer refuses of `element` for `tag`: each form of it that it
/// cannot write, small and capital.
fn refused(tag: &str, element: &str) -> Vec<String> {
    let mut forms = vec![element.to_string()];
    let capital = element.to_uppercase();
    if capital != element {
        forms.push(capital);
    }
    forms
        .into_iter()
        .filter(|form| match write_text(Choice::Language(tag), Level::NeoTeletext, PAGE, form) {
            Ok(_) => false,
            Err(WriteError::Missing { .. }) => true,
            Err(other) => panic!("{tag} {form:?}: {other}"),
        })
        .collect()
}

/// The refusals the list expects, as (tag, letter).
fn expected() -> BTreeSet<(String, String)> {
    REFUSALS
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let (tag, letter, record) = (fields.next(), fields.next(), fields.next());
            let (Some(tag), Some(letter), Some(record)) = (tag, letter, record) else {
                panic!("cldr-refusals.txt: {line:?} is not tag, letter and record")
            };
            assert!(record.contains("DEC-") || record.contains("TODO-") || record.contains("REQ-"), "{line:?}");
            (tag.to_string(), letter.to_string())
        })
        .collect()
}

/// Every refusal found for the checked languages, from CLDR's files.
fn found() -> BTreeSet<(String, String)> {
    let mut refusals = BTreeSet::new();
    for (tag, locale) in LOCALES {
        let Ok(locale) = locale else { continue };
        let name = format!("{CLDR_RELEASE}/{locale}.json");
        let json = String::from_utf8(corpus_files::read(&name)).unwrap();
        for element in exemplars(&json_string(&json, "exemplarCharacters")) {
            if is_han(&element) {
                continue;
            }
            refusals.extend(refused(tag, &element).into_iter().map(|form| (tag.to_string(), form)));
        }
    }
    refusals
}

/// Every entry of the core's table is checked or says why not, and no
/// entry is named twice.
#[test]
fn every_language_is_checked_or_says_why_not() {
    let table: Vec<String> = LANGUAGES.iter().map(tag_of).collect();
    let listed: Vec<String> = LOCALES.iter().map(|(tag, _)| (*tag).to_string()).collect();
    assert_eq!(listed, table);
}

/// The list's own lines are well-formed and name languages that are checked.
#[test]
fn the_refusal_list_names_checked_languages() {
    for (tag, _) in expected() {
        assert!(LOCALES.iter().any(|(listed, locale)| *listed == tag && locale.is_ok()), "{tag} is not checked");
    }
}

/// The parser of CLDR's sets reads ranges, sequences, escapes and marks.
#[test]
fn exemplar_sets_are_read() {
    assert_eq!(exemplars("[a-c {ch} \\- e\u{301} ä]"), ["a", "b", "c", "ch", "-", "e\u{301}", "ä"]);
    assert_eq!(json_string(r#"{"x": "[a \\- é]"}"#, "x"), "[a \\- é]");
}

// Verifies REQ-82: every letter CLDR gives each language can be written on
// a page in that language, except the refusals listed with their records.
#[test]
#[ignore = "needs $NEOTELETEXT_CORPUS with CLDR 48.0.0's files, which `cargo xtask corpus` fetches"]
fn every_cldr_letter_is_written_or_its_refusal_listed() {
    let (found, expected) = (found(), expected());
    let unlisted: Vec<_> = found.difference(&expected).collect();
    let stale: Vec<_> = expected.difference(&found).collect();
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "refused but not listed in cldr-refusals.txt: {unlisted:?}\nlisted but now written: {stale:?}"
    );
}
