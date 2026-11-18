//! The synth6581 family's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.1): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! The family is programmed by register writes and has no patch: a field's
//! number is the register's ASID identifier, 0 to 27 (S6581-11), and its
//! value the register's last written byte. Identifiers 25 to 27 are a
//! second write to the control registers of identifiers 22 to 24, so each
//! reads the same register as its first.

use super::{ADDRESS_OF_ID, REGISTER_IDS, Tracker};
use crate::midi::PatchField;

/// The readable fields, by ASID register identifier (appendix A.1).
pub const PARAMETERS: [PatchField; REGISTER_IDS] = [
    PatchField::range("voice1_frequency_low", 0, 255),
    PatchField::range("voice1_frequency_high", 0, 255),
    PatchField::range("voice1_pulse_width_low", 0, 255),
    PatchField::range("voice1_pulse_width_high", 0, 255),
    PatchField::range("voice1_attack_decay", 0, 255),
    PatchField::range("voice1_sustain_release", 0, 255),
    PatchField::range("voice2_frequency_low", 0, 255),
    PatchField::range("voice2_frequency_high", 0, 255),
    PatchField::range("voice2_pulse_width_low", 0, 255),
    PatchField::range("voice2_pulse_width_high", 0, 255),
    PatchField::range("voice2_attack_decay", 0, 255),
    PatchField::range("voice2_sustain_release", 0, 255),
    PatchField::range("voice3_frequency_low", 0, 255),
    PatchField::range("voice3_frequency_high", 0, 255),
    PatchField::range("voice3_pulse_width_low", 0, 255),
    PatchField::range("voice3_pulse_width_high", 0, 255),
    PatchField::range("voice3_attack_decay", 0, 255),
    PatchField::range("voice3_sustain_release", 0, 255),
    PatchField::range("filter_cutoff_low", 0, 255),
    PatchField::range("filter_cutoff_high", 0, 255),
    PatchField::range("filter_resonance_routing", 0, 255),
    PatchField::range("filter_mode_volume", 0, 255),
    PatchField::range("voice1_control", 0, 255),
    PatchField::range("voice2_control", 0, 255),
    PatchField::range("voice3_control", 0, 255),
    PatchField::range("voice1_control_second", 0, 255),
    PatchField::range("voice2_control_second", 0, 255),
    PatchField::range("voice3_control_second", 0, 255),
];

/// The present value of field `number`: the last byte written to the
/// register its identifier names, 0 before any; `None` for a number past
/// the table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, number: u16) -> Option<i32> {
    let address = ADDRESS_OF_ID.get(usize::from(number))?;
    tracker.registers().get(usize::from(*address)).map(|&value| i32::from(value))
}
