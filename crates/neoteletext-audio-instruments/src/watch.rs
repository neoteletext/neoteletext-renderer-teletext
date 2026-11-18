//! One synthesizer sound's state as a page's rules read it
//! (`neoteletext-common/docs/page-rules.md`, RULE-28 to RULE-30): the
//! family's tracker, which holds its patch fields, and the performance
//! state every family that plays notes shares, both fed with every message
//! given to the sound, in order.
//!
//! A receiver keeps a [`Watched`] beside each synthesizer sound whose page
//! has rules that read it (RULE-30): for a local sound it is fed the
//! sound's setup block and then every MIDI message the rules send it, so a
//! value is read with no question put to the instrument. It holds no
//! synthesis code and needs only the families' interface modules (the
//! feature `interface`, which holds every family's), so the server, which
//! turns a field's name into its number, reads the same tables through
//! [`fields`].

use crate::midi::block::{Events, Read};
use crate::midi::{Banks, Message, PatchField, Performance, class, family, rule_message};

use crate::synth7::interface as synth7;
use crate::synth8::interface as synth8;
use crate::synth53::interface as synth53;
use crate::synth80::interface as synth80;
use crate::synth106::interface as synth106;
use crate::synth6581::interface as synth6581;
use crate::synthd::interface as synthd;

/// The variant of the synth6581 family's that is synth8580, of the synth106
/// family's that is synth60 (RULE-28: "the low byte the variant").
const SECOND_VARIANT: u8 = 1;

/// The family's tracker of one sound: the state a sender tracks (MIDI-94).
#[derive(Clone, Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "a sound's tracker is held in place: the crate has no allocator to box it with, and synth7's sixteen voices are the largest"
)]
enum Tracker {
    /// The synth6581 family: the chip's registers.
    Synth6581(synth6581::Tracker),
    /// synth7: sixteen parts.
    Synth7(synth7::Tracker),
    /// synth106 or synth60: one panel.
    Synth106(synth106::Tracker),
    /// synthD: one panel.
    SynthD(synthd::Tracker),
    /// The synth53 family: one part.
    Synth53(synth53::Part),
    /// synth8: two halves.
    Synth8(synth8::Tracker),
    /// synth80: one patch.
    Synth80(synth80::Tracker),
}

/// The readable patch fields of the family of codec code `code` and variant
/// `variant` (RULE-29; appendix A of page-rules.md), a field's number being
/// its place; `None` for a code that is no family's, a variant the family
/// does not have, or a family this build leaves out.
#[must_use]
pub fn fields(code: u8, variant: u8) -> Option<&'static [PatchField]> {
    match (code, variant) {
        (family::SYNTH6581, 0 | SECOND_VARIANT) => Some(&synth6581::PARAMETERS),
        (family::SYNTH7, 0) => Some(&synth7::PARAMETERS),
        (family::SYNTH106, 0) => Some(synth106::parameters(synth106::Model::Synth106)),
        (family::SYNTH106, SECOND_VARIANT) => Some(synth106::parameters(synth106::Model::Synth60)),
        (family::SYNTHD, 0) => Some(&synthd::PARAMETERS),
        (family::SYNTH53, variant) if synth53::Variant::from_tag(variant).is_some() => Some(&synth53::PARAMETERS),
        (family::SYNTH8, 0) => Some(&synth8::PARAMETERS),
        (family::SYNTH80, 0) => Some(&synth80::PARAMETERS),
        _ => None,
    }
}

/// One synthesizer sound's state as a page's rules read it.
#[derive(Clone, Debug)]
pub struct Watched {
    code: u8,
    variant: u8,
    performance: Performance,
    tracker: Tracker,
}

impl Watched {
    /// The state of a sound of the family of codec code `code` and variant
    /// `variant` at its start, with the sound's banks (MIDI-61): `None`
    /// wherever [`fields`] is.
    #[must_use]
    pub fn new(code: u8, variant: u8, banks: &Banks<'_>) -> Option<Self> {
        fields(code, variant)?;
        let tracker = match code {
            family::SYNTH6581 => Tracker::Synth6581(synth6581::Tracker::new()),
            family::SYNTH7 => Tracker::Synth7(synth7::Tracker::new(banks)),
            family::SYNTH106 => {
                let model = if variant == 0 { synth106::Model::Synth106 } else { synth106::Model::Synth60 };
                let mut tracker = synth106::Tracker::new(model);
                tracker.start(banks);
                Tracker::Synth106(tracker)
            }
            family::SYNTHD => Tracker::SynthD(synthd::Tracker::new(banks)),
            family::SYNTH53 => {
                let mut part = synth53::Part::new(synth53::Variant::from_tag(variant)?);
                part.start(banks);
                Tracker::Synth53(part)
            }
            family::SYNTH8 => Tracker::Synth8(synth8::Tracker::new()),
            family::SYNTH80 => Tracker::Synth80(synth80::Tracker::new()),
            _ => return None,
        };
        Some(Self { code, variant, performance: Performance::new(), tracker })
    }

