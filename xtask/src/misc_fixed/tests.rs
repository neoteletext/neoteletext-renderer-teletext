//! The committed misc-fixed files against the original (REQ-2223).

use super::*;

fn committed(file: &str) -> Vec<u8> {
    let path = crate::workspace_root().join(MISC_FIXED.folder).join(file);
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// REQ-2223: the converted fonts are the original doubled pixel for pixel,
/// every one of its 1,597 glyphs in one file or the other.
#[test]
fn the_converted_fonts_are_the_original_doubled() {
    let (main, extended) = (committed(MAIN_FILE), committed(EXTENDED_FILE));
    let compared = check_doubled(&committed(ORIGINAL), &[(MAIN_FILE, &main), (EXTENDED_FILE, &extended)]).unwrap();
    assert_eq!(compared, 1597);
}

/// REQ-2223: the committed files are what `cargo xtask misc-fixed` writes.
#[test]
fn the_committed_files_are_up_to_date() {
    for (file, bytes) in outputs(&committed(ORIGINAL)).unwrap() {
        assert!(committed(file) == bytes, "{file} is stale: run cargo xtask misc-fixed");
    }
    let record = String::from_utf8(committed("PROVENANCE.md")).unwrap();
    let files = outputs(&committed(ORIGINAL)).unwrap();
    assert_eq!(record, provenance_with_checksums(&record, &files).unwrap(), "PROVENANCE.md's checksums are stale");
}

/// The main file holds every graphic character of code page 437: the
/// symbols at 1 to 31 and 127 and all of 128 to 255 (REQ-2223).
#[test]
fn the_main_file_holds_all_of_code_page_437() {
    const CP437_SYMBOLS: &str = "☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼⌂";
    const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";
    let glyphs = read_converted(MAIN_FILE, &committed(MAIN_FILE)).unwrap();
    let held: Vec<u32> = glyphs.iter().map(|glyph| glyph.code_point).collect();
    let wanted: Vec<char> = CP437_SYMBOLS.chars().chain(CP437_HIGH.chars()).chain(' '..='~').collect();
    assert_eq!(wanted.len(), 32 + 128 + 95);
    let missing: Vec<char> = wanted.into_iter().filter(|c| !held.contains(&u32::from(*c))).collect();
    assert_eq!(missing, Vec::<char>::new());
}

/// The check fails on wrong data: one pixel changed, a glyph left out, a
/// glyph in both files, a glyph the original lacks.
#[test]
fn the_check_refuses_wrong_data() {
    let original = committed(ORIGINAL);
    let files = outputs(&original).unwrap();
    let [(_, main), (_, extended), ..] = files.as_slice() else { panic!("two fonts") };
    let mut glyphs = read_converted(MAIN_FILE, main).unwrap();
    let rewrite = |glyphs: &[FontGlyph]| font_file(MAIN_FAMILY, None, glyphs).unwrap();

    let letter = glyphs.iter().position(|glyph| glyph.code_point == u32::from('A')).unwrap();
    glyphs[letter].rows[0] ^= 1;
    let error = check_doubled(&original, &[(MAIN_FILE, &rewrite(&glyphs)), (EXTENDED_FILE, extended)]).unwrap_err();
    assert!(error.contains("U+0041 line 0 column 11"), "{error}");
    glyphs[letter].rows[0] ^= 1;

    let removed = glyphs.remove(letter);
    let error = check_doubled(&original, &[(MAIN_FILE, &rewrite(&glyphs)), (EXTENDED_FILE, extended)]).unwrap_err();
    assert!(error.contains("U+0041 of the original is in no converted file"), "{error}");
    glyphs.insert(letter, removed);

    let error = check_doubled(&original, &[(MAIN_FILE, main), (EXTENDED_FILE, main)]).unwrap_err();
    assert!(error.contains("or is in two files"), "{error}");

    glyphs.push(FontGlyph::tall(0x10_FFFD, [0; CELL_HEIGHT]));
    let error = check_doubled(&original, &[(MAIN_FILE, &rewrite(&glyphs)), (EXTENDED_FILE, extended)]).unwrap_err();
    assert!(error.contains("U+10FFFD is not in the original"), "{error}");

    let mut changed = original.clone();
    changed.push(b'\n');
    assert!(check_doubled(&changed, &[(MAIN_FILE, main)]).unwrap_err().contains("has SHA-256"));
}

/// Doubling makes each pixel two across, leftmost first.
#[test]
fn a_line_is_widened_pixel_by_pixel() {
    assert_eq!(widened(0b10_0000), 0b1100_0000_0000);
    assert_eq!(widened(0b00_0001), 0b0000_0000_0011);
    assert_eq!(widened(0b10_1001), 0b1100_1100_0011);
}
