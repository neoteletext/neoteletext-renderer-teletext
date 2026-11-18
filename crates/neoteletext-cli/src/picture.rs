//! ReGIS and NAPLPS pictures for `neottx render`, `show`, `view` and
//! `check` (renderer parity PAR-7, REQ-6763 item 1): drawn as the browser
//! draws them, with the project's own renderers, the provided character
//! sets from the compiled Noto Teletext glyphs (PAR-12, REQ-6668) and the
//! joining data of the plain Arabic-script sets (PAR-3).
//!
//! A ReGIS picture is the VT340's 800 by 480 screen in its colour map; a
//! NAPLPS picture is drawn on the 480 by 462 canvas and placed on the 480
//! by 500 teletext raster from line 19 (REQ-5819, DEC-6667), the border
//! lines in the terminal's screen colour.

use neoteletext_core::charset::CharRef;
use neoteletext_core::consts::{CELL_HEIGHT, CELL_WIDTH, PAGE_WIDTH};
use neoteletext_core::glyphs::conformance::{FONT_SET_COUNT, FONT_SETS};
use neoteletext_core::glyphs::{GlyphSet, JOINED_SETS, JoiningClass, joining};
use neoteletext_image::palette::Rgba;
use neoteletext_image::raster::Raster;

/// The renderers' sets are the engine's (PAR-12).
const _: () = assert!(regis::limits::PROVIDED_SETS as usize == FONT_SET_COUNT);
const _: () = assert!(naplps::TELETEXT_SET_COUNT == FONT_SET_COUNT);

/// The first and last code of a provided set.
const FIRST_CODE: u8 = 0x20;
const LAST_CODE: u8 = 0x7F;
/// The ReGIS screen.
const REGIS_WIDTH: u32 = regis::SCREEN_WIDTH;
const REGIS_HEIGHT: u32 = regis::SCREEN_HEIGHT;
/// The NAPLPS picture's place on the teletext raster: 462 lines from line
/// 19 (REQ-5819, DEC-6667).
pub const NAPLPS_FIRST_LINE: usize = 19;
/// See [`NAPLPS_FIRST_LINE`].
pub const NAPLPS_LINES: usize = 462;
/// The teletext raster's height: 25 rows of 20 lines.
const RASTER_LINES: usize = 500;
const _: () = assert!(NAPLPS_FIRST_LINE + NAPLPS_LINES <= RASTER_LINES);
/// The colour map a NeoTeletext receiver gives the NAPLPS renderer: four
/// bits, sixteen entries (the server's and the browser's).
const NAPLPS_PALETTE_BITS: u32 = 4;
/// The device control string that starts ReGIS, for a file of bare
/// commands (the manual's chapter 1).
const REGIS_INTRODUCER: &[u8] = b"\x1bP";

/// A picture language.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    /// ReGIS.
    Regis,
    /// NAPLPS.
    Naplps,
}

/// The picture language of a file, by its name: `.regis`, or `.naplps` and
/// `.nap`.
#[must_use]
pub fn language_of(path: &str) -> Option<Language> {
    let extension = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "regis" => Some(Language::Regis),
        "naplps" | "nap" => Some(Language::Naplps),
        _ => None,
    }
}

/// The fonts a picture's own text is drawn in: ReGIS alphabet 0 and the
/// NAPLPS stroke font, from `--regis-font` and `--naplps-font` or else
/// the fonts compiled in.
#[derive(Clone, Debug, Default)]
pub struct Fonts {
    /// The text of a ReGIS font file.
    pub regis: Option<String>,
    /// The text of a NAPLPS stroke font file.
    pub naplps: Option<String>,
}

/// The glyph rows of `code` of set `set`.
fn rows(glyphs: &GlyphSet<'_>, set: u8, code: u8) -> [u16; CELL_HEIGHT] {
    let charset = FONT_SETS.get(usize::from(set)).map_or(FONT_SETS[0].1, |(_, charset)| *charset);
    glyphs.glyph(CharRef::new(charset, code))
}

