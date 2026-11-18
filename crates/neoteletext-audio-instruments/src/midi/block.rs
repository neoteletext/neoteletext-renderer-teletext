//! A block's event list: a Standard MIDI File track-event list of up to
//! [`MOST_BLOCK_BYTES`] bytes and [`MOST_EVENTS`] events, its times in
//! ticks of 0.1 ms (MIDI-23 to MIDI-38).
//!
//! The grammar (MIDI-23), every number a minimal variable-length quantity
//! of one or two bytes (MIDI-24):
//!
//! ```text
//! block           = { event }
//! event           = delta message                       ; time 0 to 199
//! message         = channel-message | sysex | cue | skipped
//! channel-message = status data data | status data      ; MIDI-26, no running status (MIDI-12)
//! sysex           = 0xF0 length { data } 0xF7           ; length 1 to 256 (MIDI-27, MIDI-28)
//! skipped         = 0xF7 length { any }                 ; continuation or escape
//!                 | 0xFF type length { any }            ; meta event, type 0x00 to 0x7F
//! cue             = 0xFF 0x07 0x01 number               ; number 0x00 to 0x7F (RULE-109)
//!                 | 0xF1 data | 0xF2 data data | 0xF3 data | 0xF6
//!                 | 0xF8 | 0xFA | 0xFB | 0xFC | 0xFE
//! ```
//!
//! The reader ([`Events`]) reads every alternative; the writer
//! ([`BlockWriter`]) writes only channel messages, system exclusive
//! events and cues, and refuses every message a block never carries
//! (MIDI-8).
//!
//! A cue (page-rules.md RULE-109; REQ-6614) is the Standard MIDI Files Cue
//! Point meta event with one byte of data, the cue's number (Standard MIDI
//! Files 1.0, "Meta-Events": `FF 07 len text`). It is a form of the meta
//! event the grammar always passed over by its length, so a reader that
//! knows no cues is unaffected; it is the one meta event a block carries,
//! at most [`MAX_CUES_PER_BLOCK`] of them. A Cue Point of any other length,
//! one whose number has its top bit set, and a cue past the block's limit
//! are passed over and counted, as every other skipped event is.

use super::{
    CUE_DATA_BYTES, CUE_EVENT_BYTES, ChannelMessage, Cue, DATA_MAX, LAST_TICK, MAX_CUES_PER_BLOCK, MOST_BLOCK_BYTES,
    MOST_EVENTS, MOST_SYSTEM_EXCLUSIVE_BYTES, Message, MidiEvent, STATUS_BIT, controller, data_bytes, meta, status,
};

/// The high byte of a two-byte variable-length quantity carries this bit
/// (Standard MIDI Files 1.0, printed page 2, PDF 131: "All bytes except
/// the last have bit 7 set").
const MORE: u8 = 0x80;
/// Bits of a variable-length quantity's byte that are its value.
const SEVEN_BITS: u16 = 0x7F;
/// The largest value of a one-byte variable-length quantity.
const ONE_BYTE_MAX: u16 = 0x7F;
/// Bits a variable-length quantity's byte carries.
const BITS_PER_BYTE: u32 = 7;

/// What the message of one event was, once its extent is known.
enum Found<'a> {
    /// A message to give the synthesizer.
    Message(Message<'a>),
    /// A cue of this number (RULE-109).
    Cue(u8),
    /// An event passed over by its length.
    Skipped,
}

/// What reading one event of a block found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Read<'a> {
    /// An event to give the synthesizer.
    Event(MidiEvent<'a>),
    /// A cue (RULE-109), for the caller: the synthesizer is given nothing.
    Cue(Cue),
    /// An event whose extent is known that is passed over and counted: a
    /// skipped form (MIDI-29), a system exclusive event that is not whole
    /// (MIDI-34), an event past the first [`MOST_EVENTS`] (MIDI-31), or a
    /// cue past the first [`MAX_CUES_PER_BLOCK`] (RULE-109).
    PassedOver,
    /// A framing error (MIDI-33): the rest of the block is passed over and
    /// the block counted once as malformed. Reading ends after it.
    Malformed,
}

/// The events of a block, read in order. It yields each event, each event
/// passed over, and at most one [`Read::Malformed`], after which it ends.
/// Its work is bounded by the block's bytes, which the caller bounds.
#[derive(Clone, Debug)]
pub struct Events<'a> {
    rest: &'a [u8],
    /// The time of the last event, in ticks from the block's start.
    time: u16,
    /// Events read so far, passed over or not.
    count: usize,
    /// Cues read so far, passed over or not.
    cues: usize,
    done: bool,
}

impl<'a> Events<'a> {
    /// The events of `block`.
    #[must_use]
    pub const fn new(block: &'a [u8]) -> Self {
        Self { rest: block, time: 0, count: 0, cues: 0, done: false }
    }

