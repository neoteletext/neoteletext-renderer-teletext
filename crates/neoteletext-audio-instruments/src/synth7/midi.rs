//! synth7 as the receiver plays it ([`crate::midi::Instrument`]): the
//! common layer ([`crate::midi::common`]) for the behaviours every
//! synthesizer shares, then synth7's own interface (`synth7.md`, cited as
//! "S7-n"): the sixteen parts (S7-1), the controllers of S7-10, channel
//! pressure (S7-22), program changes 0 to 63 from the sound's first and
//! second banks (S7-50), the single-voice dump and the parameter change
//! (S7-37 to S7-45), and the pitch bend range set by function parameter 65
//! and by registered parameter number 0 alike, the later holding (S7-9).

use crate::midi::common::{Action, CENTS_PER_SEMITONE, Common, Config, Selected};
use crate::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, RPN_PITCH_BEND_SENSITIVITY, controller};

use super::functions::{DEFAULT_BEND_SEMITONES, MAX_BEND_SEMITONES, number};
use super::interface::{check_voice_event, program_voice};
use super::rack::{PARTS, Taken};
use super::sysex::{Kind, Parameter, ParameterChange, classify_event, read_parameter_change_event};
use super::{Functions, Patch, Rack};

/// The common layer's figures for synth7: a bend of 2 semitones at the
/// start, 12 the most (S7-9, S7-35), no bank select (MIDI-64) and no
/// non-registered parameter (S7-10).
const CONFIG: Config = Config {
    bend_range_cents: DEFAULT_BEND_SEMITONES as u16 * CENTS_PER_SEMITONE,
    most_bend_range_cents: MAX_BEND_SEMITONES as u16 * CENTS_PER_SEMITONE,
    bank_select: false,
    non_registered: false,
};

/// synth7 as the receiver plays it: the rack and its common layer.
#[derive(Clone, Debug)]
pub struct Midi {
    rack: Rack,
    common: Common,
}

impl Default for Midi {
    fn default() -> Self {
        Self::new()
    }
}

/// The rack's answer as the receiver counts it.
const fn outcome(taken: Taken) -> Outcome {
    match taken {
        Taken::Applied => Outcome::Applied,
        Taken::Ignored => Outcome::PassedOver,
    }
}

impl Midi {
    /// A rack of default voices, every part at rest.
    #[must_use]
    pub fn new() -> Self {
        Self { rack: Rack::new(), common: Common::new(CONFIG) }
    }

    /// The rack.
    #[must_use]
    pub const fn rack(&self) -> &Rack {
        &self.rack
    }

    /// The common layer's state.
    #[must_use]
    pub const fn common(&self) -> &Common {
        &self.common
    }

    /// Controllers 38, 96 and 97 while registered parameter number 0 is
    /// selected: the range is held in whole semitones, as function
    /// parameter 65 holds it, so 38 changes nothing and 96 and 97 move it
    /// by a semitone (S7-9). `None` for any other message, which the
    /// common layer takes.
    fn bend_range_entry(&mut self, message: ChannelMessage) -> Option<Outcome> {
        let channel = message.channel();
        if message.kind() != crate::midi::status::CONTROL_CHANGE
            || self.common.channel(channel).selected != Selected::Registered(RPN_PITCH_BEND_SENSITIVITY)
        {
            return None;
        }
        let range = self.rack.functions(channel).map_or(DEFAULT_BEND_SEMITONES, Functions::bend_range);
        let wanted = match message.data1 {
            controller::DATA_ENTRY_FINE => return Some(Outcome::Applied),
            controller::DATA_INCREMENT => range.saturating_add(1).min(MAX_BEND_SEMITONES),
            controller::DATA_DECREMENT => range.saturating_sub(1),
            _ => return None,
        };
        Some(outcome(self.rack.set_function(channel, number::PITCH_BEND_RANGE, wanted)))
    }

