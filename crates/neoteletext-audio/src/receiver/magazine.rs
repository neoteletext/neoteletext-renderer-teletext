//! Magazine music (REQ-6925, REQ-6926; DEC-6924 items 1, 2 and 4): a live
//! sound armed for a magazine plays while any page of the magazine is open,
//! across its pages, and each page and sub-page sets it to play, duck or
//! silent. It is gated here and nowhere else: it sounds only while the
//! connection agreed the capability, the user's page sounds are on, the
//! open page is of the magazine and that page's setting is not silent.

use super::{Receiver, bump};
use crate::control::Play;
use crate::port::{SoundId, VoiceSink};
use crate::record::{MusicSetting, PageMusic, magazine_of_page};
use crate::{CHANNELS, DUCK_PERCENT, MIN_FADE_MS, MOST_PAGE_MUSIC_SETTINGS};

/// The level of magazine music on a page that lets it play, in per cent:
/// duck code 0, no attenuation.
pub(super) const UNDUCKED_PERCENT: u8 = 100;
const _: () = assert!(DUCK_PERCENT[0] == UNDUCKED_PERCENT);

/// The magazine music of an audio channel (REQ-6925 item 6, REQ-6926
/// item 6): what a host needs to label the channel's sound and to show
/// where music is heard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MagazineMusic {
    /// The magazine the channel's sound is armed for, 1 to 8.
    pub magazine: u8,
    /// Whether it is heard on the page now open: its voice plays.
    pub heard: bool,
}

/// One page's setting as the receiver keeps it, with when it was set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Kept {
    music: PageMusic,
    /// The count of settings taken when this one was: the lowest is the
    /// oldest.
    set_at: u32,
}

/// The settings a receiver keeps (DEC-6952 item 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Settings {
    kept: [Option<Kept>; MOST_PAGE_MUSIC_SETTINGS],
    taken: u32,
}

impl Settings {
    pub(super) const fn new() -> Self {
        Self { kept: [None; MOST_PAGE_MUSIC_SETTINGS], taken: 0 }
    }

    /// The setting of sub-page `sub` of `page`: play unless one is kept.
    fn of(&self, page: u16, sub: u16) -> MusicSetting {
        self.kept
            .iter()
            .flatten()
            .find(|kept| kept.music.page == page && kept.music.sub == sub)
            .map_or(MusicSetting::Play, |kept| kept.music.setting)
    }

    /// Takes a page's setting. Play, the default, is kept by keeping
    /// nothing; when every place is taken the setting set longest ago gives
    /// way, but never that of `open`, the page and sub-code now open.
    fn set(&mut self, music: PageMusic, open: Option<(u16, u16)>) {
        let same = |kept: &Kept| kept.music.page == music.page && kept.music.sub == music.sub;
        let held = self.kept.iter().position(|slot| slot.as_ref().is_some_and(same));
        self.taken = self.taken.saturating_add(1);
        let place = if music.setting == MusicSetting::Play {
            // Nothing is kept of a page that plays.
            let Some(held) = held else { return };
            if let Some(slot) = self.kept.get_mut(held) {
                *slot = None;
            }
            return;
        } else {
            held.or_else(|| self.kept.iter().position(Option::is_none)).or_else(|| {
                self.kept
                    .iter()
                    .enumerate()
                    .filter_map(|(at, slot)| slot.map(|kept| (at, kept)))
                    .filter(|(_, kept)| Some((kept.music.page, kept.music.sub)) != open)
                    .min_by_key(|(_, kept)| kept.set_at)
                    .map(|(at, _)| at)
            })
        };
        if let Some(slot) = place.and_then(|place| self.kept.get_mut(place)) {
            *slot = Some(Kept { music, set_at: self.taken });
        }
    }
}

