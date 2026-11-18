//! A patch: the panel of one of the instrument's two halves as the 42
//! control-change values of its panel controls (`synth8.md`, S8-23), and
//! the laws that turn each value into a position of its control (S8-21,
//! S8-30).
//!
//! The instrument modelled has no MIDI and no documented patch format: its
//! memories go to tape as tones (owner's manual, printed page 23). Its
//! adopted interface is Roland's MIDI implementation chart for the
//! JUPITER-08, read with that product's reference manual (DEC-6457,
//! DEC-6476), which gives a controller for 40 of the 42 controls a patch
//! holds; a patch is those controllers' values in the order of Roland's
//! "JUPITER-8/SYSTEM-8 Parameter Correspondence Table" (decision A4,
//! S8-23). [`Param`] names each byte, [`Param::controller`] its controller
//! and [`Param::kind`] its law.
//!
//! Every value 0 to 127 is a position of its control, so a patch is faulty
//! only for its length or for a byte above 127 (S8-23).
//!
//! # The bytes
//!
//! | Byte | Table | Control | Controller | Kind |
//! |---|---|---|---|---|
//! | 0 | 1 | LFO RATE | 29 | Slider |
//! | 1 | 2 | LFO DELAY TIME | 27 | Slider |
//! | 2 | 3 | LFO WAVE FORM | 35 | Selector of 6 |
//! | 3 | 4 | VCO MODULATOR LFO MOD | 26 | Modulation slider |
//! | 4 | 5 | VCO MODULATOR ENV MOD | 25 | Modulation slider |
//! | 5 | 6, 7 | VCO MODULATOR switch | 60 | Selector of 3 |
//! | 6 | 8 | PULSE WIDTH MOD | 18 | Slider |
//! | 7 | 9 | PULSE WIDTH MOD source | 19 | Selector of 3 |
//! | 8 | 10 | VCO-1 CROSS MOD | 59 | Slider |
//! | 9 | 11 | VCO-1 RANGE | 20 | Selector of 6 |
//! | 10 | 12 | VCO-1 WAVEFORM | 46 | Selector of 6 |
//! | 11 | 13 | VCO-2 SYNC | 87 | Switch |
//! | 12 | 14 | VCO-2 NORMAL / LOW FREQ | 57 | Switch |
//! | 13 | 15 | VCO-2 RANGE | 62 (and 63) | Selector of 37 |
//! | 14 | 16 | VCO-2 FINE TUNE | 56 | Centred knob |
//! | 15 | 17 | VCO-2 WAVEFORM | 61 | Selector of 6 |
//! | 16 | 18 | SOURCE MIX, VCO-1 | 16 | Slider |
//! | 17 | 18 | SOURCE MIX, VCO-2 | 17 | Slider |
//! | 18 | 19 | HPF CUTOFF | 79 | Slider |
//! | 19 | 20 | VCF CUTOFF | 3 | Slider |
//! | 20 | 21 | VCF RESONANCE | 9 | Slider |
//! | 21 | 22 | VCF SLOPE | 13 | Switch (−12 dB, −24 dB) |
//! | 22 | 23 | VCF ENV MOD | 81 | Modulation slider |
//! | 23 | 24 | VCF ENV-1 / ENV-2 | 84 | Switch |
//! | 24 | 25 | VCF LFO MOD | 28 | Modulation slider |
//! | 25 | 26 | VCF KEY FOLLOW | 82 | Modulation slider |
//! | 26 | 27 | VCA LEVEL | 110 | Slider |
//! | 27 | 28 | VCA LFO MOD | 109 | Selector of 4 |
//! | 28 to 31 | 29 to 32 | ENV-1 A, D, S, R | 83, 80, 85, 86 | Sliders |
//! | 32 | 33 | ENV-1 POLARITY | 106 | Switch |
//! | 33 to 36 | 34 to 37 | ENV-2 A, D, S, R | 89, 90, 102, 103 | Sliders |
//! | 37 | 40 | ASSIGN MODE | 119 | Selector of 4 |
//! | 38 | 41 | ENV-1 KEY FOLLOW | 104 | Switch |
//! | 39 | 42 | ENV-2 KEY FOLLOW | 105 | Switch |
//! | 40 | 43 | VCO-1 BEND | 107 (NeoTeletext's own) | Switch |
//! | 41 | 44 | VCO-2 BEND | 108 (NeoTeletext's own) | Switch |
//!
//! This module holds no synthesis: it is part of the `interface` feature
//! (server.md, SRV-88), which the server builds without the instrument.

use crate::midi::DATA_MAX;

