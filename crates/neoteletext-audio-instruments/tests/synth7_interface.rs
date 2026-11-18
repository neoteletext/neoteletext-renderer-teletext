//! synth7's adopted interface (`neoteletext-common/docs/audio/synth7.md`):
//! the voice parameters, the three system exclusive messages, the function
//! parameters, the controllers, program changes from two banks, the
//! tracker's restatement and the timing of a note. Each test names the
//! requirement it verifies.
#![cfg(feature = "synth7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::midi::block::BlockWriter;
use neoteletext_audio_instruments::midi::{
    Banks, ChannelMessage, Instrument, MOST_BLOCK_BYTES, Message, Outcome, play_block, sample_of_tick, status,
};
use neoteletext_audio_instruments::synth7::functions::{FUNCTION_DEFAULTS, number};
use neoteletext_audio_instruments::synth7::interface::{
    Tracker, accepts, build_bank, check_bank, check_patch, pack_patch, portamento_time,
};
use neoteletext_audio_instruments::synth7::midi::Midi;
use neoteletext_audio_instruments::synth7::patch::{PARAMETER_MAX, VOICE_PARAMETERS};
use neoteletext_audio_instruments::synth7::sysex::{
    DumpError, Parameter, ParameterChange, ParameterError, checksum, parameter_change, read_parameter_change,
    read_voice_dump, voice_dump,
};
use neoteletext_audio_instruments::synth7::{Patch, VoiceParams};

/// A voice whose 155 parameters are each a different value in range, the
/// name "HAND BUILT".
fn distinct() -> Patch {
    let mut values = [0_u8; VOICE_PARAMETERS];
    for (number, value) in values.iter_mut().enumerate() {
        *value = ((number * 7 + 3) % (usize::from(PARAMETER_MAX[number]) + 1)) as u8;
    }
    values[145..155].copy_from_slice(b"HAND BUILT");
    Patch::from_parameters(&values).unwrap()
}

/// The default voice with algorithm 16, whose one carrier is operator 1:
/// a sine at a single carrier's full level.
fn sine(change: impl FnOnce(&mut VoiceParams)) -> Patch {
    let mut params = *Patch::DEFAULT.params();
    params.algorithm = 15;
    change(&mut params);
    Patch::new(params).unwrap()
}

fn after_start(message: &[u8]) -> &[u8] {
    &message[1..]
}

fn channel(kind: u8, channel: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, channel, data1, data2))
}

fn render(midi: &mut Midi, samples: usize) -> Vec<i16> {
    let mut out = vec![0_i16; samples];
    midi.render(&mut out);
    out
}

fn peak(samples: &[i16]) -> i32 {
    samples.iter().map(|&sample| i32::from(sample).abs()).max().unwrap_or(0)
}

/// The frequency of a rendered sine, from its rising zero crossings.
fn hertz(samples: &[i16]) -> f64 {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0 && pair[1] >= 0)
        .map(|(index, pair)| index as f64 + f64::from(pair[0]) / f64::from(pair[0] - pair[1]))
        .collect();
    let periods = (crossings.len() - 1) as f64;
    48_000.0 * periods / (crossings[crossings.len() - 1] - crossings[0])
}

fn cents(measured: f64, expected: f64) -> f64 {
    1200.0 * (measured / expected).log2()
}

fn give(midi: &mut Midi, message: &Message<'_>, banks: &Banks<'_>) -> Outcome {
    midi.midi(message, banks)
}

fn started(banks: &Banks<'_>) -> Midi {
    let mut midi = Midi::new();
    midi.start(banks);
    midi
}

#[test]
fn s7_38_a_single_voice_dump_is_163_bytes_of_the_155_parameters_in_order() {
    let voice = distinct();
    let dump = voice_dump(&voice, 5);
    assert_eq!(dump.len(), 163);
    assert_eq!(dump[..6], [0xF0, 0x43, 0x05, 0x00, 0x01, 0x1B]);
    assert_eq!(dump[6..161], voice.to_parameters());
    assert_eq!(dump[162], 0xF7);
    assert_eq!(read_voice_dump(&dump), Ok((5, voice)));
    // Byte 6 + k is parameter k alone.
    for k in 0..VOICE_PARAMETERS {
        let mut changed = dump;
        let value = u8::from(dump[6 + k] == 0);
        changed[6 + k] = value;
        changed[161] = checksum(&changed[6..161]);
        let (_, read) = read_voice_dump(&changed).unwrap();
        for j in 0..VOICE_PARAMETERS {
            let expected = if j == k { value } else { voice.parameter(j as u16).unwrap() };
            assert_eq!(read.parameter(j as u16), Some(expected), "byte {} parameter {j}", 6 + k);
        }
    }
}

#[test]
fn s7_39_the_checksum_is_the_twos_complement_of_the_sum_in_seven_bits() {
    assert_eq!(checksum(&[]), 0);
    assert_eq!(checksum(&[1]), 127);
    assert_eq!(checksum(&[0x7F, 0x7F]), 2);
    assert_eq!(checksum(&[64, 64]), 0);
    let dump = voice_dump(&distinct(), 0);
    let sum: u32 = dump[6..162].iter().map(|&byte| u32::from(byte)).sum();
    assert_eq!(sum % 128, 0);
}

