//! The object store (REQ-AUD-009): sounds kept compressed in an arena the
//! port provides, each with a bitmap of the blocks received. An object is
//! admitted only if it fits once complete; the oldest objects nothing needs
//! are evicted to make room, and nothing leaves for a sound that is refused.
//!
//! A stored sound of an event codec, whose blocks vary in length, is kept
//! as the bytes sent and a table of where each block starts (MIDI-45); a
//! live one is a ring of slots of its longest block, each with the length
//! it holds (MIDI-46). A bank is a stored sound whose blocks are pieces of
//! one image, kept back to back so the whole image is one run of bytes
//! ([`Store::image`]).

use crate::port::SoundId;
use crate::record::{Data, event_blocks};
use crate::{Codec, MOST_BANKS, copy_into};

mod arena;
mod live;

/// A stored sound's definition and state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Its handle and generation.
    pub sound: SoundId,
    /// Stable across generations.
    pub id: u32,
    /// The codec.
    pub codec: Codec,
    /// Samples a second.
    pub rate_hz: u32,
    /// Bytes in each whole block.
    pub block_bytes: u16,
    /// Samples in the whole sound.
    pub samples: u32,
    /// Blocks in the whole sound.
    pub blocks: u16,
    /// What its codec needs to know of it: a sound of the synth6581
    /// family's chip and clock byte ([`crate::record::TAG_SYNTH6581_CHIP`]),
    /// a sound's variant ([`crate::record::TAG_VARIANT`]), or the code of
    /// the family a bank is for ([`crate::record::TAG_BANK_FAMILY`]); 0 for
    /// every other.
    pub setting: u8,
    /// A synth8 sound's key mode and split point
    /// ([`crate::record::TAG_SYNTH8_KEYS`]), whole keyboard where absent.
    pub keys: [u8; 2],
    /// The handles of the banks a sound names, bank index 0 first
    /// ([`crate::record::TAG_BANKS`]); [`Entry::banks`] gives them.
    pub banks: [u8; MOST_BANKS],
    /// How many of [`Entry::banks`] are named.
    pub bank_count: u8,
    /// The total bytes of a stored event sound's blocks
    /// ([`crate::record::TAG_TOTAL_BYTES`]); 0 for every other sound.
    pub total: u32,
    /// Blocks received so far.
    pub received: u16,
    /// A block arrived twice with different bytes: the sound is not played
    /// until a newer generation replaces it.
    pub conflict: bool,
    /// The sink holds the sound's samples.
    pub at_sink: bool,
    /// A live sound (REQ-AUD-014): `blocks` slots of a ring, filled by block
    /// index modulo their count.
    pub live: bool,
    /// The newest block index of a live sound.
    pub newest: Option<u16>,
    /// The next block index of a live sound to give the sink.
    pub next: u16,
    /// When the last new block of a live sound arrived, in the receiver's
    /// milliseconds.
    pub arrived_ms: u64,
    /// Where a live sound's samples stand at the sink, for the fades either
    /// side of a block given as silence (REQ-AUD-013).
    pub edge: LiveEdge,
    offset: usize,
    stamp: u32,
}

