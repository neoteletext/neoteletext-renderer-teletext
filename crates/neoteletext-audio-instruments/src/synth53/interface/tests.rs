#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

use std::vec::Vec;

use super::*;

/// A 64-bit linear congruential generator with Knuth's MMIX constants.
struct Random(u64);

impl Random {
    fn byte(&mut self) -> u8 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 56) as u8
    }
}

fn cc(number: u8, value: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, 0, number, value))
}

fn program_change(number: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 0, number, 0))
}

/// A program whose every parameter is distinct to `seed`.
fn distinct(seed: u16) -> Program {
    let mut program = Program::default_for(Variant::Synth53);
    let top = u16::from(KNOB_TOP) + 1;
    program.set(parameter::CUTOFF, (seed % top) as u8);
    program.set(parameter::OSC_A_FREQUENCY, (seed / top) as u8);
    program
}

/// A bank image of `count` programs of `group`, program k [`distinct`] to
/// `group × 40 + k`.
fn bank(group: u8, count: u8) -> Vec<u8> {
    let programs: Vec<Program> = (0..count).map(|number| distinct(u16::from(group) * 40 + u16::from(number))).collect();
    let mut image = std::vec![0; programs.len() * PROGRAM_DUMP_BYTES];
    assert_eq!(build_bank(group, &programs, &mut image), Ok(image.len()));
    image
}

// Verifies S53-27 and S53-21: the 67 parameters with their maxima, and
// the 57 controllers with a parameter number.
#[test]
fn s53_27_the_table_has_67_parameters_and_57_controllers() {
    let numbers: Vec<u16> = (0..=u16::from(u8::MAX)).filter(|&number| maximum(number).is_some()).collect();
    let expected: Vec<u16> = (0..=64).chain([86, 87]).collect();
    assert_eq!(numbers, expected);
    for number in (65..=85).chain(88..=94).chain(4096..=4109) {
        assert_eq!(maximum(number), None);
    }
    assert_eq!(maximum(19), Some(2));
    assert_eq!(maximum(53), Some(10));
    assert_eq!(maximum(54), Some(7));
    assert_eq!(maximum(86), Some(11));
    assert_eq!(maximum(87), Some(3));
    let controllers: Vec<u8> = (0..=DATA_MAX).filter(|&number| parameter_of_controller(number).is_some()).collect();
    assert_eq!(controllers.len(), 57);
    assert_eq!(parameter_of_controller(7), None);
    assert_eq!(parameter_of_controller(BRIGHTNESS), None);
    assert_eq!(parameter_of_controller(35), Some(parameter::FILTER_KEYBOARD_TRACK));
    assert_eq!(parameter_of_controller(70), Some(parameter::PITCH_WHEEL_RANGE));
}

// Verifies S53-32: the three worked examples pack as the family document
// gives them, and unpack back (S53-31).
#[test]
fn s53_32_the_worked_examples_pack_and_unpack() {
    // Example 1: the document's description, with every top bit unlike
    // its neighbour's.
    let mut program = [0; PROGRAM_BYTES];
    program[..7].copy_from_slice(&[0x80, 0x01, 0xFF, 0x00, 0x7F, 0x81, 0x40]);
    let packed = pack(&program);
    assert_eq!(packed[..8], [0x25, 0x00, 0x01, 0x7F, 0x00, 0x7F, 0x01, 0x40]);
    assert_eq!(unpack(&packed)[..PROGRAM_BYTES], program);
    // Example 2: the first packet of the default program on synth53.
    let default = pack(Program::default_for(Variant::Synth53).bytes());
    assert_eq!(default[..8], [0x00, 0x40, 0x40, 0x00, 0x01, 0x00, 0x00, 0x00]);
    // Example 3: the last packet of any program.
    assert_eq!(default[PACKED_BYTES - 8..], [0; 8]);
    // Every first byte of a packet of a valid program is 0.
    assert!(default.iter().step_by(PACKET_BYTES).all(|&byte| byte == 0));
}

// Verifies S53-31: a thousand random programs of eight-bit bytes pack and
// unpack to themselves, the five positions after byte 127 written 0.
#[test]
fn s53_31_random_programs_pack_and_unpack_to_themselves() {
    let mut random = Random(53);
    for _ in 0..1_000 {
        let program: [u8; PROGRAM_BYTES] = core::array::from_fn(|_| random.byte());
        let packed = pack(&program);
        assert!(packed.iter().all(|&byte| byte <= DATA_MAX));
        let data = unpack(&packed);
        assert_eq!(data[..PROGRAM_BYTES], program);
        assert_eq!(data[PROGRAM_BYTES..], [0; UNPACKED_BYTES - PROGRAM_BYTES]);
    }
    // A reader ignores what a sender wrote in the five positions.
    let mut packed = pack(&[0; PROGRAM_BYTES]);
    packed[PACKED_BYTES - 1] = 0x55;
    assert_eq!(unpack_program(&packed), Program::from_bytes([0; PROGRAM_BYTES]));
}