#[test]
fn s7_45_a_faulty_single_voice_dump_is_refused_whole() {
    let good = voice_dump(&distinct(), 0);
    for at in 1..162 {
        let mut message = good.to_vec();
        message[at] |= 0x80;
        assert!(read_voice_dump(&message).is_err(), "bit 7 at {at}");
    }
    assert_eq!(read_voice_dump(&good[..162]), Err(DumpError::Length));
    let mut longer = good.to_vec();
    longer.insert(100, 0);
    assert_eq!(read_voice_dump(&longer), Err(DumpError::Length));
    let mut message = good;
    message[161] ^= 1;
    assert_eq!(read_voice_dump(&message), Err(DumpError::Checksum));
    let mut message = good;
    message[162] = 0x7E;
    assert_eq!(read_voice_dump(&message), Err(DumpError::Terminator));
    for (at, wrong) in [(0, 0xF1), (1, 0x42), (2, 0x10), (3, 0x09), (4, 0x00), (5, 0x1C)] {
        let mut message = good;
        message[at] = wrong;
        assert_eq!(read_voice_dump(&message), Err(DumpError::Header), "byte {at}");
    }
}

#[test]
fn s7_29_every_parameter_at_its_top_is_taken_and_one_above_refused() {
    let voice = Patch::DEFAULT;
    for number in 0..VOICE_PARAMETERS {
        let top = PARAMETER_MAX[number];
        let mut values = voice.to_parameters();
        values[number] = top;
        assert!(Patch::from_parameters(&values).is_ok(), "parameter {number} at {top}");
        if top < 127 {
            values[number] = top + 1;
            assert!(Patch::from_parameters(&values).is_err(), "parameter {number} at {}", top + 1);
            let mut dump = voice_dump(&voice, 0);
            dump[6 + number] = top + 1;
            dump[161] = checksum(&dump[6..161]);
            assert!(matches!(read_voice_dump(&dump), Err(DumpError::Patch { .. })), "dump, parameter {number}");
            let change = [0xF0, 0x43, 0x10, (number >> 7) as u8, (number & 0x7F) as u8, top + 1, 0xF7];
            assert_eq!(read_parameter_change(&change), Err(ParameterError::OutOfRange), "change {number}");
        }
    }
}

#[test]
fn s7_48_the_two_forms_of_a_voice_agree() {
    let voice = distinct();
    assert_eq!(Patch::from_packed(&voice.to_packed()).unwrap(), voice);
    assert_eq!(Patch::from_parameters(&voice.to_parameters()).unwrap(), voice);
    // Random voices in range round-trip both ways.
    let mut state = 0x1234_5678_u32;
    for _ in 0..500 {
        let mut values = [0_u8; VOICE_PARAMETERS];
        for (number, value) in values.iter_mut().enumerate() {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            *value = ((state >> 16) % (u32::from(PARAMETER_MAX[number]) + 1)) as u8;
        }
        let voice = Patch::from_parameters(&values).unwrap();
        assert_eq!(voice.to_parameters(), values);
        assert_eq!(Patch::from_packed(&voice.to_packed()).unwrap().to_parameters(), values);
    }
}

#[test]
fn s7_51_the_default_voice_is_the_makers_init_voice() {
    let values = Patch::DEFAULT.to_parameters();
    for operator in 0..6 {
        let block = &values[operator * 21..operator * 21 + 21];
        // Operator 6 first: operator 1 is the last block.
        let level = if operator == 5 { 99 } else { 0 };
        assert_eq!(
            block,
            [99, 99, 99, 99, 99, 99, 99, 0, 39, 0, 0, 0, 0, 0, 0, 0, level, 0, 1, 0, 7],
            "block {operator}"
        );
    }
    assert_eq!(values[126..145], [99, 99, 99, 99, 50, 50, 50, 50, 0, 0, 1, 35, 0, 0, 0, 1, 0, 3, 24]);
    assert_eq!(&values[145..155], b"INIT VOICE");
    assert_eq!(Patch::DEFAULT.parameter(155), Some(63));
    // A new part with no bank plays it.
    let midi = started(&Banks::none());
    for part in 0..16 {
        assert_eq!(midi.rack().voice(part), Some(&Patch::DEFAULT));
    }
}

