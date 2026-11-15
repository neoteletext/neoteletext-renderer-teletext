//! The `DRAWING_LINKS` record (REQ-6370): the numbered drawings of a
//! sub-page's ReGIS picture that are links, and where each leads. A
//! receiver follows a click on such a drawing itself, as it follows a text
//! link of a teletext page (the LINKS record, DOC-1958).
//!
//! ```text
//! bytes 0-1  page: 0x100 to 0x8FF
//! bytes 2-3  sub: the sub-code, 0x0000 to 0x3F7E
//! byte 4     part: 0 to parts - 1
//! byte 5     parts: 1 to 64
//! byte 6     count: 0 to 64, then each: drawing u16, target
//! ```
//!
//! A target is a `u16` length and 1 to 8,004 bytes of printable ASCII, in
//! any form a text link's target takes, read by the TTI reader's own
//! parser. Part 0 replaces the drawing links a receiver held for the
//! sub-page and later parts add to them; a record of one part and no links
//! removes them all. Over the parts of one sending a sub-page has at most
//! [`MAX_DRAWING_LINKS`], each drawing once: the reader checks each
//! record, and the receiver that holds the links checks the parts
//! together.

use neoteletext_audio::MAX_FRAMED_BODY as MAX_BODY;
use neoteletext_core::limits::{LAST_SUBPAGE_CODE, SUBCODE_MASK};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_formats::page::LINK_TARGET_BYTES;
use neoteletext_formats::tti::parse_target;

use super::{RecordFault, TYPE_DRAWING_LINKS, take_u16, write_packets};

/// The most drawing links one sub-page has: enough for a keyboard, a map
/// or a menu of drawings, and few enough that a receiver holds them
/// without thought (64 targets of the longest kind are 512 KB).
pub const MAX_DRAWING_LINKS: usize = 64;
/// The most records one sub-page's drawing links take: one a link.
pub const MAX_PARTS: usize = MAX_DRAWING_LINKS;
/// The fixed fields: page, sub-code, part, parts and the count.
const FIXED: usize = 2 + 2 + 1 + 1 + 1;
/// A link's fields other than its target's bytes: its drawing and the
/// target's length.
const LINK_FIELDS: usize = 2 + 2;
// The longest link must fit one record: a link is never split.
const _: () = assert!(FIXED + LINK_FIELDS + LINK_TARGET_BYTES <= MAX_BODY);

/// One drawing link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrawingLink<'a> {
    /// The numbered drawing that is the link, 1 to 65,535.
    pub drawing: u16,
    /// Where it leads: a target [`parse_target`] reads.
    pub target: &'a [u8],
}

/// One `DRAWING_LINKS` record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrawingLinksRecord<'a> {
    /// The sub-page's page.
    pub page: PageAddress,
    /// Its sub-code.
    pub subcode: SubCode,
    /// Which record of the sending this is; part 0 replaces what a
    /// receiver held.
    pub part: u8,
    /// How many records the sending has.
    pub parts: u8,
    /// How many links this record has.
    count: u8,
    /// The links' bytes, checked.
    links: &'a [u8],
}

impl<'a> DrawingLinksRecord<'a> {
    /// The record's links, in order.
    pub fn links(&self) -> impl Iterator<Item = DrawingLink<'a>> + use<'a> {
        let mut bytes = self.links;
        // The record was checked whole, so every link reads.
        (0..self.count).map_while(move |_| {
            let (link, rest) = take_link(bytes).ok()?;
            bytes = rest;
            Some(link)
        })
    }
}

/// Whether a target is one a drawing link may have: printable ASCII with
/// no comma, of a length a link target may be, in a form the TTI reader's
/// parser takes.
#[must_use]
pub fn is_target(target: &[u8]) -> bool {
    let printable = target.iter().all(|&byte| byte.is_ascii_graphic() && byte != b',');
    !target.is_empty() && target.len() <= LINK_TARGET_BYTES && printable && parse_target(target).is_some()
}

/// Reads one link at the front of `bytes`, and what follows it.
fn take_link(bytes: &[u8]) -> Result<(DrawingLink<'_>, &[u8]), RecordFault> {
    let (drawing, rest) = take_u16(bytes)?;
    let (length, rest) = take_u16(rest)?;
    let (target, rest) = rest.split_at_checked(usize::from(length)).ok_or(RecordFault::Length)?;
    if drawing == 0 {
        return Err(RecordFault::BadValue("drawing link drawing"));
    }
    if !is_target(target) {
        return Err(RecordFault::BadValue("drawing link target"));
    }
    Ok((DrawingLink { drawing, target }, rest))
}

