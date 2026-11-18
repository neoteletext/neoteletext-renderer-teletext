//! synth7's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.2): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! Numbers 0 to 125 are the six operators' fields, operator 1 first, 21
//! each; 126 to 155 the voice's own, which are voice parameters 126 to 155
//! (S7-28); 156 to 169 the function parameters 64 to 77 (S7-33). The voice
//! parameters count the operators from operator 6, so operator `k`'s field
//! `j` is voice parameter `(6 - k) x 21 + j`.

use super::Tracker;
use crate::midi::PatchField;
use crate::synth7::functions::{FIRST_FUNCTION_PARAMETER, FUNCTION_PARAMETERS};
use crate::synth7::patch::{OPERATOR_PARAMETERS, OPERATOR_SWITCH_PARAMETER, OPERATORS, VOICE_PARAMETERS};
use crate::synth7::sysex::Parameter;

/// The fields of the operators: the voice parameters before the voice's
/// own.
const OPERATOR_FIELDS: usize = OPERATORS * OPERATOR_PARAMETERS;
/// The readable fields: the voice parameters 0 to 155 and the function
/// parameters.
pub const PARAMETER_COUNT: usize = VOICE_PARAMETERS + 1 + FUNCTION_PARAMETERS;
// S7-28: 6 operators of 21, then voice parameters 126 to 155, the operator
// switch the last; S7-33: 14 function parameters.
const _: () = assert!(OPERATOR_FIELDS == 126 && OPERATOR_SWITCH_PARAMETER as usize == VOICE_PARAMETERS);
const _: () = assert!(PARAMETER_COUNT == 170);