// Verifies S53-28 and S53-29: the two dumps' bytes, and how they read.
#[test]
fn s53_28_s53_29_the_dumps_are_as_page_12_gives_them() {
    let program = Program::default_for(Variant::Synth52);
    let edit = edit_buffer_dump(&program);
    assert_eq!(edit.len(), 157);
    assert_eq!(edit[..4], [0xF0, 0x01, 0x31, 0x03]);
    assert_eq!(edit[156], 0xF7);
    assert_eq!(read_sysex(&edit[1..]), Sysex::EditBuffer(program));
    assert_eq!(check_patch(&edit), Ok(program));
    let dump = program_dump(3, 39, &program).unwrap();
    assert_eq!(dump.len(), 159);
    assert_eq!(dump[..6], [0xF0, 0x01, 0x31, 0x02, 3, 39]);
    assert_eq!(read_sysex(&dump[1..]), Sysex::ProgramDump { group: 3, number: 39, program });
    assert_eq!(check_program_dump(&dump), Ok((3, 39, program)));
    assert_eq!(program_dump(10, 0, &program), None);
    assert_eq!(program_dump(0, 40, &program), None);
    // Of another length: faulty.
    let short = [&edit[..155], &[0xF7]].concat();
    assert_eq!(read_sysex(&short[1..]), Sysex::FaultyEditBuffer);
    assert_eq!(check_patch(&short), Err(DumpError::Length(156)));
    let long = [&dump[..158], &[0, 0xF7]].concat();
    assert_eq!(read_sysex(&long[1..]), Sysex::FaultyProgramDump { group: Some(3), number: Some(39) });
    // Another maker's, or another command: nothing of this family's.
    assert_eq!(read_sysex(&[0x43, 0x00, 0x09, 0xF7]), Sysex::Other);
    assert_eq!(read_sysex(&[0x01, 0x31, 0x06, 0xF7]), Sysex::Other);
    assert_eq!(check_patch(&dump), Err(DumpError::Header));
}

// Verifies S53-33: for every parameter byte and every value, the receiver
// holds it to the maximum and the server refuses it above; reserved and
// padding bytes are carried and never refused.
#[test]
fn s53_33_the_receiver_holds_to_the_maximum_and_the_server_refuses_above_it() {
    for number in 0..PROGRAM_BYTES as u8 {
        for value in 0..=u8::MAX {
            let mut bytes = [0; PROGRAM_BYTES];
            bytes[usize::from(number)] = value;
            let program = Program::from_bytes(bytes);
            if let Some(most) = maximum(u16::from(number)) {
                assert_eq!(program.get(number), value.min(most));
                let expected =
                    if value > most { Err(ParameterError { parameter: number, maximum: most, value }) } else { Ok(()) };
                assert_eq!(program.check(), expected, "parameter {number} value {value}");
            } else {
                assert_eq!(program.get(number), 0);
                assert_eq!(program.check(), Ok(()));
            }
        }
    }
    // A dump with non-zero reserved bytes passes unchanged.
    let mut bytes = *Program::default_for(Variant::Synth53).bytes();
    bytes[70] = 9;
    bytes[127] = 0x7F;
    let program = Program::from_bytes(bytes);
    assert_eq!(check_patch(&edit_buffer_dump(&program)).map(|found| *found.bytes()), Ok(bytes));
    // The server names a parameter above its maximum.
    let mut above = *Program::default_for(Variant::Synth53).bytes();
    above[usize::from(parameter::CUTOFF)] = 121;
    assert_eq!(
        check_patch(&edit_buffer_dump(&Program::from_bytes(above))),
        Err(DumpError::Parameter(ParameterError { parameter: 17, maximum: 120, value: 121 }))
    );
}

