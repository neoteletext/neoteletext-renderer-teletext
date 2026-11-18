//! synth6581's and synth8580's adopted interface, ASID: register writes
//! carried in system exclusive messages (`neoteletext-common/docs/audio/
//! synth6581-synth8580.md`, cited here as "S6581-n"; DEC-6453, DEC-6460).
//!
//! This is the one reading and writing of ASID that the receiver and the
//! server share (MIDI-21, SRV-88): the receiver's front end
//! ([`super::midi::Midi`]) plays what [`read`] takes, and the server checks
//! a program's messages with [`accepts`], packs a patch with
//! [`pack_patch`], checks one with [`check_patch`], bounds a block with
//! [`BlockBudget`] and writes a live sound's restatement from a
//! [`Tracker`]. It holds no synthesis, so it is built under the feature
//! `interface` without the chip (SRV-88), and it is `no_std`,
//! integer-only and allocation-free, reading untrusted bytes.
//!
//! The family has no bank (S6581-51) and no note layer (S6581-52, DEC-6465
//! answer 8): ASID's register data message is the only thing that writes a
//! register (S6581-2).

use crate::midi::{DATA_MAX, MOST_QUANTITY_BYTES, Message, status};

/// The ASID identifier, the first byte after `0xF0`, in the place of a
/// manufacturer identifier (S6581-4; `thomasj-asid-protocol.md`, "Original
/// ASID protocol": "the number chosen was 45").
pub const IDENTIFIER: u8 = 0x2D;

/// ASID's commands (S6581-9; `thomasj-asid-protocol.md`, every section).
pub mod command {
    /// Start playback: accepted, no effect (S6581-18).
    pub const START: u8 = 0x4C;
    /// Stop playback: accepted, no effect (S6581-18).
    pub const STOP: u8 = 0x4D;
    /// SID register data: taken (S6581-10 to S6581-17).
    pub const REGISTER_DATA: u8 = 0x4E;
    /// Display characters: not taken (S6581-19).
    pub const DISPLAY: u8 = 0x4F;
    /// Register data of the second chip, the first of sixteen further
    /// chips: not taken (S6581-20).
    pub const FIRST_FURTHER_CHIP: u8 = 0x50;
    /// Register data of the seventeenth chip: not taken (S6581-20).
    pub const LAST_FURTHER_CHIP: u8 = 0x5F;
    /// OPL-FM register data: not taken (S6581-21).
    pub const OPL_FM: u8 = 0x60;
    /// SID write order and timing: not taken (S6581-22).
    pub const WRITE_ORDER: u8 = 0x30;
    /// Speed settings: not taken (S6581-23).
    pub const SPEED: u8 = 0x31;
    /// SID type information: not taken (S6581-24).
    pub const CHIP_TYPE: u8 = 0x32;
}

/// The chip's writable registers, at addresses 0x00 to 0x18 (S6581-27).
pub const REGISTERS: usize = 25;
/// ASID register IDs, 0 to 27 (S6581-11).
pub const REGISTER_IDS: usize = 28;
/// The IDs a full register update carries, 0 to 24: every register once
/// (S6581-11, "Full register update"; S6581-50).
pub const FULL_UPDATE_IDS: usize = 25;
/// The mask bytes of a register data message, and as many
/// most-significant-bit bytes (S6581-10).
pub const MASK_BYTES: usize = 4;
/// The IDs one mask byte covers, its bits 0 to 6 (S6581-11: "The first mask
/// byte (bits 6-0) covers registers 0-6").
const IDS_PER_MASK_BYTE: usize = 7;
const _: () = assert!(MASK_BYTES * IDS_PER_MASK_BYTE == REGISTER_IDS);
/// The payload of a register data message before its data bytes: the mask
/// and most-significant-bit bytes (S6581-10).
pub const REGISTER_DATA_FIXED: usize = 2 * MASK_BYTES;
/// The bit of a register's value that a most-significant-bit byte carries
/// (S6581-12: "that bit times 128").
const HIGH_BIT: u8 = 0x80;

