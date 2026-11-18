//! MIDI 1.0, the one layer every synthesizer is played and programmed
//! through (`neoteletext-common/docs/audio/midi-architecture.md`, cited
//! here as "MIDI-n"; DEC-6453, DEC-6462, DEC-6464, DEC-6465).
//!
//! What is here is shared by every family and by the server, so that one
//! reading of a block, one set of common behaviours and one writer exist:
//!
//! - [`block`]: a block's event list, read ([`block::Events`],
//!   [`block::check`]) and written ([`block::BlockWriter`]): a Standard
//!   MIDI File track-event list of 200 ticks of 0.1 ms (MIDI-23 to
//!   MIDI-38).
//! - [`common`]: the behaviours every synthesizer that plays notes shares,
//!   one rule each (MIDI-9, MIDI-76, MIDI-84 to MIDI-92), kept per channel
//!   by [`common::Common`].
//! - [`restate`]: each part's state as the sender keeps it, and its
//!   restatement (MIDI-93 to MIDI-99), for the server.
//! - [`Instrument`]: the one trait the receiver plays every family
//!   through, with the banks of its sound ([`Banks`]).
//!
//! Everything here is `no_std`, integer-only and allocation-free, and reads
//! untrusted bytes: every bound is a named constant below.

pub mod block;
pub mod common;
mod instrument;
pub mod restate;

pub use instrument::{Banks, Instrument, Outcome, Played, play_block};

/// Ticks in a block: a block is 20 ms and a tick 0.1 ms (MIDI-25,
/// decision A3).
pub const TICKS_PER_BLOCK: u16 = 200;
/// The latest time an event may have within its block, in ticks
/// (MIDI-25).
pub const LAST_TICK: u8 = 199;
/// The most events a block holds (MIDI-31). A family may set fewer.
pub const MOST_EVENTS: usize = 256;
/// The most bytes of a system exclusive event after its `0xF0` (MIDI-28):
/// the DX7's single-voice dump, the longest timed message of any adopted
/// interface, is 162.
pub const MOST_SYSTEM_EXCLUSIVE_BYTES: usize = 256;
/// The most bytes of a block (MIDI-30). Its derivation from the record's
/// bounds is asserted in `neoteletext-audio`'s `codec.rs`, where the
/// record's figures live.
pub const MOST_BLOCK_BYTES: usize = 1_005;
/// The most bytes a variable-length quantity of a block takes (MIDI-24).
pub const MOST_QUANTITY_BYTES: usize = 2;
/// The most banks a sound names (MIDI-57): the Prophet-5's ten groups,
/// the most of any adopted interface.
pub const MOST_BANKS: usize = 10;
/// The most bytes of a bank image (MIDI-55).
pub const MOST_BANK_BYTES: usize = 16_384;
/// The most patches a bank holds (MIDI-56): a program change reaches 0 to
/// 127.
pub const MOST_BANK_PATCHES: usize = 128;
/// Microseconds in a tick (MIDI-17).
pub const MICROSECONDS_PER_TICK: u64 = 100;
/// Samples a tick, as a fraction: 4.8 samples at 48,000 Hz (MIDI-5).
const SAMPLES_PER_TICK_NUMERATOR: usize = 24;
/// See [`SAMPLES_PER_TICK_NUMERATOR`].
const SAMPLES_PER_TICK_DENOMINATOR: usize = 5;
// A block's 200 ticks are its 960 samples.
const _: () = assert!(
    TICKS_PER_BLOCK as usize * SAMPLES_PER_TICK_NUMERATOR / SAMPLES_PER_TICK_DENOMINATOR == crate::BLOCK_SAMPLES
);
const _: () = assert!(LAST_TICK as u16 + 1 == TICKS_PER_BLOCK);

/// The absolute tick of a time in microseconds from a sound's start: the
/// nearest tick, a half rounded up, `floor((time + 50) ÷ 100)` (MIDI-17).
/// Each event's tick is computed from its own time, so no rounding passes
/// from one event to the next.
#[must_use]
pub const fn tick_of_microseconds(microseconds: u64) -> u64 {
    microseconds.saturating_add(MICROSECONDS_PER_TICK / 2) / MICROSECONDS_PER_TICK
}

/// The sample of its block, 0 to 955, before which an event at `time`
/// takes effect: `floor(time × 24 ÷ 5)` (MIDI-5).
#[must_use]
pub const fn sample_of_tick(time: u8) -> usize {
    (time as usize).saturating_mul(SAMPLES_PER_TICK_NUMERATOR) / SAMPLES_PER_TICK_DENOMINATOR
}