impl Entry {
    /// Every block has arrived.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.received == self.blocks
    }

    /// Samples in a whole block.
    #[must_use]
    pub fn samples_per_block(&self) -> u32 {
        self.codec.samples_per_block(self.block_bytes)
    }

    /// Samples in block `index`: whole, or what is left in the final block.
    #[must_use]
    pub fn block_samples(&self, index: u16) -> u32 {
        let per_block = self.samples_per_block();
        if self.live {
            return per_block;
        }
        let before = u32::from(index).saturating_mul(per_block);
        self.samples.saturating_sub(before).min(per_block)
    }

    /// Bytes in block `index`, for a codec whose blocks are of the
    /// sound's fixed size (an event codec's vary, MIDI-39).
    #[must_use]
    pub fn block_length(&self, index: u16) -> usize {
        usize::try_from(self.codec.block_bytes_for(self.block_bytes, self.block_samples(index))).unwrap_or(usize::MAX)
    }

    /// The handles of the banks the sound names, bank index 0 first.
    #[must_use]
    pub fn banks(&self) -> &[u8] {
        self.banks.get(..usize::from(self.bank_count)).unwrap_or(&[])
    }

    /// A stored sound of an event codec: its blocks found by a table of
    /// offsets (MIDI-45).
    fn stored_events(&self) -> bool {
        self.codec.is_event() && !self.live
    }

    /// A live sound of an event codec: each slot of the ring holds a
    /// length (MIDI-46).
    fn live_events(&self) -> bool {
        self.codec.is_event() && self.live
    }

    /// Bytes of the blocks themselves: a stored event sound's total, or
    /// every slot of its size.
    fn data_bytes(&self) -> usize {
        if self.stored_events() {
            return usize::try_from(self.total).unwrap_or(usize::MAX);
        }
        usize::from(self.blocks).saturating_mul(usize::from(self.block_bytes))
    }

    /// Bytes of a stored event sound's table of block offsets, 4 a block.
    fn offsets_bytes(&self) -> usize {
        if self.stored_events() { usize::from(self.blocks).saturating_mul(OFFSET_BYTES) } else { 0 }
    }

    fn bitmap_bytes(&self) -> usize {
        usize::from(self.blocks).div_ceil(8)
    }

    /// A live sound's table of the block index in each slot.
    fn index_table_bytes(&self) -> usize {
        if self.live { usize::from(self.blocks).saturating_mul(SLOT_FIELD_BYTES) } else { 0 }
    }

    /// A live event sound's table of the length each slot holds.
    fn length_table_bytes(&self) -> usize {
        if self.live_events() { usize::from(self.blocks).saturating_mul(SLOT_FIELD_BYTES) } else { 0 }
    }

    /// Where the table of block offsets starts in the arena.
    fn offsets_at(&self) -> usize {
        self.offset.saturating_add(self.data_bytes())
    }

    /// Where the bitmap of blocks received starts.
    fn bitmap_at(&self) -> usize {
        self.offsets_at().saturating_add(self.offsets_bytes())
    }

    /// Where a live sound's table of block indices starts.
    fn index_table_at(&self) -> usize {
        self.bitmap_at().saturating_add(self.bitmap_bytes())
    }

    /// Where a live event sound's table of slot lengths starts.
    fn length_table_at(&self) -> usize {
        self.index_table_at().saturating_add(self.index_table_bytes())
    }

    fn size(&self) -> usize {
        self.data_bytes()
            .saturating_add(self.offsets_bytes())
            .saturating_add(self.bitmap_bytes())
            .saturating_add(self.index_table_bytes())
            .saturating_add(self.length_table_bytes())
    }
}

/// The bytes of one entry of a stored event sound's table of offsets
/// (MIDI-45).
const OFFSET_BYTES: usize = 4;
/// An entry of the table of offsets not yet known: no block's offset, which
/// is at most [`crate::MOST_EVENT_SOUND_BYTES`], is this.
const OFFSET_UNKNOWN: u32 = u32::MAX;
/// The bytes of each of a live sound's slot fields: its block index, and
/// for an event codec its length (MIDI-46).
const SLOT_FIELD_BYTES: usize = 2;

/// Where a live sound's samples stand at the sink (REQ-AUD-013). A block
/// given up as silence is faded into and out of over 5 ms; the fade into it
/// falls on the block before, which was sent before the loss was known, so
/// the last 5 ms of each block sent are held back until the next block's
/// fate is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LiveEdge {
    /// Nothing sent since the sound joined: the voice's own start ramps it in.
    #[default]
    Joined,
    /// The last 5 ms of block `next − 1` are held back; `faded_in` when that
    /// block followed a silence and its first 5 ms were faded in.
    Held {
        /// The held block followed a silence.
        faded_in: bool,
    },
    /// The last block sent was silence: the next block fades in.
    Gap,
}