    fn channel_message(&mut self, message: ChannelMessage, banks: &Banks<'_>) -> Outcome {
        if let Some(outcome) = self.bend_range_entry(message) {
            return outcome;
        }
        let taken = self.common.take(message);
        let channel = taken.channel;
        let rack = &mut self.rack;
        match taken.action {
            Action::None => Outcome::Applied,
            Action::NoteOn { key, velocity } => outcome(rack.note_on(channel, key, velocity)),
            // S7-7: the release velocity changes nothing.
            Action::NoteOff { key, .. } => outcome(rack.note_off(channel, key)),
            Action::Sustain { down } => outcome(rack.sustain(channel, down)),
            Action::AllNotesOff => outcome(rack.all_notes_off(channel)),
            Action::AllSoundOff => outcome(rack.all_sound_off(channel)),
            Action::ResetAllControllers => outcome(rack.reset_controllers(channel)),
            Action::PitchBend { value } => outcome(rack.pitch_bend(channel, value)),
            // Registered parameter number 0's data entry sets the range in
            // semitones, at most 12, as function parameter 65 (S7-9).
            Action::BendRange { cents } => {
                let semitones = u8::try_from(cents / CENTS_PER_SEMITONE).unwrap_or(MAX_BEND_SEMITONES);
                outcome(rack.set_function(channel, number::PITCH_BEND_RANGE, semitones.min(MAX_BEND_SEMITONES)))
            }
            Action::Volume { value } => outcome(rack.control_change(channel, controller::VOLUME, value)),
            Action::ChannelPressure { pressure } => outcome(rack.channel_pressure(channel, pressure)),
            // S7-21, S7-50.
            Action::ProgramChange { program, .. } => {
                let (voice, found) = program_voice(banks, program);
                rack.program_change(channel, program, &voice);
                found
            }
            Action::Control { controller, value } => outcome(rack.control_change(channel, controller, value)),
            // S7-23: no polyphonic key pressure; S7-10: no registered
            // parameter but 0 and no non-registered parameter.
            Action::KeyPressure { .. }
            | Action::Registered { .. }
            | Action::NonRegistered { .. }
            | Action::PassedOver => Outcome::PassedOver,
        }
    }

    /// A system exclusive message (S7-37 to S7-45).
    fn system_exclusive(&mut self, after_start: &[u8]) -> Outcome {
        let (kind, channel) = classify_event(after_start);
        match kind {
            // S7-40: a whole, correct dump replaces the part's current
            // voice. One that begins as a dump but breaks its rules sets
            // the default voice and is counted as a refused patch (MIDI-63).
            Kind::VoiceDump => {
                if let Ok((channel, voice)) = check_voice_event(after_start) {
                    outcome(self.rack.set_voice(channel, &voice))
                } else {
                    self.rack.set_voice(channel, &Patch::DEFAULT);
                    Outcome::PatchRefused
                }
            }
            // S7-43, S7-44: a whole parameter change in range; anything else
            // changes nothing and is counted (S7-29, S7-45).
            Kind::ParameterChange => match read_parameter_change_event(after_start) {
                Ok(ParameterChange { channel, parameter: Parameter::Voice(number), value }) => {
                    outcome(self.rack.set_voice_parameter(channel, number, value))
                }
                Ok(ParameterChange { channel, parameter: Parameter::Function(number), value }) => {
                    outcome(self.rack.set_function(channel, number, value))
                }
                Err(_) => Outcome::PassedOver,
            },
            // S7-37: a 32-voice dump travels only as a bank (MIDI-28,
            // MIDI-34); no other system exclusive message is synth7's.
            Kind::BankDump | Kind::Other => Outcome::PassedOver,
        }
    }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        self.rack.reset();
        self.common = Common::new(CONFIG);
        // S7-54, MIDI-61: every part at program 0 of the first bank, or the
        // default voice. A bank that lacks it is no fault of a message.
        let (voice, found) = program_voice(banks, 0);
        for part in (0..PARTS).filter_map(|part| u8::try_from(part).ok()) {
            self.rack.program_change(part, 0, &voice);
        }
        if found == Outcome::PassedOver { Outcome::Applied } else { found }
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        match message {
            Message::Channel(message) => self.channel_message(*message, banks),
            Message::SystemExclusive(after_start) => self.system_exclusive(after_start),
        }
    }

    fn lost(&mut self) {
        self.common.lost();
        self.rack.release_all();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.rack.render(out);
    }
}
