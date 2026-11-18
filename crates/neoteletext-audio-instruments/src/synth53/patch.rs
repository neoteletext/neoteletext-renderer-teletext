//! The panel: the instrument modelled's 24 knobs and 22 switches, the
//! fields every voice is set from, and how a program of the adopted
//! interface sets them (S53-34 to S53-37 of
//! `neoteletext-common/docs/audio/synth53-synth52-synth51.md`).
//!
//! A program travels as the 2020 instrument's 128 bytes
//! ([`super::interface::Program`], S53-33). The panel is what the voices
//! read: each knob the number the instrument's converter makes of it, 0 to
//! 127, and each switch on or off. It is kept, as the instrument kept
//! its own programs, as 24 bytes, each a knob in its low seven bits and a
//! switch in its top bit (Sequential Circuits, "Prophet-5 Synthesizer
//! Operation Manual", CM1000D, February 1982, section 6-7, "Status 3: Save
//! to tape", pages 42 and 43 of the scan: "40 24-byte programs"; "The
//! least-significant seven bits of each byte represent a programmable
//! potentiometer setting of 0-127 steps, while the MSB represents a switch
//! setting (1=on, 0=off)"), which [`Knob`] and [`Switch`] follow. That
//! form is no longer carried anywhere: it is the panel's storage alone.

/// Bytes of the panel's storage.
///
/// Source: the operation manual, section 6-7: "40 24-byte programs".
const PATCH_BYTES: usize = 24;

/// The largest value of a knob.
///
/// Source: the operation manual, section 6-7: "a programmable
/// potentiometer setting of 0-127 steps".
pub const KNOB_MAX: u8 = 127;

/// The number the converter makes of a knob at a position of its dial,
/// given in tenths of the dial's 0 to 10: twelve steps to each mark, as
/// revision 3 is trimmed, on which the dial's 10 is 120 and the numbers
/// above it are beyond the knob's travel.
///
/// Source: TM3 adjustment 4-14 (page 67): the converter's gain is set so
/// that the cutoff knob at 10 gives "as close to 10.000V as the 83-mV
/// quantized voltage steps will allow", 120 steps.
#[must_use]
pub const fn dial(tenths: u8) -> u8 {
    let steps = (tenths as u16).saturating_mul(12).saturating_add(5) / 10;
    if steps > KNOB_MAX as u16 { KNOB_MAX } else { steps as u8 }
}

/// The bit of a byte that holds its switch.
///
/// Source: the same sentence: "the MSB represents a switch setting".
const SWITCH_BIT: u8 = 0x80;

/// The knobs, each named by the byte whose low seven bits hold it.
///
/// Source: the operation manual, section 6-7, the table of data bytes
/// (page 43 of the scan).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Knob {
    /// The filter envelope's attack.
    FilterAttack = 0,
    /// The filter envelope's decay.
    FilterDecay = 1,
    /// The filter envelope's sustain level.
    FilterSustain = 2,
    /// The filter envelope's release.
    FilterRelease = 3,
    /// The amplifier envelope's attack.
    AmplifierAttack = 4,
    /// The amplifier envelope's decay.
    AmplifierDecay = 5,
    /// The amplifier envelope's sustain level.
    AmplifierSustain = 6,
    /// The amplifier envelope's release.
    AmplifierRelease = 7,
    /// The filter's cutoff.
    Cutoff = 8,
    /// How far the filter envelope moves the cutoff.
    FilterEnvelopeAmount = 9,
    /// The mixer's level of oscillator B.
    MixerOscB = 10,
    /// Oscillator B's pulse width.
    OscBPulseWidth = 11,
    /// The mixer's level of oscillator A.
    MixerOscA = 12,
    /// Oscillator A's pulse width.
    OscAPulseWidth = 13,
    /// The mixer's level of the noise.
    MixerNoise = 14,
    /// The filter's resonance.
    Resonance = 15,
    /// The glide's rate.
    Glide = 16,
    /// The low-frequency oscillator's frequency.
    LfoFrequency = 17,
    /// Wheel-Mod's source mix: the low-frequency oscillator at 0, noise
    /// at the top.
    SourceMix = 18,
    /// Poly-Mod's amount of oscillator B.
    PolyModOscB = 19,
    /// Poly-Mod's amount of the filter envelope.
    PolyModFilterEnvelope = 20,
    /// Oscillator A's frequency.
    OscAFrequency = 21,
    /// Oscillator B's frequency.
    OscBFrequency = 22,
    /// Oscillator B's fine tuning, upwards from the frequency knob's
    /// pitch.
    OscBFine = 23,
}