/// Bytes of a patch (S8-23).
pub const PATCH_BYTES: usize = 42;

/// The controller of VCO-2 RANGE in the JUPITER-08's LO FREQ mode, which
/// moves byte 13 by the same bands as controller 62 (S8-16; the chart's
/// "VCO-2 LO FREQ").
pub const VCO2_RANGE_LOW_FREQUENCY: u8 = 63;

/// The controls of a patch, each named by its byte (S8-16, S8-23).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Param {
    /// LFO RATE.
    LfoRate = 0,
    /// LFO DELAY TIME.
    LfoDelay = 1,
    /// LFO WAVE FORM ([`LfoWave`]).
    LfoWave = 2,
    /// VCO MODULATOR, LFO MOD: how far the low-frequency oscillator moves
    /// the pitch.
    VcoLfoMod = 3,
    /// VCO MODULATOR, ENV MOD: how far envelope 1 moves the pitch.
    VcoEnvMod = 4,
    /// The VCO MODULATOR switch: which oscillators the two reach
    /// ([`VcoModulator`]).
    VcoModulator = 5,
    /// PULSE WIDTH MOD: the pulse's width, or how far its source moves it.
    PwmDepth = 6,
    /// What moves the pulse's width ([`PwmSource`]).
    PwmSource = 7,
    /// VCO-1 CROSS MOD: how far VCO-2 moves VCO-1's pitch.
    CrossMod = 8,
    /// VCO-1 RANGE ([`Vco1Range`]).
    Vco1Range = 9,
    /// VCO-1 WAVEFORM ([`Vco1Wave`]).
    Vco1Wave = 10,
    /// VCO-2 SYNC: whether VCO-1 starts VCO-2's cycle.
    Sync = 11,
    /// The VCO-2 NORMAL / LOW FREQ switch.
    LowFrequency = 12,
    /// VCO-2 RANGE, in semitones above 16′.
    Vco2Range = 13,
    /// VCO-2 FINE TUNE.
    FineTune = 14,
    /// VCO-2 WAVEFORM ([`Vco2Wave`]).
    Vco2Wave = 15,
    /// The source mixer's VCO-1 level.
    MixVco1 = 16,
    /// The source mixer's VCO-2 level.
    MixVco2 = 17,
    /// HPF CUTOFF FREQ.
    HpfCutoff = 18,
    /// VCF CUTOFF FREQ.
    VcfCutoff = 19,
    /// VCF RES.
    VcfResonance = 20,
    /// VCF SLOPE: −12 dB an octave below 64, −24 dB from it.
    VcfSlope = 21,
    /// VCF ENV MOD.
    VcfEnvMod = 22,
    /// The VCF ENV-1 / ENV-2 switch.
    VcfEnvSource = 23,
    /// VCF LFO MOD.
    VcfLfoMod = 24,
    /// VCF KEY FOLLOW.
    VcfKeyFollow = 25,
    /// VCA LEVEL.
    VcaLevel = 26,
    /// VCA LFO MOD, a switch of four positions.
    VcaLfoMod = 27,
    /// ENV-1 ATTACK.
    Env1Attack = 28,
    /// ENV-1 DECAY.
    Env1Decay = 29,
    /// ENV-1 SUSTAIN.
    Env1Sustain = 30,
    /// ENV-1 RELEASE.
    Env1Release = 31,
    /// ENV-1 POLARITY: normal below 64, inverted from it.
    Env1Polarity = 32,
    /// ENV-2 ATTACK.
    Env2Attack = 33,
    /// ENV-2 DECAY.
    Env2Decay = 34,
    /// ENV-2 SUSTAIN.
    Env2Sustain = 35,
    /// ENV-2 RELEASE.
    Env2Release = 36,
    /// ASSIGN MODE ([`AssignMode`]).
    AssignMode = 37,
    /// ENV-1 KEY FOLLOW.
    Env1KeyFollow = 38,
    /// ENV-2 KEY FOLLOW.
    Env2KeyFollow = 39,
    /// The bender panel's VCO-1 BEND switch.
    Vco1Bend = 40,
    /// The bender panel's VCO-2 BEND switch.
    Vco2Bend = 41,
}

/// The law a control's value is read by (S8-21).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `v ÷ 127` of the slider's travel (S8-21 item 1).
    Slider,
    /// `v ÷ 127` of the travel from no modulation to the most (S8-21
    /// item 2).
    Modulation,
    /// Centred at 64 (S8-21 item 3): VCO-2 FINE TUNE.
    Centred,
    /// A selector of so many positions, in even bands (S8-21 item 4).
    Selector(u8),
    /// Off at 0 to 63, on at 64 to 127 (S8-21 item 5).
    Switch,
}

