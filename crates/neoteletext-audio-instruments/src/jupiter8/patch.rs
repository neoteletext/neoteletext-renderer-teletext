//! A patch: the panel of one of the instrument's two halves as a fixed
//! record.
//!
//! The Jupiter-8 has no MIDI and no documented patch format: its
//! memories go to tape as tones. The record here follows Roland's
//! "JUPITER-8/SYSTEM-8 Parameter Correspondence Table" (2016), which
//! numbers every control of the original's panel that a patch holds and
//! gives each a range: [`Param`] lists them in that table's order, one
//! byte each, and [`Patch::from_bytes`] checks every byte.
//!
//! # The bytes
//!
//! | Byte | Table | Control | Values |
//! |---|---|---|---|
//! | 0 | 1 | LFO rate | 0 to 255 |
//! | 1 | 2 | LFO delay time | 0 to 255 |
//! | 2 | 3 | LFO waveform | 0 sine, 1 sawtooth, 2 square, 3 random |
//! | 3 | 4 | VCO modulator, LFO MOD | signed |
//! | 4 | 5 | VCO modulator, ENV MOD | signed |
//! | 5 | 6 | The modulator reaches VCO-1 | 0 no, 1 yes |
//! | 6 | 7 | The modulator reaches VCO-2 | 0 no, 1 yes |
//! | 7 | 8 | Pulse width modulation | 0 to 255 |
//! | 8 | 9 | Pulse width's source | 0 LFO, 1 manual, 2 envelope 1 |
//! | 9 | 10 | VCO-1 cross modulation | 0 to 255 |
//! | 10 | 11 | VCO-1 range | 0 16', 1 8', 2 4', 3 2' |
//! | 11 | 12 | VCO-1 waveform | 0 triangle, 1 sawtooth, 2 pulse, 3 square |
//! | 12 | 13 | VCO-2 sync | 0 off, 1 on |
//! | 13 | 14 | VCO-2 low frequency | 0 normal, 1 low |
//! | 14 | 15 | VCO-2 range | semitones above 16', 0 to 36 |
//! | 15 | 16 | VCO-2 fine tune | signed |
//! | 16 | 17 | VCO-2 waveform | 0 sine, 1 sawtooth, 2 pulse, 3 noise |
//! | 17 | 18 | Mixer, VCO-1 | 0 to 255 |
//! | 18 | 18 | Mixer, VCO-2 | 0 to 255 |
//! | 19 | 19 | HPF cutoff | 0 to 255 |
//! | 20 | 20 | VCF cutoff | 0 to 255 |
//! | 21 | 21 | VCF resonance | 0 to 255 |
//! | 22 | 22 | VCF slope | 0 24 dB, 1 12 dB an octave |
//! | 23 | 23 | VCF ENV MOD | signed |
//! | 24 | 24 | VCF envelope | 0 envelope 1, 1 envelope 2 |
//! | 25 | 25 | VCF LFO MOD | signed |
//! | 26 | 26 | VCF key follow | signed |
//! | 27 | 27 | VCA level | 0 to 255 |
//! | 28 | 28 | VCA LFO MOD | signed |
//! | 29 to 32 | 29 to 32 | Envelope 1: attack, decay, sustain, release | 0 to 255 |
//! | 33 | 33 | Envelope 1 polarity | 0 normal, 1 inverted |
//! | 34 to 37 | 34 to 37 | Envelope 2: attack, decay, sustain, release | 0 to 255 |
//! | 38 | 40 | Assign mode | 0 poly 1, 1 solo, 2 unison, 3 poly 2 |
//! | 39 | 41 | Envelope 1 key follow | 0 off, 1 on |
//! | 40 | 42 | Envelope 2 key follow | 0 off, 1 on |
//! | 41 | 43 | The bender reaches VCO-1 | 0 no, 1 yes |
//! | 42 | 44 | The bender reaches VCO-2 | 0 no, 1 yes |
//!
//! A signed byte is the table's -128 to +127 with 128 added, so 128 is
//! none. The original's sliders run one way only, from none at 0 to the
//! most at 10: they are 128 to 255 here, and what is below 128 is the
//! table's other half, the same depth with the modulation upside down.
//! The fine tune is the exception, whose knob turns both ways on the
//! original too.
//!
//! What the table has and the record leaves out: the arpeggio's mode and
//! range (numbers 38 and 39), there being no arpeggiator here; the
//! SYSTEM-8's 64' and 32' ranges, its 18 dB slope, its second random
//! waveform and its triangle, none of which the original has. The
//! original's source mixer is one knob, and the table's two levels:
//! [`Patch::set_source_mix`] sets both as the knob does.

