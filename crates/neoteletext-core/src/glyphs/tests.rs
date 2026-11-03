//! Tests of the bundled glyph set: order, size, the diacritics drawn over their bases, loaded
//! fonts and replaced G3 glyphs.
#![allow(clippy::indexing_slicing, clippy::unwrap_used)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use super::*;
use crate::charset::{CharSet, G0, G0Set};
use crate::glyphs::conformance::font_set_of;
use crate::types::FontGlyph;

#[test]
fn glyphs_are_sorted_and_unique() {
    let set = GlyphSet::noto_teletext();
    for table in [set.entries, set.extended] {
        assert!(table.windows(2).all(|pair| matches!(pair, [a, b] if a.code_point < b.code_point)));
    }
}

/// REQ-RND-020: the glyph set is data; an empty set is allowed and
/// finds nothing.
#[test]
fn set_size_and_empty_set() {
    static NONE: [GlyphEntry; 0] = [];
    let set = GlyphSet::noto_teletext();
    assert_eq!(set.len(), set.entries.len() + set.extended.len());
    assert!(!set.is_empty());
    let empty = GlyphSet::from_entries(&NONE, "empty");
    assert_eq!((empty.len(), empty.is_empty(), empty.provenance), (0, true, "empty"));
    assert_eq!(empty.lookup('A'), None);
    let latin = CharSet::G0(G0 { set: G0Set::Latin, national: None });
    assert_eq!(empty.glyph(CharRef::new(latin, b'A')), PLACEHOLDER, "REQ-CHR-004: a missing glyph is visible");
}

/// REQ-RND-014, TODO-1988: Igbo writes a tone on a letter with a dot below,
/// a pair Unicode gives no single character. The tone is drawn as the font
/// draws it on the plain letter, whose capital makes room for it, and the dot
/// is kept; overlaid, a tone merged into the capital's top lines.
#[test]
fn a_tone_on_a_dotted_letter_keeps_the_tone_and_the_dot() {
    // The marks' numbers (REQ-CHR-008) and how far down a tone reaches.
    const GRAVE: u8 = 1;
    const ACUTE: u8 = 2;
    const TONE_LINES: usize = 4;
    let set = GlyphSet::noto_teletext();
    let igbo = CharSet::G0(G0 { set: G0Set::Igbo, national: None });
    for (code, dotted, plain) in
        [(0x5B, '\u{1ECC}', 'O'), (0x5D, '\u{1ECA}', 'I'), (0x7B, '\u{1ECD}', 'o'), (0x7D, '\u{1ECB}', 'i')]
    {
        for (diacritic, tone) in [(GRAVE, '\u{0300}'), (ACUTE, '\u{0301}')] {
            let drawn = set.glyph(CharRef { diacritic, ..CharRef::new(igbo, code) });
            let toned = set.lookup(compose(plain, tone).unwrap()).unwrap();
            let dotted = set.lookup(dotted).unwrap();
            let plain = set.lookup(plain).unwrap();
            let dot: GlyphRows = core::array::from_fn(|row| dotted[row] & !plain[row]);
            assert_ne!(dot, [0; CELL_HEIGHT], "{code:#x}");
            assert_eq!(drawn[..TONE_LINES], toned[..TONE_LINES], "{code:#x} has its tone");
            assert_eq!(drawn, core::array::from_fn(|row| toned[row] | dot[row]), "{code:#x} has its dot");
        }
    }
}

