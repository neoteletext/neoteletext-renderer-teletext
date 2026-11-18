//! The receiver: records in, voice operations out. It follows one audio
//! service, keeps sounds in its store, applies each channel's latest
//! command, and plays a page's sounds while that page is open and the user
//! allows page sounds (REQ-AUD-009 to REQ-AUD-013).

use crate::CHANNELS;
use crate::control::{Channel, Play, fade};
#[cfg(feature = "idl")]
use crate::idl::Reassembler;
use crate::port::{SoundId, VoiceSink};
use crate::record::{Control, Operation, Record, parse};
use crate::store::{Defined, Entry, Store, Stored};

#[cfg(feature = "idl")]
mod carriage;
#[cfg(any(feature = "pcm16", feature = "ima", feature = "opus"))]
mod fade;
mod file;
#[cfg(any(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
mod instruments;
mod loss;
#[cfg(feature = "opus")]
mod opus;
mod play;
mod send;

#[cfg(feature = "idl")]
pub use carriage::MessageError;

/// Counts of what the receiver met, for diagnostics (REQ-AUD-021).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Statistics {
    /// Audio packets dropped: damaged or malformed.
    pub packets_dropped: u32,
    /// Records read.
    pub records: u32,
    /// Records refused: a bad frame, CRC or field, or a DATA record with a
    /// block of the wrong length or beyond its sound's end (REQ-AUD-005).
    /// These last also count in `records`, as they were read.
    pub records_refused: u32,
    /// Objects or blocks in conflict with what was stored.
    pub conflicts: u32,
    /// Objects that did not fit the store.
    pub resource_limits: u32,
    /// Blocks given as silence, or concealed by their decoder, because they
    /// had not arrived when due (REQ-AUD-013): a live sound's block given up
    /// once three newer have come, and a stored sound's block not at the
    /// sink when its play reached it by the receiver's clock. Each is also
    /// reported to the sink ([`VoiceSink::concealed`]).
    pub concealed: u32,
    /// Commands ignored as repeats or late arrivals.
    pub stale_commands: u32,
    /// Blocks that could not be decoded.
    pub decode_failures: u32,
    /// OBJECT records of a codec this receiver does not decode, a reserved
    /// one or one its build leaves out (decision E.131): each is ignored,
    /// and counted at each repeat.
    pub unsupported: u32,
    /// WebSocket messages refused whole: not a whole number of packets
    /// (REQ-AUD-008).
    pub messages_refused: u32,
    /// Events of a synthesizer's block passed over: a form a block may hold
    /// but no synthesizer acts on, an event past the 256th, or a message the
    /// family does not take (midi-architecture.md, MIDI-29, MIDI-31,
    /// MIDI-34, MIDI-123).
    pub events_passed_over: u32,
    /// Synthesizer blocks with a framing error, each counted once; what was
    /// applied before the error stands (MIDI-33).
    pub blocks_malformed: u32,
    /// Patches, in a bank or sent inline, that broke their family's rules
    /// and were replaced by the family's default (MIDI-63).
    pub patches_refused: u32,
    /// Plays of a local sound refused (page-rules.md, RULE-60): a play
    /// with no page, looped or waiting for the current sound, or a third
    /// while [`crate::MOST_LOCAL_SOUNDS`] play.
    pub local_refused: u32,
}

/// The receiver of one audio service.
pub struct Receiver<'a, const OBJECTS: usize, const SLOTS: usize> {
    store: Store<'a, OBJECTS>,
    #[cfg(feature = "idl")]
    reassembler: Reassembler<SLOTS>,
    channels: [Channel; CHANNELS as usize],
    service: u8,
    opened: Option<u16>,
    page_sounds: bool,
    statistics: Statistics,
    /// The port's time, in milliseconds (REQ-AUD-014).
    now_ms: u64,
    /// Opus sounds' decoders (decision E.124).
    #[cfg(feature = "opus")]
    opus: opus::Lanes,
    /// The synthesizers' lanes, each rendering one playing into a stream
    /// (INT-32, INT-33).
    #[cfg(any(
        feature = "synth6581",
        feature = "synth7",
        feature = "synth106",
        feature = "synthd",
        feature = "synth53",
        feature = "synth8",
        feature = "synth80"
    ))]
    instruments: instruments::Lanes,
    /// When each channel's latest play is heard to its end, by the
    /// receiver's clock, for a play after it (INT-36).
    heard: [play::Heard; CHANNELS as usize],
    /// Each channel's stored-sound play, followed for the `concealed`
    /// report (REQ-AUD-013).
    progress: [Option<loss::Progress>; CHANNELS as usize],
    /// The local sound each channel plays at the sink, if it plays one
    /// (RULE-60): at most [`crate::MOST_LOCAL_SOUNDS`] at once.
    local: [Option<SoundId>; CHANNELS as usize],
}