    /// Takes a minimal variable-length quantity of at most two bytes
    /// (MIDI-24), or `None` for a framing error.
    fn quantity(&mut self) -> Option<u16> {
        let (&first, rest) = self.rest.split_first()?;
        if first & MORE == 0 {
            self.rest = rest;
            return Some(u16::from(first));
        }
        // A leading byte of 0x80 is not minimal.
        if first == MORE {
            return None;
        }
        let (&second, rest) = rest.split_first()?;
        // A third byte is past the bound.
        if second & MORE != 0 {
            return None;
        }
        self.rest = rest;
        Some((u16::from(first) & SEVEN_BITS) << BITS_PER_BYTE | u16::from(second))
    }

    /// Takes `count` bytes, or `None` where they run past the block.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.rest.split_at_checked(count)?;
        self.rest = rest;
        Some(taken)
    }

    /// Reads the message after a delta time; `None` for a framing error.
    fn message(&mut self) -> Option<Found<'a>> {
        let (&first, rest) = self.rest.split_first()?;
        self.rest = rest;
        if let Some(count) = data_bytes(first) {
            // A data byte where a status is expected is caught by
            // `data_bytes` returning `None`, since its top bit is clear.
            let data = self.take(count)?;
            if data.iter().any(|&byte| byte > DATA_MAX) {
                return None;
            }
            let data1 = data.first().copied().unwrap_or(0);
            let data2 = data.get(1).copied().unwrap_or(0);
            return Some(Found::Message(Message::Channel(ChannelMessage { status: first, data1, data2 })));
        }
        match first {
            status::SYSTEM_EXCLUSIVE => {
                let length = usize::from(self.quantity()?);
                let bytes = self.take(length)?;
                Some(if whole_system_exclusive(bytes) {
                    Found::Message(Message::SystemExclusive(bytes))
                } else {
                    Found::Skipped
                })
            }
            status::END_OF_EXCLUSIVE => {
                let length = usize::from(self.quantity()?);
                self.take(length)?;
                Some(Found::Skipped)
            }
            status::META => {
                let kind = self.take(1)?;
                if kind.iter().any(|&byte| byte > DATA_MAX) {
                    return None;
                }
                let length = usize::from(self.quantity()?);
                let data = self.take(length)?;
                // RULE-109: the Cue Point with one byte of data, a number
                // of seven bits, is a cue; every other meta event, and a
                // Cue Point of another shape, is passed over as before.
                Some(match (kind, data) {
                    (&[meta::CUE_POINT], &[number]) if number <= DATA_MAX => Found::Cue(number),
                    _ => Found::Skipped,
                })
            }
            // Bare system common and real-time messages, passed over by the
            // data bytes MIDI 1.0 gives each (Tables V and VI, PDF 105 and
            // 106): a tolerance of NeoTeletext's own (section 19, item 13).
            status::TIME_CODE | status::SONG_SELECT => self.skip_data(1),
            status::SONG_POSITION => self.skip_data(2),
            status::TUNE_REQUEST
            | status::TIMING_CLOCK
            | status::START
            | status::CONTINUE
            | status::STOP
            | status::ACTIVE_SENSING => Some(Found::Skipped),
            // A data byte or an undefined status where a message belongs.
            _ => None,
        }
    }

    /// Passes over `count` data bytes; `None` where one is a status byte
    /// or they run past the block.
    fn skip_data(&mut self, count: usize) -> Option<Found<'a>> {
        let data = self.take(count)?;
        if data.iter().any(|&byte| byte > DATA_MAX) {
            return None;
        }
        Some(Found::Skipped)
    }
}

/// Whether the bytes of a system exclusive event after its length are a
/// whole message (MIDI-27, MIDI-28): 1 to 256 bytes, the last `0xF7` and
/// every other a data byte.
#[must_use]
pub fn whole_system_exclusive(bytes: &[u8]) -> bool {
    let Some((&last, data)) = bytes.split_last() else { return false };
    bytes.len() <= MOST_SYSTEM_EXCLUSIVE_BYTES
        && last == status::END_OF_EXCLUSIVE
        && data.iter().all(|&byte| byte <= DATA_MAX)
}

impl<'a> Iterator for Events<'a> {
    type Item = Read<'a>;

    fn next(&mut self) -> Option<Read<'a>> {
        if self.done || self.rest.is_empty() {
            return None;
        }
        let read = self.quantity().and_then(|delta| {
            self.time = self.time.saturating_add(delta);
            // An event at tick 200 or later belongs to another block.
            if self.time > u16::from(LAST_TICK) {
                return None;
            }
            let time = u8::try_from(self.time).ok()?;
            self.message().map(|message| (time, message))
        });
        let Some((time, message)) = read else {
            self.done = true;
            return Some(Read::Malformed);
        };
        self.count = self.count.saturating_add(1);
        Some(match message {
            Found::Message(message) if self.count <= MOST_EVENTS => Read::Event(MidiEvent { time, message }),
            Found::Cue(number) => {
                self.cues = self.cues.saturating_add(1);
                if self.count <= MOST_EVENTS && self.cues <= MAX_CUES_PER_BLOCK {
                    Read::Cue(Cue { time, number })
                } else {
                    Read::PassedOver
                }
            }
            Found::Message(_) | Found::Skipped => Read::PassedOver,
        })
    }
}

