//! Instrument sounds rendered in order (REQ-6411, DEC-6410). A block of an
//! instrument codec is 20 ms of the instrument's input, and what it sounds
//! like depends on every block before it, as an Opus frame depends on the
//! frames before it. So the receiver keeps [`crate::INSTRUMENT_LANES`]
//! instruments, each following one sound from a block on: a stored sound's
//! blocks are rendered from the first, as far as they have arrived without
//! a gap, and its instrument is freed when the last is rendered; a live
//! sound keeps its instrument, which plays on through a block that never
//! arrived.
//!
//! Rendering is work the sound's sender chose, so a call renders at most
//! [`RENDER_BUDGET_BLOCKS`] blocks of a stored sound and says whether more
//! wait; the receiver asks to be ticked again at once while they do
//! ([`super::Receiver::next_tick_ms`]). A sound of any length therefore
//! never holds the port for longer than the budget's blocks cost.

#[cfg(feature = "dx7")]
use neoteletext_audio_instruments::dx7::{Bank, Patch, Rack};
#[cfg(feature = "sid")]
use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, Sid};

use super::Statistics;
use crate::Codec;
use crate::port::{SoundId, VoiceSink};
use crate::store::{Entry, Store};

/// The samples of one block, as a buffer's length.
const BLOCK: usize = crate::INSTRUMENT_BLOCK_SAMPLES as usize;

/// The most blocks of a stored sound one call renders: one second of sound.
/// A larger budget starts a long sound sooner and holds the port longer.
pub(super) const RENDER_BUDGET_BLOCKS: u16 = 50;

/// One instrument and the sound it follows.
struct Lane {
    /// The sound, or `None` while free.
    sound: Option<SoundId>,
    /// The next block it renders.
    next: u16,
    /// The codec of the sound it follows: which of its instruments plays.
    codec: Codec,
    /// The chip, for a SID sound.
    #[cfg(feature = "sid")]
    sid: Sid,
    /// The rack, for a DX7 sound.
    #[cfg(feature = "dx7")]
    rack: Rack,
}

/// The receiver's instruments.
pub(super) struct Lanes {
    lanes: [Lane; crate::INSTRUMENT_LANES],
}

impl Lanes {
    pub(super) fn new() -> Self {
        Self {
            lanes: core::array::from_fn(|_| Lane {
                sound: None,
                next: 0,
                codec: Codec::Sid,
                #[cfg(feature = "sid")]
                sid: Sid::new(ChipModel::Mos6581, ChipClock::Pal),
                #[cfg(feature = "dx7")]
                rack: Rack::new(),
            }),
        }
    }

    /// Frees every instrument: the sink has dropped every sound.
    pub(super) fn clear(&mut self) {
        for lane in &mut self.lanes {
            lane.sound = None;
        }
    }

    /// Frees the instrument that follows `sound`, if one does.
    pub(super) fn release(&mut self, sound: SoundId) {
        for lane in self.lanes.iter_mut().filter(|lane| lane.sound == Some(sound)) {
            lane.sound = None;
        }
    }

    /// Whether a stored sound's blocks wait to be rendered: its instrument
    /// stands at a block that has arrived.
    pub(super) fn pending<const OBJECTS: usize>(&self, store: &Store<'_, OBJECTS>) -> bool {
        self.lanes.iter().any(|lane| {
            lane.sound.is_some_and(|sound| {
                store.entries().any(|entry| entry.sound == sound && !entry.live && entry.at_sink)
                    && store.block(sound, lane.next).is_some()
            })
        })
    }

    /// The stored sounds whose instruments have blocks waiting, for the
    /// receiver to go on rendering.
    pub(super) fn waiting<const OBJECTS: usize>(
        &self,
        store: &Store<'_, OBJECTS>,
    ) -> [Option<SoundId>; crate::INSTRUMENT_LANES] {
        let mut waiting = [None; crate::INSTRUMENT_LANES];
        for (slot, lane) in waiting.iter_mut().zip(&self.lanes) {
            *slot = lane.sound.filter(|&sound| {
                store.entries().any(|entry| entry.sound == sound && !entry.live && entry.at_sink)
                    && store.block(sound, lane.next).is_some()
            });
        }
        waiting
    }