/// What an OBJECT record did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Defined<const OBJECTS: usize> {
    /// A new sound; it may replace an older generation of its handle.
    New {
        /// The older generation it replaced.
        replaced: Option<Entry>,
        /// The sounds evicted to make room that the sink holds, for it to
        /// forget (REQ-AUD-009).
        evicted: [Option<SoundId>; OBJECTS],
    },
    /// The same definition again: nothing changes.
    Repeat,
    /// The codec is reserved: the object is ignored.
    Ignored,
    /// An older generation, or a different definition of the same
    /// generation: refused.
    Conflict,
    /// It would not fit even with every evictable sound gone: the store is
    /// as it was, an older generation of the handle included (DEC-2113).
    ResourceLimit,
}

/// What a DATA record did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stored {
    /// New blocks were stored: the first new block and how many.
    Blocks {
        /// The first block in the record.
        first: u16,
        /// Blocks in the record.
        count: u16,
    },
    /// Every block was there already.
    Repeat,
    /// No such sound or generation: discarded.
    Unknown,
    /// Blocks of the wrong length, or beyond the sound's end: discarded.
    Malformed,
    /// A block differed from the one stored: the sound is now in conflict.
    Conflict,
}

/// Compressed sounds in an arena of bytes.
pub struct Store<'a, const OBJECTS: usize> {
    arena: &'a mut [u8],
    entries: [Option<Entry>; OBJECTS],
    clock: u32,
}

impl<'a, const OBJECTS: usize> Store<'a, OBJECTS> {
    /// An empty store over `arena`.
    pub fn new(arena: &'a mut [u8]) -> Self {
        Self { arena, entries: [None; OBJECTS], clock: 0 }
    }

