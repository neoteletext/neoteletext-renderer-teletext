//! synth7's three system exclusive messages (`synth7.md`, S7-37 to
//! S7-45): the single-voice dump of 163 bytes, the 32-voice dump of 4,104
//! bytes, and the parameter change of 7 bytes, each read whole and checked
//! before anything it carries is taken, and each written as the adopted
//! interface writes it.
//!
//! Every reader here takes the whole message, `F0` to `F7`; a block's
//! system exclusive event holds the bytes after its `F0`, which
//! [`classify_event`] and the `*_event` readers take.
//!
//! Source: DX7 manual, page 30 ("1-2-2. Bulk data of 1 voice", "1-2-3. Bulk
//! data of 32 voices", "1-2-4. Parameter change") and page 31 (the groups);
//! TX7 manual, printed pages 35 and 36 (section 2-5, the same messages
//! drawn bit by bit, and the parameter change's sub-group bits) and 41
//! (section 4-5, the single-voice dump's data in parameter order).

use super::functions::{FIRST_FUNCTION_PARAMETER, FUNCTION_MAX, LAST_FUNCTION_PARAMETER};
use super::patch::{
    BANK_VOICES, LAST_VOICE_PARAMETER, PACKED_VOICE_BYTES, PARAMETER_MAX, Patch, PatchError, SYSEX_BYTES,
    VOICE_PARAMETERS,
};

/// The status byte that starts a system exclusive message.
pub const START: u8 = 0xF0;
/// EOX, the byte that ends one.
pub const END: u8 = 0xF7;
/// The maker's identification number, 67.
///
/// Source: DX7 manual, page 30: "Identification No. (i=67 ; YAMAHA)".
pub const IDENTIFICATION: u8 = 0x43;
/// The sub-status of a dump, s = 0, in the third byte's high nibble.
pub const SUB_STATUS_DUMP: u8 = 0x00;
/// The sub-status of a parameter change, s = 1.
pub const SUB_STATUS_PARAMETER: u8 = 0x10;
/// The third byte's high nibble: the sub-status.
pub const SUB_STATUS_MASK: u8 = 0xF0;
/// The third byte's low nibble: the channel, 0 to 15.
pub const CHANNEL_MASK: u8 = 0x0F;
/// The format number of a single-voice dump, f = 0.
pub const FORMAT_VOICE: u8 = 0;
/// The format number of a 32-voice dump, f = 9.
pub const FORMAT_BANK: u8 = 9;
/// A seven-bit data byte's mask; a byte above it has bit 7 set.
const DATA_MAX: u8 = 0x7F;
/// The bits of a MIDI byte above its seven data bits.
const BITS_PER_DATA_BYTE: u32 = 7;

/// The bytes of a dump before its data: `F0`, the identification, the
/// sub-status and channel, the format and the two bytes of the count.
pub const DUMP_HEADER_BYTES: usize = 6;
/// The bytes after a dump's data: the checksum and `F7`.
pub const DUMP_TRAILER_BYTES: usize = 2;
/// A single-voice dump, `F0` to `F7`: 163 bytes (S7-38).
pub const VOICE_DUMP_BYTES: usize = DUMP_HEADER_BYTES + VOICE_PARAMETERS + DUMP_TRAILER_BYTES;
/// A single-voice dump's count: 155 in two seven-bit bytes, `01 1B`.
///
/// Source: DX7 manual, page 30, "b=155 ; 1 voice"; TX7 manual, printed
/// page 35, `00000001` and `00011011`.
pub const VOICE_DUMP_COUNT: [u8; 2] = count(VOICE_PARAMETERS);
/// A 32-voice dump's count: 4,096 in two seven-bit bytes, `20 00`.
///
/// Source: DX7 manual, page 30, "b=4096 ; 32 voices".
pub const BANK_DUMP_COUNT: [u8; 2] = count(BANK_VOICES * PACKED_VOICE_BYTES);
/// A parameter change, `F0` to `F7`: 7 bytes (S7-43).
pub const PARAMETER_CHANGE_BYTES: usize = 7;
/// The group of the voice parameters 0 to 155, g = 0.
///
/// Source: DX7 manual, pages 30 and 31: "g=0 ; Voice parameter".
pub const GROUP_VOICE: u8 = 0;
/// The group of the function parameters 64 to 77, g = 2.
///
/// Source: DX7 manual, pages 30 and 31: "g=2 ; Function parameter".
pub const GROUP_FUNCTION: u8 = 2;
/// Byte 3's group is in bits 2 to 6; the parameter number's two high bits
/// in bits 0 and 1 (TX7 manual, printed page 36: "0ggggghh").
const GROUP_SHIFT: u32 = 2;
/// See [`GROUP_SHIFT`].
const HIGH_BITS_MASK: u8 = 0b11;