    /// The instrument that follows `entry`'s sound; or a free one, or one
    /// whose sound the store no longer holds, made ready for `entry` and
    /// started at block `first`; or none, while every instrument is busy.
    fn lane<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        first: u16,
    ) -> Option<&mut Lane> {
        let sound = entry.sound;
        let found = self.lanes.iter().position(|lane| lane.sound == Some(sound)).or_else(|| {
            self.lanes
                .iter()
                .position(|lane| lane.sound.is_none_or(|held| store.entries().all(|entry| entry.sound != held)))
        })?;
        let lane = self.lanes.get_mut(found)?;
        if lane.sound != Some(sound) {
            lane.sound = Some(sound);
            lane.next = first;
            lane.codec = entry.codec;
            lane.prepare(store, entry);
        }
        Some(lane)
    }

    /// Renders a stored sound's blocks from where its instrument stands, as
    /// far as they have arrived without a gap and no further than the
    /// budget, and gives the sink their samples.
    pub(super) fn send_stored<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let Some(lane) = self.lane(store, entry, 0) else { return };
        let mut pcm = [0_i16; BLOCK];
        for _ in 0..RENDER_BUDGET_BLOCKS {
            if lane.next >= entry.blocks {
                break;
            }
            let Some(block) = store.block(entry.sound, lane.next) else { break };
            lane.render(block, &mut pcm, statistics);
            let samples = usize::try_from(entry.block_samples(lane.next)).unwrap_or(0).min(BLOCK);
            let first = u32::from(lane.next).saturating_mul(entry.samples_per_block());
            sink.samples(entry.sound, first, pcm.get(..samples).unwrap_or(&[]));
            lane.next = lane.next.saturating_add(1);
        }
        if lane.next >= entry.blocks {
            lane.sound = None;
        }
    }

    /// Renders a live sound's blocks in index order from its cursor: a
    /// block still missing [`crate::LIVE_LOOKAHEAD_BLOCKS`] behind the
    /// newest is played through with no input, so a lost block never stops
    /// the sound. Answers the next block, for the store's cursor.
    pub(super) fn send_live<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) -> u16 {
        let mut next = entry.next;
        let Some(newest) = entry.newest else { return next };
        let Some(lane) = self.lane(store, entry, next) else { return next };
        let mut pcm = [0_i16; BLOCK];
        // At most a ring's worth a call: the cursor never laps the ring.
        for _ in 0..entry.blocks {
            if crate::serial_newer(next, newest) {
                break;
            }
            match store.live_block(entry.sound, next) {
                Some(block) => lane.render(block, &mut pcm, statistics),
                None if newest.wrapping_sub(next) >= crate::LIVE_LOOKAHEAD_BLOCKS => {
                    lane.conceal(&mut pcm);
                    statistics.concealed = statistics.concealed.saturating_add(1);
                    sink.concealed(entry.sound, next, crate::INSTRUMENT_BLOCK_SAMPLES);
                }
                None => break,
            }
            sink.stream_samples(entry.sound, &pcm);
            next = next.wrapping_add(1);
        }
        lane.next = next;
        next
    }
}