#[test]
fn s7_43_the_parameter_change_carries_every_parameter_and_no_other() {
    for number in 0..=155_u16 {
        let change = ParameterChange { channel: 3, parameter: Parameter::Voice(number), value: 0 };
        let bytes = parameter_change(change).unwrap();
        assert_eq!(bytes[..3], [0xF0, 0x43, 0x13]);
        assert_eq!(bytes[3], (number >> 7) as u8);
        assert_eq!(bytes[4], (number & 0x7F) as u8);
        assert_eq!(read_parameter_change(&bytes), Ok(change));
    }
    for number in 64..=77_u16 {
        let change = ParameterChange { channel: 15, parameter: Parameter::Function(number), value: 1 };
        let bytes = parameter_change(change).unwrap();
        assert_eq!(bytes, [0xF0, 0x43, 0x1F, 0x08, number as u8, 1, 0xF7]);
        assert_eq!(read_parameter_change(&bytes), Ok(change));
    }
    let refused = [
        ([0xF0, 0x43, 0x10, 0x04, 0x00, 0x00, 0xF7], ParameterError::Group),
        ([0xF0, 0x43, 0x10, 0x0C, 0x00, 0x00, 0xF7], ParameterError::Group),
        ([0xF0, 0x43, 0x10, 0x01, 0x1C, 0x00, 0xF7], ParameterError::Number),
        ([0xF0, 0x43, 0x10, 0x08, 63, 0x00, 0xF7], ParameterError::Number),
        ([0xF0, 0x43, 0x10, 0x08, 78, 0x00, 0xF7], ParameterError::Number),
        ([0xF0, 0x43, 0x10, 0x00, 0x00, 0x00, 0xF6], ParameterError::Terminator),
        ([0xF0, 0x43, 0x10, 0x00, 0x00, 0x80, 0xF7], ParameterError::DataByte),
    ];
    for (bytes, error) in refused {
        assert_eq!(read_parameter_change(&bytes), Err(error), "{bytes:02X?}");
        assert!(!accepts(&Message::SystemExclusive(after_start(&bytes))));
    }
    assert!(parameter_change(ParameterChange { channel: 0, parameter: Parameter::Voice(156), value: 0 }).is_none());
}

#[test]
fn s7_44_a_parameter_change_sets_that_parameter_of_that_part_alone() {
    let banks = Banks::none();
    let mut midi = started(&banks);
    for number in 0..=155_u16 {
        let top = PARAMETER_MAX[usize::from(number)];
        let change = ParameterChange { channel: 2, parameter: Parameter::Voice(number), value: top };
        let bytes = parameter_change(change).unwrap();
        let before = *midi.rack().voice(2).unwrap();
        assert_eq!(give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks), Outcome::Applied);
        let after = midi.rack().voice(2).unwrap();
        for other in 0..=155_u16 {
            let expected = if other == number { top } else { before.parameter(other).unwrap() };
            assert_eq!(after.parameter(other), Some(expected), "change of {number}, parameter {other}");
        }
        assert_eq!(midi.rack().voice(3), Some(&Patch::DEFAULT));
    }
    for (number, &top) in (64..=77_u16).zip(&neoteletext_audio_instruments::synth7::functions::FUNCTION_MAX) {
        let change = ParameterChange { channel: 3, parameter: Parameter::Function(number), value: top };
        let bytes = parameter_change(change).unwrap();
        assert_eq!(give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks), Outcome::Applied);
        assert_eq!(midi.rack().functions(3).unwrap().get(number), Some(top));
        assert_eq!(midi.rack().functions(4).unwrap().get(number), Some(FUNCTION_DEFAULTS[usize::from(number - 64)]));
    }
}

#[test]
fn s7_44_an_output_level_change_reaches_a_held_note() {
    let banks = Banks::none();
    let mut midi = started(&banks);
    give(&mut midi, &channel(status::NOTE_ON, 0, 69, 100), &banks);
    let loud = peak(&render(&mut midi, 4800)[2400..]);
    // Operator 1's output level is parameter 121.
    let bytes = parameter_change(ParameterChange { channel: 0, parameter: Parameter::Voice(121), value: 60 }).unwrap();
    give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    let quiet = peak(&render(&mut midi, 4800)[2400..]);
    assert!(quiet * 4 < loud, "{loud} then {quiet}");
}

#[test]
fn s7_30_the_operator_switch_by_parameter_change_and_a_program_change_after() {
    let banks = Banks::none();
    let switch = |data: u8| {
        let mut midi = started(&banks);
        let bytes =
            parameter_change(ParameterChange { channel: 0, parameter: Parameter::Voice(155), value: data }).unwrap();
        assert_eq!(give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks), Outcome::Applied);
        give(&mut midi, &channel(status::NOTE_ON, 0, 69, 100), &banks);
        (peak(&render(&mut midi, 4800)), midi)
    };
    // The default voice sounds operator 1 alone.
    assert!(switch(32).0 > 1000, "operator 1 alone");
    let (silent, mut midi) = switch(1);
    assert_eq!(silent, 0, "operator 6 alone");
    give(&mut midi, &channel(status::NOTE_OFF, 0, 69, 0), &banks);
    give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 0, 0), &banks);
    assert_eq!(midi.rack().voice(0).unwrap().parameter(155), Some(63));
    give(&mut midi, &channel(status::NOTE_ON, 0, 72, 100), &banks);
    assert!(peak(&render(&mut midi, 4800)) > 1000, "a program change sounds all six");
}

#[test]
fn s7_40_a_single_voice_dump_replaces_its_parts_voice_alone() {
    let bank = build_bank(&[sine(|_| ())]).unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    let voice = distinct();
    let dump = pack_patch(&voice, 2);
    assert_eq!(give(&mut midi, &Message::SystemExclusive(after_start(&dump)), &banks), Outcome::Applied);
    assert_eq!(midi.rack().voice(2), Some(&voice));
    assert_eq!(midi.rack().voice(3), Some(&sine(|_| ())));
    give(&mut midi, &channel(status::PROGRAM_CHANGE, 2, 0, 0), &banks);
    assert_eq!(midi.rack().voice(2), Some(&sine(|_| ())));
    // A dump that breaks its rules sets the default voice and is counted
    // (MIDI-63).
    let mut bad = dump;
    bad[161] ^= 1;
    assert_eq!(give(&mut midi, &Message::SystemExclusive(after_start(&bad)), &banks), Outcome::PatchRefused);
    assert_eq!(midi.rack().voice(2), Some(&Patch::DEFAULT));
    assert_eq!(check_patch(&dump), Ok(voice));
}

