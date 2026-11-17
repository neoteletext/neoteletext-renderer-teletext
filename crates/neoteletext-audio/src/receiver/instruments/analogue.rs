//! The analogue synthesizers as instruments of a lane (DEC-6429 foundations
//! item 4): the Juno, the Minimoog, the Prophet-5, the Jupiter-8 and the
//! CS-80, each behind its feature. A sound of one is the DX7's block, slots
//! of timed MIDI messages, under the instrument's own codec, and its
//! patches come from a patch bank of the instrument's family, read when the
//! sound starts, as a DX7 sound's are.
//!
//! None of these instruments knows of a bank, so a program change is taken
//! here: it gives the instrument the patch of that number from the copy of
//! the bank the lane holds. Until one comes the instrument plays patch 0.
//! A lane holds one synthesizer at a time, so it is as large as the largest
//! built in and no larger.
#![cfg_attr(
    not(any(feature = "juno", feature = "minimoog", feature = "prophet5", feature = "jupiter8", feature = "cs80")),
    allow(
        dead_code,
        unused_imports,
        unused_variables,
        reason = "the feature `analog` alone builds what the synthesizers share and no synthesizer to use it"
    )
)]

#[cfg(feature = "cs80")]
use neoteletext_audio_instruments::cs80;
#[cfg(feature = "juno")]
use neoteletext_audio_instruments::juno;
#[cfg(feature = "jupiter8")]
use neoteletext_audio_instruments::jupiter8;
#[cfg(feature = "minimoog")]
use neoteletext_audio_instruments::minimoog;
#[cfg(feature = "prophet5")]
use neoteletext_audio_instruments::prophet5;

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
        let mut played = Self { synth, patches: bank::<S, OBJECTS>(store, entry.param) };
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
/// bank costs a sound its timbre and nothing else, as a faulty DX7 bank
/// does (REQ-6411 item 4, DEC-6421 item 4).
fn bank<S: Synth, const OBJECTS: usize>(store: &Store<'_, OBJECTS>, handle: u8) -> [S::Patch; BANK] {
    let mut patches = [S::PLAIN; BANK];
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

#[cfg(feature = "juno")]
impl Synth for juno::Juno {
    type Patch = juno::Patch;
    const PLAIN: Self::Patch = juno::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Juno;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        juno::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        juno::Juno::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        juno::Juno::render(self, out);
    }

    fn release_all(&mut self) {
        juno::Juno::release_all(self);
    }
}

#[cfg(feature = "minimoog")]
impl Synth for minimoog::Minimoog {
    type Patch = minimoog::Patch;
    const PLAIN: Self::Patch = minimoog::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Minimoog;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        minimoog::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        minimoog::Minimoog::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        minimoog::Minimoog::render(self, out);
    }

    fn release_all(&mut self) {
        minimoog::Minimoog::release_all(self);
    }
}

#[cfg(feature = "prophet5")]
impl Synth for prophet5::Prophet5 {
    type Patch = prophet5::Patch;
    const PLAIN: Self::Patch = prophet5::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Prophet5;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        // The instrument's reader holds each byte to its range itself, so
        // only a block of another length is no patch.
        Some(prophet5::Patch::from_bytes(block.try_into().ok()?))
    }

    fn program(&mut self, _channel: u8, patch: &Self::Patch) {
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        prophet5::Prophet5::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        prophet5::Prophet5::render(self, out);
    }

    fn release_all(&mut self) {
        prophet5::Prophet5::release_all(self);
    }
}

/// The MIDI channel, counted from 0, whose program change chooses the
/// Jupiter-8's upper patch: MIDI channel 1.
#[cfg(feature = "jupiter8")]
const JUPITER8_UPPER_CHANNEL: u8 = 0;
/// The controllers a Jupiter-8 block may carry beyond [`super::CONTROLLERS`],
/// which its instrument takes: portamento time, the balance of its two
/// halves, and portamento on or off.
#[cfg(feature = "jupiter8")]
const JUPITER8_CONTROLLERS: [u8; 3] = [5, 8, 65];

