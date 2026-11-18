//! Cue marks in the mixer (page-rules.md RULE-109; REQ-6614): a stream's
//! samples are rendered ahead of what is heard, so a cue of the sound comes
//! as a mark at its place in the stream, and is reported when a voice has
//! played the sample it stands at.

use super::{Mixer, VOICES};
use crate::sound::{Sound, SoundKey};

/// The most cue marks a stream holds waiting to be heard (RULE-109): one
/// more is dropped and counted. A block holds at most four cues and a
/// stream is rendered at most [`STREAM_TARGET_MS`](crate::STREAM_TARGET_MS)
/// ahead, so marks wait only as long as that.
pub const MAX_PENDING_CUE_MARKS: usize = 32;

/// A cue mark: a cue's number and the sample of its stream it stands at.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Mark {
    /// The sample, counted from the stream's first.
    position: i64,
    /// The cue's number.
    number: u8,
    /// The voices it has been reported for, a bit each.
    told: u8,
}

// A bit of `Mark::told` for each voice.
const _: () = assert!(VOICES <= u8::BITS as usize);

/// A stream's cue marks, in the order they came, held in place: the audio
/// callback reads and drops them without allocating or freeing.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Marks {
    marks: [Mark; MAX_PENDING_CUE_MARKS],
    count: usize,
}

impl Marks {
    /// No mark.
    pub(crate) const fn new() -> Self {
        Self { marks: [Mark { position: 0, number: 0, told: 0 }; MAX_PENDING_CUE_MARKS], count: 0 }
    }

    /// Forgets every mark.
    pub(crate) fn clear(&mut self) {
        self.count = 0;
    }

    /// Adds a mark, or answers `false` when [`MAX_PENDING_CUE_MARKS`] wait.
    fn push(&mut self, position: i64, number: u8) -> bool {
        let Some(place) = self.marks.get_mut(self.count) else { return false };
        *place = Mark { position, number, told: 0 };
        self.count = self.count.saturating_add(1);
        true
    }

    /// Reports each mark a voice has played past and has not been told of,
    /// in the order the marks came, then drops each mark every voice that
    /// plays the stream has been told of. `positions` is where each voice
    /// that plays the stream stands, the sample it plays next.
    fn take(&mut self, key: SoundKey, positions: &[Option<i64>; VOICES], heard: &mut dyn FnMut(HeardCue)) {
        let mut playing: u8 = 0;
        for (voice, position) in positions.iter().enumerate() {
            if position.is_some() {
                playing |= bit(voice);
            }
        }
        let mut kept: usize = 0;
        for index in 0..self.count {
            let Some(mut mark) = self.marks.get(index).copied() else { continue };
            for (voice, position) in positions.iter().enumerate() {
                if position.is_some_and(|whole| whole > mark.position) && mark.told & bit(voice) == 0 {
                    mark.told |= bit(voice);
                    heard(HeardCue { voice, key, number: mark.number });
                }
            }
            // Heard by every voice that plays it, and by at least one.
            if mark.told != 0 && mark.told & playing == playing {
                continue;
            }
            if let Some(place) = self.marks.get_mut(kept) {
                *place = mark;
            }
            kept = kept.saturating_add(1);
        }
        self.count = kept;
    }
}

/// Voice `voice`'s bit of [`Mark::told`].
fn bit(voice: usize) -> u8 {
    u32::try_from(voice).ok().and_then(|voice| 1_u8.checked_shl(voice)).unwrap_or(0)
}

/// A cue that was heard (RULE-109): the voice played the sample its mark
/// stands at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeardCue {
    /// The voice that played it, 0 to [`VOICES`] − 1: the audio channel.
    pub voice: usize,
    /// The stream's sound.
    pub key: SoundKey,
    /// The cue's number, as [`Mixer::stream_cue`] was given it.
    pub number: u8,
}

impl Mixer {
    /// Marks a cue of a live sound (RULE-109; REQ-6614 item 3): cue
    /// `number` stands at sample `position` of the stream `key`, counted
    /// from the first sample [`Mixer::stream_samples`] gave it since
    /// [`Mixer::stream`] made it. The mark may come before the samples it
    /// stands in. It is kept until a voice has played that sample, when
    /// [`Mixer::take_heard_cues`] reports it, or until the stream's voice
    /// is stopped or replaced, the stream is made again, or
    /// [`Mixer::forget_cues`] or [`Mixer::clear`] drops it.
    ///
    /// A stream holds at most [`MAX_PENDING_CUE_MARKS`]: a mark past them
    /// is dropped and counted ([`Mixer::cue_marks_dropped`]), and so is a
    /// mark for a sound that is not a live sound the mixer holds. Answers
    /// whether the mark was kept. It never allocates or frees.
    pub fn stream_cue(&mut self, key: SoundKey, position: u64, number: u8) -> bool {
        let sound = self.sounds.iter_mut().find(|sound| sound.key() == key && sound.is_live());
        let kept =
            sound.is_some_and(|sound| sound.marks_mut().push(i64::try_from(position).unwrap_or(i64::MAX), number));
        if !kept {
            self.cue_marks_dropped = self.cue_marks_dropped.saturating_add(1);
        }
        kept
    }

    /// The cue marks dropped since the mixer was made (RULE-109): those
    /// past a stream's [`MAX_PENDING_CUE_MARKS`], and those for a sound
    /// that is not a live sound the mixer holds.
    #[must_use]
    pub fn cue_marks_dropped(&self) -> u32 {
        self.cue_marks_dropped
    }

    /// Reports the cues heard since it was last called (RULE-109): each
    /// mark whose sample a voice has played, once for each voice that plays
    /// its stream, in the order the marks came. A port calls it after each
    /// device buffer it renders, so that a cue is reported after the
    /// buffer that holds its sample and before the next. A voice that is
    /// stopping, or waiting for its stream's samples, has heard nothing
    /// more; a voice the ring made skip ahead has passed the marks it
    /// skipped, and they are reported as heard, late rather than lost. It
    /// never allocates or frees.
    pub fn take_heard_cues(&mut self, heard: &mut dyn FnMut(HeardCue)) {
        let voices = &self.voices;
        for sound in self.sounds.iter_mut().filter(|sound| sound.has_marks()) {
            let key = sound.key();
            let positions: [Option<i64>; VOICES] = core::array::from_fn(|index| {
                let playing = voices.get(index)?.playing.filter(|playing| playing.key == key && !playing.stopping)?;
                Some(playing.whole)
            });
            sound.marks_mut().take(key, &positions, heard);
        }
    }

    /// Forgets every cue mark of the stream `key` (RULE-109: "a sound that
    /// is stopped, or a page that is paused or closed, forgets its
    /// marks"), for a port whose page is paused or closed while its
    /// stream plays on or fades.
    pub fn forget_cues(&mut self, key: SoundKey) {
        for sound in self.sounds.iter_mut().filter(|sound| sound.key() == key) {
            sound.marks_mut().clear();
        }
    }

    /// Forgets the cue marks of `key` when no voice plays it any longer
    /// but one that is stopping: a stopped sound has no further cue
    /// (RULE-109).
    pub(super) fn forget_stopped_cues(&mut self, key: SoundKey) {
        let heard = |playing: &super::Playing| playing.key == key && !playing.stopping;
        if !self.voices.iter().any(|voice| voice.playing.as_ref().is_some_and(heard)) {
            self.forget_cues(key);
        }
    }
}

impl Sound {
    /// Whether any cue mark waits.
    fn has_marks(&self) -> bool {
        self.marks().count > 0
    }
}

#[cfg(test)]
mod tests;
