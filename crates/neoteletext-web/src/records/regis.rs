//! The REGIS record (DEC-6335, DEC-6269 item 6): bytes of a sub-page's
//! ReGIS, the vector graphics language a receiver's ReGIS renderer draws
//! in place of the sub-page's teletext page.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub: the sub-code, 0x0000 to 0x3F7E
//! bytes 4-5   generation: which stream of this sub-page the bytes are of
//! bytes 6-9   offset: where in that stream the bytes begin
//! bytes 10-   data: 0 to 8,174 bytes of the stream
//! ```
//!
//! A sub-page's ReGIS is one stream of bytes, sent in as many records as it
//! needs. A new generation starts the stream again at offset 0, and its
//! receiver starts a new renderer for the sub-page; within a generation
//! each record's offset is where the last one ended, so a receiver that
//! lost a record knows it and waits for the next generation. The bytes are
//! not read here: the renderer bounds what they can cost.

use neoteletext_audio::MAX_FRAMED_BODY as MAX_BODY;
use neoteletext_core::limits::LAST_SUBPAGE_CODE;
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::{RecordFault, TYPE_NAPLPS, TYPE_REGIS, take_u16, write_packets};

/// The fixed fields: page, sub-code, generation and offset.
const FIXED: usize = 2 + 2 + 2 + 4;
/// The most bytes of ReGIS one record carries.
pub const MAX_DATA: usize = MAX_BODY - FIXED;

/// One REGIS record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisRecord<'a> {
    /// The sub-page's page.
    pub page: PageAddress,
    /// Its sub-code.
    pub subcode: SubCode,
    /// Which stream of the sub-page the bytes are of.
    pub generation: u16,
    /// Where in that stream the bytes begin.
    pub offset: u32,
    /// The bytes, at most [`MAX_DATA`].
    pub data: &'a [u8],
}

/// Reads a REGIS record's body.
///
/// # Errors
/// A page or sub-code outside its range, a stream that would pass the end
/// of a 32-bit offset, or a body cut short.
pub fn read(body: &[u8]) -> Result<RegisRecord<'_>, RecordFault> {
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("regis page"))?;
    let (subcode, rest) = take_u16(rest)?;
    if subcode > LAST_SUBPAGE_CODE || subcode & !neoteletext_core::limits::SUBCODE_MASK != 0 {
        return Err(RecordFault::BadValue("regis sub-code"));
    }
    let (generation, rest) = take_u16(rest)?;
    let (high, rest) = take_u16(rest)?;
    let (low, data) = take_u16(rest)?;
    let offset = (u32::from(high) << 16) | u32::from(low);
    let length = u32::try_from(data.len()).map_err(|_| RecordFault::Length)?;
    if offset.checked_add(length).is_none() {
        return Err(RecordFault::BadValue("regis offset"));
    }
    Ok(RegisRecord { page, subcode: SubCode::new(subcode), generation, offset, data })
}

/// Writes a REGIS record as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse, and data over [`MAX_DATA`].
pub fn encode(
    record: RegisRecord<'_>,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    encode_as(TYPE_REGIS, record, service, record_id, out)
}

/// Writes a NAPLPS record (REQ-6408 item 2), whose body is the REGIS
/// record's with a sub-page's NAPLPS for its data, as packets 4/31 of
/// audio service `service`, record id `record_id`. Returns the packets
/// written.
///
/// # Errors
/// What the reader would refuse, and data over [`MAX_DATA`].
pub fn encode_naplps(
    record: RegisRecord<'_>,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    encode_as(TYPE_NAPLPS, record, service, record_id, out)
}

/// Writes a stream record of type `kind`.
fn encode_as(
    kind: u8,
    record: RegisRecord<'_>,
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
    write_packets(kind, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
    )]

    use super::*;
    use crate::records::{ChannelRecord, Read, RecordReader};

    fn record(data: &[u8]) -> RegisRecord<'_> {
        let page = PageAddress::from_mpp(0x123).unwrap();
        RegisRecord { page, subcode: SubCode::new(0x0002), generation: 7, offset: 0x0001_0203, data }
    }

    /// DEC-6335: a REGIS record of any length up to the frame's reaches a
    /// receiver's reader whole, its fields as written.
    #[test]
    fn a_regis_record_reaches_a_receiver() {
        let long = [b'V'; MAX_DATA];
        for data in [b"".as_slice(), b"P[100,100]V[200,200]", &long] {
            let mut reader = RecordReader::new();
            let mut seen = None;
            encode(record(data), 3, 7, &mut |packet| {
                if let Read::Record(ChannelRecord::Regis(read)) = reader.push(&packet) {
                    seen = Some((read.page.mpp(), read.subcode.value(), read.generation, read.offset, read.data.len()));
                    assert_eq!(read.data, data);
                }
            })
            .unwrap();
            assert_eq!(seen, Some((0x123, 0x0002, 7, 0x0001_0203, data.len())));
        }
    }

    /// REQ-6408 item 2: a NAPLPS record is the same body under its own
    /// type, so a receiver's reader tells the two apart and neither is
    /// read as the other.
    #[test]
    fn a_naplps_record_reaches_a_receiver_as_naplps() {
        let data = b"\x1b\x22\x46 hello".as_slice();
        let mut reader = RecordReader::new();
        let mut seen = None;
        let mut others = 0;
        encode_naplps(record(data), 3, 7, &mut |packet| match reader.push(&packet) {
            Read::Record(ChannelRecord::Naplps(read)) => {
                seen = Some((read.page.mpp(), read.generation, read.offset, read.data.to_vec()));
            }
            Read::Record(_) => others += 1,
            _ => {}
        })
        .unwrap();
        assert_eq!(others, 0, "read as nothing else");
        assert_eq!(seen, Some((0x123, 7, 0x0001_0203, data.to_vec())));
        let mut regis = RecordReader::new();
        encode(record(data), 3, 8, &mut |packet| {
            assert!(
                !matches!(regis.push(&packet), Read::Record(ChannelRecord::Naplps(_))),
                "a REGIS record is not NAPLPS"
            );
        })
        .unwrap();
    }

    /// The body's layout, byte for byte.
    #[test]
    fn the_body_is_the_five_fields_in_order() {
        let body = [0x01, 0x23, 0x00, 0x02, 0x00, 0x07, 0x00, 0x01, 0x02, 0x03, b'V', b'1'];
        assert_eq!(read(&body), Ok(record(b"V1")));
    }

    #[test]
    fn anything_else_is_refused() {
        assert_eq!(read(&[0x01, 0x23, 0x00, 0x02, 0x00, 0x07, 0x00, 0x01, 0x02]), Err(RecordFault::Length));
        // Page 0x0FF does not exist; sub-code 0x3F7F names no sub-page.
        assert!(read(&[0x00, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(read(&[0x01, 0x00, 0x3F, 0x7F, 0, 0, 0, 0, 0, 0]).is_err());
        // The stream would pass the end of a 32-bit offset.
        assert!(read(&[0x01, 0x00, 0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, b'V']).is_err());
        let over = [0_u8; MAX_DATA + 1];
        assert!(encode(record(&over), 3, 7, &mut |_| {}).is_err());
    }
}
