#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::panic,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

extern crate std;
use std::vec;
use std::vec::Vec;

use super::{
    ADDRESS_OF_ID, Asid, BlockBudget, FULL_UPDATE_MASK, Fault, MOST_MESSAGE_BYTES, MOST_REGISTER_DATA_A_BLOCK,
    Malformed, PATCH_BYTES, Patch, REGISTER_IDS, RegisterData, Tracker, accepts, check_patch, command, pack_patch,
    read, read_midi,
};
use crate::midi::{ChannelMessage, Message, status};

/// The MIDI form of `data`.
fn midi(data: &RegisterData) -> Vec<u8> {
    let mut out = [0; MOST_MESSAGE_BYTES];
    let length = data.write_midi(&mut out);
    out[..length].to_vec()
}

/// A register data message written from S6581-10 to S6581-13 directly, the
/// reference the encoder is compared against: per ID, mask byte `id ÷ 7`,
/// bit `id mod 7`.
fn reference(values: &[Option<u8>; REGISTER_IDS]) -> Vec<u8> {
    let mut mask = [0_u8; 4];
    let mut high = [0_u8; 4];
    let mut data = Vec::new();
    for (id, value) in values.iter().enumerate() {
        if let Some(value) = value {
            mask[id / 7] |= 1 << (id % 7);
            if value & 0x80 != 0 {
                high[id / 7] |= 1 << (id % 7);
            }
            data.push(value & 0x7F);
        }
    }
    let mut out = vec![0xF0, 0x2D, 0x4E];
    out.extend_from_slice(&mask);
    out.extend_from_slice(&high);
    out.extend_from_slice(&data);
    out.push(0xF7);
    out
}

/// xorshift64 (Marsaglia, "Xorshift RNGs", 2003, the triple 13, 7, 17).
struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

// Verifies S6581-17's worked examples: voice 1's frequency 0x1D45 and
// control 0x41, and its sustain and release 0xA4.
#[test]
fn s6581_17_the_worked_examples() {
    let mut data = RegisterData::empty();
    data.set(0, 0x45);
    data.set(1, 0x1D);
    data.set(22, 0x41);
    assert_eq!(midi(&data), [0xF0, 0x2D, 0x4E, 0x03, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x45, 0x1D, 0x41, 0xF7]);
    let mut data = RegisterData::empty();
    data.set(5, 0xA4);
    assert_eq!(midi(&data), [0xF0, 0x2D, 0x4E, 0x20, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x24, 0xF7]);
    let Ok(Asid::RegisterData(read)) = read_midi(&midi(&data)) else { panic!("read") };
    assert_eq!(read.writes().collect::<Vec<_>>(), [(0x06, 0xA4)]);
}

// Verifies S6581-4 and S6581-18: the framing of each command taken, and the
// start and stop messages' four bytes.
#[test]
fn s6581_4_the_framing() {
    assert_eq!(read_midi(&[0xF0, 0x2D, 0x4C, 0xF7]), Ok(Asid::Start));
    assert_eq!(read_midi(&[0xF0, 0x2D, 0x4D, 0xF7]), Ok(Asid::Stop));
    assert_eq!(read_midi(&[0xF0, 0x00, 0x2D, 0x4C, 0xF7]), Err(Fault::NotAsid));
    assert_eq!(read_midi(&[0x2D, 0x4C, 0xF7]), Err(Fault::NotAsid));
    assert_eq!(read(&[0x2D, 0x4C, 0x00, 0xF7]), Err(Fault::Malformed(Malformed::Payload)));
    assert_eq!(read(&[0x2D, 0x4D, 0x01, 0x02, 0xF7]), Err(Fault::Malformed(Malformed::Payload)));
}

// Verifies S6581-5: a byte of 0x80 or above anywhere between the start and
// the end byte makes the message malformed.
#[test]
fn s6581_5_every_byte_is_seven_bits() {
    let mut data = RegisterData::empty();
    data.set(0, 0x12);
    data.set(27, 0x34);
    let message = midi(&data);
    for at in 1..message.len() - 1 {
        let mut broken = message.clone();
        broken[at] = 0x80;
        assert_eq!(read(&broken[1..]), Err(Fault::Malformed(Malformed::NotWhole)), "byte {at}");
    }
    assert_eq!(read(&message[1..message.len() - 1]), Err(Fault::Malformed(Malformed::NotWhole)));
}

