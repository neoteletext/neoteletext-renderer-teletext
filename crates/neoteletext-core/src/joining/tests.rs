//! Tests of the Arabic-script joining pass (REQ-CHR-015).
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and `panic!` on outcomes it rules out"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use super::*;
use crate::charset::{CharRef, G0Set, joined_form};
use crate::level1::Cell;
use std::vec::Vec;

const PERSIAN: CharSet = CharSet::G0(G0 { set: G0Set::Persian, national: None });

/// A row holding `text`, already in visual order, from column 0.
fn row(text: &str) -> [Cell; COLUMNS] {
    let mut row = [Cell::BLANK; COLUMNS];
    for (cell, letter) in row.iter_mut().zip(text.chars()) {
        let code = (0x20..0x7F).find(|&code| to_unicode(CharRef::new(PERSIAN, code)).base == letter).unwrap();
        cell.glyph = Glyph::Char(CharRef::new(PERSIAN, code));
    }
    row
}

fn forms(row: &[Cell; COLUMNS], length: usize) -> Vec<JoinForm> {
    row.iter()
        .take(length)
        .map(|cell| match cell.glyph {
            Glyph::Char(character) => character.form,
            _ => JoinForm::Isolated,
        })
        .collect()
}

/// سلام in visual order is م ا ل س: seen final... read right to left,
/// sin initial, lam medial, alef final, meem isolated (alef does not
/// join what follows).
#[test]
fn a_word_joins_right_to_left() {
    let mut cells = row("مالس");
    join_row(&mut cells);
    assert_eq!(forms(&cells, 4), [JoinForm::Isolated, JoinForm::Final, JoinForm::Medial, JoinForm::Initial]);
    let Glyph::Char(sin) = cells[3].glyph else { panic!() };
    assert_eq!(joined_form('\u{0633}', sin.form), '\u{FEB3}', "sin initial");
}

/// Joining stops at a space and at a non-joining letter; lam-alef is
/// right-joining; a Pashto letter without a presentation form draws
/// from the private-use area.
#[test]
fn breaks_ligature_and_private_forms() {
    // Visual "ب ب" (two words): both isolated.
    let mut cells = row("ب ب");
    join_row(&mut cells);
    assert_eq!(forms(&cells, 3), [JoinForm::Isolated; 3]);
    // Visual: lam-alef, beh: beh initial, lam-alef final.
    let mut cells = row("\u{FEFB}ب");
    join_row(&mut cells);
    assert_eq!(forms(&cells, 2), [JoinForm::Final, JoinForm::Initial]);
    assert_eq!(joined_form('\u{FEFB}', JoinForm::Final), '\u{FEFC}');
    // Hamza joins nothing: visual "ب ء ب" without spaces.
    let mut cells = row("بءب");
    join_row(&mut cells);
    assert_eq!(forms(&cells, 3), [JoinForm::Isolated; 3]);
    // Pashto tteh (U+067C) has no presentation forms.
    assert!(('\u{F0000}'..='\u{F00FF}').contains(&joined_form('\u{067C}', JoinForm::Medial)));
    assert_eq!(joined_form('\u{067C}', JoinForm::Isolated), '\u{067C}');
}

/// REQ-84 (REQ-CHR-015): the tatweel is join-causing (Unicode
/// `ArabicShaping.txt`), so the letters on either side of it join it and the
/// tatweel itself joins both. Visual order: beh, tatweel, beh, read right to
/// left as beh initial, tatweel medial, beh final.
#[test]
fn a_letter_beside_a_tatweel_joins_it() {
    let mut cells = row("\u{0628}\u{0640}\u{0628}");
    join_row(&mut cells);
    assert_eq!(forms(&cells, 3), [JoinForm::Final, JoinForm::Medial, JoinForm::Initial]);
}

