//! The RULES record, type 21, and the `RULES_QUERY` record, type 22
//! (neoteletext-common `docs/page-rules.md`, DEC-6522, DEC-6587; RULE-41,
//! RULE-47).
//!
//! ```text
//! RULES        bytes 0-1    page: 0x100 to 0x8FF
//!              bytes 2-3    sub-code: 0x0000 to 0x3F7E
//!              bytes 4-5    generation: 1 to 65,535
//!              byte  6      flags: bit 0, a table follows
//!              byte  7      reserved: 0
//!              bytes 8-9    total: the image's length
//!              bytes 10-11  offset: where this fragment's first byte lies
//!              bytes 12-    data: bytes of the image
//! RULES_QUERY  bytes 0-1    page
//!              bytes 2-3    sub-code
//!              bytes 4-7    identifier: the query's
//! ```
//!
//! A page's table is one image, sent in one or more RULES records. This
//! reader checks a record's own fields and gives the fragment on; an
//! [`Assembler`] puts the fragments together, and the receiver decodes
//! the whole image with [`neoteletext_rules::decode`], the one decoder a
//! receiver and the server share, so that a table it refuses is refused
//! whole and reported with its generation (RULE-48, RULE-70). The writer
//! checks the whole image with that decoder before it cuts it, so a
//! server never sends what a receiver refuses (RULE-59).

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::limits::{MAX_TABLE_BYTES, MAX_TABLE_FRAGMENT, RULES_RECORD_FIXED};
use neoteletext_rules::table::HEADER_BYTES;

use super::{RecordFault, TYPE_RULES, TYPE_RULES_QUERY, body_check, take_place, take_u16, write_packets};

// RULE-41, RULE-57: a fragment's data fills the frame's largest body, and
// the rule table's crate, which has no dependency, states that body as
// the audio profile does.
const _: () = assert!(
    neoteletext_rules::limits::MAX_FRAMED_BODY == neoteletext_audio::MAX_FRAMED_BODY
        && RULES_RECORD_FIXED + MAX_TABLE_FRAGMENT == neoteletext_audio::MAX_FRAMED_BODY
);

/// Flags bit 0: a table follows (RULE-41).
const HAS_TABLE: u8 = 1;

/// The bytes of an image one RULES record carries (RULE-41).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fragment<'a> {
    /// The image's whole length, 12 to `MAX_TABLE_BYTES`.
    pub total: u16,
    /// The place of this fragment's first byte in the image.
    pub offset: u16,
    /// 1 to `MAX_TABLE_FRAGMENT` bytes of the image.
    pub data: &'a [u8],
}

/// One RULES record: a fragment of a page's table image, or the word
/// that the page has no table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RulesRecord<'a> {
    /// The page the table is for.
    pub page: PageAddress,
    /// Its sub-code: a table belongs to one page number and sub-code
    /// together (RULE-112).
    pub subcode: SubCode,
    /// 1 to 65,535 (RULE-40), the same in every fragment of one image.
    pub generation: u16,
    /// The fragment; `None` when the page has no table, which removes the
    /// page's table at once.
    pub fragment: Option<Fragment<'a>>,
}

/// Reads a RULES record's body: its page, its sub-code, its generation,
/// its total and its offset. The image is not read here.
///
/// # Errors
/// A page, a sub-code or a generation outside its range, a reserved bit
/// or byte that is not 0, a total or an offset outside the image, no data
/// or more than `MAX_TABLE_FRAGMENT` bytes of it, or, without a table, a
/// total or an offset that is not 0 or a body that does not end there.
pub fn read(body: &[u8]) -> Result<RulesRecord<'_>, RecordFault> {
    let (page, subcode, rest) = take_place(body)?;
    let (generation, rest) = take_u16(rest)?;
    if generation == 0 {
        return Err(RecordFault::BadValue("rules generation"));
    }
    let (&[flags, reserved], rest) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    if flags & !HAS_TABLE != 0 || reserved != 0 {
        return Err(RecordFault::BadValue("rules reserved bits"));
    }
    let (total, rest) = take_u16(rest)?;
    let (offset, data) = take_u16(rest)?;
    if flags & HAS_TABLE == 0 {
        if total != 0 || offset != 0 {
            return Err(RecordFault::BadValue("rules total without a table"));
        }
        return if data.is_empty() {
            Ok(RulesRecord { page, subcode, generation, fragment: None })
        } else {
            Err(RecordFault::Length)
        };
    }
    if !(HEADER_BYTES..=MAX_TABLE_BYTES).contains(&usize::from(total)) {
        return Err(RecordFault::BadValue("rules total"));
    }
    if data.is_empty() || data.len() > MAX_TABLE_FRAGMENT {
        return Err(RecordFault::Length);
    }
    if usize::from(offset).saturating_add(data.len()) > usize::from(total) {
        return Err(RecordFault::BadValue("rules offset"));
    }
    Ok(RulesRecord { page, subcode, generation, fragment: Some(Fragment { total, offset, data }) })
}

