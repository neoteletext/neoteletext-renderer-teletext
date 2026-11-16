//! Loss (REQ-AUD-013, `design/audio.md` section 8.2): a block that has not
//! arrived when it is due is silence (faded into and out of, `fade`), and
//! reported as `concealed`. A stored sound's play is followed by the
//! voice's position where the sink reports one ([`VoiceSink::position`],
//! DEC-2470), and otherwise by the receiver's clock from where it was last
//! known, so that each block not at the sink when the play reached it is
//! reported once.

use super::{MS_PER_SECOND, Receiver, bump};
use crate::port::{SoundId, VoiceSink};
use crate::store::Entry;
use crate::{CHANNELS, Codec};

/// A stored sound's play followed by the receiver's clock, so that a block
/// not at the sink when the play reaches it is reported as `concealed`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Progress {
    sound: SoundId,
    /// When the play was last placed, in the receiver's milliseconds: when
    /// the sink was told to start it, or when the sink last reported the
    /// voice's position.
    anchor_ms: u64,
    /// The sample the voice was at then: 0 at the start.
    anchor_sample: u32,
    /// Blocks before this one have been judged.
    judged: u16,
}

impl Progress {
    /// A play of `entry` started at `now_ms`; `None` when nothing in it can
    /// be missing (a whole or a live sound) or its start is not known (a
    /// play after the current sound).
    pub(super) fn follow(entry: &Entry, after_current: bool, now_ms: u64) -> Option<Self> {
        (!entry.live && !entry.complete() && !after_current).then_some(Self {
            sound: entry.sound,
            anchor_ms: now_ms,
            anchor_sample: 0,
            judged: 0,
        })
    }
}

/// Samples of `entry` played by `elapsed_ms` after its start.
fn samples_by(entry: &Entry, elapsed_ms: u64) -> u64 {
    elapsed_ms.saturating_mul(u64::from(entry.rate_hz)).checked_div(u64::from(MS_PER_SECOND)).unwrap_or(0)
}

impl Progress {
    /// The sample the play has reached at `now_ms`, by the receiver's clock
    /// from where it was last placed.
    fn reached(&self, entry: &Entry, now_ms: u64) -> u64 {
        u64::from(self.anchor_sample).saturating_add(samples_by(entry, now_ms.saturating_sub(self.anchor_ms)))
    }
}

impl<const OBJECTS: usize, const SLOTS: usize> Receiver<'_, OBJECTS, SLOTS> {
    /// Whether block `index` of a stored sound is at the sink: a PCM or IMA
    /// ADPCM block once it has arrived, an Opus block or an instrument's once
    /// every block before it has too, since they are taken in order.
    fn at_sink(&self, entry: &Entry, index: u16) -> bool {
        if entry.codec == Codec::Opus || entry.codec.is_instrument() {
            return index < self.store.contiguous(entry.sound);
        }
        self.store.block(entry.sound, index).is_some()
    }

    /// Judges each followed play up to the receiver's time: every block whose
    /// start has passed and that is not at the sink is reported concealed.
    pub(super) fn judge(&mut self, sink: &mut dyn VoiceSink) {
        for index in 0..CHANNELS {
            let Some(mut progress) = self.progress.get(usize::from(index)).copied().flatten() else { continue };
            let entry = self.store.entries().find(|entry| entry.sound == progress.sound).copied();
            let Some(entry) = entry else {
                self.end_progress(index);
                continue;
            };
            // The sink's word, where it has one, places the play afresh: it
            // knows its own latency and when it really started.
            if let Some(position) = sink.position(index) {
                progress.anchor_ms = self.now_ms;
                progress.anchor_sample = position;
            }
            let played = progress.reached(&entry, self.now_ms);
            let started = played.checked_div(u64::from(entry.samples_per_block())).unwrap_or(0).saturating_add(1);
            let due = u16::try_from(started.min(u64::from(entry.blocks))).unwrap_or(entry.blocks);
            for block in progress.judged..due {
                if !self.at_sink(&entry, block) {
                    bump(&mut self.statistics.concealed);
                    sink.concealed(entry.sound, block, entry.block_samples(block));
                }
            }
            if due >= entry.blocks {
                self.end_progress(index);
            } else if let Some(Some(slot)) = self.progress.get_mut(usize::from(index)) {
                progress.judged = due;
                *slot = progress;
            }
        }
    }

    /// When the next followed block that is not at the sink falls due, in
    /// the receiver's milliseconds: the moment its first sample's time
    /// passes, counted from where the play was last placed. A sink whose
    /// voice runs behind the receiver's clock so moves the due time later at
    /// each tick, rather than asking for a tick that is already past.
    pub(super) fn next_due_ms(&self) -> Option<u64> {
        self.progress
            .iter()
            .flatten()
            .filter_map(|progress| {
                let entry = self.store.entries().find(|entry| entry.sound == progress.sound)?;
                let block = (progress.judged..entry.blocks).find(|&block| !self.at_sink(entry, block))?;
                let first = u64::from(block).saturating_mul(u64::from(entry.samples_per_block()));
                let ahead = first.saturating_sub(u64::from(progress.anchor_sample));
                let after_ms = ahead.saturating_mul(u64::from(MS_PER_SECOND)).div_ceil(u64::from(entry.rate_hz.max(1)));
                Some(progress.anchor_ms.saturating_add(after_ms))
            })
            .min()
    }

    /// Follows the play just started on channel `index`, or nothing.
    pub(super) fn set_progress(&mut self, index: u8, progress: Option<Progress>) {
        if let Some(slot) = self.progress.get_mut(usize::from(index)) {
            *slot = progress;
        }
    }

    /// Where the voice playing `sound` stands at the sink, if the sink says:
    /// the furthest back of the channels playing it, the one a late sample
    /// would reach first.
    #[cfg(feature = "opus")]
    pub(super) fn voice_position(&self, sound: SoundId, sink: &dyn VoiceSink) -> Option<u32> {
        (0..CHANNELS)
            .filter(|&index| {
                self.channels
                    .get(usize::from(index))
                    .is_some_and(|channel| channel.playing.is_some_and(|play| play.sound == sound))
            })
            .filter_map(|index| sink.position(index))
            .min()
    }

    /// Stops following channel `index`'s play: it was stopped or replaced.
    pub(super) fn end_progress(&mut self, index: u8) {
        self.set_progress(index, None);
    }
}