/// Bytes of a patch.
pub const PATCH_BYTES: usize = 43;

/// What a signed byte holds for none.
pub const SIGNED_ZERO: u8 = 128;

/// The controls of a patch, each named by its byte.
///
/// Source: Roland, "JUPITER-8/SYSTEM-8 Parameter Correspondence Table",
/// Parameter List; the module's documentation gives each byte's table
/// number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Param {
    /// The low-frequency oscillator's rate.
    LfoRate = 0,
    /// The low-frequency oscillator's delay time.
    LfoDelay = 1,
    /// The low-frequency oscillator's waveform ([`LfoWave`]).
    LfoWave = 2,
    /// How far the low-frequency oscillator moves the pitch; signed.
    VcoLfoMod = 3,
    /// How far envelope 1 moves the pitch; signed.
    VcoEnvMod = 4,
    /// Whether the two reach VCO-1.
    Vco1Mod = 5,
    /// Whether the two reach VCO-2.
    Vco2Mod = 6,
    /// The pulse's width, or how far its source moves it.
    PwmDepth = 7,
    /// What moves the pulse's width ([`PwmSource`]).
    PwmSource = 8,
    /// How far VCO-2 moves VCO-1's pitch.
    CrossMod = 9,
    /// VCO-1's range, 16' to 2'.
    Vco1Range = 10,
    /// VCO-1's waveform ([`Vco1Wave`]).
    Vco1Wave = 11,
    /// Whether VCO-1 starts VCO-2's cycle.
    Sync = 12,
    /// Whether VCO-2 is in its low range, off the keyboard.
    LowFrequency = 13,
    /// VCO-2's range in semitones above 16'.
    Vco2Range = 14,
    /// VCO-2's fine tune; signed.
    FineTune = 15,
    /// VCO-2's waveform ([`Vco2Wave`]).
    Vco2Wave = 16,
    /// VCO-1's level in the mixer.
    MixVco1 = 17,
    /// VCO-2's level in the mixer.
    MixVco2 = 18,
    /// The high-pass filter's cutoff.
    HpfCutoff = 19,
    /// The low-pass filter's cutoff.
    VcfCutoff = 20,
    /// The low-pass filter's resonance.
    VcfResonance = 21,
    /// The low-pass filter's slope: 0 is 24 dB an octave, 1 is 12.
    VcfSlope = 22,
    /// How far an envelope moves the cutoff; signed.
    VcfEnvMod = 23,
    /// Which envelope: 0 is envelope 1, 1 is envelope 2.
    VcfEnvSource = 24,
    /// How far the low-frequency oscillator moves the cutoff; signed.
    VcfLfoMod = 25,
    /// How far the cutoff follows the keyboard; signed.
    VcfKeyFollow = 26,
    /// The amplifier's level.
    VcaLevel = 27,
    /// How far the low-frequency oscillator moves the amplifier; signed.
    VcaLfoMod = 28,
    /// Envelope 1's attack time.
    Env1Attack = 29,
    /// Envelope 1's decay time.
    Env1Decay = 30,
    /// Envelope 1's sustain level.
    Env1Sustain = 31,
    /// Envelope 1's release time.
    Env1Release = 32,
    /// Whether envelope 1 is upside down.
    Env1Polarity = 33,
    /// Envelope 2's attack time.
    Env2Attack = 34,
    /// Envelope 2's decay time.
    Env2Decay = 35,
    /// Envelope 2's sustain level.
    Env2Sustain = 36,
    /// Envelope 2's release time.
    Env2Release = 37,
    /// How keys are given voices ([`AssignMode`]).
    AssignMode = 38,
    /// Whether envelope 1's times follow the keyboard.
    Env1KeyFollow = 39,
    /// Whether envelope 2's times follow the keyboard.
    Env2KeyFollow = 40,
    /// Whether the bender moves VCO-1.
    Vco1Bend = 41,
    /// Whether the bender moves VCO-2.
    Vco2Bend = 42,
}

/// The largest value of a switch of two positions.
const SWITCH_MAX: u8 = 1;
/// The largest value of a selector of four positions.
///
/// Source: owner's manual, printed pages 15 to 17 and 12: each VCO and
/// the low-frequency oscillator have four waveforms, VCO-1 four ranges,
/// and there are four assign modes.
const SELECTOR_MAX: u8 = 3;
/// The largest value of the pulse width's source.
///
/// Source: owner's manual, printed page 17: LFO, manual, envelope 1.
const PWM_SOURCE_MAX: u8 = 2;
/// The largest value of VCO-2's range: three octaves of semitones.
///
/// Source: owner's manual, printed page 16: VCO-2's range "covers the
/// same pitch range as VCO-1, from 16' to 2' ... quantized so that the
/// pitch will change in half steps".
pub const VCO2_RANGE_MAX: u8 = 36;