impl<'a, const OBJECTS: usize, const SLOTS: usize> Receiver<'a, OBJECTS, SLOTS> {
    /// A receiver of audio service 0, keeping its sounds in `arena`, with
    /// page sounds on.
    pub fn new(arena: &'a mut [u8]) -> Self {
        Self {
            store: Store::new(arena),
            #[cfg(feature = "idl")]
            reassembler: Reassembler::new(),
            channels: [Channel::default(); CHANNELS as usize],
            service: 0,
            opened: None,
            page_sounds: true,
            statistics: Statistics::default(),
            now_ms: 0,
            #[cfg(feature = "opus")]
            opus: opus::Lanes::new(),
            #[cfg(any(
                feature = "synth6581",
                feature = "synth7",
                feature = "synth106",
                feature = "synthd",
                feature = "synth53",
                feature = "synth8",
                feature = "synth80"
            ))]
            instruments: instruments::Lanes::new(),
            heard: [play::Heard::Idle; CHANNELS as usize],
            progress: [None; CHANNELS as usize],
            local: [None; CHANNELS as usize],
        }
    }

    /// The port's time, in milliseconds from any start: a live sound with no
    /// new block for [`crate::LIVE_STALL_MS`] fades out, and its play waits
    /// for it to resume (REQ-AUD-014).
    pub fn tick(&mut self, now_ms: u64, sink: &mut dyn VoiceSink) {
        self.now_ms = now_ms;
        // One pass over the store: a port ticks on every message it
        // handles, so this is paid constantly (decision E.103).
        let mut stalled: [Option<Entry>; OBJECTS] = [None; OBJECTS];
        for (slot, entry) in stalled.iter_mut().zip(self.store.entries()) {
            if entry.live && entry.at_sink && now_ms.saturating_sub(entry.arrived_ms) >= crate::LIVE_STALL_MS {
                *slot = Some(*entry);
            }
        }
        for entry in stalled.iter().flatten() {
            for index in 0..CHANNELS {
                let Some(channel) = self.channels.get_mut(usize::from(index)) else { continue };
                if let Some(play) = channel.playing.filter(|play| play.sound == entry.sound) {
                    sink.stop(index, crate::LIVE_STALL_FADE_MS);
                    channel.playing = None;
                    channel.waiting = Some(play);
                    if let Some(heard) = self.heard.get_mut(usize::from(index)) {
                        *heard = play::Heard::Idle;
                    }
                }
            }
            sink.forget(entry.sound);
            self.store.set_at_sink(entry.sound, false);
            #[cfg(feature = "opus")]
            self.opus.release(entry.sound);
            #[cfg(any(
                feature = "synth6581",
                feature = "synth7",
                feature = "synth106",
                feature = "synthd",
                feature = "synth53",
                feature = "synth8",
                feature = "synth80"
            ))]
            self.instruments.release(entry.sound);
        }
        self.render(sink);
        self.judge(sink);
    }

    /// Renders the synthesizers' playings as far as they are owed, within
    /// the render budget (INT-34, INT-35). Every call that can start, feed
    /// or advance a playing ends with it.
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
        expect(clippy::unused_self, reason = "a build without the MIDI layer has no lanes")
    )]
    fn render(&mut self, sink: &mut dyn VoiceSink) {
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        self.instruments.render(&self.store, self.now_ms, &mut self.statistics, sink);
        #[cfg(not(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        )))]
        let _ = sink;
    }

    /// When the receiver next needs [`Receiver::tick`], in its milliseconds:
    /// the moment the live sound at the sink whose last block came longest
    /// ago would stall (REQ-AUD-014), or a stored sound's play reaches a
    /// block that has not arrived, to report it concealed (REQ-AUD-013), or
    /// a synthesizer's playing must be rendered to keep its lead (INT-34) or
    /// is owed blocks the render budget left (INT-35); or `None` while none
    /// of these can happen, when it needs no tick at all. A port
    /// sleeps until then, rather than ticking on a fixed interval (decision
    /// E.98).
    #[must_use]
    pub fn next_tick_ms(&self) -> Option<u64> {
        let stall = self
            .store
            .entries()
            .filter(|entry| entry.live && entry.at_sink)
            .map(|entry| entry.arrived_ms.saturating_add(crate::LIVE_STALL_MS))
            .min();
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        let rendering = self.instruments.next_tick_ms(&self.store, self.now_ms);
        #[cfg(not(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        )))]
        let rendering = None;
        [stall, self.next_due_ms(), rendering].into_iter().flatten().min()
    }

    /// What the receiver has met.
    #[must_use]
    pub fn statistics(&self) -> Statistics {
        self.statistics
    }

    /// What this receiver holds, for its port to publish and to check
    /// against its level's minimum (REQ-AUD-017): the store's bytes, the
    /// objects and the records in flight it was built with.
    #[must_use]
    pub fn limits(&self) -> crate::ReceiverLimits {
        crate::ReceiverLimits { store_bytes: self.store.capacity(), objects: OBJECTS, records_in_flight: SLOTS }
    }

    /// The store, for diagnostics.
    #[must_use]
    pub fn store(&self) -> &Store<'a, OBJECTS> {
        &self.store
    }

    /// Follows broadcast audio service `service` (0 to 7) instead, forgetting
    /// everything of the one before.
    pub fn follow_service(&mut self, service: u8, sink: &mut dyn VoiceSink) {
        if service != self.service {
            self.service = service;
            self.reset(sink);
        }
    }

    /// Forgets every sound and command, as on a change of service
    /// (REQ-AUD-010), and stops every channel.
    pub fn reset(&mut self, sink: &mut dyn VoiceSink) {
        for channel in 0..CHANNELS {
            sink.stop(channel, crate::MIN_FADE_MS);
        }
        for entry in self.store.entries().filter(|entry| entry.at_sink) {
            sink.forget(entry.sound);
        }
        self.store.clear();
        #[cfg(feature = "idl")]
        self.reassembler.clear();
        #[cfg(feature = "opus")]
        self.opus.clear();
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        self.instruments.clear();
        self.channels = [Channel::default(); CHANNELS as usize];
        self.heard = [play::Heard::Idle; CHANNELS as usize];
        self.progress = [None; CHANNELS as usize];
        self.local = [None; CHANNELS as usize];
    }

    /// The sink has dropped every sound and stopped every voice, as when
    /// another tab became the one heard: sounds are sent again before they
    /// next play.
    pub fn sink_lost(&mut self) {
        self.store.sink_lost();
        #[cfg(feature = "opus")]
        self.opus.clear();
        #[cfg(any(
            feature = "synth6581",
            feature = "synth7",
            feature = "synth106",
            feature = "synthd",
            feature = "synth53",
            feature = "synth8",
            feature = "synth80"
        ))]
        self.instruments.clear();
        for channel in &mut self.channels {
            channel.playing_page = None;
            channel.playing = None;
        }
        self.heard = [play::Heard::Idle; CHANNELS as usize];
        self.progress = [None; CHANNELS as usize];
        self.local = [None; CHANNELS as usize];
    }

    /// Reads one whole record, from any carriage.
    pub fn push_record(&mut self, bytes: &[u8], sink: &mut dyn VoiceSink) {
        // The channel's records share the carriage and are another
        // reader's (DOC-1951).
        if crate::record::split(bytes).is_ok_and(|(kind, _)| crate::record::is_channel_type(kind)) {
            return;
        }
        let Ok(record) = parse(bytes) else {
            bump(&mut self.statistics.records_refused);
            return;
        };
        bump(&mut self.statistics.records);
        match record {
            // A sink with no sound output stage for it ignores a local
            // sound, as a receiver ignores a codec it does not have
            // (RULE-60).
            Record::Object(object) if object.local && object.codec.is_some() && !sink.plays_local() => {
                bump(&mut self.statistics.unsupported);
            }
            Record::Object(object) => {
                let channels = self.channels;
                #[cfg(any(
                    feature = "synth6581",
                    feature = "synth7",
                    feature = "synth106",
                    feature = "synthd",
                    feature = "synth53",
                    feature = "synth8",
                    feature = "synth80"
                ))]
                let following = self.instruments.following();
                #[cfg(not(any(
                    feature = "synth6581",
                    feature = "synth7",
                    feature = "synth106",
                    feature = "synthd",
                    feature = "synth53",
                    feature = "synth8",
                    feature = "synth80"
                )))]
                let following: [Option<SoundId>; 0] = [];
                // A sound a channel needs is kept, and so is every bank a
                // sound a channel needs or a synthesizer plays names
                // (MIDI-65), by its handle.
                let needed = |held: SoundId| channels.iter().any(|channel| needs(channel, held));
                let mut banks_kept = [false; HANDLES];
                for entry in self.store.entries() {
                    if needed(entry.sound) || following.contains(&Some(entry.sound)) {
                        for &handle in entry.banks() {
                            if let Some(kept) = banks_kept.get_mut(usize::from(handle)) {
                                *kept = true;
                            }
                        }
                    }
                }
                let busy = |sound: SoundId| {
                    needed(sound) || banks_kept.get(usize::from(sound.handle)).copied().unwrap_or(false)
                };
                match self.store.define(&object, &busy) {
                    Defined::New { replaced, evicted } => {
                        let replaced = replaced.filter(|old| old.at_sink).map(|old| old.sound);
                        for sound in evicted.into_iter().chain([replaced]).flatten() {
                            sink.forget(sound);
                            #[cfg(feature = "opus")]
                            self.opus.release(sound);
                            #[cfg(any(
                                feature = "synth6581",
                                feature = "synth7",
                                feature = "synth106",
                                feature = "synthd",
                                feature = "synth53",
                                feature = "synth8",
                                feature = "synth80"
                            ))]
                            self.instruments.release(sound);
                        }
                    }
                    Defined::Repeat => {}
                    Defined::Ignored => bump(&mut self.statistics.unsupported),
                    Defined::Conflict => bump(&mut self.statistics.conflicts),
                    Defined::ResourceLimit => bump(&mut self.statistics.resource_limits),
                }
                // An announced sound may be what a waiting play needs.
                self.start_waiting(sink);
            }
            Record::Data(data) => match self.store.put(&data) {
                Stored::Blocks { first, count } => {
                    if let Some(entry) = self.store.entry(data.handle).copied() {
                        if entry.live {
                            self.store.set_live_cursor(entry.sound, entry.next, self.now_ms);
                            if entry.at_sink {
                                self.send_live(entry.sound, sink);
                            }
                        } else if entry.at_sink {
                            self.send_blocks(&entry, first, count, sink);
                        }
                    }
                    self.start_waiting(sink);
                }
                Stored::Conflict => bump(&mut self.statistics.conflicts),
                Stored::Malformed => bump(&mut self.statistics.records_refused),
                Stored::Repeat | Stored::Unknown => {}
            },
            Record::Control(control) => self.command(&control, sink),
        }
        self.render(sink);
        self.judge(sink);
    }

    /// The user opened `page`, or none: page sounds follow it (REQ-AUD-012).
    pub fn open_page(&mut self, page: Option<u16>, sink: &mut dyn VoiceSink) {
        if page == self.opened {
            return;
        }
        let left = self.opened;
        self.opened = page;
        for index in 0..CHANNELS {
            let Some(channel) = self.channels.get(usize::from(index)) else { continue };
            if left.is_some() && channel.playing_page == left {
                self.stop_page_play(index, sink);
            }
            self.drop_waiting_page_play(index);
        }
        if self.page_sounds {
            self.start_armed(sink);
        }
        self.render(sink);
    }

    /// The user's page sounds setting (REQ-AUD-012).
    pub fn set_page_sounds(&mut self, on: bool, sink: &mut dyn VoiceSink) {
        if on == self.page_sounds {
            return;
        }
        self.page_sounds = on;
        if on {
            self.start_armed(sink);
        } else {
            for index in 0..CHANNELS {
                if self.channels.get(usize::from(index)).is_some_and(|channel| channel.playing_page.is_some()) {
                    self.stop_page_play(index, sink);
                }
                self.drop_waiting_page_play(index);
            }
        }
        self.render(sink);
    }

    /// Stops channel `index`'s page sound. `playing` goes too: left set, a
    /// live sound that then stalls would be taken for one still playing and
    /// wait to start again (REQ-AUD-012).
    fn stop_page_play(&mut self, index: u8, sink: &mut dyn VoiceSink) {
        if let Some(channel) = self.channels.get_mut(usize::from(index)) {
            channel.playing_page = None;
            channel.playing = None;
        }
        sink.stop(index, crate::MIN_FADE_MS);
        self.stop_streams(index, crate::MIN_FADE_MS, sink);
        self.end_progress(index);
        self.end_local(index);
    }

    /// Gives up channel `index`'s play waiting for its sound if it belongs to
    /// a page: it is armed still, for when the page is shown (REQ-AUD-012).
    fn drop_waiting_page_play(&mut self, index: u8) {
        if let Some(channel) = self.channels.get_mut(usize::from(index))
            && channel.waiting.is_some_and(|play| play.page.is_some())
        {
            channel.waiting = None;
        }
    }

    /// A command a rule of the open page's table gives (page-rules.md,
    /// RULE-20: the actions `play`, `stop` and `gain`), "as the audio
    /// profile's play command does": it is carried out as a CONTROL record's
    /// is, but belongs to no sender's sequence, so `control.sequence` is not
    /// read and the channel's sequence is left as it was. The caller names
    /// the page the rule belongs to in `control.page`.
    pub fn rule_command(&mut self, control: &Control, sink: &mut dyn VoiceSink) {
        self.perform(control, sink);
        self.render(sink);
        self.judge(sink);
    }

    fn command(&mut self, control: &Control, sink: &mut dyn VoiceSink) {
        let Some(channel) = self.channels.get_mut(usize::from(control.channel)) else { return };
        if !channel.accept(control.sequence) {
            bump(&mut self.statistics.stale_commands);
            return;
        }
        self.perform(control, sink);
    }

    /// Carries out a command whose place in its channel's sequence is
    /// settled.
    fn perform(&mut self, control: &Control, sink: &mut dyn VoiceSink) {
        let index = control.channel;
        let Some(channel) = self.channels.get_mut(usize::from(index)) else { return };
        // Channels beyond the sink's are ignored (REQ-AUD-016).
        if index >= sink.channels() {
            return;
        }
        match control.operation {
            Operation::Play => {
                let play = Play::from_control(control);
                channel.waiting = None;
                if let Some(page) = play.page {
                    channel.armed = Some(play);
                    if self.opened == Some(page) && self.page_sounds {
                        self.try_start(index, play, sink);
                    }
                } else {
                    channel.armed = None;
                    self.try_start(index, play, sink);
                }
            }
            Operation::Stop => {
                channel.armed = None;
                channel.waiting = None;
                channel.playing_page = None;
                channel.playing = None;
                sink.stop(index, fade(control.fade_ms));
                self.stop_streams(index, fade(control.fade_ms), sink);
                self.end_progress(index);
                self.end_local(index);
            }
            Operation::Gain => sink.gain(index, control.gain, fade(control.fade_ms)),
        }
    }
}

