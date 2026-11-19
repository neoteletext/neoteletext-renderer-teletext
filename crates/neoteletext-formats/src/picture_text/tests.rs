#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::string_slice,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    reason = "a test panics on what it rules out and indexes data it built to a known size"
)]

use super::*;
use alloc::vec;
use neoteletext_core::charset::CharRef;
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::glyphs::conformance::FONT_SETS;

/// Set numbers the tests name.
const LATIN_ENGLISH: u8 = 1;
const LATIN_GERMAN: u8 = 2;
const GREEK: u8 = 17;
const HEBREW: u8 = 19;
const G2_LATIN: u8 = 20;
const PERSIAN: u8 = 44;

fn regis_bytes(stretches: &[Stretch]) -> Vec<u8> {
    let mut out = Vec::new();
    regis(stretches, &mut out).unwrap();
    out
}

fn naplps_bytes(stretches: &[Stretch]) -> Vec<u8> {
    let mut out = Vec::new();
    naplps(stretches, &mut out).unwrap();
    out
}

fn sets_of(stretches: &[Stretch]) -> Vec<u8> {
    stretches.iter().map(|stretch| stretch.set).collect()
}

/// The code of `character` alone in `language`'s own set.
fn code_of(character: char, language: &str) -> u8 {
    let mut buffer = [0; 4];
    let [stretch] = &stretches(character.encode_utf8(&mut buffer), Some(language), false).unwrap()[..] else {
        panic!("{character}")
    };
    stretch.codes[0]
}

// PAR-3: a German run with `@` gives a German stretch and a Latin English
// one, the characters both show going with their neighbours.
#[test]
fn a_german_run_with_an_at_sign_takes_latin_english_for_it() {
    let run = stretches("Straße @", Some("de"), false).unwrap();
    assert_eq!(sets_of(&run), [LATIN_GERMAN, LATIN_ENGLISH]);
    assert_eq!(run[0].codes.len(), 7, "{run:?}");
    assert_eq!(run[1].codes, [b'@']);
}

// PAR-3: a Hebrew run is reversed into visual order.
#[test]
fn a_hebrew_run_is_in_visual_order() {
    let run = stretches("שלום", Some("he"), false).unwrap();
    let reading: Vec<u8> = "שלום".chars().map(|letter| code_of(letter, "he")).collect();
    let visual: Vec<u8> = reading.iter().rev().copied().collect();
    assert_eq!(run, [Stretch { set: HEBREW, codes: visual, ..Stretch::default() }]);
}

// PAR-3: a Persian run gives base codes in set 44, in visual order, lam
// then alef as the sets' one lam-alef code; the renderers join them.
#[test]
fn a_persian_run_gives_base_codes_in_set_44() {
    let run = stretches("سلام", Some("fa"), false).unwrap();
    assert_eq!(sets_of(&run), [PERSIAN]);
    assert_eq!(run[0].codes.len(), 3, "sin, lam-alef, mim: {run:?}");
    assert_eq!(run[0].codes[0], code_of('م', "fa"), "mim is leftmost in visual order");
    assert_eq!(run[0].codes[2], code_of('س', "fa"));
}

// PAR-3: Arabic at NeoTeletext's own level is the plain set 51, joined by
// the renderers; the teletext encoder chooses the same set for the run.
#[test]
fn arabic_is_the_plain_set_as_for_a_teletext_run() {
    let run = stretches("سلام", Some("ar"), false).unwrap();
    assert_eq!(sets_of(&run), [51]);
}