// Verifies S6581-9: of every command 0x00 to 0x7F only 0x4C, 0x4D and 0x4E
// are taken; every other is not, by its own value.
#[test]
fn s6581_9_the_commands() {
    for value in 0..=0x7F_u8 {
        let found = read(&[0x2D, value, 0xF7]);
        match value {
            command::START => assert_eq!(found, Ok(Asid::Start)),
            command::STOP => assert_eq!(found, Ok(Asid::Stop)),
            command::REGISTER_DATA => assert_eq!(found, Err(Fault::Malformed(Malformed::Short))),
            _ => assert_eq!(found, Err(Fault::NotTaken(value)), "command {value:#04X}"),
        }
    }
    // A further chip's register data, in 0x4E's own form, is not taken
    // (S6581-20).
    let mut further = midi(&RegisterData::full(&[0; 25]));
    for value in command::FIRST_FURTHER_CHIP..=command::LAST_FURTHER_CHIP {
        further[2] = value;
        assert_eq!(read_midi(&further), Err(Fault::NotTaken(value)));
    }
}

// Verifies S6581-10 and S6581-13: the encoder against the reference written
// from the requirement, for the empty mask, each single ID, the full
// register update, all 28 IDs and 10,000 seeded masks, and each read back.
#[test]
fn s6581_10_register_data_round_trips() {
    let mut cases: Vec<[Option<u8>; REGISTER_IDS]> = vec![[None; REGISTER_IDS]];
    for id in 0..REGISTER_IDS {
        for value in [0x00, 0x7F, 0x80, 0xFF] {
            let mut values = [None; REGISTER_IDS];
            values[id] = Some(value);
            cases.push(values);
        }
    }
    cases.push(core::array::from_fn(|id| (id < 25).then_some(id as u8 * 9)));
    cases.push(core::array::from_fn(|id| Some(0xFF - id as u8)));
    let mut random = Random(0x5EED_6581);
    for _ in 0..10_000 {
        let mask = random.next();
        cases.push(core::array::from_fn(|id| (mask >> id & 1 != 0).then(|| (random.next() >> 40) as u8)));
    }
    for values in &cases {
        let data = RegisterData::from_values(values);
        let bytes = midi(&data);
        assert_eq!(bytes, reference(values));
        assert_eq!(bytes.len(), 12 + values.iter().flatten().count());
        assert_eq!(read_midi(&bytes), Ok(Asid::RegisterData(data)));
        assert_eq!(accepts(&Message::SystemExclusive(&bytes[1..])), Ok(Asid::RegisterData(data)));
    }
}

// Verifies S6581-11 and S6581-27: each of the 28 IDs alone writes the
// address of the table, by mask byte `id ÷ 7` and bit `id mod 7`.
#[test]
fn s6581_11_each_id_writes_its_address() {
    let expected: [u8; REGISTER_IDS] = [
        0x00, 0x01, 0x02, 0x03, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x04, 0x0B, 0x12, 0x04, 0x0B, 0x12,
    ];
    assert_eq!(ADDRESS_OF_ID, expected);
    for id in 0..REGISTER_IDS {
        let mut message = vec![0x2D, 0x4E, 0, 0, 0, 0, 0, 0, 0, 0, 0x55, 0xF7];
        message[2 + id / 7] = 1 << (id % 7);
        let Ok(Asid::RegisterData(data)) = read(&message) else { panic!("id {id}") };
        assert_eq!(data.writes().collect::<Vec<_>>(), [(expected[id], 0x55)]);
    }
    assert_eq!(midi(&RegisterData::full(&[0; 25]))[3..7], FULL_UPDATE_MASK);
}

// Verifies S6581-12: the eighth bit from the most-significant-bit bytes;
// one set for an absent ID is ignored by the receiver and refused by the
// server.
#[test]
fn s6581_12_most_significant_bits() {
    // ID 3 present with 0x05, and the high bit of absent ID 4 set.
    let message = [0x2D, 0x4E, 0x08, 0, 0, 0, 0x18, 0, 0, 0, 0x05, 0xF7];
    let Ok(Asid::RegisterData(data)) = read(&message) else { panic!("read") };
    assert_eq!(data.writes().collect::<Vec<_>>(), [(0x03, 0x85)]);
    assert_eq!(accepts(&Message::SystemExclusive(&message)), Err(Fault::Malformed(Malformed::StrayHighBit)));
}

// Verifies S6581-14: a payload shorter than eight bytes, and one data byte
// missing or extra for five masks, are malformed.
#[test]
fn s6581_14_the_count_of_data_bytes() {
    for length in 0..8 {
        let mut message = vec![0x2D, 0x4E];
        message.extend(core::iter::repeat_n(0, length));
        message.push(0xF7);
        assert_eq!(read(&message), Err(Fault::Malformed(Malformed::Short)), "payload {length}");
    }
    let masks: [[u8; 4]; 5] = [[1, 0, 0, 0], [0x7F, 0, 0, 0], [0, 0x40, 0x01, 0], FULL_UPDATE_MASK, [0x7F; 4]];
    for mask in masks {
        let bits = mask.iter().map(|byte| byte.count_ones() as usize).sum::<usize>();
        for data in [bits - 1, bits + 1] {
            let mut message = vec![0x2D, 0x4E];
            message.extend_from_slice(&mask);
            message.extend_from_slice(&[0; 4]);
            message.extend(core::iter::repeat_n(0x11, data));
            message.push(0xF7);
            assert_eq!(read(&message), Err(Fault::Malformed(Malformed::Count)));
        }
    }
}

