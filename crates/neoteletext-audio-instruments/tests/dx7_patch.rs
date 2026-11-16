//! DX7 patches: the packed form, every field's range, and the bulk dump of
//! a bank (REQ-6412 item 5).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

use neoteletext_audio_instruments::dx7::patch::{PACKED_VOICE_BYTES, SYSEX_BYTES};
use neoteletext_audio_instruments::dx7::{Bank, BankError, Field, Patch, PatchError, Rack, VoiceParams};

/// A voice in the packed form with a different, valid value in every
/// field, written byte by byte from the data format sheet.
fn hand_built() -> [u8; PACKED_VOICE_BYTES] {
    let mut packed = [0_u8; PACKED_VOICE_BYTES];
    for stored in 0..6_u8 {
        // The packed form holds operator 6 first.
        let number = 6 - stored;
        let base = usize::from(stored) * 17;
        packed[base..base + 4].copy_from_slice(&[90 + number, 80 + number, 70 + number, 60 + number]);
        packed[base + 4..base + 8].copy_from_slice(&[99 - number, 50 + number, 40 + number, number]);
        packed[base + 8] = 30 + number; // break point
        packed[base + 9] = 10 + number; // left depth
        packed[base + 10] = 20 + number; // right depth
        packed[base + 11] = (number & 3) | (((number + 1) & 3) << 2); // left curve, right curve
        packed[base + 12] = (number & 7) | ((number + 8) << 3); // rate scaling, detune
        packed[base + 13] = (number & 3) | ((number + 1) << 2); // amp mod sensitivity, key velocity sensitivity
        packed[base + 14] = 93 + number; // output level
        packed[base + 15] = (number & 1) | ((number + 20) << 1); // mode, coarse
        packed[base + 16] = 11 * number; // fine
    }
    packed[102..106].copy_from_slice(&[91, 92, 93, 94]); // pitch envelope rates
    packed[106..110].copy_from_slice(&[51, 52, 53, 54]); // pitch envelope levels
    packed[110] = 21; // algorithm
    packed[111] = 5 | (1 << 3); // feedback, oscillator key sync
    packed[112] = 33; // speed
    packed[113] = 44; // delay
    packed[114] = 55; // pitch modulation depth
    packed[115] = 66; // amplitude modulation depth
    packed[116] = 1 | (4 << 1) | (6 << 4); // key sync, wave, pitch modulation sensitivity
    packed[117] = 36; // transpose
    packed[118..128].copy_from_slice(b"HAND BUILT");
    packed
}

#[test]
fn a_hand_built_voice_unpacks_to_its_fields() {
    let patch = Patch::from_packed(&hand_built()).unwrap();
    let params = patch.params();
    for (index, operator) in params.operators.iter().enumerate() {
        let number = index as u8 + 1;
        assert_eq!(operator.eg_rates, [90 + number, 80 + number, 70 + number, 60 + number]);
        assert_eq!(operator.eg_levels, [99 - number, 50 + number, 40 + number, number]);
        assert_eq!(operator.break_point, 30 + number);
        assert_eq!(operator.left_depth, 10 + number);
        assert_eq!(operator.right_depth, 20 + number);
        assert_eq!(operator.left_curve, number & 3);
        assert_eq!(operator.right_curve, (number + 1) & 3);
        assert_eq!(operator.rate_scaling, number & 7);
        assert_eq!(operator.detune, number + 8);
        assert_eq!(operator.amp_mod_sensitivity, number & 3);
        assert_eq!(operator.key_velocity_sensitivity, number + 1);
        assert_eq!(operator.output_level, 93 + number);
        assert_eq!(operator.oscillator_mode, number & 1);
        assert_eq!(operator.frequency_coarse, number + 20);
        assert_eq!(operator.frequency_fine, 11 * number);
    }
    assert_eq!(params.pitch_eg_rates, [91, 92, 93, 94]);
    assert_eq!(params.pitch_eg_levels, [51, 52, 53, 54]);
    assert_eq!(params.algorithm, 21);
    assert_eq!(params.feedback, 5);
    assert_eq!(params.oscillator_key_sync, 1);
    assert_eq!(params.lfo_speed, 33);
    assert_eq!(params.lfo_delay, 44);
    assert_eq!(params.lfo_pitch_mod_depth, 55);
    assert_eq!(params.lfo_amp_mod_depth, 66);
    assert_eq!(params.lfo_key_sync, 1);
    assert_eq!(params.lfo_wave, 4);
    assert_eq!(params.pitch_mod_sensitivity, 6);
    assert_eq!(params.transpose, 36);
    assert_eq!(&params.name, b"HAND BUILT");
    assert_eq!(params.operators_on, 0b11_1111);
}

