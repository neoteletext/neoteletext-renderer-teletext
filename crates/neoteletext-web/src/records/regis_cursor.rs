//! The `REGIS_CURSOR` record (REQ-6371): the numbered drawing of a
//! sub-page's ReGIS picture, or the object of its NAPLPS picture (renderer
//! parity PAR-2, REQ-6761: the meaning widened, the bytes and the name
//! unchanged), that is its cursor, the mark that shows where typing goes.
//! The channel draws it and moves it; a receiver blinks it.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub: the sub-code, 0x0000 to 0x3F7E
//! bytes 4-5   drawing or object: 1 to 65,535, or 0 for no cursor
//! ```
//!
//! It carries no input count: the cursor record of type 4 goes on
//! acknowledging input for the teletext page beneath the picture.

use neoteletext_core::limits::{LAST_SUBPAGE_CODE, SUBCODE_MASK};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::{RecordFault, TYPE_REGIS_CURSOR, take_u16, write_packets};

/// The record's bytes: page, sub-code and drawing.
pub const BYTES: usize = 2 + 2 + 2;

/// One `REGIS_CURSOR` record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisCursorRecord {
    /// The sub-page's page.
    pub page: PageAddress,
    /// Its sub-code.
    pub subcode: SubCode,
    /// The numbered drawing that is the cursor, or 0 for none.
    pub drawing: u16,
}

/// Reads a `REGIS_CURSOR` record's body.
///
/// # Errors
/// A page or sub-code outside its range, or a body of any length but
/// [`BYTES`].
pub fn read(body: &[u8]) -> Result<RegisCursorRecord, RecordFault> {
    if body.len() != BYTES {
        return Err(RecordFault::Length);
    }
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("regis cursor page"))?;
    let (subcode, rest) = take_u16(rest)?;
    if subcode > LAST_SUBPAGE_CODE || subcode & !SUBCODE_MASK != 0 {
        return Err(RecordFault::BadValue("regis cursor sub-code"));
    }
    let (drawing, _) = take_u16(rest)?;
    Ok(RegisCursorRecord { page, subcode: SubCode::new(subcode), drawing })
}

/// Writes a `REGIS_CURSOR` record as packets 4/31 of audio service
/// `service`, record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    record: RegisCursorRecord,
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
        .chain(record.drawing.to_be_bytes());
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    // The writer checks with the reader, so the two cannot disagree.
    if read(&body)? != record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_REGIS_CURSOR, &body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
    )]

    use super::*;
    use crate::records::{ChannelRecord, Read, RecordReader};

    fn record(drawing: u16) -> RegisCursorRecord {
        RegisCursorRecord { page: PageAddress::from_mpp(0x123).unwrap(), subcode: SubCode::new(0x0002), drawing }
    }

    /// REQ-6371: the record reaches a receiver's reader in one packet, its
    /// fields as written, with a drawing or with none.
    #[test]
    fn a_cursor_record_reaches_a_receiver_in_one_packet() {
        for drawing in [0, 40, 65_535] {
            let mut reader = RecordReader::new();
            let mut seen = None;
            let packets = encode(record(drawing), 3, 7, &mut |packet| {
                if let Read::Record(ChannelRecord::RegisCursor(read)) = reader.push(&packet) {
                    seen = Some(read);
                }
            })
            .unwrap();
            assert_eq!((packets, seen), (1, Some(record(drawing))));
        }
    }

    /// The body's layout, byte for byte.
    #[test]
    fn the_body_is_the_three_fields_in_order() {
        assert_eq!(read(&[0x01, 0x23, 0x00, 0x02, 0x00, 0x28]), Ok(record(40)));
    }

    #[test]
    fn anything_else_is_refused() {
        assert_eq!(read(&[0x01, 0x23, 0x00, 0x02, 0x00]), Err(RecordFault::Length));
        assert_eq!(read(&[0x01, 0x23, 0x00, 0x02, 0x00, 0x28, 0x00]), Err(RecordFault::Length));
        // Page 0x0FF does not exist; sub-code 0x3F7F names no sub-page.
        assert!(read(&[0x00, 0xFF, 0, 0, 0, 1]).is_err());
        assert!(read(&[0x01, 0x00, 0x3F, 0x7F, 0, 1]).is_err());
    }
}
