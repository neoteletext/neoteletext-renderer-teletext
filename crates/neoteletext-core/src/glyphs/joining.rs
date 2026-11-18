//! The joining data of the plain Arabic-script sets of group 1001 for the
//! picture renderers (renderer parity PAR-3, REQ-6762 item 2): for each
//! code, how it joins and the glyphs of its four forms from a glyph set, so
//! that a ReGIS or NAPLPS renderer, which links no engine crate, joins the
//! letters of a run as the teletext renderer joins a row (REQ-CHR-015).

use super::{GlyphRows, GlyphSet};
use crate::charset::{CharRef, CharSet, G0, JoinForm, JoiningType, joining_type, to_unicode};
use crate::consts::SPACE;
use crate::glyphs::conformance::{FONT_SETS, SET_POSITIONS};

/// How a code of a group 1001 set joins its neighbours, in the four
/// classes the picture renderers know (Unicode `ArabicShaping.txt` as
/// REQ-CHR-015 uses it).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum JoiningClass {
    /// Joins neither neighbour: a space, a digit, punctuation.
    #[default]
    None,
    /// Joins only the letter before it in reading order.
    Right,
    /// Joins both neighbours. The tatweel, which Unicode calls
    /// join-causing, is given as dual-joining: in a run of one set's codes
    /// it joins and lets its neighbours join exactly as a dual-joining
    /// letter does.
    Dual,
    /// A combining mark: takes no part in joining and keeps its own glyph.
    Transparent,
}

/// One code's joining data: its class and the glyph of each form. A form
/// the letter does not take is its isolated glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Joining {
    /// How it joins.
    pub class: JoiningClass,
    /// Its glyph joining neither neighbour, the set's own glyph.
    pub isolated: GlyphRows,
    /// Its glyph joining only the letter before it.
    pub final_form: GlyphRows,
    /// Its glyph joining only the letter after it.
    pub initial: GlyphRows,
    /// Its glyph joining both.
    pub medial: GlyphRows,
}

/// The Arabic combining marks a set may hold as a code of its own
/// (Unicode general category Mn in the Arabic block, joining type T).
const ARABIC_MARKS: [core::ops::RangeInclusive<char>; 4] =
    ['\u{0610}'..='\u{061A}', '\u{064B}'..='\u{065F}', '\u{0670}'..='\u{0670}', '\u{06D6}'..='\u{06ED}'];

/// The joining data of `code` (0x20 to 0x7F) of set `set` (its place in
/// [`FONT_SETS`]), drawn from `glyphs`; `None` when the set is not one of
/// group 1001 or the code is outside the set.
#[must_use]
pub fn joining_in(glyphs: &GlyphSet<'_>, set: u8, code: u8) -> Option<Joining> {
    let (_, charset) = FONT_SETS.get(usize::from(set))?;
    let CharSet::G0(G0 { set: g0, .. }) = *charset else { return None };
    if !g0.is_joined() || !(SPACE..SPACE.saturating_add(SET_POSITIONS)).contains(&code) {
        return None;
    }
    let character = CharRef::new(*charset, code);
    let base = to_unicode(character).base;
    let class = match joining_type(base) {
        Some(JoiningType::Dual | JoiningType::Causing) => JoiningClass::Dual,
        Some(JoiningType::Right) => JoiningClass::Right,
        None if ARABIC_MARKS.iter().any(|marks| marks.contains(&base)) => JoiningClass::Transparent,
        Some(JoiningType::None) | None => JoiningClass::None,
    };
    let form = |form| glyphs.glyph(CharRef { form, ..character });
    Some(Joining {
        class,
        isolated: form(JoinForm::Isolated),
        final_form: form(JoinForm::Final),
        initial: form(JoinForm::Initial),
        medial: form(JoinForm::Medial),
    })
}

/// [`joining_in`] from the compiled Noto Teletext glyph table, the forms
/// the teletext renderer draws (REQ-FNT-001).
#[must_use]
pub fn joining(set: u8, code: u8) -> Option<Joining> {
    joining_in(&GlyphSet::noto_teletext(), set, code)
}

/// The places in [`FONT_SETS`] of the group 1001 sets, 44 to 51: the sets a
/// picture renderer is given joining data for.
pub const JOINED_SETS: core::ops::RangeInclusive<u8> = 44..=51;

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "a test panics on what it rules out")]
    use super::*;
    use crate::glyphs::conformance::FONT_SET_COUNT;

    // PAR-3: a group 1001 letter's forms are the glyphs the teletext
    // renderer draws for them; other sets have no joining data.
    #[test]
    fn the_plain_arabic_sets_give_their_forms_and_no_other_set_does() {
        // JOINED_SETS is the group 1001 sets and no other.
        for set in 0..u8::try_from(FONT_SET_COUNT).unwrap() {
            let joined = matches!(FONT_SETS[usize::from(set)].1, CharSet::G0(G0 { set, .. }) if set.is_joined());
            assert_eq!(joined, JOINED_SETS.contains(&set), "set {set}");
            assert_eq!(joining(set, 0x41).is_some(), joined, "set {set}");
        }
        assert!(joining(44, 0x1F).is_none() && joining(44, 0x80).is_none());
        let space = joining(44, SPACE).unwrap();
        assert_eq!(space.class, JoiningClass::None);
        // The dual-joining letters of set 44 have forms drawn apart from
        // their isolated glyphs, and each form is the renderer's.
        let mut shaped = 0;
        for code in SPACE..=0x7F {
            let data = joining(44, code).unwrap();
            if data.class == JoiningClass::Dual {
                shaped += usize::from(data.initial != data.isolated);
                let charset = FONT_SETS[44].1;
                let drawn =
                    GlyphSet::noto_teletext().glyph(CharRef { form: JoinForm::Medial, ..CharRef::new(charset, code) });
                assert_eq!(data.medial, drawn);
            }
        }
        assert!(shaped > 10, "{shaped}");
    }
}