/// The cell's sides as the renderers take them.
fn cell() -> Result<(u8, u8), String> {
    let width = u8::try_from(CELL_WIDTH).map_err(|_| "the cell is wider than a renderer's glyph".to_owned())?;
    let height = u8::try_from(CELL_HEIGHT).map_err(|_| "the cell is taller than a renderer's glyph".to_owned())?;
    Ok((width, height))
}

/// The 55 provided sets, with joining data, for the ReGIS renderer.
///
/// # Errors
///
/// The renderer's refusal of a set, as text.
pub fn regis_sets() -> Result<Vec<regis::ProvidedSet>, String> {
    let glyphs = GlyphSet::noto_teletext();
    let (width, height) = cell()?;
    let mut sets = Vec::with_capacity(FONT_SET_COUNT);
    for set in 0..u8::try_from(FONT_SET_COUNT).map_err(|_| "too many sets".to_owned())? {
        let mut provided = regis::ProvidedSet::new(width, height).map_err(|error| format!("{error:?}"))?;
        for code in FIRST_CODE..=LAST_CODE {
            provided.set_glyph(code, &rows(&glyphs, set, code)).map_err(|error| format!("{error:?}"))?;
            if !JOINED_SETS.contains(&set) {
                continue;
            }
            let Some(data) = joining(set, code) else { continue };
            let kind = match data.class {
                JoiningClass::None => regis::JoiningType::None,
                JoiningClass::Right => regis::JoiningType::Right,
                JoiningClass::Dual => regis::JoiningType::Dual,
                JoiningClass::Transparent => regis::JoiningType::Transparent,
            };
            provided.set_joining(code, kind).map_err(|error| format!("{error:?}"))?;
            for (form, glyph) in [
                (regis::JoinedForm::Final, &data.final_form),
                (regis::JoinedForm::Initial, &data.initial),
                (regis::JoinedForm::Medial, &data.medial),
            ] {
                provided.set_form(code, form, glyph).map_err(|error| format!("{error:?}"))?;
            }
        }
        sets.push(provided);
    }
    Ok(sets)
}

/// The 55 sets, with joining data, for the NAPLPS renderer.
///
/// # Errors
///
/// The renderer's refusal of a set, as text.
pub fn naplps_sets() -> Result<Vec<naplps::BitmapSet>, String> {
    let glyphs = GlyphSet::noto_teletext();
    let (width, height) = cell()?;
    let mut sets = Vec::with_capacity(FONT_SET_COUNT);
    for set in 0..u8::try_from(FONT_SET_COUNT).map_err(|_| "too many sets".to_owned())? {
        let mut bitmap = naplps::BitmapSet::new(width, height).map_err(|error| format!("{error:?}"))?;
        for code in FIRST_CODE..=LAST_CODE {
            bitmap.set_glyph(code, &rows(&glyphs, set, code)).map_err(|error| format!("{error:?}"))?;
            if !JOINED_SETS.contains(&set) {
                continue;
            }
            let Some(data) = joining(set, code) else { continue };
            let kind = match data.class {
                JoiningClass::None => naplps::JoiningType::None,
                JoiningClass::Right => naplps::JoiningType::Right,
                JoiningClass::Dual => naplps::JoiningType::Dual,
                JoiningClass::Transparent => naplps::JoiningType::Transparent,
            };
            bitmap.set_joining(code, kind).map_err(|error| format!("{error:?}"))?;
            for (form, glyph) in [
                (naplps::JoinForm::Final, &data.final_form),
                (naplps::JoinForm::Initial, &data.initial),
                (naplps::JoinForm::Medial, &data.medial),
            ] {
                bitmap.set_form_glyph(code, form, glyph).map_err(|error| format!("{error:?}"))?;
            }
        }
        sets.push(bitmap);
    }
    Ok(sets)
}