// PAR-3: a letter with a mark its set has no precomposed glyph for gives
// the mark entry in G2-Latin, the mark at column 4 of the G2 set.
#[test]
fn a_marked_letter_names_its_mark_in_g2_latin() {
    let run = stretches("é", None, false).unwrap();
    assert_eq!(
        run,
        [Stretch { set: LATIN_ENGLISH, codes: vec![b'e'], marks: vec![(0, G2_LATIN, 0x42)], stand_ins: 0 }]
    );
    let glyph = neoteletext_core::glyphs::conformance::assigned(FONT_SETS[usize::from(G2_LATIN)].1, 0x42);
    assert_eq!(glyph, Some('\u{00B4}'), "G2 Latin 4/2 is the acute accent");
    // The combining form takes the same cell.
    assert_eq!(stretches("e\u{0301}x", None, false).unwrap()[0].codes, [b'e', b'x']);
}

// PAR-3, DEC-2326: an unshown character is refused, and with `outside`
// becomes the stand-in, counted.
#[test]
fn an_unshown_character_is_refused_or_stood_in_for() {
    assert_eq!(stretches("ab ж", None, false), Err(PictureTextError::Unshown { index: 3 }));
    let run = stretches("ab ж ж", None, true).unwrap();
    assert_eq!(run, [Stretch { set: LATIN_ENGLISH, codes: b"ab ? ?".to_vec(), marks: vec![], stand_ins: 2 }]);
    assert_eq!(measure(&run).stand_ins, 2);
}

// PAR-3: the bounds and the characters a part may not hold.
#[test]
fn too_long_forbidden_and_unknown_are_refused() {
    let long: String = core::iter::repeat_n('a', MAX_PART_CHARACTERS + 1).collect();
    assert_eq!(stretches(&long, None, false), Err(PictureTextError::TooLong { characters: MAX_PART_CHARACTERS + 1 }));
    assert!(stretches(&long[1..], None, false).is_ok(), "exactly the bound");
    assert_eq!(stretches("a\u{7}", None, false), Err(PictureTextError::Forbidden { index: 1 }));
    assert_eq!(stretches("\u{202E}a", None, false), Err(PictureTextError::Forbidden { index: 0 }));
    assert_eq!(stretches("a", Some("xx"), false), Err(PictureTextError::UnknownLanguage("xx".into())));
}

// PAR-4: byte-exact ReGIS.
#[test]
fn regis_bytes_are_exactly_par_4s() {
    assert_eq!(regis_bytes(&stretches("Hi", None, false).unwrap()), b"T(A101)\"Hi\"T(A0)");
    let german = stretches("Straße @", Some("de"), false).unwrap();
    let mut expected = b"T(A102)\"Stra".to_vec();
    expected.push(code_of('ß', "de"));
    expected.extend_from_slice(b"e \"T(A101)\"@\"T(A0)");
    assert_eq!(regis_bytes(&german), expected);
    assert_eq!(regis_bytes(&stretches("a\"b", None, false).unwrap()), b"T(A101)\"a\"\"b\"T(A0)");
    assert_eq!(
        regis_bytes(&stretches("éa", None, false).unwrap()),
        b"T(A101)\"e\"T(A120)T(W(V))\"\x08\x42\"T(A101)\"a\"T(A0)"
    );
    let long: String = core::iter::repeat_n('a', 1_500).collect();
    let mut expected = b"T(A101)\"".to_vec();
    expected.extend(core::iter::repeat_n(b'a', regis::limits::MAX_STRING_BYTES));
    expected.extend_from_slice(b"\" \"");
    expected.extend(core::iter::repeat_n(b'a', 1_500 - regis::limits::MAX_STRING_BYTES));
    expected.extend_from_slice(b"\"T(A0)");
    let long = [Stretch { set: LATIN_ENGLISH, codes: long.into_bytes(), ..Stretch::default() }];
    assert_eq!(regis_bytes(&long), expected);
    // The cut strings end where they should: what follows them is read
    // as commands, and draws.
    let mut bytes = regis_bytes(&[Stretch { set: LATIN_ENGLISH, codes: vec![b'"'; 1_030], ..Stretch::default() }]);
    bytes.extend_from_slice(b"P[0,200]T(A101)\"H\"");
    assert!(regis_drawn(LATIN_ENGLISH, &bytes).iter().any(|&(_, y)| (200..220).contains(&y)));
    let block = [Stretch { set: 0, codes: vec![0x7F], ..Stretch::default() }];
    assert_eq!(regis(&block, &mut Vec::new()), Err(PictureTextError::Unwritable { index: 0 }));
    let bad_set = [Stretch { set: 55, codes: vec![b'a'], ..Stretch::default() }];
    assert_eq!(regis(&bad_set, &mut Vec::new()), Err(PictureTextError::Unwritable { index: 0 }));
}