/// Writes one RULES record as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written. The image is not
/// checked here: [`encode_image`] does that, once, for the whole of it.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    record: &RulesRecord<'_>,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; neoteletext_audio::MAX_FRAMED_BODY];
    let (total, offset, data) =
        record.fragment.map_or((0, 0, &[][..]), |fragment| (fragment.total, fragment.offset, fragment.data));
    let fields = record
        .page
        .mpp()
        .to_be_bytes()
        .into_iter()
        .chain(record.subcode.value().to_be_bytes())
        .chain(record.generation.to_be_bytes())
        .chain([if record.fragment.is_some() { HAS_TABLE } else { 0 }, 0])
        .chain(total.to_be_bytes())
        .chain(offset.to_be_bytes())
        .chain(data.iter().copied());
    let length = RULES_RECORD_FIXED.saturating_add(data.len());
    let body = body.get_mut(..length).ok_or(RecordFault::Length)?;
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    // The writer checks with the reader, so the two cannot disagree.
    if read(body)? != *record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_RULES, body, service, record_id, out).ok_or(RecordFault::Length)
}

/// How many RULES records an image of `length` bytes travels in
/// (RULE-41): one for a table of at most `MAX_TABLE_FRAGMENT` bytes.
#[must_use]
pub const fn records_for(length: usize) -> usize {
    length.div_ceil(MAX_TABLE_FRAGMENT)
}

/// Checks a table's image whole with [`neoteletext_rules::check`] and
/// this crate's [`body_check`], then writes it as RULES records for the
/// page `page` and `subcode` at generation `generation`: one record for
/// each `MAX_TABLE_FRAGMENT` bytes, in order, record `n` with the record
/// id `record_id(n)`. Returns the packets written.
///
/// # Errors
/// [`RecordFault::BadValue`] for an image [`neoteletext_rules::check`]
/// refuses: nothing is written that a receiver would refuse (RULE-59).
pub fn encode_image(
    page: PageAddress,
    subcode: SubCode,
    generation: u16,
    image: &[u8],
    service: u8,
    record_id: &mut dyn FnMut(usize) -> u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    neoteletext_rules::check(image, page.mpp(), subcode.value(), body_check)
        .map_err(|_| RecordFault::BadValue("rules table"))?;
    let total = u16::try_from(image.len()).map_err(|_| RecordFault::Length)?;
    let mut packets = 0_usize;
    for (number, data) in image.chunks(MAX_TABLE_FRAGMENT).enumerate() {
        let offset = number.saturating_mul(MAX_TABLE_FRAGMENT);
        let offset = u16::try_from(offset).map_err(|_| RecordFault::Length)?;
        let record = RulesRecord { page, subcode, generation, fragment: Some(Fragment { total, offset, data }) };
        packets = packets.saturating_add(encode(&record, service, record_id(number), out)?);
    }
    Ok(packets)
}

/// What a RULES record gave an [`Assembler`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assembled<'a> {
    /// A fragment of an image still incomplete: the page keeps the table
    /// it had (RULE-41).
    Incomplete,
    /// The page has no table: its table is removed at once.
    Removed {
        /// The page.
        page: PageAddress,
        /// Its sub-code.
        subcode: SubCode,
        /// The record's generation.
        generation: u16,
    },
    /// The image is whole: the receiver decodes it with
    /// [`neoteletext_rules::decode`] (RULE-70).
    Whole {
        /// The page.
        page: PageAddress,
        /// Its sub-code.
        subcode: SubCode,
        /// The image's generation.
        generation: u16,
        /// The image.
        image: &'a [u8],
    },
    /// A fragment out of place: the incomplete image is discarded, the
    /// page keeps the table it had, and the receiver reports fault 1 with
    /// this generation (RULE-41, RULE-48).
    OutOfPlace {
        /// The page.
        page: PageAddress,
        /// Its sub-code.
        subcode: SubCode,
        /// The fragment's generation.
        generation: u16,
    },
}

