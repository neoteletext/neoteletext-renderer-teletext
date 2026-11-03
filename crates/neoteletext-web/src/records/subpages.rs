//! The SUBPAGES record (DOC-1959,
//! DEC-1500, REQ-1503): a page's list of sub-pages, which never travel
//! with their page, for a receiver's list, count, place and stepping.
//!
//! ```text
//! bytes 0-1  page: 0x100 to 0x8FF
//! bytes 2-3  version: the list's, the same in every part of a sending
//! bytes 4-5  part: 0 to parts - 1
//! bytes 6-7  parts: 1 to 8,190
//! bytes 8-9  count: 1 to 8,190, the page's sub-pages
//! then       entries, each: sub-code u16, title (u8 length, 0 to 40
//!            characters of UTF-8, no control character)
//! ```

use neoteletext_audio::MAX_BODY;
use neoteletext_core::limits::{LAST_SUBPAGE_CODE, MAX_SUBPAGES, SUBCODE_MASK};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::{RecordFault, TYPE_SUBPAGES, take_u16, write_packets};

/// The longest title, in characters: a teletext row.
pub const MAX_TITLE_CHARACTERS: usize = neoteletext_core::consts::COLUMNS;
/// The bytes of UTF-8 a character takes at most (RFC 3629).
const UTF8_MAX: usize = 4;
/// The longest title, in bytes.
pub const MAX_TITLE_BYTES: usize = MAX_TITLE_CHARACTERS * UTF8_MAX;
/// The fixed fields: page, version, part, parts and count.
const FIXED: usize = 2 + 2 + 2 + 2 + 2;
/// An entry's fixed fields: its sub-code and its title's length.
const ENTRY_FIXED: usize = 2 + 1;
/// The most sub-pages, and so the most records of a sending.
const MOST: u16 = MAX_SUBPAGES as u16;
const _: () = assert!(MAX_SUBPAGES <= u16::MAX as usize, "a count fits its field");
const _: () = assert!(MAX_TITLE_BYTES <= u8::MAX as usize, "a title's length fits its byte");
// At least one entry fits a record, so every list can be written.
const _: () = assert!(FIXED + ENTRY_FIXED + MAX_TITLE_BYTES <= MAX_BODY);

/// One sub-page of the list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubPageEntry<'a> {
    /// Its sub-code, `0x0000` to `0x3F7E`.
    pub subcode: SubCode,
    /// Its title; empty for none.
    pub title: &'a str,
}

/// One SUBPAGES record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubPagesRecord<'a> {
    /// The page.
    pub page: PageAddress,
    /// The list's version.
    pub version: u16,
    /// This record's place in the sending.
    pub part: u16,
    /// The records of the sending.
    pub parts: u16,
    /// The page's sub-pages, in all.
    pub count: u16,
    entries: &'a [u8],
}

impl<'a> SubPagesRecord<'a> {
    /// The record's entries, in the page's order.
    #[must_use]
    pub fn entries(&self) -> Entries<'a> {
        Entries { bytes: self.entries }
    }
}

/// The entries of a SUBPAGES record.
#[derive(Clone, Debug)]
pub struct Entries<'a> {
    bytes: &'a [u8],
}

impl<'a> Iterator for Entries<'a> {
    type Item = SubPageEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // The record was checked whole.
        let (entry, rest) = read_entry(self.bytes).ok()?;
        self.bytes = rest;
        Some(entry)
    }
}

fn check_subcode(subcode: u16) -> bool {
    subcode <= LAST_SUBPAGE_CODE && subcode & !SUBCODE_MASK == 0
}

fn check_title(title: &str) -> bool {
    let characters = title.chars().count();
    characters <= MAX_TITLE_CHARACTERS && title.len() <= MAX_TITLE_BYTES && !title.chars().any(char::is_control)
}

fn read_entry(bytes: &[u8]) -> Result<(SubPageEntry<'_>, &[u8]), RecordFault> {
    let (subcode, rest) = take_u16(bytes)?;
    if !check_subcode(subcode) {
        return Err(RecordFault::BadValue("sub-code"));
    }
    let (&length, rest) = rest.split_first().ok_or(RecordFault::Length)?;
    let (title, rest) = rest.split_at_checked(usize::from(length)).ok_or(RecordFault::Length)?;
    let title = core::str::from_utf8(title).map_err(|_| RecordFault::BadValue("sub-page title"))?;
    if !check_title(title) {
        return Err(RecordFault::BadValue("sub-page title"));
    }
    Ok((SubPageEntry { subcode: SubCode::new(subcode), title }, rest))
}