/// The largest value of each byte, in the bytes' order.
pub(super) const MAXIMA: [u8; PATCH_BYTES] = {
    let mut maxima = [u8::MAX; PATCH_BYTES];
    let limited = [
        (Param::LfoWave, SELECTOR_MAX),
        (Param::Vco1Mod, SWITCH_MAX),
        (Param::Vco2Mod, SWITCH_MAX),
        (Param::PwmSource, PWM_SOURCE_MAX),
        (Param::Vco1Range, SELECTOR_MAX),
        (Param::Vco1Wave, SELECTOR_MAX),
        (Param::Sync, SWITCH_MAX),
        (Param::LowFrequency, SWITCH_MAX),
        (Param::Vco2Range, VCO2_RANGE_MAX),
        (Param::Vco2Wave, SELECTOR_MAX),
        (Param::VcfSlope, SWITCH_MAX),
        (Param::VcfEnvSource, SWITCH_MAX),
        (Param::Env1Polarity, SWITCH_MAX),
        (Param::AssignMode, SELECTOR_MAX),
        (Param::Env1KeyFollow, SWITCH_MAX),
        (Param::Env2KeyFollow, SWITCH_MAX),
        (Param::Vco1Bend, SWITCH_MAX),
        (Param::Vco2Bend, SWITCH_MAX),
    ];
    let mut index = 0;
    while index < limited.len() {
        #[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build")]
        {
            maxima[limited[index].0 as usize] = limited[index].1;
        }
        index = index.wrapping_add(1);
    }
    maxima
};

/// The low-frequency oscillator's waveforms.
///
/// Source: owner's manual, printed page 17: "a Sine Wave, inverted
/// Sawtooth wave, Square wave and a Random output from the Sample & Hold
/// circuit".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LfoWave {
    /// A sine.
    Sine,
    /// A sawtooth that falls.
    Sawtooth,
    /// A square.
    Square,
    /// A new random level each cycle.
    Random,
}

/// VCO-1's waveforms.
///
/// Source: owner's manual, printed pages 15 and 16.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vco1Wave {
    /// The triangle.
    Triangle,
    /// The sawtooth.
    Sawtooth,
    /// The pulse, whose width the VCO modulator sets.
    Pulse,
    /// The square.
    Square,
}

/// VCO-2's waveforms.
///
/// Source: owner's manual, printed page 16.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vco2Wave {
    /// The sine.
    Sine,
    /// The sawtooth.
    Sawtooth,
    /// The pulse, whose width the VCO modulator sets.
    Pulse,
    /// Noise.
    Noise,
}

/// What moves the pulse's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwmSource {
    /// The low-frequency oscillator, as far as the slider says.
    Lfo,
    /// Nothing: the slider is the width.
    Manual,
    /// Envelope 1, as far as the slider says.
    Envelope,
}

/// How keys are given voices.
///
/// Source: owner's manual, printed page 12, "Assign Modes".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssignMode {
    /// A voice for each key; a released voice rings on.
    Poly1,
    /// One voice, the lowest key held.
    Solo,
    /// Every voice, shared among the keys held.
    Unison,
    /// A voice for each key; a new key silences what was released.
    Poly2,
}

/// Why bytes are no patch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// The byte at this place is above what its control can hold.
    Value(u8),
}

/// One patch. Every value it can hold is a patch the instrument can
/// play: [`Patch::set`] holds each control to its range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    bytes: [u8; PATCH_BYTES],
}

impl Patch {
    /// The panel as the owner's manual sets it to begin with: VCO-1's
    /// sawtooth alone at 8' through an open filter, the amplifier's
    /// envelope at full sustain, no modulation.
    ///
    /// Source: owner's manual, printed page 8, figure 1, "basic tuning
    /// patch", as nearly as its drawing can be read; own choice where it
    /// cannot.
    #[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build")]
    pub const DEFAULT: Self = {
        let mut bytes = [0u8; PATCH_BYTES];
        bytes[Param::VcoLfoMod as usize] = SIGNED_ZERO;
        bytes[Param::VcoEnvMod as usize] = SIGNED_ZERO;
        bytes[Param::Vco1Mod as usize] = 1;
        bytes[Param::Vco2Mod as usize] = 1;
        bytes[Param::PwmSource as usize] = 1;
        bytes[Param::Vco1Range as usize] = 1;
        bytes[Param::Vco1Wave as usize] = 1;
        bytes[Param::Vco2Range as usize] = 12;
        bytes[Param::FineTune as usize] = SIGNED_ZERO;
        bytes[Param::Vco2Wave as usize] = 1;
        bytes[Param::MixVco1 as usize] = u8::MAX;
        bytes[Param::VcfCutoff as usize] = u8::MAX;
        bytes[Param::VcfEnvMod as usize] = SIGNED_ZERO;
        bytes[Param::VcfLfoMod as usize] = SIGNED_ZERO;
        bytes[Param::VcfKeyFollow as usize] = SIGNED_ZERO;
        bytes[Param::VcaLevel as usize] = 192;
        bytes[Param::VcaLfoMod as usize] = SIGNED_ZERO;
        bytes[Param::Env1Sustain as usize] = u8::MAX;
        bytes[Param::Env2Sustain as usize] = u8::MAX;
        bytes[Param::Vco1Bend as usize] = 1;
        bytes[Param::Vco2Bend as usize] = 1;
        Self { bytes }
    };