/// Reads a `DRAWING_LINKS` record's body.
///
/// # Errors
/// A field outside its range, a drawing named twice, a target that is not
/// one, more links than [`MAX_DRAWING_LINKS`], a body cut short, or bytes
/// after the last link.
pub fn read(body: &[u8]) -> Result<DrawingLinksRecord<'_>, RecordFault> {
    let (page, rest) = take_u16(body)?;
    let page = PageAddress::from_mpp(page).ok_or(RecordFault::BadValue("drawing links page"))?;
    let (subcode, rest) = take_u16(rest)?;
    if subcode > LAST_SUBPAGE_CODE || subcode & !SUBCODE_MASK != 0 {
        return Err(RecordFault::BadValue("drawing links sub-code"));
    }
    let [part, parts, count, links @ ..] = rest else { return Err(RecordFault::Length) };
    let (part, parts, count) = (*part, *parts, *count);
    if parts == 0 || usize::from(parts) > MAX_PARTS || part >= parts {
        return Err(RecordFault::BadValue("drawing links part"));
    }
    if usize::from(count) > MAX_DRAWING_LINKS {
        return Err(RecordFault::BadValue("drawing links count"));
    }
    let mut seen = [0_u16; MAX_DRAWING_LINKS];
    let mut bytes = links;
    for index in 0..usize::from(count) {
        let (link, rest) = take_link(bytes)?;
        bytes = rest;
        let (before, slot) = seen.split_at_mut(index);
        if before.contains(&link.drawing) {
            return Err(RecordFault::BadValue("drawing link named twice"));
        }
        if let Some(slot) = slot.first_mut() {
            *slot = link.drawing;
        }
    }
    if !bytes.is_empty() {
        return Err(RecordFault::Length);
    }
    Ok(DrawingLinksRecord { page, subcode: SubCode::new(subcode), part, parts, count, links })
}

/// What sending a sub-page's drawing links took.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sent {
    /// The records written.
    pub records: u8,
    /// The packets written.
    pub packets: usize,
    /// The record id after the last one used.
    pub next_record_id: u8,
}