/// The audio channels on which the sink holds a stream of a live sound
/// (INT-37).
#[cfg(any(feature = "opus", feature = "pcm16", feature = "ima"))]
fn stream_channels(entry: &Entry) -> impl Iterator<Item = u8> + use<> {
    let streams = entry.streams;
    (0..CHANNELS).filter(move |&channel| streams.checked_shr(u32::from(channel)).is_some_and(|bits| bits & 1 == 1))
}

/// Gives a live sound's next samples to each of its streams (INT-37): every
/// audio channel that plays it hears the same samples.
#[cfg(any(feature = "opus", feature = "pcm16", feature = "ima"))]
fn stream_out(sink: &mut dyn VoiceSink, entry: &Entry, pcm: &[i16]) {
    for channel in stream_channels(entry) {
        sink.stream_samples(channel, entry.sound, pcm);
    }
}

// A bit of `Entry::streams` for each audio channel.
const _: () = assert!(CHANNELS as u32 <= u8::BITS);

/// Counts one more.
fn bump(counter: &mut u32) {
    *counter = counter.saturating_add(1);
}

/// Whether a channel still needs a sound: playing, armed or waiting for it.
fn needs(channel: &Channel, sound: SoundId) -> bool {
    [channel.armed, channel.waiting, channel.playing].iter().flatten().any(|play| play.sound == sound)
}

/// The handles a sound may have: one byte.
const HANDLES: usize = 1 << u8::BITS;

/// Milliseconds in a second, for a rate's samples.
const MS_PER_SECOND: u32 = 1000;

/// A sound's whole length in milliseconds.
fn duration_ms(entry: &Entry) -> u32 {
    entry.samples.saturating_mul(MS_PER_SECOND).checked_div(entry.rate_hz).unwrap_or(0)
}

#[cfg(test)]
mod tests;
