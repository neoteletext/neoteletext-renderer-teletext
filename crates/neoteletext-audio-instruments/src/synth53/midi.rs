//! synth53, synth52 and synth51 as the receiver plays them
//! ([`crate::midi::Instrument`]): the interface's part
//! ([`super::interface::Part`]) moves the edit buffer, the group, the
//! selection and the controllers exactly as the server's tracker does, and
//! the instrument is told what each message changed
//! (`neoteletext-common/docs/audio/synth53-synth52-synth51.md`, S53-1 to
//! S53-46).

use crate::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, controller, status};

use super::Synth53;
use super::interface::{Change, Part};

/// The instrument as the receiver plays it.
#[derive(Clone, Debug)]
pub struct Midi {
    part: Part,
    synth: Synth53,
}

impl Midi {
    /// `synth` behind the interface, whose kind is its model's (S53-1).
    #[must_use]
    pub fn new(synth: Synth53) -> Self {
        Self { part: Part::new(synth.model().variant()), synth }
    }

    /// The instrument.
    #[must_use]
    pub const fn family(&self) -> &Synth53 {
        &self.synth
    }

    /// The interface's state of the part.
    #[must_use]
    pub const fn part(&self) -> &Part {
        &self.part
    }

    /// A control change for the instrument's own reader, on channel 0: the
    /// instrument hears every channel alike (S53-3).
    fn control(&mut self, number: u8, value: u8) {
        let ChannelMessage { status, data1, data2 } = ChannelMessage::new(status::CONTROL_CHANGE, 0, number, value);
        self.synth.event(status, data1, data2);
    }

    /// Gives the instrument what one message changed.
    fn apply(&mut self, change: Change) {
        match change {
            Change::None | Change::PassedOver => {}
            Change::NoteOn { key } => {
                // S53-4: velocity does not change the sound.
                let ChannelMessage { status, data1, data2 } =
                    ChannelMessage::new(status::NOTE_ON, 0, key, crate::midi::DATA_MAX);
                self.synth.event(status, data1, data2);
            }
            Change::NoteOff { key } => {
                let ChannelMessage { status, data1, data2 } = ChannelMessage::new(status::NOTE_OFF, 0, key, 0);
                self.synth.event(status, data1, data2);
            }
            Change::Sustain { down } => self.control(controller::SUSTAIN, if down { crate::midi::DATA_MAX } else { 0 }),
            Change::AllNotesOff => self.control(controller::ALL_NOTES_OFF, 0),
            Change::AllSoundOff => self.control(controller::ALL_SOUND_OFF, 0),
            // The wheel to 0, the pedal up and the pitch wheel centred
            // (S53-16): what the instrument's own controller 121 does.
            Change::ResetAllControllers => self.control(controller::RESET_ALL_CONTROLLERS, 0),
            Change::PitchBend { value } => {
                let bend = ChannelMessage::new(
                    status::PITCH_BEND,
                    0,
                    u8::try_from(value & u16::from(crate::midi::DATA_MAX)).unwrap_or(0),
                    u8::try_from(value >> 7).unwrap_or(0),
                );
                self.synth.event(bend.status, bend.data1, bend.data2);
            }
            Change::BendRange => {
                self.synth.set_bend_range_cents(self.part.bend_range_cents());
                // Parameter 86 is a parameter of the edit buffer as well.
                self.synth.set_program(self.part.program());
            }
            Change::Volume { value } => self.control(controller::VOLUME, value),
            Change::Modulation { value } => self.control(controller::MODULATION, value),
            Change::Brightness { value } => self.synth.set_brightness(value),
            Change::Program { .. } | Change::Parameter { .. } => self.synth.set_program(self.part.program()),
        }
    }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let outcome = self.part.start(banks);
        self.synth.set_program(self.part.program());
        outcome
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        let change = self.part.take(message, banks);
        self.apply(change);
        change.outcome()
    }

    fn lost(&mut self) {
        self.part.lost();
        self.synth.release_all();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synth.render(out);
    }
}
