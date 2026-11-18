//! The readable patch fields of every family and the performance state a
//! page's rules read (`neoteletext-common/docs/page-rules.md`, RULE-28,
//! RULE-29, RULE-30; rdm REQ-6527): each field of appendix A read back
//! after it is set by its own control message, and each class of value
//! after the message that moves it.

// Without every family there is nothing here to test.
#![cfg(all(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

#[path = "rules_support.rs"]
mod support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Message, PatchField, class, family, status};
use neoteletext_audio_instruments::watch::{Watched, fields};
use neoteletext_audio_instruments::{synth8, synth80};
use support::{KINDS, Kind, Sent, control, note_off, note_on, set_field};

/// The parts a field is set and read on: synth7 has sixteen, synth8 two,
/// and every other family is one part whatever the channel.
fn parts(kind: Kind) -> &'static [u8] {
    match kind.code {
        family::SYNTH7 => &[0, 9, 15],
        family::SYNTH8 => &[0, 1],
        _ => &[0],
    }
}

/// The values a field is set to: its least, its greatest, and one between.
fn values(field: &PatchField) -> Vec<i32> {
    let mut values = vec![field.least, field.greatest, field.least + (field.greatest - field.least) / 3];
    values.dedup();
    values
}

fn give(watched: &mut Watched, banks: &Banks<'_>, sent: &[Sent]) {
    for message in sent {
        watched.take(&message.message(), banks);
    }
}

// Verifies RULE-29 (REQ-6527): each family publishes one table, of the
// counts appendix A has, every name once, every range a least and a
// greatest, and an enumeration read as its values' ordinals.
#[test]
fn each_family_publishes_one_table_of_named_ranges() {
    let counts = [28, 28, 170, 24, 25, 42, 66, 66, 66, 42, 85];
    for (kind, count) in KINDS.into_iter().zip(counts) {
        let table = fields(kind.code, kind.variant).unwrap();
        assert_eq!(table.len(), count, "{}", kind.name);
        for (number, field) in table.iter().enumerate() {
            assert!(field.least <= field.greatest, "{} field {number}", kind.name);
            assert!(
                !field.name.is_empty()
                    && field
                        .name
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
            );
            assert_eq!(
                table.iter().filter(|other| other.name == field.name).count(),
                1,
                "{} names {} once",
                kind.name,
                field.name
            );
            if !field.values.is_empty() {
                assert_eq!(
                    (field.least, field.greatest),
                    (0, i32::try_from(field.values.len()).unwrap() - 1),
                    "{}",
                    field.name
                );
            }
        }
    }
    // The variants of a family that differ have tables that differ, and a
    // family and variant that do not exist have none.
    assert_eq!(fields(family::SYNTH106, 0).unwrap()[19].values, ["off", "one", "two"]);
    assert_eq!(fields(family::SYNTH106, 1).unwrap()[19].values, ["off", "one", "two", "both"]);
    assert_eq!(fields(family::SYNTH106, 1).unwrap()[24].name, "sub_switch");
    for (code, variant) in
        [(11, 0), (19, 0), (family::SYNTH7, 1), (family::SYNTH106, 2), (family::SYNTH53, 3), (family::SYNTH8, 1)]
    {
        assert!(fields(code, variant).is_none() && Watched::new(code, variant, &Banks::none()).is_none());
    }
}

// Verifies RULE-28 (REQ-6527): for each family, every field of appendix A
// reads the value its own control message set, on the part it was set on.
#[test]
fn every_field_reads_what_its_control_message_set() {
    let banks = Banks::none();
    for kind in KINDS {
        let table = fields(kind.code, kind.variant).unwrap();
        for &part in parts(kind) {
            let mut watched = Watched::new(kind.code, kind.variant, &banks).unwrap();
            assert_eq!(
                (watched.code(), watched.variant(), watched.fields().len()),
                (kind.code, kind.variant, table.len())
            );
            for (number, field) in (0_u16..).zip(table) {
                for value in values(field) {
                    give(&mut watched, &banks, &set_field(kind, part, number, value));
                    assert_eq!(
                        watched.read(class::FIELD, part, number, &banks),
                        value,
                        "{} part {part} field {number} {}",
                        kind.name,
                        field.name
                    );
                    assert_eq!(watched.field(part, number, &banks), Some(value));
                }
            }
            // Every field still reads what it was last set to: none moved
            // another.
            for (number, field) in (0_u16..).zip(table) {
                let last = *values(field).last().unwrap();
                // The synth6581 family's identifiers 25 to 27 are a second
                // write to the registers of 22 to 24 (appendix A.1).
                let shared = kind.code == family::SYNTH6581 && (22..25).contains(&number);
                if !shared {
                    assert_eq!(watched.read(class::FIELD, part, number, &banks), last, "{} {}", kind.name, field.name);
                }
            }
            // A number past the table, and a part the family does not
            // have, read 0.
            let past = u16::try_from(table.len()).unwrap();
            assert_eq!((watched.read(class::FIELD, part, past, &banks), watched.field(part, past, &banks)), (0, None));
            if kind.code == family::SYNTH8 {
                assert_eq!(watched.read(class::FIELD, 2, 0, &banks), 0);
            }
        }
    }
}

