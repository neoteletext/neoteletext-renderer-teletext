//! Music (REQ-6925, REQ-6926; DEC-6924 as the owner ruled on 20 November
//! 2026): three levels of sound, of which the most specific that may sound
//! is heard alone. The channel's music is a live sound armed for the whole
//! channel; a magazine's music is one armed for a magazine; a page's sounds
//! are the page-bound sounds of REQ-AUD-012. A page's sound is heard in
//! place of its magazine's music, and a magazine's music in place of the
//! channel's; when the more specific level ends, the outer one starts
//! again at its live sound's present block. Each page and sub-page has two
//! switches, magazine music and channel music, both on unless the sender
//! says otherwise.
//!
//! Music is gated here and nowhere else ([`Receiver::music_wanted`]): it
//! sounds only while the connection agreed the capability, the user's page
//! sounds are on, a page it belongs to is open, that page's switch for its
//! level is on, and no more specific level sounds.

use super::play::Heard;
use super::{Receiver, bump};
use crate::control::{Channel, Play};
use crate::port::{Music, SoundId, VoiceSink};
use crate::record::{PageMusic, magazine_of_page};
use crate::{CHANNELS, MIN_FADE_MS, MOST_PAGE_MUSIC_SETTINGS};

/// The music of an audio channel (REQ-6925 item 6, REQ-6926 item 6): what a
/// host needs to label the channel's sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArmedMusic {
    /// The level the channel's sound is armed for.
    pub music: Music,
    /// Whether it is heard on the page now open: its voice plays.
    pub heard: bool,
}

/// The level of sound heard on the page now open (REQ-6926 item 6): the
/// most specific that sounds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sounding {
    /// No level sounds.
    Nothing,
    /// The channel's music.
    Channel,
    /// The music of this magazine, 1 to 8.
    Magazine(u8),
    /// A sound of the open page.
    Page,
}

/// One page's switches as the receiver keeps them, with when they were set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Kept {
    music: PageMusic,
    /// The count of records taken when this one was: the lowest is the
    /// oldest.
    set_at: u32,
}

/// The switches a receiver keeps (DEC-6952 item 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Settings {
    kept: [Option<Kept>; MOST_PAGE_MUSIC_SETTINGS],
    taken: u32,
}

impl Settings {
    pub(super) const fn new() -> Self {
        Self { kept: [None; MOST_PAGE_MUSIC_SETTINGS], taken: 0 }
    }

    /// Whether sub-page `sub` of `page` lets `music` sound: yes unless a
    /// switch kept for it is off.
    fn lets(&self, page: u16, sub: u16, music: Music) -> bool {
        self.kept
            .iter()
            .flatten()
            .find(|kept| kept.music.page == page && kept.music.sub == sub)
            .is_none_or(|kept| kept.music.lets(music))
    }

    /// Takes a page's switches. Both on, the default, is kept by keeping
    /// nothing; when every place is taken the page set longest ago gives
    /// way, but never `open`, the page and sub-code now open.
    fn set(&mut self, music: PageMusic, open: Option<(u16, u16)>) {
        let same = |kept: &Kept| kept.music.page == music.page && kept.music.sub == music.sub;
        let held = self.kept.iter().position(|slot| slot.as_ref().is_some_and(same));
        self.taken = self.taken.saturating_add(1);
        if music.magazine_music && music.channel_music {
            // Nothing is kept of a page with both switches on.
            if let Some(slot) = held.and_then(|held| self.kept.get_mut(held)) {
                *slot = None;
            }
            return;
        }
        let place = held.or_else(|| self.kept.iter().position(Option::is_none)).or_else(|| {
            self.kept
                .iter()
                .enumerate()
                .filter_map(|(at, slot)| slot.map(|kept| (at, kept)))
                .filter(|(_, kept)| Some((kept.music.page, kept.music.sub)) != open)
                .min_by_key(|(_, kept)| kept.set_at)
                .map(|(at, _)| at)
        });
        if let Some(slot) = place.and_then(|place| self.kept.get_mut(place)) {
            *slot = Some(Kept { music, set_at: self.taken });
        }
    }
}

/// Whether `channel` plays music of a magazine.
fn plays_magazine(channel: &Channel) -> bool {
    channel.playing.is_some_and(|play| matches!(play.music, Some(Music::Magazine(_))))
}

