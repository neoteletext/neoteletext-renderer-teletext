//! The analogue synthesizers as instruments of a lane (DEC-6429 foundations
//! item 4): synth106, synthD, synth53, synth8 and the
//! synth80, each behind its feature. A sound of one is synth7's block, slots
//! of timed MIDI messages, under the instrument's own codec, and its
//! patches come from a patch bank of the instrument's family, read when the
//! sound starts, as a synth7 sound's are.
//!
//! None of these instruments knows of a bank, so a program change is taken
//! here: it gives the instrument the patch of that number from the copy of
//! the bank the lane holds. Until one comes the instrument plays patch 0.
//! A lane holds one synthesizer at a time, so it is as large as the largest
//! built in and no larger.
#![cfg_attr(
    not(any(feature = "synth106", feature = "synthd", feature = "synth53", feature = "synth8", feature = "synth80")),
    allow(
        dead_code,
        unused_imports,
        unused_variables,
        reason = "the feature `analog` alone builds what the synthesizers share and no synthesizer to use it"
    )
)]

#[cfg(feature = "synth8")]
use neoteletext_audio_instruments::synth8;
#[cfg(feature = "synth53")]
use neoteletext_audio_instruments::synth53;
#[cfg(feature = "synth80")]
use neoteletext_audio_instruments::synth80;
#[cfg(feature = "synth106")]
use neoteletext_audio_instruments::synth106;
#[cfg(feature = "synthd")]
use neoteletext_audio_instruments::synthd;

use super::{BLOCK, MIDI_KIND, Player, Statistics, common, kind, render_slots};
use crate::Codec;
use crate::store::{Entry, Store};

/// The patches a lane holds for its synthesizer: a whole bank.
const BANK: usize = crate::PATCH_BANK_PATCHES as usize;

/// The bits of a MIDI status byte that are its channel (MIDI 1.0).
const MIDI_CHANNEL: u8 = 0x0F;

/// One of the analogue synthesizers, as a lane plays it.
pub(super) trait Synth {
    /// The instrument's patch, checked.
    type Patch: Copy;
    /// The patch the instrument plays where its bank has none, or one with
    /// a field out of its range: the instrument's own default.
    const PLAIN: Self::Patch;
    /// That patch for this instrument as it was made, where the default
    /// differs between an instrument's models.
    fn plain(&self) -> Self::Patch {
        Self::PLAIN
    }
    /// The codec of the instrument's sounds, which is also the code of its
    /// family in a patch bank.
    const FAMILY: Codec;
    /// The MIDI channels, counted from 0, whose program changes choose a
    /// patch: every channel for an instrument of one patch, since it hears
    /// every channel as one part.
    const PROGRAM_CHANNELS: u8 = MIDI_CHANNEL + 1;

    /// The patch a bank's block holds; `None` for a block of another
    /// length or with a field out of its range.
    fn patch(block: &[u8]) -> Option<Self::Patch>;
    /// Whether the instrument's codec takes a message.
    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        common(status, data1, data2)
    }
    /// Gives the instrument the patch a program change on `channel` chose.
    fn program(&mut self, channel: u8, patch: &Self::Patch);
    /// Gives the instrument any other message its codec takes.
    fn event(&mut self, status: u8, data1: u8, data2: u8);
    /// Renders the instrument's next samples.
    fn render(&mut self, out: &mut [i16]);
    /// Releases every note: no note off will come for them.
    fn release_all(&mut self);
}

/// A synthesizer and the bank its sound names.
pub(super) struct Played<S: Synth> {
    synth: S,
    patches: [S::Patch; BANK],
}

impl<S: Synth> Played<S> {
    /// `synth` with the patches of the bank `entry`'s sound names, playing
    /// the first of them.
    fn new<const OBJECTS: usize>(synth: S, store: &Store<'_, OBJECTS>, entry: &Entry) -> Self {
        let patches = bank::<S, OBJECTS>(store, entry.param, synth.plain());
        let mut played = Self { synth, patches };
        if let Some(first) = played.patches.first() {
            for channel in 0..S::PROGRAM_CHANNELS {
                played.synth.program(channel, first);
            }
        }
        played
    }

    /// Releases every note and renders 20 ms: a block never arrived, and
    /// it may have held a note off (DEC-6410 item 12).
    fn conceal(&mut self, pcm: &mut [i16; BLOCK]) {
        self.synth.release_all();
        self.synth.render(pcm);
    }
}

impl<S: Synth> Player for Played<S> {
    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        S::takes(status, data1, data2)
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        if status & MIDI_KIND == kind::PROGRAM_CHANGE {
            // A program the codec took is a patch of the bank.
            if let Some(patch) = self.patches.get(usize::from(data1)) {
                self.synth.program(status & MIDI_CHANNEL, patch);
            }
        } else {
            self.synth.event(status, data1, data2);
        }
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synth.render(out);
    }
}

