//! Opus sounds decoded in order (decision E.124). A PCM or IMA ADPCM block
//! decodes on its own; an Opus packet's frame depends on the frames before
//! it. So the receiver keeps [`crate::OPUS_DECODERS`] decoders, each
//! following one sound from a block on: a stored sound's blocks are decoded
//! from the first, as far as they have arrived without a gap, and its
//! decoder is freed when the last is decoded; a live sound keeps its
//! decoder, which conceals a block that never arrived.
//!
//! A stored sound's decoded samples end where its blocks stop arriving, and
//! the sink plays silence beyond them until more come. So the last 5 ms of
//! what is decoded are held back, and sent faded out while the next block is
//! missing, then as they were once it is decoded (REQ-AUD-013). When the
//! blocks come again after the voice has already passed the end of what it
//! had, into silence, the samples it meets are faded in from where it is,
//! if the sink says where that is ([`VoiceSink::position`], DEC-2470).

use crate::copy_into;
use neoteletext_opus::{Decoder, FRAME_SAMPLES};

use crate::port::{SoundId, VoiceSink};
use crate::store::{Entry, Store};

use super::Statistics;
use super::fade::{MAX_FADE_SAMPLES, fade_in_from, fade_out, fade_samples};

/// One decoder and the sound it follows.
struct Lane {
    /// The sound, or `None` while free.
    sound: Option<SoundId>,
    /// The next block it decodes.
    next: u16,
    decoder: Decoder,
    /// The last 5 ms decoded of a stored sound, held back from the sink.
    tail: [i16; MAX_FADE_SAMPLES],
    /// How many of `tail` are held.
    tail_len: usize,
    /// The sound's sample at which the held tail starts.
    tail_at: u32,
    /// A stored sound's decoding stopped at a missing block, its end faded
    /// out: the voice may pass into the silence beyond it.
    gapped: bool,
}

/// The receiver's Opus decoders.
pub(super) struct Lanes {
    lanes: [Lane; crate::OPUS_DECODERS],
}

impl Lanes {
    pub(super) fn new() -> Self {
        Self {
            lanes: core::array::from_fn(|_| Lane {
                sound: None,
                next: 0,
                decoder: Decoder::new(),
                tail: [0; MAX_FADE_SAMPLES],
                tail_len: 0,
                tail_at: 0,
                gapped: false,
            }),
        }
    }

    /// Frees every decoder: the sink has dropped every sound.
    pub(super) fn clear(&mut self) {
        for lane in &mut self.lanes {
            lane.sound = None;
        }
    }

    /// Frees the decoder that follows `sound`, if one does.
    pub(super) fn release(&mut self, sound: SoundId) {
        for lane in self.lanes.iter_mut().filter(|lane| lane.sound == Some(sound)) {
            lane.sound = None;
        }
    }

