//! The commands' bodies and their bindings (REQ-6612; RULE-21, RULE-22,
//! RULE-99, RULE-100, RULE-102, RULE-103, RULE-106, RULE-119).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size; and a test that holds each row of a requirement's table is one long list"
)]

use std::vec::Vec;

use super::*;
use crate::animate::{Operation, OperationKind};
use crate::table::{SOUND_LEVEL, Target};
use crate::testing::{AFTER_CURRENT, PAGE, SUB, animated, body_check, built, control};

fn bytes(command: &Command) -> Vec<u8> {
    let mut out = [0_u8; MAX_BODY_BYTES];
    let length = encode(command, PAGE, SUB, &mut out).unwrap();
    out[..length].to_vec()
}

const CONTEXT: Context = Context { page: PAGE, sub: SUB, groups: 2, sequences: 1 };

/// RULE-21, RULE-99, RULE-100, RULE-22, RULE-102, RULE-103 (REQ-6612):
/// one record of each of the types 28 to 33, byte for byte, which reads
/// back as it; cut short it is refused, and a byte more is trailing.
#[test]
fn each_record_is_its_bytes() {
    let page = [0x01, 0x01, 0x00, 0x01];
    let cases: [(Command, &[u8]); 10] = [
        (Command::LocalMidi { channel: 2, status: 0x91, data1: 60, data2: 100 }, &[2, 0x91, 60, 100]),
        (
            Command::Variable { operation: VariableOperation::Set, variable: 3, operand: Operand::Constant(-2) },
            &[1, 3, 1, 0xFF, 0xFF, 0xFF, 0xFE],
        ),
        (Command::Timer { start: true, timer: 7 }, &[1, 7]),
        (Command::Timer { start: false, timer: 0 }, &[2, 0]),
        (Command::Report { value: None }, &[0, 0, 0, 0, 0, 0, 0, 0, 0]),
        (Command::Report { value: Some(Value::sound(1, SOUND_LEVEL)) }, &[1, 5, 1, 0, 1, 0, 0, 0, 0]),
        (Command::Run { group: 5 }, &[5]),
        (
            Command::Sequence { start: true, sequence: 2, mode: StartMode::Beat { timer: 255, ticks: 30 } },
            &[1, 2, 2, 255, 0, 0, 0, 30],
        ),
        (Command::Sequence { start: true, sequence: 1, mode: StartMode::At(5400) }, &[1, 1, 1, 0, 0, 0, 0x15, 0x18]),
        (Command::Sequence { start: false, sequence: 255, mode: StartMode::Next }, &[2, 255, 0, 0, 0, 0, 0, 0]),
    ];
    for (command, rest) in cases {
        let expected: Vec<u8> = page.iter().chain(rest).copied().collect();
        assert_eq!(bytes(&command), expected, "{command:?}");
        assert_eq!(decode(command.kind(), &expected), Ok((PAGE, SUB, command)));
        for cut in 0..expected.len() {
            assert!(decode(command.kind(), &expected[..cut]).is_err(), "{command:?} cut at {cut}");
        }
        let mut longer = expected.clone();
        longer.push(0);
        assert!(decode(command.kind(), &longer).is_err(), "{command:?} with a byte more");
    }
    assert_eq!(
        [LOCAL_MIDI_BYTES, TIMER_BYTES, REPORT_BYTES, RUN_BYTES, SEQUENCE_BYTES, MAX_VARIABLE_BYTES],
        [8, 6, 13, 5, 12, 15]
    );
    assert_eq!(decode(27, &[1, 1, 0, 1, 1, 0]), Err(Fault::Command("not a record of types 28 to 33")));
    // RULE-112: the page and the sub-code are held to their ranges.
    assert_eq!(decode(TYPE_TIMER, &[0x00, 0xFF, 0, 1, 1, 0]), Err(Fault::Range("page")));
    assert_eq!(decode(TYPE_TIMER, &[0x01, 0x01, 0x3F, 0x7F, 1, 0]), Err(Fault::Range("sub-code")));
}