#[test]
fn s7_50_programs_32_to_63_choose_from_the_second_bank() {
    let first: Vec<Patch> = (0..32).map(|index| sine(|voice| voice.transpose = index)).collect();
    let second: Vec<Patch> = (0..32).map(|index| sine(|voice| voice.lfo_speed = index)).collect();
    let (one, two) = (build_bank(&first).unwrap(), build_bank(&second).unwrap());
    let images = [one.as_slice(), two.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    for (program, expected) in
        [(0, first[0]), (17, first[17]), (31, first[31]), (32, second[0]), (45, second[13]), (63, second[31])]
    {
        assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 4, program, 0), &banks), Outcome::Applied);
        assert_eq!(midi.rack().voice(4), Some(&expected), "program {program}");
    }
    for program in [64, 100, 127] {
        assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 4, program, 0), &banks), Outcome::PassedOver);
        assert_eq!(midi.rack().voice(4), Some(&Patch::DEFAULT));
    }
    // A sound naming one bank: the cartridge programs are the default.
    let images = [one.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 40, 0), &banks), Outcome::PassedOver);
    assert_eq!(midi.rack().voice(0), Some(&Patch::DEFAULT));
}

#[test]
fn s7_42_a_faulty_bank_costs_its_faulty_voices_or_all_of_them() {
    let voices: Vec<Patch> = (0..32).map(|index| sine(|voice| voice.transpose = index)).collect();
    let good = build_bank(&voices).unwrap();
    assert_eq!(check_bank(&good), Ok(()));
    // Voice 7 out of range, the checksum made right for it.
    let mut one_bad = good;
    one_bad[6 + 7 * 128 + 117] = 49;
    one_bad[4102] = checksum(&one_bad[6..4102]);
    assert!(check_bank(&one_bad).is_err());
    let images = [one_bad.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 7, 0), &banks), Outcome::PatchRefused);
    assert_eq!(midi.rack().voice(0), Some(&Patch::DEFAULT));
    assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 8, 0), &banks), Outcome::Applied);
    assert_eq!(midi.rack().voice(0), Some(&voices[8]));
    // A wrong checksum: every program the default, from the start.
    let mut wrong = good;
    wrong[4102] ^= 1;
    let images = [wrong.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = Midi::new();
    assert_eq!(midi.start(&banks), Outcome::PatchRefused);
    assert_eq!(midi.rack().voice(0), Some(&Patch::DEFAULT));
    assert_eq!(give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 9, 0), &banks), Outcome::PatchRefused);
    // DEC-6465 answer 18: fewer than 32 voices are filled with the default.
    let short = build_bank(&voices[..3]).unwrap();
    let images = [short.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 20, 0), &banks);
    assert_eq!(midi.rack().voice(0), Some(&Patch::DEFAULT));
    assert!(build_bank(&[Patch::DEFAULT; 33]).is_err());
    assert!(build_bank(&[]).is_err());
    // A 32-voice dump in a block is not taken (S7-37).
    assert!(!accepts(&Message::SystemExclusive(after_start(&good))));
}

/// The pitch of middle A at full bend up, in cents above 440 Hz.
fn full_bend_cents(midi: &mut Midi, banks: &Banks<'_>) -> f64 {
    give(midi, &channel(status::PITCH_BEND, 0, 0x7F, 0x7F), banks);
    give(midi, &channel(status::NOTE_ON, 0, 69, 100), banks);
    let samples = render(midi, 24_000);
    give(midi, &channel(status::NOTE_OFF, 0, 69, 0), banks);
    render(midi, 24_000);
    cents(hertz(&samples[4800..]), 440.0)
}

