//! Patches: the instrument's voices, checked, and a bank of 32 of them
//! (`neoteletext-common/docs/audio/synth7.md`, cited here as "S7-n").
//!
//! A voice arrives in one of two forms: the 155 voice parameters 0 to 154
//! of a single-voice dump or of parameter changes (S7-28, S7-38;
//! [`Patch::from_parameters`]), or the 128-byte packed form in which a
//! 32-voice dump carries it (S7-46; [`Patch::from_packed`]). Each is read
//! and every field checked against its range (S7-29), so that nothing
//! after it can meet a value outside the instrument's own; a [`Patch`]
//! cannot be made any other way than through that check.
//!
//! Source: Yamaha DX7 owner's manual, "MIDI data format" (printed pages 30
//! to 32), for the messages and each parameter's range; the TX7 manual,
//! "MIDI DATA FORMAT", printed pages 38 and 39 (section 4-1, the voice
//! parameters 0 to 155 and their notes), 41 (section 4-5, the single-voice
//! dump's data in parameter order) and 44 (section 4-8, the packed layout,
//! every byte and bit field), which the DX7 II manual's printed page Add-11
//! draws the same.

use super::sysex::{DumpError, bank_voice, check_bank_dump};

/// Operators in a voice.
///
/// Source: Yamaha DX7 owner's manual, MIDI data format (printed page 31):
/// voice parameters for OP6 to OP1.
pub const OPERATORS: usize = 6;

/// The algorithms of the instrument.
///
/// Source: Yamaha DX7 owner's manual, MIDI data format (printed page 31):
/// "ALGORITHM SELECT 0 ~ 31".
pub const ALGORITHM_COUNT: usize = 32;

/// The largest feedback setting of a patch.
///
/// Source: Yamaha DX7 owner's manual, MIDI data format (printed page 31):
/// "FEED BACK 0 ~ 7".
pub const FEEDBACK_MAX: u8 = 7;

/// Bytes of one voice in the packed form.
///
/// Source: Yamaha DX7 owner's manual, printed page 30: bulk data of 32
/// voices is 4,096 bytes.
pub const PACKED_VOICE_BYTES: usize = 128;

/// Voices in a bank.
///
/// Source: Yamaha DX7 owner's manual, printed page 30: "f=9; 32 voices".
pub const BANK_VOICES: usize = 32;

/// Bytes of one operator in the packed form.
///
/// Source: TX7 manual, printed page 44, section 4-8: operator 6 is bytes 0
/// to 16, operators 5 to 1 the same at 17 to 101 (S7-46).
const PACKED_OPERATOR_BYTES: usize = 17;

/// Characters in a voice's name.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "VOICE NAME 1" to
/// "VOICE NAME 10".
pub const NAME_BYTES: usize = 10;

/// The largest value of the parameters the instrument shows as 0 to 99.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 99".
pub const MAX_99: u8 = 99;
/// The largest keyboard level scaling curve.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 3".
pub const MAX_CURVE: u8 = 3;
/// The largest keyboard rate scaling.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 7".
pub const MAX_RATE_SCALING: u8 = 7;
/// The largest amplitude modulation sensitivity.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 3".
pub const MAX_AMP_MOD_SENSITIVITY: u8 = 3;
/// The largest key velocity sensitivity.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 7".
pub const MAX_KEY_VELOCITY_SENSITIVITY: u8 = 7;
/// The largest oscillator mode: 0 is a ratio of the key's frequency, 1 a
/// fixed frequency.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 1".
pub const MAX_OSCILLATOR_MODE: u8 = 1;
/// The largest coarse frequency.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 31".
pub const MAX_FREQUENCY_COARSE: u8 = 31;
/// The largest detune; 7 is no detune.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 14".
pub const MAX_DETUNE: u8 = 14;
/// The largest algorithm number, which the instrument shows as 32.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 31".
pub const MAX_ALGORITHM: u8 = (ALGORITHM_COUNT as u8) - 1;
/// The largest low frequency oscillator waveform: sample and hold. The
/// DX7 manual's table says 4, a misprint; the instrument has six
/// waveforms, 0 to 5 (S7-31).
///
/// Source: TX7 manual, printed page 38, "LFO WAVE 0 ~ 5", and page 39,
/// note *6, 0 triangle to 5 sample and hold; DX7 II manual, printed page
/// Add-8, "LFW 0 - 5"; DX7 manual, pages 13 and 27, six waveforms.
pub const MAX_LFO_WAVE: u8 = 5;
/// The largest pitch modulation sensitivity.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 7".
pub const MAX_PITCH_MOD_SENSITIVITY: u8 = 7;
/// The largest transpose; 24 is no transpose, and each step is a semitone.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: "0 ~ 48".
pub const MAX_TRANSPOSE: u8 = 48;
/// The transpose that changes nothing.
///
/// Source: TX7 manual, printed page 38, "TRANSPOSE 0 ~ 48 … Concert pitch
/// at 24" (S7-8).
pub const TRANSPOSE_CENTRE: u8 = 24;
/// Every operator switched on.
///
/// Source: Yamaha DX7 owner's manual, printed page 31: parameter 155,
/// "OPERATOR ON/OFF", one bit for each operator.
pub const ALL_OPERATORS_ON: u8 = (1 << OPERATORS) - 1;