// Verifies RULE-28 (REQ-6527): synth7's parts are apart, and a field set on
// one is not read on another.
#[test]
fn synth7s_fields_are_each_parts_own() {
    let banks = Banks::none();
    let kind = KINDS[2];
    let mut watched = Watched::new(kind.code, kind.variant, &banks).unwrap();
    // Field 134 is the algorithm, 0 to 31; the default voice's is 0.
    give(&mut watched, &banks, &set_field(kind, 3, 134, 17));
    assert_eq!(watched.read(class::FIELD, 3, 134, &banks), 17);
    assert_eq!(watched.read(class::FIELD, 4, 134, &banks), 0);
    // Operator 1's output level is field 16 and voice parameter 121.
    give(&mut watched, &banks, &set_field(kind, 3, 16, 42));
    assert_eq!(watched.read(class::FIELD, 3, 16, &banks), 42);
    // The operator switch is field 155, and a function parameter field 157.
    give(&mut watched, &banks, &set_field(kind, 3, 155, 0b10_1010));
    give(&mut watched, &banks, &set_field(kind, 3, 157, 12));
    assert_eq!(
        (watched.read(class::FIELD, 3, 155, &banks), watched.read(class::FIELD, 3, 157, &banks)),
        (0b10_1010, 12)
    );
}

// Verifies RULE-28 (REQ-6527): a field the messages have not moved reads
// what the sound's bank has for the part's program, on the families whose
// trackers keep the program and not the patch (synth8, synth80), and a
// program change reads the new program's.
#[test]
fn a_field_not_moved_reads_its_programs_patch_from_the_bank() {
    let program_change =
        |channel, program| Sent::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, program, 0));
    // synth8: two patches of 42 bytes; field 19 is the VCF cutoff.
    let image = [
        synth8::Patch::DEFAULT.with(synth8::Param::VcfCutoff, 33).to_bytes(),
        synth8::Patch::DEFAULT.with(synth8::Param::VcfCutoff, 77).to_bytes(),
    ]
    .concat();
    let images = [&image[..]];
    let banks = Banks::new(&images);
    let kind = KINDS[9];
    let mut watched = Watched::new(kind.code, kind.variant, &banks).unwrap();
    assert_eq!(watched.read(class::FIELD, 0, 19, &banks), 33, "program 0 at the start");
    give(&mut watched, &banks, &[program_change(1, 1)]);
    assert_eq!((watched.read(class::FIELD, 0, 19, &banks), watched.read(class::FIELD, 1, 19, &banks)), (33, 77));
    give(&mut watched, &banks, &set_field(kind, 1, 19, 5));
    assert_eq!(watched.read(class::FIELD, 1, 19, &banks), 5);
    give(&mut watched, &banks, &[program_change(1, 0)]);
    assert_eq!(watched.read(class::FIELD, 1, 19, &banks), 33, "a program change sets every control again");
    // synth80: field 4 is BRILLIANCE, byte 56 of the patch.
    let mut bright = synth80::Patch::DEFAULT;
    bright.set_byte(56, 99);
    let image = [synth80::Patch::DEFAULT.to_bytes(), bright.to_bytes()].concat();
    let images = [&image[..]];
    let banks = Banks::new(&images);
    let kind = KINDS[10];
    let mut watched = Watched::new(kind.code, kind.variant, &banks).unwrap();
    let default = i32::from(synth80::Patch::DEFAULT.byte(56));
    assert_eq!(watched.read(class::FIELD, 0, 4, &banks), default);
    give(&mut watched, &banks, &[program_change(0, 1)]);
    assert_eq!(watched.read(class::FIELD, 0, 4, &banks), 99);
    give(&mut watched, &banks, &set_field(kind, 0, 4, 12));
    assert_eq!(watched.read(class::FIELD, 0, 4, &banks), 12);
}

