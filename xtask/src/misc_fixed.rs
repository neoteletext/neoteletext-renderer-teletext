//! The X11 misc-fixed 6x10 font at the engine's 12 by 20 cell (REQ-2223).
//!
//! - `cargo xtask misc-fixed` writes the converted fonts, the folder's
//!   `fonts.txt` and the checksum lines of its `PROVENANCE.md` from the
//!   original `6x10.bdf` kept beside them.
//! - `cargo xtask check-misc-fixed` checks every file of the folder against
//!   its record ([`crate::font_folders`]) and that the converted fonts are
//!   the original with every pixel made two by two, every glyph once.
//!
//! The font is loaded at run time like any other (REQ-FNT-005 to
//! REQ-FNT-008); it is not compiled into the core (REQ-121, REQ-2223 item 4).

use crate::font_folders::{self, MISC_FIXED};
use neoteletext_core::consts::{CELL_FIELD_LINES, CELL_HEIGHT, CELL_WIDTH, FONT_MAX_CHARACTERS};
use neoteletext_core::glyphs::LoadedFont;
use neoteletext_core::glyphs::conformance::{FONT_SETS, conformance};
use neoteletext_core::types::FontGlyph;
use neoteletext_formats::bdf;
use neoteletext_formats::bdf::source::{SourceGlyph, read_source};
use neoteletext_formats::consts::SOURCE_FONT_MAX_CHARACTERS;
use neoteletext_formats::write::{FontText, write_font};
use std::fs;
use std::ops::RangeInclusive;
use std::path::Path;

/// The original, byte for byte as the `ucs-fonts.tar.gz` of Markus Kuhn
/// holds it (PROVENANCE.md), within [`MISC_FIXED`]'s folder.
const ORIGINAL: &str = "6x10.bdf";
/// Its SHA-256, as REQ-2223 records it.
const ORIGINAL_SHA256: &str = "bb14d95828990eb3ada68a24d716ae640a674cd75ab6caca17c798e4cb385849";
/// The original's cell, which doubles exactly to [`CELL_WIDTH`] by [`CELL_HEIGHT`].
const SOURCE_WIDTH: usize = 6;
/// The original's cell height, half of [`CELL_HEIGHT`].
const SOURCE_HEIGHT: usize = CELL_FIELD_LINES;
/// Each source pixel becomes this many pixels across and down.
const SCALE: usize = 2;
const _: () = assert!(SOURCE_WIDTH * SCALE == CELL_WIDTH && SOURCE_HEIGHT * SCALE == CELL_HEIGHT);

/// The converted font that holds all of code page 437 and the scripts most
/// pages use (DEC-2224 item 3): Basic Latin to Cyrillic Supplement,
/// General Punctuation to Dingbats (box drawing, blocks and the CP437
/// symbols among them), and the Latin ligatures and the replacement
/// character.
const MAIN_FILE: &str = "misc-fixed-6x10-12x20.bdf";
const MAIN_RANGES: [RangeInclusive<u32>; 3] = [0x0001..=0x052F, 0x2000..=0x27FF, 0xFB00..=0xFFFF];
/// Every other glyph of the original: its default glyph at U+0000, Hebrew,
/// Runic, Latin Extended Additional, Greek Extended and Braille. A font file
/// holds at most [`FONT_MAX_CHARACTERS`] characters (REQ-FNT-002), and the
/// original has 1,597.
const EXTENDED_FILE: &str = "misc-fixed-6x10-12x20-extended.bdf";
/// The family names the files and `fonts.txt` give.
const MAIN_FAMILY: &str = "Misc Fixed 6x10";
const EXTENDED_FAMILY: &str = "Misc Fixed 6x10 Extended";
/// The folder's index heading.
const INDEX_HEADING: &str = "# The X11 misc-fixed 6x10 font at 12 x 20 (REQ-2223), written by cargo xtask misc-fixed.";
/// The checksum block of PROVENANCE.md this command writes.
const CHECKSUMS_START: &str = "<!-- checksums: written by cargo xtask misc-fixed -->";
const CHECKSUMS_END: &str = "<!-- end checksums -->";

fn sha256(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&neoteletext_web::sha256::to_hex(&neoteletext_web::sha256::digest(bytes))).into_owned()
}

