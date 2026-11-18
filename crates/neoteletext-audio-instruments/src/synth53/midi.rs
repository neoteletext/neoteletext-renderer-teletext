//! synth53's front end to the receiver (MIDI-10, [`crate::midi::Instrument`]):
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

use super::Synth53;

/// synth53 as the receiver plays it.
pub type Midi = Fronted<Synth53>;

/// The instrument's bend at either end, in cents:
/// [`super::PITCH_BEND_SEMITONES`].
const BEND_CENTS: u16 = 700;
const _: () =
    assert!(super::PITCH_BEND_SEMITONES * crate::midi::common::CENTS_PER_SEMITONE as i32 == BEND_CENTS as i32);

impl ClassicFamily for Synth53 {
    type Patch = super::Patch;
    const CONFIG: Config = Config {
        bend_range_cents: BEND_CENTS,
        most_bend_range_cents: BEND_CENTS,
        bank_select: false,
        non_registered: false,
    };
    const CLASSIC: Classic = Classic {
        controllers: &[crate::midi::controller::MODULATION],
        voice_bend_range_cents: BEND_CENTS,
        reset_pedals: &[],
        expression: false,
        channel_pressure: false,
        key_pressure: false,
    };
    const PATCH_BYTES: usize = super::patch::PATCH_BYTES;
    const PROGRAM_CHANNELS: u8 = crate::midi::CHANNEL_MASK + 1;

    fn default_patch(&self) -> Self::Patch {
        super::Patch::DEFAULT
    }

    fn read_patch(bytes: &[u8]) -> Option<Self::Patch> {
        // The instrument's reader holds each byte to its range itself, so
        // only bytes of another length are no patch.
        Some(super::Patch::from_bytes(bytes.try_into().ok()?))
    }

    fn set_patch(&mut self, channel: u8, patch: &Self::Patch) {
        let _ = channel;
        self.set_patch(patch);
    }

    fn event(&mut self, status: u8, data1: u8, data2: u8) {
        Synth53::event(self, status, data1, data2);
    }

    fn release_all(&mut self) {
        Synth53::release_all(self);
    }

    fn render(&mut self, out: &mut [i16]) {
        Synth53::render(self, out);
    }
}