/// Where a block that is not wholly acted on first fails: what the server
/// refuses (MIDI-21).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockFault {
    /// More than [`MOST_BLOCK_BYTES`] bytes.
    TooLong,
    /// The event at this index, counted from 0, would be passed over.
    PassedOver(usize),
    /// A framing error at the event of this index.
    Malformed(usize),
}

/// Checks a block as the receiver reads it: every event is acted on, none
/// passed over and no framing error (MIDI-21). Gives the count of events.
///
/// # Errors
/// The first fault, as [`BlockFault`] says.
pub fn check(block: &[u8]) -> Result<usize, BlockFault> {
    if block.len() > MOST_BLOCK_BYTES {
        return Err(BlockFault::TooLong);
    }
    let mut count: usize = 0;
    for read in Events::new(block) {
        match read {
            Read::Event(_) | Read::Cue(_) => {}
            Read::PassedOver => return Err(BlockFault::PassedOver(count)),
            Read::Malformed => return Err(BlockFault::Malformed(count)),
        }
        count = count.saturating_add(1);
    }
    Ok(count)
}

/// Why an event could not be written into a block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteError {
    /// A time past [`LAST_TICK`], or before the last event's.
    Time,
    /// The event would take the block past its bytes (MIDI-30), or past the
    /// buffer the writer was given.
    Full,
    /// The block holds [`MOST_EVENTS`] events already (MIDI-31).
    TooManyEvents,
    /// The block holds [`MAX_CUES_PER_BLOCK`] cues already (RULE-109).
    TooManyCues,
    /// A cue's number is past [`DATA_MAX`] (RULE-109).
    CueNumber,
    /// Not a message a block carries: a status that is not a channel
    /// message's, a data byte with its top bit set (MIDI-11), local control
    /// (MIDI-8), or a system exclusive message that is not whole or longer
    /// than [`MOST_SYSTEM_EXCLUSIVE_BYTES`] (MIDI-27, MIDI-28).
    Message,
}

/// Writes one block's event list into a buffer, the server's writer
/// (MIDI-23): only channel messages, each with its status byte (MIDI-12),
/// whole system exclusive messages and cues (RULE-109), at nondecreasing
/// times, every number in its minimal form so that one list has one
/// encoding (MIDI-24, MIDI-44).
#[derive(Debug)]
pub struct BlockWriter<'b> {
    buffer: &'b mut [u8],
    length: usize,
    time: u8,
    events: usize,
    cues: usize,
}

impl<'b> BlockWriter<'b> {
    /// A writer of an empty block into `buffer`, of which at most
    /// [`MOST_BLOCK_BYTES`] bytes are used.
    pub fn new(buffer: &'b mut [u8]) -> Self {
        Self { buffer, length: 0, time: 0, events: 0, cues: 0 }
    }

    /// Empties the block, to write the next one into the same buffer.
    pub fn clear(&mut self) {
        self.length = 0;
        self.time = 0;
        self.events = 0;
        self.cues = 0;
    }