// PAR-5: byte-exact NAPLPS.
#[test]
fn naplps_bytes_are_exactly_par_5s() {
    assert_eq!(
        naplps_bytes(&stretches("Hi", None, false).unwrap()),
        [0x1B, 0x28, 0x31, 0x0F, b'H', b'i', 0x1B, 0x28, 0x42]
    );
    // Greek is set 17: the supplement 2/1 and the final 3/1.
    let greek = stretches("α", Some("el"), false).unwrap();
    assert_eq!(sets_of(&greek), [GREEK]);
    assert_eq!(naplps_bytes(&greek), [0x1B, 0x28, 0x21, 0x31, 0x0F, greek[0].codes[0], 0x1B, 0x28, 0x42]);
    assert_eq!(
        naplps_bytes(&stretches("é", None, false).unwrap()),
        [0x1B, 0x28, 0x31, 0x0F, b'e', 0x08, 0x1B, 0x2E, 0x21, 0x34, 0x19, 0x42, 0x1B, 0x2A, 0x7C, 0x1B, 0x28, 0x42]
    );
    let block = [Stretch { set: 0, codes: vec![b'a', 0x7F], ..Stretch::default() }];
    assert_eq!(
        naplps_bytes(&block),
        [0x1B, 0x28, 0x30, 0x0F, b'a', 0x1B, 0x2E, 0x30, 0x19, 0x7F, 0x1B, 0x2A, 0x7C, 0x1B, 0x28, 0x42]
    );
}

// PAR-4, PAR-5: the worst run of MAX_PART_CHARACTERS stays within the
// part bounds in both languages.
#[test]
fn the_worst_run_stays_within_the_part_bounds() {
    let mut worst = Vec::new();
    for index in 0..MAX_PART_CHARACTERS {
        // A set change and a mark at every character.
        let set = if index % 2 == 0 { 54 } else { 53 };
        worst.push(Stretch { set, codes: vec![b'"'], marks: vec![(0, 43, 0x7E)], stand_ins: 0 });
    }
    assert!(regis_bytes(&worst).len() <= MAX_REGIS_PART_BYTES);
    assert!(naplps_bytes(&worst).len() <= MAX_NAPLPS_PART_BYTES);
}

// PAR-6: the figures equal the writers' lengths and the cells.
#[test]
fn measure_equals_the_writers() {
    for (text, language) in [("Straße @", Some("de")), ("éa\"", None), ("שלום", Some("he")), ("αβγ", Some("el"))]
    {
        let run = stretches(text, language, false).unwrap();
        let measured = measure(&run);
        assert_eq!(measured.regis_bytes as usize, regis_bytes(&run).len(), "{text}");
        assert_eq!(measured.naplps_bytes as usize, naplps_bytes(&run).len(), "{text}");
        assert_eq!(measured.cells as usize, run.iter().map(|stretch| stretch.codes.len()).sum::<usize>());
    }
    let measured = measure(&stretches("äé @", Some("de"), false).unwrap());
    assert_eq!(measured.sets, [LATIN_GERMAN, G2_LATIN, LATIN_ENGLISH], "in order of first use, marks' sets too");
    assert_eq!(measured.cells, 4, "the mark takes no cell");
}