// Verifies RULE-28 (REQ-6527): program, notes, note, velocity, controller
// and bend follow the channel messages given to the sound, part by part,
// on every family that plays notes; the synth6581 family, which has none,
// reads 0 for them.
#[test]
fn the_performance_classes_follow_the_channel_messages() {
    let banks = Banks::none();
    for kind in KINDS {
        let mut watched = Watched::new(kind.code, kind.variant, &banks).unwrap();
        let read = |watched: &Watched, class, part, index| watched.read(class, part, index, &banks);
        assert_eq!(read(&watched, class::BEND, 0, 0), if kind.code == family::SYNTH6581 { 0 } else { 8_192 });
        give(
            &mut watched,
            &banks,
            &[
                note_on(0, 60, 100),
                note_on(0, 64, 1),
                note_on(1, 60, 127),
                control(0, 1, 55),
                Sent::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 1, 9, 0)),
                Sent::Channel(ChannelMessage::new(status::PITCH_BEND, 0, 0x7F, 0x7F)),
            ],
        );
        let got = [
            read(&watched, class::NOTES, 0, 0),
            read(&watched, class::NOTE, 0, 60),
            read(&watched, class::NOTE, 0, 61),
            read(&watched, class::VELOCITY, 0, 60),
            read(&watched, class::VELOCITY, 0, 64),
            read(&watched, class::NOTES, 1, 0),
            read(&watched, class::VELOCITY, 1, 60),
            read(&watched, class::CONTROLLER, 0, 1),
            read(&watched, class::CONTROLLER, 1, 1),
            read(&watched, class::PROGRAM, 1, 0),
            read(&watched, class::PROGRAM, 0, 0),
            read(&watched, class::BEND, 0, 0),
            read(&watched, class::BEND, 1, 0),
        ];
        if kind.code == family::SYNTH6581 {
            assert_eq!(got, [0; 13], "{}: a family with no notes", kind.name);
            continue;
        }
        assert_eq!(got, [2, 1, 0, 100, 1, 1, 127, 55, 0, 9, 0, 16_383, 8_192], "{}", kind.name);
        // A note off, and a note on of velocity 0, lift a key.
        give(&mut watched, &banks, &[note_off(0, 60), note_on(0, 64, 0)]);
        assert_eq!([read(&watched, class::NOTES, 0, 0), read(&watched, class::VELOCITY, 0, 60)], [0, 0]);
        // All notes off lifts a part's keys; reset all controllers puts the
        // wheel and the bend at rest.
        give(&mut watched, &banks, &[note_on(0, 10, 9), control(1, 123, 0), control(0, 121, 0)]);
        assert_eq!(
            [
                read(&watched, class::NOTES, 1, 0),
                read(&watched, class::NOTES, 0, 0),
                read(&watched, class::CONTROLLER, 0, 1),
                read(&watched, class::BEND, 0, 0),
            ],
            [0, 1, 0, 8_192]
        );
        // The classes metered from the instrument, a class there is none
        // of, and a key above 127 read 0 here.
        for class in [class::ENVELOPE_STAGE, class::ENVELOPE_LEVEL, class::VOICES, 10, 255] {
            assert_eq!(read(&watched, class, 0, 0), 0);
        }
        assert_eq!(read(&watched, class::NOTE, 0, 128), 0);
        // A rule's midi action is taken as its three bytes, and what is no
        // channel message is not taken.
        watched.channel_message(0x92, 72, 90, &banks);
        watched.channel_message(0xF0, 72, 90, &banks);
        watched.channel_message(0x12, 72, 90, &banks);
        assert_eq!((read(&watched, class::VELOCITY, 2, 72), read(&watched, class::NOTES, 2, 0)), (90, 1));
        watched.release_all();
        assert_eq!((read(&watched, class::NOTES, 2, 0), read(&watched, class::NOTES, 0, 0)), (0, 0));
        // A setup block is taken event by event (RULE-60).
        let mut block = Vec::new();
        for sent in [Sent::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 4, 3, 0)), note_on(4, 50, 20)] {
            block.extend(sent.event());
        }
        watched.take_block(&block, &banks);
        assert_eq!((read(&watched, class::PROGRAM, 4, 0), read(&watched, class::VELOCITY, 4, 50)), (3, 20));
        let _ = Message::SystemExclusive(&[]);
    }
}
