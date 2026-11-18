//! synth8 as the receiver plays it ([`crate::midi::Instrument`]): every
//! channel message to [`Synth8::event`], and program changes on channels 1
//! and 2 read from bank index 0, whose image is 1 to 64 patches of 42
//! bytes (`synth8.md`, S8-12, S8-29 (a); MIDI-56 form 2).

use crate::midi::{Banks, Instrument, Message, Outcome, status};

use super::interface::{BANK_INDEX, HALF_CHANNELS, MOST_PATCHES};
use super::{PATCH_BYTES, Patch, Synth8};

/// synth8 behind its front end to the receiver.
#[derive(Clone, Debug)]
pub struct Midi {
    synth: Synth8,
}

impl Midi {
    /// `synth`, its key mode and split point set from the sound's OBJECT
    /// (S8-18), to be played.
    #[must_use]
    pub const fn new(synth: Synth8) -> Self {
        Self { synth }
    }

    /// The instrument.
    #[must_use]
    pub const fn synth(&self) -> &Synth8 {
        &self.synth
    }

    /// The patch of `program` of bank index 0 (S8-12, S8-29): the default
    /// where the bank holds no patch there, program 64 to 127 among them
    /// ([`Outcome::PassedOver`], MIDI-59), or holds a faulty one
    /// ([`Outcome::PatchRefused`], S8-23, MIDI-63).
    fn patch(banks: &Banks<'_>, program: u8) -> (Patch, Outcome) {
        if usize::from(program) >= MOST_PATCHES {
            return (Patch::DEFAULT, Outcome::PassedOver);
        }
        match banks.fixed_patch(BANK_INDEX, usize::from(program), PATCH_BYTES) {
            None => (Patch::DEFAULT, Outcome::PassedOver),
            Some(bytes) => match Patch::from_slice(bytes) {
                Ok(patch) => (patch, Outcome::Applied),
                Err(_) => (Patch::DEFAULT, Outcome::PatchRefused),
            },
        }
    }
}

impl Instrument for Midi {
    /// Both halves at program 0 of bank index 0, or the default patch where
    /// it is missing or refused (S8-12, MIDI-61).
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (patch, outcome) = Self::patch(banks, 0);
        for channel in 0..HALF_CHANNELS {
            self.synth.program(channel, &patch);
        }
        // A bank that lacks program 0 leaves the default, which is no
        // fault of a message.
        if outcome == Outcome::PassedOver { Outcome::Applied } else { outcome }
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        // synth8 takes no system exclusive (S8-1, S8-29 (c); MIDI-34).
        let Message::Channel(message) = message else {
            return self.synth.event(status::SYSTEM_EXCLUSIVE, 0, 0);
        };
        if message.kind() != status::PROGRAM_CHANGE {
            return self.synth.event(message.status, message.data1, message.data2);
        }
        if message.channel() >= HALF_CHANNELS || message.data1 > crate::midi::DATA_MAX {
            return self.synth.event(message.status, message.data1, message.data2);
        }
        let (patch, outcome) = Self::patch(banks, message.data1);
        self.synth.program(message.channel(), &patch);
        outcome
    }

    fn lost(&mut self) {
        self.synth.lost();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synth.render(out);
    }
}
