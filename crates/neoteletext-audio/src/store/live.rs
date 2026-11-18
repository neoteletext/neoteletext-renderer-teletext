//! A live sound's ring of blocks (REQ-AUD-014): the newest replacing the
//! oldest, and the cursor of the block the sink gets next.

use super::{Entry, SLOT_FIELD_BYTES, Store, Stored};
use crate::copy_into;
use crate::port::SoundId;
use crate::record::Data;

impl<const OBJECTS: usize> Store<'_, OBJECTS> {
    /// Records which live block the sink gets next, and when one last
    /// arrived.
    pub fn set_live_cursor(&mut self, sound: SoundId, next: u16, arrived_ms: u64) {
        if let Some(entry) = self.entries.iter_mut().flatten().find(|entry| entry.sound == sound) {
            entry.next = next;
            entry.arrived_ms = arrived_ms;
        }
    }

    /// Block `index` of a live sound, if its slot holds that block.
    #[must_use]
    pub fn live_block(&self, sound: SoundId, index: u16) -> Option<&[u8]> {
        let entry = self.entries.iter().flatten().find(|entry| entry.sound == sound && entry.live)?;
        let slot = index.checked_rem(entry.blocks)?;
        if !self.received(entry, slot) || self.slot_index(entry, slot) != Some(index) {
            return None;
        }
        let length = if entry.live_events() {
            usize::from(self.slot_field(entry.length_table_at(), slot)?)
        } else {
            usize::from(entry.block_bytes)
        };
        let start = entry.offset.saturating_add(usize::from(slot).saturating_mul(usize::from(entry.block_bytes)));
        self.arena.get(start..start.saturating_add(length))
    }

    /// The block index a live sound's slot holds.
    fn slot_index(&self, entry: &Entry, slot: u16) -> Option<u16> {
        self.slot_field(entry.index_table_at(), slot)
    }

    /// Slot `slot`'s entry of the table of two-byte fields at `table`.
    fn slot_field(&self, table: usize, slot: u16) -> Option<u16> {
        let at = table.saturating_add(usize::from(slot).saturating_mul(SLOT_FIELD_BYTES));
        let bytes = self.arena.get(at..at.saturating_add(SLOT_FIELD_BYTES))?;
        Some(u16::from_be_bytes(bytes.try_into().ok()?))
    }

    /// Writes slot `slot`'s entry of the table of two-byte fields at `table`.
    fn set_slot_field(&mut self, table: usize, slot: u16, value: u16) {
        let at = table.saturating_add(usize::from(slot).saturating_mul(SLOT_FIELD_BYTES));
        if let Some(target) = self.arena.get_mut(at..at.saturating_add(SLOT_FIELD_BYTES)) {
            copy_into(target, &value.to_be_bytes());
        }
    }

    /// Stores a live sound's blocks, each whole, in the slot of its index
    /// modulo the ring. A block older than the ring is dropped. A live
    /// event sound's record has an offset of 0 and its blocks each their
    /// length, at most the slot's (MIDI-42, MIDI-43, MIDI-46).
    pub(super) fn put_live(&mut self, position: usize, mut entry: Entry, data: &Data<'_>) -> Stored {
        let Some(blocks) = live_blocks(&entry, data) else { return Stored::Malformed };
        // Compared before any is written, as `put` does: a record in conflict
        // stores nothing.
        let differs = blocks
            .clone()
            .any(|(index, bytes)| self.live_block(entry.sound, index).is_some_and(|stored| stored != bytes));
        if differs {
            entry.conflict = true;
            if let Some(place) = self.entries.get_mut(position) {
                *place = Some(entry);
            }
            return Stored::Conflict;
        }
        let mut fresh = false;
        let mut count: u16 = 0;
        for (index, bytes) in blocks {
            count = count.saturating_add(1);
            if entry
                .newest
                .is_some_and(|newest| newest.wrapping_sub(index) >= entry.blocks && !crate::serial_newer(index, newest))
            {
                continue;
            }
            let Some(slot) = index.checked_rem(entry.blocks) else { return Stored::Malformed };
            if self.received(&entry, slot) && self.slot_index(&entry, slot) == Some(index) {
                continue;
            }
            let start = entry.offset.saturating_add(usize::from(slot).saturating_mul(usize::from(entry.block_bytes)));
            if let Some(target) = self.arena.get_mut(start..start.saturating_add(bytes.len())) {
                copy_into(target, bytes);
            }
            self.set_slot_field(entry.index_table_at(), slot, index);
            if entry.live_events() {
                self.set_slot_field(entry.length_table_at(), slot, u16::try_from(bytes.len()).unwrap_or(0));
            }
            let bit = entry.bitmap_at().saturating_add(usize::from(slot / 8));
            if let Some(byte) = self.arena.get_mut(bit) {
                *byte |= 1 << (slot % 8);
            }
            if entry.newest.is_none_or(|newest| crate::serial_newer(index, newest)) {
                entry.newest = Some(index);
            }
            entry.received = entry.received.saturating_add(1);
            fresh = true;
        }
        if let Some(place) = self.entries.get_mut(position) {
            *place = Some(entry);
        }
        if fresh { Stored::Blocks { first: data.first_block, count } } else { Stored::Repeat }
    }
}

/// A live DATA record's blocks, each with its index, judged whole: `None`
/// for a record of blocks of the wrong length, or of an event codec with an
/// offset that is not 0, no block, or a block longer than a slot.
fn live_blocks<'d>(entry: &Entry, data: &Data<'d>) -> Option<LiveBlocks<'d>> {
    let first = data.first_block;
    if entry.live_events() {
        let (offset, blocks) = crate::record::event_blocks(data.blocks)?;
        let whole =
            blocks.clone().all(|block| block.is_some_and(|bytes| bytes.len() <= usize::from(entry.block_bytes)));
        if offset != 0 || !whole || blocks.clone().next().is_none() {
            return None;
        }
        return Some(LiveBlocks::Events { next: first, blocks });
    }
    let length = usize::from(entry.block_bytes);
    if length == 0 || !data.blocks.len().is_multiple_of(length) {
        return None;
    }
    Some(LiveBlocks::Fixed { next: first, chunks: data.blocks.chunks_exact(length) })
}

/// The blocks of a live DATA record, each with its index.
#[derive(Clone)]
enum LiveBlocks<'d> {
    /// Blocks of one size.
    Fixed { next: u16, chunks: core::slice::ChunksExact<'d, u8> },
    /// An event codec's blocks, each with its length.
    Events { next: u16, blocks: crate::record::EventBlocks<'d> },
}

impl<'d> Iterator for LiveBlocks<'d> {
    type Item = (u16, &'d [u8]);

    fn next(&mut self) -> Option<(u16, &'d [u8])> {
        let (next, bytes) = match self {
            Self::Fixed { next, chunks } => (next, chunks.next()?),
            Self::Events { next, blocks } => (next, blocks.next().flatten()?),
        };
        let index = *next;
        *next = next.wrapping_add(1);
        Some((index, bytes))
    }
}