/// REQ-84, DEC-2350: a letter carrying a diacritical mark placed by X/26
/// joins as the letter does, and so do its neighbours: visual sin, beh with
/// a mark, sin read right to left as initial, medial and final.
#[test]
fn a_letter_with_a_mark_still_joins() {
    let mut cells = row("\u{0633}\u{0628}\u{0633}");
    let Glyph::Char(beh) = &mut cells[1].glyph else { panic!() };
    beh.diacritic = 1;
    join_row(&mut cells);
    assert_eq!(forms(&cells, 3), [JoinForm::Final, JoinForm::Medial, JoinForm::Initial]);
}

/// REQ-82, REQ-84 (TODO-1164): the Balochi set holds ۏ and ࢩ, and they
/// join: visual ۏ then ࢩ, read right to left as ࢩ initial (dual-joining)
/// and ۏ final (right-joining), each drawn at a joined form of its own.
#[test]
fn balochi_waw_with_dot_above_and_yeh_join() {
    let balochi = CharSet::G0(G0 { set: G0Set::Balochi, national: None });
    let mut cells = [Cell::BLANK; COLUMNS];
    for (cell, letter) in cells.iter_mut().zip(['\u{06CF}', '\u{08A9}']) {
        let code = (0x20..0x7F).find(|&code| to_unicode(CharRef::new(balochi, code)).base == letter).unwrap();
        cell.glyph = Glyph::Char(CharRef::new(balochi, code));
    }
    join_row(&mut cells);
    assert_eq!(forms(&cells, 2), [JoinForm::Final, JoinForm::Initial]);
    assert_ne!(joined_form('\u{06CF}', JoinForm::Final), '\u{06CF}');
    assert_ne!(joined_form('\u{08A9}', JoinForm::Initial), '\u{08A9}');
}

/// REQ-84: every joining type before and after every other. In visual
/// order the right cell is read first; it joins the left cell when it
/// joins its next letter (dual-joining or join-causing) and the left cell
/// joins its previous (dual-joining, right-joining or join-causing).
#[test]
fn every_joining_type_beside_every_other() {
    // Beh dual, alef right, hamza none, tatweel causing.
    let letters =
        [('\u{0628}', true, true), ('\u{0627}', true, false), ('\u{0621}', false, false), ('\u{0640}', true, true)];
    for (left, left_joins_previous, _) in letters {
        for (right, _, right_joins_next) in letters {
            let mut cells = row(&[left, right].iter().collect::<std::string::String>());
            join_row(&mut cells);
            let joined = left_joins_previous && right_joins_next;
            let (left_form, right_form) =
                if joined { (JoinForm::Final, JoinForm::Initial) } else { (JoinForm::Isolated, JoinForm::Isolated) };
            assert_eq!(forms(&cells, 2), [left_form, right_form], "{left} after {right}");
        }
    }
}

/// REQ-84: joining breaks at a mosaic, at a cell a control code leaves
/// empty, and at a letter of another group 1001 set.
#[test]
fn joining_breaks_at_mosaics_spaces_and_other_sets() {
    let mosaic = Glyph::Mosaic { pattern: 0b11_1111, separated: false };
    for between in [mosaic, Glyph::Space] {
        let mut cells = row("\u{0628}\u{0628}\u{0628}");
        cells[1].glyph = between;
        join_row(&mut cells);
        assert_eq!(forms(&cells, 3), [JoinForm::Isolated; 3], "{between:?}");
    }
    let urdu = CharSet::G0(G0 { set: G0Set::Urdu, national: None });
    let mut cells = row("\u{0628}\u{0628}");
    let code = (0x20..0x7F).find(|&code| to_unicode(CharRef::new(urdu, code)).base == '\u{0628}').unwrap();
    cells[1].glyph = Glyph::Char(CharRef::new(urdu, code));
    join_row(&mut cells);
    assert_eq!(forms(&cells, 2), [JoinForm::Isolated; 2], "a Persian beh beside an Urdu beh");
}