#[test]
fn s7_9_the_bend_range_by_function_parameter_65_and_by_rpn_0() {
    let bank = build_bank(&[sine(|_| ())]).unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    let mut midi = started(&banks);
    // 16,383 is 8,191/8,192 of the range.
    let near = |cents: f64, semitones: f64| (cents - semitones * 100.0 * 8191.0 / 8192.0).abs() < 2.0;
    assert!(near(full_bend_cents(&mut midi, &banks), 2.0), "the default range of 2");
    let range = |value: u8| {
        parameter_change(ParameterChange {
            channel: 0,
            parameter: Parameter::Function(number::PITCH_BEND_RANGE),
            value,
        })
        .unwrap()
    };
    let seven = range(7);
    give(&mut midi, &Message::SystemExclusive(after_start(&seven)), &banks);
    assert!(near(full_bend_cents(&mut midi, &banks), 7.0));
    // RPN 0: 101 and 100 at 0, then 6 in semitones; 24 is held to 12.
    for message in [
        channel(status::CONTROL_CHANGE, 0, 101, 0),
        channel(status::CONTROL_CHANGE, 0, 100, 0),
        channel(status::CONTROL_CHANGE, 0, 6, 24),
    ] {
        assert_eq!(give(&mut midi, &message, &banks), Outcome::Applied);
    }
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 12);
    assert!(near(full_bend_cents(&mut midi, &banks), 12.0));
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 6, 1), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 1);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 96, 127), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 2);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 38, 50), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 2);
    // The later message holds: function parameter 65 after RPN 0.
    give(&mut midi, &Message::SystemExclusive(after_start(&seven)), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 7);
    // Neither a program change nor controller 121 changes it; 121 clears
    // the selection, so 6 then changes nothing.
    give(&mut midi, &channel(status::PROGRAM_CHANGE, 0, 0, 0), &banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 121, 0), &banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 6, 3), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 7);
    // A range of 0 bends nothing.
    let zero = range(0);
    give(&mut midi, &Message::SystemExclusive(after_start(&zero)), &banks);
    assert!(full_bend_cents(&mut midi, &banks).abs() < 2.0);
    // A step of 1 at range 12: only whole semitones sound.
    let mut midi = started(&banks);
    for (parameter, value) in [(number::PITCH_BEND_RANGE, 12), (number::PITCH_BEND_STEP, 1)] {
        let bytes =
            parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(parameter), value }).unwrap();
        give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    }
    for bend in [0x2000_u16 + 1000, 0x2000 + 3000, 0x2000 - 2500] {
        give(&mut midi, &channel(status::PITCH_BEND, 0, (bend & 0x7F) as u8, (bend >> 7) as u8), &banks);
        give(&mut midi, &channel(status::NOTE_ON, 0, 69, 100), &banks);
        let measured = cents(hertz(&render(&mut midi, 24_000)[4800..]), 440.0);
        give(&mut midi, &channel(status::NOTE_OFF, 0, 69, 0), &banks);
        render(&mut midi, 24_000);
        let semitones = (i32::from(bend) - 0x2000) * 12 / 0x2000;
        assert!((measured - f64::from(semitones) * 100.0).abs() < 2.0, "bend {bend}: {measured} cents");
    }
}

#[test]
fn s7_14_controller_5_sets_portamento_time() {
    assert_eq!([portamento_time(0), portamento_time(64), portamento_time(127)], [0, 50, 99]);
    let banks = Banks::none();
    let mut midi = started(&banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 1, 5, 64), &banks);
    assert_eq!(midi.rack().functions(1).unwrap().portamento_time(), 50);
    let bytes =
        parameter_change(ParameterChange { channel: 1, parameter: Parameter::Function(69), value: 10 }).unwrap();
    give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    assert_eq!(midi.rack().functions(1).unwrap().portamento_time(), 10);
}

/// The pitch, in cents above 440 Hz, of the hundredth of a second after
/// `delay` samples of a second key a fifth above 440 Hz, struck after a
/// first key at 440 Hz.
fn second_key_cents(midi: &mut Midi, banks: &Banks<'_>, delay: usize, legato: bool) -> f64 {
    give(midi, &channel(status::NOTE_ON, 0, 69, 100), banks);
    render(midi, 4800);
    if !legato {
        give(midi, &channel(status::NOTE_OFF, 0, 69, 0), banks);
    }
    give(midi, &channel(status::NOTE_ON, 0, 76, 100), banks);
    let samples = render(midi, delay + 4800);
    // The newest note alone: the first key's note has been released, or in
    // mono mode taken over.
    cents(hertz(&samples[delay..]), 440.0)
}

#[test]
fn s7_17_and_s7_27_portamento_glides_while_its_switch_is_on() {
    let bank = build_bank(&[sine(|voice| voice.operators[0].eg_rates[3] = 99)]).unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    let mono_with_time = |time: u8| {
        let mut midi = started(&banks);
        for (parameter, value) in [(number::MONO_POLY, 1), (number::PORTAMENTO_TIME, time)] {
            let bytes =
                parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(parameter), value })
                    .unwrap();
            give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
        }
        midi
    };
    // No glide at time 0; at time 50 the second key starts near the first.
    let none = second_key_cents(&mut mono_with_time(0), &banks, 0, true);
    assert!((none - 700.0).abs() < 5.0, "{none}");
    let gliding = second_key_cents(&mut mono_with_time(50), &banks, 0, true);
    assert!(gliding < 650.0, "{gliding}");
    // The switch starts on; controller 65 at 0 stops the glide, 127 starts it.
    let mut midi = mono_with_time(50);
    assert_eq!(midi.rack().portamento_switch(0), Some(true));
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 65, 0), &banks);
    let off = second_key_cents(&mut midi, &banks, 0, true);
    assert!((off - 700.0).abs() < 5.0, "{off}");
    // Controller 121 puts it off.
    let mut midi = mono_with_time(50);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 121, 0), &banks);
    assert_eq!(midi.rack().portamento_switch(0), Some(false));
    // A longer time glides longer.
    let short = second_key_cents(&mut mono_with_time(25), &banks, 4800, true);
    let long = second_key_cents(&mut mono_with_time(75), &banks, 4800, true);
    assert!(long < short, "{short} at time 25, {long} at time 75");
}

