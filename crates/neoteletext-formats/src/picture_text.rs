//! The picture text encoder (renderer parity PAR-3 to PAR-6, REQ-6762):
//! a program's Unicode text written into a ReGIS or NAPLPS picture in the
//! 55 provided character sets, the only means of text in another script a
//! picture has (the owner's DEC-6669).
//!
//! [`stretches`] chooses the sets exactly as the teletext encoder chooses
//! them for a run written for NeoTeletext's own receivers (REQ-2214 item 5,
//! TODO-2343, DEC-2473), by calling into it: the language's own set, Latin
//! English for what it cannot show, right-to-left runs in visual order,
//! the letters of EN 300 706 Table 44 in their joined forms, and the plain
//! Arabic-script letters of group 1001 in their base forms, which the
//! picture renderers join (PAR-13, PAR-14). [`regis()`] and [`naplps()`] write
//! the stretches as each language's bytes, and [`measure`] says what they
//! cost.
//!
//! A set's number is its place in the engine's `FONT_SETS`
//! (`neoteletext_core::glyphs::conformance`): the ReGIS renderer's provided
//! alphabet `100 + n` and the NAPLPS renderer's private set `n` (REQ-6668).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use neoteletext_core::charset::{CharSet, G2Set, page_mark};
use neoteletext_core::glyphs::conformance::{FONT_SET_COUNT, font_set_of};

use crate::write_text::sets::Cell;
use crate::write_text::{LATIN_ENGLISH_TAG, RunFault, stretch_sets, visual_characters, visual_stretches};

/// The most characters of one run of picture text (PAR-3).
pub const MAX_PART_CHARACTERS: usize = 1_000;
/// The most stretches one run gives: a stretch holds at least one
/// character (PAR-3).
pub const MAX_STRETCHES: usize = MAX_PART_CHARACTERS;
/// The most bytes of one picture's stream that a static channel file
/// carries (PAR-8) or the server keeps (PAR-17): defined here because both
/// the engine's tools and the server link this crate, so a static picture
/// is never larger than a live one may be.
pub const MAX_PICTURE_STREAM_BYTES: usize = 1_048_576;

/// The first code position of a set, 2/0.
const FIRST_CODE: u8 = 0x20;
/// The last code position of a set, 7/F: the block of the Latin sets.
const LAST_CODE: u8 = 0x7F;
/// Column 4 of a G2 set, which holds the diacritical marks: mark `n` of a
/// "G0 character with diacritical mark" triplet is the G2 set's 4/n (EN 300
/// 706 Table 29 and clause 15.6.3, REQ-CHR-014).
const MARK_COLUMN: u8 = 0x40;
/// The characters a character no set shows becomes, the first the run's
/// sets show: the teletext encoder's stand-ins for text from outside the
/// program (DEC-2326; the server's `text.rs` uses the same three).
pub const STAND_INS: [char; 3] = ['?', '-', '.'];

/// One stretch of a run: codes of one provided set, in visual order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Stretch {
    /// The set's number, 0 to 54: its place in the engine's `FONT_SETS`.
    pub set: u8,
    /// Its code positions, 0x20 to 0x7F.
    pub codes: Vec<u8>,
    /// For each letter written with a diacritical mark its set has no
    /// precomposed glyph for: the letter's index in `codes`, the G2 set's
    /// number and the mark's code in it.
    pub marks: Vec<(usize, u8, u8)>,
    /// How many of `codes` are stand-ins for characters no set of the
    /// language shows, written so because the text came from outside the
    /// program (PAR-3, DEC-2326).
    pub stand_ins: u32,
}

/// Why text could not be written into a picture.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum PictureTextError {
    /// No set of the language nor Latin English shows the character at
    /// this index (counted in characters of the text).
    Unshown {
        /// The character's index.
        index: usize,
    },
    /// A stretch cannot be written in this picture language: its set is
    /// not one of the 55, a code is outside 0x20 to 0x7F, or (ReGIS) it
    /// holds the code 0x7F, which ReGIS strings cannot carry. The
    /// stretch's index.
    Unwritable {
        /// The stretch's index.
        index: usize,
    },
    /// The text has more than [`MAX_PART_CHARACTERS`] characters: how many.
    TooLong {
        /// Its characters.
        characters: usize,
    },
    /// A control character or a bidirectional formatting character at this
    /// index; picture text carries neither (PAR-3, PAR-21).
    Forbidden {
        /// The character's index.
        index: usize,
    },
    /// The language is not a tag of the core's table of languages.
    UnknownLanguage(String),
}

