//! The SHEET record, type 26 (neoteletext-common `docs/animation.md`,
//! DEC-6524; ANIM-17): bytes of a page's sprite sheet, sent once and
//! named by number afterwards, in the REGIS record's form.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub-code: 0x0000 to 0x3F7E
//! byte  4     kind: 1 ReGIS, 2 NAPLPS
//! byte  5     reserved: 0
//! bytes 6-7   generation: which stream of this page's sheet the bytes are of
//! bytes 8-11  offset: where in that stream the bytes begin
//! bytes 12-   data: 0 to 8,172 bytes of the stream
//! ```
//!
//! A sheet is one stream of at most [`MAX_SHEET_BYTES`] for a page, a page
//! number and a sub-code together (DEC-6599, RULE-112), and a kind; a new
//! generation starts it again. It is never shared with another sub-code
//! of the same page number. The bytes are not read here:
//! the renderer bounds what they can cost.

use neoteletext_audio::MAX_FRAMED_BODY as MAX_BODY;
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::{RecordFault, TYPE_SHEET, take_place, take_u16, write_packets};
use crate::consts::MAX_SHEET_BYTES;

/// The fixed fields: page, kind, reserved, generation and offset.
const FIXED: usize = 2 + 2 + 1 + 1 + 2 + 4;
/// The most bytes of a sheet one record carries.
pub const MAX_DATA: usize = MAX_BODY - FIXED;

/// Which renderer a sheet is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SheetKind {
    /// 1: a ReGIS sheet, which defines hidden drawings.
    Regis,
    /// 2: a NAPLPS sheet, which defines macros, DRCS characters and
    /// textures.
    Naplps,
}

impl SheetKind {
    /// The kind's byte in the record.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Regis => 1,
            Self::Naplps => 2,
        }
    }

    /// The kind of `code`, or `None` for a byte no kind has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Regis),
            2 => Some(Self::Naplps),
            _ => None,
        }
    }
}

/// One SHEET record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SheetRecord<'a> {
    /// The page whose sheet it is.
    pub page: PageAddress,
    /// Its sub-code: a sheet belongs to one page (ANIM-17).
    pub subcode: SubCode,
    /// The renderer it is for.
    pub kind: SheetKind,
    /// Which stream of the page's sheet the bytes are of.
    pub generation: u16,
    /// Where in that stream the bytes begin.
    pub offset: u32,
    /// The bytes, at most [`MAX_DATA`].
    pub data: &'a [u8],
}

/// Reads a SHEET record's body.
///
/// # Errors
/// A page or a kind outside its range, a reserved byte that is not 0, a
/// stream that would pass [`MAX_SHEET_BYTES`], or a body cut short.
pub fn read(body: &[u8]) -> Result<SheetRecord<'_>, RecordFault> {
    let (page, subcode, rest) = take_place(body)?;
    let (&[kind, reserved], rest) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    let kind = SheetKind::from_code(kind).ok_or(RecordFault::BadValue("sheet kind"))?;
    if reserved != 0 {
        return Err(RecordFault::BadValue("sheet reserved byte"));
    }
    let (generation, rest) = take_u16(rest)?;
    let (high, rest) = take_u16(rest)?;
    let (low, data) = take_u16(rest)?;
    let offset = (u32::from(high) << 16) | u32::from(low);
    let end = usize::try_from(offset).ok().and_then(|offset| offset.checked_add(data.len()));
    if end.is_none_or(|end| end > MAX_SHEET_BYTES) {
        return Err(RecordFault::BadValue("sheet offset"));
    }
    Ok(SheetRecord { page, subcode, kind, generation, offset, data })
}

/// Writes a SHEET record as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse, and data over [`MAX_DATA`].
pub fn encode(
    record: SheetRecord<'_>,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BODY];
    let length = FIXED.checked_add(record.data.len()).filter(|&length| length <= MAX_BODY);
    let length = length.ok_or(RecordFault::Length)?;
    let target = body.get_mut(..length).ok_or(RecordFault::Length)?;
    let (fixed, data) = target.split_at_mut(FIXED);
    let fields = record
        .page
        .mpp()
        .to_be_bytes()
        .into_iter()
        .chain(record.subcode.value().to_be_bytes())
        .chain([record.kind.code(), 0])
        .chain(record.generation.to_be_bytes())
        .chain(record.offset.to_be_bytes());
    for (byte, field) in fixed.iter_mut().zip(fields) {
        *byte = field;
    }
    data.copy_from_slice(record.data);
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    if read(body)? != record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_SHEET, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