// Verifies S53-38 and the server document's SRV-37: the fields write
// FILTER REV SELECT from the kind, every reserved and padding byte 0, and
// give back the default program; a field above its maximum is refused.
#[test]
fn s53_38_fields_write_the_revision_from_the_kind() {
    for variant in [Variant::Synth53, Variant::Synth52, Variant::Synth51] {
        let default = Program::default_for(variant);
        let fields = Fields::of(&default);
        assert_eq!(fields.program(variant), Ok(default), "{variant:?}");
        assert_eq!(default.get(parameter::FILTER_REV_SELECT), u8::from(variant == Variant::Synth53));
        assert_eq!(pack_patch(&fields, variant), Ok(edit_buffer_dump(&default)));
    }
    let fields = Fields { unison_detune: 8, ..Fields::default() };
    assert_eq!(
        fields.program(Variant::Synth53),
        Err(ParameterError { parameter: parameter::UNISON_DETUNE, maximum: 7, value: 8 })
    );
    // A program of the 2020 instrument with the other filter plays on any
    // kind: the check does not refuse it.
    let mut other = Program::default_for(Variant::Synth53);
    other.set(parameter::FILTER_REV_SELECT, 0);
    assert_eq!(check_patch(&edit_buffer_dump(&other)), Ok(other));
}

// Verifies S53-26 (a): a bank is the program dumps of one group, each
// program once; the server refuses two groups and a repeated program.
#[test]
fn s53_26_a_bank_is_one_groups_program_dumps() {
    let image = bank(3, 40);
    assert_eq!(image.len(), 6_360);
    assert_eq!(check_bank(&image), Ok(BankSummary { group: 3, programs: 40 }));
    assert_eq!(bank_group(&image), Some(3));
    for number in 0..40 {
        assert_eq!(lookup_image(&image, number), Lookup::Found(distinct(3 * 40 + u16::from(number))));
    }
    assert_eq!(lookup_image(&bank(3, 2), 2), Lookup::Missing);
    let two_groups = [bank(3, 1), bank(4, 1)].concat();
    assert_eq!(check_bank(&two_groups), Err(BankError::TwoGroups(1)));
    let repeated = [bank(3, 2), bank(3, 1)].concat();
    assert_eq!(check_bank(&repeated), Err(BankError::Repeated(2)));
    assert_eq!(check_bank(&[]), Err(BankError::Count(0)));
    let mut faulty = bank(3, 1);
    faulty.insert(10, 0);
    assert!(matches!(check_bank(&faulty), Err(BankError::Dump(0, DumpError::Length(160)))));
    assert_eq!(build_bank(10, &[distinct(0)], &mut [0; PROGRAM_DUMP_BYTES]), Err(BankError::Group(10)));
    assert_eq!(build_bank(0, &[distinct(0)], &mut [0; 10]), Err(BankError::Room));
}

/// Ten banks, one for each group, in reverse order, each of 40 programs.
fn ten_banks() -> Vec<Vec<u8>> {
    (0..GROUPS).rev().map(|group| bank(group, GROUP_PROGRAMS)).collect()
}

// Verifies S53-13 and S53-18: controller 32's values 1 to 10 choose groups
// 0 to 9, 0 chooses group 0, and 11 and above leave it; program change 0
// to 39 chooses within the group, from the bank that holds it.
#[test]
fn s53_13_s53_18_bank_select_and_program_change_reach_400_programs() {
    let images = ten_banks();
    let slices: Vec<&[u8]> = images.iter().map(Vec::as_slice).collect();
    let banks = Banks::new(&slices);
    let mut part = Part::new(Variant::Synth53);
    for value in 0..=12u8 {
        let group = if value <= 10 { value.saturating_sub(1) } else { 9 };
        let selected = part.take(&cc(32, value), &banks);
        assert_eq!(selected, if value <= 10 { Change::None } else { Change::PassedOver });
        // Bank select alone changes nothing.
        assert_eq!(part.group(), group);
        for number in 0..GROUP_PROGRAMS {
            let change = part.take(&program_change(number), &banks);
            assert_eq!(change, Change::Program { outcome: Outcome::Applied });
            assert_eq!(
                *part.program(),
                distinct(u16::from(group) * 40 + u16::from(number)),
                "value {value} program {number}"
            );
        }
    }
    // 40 to 127, and a group no bank holds: the default, passed over.
    for number in [40, 127] {
        assert_eq!(part.take(&program_change(number), &banks), Change::Program { outcome: Outcome::PassedOver });
        assert_eq!(*part.program(), Program::default_for(Variant::Synth53));
    }
    let one = bank(2, 3);
    let banks = Banks::new(&[&one]);
    part.take(&cc(32, 1), &banks);
    assert_eq!(part.take(&program_change(0), &banks), Change::Program { outcome: Outcome::PassedOver });
    part.take(&cc(32, 3), &banks);
    assert_eq!(part.take(&program_change(2), &banks), Change::Program { outcome: Outcome::Applied });
    assert_eq!(part.take(&program_change(3), &banks), Change::Program { outcome: Outcome::PassedOver });
    // Controller 0 is counted.
    assert_eq!(part.take(&cc(0, 1), &banks), Change::PassedOver);
}