#[test]
fn s7_26_mono_mode_moves_one_note_between_held_keys() {
    let banks = Banks::none();
    let mut midi = started(&banks);
    let bytes = parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(64), value: 1 }).unwrap();
    give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    give(&mut midi, &channel(status::NOTE_ON, 0, 60, 100), &banks);
    render(&mut midi, 960);
    give(&mut midi, &channel(status::NOTE_ON, 0, 64, 100), &banks);
    render(&mut midi, 960);
    assert_eq!(midi.rack().sounding_notes(), 1);
    assert!(midi.rack().is_sounding(0, 64));
    give(&mut midi, &channel(status::NOTE_OFF, 0, 64, 0), &banks);
    assert!(midi.rack().is_sounding(0, 60), "the note returns to the key still held");
    give(&mut midi, &channel(status::NOTE_OFF, 0, 60, 0), &banks);
    render(&mut midi, 48_000);
    assert_eq!(midi.rack().sounding_notes(), 0);
    // Seventeen keys held: the first is forgotten.
    for key in 40..57 {
        give(&mut midi, &channel(status::NOTE_ON, 0, key, 100), &banks);
    }
    for key in (41..57).rev() {
        give(&mut midi, &channel(status::NOTE_OFF, 0, key, 0), &banks);
    }
    render(&mut midi, 48_000);
    assert_eq!(midi.rack().sounding_notes(), 0, "key 40 was forgotten");
}

#[test]
fn s7_20_reset_all_controllers_follows_rp_015_and_synth7s_list() {
    let banks = Banks::none();
    let mut midi = started(&banks);
    let messages = [
        channel(status::CONTROL_CHANGE, 0, 1, 90),
        channel(status::CONTROL_CHANGE, 0, 2, 80),
        channel(status::CONTROL_CHANGE, 0, 4, 70),
        channel(status::CHANNEL_PRESSURE, 0, 60, 0),
        channel(status::PITCH_BEND, 0, 0, 0x50),
        channel(status::CONTROL_CHANGE, 0, 64, 127),
        channel(status::CONTROL_CHANGE, 0, 65, 127),
        channel(status::CONTROL_CHANGE, 0, 7, 50),
        channel(status::CONTROL_CHANGE, 0, 5, 127),
    ];
    for message in messages {
        assert_eq!(give(&mut midi, &message, &banks), Outcome::Applied);
    }
    let bytes = parameter_change(ParameterChange { channel: 0, parameter: Parameter::Voice(134), value: 4 }).unwrap();
    give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    let voice = *midi.rack().voice(0).unwrap();
    let functions = *midi.rack().functions(0).unwrap();
    for value in [0, 127] {
        give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 121, value), &banks);
        assert_eq!(midi.rack().performance(0), Some([0, 0, 0, 0]));
        assert_eq!(midi.rack().bend_and_sustain(0), Some((0x2000, false)));
        assert_eq!(midi.rack().portamento_switch(0), Some(false));
        assert_eq!(midi.rack().voice(0), Some(&voice));
        assert_eq!(midi.rack().functions(0), Some(&functions), "portamento time and range stay");
        assert_eq!(midi.common().channel(0).volume, 50);
    }
}

#[test]
fn s7_19_the_mode_messages_are_all_notes_off_and_nothing_more() {
    let banks = Banks::none();
    for controller in [124, 125, 126, 127] {
        let mut midi = started(&banks);
        for key in [60, 64, 67] {
            give(&mut midi, &channel(status::NOTE_ON, 0, key, 100), &banks);
        }
        assert_eq!(give(&mut midi, &channel(status::CONTROL_CHANGE, 0, controller, 1), &banks), Outcome::Applied);
        render(&mut midi, 48_000);
        assert_eq!(midi.rack().sounding_notes(), 0, "controller {controller}");
        assert!(!midi.rack().functions(0).unwrap().mono());
    }
}

/// S7-36: the four controllers act through their ranges and assigns.
#[test]
fn s7_36_the_controllers_act_through_range_and_assign() {
    let bank = build_bank(&[sine(|voice| {
        voice.pitch_mod_sensitivity = 7;
        voice.lfo_speed = 60;
        voice.operators[0].amp_mod_sensitivity = 3;
    })])
    .unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    // The spread of the loudest and quietest hundredths of a second, in
    // doublings, with a controller set.
    let swing = |setup: &[(u16, u8)], message: Message<'static>| {
        let mut midi = started(&banks);
        for &(parameter, value) in setup {
            let bytes =
                parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(parameter), value })
                    .unwrap();
            give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
        }
        give(&mut midi, &message, &banks);
        give(&mut midi, &channel(status::NOTE_ON, 0, 81, 100), &banks);
        let samples = render(&mut midi, 96_000);
        let peaks: Vec<i32> = samples[4800..].chunks(480).map(peak).collect();
        let (loud, quiet) = (*peaks.iter().max().unwrap(), *peaks.iter().min().unwrap());
        (f64::from(loud) / f64::from(quiet.max(1))).log2()
    };
    let breath = channel(status::CONTROL_CHANGE, 0, 2, 127);
    // Breath at range 0 does nothing; at range 99 on amplitude it swings.
    assert!(swing(&[], breath) < 0.05);
    let amplitude = [(number::BREATH_CONTROLLER_RANGE, 99), (number::BREATH_CONTROLLER_ASSIGN, 2)];
    assert!(swing(&amplitude, breath) > 4.0);
    // After touch on EG bias at its bottom attenuates most; at the top not.
    let bias = [(number::AFTER_TOUCH_RANGE, 99), (number::AFTER_TOUCH_ASSIGN, 4)];
    let level = |pressure: u8| {
        let mut midi = started(&banks);
        for &(parameter, value) in &bias {
            let bytes =
                parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(parameter), value })
                    .unwrap();
            give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
        }
        give(&mut midi, &channel(status::CHANNEL_PRESSURE, 0, pressure, 0), &banks);
        give(&mut midi, &channel(status::NOTE_ON, 0, 81, 100), &banks);
        peak(&render(&mut midi, 9600)[4800..])
    };
    let (bottom, top) = (level(0), level(127));
    assert!(top > 3000, "{top}");
    assert!(bottom * 100 < top, "EG bias at the bottom: {bottom} against {top}");
}

