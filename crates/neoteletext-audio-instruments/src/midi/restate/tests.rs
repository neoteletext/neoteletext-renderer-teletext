#![allow(clippy::indexing_slicing, clippy::unwrap_used, reason = "tests")]

use std::vec;
use std::vec::Vec;

use super::super::{ChannelMessage, Message, status};
use super::{Parts, SysexKind};

/// A message as its bytes: a channel message's status and data, or a
/// system exclusive message's `F0` and bytes.
fn bytes(message: Message<'_>) -> Vec<u8> {
    match message {
        Message::Channel(channel) => {
            let count = super::super::data_bytes(channel.status).unwrap();
            [channel.status, channel.data1, channel.data2][..=count].to_vec()
        }
        Message::SystemExclusive(bytes) => [&[0xF0], bytes].concat(),
    }
}

fn restatement(parts: &Parts, channel: u8) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    parts.part(channel).unwrap().restate(channel, &mut |message| out.push(bytes(message)));
    out
}

fn cc(parts: &mut Parts, channel: u8, number: u8, value: u8) {
    parts.take(&Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)), None);
}

/// MIDI-93: a fresh part restates nothing; a part restates its program,
/// controllers, parameters, bend, pressure, volume, modulation and pedal,
/// in MIDI-95's order.
#[test]
fn a_part_is_restated_in_order() {
    let mut parts = Parts::new();
    assert_eq!(restatement(&parts, 2), Vec::<Vec<u8>>::new());
    cc(&mut parts, 2, 0, 1);
    cc(&mut parts, 2, 32, 3);
    parts.take(&Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 2, 5, 0)), None);
    cc(&mut parts, 2, 35, 9);
    cc(&mut parts, 2, 3, 70);
    cc(&mut parts, 2, 99, 0);
    cc(&mut parts, 2, 98, 18);
    cc(&mut parts, 2, 6, 40);
    cc(&mut parts, 2, 38, 2);
    cc(&mut parts, 2, 101, 0);
    cc(&mut parts, 2, 100, 0);
    cc(&mut parts, 2, 6, 7);
    parts.take(&Message::Channel(ChannelMessage::new(status::PITCH_BEND, 2, 0, 0x50)), None);
    cc(&mut parts, 2, 7, 90);
    cc(&mut parts, 2, 64, 127);
    // Notes and all notes off are not state.
    parts.take(&Message::Channel(ChannelMessage::new(status::NOTE_ON, 2, 60, 100)), None);
    cc(&mut parts, 2, 123, 0);
    assert_eq!(
        restatement(&parts, 2),
        vec![
            vec![0xB2, 0, 1],
            vec![0xB2, 32, 3],
            vec![0xC2, 5],
            vec![0xB2, 3, 70],
            vec![0xB2, 35, 9],
            vec![0xB2, 101, 0],
            vec![0xB2, 100, 0],
            vec![0xB2, 6, 7],
            vec![0xB2, 38, 0],
            vec![0xB2, 99, 0],
            vec![0xB2, 98, 18],
            vec![0xB2, 6, 40],
            vec![0xB2, 38, 2],
            vec![0xE2, 0, 0x50],
            vec![0xB2, 7, 90],
            vec![0xB2, 64, 127],
        ]
    );
    // The other parts are untouched.
    assert_eq!(restatement(&parts, 3), Vec::<Vec<u8>>::new());
}

/// MIDI-93: an inline patch after the program change is restated in its
/// place; a system exclusive parameter change is restated as its latest.
#[test]
fn an_inline_patch_and_sysex_parameters_are_restated() {
    let mut parts = Parts::new();
    parts.take(&Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 5, 0)), None);
    parts.take(&Message::SystemExclusive(&[0x41, 0x30, 0x00, 0xF7]), Some((0, SysexKind::Patch)));
    parts.take(&Message::SystemExclusive(&[0x43, 0x10, 0x01, 0x10, 0xF7]), Some((0, SysexKind::Parameter(1))));
    parts.take(&Message::SystemExclusive(&[0x43, 0x10, 0x01, 0x20, 0xF7]), Some((0, SysexKind::Parameter(1))));
    parts.take(&Message::SystemExclusive(&[0x7D, 0xF7]), Some((0, SysexKind::Other)));
    assert_eq!(
        restatement(&parts, 0),
        vec![vec![0xF0, 0x41, 0x30, 0x00, 0xF7], vec![0xF0, 0x43, 0x10, 0x01, 0x20, 0xF7]]
    );
    parts.take(&Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, 0, 6, 0)), None);
    assert_eq!(restatement(&parts, 0)[0], vec![0xC0, 6]);
}

/// MIDI-93, MIDI-84: a part restated into a fresh one holds the same
/// state; reset all controllers takes back what RP-015 sets.
#[test]
fn a_restatement_brings_a_fresh_part_to_the_same_state() {
    let mut parts = Parts::new();
    for (number, value) in [(1, 30), (11, 50), (64, 100), (74, 3), (7, 0)] {
        cc(&mut parts, 1, number, value);
    }
    parts.take(&Message::Channel(ChannelMessage::new(status::CHANNEL_PRESSURE, 1, 9, 0)), None);
    let mut fresh = Parts::new();
    let first = restatement(&parts, 1);
    parts.part(1).unwrap().restate(1, &mut |message| fresh.take(&message, None));
    assert_eq!(restatement(&fresh, 1), first);
    cc(&mut parts, 1, 121, 0);
    assert_eq!(restatement(&parts, 1), vec![vec![0xB1, 74, 3], vec![0xB1, 7, 0]]);
}