/// A ReGIS terminal with its fonts and the provided sets.
fn regis_terminal(fonts: &Fonts) -> Result<regis::Terminal, String> {
    let mut terminal = regis::Terminal::new();
    let text = fonts.regis.as_deref().unwrap_or(regis_fonts::VT340);
    terminal.set_font(regis::Font::parse(text).map_err(|error| format!("the ReGIS font: {error:?}"))?);
    for (number, set) in (0_u8..).zip(regis_sets()?) {
        terminal.set_provided_set(number, set).map_err(|error| format!("{error:?}"))?;
    }
    Ok(terminal)
}

/// A NAPLPS terminal with its fonts and the 55 sets.
fn naplps_terminal(fonts: &Fonts) -> Result<naplps::Terminal, String> {
    let mut terminal = naplps::Terminal::new(NAPLPS_PALETTE_BITS);
    let text = fonts.naplps.as_deref().unwrap_or(naplps_fonts::NEOTELETEXT);
    terminal.set_font(naplps::StrokeFont::parse(text).map_err(|error| format!("the NAPLPS font: {error:?}"))?);
    for (number, set) in (0_u8..).zip(naplps_sets()?) {
        terminal.set_teletext_set(number, set).map_err(|error| format!("{error:?}"))?;
    }
    Ok(terminal)
}

/// ReGIS bytes as a terminal stream: a file of bare commands is put in a
/// device control string.
fn regis_stream(bytes: &[u8]) -> Vec<u8> {
    if bytes.windows(REGIS_INTRODUCER.len()).any(|window| window == REGIS_INTRODUCER) {
        return bytes.to_vec();
    }
    let mut stream = b"\x1bPp".to_vec();
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"\x1b\\");
    stream
}

/// What reading a picture found, for `neottx check` (PAR-7, as the
/// server's checks of PAR-23 report it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Findings {
    /// One line a finding.
    pub lines: Vec<String>,
    /// Whether any is a refusal, which `check` exits non-zero on.
    pub refused: bool,
}

/// Draws `bytes` of `language` as the browser draws them, and what reading
/// them found.
///
/// # Errors
///
/// A font or set the renderer refuses, as text.
pub fn draw(language: Language, bytes: &[u8], fonts: &Fonts) -> Result<(Raster, Findings), String> {
    match language {
        Language::Regis => draw_regis(bytes, fonts),
        Language::Naplps => draw_naplps(bytes, fonts),
    }
}

fn colour([red, green, blue]: [u8; 3]) -> Rgba {
    Rgba::opaque(red, green, blue)
}

fn draw_regis(bytes: &[u8], fonts: &Fonts) -> Result<(Raster, Findings), String> {
    let mut terminal = regis_terminal(fonts)?;
    let mut screen = regis::Screen::new();
    let mut events = terminal.feed(&mut screen, &regis_stream(bytes));
    events.extend(terminal.finish(&mut screen));
    let mut findings = Findings::default();
    for event in &events {
        match event {
            regis::Event::CommitFailed { error, drawing } => {
                findings.lines.push(format!("a commit failed: code {}, drawing {drawing}", error.code()));
                findings.refused = true;
            }
            regis::Event::LimitReached => {
                findings.lines.push("a work or size limit was reached: some input was not drawn".to_owned());
                findings.refused = true;
            }
            _ => {}
        }
    }
    // Numbered drawings (regis-v2.md) are composed over the screen, as the
    // browser shows them.
    let mut picture = regis::Picture::new();
    terminal.compose(&screen, &mut picture);
    let canvas = if terminal.has_drawings() { picture.canvas() } else { screen.displayed() };
    let mut indices = Vec::new();
    for y in 0..REGIS_HEIGHT {
        for x in 0..REGIS_WIDTH {
            indices.push(canvas.pixel(x, y).unwrap_or(0));
        }
    }
    let palette = terminal.palette().colours().iter().copied().map(colour).collect();
    let width = usize::try_from(REGIS_WIDTH).map_err(|_| "the screen's width".to_owned())?;
    let height = usize::try_from(REGIS_HEIGHT).map_err(|_| "the screen's height".to_owned())?;
    Ok((Raster { width, height, indices, palette }, findings))
}