impl Lane {
    /// Makes the lane's instrument ready for `entry` from its start: the
    /// chip its sound names, reset; or a rack, reset, with the patches of
    /// the bank its sound names.
    #[cfg_attr(
        not(feature = "dx7"),
        expect(unused_variables, reason = "only a DX7 sound reads its bank from the store")
    )]
    #[cfg_attr(
        not(any(feature = "sid", feature = "dx7")),
        expect(clippy::unused_self, reason = "a build with no instrument prepares nothing")
    )]
    fn prepare<const OBJECTS: usize>(&mut self, store: &Store<'_, OBJECTS>, entry: &Entry) {
        match entry.codec {
            #[cfg(feature = "sid")]
            Codec::Sid => {
                let model = if entry.param & crate::record::SID_CHIP_8580 == 0 {
                    ChipModel::Mos6581
                } else {
                    ChipModel::Mos8580
                };
                let clock =
                    if entry.param & crate::record::SID_CLOCK_NTSC == 0 { ChipClock::Pal } else { ChipClock::Ntsc };
                self.sid = Sid::new(model, clock);
            }
            #[cfg(feature = "dx7")]
            Codec::Dx7 => {
                self.rack.reset();
                self.rack.set_bank(&bank(store, entry.param));
            }
            // No other codec has an instrument, and none of these is stored
            // in a build without it (`Codec::from_code`).
            _ => {}
        }
    }

    /// Plays one block into the lane's instrument and renders its 20 ms.
    #[cfg_attr(not(feature = "dx7"), expect(unused_variables, reason = "only a DX7 block counts what it passes over"))]
    #[cfg_attr(
        not(any(feature = "sid", feature = "dx7")),
        expect(clippy::unused_self, reason = "a build with no instrument renders nothing")
    )]
    fn render(&mut self, block: &[u8], pcm: &mut [i16; BLOCK], statistics: &mut Statistics) {
        match self.codec {
            #[cfg(feature = "sid")]
            Codec::Sid => render_sid(&mut self.sid, block, pcm),
            #[cfg(feature = "dx7")]
            Codec::Dx7 => render_dx7(&mut self.rack, block, pcm, statistics),
            _ => pcm.fill(0),
        }
    }

    /// Renders 20 ms for a block that never arrived (REQ-6411): the chip
    /// sounds on from the registers it holds; a rack releases every note
    /// first, since the lost block may have held a note off.
    fn conceal(&mut self, pcm: &mut [i16; BLOCK]) {
        match self.codec {
            #[cfg(feature = "sid")]
            Codec::Sid => self.sid.render(pcm),
            #[cfg(feature = "dx7")]
            Codec::Dx7 => {
                self.rack.release_all();
                self.rack.render(pcm);
            }
            _ => pcm.fill(0),
        }
    }
}

/// The bank a DX7 sound names: the patches of the stored bank at `handle`,
/// each checked whole. A patch that has not arrived, or that has a field
/// out of its range, leaves the rack's own plain patch in its place, so a
/// faulty bank costs a sound its timbre and nothing else (REQ-6411 item 4).
#[cfg(feature = "dx7")]
fn bank<const OBJECTS: usize>(store: &Store<'_, OBJECTS>, handle: u8) -> Bank {
    let mut bank = Bank::new();
    let Some(entry) = store.entry(handle).filter(|entry| entry.codec == Codec::Dx7Bank) else { return bank };
    for index in 0..entry.blocks {
        let patch = store
            .block(entry.sound, index)
            .and_then(|block| <&[u8; crate::DX7_PATCH_BYTES as usize]>::try_from(block).ok())
            .and_then(|packed| Patch::from_packed(packed).ok());
        if let Some(patch) = patch {
            bank.set(usize::from(index), patch);
        }
    }
    bank
}

/// A SID block: each of its register images written at its step of the
/// 20 ms, register 0 first (REQ-6411 item 2).
#[cfg(feature = "sid")]
fn render_sid(sid: &mut Sid, block: &[u8], pcm: &mut [i16; BLOCK]) {
    let (images, _) = block.as_chunks::<{ crate::SID_IMAGE_BYTES as usize }>();
    // 1, 2 or 4 images (`Codec::allows_block`), each of which divides the
    // block's samples exactly.
    let step = BLOCK.checked_div(images.len()).unwrap_or(BLOCK).max(1);
    let mut rest = pcm.as_mut_slice();
    for image in images {
        sid.write_image(image);
        let (now, later) = rest.split_at_mut(step.min(rest.len()));
        sid.render(now);
        rest = later;
    }
    // A block with no whole image, which no stored sound has, still sounds.
    sid.render(rest);
}