#[cfg(feature = "jupiter8")]
impl Synth for jupiter8::Jupiter8 {
    type Patch = jupiter8::Patch;
    const PLAIN: Self::Patch = jupiter8::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Jupiter8;
    /// MIDI channel 1 chooses the upper patch and channel 2 the lower.
    const PROGRAM_CHANNELS: u8 = 2;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        jupiter8::Patch::from_bytes(block.try_into().ok()?).ok()
    }

    fn takes(status: u8, data1: u8, data2: u8) -> bool {
        match status & MIDI_KIND {
            kind::CONTROL_CHANGE => common(status, data1, data2) || JUPITER8_CONTROLLERS.contains(&data1),
            // The instrument has two patches, so only their two channels
            // choose one.
            kind::PROGRAM_CHANGE => common(status, data1, data2) && status & MIDI_CHANNEL < Self::PROGRAM_CHANNELS,
            _ => common(status, data1, data2),
        }
    }

    fn program(&mut self, channel: u8, patch: &Self::Patch) {
        if channel == JUPITER8_UPPER_CHANNEL {
            self.set_patch(patch);
        } else {
            self.set_lower_patch(patch);
        }
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        jupiter8::Jupiter8::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        jupiter8::Jupiter8::render(self, out);
    }

    fn release_all(&mut self) {
        jupiter8::Jupiter8::release_all(self);
    }
}

#[cfg(feature = "cs80")]
impl Synth for cs80::Cs80 {
    type Patch = cs80::Patch;
    const PLAIN: Self::Patch = cs80::Patch::DEFAULT;
    const FAMILY: Codec = Codec::Cs80;

    fn patch(block: &[u8]) -> Option<Self::Patch> {
        cs80::Patch::from_bytes(block.try_into().ok()?).ok()
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
        cs80::Cs80::event(self, status, data1, data2);
    }

    fn render(&mut self, out: &mut [i16]) {
        cs80::Cs80::render(self, out);
    }

    fn release_all(&mut self) {
        cs80::Cs80::release_all(self);
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
    /// A Juno-106 or Juno-60.
    #[cfg(feature = "juno")]
    Juno(Played<juno::Juno>),
    /// A Minimoog.
    #[cfg(feature = "minimoog")]
    Minimoog(Played<minimoog::Minimoog>),
    /// A Prophet-5.
    #[cfg(feature = "prophet5")]
    Prophet5(Played<prophet5::Prophet5>),
    /// A Jupiter-8.
    #[cfg(feature = "jupiter8")]
    Jupiter8(Played<jupiter8::Jupiter8>),
    /// A CS-80.
    #[cfg(feature = "cs80")]
    Cs80(Played<cs80::Cs80>),
}

/// Does `$then` with the synthesizer `$value` holds, whichever it is, bound
/// to `$played`; or `$otherwise` when it holds none.
macro_rules! with_synth {
    ($value:expr, $played:ident => $then:expr, $otherwise:expr) => {
        match $value {
            #[cfg(feature = "juno")]
            Analogue::Juno($played) => $then,
            #[cfg(feature = "minimoog")]
            Analogue::Minimoog($played) => $then,
            #[cfg(feature = "prophet5")]
            Analogue::Prophet5($played) => $then,
            #[cfg(feature = "jupiter8")]
            Analogue::Jupiter8($played) => $then,
            #[cfg(feature = "cs80")]
            Analogue::Cs80($played) => $then,
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
            #[cfg(feature = "juno")]
            Codec::Juno => {
                // `record::TAG_MODEL`: 0 the Juno-106, 1 the Juno-60.
                let [model, _] = entry.setup;
                let model = if model == 0 { juno::Model::Juno106 } else { juno::Model::Juno60 };
                Self::Juno(Played::new(juno::Juno::new(model), store, entry))
            }
            #[cfg(feature = "minimoog")]
            Codec::Minimoog => Self::Minimoog(Played::new(minimoog::Minimoog::new(), store, entry)),
            #[cfg(feature = "prophet5")]
            Codec::Prophet5 => {
                // `record::TAG_MODEL`: 0 revision 3, 1 revisions 1 and 2.
                let [model, _] = entry.setup;
                let model = if model == 0 { prophet5::Model::Rev3 } else { prophet5::Model::Rev2 };
                Self::Prophet5(Played::new(prophet5::Prophet5::new(model), store, entry))
            }
            #[cfg(feature = "jupiter8")]
            Codec::Jupiter8 => {
                let mut synth = jupiter8::Jupiter8::new();
                // `record::TAG_JUPITER8_KEYS`, checked when it was read.
                let [mode, split] = entry.setup;
                synth.set_key_mode(match mode {
                    crate::record::JUPITER8_KEYS_DUAL => jupiter8::KeyMode::Dual,
                    crate::record::JUPITER8_KEYS_SPLIT => jupiter8::KeyMode::Split,
                    _ => jupiter8::KeyMode::Whole,
                });
                synth.set_split(split);
                Self::Jupiter8(Played::new(synth, store, entry))
            }
            #[cfg(feature = "cs80")]
            Codec::Cs80 => Self::Cs80(Played::new(cs80::Cs80::new(), store, entry)),
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
