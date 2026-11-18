//! synth8's front end to the receiver (MIDI-10, [`crate::midi::Instrument`]):
//! the common layer ([`crate::midi::common`]) before the instrument's own
//! message reader, and its patches read from bank index 0 as fixed patches
//! of its own byte layout.
//!
//! This is the interim front end the new carriage was built with. The
//! family's adopted interface (its family document's messages, control map,
//! patch form, banks, mapping of program changes and restatement) is to be
//! built here, replacing [`crate::midi::classic`]'s bridge.

use crate::midi::classic::{Classic, ClassicFamily, Fronted};
use crate::midi::common::Config;

use super::Synth8;

/// synth8 as the receiver plays it.
pub type Midi = Fronted<Synth8>;

/// The channel, counted from 0, whose program change chooses the upper
/// patch: MIDI channel 1; channel 2 chooses the lower (`synth8.md`, S8-2).
const UPPER_CHANNEL: u8 = 0;
/// The bend of synth8's voices at either end before the performance
/// changes it, in cents ([`super::Performance`]'s default).
const BEND_CENTS: u16 = 200;
/// Portamento time (MIDI 1.0 4.2, Table III).
const PORTAMENTO_TIME: u8 = 5;
/// Balance of the two halves (MIDI 1.0 4.2, Table III).
const BALANCE: u8 = 8;

impl ClassicFamily for Synth8 {
    type Patch = super::Patch;
    const CONFIG: Config = Config {
        bend_range_cents: BEND_CENTS,
        most_bend_range_cents: BEND_CENTS,
        bank_select: false,
        non_registered: false,
    };
    const CLASSIC: Classic = Classic {
        controllers: &[
            crate::midi::controller::MODULATION,
            PORTAMENTO_TIME,
            BALANCE,
            crate::midi::controller::PORTAMENTO,
        ],
        voice_bend_range_cents: BEND_CENTS,
        reset_pedals: &[crate::midi::controller::PORTAMENTO],
        expression: false,
        channel_pressure: false,
        key_pressure: false,
    };
    const PATCH_BYTES: usize = super::PATCH_BYTES;
    const PROGRAM_CHANNELS: u8 = 2;

    fn default_patch(&self) -> Self::Patch {
        super::Patch::DEFAULT
    }

    fn read_patch(bytes: &[u8]) -> Option<Self::Patch> {
        super::Patch::from_bytes(bytes.try_into().ok()?).ok()
    }

    fn set_patch(&mut self, channel: u8, patch: &Self::Patch) {
        if channel == UPPER_CHANNEL {
            self.set_patch(patch);
        } else {
            self.set_lower_patch(patch);
        }
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        Synth8::event(self, status, data1, data2);
    }

    fn release_all(&mut self) {
        Synth8::release_all(self);
    }

    fn render(&mut self, out: &mut [i16]) {
        Synth8::render(self, out);
    }
}
