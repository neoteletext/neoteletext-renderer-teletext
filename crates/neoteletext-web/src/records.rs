//! The channel's records on its push connection beside its pages
//! (DOC-1951): the index (type 6), LOG
//! (type 7), LINKS (type 9), SUBPAGES (type 10),
//! REDIRECT (type 11), DENIED (type 12), and viewer state's
//! `STATE_CHALLENGE`, `STATE_END` and `STATE_KEEP_OFFER` (types 18 to 20), in the audio profile's record frame
//! (REQ-AUD-003) on data channel 12, packet 4/31, cut into as many
//! fragments as a record needs (DOC-1952).
//!
//! Each record's encoder writes its packets through a callback, so it
//! allocates nothing, and checks what it writes with its reader, so a
//! server can never send what a receiver refuses. [`RecordReader`]
//! reassembles a push stream's fragments and reads the records it carries.

use neoteletext_audio::MAX_FRAMED_RECORD;
use neoteletext_audio::idl::{FRAGMENT_DATA, MAX_FRAGMENTS, Reassembler, fragment, packet};
use neoteletext_audio::record::{RecordError, record_length, split, write};
use neoteletext_core::packet::Packet;

pub mod denied;
pub mod drawing_links;
pub mod index;
pub mod links;
pub mod log;
pub mod redirect;
pub mod regis;
pub mod regis_cursor;
pub mod state;
pub mod subpages;

pub use neoteletext_audio::record::{
    TYPE_DENIED, TYPE_DRAWING_LINKS, TYPE_INDEX, TYPE_LINKS, TYPE_LOG, TYPE_NAPLPS, TYPE_REDIRECT, TYPE_REGIS,
    TYPE_REGIS_CURSOR, TYPE_SUBPAGES,
};

/// Why a record was refused: it is discarded whole, and its packets are
/// counted as invalid (REQ-1462, DOC-1952).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecordFault {
    /// The frame failed: its length, CRC-32 or version.
    Frame(RecordError),
    /// A field outside its range, and which.
    BadValue(&'static str),
    /// Bytes after the last field, or a field cut short.
    Length,
}

impl From<RecordError> for RecordFault {
    fn from(error: RecordError) -> Self {
        Self::Frame(error)
    }
}

/// The fragments a record of `body` bytes takes (DOC-1952).
#[must_use]
pub const fn fragments(body: usize) -> usize {
    record_length(body).div_ceil(FRAGMENT_DATA)
}

/// Writes a record of type `kind` around `body` as packets 4/31 of audio
/// service `service`, record id `record_id`, each given to `out` in order,
/// fragment 0 first. Returns the packets written, or `None` when the body
/// is over the frame's largest.
pub fn write_packets(kind: u8, body: &[u8], service: u8, record_id: u8, out: &mut dyn FnMut(Packet)) -> Option<usize> {
    let mut record = [0_u8; MAX_FRAMED_RECORD];
    let length = write(kind, body, &mut record)?;
    let bytes = record.get(..length)?;
    let count = bytes.len().div_ceil(FRAGMENT_DATA);
    for (index, chunk) in bytes.chunks(FRAGMENT_DATA).enumerate() {
        let index = u8::try_from(index).ok()?;
        let last = usize::from(index).saturating_add(1) == count;
        out(packet(service, record_id, index, last, chunk));
    }
    Some(count)
}

/// Reads a big-endian `u16` at the front of `bytes`, and what follows it.
pub(crate) fn take_u16(bytes: &[u8]) -> Result<(u16, &[u8]), RecordFault> {
    let (head, rest) = bytes.split_at_checked(2).ok_or(RecordFault::Length)?;
    let head: [u8; 2] = head.try_into().map_err(|_| RecordFault::Length)?;
    Ok((u16::from_be_bytes(head), rest))
}

