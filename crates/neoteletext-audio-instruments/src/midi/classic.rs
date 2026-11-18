//! The bridge from the common layer ([`super::common`]) to an instrument
//! built before it, which takes three-byte channel messages through its
//! own `event(status, data1, data2)` and bends by a fixed range.
//!
//! The common layer settles each common behaviour once (MIDI-84 to
//! MIDI-92); this bridge then gives the instrument only messages in the
//! one form it already reads the same way: a sustain pedal of 0 or 127,
//! all notes off for controllers 123 to 127, reset all controllers as the
//! control changes RP-015 sets, and a pitch bend scaled so that the
//! instrument's fixed range bends by the channel's range of registered
//! parameter 0. A family whose voices take the common layer's actions
//! directly does not need it.

use super::common::{Action, Common, EXPRESSION_FULL, Taken, bend_for_fixed_range};
use super::{DATA_MAX, Outcome, PITCH_BEND_CENTRE, controller, status};

/// What an instrument built before the common layer takes, as its family
/// sets it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Classic {
    /// The controllers the instrument's own `event` acts on, besides those
    /// the common layer settles (7, 64, 120, 121 and 123 to 127): every
    /// other control change is passed over and counted (MIDI-10).
    pub controllers: &'static [u8],
    /// The range of a full bend of the instrument's voices, in cents.
    pub voice_bend_range_cents: u16,
    /// The pedals among 65, 66 and 67 the instrument responds to as
    /// RP-015's pedals, which reset all controllers sets to 0 (MIDI-84
    /// item 2). Controller 64 is always set to 0.
    pub reset_pedals: &'static [u8],
    /// Whether the instrument takes expression, controller 11, which reset
    /// all controllers sets to 127.
    pub expression: bool,
    /// Whether the instrument takes channel pressure.
    pub channel_pressure: bool,
    /// Whether the instrument takes polyphonic key pressure.
    pub key_pressure: bool,
}

/// Keys, for setting every key's pressure to 0 on a reset.
const KEYS: u8 = DATA_MAX;

/// Gives the instrument, through `event`, what the common layer answered
/// for one message. A program change, and a data entry to a parameter the
/// common layer does not know, are the family's own and are answered
/// [`Outcome::PassedOver`] here: the family takes them before calling this.
pub fn give(classic: &Classic, common: &Common, taken: &Taken, mut event: impl FnMut(u8, u8, u8)) -> Outcome {
    let channel = taken.channel;
    let control = |number: u8, value: u8| (status::CONTROL_CHANGE | channel, number, value);
    let mut send = |(status, data1, data2): (u8, u8, u8)| event(status, data1, data2);
    match taken.action {
        Action::None => {}
        Action::NoteOn { key, velocity } => send((status::NOTE_ON | channel, key, velocity)),
        Action::NoteOff { key, velocity } => send((status::NOTE_OFF | channel, key, velocity)),
        Action::Sustain { down } => send(control(controller::SUSTAIN, if down { DATA_MAX } else { 0 })),
        Action::AllNotesOff => send(control(controller::ALL_NOTES_OFF, 0)),
        Action::AllSoundOff => send(control(controller::ALL_SOUND_OFF, 0)),
        Action::ResetAllControllers => {
            send(bend(PITCH_BEND_CENTRE, channel));
            if classic.controllers.contains(&controller::MODULATION) {
                send(control(controller::MODULATION, 0));
            }
            if classic.expression {
                send(control(controller::EXPRESSION, EXPRESSION_FULL));
            }
            send(control(controller::SUSTAIN, 0));
            for &pedal in classic.reset_pedals {
                send(control(pedal, 0));
            }
            if classic.channel_pressure {
                send((status::CHANNEL_PRESSURE | channel, 0, 0));
            }
            if classic.key_pressure {
                for key in 0..=KEYS {
                    send((status::KEY_PRESSURE | channel, key, 0));
                }
            }
        }
        Action::PitchBend { .. } | Action::BendRange { .. } => {
            let value = bend_for_fixed_range(common.bend_cents(channel), classic.voice_bend_range_cents);
            send(bend(value, channel));
        }
        Action::Volume { value } => send(control(controller::VOLUME, value)),
        Action::ChannelPressure { pressure } if classic.channel_pressure => {
            send((status::CHANNEL_PRESSURE | channel, pressure, 0));
        }
        Action::KeyPressure { key, pressure } if classic.key_pressure => {
            send((status::KEY_PRESSURE | channel, key, pressure));
        }
        Action::Control { controller, value } if classic.controllers.contains(&controller) => {
            send(control(controller, value));
        }
        Action::PassedOver
        | Action::ChannelPressure { .. }
        | Action::KeyPressure { .. }
        | Action::Control { .. }
        | Action::ProgramChange { .. }
        | Action::Registered { .. }
        | Action::NonRegistered { .. } => return Outcome::PassedOver,
    }
    Outcome::Applied
}