/// RULE-21 (REQ-6612): each status a `LOCAL_MIDI` may carry, and each it
/// may not.
#[test]
fn local_midi_carries_channel_voice_messages_only() {
    for status in [0x80, 0x9F, 0xB3, 0xC0, 0xE1] {
        let data2 = if status & 0xF0 == 0xC0 { 0 } else { 127 };
        let command = Command::LocalMidi { channel: 3, status, data1: 127, data2 };
        assert_eq!(decode(TYPE_LOCAL_MIDI, &bytes(&command)), Ok((PAGE, SUB, command)), "{status:#x}");
    }
    let refused = |channel, status, data1, data2| {
        let mut out = [0_u8; LOCAL_MIDI_BYTES];
        encode(&Command::LocalMidi { channel, status, data1, data2 }, PAGE, SUB, &mut out)
    };
    // Polyphonic and channel pressure, system exclusive, real time.
    for status in [0xA0, 0xD0, 0xF0, 0xF8, 0x7F] {
        assert_eq!(refused(0, status, 0, 0), Err(Fault::Range("midi status")), "{status:#x}");
    }
    assert_eq!(refused(4, 0x90, 0, 0), Err(Fault::Range("audio channel")));
    assert_eq!(refused(0, 0x90, 128, 0), Err(Fault::Range("midi data")));
    assert_eq!(refused(0, 0x90, 0, 128), Err(Fault::Range("midi data")));
    assert_eq!(refused(0, 0xC0, 5, 1), Err(Fault::Range("program change's second byte")));
}

/// RULE-99 (REQ-6612): each operation with each operand kind its table
/// gives it is accepted and reads back; every other pairing is refused,
/// as are variable 16, "all" with anything but `clear`, and a range whose
/// least is above its greatest.
#[test]
fn variable_pairs_operations_with_their_operand_kinds() {
    let operands = [
        Operand::None,
        Operand::Constant(i32::MIN),
        Operand::Variable { constant: i32::MAX, variable: 15 },
        Operand::Fired { constant: -1, property: 4 },
        Operand::Value(Value::variable(2)),
        Operand::Range { low: -3, high: 3 },
    ];
    let mut accepted = 0;
    for operation in VariableOperation::ALL {
        for operand in operands {
            let command = Command::Variable { operation, variable: 15, operand };
            let mut out = [0_u8; MAX_VARIABLE_BYTES];
            let result = encode(&command, PAGE, SUB, &mut out);
            if operation.takes(operand.kind()) {
                let length = result.unwrap();
                assert_eq!(decode(TYPE_VARIABLE, &out[..length]), Ok((PAGE, SUB, command)));
                accepted += 1;
            } else {
                assert_eq!(result, Err(Fault::Range("operand kind")), "{operation:?} with {operand:?}");
            }
        }
    }
    // `set` four kinds; `add`, `sub`, `min`, `max` three each; `inc`,
    // `dec`, `toggle`, `clear` none; `wrap` a range.
    assert_eq!(accepted, 4 + 4 * 3 + 4 + 1);
    let refused = |operation, variable, operand| {
        let mut out = [0_u8; MAX_VARIABLE_BYTES];
        encode(&Command::Variable { operation, variable, operand }, PAGE, SUB, &mut out)
    };
    assert_eq!(refused(VariableOperation::Inc, 16, Operand::None), Err(Fault::Range("variable")));
    assert_eq!(refused(VariableOperation::Inc, 255, Operand::None), Err(Fault::Range("variable")));
    assert!(refused(VariableOperation::Clear, 255, Operand::None).is_ok());
    assert_eq!(
        refused(VariableOperation::Wrap, 0, Operand::Range { low: 4, high: 3 }),
        Err(Fault::Range("wrap's range"))
    );
    assert_eq!(
        refused(VariableOperation::Add, 0, Operand::Variable { constant: 0, variable: 16 }),
        Err(Fault::Range("operand's variable"))
    );
    assert_eq!(
        refused(VariableOperation::Add, 0, Operand::Fired { constant: 0, property: 8 }),
        Err(Fault::Range("operand's fired property"))
    );
    // A body one byte longer than its operand kind's is refused.
    let mut long = bytes(&Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None });
    long.push(0);
    assert_eq!(decode(TYPE_VARIABLE, &long), Err(Fault::Trailing));
}