/// The positions of the reference manual's lists for the six-way
/// selectors (S8-21 item 4: LFO Waveform, VCO-1 Range, VCO-1 and VCO-2
/// Waveform).
const SIX: u8 = 6;
/// The positions of VCO MODULATOR's and PULSE WIDTH MOD's selectors.
const THREE: u8 = 3;
/// The positions of VCA LFO MOD ("Amp Mod-Step, 0–3") and of ASSIGN MODE.
const FOUR: u8 = 4;
/// VCO-2 RANGE's positions: 16′ and the 36 semitones above it to 2′
/// (owner's manual, printed page 16; S8-30).
pub const VCO2_RANGE_POSITIONS: u8 = 37;
/// The values of a data byte, the denominator of every band (S8-21
/// item 4).
const VALUES: u16 = 128;
/// A switch is on from this value (S8-21 item 5; MIDI 1.0 4.2, printed
/// page 12, PDF 44).
pub const SWITCH_ON: u8 = 64;
/// The canonical value of a switch that is on (S8-21 item 5).
pub const SWITCH_ON_VALUE: u8 = DATA_MAX;
/// The centre of VCO-2 FINE TUNE (S8-21 item 3).
pub const CENTRE: u8 = 64;

impl Param {
    /// Every control, in byte order.
    pub const ALL: [Self; PATCH_BYTES] = [
        Self::LfoRate,
        Self::LfoDelay,
        Self::LfoWave,
        Self::VcoLfoMod,
        Self::VcoEnvMod,
        Self::VcoModulator,
        Self::PwmDepth,
        Self::PwmSource,
        Self::CrossMod,
        Self::Vco1Range,
        Self::Vco1Wave,
        Self::Sync,
        Self::LowFrequency,
        Self::Vco2Range,
        Self::FineTune,
        Self::Vco2Wave,
        Self::MixVco1,
        Self::MixVco2,
        Self::HpfCutoff,
        Self::VcfCutoff,
        Self::VcfResonance,
        Self::VcfSlope,
        Self::VcfEnvMod,
        Self::VcfEnvSource,
        Self::VcfLfoMod,
        Self::VcfKeyFollow,
        Self::VcaLevel,
        Self::VcaLfoMod,
        Self::Env1Attack,
        Self::Env1Decay,
        Self::Env1Sustain,
        Self::Env1Release,
        Self::Env1Polarity,
        Self::Env2Attack,
        Self::Env2Decay,
        Self::Env2Sustain,
        Self::Env2Release,
        Self::AssignMode,
        Self::Env1KeyFollow,
        Self::Env2KeyFollow,
        Self::Vco1Bend,
        Self::Vco2Bend,
    ];

    /// The byte of the patch the control is.
    #[must_use]
    pub const fn byte(self) -> usize {
        self as usize
    }

    /// The controller that moves the control (S8-16): the chart's, but
    /// for the two bend switches, NeoTeletext's own 107 and 108.
    #[must_use]
    pub const fn controller(self) -> u8 {
        match self {
            Self::LfoRate => 29,
            Self::LfoDelay => 27,
            Self::LfoWave => 35,
            Self::VcoLfoMod => 26,
            Self::VcoEnvMod => 25,
            Self::VcoModulator => 60,
            Self::PwmDepth => 18,
            Self::PwmSource => 19,
            Self::CrossMod => 59,
            Self::Vco1Range => 20,
            Self::Vco1Wave => 46,
            Self::Sync => 87,
            Self::LowFrequency => 57,
            Self::Vco2Range => 62,
            Self::FineTune => 56,
            Self::Vco2Wave => 61,
            Self::MixVco1 => 16,
            Self::MixVco2 => 17,
            Self::HpfCutoff => 79,
            Self::VcfCutoff => 3,
            Self::VcfResonance => 9,
            Self::VcfSlope => 13,
            Self::VcfEnvMod => 81,
            Self::VcfEnvSource => 84,
            Self::VcfLfoMod => 28,
            Self::VcfKeyFollow => 82,
            Self::VcaLevel => 110,
            Self::VcaLfoMod => 109,
            Self::Env1Attack => 83,
            Self::Env1Decay => 80,
            Self::Env1Sustain => 85,
            Self::Env1Release => 86,
            Self::Env1Polarity => 106,
            Self::Env2Attack => 89,
            Self::Env2Decay => 90,
            Self::Env2Sustain => 102,
            Self::Env2Release => 103,
            Self::AssignMode => 119,
            Self::Env1KeyFollow => 104,
            Self::Env2KeyFollow => 105,
            Self::Vco1Bend => 107,
            Self::Vco2Bend => 108,
        }
    }