// The single-voice dump is one event of a block (MIDI-28: at most 256
// bytes after F0), and the 32-voice dump is the length the bank reads.
const _: () = assert!(VOICE_DUMP_BYTES - 1 <= crate::midi::MOST_SYSTEM_EXCLUSIVE_BYTES);
const _: () = assert!(DUMP_HEADER_BYTES + BANK_VOICES * PACKED_VOICE_BYTES + DUMP_TRAILER_BYTES == SYSEX_BYTES);

/// A count in two seven-bit bytes, most significant first.
#[expect(
    clippy::cast_possible_truncation,
    reason = "evaluated only at compile time, on counts of 155 and 4,096, which two seven-bit bytes hold"
)]
const fn count(value: usize) -> [u8; 2] {
    [((value >> BITS_PER_DATA_BYTE) as u8) & DATA_MAX, (value as u8) & DATA_MAX]
}

/// The checksum of a dump's data bytes: the two's complement of their sum,
/// in seven bits, so that the data and the checksum sum to a multiple of
/// 128 (S7-39; DX7 manual, page 30, blue text: "add 155th byte and make
/// the 2's complement").
#[must_use]
pub fn checksum(data: &[u8]) -> u8 {
    data.iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte)).wrapping_neg() & DATA_MAX
}

/// What a system exclusive message is to synth7 (S7-37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// It begins as a single-voice dump: `F0 43 0n 00`.
    VoiceDump,
    /// It begins as a 32-voice dump: `F0 43 0n 09`.
    BankDump,
    /// It begins as a parameter change: `F0 43 1n`.
    ParameterChange,
    /// Anything else, which synth7 does not take.
    Other,
}

/// What the bytes after a system exclusive event's `F0` begin as, with the
/// channel of its sub-status byte.
#[must_use]
pub fn classify_event(after_start: &[u8]) -> (Kind, u8) {
    let [IDENTIFICATION, sub_status, rest @ ..] = after_start else { return (Kind::Other, 0) };
    let channel = sub_status & CHANNEL_MASK;
    let kind = match (sub_status & SUB_STATUS_MASK, rest.first()) {
        (SUB_STATUS_DUMP, Some(&FORMAT_VOICE)) => Kind::VoiceDump,
        (SUB_STATUS_DUMP, Some(&FORMAT_BANK)) => Kind::BankDump,
        (SUB_STATUS_PARAMETER, _) => Kind::ParameterChange,
        _ => return (Kind::Other, 0),
    };
    (kind, channel)
}

/// Why a dump was refused (S7-45).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DumpError {
    /// The message is not the dump's exact length.
    Length,
    /// Its header is not the dump's: `F0 43 0n 00 01 1B` for one voice,
    /// `F0 43 0n 09 20 00` for 32.
    Header,
    /// A byte before the last has bit 7 set.
    DataByte {
        /// Its place, counted from `F0`.
        at: u16,
    },
    /// The checksum fails S7-39.
    Checksum,
    /// The last byte is not `F7`.
    Terminator,
    /// A voice is out of range (S7-29).
    Patch {
        /// Which voice: 0 for a single-voice dump, 0 to 31 in a 32-voice
        /// dump.
        index: u8,
        /// Why.
        error: PatchError,
    },
}

/// Checks a dump's frame: its length, header, data bytes, checksum and
/// `F7`, and gives its data bytes.
fn check_dump(message: &[u8], format: u8, count: [u8; 2], data_bytes: usize) -> Result<&[u8], DumpError> {
    if message.len() != DUMP_HEADER_BYTES.saturating_add(data_bytes).saturating_add(DUMP_TRAILER_BYTES) {
        return Err(DumpError::Length);
    }
    let Some((&[start, identification, sub_status, found_format, high, low], rest)) = message.split_first_chunk()
    else {
        return Err(DumpError::Length);
    };
    if start != START
        || identification != IDENTIFICATION
        || sub_status & SUB_STATUS_MASK != SUB_STATUS_DUMP
        || found_format != format
        || [high, low] != count
    {
        return Err(DumpError::Header);
    }
    let last = message.len().saturating_sub(1);
    if let Some(at) = message.iter().take(last).skip(1).position(|&byte| byte > DATA_MAX) {
        return Err(DumpError::DataByte { at: u16::try_from(at.saturating_add(1)).unwrap_or(u16::MAX) });
    }
    let Some((data, &[sum, end])) = rest.split_last_chunk() else { return Err(DumpError::Length) };
    if end != END {
        return Err(DumpError::Terminator);
    }
    if checksum(data) != sum {
        return Err(DumpError::Checksum);
    }
    Ok(data)
}