/// The seven bits of a MIDI data byte.
const DATA_MASK: u8 = 0x7f;

/// The parameters of one operator of a voice, as plain numbers. Nothing
/// here is checked: [`Patch::new`] does that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperatorParams {
    /// Envelope rates 1 to 4, each 0 to 99.
    pub eg_rates: [u8; 4],
    /// Envelope levels 1 to 4, each 0 to 99.
    pub eg_levels: [u8; 4],
    /// Keyboard level scaling break point, 0 to 99; 39 is C3.
    pub break_point: u8,
    /// Keyboard level scaling depth below the break point, 0 to 99.
    pub left_depth: u8,
    /// Keyboard level scaling depth above the break point, 0 to 99.
    pub right_depth: u8,
    /// Curve below the break point: 0 linear fall, 1 exponential fall, 2
    /// exponential rise, 3 linear rise.
    pub left_curve: u8,
    /// Curve above the break point, as [`OperatorParams::left_curve`].
    pub right_curve: u8,
    /// Keyboard rate scaling, 0 to 7.
    pub rate_scaling: u8,
    /// Amplitude modulation sensitivity, 0 to 3.
    pub amp_mod_sensitivity: u8,
    /// Key velocity sensitivity, 0 to 7.
    pub key_velocity_sensitivity: u8,
    /// Output level, 0 to 99.
    pub output_level: u8,
    /// Oscillator mode: 0 a ratio of the key's frequency, 1 a fixed
    /// frequency.
    pub oscillator_mode: u8,
    /// Coarse frequency, 0 to 31: in ratio mode 0 is a half and `n` is `n`.
    pub frequency_coarse: u8,
    /// Fine frequency, 0 to 99: hundredths added to the ratio.
    pub frequency_fine: u8,
    /// Detune, 0 to 14; 7 is none.
    pub detune: u8,
}

/// The parameters of a voice, as plain numbers. Nothing here is checked:
/// [`Patch::new`] does that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceParams {
    /// The six operators; index 0 is operator 1.
    pub operators: [OperatorParams; OPERATORS],
    /// Pitch envelope rates 1 to 4, each 0 to 99.
    pub pitch_eg_rates: [u8; 4],
    /// Pitch envelope levels 1 to 4, each 0 to 99; 50 is no change.
    pub pitch_eg_levels: [u8; 4],
    /// Algorithm, 0 to 31, which the instrument shows as 1 to 32.
    pub algorithm: u8,
    /// Feedback, 0 to 7.
    pub feedback: u8,
    /// Oscillator key sync, 0 or 1: at 1 every operator's phase starts
    /// again when a key goes down.
    pub oscillator_key_sync: u8,
    /// Low frequency oscillator speed, 0 to 99.
    pub lfo_speed: u8,
    /// Low frequency oscillator delay, 0 to 99.
    pub lfo_delay: u8,
    /// Low frequency oscillator pitch modulation depth, 0 to 99.
    pub lfo_pitch_mod_depth: u8,
    /// Low frequency oscillator amplitude modulation depth, 0 to 99.
    pub lfo_amp_mod_depth: u8,
    /// Low frequency oscillator key sync, 0 or 1.
    pub lfo_key_sync: u8,
    /// Low frequency oscillator waveform, 0 to 5: triangle, sawtooth down,
    /// sawtooth up, square, sine, sample and hold.
    pub lfo_wave: u8,
    /// Pitch modulation sensitivity, 0 to 7.
    pub pitch_mod_sensitivity: u8,
    /// Transpose, 0 to 48 semitones; 24 is none.
    pub transpose: u8,
    /// The name, ten characters.
    pub name: [u8; NAME_BYTES],
    /// The operators switched on, voice parameter 155: bit 5 is operator 1
    /// and bit 0 operator 6, a 1 on (`synth7.md`, S7-30; DX7 manual, page
    /// 31; TX7 manual, printed page 39, note *7). No stored voice holds it;
    /// an unpacked voice has all six on.
    pub operators_on: u8,
}