/// RULE-102, RULE-100, RULE-103, RULE-22 (REQ-6612, DEC-6635 item 6):
/// `RUN` is a group alone, of 5 bytes; each start mode of `SEQUENCE` with
/// its bounds; and the bounds of the other two records.
#[test]
fn the_housekeeping_records_are_bounded() {
    let run = |group| {
        let mut out = [0_u8; RUN_BYTES];
        encode(&Command::Run { group }, PAGE, SUB, &mut out)
    };
    assert_eq!(run(63), Ok(RUN_BYTES));
    assert_eq!(RUN_BYTES, 5);
    assert_eq!(run(64), Err(Fault::Limit("MAX_GROUPS")));
    let body = |tail: &[u8]| [&[0x01, 0x01, 0x00, 0x01][..], tail].concat();
    // The second form's modes, timer and ticks are gone.
    assert_eq!(decode(TYPE_RUN, &body(&[0, 0, 0, 0, 0, 0, 0])), Err(Fault::Trailing));
    let sequence = |start, mode| {
        let mut out = [0_u8; SEQUENCE_BYTES];
        encode(&Command::Sequence { start, sequence: 0, mode }, PAGE, SUB, &mut out)
    };
    assert_eq!(SEQUENCE_BYTES, 12);
    assert_eq!(sequence(true, StartMode::Next), Ok(SEQUENCE_BYTES));
    assert!(sequence(true, StartMode::At(u32::MAX)).is_ok());
    assert!(sequence(true, StartMode::Beat { timer: 7, ticks: MAX_TIMER_TICKS }).is_ok());
    assert!(sequence(true, StartMode::Beat { timer: 255, ticks: 1 }).is_ok());
    assert_eq!(sequence(true, StartMode::Beat { timer: 8, ticks: 1 }), Err(Fault::Range("sequence's timer")));
    assert_eq!(sequence(true, StartMode::Beat { timer: 0, ticks: 0 }), Err(Fault::Limit("MAX_TIMER_TICKS")));
    assert_eq!(
        sequence(true, StartMode::Beat { timer: 0, ticks: MAX_TIMER_TICKS + 1 }),
        Err(Fault::Limit("MAX_TIMER_TICKS"))
    );
    assert_eq!(sequence(false, StartMode::At(5)), Err(Fault::Reserved("a stop's start mode")));
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 0, 0, 0, 0, 0, 0, 1])), Err(Fault::Reserved("sequence's ticks")));
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 0, 1, 3, 0, 0, 0, 1])), Err(Fault::Reserved("sequence's timer")));
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 0, 3, 0, 0, 0, 0, 0])), Err(Fault::Range("sequence's start mode")));
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 0])), Err(Fault::Short), "the second form's 6 bytes");
    // The page timer cannot be named by `TIMER`.
    assert_eq!(decode(TYPE_TIMER, &body(&[1, 8])), Err(Fault::Range("timer")));
    assert_eq!(decode(TYPE_TIMER, &body(&[1, 255])), Err(Fault::Range("timer")));
    assert_eq!(decode(TYPE_TIMER, &body(&[3, 0])), Err(Fault::Range("start or stop")));
    // Every sequence is named with stop only.
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 255, 0, 0, 0, 0, 0, 0])), Err(Fault::Range("sequence")));
    assert_eq!(decode(TYPE_SEQUENCE, &body(&[1, 8, 0, 0, 0, 0, 0, 0])), Err(Fault::Range("sequence")));
    assert_eq!(decode(TYPE_REPORT, &body(&[0, 1, 0, 0, 0, 0, 0, 0, 0])), Err(Fault::Reserved("report's value")));
    assert_eq!(decode(TYPE_REPORT, &body(&[2, 0, 0, 0, 0, 0, 0, 0, 0])), Err(Fault::Range("report's what")));
    assert_eq!(decode(TYPE_REPORT, &body(&[1, 1, 16, 0, 0, 0, 0, 0, 0])), Err(Fault::Range("value's variable")));
}