/// The register address each ASID register ID writes, by ID (S6581-11's
/// table): IDs 0 to 21 the non-control registers in address order, 22 to
/// 24 the control registers of voices 1 to 3, and 25 to 27 a second write
/// to the same three.
pub const ADDRESS_OF_ID: [u8; REGISTER_IDS] = [
    0x00, 0x01, 0x02, 0x03, 0x05, 0x06, // voice 1
    0x07, 0x08, 0x09, 0x0A, 0x0C, 0x0D, // voice 2
    0x0E, 0x0F, 0x10, 0x11, 0x13, 0x14, // voice 3
    0x15, 0x16, 0x17, 0x18, // filter and volume
    0x04, 0x0B, 0x12, // control
    0x04, 0x0B, 0x12, // control, second write
];

/// The mask bytes of a full register update (S6581-11: "0x7F, 0x7F, 0x7F,
/// 0x0F").
pub const FULL_UPDATE_MASK: [u8; MASK_BYTES] = [0x7F, 0x7F, 0x7F, 0x0F];

/// The bytes of a message in MIDI form beside its payload: `0xF0`, the
/// identifier, the command and `0xF7` (S6581-4).
const FRAME_BYTES: usize = 4;
/// The longest register data message in MIDI form, all 28 IDs: 40 bytes
/// (S6581-10).
pub const MOST_MESSAGE_BYTES: usize = FRAME_BYTES + REGISTER_DATA_FIXED + REGISTER_IDS;
/// A patch, one full register update in MIDI form: 37 bytes (S6581-50).
pub const PATCH_BYTES: usize = FRAME_BYTES + REGISTER_DATA_FIXED + FULL_UPDATE_IDS;
/// A start or stop message in MIDI form: 4 bytes (S6581-18).
pub const START_STOP_BYTES: usize = FRAME_BYTES;
const _: () = assert!(MOST_MESSAGE_BYTES == 40 && PATCH_BYTES == 37);

/// The most register data messages a block holds (S6581-42, DEC-6465
/// answer to the family's open question 5): one update a millisecond, the
/// highest speed command 0x31 names, 16 a frame at 50 frames a second, with
/// a quarter more.
pub const MOST_REGISTER_DATA_A_BLOCK: usize = 20;
/// The most start messages a block holds, and the most stop messages
/// (S6581-42).
pub const MOST_STARTS_A_BLOCK: usize = 1;

/// The block-form bytes of a message of `midi_form` bytes, with the longest
/// delta time: the delta, `0xF0`, a one-byte length and the bytes after
/// `0xF0` (S6581-6).
const fn longest_event(midi_form: usize) -> usize {
    MOST_QUANTITY_BYTES.saturating_add(1).saturating_add(midi_form)
}
/// The most bytes a block of this family needs: 874 (S6581-43), 20 register
/// data messages of all 28 IDs and one start and one stop message, each at
/// the longest delta time.
pub const MOST_BLOCK_BYTES: usize = MOST_REGISTER_DATA_A_BLOCK * longest_event(MOST_MESSAGE_BYTES)
    + 2 * MOST_STARTS_A_BLOCK * longest_event(START_STOP_BYTES);
const _: () = assert!(MOST_BLOCK_BYTES == 874);
// Within the architecture's bound for every event codec (MIDI-30), and every
// message's length after 0xF0 is one byte of variable-length quantity
// (S6581-6: "at most 39 and so is one byte").
const _: () = assert!(MOST_BLOCK_BYTES <= crate::midi::MOST_BLOCK_BYTES);
const _: () = assert!(MOST_MESSAGE_BYTES - 1 <= DATA_MAX as usize);

/// Registers of one voice, the control register fourth of them, and the
/// address of the first filter register (S6581-27).
pub(super) const VOICE_REGISTERS: usize = 7;
/// See [`VOICE_REGISTERS`].
const CONTROL_OFFSET: usize = 4;
/// See [`VOICE_REGISTERS`].
pub(super) const FIRST_FILTER_REGISTER: usize = 3 * VOICE_REGISTERS;

/// S6581-27 and S6581-37: the 28 IDs reach exactly the addresses 0x00 to
/// 0x18, each at least once, the three control addresses twice, and none
/// above.
#[allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "evaluated only at compile time, where a fault is a build error"
)]
const fn ids_reach_every_register() -> bool {
    let mut reached = [0_u8; REGISTERS];
    let mut id = 0;
    while id < REGISTER_IDS {
        let address = ADDRESS_OF_ID[id] as usize;
        if address >= REGISTERS {
            return false;
        }
        reached[address] += 1;
        id += 1;
    }
    let mut address = 0;
    while address < REGISTERS {
        let control = address % VOICE_REGISTERS == CONTROL_OFFSET && address < FIRST_FILTER_REGISTER;
        if reached[address] != if control { 2 } else { 1 } {
            return false;
        }
        address += 1;
    }
    true
}
const _: () = assert!(ids_reach_every_register());

