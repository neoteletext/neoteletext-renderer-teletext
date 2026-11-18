//! synth80 as the receiver plays it ([`crate::midi::Instrument`]): its
//! family document's messages, control map, patches, banks and program
//! changes (`neoteletext-common/docs/audio/synth80.md`), on the common
//! layer of [`crate::midi::common`], which [`Synth80`] keeps.

use crate::midi::{Banks, Instrument, Message, Outcome};

use super::Synth80;

/// synth80 behind the receiver's one trait.
#[derive(Clone, Debug)]
pub struct Midi {
    synth80: Synth80,
}

impl Midi {
    /// `synth80`, as the receiver plays it.
    #[must_use]
    pub const fn new(synth80: Synth80) -> Self {
        Self { synth80 }
    }

    /// The instrument.
    #[must_use]
    pub const fn synth80(&self) -> &Synth80 {
        &self.synth80
    }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        self.synth80.start(banks)
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        self.synth80.midi(message, banks)
    }

    fn lost(&mut self) {
        self.synth80.lost();
    }

    fn render(&mut self, out: &mut [i16]) {
        self.synth80.render(out);
    }

    /// Every channel is the one instrument (S80-2).
    fn meter(&self, _part: u8) -> crate::midi::Meter {
        self.synth80.meter()
    }
}
