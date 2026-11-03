//! The page file's lines: page 100's rows and enhancement packets, and at
//! Levels 2.5 and 3.5 with objects, the Public Object Page 1A0 after it.

use neoteletext_core::consts::{
    BLACK_SUBSTITUTION_SHIFT, CLUT_ENTRIES, COLOUR_MAP_ENTRIES, COLOUR_PRIMARY_BITS, COLOUR_WORD_BITS,
    COLOUR_WORDS_START, COMPOSITIONAL_LINKS, DESIGNATION_LEVEL_35_DATA, DESIGNATION_LINKS_LEVEL_25,
    DESIGNATION_PAGE_DATA, ESC, LEVEL_BIT_25, LEVEL_BIT_35, LINK_FUNCTION_POP, LINK_MARKER_FIRST_SHIFT,
    LINK_SUBPAGES_MASK, LINK_SUBPAGES_SHIFT, LINK_TENS_SHIFT, LINK_UNITS_SHIFT, LINK_VALIDITY_SHIFT, LOCAL_TRIPLETS,
    MODE_OBJECT_DEFINITION_FIRST, MODE_OBJECT_INVOCATION_FIRST, MODE_TERMINATION, NIBBLE_MASK, OBJECT_ADDRESS_BIT6,
    OBJECT_BODY_TRIPLETS, OBJECT_DEFINITION_FIRST_PACKET, OBJECT_POINTER_BITS, OBJECT_POINTER_UNUSED,
    OBJECT_SOURCE_SHIFT, PACKET_ENHANCEMENT, PACKET_LINKS, PACKET_PAGE_ENHANCEMENT, PACKET_ROW_25, PAGE_FUNCTION_POP,
    ROW_ADDRESS_LAST, ROW_ADDRESS_RESERVED_DEFAULT, SPACE, TRIPLET_ADDRESS_BITS, TRIPLET_DATA_BITS, TRIPLET_MODE_BITS,
    TRIPLETS_PER_PACKET, TTI_ESCAPE_OFFSET,
};
use neoteletext_core::limits::LINK_NULL_PAGE;
use neoteletext_formats::consts::{
    PAGE_CODING_TRIPLETS, TERMINATION_FINAL_LAST, TERMINATION_INTERMEDIATE_LAST, TERMINATION_LOCAL_DATA_END,
    TTI_FL_LINKS, TTI_PACKED_CHARACTER_BASE, TTI_PS_TRANSMIT, TTI_TRIPLET_CHARACTER_BITS, TTI_TRIPLET_CHARACTER_MASK,
};
use neoteletext_formats::{Item, Reader, tti::TtiReader, write::T42Writer};

use crate::error::Error;
use crate::page::{Rgb, first_column};

/// The page the picture is written as: magazine 1, page 00, page 100.
pub const PAGE_NUMBER: u16 = 0x100;
/// The Public Object Page that carries the rows' objects: page 1A0, a
/// page number no viewer can key, in the picture's magazine.
const OBJECT_PAGE: u16 = 0x1A0;
/// The page-number byte of a page address: its tens and units.
const PAGE_BYTE: u16 = 0xFF;
/// The description the file gives its page.
const DESCRIPTION: &str = "Picture";
/// Sub-pages of an object page: S1 is four bits (EN 300 706 Annex A.1).
pub const OBJECT_SUBPAGES: usize = 16;
/// Triplets of the page's own X/26 that carry data: all but the
/// terminator.
pub const LOCAL_DATA_TRIPLETS: usize = LOCAL_TRIPLETS as usize - 1;
/// Triplets of an object's body: an object sub-page less its definition
/// and its terminator.
pub const OBJECT_BODY: usize = OBJECT_BODY_TRIPLETS as usize - 2;
/// The packets of an object page's definition area that are rows, X/3 to
/// X/25; the rest go in X/26 (clause 10.5.1.2).
const DEFINITION_ROW_PACKETS: usize = (PACKET_ROW_25 - OBJECT_DEFINITION_FIRST_PACKET + 1) as usize;
/// Shift of a triplet's mode above its address (Table 28).
const MODE_SHIFT: u32 = TRIPLET_ADDRESS_BITS;
/// Shift of a triplet's data above its mode.
const DATA_SHIFT: u32 = TRIPLET_ADDRESS_BITS + TRIPLET_MODE_BITS;
/// Mode 00010 of the row address group, reserved (Table 28): the filler of
/// an object page's pointer packets before their first pointer.
const MODE_ROW_RESERVED: u32 = 0x02;
/// Source bits S1 S0 of a Public Object, 10 (Table 28).
const SOURCE_PUBLIC: u32 = 0b10;
/// Both levels, the usage of an object definition and the validity of a
/// link (Tables 15 and 28).
const BOTH_LEVELS: u32 = (LEVEL_BIT_25 | LEVEL_BIT_35) as u32;