fn draw_naplps(bytes: &[u8], fonts: &Fonts) -> Result<(Raster, Findings), String> {
    let mut terminal = naplps_terminal(fonts)?;
    let width = u32::try_from(PAGE_WIDTH).map_err(|_| "the raster's width".to_owned())?;
    let lines = u32::try_from(NAPLPS_LINES).map_err(|_| "the picture's height".to_owned())?;
    let mut canvas = naplps::Canvas::new(width, lines).map_err(|error| format!("{error:?}"))?;
    terminal.feed(&mut canvas, bytes);
    terminal.finish(&mut canvas);
    let diagnostics = terminal.diagnostics();
    let mut findings = Findings::default();
    for kind in naplps::Refusal::ALL {
        let count = diagnostics.count(kind);
        if count > 0 {
            findings.lines.push(format!("refused: {} ({count})", kind.name()));
            findings.refused = true;
        }
    }
    if let Some(first) = diagnostics.first_refused {
        findings.lines.push(format!("the first refusal is at byte {first}"));
    }
    if diagnostics.passed_over > 0 {
        let first = diagnostics.first_passed_over.unwrap_or(0);
        findings.lines.push(format!("{} bytes passed over, the first at byte {first}", diagnostics.passed_over));
    }
    let border = terminal.screen_colour();
    let mut indices = vec![border; PAGE_WIDTH.saturating_mul(RASTER_LINES)];
    for (line, row) in canvas.pixels().chunks(PAGE_WIDTH).enumerate() {
        let start = NAPLPS_FIRST_LINE.saturating_add(line).saturating_mul(PAGE_WIDTH);
        if let Some(target) = indices.get_mut(start..start.saturating_add(PAGE_WIDTH)) {
            target.copy_from_slice(row);
        }
    }
    let palette = terminal.palette().colours().iter().copied().map(colour).collect();
    Ok((Raster { width: PAGE_WIDTH, height: RASTER_LINES, indices, palette }, findings))
}

/// The columns and the rows of the cell grid a page of text is laid out on
/// in a picture (PAR-10, PAR-42): 40 columns, rows 1 to 23.
pub const TEXT_COLUMNS: u32 = 40;
/// See [`TEXT_COLUMNS`].
pub const FIRST_TEXT_ROW: u32 = 1;
/// See [`TEXT_COLUMNS`].
pub const LAST_TEXT_ROW: u32 = 23;
/// The ReGIS cell of PAR-10: a display cell 20 by 20 pixels and a unit
/// cell 15 by 20, 15 being the teletext cell's 12 dots at their 16/13
/// shape on a 20-pixel row (DEC-2049); row `r` at y = 10 + 20 (r - 1).
const REGIS_CELL: u32 = 20;
const REGIS_TOP_MARGIN: u32 = 10;
const REGIS_SETUP: &[u8] = b"S(E)T(S[20,20])T(U[15,20])";
/// The NAPLPS cell of PAR-10: 1/40 of the unit screen wide and 20 raster
/// lines high, written at a three-byte operand's precision (eight bits of
/// fraction); row `r` at the point whose raster line is 20 r.
const NAPLPS_ROW_LINES: u32 = 20;
const NAPLPS_FRACTION_BITS: u32 = 8;
/// The unit screen's height, 25/32 of its width, as the NAPLPS renderer
/// maps it onto its canvas.
const UNIT_HEIGHT_NUMERATOR: u64 = 25;
const UNIT_HEIGHT_DENOMINATOR: u64 = 32;
/// SO, SI, RESET clearing the screen to black, TEXT and POINT SET (ABS)
/// (ANSI X3.110-1983 sections 4.3, 5.3.2.1, 5.3.2.3 and 5.3.3.1); a data
/// byte has bit 7 set.
const SHIFT_OUT: u8 = 0x0E;
const SHIFT_IN: u8 = 0x0F;
const NAPLPS_RESET_CLEAR: [u8; 3] = [0x20, 0x48, 0x40];
const NAPLPS_TEXT: u8 = 0x22;
const NAPLPS_POINT_SET: u8 = 0x24;
const DATA_BIT: u8 = 0x40;
/// TEXT's two fixed-format bytes: no spacing change, path right, no
/// rotation; the cursor and the drawing point move together, no rows.
const NAPLPS_TEXT_FORMAT: [u8; 2] = [DATA_BIT, DATA_BIT];

