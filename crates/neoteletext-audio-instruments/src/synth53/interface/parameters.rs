//! The synth53 family's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.6): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! The fields are the program parameters a program change, a dump or a
//! non-registered parameter number sets (S53-21, S53-23): every one but
//! parameter 20, which the family does not name, so fields 0 to 19 are
//! program parameters 0 to 19, fields 20 to 63 program parameters 21 to 64,
//! and the last two program parameters 86 and 87.

use super::{PROGRAM_RANGES, Part};
use crate::midi::PatchField;

/// The readable fields: every program parameter but parameter 20.
pub const PARAMETER_COUNT: usize = PROGRAM_RANGES.len() - 1;
const _: () = assert!(PARAMETER_COUNT == 66);

/// The readable fields (appendix A.6).
pub const PARAMETERS: [PatchField; PARAMETER_COUNT] = [
    PatchField::range("osc_a_frequency", 0, 120),
    PatchField::range("osc_b_frequency", 0, 120),
    PatchField::range("osc_b_fine_tune", 0, 127),
    PatchField::range("osc_a_saw", 0, 1),
    PatchField::range("osc_a_square", 0, 1),
    PatchField::range("osc_b_saw", 0, 1),
    PatchField::range("osc_b_tri", 0, 1),
    PatchField::range("osc_b_square", 0, 1),
    PatchField::range("osc_a_pulse_width", 0, 120),
    PatchField::range("osc_b_pulse_width", 0, 120),
    PatchField::range("osc_sync", 0, 1),
    PatchField::range("osc_b_low_freq", 0, 1),
    PatchField::range("osc_b_keyboard", 0, 1),
    PatchField::range("glide_rate", 0, 120),
    PatchField::range("osc_a_level", 0, 120),
    PatchField::range("osc_b_level", 0, 120),
    PatchField::range("noise_level", 0, 120),
    PatchField::range("cutoff", 0, 120),
    PatchField::range("resonance", 0, 120),
    PatchField::range("filter_keyboard_track", 0, 2),
    PatchField::range("lfo_frequency", 0, 120),
    PatchField::range("lfo_initial_amount", 0, 120),
    PatchField::range("lfo_saw", 0, 1),
    PatchField::range("lfo_tri", 0, 1),
    PatchField::range("lfo_square", 0, 1),
    PatchField::range("lfo_source_mix", 0, 120),
    PatchField::range("lfo_freq_a", 0, 1),
    PatchField::range("lfo_freq_b", 0, 1),
    PatchField::range("lfo_freq_pw_a", 0, 1),
    PatchField::range("lfo_freq_pw_b", 0, 1),
    PatchField::range("lfo_filter", 0, 1),
    PatchField::range("poly_mod_filt_env_amount", 0, 127),
    PatchField::range("poly_mod_osc_b_amount", 0, 120),
    PatchField::range("poly_mod_freq_a", 0, 1),
    PatchField::range("poly_mod_pw", 0, 1),
    PatchField::range("poly_mod_filter", 0, 1),
    PatchField::range("vintage", 0, 127),
    PatchField::range("aftertouch_to_filter", 0, 1),
    PatchField::range("aftertouch_to_lfo", 0, 1),
    PatchField::range("env_filter_amount", 0, 120),
    PatchField::range("velocity_to_filter", 0, 1),
    PatchField::range("velocity_to_amp", 0, 1),
    PatchField::range("attack_filter", 0, 120),
    PatchField::range("attack_vca", 0, 120),
    PatchField::range("decay_filter", 0, 120),
    PatchField::range("decay_vca", 0, 120),
    PatchField::range("sustain_filter", 0, 120),
    PatchField::range("sustain_vca", 0, 120),
    PatchField::range("release_filter", 0, 120),
    PatchField::range("release_vca", 0, 120),
    PatchField::range("release_switch", 0, 1),
    PatchField::range("unison", 0, 1),
    PatchField::range("unison_voice_count", 0, 10),
    PatchField::range("unison_detune", 0, 7),
    PatchField::range("unison_note_1", 0, 10),
    PatchField::range("unison_note_2", 0, 10),
    PatchField::range("unison_note_3", 0, 10),
    PatchField::range("unison_note_4", 0, 10),
    PatchField::range("unison_note_5", 0, 10),
    PatchField::range("unison_note_6", 0, 10),
    PatchField::range("unison_note_7", 0, 10),
    PatchField::range("unison_note_8", 0, 10),
    PatchField::range("unison_note_9", 0, 10),
    PatchField::range("unison_note_10", 0, 10),
    PatchField::range("pitch_wheel_range", 0, 11),
    PatchField::range("retrigger_and_unison", 0, 3),
];

/// The program parameter that is none of the named fields.
const LEFT_OUT: u8 = 20;

/// The program parameter number of field `number`, which a non-registered
/// parameter number selects (S53-23); `None` for a number past the table.
#[must_use]
pub fn program_parameter(number: u16) -> Option<u8> {
    let index = usize::from(number);
    if index >= PARAMETER_COUNT {
        return None;
    }
    // The table of program parameters is in ascending order of number, with
    // parameter 20 in it: the fields are its entries without that one.
    let mut named = PROGRAM_RANGES.iter().map(|&(parameter, _)| parameter).filter(|&parameter| parameter != LEFT_OUT);
    named.nth(index)
}

/// The present value of field `number`: of the edit buffer as the messages
/// taken left it. Every channel is the one part (S53-3), so there is no
/// part number. `None` for a number past the table.
#[must_use]
pub fn read_parameter(part: &Part, number: u16) -> Option<i32> {
    let field = PARAMETERS.get(usize::from(number))?;
    Some(field.hold(i32::from(part.program().get(program_parameter(number)?))))
}