/// Why a message is malformed: dropped whole and counted (S6581-8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Malformed {
    /// Not a whole system exclusive message: no `0xF7` at its end, or a
    /// byte of 0x80 or above before it (S6581-5, S6581-6).
    NotWhole,
    /// A start or stop message with a payload (S6581-18).
    Payload,
    /// A register data message shorter than its eight mask and
    /// most-significant-bit bytes (S6581-14).
    Short,
    /// A register data message whose data bytes are not as many as its
    /// mask's bits (S6581-14).
    Count,
    /// A most-significant bit set for an ID absent from the mask: ignored
    /// by the receiver, refused by the server (S6581-12, S6581-54 item 5).
    StrayHighBit,
}

/// Why a system exclusive message is not one this family acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// Not a system exclusive message (S6581-26), or one whose first byte
    /// after `0xF0` is not [`IDENTIFIER`] (S6581-7).
    NotAsid,
    /// An ASID command the family does not take, which is given
    /// (S6581-9, S6581-19 to S6581-25).
    NotTaken(u8),
    /// A malformed ASID message (S6581-8).
    Malformed(Malformed),
    /// A patch that is a register data message but not a full register
    /// update (S6581-50).
    NotFullUpdate,
}

/// One register data message: which IDs it carries and their values
/// (S6581-10 to S6581-13).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisterData {
    /// Bit `id` set for each ID present.
    present: u32,
    /// The value of each ID present, by ID; 0 for an absent one.
    values: [u8; REGISTER_IDS],
}

impl Default for RegisterData {
    fn default() -> Self {
        Self::empty()
    }
}

impl RegisterData {
    /// The message with no register (S6581-16).
    #[must_use]
    pub const fn empty() -> Self {
        Self { present: 0, values: [0; REGISTER_IDS] }
    }

    /// The message carrying each ID that `values` gives, as the server's
    /// named fields give them in ID order (SRV-12).
    #[must_use]
    pub fn from_values(values: &[Option<u8>; REGISTER_IDS]) -> Self {
        let mut data = Self::empty();
        for (id, value) in (0_u8..).zip(values) {
            if let Some(value) = value {
                data.set(id, *value);
            }
        }
        data
    }

    /// A full register update (S6581-50) of `values`, by ID 0 to 24.
    #[must_use]
    pub fn full(values: &[u8; FULL_UPDATE_IDS]) -> Self {
        let mut data = Self::empty();
        for (id, &value) in (0_u8..).zip(values) {
            data.set(id, value);
        }
        data
    }

    /// Carries `value` for ID `id`; an ID of [`REGISTER_IDS`] or above is
    /// ignored. Any value is valid (S6581-17).
    pub fn set(&mut self, id: u8, value: u8) {
        if let Some(place) = self.values.get_mut(usize::from(id)) {
            *place = value;
            self.present |= 1 << id;
        }
    }

    /// The value carried for ID `id`, or `None` where it is absent.
    #[must_use]
    pub fn get(&self, id: u8) -> Option<u8> {
        if self.has(id) { self.values.get(usize::from(id)).copied() } else { None }
    }

    /// Whether ID `id` is present.
    #[must_use]
    pub const fn has(&self, id: u8) -> bool {
        (id as usize) < REGISTER_IDS && self.present & (1 << id) != 0
    }

    /// How many IDs are present, 0 to 28: the data bytes the message has
    /// (S6581-14).
    #[must_use]
    pub const fn count(&self) -> usize {
        self.present.count_ones() as usize
    }

    /// Whether the message is a full register update: IDs 0 to 24, each
    /// once, and no other (S6581-11's terms, S6581-45, S6581-50).
    #[must_use]
    pub const fn is_full_update(&self) -> bool {
        self.present == (1 << FULL_UPDATE_IDS) - 1
    }

