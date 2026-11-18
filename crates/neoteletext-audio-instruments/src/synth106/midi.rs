//! synth106 and synth60 as the receiver plays them ([`crate::midi::Instrument`]):
//! every message read by the family's [`Tracker`], as
//! `neoteletext-common/docs/audio/synth106-synth60.md` says, and what it
//! does to the sound given to the engine ([`Synth106`]).

use super::interface::{Effect, Tracker};
use super::{Patch, Synth106};
use crate::midi::{Banks, Instrument, Message, Meter, Outcome};

/// synth106 or synth60 as the receiver plays it.
#[derive(Clone, Debug)]
pub struct Midi {
    engine: Synth106,
    tracker: Tracker,
    /// Whether the panel has moved since the engine was last given it.
    /// The engine is given the panel only before a key or before samples,
    /// so that the messages of one instant (a restatement's program change
    /// and patch message, S106-40) are one change of the panel, and the
    /// panel the engine already plays changes nothing (MIDI-97).
    panel_moved: bool,
}

impl Midi {
    /// `engine`, behind a fresh tracker of its model.
    #[must_use]
    pub fn new(engine: Synth106) -> Self {
        let tracker = Tracker::new(engine.model());
        Self { engine, tracker, panel_moved: true }
    }

    /// The engine.
    #[must_use]
    pub const fn engine(&self) -> &Synth106 {
        &self.engine
    }

    /// The state the messages have set.
    #[must_use]
    pub const fn tracker(&self) -> &Tracker {
        &self.tracker
    }

    /// The panel as the messages have set it (S106-13).
    #[must_use]
    pub const fn panel(&self) -> &Patch {
        self.tracker.panel()
    }

    /// Gives the engine the panel, if it has moved.
    fn settle(&mut self) {
        if self.panel_moved {
            self.panel_moved = false;
            self.engine.set_patch(self.tracker.panel());
        }
    }
}

impl Instrument for Midi {
    fn start(&mut self, banks: &Banks<'_>) -> Outcome {
        let outcome = self.tracker.start(banks);
        self.panel_moved = true;
        outcome
    }

    fn midi(&mut self, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
        let (outcome, effect) = self.tracker.take(message, banks);
        match effect {
            Effect::None => {}
            Effect::Panel => self.panel_moved = true,
            Effect::KeyDown(key) => {
                // A note sounds with the panel as it stands at its instant
                // (S106-20).
                self.settle();
                self.engine.key_down(key);
            }
            Effect::KeyUp(key) => self.engine.key_up(key),
            Effect::Sustain(down) => self.engine.sustain(down),
            Effect::AllNotesOff => self.engine.all_notes_off(),
            Effect::AllSoundOff => self.engine.all_sound_off(),
            Effect::ResetControllers => self.engine.reset_controllers(),
            Effect::Bend(value) => self.engine.set_bend(value),
            Effect::BendRange(cents) => self.engine.set_bend_range_cents(cents),
            Effect::Volume(value) => self.engine.set_volume(value),
            Effect::Wheel(value) => self.engine.set_wheel(value),
        }
        outcome
    }

    fn lost(&mut self) {
        self.tracker.lost();
        self.engine.release_all();
    }

    fn render(&mut self, out: &mut [i16]) {
        if !out.is_empty() {
            self.settle();
        }
        self.engine.render(out);
    }

    /// Every channel is the one part (S106-2).
    fn meter(&self, _part: u8) -> Meter {
        self.engine.meter()
    }
}