/// A parameter of a voice, to say which one was out of range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// An operator's envelope rate.
    EgRate,
    /// An operator's envelope level.
    EgLevel,
    /// An operator's keyboard level scaling break point.
    BreakPoint,
    /// An operator's keyboard level scaling left depth.
    LeftDepth,
    /// An operator's keyboard level scaling right depth.
    RightDepth,
    /// An operator's keyboard level scaling left curve.
    LeftCurve,
    /// An operator's keyboard level scaling right curve.
    RightCurve,
    /// An operator's keyboard rate scaling.
    RateScaling,
    /// An operator's amplitude modulation sensitivity.
    AmpModSensitivity,
    /// An operator's key velocity sensitivity.
    KeyVelocitySensitivity,
    /// An operator's output level.
    OutputLevel,
    /// An operator's oscillator mode.
    OscillatorMode,
    /// An operator's coarse frequency.
    FrequencyCoarse,
    /// An operator's fine frequency.
    FrequencyFine,
    /// An operator's detune.
    Detune,
    /// A pitch envelope rate.
    PitchEgRate,
    /// A pitch envelope level.
    PitchEgLevel,
    /// The algorithm.
    Algorithm,
    /// The feedback.
    Feedback,
    /// The oscillator key sync.
    OscillatorKeySync,
    /// The low frequency oscillator's speed.
    LfoSpeed,
    /// The low frequency oscillator's delay.
    LfoDelay,
    /// The low frequency oscillator's pitch modulation depth.
    LfoPitchModDepth,
    /// The low frequency oscillator's amplitude modulation depth.
    LfoAmpModDepth,
    /// The low frequency oscillator's key sync.
    LfoKeySync,
    /// The low frequency oscillator's waveform.
    LfoWave,
    /// The pitch modulation sensitivity.
    PitchModSensitivity,
    /// The transpose.
    Transpose,
    /// The mask of operators switched on.
    OperatorsOn,
    /// A character of the name: a seven-bit data byte (S7-32).
    Name,
}

/// Why a voice was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    /// A parameter is above the largest value the instrument gives it.
    OutOfRange {
        /// Which parameter.
        field: Field,
        /// The operator it belongs to, 1 to 6; none for a parameter of the
        /// whole voice.
        operator: Option<u8>,
        /// The value found.
        value: u8,
        /// The largest value allowed.
        max: u8,
    },
}

/// Why a bank was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankError {
    /// The message is not 4,104 bytes long.
    Length,
    /// The message does not start with the six bytes of a 32-voice bulk
    /// dump: F0 43 0n 09 20 00, for any channel n.
    Header,
    /// The checksum is not the two's complement of the sum of the 4,096
    /// data bytes, in seven bits.
    Checksum,
    /// The message does not end with F7.
    Terminator,
    /// A byte before the last has bit 7 set (S7-45).
    DataByte {
        /// Its place, counted from F0.
        at: u16,
    },
    /// A voice was refused.
    Patch {
        /// Which voice, 0 to 31.
        index: u8,
        /// Why.
        error: PatchError,
    },
}

/// One voice of the instrument, every parameter within its range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patch {
    params: VoiceParams,
}

/// A checker of one voice: each call refuses a value above its maximum.
struct Check {
    operator: Option<u8>,
}

impl Check {
    fn field(&self, field: Field, value: u8, max: u8) -> Result<(), PatchError> {
        if value > max {
            return Err(PatchError::OutOfRange { field, operator: self.operator, value, max });
        }
        Ok(())
    }

    fn fields(&self, field: Field, values: [u8; 4], max: u8) -> Result<(), PatchError> {
        values.iter().try_for_each(|&value| self.field(field, value, max))
    }
}

impl Patch {
    /// The voice a part plays when its bank has none, or the voice it holds
    /// was refused: the maker's initial voice, "INIT VOICE" (S7-51).
    /// Algorithm 1; operator 1 alone sounds, at output level 99 and a ratio
    /// of 1.00, with an envelope that starts and stops at once; the other
    /// five are at level 0. Algorithm 1 has two carriers, operators 1 and
    /// 3, so the kernel's carrier compensation sounds operator 1 a doubling
    /// below a single carrier's full level.
    ///
    /// Source: DX7 II manual, printed page Add-8, section 5-1, the "INIT"
    /// column of the voice parameter table, value by value (DEC-6465 answer
    /// 16). The operator switch, which that column does not give, is all
    /// six on (S7-21, S7-54).
    pub const DEFAULT: Self = Self {
        params: VoiceParams {
            operators: [
                Self::default_operator(MAX_99),
                Self::default_operator(0),
                Self::default_operator(0),
                Self::default_operator(0),
                Self::default_operator(0),
                Self::default_operator(0),
            ],
            pitch_eg_rates: [MAX_99; 4],
            pitch_eg_levels: [50; 4],
            algorithm: Self::DEFAULT_ALGORITHM,
            feedback: 0,
            oscillator_key_sync: 1,
            lfo_speed: 35,
            lfo_delay: 0,
            lfo_pitch_mod_depth: 0,
            lfo_amp_mod_depth: 0,
            lfo_key_sync: 1,
            lfo_wave: 0,
            pitch_mod_sensitivity: 3,
            transpose: TRANSPOSE_CENTRE,
            name: *b"INIT VOICE",
            operators_on: ALL_OPERATORS_ON,
        },
    };