    /// The writes the message makes, as (address, value), in ascending
    /// order of ID (S6581-13, S6581-15): every non-control register, then
    /// the control registers, then their second writes.
    pub fn writes(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        (0_u8..)
            .zip(ADDRESS_OF_ID.iter().zip(&self.values))
            .filter(|(id, _)| self.has(*id))
            .map(|(_, (&address, &value))| (address, value))
    }

    /// Writes the message in MIDI form, `F0 2D 4E`, the mask bytes, the
    /// most-significant-bit bytes, the data bytes and `F7` (S6581-4,
    /// S6581-10 to S6581-13), into `out`; gives its length, 12 to 40. The
    /// most-significant bit of every absent ID is 0 (S6581-12).
    pub fn write_midi(&self, out: &mut [u8; MOST_MESSAGE_BYTES]) -> usize {
        let mut mask = [0_u8; MASK_BYTES];
        let mut high = [0_u8; MASK_BYTES];
        for (id, &value) in self.values.iter().enumerate() {
            let byte = id / IDS_PER_MASK_BYTE;
            let bit = 1_u8 << (id % IDS_PER_MASK_BYTE);
            if self.present & (1 << id) != 0 {
                if let Some(place) = mask.get_mut(byte) {
                    *place |= bit;
                }
                if value & HIGH_BIT != 0
                    && let Some(place) = high.get_mut(byte)
                {
                    *place |= bit;
                }
            }
        }
        let mut at = 0;
        let mut put = |byte: u8| {
            if let Some(place) = out.get_mut(at) {
                *place = byte;
            }
            at = at.saturating_add(1);
        };
        put(status::SYSTEM_EXCLUSIVE);
        put(IDENTIFIER);
        put(command::REGISTER_DATA);
        for byte in mask.into_iter().chain(high) {
            put(byte);
        }
        for (_, value) in self.writes_by_id() {
            put(value & DATA_MAX);
        }
        put(status::END_OF_EXCLUSIVE);
        at.min(MOST_MESSAGE_BYTES)
    }

    /// The values present, as (ID, value), in ascending ID.
    fn writes_by_id(&self) -> impl Iterator<Item = (u8, u8)> + '_ {
        (0_u8..).zip(self.values).filter(|(id, _)| self.has(*id))
    }
}

/// One ASID message the family acts on (S6581-9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asid {
    /// Start playback: no effect (S6581-18).
    Start,
    /// Stop playback: no effect (S6581-18).
    Stop,
    /// Register data (S6581-10).
    RegisterData(RegisterData),
}

/// Reads a system exclusive message as its block form carries it, every
/// byte after `0xF0` (`crate::midi::Message::SystemExclusive`), as the
/// receiver plays it: the most-significant bit of an absent ID is ignored
/// (S6581-12).
///
/// # Errors
/// [`Fault::NotAsid`] for a message whose first byte is not
/// [`IDENTIFIER`]; [`Fault::NotTaken`] for a command other than 0x4C, 0x4D
/// and 0x4E; [`Fault::Malformed`] for a message breaking S6581-5, S6581-6,
/// S6581-14 or S6581-18.
pub fn read(after_start: &[u8]) -> Result<Asid, Fault> {
    read_strictly(after_start, false)
}

/// [`read`] for a message in MIDI form, `F0` first (S6581-4), as a program
/// gives it to the server (SRV-12's `asid` bytes).
///
/// # Errors
/// As [`read`]; and [`Fault::NotAsid`] for bytes not beginning `0xF0`.
pub fn read_midi(bytes: &[u8]) -> Result<Asid, Fault> {
    match bytes.split_first() {
        Some((&status::SYSTEM_EXCLUSIVE, after_start)) => read(after_start),
        _ => Err(Fault::NotAsid),
    }
}

/// Whether the server takes `message` from a program for these kinds, as
/// [`read`] reads it and also refusing a most-significant bit set for an
/// absent ID (S6581-54 items 1 to 5, SRV-24).
///
/// # Errors
/// [`Fault::NotAsid`] for a channel message or a system exclusive message
/// that is not ASID (S6581-7, S6581-26); otherwise as [`read`], with
/// [`Malformed::StrayHighBit`] for S6581-12.
pub fn accepts(message: &Message<'_>) -> Result<Asid, Fault> {
    match message {
        Message::SystemExclusive(after_start) => read_strictly(after_start, true),
        Message::Channel(_) => Err(Fault::NotAsid),
    }
}