// Verifies S6581-15: the writes in ascending ID, so the non-control
// registers, then the control registers, then their second writes.
#[test]
fn s6581_15_the_order_of_writes() {
    let data = RegisterData::from_values(&core::array::from_fn(|id| Some(id as u8)));
    let writes: Vec<_> = data.writes().collect();
    let expected: Vec<_> = ADDRESS_OF_ID.iter().zip(0..).map(|(&address, value)| (address, value)).collect();
    assert_eq!(writes, expected);
}

// Verifies S6581-16: the empty update is valid and writes nothing.
#[test]
fn s6581_16_the_empty_update() {
    let Ok(Asid::RegisterData(data)) = read(&[0x2D, 0x4E, 0, 0, 0, 0, 0, 0, 0, 0, 0xF7]) else { panic!("read") };
    assert_eq!(data.writes().count(), 0);
    assert_eq!(midi(&RegisterData::empty()).len(), 12);
}

// Verifies S6581-42: twenty register data messages, one start and one stop
// a block.
#[test]
fn s6581_42_the_block_budget() {
    let mut budget = BlockBudget::new();
    let data = Asid::RegisterData(RegisterData::empty());
    for _ in 0..MOST_REGISTER_DATA_A_BLOCK {
        assert!(budget.admit(&data));
    }
    assert!(!budget.admit(&data));
    assert!(budget.admit(&Asid::Start));
    assert!(!budget.admit(&Asid::Start));
    assert!(budget.admit(&Asid::Stop));
    assert!(!budget.admit(&Asid::Stop));
    assert_eq!(budget.register_data(), MOST_REGISTER_DATA_A_BLOCK);
}

// Verifies S6581-50: a patch is 37 bytes, begins `F0 2D 4E 7F 7F 7F 0F`
// and decodes to its 25 values; a partial update, or one with a second
// control write, is no patch.
#[test]
fn s6581_50_a_patch_is_a_full_register_update() {
    let patch = Patch { values: core::array::from_fn(|id| 0x80 + id as u8 * 3) };
    let packed = pack_patch(&patch);
    assert_eq!(packed.len(), PATCH_BYTES);
    assert_eq!(packed[..7], [0xF0, 0x2D, 0x4E, 0x7F, 0x7F, 0x7F, 0x0F]);
    assert_eq!(packed[36], 0xF7);
    assert_eq!(check_patch(&packed), Ok(patch));
    let mut partial = RegisterData::full(&patch.values);
    partial.set(25, 1);
    assert_eq!(check_patch(&midi(&partial)), Err(Fault::NotFullUpdate));
    assert_eq!(check_patch(&[0xF0, 0x2D, 0x4C, 0xF7]), Err(Fault::NotFullUpdate));
    assert_eq!(check_patch(&packed[1..]), Err(Fault::NotAsid));
}

// Verifies S6581-26: a channel message is not ASID to the server.
#[test]
fn s6581_26_channel_messages_are_not_taken() {
    let note = Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, 60, 100));
    assert_eq!(accepts(&note), Err(Fault::NotAsid));
}

// Verifies S6581-46 and MIDI-93: the tracker's restatement is a full
// register update of every register's latest value, the second control
// write the one kept.
#[test]
fn s6581_46_the_restatement_carries_the_registers() {
    let mut tracker = Tracker::new();
    assert!(!tracker.opened());
    tracker.take(&Asid::RegisterData(RegisterData::full(&core::array::from_fn(|id| id as u8))));
    assert!(tracker.opened());
    let mut later = RegisterData::empty();
    later.set(19, 0xC0);
    later.set(22, 0x40);
    later.set(25, 0x41);
    tracker.take(&Asid::RegisterData(later));
    tracker.take(&Asid::Stop);
    let restatement = tracker.restatement();
    assert!(restatement.is_full_update());
    assert_eq!(restatement.get(19), Some(0xC0));
    assert_eq!(restatement.get(22), Some(0x41));
    assert_eq!(restatement.get(0), Some(0));
    assert_eq!(restatement.get(21), Some(21));
    let mut written = Vec::new();
    tracker.restate(&mut |message| {
        if let Message::SystemExclusive(bytes) = message {
            written.extend_from_slice(bytes);
        }
    });
    assert_eq!(written, midi(&restatement)[1..]);
    assert_eq!(read(&written), Ok(Asid::RegisterData(restatement)));
}
