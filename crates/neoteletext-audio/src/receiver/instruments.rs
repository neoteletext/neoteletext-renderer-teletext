//! Synthesizer sounds rendered as they play, into streams (DEC-6410 item 13;
//! midi-architecture.md, MIDI-3 to MIDI-5, MIDI-100 to MIDI-106;
//! integration.md, INT-32 to INT-37). A block of an event codec is 20 ms of
//! the synthesizer's input, an event list (MIDI-23), and what it sounds like
//! depends on every block before it, as an Opus frame depends on the frames
//! before it. So the receiver keeps [`crate::INSTRUMENT_LANES`] lanes, each
//! rendering one playing of a sound with an instance new at its start
//! (MIDI-3): a playing is a stream the sink holds under its audio channel
//! and its sound (INT-37), so that one sound on two audio channels is two
//! playings, each from its own start.
//!
//! A stored sound is never rendered whole nor defined to the sink (INT-33):
//! its playing is rendered block by block, a lead of
//! [`crate::INSTRUMENT_LEAD_MIN_BLOCKS`] to
//! [`crate::INSTRUMENT_LEAD_MAX_BLOCKS`] blocks ahead of what is heard by the
//! receiver's clock (INT-34), on across a loop in the same stream (MIDI-105),
//! and its stream ended after its last block (INT-36). A live sound's
//! playing is rendered as its blocks arrive, and plays on through a block
//! that never arrived (MIDI-102).
//!
//! Every family is played through one trait,
//! [`neoteletext_audio_instruments::midi::Instrument`], with the bank
//! images of its sound read from the store each call ([`banks`]); the
//! event list is read by the one reader every family and the server share
//! ([`neoteletext_audio_instruments::midi::play_block`]).
//!
//! Rendering is work the sound's sender chose, so a call renders at most
//! [`RENDER_BUDGET_BLOCKS`] blocks of all the lanes together and asks to be
//! called again at once while more are owed ([`super::Receiver::next_tick_ms`];
//! MIDI-106, INT-35).
//!
//! With the feature `watch`, a lane whose audio channel a port has asked to
//! watch ([`super::Receiver::watch`]) also keeps what a page's rules read of
//! the sound it renders (page-rules.md RULE-30): the family's tracker and
//! the performance state, fed every event of every block the lane renders,
//! so that a value is read at the position rendered. A lane that is not
//! watched keeps none and does no such work.

use neoteletext_audio_instruments::midi::{Banks, Instrument, Meter, Outcome, Played, play_block};
#[cfg(feature = "synth7")]
use neoteletext_audio_instruments::synth7;
#[cfg(feature = "synth8")]
use neoteletext_audio_instruments::synth8;
#[cfg(feature = "synth53")]
use neoteletext_audio_instruments::synth53;
#[cfg(feature = "synth80")]
use neoteletext_audio_instruments::synth80;
#[cfg(feature = "synth106")]
use neoteletext_audio_instruments::synth106;
#[cfg(feature = "synth6581")]
use neoteletext_audio_instruments::synth6581;
#[cfg(feature = "synthd")]
use neoteletext_audio_instruments::synthd;
#[cfg(feature = "watch")]
use neoteletext_audio_instruments::watch::Watched;

use super::Statistics;
use crate::Codec;
use crate::port::{SoundId, VoiceSink};
use crate::store::{Entry, Store};

/// The samples of one block, as a buffer's length.
const BLOCK: usize = crate::INSTRUMENT_BLOCK_SAMPLES as usize;

/// The most blocks of all the lanes one call renders: one second of sound
/// (MIDI-106, INT-35). A larger budget catches up sooner after a slow port
/// and holds the port longer.
pub(super) const RENDER_BUDGET_BLOCKS: u16 = 50;