    /// The block written so far.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.buffer.get(..self.length).unwrap_or(&[])
    }

    /// Events written so far.
    #[must_use]
    pub const fn events(&self) -> usize {
        self.events
    }

    /// The bytes an event of `message` would take at a delta time of
    /// `delta` ticks, or `None` for a message a block does not carry.
    #[must_use]
    pub fn encoded_length(delta: u8, message: &Message<'_>) -> Option<usize> {
        let body = match message {
            Message::Channel(channel) => {
                valid_channel_message(*channel).then_some(())?;
                data_bytes(channel.status)?.saturating_add(1)
            }
            Message::SystemExclusive(bytes) => {
                whole_system_exclusive(bytes).then_some(())?;
                let length = u16::try_from(bytes.len()).ok()?;
                1_usize.saturating_add(quantity_length(length)).saturating_add(bytes.len())
            }
        };
        Some(quantity_length(u16::from(delta)).saturating_add(body))
    }

    /// Cues written so far.
    #[must_use]
    pub const fn cues(&self) -> usize {
        self.cues
    }

    /// The bytes a cue takes at a delta time of `delta` ticks: the delta
    /// and `FF 07 01 nn` (RULE-109).
    #[must_use]
    pub const fn cue_length(delta: u8) -> usize {
        quantity_length(delta as u16).saturating_add(CUE_EVENT_BYTES)
    }

    /// Where an event of `length` bytes at tick `time` goes: its delta time
    /// and the place for its bytes, or why it cannot be written.
    fn place(&mut self, time: u8, length: impl FnOnce(u8) -> Option<usize>) -> Result<(u8, &mut [u8]), WriteError> {
        if time > LAST_TICK || time < self.time {
            return Err(WriteError::Time);
        }
        if self.events >= MOST_EVENTS {
            return Err(WriteError::TooManyEvents);
        }
        let delta = time.saturating_sub(self.time);
        let end = self.length.saturating_add(length(delta).ok_or(WriteError::Message)?);
        if end > MOST_BLOCK_BYTES {
            return Err(WriteError::Full);
        }
        let place = self.buffer.get_mut(self.length..end).ok_or(WriteError::Full)?;
        Ok((delta, place))
    }

    /// Adds the cue `number`, 0 to [`DATA_MAX`], at tick `time` of the
    /// block (RULE-109; REQ-6614 item 5): the Cue Point meta event with
    /// one byte of data (Standard MIDI Files 1.0, "Meta-Events"). A cue
    /// counts among the block's events (MIDI-31).
    ///
    /// # Errors
    /// [`WriteError`], with nothing written.
    pub fn push_cue(&mut self, time: u8, number: u8) -> Result<(), WriteError> {
        if number > DATA_MAX {
            return Err(WriteError::CueNumber);
        }
        if self.cues >= MAX_CUES_PER_BLOCK {
            return Err(WriteError::TooManyCues);
        }
        let (delta, place) = self.place(time, |delta| Some(Self::cue_length(delta)))?;
        let mut at = write_quantity(place, 0, u16::from(delta));
        let data_bytes = u8::try_from(CUE_DATA_BYTES).unwrap_or(0);
        for byte in [status::META, meta::CUE_POINT, data_bytes, number] {
            put(place, &mut at, byte);
        }
        self.length = self.length.saturating_add(at);
        self.time = time;
        self.events = self.events.saturating_add(1);
        self.cues = self.cues.saturating_add(1);
        Ok(())
    }

    /// Adds `message` at tick `time` of the block.
    ///
    /// # Errors
    /// [`WriteError`], with nothing written.
    pub fn push(&mut self, time: u8, message: &Message<'_>) -> Result<(), WriteError> {
        let (delta, place) = self.place(time, |delta| Self::encoded_length(delta, message))?;
        let length = place.len();
        let mut at = write_quantity(place, 0, u16::from(delta));
        match message {
            Message::Channel(channel) => {
                for byte in [channel.status, channel.data1, channel.data2].into_iter().take(length.saturating_sub(at)) {
                    put(place, &mut at, byte);
                }
            }
            Message::SystemExclusive(bytes) => {
                put(place, &mut at, status::SYSTEM_EXCLUSIVE);
                at = write_quantity(place, at, u16::try_from(bytes.len()).unwrap_or(0));
                for &byte in *bytes {
                    put(place, &mut at, byte);
                }
            }
        }
        self.length = self.length.saturating_add(length);
        self.time = time;
        self.events = self.events.saturating_add(1);
        Ok(())
    }
}

/// Whether a channel message is one a block carries (MIDI-7, MIDI-8,
/// MIDI-11, MIDI-26): a channel status, data bytes of seven bits, the
/// second 0 where the message has one data byte, and not local control.
fn valid_channel_message(message: ChannelMessage) -> bool {
    let Some(count) = data_bytes(message.status) else { return false };
    let second_fits = if count == 1 { message.data2 == 0 } else { message.data2 <= DATA_MAX };
    let local = message.kind() == status::CONTROL_CHANGE && message.data1 == controller::LOCAL_CONTROL;
    message.status & STATUS_BIT != 0 && message.data1 <= DATA_MAX && second_fits && !local
}

/// The bytes of `value`'s minimal variable-length quantity.
const fn quantity_length(value: u16) -> usize {
    if value <= ONE_BYTE_MAX { 1 } else { 2 }
}

/// Writes `value` as a minimal variable-length quantity at `at`, giving
/// where it ended.
fn write_quantity(place: &mut [u8], mut at: usize, value: u16) -> usize {
    if value > ONE_BYTE_MAX {
        let high = u8::try_from(value >> BITS_PER_BYTE).unwrap_or(0);
        put(place, &mut at, MORE | high);
    }
    put(place, &mut at, u8::try_from(value & SEVEN_BITS).unwrap_or(0));
    at
}

/// Writes one byte at `at` and moves on.
fn put(place: &mut [u8], at: &mut usize, byte: u8) {
    if let Some(slot) = place.get_mut(*at) {
        *slot = byte;
    }
    *at = at.saturating_add(1);
}

#[cfg(test)]
mod tests;