    /// The algorithm of [`Patch::DEFAULT`]: the instrument's 1 (S7-31: value
    /// 0 is algorithm 1).
    const DEFAULT_ALGORITHM: u8 = 0;

    /// An operator of [`Patch::DEFAULT`] at an output level.
    const fn default_operator(output_level: u8) -> OperatorParams {
        OperatorParams {
            eg_rates: [MAX_99; 4],
            eg_levels: [MAX_99, MAX_99, MAX_99, 0],
            break_point: 39,
            left_depth: 0,
            right_depth: 0,
            left_curve: 0,
            right_curve: 0,
            rate_scaling: 0,
            amp_mod_sensitivity: 0,
            key_velocity_sensitivity: 0,
            output_level,
            oscillator_mode: 0,
            frequency_coarse: 1,
            frequency_fine: 0,
            detune: 7,
        }
    }

    /// Checks a voice.
    ///
    /// # Errors
    ///
    /// [`PatchError::OutOfRange`] for the first parameter found above its
    /// largest value, naming it. The name is not checked: its bytes are
    /// kept as they are.
    pub fn new(params: VoiceParams) -> Result<Self, PatchError> {
        for (operator, number) in params.operators.iter().zip(1_u8..) {
            let check = Check { operator: Some(number) };
            check.fields(Field::EgRate, operator.eg_rates, MAX_99)?;
            check.fields(Field::EgLevel, operator.eg_levels, MAX_99)?;
            check.field(Field::BreakPoint, operator.break_point, MAX_99)?;
            check.field(Field::LeftDepth, operator.left_depth, MAX_99)?;
            check.field(Field::RightDepth, operator.right_depth, MAX_99)?;
            check.field(Field::LeftCurve, operator.left_curve, MAX_CURVE)?;
            check.field(Field::RightCurve, operator.right_curve, MAX_CURVE)?;
            check.field(Field::RateScaling, operator.rate_scaling, MAX_RATE_SCALING)?;
            check.field(Field::AmpModSensitivity, operator.amp_mod_sensitivity, MAX_AMP_MOD_SENSITIVITY)?;
            check.field(
                Field::KeyVelocitySensitivity,
                operator.key_velocity_sensitivity,
                MAX_KEY_VELOCITY_SENSITIVITY,
            )?;
            check.field(Field::OutputLevel, operator.output_level, MAX_99)?;
            check.field(Field::OscillatorMode, operator.oscillator_mode, MAX_OSCILLATOR_MODE)?;
            check.field(Field::FrequencyCoarse, operator.frequency_coarse, MAX_FREQUENCY_COARSE)?;
            check.field(Field::FrequencyFine, operator.frequency_fine, MAX_99)?;
            check.field(Field::Detune, operator.detune, MAX_DETUNE)?;
        }
        let check = Check { operator: None };
        check.fields(Field::PitchEgRate, params.pitch_eg_rates, MAX_99)?;
        check.fields(Field::PitchEgLevel, params.pitch_eg_levels, MAX_99)?;
        check.field(Field::Algorithm, params.algorithm, MAX_ALGORITHM)?;
        check.field(Field::Feedback, params.feedback, FEEDBACK_MAX)?;
        check.field(Field::OscillatorKeySync, params.oscillator_key_sync, MAX_SWITCH)?;
        check.field(Field::LfoSpeed, params.lfo_speed, MAX_99)?;
        check.field(Field::LfoDelay, params.lfo_delay, MAX_99)?;
        check.field(Field::LfoPitchModDepth, params.lfo_pitch_mod_depth, MAX_99)?;
        check.field(Field::LfoAmpModDepth, params.lfo_amp_mod_depth, MAX_99)?;
        check.field(Field::LfoKeySync, params.lfo_key_sync, MAX_SWITCH)?;
        check.field(Field::LfoWave, params.lfo_wave, MAX_LFO_WAVE)?;
        check.field(Field::PitchModSensitivity, params.pitch_mod_sensitivity, MAX_PITCH_MOD_SENSITIVITY)?;
        check.field(Field::Transpose, params.transpose, MAX_TRANSPOSE)?;
        check.field(Field::OperatorsOn, params.operators_on, ALL_OPERATORS_ON)?;
        Ok(Self { params })
    }