/// The synthesizer a lane holds: at most one, so a lane is as large as the
/// largest built in.
#[allow(
    clippy::large_enum_variant,
    reason = "a lane holds its synthesizer in place: there is no allocator to box it with, and every lane must have room for the largest"
)]
enum Synthesizer {
    /// None yet, or the sound's family is not built in.
    None,
    /// The synth6581 family.
    #[cfg(feature = "synth6581")]
    Synth6581(synth6581::midi::Midi),
    /// synth7.
    #[cfg(feature = "synth7")]
    Synth7(synth7::midi::Midi),
    /// The synth106 family.
    #[cfg(feature = "synth106")]
    Synth106(synth106::midi::Midi),
    /// synthD.
    #[cfg(feature = "synthd")]
    SynthD(synthd::midi::Midi),
    /// The synth53 family.
    #[cfg(feature = "synth53")]
    Synth53(synth53::midi::Midi),
    /// synth8.
    #[cfg(feature = "synth8")]
    Synth8(synth8::midi::Midi),
    /// synth80.
    #[cfg(feature = "synth80")]
    Synth80(synth80::midi::Midi),
}

/// Does `$then` with the [`Instrument`] `$value` holds, whichever it is,
/// bound to `$instrument`; or `$otherwise` when it holds none.
macro_rules! with_instrument {
    ($value:expr, $instrument:ident => $then:expr, $otherwise:expr) => {
        match $value {
            #[cfg(feature = "synth6581")]
            Synthesizer::Synth6581($instrument) => $then,
            #[cfg(feature = "synth7")]
            Synthesizer::Synth7($instrument) => $then,
            #[cfg(feature = "synth106")]
            Synthesizer::Synth106($instrument) => $then,
            #[cfg(feature = "synthd")]
            Synthesizer::SynthD($instrument) => $then,
            #[cfg(feature = "synth53")]
            Synthesizer::Synth53($instrument) => $then,
            #[cfg(feature = "synth8")]
            Synthesizer::Synth8($instrument) => $then,
            #[cfg(feature = "synth80")]
            Synthesizer::Synth80($instrument) => $then,
            Synthesizer::None => $otherwise,
        }
    };
}

impl Synthesizer {
    /// The synthesizer `entry`'s sound names, new (MIDI-3), as its OBJECT
    /// chose it: the chip and clock, the variant, or the keyboard.
    fn of(entry: &Entry) -> Self {
        match entry.codec {
            #[cfg(feature = "synth6581")]
            Codec::Synth6581 => {
                let model = if entry.setting & crate::record::SYNTH6581_CHIP_8580 == 0 {
                    synth6581::Variant::Synth6581
                } else {
                    synth6581::Variant::Synth8580
                };
                let clock = if entry.setting & crate::record::SYNTH6581_CLOCK_NTSC == 0 {
                    synth6581::ChipClock::Pal
                } else {
                    synth6581::ChipClock::Ntsc
                };
                Self::Synth6581(synth6581::midi::Midi::new(model, clock))
            }
            #[cfg(feature = "synth7")]
            Codec::Synth7 => Self::Synth7(synth7::midi::Midi::new()),
            #[cfg(feature = "synth106")]
            Codec::Synth106 => {
                // `record::TAG_VARIANT`: 0 synth106, 1 synth60.
                let model = if entry.setting == 0 { synth106::Model::Synth106 } else { synth106::Model::Synth60 };
                Self::Synth106(synth106::midi::Midi::new(synth106::Synth106::new(model)))
            }
            #[cfg(feature = "synthd")]
            Codec::SynthD => Self::SynthD(synthd::midi::Midi::new(synthd::SynthD::new())),
            #[cfg(feature = "synth53")]
            Codec::Synth53 => {
                // `record::TAG_VARIANT`: 0 synth53, 1 synth52, 2 synth51; the
                // kind alone chooses the model (S53-1), checked when it was
                // read.
                let variant = synth53::interface::Variant::from_tag(entry.setting)
                    .unwrap_or(synth53::interface::Variant::Synth53);
                Self::Synth53(synth53::midi::Midi::new(synth53::Synth53::new(synth53::Model::of(variant))))
            }
            #[cfg(feature = "synth8")]
            Codec::Synth8 => {
                let mut synth = synth8::Synth8::new();
                // `record::TAG_SYNTH8_KEYS`, checked when it was read.
                let [mode, split] = entry.keys;
                synth.set_key_mode(match mode {
                    crate::record::SYNTH8_KEYS_DUAL => synth8::KeyMode::Dual,
                    crate::record::SYNTH8_KEYS_SPLIT => synth8::KeyMode::Split,
                    _ => synth8::KeyMode::Whole,
                });
                synth.set_split(split);
                Self::Synth8(synth8::midi::Midi::new(synth))
            }
            #[cfg(feature = "synth80")]
            Codec::Synth80 => Self::Synth80(synth80::midi::Midi::new(synth80::Synth80::new())),
            // No other codec has a synthesizer, and none of a family left
            // out of the build is stored (`Codec::from_code`).
            _ => Self::None,
        }
    }
}