/// One slot of a DX7 block that holds a message the codec takes.
#[cfg(feature = "dx7")]
#[derive(Clone, Copy)]
struct Slot {
    /// When, in tenths of a millisecond from the block's start.
    time: u8,
    status: u8,
    data1: u8,
    data2: u8,
}

/// The top bit of a MIDI byte: set on a status byte, clear on a data byte
/// (MIDI 1.0).
#[cfg(feature = "dx7")]
const MIDI_STATUS_BIT: u8 = 0x80;
/// The controllers a DX7 block may carry (REQ-6411 item 3): the modulation
/// wheel, volume, the sustain pedal, all sound off, reset all controllers
/// and all notes off.
#[cfg(feature = "dx7")]
const CONTROLLERS: [u8; 6] = [1, 7, 64, 120, 121, 123];
/// The highest program a part may change to: the last patch of a bank.
#[cfg(feature = "dx7")]
const LAST_PROGRAM: u8 = (crate::DX7_BANK_PATCHES - 1) as u8;

/// The slot `bytes` hold, if it is a message the codec takes (REQ-6411 item
/// 3); `None` for an empty slot or one to pass over, the second counted.
#[cfg(feature = "dx7")]
fn slot(bytes: [u8; crate::DX7_SLOT_BYTES as usize], statistics: &mut Statistics) -> Option<Slot> {
    let [time, status, data1, data2] = bytes;
    if bytes == [0; crate::DX7_SLOT_BYTES as usize] {
        return None;
    }
    let data = data1 & MIDI_STATUS_BIT == 0 && data2 & MIDI_STATUS_BIT == 0;
    let taken = match status & 0xF0 {
        // Note off, note on and pitch bend.
        0x80 | 0x90 | 0xE0 => true,
        // Control change.
        0xB0 => CONTROLLERS.contains(&data1),
        // Program change.
        0xC0 => data1 <= LAST_PROGRAM && data2 == 0,
        _ => false,
    };
    if time > crate::DX7_SLOT_LAST_TIME || !data || !taken {
        statistics.slots_passed_over = statistics.slots_passed_over.saturating_add(1);
        return None;
    }
    Some(Slot { time, status, data1, data2 })
}

/// A DX7 block: its messages given to the rack in order of their time, and
/// in slot order where times are equal, each at its sample of the 20 ms
/// (REQ-6411 item 3).
#[cfg(feature = "dx7")]
fn render_dx7(rack: &mut Rack, block: &[u8], pcm: &mut [i16; BLOCK], statistics: &mut Statistics) {
    let mut slots = [None; crate::DX7_MAX_SLOTS as usize];
    let (quads, _) = block.as_chunks::<{ crate::DX7_SLOT_BYTES as usize }>();
    for (place, bytes) in slots.iter_mut().zip(quads) {
        *place = slot(*bytes, statistics);
    }
    // The times in order, each slot taken once: at most 16 slots, so the
    // walk is short, and it keeps slot order among equal times.
    let mut rest = pcm.as_mut_slice();
    let mut rendered: usize = 0;
    for time in 0..=crate::DX7_SLOT_LAST_TIME {
        for message in slots.iter().flatten().filter(|message| message.time == time) {
            // Tenths of a millisecond to samples at 48,000 Hz: 4.8 each.
            let at = usize::from(time).saturating_mul(24) / 5;
            let (now, later) = rest.split_at_mut(at.saturating_sub(rendered).min(rest.len()));
            rack.render(now);
            rendered = rendered.saturating_add(now.len());
            rest = later;
            rack.event(message.status, message.data1, message.data2);
        }
    }
    rack.render(rest);
}