/// The switches, each named by the byte whose top bit holds it.
///
/// Source: the operation manual, section 6-7, the same table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Switch {
    /// Oscillator A's pulse.
    OscAPulse = 0,
    /// Oscillator A's sawtooth.
    OscASawtooth = 1,
    /// Sync: oscillator A follows oscillator B.
    Sync = 2,
    /// Oscillator B's sawtooth.
    OscBSawtooth = 3,
    /// Oscillator B's triangle.
    OscBTriangle = 4,
    /// Oscillator B's pulse.
    OscBPulse = 5,
    /// Oscillator B's keyboard switch: off, it ignores the keys.
    OscBKeyboard = 6,
    /// Unison.
    Unison = 7,
    /// Poly-Mod to oscillator A's frequency.
    PolyModFrequencyA = 8,
    /// Poly-Mod to oscillator A's pulse width.
    PolyModPulseWidthA = 9,
    /// Poly-Mod to the filter's cutoff.
    PolyModFilter = 10,
    /// The low-frequency oscillator's sawtooth.
    LfoSawtooth = 11,
    /// The low-frequency oscillator's triangle.
    LfoTriangle = 12,
    /// The low-frequency oscillator's square.
    LfoSquare = 13,
    /// The filter's keyboard switch: on, the cutoff follows the keys.
    FilterKeyboard = 14,
    /// Release: off, both envelopes' releases are as short as they go,
    /// whatever their knobs say, unless the footswitch is down.
    Release = 15,
    /// Wheel-Mod to oscillator A's frequency.
    WheelFrequencyA = 16,
    /// Wheel-Mod to oscillator B's frequency.
    WheelFrequencyB = 17,
    /// Wheel-Mod to oscillator A's pulse width.
    WheelPulseWidthA = 18,
    /// Wheel-Mod to oscillator B's pulse width.
    WheelPulseWidthB = 19,
    /// Wheel-Mod to the filter's cutoff.
    WheelFilter = 20,
    /// Oscillator B's low-frequency switch.
    OscBLowFrequency = 21,
}

/// The first byte whose top bit holds no switch.
///
/// Source: the operation manual's table marks the switch of bytes 22
/// and 23 "X".
const SWITCH_BYTES: usize = 22;
const _: () = assert!(Switch::OscBLowFrequency as usize == SWITCH_BYTES - 1);
const _: () = assert!(Knob::OscBFine as usize == PATCH_BYTES - 1);

/// The panel. Every value of every field is a setting the instrument can
/// play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    bytes: [u8; PATCH_BYTES],
}