/// How many of `links`, from the front, fit one record.
fn fitting(links: &[DrawingLink<'_>]) -> usize {
    let mut room = MAX_BODY.saturating_sub(FIXED);
    links
        .iter()
        .take_while(|link| {
            let bytes = LINK_FIELDS.saturating_add(link.target.len());
            let fits = bytes <= room;
            room = room.saturating_sub(bytes);
            fits
        })
        .count()
}

/// Writes a sub-page's drawing links as `DRAWING_LINKS` records, as many
/// links in each as fit, in packets 4/31 of audio service `service` from
/// record id `record_id`. No links at all are one record that removes
/// those a receiver held.
///
/// # Errors
/// More links than [`MAX_DRAWING_LINKS`], and what the reader would
/// refuse.
pub fn encode(
    page: PageAddress,
    subcode: SubCode,
    links: &[DrawingLink<'_>],
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<Sent, RecordFault> {
    if links.len() > MAX_DRAWING_LINKS {
        return Err(RecordFault::BadValue("drawing links count"));
    }
    // The parts are counted before the first is written, since each names
    // their number.
    let mut parts = 0_u8;
    let mut left = links;
    loop {
        let taken = fitting(left);
        if taken == 0 && !left.is_empty() {
            return Err(RecordFault::Length);
        }
        parts = parts.checked_add(1).ok_or(RecordFault::Length)?;
        left = left.get(taken..).unwrap_or(&[]);
        if left.is_empty() {
            break;
        }
    }
    let mut sent = Sent { records: 0, packets: 0, next_record_id: record_id };
    let mut left = links;
    for part in 0..parts {
        let (now, later) = left.split_at_checked(fitting(left)).ok_or(RecordFault::Length)?;
        left = later;
        let mut body = [0_u8; MAX_BODY];
        let mut length = 0_usize;
        let mut put = |bytes: &[u8]| -> Result<(), RecordFault> {
            let end = length.checked_add(bytes.len()).ok_or(RecordFault::Length)?;
            body.get_mut(length..end).ok_or(RecordFault::Length)?.copy_from_slice(bytes);
            length = end;
            Ok(())
        };
        put(&page.mpp().to_be_bytes())?;
        put(&subcode.value().to_be_bytes())?;
        put(&[part, parts, u8::try_from(now.len()).map_err(|_| RecordFault::Length)?])?;
        for link in now {
            put(&link.drawing.to_be_bytes())?;
            put(&u16::try_from(link.target.len()).map_err(|_| RecordFault::Length)?.to_be_bytes())?;
            put(link.target)?;
        }
        let body = body.get(..length).ok_or(RecordFault::Length)?;
        // The writer checks with the reader, so the two cannot disagree.
        let read = read(body)?;
        if !read.links().eq(now.iter().copied()) {
            return Err(RecordFault::BadValue("does not read back"));
        }
        let packets =
            write_packets(TYPE_DRAWING_LINKS, body, service, sent.next_record_id, out).ok_or(RecordFault::Length)?;
        sent.records = sent.records.saturating_add(1);
        sent.packets = sent.packets.saturating_add(packets);
        sent.next_record_id = sent.next_record_id.wrapping_add(1);
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
    )]

    extern crate alloc;
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;
    use crate::records::{ChannelRecord, Read, RecordReader};

    fn page() -> PageAddress {
        PageAddress::from_mpp(0x123).unwrap()
    }

    /// A record as read: its part, its parts and its links.
    type Seen = (u8, u8, Vec<(u16, Vec<u8>)>);

    /// Sends `links` and gives what a receiver's reader reads.
    fn sent(links: &[DrawingLink<'_>]) -> (Sent, Vec<Seen>) {
        let mut reader = RecordReader::new();
        let mut records = Vec::new();
        let sent = encode(page(), SubCode::new(2), links, 3, 250, &mut |packet| {
            if let Read::Record(ChannelRecord::DrawingLinks(read)) = reader.push(&packet) {
                assert_eq!((read.page, read.subcode), (page(), SubCode::new(2)));
                let links = read.links().map(|link| (link.drawing, link.target.to_vec())).collect();
                records.push((read.part, read.parts, links));
            }
        })
        .unwrap();
        (sent, records)
    }

    /// REQ-6370: links to a page, a page of another service, an address
    /// and the web reach a receiver's reader as written, in one record.
    #[test]
    fn drawing_links_reach_a_receiver() {
        let links = [
            DrawingLink { drawing: 12, target: b"200" },
            DrawingLink { drawing: 13, target: b"news:300:0001" },
            DrawingLink { drawing: 65_535, target: b"web:https://example.org/a" },
        ];
        let (sent, records) = sent(&links);
        assert_eq!((sent.records, sent.next_record_id), (1, 251));
        let expected: Vec<_> = links.iter().map(|link| (link.drawing, link.target.to_vec())).collect();
        assert_eq!(records, [(0, 1, expected)]);
    }

    /// No links are one record that removes those a receiver held.
    #[test]
    fn no_links_are_one_empty_record() {
        let (sent, records) = sent(&[]);
        assert_eq!(sent.records, 1);
        assert_eq!(records, [(0, 1, vec![])]);
    }

    /// Links too long for one record go in several, in order, none split,
    /// and the record ids wrap.
    #[test]
    fn long_links_are_split_between_links() {
        let mut long = vec![b'a'; 3000];
        long.splice(..0, *b"web:https://example.org/");
        let links: Vec<_> = (1..=5).map(|drawing| DrawingLink { drawing, target: &long }).collect();
        let (sent, records) = sent(&links);
        assert_eq!((sent.records, sent.next_record_id), (3, 253));
        let drawings: Vec<Vec<u16>> =
            records.iter().map(|(_, _, links)| links.iter().map(|(drawing, _)| *drawing).collect()).collect();
        assert_eq!(drawings, [vec![1, 2], vec![3, 4], vec![5]]);
        assert_eq!(
            records.iter().map(|(part, parts, _)| (*part, *parts)).collect::<Vec<_>>(),
            [(0, 3), (1, 3), (2, 3)]
        );
    }

    /// The body's layout, byte for byte.
    #[test]
    fn the_body_is_the_fields_then_the_links() {
        let body = [0x01, 0x23, 0x00, 0x02, 0, 1, 1, 0x00, 0x0C, 0x00, 0x03, b'2', b'0', b'0'];
        let record = read(&body).unwrap();
        assert_eq!((record.part, record.parts), (0, 1));
        assert_eq!(record.links().collect::<Vec<_>>(), [DrawingLink { drawing: 12, target: b"200" }]);
    }

    #[test]
    fn anything_else_is_refused() {
        let with = |tail: &[u8]| [&[0x01, 0x23, 0x00, 0x02][..], tail].concat();
        // A drawing of 0; a drawing twice; no target; a target with a
        // space; a target that is no target; a part past the parts; no
        // parts; more parts than links can have; a byte after the last
        // link; a link cut short.
        for bad in [
            with(&[0, 1, 1, 0, 0, 0, 3, b'2', b'0', b'0']),
            with(&[0, 1, 2, 0, 12, 0, 3, b'2', b'0', b'0', 0, 12, 0, 3, b'3', b'0', b'0']),
            with(&[0, 1, 1, 0, 12, 0, 0]),
            with(&[0, 1, 1, 0, 12, 0, 3, b'2', b' ', b'0']),
            with(&[0, 1, 1, 0, 12, 0, 3, b'z', b'z', b'z']),
            with(&[1, 1, 0]),
            with(&[0, 0, 0]),
            with(&[0, 65, 0]),
            with(&[0, 1, 1, 0, 12, 0, 3, b'2', b'0', b'0', 0]),
            with(&[0, 1, 1, 0, 12, 0, 3, b'2', b'0']),
        ] {
            assert!(read(&bad).is_err(), "{bad:?}");
        }
        // A 65th link is refused by the count, and by the writer.
        assert!(read(&with(&[0, 1, 65])).is_err());
        let many: Vec<_> = (1..=65).map(|drawing| DrawingLink { drawing, target: b"200" }).collect();
        assert!(encode(page(), SubCode::new(2), &many, 3, 0, &mut |_| {}).is_err());
        assert!(encode(page(), SubCode::new(2), many.split_last().unwrap().1, 3, 0, &mut |_| {}).is_ok());
    }
}
