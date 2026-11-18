//! synth80's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.8): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! The fields are the 85 controls of a patch, each read as its controller's
//! value: the master controls first, bytes 52 to 84 of the patch, then
//! channel I's tone, bytes 0 to 25, then channel II's, bytes 26 to 51
//! (S80-33), the order of the family document's table.
//!
//! The tracker keeps how the patch was last set and the controls moved
//! since, as its restatement needs them: a control not moved since is read
//! from the patch set, which a program change takes from the sound's banks.

use super::{Base, PATCH_BYTES, Tracker, controller_of, program_patch};
use crate::midi::{Banks, PatchField};
use crate::synth80::patch::{GLOBAL_BYTES, TONE_BYTES};

/// The readable fields (appendix A.8).
pub const PARAMETERS: [PatchField; PATCH_BYTES] = [
    PatchField::range("feet_1", 0, 127),
    PatchField::range("feet_2", 0, 127),
    PatchField::range("detune_channel_2", 0, 127),
    PatchField::range("mix", 0, 127),
    PatchField::range("brilliance", 0, 127),
    PatchField::range("resonance", 0, 127),
    PatchField::range("keyboard_brilliance_low", 0, 127),
    PatchField::range("keyboard_brilliance_high", 0, 127),
    PatchField::range("keyboard_level_low", 0, 127),
    PatchField::range("keyboard_level_high", 0, 127),
    PatchField::range("sub_oscillator_function", 0, 127),
    PatchField::range("sub_oscillator_speed", 0, 127),
    PatchField::range("sub_oscillator_vco", 0, 127),
    PatchField::range("sub_oscillator_vcf", 0, 127),
    PatchField::range("sub_oscillator_vca", 0, 127),
    PatchField::range("touch_pitch_bend", 0, 127),
    PatchField::range("touch_speed", 0, 127),
    PatchField::range("touch_vco", 0, 127),
    PatchField::range("touch_vcf", 0, 127),
    PatchField::range("ring_modulator_attack_time", 0, 127),
    PatchField::range("ring_modulator_decay_time", 0, 127),
    PatchField::range("ring_modulator_depth", 0, 127),
    PatchField::range("ring_modulator_speed", 0, 127),
    PatchField::range("ring_modulator_modulation", 0, 127),
    PatchField::range("sustain_mode", 0, 127),
    PatchField::range("sustain", 0, 127),
    PatchField::range("glide_mode", 0, 127),
    PatchField::range("glide_speed", 0, 127),
    PatchField::range("tremolo_chorus", 0, 127),
    PatchField::range("chorus", 0, 127),
    PatchField::range("tremolo", 0, 127),
    PatchField::range("tremolo_chorus_speed", 0, 127),
    PatchField::range("tremolo_chorus_depth", 0, 127),
    PatchField::range("channel_1_pwm_speed", 0, 127),
    PatchField::range("channel_1_pwm", 0, 127),
    PatchField::range("channel_1_pw", 0, 127),
    PatchField::range("channel_1_square", 0, 127),
    PatchField::range("channel_1_sawtooth", 0, 127),
    PatchField::range("channel_1_noise", 0, 127),
    PatchField::range("channel_1_hpf", 0, 127),
    PatchField::range("channel_1_hpf_resonance", 0, 127),
    PatchField::range("channel_1_lpf", 0, 127),
    PatchField::range("channel_1_lpf_resonance", 0, 127),
    PatchField::range("channel_1_filter_initial_level", 0, 127),
    PatchField::range("channel_1_filter_attack_level", 0, 127),
    PatchField::range("channel_1_filter_attack", 0, 127),
    PatchField::range("channel_1_filter_decay", 0, 127),
    PatchField::range("channel_1_filter_release", 0, 127),
    PatchField::range("channel_1_vcf_level", 0, 127),
    PatchField::range("channel_1_sine", 0, 127),
    PatchField::range("channel_1_amplifier_attack", 0, 127),
    PatchField::range("channel_1_amplifier_decay", 0, 127),
    PatchField::range("channel_1_amplifier_sustain", 0, 127),
    PatchField::range("channel_1_amplifier_release", 0, 127),
    PatchField::range("channel_1_level", 0, 127),
    PatchField::range("channel_1_initial_brilliance", 0, 127),
    PatchField::range("channel_1_initial_level", 0, 127),
    PatchField::range("channel_1_after_brilliance", 0, 127),
    PatchField::range("channel_1_after_level", 0, 127),
    PatchField::range("channel_2_pwm_speed", 0, 127),
    PatchField::range("channel_2_pwm", 0, 127),
    PatchField::range("channel_2_pw", 0, 127),
    PatchField::range("channel_2_square", 0, 127),
    PatchField::range("channel_2_sawtooth", 0, 127),
    PatchField::range("channel_2_noise", 0, 127),
    PatchField::range("channel_2_hpf", 0, 127),
    PatchField::range("channel_2_hpf_resonance", 0, 127),
    PatchField::range("channel_2_lpf", 0, 127),
    PatchField::range("channel_2_lpf_resonance", 0, 127),
    PatchField::range("channel_2_filter_initial_level", 0, 127),
    PatchField::range("channel_2_filter_attack_level", 0, 127),
    PatchField::range("channel_2_filter_attack", 0, 127),
    PatchField::range("channel_2_filter_decay", 0, 127),
    PatchField::range("channel_2_filter_release", 0, 127),
    PatchField::range("channel_2_vcf_level", 0, 127),
    PatchField::range("channel_2_sine", 0, 127),
    PatchField::range("channel_2_amplifier_attack", 0, 127),
    PatchField::range("channel_2_amplifier_decay", 0, 127),
    PatchField::range("channel_2_amplifier_sustain", 0, 127),
    PatchField::range("channel_2_amplifier_release", 0, 127),
    PatchField::range("channel_2_level", 0, 127),
    PatchField::range("channel_2_initial_brilliance", 0, 127),
    PatchField::range("channel_2_initial_level", 0, 127),
    PatchField::range("channel_2_after_brilliance", 0, 127),
    PatchField::range("channel_2_after_level", 0, 127),
];

/// The bytes of the two tones, which stand before the master controls in
/// the patch (S80-33).
const TONES_BYTES: usize = 2 * TONE_BYTES;
const _: () = assert!(TONES_BYTES + GLOBAL_BYTES == PATCH_BYTES);

/// The byte of the patch, 0 to 84, that field `number` is (S80-33);
/// `None` for a number past the table.
#[must_use]
pub fn place_of_parameter(number: u16) -> Option<usize> {
    let index = usize::from(number);
    if index >= PATCH_BYTES {
        return None;
    }
    match index.checked_sub(GLOBAL_BYTES) {
        // The master controls are the first fields and the last bytes.
        None => index.checked_add(TONES_BYTES),
        Some(tone) => Some(tone),
    }
}

/// The present value of field `number`: the control's value as last moved,
/// or as the patch last set has it, a program's read from `banks`. Every
/// channel is the one instrument (S80-2), so there is no part. `None` for
/// a number past the table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, banks: &Banks<'_>, number: u16) -> Option<i32> {
    let field = PARAMETERS.get(usize::from(number))?;
    let place = place_of_parameter(number)?;
    let moved = controller_of(place).and_then(|controller| tracker.moved(controller));
    let value = moved.unwrap_or_else(|| {
        let patch = match tracker.base() {
            Base::Start => program_patch(banks, 0, 0).0,
            Base::Program { bank, program } => program_patch(banks, bank, program).0,
            Base::Inline(patch) => patch,
        };
        patch.byte(place)
    });
    Some(field.hold(i32::from(value)))
}