impl Patch {
    /// A plain starting sound, the panel of the default program (S53-30): oscillator A's sawtooth, two octaves up
    /// its knob, through an open filter that follows the keys; both
    /// envelopes at full sustain; the release switch on.
    ///
    /// Source: own choice.
    #[allow(
        clippy::indexing_slicing,
        reason = "worked out by the compiler: a fault here stops the build, and no program"
    )]
    pub const DEFAULT: Self = {
        let mut bytes = [0u8; PATCH_BYTES];
        let middle = KNOB_MAX / 2 + 1;
        bytes[Knob::OscAFrequency as usize] = middle;
        bytes[Knob::OscBFrequency as usize] = middle;
        bytes[Knob::OscAPulseWidth as usize] = middle;
        bytes[Knob::OscBPulseWidth as usize] = middle;
        bytes[Knob::MixerOscA as usize] = KNOB_MAX;
        bytes[Knob::Cutoff as usize] = KNOB_MAX;
        bytes[Knob::FilterSustain as usize] = KNOB_MAX;
        bytes[Knob::AmplifierSustain as usize] = KNOB_MAX;
        bytes[Switch::OscASawtooth as usize] |= SWITCH_BIT;
        bytes[Switch::OscBKeyboard as usize] |= SWITCH_BIT;
        bytes[Switch::FilterKeyboard as usize] |= SWITCH_BIT;
        bytes[Switch::LfoTriangle as usize] |= SWITCH_BIT;
        bytes[Switch::Release as usize] |= SWITCH_BIT;
        Self { bytes }
    };

    /// The panel of its 24 bytes. Every value of every byte is a setting
    /// of a knob and a switch; the top bits of the last two bytes, which
    /// hold no switch, are not kept.
    #[must_use]
    pub(super) fn from_stored(bytes: &[u8; PATCH_BYTES]) -> Self {
        let mut kept = *bytes;
        for byte in kept.iter_mut().skip(SWITCH_BYTES) {
            *byte &= KNOB_MAX;
        }
        Self { bytes: kept }
    }

    /// The panel as its 24 bytes.
    #[must_use]
    pub(super) const fn stored(&self) -> [u8; PATCH_BYTES] {
        self.bytes
    }

    /// A knob's value, 0 to [`KNOB_MAX`].
    #[must_use]
    pub fn get(&self, knob: Knob) -> u8 {
        self.bytes.get(usize::from(knob as u8)).map_or(0, |byte| byte & KNOB_MAX)
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, switch: Switch) -> bool {
        self.bytes.get(usize::from(switch as u8)).is_some_and(|byte| byte & SWITCH_BIT != 0)
    }

    /// Sets a knob; a value above [`KNOB_MAX`] is taken as it.
    pub fn set(&mut self, knob: Knob, value: u8) {
        if let Some(byte) = self.bytes.get_mut(usize::from(knob as u8)) {
            *byte = (*byte & SWITCH_BIT) | value.min(KNOB_MAX);
        }
    }

    /// Sets a switch.
    pub fn switch(&mut self, switch: Switch, on: bool) {
        if let Some(byte) = self.bytes.get_mut(usize::from(switch as u8)) {
            *byte = (*byte & KNOB_MAX) | if on { SWITCH_BIT } else { 0 };
        }
    }

    /// The program with a knob set: for building one in a chain.
    #[must_use]
    pub fn with(mut self, knob: Knob, value: u8) -> Self {
        self.set(knob, value);
        self
    }

    /// The program with a switch set: for building one in a chain.
    #[must_use]
    pub fn with_switch(mut self, switch: Switch, on: bool) -> Self {
        self.switch(switch, on);
        self
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The knobs and the parameter of the adopted interface that sets each
/// (S53-37; the table of section 10.2 of the family document).
pub const KNOB_PARAMETERS: [(Knob, u8); 24] = {
    use super::interface::parameter as p;
    [
        (Knob::OscAFrequency, p::OSC_A_FREQUENCY),
        (Knob::OscBFrequency, p::OSC_B_FREQUENCY),
        (Knob::OscBFine, p::OSC_B_FINE_TUNE),
        (Knob::OscAPulseWidth, p::OSC_A_PULSE_WIDTH),
        (Knob::OscBPulseWidth, p::OSC_B_PULSE_WIDTH),
        (Knob::Glide, p::GLIDE_RATE),
        (Knob::MixerOscA, p::OSC_A_LEVEL),
        (Knob::MixerOscB, p::OSC_B_LEVEL),
        (Knob::MixerNoise, p::NOISE_LEVEL),
        (Knob::Cutoff, p::CUTOFF),
        (Knob::Resonance, p::RESONANCE),
        (Knob::LfoFrequency, p::LFO_FREQUENCY),
        (Knob::SourceMix, p::LFO_SOURCE_MIX),
        (Knob::PolyModFilterEnvelope, p::POLY_MOD_FILT_ENV_AMOUNT),
        (Knob::PolyModOscB, p::POLY_MOD_OSC_B_AMOUNT),
        (Knob::FilterEnvelopeAmount, p::ENV_FILTER_AMOUNT),
        (Knob::FilterAttack, p::ATTACK_FILTER),
        (Knob::AmplifierAttack, p::ATTACK_VCA),
        (Knob::FilterDecay, p::DECAY_FILTER),
        (Knob::AmplifierDecay, p::DECAY_VCA),
        (Knob::FilterSustain, p::SUSTAIN_FILTER),
        (Knob::AmplifierSustain, p::SUSTAIN_VCA),
        (Knob::FilterRelease, p::RELEASE_FILTER),
        (Knob::AmplifierRelease, p::RELEASE_VCA),
    ]
};

/// The switches and the parameter that sets each (S53-37). The filter's
/// keyboard switch is on at FILTER KEYBOARD TRACK's half and full
/// (S53-41).
pub const SWITCH_PARAMETERS: [(Switch, u8); 22] = {
    use super::interface::parameter as p;
    [
        (Switch::OscASawtooth, p::OSC_A_SAW),
        (Switch::OscAPulse, p::OSC_A_SQUARE),
        (Switch::OscBSawtooth, p::OSC_B_SAW),
        (Switch::OscBTriangle, p::OSC_B_TRI),
        (Switch::OscBPulse, p::OSC_B_SQUARE),
        (Switch::Sync, p::OSC_SYNC),
        (Switch::OscBLowFrequency, p::OSC_B_LOW_FREQ),
        (Switch::OscBKeyboard, p::OSC_B_KEYBOARD),
        (Switch::FilterKeyboard, p::FILTER_KEYBOARD_TRACK),
        (Switch::LfoSawtooth, p::LFO_SAW),
        (Switch::LfoTriangle, p::LFO_TRI),
        (Switch::LfoSquare, p::LFO_SQUARE),
        (Switch::WheelFrequencyA, p::LFO_FREQ_A),
        (Switch::WheelFrequencyB, p::LFO_FREQ_B),
        (Switch::WheelPulseWidthA, p::LFO_FREQ_PW_A),
        (Switch::WheelPulseWidthB, p::LFO_FREQ_PW_B),
        (Switch::WheelFilter, p::LFO_FILTER),
        (Switch::PolyModFrequencyA, p::POLY_MOD_FREQ_A),
        (Switch::PolyModPulseWidthA, p::POLY_MOD_PW),
        (Switch::PolyModFilter, p::POLY_MOD_FILTER),
        (Switch::Release, p::RELEASE_SWITCH),
        (Switch::Unison, p::UNISON),
    ]
};

// S53-37: every knob and every switch is set by exactly one parameter, and
// no parameter sets two fields.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "worked out by the compiler: a fault here stops the build, and no program"
)]
const _: () = {
    let mut knobs = [false; PATCH_BYTES];
    let mut switches = [false; SWITCH_BYTES];
    let mut parameters = [false; super::interface::PARAMETER_SPAN];
    let mut index = 0;
    while index < KNOB_PARAMETERS.len() {
        let (knob, parameter) = KNOB_PARAMETERS[index];
        assert!(!knobs[knob as usize] && !parameters[parameter as usize]);
        knobs[knob as usize] = true;
        parameters[parameter as usize] = true;
        index += 1;
    }
    index = 0;
    while index < SWITCH_PARAMETERS.len() {
        let (switch, parameter) = SWITCH_PARAMETERS[index];
        assert!(!switches[switch as usize] && !parameters[parameter as usize]);
        switches[switch as usize] = true;
        parameters[parameter as usize] = true;
        index += 1;
    }
    assert!(KNOB_PARAMETERS.len() == PATCH_BYTES && SWITCH_PARAMETERS.len() == SWITCH_BYTES);
};