    /// The control a controller moves, if it moves one: the 42 of
    /// [`Param::controller`], and [`VCO2_RANGE_LOW_FREQUENCY`] for VCO-2
    /// RANGE (S8-16).
    #[must_use]
    pub fn of_controller(controller: u8) -> Option<Self> {
        if controller == VCO2_RANGE_LOW_FREQUENCY {
            return Some(Self::Vco2Range);
        }
        Self::ALL.into_iter().find(|param| param.controller() == controller)
    }

    /// The law of the control's value (S8-16's "Kind" column).
    #[must_use]
    pub const fn kind(self) -> Kind {
        match self {
            Self::LfoRate
            | Self::LfoDelay
            | Self::PwmDepth
            | Self::CrossMod
            | Self::MixVco1
            | Self::MixVco2
            | Self::HpfCutoff
            | Self::VcfCutoff
            | Self::VcfResonance
            | Self::VcaLevel
            | Self::Env1Attack
            | Self::Env1Decay
            | Self::Env1Sustain
            | Self::Env1Release
            | Self::Env2Attack
            | Self::Env2Decay
            | Self::Env2Sustain
            | Self::Env2Release => Kind::Slider,
            Self::VcoLfoMod | Self::VcoEnvMod | Self::VcfEnvMod | Self::VcfLfoMod | Self::VcfKeyFollow => {
                Kind::Modulation
            }
            Self::FineTune => Kind::Centred,
            Self::LfoWave | Self::Vco1Range | Self::Vco1Wave | Self::Vco2Wave => Kind::Selector(SIX),
            Self::VcoModulator | Self::PwmSource => Kind::Selector(THREE),
            Self::VcaLfoMod | Self::AssignMode => Kind::Selector(FOUR),
            Self::Vco2Range => Kind::Selector(VCO2_RANGE_POSITIONS),
            Self::Sync
            | Self::LowFrequency
            | Self::VcfSlope
            | Self::VcfEnvSource
            | Self::Env1Polarity
            | Self::Env1KeyFollow
            | Self::Env2KeyFollow
            | Self::Vco1Bend
            | Self::Vco2Bend => Kind::Switch,
        }
    }

    /// The control's name as the server's typed patch names its field
    /// (server.md, SRV-40).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::LfoRate => "lfo_rate",
            Self::LfoDelay => "lfo_delay_time",
            Self::LfoWave => "lfo_waveform",
            Self::VcoLfoMod => "vco_lfo_mod",
            Self::VcoEnvMod => "vco_env_mod",
            Self::VcoModulator => "vco_modulator",
            Self::PwmDepth => "pulse_width_mod",
            Self::PwmSource => "pulse_width_mod_source",
            Self::CrossMod => "cross_mod",
            Self::Vco1Range => "vco1_range",
            Self::Vco1Wave => "vco1_waveform",
            Self::Sync => "vco2_sync",
            Self::LowFrequency => "vco2_low_freq",
            Self::Vco2Range => "vco2_range",
            Self::FineTune => "vco2_fine_tune",
            Self::Vco2Wave => "vco2_waveform",
            Self::MixVco1 => "vco1_level",
            Self::MixVco2 => "vco2_level",
            Self::HpfCutoff => "hpf_cutoff",
            Self::VcfCutoff => "vcf_cutoff",
            Self::VcfResonance => "vcf_resonance",
            Self::VcfSlope => "vcf_slope",
            Self::VcfEnvMod => "vcf_env_mod",
            Self::VcfEnvSource => "vcf_envelope_select",
            Self::VcfLfoMod => "vcf_lfo_mod",
            Self::VcfKeyFollow => "vcf_key_follow",
            Self::VcaLevel => "vca_level",
            Self::VcaLfoMod => "vca_lfo_mod",
            Self::Env1Attack => "env1_attack",
            Self::Env1Decay => "env1_decay",
            Self::Env1Sustain => "env1_sustain",
            Self::Env1Release => "env1_release",
            Self::Env1Polarity => "env1_polarity",
            Self::Env2Attack => "env2_attack",
            Self::Env2Decay => "env2_decay",
            Self::Env2Sustain => "env2_sustain",
            Self::Env2Release => "env2_release",
            Self::AssignMode => "assign_mode",
            Self::Env1KeyFollow => "env1_key_follow",
            Self::Env2KeyFollow => "env2_key_follow",
            Self::Vco1Bend => "vco1_bend",
            Self::Vco2Bend => "vco2_bend",
        }
    }
}

