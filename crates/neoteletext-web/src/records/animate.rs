//! The ANIMATE record, type 23 (neoteletext-common `docs/animation.md`,
//! DEC-6524; ANIM-6): operations on the animation state of a sub-page's
//! controls and objects, applied together.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub: the sub-code, or 0x3F7F for whichever sub-page is shown
//! byte  4     count: 1 to 64
//! byte  5     reserved: 0
//! bytes 6-    the operations
//! ```
//!
//! An operation is the same bytes here as in a rule's action, and one
//! function of `neoteletext-rules` reads both
//! ([`neoteletext_rules::animate::read`]).

use neoteletext_core::limits::SUBCODE_ANY;
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::animate::{self, MAX_OPERATION_BYTES, Operation, Place};
use neoteletext_rules::limits::MAX_ANIMATE_OPERATIONS;

use super::{RecordFault, TYPE_ANIMATE, take_place, take_u16, write_packets};

/// The fixed fields: page, sub-code, count and the reserved byte.
const FIXED: usize = 2 + 2 + 1 + 1;
/// The most bytes of an ANIMATE record's body.
pub const MAX_BYTES: usize = FIXED + MAX_ANIMATE_OPERATIONS * MAX_OPERATION_BYTES;
const _: () = assert!(MAX_BYTES <= neoteletext_audio::MAX_FRAMED_BODY);

/// One ANIMATE record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnimateRecord<'a> {
    /// The page.
    pub page: PageAddress,
    /// The sub-page, or `None` for whichever sub-page of the page is
    /// shown.
    pub subcode: Option<SubCode>,
    /// How many operations: 1 to 64.
    pub count: u8,
    /// The operations' bytes, checked: [`AnimateRecord::operations`] reads
    /// them.
    bytes: &'a [u8],
}

impl<'a> AnimateRecord<'a> {
    /// The operations, in order.
    pub fn operations(&self) -> impl Iterator<Item = Operation> + 'a {
        let mut rest = self.bytes;
        core::iter::from_fn(move || {
            let (operation, used) = animate::read(rest, Place::Record).ok()?;
            rest = rest.get(used..).unwrap_or(&[]);
            Some(operation)
        })
    }
}

/// Reads an ANIMATE record's body.
///
/// # Errors
/// A page or sub-code outside its range, a count of 0 or over 64, a
/// reserved byte that is not 0, an operation
/// [`neoteletext_rules::animate::read`] refuses, or bytes after the last
/// operation.
pub fn read(body: &[u8]) -> Result<AnimateRecord<'_>, RecordFault> {
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("animate page"))?;
    let (sub, rest) = take_u16(rest)?;
    let subcode = if sub == SUBCODE_ANY { None } else { Some(take_place(body)?.1) };
    let (&[count, reserved], bytes) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    if count == 0 || usize::from(count) > MAX_ANIMATE_OPERATIONS {
        return Err(RecordFault::BadValue("animate count"));
    }
    if reserved != 0 {
        return Err(RecordFault::BadValue("animate reserved byte"));
    }
    let mut rest = bytes;
    for _ in 0..count {
        let (_, used) = animate::read(rest, Place::Record).map_err(|_| RecordFault::BadValue("animate operation"))?;
        rest = rest.get(used..).ok_or(RecordFault::Length)?;
    }
    if !rest.is_empty() {
        return Err(RecordFault::Length);
    }
    Ok(AnimateRecord { page, subcode, count, bytes })
}

/// Writes an ANIMATE record of `operations` for `page` and `subcode`
/// (`None` for whichever sub-page is shown) as packets 4/31 of audio
/// service `service`, record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse: no operation, more than 64, or one out
/// of range.
pub fn encode(
    page: PageAddress,
    subcode: Option<SubCode>,
    operations: &[Operation],
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BYTES];
    let count = u8::try_from(operations.len()).map_err(|_| RecordFault::BadValue("animate count"))?;
    let sub = subcode.map_or(SUBCODE_ANY, SubCode::value);
    let fields = page.mpp().to_be_bytes().into_iter().chain(sub.to_be_bytes()).chain([count, 0]);
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    let mut length = FIXED;
    for operation in operations {
        let room = body.get_mut(length..).ok_or(RecordFault::Length)?;
        let used =
            animate::write(operation, Place::Record, room).map_err(|_| RecordFault::BadValue("animate operation"))?;
        length = length.saturating_add(used);
    }
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