/// The readable fields (appendix A.2).
pub const PARAMETERS: [PatchField; PARAMETER_COUNT] = [
    PatchField::range("op1_eg_rate_1", 0, 99),
    PatchField::range("op1_eg_rate_2", 0, 99),
    PatchField::range("op1_eg_rate_3", 0, 99),
    PatchField::range("op1_eg_rate_4", 0, 99),
    PatchField::range("op1_eg_level_1", 0, 99),
    PatchField::range("op1_eg_level_2", 0, 99),
    PatchField::range("op1_eg_level_3", 0, 99),
    PatchField::range("op1_eg_level_4", 0, 99),
    PatchField::range("op1_break_point", 0, 99),
    PatchField::range("op1_left_depth", 0, 99),
    PatchField::range("op1_right_depth", 0, 99),
    PatchField::range("op1_left_curve", 0, 3),
    PatchField::range("op1_right_curve", 0, 3),
    PatchField::range("op1_rate_scaling", 0, 7),
    PatchField::range("op1_amplitude_sensitivity", 0, 3),
    PatchField::range("op1_velocity_sensitivity", 0, 7),
    PatchField::range("op1_output_level", 0, 99),
    PatchField::range("op1_oscillator_mode", 0, 1),
    PatchField::range("op1_frequency_coarse", 0, 31),
    PatchField::range("op1_frequency_fine", 0, 99),
    PatchField::range("op1_detune", 0, 14),
    PatchField::range("op2_eg_rate_1", 0, 99),
    PatchField::range("op2_eg_rate_2", 0, 99),
    PatchField::range("op2_eg_rate_3", 0, 99),
    PatchField::range("op2_eg_rate_4", 0, 99),
    PatchField::range("op2_eg_level_1", 0, 99),
    PatchField::range("op2_eg_level_2", 0, 99),
    PatchField::range("op2_eg_level_3", 0, 99),
    PatchField::range("op2_eg_level_4", 0, 99),
    PatchField::range("op2_break_point", 0, 99),
    PatchField::range("op2_left_depth", 0, 99),
    PatchField::range("op2_right_depth", 0, 99),
    PatchField::range("op2_left_curve", 0, 3),
    PatchField::range("op2_right_curve", 0, 3),
    PatchField::range("op2_rate_scaling", 0, 7),
    PatchField::range("op2_amplitude_sensitivity", 0, 3),
    PatchField::range("op2_velocity_sensitivity", 0, 7),
    PatchField::range("op2_output_level", 0, 99),
    PatchField::range("op2_oscillator_mode", 0, 1),
    PatchField::range("op2_frequency_coarse", 0, 31),
    PatchField::range("op2_frequency_fine", 0, 99),
    PatchField::range("op2_detune", 0, 14),
    PatchField::range("op3_eg_rate_1", 0, 99),
    PatchField::range("op3_eg_rate_2", 0, 99),
    PatchField::range("op3_eg_rate_3", 0, 99),
    PatchField::range("op3_eg_rate_4", 0, 99),
    PatchField::range("op3_eg_level_1", 0, 99),
    PatchField::range("op3_eg_level_2", 0, 99),
    PatchField::range("op3_eg_level_3", 0, 99),
    PatchField::range("op3_eg_level_4", 0, 99),
    PatchField::range("op3_break_point", 0, 99),
    PatchField::range("op3_left_depth", 0, 99),
    PatchField::range("op3_right_depth", 0, 99),
    PatchField::range("op3_left_curve", 0, 3),
    PatchField::range("op3_right_curve", 0, 3),
    PatchField::range("op3_rate_scaling", 0, 7),
    PatchField::range("op3_amplitude_sensitivity", 0, 3),
    PatchField::range("op3_velocity_sensitivity", 0, 7),
    PatchField::range("op3_output_level", 0, 99),
    PatchField::range("op3_oscillator_mode", 0, 1),
    PatchField::range("op3_frequency_coarse", 0, 31),
    PatchField::range("op3_frequency_fine", 0, 99),
    PatchField::range("op3_detune", 0, 14),
    PatchField::range("op4_eg_rate_1", 0, 99),
    PatchField::range("op4_eg_rate_2", 0, 99),
    PatchField::range("op4_eg_rate_3", 0, 99),
    PatchField::range("op4_eg_rate_4", 0, 99),
    PatchField::range("op4_eg_level_1", 0, 99),
    PatchField::range("op4_eg_level_2", 0, 99),
    PatchField::range("op4_eg_level_3", 0, 99),
    PatchField::range("op4_eg_level_4", 0, 99),
    PatchField::range("op4_break_point", 0, 99),
    PatchField::range("op4_left_depth", 0, 99),
    PatchField::range("op4_right_depth", 0, 99),
    PatchField::range("op4_left_curve", 0, 3),
    PatchField::range("op4_right_curve", 0, 3),
    PatchField::range("op4_rate_scaling", 0, 7),
    PatchField::range("op4_amplitude_sensitivity", 0, 3),
    PatchField::range("op4_velocity_sensitivity", 0, 7),
    PatchField::range("op4_output_level", 0, 99),
    PatchField::range("op4_oscillator_mode", 0, 1),
    PatchField::range("op4_frequency_coarse", 0, 31),
    PatchField::range("op4_frequency_fine", 0, 99),
    PatchField::range("op4_detune", 0, 14),
    PatchField::range("op5_eg_rate_1", 0, 99),
    PatchField::range("op5_eg_rate_2", 0, 99),
    PatchField::range("op5_eg_rate_3", 0, 99),
    PatchField::range("op5_eg_rate_4", 0, 99),
    PatchField::range("op5_eg_level_1", 0, 99),
    PatchField::range("op5_eg_level_2", 0, 99),
    PatchField::range("op5_eg_level_3", 0, 99),
    PatchField::range("op5_eg_level_4", 0, 99),
    PatchField::range("op5_break_point", 0, 99),
    PatchField::range("op5_left_depth", 0, 99),
    PatchField::range("op5_right_depth", 0, 99),
    PatchField::range("op5_left_curve", 0, 3),
    PatchField::range("op5_right_curve", 0, 3),
    PatchField::range("op5_rate_scaling", 0, 7),
    PatchField::range("op5_amplitude_sensitivity", 0, 3),
    PatchField::range("op5_velocity_sensitivity", 0, 7),
    PatchField::range("op5_output_level", 0, 99),
    PatchField::range("op5_oscillator_mode", 0, 1),
    PatchField::range("op5_frequency_coarse", 0, 31),
    PatchField::range("op5_frequency_fine", 0, 99),
    PatchField::range("op5_detune", 0, 14),
    PatchField::range("op6_eg_rate_1", 0, 99),
    PatchField::range("op6_eg_rate_2", 0, 99),
    PatchField::range("op6_eg_rate_3", 0, 99),
    PatchField::range("op6_eg_rate_4", 0, 99),
    PatchField::range("op6_eg_level_1", 0, 99),
    PatchField::range("op6_eg_level_2", 0, 99),
    PatchField::range("op6_eg_level_3", 0, 99),
    PatchField::range("op6_eg_level_4", 0, 99),
    PatchField::range("op6_break_point", 0, 99),
    PatchField::range("op6_left_depth", 0, 99),
    PatchField::range("op6_right_depth", 0, 99),
    PatchField::range("op6_left_curve", 0, 3),
    PatchField::range("op6_right_curve", 0, 3),
    PatchField::range("op6_rate_scaling", 0, 7),
    PatchField::range("op6_amplitude_sensitivity", 0, 3),
    PatchField::range("op6_velocity_sensitivity", 0, 7),
    PatchField::range("op6_output_level", 0, 99),
    PatchField::range("op6_oscillator_mode", 0, 1),
    PatchField::range("op6_frequency_coarse", 0, 31),
    PatchField::range("op6_frequency_fine", 0, 99),
    PatchField::range("op6_detune", 0, 14),
    PatchField::range("pitch_eg_rate_1", 0, 99),
    PatchField::range("pitch_eg_rate_2", 0, 99),
    PatchField::range("pitch_eg_rate_3", 0, 99),
    PatchField::range("pitch_eg_rate_4", 0, 99),
    PatchField::range("pitch_eg_level_1", 0, 99),
    PatchField::range("pitch_eg_level_2", 0, 99),
    PatchField::range("pitch_eg_level_3", 0, 99),
    PatchField::range("pitch_eg_level_4", 0, 99),
    PatchField::range("algorithm", 0, 31),
    PatchField::range("feedback", 0, 7),
    PatchField::range("oscillator_sync", 0, 1),
    PatchField::range("lfo_speed", 0, 99),
    PatchField::range("lfo_delay", 0, 99),
    PatchField::range("lfo_pitch_depth", 0, 99),
    PatchField::range("lfo_amplitude_depth", 0, 99),
    PatchField::range("lfo_sync", 0, 1),
    PatchField::range("lfo_wave", 0, 5),
    PatchField::range("pitch_sensitivity", 0, 7),
    PatchField::range("transpose", 0, 48),
    PatchField::range("name_1", 0, 127),
    PatchField::range("name_2", 0, 127),
    PatchField::range("name_3", 0, 127),
    PatchField::range("name_4", 0, 127),
    PatchField::range("name_5", 0, 127),
    PatchField::range("name_6", 0, 127),
    PatchField::range("name_7", 0, 127),
    PatchField::range("name_8", 0, 127),
    PatchField::range("name_9", 0, 127),
    PatchField::range("name_10", 0, 127),
    PatchField::range("operator_switch", 0, 63),
    PatchField::range("mono_poly", 0, 1),
    PatchField::range("pitch_bend_range", 0, 12),
    PatchField::range("pitch_bend_step", 0, 12),
    PatchField::range("portamento_mode", 0, 1),
    PatchField::range("glissando", 0, 1),
    PatchField::range("portamento_time", 0, 99),
    PatchField::range("modulation_wheel_range", 0, 99),
    PatchField::range("modulation_wheel_assign", 0, 7),
    PatchField::range("foot_controller_range", 0, 99),
    PatchField::range("foot_controller_assign", 0, 7),
    PatchField::range("breath_controller_range", 0, 99),
    PatchField::range("breath_controller_assign", 0, 7),
    PatchField::range("after_touch_range", 0, 99),
    PatchField::range("after_touch_assign", 0, 7),
];