    /// The decoder that follows `sound`; or a free one, or one whose sound
    /// the store no longer holds, started at block `first`; or none, while
    /// every decoder is busy.
    fn lane<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        sound: SoundId,
        first: u16,
    ) -> Option<&mut Lane> {
        let found = self.lanes.iter().position(|lane| lane.sound == Some(sound)).or_else(|| {
            self.lanes
                .iter()
                .position(|lane| lane.sound.is_none_or(|held| store.entries().all(|entry| entry.sound != held)))
        })?;
        let lane = self.lanes.get_mut(found)?;
        if lane.sound != Some(sound) {
            lane.sound = Some(sound);
            lane.next = first;
            lane.tail_len = 0;
            lane.gapped = false;
            lane.decoder.reset();
        }
        Some(lane)
    }

    /// Decodes a stored sound's blocks from where its decoder stands, as far
    /// as they have arrived without a gap, and gives the sink their samples.
    /// `voice` is where the voice playing it stands, if the sink says.
    pub(super) fn send_stored<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        voice: Option<u32>,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let Some(lane) = self.lane(store, entry.sound, 0) else { return };
        let mut pcm = [0_i16; FRAME_SAMPLES];
        let fade = fade_samples(entry.rate_hz);
        // Decoding resumes after a gap: where the voice has passed the start
        // of what is written now, it is playing the silence beyond the faded
        // end, and meets these samples mid-block, so they fade in from it.
        let resumes_at = if lane.tail_len > 0 {
            lane.tail_at
        } else {
            u32::from(lane.next).saturating_mul(entry.samples_per_block())
        };
        let fade_from = voice.filter(|&position| lane.gapped && position > resumes_at);
        if store.block(entry.sound, lane.next).is_some() {
            lane.gapped = false;
        }
        while lane.next < entry.blocks {
            let Some(packet) = store.block(entry.sound, lane.next) else { break };
            if lane.decoder.decode(packet, &mut pcm).is_err() {
                // A refused packet keeps the timeline: its frame is concealed.
                statistics.decode_failures = statistics.decode_failures.saturating_add(1);
                lane.decoder.conceal(&mut pcm);
            }
            // The tail held before this block goes as it was decoded.
            if lane.tail_len > 0 {
                let tail = lane.tail.get_mut(..lane.tail_len).unwrap_or(&mut []);
                if let Some(from) = fade_from {
                    fade_in_from(tail, lane.tail_at, from, fade);
                }
                sink.samples(entry.sound, lane.tail_at, tail);
                lane.tail_len = 0;
            }
            let samples = usize::try_from(entry.block_samples(lane.next)).unwrap_or(0).min(pcm.len());
            let first = u32::from(lane.next).saturating_mul(entry.samples_per_block());
            if let Some(from) = fade_from {
                fade_in_from(pcm.get_mut(..samples).unwrap_or(&mut []), first, from, fade);
            }
            lane.next = lane.next.saturating_add(1);
            // The final block has nothing after it to wait for.
            let held = if lane.next < entry.blocks { fade.min(samples) } else { 0 };
            let kept = samples.saturating_sub(held);
            sink.samples(entry.sound, first, pcm.get(..kept).unwrap_or(&[]));
            if let (Some(target), Some(source)) = (lane.tail.get_mut(..held), pcm.get(kept..samples)) {
                copy_into(target, source);
                lane.tail_len = held;
                lane.tail_at = first.saturating_add(u32::try_from(kept).unwrap_or(0));
            }
        }
        if lane.next >= entry.blocks {
            lane.sound = None;
        } else if lane.tail_len > 0 {
            // The next block is missing: the sink fades into the silence
            // after what it holds, and gets the tail again unfaded when the
            // block comes.
            let mut faded = lane.tail;
            let faded = faded.get_mut(..lane.tail_len).unwrap_or(&mut []);
            fade_out(faded);
            sink.samples(entry.sound, lane.tail_at, faded);
            lane.gapped = true;
        }
    }

    /// Decodes a live sound's blocks in index order from its cursor: a block
    /// still missing [`crate::LIVE_LOOKAHEAD_BLOCKS`] behind the newest is
    /// concealed, so a lost packet never stops the sound. Answers the next
    /// block, for the store's cursor.
    pub(super) fn send_live<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) -> u16 {
        let mut next = entry.next;
        let Some(newest) = entry.newest else { return next };
        let Some(lane) = self.lane(store, entry.sound, next) else { return next };
        let mut pcm = [0_i16; FRAME_SAMPLES];
        // At most a ring's worth a call: the cursor never laps the ring.
        for _ in 0..entry.blocks {
            if crate::serial_newer(next, newest) {
                break;
            }
            match store.live_block(entry.sound, next) {
                Some(packet) => {
                    if lane.decoder.decode(packet, &mut pcm).is_err() {
                        statistics.decode_failures = statistics.decode_failures.saturating_add(1);
                        lane.decoder.conceal(&mut pcm);
                    }
                }
                None if newest.wrapping_sub(next) >= crate::LIVE_LOOKAHEAD_BLOCKS => {
                    lane.decoder.conceal(&mut pcm);
                    statistics.concealed = statistics.concealed.saturating_add(1);
                    sink.concealed(entry.sound, next, crate::OPUS_FRAME_SAMPLES);
                }
                None => break,
            }
            super::stream_out(sink, entry, &pcm);
            next = next.wrapping_add(1);
        }
        lane.next = next;
        next
    }
}