// No controller is two controls' (S8-16's assertion): every pair of the
// 42 differ, and none is 63 or a performance controller.
const _: () = {
    let mut first = 0;
    while first < PATCH_BYTES {
        #[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build")]
        let controller = Param::ALL[first].controller();
        assert!(controller != VCO2_RANGE_LOW_FREQUENCY);
        assert!(!crate::synth8::interface::is_performance_controller(controller));
        let mut second = first.wrapping_add(1);
        while second < PATCH_BYTES {
            #[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build")]
            let other = Param::ALL[second].controller();
            assert!(controller != other);
            second = second.wrapping_add(1);
        }
        #[allow(clippy::indexing_slicing, reason = "worked out by the compiler: a fault here stops the build")]
        let byte = Param::ALL[first].byte();
        assert!(byte == first);
        first = first.wrapping_add(1);
    }
};

/// The position, counted from 0, of value `value` of a selector of
/// `positions`: `floor(v × n ÷ 128)` (S8-21 item 4).
#[must_use]
pub const fn position(value: u8, positions: u8) -> u8 {
    let value = if value > DATA_MAX { DATA_MAX } else { value };
    let position = (value as u16).saturating_mul(positions as u16) / VALUES;
    // At most n − 1, which is below 128.
    position as u8
}

/// The canonical value of position `position` of a selector of
/// `positions`, the lowest value of its band: `ceil(p × 128 ÷ n)` (S8-21
/// item 4). A position past the last is taken as the last.
#[must_use]
pub const fn canonical(position: u8, positions: u8) -> u8 {
    if positions == 0 {
        return 0;
    }
    let last = positions.saturating_sub(1);
    let position = if position > last { last } else { position };
    let product = (position as u16).saturating_mul(VALUES);
    let value = product.div_ceil(positions as u16);
    // Below 128, since position < positions.
    value as u8
}

/// The byte of 0 to 255 a slider's value stands for in the instrument's
/// units: `floor((v × 255 + 63) ÷ 127)` (S8-21 item 1).
#[must_use]
pub const fn slider_byte(value: u8) -> u8 {
    let value = if value > DATA_MAX { DATA_MAX } else { value };
    let byte =
        match (value as u16).saturating_mul(u8::MAX as u16).saturating_add(HALF_STEP).checked_div(DATA_MAX as u16) {
            Some(byte) => byte,
            None => 0,
        };
    // At most 255 at value 127.
    byte as u8
}

/// The half of a data step, which [`slider_byte`] rounds by.
const HALF_STEP: u16 = 63;
/// Fine tune's right half rounds by this, half its 63 steps (S8-21
/// item 3).
const FINE_HALF_STEP: i32 = 31;
/// The signed byte's largest magnitude (the table's −128 to +127).
const SIGNED_TOP: i32 = 127;

/// VCO-2 FINE TUNE's signed byte, −128 to +127, for its value: `−2 × (64
/// − v)` below the centre and `floor(((v − 64) × 127 + 31) ÷ 63)` from it
/// (S8-21 item 3, the signed byte less 128).
#[must_use]
pub const fn fine_tune_signed(value: u8) -> i32 {
    let value = if value > DATA_MAX { DATA_MAX } else { value };
    if value < CENTRE {
        // Two signed steps a value: −128 at value 0.
        (CENTRE as i32).saturating_sub(value as i32).saturating_mul(-2)
    } else {
        let above = (value as i32).saturating_sub(CENTRE as i32);
        let steps = (DATA_MAX as i32).saturating_sub(CENTRE as i32);
        match above.saturating_mul(SIGNED_TOP).saturating_add(FINE_HALF_STEP).checked_div(steps) {
            Some(signed) => signed,
            None => 0,
        }
    }
}

/// The low-frequency oscillator's waveforms (owner's manual, printed page
/// 17: "a Sine Wave, inverted Sawtooth wave, Square wave and a Random
/// output from the Sample & Hold circuit").
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

/// The reference manual's LFO Waveform list, SINE, TRI, SAW-DW, SQR,
/// RANDOM, CHAOS, as the instrument's: TRI folded to sine, CHAOS to random
/// (S8-21 item 4).
const LFO_WAVES: [LfoWave; SIX as usize] =
    [LfoWave::Sine, LfoWave::Sine, LfoWave::Sawtooth, LfoWave::Square, LfoWave::Random, LfoWave::Random];

/// VCO-1's ranges (owner's manual, printed page 15).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vco1Range {
    /// 16′.
    Sixteen,
    /// 8′.
    Eight,
    /// 4′.
    Four,
    /// 2′.
    Two,
}