/// The bank a sound names: the patches of the stored patch bank at `handle`
/// that is for the synthesizer's family, each checked by the instrument's
/// own reader. A patch that has not arrived, or that has a field out of its
/// range, leaves the instrument's default patch in its place, so a faulty
/// bank costs a sound its timbre and nothing else, as a faulty synth7 bank
/// does (REQ-6411 item 4, DEC-6421 item 4).
fn bank<S: Synth, const OBJECTS: usize>(store: &Store<'_, OBJECTS>, handle: u8, plain: S::Patch) -> [S::Patch; BANK] {
    let mut patches = [plain; BANK];
    let held = store
        .entry(handle)
        .filter(|entry| entry.codec == Codec::PatchBank && entry.setup.first() == Some(&S::FAMILY.code()));
    let Some(entry) = held else { return patches };
    for (index, place) in (0..entry.blocks).zip(&mut patches) {
        if let Some(patch) = store.block(entry.sound, index).and_then(S::patch) {
            *place = patch;
        }
    }
    patches
}

/// The controllers a synth106 block may carry beyond [`super::CONTROLLERS`],
/// which its instrument takes: the four mode messages, omni off, omni on,
/// mono on and poly on, each of which is to a synth106 an all notes off
/// and nothing more (its owner's manual, MIDI Implementation 2.1: "Mode
/// messages (123 - 127) are also recognized as ALL NOTES OFF").
#[cfg(feature = "synth106")]
const SYNTH106_CONTROLLERS: [u8; 4] = [124, 125, 126, 127];

#[cfg(feature = "synth106")]
impl Synth for synth106::Synth106 {
    type Patch = synth106::Patch;
    const PLAIN: Self::Patch = synth106::Patch::DEFAULT;

    // synth60's high-pass switch is flat at another position than
    // synth106's: each model has its own plain patch.
    fn plain(&self) -> Self::Patch {
        synth106::Patch::default_for(self.model())
    }
    const FAMILY: Codec = Codec::Synth106;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        synth106::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        match status & MIDI_KIND {
            kind::CONTROL_CHANGE => common(status, data1, data2) || SYNTH106_CONTROLLERS.contains(&data1),
            _ => common(status, data1, data2),
        }
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        synth106::Synth106::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        synth106::Synth106::render(self, out);
    }

    fn release_all(&mut self) {
        synth106::Synth106::release_all(self);
    }
}

#[cfg(feature = "synthd")]
impl Synth for synthd::SynthD {
    type Patch = synthd::Patch;
    const PLAIN: Self::Patch = synthd::Patch::DEFAULT;
    const FAMILY: Codec = Codec::SynthD;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        synthd::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        synthd::SynthD::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        synthd::SynthD::render(self, out);
    }

    fn release_all(&mut self) {
        synthd::SynthD::release_all(self);
    }
}

#[cfg(feature = "synth53")]
impl Synth for synth53::Synth53 {
    type Patch = synth53::Patch;
    const PLAIN: Self::Patch = synth53::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Synth53;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        // The instrument's reader holds each byte to its range itself, so
        // only a block of another length is no patch.
        Some(synth53::Patch::from_bytes(block.try_into().ok()?))
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        synth53::Synth53::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        synth53::Synth53::render(self, out);
    }

    fn release_all(&mut self) {
        synth53::Synth53::release_all(self);
    }
}

/// The MIDI channel, counted from 0, whose program change chooses the
/// synth8's upper patch: MIDI channel 1.
#[cfg(feature = "synth8")]
const SYNTH8_UPPER_CHANNEL: u8 = 0;
/// The controllers a synth8 block may carry beyond [`super::CONTROLLERS`],
/// which its instrument takes: portamento time, the balance of its two
/// halves, and portamento on or off.
#[cfg(feature = "synth8")]
const SYNTH8_CONTROLLERS: [u8; 3] = [5, 8, 65];

#[cfg(feature = "synth8")]
impl Synth for synth8::Synth8 {
    type Patch = synth8::Patch;
    const PLAIN: Self::Patch = synth8::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Synth8;
    /// MIDI channel 1 chooses the upper patch and channel 2 the lower.
    const PROGRAM_CHANNELS: u8 = 2;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        synth8::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        match status & MIDI_KIND {
            kind::CONTROL_CHANGE => common(status, data1, data2) || SYNTH8_CONTROLLERS.contains(&data1),
            // The instrument has two patches, so only their two channels
            // choose one.
            kind::PROGRAM_CHANGE => common(status, data1, data2) && status & MIDI_CHANNEL < Self::PROGRAM_CHANNELS,
            _ => common(status, data1, data2),
        }
    }

    fn program(&mut self, channel: u8, patch: &Self::Patch) {
        if channel == SYNTH8_UPPER_CHANNEL {
            self.set_patch(patch);
        } else {
            self.set_lower_patch(patch);
        }
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        synth8::Synth8::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        synth8::Synth8::render(self, out);
    }

    fn release_all(&mut self) {
        synth8::Synth8::release_all(self);
    }
}