impl core::fmt::Display for PictureTextError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unshown { index } => write!(formatter, "no provided set of the language shows character {index}"),
            Self::Unwritable { index } => write!(formatter, "stretch {index} cannot be written in this language"),
            Self::TooLong { characters } => {
                write!(formatter, "{characters} characters; a text part has at most {MAX_PART_CHARACTERS}")
            }
            Self::Forbidden { index } => {
                write!(formatter, "character {index} is a control or bidirectional formatting character")
            }
            Self::UnknownLanguage(tag) => write!(formatter, "no set writes the language {tag:?}"),
        }
    }
}

impl core::error::Error for PictureTextError {}

/// What a run of picture text costs (PAR-6).
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Measured {
    /// The character cells it advances; a mark takes none.
    pub cells: u32,
    /// The bytes [`regis()`] writes for it.
    pub regis_bytes: u32,
    /// The bytes [`naplps()`] writes for it.
    pub naplps_bytes: u32,
    /// The sets it uses, in order of first use.
    pub sets: Vec<u8>,
    /// The characters that became stand-ins.
    pub stand_ins: u32,
}

/// The bidirectional formatting characters (Unicode's general category Cf
/// of UAX #9 section 2): marks, embeddings, overrides and isolates.
const fn is_bidi_format(character: char) -> bool {
    matches!(character, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A combining diacritical mark of the block the teletext marks come from
/// (U+0300 to U+036F): a letter followed by one takes one cell.
const fn is_combining(character: char) -> bool {
    matches!(character, '\u{0300}'..='\u{036F}')
}

/// A set's number, as a `u8`; every place in `FONT_SETS` fits.
fn set_number(set: CharSet) -> Option<u8> {
    font_set_of(set).and_then(|place| u8::try_from(place).ok())
}

/// One run's `text` in its `language` (a BCP 47 tag of the core's table of
/// languages; none for Latin English) as stretches of the provided sets, in
/// visual order (PAR-3). The text is in Unicode normalisation form C, which
/// the caller checks. With `outside`, a character no set shows becomes the
/// first of [`STAND_INS`] the run's sets show.
///
/// # Errors
///
/// [`PictureTextError::TooLong`], [`PictureTextError::Forbidden`],
/// [`PictureTextError::UnknownLanguage`], and without `outside`
/// [`PictureTextError::Unshown`].
pub fn stretches(text: &str, language: Option<&str>, outside: bool) -> Result<Vec<Stretch>, PictureTextError> {
    let characters = text.chars().count();
    if characters > MAX_PART_CHARACTERS {
        return Err(PictureTextError::TooLong { characters });
    }
    if let Some(index) = text.chars().position(|character| character.is_control() || is_bidi_format(character)) {
        return Err(PictureTextError::Forbidden { index });
    }
    let tag = language.unwrap_or(LATIN_ENGLISH_TAG);
    let unknown = |fault: RunFault| match fault {
        RunFault::UnknownLanguage(_) => PictureTextError::UnknownLanguage(tag.to_string()),
        _ => PictureTextError::Unshown { index: 0 },
    };
    // A picture is drawn only by NeoTeletext's own receivers, so its text
    // is chosen as a run for them is, in a run of its own set.
    let own = true;
    let (value, mut visual) = visual_characters(text, tag, 0, own).map_err(unknown)?;
    let mut stood: Vec<bool> = alloc::vec![false; visual.len()];
    // Each pass replaces one character no set shows; the visual text holds
    // at most MAX_PART_CHARACTERS distinct characters.
    let encoded = loop {
        match visual_stretches(&visual, value, value, own) {
            Ok(encoded) => break encoded,
            Err(RunFault::Unshown(character)) => {
                if !outside {
                    let index = text.chars().position(|known| known == character).unwrap_or_default();
                    return Err(PictureTextError::Unshown { index });
                }
                let shows = |candidate: char| visual_stretches(&[candidate], value, value, own).is_ok();
                let Some(stand_in) = STAND_INS.into_iter().find(|&candidate| shows(candidate)) else {
                    let index = text.chars().position(|known| known == character).unwrap_or_default();
                    return Err(PictureTextError::Unshown { index });
                };
                for (slot, stands) in visual.iter_mut().zip(stood.iter_mut()) {
                    if *slot == character {
                        *slot = stand_in;
                        *stands = true;
                    }
                }
            }
            Err(fault) => return Err(unknown(fault)),
        }
    };
    let mut out: Vec<Stretch> = Vec::new();
    let mut at = 0_usize;
    for (stretch_value, cells) in encoded {
        let (g0, g2) = stretch_sets(stretch_value, value, own).ok_or_else(|| unknown(RunFault::OutOfRows))?;
        let own_set = set_number(g0).ok_or_else(|| unknown(RunFault::OutOfRows))?;
        let g2_set = match g2 {
            CharSet::G2(set) => set,
            CharSet::G0(_) | CharSet::G3 => G2Set::Latin,
        };
        for cell in cells {
            let taken = match cell {
                Cell::Marked { .. } if visual.get(at.saturating_add(1)).copied().is_some_and(is_combining) => 2,
                _ => 1,
            };
            let stand_in = stood.get(at).copied().unwrap_or(false);
            at = at.saturating_add(taken);
            let (set, code, mark) = match cell {
                Cell::Code(code) => (own_set, code, None),
                Cell::Marked { code, mark } => {
                    // The mark is Latin G2's, but for the Vietnamese set's
                    // hook above, as the teletext encoder places it.
                    let (mark_set, mark) = if g2_set == G2Set::Vietnamese {
                        (CharSet::G2(G2Set::Vietnamese), mark)
                    } else {
                        (CharSet::G2(G2Set::Latin), page_mark(g2_set, mark))
                    };
                    let mark_set = set_number(mark_set).ok_or_else(|| unknown(RunFault::OutOfRows))?;
                    (own_set, code, Some((mark_set, MARK_COLUMN | mark)))
                }
                Cell::G2 { code, .. } => (set_number(g2).ok_or_else(|| unknown(RunFault::OutOfRows))?, code, None),
                Cell::Run { code, value } => {
                    let (run_set, _) = stretch_sets(value, value, own).ok_or_else(|| unknown(RunFault::OutOfRows))?;
                    (set_number(run_set).ok_or_else(|| unknown(RunFault::OutOfRows))?, code, None)
                }
            };
            if out.last().is_none_or(|last| last.set != set) {
                out.push(Stretch { set, ..Stretch::default() });
            }
            if let Some(last) = out.last_mut() {
                if let Some((mark_set, mark_code)) = mark {
                    last.marks.push((last.codes.len(), mark_set, mark_code));
                }
                last.codes.push(code);
                last.stand_ins = last.stand_ins.saturating_add(u32::from(stand_in));
            }
        }
    }
    Ok(out)
}

// The ReGIS form (PAR-4): the manual's chapter 7 ("Select Character Set",
// the text control characters) and chapter 5 ("Temporary Write Controls").

/// The text command selecting an alphabet, before its number.
const REGIS_SELECT: &[u8] = b"T(A";
/// The end of an option.
const REGIS_CLOSE: u8 = b')';
/// A quoted string's delimiter; one inside a string is written twice (the
/// manual's chapter 1).
const REGIS_QUOTE: u8 = b'"';
/// Between two quoted strings of one stretch: a space, so that the second
/// string's opening quote does not read as a doubled quote of the first.
const REGIS_STRING_GAP: u8 = b' ';
/// A text command with the temporary overlay write control, so that a mark
/// keeps the letter beneath it whatever writing mode is in force.
const REGIS_OVERLAY_TEXT: &[u8] = b"T(W(V))";
/// BS: moves the text position back one character cell (chapter 7, text
/// control characters).
const REGIS_BACKSPACE: u8 = 0x08;
/// Alphabet 0, the built-in set, selected after the last stretch.
const REGIS_ALPHABET_ZERO: &[u8] = b"T(A0)";

/// The bytes of `T(A154)`, the longest alphabet selection.
const REGIS_SELECT_BYTES: usize = REGIS_SELECT.len() + "154".len() + 1;
/// A character's own bytes: opened and closed quotes, and the character
/// written twice when it is a quote.
const REGIS_CHARACTER_BYTES: usize = 2 + 2;
/// A mark's bytes: the string closed, the G2 alphabet selected, the overlay
/// text command, then a quoted BS and the mark's code (PAR-4).
const REGIS_MARK_BYTES: usize = 1 + REGIS_SELECT_BYTES + REGIS_OVERLAY_TEXT.len() + 4;
/// The most bytes one character's writing takes (PAR-4): a selection of its
/// alphabet, the character and a mark; a string split takes at most one
/// gap and two quotes, which a character's quotes already count.
pub const MAX_REGIS_BYTES_PER_CHARACTER: usize = 32;
const _: () = assert!(REGIS_SELECT_BYTES + REGIS_CHARACTER_BYTES + REGIS_MARK_BYTES <= MAX_REGIS_BYTES_PER_CHARACTER);
const _: () = assert!(REGIS_MARK_BYTES == 19, "PAR-4: a mark takes at most 19 bytes");
/// The most bytes [`regis()`] writes for one run of [`stretches`].
pub const MAX_REGIS_PART_BYTES: usize = MAX_PART_CHARACTERS * MAX_REGIS_BYTES_PER_CHARACTER;
const _: () = assert!(MAX_REGIS_PART_BYTES == 32_000);
// The whole run within its bound: every character at its most, a gap and
// two quotes at each string split, and the closing selection of alphabet 0.
const _: () = assert!(
    MAX_PART_CHARACTERS * (REGIS_SELECT_BYTES + REGIS_CHARACTER_BYTES + REGIS_MARK_BYTES)
        + (MAX_PART_CHARACTERS / regis::limits::MAX_STRING_BYTES + 1) * 3
        + REGIS_ALPHABET_ZERO.len()
        <= MAX_REGIS_PART_BYTES
);

/// Checks a stretch's set and codes for `last` (the last code it may hold).
fn checked(stretches: &[Stretch], last: u8) -> Result<(), PictureTextError> {
    for (index, stretch) in stretches.iter().enumerate() {
        let codes_fit = stretch.codes.iter().all(|code| (FIRST_CODE..=last).contains(code));
        let marks_fit = stretch.marks.iter().all(|&(at, set, code)| {
            at < stretch.codes.len() && usize::from(set) < FONT_SET_COUNT && (FIRST_CODE..=last).contains(&code)
        });
        if usize::from(stretch.set) >= FONT_SET_COUNT || !codes_fit || !marks_fit {
            return Err(PictureTextError::Unwritable { index });
        }
    }
    Ok(())
}

/// Appends the selection of the provided alphabet of `set`.
fn regis_select(set: u8, out: &mut Vec<u8>) {
    out.extend_from_slice(REGIS_SELECT);
    let alphabet = u16::from(regis::limits::FIRST_PROVIDED_ALPHABET).saturating_add(u16::from(set));
    out.extend_from_slice(alphabet.to_string().as_bytes());
    out.push(REGIS_CLOSE);
}

/// Appends `stretches` as ReGIS text commands (PAR-4): for each stretch its
/// alphabet `T(A100+set)`, its codes as quoted strings of at most
/// `regis::limits::MAX_STRING_BYTES` each, and each mark over its letter;
/// then `T(A0)`. It leaves alphabet 0 selected and the text position after
/// the last character, and changes no other text control, writing control
/// or position.
///
/// # Errors
///
/// [`PictureTextError::Unwritable`] for a stretch holding 0x7F, a code
/// outside 0x20 to 0x7E, or a set past the 55; nothing is appended.
pub fn regis(stretches: &[Stretch], out: &mut Vec<u8>) -> Result<(), PictureTextError> {
    checked(stretches, LAST_CODE.saturating_sub(1))?;
    for stretch in stretches {
        regis_select(stretch.set, out);
        let mut open = false;
        let mut in_string = 0_usize;
        for (index, &code) in stretch.codes.iter().enumerate() {
            if open && in_string == regis::limits::MAX_STRING_BYTES {
                out.push(REGIS_QUOTE);
                out.push(REGIS_STRING_GAP);
                open = false;
            }
            if !open {
                out.push(REGIS_QUOTE);
                open = true;
                in_string = 0;
            }
            out.push(code);
            if code == REGIS_QUOTE {
                out.push(code);
            }
            in_string = in_string.saturating_add(1);
            let marks = stretch.marks.iter().filter(|(at, ..)| *at == index);
            for &(_, mark_set, mark_code) in marks {
                out.push(REGIS_QUOTE);
                regis_select(mark_set, out);
                out.extend_from_slice(REGIS_OVERLAY_TEXT);
                out.extend_from_slice(&[REGIS_QUOTE, REGIS_BACKSPACE, mark_code, REGIS_QUOTE]);
                open = false;
                if index.saturating_add(1) < stretch.codes.len() {
                    regis_select(stretch.set, out);
                }
            }
        }
        if open {
            out.push(REGIS_QUOTE);
        }
    }
    out.extend_from_slice(REGIS_ALPHABET_ZERO);
    Ok(())
}

// The NAPLPS form (PAR-5): ANSI X3.110-1983 section 4.3.2 Table 1 and
// section 6.1.2.1; the private designations of REQ-6668.

/// ESC.
const ESC: u8 = 0x1B;
/// The intermediate designating a 94-character set into G0 (2/8).
const DESIGNATE_G0: u8 = 0x28;
/// The intermediate designating a 94-character set into G2 (2/10).
const DESIGNATE_G2: u8 = 0x2A;
/// The intermediate designating a 96-character set into G2 (2/14).
const DESIGNATE_G2_96: u8 = 0x2E;
/// The final byte of the primary set (4/2, Table 1).
const PRIMARY_FINAL: u8 = 0x42;
/// The final byte of the supplementary set (7/12, Table 1).
const SUPPLEMENTARY_FINAL: u8 = 0x7C;
/// SI, shift in: G0 invoked into the in-use table.
const SHIFT_IN: u8 = 0x0F;
/// SS2, single shift 2: the next character from G2.
const SINGLE_SHIFT_2: u8 = 0x19;
/// APB, active position backward, of the C0 set (section 6.1.2.1).
const ACTIVE_POSITION_BACKWARD: u8 = 0x08;

/// The most bytes of a designation: ESC, an intermediate, a supplement and
/// a final.
const NAPLPS_DESIGNATION_BYTES: usize = 4;
/// A stretch's opening: its designation into G0 and SI.
const NAPLPS_OPENING_BYTES: usize = NAPLPS_DESIGNATION_BYTES + 1;
/// A code 7/15: its set's 96-character designation into G2, SS2, the code,
/// and the supplementary set designated back (three bytes).
const NAPLPS_BLOCK_BYTES: usize = NAPLPS_DESIGNATION_BYTES + 1 + 1 + 3;
/// A mark: APB, the G2 set's 96-character designation, SS2, the code, and
/// the supplementary set designated back.
const NAPLPS_MARK_BYTES: usize = 1 + NAPLPS_DESIGNATION_BYTES + 1 + 1 + 3;
/// The primary set designated back into G0 after the last stretch.
const NAPLPS_CLOSING_BYTES: usize = 3;
/// The most bytes one character's writing takes (PAR-5).
pub const MAX_NAPLPS_BYTES_PER_CHARACTER: usize = 32;
const _: () = assert!(NAPLPS_MARK_BYTES <= 13, "PAR-5: a mark takes at most 13 bytes");
const _: () = assert!(
    NAPLPS_OPENING_BYTES + NAPLPS_BLOCK_BYTES + NAPLPS_MARK_BYTES + NAPLPS_CLOSING_BYTES
        <= MAX_NAPLPS_BYTES_PER_CHARACTER
);
/// The most bytes [`naplps()`] writes for one run of [`stretches`].
pub const MAX_NAPLPS_PART_BYTES: usize = MAX_PART_CHARACTERS * MAX_NAPLPS_BYTES_PER_CHARACTER;
const _: () = assert!(MAX_NAPLPS_PART_BYTES == 32_000);

/// Appends the designation of set `set` with the intermediate `slot`.
fn designate(slot: u8, set: u8, out: &mut Vec<u8>) {
    // `checked` has bounded every set to the 55, each of which has a
    // designation.
    let Some((supplement, final_byte)) = naplps::teletext::designation_of(set) else { return };
    out.push(ESC);
    out.push(slot);
    out.extend(supplement);
    out.push(final_byte);
}

/// Appends `code` of set `set` through G2 by SS2, then the supplementary
/// set designated back into G2.
fn through_g2(set: u8, code: u8, out: &mut Vec<u8>) {
    designate(DESIGNATE_G2_96, set, out);
    out.extend_from_slice(&[SINGLE_SHIFT_2, code, ESC, DESIGNATE_G2, SUPPLEMENTARY_FINAL]);
}

/// Appends `stretches` as NAPLPS text (PAR-5): for each stretch its set's
/// designation into G0 and SI, then its codes, a code 7/15 through G2 in
/// the set's 96-character form, and each mark after its letter as APB and
/// the mark through G2; then the primary set designated back into G0. It
/// leaves G0, holding the primary set, invoked in the in-use table, and G2
/// holding the supplementary set; a page that draws with picture
/// description instructions after it sends SO again.
///
/// # Errors
///
/// [`PictureTextError::Unwritable`] for a code outside 0x20 to 0x7F or a
/// set past the 55; nothing is appended.
pub fn naplps(stretches: &[Stretch], out: &mut Vec<u8>) -> Result<(), PictureTextError> {
    checked(stretches, LAST_CODE)?;
    for stretch in stretches {
        designate(DESIGNATE_G0, stretch.set, out);
        out.push(SHIFT_IN);
        for (index, &code) in stretch.codes.iter().enumerate() {
            if code == LAST_CODE {
                through_g2(stretch.set, code, out);
            } else {
                out.push(code);
            }
            for &(_, mark_set, mark_code) in stretch.marks.iter().filter(|(at, ..)| *at == index) {
                out.push(ACTIVE_POSITION_BACKWARD);
                through_g2(mark_set, mark_code, out);
            }
        }
    }
    out.extend_from_slice(&[ESC, DESIGNATE_G0, PRIMARY_FINAL]);
    Ok(())
}

/// What `stretches` cost (PAR-6): the cells they advance, the bytes of each
/// language, the sets in order of first use and the stand-ins. Both picture
/// languages space text by whole cells, so a run's width is `cells` times
/// the cell width the program set. A stretch a language cannot write counts
/// no bytes of it.
#[must_use]
pub fn measure(stretches: &[Stretch]) -> Measured {
    let mut measured = Measured::default();
    let used = |set: u8, sets: &mut Vec<u8>| {
        if !sets.contains(&set) {
            sets.push(set);
        }
    };
    for stretch in stretches {
        used(stretch.set, &mut measured.sets);
        for &(_, mark_set, _) in &stretch.marks {
            used(mark_set, &mut measured.sets);
        }
        measured.cells = measured.cells.saturating_add(u32::try_from(stretch.codes.len()).unwrap_or(u32::MAX));
        measured.stand_ins = measured.stand_ins.saturating_add(stretch.stand_ins);
    }
    let length = |bytes: &Vec<u8>| u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    let mut bytes = Vec::new();
    if regis(stretches, &mut bytes).is_ok() {
        measured.regis_bytes = length(&bytes);
    }
    bytes.clear();
    if naplps(stretches, &mut bytes).is_ok() {
        measured.naplps_bytes = length(&bytes);
    }
    measured
}

// The provided sets are the engine's character sets in both renderers.
const _: () = assert!(regis::limits::PROVIDED_SETS as usize == FONT_SET_COUNT);
const _: () = assert!(naplps::TELETEXT_SET_COUNT == FONT_SET_COUNT);
const _: () = assert!(MAX_STRETCHES == MAX_PART_CHARACTERS);

#[cfg(test)]
mod tests;