    /// The family's codec code.
    #[must_use]
    pub const fn code(&self) -> u8 {
        self.code
    }

    /// The variant of the family.
    #[must_use]
    pub const fn variant(&self) -> u8 {
        self.variant
    }

    /// The family's readable patch fields ([`fields`]).
    #[must_use]
    pub fn fields(&self) -> &'static [PatchField] {
        fields(self.code, self.variant).unwrap_or(&[])
    }

    /// Takes one message given to the sound, with the sound's banks for a
    /// program change. A message the family does not take changes nothing
    /// of its patch.
    pub fn take(&mut self, message: &Message<'_>, banks: &Banks<'_>) {
        if let Message::Channel(channel) = message {
            self.performance.take(*channel);
        }
        match &mut self.tracker {
            Tracker::Synth6581(tracker) => {
                if let Message::SystemExclusive(after_start) = message
                    && let Ok(asid) = synth6581::read(after_start)
                {
                    tracker.take(&asid);
                }
            }
            Tracker::Synth7(tracker) => tracker.take(message, banks),
            Tracker::Synth106(tracker) => {
                tracker.take(message, banks);
            }
            Tracker::SynthD(tracker) => tracker.take(message, banks),
            Tracker::Synth53(part) => {
                part.take(message, banks);
            }
            Tracker::Synth8(tracker) => tracker.take(message),
            Tracker::Synth80(tracker) => tracker.take(message),
        }
    }

    /// Takes every event of a block's event list, in order (MIDI-23): a
    /// local sound's setup (RULE-60), or a block the instrument was given.
    /// What the reader passes over or finds malformed is passed over here
    /// too.
    pub fn take_block(&mut self, block: &[u8], banks: &Banks<'_>) {
        for read in Events::new(block) {
            if let Read::Event(event) = read {
                self.take(&event.message, banks);
            }
        }
    }

    /// Takes the channel message of a rule's `midi` action (RULE-21): a
    /// status byte and its data bytes ([`rule_message`]), as the local
    /// sound's instrument takes it
    /// (`local::Local::channel_message`). Bytes that are none of
    /// the messages a rule may send change nothing.
    pub fn channel_message(&mut self, status: u8, data1: u8, data2: u8, banks: &Banks<'_>) {
        if let Some(message) = rule_message(status, data1, data2) {
            self.take(&Message::Channel(message), banks);
        }
    }

    /// Every note of the sound was released (RULE-60: a local sound ends
    /// "with all notes released"; MIDI-102): no key is down on any part.
    pub fn release_all(&mut self) {
        self.performance.release_all();
        match &mut self.tracker {
            Tracker::Synth106(tracker) => tracker.lost(),
            Tracker::Synth53(part) => part.lost(),
            _ => {}
        }
    }

    /// The present value of patch field `number` on part `part`, its MIDI
    /// channel less one (RULE-28 class 4): `None` for a number past the
    /// family's table or a part the family does not have. A family that is
    /// one part whatever the channel answers the same for every part;
    /// synth8's part 0 is the upper patch and 1 the lower.
    #[must_use]
    pub fn field(&self, part: u8, number: u16, banks: &Banks<'_>) -> Option<i32> {
        match &self.tracker {
            Tracker::Synth6581(tracker) => synth6581::read_parameter(tracker, number),
            Tracker::Synth7(tracker) => synth7::read_parameter(tracker, part, number),
            Tracker::Synth106(tracker) => synth106::read_parameter(tracker, number),
            Tracker::SynthD(tracker) => synthd::read_parameter(tracker, number),
            Tracker::Synth53(part) => synth53::read_parameter(part, number),
            Tracker::Synth8(tracker) => synth8::read_parameter(tracker, banks, part, number),
            Tracker::Synth80(tracker) => synth80::read_parameter(tracker, banks, number),
        }
    }

    /// The value of `class` for part `part`, its MIDI channel less one, 0
    /// to 15, and `index` (RULE-28): the classes read from what the sound
    /// was given, [`class::PROGRAM`], [`class::NOTES`], [`class::NOTE`],
    /// [`class::VELOCITY`], [`class::FIELD`], [`class::CONTROLLER`] and
    /// [`class::BEND`]. Whatever cannot be read is 0: another class (the
    /// envelope and the voice count are metered from the instrument,
    /// RULE-31), an index out of range, and, for the synth6581 family,
    /// which has no notes, every class but [`class::FIELD`].
    #[must_use]
    pub fn read(&self, class: u8, part: u8, index: u16, banks: &Banks<'_>) -> i32 {
        if class == class::FIELD {
            return self.field(part, index, banks).unwrap_or(0);
        }
        if self.code == family::SYNTH6581 {
            return 0;
        }
        self.performance.read(class, part, index).unwrap_or(0)
    }
}