/// Puts a page's table image together from its RULES records (RULE-41).
/// It holds one incomplete image, for one page: a receiver holds one
/// table for each view, that of the page the view has selected (RULE-58).
pub struct Assembler {
    /// The page, the generation and the total of the image in flight.
    following: Option<(PageAddress, SubCode, u16, u16)>,
    /// The bytes held of it.
    held: usize,
    image: [u8; MAX_TABLE_BYTES],
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    /// An assembler with nothing in flight.
    #[must_use]
    #[allow(
        clippy::large_stack_arrays,
        reason = "an assembler holds one whole image, which RULE-41 bounds at `MAX_TABLE_BYTES`: a receiver keeps its one assembler in static or heap memory"
    )]
    pub const fn new() -> Self {
        Self { following: None, held: 0, image: [0; MAX_TABLE_BYTES] }
    }

    /// Forgets the image in flight, as leaving the page or a new
    /// connection does (RULE-111).
    pub fn clear(&mut self) {
        self.following = None;
        self.held = 0;
    }

    /// Takes one RULES record. A fragment with offset 0 begins an image
    /// and discards any still incomplete; each later one must carry the
    /// same page, generation and total and an offset equal to the bytes
    /// already held.
    pub fn push(&mut self, record: &RulesRecord<'_>) -> Assembled<'_> {
        let (page, subcode, generation) = (record.page, record.subcode, record.generation);
        let Some(fragment) = record.fragment else {
            self.clear();
            return Assembled::Removed { page, subcode, generation };
        };
        let key = (page, subcode, generation, fragment.total);
        if fragment.offset == 0 {
            self.following = Some(key);
            self.held = 0;
        } else if self.following != Some(key) || usize::from(fragment.offset) != self.held {
            self.clear();
            return Assembled::OutOfPlace { page, subcode, generation };
        }
        let end = self.held.saturating_add(fragment.data.len());
        let Some(room) = self.image.get_mut(self.held..end).filter(|_| end <= usize::from(fragment.total)) else {
            self.clear();
            return Assembled::OutOfPlace { page, subcode, generation };
        };
        room.copy_from_slice(fragment.data);
        self.held = end;
        if end < usize::from(fragment.total) {
            return Assembled::Incomplete;
        }
        self.following = None;
        self.held = 0;
        Assembled::Whole { page, subcode, generation, image: self.image.get(..end).unwrap_or(&[]) }
    }
}

/// The bytes of a `RULES_QUERY` record: the page, the sub-code and the
/// identifier.
pub const QUERY_BYTES: usize = 2 + 2 + 4;

/// One `RULES_QUERY` record (RULE-47): a receiver that has the page open
/// answers once with a `PAGE_STATE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RulesQueryRecord {
    /// The page asked about.
    pub page: PageAddress,
    /// Its sub-code (RULE-112).
    pub subcode: SubCode,
    /// The query's identifier, which the answer repeats.
    pub identifier: u32,
}

/// Reads a `RULES_QUERY` record's body.
///
/// # Errors
/// A page or a sub-code outside its range, or a body of any length but
/// [`QUERY_BYTES`].
pub fn read_query(body: &[u8]) -> Result<RulesQueryRecord, RecordFault> {
    if body.len() != QUERY_BYTES {
        return Err(RecordFault::Length);
    }
    let (page, subcode, rest) = take_place(body)?;
    let identifier: [u8; 4] = rest.try_into().map_err(|_| RecordFault::Length)?;
    Ok(RulesQueryRecord { page, subcode, identifier: u32::from_be_bytes(identifier) })
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
    let fields = record
        .page
        .mpp()
        .to_be_bytes()
        .into_iter()
        .chain(record.subcode.value().to_be_bytes())
        .chain(record.identifier.to_be_bytes());
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