fn check_built(built: &crate::testing::Built) -> Result<(), Fault> {
    check(built.kind, &built.body, &built.bindings, &CONTEXT, body_check)
}

/// RULE-119 (REQ-6612): each record type a group may hold is accepted;
/// each it may not is refused, with its reason of RULE-119's second
/// table: a table, page content, a sound's definition and blocks, a
/// frame, navigation, the channel's and the session's records.
#[test]
fn a_group_holds_only_the_allowed_records() {
    let midi = built(Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 });
    let allowed = [
        control(0, 1, 5, 80, 0),
        animated(&[Operation { target: Target::object(1), kind: OperationKind::Reset }]),
        crate::testing::Built { kind: TYPE_SPRITES, body: [1, 1, 0, 1, 1, 0].to_vec(), bindings: Vec::new() },
        crate::testing::Built { kind: TYPE_NAPLPS_OBJECTS, body: [1, 1, 0, 1, 1, 0].to_vec(), bindings: Vec::new() },
        // Renderer parity PAR-20: a picture's links and cursor.
        crate::testing::Built { kind: TYPE_DRAWING_LINKS, body: [1, 1, 0, 1, 1, 0].to_vec(), bindings: Vec::new() },
        crate::testing::Built { kind: TYPE_REGIS_CURSOR, body: [1, 1, 0, 1, 1, 0].to_vec(), bindings: Vec::new() },
        midi.clone(),
        built(Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None }),
        built(Command::Timer { start: true, timer: 0 }),
        built(Command::Report { value: None }),
        built(Command::Run { group: 1 }),
        built(Command::Sequence { start: true, sequence: 0, mode: StartMode::Next }),
    ];
    for command in &allowed {
        assert_eq!(check_built(command), Ok(()), "type {}", command.kind);
        assert!(is_allowed(command.kind));
    }
    assert_eq!(allowed.iter().filter(|command| is_sound(command.kind)).count(), 2);
    // OBJECT, DATA, CURSOR, INPUT_CONTROL, INDEX, LOG, LINKS, SUBPAGES,
    // REDIRECT, DENIED, REGIS, NAPLPS, the three of viewer state, RULES,
    // RULES_QUERY, SHEET, FRAME, PAGE_MUSIC, PAINT, and types not defined.
    for kind in [1, 2, 4, 5, 6, 7, 9, 10, 11, 12, 14, 17, 18, 19, 20, 21, 22, 26, 27, 34, 35, 0, 8, 13, 36, 255] {
        let command = crate::testing::Built { kind, ..midi.clone() };
        assert_eq!(check_built(&command), Err(Fault::Command("record type a group may not hold")), "type {kind}");
    }
}

/// RULE-119 (REQ-6612, REQ-6615): a command names the table's own page: a
/// `CONTROL` of another page or of the whole service, one with a
/// sequence or "after the current sound", an `ANIMATE` of another
/// sub-code, and a run of a group or a sequence the table has not, are
/// each refused.
#[test]
fn a_command_names_its_tables_page() {
    let mut other = control(0, 1, 5, 80, 0);
    other.body[12..14].copy_from_slice(&0x102_u16.to_be_bytes());
    assert_eq!(check_built(&other), Err(Fault::Command("names another page")));
    let mut service = control(0, 1, 5, 80, 0);
    service.body[12..14].copy_from_slice(&[0xFF, 0xFF]);
    assert_eq!(check_built(&service), Err(Fault::Command("names another page")));
    let mut sequenced = control(0, 1, 5, 80, 0);
    sequenced.body[2] = 1;
    let waits = Err(Fault::Command("CONTROL with a sequence or after the current sound"));
    assert_eq!(check_built(&sequenced), waits);
    let mut after = control(0, 1, 5, 80, 0);
    after.body[10] = AFTER_CURRENT;
    assert_eq!(check_built(&after), waits);
    let mut refused = control(0, 1, 5, 80, 0);
    refused.body[3] = 9;
    assert_eq!(check_built(&refused), Err(Fault::Command("body its record's reader refuses")));

    let mut elsewhere = animated(&[Operation { target: Target::object(1), kind: OperationKind::Show }]);
    elsewhere.body[3] = 2;
    assert_eq!(check_built(&elsewhere), Err(Fault::Command("names another page")));
    let mut midi = built(Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 });
    midi.body[1] = 2;
    assert_eq!(check_built(&midi), Err(Fault::Command("names another page")));

    let run = built(Command::Run { group: 2 });
    assert_eq!(check_built(&run), Err(Fault::Reference("run's group")));
    let sequence = built(Command::Sequence { start: false, sequence: 1, mode: StartMode::Next });
    assert_eq!(check_built(&sequence), Err(Fault::Reference("sequence")));
    assert_eq!(check_built(&built(Command::Sequence { start: false, sequence: 255, mode: StartMode::Next })), Ok(()));
    let reads = built(Command::Report { value: Some(Value::sequence(1, 0)) });
    assert_eq!(check_built(&reads), Err(Fault::Reference("value's sequence")));
}

