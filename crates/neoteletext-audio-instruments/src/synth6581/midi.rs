//! synth6581's and synth8580's front end to the receiver
//! ([`crate::midi::Instrument`]): ASID's register data message writes the
//! chip's registers, its start and stop messages are accepted with no
//! effect, and everything else is passed over and counted
//! (`synth6581-synth8580.md`, S6581-2, S6581-9, S6581-26).
//!
//! There is no bank (S6581-51) and no note layer (S6581-52, DEC-6465 answer
//! 8): a channel message, a meta event or a system exclusive message that
//! is not ASID changes nothing. The chip is made new for each sound from
//! the OBJECT record's chip and clock (S6581-1, S6581-36, S6581-39), sounds
//! on unchanged through a lost block (S6581-48) and runs on across a loop's
//! join (S6581-49).

use crate::BLOCK_SAMPLES;
use crate::midi::{Banks, Instrument, Message, Outcome};

use super::interface::{self, Asid, BlockBudget};
use super::{ChipClock, Synth6581, Variant};

/// synth6581 or synth8580 as the receiver plays it.
#[derive(Clone, Debug)]
pub struct Midi {
    chip: Synth6581,
    /// What the block being played may still hold (S6581-42).
    budget: BlockBudget,
    /// Samples rendered of the block being played. The receiver renders
    /// each block's 960 samples whole, around its events
    /// ([`crate::midi::play_block`]), so a message is in the block whose
    /// samples are being rendered, and the block's budget starts again
    /// when its last sample is rendered.
    rendered: usize,
}

impl Midi {
    /// A chip of `model` on `clock`, reset (S6581-36).
    #[must_use]
    pub const fn new(model: Variant, clock: ChipClock) -> Self {
        Self { chip: Synth6581::new(model, clock), budget: BlockBudget::new(), rendered: 0 }
    }

    /// The chip.
    #[must_use]
    pub const fn chip(&self) -> &Synth6581 {
        &self.chip
    }
}

impl Instrument for Midi {
    /// Nothing to ready: no bank, and the chip is new (S6581-36, S6581-51).
    fn start(&mut self, _banks: &Banks<'_>) -> Outcome {
        Outcome::Applied
    }

    fn midi(&mut self, message: &Message<'_>, _banks: &Banks<'_>) -> Outcome {
        // S6581-7, S6581-26: only an ASID system exclusive message is
        // taken; S6581-8: a malformed one is dropped whole.
        let Message::SystemExclusive(after_start) = message else { return Outcome::PassedOver };
        let Ok(asid) = interface::read(after_start) else { return Outcome::PassedOver };
        if !self.budget.admit(&asid) {
            return Outcome::PassedOver;
        }
        // S6581-15: every write at this instant, in ascending ID, with no
        // chip time between; both writes of a control register made.
        if let Asid::RegisterData(data) = asid {
            for (address, value) in data.writes() {
                self.chip.write(address, value);
            }
        }
        Outcome::Applied
    }

    /// The registers sound on (S6581-48).
    fn lost(&mut self) {}

    fn render(&mut self, out: &mut [i16]) {
        self.chip.render(out);
        self.rendered = self.rendered.saturating_add(out.len());
        if self.rendered >= BLOCK_SAMPLES {
            self.rendered %= BLOCK_SAMPLES;
            self.budget = BlockBudget::new();
        }
    }

    /// The part is the voice, 0 to 2 (page-rules.md, RULE-28).
    fn meter(&self, part: u8) -> crate::midi::Meter {
        self.chip.meter(usize::from(part))
    }
}