    /// The arena's size in bytes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.arena.len()
    }

    /// Forgets every sound.
    pub fn clear(&mut self) {
        self.entries = [None; OBJECTS];
    }

    /// The stored sound with this handle.
    #[must_use]
    pub fn entry(&self, handle: u8) -> Option<&Entry> {
        self.entries.iter().flatten().find(|entry| entry.sound.handle == handle)
    }

    /// Every stored sound.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().flatten()
    }

    /// Marks whether the sink holds a sound's samples.
    pub fn set_at_sink(&mut self, sound: SoundId, at_sink: bool) {
        if let Some(entry) = self.entries.iter_mut().flatten().find(|entry| entry.sound == sound) {
            entry.at_sink = at_sink;
        }
    }

    /// Records where a live sound's samples stand at the sink.
    pub fn set_live_edge(&mut self, sound: SoundId, edge: LiveEdge) {
        if let Some(entry) = self.entries.iter_mut().flatten().find(|entry| entry.sound == sound) {
            entry.edge = edge;
        }
    }

    /// Marks that the sink holds nothing.
    pub fn sink_lost(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.at_sink = false;
        }
    }

    /// Block `index` of a sound, if it has arrived.
    #[must_use]
    pub fn block(&self, sound: SoundId, index: u16) -> Option<&[u8]> {
        let entry = self.entries.iter().flatten().find(|entry| entry.sound == sound)?;
        if !self.received(entry, index) {
            return None;
        }
        if entry.stored_events() {
            let (start, end) = self.event_span(entry, index)?;
            return self.arena.get(entry.offset.saturating_add(start)..entry.offset.saturating_add(end));
        }
        let start = entry.offset.saturating_add(usize::from(index).saturating_mul(usize::from(entry.block_bytes)));
        self.arena.get(start..start.saturating_add(entry.block_length(index)))
    }

    /// The whole image of a bank that has wholly arrived (MIDI-52): its
    /// pieces lie back to back. `None` for another codec, a bank not
    /// complete, or one in conflict.
    #[must_use]
    pub fn image(&self, handle: u8) -> Option<&[u8]> {
        let entry = self.entry(handle)?;
        if entry.codec != Codec::Bank || !entry.complete() || entry.conflict {
            return None;
        }
        let length = usize::try_from(entry.samples).ok()?;
        self.arena.get(entry.offset..entry.offset.saturating_add(length))
    }

    /// The offset of block `index` of a stored event sound within its
    /// bytes, if it is known.
    fn event_offset(&self, entry: &Entry, index: u16) -> Option<u32> {
        let at = entry.offsets_at().saturating_add(usize::from(index).saturating_mul(OFFSET_BYTES));
        let bytes = self.arena.get(at..at.saturating_add(OFFSET_BYTES))?;
        let offset = u32::from_be_bytes(bytes.try_into().ok()?);
        (index < entry.blocks && offset != OFFSET_UNKNOWN).then_some(offset)
    }

    /// Where block `index` of a stored event sound starts and ends within
    /// its bytes: from its offset to the next block's, or to the total for
    /// the last (MIDI-45).
    fn event_span(&self, entry: &Entry, index: u16) -> Option<(usize, usize)> {
        let start = self.event_offset(entry, index)?;
        let next = index.saturating_add(1);
        let end = if next < entry.blocks { self.event_offset(entry, next)? } else { entry.total };
        Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
    }

    /// Writes the offset of block `index` of a stored event sound.
    fn set_event_offset(&mut self, entry: &Entry, index: u16, offset: u32) {
        if index >= entry.blocks {
            return;
        }
        let at = entry.offsets_at().saturating_add(usize::from(index).saturating_mul(OFFSET_BYTES));
        if let Some(target) = self.arena.get_mut(at..at.saturating_add(OFFSET_BYTES)) {
            copy_into(target, &offset.to_be_bytes());
        }
    }

    /// Whole blocks received from the start, contiguously.
    #[must_use]
    pub fn contiguous(&self, sound: SoundId) -> u16 {
        let Some(entry) = self.entries.iter().flatten().find(|entry| entry.sound == sound) else { return 0 };
        (0..entry.blocks).take_while(|&index| self.received(entry, index)).count().try_into().unwrap_or(0)
    }

    fn received(&self, entry: &Entry, index: u16) -> bool {
        let at = entry.bitmap_at().saturating_add(usize::from(index / 8));
        index < entry.blocks && self.arena.get(at).is_some_and(|byte| byte & (1 << (index % 8)) != 0)
    }

    /// Stores a DATA record's blocks. The record is judged whole before any
    /// of it is written, first each block's place and length and then each
    /// block already stored against the record's, so a record refused or in
    /// conflict stores nothing and the bitmap and `received` keep agreeing
    /// (REQ-AUD-003, REQ-AUD-005, REQ-AUD-009).
    pub fn put(&mut self, data: &Data<'_>) -> Stored {
        let Some(index) =
            self.entries.iter().position(|entry| entry.is_some_and(|entry| entry.sound.handle == data.handle))
        else {
            return Stored::Unknown;
        };
        let Some(mut entry) = self.entries.get(index).copied().flatten() else { return Stored::Unknown };
        if entry.sound.generation != data.generation {
            return Stored::Unknown;
        }
        if entry.live {
            return self.put_live(index, entry, data);
        }
        if entry.stored_events() {
            return self.put_events(index, entry, data);
        }
        if record_blocks(&entry, data).any(|block| block.is_none()) {
            return Stored::Malformed;
        }
        let differs = record_blocks(&entry, data)
            .flatten()
            .any(|(block, bytes)| self.block(entry.sound, block).is_some_and(|stored| stored != bytes));
        if differs {
            entry.conflict = true;
            if let Some(slot) = self.entries.get_mut(index) {
                *slot = Some(entry);
            }
            return Stored::Conflict;
        }
        let mut count: u16 = 0;
        let mut fresh = false;
        for (block, bytes) in record_blocks(&entry, data).flatten() {
            count = count.saturating_add(1);
            if self.received(&entry, block) {
                continue;
            }
            let start = entry.offset.saturating_add(usize::from(block).saturating_mul(usize::from(entry.block_bytes)));
            if let Some(target) = self.arena.get_mut(start..start.saturating_add(bytes.len())) {
                copy_into(target, bytes);
            }
            self.mark_received(&mut entry, block);
            fresh = true;
        }
        if let Some(slot) = self.entries.get_mut(index) {
            *slot = Some(entry);
        }
        if fresh { Stored::Blocks { first: data.first_block, count } } else { Stored::Repeat }
    }

    /// Sets block `block`'s bit in the bitmap and counts it received.
    fn mark_received(&mut self, entry: &mut Entry, block: u16) {
        let bit = entry.bitmap_at().saturating_add(usize::from(block / 8));
        if let Some(byte) = self.arena.get_mut(bit) {
            *byte |= 1 << (block % 8);
        }
        entry.received = entry.received.saturating_add(1);
    }

    /// Stores a DATA record of a stored event sound (MIDI-42 to MIDI-44):
    /// judged whole first, each block's number, length and offset against
    /// the sound and the offsets already held (MIDI-43), then each block
    /// already held against the record's (MIDI-44), and only then written.
    fn put_events(&mut self, position: usize, mut entry: Entry, data: &Data<'_>) -> Stored {
        let Some((offset, blocks)) = event_blocks(data.blocks) else { return Stored::Malformed };
        let mut start = offset;
        let mut number = data.first_block;
        let mut count: u16 = 0;
        for block in blocks.clone() {
            let Some(bytes) = block else { return Stored::Malformed };
            let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            let end = start.saturating_add(length);
            let next = number.saturating_add(1);
            // The start of the next block as the store holds it, or the
            // total after the last: the block must end exactly there.
            let next_start = if next < entry.blocks { self.event_offset(&entry, next) } else { Some(entry.total) };
            let faulty = length > u32::from(entry.block_bytes)
                || number >= entry.blocks
                || end > entry.total
                || (number == 0 && start != 0)
                || self.event_offset(&entry, number).is_some_and(|known| known != start)
                || next_start.is_some_and(|known| known != end);
            if faulty {
                return Stored::Malformed;
            }
            start = end;
            number = next;
            count = count.saturating_add(1);
        }
        if count == 0 {
            return Stored::Malformed;
        }
        let spans = || {
            let mut start = offset;
            (data.first_block..).zip(blocks.clone().flatten()).map(move |(number, bytes)| {
                let at = start;
                start = start.saturating_add(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
                (number, at, bytes)
            })
        };
        let differs =
            spans().any(|(number, _, bytes)| self.block(entry.sound, number).is_some_and(|held| held != bytes));
        if differs {
            entry.conflict = true;
            if let Some(slot) = self.entries.get_mut(position) {
                *slot = Some(entry);
            }
            return Stored::Conflict;
        }
        let mut fresh = false;
        for (number, at, bytes) in spans() {
            if self.received(&entry, number) {
                continue;
            }
            let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            let place = entry.offset.saturating_add(usize::try_from(at).unwrap_or(usize::MAX));
            if let Some(target) = self.arena.get_mut(place..place.saturating_add(bytes.len())) {
                copy_into(target, bytes);
            }
            self.set_event_offset(&entry, number, at);
            self.set_event_offset(&entry, number.saturating_add(1), at.saturating_add(length));
            self.mark_received(&mut entry, number);
            fresh = true;
        }
        if let Some(slot) = self.entries.get_mut(position) {
            *slot = Some(entry);
        }
        if fresh { Stored::Blocks { first: data.first_block, count } } else { Stored::Repeat }
    }
}

/// A DATA record's blocks in order, whole ones then possibly the final,
/// shorter one: each block's index and bytes, or `None` for a block past the
/// sound's end or cut short, which ends the walk.
fn record_blocks<'d>(entry: &Entry, data: &Data<'d>) -> impl Iterator<Item = Option<(u16, &'d [u8])>> + use<'d> {
    let entry = *entry;
    let mut rest = data.blocks;
    let mut block = data.first_block;
    core::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let parts = (block < entry.blocks).then(|| rest.split_at_checked(entry.block_length(block))).flatten();
        let Some((bytes, after)) = parts else {
            rest = &[];
            return Some(None);
        };
        rest = after;
        let index = block;
        block = block.saturating_add(1);
        Some(Some((index, bytes)))
    })
}

#[cfg(test)]
mod tests;
