//! `neottx font convert` (REQ-FNT-010, decision E.72 item 7): a BDF font of
//! another cell size to the 12 by 10 form `neottx render --font` loads, offline and
//! sandboxed, only where the conversion is exact. `neottx font sets`
//! (REQ-FNT-011, decision E.75): the character sets a font conforms for.

use super::io_failure;
use super::sandbox_closed;
use neoteletext_app::bounded_file::read_bounded;
use neoteletext_app::input::Options;
use neoteletext_app::{Failure, exit};
use neoteletext_core::consts::{FONT_MAX_CHARACTERS, FONT_MAX_FILE_BYTES};
use neoteletext_core::glyphs::LoadedFont;
use neoteletext_core::glyphs::conformance::{
    FONT_SETS, Failure as RuleFailure, SET_POSITIONS, conformance, font_set_index,
};
use neoteletext_core::types::FontGlyph;
use neoteletext_formats::bdf::FontErrorKind;
use neoteletext_formats::bdf::convert::{ConvertError, convert, read_map};
use neoteletext_formats::bdf::read;
use neoteletext_formats::bdf::source::{SourceGlyph, read_source};
use neoteletext_formats::consts::{FONT_MAP_MAX_FILE_BYTES, SOURCE_FONT_MAX_CHARACTERS, SOURCE_FONT_MAX_FILE_BYTES};
use neoteletext_formats::write::{FontText, write_font};
use neoteletext_web::font_library::{self, Entry, INDEX_FILE};
use neoteletext_web::sha256::{self, Digest};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// `neottx font convert INPUT OUTPUT [--map FILE]`.
pub fn convert_font(input: &Path, output: &Path, map: Option<&Path>, no_sandbox: bool) -> Result<(), Failure> {
    let source = read_bounded(input, SOURCE_FONT_MAX_FILE_BYTES)?;
    let map_text = map.map(|path| read_bounded(path, FONT_MAP_MAX_FILE_BYTES)).transpose()?;
    let mut out = File::create(output).map_err(|error| io_failure(output, error))?;
    // Every file is open: nothing that follows needs the file system or the
    // network (REQ-SEC-017).
    sandbox_closed(no_sandbox)?;
    let name = input.display();
    let mut glyphs = vec![SourceGlyph::EMPTY; SOURCE_FONT_MAX_CHARACTERS];
    let font = read_source(&source, &mut glyphs)
        .map_err(|error| (exit::INPUT, format!("{name}:{}: {}", error.line, source_fault(error.kind))))?;
    let mut pairs = vec![(0u32, 0u32); SOURCE_FONT_MAX_CHARACTERS];
    let pairs = match (&map_text, map) {
        (Some(text), Some(path)) => {
            let count = read_map(text, &mut pairs).map_err(|error| {
                (exit::INPUT, format!("{}:{}: not a `code<TAB>U+XXXX` line", path.display(), error.line))
            })?;
            pairs.get(..count)
        }
        _ => None,
    };
    let mut table = vec![FontGlyph::EMPTY; FONT_MAX_CHARACTERS];
    let count = convert(&font, &glyphs, pairs, &mut table)
        .map_err(|error| (exit::INPUT, format!("{name}: {}", conversion_fault(error))))?;
    let comment = format!(
        "Converted by neottx font convert from a {} x {} cell (REQ-FNT-010); every pixel kept.",
        font.width, font.height
    );
    let text = FontText { family: font.family, copyright: font.copyright, notice: font.notice, comment: &comment };
    let mut written = Vec::new();
    write_font(&mut written, &text, table.get(..count).unwrap_or(&[]))
        .map_err(|error| (exit::INPUT, format!("{name}: {error}")))?;
    out.write_all(&written).and_then(|()| out.flush()).map_err(|error| io_failure(output, error))?;
    eprintln!("{}: {count} characters from a {} x {} cell", output.display(), font.width, font.height);
    Ok(())
}

/// A SHA-256 as `--font-sha256` gives it: 64 hexadecimal digits.
pub(super) fn parse_sha256(text: &str) -> Result<Digest, Failure> {
    sha256::parse_hex(text.as_bytes())
        .ok_or_else(|| (exit::USAGE, format!("--font-sha256 {text:?}: not 64 hexadecimal digits")))
}

/// Reads the file of a loaded font for drawing pages (`--font`,
/// REQ-FNT-008), within its bound, before the sandbox closes the file system;
/// `parse_table` reads it after.
pub(super) fn read_table_file(path: &Path) -> Result<Vec<u8>, Failure> {
    read_bounded(path, FONT_MAX_FILE_BYTES)
}

