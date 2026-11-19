//! Starting plays: a play starts once its sound is ready, or waits for it
//! (REQ-AUD-011 to REQ-AUD-013).

#[cfg(any(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
use super::instruments::Begun;
use super::loss::Progress;
use super::magazine::music_changed;
use super::{MS_PER_SECOND, Receiver, duration_ms};
use crate::control::Play;
use crate::port::{LocalStart, Start, VoiceSink};
use crate::store::{Entry, LiveEdge};
use crate::{CHANNELS, PROGRESSIVE_BUFFER_MS};

impl<const OBJECTS: usize, const SLOTS: usize> Receiver<'_, OBJECTS, SLOTS> {
    /// Starts a play now if its sound is ready, or keeps it waiting.
    pub(super) fn try_start(&mut self, index: u8, play: Play, sink: &mut dyn VoiceSink) {
        // Magazine music is a live sound, and sounds only where its gate
        // lets it (REQ-6925 items 2 and 3).
        let Some(music_percent) = self.starting_percent(index, play) else { return };
        let ready = self
            .store
            .entry(play.sound.handle)
            .copied()
            .filter(|entry| entry.sound == play.sound && !entry.conflict)
            // A sound that plays patches plays those of its banks, so it
            // waits for them (MIDI-62).
            .filter(|entry| self.bank_ready(entry))
            .filter(|entry| {
                if entry.live {
                    // A live sound plays while its blocks keep coming.
                    return entry.newest.is_some()
                        && self.now_ms.saturating_sub(entry.arrived_ms) < crate::LIVE_STALL_MS;
                }
                entry.complete()
                    || (play.progressive && self.buffered_ms(entry) >= PROGRESSIVE_BUFFER_MS.min(duration_ms(entry)))
            });
        // A bank is patches and is never played (MIDI-55).
        if self.store.entry(play.sound.handle).is_some_and(|entry| entry.codec.is_bank()) {
            return;
        }
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        let Some(entry) = ready else {
            channel.waiting = Some(play);
            return;
        };
        // A local sound is never rendered from its block nor streamed: the
        // sink is given its local start (RULE-60).
        if entry.local {
            self.start_local(index, play, &entry, sink);
            return;
        }
        self.end_local(index);
        let start_ms = self.start_ms(index, play);
        // A synthesizer's playing is a stream of its own, rendered as it
        // plays (INT-33, INT-37). It needs a lane (INT-32): without one the
        // play waits, and is tried again with the next record.
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        if entry.codec.is_instrument() {
            if !play.after_current {
                self.stop_streams(index, play.fade_ms, sink);
            }
            let begun = self.instruments.begin(
                &self.store,
                &entry,
                index,
                start_ms,
                play.looped,
                play.after_current,
                &mut self.statistics,
                sink,
            );
            if begun == Begun::NoLane {
                if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                    channel.waiting = Some(play);
                }
                return;
            }
            self.store.set_at_sink(entry.sound, true);
            self.played(index, play, &entry, start_ms, music_percent, sink);
            // The playing is a stream of its own, begun just now.
            if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                channel.labelled = None;
            }
            self.label(index, entry.sound, play.magazine, sink);
            if begun == Begun::Started {
                sink.start(index, start_of(play, music_percent));
            }
            return;
        }
        if !play.after_current {
            self.stop_streams(index, play.fade_ms, sink);
        }
        self.played(index, play, &entry, start_ms, music_percent, sink);
        if entry.live {
            let bit = 1_u8.checked_shl(u32::from(index)).unwrap_or(0);
            if entry.streams & bit == 0 {
                // Each audio channel that plays a live sound has its own
                // stream of it (INT-37), joined where the sound is now.
                sink.stream(index, entry.sound, entry.rate_hz);
                self.store.add_stream(entry.sound, index);
                if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                    channel.labelled = None;
                }
            }
            // Labelled before its samples are given (REQ-6925 item 6).
            self.label(index, entry.sound, play.magazine, sink);
            if !entry.at_sink {
                // Join at the newest block: a live sound is heard as it is now.
                self.store.set_at_sink(entry.sound, true);
                self.store.set_live_cursor(entry.sound, entry.newest.unwrap_or(0), entry.arrived_ms);
                self.store.set_live_edge(entry.sound, LiveEdge::Joined);
                self.send_live(entry.sound, sink);
            }
        } else if !entry.at_sink {
            self.send_sound(&entry, sink);
        }
        sink.start(index, start_of(play, music_percent));
    }

    /// Starts a local sound on channel `index` (page-rules.md, RULE-60):
    /// the sink is given the sound's banks and then its local start, with
    /// the setup block, in place of a stream. The play is refused and
    /// counted when it names no page (a local sound belongs to a page, and
    /// ends when that page is no longer open), when it is looped or waits
    /// for the current sound (it has no end to loop at or to follow), or
    /// when [`crate::MOST_LOCAL_SOUNDS`] already play on other channels.
    fn start_local(&mut self, index: u8, play: Play, entry: &Entry, sink: &mut dyn VoiceSink) {
        let others =
            self.local.iter().zip(0..CHANNELS).filter(|&(sound, channel)| sound.is_some() && channel != index).count();
        if play.page.is_none() || play.looped || play.after_current || others >= crate::MOST_LOCAL_SOUNDS {
            if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                channel.waiting = None;
            }
            self.statistics.local_refused = self.statistics.local_refused.saturating_add(1);
            return;
        }
        if self.store.block(entry.sound, 0).is_none() {
            return;
        }
        self.stop_streams(index, play.fade_ms, sink);
        // The setup is the sound's one block, which has arrived: a play
        // waits until its sound is whole.
        let Some(setup) = self.store.block(entry.sound, 0) else { return };
        for (bank, &handle) in (0_u8..).zip(entry.banks()) {
            // Each is held whole, or the play would still wait
            // (`Receiver::bank_ready`).
            if let Some(image) = self.store.image(handle) {
                sink.local_bank(index, entry.sound, bank, image);
            }
        }
        let [option, split] = entry.keys;
        let synth8 = entry.codec == crate::Codec::Synth8;
        sink.local(
            index,
            LocalStart {
                sound: entry.sound,
                family: entry.codec.code(),
                // The variant tag is the setting of the two families that
                // have one (`record::TAG_VARIANT`); the others' is 0.
                variant: if entry.codec.variants() > 1 { entry.setting } else { 0 },
                option: if synth8 { option } else { 0 },
                split: if synth8 { split } else { 0 },
                gain: play.gain,
                fade_ms: play.fade_ms,
                duck_percent: play.duck_percent,
                setup,
            },
        );
        if let Some(channel) = self.channels.get_mut(usize::from(index)) {
            channel.waiting = None;
            channel.playing_page = play.page;
            let was = channel.playing.replace(play);
            music_changed(index, was, Some(play), sink);
        }
        // It plays until it is stopped, and none of its blocks is awaited.
        self.end_progress(index);
        if let Some(heard) = self.heard.get_mut(usize::from(index)) {
            *heard = Heard::Unknown;
        }
        if let Some(local) = self.local.get_mut(usize::from(index)) {
            *local = Some(entry.sound);
        }
    }

    /// Channel `index` plays a local sound no longer (RULE-60).
    pub(super) fn end_local(&mut self, index: u8) {
        if let Some(local) = self.local.get_mut(usize::from(index)) {
            *local = None;
        }
    }

    /// The local sound channel `index` plays at the sink, if it plays one
    /// (RULE-60), for a port that gives its rules' MIDI messages to it.
    #[must_use]
    pub fn local_sound(&self, index: u8) -> Option<crate::port::SoundId> {
        self.local.get(usize::from(index)).copied().flatten()
    }

    /// When a play on channel `index` starting now is first heard, by the
    /// receiver's clock: now, or after the channel's current sound when
    /// the play waits for it, at its end where the receiver knows it;
    /// `None` where it does not (a looped or a live sound plays on).
    fn start_ms(&self, index: u8, play: Play) -> Option<u64> {
        if !play.after_current {
            return Some(self.now_ms);
        }
        match self.heard.get(usize::from(index)).copied().unwrap_or_default() {
            Heard::Idle => Some(self.now_ms),
            Heard::Until(end_ms) => Some(end_ms.max(self.now_ms)),
            Heard::Unknown => None,
        }
    }

    /// Records a play just started on channel `index`: it is what the channel
    /// plays, followed for loss, and heard to its end at a time the receiver
    /// knows for a stored sound played once from a known start.
    fn played(
        &mut self,
        index: u8,
        play: Play,
        entry: &Entry,
        start_ms: Option<u64>,
        music_percent: u8,
        sink: &mut dyn VoiceSink,
    ) {
        if let Some(channel) = self.channels.get_mut(usize::from(index)) {
            channel.waiting = None;
            channel.playing_page = play.page;
            channel.music_percent = music_percent;
            let was = channel.playing.replace(play);
            music_changed(index, was, Some(play), sink);
        }
        self.set_progress(index, Progress::follow(entry, play.after_current, self.now_ms));
        let heard = match start_ms {
            Some(start_ms) if !play.looped && !entry.live => {
                Heard::Until(start_ms.saturating_add(u64::from(duration_ms(entry))))
            }
            _ => Heard::Unknown,
        };
        if let Some(slot) = self.heard.get_mut(usize::from(index)) {
            *slot = heard;
        }
    }

    /// Channel `index` was stopped, or is to start another sound, with a fade
    /// of `fade_ms`: what it played is heard no longer, and its synthesizer
    /// streams end with the fade (INT-26, INT-36).
    #[cfg_attr(
        not(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        )),
        expect(unused_variables, reason = "only the lanes fade a stream out")
    )]
    pub(super) fn stop_streams(&mut self, index: u8, fade_ms: u16, sink: &mut dyn VoiceSink) {
        if let Some(heard) = self.heard.get_mut(usize::from(index)) {
            *heard = Heard::Idle;
        }
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        self.instruments.stop(index, fade_ms, self.now_ms, sink);
    }

    pub(super) fn start_waiting(&mut self, sink: &mut dyn VoiceSink) {
        for index in 0..CHANNELS {
            if let Some(play) = self.channels.get(usize::from(index)).and_then(|channel| channel.waiting) {
                // Magazine music waits only while its gate lets it sound;
                // `try_start` gives it up otherwise (REQ-6925 item 3).
                if play.magazine.is_some() || play.page.is_none() || (self.page_sounds && play.page == self.opened) {
                    self.try_start(index, play, sink);
                } else {
                    self.drop_waiting_page_play(index);
                }
            }
        }
    }

    pub(super) fn start_armed(&mut self, sink: &mut dyn VoiceSink) {
        let Some(opened) = self.opened else { return };
        for index in 0..CHANNELS.min(sink.channels()) {
            if let Some(play) = self.channels.get(usize::from(index)).and_then(|channel| channel.armed)
                && play.page == Some(opened)
            {
                self.try_start(index, play, sink);
            }
        }
    }

    /// Whether every bank `entry`'s sound names is held whole (MIDI-52,
    /// MIDI-62): a bank for the sound's family, every piece of it arrived
    /// and none in conflict. A sound that names none waits for none.
    fn bank_ready(&self, entry: &Entry) -> bool {
        entry.banks().iter().all(|&handle| {
            self.store.entry(handle).is_some_and(|bank| bank.setting == entry.codec.code())
                && self.store.image(handle).is_some()
        })
    }

    /// Milliseconds of sound received contiguously from the start.
    fn buffered_ms(&self, entry: &Entry) -> u32 {
        let blocks = self.store.contiguous(entry.sound);
        let samples = (0..blocks).map(|index| entry.block_samples(index)).fold(0_u32, u32::saturating_add);
        samples.saturating_mul(MS_PER_SECOND).checked_div(entry.rate_hz).unwrap_or(0)
    }
}

/// The sink's start of `play`, magazine music at `music_percent` of its
/// gain by its page's setting.
fn start_of(play: Play, music_percent: u8) -> Start {
    Start {
        sound: play.sound,
        gain: play.gain,
        fade_ms: play.fade_ms,
        looped: play.looped,
        duck_percent: play.duck_percent,
        after_current: play.after_current,
        magazine: play.magazine,
        music_percent,
    }
}

/// When a channel's latest play is heard to its end (INT-36), for a play
/// that waits for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum Heard {
    /// The channel plays nothing: a play after it starts at once.
    #[default]
    Idle,
    /// Its sound ends then, in the receiver's milliseconds.
    Until(u64),
    /// Its sound plays on for a time the receiver does not know: looped, or
    /// live, or itself waiting for one that does.
    Unknown,
}