// Verifies S53-18 and S53-28: before any program change the edit buffer is
// program 0 of bank index 0; a faulty dump at a program's place is the
// default, refused.
#[test]
fn s53_18_the_first_program_and_a_faulty_one() {
    let image = bank(5, 2);
    let mut part = Part::new(Variant::Synth52);
    assert_eq!(part.start(&Banks::new(&[&image])), Outcome::Applied);
    assert_eq!(*part.program(), distinct(200));
    assert_eq!(part.group(), 0);
    let mut faulty = bank(5, 1);
    faulty.insert(20, 1);
    let mut part = Part::new(Variant::Synth52);
    assert_eq!(part.start(&Banks::new(&[&faulty])), Outcome::PatchRefused);
    assert_eq!(*part.program(), Program::default_for(Variant::Synth52));
    let banks = Banks::new(&[&faulty]);
    part.take(&cc(32, 6), &banks);
    assert_eq!(part.take(&program_change(0), &banks), Change::Program { outcome: Outcome::PatchRefused });
    // A bank without program 0 starts at the default, which is no fault.
    let mut part = Part::new(Variant::Synth53);
    assert_eq!(part.start(&Banks::none()), Outcome::Applied);
    assert_eq!(*part.program(), Program::default_for(Variant::Synth53));
}

// Verifies S53-21: each controller of the table sets exactly its
// parameter, held to its maximum.
#[test]
fn s53_21_each_controller_sets_its_parameter() {
    let banks = Banks::none();
    for (number, parameter) in CONTROLLERS {
        let most = maximum(u16::from(parameter)).unwrap();
        for value in [0, 1, most, DATA_MAX] {
            let mut part = Part::new(Variant::Synth53);
            let before = *part.program();
            part.take(&cc(number, value), &banks);
            let after = part.program();
            assert_eq!(after.get(parameter), value.min(most), "controller {number}");
            for other in 0..PROGRAM_BYTES as u8 {
                if other != parameter {
                    assert_eq!(after.bytes()[usize::from(other)], before.bytes()[usize::from(other)]);
                }
            }
        }
    }
}

/// The four control changes of a parameter number message.
fn nrpn(part: &mut Part, number: u16, value: u16) -> Change {
    let banks = Banks::none();
    let seven = u16::from(DATA_MAX);
    part.take(&cc(99, (number >> 7) as u8), &banks);
    part.take(&cc(98, (number & seven) as u8), &banks);
    part.take(&cc(6, (value >> 7) as u8), &banks);
    part.take(&cc(38, (value & seven) as u8), &banks)
}