/// REQ-RND-014, decision E.26: every one of the fifteen G2 column 4
/// marks (Table 28, modes 10001 to 11111) has a spacing form that
/// the font draws, and with no precomposed letter the mark is overlaid.
#[test]
fn every_diacritic_overlays_its_spacing_form() {
    // The spacing form Unicode pairs with each combining mark, in mark
    // order 4/1 to 4/F: grave, acute, circumflex, tilde, macron, breve,
    // dot above, diaeresis, dot below, ring, cedilla, low line, double
    // acute, ogonek and caron.
    const SPACING: [char; 15] = [
        '`', '\u{B4}', '\u{2C6}', '\u{2DC}', '\u{AF}', '\u{2D8}', '\u{2D9}', '\u{A8}', '.', '\u{2DA}', '\u{B8}', '_',
        '\u{2DD}', '\u{2DB}', '\u{2C7}',
    ];
    let set = GlyphSet::noto_teletext();
    let latin = CharSet::G0(G0 { set: G0Set::Latin, national: None });
    // "q" has no precomposed form with any of the marks.
    let base = set.lookup('q').unwrap();
    for (mark, &spacing) in (1u8..).zip(&SPACING) {
        let character = CharRef { diacritic: mark, ..CharRef::new(latin, b'q') };
        let combining = to_unicode(character).combining.unwrap();
        assert_eq!(spacing_mark(combining), Some(spacing), "mark {mark}");
        assert_eq!(compose('q', combining), None);
        // TODO-1110: the dot below is the font's own, from ạ, not the full stop.
        let mark_glyph = if combining == '\u{0323}' { &dot_below(&set) } else { set.lookup(spacing).unwrap() };
        assert_ne!(*mark_glyph, [0; CELL_HEIGHT], "mark {mark} draws something");
        let overlay: GlyphRows = core::array::from_fn(|row| base[row] | mark_glyph[row]);
        assert_eq!(set.glyph(character), overlay, "mark {mark}");
    }
    // A combining character outside the fifteen has no spacing form.
    assert_eq!(spacing_mark('\u{0305}'), None);
    assert_eq!(spacing_mark('a'), None);
}

/// The dot ạ adds to a in `set`.
fn dot_below(set: &GlyphSet<'_>) -> GlyphRows {
    let (dotted, plain) = (set.lookup('\u{1EA1}').unwrap(), set.lookup('a').unwrap());
    core::array::from_fn(|row| dotted[row] & !plain[row])
}

/// REQ-RND-014, TODO-1110: a letter with the dot below and no precomposed
/// form, such as q, has the dot drawn under it, below the baseline where
/// ạ has it, and not the full stop, which stands on the baseline beside
/// the letter's foot.
#[test]
fn the_dot_below_is_drawn_under_the_letter() {
    const DOT_BELOW: u8 = 9;
    const BASELINE: usize = 16;
    let set = GlyphSet::noto_teletext();
    let latin = CharSet::G0(G0 { set: G0Set::Latin, national: None });
    let character = CharRef { diacritic: DOT_BELOW, ..CharRef::new(latin, b'q') };
    assert_eq!(compose('q', to_unicode(character).combining.unwrap()), None, "q has no precomposed dotted form");
    let dot = dot_below(&set);
    let dot_lines: std::vec::Vec<usize> = (0..CELL_HEIGHT).filter(|&line| dot[line] != 0).collect();
    assert!(
        !dot_lines.is_empty() && dot_lines.iter().all(|&line| line >= BASELINE),
        "the dot lies below the baseline: {dot_lines:?}"
    );
    let base = set.lookup('q').unwrap();
    let expected: GlyphRows = core::array::from_fn(|row| base[row] | dot[row]);
    assert_eq!(set.glyph(character), expected);
    let full_stop = set.lookup('.').unwrap();
    let with_full_stop: GlyphRows = core::array::from_fn(|row| base[row] | full_stop[row]);
    assert_ne!(set.glyph(character), with_full_stop, "not the full stop");
}