    /// Unpacks and checks one voice of a bulk dump.
    ///
    /// Bit 7 of every byte, which a MIDI data byte cannot carry, and the
    /// bits the format leaves unused (bits 4 to 6 of byte 11 of an
    /// operator, bits 5 and 6 of its byte 13, bit 6 of its byte 15; bits 5
    /// and 6 of byte 110 and bits 4 to 6 of byte 111) are ignored, as the
    /// instrument's own masks ignore them. Every field is then checked
    /// against its range. All six operators are switched on.
    ///
    /// # Errors
    ///
    /// [`PatchError::OutOfRange`] for the first field found above its
    /// largest value, naming it.
    pub fn from_packed(packed: &[u8; PACKED_VOICE_BYTES]) -> Result<Self, PatchError> {
        let mut operators = [Self::default_operator(0); OPERATORS];
        // The packed form holds operator 6 first.
        let (packed_operators, _) = packed.as_chunks::<PACKED_OPERATOR_BYTES>();
        for (operator, bytes) in operators.iter_mut().rev().zip(packed_operators) {
            let &[r1, r2, r3, r4, l1, l2, l3, l4, bp, ld, rd, curves, detune_rs, kvs_ams, ol, coarse_mode, fine] =
                bytes;
            *operator = OperatorParams {
                eg_rates: [r1, r2, r3, r4].map(|byte| byte & DATA_MASK),
                eg_levels: [l1, l2, l3, l4].map(|byte| byte & DATA_MASK),
                break_point: bp & DATA_MASK,
                left_depth: ld & DATA_MASK,
                right_depth: rd & DATA_MASK,
                left_curve: curves & 3,
                right_curve: (curves >> 2) & 3,
                rate_scaling: detune_rs & 7,
                detune: (detune_rs >> 3) & 0x0f,
                amp_mod_sensitivity: kvs_ams & 3,
                key_velocity_sensitivity: (kvs_ams >> 2) & 7,
                output_level: ol & DATA_MASK,
                oscillator_mode: coarse_mode & 1,
                frequency_coarse: (coarse_mode >> 1) & 0x1f,
                frequency_fine: fine & DATA_MASK,
            };
        }
        let [
            ..,
            pr1,
            pr2,
            pr3,
            pr4,
            pl1,
            pl2,
            pl3,
            pl4,
            algorithm,
            sync_feedback,
            speed,
            delay,
            pmd,
            amd,
            lfo,
            transpose,
            n0,
            n1,
            n2,
            n3,
            n4,
            n5,
            n6,
            n7,
            n8,
            n9,
        ] = *packed;
        Self::new(VoiceParams {
            operators,
            pitch_eg_rates: [pr1, pr2, pr3, pr4].map(|byte| byte & DATA_MASK),
            pitch_eg_levels: [pl1, pl2, pl3, pl4].map(|byte| byte & DATA_MASK),
            algorithm: algorithm & 0x1f,
            feedback: sync_feedback & 7,
            oscillator_key_sync: (sync_feedback >> 3) & 1,
            lfo_speed: speed & DATA_MASK,
            lfo_delay: delay & DATA_MASK,
            lfo_pitch_mod_depth: pmd & DATA_MASK,
            lfo_amp_mod_depth: amd & DATA_MASK,
            lfo_key_sync: lfo & 1,
            lfo_wave: (lfo >> 1) & 7,
            pitch_mod_sensitivity: (lfo >> 4) & 7,
            transpose: transpose & DATA_MASK,
            name: [n0, n1, n2, n3, n4, n5, n6, n7, n8, n9].map(|byte| byte & DATA_MASK),
            operators_on: ALL_OPERATORS_ON,
        })
    }

    /// The voice in the packed form of a bulk dump. Which operators are
    /// switched on is not part of that form and is lost.
    #[must_use]
    pub fn to_packed(&self) -> [u8; PACKED_VOICE_BYTES] {
        let mut packed = [0_u8; PACKED_VOICE_BYTES];
        let params = &self.params;
        let (packed_operators, _) = packed.as_chunks_mut::<PACKED_OPERATOR_BYTES>();
        for (operator, bytes) in params.operators.iter().rev().zip(packed_operators) {
            let [r1, r2, r3, r4] = operator.eg_rates;
            let [l1, l2, l3, l4] = operator.eg_levels;
            let fields = [
                r1,
                r2,
                r3,
                r4,
                l1,
                l2,
                l3,
                l4,
                operator.break_point,
                operator.left_depth,
                operator.right_depth,
                operator.left_curve | (operator.right_curve << 2),
                operator.rate_scaling | (operator.detune << 3),
                operator.amp_mod_sensitivity | (operator.key_velocity_sensitivity << 2),
                operator.output_level,
                operator.oscillator_mode | (operator.frequency_coarse << 1),
                operator.frequency_fine,
            ];
            *bytes = fields;
        }
        let [pr1, pr2, pr3, pr4] = params.pitch_eg_rates;
        let [pl1, pl2, pl3, pl4] = params.pitch_eg_levels;
        let globals = [
            pr1,
            pr2,
            pr3,
            pr4,
            pl1,
            pl2,
            pl3,
            pl4,
            params.algorithm,
            params.feedback | (params.oscillator_key_sync << 3),
            params.lfo_speed,
            params.lfo_delay,
            params.lfo_pitch_mod_depth,
            params.lfo_amp_mod_depth,
            params.lfo_key_sync | (params.lfo_wave << 1) | (params.pitch_mod_sensitivity << 4),
            params.transpose,
        ];
        let tail = packed.iter_mut().skip(PACKED_OPERATOR_BYTES.saturating_mul(OPERATORS));
        for (byte, field) in tail.zip(globals.into_iter().chain(params.name)) {
            *byte = field;
        }
        packed
    }