/// The reference manual's VCO-1 Range list, 32′, 16′, 8′, 4′, 2′, 1′, as
/// the instrument's: 32′ folded to 16′, 1′ to 2′ (S8-21 item 4).
const VCO1_RANGES: [Vco1Range; SIX as usize] =
    [Vco1Range::Sixteen, Vco1Range::Sixteen, Vco1Range::Eight, Vco1Range::Four, Vco1Range::Two, Vco1Range::Two];

/// VCO-1's waveforms (owner's manual, printed pages 15 and 16).
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

/// The reference manual's VCO-1 Waveform list, SINE, TRI, SAW, PW, SQR,
/// NOISE, as the instrument's: SINE folded to triangle, NOISE to square
/// (S8-21 item 4).
const VCO1_WAVES: [Vco1Wave; SIX as usize] =
    [Vco1Wave::Triangle, Vco1Wave::Triangle, Vco1Wave::Sawtooth, Vco1Wave::Pulse, Vco1Wave::Square, Vco1Wave::Square];

/// VCO-2's waveforms (owner's manual, printed page 16).
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

/// The reference manual's VCO-2 Waveform list, SINE, TRI, SAW, PW, SQR,
/// NOISE, as the instrument's: TRI folded to sine, SQR to pulse (S8-21
/// item 4).
const VCO2_WAVES: [Vco2Wave; SIX as usize] =
    [Vco2Wave::Sine, Vco2Wave::Sine, Vco2Wave::Sawtooth, Vco2Wave::Pulse, Vco2Wave::Pulse, Vco2Wave::Noise];

/// Which oscillators the VCO modulator reaches: the reference manual's
/// "Dest Select: VCO-1, BOTH, VCO-2", the instrument's one switch of
/// three positions (owner's manual, printed page 17; S8-16 byte 5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VcoModulator {
    /// VCO-1 alone.
    Vco1,
    /// Both.
    Both,
    /// VCO-2 alone.
    Vco2,
}

/// What moves the pulse's width: the reference manual's "PW Mode: LFO, M
/// (MANUAL), ENV-1" (owner's manual, printed page 17).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwmSource {
    /// The low-frequency oscillator, as far as the slider says.
    Lfo,
    /// Nothing: the slider is the width.
    Manual,
    /// Envelope 1, as far as the slider says.
    Envelope,
}

/// How keys are given voices (owner's manual, printed page 12, "Assign
/// Modes"), in the order of the correspondence table's parameter 40,
/// "POLY1, MONO, UNISON, POLY2" (S8-21 item 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssignMode {
    /// A voice for each key; a released voice rings on.
    Poly1,
    /// One voice, the lowest key held.
    Solo,
    /// Every voice, shared among the keys held.
    Unison,
    /// A voice for each key; a new key silences what was released. Its
    /// band is the JUPITER-08's SOLO UNISON's.
    Poly2,
}

/// The place in the reference manual's list of each of the instrument's
/// positions, in the order of its enumeration: the band of the position
/// the manual names as the instrument's own, never a fold's, whose
/// lowest value is the canonical one (S8-21 item 4).
const LFO_WAVE_PLACES: [u8; 4] = [0, 2, 3, 4];
/// See [`LFO_WAVE_PLACES`]: 16′, 8′, 4′, 2′ among 32′ to 1′.
const VCO1_RANGE_PLACES: [u8; 4] = [1, 2, 3, 4];
/// See [`LFO_WAVE_PLACES`]: TRI, SAW, PW, SQR among SINE to NOISE.
const VCO1_WAVE_PLACES: [u8; 4] = [1, 2, 3, 4];
/// See [`LFO_WAVE_PLACES`]: SINE, SAW, PW, NOISE among SINE to NOISE.
const VCO2_WAVE_PLACES: [u8; 4] = [0, 2, 3, 5];

/// The canonical value of the instrument's position `index` whose place
/// in a six-way list `places` gives.
const fn six_way(places: [u8; 4], index: u8) -> u8 {
    let place = match index {
        0 => places[0],
        1 => places[1],
        2 => places[2],
        _ => places[3],
    };
    canonical(place, SIX)
}

impl LfoWave {
    /// The canonical value of the waveform (S8-21 item 4).
    #[must_use]
    pub const fn value(self) -> u8 {
        six_way(LFO_WAVE_PLACES, self as u8)
    }
}

impl Vco1Range {
    /// The canonical value of the range.
    #[must_use]
    pub const fn value(self) -> u8 {
        six_way(VCO1_RANGE_PLACES, self as u8)
    }