#[test]
fn s7_5_accepts_exactly_the_messages_synth7_acts_on() {
    for kind in [0x80_u8, 0x90, 0xC0, 0xD0, 0xE0] {
        assert!(accepts(&channel(kind, 3, 1, 0)), "{kind:#x}");
    }
    assert!(!accepts(&channel(0xA0, 3, 60, 10)));
    let taken = [1_u8, 2, 4, 5, 6, 7, 38, 64, 65, 96, 97, 100, 101, 120, 121, 123, 124, 125, 126, 127];
    for controller in 0..=127_u8 {
        assert_eq!(accepts(&channel(0xB0, 0, controller, 0)), taken.contains(&controller), "controller {controller}");
    }
    let dump = voice_dump(&distinct(), 0);
    assert!(accepts(&Message::SystemExclusive(after_start(&dump))));
    assert!(!accepts(&Message::SystemExclusive(&[0x41, 0x10, 0xF7])));
}

/// Plays the restatement of every part of `tracker` into a fresh synth7.
fn restated(tracker: &Tracker, banks: &Banks<'_>) -> (Midi, usize) {
    let mut midi = started(banks);
    let mut bytes = 0;
    for part in 0..16 {
        let mut buffer = [0_u8; MOST_BLOCK_BYTES];
        let mut writer = BlockWriter::new(&mut buffer);
        tracker.restate(part, &mut |message| writer.push(0, &message).unwrap());
        bytes = bytes.max(writer.bytes().len());
        let block = writer.bytes().to_vec();
        play_block(&mut midi, &block, banks, &mut []);
    }
    (midi, bytes)
}

#[test]
fn s7_49_a_restatement_brings_a_fresh_synth7_to_the_state() {
    let first: Vec<Patch> = (0..4).map(|index| sine(|voice| voice.transpose = 20 + index)).collect();
    let bank = build_bank(&first).unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    let mut tracker = Tracker::new(&banks);
    let mut heard = started(&banks);
    let dump = pack_patch(&distinct(), 1);
    let function = |parameter, value, channel| {
        parameter_change(ParameterChange { channel, parameter: Parameter::Function(parameter), value }).unwrap()
    };
    let voice = |parameter, value, channel| {
        parameter_change(ParameterChange { channel, parameter: Parameter::Voice(parameter), value }).unwrap()
    };
    let sysex: Vec<Vec<u8>> = vec![
        dump.to_vec(),
        voice(3, 11, 1).to_vec(),
        voice(155, 0b10_1010, 1).to_vec(),
        function(number::MONO_POLY, 1, 2).to_vec(),
        function(number::PITCH_BEND_RANGE, 9, 2).to_vec(),
        function(number::AFTER_TOUCH_ASSIGN, 5, 2).to_vec(),
        voice(16, 50, 3).to_vec(),
    ];
    let mut messages: Vec<Message<'_>> = vec![
        channel(status::PROGRAM_CHANGE, 0, 2, 0),
        channel(status::PROGRAM_CHANGE, 3, 1, 0),
        channel(status::CONTROL_CHANGE, 2, 1, 33),
        channel(status::CONTROL_CHANGE, 2, 2, 44),
        channel(status::CONTROL_CHANGE, 2, 4, 55),
        channel(status::CONTROL_CHANGE, 2, 7, 66),
        channel(status::CONTROL_CHANGE, 2, 64, 127),
        channel(status::CONTROL_CHANGE, 2, 65, 0),
        channel(status::CONTROL_CHANGE, 2, 5, 100),
        channel(status::CHANNEL_PRESSURE, 2, 77, 0),
        channel(status::PITCH_BEND, 2, 5, 90),
    ];
    messages.extend(sysex.iter().map(|bytes| Message::SystemExclusive(after_start(bytes))));
    for message in &messages {
        tracker.take(message, &banks);
        give(&mut heard, message, &banks);
    }
    let (joined, longest) = restated(&tracker, &banks);
    assert!(longest <= 332, "a part's restatement is {longest} bytes");
    for part in 0..16 {
        assert_eq!(joined.rack().voice(part), heard.rack().voice(part), "voice of part {part}");
        // A part whose voice was edited is restated by a dump in place of
        // its program change (S7-49 item 4 (a)), which carries no program
        // number: parts 1 and 3.
        if part != 1 && part != 3 {
            assert_eq!(joined.rack().program(part), heard.rack().program(part), "program of part {part}");
        }
        assert_eq!(joined.rack().functions(part), heard.rack().functions(part), "functions of part {part}");
        assert_eq!(joined.rack().performance(part), heard.rack().performance(part), "performance of part {part}");
        assert_eq!(joined.rack().bend_and_sustain(part), heard.rack().bend_and_sustain(part), "bend of part {part}");
        assert_eq!(joined.rack().portamento_switch(part), heard.rack().portamento_switch(part), "65 of part {part}");
        assert_eq!(joined.common().channel(part).volume, heard.common().channel(part).volume, "7 of part {part}");
    }
    // The order of S7-49 item 4 on part 2: the program change, the three
    // function parameters in ascending order, then bend, pressure and the
    // controllers.
    let mut order = Vec::new();
    tracker.restate(2, &mut |message| {
        order.push(match message {
            Message::Channel(message) => (message.status, message.data1),
            Message::SystemExclusive(bytes) => (0xF0, bytes[3]),
        });
    });
    assert_eq!(
        order,
        [
            (0xC2, 0),
            (0xF0, 64),
            (0xF0, 65),
            (0xF0, 69),
            (0xF0, 77),
            (0xE2, 5),
            (0xD2, 77),
            (0xB2, 1),
            (0xB2, 2),
            (0xB2, 4),
            (0xB2, 7),
            (0xB2, 64),
            (0xB2, 65)
        ]
    );
}