    /// The voice's parameters.
    #[must_use]
    pub const fn params(&self) -> &VoiceParams {
        &self.params
    }

    /// The voice as its 155 voice parameters 0 to 154, in the order of the
    /// parameter change's numbers and of a single-voice dump's data (S7-28,
    /// S7-38): operator 6's 21 first, operator 1's at 105 to 125, then the
    /// voice's own 126 to 154.
    #[must_use]
    pub fn to_parameters(&self) -> [u8; VOICE_PARAMETERS] {
        let mut out = [0_u8; VOICE_PARAMETERS];
        let params = &self.params;
        let (blocks, tail) = out.split_at_mut(OPERATOR_PARAMETERS.saturating_mul(OPERATORS));
        let (blocks, _) = blocks.as_chunks_mut::<OPERATOR_PARAMETERS>();
        // Operator 6 first: the operators array holds operator 1 first.
        for (block, operator) in blocks.iter_mut().zip(params.operators.iter().rev()) {
            let [r1, r2, r3, r4] = operator.eg_rates;
            let [l1, l2, l3, l4] = operator.eg_levels;
            *block = [
                r1,
                r2,
                r3,
                r4,
                l1,
                l2,
                l3,
                l4,
                operator.break_point,
                operator.left_depth,
                operator.right_depth,
                operator.left_curve,
                operator.right_curve,
                operator.rate_scaling,
                operator.amp_mod_sensitivity,
                operator.key_velocity_sensitivity,
                operator.output_level,
                operator.oscillator_mode,
                operator.frequency_coarse,
                operator.frequency_fine,
                operator.detune,
            ];
        }
        let [pr1, pr2, pr3, pr4] = params.pitch_eg_rates;
        let [pl1, pl2, pl3, pl4] = params.pitch_eg_levels;
        let voice = [
            pr1,
            pr2,
            pr3,
            pr4,
            pl1,
            pl2,
            pl3,
            pl4,
            params.algorithm,
            params.feedback,
            params.oscillator_key_sync,
            params.lfo_speed,
            params.lfo_delay,
            params.lfo_pitch_mod_depth,
            params.lfo_amp_mod_depth,
            params.lfo_key_sync,
            params.lfo_wave,
            params.pitch_mod_sensitivity,
            params.transpose,
        ];
        for (place, value) in tail.iter_mut().zip(voice.into_iter().chain(params.name)) {
            *place = value;
        }
        out
    }

    /// Reads and checks a voice from its 155 voice parameters, as
    /// [`Patch::to_parameters`] lays them out, with every operator on.
    ///
    /// # Errors
    ///
    /// [`PatchError::OutOfRange`] for the first parameter above its range
    /// (S7-29), the name's characters above 127 among them (S7-32).
    pub fn from_parameters(values: &[u8; VOICE_PARAMETERS]) -> Result<Self, PatchError> {
        let (blocks, tail) = values.split_at(OPERATOR_PARAMETERS.saturating_mul(OPERATORS));
        let (blocks, _) = blocks.as_chunks::<OPERATOR_PARAMETERS>();
        let mut operators = [Self::default_operator(0); OPERATORS];
        for (operator, block) in operators.iter_mut().rev().zip(blocks) {
            let &[r1, r2, r3, r4, l1, l2, l3, l4, bp, ld, rd, lc, rc, rs, ams, kvs, ol, mode, coarse, fine, detune] =
                block;
            *operator = OperatorParams {
                eg_rates: [r1, r2, r3, r4],
                eg_levels: [l1, l2, l3, l4],
                break_point: bp,
                left_depth: ld,
                right_depth: rd,
                left_curve: lc,
                right_curve: rc,
                rate_scaling: rs,
                amp_mod_sensitivity: ams,
                key_velocity_sensitivity: kvs,
                output_level: ol,
                oscillator_mode: mode,
                frequency_coarse: coarse,
                frequency_fine: fine,
                detune,
            };
        }
        let Some((
            &[
                pr1,
                pr2,
                pr3,
                pr4,
                pl1,
                pl2,
                pl3,
                pl4,
                algorithm,
                feedback,
                sync,
                speed,
                delay,
                pmd,
                amd,
                lfo_sync,
                wave,
                pms,
                transpose,
            ],
            name,
        )) = tail.split_first_chunk::<VOICE_OWN_PARAMETERS>()
        else {
            return Err(PatchError::OutOfRange { field: Field::Name, operator: None, value: 0, max: 0 });
        };
        let mut name_bytes = [0_u8; NAME_BYTES];
        for (to, &from) in name_bytes.iter_mut().zip(name) {
            if from > DATA_MASK {
                return Err(PatchError::OutOfRange { field: Field::Name, operator: None, value: from, max: DATA_MASK });
            }
            *to = from;
        }
        Self::new(VoiceParams {
            operators,
            pitch_eg_rates: [pr1, pr2, pr3, pr4],
            pitch_eg_levels: [pl1, pl2, pl3, pl4],
            algorithm,
            feedback,
            oscillator_key_sync: sync,
            lfo_speed: speed,
            lfo_delay: delay,
            lfo_pitch_mod_depth: pmd,
            lfo_amp_mod_depth: amd,
            lfo_key_sync: lfo_sync,
            lfo_wave: wave,
            pitch_mod_sensitivity: pms,
            transpose,
            name: name_bytes,
            operators_on: ALL_OPERATORS_ON,
        })
    }

