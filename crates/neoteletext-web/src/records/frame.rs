//! The FRAME record, type 27 (neoteletext-common `docs/animation.md`,
//! DEC-6524; ANIM-2): the begin and the end of an atomic frame, between
//! which a receiver holds everything that changes what a page shows and
//! then applies it all on one tick.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub: the sub-code, 0x0000 to 0x3F7E
//! byte  4     mark: 1 begin, 2 end
//! byte  5     reserved: 0
//! bytes 6-7   identifier: an end repeats its begin's
//! ```

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};

use super::{RecordFault, TYPE_FRAME, take_place, take_u16, write_packets};
use crate::consts::{MAX_FRAME_BYTES, MAX_FRAME_TICKS, MAX_OPEN_FRAMES};

/// The record's bytes.
pub const BYTES: usize = 2 + 2 + 1 + 1 + 2;

/// Which end of a frame a record marks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameMark {
    /// 1: what follows for the page is held.
    Begin,
    /// 2: what was held is applied, and shown on one tick.
    End,
}

impl FrameMark {
    /// The mark's byte in the record.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Begin => 1,
            Self::End => 2,
        }
    }

    /// The mark of `code`, or `None` for a byte no mark has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Begin),
            2 => Some(Self::End),
            _ => None,
        }
    }
}

/// One FRAME record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameRecord {
    /// The page.
    pub page: PageAddress,
    /// Its sub-page.
    pub subcode: SubCode,
    /// Begin or end.
    pub mark: FrameMark,
    /// The frame's identifier.
    pub identifier: u16,
}

/// Reads a FRAME record's body.
///
/// # Errors
/// A page, a sub-code or a mark outside its range, a reserved byte that
/// is not 0, or a body of any length but [`BYTES`].
pub fn read(body: &[u8]) -> Result<FrameRecord, RecordFault> {
    if body.len() != BYTES {
        return Err(RecordFault::Length);
    }
    let (page, subcode, rest) = take_place(body)?;
    let (&[mark, reserved], rest) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    let mark = FrameMark::from_code(mark).ok_or(RecordFault::BadValue("frame mark"))?;
    if reserved != 0 {
        return Err(RecordFault::BadValue("frame reserved byte"));
    }
    let (identifier, _) = take_u16(rest)?;
    Ok(FrameRecord { page, subcode, mark, identifier })
}

/// Writes a FRAME record as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    record: FrameRecord,
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
        .chain([record.mark.code(), 0])
        .chain(record.identifier.to_be_bytes());
    for (byte, field) in body.iter_mut().zip(fields) {
        *byte = field;
    }
    // The writer checks with the reader, so the two cannot disagree.
    if read(&body)? != record {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_FRAME, &body, service, record_id, out).ok_or(RecordFault::Length)
}

/// Why a frame ended (ANIM-2). However it ends, what it held is applied,
/// in order, and shown: a fault delays a picture and never loses one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameEnd {
    /// Its end arrived.
    Ended,
    /// It held [`MAX_FRAME_BYTES`].
    Full,
    /// [`MAX_FRAME_TICKS`] passed with no end.
    Late,
    /// A second begin arrived for the page.
    BegunAgain,
    /// The page stopped being shown, or the connection ended.
    Closed,
}

/// What a FRAME record did to a receiver's open frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameChange {
    /// A frame is now open for the page: what changes its picture is held.
    Opened,
    /// The page's frame ended: what it held is applied now.
    Ended(FrameEnd),
    /// A second begin: the frame that was open ended, what it held is
    /// applied now, and a new frame is open.
    Reopened,
    /// Nothing changed: an end with no frame open or another identifier,
    /// or a begin when [`MAX_OPEN_FRAMES`] pages already have one, which
    /// is treated as having no frame.
    Nothing,
}

/// One page's open frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct OpenFrame {
    page: PageAddress,
    identifier: u16,
    /// The bytes held so far.
    bytes: usize,
    /// The ticks since it began.
    ticks: u32,
}

/// Which pages have a frame open in a receiver, and how much each holds
/// (ANIM-2): at most [`MAX_OPEN_FRAMES`] pages, each at most
/// [`MAX_FRAME_BYTES`] for at most [`MAX_FRAME_TICKS`]. It holds no bytes
/// itself: the receiver keeps what it is told to hold, and applies it when
/// it is told the frame ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct OpenFrames {
    open: [Option<OpenFrame>; MAX_OPEN_FRAMES],
}