/// One record the reader accepted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelRecord<'a> {
    /// A part of the channel's index (DOC-1955).
    Index(index::IndexRecord<'a>),
    /// A log line for the channel's author (DOC-1956).
    Log(log::LogRecord<'a>),
    /// A sub-page's Link Extension links (DOC-1958).
    Links(links::LinksRecord<'a>),
    /// A part of a page's list of sub-pages (DOC-1959).
    SubPages(subpages::SubPagesRecord<'a>),
    /// A channel's redirect (DOC-1960).
    Redirect(redirect::RedirectRecord<'a>),
    /// A selection refused, or a program not answering (DOC-1961).
    Denied(denied::DeniedRecord<'a>),
    /// Bytes of a sub-page's ReGIS (DEC-6335).
    Regis(regis::RegisRecord<'a>),
    /// Bytes of a sub-page's NAPLPS (REQ-6408 item 2), in the REGIS
    /// record's form: page, sub-code, generation, offset and data.
    Naplps(regis::RegisRecord<'a>),
    /// The numbered drawings of a sub-page's ReGIS picture that are links
    /// (REQ-6370).
    DrawingLinks(drawing_links::DrawingLinksRecord<'a>),
    /// The numbered drawing that is a ReGIS sub-page's cursor (REQ-6371).
    RegisCursor(regis_cursor::RegisCursorRecord),
    /// A challenge for the viewer's key (VS-204, VS-205).
    StateChallenge(state::StateChallenge),
    /// Delete the viewer's key with this identifier (VS-204).
    StateEnd(state::StateEnd),
    /// The channel suggests that the viewer have it remember them (VS-208).
    StateKeepOffer,
}

/// What one packet gave the reader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Read<'a> {
    /// Nothing yet: a fragment of a record still incomplete, or a packet
    /// that is not one of these records.
    Nothing,
    /// A whole record, checked.
    Record(ChannelRecord<'a>),
    /// A damaged fragment, or a whole record refused: this many packets
    /// are invalid.
    Invalid(usize),
}

/// Reassembles the index, LOG, LINKS, SUBPAGES, REDIRECT, DENIED and viewer-state records from a push stream's
/// packets (DOC-1952). A record is followed from its fragment 0, which
/// names its type, so the audio profile's records and the one-fragment
/// cursor and control records, which other readers take, are never copied
/// here.
pub struct RecordReader {
    reassembler: Reassembler<1, { MAX_FRAGMENTS * FRAGMENT_DATA }>,
    /// The service and record id of the record being reassembled.
    following: Option<(u8, u8)>,
}

impl Default for RecordReader {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordReader {
    /// A reader with nothing in flight.
    #[must_use]
    pub const fn new() -> Self {
        Self { reassembler: Reassembler::new(), following: None }
    }

    /// Forgets the record in flight, as a new connection does.
    pub fn clear(&mut self) {
        self.reassembler.clear();
        self.following = None;
    }

    /// Reads one packet of the stream.
    pub fn push(&mut self, packet: &Packet) -> Read<'_> {
        self.push_counted(packet).0
    }

    /// Reads one packet of the stream, and gives with a whole record the
    /// packets it took, so that a receiver that refuses a sending across
    /// its records can count each of them invalid (REQ-1462). The count is
    /// 0 for anything but [`Read::Record`].
    pub fn push_counted(&mut self, packet: &Packet) -> (Read<'_>, usize) {
        let Some(result) = fragment(packet) else { return (Read::Nothing, 0) };
        let Ok(fragment) = result else { return (Read::Invalid(1), 0) };
        if fragment.index == 0 {
            let kind = fragment.data.get(1).copied().unwrap_or(0);
            if !matches!(
                kind,
                TYPE_INDEX
                    | TYPE_LOG
                    | TYPE_LINKS
                    | TYPE_SUBPAGES
                    | TYPE_REDIRECT
                    | TYPE_DENIED
                    | TYPE_REGIS
                    | TYPE_DRAWING_LINKS
                    | TYPE_REGIS_CURSOR
                    | TYPE_NAPLPS
                    | state::TYPE_STATE_CHALLENGE
                    | state::TYPE_STATE_END
                    | state::TYPE_STATE_KEEP_OFFER
            ) {
                return (Read::Nothing, 0);
            }
            self.reassembler.clear();
            self.following = Some((fragment.service, fragment.record));
        } else if self.following != Some((fragment.service, fragment.record)) {
            return (Read::Nothing, 0);
        }
        let Some((_, bytes)) = self.reassembler.push(&fragment) else { return (Read::Nothing, 0) };
        self.following = None;
        let packets = bytes.len().div_ceil(FRAGMENT_DATA);
        match read_record(bytes) {
            Ok(record) => (Read::Record(record), packets),
            Err(_) => (Read::Invalid(packets), 0),
        }
    }
}

/// Reads one whole record of the types this reader follows, frame and CRC included.
///
/// # Errors
/// The frame's failures, another record type, or any field of the
/// record's layout.
pub fn read_record(bytes: &[u8]) -> Result<ChannelRecord<'_>, RecordFault> {
    let (kind, body) = split(bytes)?;
    match kind {
        TYPE_INDEX => index::read(body).map(ChannelRecord::Index),
        TYPE_LOG => log::read(body).map(ChannelRecord::Log),
        TYPE_LINKS => links::read(body).map(ChannelRecord::Links),
        TYPE_SUBPAGES => subpages::read(body).map(ChannelRecord::SubPages),
        TYPE_REDIRECT => redirect::read(body).map(ChannelRecord::Redirect),
        TYPE_DENIED => denied::read(body).map(ChannelRecord::Denied),
        TYPE_REGIS => regis::read(body).map(ChannelRecord::Regis),
        TYPE_DRAWING_LINKS => drawing_links::read(body).map(ChannelRecord::DrawingLinks),
        TYPE_REGIS_CURSOR => regis_cursor::read(body).map(ChannelRecord::RegisCursor),
        // The body is the REGIS record's: a stream of a sub-page's bytes.
        TYPE_NAPLPS => regis::read(body).map(ChannelRecord::Naplps),
        state::TYPE_STATE_CHALLENGE => state::read_challenge(body).map(ChannelRecord::StateChallenge),
        state::TYPE_STATE_END => state::read_end(body).map(ChannelRecord::StateEnd),
        state::TYPE_STATE_KEEP_OFFER => state::read_keep_offer(body).map(|()| ChannelRecord::StateKeepOffer),
        _ => Err(RecordFault::BadValue("record type")),
    }
}

#[cfg(test)]
mod tests;