/// The bytes of the font `--font` names (REQ-123): a file, read within its
/// bound, or an `http` or `https` address, fetched by the confined fetcher
/// process under the network rules of a channel's files, which only a font
/// named with its SHA-256 may be (DEC-2481). Runs before the sandbox, which
/// then forbids starting the fetcher; `parse_table` reads the bytes after.
pub(super) fn read_font(font: &Path, expected: Option<&Digest>, options: &Options) -> Result<Vec<u8>, Failure> {
    let Some(address) = font.to_str().filter(|text| is_web_address(text)) else {
        return read_table_file(font);
    };
    if expected.is_none() {
        return Err((exit::USAGE, format!("--font {address}: a font fetched from an address needs --font-sha256")));
    }
    fetch_font(address, options)
}

/// Whether `--font` names an address rather than a file.
fn is_web_address(text: &str) -> bool {
    WEB_SCHEMES.iter().any(|scheme| text.get(..scheme.len()).is_some_and(|start| start.eq_ignore_ascii_case(scheme)))
}

/// The schemes a font may be fetched with (REQ-123).
const WEB_SCHEMES: [&str; 2] = ["http://", "https://"];

#[cfg(feature = "live")]
fn fetch_font(address: &str, options: &Options) -> Result<Vec<u8>, Failure> {
    let (bytes, secure) =
        neoteletext_app::live::fetch_font(address, options.no_sandbox, &options.allow_local, options.proxy.as_deref())?;
    // REQ-SEC-005, REQ-SEC-008: a font that came over plain HTTP is said to.
    if !secure {
        eprintln!("neottx: {address}: the font came over plain HTTP, unencrypted");
    }
    Ok(bytes)
}

#[cfg(not(feature = "live"))]
fn fetch_font(_address: &str, _options: &Options) -> Result<Vec<u8>, Failure> {
    Err(neoteletext_app::not_in_build("a font fetched from an address", "live"))
}

/// The glyph table of a font file `read_table_file` read, checked by the
/// reader of REQ-FNT-002 and for shape; `path` names it in messages. When
/// the user names the font with a SHA-256, a file with any other is refused
/// before it is parsed (REQ-FNT-003).
pub(super) fn parse_table(path: &Path, bytes: &[u8], expected: Option<&Digest>) -> Result<Vec<FontGlyph>, Failure> {
    let name = path.display();
    if let Some(expected) = expected {
        font_library::check_font(bytes, expected).map_err(|actual| {
            let actual = String::from_utf8_lossy(&sha256::to_hex(&actual)).into_owned();
            (exit::INPUT, format!("{name}: SHA-256 {actual} is not the one named (REQ-FNT-003)"))
        })?;
    }
    let mut glyphs = vec![FontGlyph::EMPTY; FONT_MAX_CHARACTERS];
    let font = read(bytes, &mut glyphs)
        .map_err(|error| (exit::INPUT, format!("{name}:{}: {:?} (REQ-FNT-002)", error.line, error.kind)))?;
    glyphs.truncate(font.characters);
    LoadedFont::new(&glyphs).map_err(|error| (exit::INPUT, format!("{name}: {error:?}")))?;
    Ok(glyphs)
}

/// `neottx font sets FONT [--library-line NAME]`.
pub fn font_sets(path: &Path, library_line: Option<&str>, no_sandbox: bool) -> Result<(), Failure> {
    let bytes = read_bounded(path, FONT_MAX_FILE_BYTES)?;
    sandbox_closed(no_sandbox)?;
    let name = path.display();
    let mut glyphs = vec![FontGlyph::EMPTY; FONT_MAX_CHARACTERS];
    let font = read(&bytes, &mut glyphs)
        .map_err(|error| (exit::INPUT, format!("{name}:{}: {:?} (REQ-FNT-011 rule 1)", error.line, error.kind)))?;
    let table = LoadedFont::new(glyphs.get(..font.characters).unwrap_or(&[]))
        .map_err(|error| (exit::INPUT, format!("{name}: {error:?}")))?;
    let results: Vec<_> = FONT_SETS.iter().map(|(set_name, set)| (*set_name, conformance(&table, *set))).collect();
    if let Some(file) = library_line {
        let sets: Vec<String> = results
            .iter()
            .filter(|(_, result)| result.conforms())
            .map(|(set_name, result)| format!("{set_name}{}", if result.saa5050_cell { "*" } else { "" }))
            .collect();
        println!("c\t{file}\t{}", sets.join(" "));
        return Ok(());
    }
    for (set_name, result) in &results {
        // REQ-131: every position that fails the first rule, not only the first.
        let positions = result
            .failing_codes()
            .map(|code| format!("{:X}/{:X}", code >> 4, code & 0xF))
            .collect::<Vec<_>>()
            .join(", ");
        let verdict = match result.failure {
            None if result.saa5050_cell => "conforms, SAA5050 cell".to_string(),
            None => "conforms".to_string(),
            Some(RuleFailure::Missing(_)) => format!("rule 2: no glyph at {positions}"),
            Some(RuleFailure::Space) => "rule 3: the space is missing or not blank".to_string(),
            Some(RuleFailure::Blank(_)) => format!("rule 4: blank glyph at {positions}"),
            Some(RuleFailure::MissingForm(_)) => format!("no glyph for a joined form of the letter at {positions}"),
        };
        println!("{set_name:34} {:>2}/{SET_POSITIONS}  {verdict}", result.drawn);
    }
    Ok(())
}