fn resolved(command: &crate::testing::Built, source: i32) -> (Vec<u8>, usize) {
    let mut out = [0_u8; MAX_COMMAND_BYTES];
    let bound = apply_bindings(command.kind, &command.body, &command.bindings, &|_| source, &mut out);
    (out[..bound.length].to_vec(), bound.skipped)
}

/// RULE-106 (REQ-6612): each row of the table of fields: the field takes
/// its constant plus the source, the sum saturates, and it is held to the
/// field's range, below the least and above the greatest.
#[test]
fn each_bound_field_takes_the_sum_held_to_its_range() {
    let variable = Source::Variable(3);
    let object = Target::object(9);
    let operation = |kind| animated(&[Operation { target: object, kind }]);
    // (the command, where the field is, how it is read, its least and its
    // greatest value, and its constant)
    let cases: [(crate::testing::Built, usize, usize, i32, i32, i32); 11] = [
        (
            built(Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 })
                .bound(Binding::of(field::MIDI_DATA1, variable)),
            6,
            1,
            0,
            127,
            60,
        ),
        (
            built(Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 })
                .bound(Binding::of(field::MIDI_DATA2, variable)),
            7,
            1,
            0,
            127,
            100,
        ),
        (control(1, 3, 0, 40, 500).bound(Binding::of(field::CONTROL_GAIN, variable)), 7, 1, 0, 100, 40),
        (control(1, 3, 0, 40, 500).bound(Binding::of(field::CONTROL_FADE, variable)), 8, 2, 0, 10_000, 500),
        (
            operation(OperationKind::Move { relative: false, x: 10, y: 20 }).bound(Binding::of_operation(
                0,
                field::X,
                variable,
            )),
            11,
            2,
            -32_768,
            32_767,
            10,
        ),
        (
            operation(OperationKind::Move { relative: false, x: 10, y: 20 }).bound(Binding::of_operation(
                0,
                field::Y,
                variable,
            )),
            13,
            2,
            -32_768,
            32_767,
            20,
        ),
        (operation(OperationKind::Frame(3)).bound(Binding::of_operation(0, field::FRAME, variable)), 10, 1, 0, 15, 3),
        (operation(OperationKind::Look(1)).bound(Binding::of_operation(0, field::LOOK, variable)), 10, 1, 0, 3, 1),
        (
            operation(OperationKind::Order { layer: 1, order: 5 }).bound(Binding::of_operation(
                0,
                field::ORDER,
                variable,
            )),
            11,
            1,
            -128,
            127,
            5,
        ),
        (
            operation(OperationKind::Cycle { first: 0, last: 3, period: 30, once: false })
                .bound(Binding::of_operation(0, field::PERIOD, variable)),
            12,
            2,
            1,
            3600,
            30,
        ),
        (
            operation(OperationKind::Show).bound(Binding::of_operation(0, field::TARGET, Source::Parameter)),
            8,
            2,
            1,
            65_535,
            9,
        ),
    ];
    for (command, at, width, least, greatest, constant) in cases {
        assert_eq!(check_built(&command), Ok(()), "{command:?}");
        let signed = least < 0;
        let read = |source: i32| {
            let (body, _) = resolved(&command, source);
            match (width, signed) {
                (1, false) => i32::from(body[at]),
                (1, true) => i32::from(i8::from_be_bytes([body[at]])),
                (_, false) => i32::from(u16::from_be_bytes([body[at], body[at + 1]])),
                (_, true) => i32::from(i16::from_be_bytes([body[at], body[at + 1]])),
            }
        };
        assert_eq!(read(0), constant, "{command:?}");
        assert_eq!(read(1), constant + 1, "{command:?}");
        if command.bindings[0].field != field::TARGET || command.kind != TYPE_ANIMATE {
            assert_eq!(read(i32::MAX), greatest, "{command:?}: saturating, then held to the greatest");
            assert_eq!(read(i32::MIN), least, "{command:?}: held to the least");
            assert_eq!(read(greatest - constant), greatest);
            assert_eq!(read(least - constant), least);
        }
    }
    // The 14-bit pitch bend value, two data bytes of seven bits.
    let bend = built(Command::LocalMidi { channel: 0, status: 0xE0, data1: 0x00, data2: 0x40 })
        .bound(Binding::of(field::MIDI_BEND, Source::Fired(0)));
    assert_eq!(check_built(&bend), Ok(()));
    assert_eq!(resolved(&bend, 0).0[6..], [0x00, 0x40]);
    assert_eq!(resolved(&bend, 129).0[6..], [0x01, 0x41]);
    assert_eq!(resolved(&bend, i32::MAX).0[6..], [0x7F, 0x7F]);
    assert_eq!(resolved(&bend, -9000).0[6..], [0x00, 0x00]);
}