/// The original's glyphs, after checking it is the recorded file.
fn source_glyphs(original: &[u8]) -> Result<(Vec<SourceGlyph>, Option<Vec<u8>>), String> {
    let digest = sha256(original);
    if digest != ORIGINAL_SHA256 {
        return Err(format!("{ORIGINAL} has SHA-256 {digest}, not {ORIGINAL_SHA256}"));
    }
    let mut glyphs = vec![SourceGlyph::EMPTY; SOURCE_FONT_MAX_CHARACTERS];
    let font = read_source(original, &mut glyphs).map_err(|e| format!("{ORIGINAL}:{}: {:?}", e.line, e.kind))?;
    if (font.width, font.height) != (SOURCE_WIDTH, SOURCE_HEIGHT) || !font.unicode {
        return Err(format!("{ORIGINAL} is not a {SOURCE_WIDTH} by {SOURCE_HEIGHT} ISO10646 font"));
    }
    glyphs.truncate(font.characters);
    Ok((glyphs, font.copyright.map(<[u8]>::to_vec)))
}

/// A 6-pixel line made 12, each pixel two wide; the leftmost pixel is the
/// highest bit in both.
fn widened(line: u16) -> u16 {
    (0..SOURCE_WIDTH).filter(|bit| line >> bit & 1 == 1).fold(0, |out, bit| out | 0b11 << (bit * SCALE))
}

/// One glyph doubled across and down.
fn doubled(glyph: &SourceGlyph) -> FontGlyph {
    let mut rows = [0u16; CELL_HEIGHT];
    for (line, pair) in glyph.rows.iter().take(SOURCE_HEIGHT).zip(rows.as_chunks_mut::<SCALE>().0) {
        pair.fill(widened(*line));
    }
    FontGlyph::tall(glyph.encoding, rows)
}

fn in_main(code_point: u32) -> bool {
    MAIN_RANGES.iter().any(|range| range.contains(&code_point))
}

/// One converted file's bytes.
fn font_file(family: &str, copyright: Option<&[u8]>, glyphs: &[FontGlyph]) -> Result<Vec<u8>, String> {
    if glyphs.len() > FONT_MAX_CHARACTERS {
        return Err(format!("{family}: {} characters, more than {FONT_MAX_CHARACTERS}", glyphs.len()));
    }
    let comment = format!(
        "The X11 misc-fixed 6x10 font (6x10.bdf, SHA-256 {ORIGINAL_SHA256}),\n\
         every pixel made two by two by cargo xtask misc-fixed (REQ-2223)."
    );
    let text = FontText { family: Some(family.as_bytes()), copyright, notice: None, comment: &comment };
    let mut bytes = Vec::new();
    write_font(&mut bytes, &text, glyphs).map_err(|e| format!("{family}: {e}"))?;
    Ok(bytes)
}

/// The library index's conformance line (REQ-FNT-004, REQ-FNT-011), as
/// `neottx font sets --library-line` writes it.
fn conformance_line(file: &str, bytes: &[u8]) -> Result<String, String> {
    let glyphs = read_converted(file, bytes)?;
    let table = LoadedFont::new(&glyphs).map_err(|e| format!("{file}: {e:?}"))?;
    let sets: Vec<String> = FONT_SETS
        .iter()
        .map(|(name, set)| (name, conformance(&table, *set)))
        .filter(|(_, result)| result.conforms())
        .map(|(name, result)| format!("{name}{}", if result.saa5050_cell { "*" } else { "" }))
        .collect();
    Ok(format!("c\t{file}\t{}", sets.join(" ")))
}

/// Every output: the two fonts, then `fonts.txt`, by file name.
fn outputs(original: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    let (source, copyright) = source_glyphs(original)?;
    let mut glyphs: Vec<FontGlyph> = source.iter().map(doubled).collect();
    glyphs.sort_by_key(|glyph| glyph.code_point);
    let (main, extended): (Vec<FontGlyph>, Vec<FontGlyph>) =
        glyphs.into_iter().partition(|glyph| in_main(glyph.code_point));
    let mut files = vec![
        (MAIN_FILE, font_file(MAIN_FAMILY, copyright.as_deref(), &main)?),
        (EXTENDED_FILE, font_file(EXTENDED_FAMILY, copyright.as_deref(), &extended)?),
    ];
    let mut index = vec![INDEX_HEADING.to_string()];
    for ((file, bytes), family) in files.iter().zip([MAIN_FAMILY, EXTENDED_FAMILY]) {
        index.push(format!("f\t{file}\t{family}\t{}", sha256(bytes)));
        index.push(conformance_line(file, bytes)?);
    }
    files.push(("fonts.txt", (index.join("\n") + "\n").into_bytes()));
    Ok(files)
}