/// One playing of an event sound: the stream the sink holds under its audio
/// channel and sound (INT-37), and how far it has been rendered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Stream {
    /// The audio channel that plays it.
    channel: u8,
    /// The sound.
    sound: SoundId,
    /// A live sound: rendered as its blocks arrive, by the sender's clock
    /// (MIDI-101). A stored one is rendered by the lead (INT-34).
    live: bool,
    /// Go on from the last block to block 0 (MIDI-105).
    looped: bool,
    /// The next block it renders.
    next: u16,
    /// The blocks given to the stream since it began, across loops.
    rendered: u32,
    /// When its first sample is heard, in the receiver's milliseconds; `None`
    /// while that cannot be known (a play after a sound whose end the
    /// receiver does not know), when the stream is rendered to the largest
    /// lead and held there.
    anchor_ms: Option<u64>,
    /// A stopped stream's fade ends then, and the stream with it (INT-36).
    until_ms: Option<u64>,
    /// A play after the current one of this same sound on this channel is
    /// queued: at the last block the stream goes on with a new instance from
    /// block 0 (MIDI-3), looped as this says, rather than ending.
    again: Option<bool>,
}

impl Stream {
    /// The blocks heard by `now_ms`, by the receiver's clock.
    fn played(&self, now_ms: u64) -> Option<u32> {
        let anchor = self.anchor_ms?;
        let blocks = now_ms.saturating_sub(anchor) / BLOCK_MS;
        Some(u32::try_from(blocks).unwrap_or(u32::MAX))
    }

    /// The blocks the stream should hold rendered by `now_ms`: the largest
    /// lead past what has been heard (INT-34).
    fn target(&self, now_ms: u64) -> u32 {
        self.played(now_ms).unwrap_or(0).saturating_add(crate::INSTRUMENT_LEAD_MAX_BLOCKS)
    }

    /// Whether the stream's lead has fallen below the least (INT-34): a
    /// block not yet arrived must then be played through as lost (INT-25).
    fn due(&self, now_ms: u64) -> bool {
        self.played(now_ms)
            .is_some_and(|played| self.rendered < played.saturating_add(crate::INSTRUMENT_LEAD_MIN_BLOCKS))
    }

    /// When the lead falls to the least, by the receiver's clock: the latest
    /// moment the receiver must render again (INT-34).
    fn refill_ms(&self) -> Option<u64> {
        let anchor = self.anchor_ms?;
        let blocks = self.rendered.saturating_sub(crate::INSTRUMENT_LEAD_MIN_BLOCKS);
        Some(anchor.saturating_add(u64::from(blocks).saturating_mul(BLOCK_MS)))
    }
}

/// The milliseconds of one block: 20 (REQ-6411 item 1).
const BLOCK_MS: u64 = crate::INSTRUMENT_BLOCK_SAMPLES as u64 * MS_PER_SECOND / crate::INSTRUMENT_RATE as u64;
/// Milliseconds in a second.
const MS_PER_SECOND: u64 = 1_000;
const _: () = assert!(BLOCK_MS == 20);
// INT-34: the largest lead stays below what a sink holds of a stream.
const _: () = {
    assert!(crate::INSTRUMENT_LEAD_MIN_BLOCKS < crate::INSTRUMENT_LEAD_MAX_BLOCKS);
    assert!((crate::INSTRUMENT_LEAD_MAX_BLOCKS as u64) * BLOCK_MS < crate::STREAM_HELD_MS);
};