// Verifies S53-22 and S53-23: selection, data entry, increment and
// decrement, their clamping, and what is counted as ignored.
#[test]
fn s53_22_s53_23_parameter_numbers_set_the_edit_buffer() {
    let banks = Banks::none();
    for &(number, most) in &PROGRAM_RANGES {
        let mut part = Part::new(Variant::Synth53);
        nrpn(&mut part, u16::from(number), u16::from(most));
        assert_eq!(part.program().get(number), most);
        // Above the maximum: the maximum.
        nrpn(&mut part, u16::from(number), 200);
        assert_eq!(part.program().get(number), most);
        // Controller 38 alone after a selection.
        part.take(&cc(99, 0), &banks);
        part.take(&cc(38, 0), &banks);
        assert_eq!(part.program().get(number), 0);
        // Decrement at 0 and increment at the top hold.
        part.take(&cc(97, 0), &banks);
        assert_eq!(part.program().get(number), 0);
        part.take(&cc(96, 0), &banks);
        assert_eq!(part.program().get(number), 1.min(most));
        nrpn(&mut part, u16::from(number), u16::from(most));
        part.take(&cc(96, 0), &banks);
        assert_eq!(part.program().get(number), most);
    }
    let mut part = Part::new(Variant::Synth53);
    // 98 alone keeps the most significant part.
    nrpn(&mut part, 17, 100);
    part.take(&cc(98, 18), &banks);
    part.take(&cc(38, 77), &banks);
    assert_eq!((part.program().get(17), part.program().get(18)), (100, 77));
    // The null number, nothing selected, and numbers no program has.
    part.take(&cc(101, 127), &banks);
    part.take(&cc(100, 127), &banks);
    assert_eq!(part.take(&cc(6, 1), &banks), Change::PassedOver);
    for number in [70, 4096, 90] {
        assert_eq!(nrpn(&mut part, number, 1), Change::PassedOver, "{number}");
    }
    // Registered parameter 0 is the range; another registered number
    // selects nothing (S53-22).
    part.take(&cc(101, 0), &banks);
    part.take(&cc(100, 0), &banks);
    assert_eq!(part.take(&cc(6, 2), &banks), Change::BendRange);
    assert_eq!(part.bend_range_cents(), 200);
    assert_eq!(part.take(&cc(96, 0), &banks), Change::PassedOver);
    assert_eq!(part.bend_range_cents(), 200);
    assert_eq!(part.take(&cc(101, 1), &banks), Change::PassedOver);
    assert_eq!(part.take(&cc(6, 1), &banks), Change::PassedOver);
    // After reset all controllers and after lost input, nothing is
    // selected.
    nrpn(&mut part, 17, 5);
    part.take(&cc(121, 0), &banks);
    assert_eq!(part.take(&cc(38, 9), &banks), Change::PassedOver);
    nrpn(&mut part, 17, 5);
    part.lost();
    assert_eq!(part.take(&cc(38, 9), &banks), Change::PassedOver);
    assert_eq!(part.program().get(17), 5);
}

// Verifies S53-8: the range by controller 70 and by registered parameter
// 0, held to 12 semitones, 7 before any; a program change, an edit buffer
// dump and reset all controllers leave it.
#[test]
fn s53_8_the_pitch_wheel_range() {
    let banks = Banks::none();
    let mut part = Part::new(Variant::Synth53);
    assert_eq!(part.bend_range_cents(), 700);
    for (value, cents) in [(0, 100), (6, 700), (11, 1_200), (127, 1_200)] {
        assert_eq!(part.take(&cc(70, value), &banks), Change::BendRange);
        assert_eq!(part.bend_range_cents(), cents);
    }
    for (semitones, cents) in [(1, 100), (7, 700), (24, 1_200)] {
        part.take(&cc(101, 0), &banks);
        part.take(&cc(100, 0), &banks);
        part.take(&cc(6, semitones), &banks);
        assert_eq!(part.bend_range_cents(), cents);
    }
    part.take(&cc(70, 2), &banks);
    let mut zero = Program::default_for(Variant::Synth53);
    zero.set(parameter::PITCH_WHEEL_RANGE, 0);
    let image = program_dump(0, 0, &zero).unwrap();
    let banks = Banks::new(&[&image]);
    part.take(&program_change(0), &banks);
    let dump = edit_buffer_dump(&zero);
    part.take(&Message::SystemExclusive(&dump[1..]), &banks);
    part.take(&cc(121, 0), &banks);
    assert_eq!(part.bend_range_cents(), 300);
    assert_eq!(part.program().get(parameter::PITCH_WHEEL_RANGE), 0);
}

// Verifies S53-16: reset all controllers sets the wheel, the pedal, the
// selection and the pitch wheel, and leaves the volume, brightness, the
// group, the edit buffer and the range.
#[test]
fn s53_16_reset_all_controllers_is_rp_015() {
    let banks = Banks::none();
    for value in [0, 127] {
        let mut part = Part::new(Variant::Synth53);
        for (number, set) in [(1, 90), (64, 127), (7, 40), (74, 33), (32, 4), (73, 50), (70, 3)] {
            part.take(&cc(number, set), &banks);
        }
        nrpn(&mut part, u16::from(parameter::RESONANCE), 70);
        part.take(&Message::Channel(ChannelMessage::new(status::PITCH_BEND, 0, 0, 0)), &banks);
        let program = *part.program();
        assert_eq!(part.take(&cc(121, value), &banks), Change::ResetAllControllers);
        assert_eq!((part.modulation(), part.sustain(), part.bend()), (0, false, PITCH_BEND_CENTRE));
        assert_eq!((part.volume(), part.brightness(), part.group()), (40, 33, 3));
        assert_eq!((*part.program(), part.bend_range_cents()), (program, 400));
        assert_eq!(part.take(&cc(6, 1), &banks), Change::PassedOver);
    }
}