/// PROVENANCE.md with its checksum block rewritten for `files`.
fn provenance_with_checksums(record: &str, files: &[(&str, Vec<u8>)]) -> Result<String, String> {
    let start = record.find(CHECKSUMS_START).ok_or("PROVENANCE.md has no checksum block")?;
    let end = record.find(CHECKSUMS_END).ok_or("PROVENANCE.md has no end of its checksum block")?;
    let mut lines = vec![format!("- `{ORIGINAL}`: `{ORIGINAL_SHA256}`")];
    for (file, bytes) in files.iter().filter(|(file, _)| [MAIN_FILE, EXTENDED_FILE].contains(file)) {
        lines.push(format!("- `{file}`: `{}`", sha256(bytes)));
    }
    let head = record.get(..start + CHECKSUMS_START.len()).ok_or("bad checksum block")?;
    let tail = record.get(end..).ok_or("bad checksum block")?;
    Ok(format!("{head}\n{}\n{tail}", lines.join("\n")))
}

/// `cargo xtask misc-fixed`.
pub fn generate(root: &Path) -> Result<(), String> {
    let path = format!("{}/{ORIGINAL}", MISC_FIXED.folder);
    let original = fs::read(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
    let files = outputs(&original)?;
    for (file, bytes) in &files {
        let path = format!("{}/{file}", MISC_FIXED.folder);
        fs::write(root.join(&path), bytes).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}");
    }
    let provenance = format!("{}/PROVENANCE.md", MISC_FIXED.folder);
    let record = fs::read_to_string(root.join(&provenance)).map_err(|e| format!("{provenance}: {e}"))?;
    fs::write(root.join(&provenance), provenance_with_checksums(&record, &files)?)
        .map_err(|e| format!("{provenance}: {e}"))?;
    println!("wrote the checksums of {provenance}");
    Ok(())
}

/// A converted file's glyphs, read as the engine reads a loaded font.
fn read_converted(file: &str, bytes: &[u8]) -> Result<Vec<FontGlyph>, String> {
    let mut table = vec![FontGlyph::EMPTY; FONT_MAX_CHARACTERS];
    let font = bdf::read(bytes, &mut table).map_err(|e| format!("{file}:{}: {:?}", e.line, e.kind))?;
    table.truncate(font.characters);
    Ok(table)
}

/// The pixel at `column` of a line `width` pixels wide, leftmost first.
fn pixel(line: u16, width: usize, column: usize) -> bool {
    line >> (width - 1 - column) & 1 == 1
}

/// The converted files are the original with every pixel two by two: every
/// glyph of the original is in exactly one file, 12 by 20, and nothing
/// else is. Returns how many glyphs were compared. Written apart from the
/// conversion above, pixel by pixel, so that it checks it.
fn check_doubled(original: &[u8], converted: &[(&str, &[u8])]) -> Result<usize, String> {
    let (source, _) = source_glyphs(original)?;
    let mut unmatched: std::collections::BTreeMap<u32, &SourceGlyph> =
        source.iter().map(|glyph| (glyph.encoding, glyph)).collect();
    let mut compared = 0usize;
    for (file, bytes) in converted {
        for glyph in read_converted(file, bytes)? {
            let code = glyph.code_point;
            let original_glyph = unmatched
                .remove(&code)
                .ok_or(format!("{file}: U+{code:04X} is not in the original, or is in two files"))?;
            if !glyph.tall {
                return Err(format!("{file}: U+{code:04X} is not 12 by 20"));
            }
            for line in 0..CELL_HEIGHT {
                for column in 0..CELL_WIDTH {
                    let source_line = original_glyph.rows.get(line / SCALE).copied().unwrap_or(0);
                    let expected = pixel(source_line, SOURCE_WIDTH, column / SCALE);
                    let actual = pixel(glyph.rows.get(line).copied().unwrap_or(0), CELL_WIDTH, column);
                    if expected != actual {
                        return Err(format!(
                            "{file}: U+{code:04X} line {line} column {column} is {}, but the original's pixel is {}",
                            u8::from(actual),
                            u8::from(expected)
                        ));
                    }
                }
            }
            compared += 1;
        }
    }
    if let Some(code) = unmatched.keys().next() {
        return Err(format!("U+{code:04X} of the original is in no converted file"));
    }
    Ok(compared)
}

/// `cargo xtask check-misc-fixed`.
pub fn check(root: &Path) -> Result<(), String> {
    font_folders::check(root, &MISC_FIXED)?;
    let read = |file: &str| {
        let path = format!("{}/{file}", MISC_FIXED.folder);
        fs::read(root.join(&path)).map_err(|e| format!("{path}: {e}"))
    };
    let original = read(ORIGINAL)?;
    let (main, extended) = (read(MAIN_FILE)?, read(EXTENDED_FILE)?);
    let compared = check_doubled(&original, &[(MAIN_FILE, &main), (EXTENDED_FILE, &extended)])?;
    println!("{}: {compared} glyphs are the original's, each pixel two by two", MISC_FIXED.folder);
    Ok(())
}

#[cfg(test)]
mod tests;