/// One synthesizer and the playing it renders.
struct Lane {
    /// The playing, or `None` while the lane is free.
    stream: Option<Stream>,
    /// The synthesizer.
    synthesizer: Synthesizer,
    /// What a page's rules read of the playing (RULE-30), while its audio
    /// channel is watched; `None` otherwise, when nothing is tracked.
    #[cfg(feature = "watch")]
    watched: Option<Watched>,
}

/// The state a page's rules read of `entry`'s sound, new at its start with
/// its banks (RULE-30); `None` for a codec that is no family's.
#[cfg(feature = "watch")]
fn watched_of<const OBJECTS: usize>(store: &Store<'_, OBJECTS>, entry: &Entry) -> Option<Watched> {
    let variant = match entry.codec {
        // The chip is the variant (RULE-28): 0 synth6581, 1 synth8580.
        Codec::Synth6581 => u8::from(entry.setting & crate::record::SYNTH6581_CHIP_8580 != 0),
        // `record::TAG_VARIANT` is the setting of the families with one.
        codec if codec.variants() > 1 => entry.setting,
        _ => 0,
    };
    Watched::new(entry.codec.code(), variant, &banks(store, entry))
}

/// What [`Lanes::begin`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Begun {
    /// A new stream, with a new instance (MIDI-3): the sink is to start it.
    Started,
    /// The play follows the same sound on the same channel: the stream the
    /// sink plays goes on into it, and the sink is told nothing more.
    Continued,
    /// Every lane renders a playing that is heard: the play waits.
    NoLane,
}

/// The receiver's synthesizers.
pub(super) struct Lanes {
    lanes: [Lane; crate::INSTRUMENT_LANES],
    /// The audio channels a port asked to watch: bit `n` for channel `n`.
    #[cfg(feature = "watch")]
    watching: u8,
}

// Every audio channel has its bit.
#[cfg(feature = "watch")]
const _: () = assert!(crate::CHANNELS as u32 <= u8::BITS);

impl Lanes {
    pub(super) fn new() -> Self {
        Self {
            lanes: core::array::from_fn(|_| Lane {
                stream: None,
                synthesizer: Synthesizer::None,
                #[cfg(feature = "watch")]
                watched: None,
            }),
            #[cfg(feature = "watch")]
            watching: 0,
        }
    }

    /// Frees every lane: the sink has dropped every sound.
    pub(super) fn clear(&mut self) {
        for lane in &mut self.lanes {
            lane.free();
        }
    }

    /// Frees the lanes that render `sound`: the sink has been told to forget
    /// it, which ends its streams.
    pub(super) fn release(&mut self, sound: SoundId) {
        for lane in &mut self.lanes {
            if lane.stream.is_some_and(|stream| stream.sound == sound) {
                lane.free();
            }
        }
    }

    /// Whether audio channel `channel` is watched.
    #[cfg(feature = "watch")]
    pub(super) fn is_watched(&self, channel: u8) -> bool {
        1_u8.checked_shl(u32::from(channel)).is_some_and(|bit| self.watching & bit != 0)
    }

    /// Watches audio channel `channel`, or stops (RULE-30): while it is
    /// watched, each playing on it keeps what a page's rules read of its
    /// sound. A playing already under way is caught up: a stored sound's
    /// blocks rendered so far are read again, events only, from its start,
    /// so its values are those at the position rendered; a live sound's are
    /// followed from here on, since its earlier blocks are gone.
    #[cfg(feature = "watch")]
    pub(super) fn watch<const OBJECTS: usize>(&mut self, store: &Store<'_, OBJECTS>, channel: u8, on: bool) {
        let Some(bit) = 1_u8.checked_shl(u32::from(channel)).filter(|_| channel < crate::CHANNELS) else { return };
        self.watching = if on { self.watching | bit } else { self.watching & !bit };
        for lane in &mut self.lanes {
            let Some(stream) = lane.stream.filter(|stream| stream.channel == channel) else { continue };
            if !on {
                lane.watched = None;
            } else if lane.watched.is_none() {
                let entry = store.entries().find(|entry| entry.sound == stream.sound);
                lane.watched = entry.and_then(|entry| {
                    let mut watched = watched_of(store, entry)?;
                    if !stream.live {
                        let banks = banks(store, entry);
                        for index in 0..stream.next {
                            match store.block(entry.sound, index) {
                                Some(block) => watched.take_block(block, &banks),
                                // It was played through as lost (MIDI-102).
                                None => watched.release_all(),
                            }
                        }
                    }
                    Some(watched)
                });
            }
        }
    }