/// A triplet's 18 data bits from its fields.
pub const fn triplet(address: u32, mode: u32, data: u32) -> u32 {
    address | mode << MODE_SHIFT | data << DATA_SHIFT
}

/// A Termination Marker of `kind` (Table 28).
const fn termination(kind: u8) -> u32 {
    triplet(ROW_ADDRESS_LAST as u32, MODE_TERMINATION as u32, kind as u32)
}

/// The lines every page starts with: its description, number, sub-code,
/// status, and Fastext links to itself, so row 24 is shown (Annex H).
fn page_start() -> Vec<String> {
    let links = vec![format!("{PAGE_NUMBER:03X}"); TTI_FL_LINKS].join(",");
    vec![
        format!("DE,{DESCRIPTION}"),
        format!("PN,{PAGE_NUMBER:03X}00"),
        "SC,0000".to_string(),
        format!("PS,{TTI_PS_TRANSMIT:04X}"),
        format!("FL,{links}"),
    ]
}

/// The row lines `OL,n,` of `codes`, each row's codes from its first
/// column; attribute codes are written escaped.
#[allow(clippy::arithmetic_side_effects, reason = "row numbers within the page")]
pub fn row_lines(codes: &[Vec<u8>], first_row: usize) -> Vec<String> {
    codes
        .iter()
        .enumerate()
        .map(|(index, codes)| {
            let row = index + first_row;
            let mut text = format!("OL,{row},");
            text.extend(core::iter::repeat_n(char::from(SPACE), first_column(row)));
            for &code in codes {
                if code < SPACE {
                    text.push(char::from(ESC));
                    text.push(char::from(code | TTI_ESCAPE_OFFSET));
                } else {
                    text.push(char::from(code));
                }
            }
            text
        })
        .collect()
}

/// A packed enhancement line: the designation code, then each triplet as
/// three characters of six bits, low bits first.
fn packed(designation: u8, values: &[u32]) -> String {
    let mut text = String::with_capacity(values.len().saturating_mul(3).saturating_add(1));
    text.push(char::from(TTI_PACKED_CHARACTER_BASE | designation));
    for &value in values {
        for shift in (0..TRIPLET_DATA_BITS).step_by(TTI_TRIPLET_CHARACTER_BITS as usize) {
            let bits = value.checked_shr(shift).unwrap_or(0) & TTI_TRIPLET_CHARACTER_MASK;
            text.push(char::from(TTI_PACKED_CHARACTER_BASE | u8::try_from(bits).unwrap_or(0)));
        }
    }
    text
}

/// The thirteen triplets of an X/28 colour packet of sixteen colour words
/// (Tables 4 and 10), with Black Background Colour Substitution set so each
/// row's Full Row Colour shows behind black (triplet 13).
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "bit positions within the packet's 234 bits"
)]
fn colour_packet(words: &[Rgb]) -> Vec<u32> {
    let mut bits = [0u32; TRIPLETS_PER_PACKET * TRIPLET_DATA_BITS as usize];
    for (index, &[red, green, blue]) in words.iter().enumerate() {
        let word = red | green << COLOUR_PRIMARY_BITS | blue << (2 * COLOUR_PRIMARY_BITS);
        for k in 0..COLOUR_WORD_BITS {
            bits[COLOUR_WORDS_START + index * COLOUR_WORD_BITS + k] = u32::from(word >> k & 1 == 1);
        }
    }
    bits[(TRIPLETS_PER_PACKET - 1) * TRIPLET_DATA_BITS as usize + BLACK_SUBSTITUTION_SHIFT as usize] = 1;
    bits.chunks(TRIPLET_DATA_BITS as usize)
        .map(|chunk| chunk.iter().enumerate().map(|(k, bit)| bit << k).sum())
        .collect()
}