/// `text`'s lines wrapped into rows of at most [`TEXT_COLUMNS`] cells, at
/// spaces where a line can be broken and through a word longer than a row,
/// as the teletext writer wraps a page (REQ-95).
///
/// # Errors
///
/// The encoder's refusal of a line's text, and more rows than 1 to 23.
pub fn wrap(
    text: &str,
    language: Option<&str>,
) -> Result<Vec<Vec<neoteletext_formats::picture_text::Stretch>>, String> {
    use neoteletext_formats::picture_text::{measure, stretches};
    let encode = |line: &str| stretches(line, language, true).map_err(|error| format!("{line:?}: {error}"));
    let cells = |line: &str| encode(line).map(|run| measure(&run).cells);
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut row = String::new();
        for word in line.split(' ') {
            let candidate = if row.is_empty() { word.to_owned() } else { format!("{row} {word}") };
            if cells(&candidate)? <= TEXT_COLUMNS {
                row = candidate;
                continue;
            }
            if !row.is_empty() {
                rows.push(encode(&row)?);
            }
            // A word longer than a row is cut.
            let mut rest: Vec<char> = word.chars().collect();
            row = String::new();
            while !rest.is_empty() {
                let mut taken = rest.len();
                while taken > 1 && cells(&rest.iter().take(taken).collect::<String>())? > TEXT_COLUMNS {
                    taken = taken.saturating_sub(1);
                }
                let piece: String = rest.drain(..taken).collect();
                if rest.is_empty() {
                    row = piece;
                } else {
                    rows.push(encode(&piece)?);
                }
            }
        }
        rows.push(encode(&row)?);
    }
    let most = usize::try_from(LAST_TEXT_ROW.saturating_sub(FIRST_TEXT_ROW).saturating_add(1)).unwrap_or(0);
    if rows.len() > most {
        return Err(format!("the text needs {} rows; a page has rows 1 to 23", rows.len()));
    }
    Ok(rows)
}

/// A page of rows as a ReGIS picture (PAR-10): the screen erased, the cell
/// of PAR-10, each row a text part at its place.
///
/// # Errors
///
/// The encoder's refusal of a row.
pub fn regis_page(rows: &[Vec<neoteletext_formats::picture_text::Stretch>]) -> Result<Vec<u8>, String> {
    let mut out = b"\x1bPp".to_vec();
    out.extend_from_slice(REGIS_SETUP);
    for (row, stretches) in (FIRST_TEXT_ROW..).zip(rows) {
        let y = REGIS_TOP_MARGIN.saturating_add(REGIS_CELL.saturating_mul(row.saturating_sub(1)));
        out.extend_from_slice(format!("P[0,{y}]").as_bytes());
        neoteletext_formats::picture_text::regis(stretches, &mut out).map_err(|error| error.to_string())?;
    }
    out.extend_from_slice(b"\x1b\\");
    Ok(out)
}

/// A unit-screen fraction `numerator / denominator` as a three-byte
/// operand's nine bits, the first the sign.
fn nine_bits(numerator: u64, denominator: u64) -> u64 {
    let scaled = numerator.saturating_mul(1 << NAPLPS_FRACTION_BITS);
    scaled.saturating_add(denominator / 2).checked_div(denominator).unwrap_or(0).min((1 << NAPLPS_FRACTION_BITS) - 1)
}