    /// The value of `class` for part `part` and `index` of the synthesizer
    /// sound whose playing is heard on audio channel `channel`, at the
    /// position rendered (RULE-28, RULE-30): the classes a sound's tracker
    /// holds, 0 to 4, 8 and 9. `None` where the channel is not watched or
    /// no synthesizer's playing is heard there. A playing that is fading
    /// out of a stop gives way to the one that replaced it, as for
    /// [`Lanes::meter`].
    #[cfg(feature = "watch")]
    pub(super) fn tracked<const OBJECTS: usize>(
        &self,
        store: &Store<'_, OBJECTS>,
        channel: u8,
        class: u8,
        part: u8,
        index: u16,
    ) -> Option<i32> {
        let playing = |lane: &&Lane| lane.stream.is_some_and(|stream| stream.channel == channel);
        let lanes = self.lanes.iter().filter(playing);
        let lane = lanes.min_by_key(|lane| lane.stream.is_some_and(|stream| stream.until_ms.is_some()))?;
        let watched = lane.watched.as_ref()?;
        let sound = lane.stream?.sound;
        let entry = store.entries().find(|entry| entry.sound == sound)?;
        Some(watched.read(class, part, index, &banks(store, entry)))
    }

    /// The envelope position of part `part` and the voice count of the
    /// synthesizer that renders the playing heard on audio channel
    /// `channel` (page-rules.md, RULE-30, RULE-31), at the position
    /// rendered; `None` where no synthesizer's playing is heard there. A
    /// playing that is fading out of a stop gives way to the one that
    /// replaced it.
    pub(super) fn meter(&self, channel: u8, part: u8) -> Option<Meter> {
        let playing = |lane: &&Lane| lane.stream.is_some_and(|stream| stream.channel == channel);
        let lanes = self.lanes.iter().filter(playing);
        let lane = lanes.min_by_key(|lane| lane.stream.is_some_and(|stream| stream.until_ms.is_some()))?;
        Some(with_instrument!(&lane.synthesizer, instrument => instrument.meter(part), Meter::IDLE))
    }

    /// The sounds the synthesizers render, whose banks the store must keep
    /// (MIDI-65).
    pub(super) fn following(&self) -> [Option<SoundId>; crate::INSTRUMENT_LANES] {
        core::array::from_fn(|index| self.lanes.get(index).and_then(|lane| lane.stream).map(|stream| stream.sound))
    }

    /// Channel `channel` was stopped, or its sound replaced, with a fade of
    /// `fade_ms`: what it plays goes on being rendered to the end of the fade
    /// and then ends, freeing its lane (INT-26, INT-36); what waits to follow
    /// it, which the sink drops, ends at once.
    pub(super) fn stop(&mut self, channel: u8, fade_ms: u16, now_ms: u64, sink: &mut dyn VoiceSink) {
        for lane in &mut self.lanes {
            let Some(stream) = lane.stream.as_mut().filter(|stream| stream.channel == channel) else { continue };
            if stream.until_ms.is_some() {
                continue;
            }
            if stream.anchor_ms.is_some_and(|anchor| anchor <= now_ms) {
                stream.until_ms = Some(now_ms.saturating_add(u64::from(fade_ms)));
                stream.again = None;
            } else {
                sink.end_stream(channel, stream.sound);
                lane.free();
            }
        }
    }

