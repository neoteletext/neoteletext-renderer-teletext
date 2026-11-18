//! synth106's and synth60's readable patch fields (`neoteletext-common/docs/page-rules.md`,
//! RULE-28, RULE-29, appendix A.3 and A.4): the table a page's rules name a field by
//! its place in, and the function that reads a field's present value from
//! the family's tracker. The names, order and ranges are those of the
//! program protocol's parameter type of the family, which the server
//! asserts; a number is never reused or reordered, and new fields are added
//! at the end.
//!
//! Numbers 0 to 15 are the sliders, by their parameter number (S106-13);
//! then the switches, each read as the ordinal of its position. synth60's
//! table is synth106's with the positions synth60 alone has (chorus I+II,
//! the envelope as the pulse width's source) and its sub-oscillator switch
//! (S106-31).

use super::{ChorusMode, Model, PwmSource, Range, Tracker};
use crate::midi::PatchField;
use crate::synth106::patch::SLIDERS;

/// The fields that are sliders: numbers 0 to 15.
const SLIDER_FIELDS: u16 = SLIDERS as u16;
/// The field after the sliders that is the range switch, and so on.
const RANGE: u16 = SLIDER_FIELDS;
const PULSE: u16 = RANGE + 1;
const SAWTOOTH: u16 = PULSE + 1;
const CHORUS: u16 = SAWTOOTH + 1;
const PWM: u16 = CHORUS + 1;
const POLARITY: u16 = PWM + 1;
const VCA: u16 = POLARITY + 1;
const HPF: u16 = VCA + 1;
const SUB_SWITCH: u16 = HPF + 1;

/// synth106's readable fields (appendix A.3).
pub const PARAMETERS: [PatchField; SUB_SWITCH as usize] = [
    PatchField::range("lfo_rate", 0, 127),
    PatchField::range("lfo_delay", 0, 127),
    PatchField::range("dco_lfo", 0, 127),
    PatchField::range("dco_pwm", 0, 127),
    PatchField::range("noise_level", 0, 127),
    PatchField::range("vcf_frequency", 0, 127),
    PatchField::range("vcf_resonance", 0, 127),
    PatchField::range("vcf_envelope", 0, 127),
    PatchField::range("vcf_lfo", 0, 127),
    PatchField::range("vcf_keyboard", 0, 127),
    PatchField::range("vca_level", 0, 127),
    PatchField::range("attack", 0, 127),
    PatchField::range("decay", 0, 127),
    PatchField::range("sustain", 0, 127),
    PatchField::range("release", 0, 127),
    PatchField::range("sub_level", 0, 127),
    PatchField::choice("range", &["feet_16", "feet_8", "feet_4"]),
    PatchField::choice("pulse", &["false", "true"]),
    PatchField::choice("sawtooth", &["false", "true"]),
    PatchField::choice("chorus", &["off", "one", "two"]),
    PatchField::choice("pwm", &["lfo", "manual"]),
    PatchField::choice("envelope_polarity", &["positive", "negative"]),
    PatchField::choice("vca", &["envelope", "gate"]),
    PatchField::range("hpf", 0, 3),
];

/// synth60's readable fields (appendix A.4).
pub const SYNTH60_PARAMETERS: [PatchField; SUB_SWITCH as usize + 1] = [
    PatchField::range("lfo_rate", 0, 127),
    PatchField::range("lfo_delay", 0, 127),
    PatchField::range("dco_lfo", 0, 127),
    PatchField::range("dco_pwm", 0, 127),
    PatchField::range("noise_level", 0, 127),
    PatchField::range("vcf_frequency", 0, 127),
    PatchField::range("vcf_resonance", 0, 127),
    PatchField::range("vcf_envelope", 0, 127),
    PatchField::range("vcf_lfo", 0, 127),
    PatchField::range("vcf_keyboard", 0, 127),
    PatchField::range("vca_level", 0, 127),
    PatchField::range("attack", 0, 127),
    PatchField::range("decay", 0, 127),
    PatchField::range("sustain", 0, 127),
    PatchField::range("release", 0, 127),
    PatchField::range("sub_level", 0, 127),
    PatchField::choice("range", &["feet_16", "feet_8", "feet_4"]),
    PatchField::choice("pulse", &["false", "true"]),
    PatchField::choice("sawtooth", &["false", "true"]),
    PatchField::choice("chorus", &["off", "one", "two", "both"]),
    PatchField::choice("pwm", &["lfo", "manual", "envelope"]),
    PatchField::choice("envelope_polarity", &["positive", "negative"]),
    PatchField::choice("vca", &["envelope", "gate"]),
    PatchField::range("hpf", 0, 3),
    PatchField::choice("sub_switch", &["false", "true"]),
];

/// The readable fields of `model`.
#[must_use]
pub const fn parameters(model: Model) -> &'static [PatchField] {
    match model {
        Model::Synth106 => &PARAMETERS,
        Model::Synth60 => &SYNTH60_PARAMETERS,
    }
}

/// The present value of field `number` of the tracker's model: of the
/// panel as the messages taken left it. Every channel is the one panel
/// (S106-2), so there is no part. `None` for a number past the model's
/// table.
#[must_use]
pub fn read_parameter(tracker: &Tracker, number: u16) -> Option<i32> {
    let field = parameters(tracker.model()).get(usize::from(number))?;
    let panel = tracker.panel();
    let value = match number {
        RANGE => match panel.range {
            Range::Sixteen => 0,
            Range::Eight => 1,
            Range::Four => 2,
        },
        PULSE => i32::from(panel.pulse),
        SAWTOOTH => i32::from(panel.sawtooth),
        CHORUS => match panel.chorus {
            ChorusMode::Off => 0,
            ChorusMode::One => 1,
            ChorusMode::Two => 2,
            ChorusMode::Both => 3,
        },
        PWM => match panel.pwm {
            PwmSource::Lfo => 0,
            PwmSource::Manual => 1,
            PwmSource::Envelope => 2,
        },
        POLARITY => i32::from(panel.envelope_inverted),
        VCA => i32::from(panel.vca_gate),
        HPF => i32::from(panel.hpf),
        SUB_SWITCH => i32::from(panel.sub),
        slider => i32::from(panel.parameter(u8::try_from(slider).ok()?)?),
    };
    Some(field.hold(value))
}