impl OpenFrames {
    /// No frame open.
    #[must_use]
    pub const fn new() -> Self {
        Self { open: [None; MAX_OPEN_FRAMES] }
    }

    /// Where the page's frame is.
    fn place(&self, page: PageAddress) -> Option<usize> {
        self.open.iter().position(|frame| frame.is_some_and(|frame| frame.page == page))
    }

    /// Whether the page has a frame open, so that what changes its picture
    /// is to be held.
    #[must_use]
    pub fn is_open(&self, page: PageAddress) -> bool {
        self.place(page).is_some()
    }

    /// How many pages have a frame open.
    #[must_use]
    pub fn count(&self) -> usize {
        self.open.iter().flatten().count()
    }

    /// The bytes held for the page's frame.
    #[must_use]
    pub fn held(&self, page: PageAddress) -> usize {
        self.place(page).and_then(|place| self.open.get(place).copied().flatten()).map_or(0, |frame| frame.bytes)
    }

    /// Takes a FRAME record.
    pub fn record(&mut self, record: &FrameRecord) -> FrameChange {
        let place = self.place(record.page);
        let fresh = OpenFrame { page: record.page, identifier: record.identifier, bytes: 0, ticks: 0 };
        match (record.mark, place) {
            (FrameMark::Begin, Some(place)) => {
                if let Some(slot) = self.open.get_mut(place) {
                    *slot = Some(fresh);
                }
                FrameChange::Reopened
            }
            (FrameMark::Begin, None) => match self.open.iter_mut().find(|slot| slot.is_none()) {
                Some(slot) => {
                    *slot = Some(fresh);
                    FrameChange::Opened
                }
                None => FrameChange::Nothing,
            },
            (FrameMark::End, Some(place)) => {
                let Some(slot) = self.open.get_mut(place) else { return FrameChange::Nothing };
                if slot.is_some_and(|frame| frame.identifier == record.identifier) {
                    *slot = None;
                    FrameChange::Ended(FrameEnd::Ended)
                } else {
                    FrameChange::Nothing
                }
            }
            (FrameMark::End, None) => FrameChange::Nothing,
        }
    }

    /// Something of `bytes` bytes that changes the page's picture arrived.
    /// `None`: the page has no frame open, and it is applied as it
    /// arrives. `Some(None)`: hold it. `Some(Some(FrameEnd::Full))`: the
    /// frame is full and has ended; what was held is applied now, and then
    /// this.
    pub fn hold(&mut self, page: PageAddress, bytes: usize) -> Option<Option<FrameEnd>> {
        let place = self.place(page)?;
        let slot = self.open.get_mut(place)?;
        let frame = slot.as_mut()?;
        frame.bytes = frame.bytes.saturating_add(bytes);
        if frame.bytes > MAX_FRAME_BYTES {
            *slot = None;
            return Some(Some(FrameEnd::Full));
        }
        Some(None)
    }

    /// A tick of the presentation clock passed (ANIM-1): each frame that
    /// has been open [`MAX_FRAME_TICKS`] ends, and its page is given to
    /// `ended` for what it held to be applied.
    pub fn tick(&mut self, ended: &mut dyn FnMut(PageAddress, FrameEnd)) {
        for slot in &mut self.open {
            let Some(frame) = slot.as_mut() else { continue };
            frame.ticks = frame.ticks.saturating_add(1);
            if frame.ticks >= MAX_FRAME_TICKS {
                let page = frame.page;
                *slot = None;
                ended(page, FrameEnd::Late);
            }
        }
    }

    /// The page stopped being shown: its frame, if it has one, ends, and
    /// what it held is applied. True when it had one.
    pub fn close(&mut self, page: PageAddress) -> bool {
        let Some(slot) = self.place(page).and_then(|place| self.open.get_mut(place)) else { return false };
        *slot = None;
        true
    }

    /// The connection ended: every frame ends, each page given to `ended`
    /// for what it held to be applied.
    pub fn close_all(&mut self, ended: &mut dyn FnMut(PageAddress, FrameEnd)) {
        for slot in &mut self.open {
            if let Some(frame) = slot.take() {
                ended(frame.page, FrameEnd::Closed);
            }
        }
    }
}

#[cfg(test)]
mod tests;