    /// Begins a playing of `entry`'s sound on `channel` (INT-32, INT-37): a
    /// new instance in a free lane, its stream announced to the sink; heard
    /// from `anchor_ms` (`None`: not known yet). A live sound is joined at
    /// its newest block (MIDI-101), a stored one starts at block 0.
    #[expect(
        clippy::too_many_arguments,
        reason = "a playing is the sound, its channel, when it is heard and how, and the receiver's parts it is rendered with"
    )]
    pub(super) fn begin<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        channel: u8,
        anchor_ms: Option<u64>,
        looped: bool,
        after_current: bool,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) -> Begun {
        let same = |stream: &Stream| stream.channel == channel && stream.sound == entry.sound;
        // One stream a name (INT-37): a play after the current one of the
        // same sound goes on in its stream; any other play of it ends it.
        if let Some(lane) = self.lanes.iter_mut().find(|lane| lane.stream.as_ref().is_some_and(same)) {
            if let Some(stream) = lane.stream.as_mut()
                && after_current
                && !entry.live
                && !stream.looped
                && stream.until_ms.is_none()
                && stream.again.is_none()
            {
                stream.again = Some(looped);
                return Begun::Continued;
            }
            sink.end_stream(channel, entry.sound);
            lane.free();
        }
        let free = self.lanes.iter().position(|lane| lane.stream.is_none());
        // A stream fading out of a stop gives way: the sink ramps what it
        // holds of it.
        let fading = || self.lanes.iter().position(|lane| lane.stream.is_some_and(|stream| stream.until_ms.is_some()));
        let Some(index) = free.or_else(fading) else { return Begun::NoLane };
        #[cfg(feature = "watch")]
        let watched = self.is_watched(channel);
        let Some(lane) = self.lanes.get_mut(index) else { return Begun::NoLane };
        if let Some(old) = lane.stream.take() {
            sink.end_stream(old.channel, old.sound);
        }
        // A playing on a watched channel is tracked from its first block.
        #[cfg(feature = "watch")]
        {
            lane.watched = if watched { watched_of(store, entry) } else { None };
        }
        lane.start(store, entry, statistics);
        lane.stream = Some(Stream {
            channel,
            sound: entry.sound,
            live: entry.live,
            looped,
            next: if entry.live { entry.newest.unwrap_or(0) } else { 0 },
            rendered: 0,
            anchor_ms,
            until_ms: None,
            again: None,
        });
        sink.stream(channel, entry.sound, crate::INSTRUMENT_RATE);
        Begun::Started
    }

    /// Renders every playing as far as it is owed, at most
    /// [`RENDER_BUDGET_BLOCKS`] blocks of all of them together (INT-35): a
    /// stored sound's to the lead (INT-34), a live sound's as its blocks
    /// have arrived (MIDI-101, MIDI-102). Ends the streams that are over.
    pub(super) fn render<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        now_ms: u64,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let mut budget = RENDER_BUDGET_BLOCKS;
        for lane in &mut self.lanes {
            let Some(stream) = lane.stream else { continue };
            let entry = store.entries().find(|entry| entry.sound == stream.sound).copied();
            let over = stream.until_ms.is_some_and(|until| now_ms >= until);
            let Some(entry) = entry.filter(|_| !over) else {
                sink.end_stream(stream.channel, stream.sound);
                lane.free();
                continue;
            };
            if stream.live {
                lane.render_live(store, &entry, &mut budget, statistics, sink);
            } else {
                lane.render_stored(store, &entry, now_ms, &mut budget, statistics, sink);
            }
        }
    }

    /// The latest moment the receiver must next render to keep every lead
    /// (INT-34) and end every faded stream (INT-36); `now_ms` while blocks
    /// are owed beyond the budget (INT-35); `None` while nothing is owed.
    pub(super) fn next_tick_ms<const OBJECTS: usize>(&self, store: &Store<'_, OBJECTS>, now_ms: u64) -> Option<u64> {
        self.lanes
            .iter()
            .filter_map(|lane| {
                let stream = lane.stream?;
                let Some(entry) = store.entries().find(|entry| entry.sound == stream.sound) else {
                    return Some(now_ms);
                };
                let render = if stream.live {
                    live_owed(store, entry, &stream).then_some(now_ms)
                } else if stream.anchor_ms.is_some() {
                    stream.refill_ms().map(|refill| refill.max(now_ms))
                } else {
                    // Held at the lead until its start is known.
                    (stream.rendered < stream.target(now_ms) && store.block(stream.sound, stream.next).is_some())
                        .then_some(now_ms)
                };
                [render, stream.until_ms].into_iter().flatten().min()
            })
            .min()
    }
}