/// `values` filled with `fill` to whole packets.
fn padded(mut values: Vec<u32>, fill: u32) -> Vec<u32> {
    while !values.len().is_multiple_of(TRIPLETS_PER_PACKET) {
        values.push(fill);
    }
    values
}

/// An X/27/4 link's two triplets (Table 15): a function and validity, the
/// page in its own magazine, and the sub-pages it covers.
const fn link(function: u32, validity: u32, page: u32, subpages: u32) -> [u32; 2] {
    let nibble = NIBBLE_MASK as u32;
    [
        function
            | validity << LINK_VALIDITY_SHIFT
            | (page & nibble) << LINK_UNITS_SHIFT
            | 1 << LINK_MARKER_FIRST_SHIFT
            | (page >> 4 & nibble) << LINK_TENS_SHIFT,
        subpages << LINK_SUBPAGES_SHIFT,
    ]
}

/// What the enhanced encoder hands the writer.
pub struct Enhanced<'a> {
    /// Level 3.5, whose X/28/4 redefines CLUTs 0 and 1.
    pub level_35: bool,
    /// The Colour Map, each entry's primaries 0 to 15.
    pub colour_map: &'a [Rgb; COLOUR_MAP_ENTRIES],
    /// Each row's Level 1 codes from its first column.
    pub level_1: &'a [Vec<u8>],
    /// Each row's triplets.
    pub row_triplets: &'a [Vec<u32>],
    /// Carry the rows' triplets in objects.
    pub objects: bool,
    /// 0, or 1 when row 0 is the header's.
    pub first_row: usize,
}

/// The rows' triplets in objects of at most [`OBJECT_BODY`] triplets, a
/// row never split.
#[allow(clippy::arithmetic_side_effects, reason = "lengths within an object page")]
pub fn objects_of(row_triplets: &[Vec<u32>]) -> Result<Vec<Vec<u32>>, Error> {
    let mut objects: Vec<Vec<u32>> = vec![Vec::new()];
    for row in row_triplets {
        if objects.last().is_none_or(|object| object.len() + row.len() > OBJECT_BODY) {
            objects.push(Vec::new());
        }
        if let Some(object) = objects.last_mut() {
            object.extend_from_slice(row);
        }
    }
    if objects.len() > OBJECT_SUBPAGES {
        return Err(Error::TooManyObjectSubpages { needed: objects.len() });
    }
    Ok(objects)
}