/// Reads one message, refusing a stray most-significant bit when `strict`.
fn read_strictly(after_start: &[u8], strict: bool) -> Result<Asid, Fault> {
    let malformed = Err(Fault::Malformed(Malformed::NotWhole));
    let Some((&end, inner)) = after_start.split_last() else { return malformed };
    if end != status::END_OF_EXCLUSIVE || inner.iter().any(|&byte| byte > DATA_MAX) {
        return malformed;
    }
    let Some((&identifier, rest)) = inner.split_first() else { return malformed };
    if identifier != IDENTIFIER {
        return Err(Fault::NotAsid);
    }
    let Some((&command, payload)) = rest.split_first() else { return malformed };
    match command {
        command::START | command::STOP if !payload.is_empty() => Err(Fault::Malformed(Malformed::Payload)),
        command::START => Ok(Asid::Start),
        command::STOP => Ok(Asid::Stop),
        command::REGISTER_DATA => register_data(payload, strict).map(Asid::RegisterData),
        other => Err(Fault::NotTaken(other)),
    }
}

/// Reads a register data message's payload (S6581-10 to S6581-14).
fn register_data(payload: &[u8], strict: bool) -> Result<RegisterData, Fault> {
    let (Some(mask), Some(high), Some(data)) =
        (payload.get(..MASK_BYTES), payload.get(MASK_BYTES..REGISTER_DATA_FIXED), payload.get(REGISTER_DATA_FIXED..))
    else {
        return Err(Fault::Malformed(Malformed::Short));
    };
    let bits = |bytes: &[u8]| {
        bytes.iter().enumerate().fold(0_u32, |bits, (index, &byte)| {
            bits | u32::from(byte & DATA_MAX)
                .checked_shl(u32::try_from(index.saturating_mul(IDS_PER_MASK_BYTE)).unwrap_or(u32::MAX))
                .unwrap_or(0)
        })
    };
    let present = bits(mask);
    let high = bits(high);
    if present.count_ones() as usize != data.len() {
        return Err(Fault::Malformed(Malformed::Count));
    }
    if strict && high & !present != 0 {
        return Err(Fault::Malformed(Malformed::StrayHighBit));
    }
    let mut message = RegisterData::empty();
    let mut data = data.iter();
    for id in 0..REGISTER_IDS {
        if present & (1 << id) != 0
            && let Some(&low) = data.next()
        {
            let value = if high & (1 << id) != 0 { low | HIGH_BIT } else { low };
            message.set(u8::try_from(id).unwrap_or(u8::MAX), value);
        }
    }
    Ok(message)
}

/// What a block may still hold: the receiver applies, and the server
/// writes, at most [`MOST_REGISTER_DATA_A_BLOCK`] register data messages
/// and one start and one stop message a block (S6581-42).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BlockBudget {
    register_data: usize,
    starts: usize,
    stops: usize,
}

impl BlockBudget {
    /// An empty block's.
    #[must_use]
    pub const fn new() -> Self {
        Self { register_data: 0, starts: 0, stops: 0 }
    }

    /// Takes `message` into the block if it fits there, answering whether
    /// it did; one that does not is passed over and counted by the
    /// receiver, and refused by the server with `block_full`.
    pub fn admit(&mut self, message: &Asid) -> bool {
        let (count, most) = match message {
            Asid::Start => (&mut self.starts, MOST_STARTS_A_BLOCK),
            Asid::Stop => (&mut self.stops, MOST_STARTS_A_BLOCK),
            Asid::RegisterData(_) => (&mut self.register_data, MOST_REGISTER_DATA_A_BLOCK),
        };
        if *count >= most {
            return false;
        }
        *count = count.saturating_add(1);
        true
    }

    /// Register data messages taken so far.
    #[must_use]
    pub const fn register_data(&self) -> usize {
        self.register_data
    }
}

/// A patch: one full register update's 25 values, by ASID ID 0 to 24, so
/// the 22 non-control registers in address order and then the control
/// registers of voices 1, 2 and 3 (S6581-50). The same for both chips.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Patch {
    /// The values, by ID.
    pub values: [u8; FULL_UPDATE_IDS],
}