/// The largest data byte: seven bits (MIDI 1.0 4.2, printed page 6, PDF
/// 38).
pub const DATA_MAX: u8 = 0x7F;
/// The top bit of a MIDI byte: set on a status byte, clear on a data byte.
pub const STATUS_BIT: u8 = 0x80;
/// The bits of a channel message's status byte that name the message.
pub const KIND_MASK: u8 = 0xF0;
/// The bits of a channel message's status byte that are its channel,
/// 0 to 15 for channels 1 to 16 (MIDI 1.0 4.2, Table I, PDF 100).
pub const CHANNEL_MASK: u8 = 0x0F;
/// MIDI channels.
pub const CHANNELS: usize = 16;
/// The centre of a pitch bend, which bends nothing (MIDI 1.0 4.2, printed
/// page 19, PDF 51: "00, 64").
pub const PITCH_BEND_CENTRE: u16 = 0x2000;
/// The largest pitch bend: 14 bits.
pub const PITCH_BEND_MAX: u16 = 0x3FFF;

/// Status bytes (MIDI 1.0 4.2, Tables I, V, VI and VII, PDF 100 to 107).
/// Channel messages are given with channel 0, the kind bits alone.
pub mod status {
    /// Note off.
    pub const NOTE_OFF: u8 = 0x80;
    /// Note on; a velocity of 0 is a note off.
    pub const NOTE_ON: u8 = 0x90;
    /// Polyphonic key pressure.
    pub const KEY_PRESSURE: u8 = 0xA0;
    /// Control change, and the channel mode messages 120 to 127.
    pub const CONTROL_CHANGE: u8 = 0xB0;
    /// Program change.
    pub const PROGRAM_CHANGE: u8 = 0xC0;
    /// Channel pressure.
    pub const CHANNEL_PRESSURE: u8 = 0xD0;
    /// Pitch bend.
    pub const PITCH_BEND: u8 = 0xE0;
    /// System exclusive start; in a block, the `F0 <length> …` event.
    pub const SYSTEM_EXCLUSIVE: u8 = 0xF0;
    /// MIDI time code quarter frame: one data byte.
    pub const TIME_CODE: u8 = 0xF1;
    /// Song position pointer: two data bytes.
    pub const SONG_POSITION: u8 = 0xF2;
    /// Song select: one data byte.
    pub const SONG_SELECT: u8 = 0xF3;
    /// Undefined (Table V).
    pub const UNDEFINED_F4: u8 = 0xF4;
    /// Undefined (Table V).
    pub const UNDEFINED_F5: u8 = 0xF5;
    /// Tune request.
    pub const TUNE_REQUEST: u8 = 0xF6;
    /// End of exclusive; in a Standard MIDI File, the `F7 <length> …`
    /// continuation or escape event.
    pub const END_OF_EXCLUSIVE: u8 = 0xF7;
    /// Timing clock.
    pub const TIMING_CLOCK: u8 = 0xF8;
    /// Undefined (Table VI).
    pub const UNDEFINED_F9: u8 = 0xF9;
    /// Start.
    pub const START: u8 = 0xFA;
    /// Continue.
    pub const CONTINUE: u8 = 0xFB;
    /// Stop.
    pub const STOP: u8 = 0xFC;
    /// Undefined (Table VI).
    pub const UNDEFINED_FD: u8 = 0xFD;
    /// Active sensing.
    pub const ACTIVE_SENSING: u8 = 0xFE;
    /// System reset; in a Standard MIDI File, a meta event.
    pub const META: u8 = 0xFF;
}

