//! A half's panel in the units the instrument is built in: what the
//! voices read, and what a slider slides through.
//!
//! A [`Patch`] is the 42 control-change values of `synth8.md` (S8-23);
//! [`Panel::of`] turns each into the unit the laws of [`super::law`] were
//! written in, by S8-21: a slider a byte of 0 to 255, `floor((v × 255 +
//! 63) ÷ 127)` (item 1); a modulation slider the signed byte `128 + v`,
//! 128 being none (item 2); the fine tune the signed byte of item 3; a
//! selector or a switch the position the instrument has, the reference
//! manual's positions it lacks folded to the nearest (item 4), counted as
//! the voices count them:
//!
//! | Control | Panel byte |
//! |---|---|
//! | LFO WAVE FORM | 0 sine, 1 sawtooth, 2 square, 3 random |
//! | VCO MODULATOR switch | 0 VCO-1, 1 both, 2 VCO-2 |
//! | PULSE WIDTH MOD source | 0 LFO, 1 manual, 2 envelope 1 |
//! | VCO-1 RANGE | 0 16′, 1 8′, 2 4′, 3 2′ |
//! | VCO-1 WAVEFORM | 0 triangle, 1 sawtooth, 2 pulse, 3 square |
//! | VCO-2 RANGE | semitones above 16′, 0 to 36 |
//! | VCO-2 WAVEFORM | 0 sine, 1 sawtooth, 2 pulse, 3 noise |
//! | VCF SLOPE | 1 for 12 dB an octave (the two-pole tap), 0 for 24 |
//! | VCA LFO MOD | the switch's position, 0 to 3 |
//! | ASSIGN MODE | 0 poly 1, 1 solo, 2 unison, 3 poly 2 |
//! | Every other switch | 0 off, 1 on |

use super::patch::{
    AssignMode, Kind, LfoWave, PATCH_BYTES, Param, Patch, PwmSource, Vco1Wave, Vco2Wave, fine_tune_signed, slider_byte,
};

/// What a signed byte holds for none.
pub const SIGNED_ZERO: u8 = 128;

/// One half's panel in the instrument's units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Panel {
    bytes: [u8; PATCH_BYTES],
}

impl Panel {
    /// The panel a patch's values give (S8-21).
    #[must_use]
    pub fn of(patch: &Patch) -> Self {
        let mut bytes = [0u8; PATCH_BYTES];
        for (byte, param) in bytes.iter_mut().zip(Param::ALL) {
            *byte = engine(patch, param);
        }
        Self { bytes }
    }

    /// The panel's bytes.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; PATCH_BYTES] {
        self.bytes
    }

    /// The panel of bytes as they are, for a slide's step.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; PATCH_BYTES]) -> Self {
        Self { bytes }
    }

    /// Whether a control is a slider or a knob, which passes every place
    /// between two, and not a switch or a selector, which has none
    /// between (S8-24).
    #[must_use]
    pub const fn slides(param: Param) -> bool {
        matches!(param.kind(), Kind::Slider | Kind::Modulation | Kind::Centred)
    }

    /// A control's byte.
    #[must_use]
    pub fn get(&self, param: Param) -> u8 {
        self.bytes.get(param.byte()).copied().unwrap_or(0)
    }

    /// A signed control's value, −128 to 127: its byte less 128.
    #[must_use]
    pub fn signed(&self, param: Param) -> i32 {
        i32::from(self.get(param)).saturating_sub(i32::from(SIGNED_ZERO))
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, param: Param) -> bool {
        self.get(param) != 0
    }

    /// The panel with a control's byte set as it is: for the voice's own
    /// tests, which work in the instrument's units.
    #[cfg(test)]
    #[must_use]
    pub fn with(mut self, param: Param, byte: u8) -> Self {
        if let Some(kept) = self.bytes.get_mut(param.byte()) {
            *kept = byte;
        }
        self
    }

    /// The low-frequency oscillator's waveform.
    #[must_use]
    pub fn lfo_wave(&self) -> LfoWave {
        match self.get(Param::LfoWave) {
            0 => LfoWave::Sine,
            1 => LfoWave::Sawtooth,
            2 => LfoWave::Square,
            _ => LfoWave::Random,
        }
    }

    /// VCO-1's waveform.
    #[must_use]
    pub fn vco1_wave(&self) -> Vco1Wave {
        match self.get(Param::Vco1Wave) {
            0 => Vco1Wave::Triangle,
            1 => Vco1Wave::Sawtooth,
            2 => Vco1Wave::Pulse,
            _ => Vco1Wave::Square,
        }
    }

    /// VCO-2's waveform.
    #[must_use]
    pub fn vco2_wave(&self) -> Vco2Wave {
        match self.get(Param::Vco2Wave) {
            0 => Vco2Wave::Sine,
            1 => Vco2Wave::Sawtooth,
            2 => Vco2Wave::Pulse,
            _ => Vco2Wave::Noise,
        }
    }

    /// Whether the VCO modulator reaches VCO-1 and VCO-2.
    #[must_use]
    pub fn modulated(&self) -> (bool, bool) {
        match self.get(Param::VcoModulator) {
            0 => (true, false),
            1 => (true, true),
            _ => (false, true),
        }
    }

    /// What moves the pulse's width.
    #[must_use]
    pub fn pwm_source(&self) -> PwmSource {
        match self.get(Param::PwmSource) {
            0 => PwmSource::Lfo,
            1 => PwmSource::Manual,
            _ => PwmSource::Envelope,
        }
    }

    /// How keys are given voices.
    #[must_use]
    pub fn assign_mode(&self) -> AssignMode {
        match self.get(Param::AssignMode) {
            0 => AssignMode::Poly1,
            1 => AssignMode::Solo,
            2 => AssignMode::Unison,
            _ => AssignMode::Poly2,
        }
    }
}

impl Default for Panel {
    fn default() -> Self {
        Self::of(&Patch::DEFAULT)
    }
}

/// The byte of the instrument's units control `param` of `patch` gives
/// (S8-21).
fn engine(patch: &Patch, param: Param) -> u8 {
    let value = patch.get(param);
    let index = |position: usize| u8::try_from(position).unwrap_or(0);
    match param {
        Param::LfoWave => index(patch.lfo_wave() as usize),
        Param::VcoModulator => index(patch.vco_modulator() as usize),
        Param::PwmSource => index(patch.pwm_source() as usize),
        Param::Vco1Range => patch.vco1_range().index(),
        Param::Vco1Wave => index(patch.vco1_wave() as usize),
        Param::Vco2Range => patch.vco2_range(),
        Param::Vco2Wave => index(patch.vco2_wave() as usize),
        // The first position the manual lists is −12 dB, the filter's
        // two-pole tap (S8-21 item 5).
        Param::VcfSlope => u8::from(!patch.on(param)),
        Param::VcaLfoMod => patch.vca_lfo_mod(),
        Param::AssignMode => index(patch.assign_mode() as usize),
        _ => match param.kind() {
            Kind::Slider => slider_byte(value),
            Kind::Modulation => SIGNED_ZERO.saturating_add(value),
            Kind::Centred => {
                let signed = fine_tune_signed(value).saturating_add(i32::from(SIGNED_ZERO));
                u8::try_from(signed.clamp(0, i32::from(u8::MAX))).unwrap_or(SIGNED_ZERO)
            }
            Kind::Selector(_) | Kind::Switch => u8::from(patch.on(param)),
        },
    }
}