/// A two-dimensional three-byte operand of `x` and `y` (nine bits each,
/// three a byte, high bits first: section 5.3.2.2.5).
fn operand(x: u64, y: u64, out: &mut Vec<u8>) {
    for shift in [6_u32, 3, 0] {
        let part = |value: u64| u8::try_from((value >> shift) & 0b111).unwrap_or(0);
        out.push(DATA_BIT | (part(x) << 3) | part(y));
    }
}

/// A page of rows as a NAPLPS picture (PAR-10): the screen cleared, the
/// field of PAR-10, each row a text part at its place, SO before each
/// PDI.
///
/// # Errors
///
/// The encoder's refusal of a row.
pub fn naplps_page(rows: &[Vec<neoteletext_formats::picture_text::Stretch>]) -> Result<Vec<u8>, String> {
    let lines = u64::try_from(NAPLPS_LINES).unwrap_or(1);
    let unit_lines = lines.saturating_mul(UNIT_HEIGHT_DENOMINATOR);
    let mut out = vec![SHIFT_OUT];
    out.extend_from_slice(&NAPLPS_RESET_CLEAR);
    out.push(NAPLPS_TEXT);
    out.extend_from_slice(&NAPLPS_TEXT_FORMAT);
    let field_high = u64::from(NAPLPS_ROW_LINES).saturating_mul(UNIT_HEIGHT_NUMERATOR);
    operand(nine_bits(1, u64::from(TEXT_COLUMNS)), nine_bits(field_high, unit_lines), &mut out);
    let first = u64::try_from(NAPLPS_FIRST_LINE).unwrap_or(0);
    for (row, stretches) in (FIRST_TEXT_ROW..).zip(rows) {
        // The canvas's line of raster line 20 r, counted up from its last.
        let line = u64::from(NAPLPS_ROW_LINES.saturating_mul(row)).saturating_sub(first);
        let up = lines.saturating_sub(1).saturating_sub(line).saturating_mul(UNIT_HEIGHT_NUMERATOR);
        out.push(SHIFT_OUT);
        out.push(NAPLPS_POINT_SET);
        operand(0, nine_bits(up, unit_lines), &mut out);
        out.push(SHIFT_IN);
        neoteletext_formats::picture_text::naplps(stretches, &mut out).map_err(|error| error.to_string())?;
    }
    Ok(out)
}

/// The audio service a static channel's records are written on, as the
/// server writes them (DOC-1951: packets 4/31 of service 0).
const RECORD_SERVICE: u8 = 0;
/// The generation of a static picture's stream: its first and only.
const STATIC_GENERATION: u16 = 1;

/// One sub-page's picture as a static channel carries it (PAR-8): its
/// stream, its links and its cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PictureSource {
    /// The language of the stream, once one is read.
    pub language: Option<Language>,
    /// The stream's bytes, in order, of its latest generation.
    pub stream: Vec<u8>,
    /// The latest generation read.
    pub generation: u16,
    /// The links: each drawing or object and its target.
    pub links: Vec<(u16, Vec<u8>)>,
    /// The drawing or object that is the cursor, 0 for none.
    pub cursor: u16,
}

