//! Patches: the instrument's voices, checked, and a bank of 32 of them.
//!
//! A voice arrives as 128 bytes, the packed form in which the instrument
//! sends its 32 voices in bulk. [`Patch::from_packed`] unpacks one and
//! checks every field against its range, so that nothing after it can meet
//! a value outside the instrument's own; a [`Patch`] cannot be made any
//! other way than through that check.
//!
//! Source: Yamaha DX7 owner's manual, "MIDI data format" (printed pages 30
//! to 32), for the bulk message and each parameter's range; the packed
//! layout is the instrument's data format sheet as transcribed in "DX7
//! Data Format" (E. Macpherson, 1993, section F), which agrees with the
//! unpacking in music-synthesizer-for-android's `patch.cc`.

use super::kernel::{ALGORITHM_COUNT, FEEDBACK_MAX, OPERATORS};

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
/// Source: "DX7 Data Format", section F: operator 6 is bytes 0 to 16.
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
/// manual's table says 4; the instrument has six waveforms, 0 to 5.
///
/// Source: "DX7 Data Format", section D: "0-5, (data sheet claims 9-4 ?!?)
/// 0:TR, 1:SD, 2:SU, 3:SQ, 4:SI, 5:SH".
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
/// Source: "DX7 Data Format", section D: "TRANSPOSE 0-48 12 = C2".
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
    /// The operators switched on: bit 0 is operator 1. The packed form does
    /// not hold it; an unpacked voice has all six on.
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
    /// The voice a part plays when its bank has none: a plain sine.
    /// Operator 1 alone sounds, at full level and a ratio of 1.00, with an
    /// envelope that starts and stops at once; the other five are at level
    /// 0. The algorithm is the instrument's 16, in which operator 1 is the
    /// only carrier, so that the sine is at a single carrier's full level:
    /// in algorithm 32 each of six carriers is attenuated for six.
    ///
    /// Source: own choice of every value.
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
            name: *b"PLAIN SINE",
            operators_on: ALL_OPERATORS_ON,
        },
    };

    /// The algorithm of [`Patch::DEFAULT`]: the instrument's 16.
    const DEFAULT_ALGORITHM: u8 = 15;

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
        check.field(Field::OscillatorKeySync, params.oscillator_key_sync, 1)?;
        check.field(Field::LfoSpeed, params.lfo_speed, MAX_99)?;
        check.field(Field::LfoDelay, params.lfo_delay, MAX_99)?;
        check.field(Field::LfoPitchModDepth, params.lfo_pitch_mod_depth, MAX_99)?;
        check.field(Field::LfoAmpModDepth, params.lfo_amp_mod_depth, MAX_99)?;
        check.field(Field::LfoKeySync, params.lfo_key_sync, 1)?;
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

/// The bytes before the voices of a bulk dump of 32 voices.
///
/// Source: Yamaha DX7 owner's manual, printed page 30, "Bulk data of 32
/// voices": status, identification, sub-status and channel, format number
/// and the two bytes of the count.
const SYSEX_HEADER_BYTES: usize = 6;
/// The bytes after the voices: the checksum and the end of exclusive.
const SYSEX_TRAILER_BYTES: usize = 2;
/// The length of a bulk dump of 32 voices: 4,104 bytes.
pub const SYSEX_BYTES: usize = SYSEX_HEADER_BYTES + BANK_VOICES * PACKED_VOICE_BYTES + SYSEX_TRAILER_BYTES;
/// The status byte that starts a system exclusive message.
const SYSEX_START: u8 = 0xf0;
/// The status byte that ends one.
const SYSEX_END: u8 = 0xf7;
/// The maker's identification number in a bank's message: 67.
const SYSEX_MAKER: u8 = 0x43;
/// The format number of 32 voices.
const SYSEX_FORMAT_32_VOICES: u8 = 0x09;
/// The two bytes of the count, 4,096, seven bits in each.
const SYSEX_COUNT: [u8; 2] = [0x20, 0x00];
/// The bits of the third byte that must be zero: the sub-status. The four
/// below it are the channel, which may be any.
const SYSEX_SUB_STATUS_MASK: u8 = 0xf0;

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
    /// exclusive message of 4,104 bytes.
    ///
    /// # Errors
    ///
    /// - [`BankError::Length`] if `message` is not 4,104 bytes.
    /// - [`BankError::Header`] if it does not start F0 43 0n 09 20 00.
    /// - [`BankError::Terminator`] if it does not end with F7.
    /// - [`BankError::Checksum`] if the sum of the 4,096 data bytes and the
    ///   checksum byte is not zero in its low seven bits.
    /// - [`BankError::Patch`] for the first voice [`Patch::from_packed`]
    ///   refuses.
    pub fn from_sysex(message: &[u8]) -> Result<Self, BankError> {
        if message.len() != SYSEX_BYTES {
            return Err(BankError::Length);
        }
        let Some((&[start, maker, channel, format, count_high, count_low], rest)) = message.split_first_chunk() else {
            return Err(BankError::Length);
        };
        if start != SYSEX_START
            || maker != SYSEX_MAKER
            || channel & SYSEX_SUB_STATUS_MASK != 0
            || format != SYSEX_FORMAT_32_VOICES
            || [count_high, count_low] != SYSEX_COUNT
        {
            return Err(BankError::Header);
        }
        let Some((data, &[checksum, end])) = rest.split_last_chunk() else {
            return Err(BankError::Length);
        };
        if end != SYSEX_END {
            return Err(BankError::Terminator);
        }
        let sum = data.iter().fold(checksum, |sum, &byte| sum.wrapping_add(byte));
        if sum & DATA_MASK != 0 {
            return Err(BankError::Checksum);
        }
        let mut bank = Self::new();
        let (voices, _) = data.as_chunks::<PACKED_VOICE_BYTES>();
        for ((place, packed), index) in bank.patches.iter_mut().zip(voices).zip(0_u8..) {
            *place = Some(Patch::from_packed(packed).map_err(|error| BankError::Patch { index, error })?);
        }
        Ok(bank)
    }
}
