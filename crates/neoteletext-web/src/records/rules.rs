//! The RULES record, type 21, and the `RULES_QUERY` record, type 22
//! (neoteletext-common `docs/page-rules.md`, DEC-6522; RULE-41, RULE-47).
//!
//! ```text
//! RULES        bytes 0-1   page: 0x100 to 0x8FF
//!              bytes 2-3   generation: 1 to 65,535
//!              bytes 4-11  format, flags, the five counts, reserved
//!              bytes 12-   the table: declarations, rules, sub-page masks
//! RULES_QUERY  bytes 0-1   page
//!              bytes 2-5   identifier: the query's
//! ```
//!
//! A RULES record carries one page's rule table whole, at most 8,184
//! bytes, so that a table is never half arrived. This reader takes the
//! page and the generation and leaves the table to
//! [`neoteletext_rules::decode`], the one decoder a receiver and the
//! server share: a receiver decodes the record it is given, and a table
//! it refuses is refused whole and reported with the generation read here
//! (RULE-48, RULE-70). The writer checks the whole body with that
//! decoder, so a server never sends what a receiver refuses (RULE-59).

use neoteletext_core::packet::Packet;
use neoteletext_core::types::PageAddress;
use neoteletext_rules::limits::MAX_TABLE_BYTES;
use neoteletext_rules::table::{Fault, HEADER_BYTES, Header, Table};

use super::{RecordFault, TYPE_RULES, TYPE_RULES_QUERY, take_u16, write_packets};

// RULE-41: a table is one record body.
const _: () = assert!(MAX_TABLE_BYTES == neoteletext_audio::MAX_FRAMED_BODY);

/// One RULES record: a page's table, not yet decoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RulesRecord<'a> {
    /// The page the table is for.
    pub page: PageAddress,
    /// 1 to 65,535 (RULE-40): a receiver that holds a table for the page
    /// replaces it when the generation differs.
    pub generation: u16,
    /// The record's whole body, header included: what
    /// [`neoteletext_rules::decode`] reads.
    pub body: &'a [u8],
}

impl RulesRecord<'_> {
    /// Decodes the table into `table`, and gives the header: whether a
    /// table follows, or the page has none (RULE-41).
    ///
    /// # Errors
    /// Everything [`neoteletext_rules::decode`] refuses (RULE-70); `table`
    /// is then empty, the page has no table, and the receiver reports
    /// fault 1 (RULE-48).
    pub fn decode(&self, table: &mut Table) -> Result<Header, Fault> {
        neoteletext_rules::decode(self.body, table)
    }
}

/// Reads a RULES record's body as far as its page and generation.
///
/// # Errors
/// A body shorter than its header or longer than a table may be, a page
/// outside its range, or a generation of 0.
pub fn read(body: &[u8]) -> Result<RulesRecord<'_>, RecordFault> {
    if body.len() < HEADER_BYTES || body.len() > MAX_TABLE_BYTES {
        return Err(RecordFault::Length);
    }
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("rules page"))?;
    let (generation, _) = take_u16(rest)?;
    if generation == 0 {
        return Err(RecordFault::BadValue("rules generation"));
    }
    Ok(RulesRecord { page, generation, body })
}

/// Writes a RULES record around `body`, which
/// [`neoteletext_rules::encode`] wrote, as packets 4/31 of audio service
/// `service`, record id `record_id`. Returns the packets written.
///
/// # Errors
/// [`RecordFault::BadValue`] for a body [`neoteletext_rules::check`]
/// refuses: nothing is written that a receiver would refuse (RULE-59).
pub fn encode(body: &[u8], service: u8, record_id: u8, out: &mut dyn FnMut(Packet)) -> Result<usize, RecordFault> {
    neoteletext_rules::check(body).map_err(|_| RecordFault::BadValue("rules table"))?;
    // The writer checks with the reader, so the two cannot disagree.
    read(body)?;
    write_packets(TYPE_RULES, body, service, record_id, out).ok_or(RecordFault::Length)
}

/// The bytes of a `RULES_QUERY` record: the page and the identifier.
pub const QUERY_BYTES: usize = 2 + 4;

/// One `RULES_QUERY` record (RULE-47): a receiver that has the page open
/// answers once with a `PAGE_STATE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RulesQueryRecord {
    /// The page asked about.
    pub page: PageAddress,
    /// The query's identifier, which the answer repeats.
    pub identifier: u32,
}

/// Reads a `RULES_QUERY` record's body.
///
/// # Errors
/// A page outside its range, or a body of any length but [`QUERY_BYTES`].
pub fn read_query(body: &[u8]) -> Result<RulesQueryRecord, RecordFault> {
    if body.len() != QUERY_BYTES {
        return Err(RecordFault::Length);
    }
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("rules query page"))?;
    let (high, rest) = take_u16(rest)?;
    let (low, _) = take_u16(rest)?;
    Ok(RulesQueryRecord { page, identifier: (u32::from(high) << 16) | u32::from(low) })
}

/// Writes a `RULES_QUERY` record as packets 4/31 of audio service
/// `service`, record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode_query(
    record: RulesQueryRecord,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; QUERY_BYTES];
    let fields = record.page.mpp().to_be_bytes().into_iter().chain(record.identifier.to_be_bytes());
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    // The writer checks with the reader, so the two cannot disagree.
    if read_query(&body)? != record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_RULES_QUERY, &body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