/// Packs a patch into its one form, the full register update in MIDI form,
/// 37 bytes: `F0 2D 4E 7F 7F 7F 0F`, the most-significant-bit bytes, the 25
/// data bytes and `F7` (S6581-50, SRV-55).
#[must_use]
pub fn pack_patch(patch: &Patch) -> [u8; PATCH_BYTES] {
    let mut message = [0; MOST_MESSAGE_BYTES];
    let length = RegisterData::full(&patch.values).write_midi(&mut message);
    let mut packed = [0; PATCH_BYTES];
    if let Some(bytes) = message.get(..length) {
        for (to, from) in packed.iter_mut().zip(bytes) {
            *to = *from;
        }
    }
    packed
}

/// Checks bytes in MIDI form as a patch, giving its values (S6581-50).
///
/// # Errors
/// As [`accepts`]; [`Fault::NotFullUpdate`] for a start or stop message or
/// a register data message that is not a full register update.
pub fn check_patch(bytes: &[u8]) -> Result<Patch, Fault> {
    let Some((&status::SYSTEM_EXCLUSIVE, after_start)) = bytes.split_first() else { return Err(Fault::NotAsid) };
    match accepts(&Message::SystemExclusive(after_start))? {
        Asid::RegisterData(data) if data.is_full_update() => {
            let mut patch = Patch::default();
            for (id, value) in (0_u8..).zip(&mut patch.values) {
                *value = data.get(id).unwrap_or(0);
            }
            Ok(patch)
        }
        Asid::RegisterData(_) | Asid::Start | Asid::Stop => Err(Fault::NotFullUpdate),
    }
}

/// The registers of a sound as its sender writes them, for the server
/// (MIDI-94, S6581-46, SRV-68): it is given every message written, in
/// order, and gives the family's restatement, a full register update of
/// the values the 25 registers hold (MIDI-93).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tracker {
    /// By address.
    registers: [u8; REGISTERS],
    opened: bool,
}

impl Tracker {
    /// A sound's before its first message: every register zero, as a chip
    /// is at the start of a sound (S6581-36).
    #[must_use]
    pub const fn new() -> Self {
        Self { registers: [0; REGISTERS], opened: false }
    }

    /// Takes one message written into the sound.
    pub fn take(&mut self, message: &Asid) {
        if let Asid::RegisterData(data) = message {
            if data.is_full_update() {
                self.opened = true;
            }
            for (address, value) in data.writes() {
                if let Some(place) = self.registers.get_mut(usize::from(address)) {
                    *place = value;
                }
            }
        }
    }

    /// Whether a full register update has been taken, which a sound's first
    /// update must be (S6581-45, S6581-46).
    #[must_use]
    pub const fn opened(&self) -> bool {
        self.opened
    }

    /// The registers, by address.
    #[must_use]
    pub const fn registers(&self) -> &[u8; REGISTERS] {
        &self.registers
    }

    /// The restatement: a full register update of every register's present
    /// value (S6581-46), which changes nothing in a receiver that missed
    /// nothing (S6581-47).
    #[must_use]
    pub fn restatement(&self) -> RegisterData {
        let mut values = [0; FULL_UPDATE_IDS];
        for (value, &address) in values.iter_mut().zip(&ADDRESS_OF_ID) {
            *value = self.registers.get(usize::from(address)).copied().unwrap_or(0);
        }
        RegisterData::full(&values)
    }

    /// Gives the restatement to `out` as the message a block carries,
    /// every byte after `0xF0` (SRV-69).
    pub fn restate(&self, out: &mut dyn FnMut(Message<'_>)) {
        let mut message = [0; MOST_MESSAGE_BYTES];
        let length = self.restatement().write_midi(&mut message);
        if let Some(after_start) = message.get(1..length) {
            out(Message::SystemExclusive(after_start));
        }
    }
}

// Named by its path so that it is found both where this module is a file
// of its own and where the feature `interface` alone reads it through a
// path from the crate root (lib.rs).
#[path = "interface/parameters.rs"]
mod parameters;
pub use parameters::{PARAMETERS, read_parameter};

// Named by its path so that it is found both where the module is a file of
// its own and where the feature `interface` alone reads it through a path
// from the crate root (lib.rs), whose `mod tests` would otherwise be looked
// for beside this file.
#[cfg(test)]
#[path = "interface/tests.rs"]
mod tests;