/// The engine's glyphs of set `set`, as the ReGIS renderer's provided set.
fn provided(glyphs: &GlyphSet<'_>, set: u8) -> regis::ProvidedSet {
    let (_, charset) = FONT_SETS[usize::from(set)];
    let mut provided = regis::ProvidedSet::new(12, 20).unwrap();
    for code in FIRST_CODE..=LAST_CODE {
        provided.set_glyph(code, &glyphs.glyph(CharRef::new(charset, code))).unwrap();
    }
    provided
}

/// The engine's glyphs of set `set`, as the NAPLPS renderer's bitmap set.
fn bitmap(glyphs: &GlyphSet<'_>, set: u8, blank: bool) -> naplps::BitmapSet {
    let (_, charset) = FONT_SETS[usize::from(set)];
    let mut bitmap = naplps::BitmapSet::new(12, 20).unwrap();
    for code in FIRST_CODE..=LAST_CODE {
        let rows = if blank { [0; 20] } else { glyphs.glyph(CharRef::new(charset, code)) };
        bitmap.set_glyph(code, &rows).unwrap();
    }
    bitmap
}

/// The pixels lit when `bytes` are drawn by the ReGIS renderer with set
/// `set` given.
fn regis_drawn(set: u8, bytes: &[u8]) -> Vec<(usize, usize)> {
    let glyphs = GlyphSet::noto_teletext();
    let mut screen = regis::Screen::new();
    let mut terminal = regis::Terminal::new();
    terminal.set_provided_set(set, provided(&glyphs, set)).unwrap();
    let mut stream = b"\x1bPpP[0,0]".to_vec();
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"\x1b\\");
    let _ = terminal.feed(&mut screen, &stream);
    let _ = terminal.finish(&mut screen);
    let mut lit = Vec::new();
    for y in 0..regis::SCREEN_HEIGHT {
        for x in 0..regis::SCREEN_WIDTH {
            if screen.displayed().pixel(x, y).is_some_and(|index| index != 0) {
                lit.push((usize::try_from(x).unwrap(), usize::try_from(y).unwrap()));
            }
        }
    }
    lit
}

/// The default ReGIS character display cell's width and its unit cell's
/// (the ReGIS renderer's defaults: 9 by 20, 8 by 20).
const REGIS_DISPLAY_WIDTH: usize = 9;
const REGIS_UNIT_WIDTH: usize = 8;
const GLYPH_WIDTH: usize = 12;
const GLYPH_HEIGHT: usize = 20;

/// The pixels a run of `codes` of set `set` lights by the provided glyph
/// scaled into the default unit cell, the text at `origin`.
fn regis_expected(set: u8, codes: &[u8], origin: (usize, usize)) -> Vec<(usize, usize)> {
    let glyphs = GlyphSet::noto_teletext();
    let (_, charset) = FONT_SETS[usize::from(set)];
    let mut lit = Vec::new();
    for (cell, &code) in codes.iter().enumerate() {
        let rows = glyphs.glyph(CharRef::new(charset, code));
        for (line, row) in rows.iter().enumerate() {
            for unit in 0..REGIS_UNIT_WIDTH {
                let column = unit * GLYPH_WIDTH / REGIS_UNIT_WIDTH;
                if row & (1 << (GLYPH_WIDTH - 1 - column)) != 0 {
                    lit.push((origin.0 + cell * REGIS_DISPLAY_WIDTH + unit, origin.1 + line));
                }
            }
        }
    }
    lit.sort_by_key(|&(x, y)| (y, x));
    lit
}