/// The lines of an enhanced page, and the object sub-pages it used.
#[allow(clippy::arithmetic_side_effects, reason = "positions within the page and its object page")]
pub fn enhanced_lines(page: &Enhanced<'_>) -> Result<(Vec<String>, usize), Error> {
    let mut lines = page_start();
    let enhancement = PACKET_PAGE_ENHANCEMENT;
    lines.push(format!(
        "OL,{enhancement},{}",
        packed(DESIGNATION_PAGE_DATA, &colour_packet(&page.colour_map[2 * CLUT_ENTRIES..]))
    ));
    if page.level_35 {
        lines.push(format!(
            "OL,{enhancement},{}",
            packed(DESIGNATION_LEVEL_35_DATA, &colour_packet(&page.colour_map[..2 * CLUT_ENTRIES]))
        ));
    }
    let objects = if page.objects { objects_of(page.row_triplets)? } else { Vec::new() };
    let mut local: Vec<u32> = if page.objects {
        let no_link = link(0, 0, u32::from(LINK_NULL_PAGE), LINK_SUBPAGES_MASK);
        let pop_link = link(
            u32::from(LINK_FUNCTION_POP),
            BOTH_LEVELS,
            u32::from(OBJECT_PAGE & PAGE_BYTE),
            (1 << objects.len()) - 1,
        );
        // Link 1 of X/27/4 is the POP link (Table 16); the rest point
        // nowhere.
        let mut link_triplets: Vec<u32> = Vec::new();
        for position in 0..COMPOSITIONAL_LINKS {
            link_triplets.extend_from_slice(if position == 1 { &pop_link } else { &no_link });
        }
        link_triplets.push(0);
        lines.push(format!("OL,{PACKET_LINKS},{}", packed(DESIGNATION_LINKS_LEVEL_25, &link_triplets)));
        (0..objects.len() as u32)
            .map(|subpage| {
                triplet(
                    u32::from(OBJECT_ADDRESS_BIT6) | SOURCE_PUBLIC << OBJECT_SOURCE_SHIFT,
                    u32::from(MODE_OBJECT_INVOCATION_FIRST),
                    subpage,
                )
            })
            .collect()
    } else {
        page.row_triplets.iter().flatten().copied().collect()
    };
    local.push(termination(TERMINATION_LOCAL_DATA_END));
    let local = padded(local, termination(TERMINATION_LOCAL_DATA_END));
    for (designation, chunk) in local.chunks(TRIPLETS_PER_PACKET).enumerate() {
        lines.push(format!("OL,{PACKET_ENHANCEMENT},{}", packed(designation as u8, chunk)));
    }
    lines.extend(row_lines(page.level_1, page.first_row));
    let filler = triplet(u32::from(ROW_ADDRESS_RESERVED_DEFAULT), MODE_ROW_RESERVED, 0);
    let unused = u32::from(OBJECT_POINTER_UNUSED) | u32::from(OBJECT_POINTER_UNUSED) << OBJECT_POINTER_BITS;
    for (subpage, body) in objects.iter().enumerate() {
        lines.push(format!("PN,{OBJECT_PAGE:03X}{subpage:02}"));
        lines.push(format!("SC,{subpage:04X}"));
        lines.push(format!("PS,{TTI_PS_TRANSMIT:04X}"));
        lines.push(format!("PF,{PAGE_FUNCTION_POP},{PAGE_CODING_TRIPLETS}"));
        // The one Active object of each sub-page starts at triplet 0 of the
        // definition area (clause 10.5.1.2).
        let mut pointers = vec![filler, u32::from(OBJECT_POINTER_UNUSED) << OBJECT_POINTER_BITS];
        pointers.resize(TRIPLETS_PER_PACKET, unused);
        lines.push(format!("OL,1,{}", packed(1, &pointers)));
        let mut pointers = vec![filler];
        pointers.resize(TRIPLETS_PER_PACKET, unused);
        lines.push(format!("OL,2,{}", packed(1, &pointers)));
        let definition = triplet(
            u32::from(OBJECT_ADDRESS_BIT6) | BOTH_LEVELS << OBJECT_SOURCE_SHIFT,
            u32::from(MODE_OBJECT_DEFINITION_FIRST),
            subpage as u32,
        );
        let last = subpage + 1 == objects.len();
        let end = termination(if last { TERMINATION_FINAL_LAST } else { TERMINATION_INTERMEDIATE_LAST });
        let mut definitions = vec![definition];
        definitions.extend_from_slice(body);
        definitions.push(end);
        let definitions = padded(definitions, end);
        for (k, chunk) in definitions.chunks(TRIPLETS_PER_PACKET).enumerate() {
            if k < DEFINITION_ROW_PACKETS {
                lines.push(format!("OL,{},{}", usize::from(OBJECT_DEFINITION_FIRST_PACKET) + k, packed(0, chunk)));
            } else {
                lines.push(format!("OL,{PACKET_ENHANCEMENT},{}", packed((k - DEFINITION_ROW_PACKETS) as u8, chunk)));
            }
        }
    }
    Ok((lines, objects.len()))
}

/// The lines of a Level 1 page.
pub fn level_1_lines(codes: &[Vec<u8>], first_row: usize) -> Vec<String> {
    let mut lines = page_start();
    lines.extend(row_lines(codes, first_row));
    lines
}

/// The page file's text: its lines, each ended.
pub fn text(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The TTI page file encoded as T42 packets by the engine's reader and
/// writer, in transmission order.
pub fn to_t42(tti: &str) -> Result<Vec<u8>, Error> {
    let mut reader = TtiReader::new();
    let mut pages = Vec::new();
    let mut keep = |item: Item<'_>| {
        if let Item::Page(page) = item {
            pages.push(page.clone());
        }
    };
    reader.feed(tti.as_bytes(), &mut keep);
    reader.finish(&mut keep);
    let references: Vec<_> = pages.iter().collect();
    let mut out = Vec::new();
    T42Writer::new(&mut out).write_pages(&references).map_err(|error| Error::T42(error.to_string()))?;
    Ok(out)
}