/// Whether a live playing has a block to render now: the next has arrived,
/// or is to be played through as lost.
fn live_owed<const OBJECTS: usize>(store: &Store<'_, OBJECTS>, entry: &Entry, stream: &Stream) -> bool {
    let Some(newest) = entry.newest else { return false };
    !crate::serial_newer(stream.next, newest)
        && (store.live_block(stream.sound, stream.next).is_some()
            || newest.wrapping_sub(stream.next) >= crate::LIVE_LOOKAHEAD_BLOCKS)
}

impl Lane {
    /// Frees the lane: its playing is over, and what was read of it with it.
    fn free(&mut self) {
        self.stream = None;
        #[cfg(feature = "watch")]
        {
            self.watched = None;
        }
    }

    /// Makes the lane's synthesizer new for `entry`'s sound and starts it
    /// with its banks (MIDI-3, MIDI-61). What a page's rules read of the
    /// sound, if the lane keeps it, is new with the instance.
    fn start<const OBJECTS: usize>(&mut self, store: &Store<'_, OBJECTS>, entry: &Entry, statistics: &mut Statistics) {
        self.synthesizer = Synthesizer::of(entry);
        #[cfg(feature = "watch")]
        if self.watched.is_some() {
            self.watched = watched_of(store, entry);
        }
        let banks = banks(store, entry);
        let outcome = with_instrument!(&mut self.synthesizer, instrument => instrument.start(&banks), Outcome::Applied);
        if outcome == Outcome::PatchRefused {
            statistics.patches_refused = statistics.patches_refused.saturating_add(1);
        }
    }