    /// The range's place counted from 16′: 0 to 3.
    #[must_use]
    pub const fn index(self) -> u8 {
        self as u8
    }
}

impl Vco1Wave {
    /// The canonical value of the waveform.
    #[must_use]
    pub const fn value(self) -> u8 {
        six_way(VCO1_WAVE_PLACES, self as u8)
    }
}

impl Vco2Wave {
    /// The canonical value of the waveform.
    #[must_use]
    pub const fn value(self) -> u8 {
        six_way(VCO2_WAVE_PLACES, self as u8)
    }
}

impl VcoModulator {
    /// The canonical value of the switch's position.
    #[must_use]
    pub const fn value(self) -> u8 {
        canonical(self as u8, THREE)
    }
}

impl PwmSource {
    /// The canonical value of the source.
    #[must_use]
    pub const fn value(self) -> u8 {
        canonical(self as u8, THREE)
    }
}

impl AssignMode {
    /// The canonical value of the mode.
    #[must_use]
    pub const fn value(self) -> u8 {
        canonical(self as u8, FOUR)
    }
}

/// The canonical value of VCO-2 RANGE at `semitones` above 16′, 0 to 36
/// (S8-30); a larger number is taken as 36.
#[must_use]
pub const fn vco2_range_value(semitones: u8) -> u8 {
    canonical(semitones, VCO2_RANGE_POSITIONS)
}

/// The canonical value of VCA LFO MOD at position `position`, 0 to 3.
#[must_use]
pub const fn vca_lfo_mod_value(position: u8) -> u8 {
    canonical(position, FOUR)
}

/// The canonical value of a switch (S8-21 item 5).
#[must_use]
pub const fn switch_value(on: bool) -> u8 {
    if on { SWITCH_ON_VALUE } else { 0 }
}

/// Why bytes are no patch (S8-23).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PatchError {
    /// Not [`PATCH_BYTES`] bytes: the length found.
    Length(usize),
    /// The byte at this place is above 127.
    Value(u8),
}

/// One patch: the 42 values of S8-23. Every value it can hold is a patch
/// the instrument can play.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Patch {
    values: [u8; PATCH_BYTES],
}

impl Patch {
    /// The default patch (S8-22): VCO-1's sawtooth alone at 8′ through an
    /// open filter, the amplifier's envelope at full sustain, no
    /// modulation; S8-23's last column.
    ///
    /// Source: owner's manual, printed page 8, figure 1, "basic tuning
    /// patch", as nearly as its drawing can be read; own choice where it
    /// cannot (S8-22).
    pub const DEFAULT: Self = Self {
        values: [
            0, 0, 0, 0, 0, 43, 0, 43, 0, 43, 43, 0, 0, 42, 64, 43, 127, 0, 0, 127, 0, 127, 0, 0, 0, 0, 96, 0, 0, 0,
            127, 0, 0, 0, 0, 127, 0, 0, 0, 0, 127, 127,
        ],
    };

    /// Reads a patch from its bytes.
    ///
    /// # Errors
    ///
    /// [`PatchError::Value`], with the byte's place, for the first byte
    /// above 127.
    pub fn from_bytes(bytes: &[u8; PATCH_BYTES]) -> Result<Self, PatchError> {
        for (&value, place) in bytes.iter().zip(0u8..) {
            if value > DATA_MAX {
                return Err(PatchError::Value(place));
            }
        }
        Ok(Self { values: *bytes })
    }

    /// Reads a patch from bytes of any length.
    ///
    /// # Errors
    ///
    /// [`PatchError::Length`] for bytes that are not [`PATCH_BYTES`] long,
    /// else as [`Patch::from_bytes`].
    pub fn from_slice(bytes: &[u8]) -> Result<Self, PatchError> {
        let bytes: &[u8; PATCH_BYTES] = bytes.try_into().map_err(|_| PatchError::Length(bytes.len()))?;
        Self::from_bytes(bytes)
    }

    /// The patch as its bytes.
    #[must_use]
    pub const fn to_bytes(&self) -> [u8; PATCH_BYTES] {
        self.values
    }

    /// A control's value, 0 to 127.
    #[must_use]
    pub fn get(&self, param: Param) -> u8 {
        self.values.get(param.byte()).copied().unwrap_or(0)
    }

    /// Sets a control; a value above 127 is taken as 127.
    pub fn set(&mut self, param: Param, value: u8) {
        if let Some(kept) = self.values.get_mut(param.byte()) {
            *kept = value.min(DATA_MAX);
        }
    }