#[test]
fn a_hand_built_voice_round_trips() {
    let packed = hand_built();
    let patch = Patch::from_packed(&packed).unwrap();
    assert_eq!(patch.to_packed(), packed);
    assert_eq!(Patch::from_packed(&patch.to_packed()).unwrap(), patch);
    let default = Patch::DEFAULT;
    assert_eq!(Patch::from_packed(&default.to_packed()).unwrap(), default);
    assert_eq!(Patch::new(*default.params()).unwrap(), default);
}

/// Every field of a voice with its largest value, as (field, operator,
/// maximum, how to set it).
type Setter = fn(&mut VoiceParams, u8);
fn fields() -> Vec<(Field, Option<u8>, u8, Setter)> {
    let mut all: Vec<(Field, Option<u8>, u8, Setter)> = Vec::new();
    macro_rules! operator_fields {
        ($($index:literal),*) => {$(
            let number = Some($index + 1);
            all.push((Field::EgRate, number, 99, |v, x| v.operators[$index].eg_rates[0] = x));
            all.push((Field::EgRate, number, 99, |v, x| v.operators[$index].eg_rates[3] = x));
            all.push((Field::EgLevel, number, 99, |v, x| v.operators[$index].eg_levels[0] = x));
            all.push((Field::EgLevel, number, 99, |v, x| v.operators[$index].eg_levels[3] = x));
            all.push((Field::BreakPoint, number, 99, |v, x| v.operators[$index].break_point = x));
            all.push((Field::LeftDepth, number, 99, |v, x| v.operators[$index].left_depth = x));
            all.push((Field::RightDepth, number, 99, |v, x| v.operators[$index].right_depth = x));
            all.push((Field::LeftCurve, number, 3, |v, x| v.operators[$index].left_curve = x));
            all.push((Field::RightCurve, number, 3, |v, x| v.operators[$index].right_curve = x));
            all.push((Field::RateScaling, number, 7, |v, x| v.operators[$index].rate_scaling = x));
            all.push((Field::AmpModSensitivity, number, 3, |v, x| v.operators[$index].amp_mod_sensitivity = x));
            all.push((Field::KeyVelocitySensitivity, number, 7, |v, x| v.operators[$index].key_velocity_sensitivity = x));
            all.push((Field::OutputLevel, number, 99, |v, x| v.operators[$index].output_level = x));
            all.push((Field::OscillatorMode, number, 1, |v, x| v.operators[$index].oscillator_mode = x));
            all.push((Field::FrequencyCoarse, number, 31, |v, x| v.operators[$index].frequency_coarse = x));
            all.push((Field::FrequencyFine, number, 99, |v, x| v.operators[$index].frequency_fine = x));
            all.push((Field::Detune, number, 14, |v, x| v.operators[$index].detune = x));
        )*};
    }
    operator_fields!(0, 1, 2, 3, 4, 5);
    all.push((Field::PitchEgRate, None, 99, |v, x| v.pitch_eg_rates[1] = x));
    all.push((Field::PitchEgLevel, None, 99, |v, x| v.pitch_eg_levels[2] = x));
    all.push((Field::Algorithm, None, 31, |v, x| v.algorithm = x));
    all.push((Field::Feedback, None, 7, |v, x| v.feedback = x));
    all.push((Field::OscillatorKeySync, None, 1, |v, x| v.oscillator_key_sync = x));
    all.push((Field::LfoSpeed, None, 99, |v, x| v.lfo_speed = x));
    all.push((Field::LfoDelay, None, 99, |v, x| v.lfo_delay = x));
    all.push((Field::LfoPitchModDepth, None, 99, |v, x| v.lfo_pitch_mod_depth = x));
    all.push((Field::LfoAmpModDepth, None, 99, |v, x| v.lfo_amp_mod_depth = x));
    all.push((Field::LfoKeySync, None, 1, |v, x| v.lfo_key_sync = x));
    all.push((Field::LfoWave, None, 5, |v, x| v.lfo_wave = x));
    all.push((Field::PitchModSensitivity, None, 7, |v, x| v.pitch_mod_sensitivity = x));
    all.push((Field::Transpose, None, 48, |v, x| v.transpose = x));
    all.push((Field::OperatorsOn, None, 0b11_1111, |v, x| v.operators_on = x));
    all
}