/// Reads a SUBPAGES record's body, checking every field.
///
/// # Errors
/// A page out of range, a part not below its parts, a count or parts
/// outside 1 to 8,190, no entries, a sub-code out of range or not above the
/// one before it in the record, a title that breaks its rule, or a field cut
/// short.
pub fn read(body: &[u8]) -> Result<SubPagesRecord<'_>, RecordFault> {
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("sub-pages page"))?;
    let (version, rest) = take_u16(rest)?;
    let (part, rest) = take_u16(rest)?;
    let (parts, rest) = take_u16(rest)?;
    let (count, entries) = take_u16(rest)?;
    if parts == 0 || part >= parts || parts > MOST || count == 0 || count > MOST || entries.is_empty() {
        return Err(RecordFault::BadValue("sub-pages part"));
    }
    let mut walk = entries;
    let mut seen: usize = 0;
    let mut last: Option<u16> = None;
    while !walk.is_empty() {
        let (entry, rest) = read_entry(walk)?;
        // Ascending, which also gives each sub-code once (REQ-1503 item 3).
        let subcode = entry.subcode.value();
        if last.is_some_and(|last| subcode <= last) {
            return Err(RecordFault::BadValue("sub-codes not ascending"));
        }
        last = Some(subcode);
        seen = seen.saturating_add(1);
        walk = rest;
    }
    if seen > usize::from(count) {
        return Err(RecordFault::BadValue("sub-pages count"));
    }
    Ok(SubPagesRecord { page, version, part, parts, count, entries })
}

fn entry_bytes(entry: &SubPageEntry<'_>) -> usize {
    ENTRY_FIXED.saturating_add(entry.title.len())
}

/// What [`encode`] wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sent {
    /// The records of the sending.
    pub records: u16,
    /// The packets written.
    pub packets: usize,
    /// The record id after the last record's, for the service's next
    /// record (DOC-1952).
    pub next_record_id: u8,
}

