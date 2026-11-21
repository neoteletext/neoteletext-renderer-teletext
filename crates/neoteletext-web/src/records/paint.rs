//! The PAINT record, type 34 (DEC-7031 item 1; REQ-7024, REQ-7025): how
//! one generation of a page's ReGIS or NAPLPS stream is painted, sent
//! immediately before that generation's first REGIS or NAPLPS record.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub-code: 0x0000 to 0x3F7E
//! byte  4     language: 1 ReGIS, 2 NAPLPS (the SHEET record's kinds)
//! byte  5     reserved: 0
//! bytes 6-7   generation: the stream generation the values are for
//! bytes 8-11  line speed in bits a second: 0 none, else 300 to 115,200
//! bytes 12-13 default paint rate: 0 none, else 1 to 32,767; ReGIS only
//! ```
//!
//! Every integer is big-endian. A record of any other length, with a
//! reserved byte that is not 0, a language no renderer has, a value
//! outside its range, a rate for NAPLPS or neither value is refused whole,
//! and the picture then paints at once. Nothing here paces a stream: a
//! receiver turns the speed into bytes a tick with
//! [`crate::consts::line_bytes`], and gives the rate to its ReGIS renderer.

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::sheet::SheetKind;
use super::{RecordFault, TYPE_PAINT, take_place, take_u16, write_packets};
use crate::consts::{MAX_DEFAULT_PAINT_RATE, MAX_LINE_SPEED, MIN_LINE_SPEED};

/// The record's bytes: page, sub-code, language, reserved, generation,
/// line speed and default paint rate.
pub const BYTES: usize = 2 + 2 + 1 + 1 + 2 + 4 + 2;

/// One PAINT record, checked: at least one of its two values is stated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaintRecord {
    /// The page whose picture it is for.
    pub page: PageAddress,
    /// Its sub-code: a page is its number and its sub-code together.
    pub subcode: SubCode,
    /// Which of the page's streams: its ReGIS or its NAPLPS.
    pub language: SheetKind,
    /// The generation of that stream the values are for.
    pub generation: u16,
    /// The line speed the stream's bytes are fed at, in bits a second,
    /// [`MIN_LINE_SPEED`] to [`MAX_LINE_SPEED`] (REQ-7024), or `None`.
    pub line_speed: Option<u32>,
    /// The paint rate taken by every ReGIS numbered drawing that sets none
    /// of its own, 1 to [`MAX_DEFAULT_PAINT_RATE`] (REQ-7025), or `None`.
    /// Never stated for NAPLPS.
    pub default_rate: Option<u16>,
}

/// Reads a PAINT record's body.
///
/// # Errors
/// A body of any length but [`BYTES`], a page, a sub-code or a language
/// outside its range, a reserved byte that is not 0, a line speed or a
/// rate outside its range, a rate for NAPLPS, or neither value stated.
pub fn read(body: &[u8]) -> Result<PaintRecord, RecordFault> {
    if body.len() != BYTES {
        return Err(RecordFault::Length);
    }
    let (page, subcode, rest) = take_place(body)?;
    let (&[language, reserved], rest) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    let language = SheetKind::from_code(language).ok_or(RecordFault::BadValue("paint language"))?;
    if reserved != 0 {
        return Err(RecordFault::BadValue("paint reserved byte"));
    }
    let (generation, rest) = take_u16(rest)?;
    let (high, rest) = take_u16(rest)?;
    let (low, rest) = take_u16(rest)?;
    let (rate, _) = take_u16(rest)?;
    let speed = (u32::from(high) << 16) | u32::from(low);
    let line_speed = match speed {
        0 => None,
        MIN_LINE_SPEED..=MAX_LINE_SPEED => Some(speed),
        _ => return Err(RecordFault::BadValue("paint line speed")),
    };
    let default_rate = match rate {
        0 => None,
        1..=MAX_DEFAULT_PAINT_RATE if language == SheetKind::Regis => Some(rate),
        1..=MAX_DEFAULT_PAINT_RATE => return Err(RecordFault::BadValue("paint rate for NAPLPS")),
        _ => return Err(RecordFault::BadValue("paint default rate")),
    };
    if line_speed.is_none() && default_rate.is_none() {
        return Err(RecordFault::BadValue("paint with no value"));
    }
    Ok(PaintRecord { page, subcode, language, generation, line_speed, default_rate })
}

/// Writes a PAINT record as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse: among them a value given as `Some(0)`,
/// which reads back as none.
pub fn encode(
    record: PaintRecord,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; BYTES];
    let fields = record
        .page
        .mpp()
        .to_be_bytes()
        .into_iter()
        .chain(record.subcode.value().to_be_bytes())
        .chain([record.language.code(), 0])
        .chain(record.generation.to_be_bytes())
        .chain(record.line_speed.unwrap_or(0).to_be_bytes())
        .chain(record.default_rate.unwrap_or(0).to_be_bytes());
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    // The writer checks with the reader, so the two cannot disagree.
    if read(&body)? != record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_PAINT, &body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