#[cfg(feature = "synth80")]
impl Synth for synth80::Synth80 {
    type Patch = synth80::Patch;
    const PLAIN: Self::Patch = synth80::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Synth80;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        synth80::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        match status & MIDI_KIND {
            // The instrument's keys feel how hard each is pressed.
            kind::KEY_PRESSURE => true,
            // A message of one data byte leaves its second 0.
            kind::CHANNEL_PRESSURE => data2 == 0,
            _ => common(status, data1, data2),
        }
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        synth80::Synth80::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        synth80::Synth80::render(self, out);
    }

    fn release_all(&mut self) {
        synth80::Synth80::release_all(self);
    }
}

/// The synthesizer a lane holds: at most one, so a lane is as large as the
/// largest built in.
#[allow(
    clippy::large_enum_variant,
    reason = "a lane holds its instrument in place: there is no allocator to box it with, and every lane must have room for the largest"
)]
pub(super) enum Analogue {
    /// None yet, or the sound's instrument is not built in.
    None,
    /// A synth106 or synth60.
    #[cfg(feature = "synth106")]
    Synth106(Played<synth106::Synth106>),
    /// A synthD.
    #[cfg(feature = "synthd")]
    SynthD(Played<synthd::SynthD>),
    /// A synth53.
    #[cfg(feature = "synth53")]
    Synth53(Played<synth53::Synth53>),
    /// A synth8.
    #[cfg(feature = "synth8")]
    Synth8(Played<synth8::Synth8>),
    /// A synth80.
    #[cfg(feature = "synth80")]
    Synth80(Played<synth80::Synth80>),
}

/// Does `$then` with the synthesizer `$value` holds, whichever it is, bound
/// to `$played`; or `$otherwise` when it holds none.
macro_rules! with_synth {
    ($value:expr, $played:ident => $then:expr, $otherwise:expr) => {
        match $value {
            #[cfg(feature = "synth106")]
            Analogue::Synth106($played) => $then,
            #[cfg(feature = "synthd")]
            Analogue::SynthD($played) => $then,
            #[cfg(feature = "synth53")]
            Analogue::Synth53($played) => $then,
            #[cfg(feature = "synth8")]
            Analogue::Synth8($played) => $then,
            #[cfg(feature = "synth80")]
            Analogue::Synth80($played) => $then,
            Analogue::None => $otherwise,
        }
    };
}

impl Analogue {
    /// The synthesizer `entry`'s sound names, new, in the model or with the
    /// keyboard its OBJECT chose (`Entry::setup`), and with the patches of
    /// its bank.
    pub(super) fn of<const OBJECTS: usize>(store: &Store<'_, OBJECTS>, entry: &Entry) -> Self {
        match entry.codec {
            #[cfg(feature = "synth106")]
            Codec::Synth106 => {
                // `record::TAG_MODEL`: 0 synth106, 1 synth60.
                let [model, _] = entry.setup;
                let model = if model == 0 { synth106::Model::Synth106 } else { synth106::Model::Synth60 };
                Self::Synth106(Played::new(synth106::Synth106::new(model), store, entry))
            }
            #[cfg(feature = "synthd")]
            Codec::SynthD => Self::SynthD(Played::new(synthd::SynthD::new(), store, entry)),
            #[cfg(feature = "synth53")]
            Codec::Synth53 => {
                // `record::TAG_MODEL`: 0 synth53, 1 synth52 and synth51, one model
                // (DEC-6461 item 6).
                let [model, _] = entry.setup;
                let model = if model == 0 { synth53::Model::Synth53 } else { synth53::Model::Synth52 };
                Self::Synth53(Played::new(synth53::Synth53::new(model), store, entry))
            }
            #[cfg(feature = "synth8")]
            Codec::Synth8 => {
                let mut synth = synth8::Synth8::new();
                // `record::TAG_SYNTH8_KEYS`, checked when it was read.
                let [mode, split] = entry.setup;
                synth.set_key_mode(match mode {
                    crate::record::SYNTH8_KEYS_DUAL => synth8::KeyMode::Dual,
                    crate::record::SYNTH8_KEYS_SPLIT => synth8::KeyMode::Split,
                    _ => synth8::KeyMode::Whole,
                });
                synth.set_split(split);
                Self::Synth8(Played::new(synth, store, entry))
            }
            #[cfg(feature = "synth80")]
            Codec::Synth80 => Self::Synth80(Played::new(synth80::Synth80::new(), store, entry)),
            _ => Self::None,
        }
    }

    /// Plays one block of slots into the synthesizer and renders its 20 ms.
    pub(super) fn render(&mut self, block: &[u8], pcm: &mut [i16; BLOCK], statistics: &mut Statistics) {
        with_synth!(self, played => render_slots(played, block, pcm, statistics), pcm.fill(0));
    }

    /// Renders 20 ms for a block that never arrived, every note released
    /// first.
    pub(super) fn conceal(&mut self, pcm: &mut [i16; BLOCK]) {
        with_synth!(self, played => played.conceal(pcm), pcm.fill(0));
    }
}