    /// Voice parameter `number`, 0 to 155 (S7-28), or `None` above 155.
    #[must_use]
    pub fn parameter(&self, number: u16) -> Option<u8> {
        if number == OPERATOR_SWITCH_PARAMETER {
            return Some(self.params.operators_on);
        }
        self.to_parameters().get(usize::from(number)).copied()
    }

    /// The voice with voice parameter `number` (0 to 155) set to `value`:
    /// what a parameter change of group 0 does (S7-44). The operator switch
    /// (155) is kept as it is unless it is the one set.
    ///
    /// # Errors
    ///
    /// [`PatchError::OutOfRange`] for a value above the parameter's range
    /// (S7-29), or for a number above 155 (as [`Field::OperatorsOn`]).
    pub fn with_parameter(&self, number: u16, value: u8) -> Result<Self, PatchError> {
        if number == OPERATOR_SWITCH_PARAMETER {
            let mut params = self.params;
            params.operators_on = value;
            return Self::new(params);
        }
        let mut values = self.to_parameters();
        let Some(place) = values.get_mut(usize::from(number)) else {
            return Err(PatchError::OutOfRange {
                field: Field::OperatorsOn,
                operator: None,
                value,
                max: ALL_OPERATORS_ON,
            });
        };
        *place = value;
        let mut patch = Self::from_parameters(&values)?;
        patch.params.operators_on = self.params.operators_on;
        Ok(patch)
    }

    /// The voice with every operator on: what a program change or a
    /// single-voice dump gives a part (S7-21, S7-40).
    #[must_use]
    pub const fn all_operators_on(mut self) -> Self {
        self.params.operators_on = ALL_OPERATORS_ON;
        self
    }
}

/// The voice parameters a voice holds: 0 to 154 (S7-28).
///
/// Source: TX7 manual, printed page 41, section 4-5: "155 bytes of data …
/// parameters 0 thru 154".
pub const VOICE_PARAMETERS: usize = 155;
/// The parameters of each operator, at offsets 0 to 20 of its block.
///
/// Source: TX7 manual, printed page 38, section 4-1: "21 ~ 41 OP5" and on.
pub const OPERATOR_PARAMETERS: usize = 21;
/// The voice parameters after the six operators' and before the name:
/// 126 to 144 (S7-28).
const VOICE_OWN_PARAMETERS: usize = 19;
/// Voice parameter 155, OPERATOR ON/OFF: a parameter of the part's current
/// voice that no stored voice holds (S7-30).
///
/// Source: DX7 manual, page 31.
pub const OPERATOR_SWITCH_PARAMETER: u16 = 155;
/// The highest voice parameter number a parameter change sets: 155. The
/// TX7's 156, OPERATOR SELECT, is not taken (S7-43).
pub const LAST_VOICE_PARAMETER: u16 = OPERATOR_SWITCH_PARAMETER;
// The operators' blocks, the voice's own parameters and the name are the
// 155, and the packed form holds the same in 128 bytes.
const _: () = assert!(OPERATOR_PARAMETERS * OPERATORS + VOICE_OWN_PARAMETERS + NAME_BYTES == VOICE_PARAMETERS);

/// The largest value of each voice parameter 0 to 155, as S7-28's table
/// gives them: operator 6's 21 first, then operators 5 to 1, the voice's
/// own, the ten characters of the name (seven bits each) and the operator
/// switch.
///
/// Source: DX7 manual, page 31; TX7 manual, printed page 38.
pub const PARAMETER_MAX: [u8; VOICE_PARAMETERS + 1] = parameter_max();