/// MIDI-97: a restatement played into a synth7 that holds its state
/// changes no sample.
#[test]
fn s7_49_a_restatement_is_inaudible_to_a_synth7_that_has_the_state() {
    let bank = build_bank(&[sine(|voice| voice.lfo_pitch_mod_depth = 40), sine(|voice| voice.transpose = 30)]).unwrap();
    let images = [bank.as_slice()];
    let banks = Banks::new(&images);
    let mut tracker = Tracker::new(&banks);
    let mut with = started(&banks);
    let edit = parameter_change(ParameterChange { channel: 0, parameter: Parameter::Voice(16), value: 80 }).unwrap();
    let setup = [
        channel(status::PROGRAM_CHANGE, 0, 1, 0),
        Message::SystemExclusive(after_start(&edit)),
        channel(status::CONTROL_CHANGE, 0, 1, 50),
        channel(status::NOTE_ON, 0, 64, 100),
    ];
    for message in &setup {
        tracker.take(message, &banks);
        give(&mut with, message, &banks);
    }
    let mut without = with.clone();
    render(&mut with, 1000);
    render(&mut without, 1000);
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    tracker.restate(0, &mut |message| writer.push(0, &message).unwrap());
    let block = writer.bytes().to_vec();
    let mut a = vec![0_i16; 960];
    let mut b = vec![0_i16; 960];
    play_block(&mut with, &block, &banks, &mut a);
    play_block(&mut without, &[], &banks, &mut b);
    assert_eq!(a, b);
    a.resize(48_000, 0);
    b.resize(48_000, 0);
    with.render(&mut a);
    without.render(&mut b);
    assert_eq!(a, b);
}

#[test]
fn s7_52_a_note_starts_at_its_own_sample() {
    let banks = Banks::none();
    for tick in [0_u8, 1, 3, 7, 50, 101, 199] {
        let mut midi = started(&banks);
        let mut buffer = [0_u8; MOST_BLOCK_BYTES];
        let mut writer = BlockWriter::new(&mut buffer);
        writer.push(tick, &channel(status::NOTE_ON, 0, 69, 127)).unwrap();
        let block = writer.bytes().to_vec();
        let mut first = vec![0_i16; 960];
        play_block(&mut midi, &[], &banks, &mut first);
        let mut out = vec![0_i16; 960];
        play_block(&mut midi, &block, &banks, &mut out);
        let at = sample_of_tick(tick);
        assert!(out[..at].iter().all(|&sample| sample == 0), "tick {tick}: sound before sample {at}");
        assert!(out[at..(at + 32).min(960)].iter().any(|&sample| sample != 0), "tick {tick}: silent after {at}");
    }
}

/// S7-53: a lost block releases every note and clears the RPN selection,
/// and leaves the function parameters.
#[test]
fn s7_53_a_lost_block_releases_notes_and_clears_the_selection() {
    let banks = Banks::none();
    let mut midi = started(&banks);
    let bytes = parameter_change(ParameterChange { channel: 0, parameter: Parameter::Function(65), value: 5 }).unwrap();
    give(&mut midi, &Message::SystemExclusive(after_start(&bytes)), &banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 101, 0), &banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 100, 0), &banks);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 64, 127), &banks);
    give(&mut midi, &channel(status::NOTE_ON, 0, 60, 100), &banks);
    midi.lost();
    render(&mut midi, 48_000);
    assert_eq!(midi.rack().sounding_notes(), 0);
    give(&mut midi, &channel(status::CONTROL_CHANGE, 0, 6, 9), &banks);
    assert_eq!(midi.rack().functions(0).unwrap().bend_range(), 5);
}