    /// Renders a stored sound's playing to the largest lead (INT-34): in
    /// order from where it stands, on from the last block to block 0 when
    /// looped (MIDI-105), a block not arrived when its lead runs out played
    /// through as lost (INT-25); ends its stream after the last block of an
    /// unlooped sound (INT-36).
    fn render_stored<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        now_ms: u64,
        budget: &mut u16,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let Some(mut stream) = self.stream else { return };
        let banks = banks(store, entry);
        let mut pcm = [0_i16; BLOCK];
        while *budget > 0 && stream.rendered < stream.target(now_ms) {
            if stream.next >= entry.blocks {
                if stream.looped {
                    stream.next = 0;
                    continue;
                }
                if let Some(looped) = stream.again.take() {
                    self.start(store, entry, statistics);
                    stream.looped = looped;
                    stream.next = 0;
                    continue;
                }
                sink.end_stream(stream.channel, stream.sound);
                self.free();
                return;
            }
            match store.block(entry.sound, stream.next) {
                Some(block) => {
                    let played = self.render(block, &banks, &mut pcm, statistics);
                    mark_cues(&stream, &played, sink);
                }
                None if stream.due(now_ms) => {
                    self.conceal(&mut pcm);
                    statistics.concealed = statistics.concealed.saturating_add(1);
                    sink.concealed(entry.sound, stream.next, crate::INSTRUMENT_BLOCK_SAMPLES);
                }
                None => break,
            }
            sink.stream_samples(stream.channel, stream.sound, &pcm);
            stream.next = stream.next.saturating_add(1);
            stream.rendered = stream.rendered.saturating_add(1);
            *budget = budget.saturating_sub(1);
        }
        self.stream = Some(stream);
    }

    /// Renders a live sound's playing in index order from its cursor: a
    /// block still missing [`crate::LIVE_LOOKAHEAD_BLOCKS`] behind the
    /// newest is played through as lost, so a lost block never stops the
    /// sound (MIDI-101, MIDI-102).
    fn render_live<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        budget: &mut u16,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let Some(mut stream) = self.stream else { return };
        let banks = banks(store, entry);
        let mut pcm = [0_i16; BLOCK];
        // At most a ring's worth a call: the cursor never laps the ring.
        for _ in 0..entry.blocks {
            if *budget == 0 || !live_owed(store, entry, &stream) {
                break;
            }
            if let Some(block) = store.live_block(entry.sound, stream.next) {
                let played = self.render(block, &banks, &mut pcm, statistics);
                mark_cues(&stream, &played, sink);
            } else {
                self.conceal(&mut pcm);
                statistics.concealed = statistics.concealed.saturating_add(1);
                sink.concealed(entry.sound, stream.next, crate::INSTRUMENT_BLOCK_SAMPLES);
            }
            sink.stream_samples(stream.channel, stream.sound, &pcm);
            stream.next = stream.next.wrapping_add(1);
            stream.rendered = stream.rendered.saturating_add(1);
            *budget = budget.saturating_sub(1);
        }
        self.stream = Some(stream);
    }
    /// Plays one block's event list into the lane's synthesizer and renders
    /// its 20 ms, counting what it passed over (MIDI-123). Answers what the
    /// block held, for its cues (RULE-109).
    fn render(
        &mut self,
        block: &[u8],
        banks: &Banks<'_>,
        pcm: &mut [i16; BLOCK],
        statistics: &mut Statistics,
    ) -> Played {
        let played = with_instrument!(
            &mut self.synthesizer,
            instrument => play_block(instrument, block, banks, pcm),
            {
                pcm.fill(0);
                Played::default()
            }
        );
        // RULE-30: what a rule reads is the state at the position rendered.
        #[cfg(feature = "watch")]
        if let Some(watched) = self.watched.as_mut() {
            watched.take_block(block, banks);
        }
        statistics.events_passed_over = statistics.events_passed_over.saturating_add(played.passed_over);
        statistics.patches_refused = statistics.patches_refused.saturating_add(played.patches_refused);
        if played.malformed {
            statistics.blocks_malformed = statistics.blocks_malformed.saturating_add(1);
        }
        played
    }

    /// Renders 20 ms for a block that never arrived (MIDI-102): the
    /// synthesizer is told input was lost first, since the lost block may
    /// have held a note off.
    fn conceal(&mut self, pcm: &mut [i16; BLOCK]) {
        // No key is down after input was lost, for the rules as for the ear.
        #[cfg(feature = "watch")]
        if let Some(watched) = self.watched.as_mut() {
            watched.release_all();
        }
        with_instrument!(
            &mut self.synthesizer,
            instrument => {
                instrument.lost();
                instrument.render(pcm);
            },
            pcm.fill(0)
        );
    }
}

/// Gives the sink a mark for each cue of the block `stream` renders next
/// (page-rules.md RULE-109; REQ-6614 item 3): the cue's number and its
/// place in the stream, a count of samples from the stream's first, which
/// is every block given to the stream so far and the cue's sample within
/// this one. The receiver renders ahead of what is heard, so the sink
/// reports the cue when that sample is heard. A stream fading out of a
/// stop marks none: a stopped sound has no further cue.
fn mark_cues(stream: &Stream, played: &Played, sink: &mut dyn VoiceSink) {
    if stream.until_ms.is_some() {
        return;
    }
    let block_start = u64::from(stream.rendered).saturating_mul(u64::from(crate::INSTRUMENT_BLOCK_SAMPLES));
    for cue in played.cues.as_slice() {
        let position = block_start.saturating_add(u64::from(cue.sample));
        sink.stream_cue(stream.channel, stream.sound, position, cue.number);
    }
}

/// The bank images a sound names, as the store holds them now: each one a
/// complete bank for the sound's family (MIDI-52, MIDI-57). A sound starts
/// only once all are (`Receiver::bank_ready`), and the store keeps them
/// while it plays (MIDI-65); one that is gone is left out with every bank
/// after it, and its programs play the default patch (MIDI-59).
pub(super) fn banks<'s, const OBJECTS: usize>(store: &'s Store<'_, OBJECTS>, entry: &Entry) -> Banks<'s> {
    let mut banks = Banks::none();
    for &handle in entry.banks() {
        let image =
            store.entry(handle).filter(|bank| bank.setting == entry.codec.code()).and_then(|_| store.image(handle));
        let Some(image) = image else { break };
        banks.push(image);
    }
    banks
}