/// Reads a single-voice dump, `F0` to `F7` (S7-38, S7-45): its channel and
/// its voice, with every operator on.
///
/// # Errors
///
/// A [`DumpError`] naming the first fault found, in the order of the list.
pub fn read_voice_dump(message: &[u8]) -> Result<(u8, Patch), DumpError> {
    let data = check_dump(message, FORMAT_VOICE, VOICE_DUMP_COUNT, VOICE_PARAMETERS)?;
    let channel = message.get(2).map_or(0, |byte| byte & CHANNEL_MASK);
    let values: &[u8; VOICE_PARAMETERS] = data.try_into().map_err(|_| DumpError::Length)?;
    let patch = Patch::from_parameters(values).map_err(|error| DumpError::Patch { index: 0, error })?;
    Ok((channel, patch))
}

/// The single-voice dump of `patch` on `channel` (0 to 15; a larger one is
/// taken modulo 16): `F0 43 0n 00 01 1B`, the 155 voice parameters, the
/// checksum, `F7` (S7-38). The operator switch is in no dump.
#[must_use]
pub fn voice_dump(patch: &Patch, channel: u8) -> [u8; VOICE_DUMP_BYTES] {
    let [high, low] = VOICE_DUMP_COUNT;
    let mut message = [0_u8; VOICE_DUMP_BYTES];
    let values = patch.to_parameters();
    let header = [START, IDENTIFICATION, SUB_STATUS_DUMP | (channel & CHANNEL_MASK), FORMAT_VOICE, high, low];
    let trailer = [checksum(&values), END];
    for (place, byte) in message.iter_mut().zip(header.into_iter().chain(values).chain(trailer)) {
        *place = byte;
    }
    message
}

/// Checks a 32-voice dump's frame, `F0` to `F7` (S7-41, S7-45), and gives
/// its 4,096 data bytes: voice v is bytes `128 × v` to `128 × v + 127`.
///
/// # Errors
///
/// A [`DumpError`] naming the first fault found; the voices' ranges are not
/// checked here ([`bank_voice`]).
pub fn check_bank_dump(message: &[u8]) -> Result<&[u8], DumpError> {
    check_dump(message, FORMAT_BANK, BANK_DUMP_COUNT, BANK_VOICES.saturating_mul(PACKED_VOICE_BYTES))
}

/// Voice `index` (0 to 31) of a 32-voice dump whose frame
/// [`check_bank_dump`] passed, given as its data bytes.
///
/// # Errors
///
/// [`DumpError::Length`] for an index past 31, and [`DumpError::Patch`]
/// for a voice out of range (S7-29, S7-47).
pub fn bank_voice(data: &[u8], index: usize) -> Result<Patch, DumpError> {
    let (voices, _) = data.as_chunks::<PACKED_VOICE_BYTES>();
    let packed = voices.get(index).ok_or(DumpError::Length)?;
    Patch::from_packed(packed)
        .map_err(|error| DumpError::Patch { index: u8::try_from(index).unwrap_or(u8::MAX), error })
}

/// The 32-voice dump of `voices`, voice k at program k, on channel 0, as
/// the server writes one (S7-41; `server.md`, SRV-29).
#[must_use]
pub fn bank_dump(voices: &[Patch; BANK_VOICES]) -> [u8; SYSEX_BYTES] {
    let [high, low] = BANK_DUMP_COUNT;
    let mut message = [0_u8; SYSEX_BYTES];
    let header = [START, IDENTIFICATION, SUB_STATUS_DUMP, FORMAT_BANK, high, low];
    for (place, byte) in message.iter_mut().zip(header) {
        *place = byte;
    }
    let data = message.iter_mut().skip(DUMP_HEADER_BYTES);
    for (place, byte) in data.zip(voices.iter().flat_map(Patch::to_packed)) {
        *place = byte;
    }
    let sum = message.get(DUMP_HEADER_BYTES..SYSEX_BYTES.saturating_sub(DUMP_TRAILER_BYTES)).map_or(0, checksum);
    if let Some(trailer) = message.last_chunk_mut::<DUMP_TRAILER_BYTES>() {
        *trailer = [sum, END];
    }
    message
}