    /// Reads a patch from its bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`], with the byte's place, for the first byte
    /// that is above what its control can hold.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        for ((&value, &most), place) in bytes.iter().zip(&MAXIMA).zip(0u8..) {
            if value > most {
                return Err(PatchError::Value(place));
            }
        }
        Ok(Self { bytes: *bytes })
    }

    /// The patch as its bytes.
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        self.bytes
    }

    /// The largest value a control can hold.
    #[must_use]
    pub fn max(param: Param) -> u8 {
        MAXIMA.get(usize::from(param as u8)).copied().unwrap_or(0)
    }

    /// A control's value.
    #[must_use]
    pub fn get(&self, param: Param) -> u8 {
        self.bytes.get(usize::from(param as u8)).copied().unwrap_or(0)
    }

    /// A signed control's value, -128 to 127; for any other control,
    /// its value less 128.
    #[must_use]
    pub fn signed(&self, param: Param) -> i32 {
        i32::from(self.get(param)).saturating_sub(i32::from(SIGNED_ZERO))
    }

    /// Whether a switch is on.
    #[must_use]
    pub fn on(&self, param: Param) -> bool {
        self.get(param) != 0
    }

    /// Sets a control; a value above what it can hold is taken as the
    /// most it can.
    pub fn set(&mut self, param: Param, value: u8) {
        if let Some(kept) = self.bytes.get_mut(usize::from(param as u8)) {
            *kept = value.min(Self::max(param));
        }
    }

    /// The patch with a control set: for building one in a chain.
    #[must_use]
    pub fn with(mut self, param: Param, value: u8) -> Self {
        self.set(param, value);
        self
    }

    /// The patch with a slider of the original at a position of its
    /// scale of 0 to 10, in tenths: 0 to 100. A signed control goes from
    /// none to its most, as the original's slider does; any other from 0
    /// to 255.
    #[must_use]
    pub fn at(self, param: Param, tenths: u8) -> Self {
        /// The top of the scale, in tenths.
        const TOP: u32 = 100;
        let tenths = u32::from(tenths).min(TOP);
        let (from, span) = if Self::is_signed(param) {
            (u32::from(SIGNED_ZERO), u32::from(u8::MAX - SIGNED_ZERO))
        } else {
            (0, u32::from(u8::MAX))
        };
        let value = from.saturating_add(span.saturating_mul(tenths).saturating_add(TOP / 2) / TOP);
        self.with(param, u8::try_from(value).unwrap_or(u8::MAX))
    }

    /// Whether a control is signed.
    #[must_use]
    pub const fn is_signed(param: Param) -> bool {
        matches!(
            param,
            Param::VcoLfoMod
                | Param::VcoEnvMod
                | Param::FineTune
                | Param::VcfEnvMod
                | Param::VcfLfoMod
                | Param::VcfKeyFollow
                | Param::VcaLfoMod
        )
    }

    /// Sets the mixer's two levels as the original's one knob does: 0 is
    /// VCO-1 alone, 128 both in full (VCO-1 a step short of it), 255 VCO-2
    /// alone.
    ///
    /// Source: owner's manual, printed page 18: "In it's fully
    /// counter-clockwise position only VCO-1 is heard. As the control is
    /// rotated clockwise, VCO-1 decreases and VCO-2 increases level in
    /// direct proportion to each other, through an equal mix at the 12
    /// o'clock position". Own choice: each is at its full level at the
    /// middle, not at half.
    pub fn set_source_mix(&mut self, knob: u8) {
        let twice = |distance: u8| distance.saturating_mul(2);
        self.set(Param::MixVco1, twice(u8::MAX.saturating_sub(knob)));
        self.set(Param::MixVco2, twice(knob));
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

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}