/// Writes a page's whole list of sub-pages as one sending of `version`:
/// its records as packets 4/31 of audio service `service`, the first with
/// id `first_record_id` and each after it the next, given to `out` in
/// order.
///
/// # Errors
/// An empty list or one of more than 8,190, a sub-code out of range or not
/// above the one before it, or a title that breaks its rule: nothing is
/// written then.
pub fn encode(
    page: PageAddress,
    version: u16,
    entries: &[SubPageEntry<'_>],
    service: u8,
    first_record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<Sent, RecordFault> {
    let count = u16::try_from(entries.len()).ok().filter(|&count| count > 0 && count <= MOST);
    let count = count.ok_or(RecordFault::BadValue("sub-pages count"))?;
    for entry in entries {
        if !check_subcode(entry.subcode.value()) || !check_title(entry.title) {
            return Err(RecordFault::BadValue("sub-page entry"));
        }
    }
    if !entries.is_sorted_by(|before, after| before.subcode.value() < after.subcode.value()) {
        return Err(RecordFault::BadValue("sub-codes not ascending"));
    }
    // Each record takes as many entries as fit, in order.
    let records = |each: &mut dyn FnMut(&[SubPageEntry<'_>])| {
        let mut rest = entries;
        while !rest.is_empty() {
            let mut used = FIXED;
            let mut taken: usize = 0;
            for entry in rest {
                let next = used.saturating_add(entry_bytes(entry));
                if next > MAX_BODY {
                    break;
                }
                used = next;
                taken = taken.saturating_add(1);
            }
            let (these, after) = rest.split_at(taken.max(1).min(rest.len()));
            each(these);
            rest = after;
        }
    };
    let mut parts: u16 = 0;
    records(&mut |_| parts = parts.saturating_add(1));
    let mut part: u16 = 0;
    let mut record_id = first_record_id;
    let mut packets: usize = 0;
    records(&mut |these| {
        let mut body = [0_u8; MAX_BODY];
        let mut at: usize = 0;
        let mut put = |bytes: &[u8]| {
            let end = at.saturating_add(bytes.len());
            if let Some(target) = body.get_mut(at..end) {
                target.copy_from_slice(bytes);
            }
            at = end;
        };
        for field in [page.mpp(), version, part, parts, count] {
            put(&field.to_be_bytes());
        }
        for entry in these {
            put(&entry.subcode.value().to_be_bytes());
            put(&[u8::try_from(entry.title.len()).unwrap_or(u8::MAX)]);
            put(entry.title.as_bytes());
        }
        let body = body.get(..at).unwrap_or(&[]);
        // The writer checks with the reader: what it writes always reads.
        if read(body).is_ok() {
            packets = packets.saturating_add(write_packets(TYPE_SUBPAGES, body, service, record_id, out).unwrap_or(0));
        }
        part = part.saturating_add(1);
        record_id = record_id.wrapping_add(1);
    });
    Ok(Sent { records: parts, packets, next_record_id: record_id })
}

/// What one SUBPAGES record meant for its page's sending (DOC-1959).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// Its entries are shown; more parts are to come, and a partial
    /// sending removes nothing.
    Partial,
    /// Every part has arrived, its entries number `count` and ascend over
    /// the whole list: it is the page's whole list.
    Complete,
    /// It breaks its sending: a part out of turn, a `parts` or `count`
    /// that differs from part 0's, a sub-code not above the last of the
    /// part before, more entries than `count`, or a last part leaving
    /// fewer. The sending is discarded whole, the page keeps the list it
    /// had, and this many packets, the sending's records so far and this
    /// one, are invalid (REQ-1462).
    Refused {
        /// The packets of the sending's records, this one included.
        packets: usize,
    },
}

/// The sending in progress for one page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Current {
    page: PageAddress,
    version: u16,
    parts: u16,
    count: u16,
    /// The part expected next.
    next: u16,
    /// The entries so far.
    entries: u16,
    /// The last sub-code so far.
    last: u16,
    /// The packets of the sending's records so far.
    packets: usize,
}

/// A receiver's SUBPAGES sending in progress (DOC-1959): the one
/// reassembly of a page's list of sub-pages, which every receiver uses in
/// place of its own. A record of another `version` of the page begins a
/// new sending; the records of one sending arrive in order on one
/// connection, part 0 first, as [`encode`] writes them, so a part out of
/// turn breaks its sending, and sub-codes are checked to ascend across
/// the parts as [`read`] checks them within one.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sending {
    current: Option<Current>,
}

impl Sending {
    /// Nothing in progress.
    #[must_use]
    pub const fn new() -> Self {
        Self { current: None }
    }

    /// Notes a record the reader accepted, which took `packets` packets.
    pub fn note(&mut self, record: &SubPagesRecord<'_>, packets: usize) -> Step {
        let held = if record.part == 0 {
            Current {
                page: record.page,
                version: record.version,
                parts: record.parts,
                count: record.count,
                next: 0,
                entries: 0,
                last: 0,
                packets: 0,
            }
        } else {
            match self.current.take() {
                Some(current)
                    if current.page == record.page
                        && current.version == record.version
                        && current.parts == record.parts
                        && current.count == record.count
                        && current.next == record.part =>
                {
                    current
                }
                Some(current) if current.page == record.page && current.version == record.version => {
                    return Step::Refused { packets: current.packets.saturating_add(packets) };
                }
                _ => return Step::Refused { packets },
            }
        };
        let mut held = held;
        held.packets = held.packets.saturating_add(packets);
        for entry in record.entries() {
            let subcode = entry.subcode.value();
            // Within a record `read` holds them ascending; across parts,
            // the first of a part must pass the last of the part before.
            if held.entries > 0 && subcode <= held.last {
                return Step::Refused { packets: held.packets };
            }
            held.last = subcode;
            held.entries = held.entries.saturating_add(1);
        }
        if held.entries > held.count {
            return Step::Refused { packets: held.packets };
        }
        held.next = held.next.saturating_add(1);
        if held.next < held.parts {
            self.current = Some(held);
            return Step::Partial;
        }
        if held.entries == held.count { Step::Complete } else { Step::Refused { packets: held.packets } }
    }
}

#[cfg(test)]
mod tests;