/// The largest value of each operator parameter, offsets 0 to 20 (S7-28).
const OPERATOR_PARAMETER_MAX: [u8; OPERATOR_PARAMETERS] = [
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_CURVE,
    MAX_CURVE,
    MAX_RATE_SCALING,
    MAX_AMP_MOD_SENSITIVITY,
    MAX_KEY_VELOCITY_SENSITIVITY,
    MAX_99,
    MAX_OSCILLATOR_MODE,
    MAX_FREQUENCY_COARSE,
    MAX_99,
    MAX_DETUNE,
];

/// The largest value of the voice's own parameters, 126 to 144 (S7-28).
const VOICE_OWN_PARAMETER_MAX: [u8; VOICE_OWN_PARAMETERS] = [
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_ALGORITHM,
    FEEDBACK_MAX,
    MAX_SWITCH,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_99,
    MAX_SWITCH,
    MAX_LFO_WAVE,
    MAX_PITCH_MOD_SENSITIVITY,
    MAX_TRANSPOSE,
];

/// The largest value of a parameter of two settings, off and on: the two
/// sync parameters 136 and 141 (S7-31).
///
/// Source: DX7 manual, page 31: "0 ~ 1".
pub const MAX_SWITCH: u8 = 1;

#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "evaluated only at compile time, where an index out of range fails the build"
)]
const fn parameter_max() -> [u8; VOICE_PARAMETERS + 1] {
    let mut table = [DATA_MASK; VOICE_PARAMETERS + 1];
    let mut index = 0;
    while index < OPERATOR_PARAMETERS * OPERATORS {
        table[index] = OPERATOR_PARAMETER_MAX[index % OPERATOR_PARAMETERS];
        index += 1;
    }
    let mut own = 0;
    while own < VOICE_OWN_PARAMETERS {
        table[OPERATOR_PARAMETERS * OPERATORS + own] = VOICE_OWN_PARAMETER_MAX[own];
        own += 1;
    }
    table[VOICE_PARAMETERS] = ALL_OPERATORS_ON;
    table
}

impl Default for Patch {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Up to 32 voices, each checked; a place may be empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bank {
    patches: [Option<Patch>; BANK_VOICES],
}

impl Default for Bank {
    fn default() -> Self {
        Self::new()
    }
}

/// The length of a bulk dump of 32 voices: 4,104 bytes (S7-41): a header
/// of six bytes, 32 packed voices, the checksum and `F7`.
///
/// Source: DX7 manual, printed page 30, "Bulk data of 32 voices".
pub const SYSEX_BYTES: usize = 6 + BANK_VOICES * PACKED_VOICE_BYTES + 2;

impl Bank {
    /// An empty bank.
    #[must_use]
    pub const fn new() -> Self {
        Self { patches: [None; BANK_VOICES] }
    }

    /// Puts a voice at `index`, 0 to 31; a larger index does nothing.
    pub fn set(&mut self, index: usize, patch: Patch) {
        if let Some(place) = self.patches.get_mut(index) {
            *place = Some(patch);
        }
    }

    /// The voice at `index`, 0 to 31; none for an empty place or a larger
    /// index.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Patch> {
        self.patches.get(index).and_then(Option::as_ref)
    }

    /// Reads a bank from the instrument's bulk dump of 32 voices: a system
    /// exclusive message of 4,104 bytes (S7-41), every voice of which must
    /// be in range.
    ///
    /// # Errors
    ///
    /// - [`BankError::Length`] if `message` is not 4,104 bytes.
    /// - [`BankError::Header`] if it does not start F0 43 0n 09 20 00.
    /// - [`BankError::DataByte`] if a byte before the last has bit 7 set.
    /// - [`BankError::Terminator`] if it does not end with F7.
    /// - [`BankError::Checksum`] if the sum of the 4,096 data bytes and the
    ///   checksum byte is not zero in its low seven bits.
    /// - [`BankError::Patch`] for the first voice [`Patch::from_packed`]
    ///   refuses.
    pub fn from_sysex(message: &[u8]) -> Result<Self, BankError> {
        let data = check_bank_dump(message).map_err(BankError::from)?;
        let mut bank = Self::new();
        for (index, place) in bank.patches.iter_mut().enumerate() {
            *place = Some(bank_voice(data, index).map_err(BankError::from)?);
        }
        Ok(bank)
    }
}

impl From<DumpError> for BankError {
    fn from(error: DumpError) -> Self {
        match error {
            DumpError::Length => Self::Length,
            DumpError::Header => Self::Header,
            DumpError::DataByte { at } => Self::DataByte { at },
            DumpError::Checksum => Self::Checksum,
            DumpError::Terminator => Self::Terminator,
            DumpError::Patch { index, error } => Self::Patch { index, error },
        }
    }
}
