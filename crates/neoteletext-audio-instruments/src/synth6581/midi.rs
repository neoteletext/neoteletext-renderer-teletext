//! synth6581's and synth8580's front end to the receiver
//! ([`crate::midi::Instrument`]): the chip, which takes no channel message
//! (S6581-26) and has no bank (S6581-51).
//!
//! This is the interim front end the new carriage was built with: it
//! passes over every message. The family's adopted interface, ASID's
//! register data message (`synth6581-synth8580.md`, S6581-9 to S6581-17)
//! and its full register update as the patch (S6581-50), is to be built
//! here.

use crate::midi::{Banks, Instrument, Message, Outcome};

use super::{ChipClock, Synth6581, Variant};

/// synth6581 or synth8580 as the receiver plays it.
#[derive(Clone, Debug)]
pub struct Midi {
    chip: Synth6581,
}

impl Midi {
    /// A chip of `model` on `clock`, reset.
    #[must_use]
    pub const fn new(model: Variant, clock: ChipClock) -> Self {
        Self { chip: Synth6581::new(model, clock) }
    }

    /// The chip.
    #[must_use]
    pub const fn chip(&self) -> &Synth6581 {
        &self.chip
    }
}

impl Instrument for Midi {
    fn start(&mut self, _banks: &Banks<'_>) -> Outcome {
        Outcome::Applied
    }

    fn midi(&mut self, _message: &Message<'_>, _banks: &Banks<'_>) -> Outcome {
        Outcome::PassedOver
    }

    /// The registers sound on (S6581-48).
    fn lost(&mut self) {}

    fn render(&mut self, out: &mut [i16]) {
        self.chip.render(out);
    }
}
