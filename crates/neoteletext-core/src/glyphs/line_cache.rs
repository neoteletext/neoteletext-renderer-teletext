//! The glyph line cache of REQ-392 (REQ-RND-026, decision E.29 item 4 in
//! DEC-553): the twenty lines of every character of one G0 set, found once
//! through the compiled font, so that drawing a line of a cell of that set
//! is a table lookup. Without it each line of a text cell looks its glyph
//! up in the font again, which measured about thirty times the cost of a
//! cell whose lines are already known (TODO-2068).

use super::{GlyphRows, GlyphSet};
use crate::charset::{CharRef, CharSet, JoinForm};
use crate::consts::{CELL_HEIGHT, CODE_MASK_7BIT, DIACRITIC_NONE, SPACE};

/// The first character code a cache holds, 2/0 (the space).
const FIRST_CODE: u8 = SPACE;
/// Characters of one set: codes 2/0 to 7/F (EN 300 706 clause 12.1,
/// Table 25: each G0 set has address range 2/0 to 7/F).
const CODES: usize = (CODE_MASK_7BIT - FIRST_CODE) as usize + 1;

/// The lines of every character of one G0 set, in memory the caller
/// provides (no allocation): 96 characters of 20 lines of 16 bits, 3,840
/// bytes. A port keeps one, fills it with [`GlyphLineCache::fill`] for the
/// set its pages are designated in (and again when that changes), and hands
/// it to the renderer with [`GlyphSet::with_line_cache`], whose
/// [`GlyphSet::glyph`] then answers from it.
///
/// A character it holds is a character of its set with no diacritical mark
/// and the isolated form; every other character, and every G2 and G3
/// character, is drawn through the font as before. The cell raster stays the
/// reference: an entry is exactly the glyph lines the raster would look up.
#[derive(Clone, Debug)]
pub struct GlyphLineCache {
    /// The set it holds, or `None` while empty.
    set: Option<CharSet>,
    /// The lines of codes 2/0 to 7/F, in code order.
    rows: [GlyphRows; CODES],
}

impl Default for GlyphLineCache {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl GlyphLineCache {
    /// A cache that holds no set: every character is drawn through the
    /// font.
    pub const EMPTY: Self = Self { set: None, rows: [[0; CELL_HEIGHT]; CODES] };

    /// Fills the cache with the lines of every character of `set` from
    /// `glyphs`, the glyph set it will be used with: the lines are those
    /// [`GlyphSet::glyph`] gives, so a cached cell draws as it would
    /// uncached.
    pub fn fill(&mut self, glyphs: &GlyphSet<'_>, set: CharSet) {
        // Through the font itself, never through a cache `glyphs` carries.
        let glyphs = GlyphSet { line_cache: None, ..*glyphs };
        for (code, rows) in (FIRST_CODE..=CODE_MASK_7BIT).zip(self.rows.iter_mut()) {
            *rows = glyphs.glyph(CharRef::new(set, code));
        }
        self.set = Some(set);
    }

    /// The set it holds, if any.
    #[must_use]
    pub const fn set(&self) -> Option<CharSet> {
        self.set
    }

    /// The lines of `character`, when the cache holds it.
    #[must_use]
    pub fn rows(&self, character: CharRef) -> Option<&GlyphRows> {
        let held = matches!(self.set, Some(set) if set == character.set)
            && character.diacritic == DIACRITIC_NONE
            && character.form == JoinForm::Isolated;
        if !held {
            return None;
        }
        self.rows.get(usize::from(character.code.checked_sub(FIRST_CODE)?))
    }
}