// PAR-4: for every one of the 55 sets, the bytes drawn by the ReGIS
// renderer with the engine's glyphs give the provided glyphs at the cells
// expected.
#[test]
fn regis_text_draws_the_provided_glyphs_of_every_set() {
    // Where text at P[0,0] begins, found once from a known glyph.
    let probe = [Stretch { set: LATIN_ENGLISH, codes: vec![b'H'], ..Stretch::default() }];
    let drawn = regis_drawn(LATIN_ENGLISH, &regis_bytes(&probe));
    let origin = (0..GLYPH_HEIGHT)
        .flat_map(|y| (0..REGIS_DISPLAY_WIDTH).map(move |x| (x, y)))
        .find(|&origin| regis_expected(LATIN_ENGLISH, b"H", origin) == drawn)
        .expect("text at P[0,0] draws H's glyph");
    // A row of the screen holds 88 cells; each half of a set is a run.
    for set in 0..u8::try_from(FONT_SET_COUNT).unwrap() {
        let all: Vec<u8> = (FIRST_CODE..LAST_CODE).collect();
        for codes in all.chunks(48) {
            let run = [Stretch { set, codes: codes.to_vec(), ..Stretch::default() }];
            let mut drawn = regis_drawn(set, &regis_bytes(&run));
            drawn.sort_by_key(|&(x, y)| (y, x));
            assert_eq!(drawn, regis_expected(set, codes, origin), "set {set} from {:#x}", codes[0]);
        }
    }
}

/// `bytes` drawn by the NAPLPS renderer with set `set` given (blank, or
/// the engine's glyphs) on the receiver's canvas of 480 by 460 (RAS-11,
/// DEC-6857): the canvas and the diagnostics.
fn naplps_drawn(set: u8, blank: bool, bytes: &[u8]) -> (Vec<u8>, naplps::Diagnostics) {
    use naplps::limits::{RECEIVER_CANVAS_HEIGHT, RECEIVER_CANVAS_WIDTH};
    let glyphs = GlyphSet::noto_teletext();
    let mut canvas = naplps::Canvas::new(RECEIVER_CANVAS_WIDTH, RECEIVER_CANVAS_HEIGHT).unwrap();
    let mut terminal = naplps::Terminal::new(4);
    terminal.set_teletext_set(set, bitmap(&glyphs, set, blank)).unwrap();
    if set != G2_LATIN {
        terminal.set_teletext_set(G2_LATIN, bitmap(&glyphs, G2_LATIN, blank)).unwrap();
    }
    terminal.feed(&mut canvas, bytes);
    terminal.finish(&mut canvas);
    let pixels = (0..RECEIVER_CANVAS_HEIGHT)
        .flat_map(|y| (0..RECEIVER_CANVAS_WIDTH).map(move |x| (x, y)))
        .map(|(x, y)| canvas.pixel(x, y).unwrap_or(0))
        .collect();
    (pixels, terminal.diagnostics())
}

// PAR-5: for every one of the 55 sets, the bytes drawn by the NAPLPS
// renderer read without a byte passed over or refused, and what they draw
// is the given set's glyphs: the same bytes with that set blank draw
// nothing. A mark and a 7/15 code read the same way.
#[test]
fn naplps_text_draws_the_given_glyphs_of_every_set() {
    for set in 0..u8::try_from(FONT_SET_COUNT).unwrap() {
        let run = [Stretch { set, codes: (b'!'..=b'~').collect(), ..Stretch::default() }];
        let bytes = naplps_bytes(&run);
        let (drawn, diagnostics) = naplps_drawn(set, false, &bytes);
        assert_eq!(diagnostics, naplps::Diagnostics::default(), "set {set}");
        let (blank, _) = naplps_drawn(set, true, &bytes);
        let glyphs_lit = (b'!'..=b'~').any(|code| {
            GlyphSet::noto_teletext()
                .glyph(CharRef::new(FONT_SETS[usize::from(set)].1, code))
                .iter()
                .any(|row| *row != 0)
        });
        assert_eq!(drawn != blank, glyphs_lit, "set {set}: its glyphs are what is drawn");
    }
    let marked = naplps_bytes(&stretches("é", None, false).unwrap());
    assert_eq!(naplps_drawn(LATIN_ENGLISH, false, &marked).1, naplps::Diagnostics::default());
    let block = naplps_bytes(&[Stretch { set: 0, codes: vec![0x7F], ..Stretch::default() }]);
    assert_eq!(naplps_drawn(0, false, &block).1, naplps::Diagnostics::default());
}