fn source_fault(kind: FontErrorKind) -> String {
    match kind {
        FontErrorKind::Cell => {
            "the cell is not 5 x 9, 6 x 9, 5 x 10, 6 x 10 or 12 x 20, so it has no exact 12 x 10 or 12 x 20 form".into()
        }
        FontErrorKind::GlyphTooLarge => "the glyph reaches outside its cell".into(),
        FontErrorKind::Greyscale => "more than one bit per pixel".into(),
        FontErrorKind::Code => "the character has no code (ENCODING must be a number from 0)".into(),
        other => format!("{other:?}"),
    }
}

fn conversion_fault(error: ConvertError) -> String {
    match error {
        ConvertError::Cell => "the cell has no exact 12 x 10 or 12 x 20 form".into(),
        ConvertError::Unmapped { encoding } => {
            format!("character {encoding}: the font is not in ISO10646 and the map (--map) has no line for it")
        }
        ConvertError::NotScalar { encoding } => format!("character {encoding}: not a Unicode code point"),
        ConvertError::Repeated { encoding } => format!("character {encoding}: its code point is already in the font"),
        ConvertError::TooMany => format!("more than {FONT_MAX_CHARACTERS} characters"),
    }
}

/// `neottx font library LIBRARY` (REQ-FNT-004): the fonts of a library's
/// index by title, one line each, `title<TAB>font<TAB>SHA-256<TAB>sets`,
/// the sets as its `c` line states them; then the malformed lines on
/// standard error, each with its number and fault, and their count. Only
/// the index is read: listing a library fetches no font.
pub fn font_library(library: &Path, no_sandbox: bool) -> Result<(), Failure> {
    let index = if library.is_dir() { library.join(INDEX_FILE) } else { library.to_path_buf() };
    let bytes = read_bounded(&index, font_library::MAX_BYTES)?;
    sandbox_closed(no_sandbox)?;
    let name = index.display();
    // REQ-FNT-011: the set names libraries give are the core's.
    let entries = font_library::read(&bytes, |set| font_set_index(set).is_some())
        .map_err(|error| (exit::INPUT, format!("{name}: {error:?} (REQ-FNT-004)")))?;
    let mut fonts = Vec::new();
    let mut stated = std::collections::BTreeMap::new();
    let mut malformed = 0usize;
    for entry in entries {
        match entry {
            Ok(Entry::Font(font)) => fonts.push(font),
            Ok(Entry::Conformance(conformance)) => {
                let sets: Vec<String> = conformance
                    .sets()
                    .map(|(set, saa5050)| format!("{set}{}", if saa5050 { "*" } else { "" }))
                    .collect();
                stated.insert(conformance.location.as_str(), sets.join(" "));
            }
            Err(fault) if fault.fault.is_malformed() => {
                malformed = malformed.saturating_add(1);
                eprintln!("{name}:{}: {:?} (REQ-FNT-004)", fault.line, fault.fault);
            }
            Err(_) => {}
        }
    }
    let mut out = std::io::stdout().lock();
    for font in &fonts {
        let at = font.location.as_str();
        let hash = String::from_utf8_lossy(&sha256::to_hex(&font.sha256)).into_owned();
        let sets = stated.get(at).map_or("", String::as_str);
        writeln!(out, "{}\t{at}\t{hash}\t{sets}", font.title).map_err(|error| io_failure(Path::new("-"), error))?;
    }
    eprintln!("{name}: {} font(s), {malformed} malformed line(s)", fonts.len());
    Ok(())
}
