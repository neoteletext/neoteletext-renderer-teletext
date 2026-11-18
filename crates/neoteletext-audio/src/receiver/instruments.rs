//! Instrument sounds rendered in order (DEC-6410; midi-architecture.md,
//! MIDI-3 to MIDI-5, MIDI-100 to MIDI-106). A block of an event codec is
//! 20 ms of the synthesizer's input, an event list (MIDI-23), and what it
//! sounds like depends on every block before it, as an Opus frame depends
//! on the frames before it. So the receiver keeps
//! [`crate::INSTRUMENT_LANES`] synthesizers, each following one sound from a
//! block on (MIDI-3): a stored sound's blocks are rendered from the first,
//! as far as they have arrived without a gap, and its synthesizer is freed
//! when the last is rendered; a live sound keeps its synthesizer, which
//! plays on through a block that never arrived (MIDI-102).
//!
//! Every family is played through one trait,
//! [`neoteletext_audio_instruments::midi::Instrument`], with the bank
//! images of its sound read from the store each call ([`banks`]); the
//! event list is read by the one reader every family and the server share
//! ([`neoteletext_audio_instruments::midi::play_block`]).
//!
//! Rendering is work the sound's sender chose, so a call renders at most
//! [`RENDER_BUDGET_BLOCKS`] blocks of a stored sound and says whether more
//! wait; the receiver asks to be ticked again at once while they do
//! ([`super::Receiver::next_tick_ms`]). A sound of any length therefore
//! never holds the port for longer than the budget's blocks cost (MIDI-106).

use neoteletext_audio_instruments::midi::{Banks, Instrument, Outcome, Played, play_block};
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

use super::Statistics;
use crate::Codec;
use crate::port::{SoundId, VoiceSink};
use crate::store::{Entry, Store};

/// The samples of one block, as a buffer's length.
const BLOCK: usize = crate::INSTRUMENT_BLOCK_SAMPLES as usize;

/// The most blocks of a stored sound one call renders: one second of sound
/// (MIDI-106). A larger budget starts a long sound sooner and holds the
/// port longer.
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
                // `record::TAG_VARIANT`: 0 synth53; 1 synth52 and 2 synth51,
                // which are one model today (MIDI-1).
                let model = if entry.setting == 0 { synth53::Model::Synth53 } else { synth53::Model::Synth52 };
                Self::Synth53(synth53::midi::Midi::new(synth53::Synth53::new(model)))
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

/// One synthesizer and the sound it follows.
struct Lane {
    /// The sound, or `None` while free.
    sound: Option<SoundId>,
    /// The next block it renders.
    next: u16,
    /// The synthesizer.
    synthesizer: Synthesizer,
}

/// The receiver's synthesizers.
pub(super) struct Lanes {
    lanes: [Lane; crate::INSTRUMENT_LANES],
}

impl Lanes {
    pub(super) fn new() -> Self {
        Self { lanes: core::array::from_fn(|_| Lane { sound: None, next: 0, synthesizer: Synthesizer::None }) }
    }

    /// Frees every synthesizer: the sink has dropped every sound.
    pub(super) fn clear(&mut self) {
        for lane in &mut self.lanes {
            lane.sound = None;
        }
    }

    /// Frees the synthesizer that follows `sound`, if one does.
    pub(super) fn release(&mut self, sound: SoundId) {
        for lane in self.lanes.iter_mut().filter(|lane| lane.sound == Some(sound)) {
            lane.sound = None;
        }
    }

    /// The sounds the synthesizers follow, whose banks the store must keep
    /// (MIDI-65).
    pub(super) fn following(&self) -> [Option<SoundId>; crate::INSTRUMENT_LANES] {
        core::array::from_fn(|index| self.lanes.get(index).and_then(|lane| lane.sound))
    }

    /// Whether a stored sound's blocks wait to be rendered: its synthesizer
    /// stands at a block that has arrived.
    pub(super) fn pending<const OBJECTS: usize>(&self, store: &Store<'_, OBJECTS>) -> bool {
        self.waiting(store).iter().any(Option::is_some)
    }