/// The parameter a parameter change sets (S7-43).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    /// Group 0: voice parameter 0 to 155 of the part's current voice.
    Voice(u16),
    /// Group 2: function parameter 64 to 77 of the part.
    Function(u16),
}

/// One parameter change: a part, a parameter and its new value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterChange {
    /// The channel, 0 to 15: the part.
    pub channel: u8,
    /// The parameter.
    pub parameter: Parameter,
    /// The value, within the parameter's range.
    pub value: u8,
}

/// Why a parameter change was not taken (S7-43, S7-45).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterError {
    /// The message is not 7 bytes, or does not begin `F0 43 1n`.
    Length,
    /// A byte before the last has bit 7 set.
    DataByte,
    /// The last byte is not `F7`.
    Terminator,
    /// The group is neither 0 nor 2.
    Group,
    /// The number is above 155 in group 0, or not 64 to 77 in group 2.
    Number,
    /// The value is above the parameter's range (S7-29).
    OutOfRange,
}

/// The largest value of `parameter`, or `None` for a number it does not
/// have.
#[must_use]
pub fn parameter_max(parameter: Parameter) -> Option<u8> {
    match parameter {
        Parameter::Voice(number) if number <= LAST_VOICE_PARAMETER => PARAMETER_MAX.get(usize::from(number)).copied(),
        Parameter::Function(number) if (FIRST_FUNCTION_PARAMETER..=LAST_FUNCTION_PARAMETER).contains(&number) => {
            FUNCTION_MAX.get(usize::from(number.saturating_sub(FIRST_FUNCTION_PARAMETER))).copied()
        }
        Parameter::Voice(_) | Parameter::Function(_) => None,
    }
}

/// Reads a parameter change, `F0` to `F7` (S7-43).
///
/// # Errors
///
/// A [`ParameterError`] naming the fault; such a message changes nothing.
pub fn read_parameter_change(message: &[u8]) -> Result<ParameterChange, ParameterError> {
    let Some((&START, after_start)) = message.split_first() else { return Err(ParameterError::Length) };
    read_parameter_change_event(after_start)
}

/// Reads the bytes after a parameter change's `F0`, as a block's event
/// holds them.
///
/// # Errors
///
/// As [`read_parameter_change`].
pub fn read_parameter_change_event(after_start: &[u8]) -> Result<ParameterChange, ParameterError> {
    let &[identification, sub_status, group_bits, low, value, end] = after_start else {
        return Err(ParameterError::Length);
    };
    if identification != IDENTIFICATION || sub_status & SUB_STATUS_MASK != SUB_STATUS_PARAMETER {
        return Err(ParameterError::Length);
    }
    if [identification, sub_status, group_bits, low, value].iter().any(|&byte| byte > DATA_MAX) {
        return Err(ParameterError::DataByte);
    }
    if end != END {
        return Err(ParameterError::Terminator);
    }
    let number = u16::from(group_bits & HIGH_BITS_MASK) << BITS_PER_DATA_BYTE | u16::from(low);
    let parameter = match group_bits >> GROUP_SHIFT {
        GROUP_VOICE => Parameter::Voice(number),
        GROUP_FUNCTION => Parameter::Function(number),
        _ => return Err(ParameterError::Group),
    };
    let max = parameter_max(parameter).ok_or(ParameterError::Number)?;
    if value > max {
        return Err(ParameterError::OutOfRange);
    }
    Ok(ParameterChange { channel: sub_status & CHANNEL_MASK, parameter, value })
}

/// The parameter change of `change` (S7-43; `server.md`, SRV-29), or
/// `None` where its parameter number does not exist or its value is above
/// the range.
#[must_use]
pub fn parameter_change(change: ParameterChange) -> Option<[u8; PARAMETER_CHANGE_BYTES]> {
    let max = parameter_max(change.parameter)?;
    if change.value > max {
        return None;
    }
    let (group, number) = match change.parameter {
        Parameter::Voice(number) => (GROUP_VOICE, number),
        Parameter::Function(number) => (GROUP_FUNCTION, number),
    };
    let high = u8::try_from(number >> BITS_PER_DATA_BYTE).ok()? & HIGH_BITS_MASK;
    let low = u8::try_from(number & u16::from(DATA_MAX)).ok()?;
    Some([
        START,
        IDENTIFICATION,
        SUB_STATUS_PARAMETER | (change.channel & CHANNEL_MASK),
        group << GROUP_SHIFT | high,
        low,
        change.value,
        END,
    ])
}