/// Controller numbers (MIDI 1.0 4.2, Tables III and IV, PDF 102 and 104).
pub mod controller {
    /// Bank select, most significant byte.
    pub const BANK_SELECT: u8 = 0;
    /// Modulation wheel.
    pub const MODULATION: u8 = 1;
    /// Data entry, most significant byte.
    pub const DATA_ENTRY: u8 = 6;
    /// Channel volume.
    pub const VOLUME: u8 = 7;
    /// Expression.
    pub const EXPRESSION: u8 = 11;
    /// Bank select, least significant byte.
    pub const BANK_SELECT_FINE: u8 = 32;
    /// Data entry, least significant byte.
    pub const DATA_ENTRY_FINE: u8 = 38;
    /// Damper pedal (sustain).
    pub const SUSTAIN: u8 = 64;
    /// Portamento on and off.
    pub const PORTAMENTO: u8 = 65;
    /// Sostenuto.
    pub const SOSTENUTO: u8 = 66;
    /// Soft pedal.
    pub const SOFT: u8 = 67;
    /// Data increment.
    pub const DATA_INCREMENT: u8 = 96;
    /// Data decrement.
    pub const DATA_DECREMENT: u8 = 97;
    /// Non-registered parameter number, least significant byte.
    pub const NRPN_FINE: u8 = 98;
    /// Non-registered parameter number, most significant byte.
    pub const NRPN_COARSE: u8 = 99;
    /// Registered parameter number, least significant byte.
    pub const RPN_FINE: u8 = 100;
    /// Registered parameter number, most significant byte.
    pub const RPN_COARSE: u8 = 101;
    /// All sound off.
    pub const ALL_SOUND_OFF: u8 = 120;
    /// Reset all controllers.
    pub const RESET_ALL_CONTROLLERS: u8 = 121;
    /// Local control: never carried (MIDI-8).
    pub const LOCAL_CONTROL: u8 = 122;
    /// All notes off.
    pub const ALL_NOTES_OFF: u8 = 123;
    /// Omni off.
    pub const OMNI_OFF: u8 = 124;
    /// Omni on.
    pub const OMNI_ON: u8 = 125;
    /// Mono on.
    pub const MONO_ON: u8 = 126;
    /// Poly on.
    pub const POLY_ON: u8 = 127;
}

/// The registered parameter number of pitch bend sensitivity, 00 00
/// (MIDI 1.0 4.2, printed page 18, PDF 50; Table `IIIa`, PDF 103).
pub const RPN_PITCH_BEND_SENSITIVITY: u16 = 0;
/// The null parameter number, `7F 7F`, which selects no parameter (GM
/// Developer Guidelines, page 6, PDF 315).
pub const NULL_PARAMETER: u16 = 0x3FFF;

/// One channel message: a status byte of `0x80` to `0xEF` and its data
/// bytes, `data2` being 0 for a message of one data byte (program change,
/// channel pressure).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChannelMessage {
    /// The status byte: kind and channel.
    pub status: u8,
    /// The first data byte.
    pub data1: u8,
    /// The second data byte, or 0.
    pub data2: u8,
}

impl ChannelMessage {
    /// A message of `kind` (a constant of [`status`]) on `channel`, 0 to
    /// 15 (a larger channel is taken modulo 16).
    #[must_use]
    pub const fn new(kind: u8, channel: u8, data1: u8, data2: u8) -> Self {
        Self { status: (kind & KIND_MASK) | (channel & CHANNEL_MASK), data1, data2 }
    }

    /// The kind bits of the status, a constant of [`status`].
    #[must_use]
    pub const fn kind(self) -> u8 {
        self.status & KIND_MASK
    }

    /// The channel, 0 to 15.
    #[must_use]
    pub const fn channel(self) -> u8 {
        self.status & CHANNEL_MASK
    }

    /// The 14-bit value of a pitch bend: `data2 × 128 + data1` (MIDI-91).
    #[must_use]
    pub const fn bend(self) -> u16 {
        ((self.data2 & DATA_MAX) as u16) << 7 | (self.data1 & DATA_MAX) as u16
    }
}

/// The data bytes a channel message of status `status` carries (MIDI-26),
/// or `None` for a status that is not a channel message.
#[must_use]
pub const fn data_bytes(status: u8) -> Option<usize> {
    match status & KIND_MASK {
        status::PROGRAM_CHANGE | status::CHANNEL_PRESSURE => Some(1),
        status::NOTE_OFF | status::NOTE_ON | status::KEY_PRESSURE | status::CONTROL_CHANGE | status::PITCH_BEND => {
            Some(2)
        }
        _ => None,
    }
}

/// One MIDI message of an event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Message<'a> {
    /// A channel message.
    Channel(ChannelMessage),
    /// A system exclusive message: every byte after its `0xF0`, the last
    /// `0xF7` and every other a data byte, 1 to
    /// [`MOST_SYSTEM_EXCLUSIVE_BYTES`] of them (MIDI-27).
    SystemExclusive(&'a [u8]),
}

/// One event of a block: a time and one message (MIDI-5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MidiEvent<'a> {
    /// The tick within the block, 0 to [`LAST_TICK`].
    pub time: u8,
    /// The message.
    pub message: Message<'a>,
}