#[test]
fn each_field_is_accepted_at_its_maximum_and_refused_above_it() {
    let fields = fields();
    assert_eq!(fields.len(), 6 * 17 + 14);
    for (field, operator, max, set) in fields {
        let mut params = *Patch::DEFAULT.params();
        set(&mut params, max);
        let patch = Patch::new(params).unwrap_or_else(|error| panic!("{field:?} at {max}: {error:?}"));
        // At its maximum the voice plays, and survives the packed form
        // (which does not hold the operators switched on).
        if field != Field::OperatorsOn {
            assert_eq!(Patch::from_packed(&patch.to_packed()).unwrap(), patch, "{field:?}");
        }
        let mut bank = Bank::new();
        bank.set(0, patch);
        let mut rack = Rack::new();
        rack.set_bank(&bank);
        rack.event(0x90, 60, 127);
        rack.render(&mut [0_i16; 960]);

        set(&mut params, max + 1);
        assert_eq!(
            Patch::new(params),
            Err(PatchError::OutOfRange { field, operator, value: max + 1, max }),
            "{field:?} at {}",
            max + 1
        );
    }
}

/// The fields of the packed form that can hold more than their range, each
/// one above its maximum.
#[test]
fn a_packed_field_above_its_maximum_is_refused_by_name() {
    let good = Patch::DEFAULT.to_packed();
    let cases: [(usize, u8, Field, Option<u8>, u8, u8); 9] = [
        (0, 100, Field::EgRate, Some(6), 100, 99),
        (17 + 7, 127, Field::EgLevel, Some(5), 127, 99),
        (34 + 8, 100, Field::BreakPoint, Some(4), 100, 99),
        (51 + 12, 15 << 3, Field::Detune, Some(3), 15, 14),
        (68 + 14, 100, Field::OutputLevel, Some(2), 100, 99),
        (85 + 16, 100, Field::FrequencyFine, Some(1), 100, 99),
        (109, 100, Field::PitchEgLevel, None, 100, 99),
        (116, 6 << 1, Field::LfoWave, None, 6, 5),
        (117, 49, Field::Transpose, None, 49, 48),
    ];
    for (byte, written, field, operator, value, max) in cases {
        let mut packed = good;
        packed[byte] = written;
        assert_eq!(
            Patch::from_packed(&packed),
            Err(PatchError::OutOfRange { field, operator, value, max }),
            "byte {byte}"
        );
    }
}

/// Bit 7 of any byte and the unused bits of the format are ignored: the
/// voice is the one without them.
#[test]
fn bit_seven_and_unused_bits_are_ignored() {
    let packed = hand_built();
    let patch = Patch::from_packed(&packed).unwrap();
    for byte in 0..PACKED_VOICE_BYTES {
        let mut with_bit = packed;
        with_bit[byte] |= 0x80;
        assert_eq!(Patch::from_packed(&with_bit), Ok(patch), "byte {byte}");
    }
    let mut unused = packed;
    for stored in 0..6 {
        unused[stored * 17 + 11] |= 0x70;
        unused[stored * 17 + 13] |= 0x60;
        unused[stored * 17 + 15] |= 0x40;
    }
    unused[110] |= 0x60;
    unused[111] |= 0x70;
    assert_eq!(Patch::from_packed(&unused), Ok(patch));

    // Byte 116 with bit 7 set and every other bit too: wave 7 is refused,
    // by name, without a panic.
    let mut all_ones = packed;
    all_ones[116] = 0xff;
    assert_eq!(
        Patch::from_packed(&all_ones),
        Err(PatchError::OutOfRange { field: Field::LfoWave, operator: None, value: 7, max: 5 })
    );
}

/// A bulk dump of 32 voices: the hand-built voice in every place, each
/// with its place's number as its transpose.
fn sysex(channel: u8) -> Vec<u8> {
    let mut message = vec![0xf0, 0x43, channel, 0x09, 0x20, 0x00];
    for index in 0..32_u8 {
        let mut voice = hand_built();
        voice[117] = index;
        message.extend_from_slice(&voice);
    }
    let sum = message[6..].iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte));
    message.push(sum.wrapping_neg() & 0x7f);
    message.push(0xf7);
    message
}