/// REQ-RND-014, decision E.26: a precomposed glyph when the set has
/// one, otherwise the spacing mark's glyph overlaid on the base.
#[test]
fn diacritics_use_the_precomposed_glyph_or_an_overlay() {
    const BASE: GlyphRows = [0, 0, 0, 0, 0x7E, 0x81, 0xFF, 0x80, 0x7E, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const MARK: GlyphRows = [0x0C, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    // Deliberately unlike the overlay, so the test tells them apart.
    const PRECOMPOSED: GlyphRows = [0x30, 0xC0, 0, 0, 0x7E, 0x81, 0xFF, 0x80, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    static WITH_PRECOMPOSED: [GlyphEntry; 3] = [
        GlyphEntry { code_point: 'e' as u32, rows: BASE },
        GlyphEntry { code_point: '\u{B4}' as u32, rows: MARK },
        GlyphEntry { code_point: '\u{E9}' as u32, rows: PRECOMPOSED },
    ];
    static OVERLAY_ONLY: [GlyphEntry; 2] =
        [GlyphEntry { code_point: 'e' as u32, rows: BASE }, GlyphEntry { code_point: '\u{B4}' as u32, rows: MARK }];
    let latin = CharSet::G0(G0 { set: G0Set::Latin, national: None });
    // Diacritic 2 is the acute accent (Table 28, mode 10010).
    let character = CharRef { diacritic: 2, ..CharRef::new(latin, b'e') };
    assert_eq!(GlyphSet::from_entries(&WITH_PRECOMPOSED, "test").glyph(character), PRECOMPOSED);
    let overlay: GlyphRows = core::array::from_fn(|row| BASE[row] | MARK[row]);
    assert_eq!(GlyphSet::from_entries(&OVERLAY_ONLY, "test").glyph(character), overlay);
}

/// REQ-FNT-005, decision E.26: a loaded font draws a character with a
/// diacritical mark only by its precomposed form; with none, as for "q"
/// with an acute, the font is not asked and the set's own glyphs overlay
/// the mark. G3 characters never come from a font, and G3 has no place
/// among the sets fonts draw (REQ-FNT-011).
#[test]
fn loaded_fonts_draw_precomposed_marks_only() {
    const DOTS: [u16; CELL_FIELD_LINES] = [0xFFF; CELL_FIELD_LINES];
    static FONT: [FontGlyph; 3] = [
        FontGlyph::small('q' as u32, DOTS),
        FontGlyph::small('\u{B4}' as u32, DOTS),
        FontGlyph::small('\u{E9}' as u32, DOTS),
    ];
    let glyphs = GlyphSet::noto_teletext().with_fonts(Some(LoadedFont::new(&FONT).unwrap()), [None; FONT_SET_COUNT]);
    let latin = CharSet::G0(G0 { set: G0Set::Latin, national: None });
    // Diacritic 2 is the acute accent (Table 28, mode 10010).
    let acute = |code| CharRef { diacritic: 2, ..CharRef::new(latin, code) };
    assert_eq!(glyphs.loaded(acute(b'e')).map(|glyph| glyph.rows[0]), Some(0xFFF), "é, precomposed");
    assert_eq!(glyphs.loaded(acute(b'q')), None, "no precomposed form");
    assert!(glyphs.loaded(CharRef::new(latin, b'q')).is_some());
    assert_eq!(glyphs.loaded(CharRef::new(CharSet::G3, 0x20)), None);
    assert_eq!(font_set_of(CharSet::G3), None);
}

/// REQ-RND-020, decision E.50: the G3 glyphs are data that a set can
/// replace, named by their provenance; a code outside 2/0 to 7/F is blank.
#[test]
fn g3_glyphs_can_be_replaced() {
    const FULL: G3Glyph = G3Glyph { small: [0xFFF; CELL_FIELD_LINES], large: [0xFFF; CELL_HEIGHT] };
    static ALL_FULL: [G3Glyph; G3_CODES] = [FULL; G3_CODES];
    let shipped = GlyphSet::noto_teletext();
    let replaced = shipped.with_g3(&ALL_FULL, "test");
    assert_eq!(replaced.g3_provenance, "test");
    assert_eq!(*replaced.g3(0x20), FULL);
    assert_ne!(*shipped.g3(0x20), FULL, "the shipped 2/0 differs");
    assert_eq!(*replaced.g3(0x1F), G3Glyph::BLANK);
    assert_eq!(*replaced.g3(0x80), G3Glyph::BLANK);
    assert_eq!(replaced.lookup('A'), shipped.lookup('A'), "the text glyphs are kept");
}

/// REQ-FNT-001: a build without `extended-sets` carries none of the glyphs
/// only neoteletext's own sets use; the standard's sets are drawn either way.
#[test]
fn the_extended_table_follows_the_feature() {
    let set = GlyphSet::noto_teletext();
    assert_eq!(set.extended.is_empty(), cfg!(not(feature = "extended-sets")));
    assert!(set.lookup('\u{10D0}').is_some() == cfg!(feature = "extended-sets"), "Georgian an");
    assert!(set.lookup('A').is_some() && set.lookup('\u{00C0}').is_some(), "Latin, with the composed letters");
}

/// REQ-CHR-004: the placeholder stands where a capital does, in the cell's
/// own twelve pixels.
#[test]
fn the_placeholder_is_a_visible_box() {
    assert!(PLACEHOLDER.iter().all(|row| row >> crate::consts::CELL_WIDTH == 0));
    assert_eq!(PLACEHOLDER.iter().filter(|&&row| row != 0).count(), 14, "lines 2 to 15");
    assert_ne!(GlyphSet::noto_teletext().lookup('A'), Some(&PLACEHOLDER));
}
