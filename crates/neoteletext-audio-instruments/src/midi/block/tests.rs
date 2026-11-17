#![allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects, reason = "tests")]

use std::vec;
use std::vec::Vec;

use super::super::{ChannelMessage, Message, MidiEvent, status};
use super::{BlockFault, BlockWriter, Events, MOST_BLOCK_BYTES, MOST_EVENTS, Read, WriteError, check};

fn reads(block: &[u8]) -> Vec<Read<'_>> {
    Events::new(block).collect()
}

fn channel(status: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage { status, data1, data2 })
}

fn event(time: u8, message: Message<'_>) -> Read<'_> {
    Read::Event(MidiEvent { time, message })
}

/// MIDI-32: an empty block holds no event.
#[test]
fn an_empty_block_holds_no_event() {
    assert_eq!(reads(&[]), []);
    assert_eq!(check(&[]), Ok(0));
}

/// MIDI-23, MIDI-26: channel messages of two and one data bytes, times
/// summed from the deltas.
#[test]
fn channel_messages_are_read_with_their_times() {
    let block = [0, 0x90, 60, 100, 10, 0xC3, 5, 0x81, 0x00, 0xE0, 0, 64];
    assert_eq!(
        reads(&block),
        vec![event(0, channel(0x90, 60, 100)), event(10, channel(0xC3, 5, 0)), event(138, channel(0xE0, 0, 64))]
    );
}

/// MIDI-24: one- and two-byte quantities, minimal only; 0x80 first and a
/// third byte are framing errors.
#[test]
fn quantities_are_minimal_and_at_most_two_bytes() {
    // 199 is 0x81 0x47.
    assert_eq!(reads(&[0x81, 0x47, 0xD0, 1]), vec![event(199, channel(0xD0, 1, 0))]);
    assert_eq!(reads(&[0x80, 0x05, 0xD0, 1]), vec![Read::Malformed]);
    assert_eq!(reads(&[0x81, 0x81, 0x00, 0xD0, 1]), vec![Read::Malformed]);
}

/// MIDI-25: an event whose time reaches 200 ends the block as malformed;
/// the events before it stand.
#[test]
fn an_event_past_the_block_is_malformed() {
    let block = [0, 0x90, 60, 100, 0x81, 0x47, 0x80, 60, 0, 1, 0x90, 61, 100, 0, 0x90, 62, 1];
    assert_eq!(
        reads(&block),
        vec![event(0, channel(0x90, 60, 100)), event(199, channel(0x80, 60, 0)), Read::Malformed]
    );
}

/// MIDI-12, MIDI-33: a data byte where a status belongs (running status),
/// an undefined status, a cut message and a status byte as data are
/// framing errors.
#[test]
fn framing_errors() {
    for block in [
        &[0, 0x90, 60, 100, 0, 61, 100][..],
        &[0, 0xF4],
        &[0, 0xF5],
        &[0, 0xF9],
        &[0, 0xFD],
        &[0, 0x90, 60],
        &[0, 0x90, 0x80, 1],
        &[0, 0xF0, 5, 1, 0xF7],
        &[0, 0xFF, 0x80, 0],
    ] {
        let read = reads(block);
        assert_eq!(read.last(), Some(&Read::Malformed), "{block:02X?}");
        assert_eq!(read.iter().filter(|read| **read == Read::Malformed).count(), 1);
    }
}

/// MIDI-27, MIDI-28, MIDI-34: a whole system exclusive event is read; one
/// without its 0xF7, with a status byte inside, of length 0 or over 256 is
/// passed over by its length, and the next event is read.
#[test]
fn system_exclusive_events() {
    let whole = [0, 0xF0, 3, 0x43, 0x10, 0xF7, 0, 0xD0, 1];
    assert_eq!(
        reads(&whole),
        vec![event(0, Message::SystemExclusive(&[0x43, 0x10, 0xF7])), event(0, channel(0xD0, 1, 0))]
    );
    for block in [
        &[0, 0xF0, 2, 0x43, 0x10, 0, 0xD0, 1][..],
        &[0, 0xF0, 3, 0x43, 0x90, 0xF7, 0, 0xD0, 1],
        &[0, 0xF0, 0, 0, 0xD0, 1],
    ] {
        assert_eq!(reads(block), vec![Read::PassedOver, event(0, channel(0xD0, 1, 0))], "{block:02X?}");
    }
    let mut long = vec![0, 0xF0, 0x82, 0x01];
    long.extend(core::iter::repeat_n(0, 256));
    long.push(0xF7);
    long.extend([0, 0xD0, 1]);
    assert_eq!(reads(&long), vec![Read::PassedOver, event(0, channel(0xD0, 1, 0))]);
    let mut longest = vec![0, 0xF0, 0x82, 0x00];
    longest.extend(core::iter::repeat_n(0, 255));
    longest.push(0xF7);
    assert!(
        matches!(reads(&longest)[..], [Read::Event(MidiEvent { message: Message::SystemExclusive(bytes), .. })] if bytes.len() == 256)
    );
}

/// MIDI-23, MIDI-29, MIDI-50: continuation and escape events, meta events
/// and bare system common and real-time messages are passed over by their
/// lengths.
#[test]
fn skipped_events_are_passed_over_by_their_length() {
    let block = [
        0, 0xF7, 2, 0x01, 0xF7, 0, 0xFF, 0x51, 3, 0x0F, 0x42, 0x40, 0, 0xF1, 5, 0, 0xF2, 1, 2, 0, 0xF3, 4, 0, 0xF6, 0,
        0xF8, 0, 0xFA, 0, 0xFB, 0, 0xFC, 0, 0xFE, 0, 0xD0, 9,
    ];
    let read = reads(&block);
    assert_eq!(read.len(), 12);
    assert!(read[..11].iter().all(|read| *read == Read::PassedOver));
    assert_eq!(read[11], event(0, channel(0xD0, 9, 0)));
}

/// MIDI-31: the first 256 events are acted on, every later one passed
/// over.
#[test]
fn events_past_256_are_passed_over() {
    let block: Vec<u8> = (0..MOST_EVENTS + 2).flat_map(|_| [0, 0xD0, 1]).collect();
    let read = reads(&block);
    assert_eq!(read.iter().filter(|read| matches!(read, Read::Event(_))).count(), MOST_EVENTS);
    assert_eq!(read.iter().filter(|read| **read == Read::PassedOver).count(), 2);
    assert_eq!(check(&block), Err(BlockFault::PassedOver(MOST_EVENTS)));
}

/// MIDI-21: the server's check says where a block fails.
#[test]
fn the_check_names_the_first_fault() {
    assert_eq!(check(&[0, 0x90, 60, 1, 0, 0xF8]), Err(BlockFault::PassedOver(1)));
    assert_eq!(check(&[0, 0x90, 60, 1, 0, 61]), Err(BlockFault::Malformed(1)));
    assert_eq!(check(&vec![0; MOST_BLOCK_BYTES + 1]), Err(BlockFault::TooLong));
}

/// MIDI-23, MIDI-24, MIDI-44: the writer's block reads back as written,
/// with one encoding of each number.
#[test]
fn the_writer_writes_what_the_reader_reads() {
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    let sysex = [0x43, 0x10, 0x01, 0x02, 0xF7];
    writer.push(0, &channel(0x90, 60, 100)).unwrap();
    writer.push(127, &channel(0xC0, 3, 0)).unwrap();
    writer.push(128, &Message::SystemExclusive(&sysex)).unwrap();
    writer.push(199, &channel(0x80, 60, 64)).unwrap();
    assert_eq!(
        writer.bytes(),
        [0, 0x90, 60, 100, 127, 0xC0, 3, 1, 0xF0, 5, 0x43, 0x10, 0x01, 0x02, 0xF7, 71, 0x80, 60, 64]
    );
    assert_eq!(
        reads(writer.bytes()),
        vec![
            event(0, channel(0x90, 60, 100)),
            event(127, channel(0xC0, 3, 0)),
            event(128, Message::SystemExclusive(&sysex)),
            event(199, channel(0x80, 60, 64)),
        ]
    );
}

/// MIDI-28: a system exclusive event of 128 to 256 bytes takes a two-byte
/// length; 256 is `0x82 0x00`.
#[test]
fn a_long_system_exclusive_length_takes_two_bytes() {
    let mut message = vec![0_u8; 255];
    message.push(0xF7);
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    writer.push(0, &Message::SystemExclusive(&message)).unwrap();
    assert_eq!(&writer.bytes()[..4], [0, 0xF0, 0x82, 0x00]);
    message.insert(0, 0);
    assert_eq!(writer.push(0, &Message::SystemExclusive(&message)), Err(WriteError::Message));
}

/// MIDI-8, MIDI-11, MIDI-25, MIDI-26, MIDI-30, MIDI-31: what the writer
/// refuses, writing nothing.
#[test]
fn the_writer_refuses_what_a_block_never_carries() {
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    writer.push(10, &channel(0xD0, 1, 0)).unwrap();
    assert_eq!(writer.push(9, &channel(0xD0, 1, 0)), Err(WriteError::Time));
    assert_eq!(writer.push(200, &channel(0xD0, 1, 0)), Err(WriteError::Time));
    assert_eq!(writer.push(10, &channel(0x90, 0x80, 1)), Err(WriteError::Message));
    assert_eq!(writer.push(10, &channel(0xC0, 1, 1)), Err(WriteError::Message));
    assert_eq!(writer.push(10, &channel(status::TIMING_CLOCK, 0, 0)), Err(WriteError::Message));
    assert_eq!(writer.push(10, &channel(0xB0, 122, 0)), Err(WriteError::Message));
    assert_eq!(writer.push(10, &Message::SystemExclusive(&[0x43, 0x10])), Err(WriteError::Message));
    assert_eq!(writer.bytes(), [10, 0xD0, 1]);
    for _ in 1..MOST_EVENTS {
        writer.push(10, &channel(0xD0, 1, 0)).unwrap();
    }
    assert_eq!(writer.push(10, &channel(0xD0, 1, 0)), Err(WriteError::TooManyEvents));
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    // 251 four-byte events are 1,004 bytes; a fifth byte is past 1,005.
    for _ in 0..251 {
        writer.push(0, &channel(0x90, 1, 1)).unwrap();
    }
    assert_eq!(writer.push(0, &channel(0x90, 1, 1)), Err(WriteError::Full));
    assert_eq!(writer.push(0, &channel(0xD0, 1, 0)), Err(WriteError::Full));
    assert_eq!(writer.bytes().len(), 1_004);
}
