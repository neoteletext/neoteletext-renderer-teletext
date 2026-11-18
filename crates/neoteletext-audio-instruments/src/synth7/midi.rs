//! synth7's front end to the receiver (MIDI-10, [`crate::midi::Instrument`]):
//! the common layer ([`crate::midi::common`]) before the rack's own message
//! reader, and the rack's bank read from bank index 0, the DX7's 32-voice
//! bulk dump (MIDI-56; DX7 manual, page 30; [`super::Bank::from_sysex`]).
//!
//! This is the interim front end the new carriage was built with: program
//! changes 0 to 31 choose a voice of bank index 0. The rest of synth7's
//! adopted interface (`synth7.md`: the single-voice dump inline, the
//! parameter change, function parameter 65 beside RPN 0, programs 32 to 63
//! from bank index 1, the restatement) is to be built here.

use crate::midi::classic::{Classic, give};
use crate::midi::common::{Action, CENTS_PER_SEMITONE, Common, Config};
use crate::midi::{Banks, Instrument, Message, Outcome, controller, status};

use super::patch::BANK_VOICES;
use super::rack::PITCH_BEND_SEMITONES;
use super::{Bank, Rack};

/// The rack's bend at either end, in cents: [`PITCH_BEND_SEMITONES`].
const BEND_CENTS: u16 = 200;
const _: () = assert!(PITCH_BEND_SEMITONES * CENTS_PER_SEMITONE as i32 == BEND_CENTS as i32);

/// The common layer's figures for synth7.
const CONFIG: Config = Config {
    bend_range_cents: BEND_CENTS,
    most_bend_range_cents: BEND_CENTS,
    bank_select: false,
    non_registered: false,
};

/// What the rack's own reader takes besides the common layer's messages.
const CLASSIC: Classic = Classic {
    controllers: &[controller::MODULATION],
    voice_bend_range_cents: BEND_CENTS,
    reset_pedals: &[],
    expression: false,
    channel_pressure: false,
    key_pressure: false,
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

impl Midi {
    /// A rack of plain patches, every part at rest.
    #[must_use]
    pub fn new() -> Self {
        Self { rack: Rack::new(), common: Common::new(CONFIG) }
    }

    /// The rack.
    #[must_use]
    pub const fn rack(&self) -> &Rack {
        &self.rack
    }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let (bank, outcome) = match banks.get(0).map(Bank::from_sysex) {
            None => (Bank::new(), Outcome::Applied),
            Some(Ok(bank)) => (bank, Outcome::Applied),
            // A faulty dump leaves every voice the rack's plain patch.
            Some(Err(_)) => (Bank::new(), Outcome::PatchRefused),
        };
        self.rack.reset();
        self.rack.set_bank(&bank);
        outcome
    }

    fn midi(&mut self, message: &Message<'_>, _banks: &Banks<'_>) -> Outcome {
        let Message::Channel(message) = message else { return Outcome::PassedOver };
        let taken = self.common.take(*message);
        let rack = &mut self.rack;
        match taken.action {
            Action::ProgramChange { program, .. } if usize::from(program) < BANK_VOICES => {
                rack.event(status::PROGRAM_CHANGE | taken.channel, program, 0);
                Outcome::Applied
            }
            _ => give(&CLASSIC, &self.common, &taken, |status, data1, data2| rack.event(status, data1, data2)),
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
