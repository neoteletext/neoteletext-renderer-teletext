//! The ANIMATE record, type 23 (neoteletext-common `docs/animation.md`,
//! DEC-6524; ANIM-6): operations on the animation state of a page's
//! controls and objects, applied together.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub-code: 0x0000 to 0x3F7E
//! byte  4     count: 1 to 64
//! byte  5     reserved: 0
//! bytes 6-    the operations
//! ```
//!
//! The record is the same bytes whether the server sends it or a group of
//! a page's rule table holds it as a command (RULE-119), and one function
//! of `neoteletext-rules` reads both
//! ([`neoteletext_rules::animate::read_record`]). It names one page: the
//! first form's sub-code `0x3F7F`, "whichever sub-page is shown", is
//! withdrawn and refused (DEC-6599).

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::animate::{self, MAX_RECORD_BYTES, Operation};

use super::{RecordFault, TYPE_ANIMATE, take_place, write_packets};

/// The most bytes of an ANIMATE record's body.
pub const MAX_BYTES: usize = MAX_RECORD_BYTES;
const _: () = assert!(MAX_BYTES <= neoteletext_audio::MAX_FRAMED_BODY);

/// One ANIMATE record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnimateRecord<'a> {
    /// The page.
    pub page: PageAddress,
    /// Its sub-code (RULE-112).
    pub subcode: SubCode,
    /// How many operations: 1 to 64.
    pub count: u8,
    /// The record's body, which a receiver applies as it applies the same
    /// record run from a group (RULE-118).
    pub body: &'a [u8],
    /// The operations' bytes, checked: [`AnimateRecord::operations`] reads
    /// them.
    bytes: &'a [u8],
}

impl<'a> AnimateRecord<'a> {
    /// The operations, in order.
    pub fn operations(&self) -> impl Iterator<Item = Operation> + 'a {
        animate::operations(self.bytes)
    }
}

/// Reads an ANIMATE record's body.
///
/// # Errors
/// A page or a sub-code outside its range, the sub-code `0x3F7F`, a count
/// of 0 or over 64, a reserved byte that is not 0, an operation
/// [`neoteletext_rules::animate::read`] refuses, or bytes after the last
/// operation.
pub fn read(body: &[u8]) -> Result<AnimateRecord<'_>, RecordFault> {
    let (page, subcode, _) = take_place(body)?;
    let (header, bytes) = animate::read_record(body).map_err(|_| RecordFault::BadValue("animate"))?;
    Ok(AnimateRecord { page, subcode, count: header.count, body, bytes })
}

/// Writes an ANIMATE record of `operations` for `page` and `subcode` as
/// packets 4/31 of audio service `service`, record id `record_id`.
/// Returns the packets written.
///
/// # Errors
/// What the reader would refuse: no operation, more than 64, or one out
/// of range.
pub fn encode(
    page: PageAddress,
    subcode: SubCode,
    operations: &[Operation],
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BYTES];
    let length = animate::write_record(page.mpp(), subcode.value(), operations, &mut body)
        .map_err(|_| RecordFault::BadValue("animate"))?;
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    let back = read(body)?;
    if (back.page, back.subcode) != (page, subcode) || !back.operations().eq(operations.iter().copied()) {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_ANIMATE, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
