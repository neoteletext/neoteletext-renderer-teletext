//! A local sound's instrument (`neoteletext-common/docs/page-rules.md`,
//! RULE-60, RULE-62): the instrument of any family that plays notes, made
//! from what a local sound's start names (the family's codec code, its
//! variant and its option), started with the sound's banks and its setup
//! block, and then played a channel message at a time as a page's rules
//! send them.
//!
//! It is what a receiver's sound output stage holds for each local sound
//! and renders in its audio callback: [`Local`] is an
//! [`Instrument`], so it is given messages, rendered in calls of any
//! length and metered ([`Instrument::meter`], RULE-31) as every family is.
//! Nothing here allocates, waits or computes in floating point.

use crate::midi::{Banks, Instrument, Message, Meter, Outcome, Played, family, play_block, rule_message};
#[cfg(feature = "synth7")]
use crate::synth7;
#[cfg(feature = "synth8")]
use crate::synth8;
#[cfg(feature = "synth53")]
use crate::synth53;
#[cfg(feature = "synth80")]
use crate::synth80;
#[cfg(feature = "synth106")]
use crate::synth106;
#[cfg(feature = "synthd")]
use crate::synthd;

/// The instrument of one local sound: the family's, behind its front end.
#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "a local sound's instrument is held in place: the crate has no allocator to box it with, and the holder needs room for the largest"
)]
pub enum Local {
    /// synth7.
    #[cfg(feature = "synth7")]
    Synth7(synth7::midi::Midi),
    /// synth106 or synth60.
    #[cfg(feature = "synth106")]
    Synth106(synth106::midi::Midi),
    /// synthD.
    #[cfg(feature = "synthd")]
    SynthD(synthd::midi::Midi),
    /// synth53, synth52 or synth51.
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
/// bound to `$instrument`.
macro_rules! with_instrument {
    ($value:expr, $instrument:ident => $then:expr) => {
        match $value {
            #[cfg(feature = "synth7")]
            Local::Synth7($instrument) => $then,
            #[cfg(feature = "synth106")]
            Local::Synth106($instrument) => $then,
            #[cfg(feature = "synthd")]
            Local::SynthD($instrument) => $then,
            #[cfg(feature = "synth53")]
            Local::Synth53($instrument) => $then,
            #[cfg(feature = "synth8")]
            Local::Synth8($instrument) => $then,
            #[cfg(feature = "synth80")]
            Local::Synth80($instrument) => $then,
        }
    };
}

impl Local {
    /// The instrument a local sound's start names, new (MIDI-3): the family
    /// of codec code `code`, 13 to 18; its `variant` (0, or 1 for synth60
    /// and synth52, 2 for synth51); and for synth8 its key mode `option`
    /// (`synth8::interface::KEY_MODE_WHOLE` and the others) and
    /// its split point `split`, a MIDI key. `None` for the synth6581
    /// family, which has no notes and so no local sound; for a code that
    /// is no family's; for a variant or an option the family does not
    /// have; and for a family this build leaves out.
    #[must_use]
    pub fn new(code: u8, variant: u8, option: u8, split: u8) -> Option<Self> {
        // Each is read by the families that have it, where they are built.
        let _ = (option, split);
        match (code, variant) {
            #[cfg(feature = "synth7")]
            (family::SYNTH7, 0) => Some(Self::Synth7(synth7::midi::Midi::new())),
            #[cfg(feature = "synth106")]
            (family::SYNTH106, 0 | 1) => {
                let model = if variant == 0 { synth106::Model::Synth106 } else { synth106::Model::Synth60 };
                Some(Self::Synth106(synth106::midi::Midi::new(synth106::Synth106::new(model))))
            }
            #[cfg(feature = "synthd")]
            (family::SYNTHD, 0) => Some(Self::SynthD(synthd::midi::Midi::new(synthd::SynthD::new()))),
            #[cfg(feature = "synth53")]
            (family::SYNTH53, variant) => {
                let model = synth53::Model::of(synth53::interface::Variant::from_tag(variant)?);
                Some(Self::Synth53(synth53::midi::Midi::new(synth53::Synth53::new(model))))
            }
            #[cfg(feature = "synth8")]
            (family::SYNTH8, 0) => {
                let mut synth = synth8::Synth8::new();
                synth.set_key_mode(synth8::KeyMode::from_code(option)?);
                synth.set_split(split.min(crate::midi::DATA_MAX));
                Some(Self::Synth8(synth8::midi::Midi::new(synth)))
            }
            #[cfg(feature = "synth80")]
            (family::SYNTH80, 0) => Some(Self::Synth80(synth80::midi::Midi::new(synth80::Synth80::new()))),
            _ => None,
        }
    }

    /// Starts the instrument with the sound's banks and applies its setup
    /// block (RULE-60): an event list (MIDI-23) whose every event is given
    /// in order, whatever its tick, before any sample is rendered. Answers
    /// what the setup held, for the receiver's counts (MIDI-123); a patch
    /// the start refused (MIDI-63) is counted among its patches refused.
    pub fn start_with(&mut self, banks: &Banks<'_>, setup: &[u8]) -> Played {
        let started = self.start(banks);
        // With no samples to render, every event is applied at once.
        let mut played = play_block(self, setup, banks, &mut []);
        if started == Outcome::PatchRefused {
            played.patches_refused = played.patches_refused.saturating_add(1);
        }
        played
    }

    /// Gives the instrument the channel message of a rule's `midi` action
    /// (RULE-21): note off, note on, control change, program change (its
    /// second data byte is not given) or pitch bend, each data byte 0 to
    /// 127 ([`rule_message`]). Any other status, and a data byte above 127,
    /// is passed over and changes nothing.
    pub fn channel_message(&mut self, status: u8, data1: u8, data2: u8, banks: &Banks<'_>) -> Outcome {
        match rule_message(status, data1, data2) {
            Some(message) => self.midi(&Message::Channel(message), banks),
            None => Outcome::PassedOver,
        }
    }

    /// Releases every note, lifts the sustain pedal on every part and
    /// clears every parameter selection: a local sound ends "with all notes
    /// released" (RULE-60). It is [`Instrument::lost`].
    pub fn release_all(&mut self) {
        self.lost();
    }
}

impl Instrument for Local {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        with_instrument!(self, instrument => instrument.start(banks))
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        with_instrument!(self, instrument => instrument.midi(message, banks))
    }

    fn lost(&mut self) {
        with_instrument!(self, instrument => instrument.lost());
    }

    fn render(&mut self, out: &mut [i16]) {
        with_instrument!(self, instrument => instrument.render(out));
    }

    fn meter(&self, part: u8) -> Meter {
        with_instrument!(self, instrument => instrument.meter(part))
    }
}