/// A sub-page's picture as records, each written by its encoder and so
/// checked by its reader: the stream as REGIS or NAPLPS records of
/// generation 1 from offset 0, then `DRAWING_LINKS` and `REGIS_CURSOR`
/// (PAR-8). Gives the packets' bytes.
///
/// # Errors
///
/// What a record's writer refuses, as text.
pub fn picture_packets(
    page: neoteletext_core::types::PageAddress,
    subcode: neoteletext_core::types::SubCode,
    language: Language,
    stream: &[u8],
    links: &[(u16, Vec<u8>)],
    cursor: Option<u16>,
) -> Result<Vec<u8>, String> {
    use neoteletext_web::records::{drawing_links, regis, regis_cursor};
    let mut out = Vec::new();
    let mut record_id = 0_u8;
    let mut offset = 0_u32;
    let fault = |fault: neoteletext_web::records::RecordFault| format!("{fault:?}");
    for chunk in stream.chunks(regis::MAX_DATA) {
        let record = regis::RegisRecord { page, subcode, generation: STATIC_GENERATION, offset, data: chunk };
        let mut write = |packet: neoteletext_core::packet::Packet| out.extend_from_slice(&packet.bytes);
        match language {
            Language::Regis => regis::encode(record, RECORD_SERVICE, record_id, &mut write),
            Language::Naplps => regis::encode_naplps(record, RECORD_SERVICE, record_id, &mut write),
        }
        .map_err(fault)?;
        record_id = record_id.wrapping_add(1);
        offset = offset.saturating_add(u32::try_from(chunk.len()).map_err(|_| "a chunk passes 32 bits".to_owned())?);
    }
    if !links.is_empty() {
        let links: Vec<drawing_links::DrawingLink<'_>> =
            links.iter().map(|(drawing, target)| drawing_links::DrawingLink { drawing: *drawing, target }).collect();
        let mut write = |packet: neoteletext_core::packet::Packet| out.extend_from_slice(&packet.bytes);
        let sent =
            drawing_links::encode(page, subcode, &links, RECORD_SERVICE, record_id, &mut write).map_err(fault)?;
        record_id = sent.next_record_id;
    }
    if let Some(drawing) = cursor {
        let record = regis_cursor::RegisCursorRecord { page, subcode, drawing };
        let mut write = |packet: neoteletext_core::packet::Packet| out.extend_from_slice(&packet.bytes);
        regis_cursor::encode(record, RECORD_SERVICE, record_id, &mut write).map_err(fault)?;
    }
    Ok(out)
}

