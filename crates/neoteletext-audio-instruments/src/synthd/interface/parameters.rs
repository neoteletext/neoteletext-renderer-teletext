//! synthD's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.5): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! Each field is one control of the panel: a pair of controllers, read as
//! its 14-bit number, coarse x 128 + fine (SD-18), or a single controller's
//! value.

use super::{LAYOUT, Role, Tracker, byte, pair};
use crate::midi::PatchField;

/// The readable fields.
pub const PARAMETER_COUNT: usize = 42;

/// The readable fields (appendix A.5).
pub const PARAMETERS: [PatchField; PARAMETER_COUNT] = [
    PatchField::range("glide", 0, 16383),
    PatchField::range("oscillator_2_frequency", 0, 16383),
    PatchField::range("oscillator_3_frequency", 0, 16383),
    PatchField::range("external_input_volume", 0, 16383),
    PatchField::range("oscillator_1_volume", 0, 16383),
    PatchField::range("oscillator_2_volume", 0, 16383),
    PatchField::range("oscillator_3_volume", 0, 16383),
    PatchField::range("noise_volume", 0, 16383),
    PatchField::range("cutoff_frequency", 0, 16383),
    PatchField::range("emphasis", 0, 16383),
    PatchField::range("filter_attack_time", 0, 16383),
    PatchField::range("filter_decay_time", 0, 16383),
    PatchField::range("filter_sustain_level", 0, 16383),
    PatchField::range("amount_of_contour", 0, 16383),
    PatchField::range("loudness_attack_time", 0, 16383),
    PatchField::range("loudness_decay_time", 0, 16383),
    PatchField::range("loudness_sustain_level", 0, 16383),
    PatchField::range("glide_switch", 0, 127),
    PatchField::range("oscillator_1_range_octave", 0, 127),
    PatchField::range("oscillator_2_range_octave", 0, 127),
    PatchField::range("oscillator_3_range_octave", 0, 127),
    PatchField::range("oscillator_3_control", 0, 127),
    PatchField::range("oscillator_3_range_lo_hi", 0, 127),
    PatchField::range("external_input_switch", 0, 127),
    PatchField::range("oscillator_1_switch", 0, 127),
    PatchField::range("oscillator_2_switch", 0, 127),
    PatchField::range("oscillator_3_switch", 0, 127),
    PatchField::range("noise_switch", 0, 127),
    PatchField::range("decay_switch", 0, 127),
    PatchField::range("tune", 0, 16383),
    PatchField::range("modulation_mix", 0, 127),
    PatchField::range("oscillator_1_range_lo_hi", 0, 127),
    PatchField::range("oscillator_1_waveform", 0, 127),
    PatchField::range("oscillator_2_range_lo_hi", 0, 127),
    PatchField::range("oscillator_2_waveform", 0, 127),
    PatchField::range("oscillator_3_waveform", 0, 127),
    PatchField::range("main_output_volume", 0, 127),
    PatchField::range("oscillator_modulation", 0, 127),
    PatchField::range("noise_colour", 0, 127),
    PatchField::range("filter_modulation", 0, 127),
    PatchField::range("keyboard_control_1", 0, 127),
    PatchField::range("keyboard_control_2", 0, 127),
];

/// Where each field stands in the patch's 60 bytes: its one byte, or the
/// coarse byte of its pair ([`LAYOUT`]).
pub const PLACES: [usize; PARAMETER_COUNT] = [
    byte::GLIDE,
    byte::OSCILLATOR_2_FREQUENCY,
    byte::OSCILLATOR_3_FREQUENCY,
    byte::EXTERNAL_INPUT_VOLUME,
    byte::OSCILLATOR_1_VOLUME,
    byte::OSCILLATOR_2_VOLUME,
    byte::OSCILLATOR_3_VOLUME,
    byte::NOISE_VOLUME,
    byte::CUTOFF_FREQUENCY,
    byte::EMPHASIS,
    byte::FILTER_ATTACK_TIME,
    byte::FILTER_DECAY_TIME,
    byte::FILTER_SUSTAIN_LEVEL,
    byte::AMOUNT_OF_CONTOUR,
    byte::LOUDNESS_ATTACK_TIME,
    byte::LOUDNESS_DECAY_TIME,
    byte::LOUDNESS_SUSTAIN_LEVEL,
    byte::GLIDE_SWITCH,
    byte::OSCILLATOR_1_RANGE_OCTAVE,
    byte::OSCILLATOR_2_RANGE_OCTAVE,
    byte::OSCILLATOR_3_RANGE_OCTAVE,
    byte::OSCILLATOR_3_CONTROL,
    byte::OSCILLATOR_3_RANGE_LO_HI,
    byte::EXTERNAL_INPUT_SWITCH,
    byte::OSCILLATOR_1_SWITCH,
    byte::OSCILLATOR_2_SWITCH,
    byte::OSCILLATOR_3_SWITCH,
    byte::NOISE_SWITCH,
    byte::DECAY_SWITCH,
    byte::TUNE,
    byte::MODULATION_MIX,
    byte::OSCILLATOR_1_RANGE_LO_HI,
    byte::OSCILLATOR_1_WAVEFORM,
    byte::OSCILLATOR_2_RANGE_LO_HI,
    byte::OSCILLATOR_2_WAVEFORM,
    byte::OSCILLATOR_3_WAVEFORM,
    byte::MAIN_OUTPUT_VOLUME,
    byte::OSCILLATOR_MODULATION,
    byte::NOISE_COLOUR,
    byte::FILTER_MODULATION,
    byte::KEYBOARD_CONTROL_1,
    byte::KEYBOARD_CONTROL_2,
];

/// The present value of field `number`: of the panel as the messages taken
/// left it. Every channel is the one panel (SD-2), so there is no part.
/// `None` for a number past the table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, number: u16) -> Option<i32> {
    let field = PARAMETERS.get(usize::from(number))?;
    let place = *PLACES.get(usize::from(number))?;
    let values = tracker.values();
    let value = match LAYOUT.get(place)? {
        (_, Role::Coarse) => i32::from(pair(values, place)),
        (_, Role::Single | Role::Fine) => i32::from(*values.get(place)?),
    };
    Some(field.hold(value))
}
