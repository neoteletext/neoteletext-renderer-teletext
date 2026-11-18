//! synthD as the receiver plays it (MIDI-10, [`crate::midi::Instrument`]):
//! the common layer ([`crate::midi::common`]) settles what every
//! synthesizer shares, and synthD's own messages go to its voice, as
//! `synthd.md` says.
//!
//! Every channel is the one panel (SD-2): each message is taken as if on
//! channel 0. A program change sets the panel from patch `program` of bank
//! index 0, 60 bytes a patch (SD-12, SD-27); registered parameter 0 sets
//! the pitch wheel's range (SD-6); everything synthD does not take is
//! passed over and counted (SD-1).

use crate::midi::common::{Action, Common, Selected};
use crate::midi::{Banks, Instrument, Message, Outcome, RPN_PITCH_BEND_SENSITIVITY, controller, status};

use super::SynthD;
use super::interface::{self, COMMON};

/// synthD as the receiver plays it.
#[derive(Clone, Debug)]
pub struct Midi {
    synthd: SynthD,
    common: Common,
}

impl Midi {
    /// `synthd`, behind a fresh common layer.
    #[must_use]
    pub fn new(synthd: SynthD) -> Self {
        Self { synthd, common: Common::new(COMMON) }
    }

    /// The instrument.
    #[must_use]
    pub const fn synthd(&self) -> &SynthD {
        &self.synthd
    }

    /// The instrument, to set what no message sets (its drift).
    pub const fn synthd_mut(&mut self) -> &mut SynthD {
        &mut self.synthd
    }

    /// The common layer's state; its channel 0 is synthD's.
    #[must_use]
    pub const fn common(&self) -> &Common {
        &self.common
    }

    fn control(&mut self, number: u8, value: u8) {
        self.synthd.event(status::CONTROL_CHANGE, number, value);
    }
}

/// A switch pedal's value to the voice: down or up.
const fn pedal(down: bool) -> u8 {
    if down { crate::midi::DATA_MAX } else { 0 }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (values, outcome) = interface::bank_patch(banks, 0);
        self.synthd.set_values(&values);
        // A bank that lacks program 0 leaves the default, which is no
        // fault of a message (MIDI-61).
        if outcome == Outcome::PassedOver { Outcome::Applied } else { outcome }
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        // synthD takes no system exclusive (SD-8).
        let Message::Channel(message) = message else { return Outcome::PassedOver };
        let message = interface::on_one_channel(*message);
        // A data entry to no parameter changes nothing and is counted
        // (SD-6 item 3); the common layer answers it as nothing.
        let data_entry = message.kind() == status::CONTROL_CHANGE
            && matches!(
                message.data1,
                controller::DATA_ENTRY
                    | controller::DATA_ENTRY_FINE
                    | controller::DATA_INCREMENT
                    | controller::DATA_DECREMENT
            );
        let to_bend_range = self.common.channel(0).selected == Selected::Registered(RPN_PITCH_BEND_SENSITIVITY);
        if data_entry && !to_bend_range {
            let _ = self.common.take(message);
            return Outcome::PassedOver;
        }
        let taken = self.common.take(message);
        match taken.action {
            Action::None => {}
            Action::NoteOn { key, velocity } => self.synthd.event(status::NOTE_ON, key, velocity),
            Action::NoteOff { key, velocity } => self.synthd.event(status::NOTE_OFF, key, velocity),
            Action::Sustain { down } => self.control(controller::SUSTAIN, pedal(down)),
            Action::AllNotesOff => self.control(controller::ALL_NOTES_OFF, 0),
            Action::AllSoundOff => self.control(controller::ALL_SOUND_OFF, 0),
            Action::ResetAllControllers => self.control(controller::RESET_ALL_CONTROLLERS, 0),
            Action::PitchBend { .. } => {
                self.synthd.event(message.status, message.data1, message.data2);
            }
            Action::BendRange { cents } => self.synthd.set_bend_range_cents(cents),
            Action::Volume { value } => self.control(controller::VOLUME, value),
            Action::ProgramChange { program, .. } => {
                let (values, outcome) = interface::bank_patch(banks, program);
                self.synthd.set_values(&values);
                return outcome;
            }
            Action::Control { controller: number, value } if interface::takes_controller(number) => {
                self.control(number, value);
            }
            Action::PassedOver
            | Action::ChannelPressure { .. }
            | Action::KeyPressure { .. }
            | Action::Control { .. }
            | Action::Registered { .. }
            | Action::NonRegistered { .. } => return Outcome::PassedOver,
        }
        Outcome::Applied
    }

    fn lost(&mut self) {
        // SD-25: the note released, the pedal lifted, no parameter
        // selected; the panel, the wheels, the range and volume stay.
        self.common.lost();
        self.synthd.release_all();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synthd.render(out);
    }
}