#[test]
fn a_bank_is_read_from_a_bulk_dump() {
    let message = sysex(0);
    assert_eq!(message.len(), SYSEX_BYTES);
    assert_eq!(SYSEX_BYTES, 4104);
    let bank = Bank::from_sysex(&message).unwrap();
    for index in 0..32 {
        assert_eq!(bank.get(index).unwrap().params().transpose, index as u8);
        assert_eq!(&bank.get(index).unwrap().params().name, b"HAND BUILT");
    }
    assert!(bank.get(32).is_none());
    // Any channel in the third byte.
    assert_eq!(Bank::from_sysex(&sysex(1)), Ok(bank));
    assert_eq!(Bank::from_sysex(&sysex(15)), Ok(bank));
}

#[test]
fn a_wrong_bulk_dump_is_refused() {
    let good = sysex(0);
    assert_eq!(Bank::from_sysex(&good[..4103]), Err(BankError::Length));
    assert_eq!(Bank::from_sysex(&[]), Err(BankError::Length));
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(Bank::from_sysex(&longer), Err(BankError::Length));

    for (byte, wrong) in [(0, 0xf7), (1, 0x42), (2, 0x10), (3, 0x00), (4, 0x21), (5, 0x01)] {
        let mut message = good.clone();
        message[byte] = wrong;
        assert_eq!(Bank::from_sysex(&message), Err(BankError::Header), "byte {byte}");
    }

    let mut message = good.clone();
    message[4103] = 0x00;
    assert_eq!(Bank::from_sysex(&message), Err(BankError::Terminator));

    let mut message = good.clone();
    message[4102] ^= 1;
    assert_eq!(Bank::from_sysex(&message), Err(BankError::Checksum));
    let mut message = good.clone();
    message[6 + 118] ^= 1;
    assert_eq!(Bank::from_sysex(&message), Err(BankError::Checksum));

    // A voice out of range, with the checksum made right for it.
    let mut message = good;
    message[6 + 3 * 128 + 117] = 49;
    let sum = message[6..4102].iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte));
    message[4102] = sum.wrapping_neg() & 0x7f;
    assert_eq!(
        Bank::from_sysex(&message),
        Err(BankError::Patch {
            index: 3,
            error: PatchError::OutOfRange { field: Field::Transpose, operator: None, value: 49, max: 48 }
        })
    );
}

#[test]
fn a_bank_holds_what_is_set() {
    let mut bank = Bank::new();
    assert!((0..40).all(|index| bank.get(index).is_none()));
    let patch = Patch::from_packed(&hand_built()).unwrap();
    bank.set(31, patch);
    bank.set(32, patch);
    assert_eq!(bank.get(31), Some(&patch));
    assert!(bank.get(30).is_none());
    assert!(bank.get(32).is_none());
}

/// A small generator of random numbers for the fuzz below: xorshift.
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Random blocks of 128 bytes never panic; those accepted play a note
/// through its release without a panic (overflow checks are on in this
/// build). Half the blocks are any bytes at all, which are nearly always
/// refused; half have each byte within its field's width, of which many
/// are accepted.
#[test]
fn random_voices_never_panic() {
    let mut random = Random(0x0dd7_5eed_1234_5678);
    let mut accepted = 0;
    let mut refused = 0;
    for round in 0..4000 {
        let mut packed = [0_u8; PACKED_VOICE_BYTES];
        for byte in &mut packed {
            *byte = random.next() as u8;
        }
        if round % 2 == 1 {
            for (index, byte) in packed.iter_mut().enumerate() {
                let limit = match index {
                    0..102 if matches!(index % 17, 11..=13 | 15) => 128,
                    110 | 111 | 116 | 118.. => 128,
                    117 => 49,
                    _ => 100,
                };
                *byte %= limit;
            }
        }
        match Patch::from_packed(&packed) {
            Ok(patch) => {
                accepted += 1;
                let mut bank = Bank::new();
                bank.set(0, patch);
                let mut rack = Rack::new();
                rack.set_bank(&bank);
                let key = (random.next() % 128) as u8;
                rack.event(0x90, key, 1 + (random.next() % 127) as u8);
                let mut samples = [0_i16; 960];
                rack.render(&mut samples);
                rack.event(0x80, key, 0);
                rack.render(&mut samples);
            }
            Err(PatchError::OutOfRange { value, max, .. }) => {
                refused += 1;
                assert!(value > max);
            }
        }
    }
    assert!(accepted > 300, "{accepted} accepted");
    assert!(refused > 2000, "{refused} refused");
}