// Verifies S53-2: the messages the family takes.
#[test]
fn s53_2_what_the_family_takes() {
    let channel = |kind, data1| Message::Channel(ChannelMessage::new(kind, 5, data1, 0));
    for kind in [status::NOTE_OFF, status::NOTE_ON, status::PROGRAM_CHANGE, status::PITCH_BEND] {
        assert!(accepts(&channel(kind, 1)));
    }
    for kind in [status::KEY_PRESSURE, status::CHANNEL_PRESSURE] {
        assert!(!accepts(&channel(kind, 1)));
    }
    let taken: Vec<u8> = (0..=DATA_MAX).filter(|&number| accepts(&channel(status::CONTROL_CHANGE, number))).collect();
    let mut expected: Vec<u8> = [1, 6, 7, 32, 38, 64, 74, 96, 97, 98, 99, 100, 101, 120, 121, 123, 124, 125, 126, 127]
        .into_iter()
        .chain(CONTROLLERS.map(|(number, _)| number))
        .collect();
    expected.sort_unstable();
    assert_eq!(taken, expected);
    let program = Program::default_for(Variant::Synth53);
    assert!(accepts(&Message::SystemExclusive(&edit_buffer_dump(&program)[1..])));
    assert!(!accepts(&Message::SystemExclusive(&program_dump(0, 0, &program).unwrap()[1..])));
}

/// Plays `messages` into a part, then its restatement into a fresh one and
/// into one that has been elsewhere, and checks the three agree.
fn restated(messages: &[Message<'_>], banks: &Banks<'_>) {
    let mut part = Part::new(Variant::Synth53);
    part.start(banks);
    for message in messages {
        part.take(message, banks);
    }
    let mut writes: Vec<(Vec<u8>, Option<ChannelMessage>)> = Vec::new();
    part.restate(0, &mut |message| match message {
        Message::Channel(channel) => writes.push((Vec::new(), Some(channel))),
        Message::SystemExclusive(bytes) => writes.push((bytes.to_vec(), None)),
    });
    let total: usize = writes.iter().map(|(bytes, channel)| if channel.is_some() { 3 } else { bytes.len() + 2 }).sum();
    assert!(total <= 200, "{total} bytes");
    // A part that went elsewhere in what a restatement carries, then lost
    // its input.
    let mut stray = Part::new(Variant::Synth53);
    stray.start(banks);
    for message in [cc(32, 9), program_change(5), cc(73, 9), cc(64, 127)] {
        stray.take(&message, banks);
    }
    stray.lost();
    let mut fresh = Part::new(Variant::Synth53);
    fresh.start(banks);
    for mut fresh in [fresh, stray] {
        for (bytes, channel) in &writes {
            let message = channel.map_or(Message::SystemExclusive(bytes), Message::Channel);
            fresh.take(&message, banks);
        }
        assert_eq!(fresh.program(), part.program());
        assert_eq!(fresh.group(), part.group());
        assert_eq!(fresh.bend_range_cents(), part.bend_range_cents());
        assert_eq!(fresh.bend(), part.bend());
        assert_eq!(fresh.volume(), part.volume());
        assert_eq!(fresh.modulation(), part.modulation());
        assert_eq!(fresh.sustain(), part.sustain());
        assert_eq!(fresh.brightness(), part.brightness());
    }
}

// Verifies S53-46: the restatement brings a fresh part, and one that has
// been elsewhere, to the same edit buffer, group and controllers.
#[test]
fn s53_46_the_restatement_restores_the_part() {
    let images = ten_banks();
    let slices: Vec<&[u8]> = images.iter().map(Vec::as_slice).collect();
    let banks = Banks::new(&slices);
    restated(&[], &banks);
    restated(&[cc(32, 4), program_change(7)], &banks);
    // The group moved after the program change: the dump stands for it.
    restated(&[cc(32, 4), program_change(7), cc(32, 6)], &banks);
    let mut moved = std::vec![cc(32, 2), program_change(12)];
    for number in [3, 9, 21, 22, 26, 27, 28, 29, 30, 31] {
        moved.push(cc(number, 64));
    }
    moved.extend([
        cc(101, 0),
        cc(100, 0),
        cc(6, 9),
        cc(38, 50),
        Message::Channel(ChannelMessage::new(status::PITCH_BEND, 3, 1, 100)),
        cc(7, 50),
        cc(1, 20),
        cc(64, 127),
        cc(74, 12),
    ]);
    restated(&moved, &banks);
    let dump = edit_buffer_dump(&distinct(77));
    restated(&[Message::SystemExclusive(&dump[1..])], &banks);
}