impl<const OBJECTS: usize, const SLOTS: usize> Receiver<'_, OBJECTS, SLOTS> {
    /// Whether the connection agreed the capability of magazine music
    /// (REQ-6927 item 3, DEC-6953 item 5): the push protocol's bit 26. A
    /// receiver starts with it not agreed, and until its port says it was,
    /// a CONTROL record naming a magazine and a `MAGAZINE_MUSIC` record are
    /// refused and counted ([`super::Statistics::magazine_refused`]), so a
    /// broadcast, a file or an older connection never starts magazine music
    /// (REQ-6938 item 2). Saying it is no longer agreed, as when the
    /// connection ends, stops and disarms every magazine's music and
    /// forgets every page's setting.
    pub fn set_magazine_sound(&mut self, agreed: bool, sink: &mut dyn VoiceSink) {
        if agreed == self.magazine_sound {
            return;
        }
        self.magazine_sound = agreed;
        if !agreed {
            for channel in &mut self.channels {
                if channel.armed.is_some_and(|play| play.magazine.is_some()) {
                    channel.armed = None;
                }
            }
            self.settings = Settings::new();
        }
        self.follow_magazines(sink);
        self.render(sink);
    }

    /// The magazine music of audio channel `channel` (REQ-6925 item 6,
    /// REQ-6926 item 6): the magazine its sound is armed for, and whether
    /// it is heard on the page now open. `None` where the channel has no
    /// magazine's sound armed or playing, or is past the last.
    #[must_use]
    pub fn magazine_music(&self, channel: u8) -> Option<MagazineMusic> {
        let channel = self.channels.get(usize::from(channel))?;
        let playing = channel.playing.and_then(|play| play.magazine);
        let magazine = playing.or(channel.armed.and_then(|play| play.magazine))?;
        Some(MagazineMusic { magazine, heard: playing.is_some() })
    }

    /// The level at which the magazine music of `play` sounds now, in per
    /// cent of its gain, or `None` when it must be silent (REQ-6925 items 3
    /// and 5, REQ-6926 item 1): the one gate of magazine music.
    pub(super) fn music_percent(&self, play: Play) -> Option<u8> {
        if !self.magazine_sound || !self.page_sounds {
            return None;
        }
        let page = self.opened?;
        if play.magazine.is_none() || magazine_of_page(page) != play.magazine {
            return None;
        }
        match self.settings.of(page, self.opened_sub) {
            MusicSetting::Play => Some(UNDUCKED_PERCENT),
            MusicSetting::Duck(code) => DUCK_PERCENT.get(usize::from(code)).copied(),
            MusicSetting::Silent => None,
        }
    }

    /// Brings every channel's magazine music to what the gate says now:
    /// after the open page, its setting, the user's page sounds or the
    /// channel's command changed. Music that may sound and does not is
    /// started, at the live sound's present block; music that sounds and
    /// may not is faded out; music that plays on has its level moved to the
    /// page's. Moving between two pages that let it play changes nothing,
    /// so it plays on without a break (REQ-6925 item 3).
    pub(super) fn follow_magazines(&mut self, sink: &mut dyn VoiceSink) {
        for index in 0..CHANNELS.min(sink.channels()) {
            let Some(channel) = self.channels.get(usize::from(index)).copied() else { continue };
            let playing = channel.playing.is_some_and(|play| play.magazine.is_some());
            let armed = channel.armed.filter(|play| play.magazine.is_some());
            match armed.and_then(|play| Some((play, self.music_percent(play)?))) {
                Some((play, percent)) if playing && channel.playing == Some(play) => {
                    if channel.music_percent != percent {
                        if let Some(channel) = self.channels.get_mut(usize::from(index)) {
                            channel.music_percent = percent;
                        }
                        sink.music_level(index, percent, MIN_FADE_MS);
                    }
                }
                Some((play, _)) => self.try_start(index, play, sink),
                None if playing => self.stop_music(index, sink),
                None => {
                    if let Some(channel) = self.channels.get_mut(usize::from(index))
                        && channel.waiting.is_some_and(|play| play.magazine.is_some())
                    {
                        channel.waiting = None;
                    }
                }
            }
        }
    }

    /// Fades out the magazine music channel `index` plays, over the ramp of
    /// DEC-610 item 3. Its play stays armed, and the live sound's blocks
    /// are taken as they come, so that it starts again at the sound's
    /// present block.
    fn stop_music(&mut self, index: u8, sink: &mut dyn VoiceSink) {
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        let was = channel.playing.take();
        channel.waiting = None;
        sink.stop(index, MIN_FADE_MS);
        self.stop_streams(index, MIN_FADE_MS, sink);
        self.end_progress(index);
        music_changed(index, was, None, sink);
    }

    /// Takes a page's setting (REQ-6926 item 1) and applies it if that page
    /// is open.
    pub(super) fn set_page_music(&mut self, music: PageMusic, sink: &mut dyn VoiceSink) {
        // Refused whole where the capability was not agreed (REQ-6927
        // item 3, DEC-6953 item 5).
        if !self.magazine_sound {
            bump(&mut self.statistics.magazine_refused);
            return;
        }
        let open = self.opened.map(|page| (page, self.opened_sub));
        self.settings.set(music, open);
        self.follow_magazines(sink);
    }

    /// The level at which `play` starts on channel `index`: the full level
    /// for a sound that is no magazine's, the level of its page's setting
    /// for magazine music, and `None` for magazine music that must not
    /// start, which is then no longer waiting: a stored sound, or one the
    /// gate holds silent (REQ-6925 items 2 and 3).
    pub(super) fn starting_percent(&mut self, index: u8, play: Play) -> Option<u8> {
        if play.magazine.is_none() {
            return Some(UNDUCKED_PERCENT);
        }
        if self.refuse_stored_music(index, play) {
            return None;
        }
        let percent = self.music_percent(play);
        if percent.is_none()
            && let Some(channel) = self.channels.get_mut(usize::from(index))
        {
            channel.waiting = None;
        }
        percent
    }

    /// Refuses a magazine play of a stored sound (REQ-6925 item 2):
    /// magazine music is a live sound, at the same point for every view.
    /// Returns whether `play` was refused; it is then disarmed and counted.
    pub(super) fn refuse_stored_music(&mut self, index: u8, play: Play) -> bool {
        let stored = play.magazine.is_some()
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

    /// Labels the stream of `sound` on channel `index` as `magazine`'s
    /// music, or as none's, when the sink holds another label for it
    /// (REQ-6925 item 6).
    pub(super) fn label(&mut self, index: u8, sound: SoundId, magazine: Option<u8>, sink: &mut dyn VoiceSink) {
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        let wanted = magazine.map(|magazine| (sound, magazine));
        if channel.labelled == wanted {
            return;
        }
        // A stream no label was given for is no magazine's already.
        if wanted.is_some() || channel.labelled.is_some_and(|(labelled, _)| labelled == sound) {
            sink.stream_magazine(index, sound, magazine);
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

/// Tells the sink when what channel `index` plays stops or starts being a
/// magazine's music (REQ-6926 item 6): `was` is the play it played and
/// `now` the one it plays.
pub(super) fn music_changed(index: u8, was: Option<Play>, now: Option<Play>, sink: &mut dyn VoiceSink) {
    let (old, new) = (was.and_then(|play| play.magazine), now.and_then(|play| play.magazine));
    if old == new {
        return;
    }
    if let Some(magazine) = old {
        sink.magazine_heard(index, magazine, false);
    }
    if let Some(magazine) = new {
        sink.magazine_heard(index, magazine, true);
    }
}