/// RULE-106 (REQ-6612): a target's number is an identity: it takes a row
/// parameter or the firing's target and never a variable, and a number
/// outside its range at run time leaves that one operation out, counted,
/// while the record's other operations stand.
#[test]
fn a_bound_target_number_is_an_identity() {
    let two = animated(&[
        Operation { target: Target::control(1), kind: OperationKind::Look(2) },
        Operation { target: Target::object(7), kind: OperationKind::Show },
    ]);
    let by_row = two.clone().bound(Binding::of_operation(0, field::TARGET, Source::Parameter));
    assert_eq!(check_built(&by_row), Ok(()));
    let by_target = two.clone().bound(Binding::of_operation(1, field::TARGET, Source::Fired(1)));
    assert_eq!(check_built(&by_target), Ok(()));
    // The firing's row may give a target's number too (DEC-6635 item 8:
    // the meter's segment `301 + row`).
    let by_place = two.clone().bound(Binding::of_operation(1, field::TARGET, Source::Fired(2)));
    assert_eq!(check_built(&by_place), Ok(()));
    let refusal = Err(Fault::Binding("a target's number takes the row's p, the firing's row or its target"));
    for source in [Source::Variable(0), Source::Fired(0), Source::Fired(3)] {
        let by_other = two.clone().bound(Binding::of_operation(0, field::TARGET, source));
        assert_eq!(check_built(&by_other), refusal, "{source:?}");
    }
    // Control 1 plus 31 is control 32, the last; plus 32 is none.
    let (body, skipped) = resolved(&by_row, 31);
    assert_eq!((body[4], &body[6..10], skipped), (2, &[7, 1, 0, 32][..], 0));
    let (body, skipped) = resolved(&by_row, 32);
    assert_eq!(skipped, 1);
    assert_eq!(body, [0x01, 0x01, 0x00, 0x01, 1, 0, 5, 2, 0, 7], "the other operation stands");
    let (body, skipped) = resolved(&by_row, -1);
    assert_eq!((body[4], skipped), (1, 1));
    // The only operation left out leaves nothing to apply.
    let one = animated(&[Operation { target: Target::object(7), kind: OperationKind::Show }])
        .bound(Binding::of_operation(0, field::TARGET, Source::Parameter));
    assert_eq!(resolved(&one, -7), (Vec::new(), 1));
    assert_eq!(resolved(&one, 65_528).0[8..], [0xFF, 0xFF]);
    assert_eq!(resolved(&one, 65_529), (Vec::new(), 1));
    // A bound number's constant may be 0: `{"object":{"row":"p2"}}`.
    let mut zero = one.clone();
    zero.body[8..10].copy_from_slice(&[0, 0]);
    assert_eq!(check_built(&zero), Ok(()));
    assert_eq!(resolved(&zero, 301).0[8..], [0x01, 0x2D]);
    zero.bindings.clear();
    assert!(check_built(&zero).is_err(), "unbound, number 0 names nothing");
}

