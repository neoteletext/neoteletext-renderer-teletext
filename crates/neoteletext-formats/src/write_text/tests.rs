#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use super::rtl::break_line_rtl;
use super::sets::{Cell, encode, page_sets, set_value};
use super::*;
use neoteletext_core::charset::{CharRef, compose, default_second_g0, page_mark, to_unicode};
use neoteletext_core::consts::{DESIGNATION_GROUP_SHIFT, ESC, SPACE};

/// The value and sets of the set named `name`, for a page for
/// NeoTeletext's own receivers.
fn named(name: &str) -> (u8, Sets) {
    let value = set_value(name).unwrap();
    (value, page_sets(value, true).unwrap())
}

/// Writes `text` in the set named `name` for NeoTeletext's own receivers.
fn write(name: &str, mpp: u16, text: &str) -> Result<Written, WriteError> {
    write_text(Choice::Set(name), Level::NeoTeletext, mpp, text)
}

/// What a decoder shows for cells written in `name`'s sets, ESC as a
/// space (REQ-CHR-009): codes through the set in force, marks composed
/// and G2 characters from the page's G2 set.
fn shown(name: &str, cells: &[Cell]) -> String {
    let (_, sets) = named(name);
    let mut in_second = false;
    let mut run = sets.default;
    cells
        .iter()
        .map(|cell| match *cell {
            Cell::Code(ESC) => {
                in_second = !in_second;
                ' '
            }
            Cell::Code(code) => {
                let set = if in_second { sets.second.unwrap() } else { run };
                to_unicode(CharRef::new(set, code)).base
            }
            Cell::Run { code, value } => {
                run = CharSet::G0(neoteletext_core::charset::designation(value).unwrap().g0);
                in_second = false;
                to_unicode(CharRef::new(run, code)).base
            }
            Cell::Marked { code, mark } => {
                let placed = CharRef { diacritic: page_mark(sets.g2, mark), ..CharRef::new(sets.base, code) };
                let unicode = to_unicode(placed);
                compose(unicode.base, unicode.combining.unwrap()).unwrap()
            }
            Cell::G2 { code, .. } => to_unicode(CharRef::new(CharSet::G2(sets.g2), code)).base,
        })
        .collect()
}

fn cells(name: &str, text: &str) -> Vec<Cell> {
    let (_, sets) = named(name);
    let characters: Vec<char> = text.chars().collect();
    encode(&sets, &characters, false, 1).unwrap().0
}

#[test]
fn every_set_name_designates_its_set() {
    for (name, group, option) in SETS {
        let (value, sets) = named(name);
        assert_eq!(value, (group << DESIGNATION_GROUP_SHIFT) | option);
        // NeoTeletext's own sets imply their second set; the writer names
        // Latin English for the standard's Arabic set of Table 44
        // (REQ-2190); no other standard set has one.
        let own = default_second_g0(value).is_some() || name == "arabic-table44";
        assert_eq!(sets.second.is_some(), own, "{name}: only neoteletext's own sets and Table 44 have a second set");
    }
    assert_eq!(write("klingon", 0x100, "a").unwrap_err(), WriteError::UnknownSet("klingon".into()));
    // DEC-2239, DEC-2240, DEC-2244: the withdrawn sets' names write nothing.
    for withdrawn in ["thai", "lao", "yoruba-igbo"] {
        assert_eq!(write(withdrawn, 0x100, "a").unwrap_err(), WriteError::UnknownSet(withdrawn.into()));
    }
}

/// Each set writes a sample and reads it back unchanged.
#[test]
fn samples_round_trip() {
    let samples = [
        ("vietnamese", "Tiếng Việt có dấu, Hà Nội và Đà Nẵng."),
        ("igbo", "Ndị Igbo na-asụ Igbo, ọ dị mma"),
        ("hausa-fula", "Ɓarna ƙasa ɗaya Ƴan"),
        ("ewe", "Eʋegbe ɖe Ŋkɔ"),
        ("azerbaijani", "Azərbaycan dili Ğ İ ı"),
        ("ukrainian", "Україна, ґанок і Київ"),
        ("serbian", "Србија, Ђорђе и Љубица"),
        ("macedonian", "Македонија Ѓ Ќ Ѕ"),
        ("georgian", "საქართველო"),
        ("armenian", "Հայաստան, Երևան"),
        ("tifinagh", "ⵜⴰⵎⴰⵣⵉⵖⵜ"),
    ];
    for (name, text) in samples {
        let (_, sets) = named(name);
        let characters: Vec<char> = text.chars().collect();
        let (written, _) = encode(&sets, &characters, false, 1).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(shown(name, &written), text, "{name}");
    }
}

/// Vietnamese tones go in X/26, the letter in the row.
#[test]
fn a_tone_is_a_triplet_over_its_letter() {
    let written = cells("vietnamese", "ệ");
    let [Cell::Marked { code, mark }] = written[..] else { panic!("{written:?}") };
    assert_eq!(to_unicode(CharRef::new(named("vietnamese").1.base, code)).base, 'ê');
    assert_eq!(mark, 0x9, "dot below");
    let [Cell::Marked { mark, .. }] = cells("vietnamese", "ả")[..] else { panic!() };
    assert_eq!(mark, 0xC, "the hook above is 4/C of the Vietnamese G2 set");
}