/// The parameter of the adopted interface that field `number` is: a voice
/// parameter (S7-28) or a function parameter (S7-33), as a parameter change
/// names it (S7-43). `None` for a number past the table.
#[must_use]
pub fn parameter_of(number: u16) -> Option<Parameter> {
    let index = usize::from(number);
    if index < OPERATOR_FIELDS {
        // Operator 1's fields come first here and last among the voice
        // parameters.
        let operator = index.checked_div(OPERATOR_PARAMETERS)?;
        let field = index.checked_rem(OPERATOR_PARAMETERS)?;
        let first = OPERATORS.checked_sub(1)?.checked_sub(operator)?.checked_mul(OPERATOR_PARAMETERS)?;
        return u16::try_from(first.checked_add(field)?).ok().map(Parameter::Voice);
    }
    if index <= VOICE_PARAMETERS {
        return Some(Parameter::Voice(number));
    }
    let function = index.checked_sub(VOICE_PARAMETERS)?.checked_sub(1)?;
    if function >= FUNCTION_PARAMETERS {
        return None;
    }
    Some(Parameter::Function(FIRST_FUNCTION_PARAMETER.checked_add(u16::try_from(function).ok()?)?))
}

/// The present value of field `number` on part `part`, its MIDI channel
/// less one, 0 to 15: of the part's current voice, its operator switch or
/// its function parameters, as the messages taken left them. `None` for a
/// number past the table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, part: u8, number: u16) -> Option<i32> {
    let value = match parameter_of(number)? {
        Parameter::Voice(OPERATOR_SWITCH_PARAMETER) => tracker.operator_switch(part)?,
        Parameter::Voice(voice) => tracker.voice(part)?.0.parameter(voice)?,
        Parameter::Function(function) => tracker.functions(part)?.get(function)?,
    };
    Some(i32::from(value))
}
