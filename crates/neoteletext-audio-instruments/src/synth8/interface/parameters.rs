//! synth8's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.7): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! The fields are the 42 controls of a patch, in the patch's byte order
//! (S8-23), each read as its controller's value. Part 0 is the upper patch
//! and part 1 the lower (S8-18).
//!
//! The tracker keeps each half's program and the controls moved since, as
//! its restatement needs them, and not the patch itself: a control not
//! moved since the program change is read from that program in the sound's
//! bank, as the instrument read it.

use super::{BANK_INDEX, HALF_CHANNELS, MOST_PATCHES, PATCH_BYTES, Param, Patch, Tracker};
use crate::midi::{Banks, PatchField};

/// The readable fields (appendix A.7).
pub const PARAMETERS: [PatchField; PATCH_BYTES] = [
    PatchField::range("lfo_rate", 0, 127),
    PatchField::range("lfo_delay_time", 0, 127),
    PatchField::range("lfo_waveform", 0, 127),
    PatchField::range("vco_lfo_mod", 0, 127),
    PatchField::range("vco_env_mod", 0, 127),
    PatchField::range("vco_modulator", 0, 127),
    PatchField::range("pulse_width_mod", 0, 127),
    PatchField::range("pulse_width_mod_source", 0, 127),
    PatchField::range("cross_mod", 0, 127),
    PatchField::range("vco1_range", 0, 127),
    PatchField::range("vco1_waveform", 0, 127),
    PatchField::range("vco2_sync", 0, 127),
    PatchField::range("vco2_low_freq", 0, 127),
    PatchField::range("vco2_range", 0, 127),
    PatchField::range("vco2_fine_tune", 0, 127),
    PatchField::range("vco2_waveform", 0, 127),
    PatchField::range("vco1_level", 0, 127),
    PatchField::range("vco2_level", 0, 127),
    PatchField::range("hpf_cutoff", 0, 127),
    PatchField::range("vcf_cutoff", 0, 127),
    PatchField::range("vcf_resonance", 0, 127),
    PatchField::range("vcf_slope", 0, 127),
    PatchField::range("vcf_env_mod", 0, 127),
    PatchField::range("vcf_envelope_select", 0, 127),
    PatchField::range("vcf_lfo_mod", 0, 127),
    PatchField::range("vcf_key_follow", 0, 127),
    PatchField::range("vca_level", 0, 127),
    PatchField::range("vca_lfo_mod", 0, 127),
    PatchField::range("env1_attack", 0, 127),
    PatchField::range("env1_decay", 0, 127),
    PatchField::range("env1_sustain", 0, 127),
    PatchField::range("env1_release", 0, 127),
    PatchField::range("env1_polarity", 0, 127),
    PatchField::range("env2_attack", 0, 127),
    PatchField::range("env2_decay", 0, 127),
    PatchField::range("env2_sustain", 0, 127),
    PatchField::range("env2_release", 0, 127),
    PatchField::range("assign_mode", 0, 127),
    PatchField::range("env1_key_follow", 0, 127),
    PatchField::range("env2_key_follow", 0, 127),
    PatchField::range("vco1_bend", 0, 127),
    PatchField::range("vco2_bend", 0, 127),
];

/// The control that field `number` is; `None` for a number past the table.
#[must_use]
pub fn param_of(number: u16) -> Option<Param> {
    Param::ALL.get(usize::from(number)).copied()
}

/// The patch a program change to `program` sets (S8-12, S8-29): that
/// patch of bank index 0, or the default where the bank holds none there
/// or holds a faulty one (MIDI-59, MIDI-63).
#[must_use]
pub fn program_patch(banks: &Banks<'_>, program: u8) -> Patch {
    if usize::from(program) >= MOST_PATCHES {
        return Patch::DEFAULT;
    }
    banks
        .fixed_patch(BANK_INDEX, usize::from(program), PATCH_BYTES)
        .and_then(|bytes| Patch::from_slice(bytes).ok())
        .unwrap_or(Patch::DEFAULT)
}

/// The present value of field `number` of half `part`, 0 the upper patch
/// and 1 the lower: the control's value as last moved, or as the half's
/// program in `banks` has it. `None` for another part or a number past the
/// table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, banks: &Banks<'_>, part: u8, number: u16) -> Option<i32> {
    if part >= HALF_CHANNELS {
        return None;
    }
    let field = PARAMETERS.get(usize::from(number))?;
    let param = param_of(number)?;
    let (program, moved) = tracker.half(part)?;
    let value = moved.get(param.byte()).copied().flatten().unwrap_or_else(|| program_patch(banks, program).get(param));
    Some(field.hold(i32::from(value)))
}