/// RULE-106 (REQ-6612): a binding of a field not listed, of a record
/// that takes none, a source no source has, two bindings of one field,
/// an item beyond the operations and one binding over the limit are each
/// refused.
#[test]
fn bindings_outside_the_table_are_refused() {
    let variable = Source::Variable(0);
    let midi = |status| built(Command::LocalMidi { channel: 0, status, data1: 1, data2: 0 });
    let refused = |command: crate::testing::Built, why| assert_eq!(check_built(&command), Err(Fault::Binding(why)));
    let not_this_message = "field of this MIDI message";
    refused(midi(0x90).bound(Binding::of(field::MIDI_BEND, variable)), not_this_message);
    refused(midi(0xC0).bound(Binding::of(field::MIDI_DATA2, variable)), not_this_message);
    refused(midi(0xE0).bound(Binding::of(field::MIDI_DATA1, variable)), not_this_message);
    refused(midi(0x90).bound(Binding::of(4, variable)), not_this_message);
    refused(midi(0x90).bound(Binding { item: 1, field: 1, source: 0x10 }), "item of a record with no operations");
    refused(midi(0x90).bound(Binding { item: 0, field: 1, source: 0x24 }), "source");
    refused(midi(0x90).bound(Binding { item: 0, field: 1, source: 0x34 }), "source");
    refused(midi(0x90).bound(Binding { item: 0, field: 1, source: 0x0F }), "source");
    let twice = midi(0x90).bound(Binding::of(1, variable)).bound(Binding::of(1, Source::Parameter));
    refused(twice, "field bound twice");
    refused(control(0, 1, 5, 80, 0).bound(Binding::of(3, variable)), "record or field that takes none");
    let takes_none = "record or field that takes none";
    for command in [
        Command::Variable { operation: VariableOperation::Inc, variable: 0, operand: Operand::None },
        Command::Timer { start: true, timer: 0 },
        Command::Report { value: None },
        Command::Run { group: 0 },
        Command::Sequence { start: true, sequence: 0, mode: StartMode::Next },
    ] {
        refused(built(command).bound(Binding::of(1, variable)), takes_none);
    }
    let show = animated(&[Operation { target: Target::object(7), kind: OperationKind::Show }]);
    refused(
        show.clone().bound(Binding::of_operation(1, field::TARGET, Source::Parameter)),
        "item beyond the operations",
    );
    refused(show.clone().bound(Binding::of_operation(0, field::X, variable)), "field of this operation");
    refused(show.clone().bound(Binding::of_operation(0, 8, variable)), "field of this operation");
    let mut many = midi(0x90);
    many.bindings = std::vec![Binding::of(1, variable); MAX_COMMAND_BINDINGS + 1];
    assert_eq!(check_built(&many), Err(Fault::Limit("MAX_COMMAND_BINDINGS")));
    for code in 0..=u8::MAX {
        assert_eq!(Source::from_code(code).map(Source::code), Source::from_code(code).map(|_| code));
    }
    assert_eq!(Source::from_code(0x1F), Some(Source::Variable(15)));
    assert_eq!(Source::from_code(0x20), Some(Source::Parameter));
    for withdrawn in 0x21..=0x23 {
        assert_eq!(Source::from_code(withdrawn), None, "the second form's p1 to p3 (DEC-6635 item 8)");
    }
    assert_eq!(Source::from_code(0x33), Some(Source::Fired(3)));
}