/// The pictures a file of T42 packets carries, by page and sub-code, read
/// by the engine's record reader (PAR-7, PAR-8): each stream's latest
/// generation, a record out of order in it dropping the stream, as a
/// receiver drops it.
#[must_use]
pub fn pictures_in_t42(
    bytes: &[u8],
) -> std::collections::BTreeMap<(neoteletext_core::types::PageAddress, u16), PictureSource> {
    use neoteletext_core::consts::PACKET_BYTES;
    use neoteletext_web::records::{ChannelRecord, Read, RecordReader};
    let mut reader = RecordReader::new();
    let mut pictures: std::collections::BTreeMap<_, PictureSource> = std::collections::BTreeMap::new();
    for packet_bytes in bytes.as_chunks::<PACKET_BYTES>().0 {
        let packet = neoteletext_core::packet::Packet { bytes: *packet_bytes };
        let Read::Record(record) = reader.push(&packet) else { continue };
        match record {
            ChannelRecord::Regis(stream) | ChannelRecord::Naplps(stream) => {
                let language =
                    if matches!(record, ChannelRecord::Regis(_)) { Language::Regis } else { Language::Naplps };
                let source = pictures.entry((stream.page, stream.subcode.value())).or_default();
                if stream.offset == 0 {
                    *source = PictureSource {
                        language: Some(language),
                        generation: stream.generation,
                        ..PictureSource::default()
                    };
                }
                let at = u32::try_from(source.stream.len()).unwrap_or(u32::MAX);
                if source.generation == stream.generation && source.language == Some(language) && stream.offset == at {
                    source.stream.extend_from_slice(stream.data);
                } else {
                    source.stream.clear();
                    source.language = None;
                }
            }
            ChannelRecord::DrawingLinks(links) => {
                let source = pictures.entry((links.page, links.subcode.value())).or_default();
                if links.part == 0 {
                    source.links.clear();
                }
                source.links.extend(links.links().map(|link| (link.drawing, link.target.to_vec())));
            }
            ChannelRecord::RegisCursor(cursor) => {
                pictures.entry((cursor.page, cursor.subcode.value())).or_default().cursor = cursor.drawing;
            }
            _ => {}
        }
    }
    pictures.retain(|_, source| source.language.is_some());
    pictures
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "a test panics on what it rules out")]
    use super::*;

    // PAR-7: a ReGIS file of bare commands draws on the 800 by 480 screen.
    #[test]
    fn a_regis_file_draws_on_the_vt340_screen() {
        let (raster, findings) = draw(Language::Regis, b"W(I3)P[100,100]V[200,100]", &Fonts::default()).unwrap();
        assert_eq!((raster.width, raster.height), (800, 480));
        assert_eq!(raster.indices[100 * 800 + 150], 3);
        assert!(!findings.refused);
    }

    // PAR-7, PAR-12: text in a provided alphabet draws the engine's glyph.
    #[test]
    fn a_regis_provided_alphabet_draws_the_engine_glyphs() {
        let mut bytes = Vec::new();
        let run = neoteletext_formats::picture_text::stretches("Ω", Some("el"), false).unwrap();
        neoteletext_formats::picture_text::regis(&run, &mut bytes).unwrap();
        let (raster, _) = draw(Language::Regis, &bytes, &Fonts::default()).unwrap();
        assert!(raster.indices.iter().any(|&index| index != 0));
    }

    // PAR-7: a NAPLPS picture lies on lines 19 to 480 of the raster, the
    // border lines in the screen colour.
    #[test]
    fn a_naplps_file_draws_on_the_raster_from_line_19() {
        // SO, RECT FILLED from (0, 0): the whole unit screen.
        let (raster, findings) = draw(Language::Naplps, &[0x0E, 0x31, 0x5B, 0x7F, 0x7F], &Fonts::default()).unwrap();
        assert_eq!((raster.width, raster.height), (480, 500));
        assert_eq!(raster.indices[0], raster.indices[18 * 480], "the border");
        assert_ne!(raster.indices[100 * 480 + 100], raster.indices[0], "the picture");
        assert!(!findings.refused);
    }

    // PAR-7: `check` sees a NAPLPS stream past MAX_PROCESSED_BYTES as a
    // refusal.
    #[test]
    fn a_naplps_stream_past_the_processing_limit_is_refused() {
        // NUL is passed over (ANSI X3.110-1983 section 6.1), the cheapest byte to process.
        let bytes = vec![0; naplps::limits::MAX_PROCESSED_BYTES + 1];
        let (_, findings) = draw(Language::Naplps, &bytes, &Fonts::default()).unwrap();
        assert!(findings.refused, "{findings:?}");
    }

    // PAR-10: a line longer than 40 cells wraps; each row is a text part
    // drawn without a refusal in both languages.
    #[test]
    fn text_pages_wrap_and_draw_in_both_languages() {
        let long = "word ".repeat(12);
        let rows = wrap(long.trim_end(), None).unwrap();
        assert_eq!(rows.len(), 2, "60 cells wrap into two rows");
        for (text, language) in [
            ("Grüße aus Köln", Some("de")),
            ("Привет мир", Some("ru")),
            ("שלום עולם", Some("he")),
            ("مرحبا", Some("ar")),
        ] {
            let rows = wrap(text, language).unwrap();
            let (regis, found) = draw(Language::Regis, &regis_page(&rows).unwrap(), &Fonts::default()).unwrap();
            assert!(!found.refused && regis.indices.iter().any(|&index| index != 0), "{text}");
            let (naplps, found) = draw(Language::Naplps, &naplps_page(&rows).unwrap(), &Fonts::default()).unwrap();
            assert!(!found.refused, "{text}: {found:?}");
            assert_eq!(found.lines, Vec::<String>::new(), "{text}: nothing passed over");
            let border = naplps.indices[0];
            assert!(naplps.indices.iter().any(|&index| index != border), "{text}");
        }
        assert!(wrap(&"x\n".repeat(24), None).is_err(), "24 rows");
    }

    #[test]
    fn languages_by_name() {
        assert_eq!(language_of("a/b.regis"), Some(Language::Regis));
        assert_eq!(language_of("b.NAP"), Some(Language::Naplps));
        assert_eq!(language_of("b.naplps"), Some(Language::Naplps));
        assert_eq!(language_of("b.t42"), None);
    }
}