// REQ-95: a letter only a G2 set holds is written as an X/26 character
// over a stand-in in the row, which a Level 1 receiver shows instead. The
// stand-ins are the writer's own choice (EN 300 706 leaves the row's byte
// to the broadcaster): the plain letter for the ten listed in REQ-95, and
// a space for any other G2 character.
#[test]
fn a_g2_character_stands_on_its_plain_letter_or_a_space() {
    let stand_in_of = |name: &str, text: &str| {
        let written = cells(name, text);
        let [Cell::G2 { stand_in, .. }] = written[..] else { panic!("{text}: {written:?}") };
        stand_in
    };
    assert_eq!(stand_in_of("english", "Ø"), b'O');
    assert_eq!(stand_in_of("english", "ł"), b'l');
    assert_eq!(stand_in_of("english", "ß"), b's');
    assert_eq!(stand_in_of("english", "©"), SPACE, "a G2 character with no plain letter stands on a space");
}

/// Latin text on a Cyrillic page is switched with ESC, and each ESC
/// takes a cell.
#[test]
fn latin_text_is_switched_with_esc() {
    let written = cells("ukrainian", "Київ BBC");
    assert_eq!(written.iter().filter(|cell| **cell == Cell::Code(ESC)).count(), 1);
    assert_eq!(written.len(), "Київ BBC".chars().count(), "the ESC takes the space's cell");
    assert_eq!(cells("ukrainian", "BBC").len(), "BBC".len() + 1, "without a space the ESC takes a cell");
}

/// A combining mark after a letter is written as that letter with the
/// mark by X/26: Igbo ọ̀ is ọ in the row and a grave. A letter with
/// two marks needs two in one cell, which X/26 cannot place: the
/// second is refused.
#[test]
fn a_combining_mark_marks_the_letter_before_it() {
    let written = cells("igbo", "ọ\u{0300}");
    let [Cell::Marked { code, mark: 1 }] = written[..] else { panic!("{written:?}") };
    assert_eq!(to_unicode(CharRef::new(named("igbo").1.base, code)).base, 'ọ');
    let error = write("igbo", 0x100, "ọ\u{0300}\u{0301}").unwrap_err();
    assert_eq!(error, WriteError::Missing { character: '\u{0301}', line: 1 });
}

#[test]
fn a_missing_character_is_refused_with_its_line() {
    let error = write("georgian", 0x100, "საქართველო\nok ☃").unwrap_err();
    assert_eq!(error, WriteError::Missing { character: '☃', line: 2 });
}

#[test]
fn lines_break_at_the_row_width_and_pages_end_at_row_23() {
    let word = "ა".repeat(10);
    let line = [word.as_str(); 8].join(" ");
    let written = write("georgian", 0x100, &line).unwrap();
    // Three words of ten cells and two spaces take 32 cells; a fourth
    // with its space would take 43.
    assert_eq!(written.rows, 3);
    let many = "ა\n".repeat(24);
    assert_eq!(write("georgian", 0x100, &many).unwrap_err(), WriteError::TooManyRows(24));
}

/// The cells of a right-to-left row as shown, left to right, without
/// the right-aligning indent.
fn shown_rtl(name: &str, text: &str) -> String {
    let (_, sets) = named(name);
    let rows = break_line_rtl(&sets, text, 1).unwrap();
    shown(name, &rows[0])
}

/// REQ-CLI-010: لا is the lam-alef code, and a line is reordered into
/// visual order with digits and Latin left to right and brackets
/// mirrored.
#[test]
fn arabic_script_is_written_in_visual_order() {
    assert_eq!(shown_rtl("persian", "سلام"), "م\u{FEFB}س");
    // Logical: salam, space, (1405); visual: (۱۴۰۵) on the left with
    // its digits in order and its brackets mirrored back.
    assert_eq!(shown_rtl("persian", "سلام (۱۴۰۵)"), "(۱۴۰۵) م\u{FEFB}س");
    // A Latin run keeps its order. The row starts in the Urdu set, so
    // an ESC (shown as a space) leads into BBC, and the ESC back takes
    // the place of the space between the words.
    assert_eq!(shown_rtl("urdu", "خبر BBC"), " BBC ربخ");
}

/// REQ-95, DEC-2396: in an Arabic-script set the zero-width non-joiner of
/// می‌خواهم is written as a space cell, which keeps the letters on either
/// side unjoined; in a set that joins nothing it is refused, naming it.
#[test]
fn the_zero_width_non_joiner_is_a_space_in_arabic_script() {
    assert_eq!(shown_rtl("persian", "می خواهم"), "مهاوخ یم");
    let written = write("persian", 0x100, "می\u{200C}خواهم").unwrap();
    let spaced = write("persian", 0x100, "می خواهم").unwrap();
    assert_eq!(written.tti, spaced.tti, "written exactly as with a space");
    assert_eq!(
        write("english", 0x100, "a\u{200C}b").unwrap_err(),
        WriteError::Missing { character: '\u{200C}', line: 1 }
    );
}

/// A long right-to-left line breaks between words, and each row is
/// right-aligned on the page.
#[test]
fn right_to_left_rows_break_and_align_right() {
    let word = "ب".repeat(12);
    let line = [word.as_str(); 4].join(" ");
    let written = write("persian", 0x100, &line).unwrap();
    assert_eq!(written.rows, 2, "three words of 12 and two spaces fit 40 cells");
}

/// REQ-CLI-010: a page whose marks exceed the sixteen packets X/26 is
/// refused.
#[test]
fn the_x26_budget_is_enforced() {
    let row = "ả".repeat(COLUMNS);
    let fits = [row.as_str(); 5].join("\n");
    let written = write("vietnamese", 0x100, &fits).unwrap();
    assert_eq!(written.triplets, 5 * (COLUMNS + 1));
    let over = [row.as_str(); 6].join("\n");
    assert_eq!(write("vietnamese", 0x100, &over).unwrap_err(), WriteError::Author(AuthorError::LocalDataFull));
}