    /// The patch with a control set: for building one in a chain.
    #[must_use]
    pub fn with(mut self, param: Param, value: u8) -> Self {
        self.set(param, value);
        self
    }

    /// The patch with a control at a position of the original's scale of
    /// 0 to 10, in tenths, 0 to 100: the value `round(127 × tenths ÷
    /// 100)`, a larger number taken as 100.
    #[must_use]
    pub fn at(self, param: Param, tenths: u8) -> Self {
        /// The top of the scale, in tenths.
        const TOP: u16 = 100;
        let tenths = u16::from(tenths).min(TOP);
        let value = (tenths.saturating_mul(u16::from(DATA_MAX)).saturating_add(TOP / 2)) / TOP;
        self.with(param, u8::try_from(value).unwrap_or(DATA_MAX))
    }

    /// Whether a switch is on: its value is 64 or more (S8-21 item 5).
    #[must_use]
    pub fn on(&self, param: Param) -> bool {
        self.get(param) >= SWITCH_ON
    }

    /// A selector's position in the reference manual's list, counted from
    /// 0; for any other control, the position of a selector of two.
    #[must_use]
    pub fn position(&self, param: Param) -> u8 {
        let positions = match param.kind() {
            Kind::Selector(positions) => positions,
            Kind::Slider | Kind::Modulation | Kind::Centred | Kind::Switch => 2,
        };
        position(self.get(param), positions)
    }

    /// Sets the mixer's two levels as the original's one knob does, the
    /// knob 0 to 127: 0 is VCO-1 alone, 64 both in full (VCO-1 a step
    /// short of it), 127 VCO-2 alone.
    ///
    /// Source: owner's manual, printed page 18: "In it's fully
    /// counter-clockwise position only VCO-1 is heard. As the control is
    /// rotated clockwise, VCO-1 decreases and VCO-2 increases level in
    /// direct proportion to each other, through an equal mix at the 12
    /// o'clock position". Own choice: each is at its full level at the
    /// middle, not at half. On the wire the two levels are controllers 16
    /// and 17 (`synth8.md`, section 15 item 6).
    pub fn set_source_mix(&mut self, knob: u8) {
        let twice = |distance: u8| distance.saturating_mul(2);
        let knob = knob.min(DATA_MAX);
        self.set(Param::MixVco1, twice(DATA_MAX.saturating_sub(knob)));
        self.set(Param::MixVco2, twice(knob));
    }

    /// The low-frequency oscillator's waveform.
    #[must_use]
    pub fn lfo_wave(&self) -> LfoWave {
        LFO_WAVES.get(usize::from(self.position(Param::LfoWave))).copied().unwrap_or(LfoWave::Sine)
    }

    /// VCO-1's range.
    #[must_use]
    pub fn vco1_range(&self) -> Vco1Range {
        VCO1_RANGES.get(usize::from(self.position(Param::Vco1Range))).copied().unwrap_or(Vco1Range::Eight)
    }

    /// VCO-1's waveform.
    #[must_use]
    pub fn vco1_wave(&self) -> Vco1Wave {
        VCO1_WAVES.get(usize::from(self.position(Param::Vco1Wave))).copied().unwrap_or(Vco1Wave::Sawtooth)
    }

    /// VCO-2's waveform.
    #[must_use]
    pub fn vco2_wave(&self) -> Vco2Wave {
        VCO2_WAVES.get(usize::from(self.position(Param::Vco2Wave))).copied().unwrap_or(Vco2Wave::Sawtooth)
    }

    /// Which oscillators the VCO modulator reaches.
    #[must_use]
    pub fn vco_modulator(&self) -> VcoModulator {
        match self.position(Param::VcoModulator) {
            0 => VcoModulator::Vco1,
            1 => VcoModulator::Both,
            _ => VcoModulator::Vco2,
        }
    }

    /// What moves the pulse's width.
    #[must_use]
    pub fn pwm_source(&self) -> PwmSource {
        match self.position(Param::PwmSource) {
            0 => PwmSource::Lfo,
            1 => PwmSource::Manual,
            _ => PwmSource::Envelope,
        }
    }

    /// VCO-2's range in semitones above 16′, 0 to 36 (S8-30).
    #[must_use]
    pub fn vco2_range(&self) -> u8 {
        self.position(Param::Vco2Range)
    }

    /// VCA LFO MOD's position, 0 to 3.
    #[must_use]
    pub fn vca_lfo_mod(&self) -> u8 {
        self.position(Param::VcaLfoMod)
    }

    /// How keys are given voices.
    #[must_use]
    pub fn assign_mode(&self) -> AssignMode {
        match self.position(Param::AssignMode) {
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