impl<const OBJECTS: usize, const SLOTS: usize> Receiver<'_, OBJECTS, SLOTS> {
    /// Whether the connection agreed the capability of music (REQ-6927
    /// item 3, DEC-6953 item 5): the push protocol's bit 26. A receiver
    /// starts with it not agreed, and until its port says it was, a CONTROL
    /// record naming a level of music and a `PAGE_MUSIC` record are refused
    /// and counted ([`super::Statistics::magazine_refused`]), so a
    /// broadcast, a file or an older connection never starts music
    /// (REQ-6938 item 2). Saying it is no longer agreed, as when the
    /// connection ends, stops and disarms every level's music and forgets
    /// every page's switches.
    pub fn set_magazine_sound(&mut self, agreed: bool, sink: &mut dyn VoiceSink) {
        if agreed == self.magazine_sound {
            return;
        }
        self.magazine_sound = agreed;
        if !agreed {
            for channel in &mut self.channels {
                if channel.armed.is_some_and(|play| play.music.is_some()) {
                    channel.armed = None;
                }
            }
            self.settings = Settings::new();
        }
        self.follow_music(sink);
        self.render(sink);
    }

    /// The music of audio channel `channel` (REQ-6925 item 6): the level
    /// its sound is armed for, and whether it is heard on the page now
    /// open. `None` where the channel has no music armed or playing, or is
    /// past the last.
    #[must_use]
    pub fn music(&self, channel: u8) -> Option<ArmedMusic> {
        let channel = self.channels.get(usize::from(channel))?;
        let playing = channel.playing.and_then(|play| play.music);
        let music = playing.or(channel.armed.and_then(|play| play.music))?;
        Some(ArmedMusic { music, heard: playing.is_some() })
    }

    /// The level of sound heard on the page now open (REQ-6926 item 6): a
    /// page's sound if one sounds, else a magazine's music if it plays,
    /// else the channel's, else nothing. A sound played once for the whole
    /// service is of no level and is not counted.
    #[must_use]
    pub fn sounding(&self) -> Sounding {
        if self.page_sounding() {
            return Sounding::Page;
        }
        let playing = |wanted: fn(Music) -> bool| {
            self.channels.iter().filter_map(|channel| channel.playing?.music).find(|&music| wanted(music))
        };
        match (playing(|music| matches!(music, Music::Magazine(_))), playing(|music| music == Music::Channel)) {
            (Some(Music::Magazine(magazine)), _) => Sounding::Magazine(magazine),
            (_, Some(_)) => Sounding::Channel,
            _ => Sounding::Nothing,
        }
    }

    /// Whether a sound of the open page sounds now: a channel plays a
    /// page's play that has not been heard to its end by the receiver's
    /// clock. A looped, a live or a local sound sounds until it is stopped
    /// or its page is left.
    fn page_sounding(&self) -> bool {
        self.channels.iter().zip(&self.heard).any(|(channel, heard)| {
            channel.playing_page.is_some()
                && match heard {
                    Heard::Idle => false,
                    Heard::Until(end_ms) => self.now_ms < *end_ms,
                    Heard::Unknown => true,
                }
        })
    }

    /// When a page's sound that holds music silent ends, by the receiver's
    /// clock, for [`Receiver::next_tick_ms`]: the music starts again then.
    /// `None` when no music is armed or no such end is known.
    pub(super) fn music_due_ms(&self) -> Option<u64> {
        if !self.channels.iter().any(|channel| channel.armed.is_some_and(|play| play.music.is_some())) {
            return None;
        }
        self.channels
            .iter()
            .zip(&self.heard)
            .filter(|(channel, _)| channel.playing_page.is_some())
            .filter_map(|(_, heard)| if let Heard::Until(end_ms) = heard { Some(*end_ms) } else { None })
            .filter(|&end_ms| end_ms > self.now_ms)
            .min()
    }

    /// Whether the music of `play` sounds now (REQ-6925 items 3 and 5,
    /// REQ-6926 items 1 and 4): the one gate of music. The capability was
    /// agreed, the user's page sounds are on, a page the music belongs to
    /// is open, that page's switch for its level is on, and no more
    /// specific level sounds.
    pub(super) fn music_wanted(&self, play: Play) -> bool {
        let (Some(music), Some(page)) = (play.music, self.opened) else { return false };
        if !self.magazine_sound || !self.page_sounds || !self.settings.lets(page, self.opened_sub, music) {
            return false;
        }
        if self.page_sounding() {
            return false;
        }
        match music {
            Music::Magazine(magazine) => magazine_of_page(page) == Some(magazine),
            Music::Channel => !self.channels.iter().any(plays_magazine),
        }
    }

    /// Brings every channel's music to what the gate says now: after the
    /// open page, its switches, the user's page sounds, a channel's command
    /// or a page's sound changed. Magazines' music is settled before the
    /// channel's, since it is the more specific. Music that may sound and
    /// does not is started, at the live sound's present block; music that
    /// sounds and may not is faded out as the level that replaces it fades
    /// in. Moving between two pages that let it sound changes nothing, so
    /// it plays on without a break (REQ-6925 item 3).
    pub(super) fn follow_music(&mut self, sink: &mut dyn VoiceSink) {
        for magazines in [true, false] {
            for index in 0..CHANNELS.min(sink.channels()) {
                let Some(channel) = self.channels.get(usize::from(index)).copied() else { continue };
                let of_level =
                    |play: &Play| play.music.is_some_and(|music| matches!(music, Music::Magazine(_)) == magazines);
                let playing = channel.playing.filter(of_level);
                let armed = channel.armed.filter(of_level);
                match armed.filter(|&play| self.music_wanted(play)) {
                    Some(play) if playing == Some(play) => {}
                    Some(play) => self.try_start(index, play, sink),
                    None if playing.is_some() => self.stop_music(index, sink),
                    None => {
                        if let Some(channel) = self.channels.get_mut(usize::from(index))
                            && channel.waiting.is_some_and(|play| of_level(&play))
                        {
                            channel.waiting = None;
                        }
                    }
                }
            }
        }
    }

    /// Fades out the music channel `index` plays, over the ramp of DEC-610
    /// item 3. Its play stays armed, and the live sound's blocks are taken
    /// as they come, so that it starts again at the sound's present block.
    fn stop_music(&mut self, index: u8, sink: &mut dyn VoiceSink) {
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        let was = channel.playing.take();
        channel.waiting = None;
        sink.stop(index, MIN_FADE_MS);
        self.stop_streams(index, MIN_FADE_MS, sink);
        self.end_progress(index);
        music_changed(index, was, None, sink);
    }

    /// Takes a page's switches (REQ-6926 item 1); they apply at once if
    /// that page is open.
    pub(super) fn set_page_music(&mut self, music: PageMusic) {
        // Refused whole where the capability was not agreed (REQ-6927
        // item 3, DEC-6953 item 5).
        if !self.magazine_sound {
            bump(&mut self.statistics.magazine_refused);
            return;
        }
        let open = self.opened.map(|page| (page, self.opened_sub));
        self.settings.set(music, open);
    }

    /// Whether `play` may start on channel `index` now. A play that is no
    /// music may; music may where its gate lets it sound, and otherwise is
    /// no longer waiting: a stored sound, which is refused, or one held
    /// silent (REQ-6925 items 2 and 3).
    pub(super) fn may_start(&mut self, index: u8, play: Play) -> bool {
        if play.music.is_none() {
            return true;
        }
        if self.refuse_stored_music(index, play) {
            return false;
        }
        let wanted = self.music_wanted(play);
        if !wanted && let Some(channel) = self.channels.get_mut(usize::from(index)) {
            channel.waiting = None;
        }
        wanted
    }

    /// Refuses a play of a stored sound as music (REQ-6925 item 2): music
    /// is a live sound, at the same point for every view. Returns whether
    /// `play` was refused; it is then disarmed and counted.
    pub(super) fn refuse_stored_music(&mut self, index: u8, play: Play) -> bool {
        let stored = play.music.is_some()
            && self.store.entry(play.sound.handle).is_some_and(|entry| entry.sound == play.sound && !entry.live);
        if stored {
            if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                if channel.armed == Some(play) {
                    channel.armed = None;
                }
                if channel.waiting == Some(play) {
                    channel.waiting = None;
                }
            }
            bump(&mut self.statistics.magazine_refused);
        }
        stored
    }

    /// Labels the stream of `sound` on channel `index` as `music`, or as
    /// none, when the sink holds another label for it (REQ-6925 item 6).
    pub(super) fn label(&mut self, index: u8, sound: SoundId, music: Option<Music>, sink: &mut dyn VoiceSink) {
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        let wanted = music.map(|music| (sound, music));
        if channel.labelled == wanted {
            return;
        }
        // A stream no label was given for is no music already.
        if wanted.is_some() || channel.labelled.is_some_and(|(labelled, _)| labelled == sound) {
            sink.stream_music(index, sound, music);
        }
        channel.labelled = wanted;
    }

    /// The sink holds no stream of `sound` any more, so none is labelled.
    pub(super) fn unlabel(&mut self, sound: SoundId) {
        for channel in &mut self.channels {
            if channel.labelled.is_some_and(|(labelled, _)| labelled == sound) {
                channel.labelled = None;
            }
        }
    }
}

/// Tells the sink when what channel `index` plays stops or starts being
/// music of a level (REQ-6926 item 6): `was` is the play it played and
/// `now` the one it plays.
pub(super) fn music_changed(index: u8, was: Option<Play>, now: Option<Play>, sink: &mut dyn VoiceSink) {
    let (old, new) = (was.and_then(|play| play.music), now.and_then(|play| play.music));
    if old == new {
        return;
    }
    if let Some(music) = old {
        sink.music_heard(index, music, false);
    }
    if let Some(music) = new {
        sink.music_heard(index, music, true);
    }
}