/// The panel's number of a knob parameter's value on a model (S53-35,
/// S53-36): on synth53 the value itself, one value a dial step and 120 the
/// dial's 10; on synth52 and synth51, whose converter puts the dial's 10 at
/// 127, `floor((value × 127 + 60) ÷ 120)`. A parameter of range 0 to 127
/// (OSC B FINE TUNE, POLY MOD FILT ENV AMOUNT) is the value on every
/// model.
#[must_use]
pub fn knob_of_value(value: u8, maximum: u8, scaled: bool) -> u8 {
    let top = super::interface::KNOB_TOP;
    if !scaled || maximum != top {
        return value.min(KNOB_MAX);
    }
    let value = u16::from(value.min(top));
    let field =
        value.saturating_mul(u16::from(KNOB_MAX)).saturating_add(u16::from(top / 2)).checked_div(u16::from(top));
    field.and_then(|field| u8::try_from(field).ok()).unwrap_or(KNOB_MAX)
}

/// The parameter value a writer gives for a panel number on synth52 and
/// synth51 (S53-36): `floor((field × 120 + 63) ÷ 127)`, which
/// [`knob_of_value`] returns as the nearest number a parameter can
/// express.
#[must_use]
pub fn value_of_knob(field: u8) -> u8 {
    let top = super::interface::KNOB_TOP;
    let field = u16::from(field.min(KNOB_MAX));
    let value =
        field.saturating_mul(u16::from(top)).saturating_add(u16::from(KNOB_MAX / 2)).checked_div(u16::from(KNOB_MAX));
    value.and_then(|value| u8::try_from(value).ok()).unwrap_or(top)
}

impl Patch {
    /// The panel a program sets on a model (S53-35 to S53-37): each knob
    /// from its parameter on the model's scale, each switch on where its
    /// parameter is not 0, every parameter byte held to its maximum first
    /// (S53-33).
    #[must_use]
    pub fn of_program(program: &super::interface::Program, model: super::Model) -> Self {
        let scaled = !matches!(model, super::Model::Synth53);
        let mut patch = Self { bytes: [0; PATCH_BYTES] };
        for (knob, parameter) in KNOB_PARAMETERS {
            let maximum = super::interface::maximum(u16::from(parameter)).unwrap_or(KNOB_MAX);
            patch.set(knob, knob_of_value(program.get(parameter), maximum, scaled));
        }
        for (switch, parameter) in SWITCH_PARAMETERS {
            patch.switch(switch, program.get(parameter) != 0);
        }
        patch
    }
}