    /// The stored sounds whose synthesizers have blocks waiting, for the
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

    /// The synthesizer that follows `entry`'s sound; or a free one, or one
    /// whose sound the store no longer holds, made new for `entry` and
    /// started at block `first` with its banks (MIDI-61); or none, while
    /// every synthesizer is busy.
    fn lane<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        first: u16,
        statistics: &mut Statistics,
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
            lane.synthesizer = Synthesizer::of(entry);
            let banks = banks(store, entry);
            let outcome =
                with_instrument!(&mut lane.synthesizer, instrument => instrument.start(&banks), Outcome::Applied);
            if outcome == Outcome::PatchRefused {
                statistics.patches_refused = statistics.patches_refused.saturating_add(1);
            }
        }
        Some(lane)
    }

    /// Renders a stored sound's blocks from where its synthesizer stands, as
    /// far as they have arrived without a gap and no further than the
    /// budget, and gives the sink their samples.
    pub(super) fn send_stored<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) {
        let Some(lane) = self.lane(store, entry, 0, statistics) else { return };
        let banks = banks(store, entry);
        let mut pcm = [0_i16; BLOCK];
        for _ in 0..RENDER_BUDGET_BLOCKS {
            if lane.next >= entry.blocks {
                break;
            }
            let Some(block) = store.block(entry.sound, lane.next) else { break };
            lane.render(block, &banks, &mut pcm, statistics);
            let first = u32::from(lane.next).saturating_mul(entry.samples_per_block());
            sink.samples(entry.sound, first, &pcm);
            lane.next = lane.next.saturating_add(1);
        }
        if lane.next >= entry.blocks {
            lane.sound = None;
        }
    }

    /// Renders a live sound's blocks in index order from its cursor: a
    /// block still missing [`crate::LIVE_LOOKAHEAD_BLOCKS`] behind the
    /// newest is played through as lost, so a lost block never stops the
    /// sound (MIDI-101, MIDI-102). Answers the next block, for the store's
    /// cursor.
    pub(super) fn send_live<const OBJECTS: usize>(
        &mut self,
        store: &Store<'_, OBJECTS>,
        entry: &Entry,
        statistics: &mut Statistics,
        sink: &mut dyn VoiceSink,
    ) -> u16 {
        let mut next = entry.next;
        let Some(newest) = entry.newest else { return next };
        let Some(lane) = self.lane(store, entry, next, statistics) else { return next };
        let banks = banks(store, entry);
        let mut pcm = [0_i16; BLOCK];
        // At most a ring's worth a call: the cursor never laps the ring.
        for _ in 0..entry.blocks {
            if crate::serial_newer(next, newest) {
                break;
            }
            match store.live_block(entry.sound, next) {
                Some(block) => lane.render(block, &banks, &mut pcm, statistics),
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
    /// Plays one block's event list into the lane's synthesizer and renders
    /// its 20 ms, counting what it passed over (MIDI-123).
    fn render(&mut self, block: &[u8], banks: &Banks<'_>, pcm: &mut [i16; BLOCK], statistics: &mut Statistics) {
        let played = with_instrument!(
            &mut self.synthesizer,
            instrument => play_block(instrument, block, banks, pcm),
            {
                pcm.fill(0);
                Played::default()
            }
        );
        statistics.events_passed_over = statistics.events_passed_over.saturating_add(played.passed_over);
        statistics.patches_refused = statistics.patches_refused.saturating_add(played.patches_refused);
        if played.malformed {
            statistics.blocks_malformed = statistics.blocks_malformed.saturating_add(1);
        }
    }

    /// Renders 20 ms for a block that never arrived (MIDI-102): the
    /// synthesizer is told input was lost first, since the lost block may
    /// have held a note off.
    fn conceal(&mut self, pcm: &mut [i16; BLOCK]) {
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