/// A pitch bend message of a 14-bit value.
fn bend(value: u16, channel: u8) -> (u8, u8, u8) {
    let low = u8::try_from(value & u16::from(DATA_MAX)).unwrap_or(0);
    let high = u8::try_from((value >> 7) & u16::from(DATA_MAX)).unwrap_or(0);
    (status::PITCH_BEND | channel, low, high)
}

/// An instrument built before the common layer, as its family's interim
/// front end gives it to the receiver: three-byte messages to its own
/// `event`, and patches of the instrument's own byte layout, which a
/// bank image holds as fixed patches (MIDI-56 form 2) until the family's
/// adopted patch form is built.
pub trait ClassicFamily {
    /// The instrument's patch, checked.
    type Patch: Copy;
    /// The family's figures for the common layer.
    const CONFIG: super::common::Config;
    /// What the instrument's own `event` takes.
    const CLASSIC: Classic;
    /// The bytes of one patch in a bank image.
    const PATCH_BYTES: usize;
    /// The channels, counted from 0, whose program changes choose a patch:
    /// 16 for an instrument of one part.
    const PROGRAM_CHANNELS: u8;

    /// The patch the instrument plays where its bank has none, or one that
    /// is faulty (MIDI-61, MIDI-63).
    fn default_patch(&self) -> Self::Patch;
    /// The patch `bytes` hold, or `None` for a faulty one.
    fn read_patch(bytes: &[u8]) -> Option<Self::Patch>;
    /// Sets the patch of the part `channel` addresses, below
    /// [`ClassicFamily::PROGRAM_CHANNELS`].
    fn set_patch(&mut self, channel: u8, patch: &Self::Patch);
    /// The instrument's own message reader.
    fn event(&mut self, status: u8, data1: u8, data2: u8);
    /// Releases every note and lifts the pedal: no note off will come.
    fn release_all(&mut self);
    /// Renders the next samples.
    fn render(&mut self, out: &mut [i16]);
}

/// A [`ClassicFamily`] instrument behind the common layer: the
/// [`super::Instrument`] the receiver plays.
#[derive(Clone, Debug)]
pub struct Fronted<F> {
    family: F,
    common: Common,
}

impl<F: ClassicFamily> Fronted<F> {
    /// `family`, new, behind a fresh common layer.
    pub fn new(family: F) -> Self {
        Self { family, common: Common::new(F::CONFIG) }
    }

    /// The instrument.
    pub const fn family(&self) -> &F {
        &self.family
    }

    /// The common layer's state.
    pub const fn common(&self) -> &Common {
        &self.common
    }

    /// The patch of `program` of bank index 0, or the default where the
    /// bank has none there ([`Outcome::PassedOver`], MIDI-59) or it is
    /// faulty ([`Outcome::PatchRefused`], MIDI-63).
    fn patch(&self, banks: &super::Banks<'_>, program: u8) -> (F::Patch, Outcome) {
        match banks.fixed_patch(0, usize::from(program), F::PATCH_BYTES) {
            None => (self.family.default_patch(), Outcome::PassedOver),
            Some(bytes) => match F::read_patch(bytes) {
                Some(patch) => (patch, Outcome::Applied),
                None => (self.family.default_patch(), Outcome::PatchRefused),
            },
        }
    }
}

impl<F: ClassicFamily> super::Instrument for Fronted<F> {
    fn start(&mut self, banks: &super::Banks<'_>) -> Outcome {
        let (patch, outcome) = self.patch(banks, 0);
        for channel in 0..F::PROGRAM_CHANNELS {
            self.family.set_patch(channel, &patch);
        }
        // A bank that lacks program 0 leaves the default, which is no
        // fault of a message.
        if outcome == Outcome::PassedOver { Outcome::Applied } else { outcome }
    }

    fn midi(&mut self, message: &super::Message<'_>, banks: &super::Banks<'_>) -> Outcome {
        // No interim family takes system exclusive.
        let super::Message::Channel(message) = message else { return Outcome::PassedOver };
        let taken = self.common.take(*message);
        if let Action::ProgramChange { program, .. } = taken.action {
            if taken.channel >= F::PROGRAM_CHANNELS {
                return Outcome::PassedOver;
            }
            let (patch, outcome) = self.patch(banks, program);
            self.family.set_patch(taken.channel, &patch);
            return outcome;
        }
        let family = &mut self.family;
        give(&F::CLASSIC, &self.common, &taken, |status, data1, data2| family.event(status, data1, data2))
    }

    fn lost(&mut self) {
        self.common.lost();
        self.family.release_all();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.family.render(out);
    }
}
