//! synth8's interface module (server.md, SRV-88): its tables and checks
//! without its synthesis, for the receiver's instrument and for the server,
//! which builds them with the feature `interface` alone.
//!
//! - [`accepts`]: which messages synth8 acts on, and on which channels
//!   (`synth8.md`, S8-1, S8-2), for the server's check of every event
//!   (SRV-24).
//! - [`pack_patch`] and [`check_patch`]: the 42-value patch (S8-23) from
//!   its value type [`Patch`] and back.
//! - [`build_bank`] and [`check_bank`]: the bank image, 1 to 64 patches of
//!   42 bytes back to back (S8-29 (a); MIDI-56 form 2).
//! - [`Tracker`]: the two halves' state and the performance controls as the
//!   server writes them, and their restatement (S8-29 (e); MIDI-93 to
//!   MIDI-99).
//! - [`vco_bend_cents`] and [`bend_entry`]: the VCO bend depth set by
//!   controller 41 and by registered parameter number 0 (S8-14, S8-31),
//!   the one reading both the instrument and the tracker use.
//! - [`program_of_patch_number`] and [`patch_number_of_program`]: the
//!   instrument's patch numbers 11 to 88 against programs 0 to 63 (S8-28).

use crate::midi::common::{CENTS_PER_SEMITONE, DataEntry, PEDAL_DOWN};
use crate::midi::{
    CHANNEL_MASK, CHANNELS, ChannelMessage, DATA_MAX, Message, NULL_PARAMETER, PITCH_BEND_CENTRE,
    RPN_PITCH_BEND_SENSITIVITY, controller, status,
};

pub use super::patch::{PATCH_BYTES, Param, Patch, PatchError, VCO2_RANGE_LOW_FREQUENCY};

/// The channel, counted from 0, of the upper half's panel controls and
/// program changes: MIDI channel 1 (S8-2 item 4).
pub const UPPER_CHANNEL: u8 = 0;
/// The channel of the lower half's: MIDI channel 2.
pub const LOWER_CHANNEL: u8 = 1;
/// The channels that address a half: channels 1 and 2 (S8-2 item 3).
pub const HALF_CHANNELS: u8 = 2;
/// The most patches a bank holds: the instrument's 64 memories, 11 to 88
/// (S8-28, S8-29 (a); owner's manual, printed page 3).
pub const MOST_PATCHES: usize = 64;
/// The most bytes of a bank image: 64 patches of 42 bytes (S8-29 (a)).
pub const MOST_BANK_BYTES: usize = MOST_PATCHES * PATCH_BYTES;
const _: () = assert!(MOST_BANK_BYTES <= crate::midi::MOST_BANK_BYTES);
const _: () = assert!(MOST_PATCHES <= crate::midi::MOST_BANK_PATCHES);
/// The bank index every program change chooses from: synth8 has no bank
/// select (S8-12, MIDI-64).
pub const BANK_INDEX: usize = 0;

/// Portamento time (S8-15; MIDI 1.0 4.2, Table III, PDF 102).
pub const PORTAMENTO_TIME: u8 = 5;
/// Balance of the two halves (S8-10; MIDI 1.0 4.2, Table III).
pub const BALANCE: u8 = 8;
/// The chart's "BEND PITCH": the VCO bend depth (S8-14).
pub const BEND_PITCH: u8 = 41;
/// The chart's "PORTAMENTO SW" (S8-15).
pub const PORTAMENTO_SWITCH: u8 = 118;
/// The VCO bend depth before any message, in cents: two semitones
/// (S8-14; MIDI-90).
pub const BEND_DEFAULT_CENTS: u16 = 200;
/// The most VCO bend depth, in cents: an octave (S8-14; `mod.rs`'s
/// `BEND_MAX`, from the service notes' adjustment 25-1; MIDI-90).
pub const BEND_MOST_CENTS: u16 = 1_200;
/// The largest cents of data entry's fine part that registered parameter
/// number 0 takes; a larger value is taken as this (S8-31 item 2).
pub const BEND_FINE_MOST: u8 = 99;
/// The balance before controller 8 arrives (S8-10).
pub const BALANCE_CENTRE: u8 = 64;

/// The controllers that act on the whole instrument on any channel (S8-2
/// item 2): the performance controls, the registered parameter
/// controllers and the common layer's 120, 121 and 123 to 127.
#[must_use]
pub const fn is_performance_controller(number: u8) -> bool {
    matches!(
        number,
        controller::MODULATION
            | PORTAMENTO_TIME
            | controller::VOLUME
            | BALANCE
            | controller::EXPRESSION
            | BEND_PITCH
            | controller::SUSTAIN
            | PORTAMENTO_SWITCH
            | controller::DATA_ENTRY
            | controller::DATA_ENTRY_FINE
            | controller::DATA_INCREMENT
            | controller::DATA_DECREMENT
            | controller::NRPN_FINE
            | controller::NRPN_COARSE
            | controller::RPN_FINE
            | controller::RPN_COARSE
            | controller::ALL_SOUND_OFF
            | controller::RESET_ALL_CONTROLLERS
            | controller::ALL_NOTES_OFF
            | controller::OMNI_OFF
            | controller::OMNI_ON
            | controller::MONO_ON
            | controller::POLY_ON
    )
}

/// Why synth8 does not take a message: the requirement of `synth8.md` (or
/// the architecture) that says so, for the server's refusal (SRV-24).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NotTaken {
    /// The requirement, such as `"S8-2"`.
    pub requirement: &'static str,
}

const fn not_taken(requirement: &'static str) -> Result<(), NotTaken> {
    Err(NotTaken { requirement })
}

/// Whether synth8 acts on `message` (S8-1): notes and pitch bend on every
/// channel; the performance controllers on every channel (S8-2 item 2);
/// the 42 panel controllers and controller 63, and program changes 0 to
/// 63, on channels 1 and 2 alone (S8-2 items 4 and 5, S8-29 (b)). Key and
/// channel pressure (S8-6), every other controller, system exclusive and a
/// data byte above 127 are not taken. A data entry is taken whatever the
/// selection, which only the message's place in the stream decides
/// (S8-31).
///
/// # Errors
///
/// [`NotTaken`], naming the requirement, for a message synth8 does not act
/// on.
pub fn accepts(message: &Message<'_>) -> Result<(), NotTaken> {
    let Message::Channel(message) = message else { return not_taken("S8-1") };
    if message.data1 > DATA_MAX || message.data2 > DATA_MAX || message.status < crate::midi::STATUS_BIT {
        return not_taken("MIDI-11");
    }
    let half = message.channel() < HALF_CHANNELS;
    match message.kind() {
        status::KEY_PRESSURE | status::CHANNEL_PRESSURE => not_taken("S8-6"),
        status::PROGRAM_CHANGE if !half => not_taken("S8-2"),
        status::PROGRAM_CHANGE if usize::from(message.data1) >= MOST_PATCHES => not_taken("S8-29"),
        status::NOTE_OFF | status::NOTE_ON | status::PITCH_BEND | status::PROGRAM_CHANGE => Ok(()),
        status::CONTROL_CHANGE => {
            let number = message.data1;
            if number == controller::LOCAL_CONTROL {
                not_taken("MIDI-8")
            } else if is_performance_controller(number) {
                Ok(())
            } else if Param::of_controller(number).is_some() {
                if half { Ok(()) } else { not_taken("S8-2") }
            } else {
                not_taken("S8-16")
            }
        }
        _ => not_taken("S8-1"),
    }
}

/// The VCO bend depth controller 41's value sets, in cents: `floor((v ×
/// 1,200 + 63) ÷ 127)` (S8-14). A value above 127 is taken as 127.
#[must_use]
pub fn vco_bend_cents(value: u8) -> u16 {
    let value = u32::from(value.min(DATA_MAX));
    let half = u32::from(DATA_MAX >> 1);
    let cents = value.saturating_mul(u32::from(BEND_MOST_CENTS)).saturating_add(half).checked_div(u32::from(DATA_MAX));
    u16::try_from(cents.unwrap_or(0)).unwrap_or(BEND_MOST_CENTS)
}

/// The VCO bend depth after a data entry, increment or decrement to
/// registered parameter number 0, from the depth `cents` before it (S8-31
/// item 2): coarse sets the semitones and clears the cents; fine sets the
/// cents above the semitones, a value above 99 taken as 99; increment and
/// decrement move by one cent; each result held to 0 to 1,200 cents.
#[must_use]
pub fn bend_entry(cents: u16, entry: DataEntry) -> u16 {
    let semitones = cents / CENTS_PER_SEMITONE;
    let wanted = match entry {
        DataEntry::Coarse(value) => u16::from(value).saturating_mul(CENTS_PER_SEMITONE),
        DataEntry::Fine(value) => {
            semitones.saturating_mul(CENTS_PER_SEMITONE).saturating_add(u16::from(value.min(BEND_FINE_MOST)))
        }
        DataEntry::Increment(_) => cents.saturating_add(1),
        DataEntry::Decrement(_) => cents.saturating_sub(1),
    };
    wanted.min(BEND_MOST_CENTS)
}

/// The data entry a control change is, if it is one.
#[must_use]
pub const fn data_entry(number: u8, value: u8) -> Option<DataEntry> {
    match number {
        controller::DATA_ENTRY => Some(DataEntry::Coarse(value)),
        controller::DATA_ENTRY_FINE => Some(DataEntry::Fine(value)),
        controller::DATA_INCREMENT => Some(DataEntry::Increment(value)),
        controller::DATA_DECREMENT => Some(DataEntry::Decrement(value)),
        _ => None,
    }
}

/// The patch as its 42 bytes (S8-23; server.md, SRV-55).
#[must_use]
pub const fn pack_patch(patch: &Patch) -> [u8; PATCH_BYTES] {
    patch.to_bytes()
}

/// The patch bytes hold (S8-23).
///
/// # Errors
///
/// [`PatchError::Length`] for bytes that are not 42 long, and
/// [`PatchError::Value`] for a byte above 127.
pub fn check_patch(bytes: &[u8]) -> Result<Patch, PatchError> {
    Patch::from_slice(bytes)
}

/// Why bytes are no synth8 bank image (S8-29 (a)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BankError {
    /// No patch.
    Empty,
    /// More than [`MOST_PATCHES`] patches: how many.
    TooMany(usize),
    /// A length that is not a whole number of patches: the length.
    Length(usize),
    /// The output has no room for the image: the bytes it needs.
    Room(usize),
    /// Patch `index` is faulty.
    Patch {
        /// The patch's number in the bank, from 0.
        index: usize,
        /// What is wrong with it.
        error: PatchError,
    },
}

/// Writes the bank image of `patches` into `out`, patch k at byte `k ×
/// 42` (S8-29 (a)), and answers its length.
///
/// # Errors
///
/// [`BankError::Empty`] for no patch, [`BankError::TooMany`] past 64,
/// [`BankError::Room`] when `out` is shorter than the image.
pub fn build_bank(patches: &[Patch], out: &mut [u8]) -> Result<usize, BankError> {
    if patches.is_empty() {
        return Err(BankError::Empty);
    }
    if patches.len() > MOST_PATCHES {
        return Err(BankError::TooMany(patches.len()));
    }
    let length = patches.len().saturating_mul(PATCH_BYTES);
    let room = out.get_mut(..length).ok_or(BankError::Room(length))?;
    for (chunk, patch) in room.as_chunks_mut::<PATCH_BYTES>().0.iter_mut().zip(patches) {
        *chunk = pack_patch(patch);
    }
    Ok(length)
}

/// Checks a bank image (S8-29 (a)) and answers how many patches it holds,
/// every one of which the instrument would take.
///
/// # Errors
///
/// [`BankError::Empty`], [`BankError::Length`], [`BankError::TooMany`], or
/// [`BankError::Patch`] for the first faulty patch.
pub fn check_bank(image: &[u8]) -> Result<usize, BankError> {
    if image.is_empty() {
        return Err(BankError::Empty);
    }
    if !image.len().is_multiple_of(PATCH_BYTES) {
        return Err(BankError::Length(image.len()));
    }
    let count = image.len() / PATCH_BYTES;
    if count > MOST_PATCHES {
        return Err(BankError::TooMany(count));
    }
    for (index, chunk) in image.as_chunks::<PATCH_BYTES>().0.iter().enumerate() {
        Patch::from_bytes(chunk).map_err(|error| BankError::Patch { index, error })?;
    }
    Ok(count)
}

/// The units of the instrument's patch numbers, and their tens: each 1 to
/// 8 (owner's manual, printed pages 9 and 10, figure 2).
const DIGITS: u8 = 8;
/// A patch number's tens.
const TEN: u8 = 10;

/// The program of the instrument's patch number 11 to 88, `(tens − 1) ×
/// 8 + (units − 1)` (S8-28); `None` for a number whose either digit is
/// not 1 to 8.
#[must_use]
pub const fn program_of_patch_number(number: u8) -> Option<u8> {
    let (tens, units) = (number / TEN, number % TEN);
    if tens < 1 || tens > DIGITS || units < 1 || units > DIGITS {
        return None;
    }
    Some(tens.saturating_sub(1).saturating_mul(DIGITS).saturating_add(units.saturating_sub(1)))
}

/// The instrument's patch number of program 0 to 63 (S8-28); `None` past
/// 63.
#[must_use]
pub const fn patch_number_of_program(program: u8) -> Option<u8> {
    if program as usize >= MOST_PATCHES {
        return None;
    }
    let (tens, units) = (program / DIGITS, program % DIGITS);
    Some(tens.saturating_add(1).saturating_mul(TEN).saturating_add(units).saturating_add(1))
}

/// How the VCO bend depth was last set, for its restatement (S8-29 (e)).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Depth {
    /// Never: the default.
    Default,
    /// By controller 41, with its value.
    Controller(u8),
    /// By registered parameter number 0, to so many cents.
    Registered(u16),
}

/// The bytes of a parameter number as last selected on a channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Selection {
    registered: [u8; 2],
    non_registered: [u8; 2],
    /// Whether the registered number was selected last.
    registered_last: bool,
}

/// Both bytes of the null parameter number.
const NULL_BYTES: [u8; 2] = [DATA_MAX, DATA_MAX];
/// A selection of none.
const NO_SELECTION: Selection = Selection { registered: NULL_BYTES, non_registered: NULL_BYTES, registered_last: true };

impl Selection {
    /// Whether registered parameter number 0 is selected (S8-31).
    fn bend_range(self) -> bool {
        let [coarse, fine] = self.registered;
        let number = u16::from(coarse & DATA_MAX) << 7 | u16::from(fine & DATA_MAX);
        self.registered_last && number == RPN_PITCH_BEND_SENSITIVITY && number != NULL_PARAMETER
    }
}

/// One half's state as its restatement carries it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Half {
    /// The last program change, program 0 before any (MIDI-61, SRV-70).
    program: u8,
    /// Each panel control's value moved since the program change.
    moved: [Option<u8>; PATCH_BYTES],
}

const FRESH_HALF: Half = Half { program: 0, moved: [None; PATCH_BYTES] };

/// synth8's state as the server writes it (S8-29 (e); MIDI-94): each
/// half's program and the panel controls moved since, and the performance
/// controls of the whole instrument. It is of a fixed size: two halves of
/// 42 controls and sixteen channels' parameter selections.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tracker {
    halves: [Half; HALF_CHANNELS as usize],
    bend: u16,
    modulation: u8,
    portamento_time: u8,
    volume: u8,
    balance: u8,
    expression: u8,
    hold: bool,
    portamento: bool,
    depth: Depth,
    depth_cents: u16,
    selections: [Selection; CHANNELS],
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    /// synth8 as a sound starts it: both halves at program 0, every
    /// control at its start value.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            halves: [FRESH_HALF; HALF_CHANNELS as usize],
            bend: PITCH_BEND_CENTRE,
            modulation: 0,
            portamento_time: 0,
            volume: DATA_MAX,
            balance: BALANCE_CENTRE,
            expression: DATA_MAX,
            hold: false,
            portamento: false,
            depth: Depth::Default,
            depth_cents: BEND_DEFAULT_CENTS,
            selections: [NO_SELECTION; CHANNELS],
        }
    }

    /// The VCO bend depth the messages taken have set, in cents.
    #[must_use]
    pub const fn bend_depth_cents(&self) -> u16 {
        self.depth_cents
    }

    /// Takes one message the server wrote. A message synth8 does not act
    /// on changes nothing.
    pub fn take(&mut self, message: &Message<'_>) {
        let Message::Channel(message) = message else { return };
        if accepts(&Message::Channel(*message)).is_err() {
            return;
        }
        let channel = message.channel();
        let ChannelMessage { data1, data2, .. } = *message;
        match message.kind() {
            status::PROGRAM_CHANGE => {
                if let Some(half) = self.halves.get_mut(usize::from(channel)) {
                    *half = Half { program: data1, moved: [None; PATCH_BYTES] };
                }
            }
            status::PITCH_BEND => self.bend = message.bend(),
            status::CONTROL_CHANGE => self.control(channel, data1, data2),
            _ => {}
        }
    }

    fn control(&mut self, channel: u8, number: u8, value: u8) {
        let Some(selection) = self.selections.get_mut(usize::from(channel & CHANNEL_MASK)) else { return };
        match number {
            controller::MODULATION => self.modulation = value,
            PORTAMENTO_TIME => self.portamento_time = value,
            controller::VOLUME => self.volume = value,
            BALANCE => self.balance = value,
            controller::EXPRESSION => self.expression = value,
            controller::SUSTAIN => self.hold = value >= PEDAL_DOWN,
            PORTAMENTO_SWITCH => self.portamento = value >= PEDAL_DOWN,
            BEND_PITCH => {
                self.depth = Depth::Controller(value);
                self.depth_cents = vco_bend_cents(value);
            }
            controller::RPN_COARSE | controller::RPN_FINE => {
                if let Some(byte) = selection.registered.get_mut(usize::from(number == controller::RPN_FINE)) {
                    *byte = value;
                }
                selection.registered_last = true;
            }
            controller::NRPN_COARSE | controller::NRPN_FINE => {
                if let Some(byte) = selection.non_registered.get_mut(usize::from(number == controller::NRPN_FINE)) {
                    *byte = value;
                }
                selection.registered_last = false;
            }
            controller::RESET_ALL_CONTROLLERS => {
                // RP-015 as S8-13 item 2 lists it: on every channel.
                self.modulation = 0;
                self.expression = DATA_MAX;
                self.hold = false;
                self.bend = PITCH_BEND_CENTRE;
                self.selections = [NO_SELECTION; CHANNELS];
            }
            _ => {
                if let Some(entry) = data_entry(number, value) {
                    if selection.bend_range() {
                        self.depth_cents = bend_entry(self.depth_cents, entry);
                        self.depth = Depth::Registered(self.depth_cents);
                    }
                } else if let Some(param) = Param::of_controller(number)
                    && let Some(half) = self.halves.get_mut(usize::from(channel))
                    && let Some(place) = half.moved.get_mut(param.byte())
                {
                    *place = Some(value);
                }
            }
        }
    }

    /// Gives synth8's restatement to `out` (S8-29 (e)): on channel 1 the
    /// upper half's program change, then each panel controller moved since
    /// in ascending order of controller number (a move by controller 63
    /// as controller 62); the performance controls on channel 1, each only
    /// where it is not at its start value: pitch bend, controllers 1, 5,
    /// 7, 8, 11, 64 and 118, and the VCO bend depth as controller 41 or as
    /// registered parameter number 0 followed by the null selection, as it
    /// was last set; then the lower half's on channel 2. The server writes
    /// it whole at one tick of one block (MIDI-98).
    pub fn restate(&self, out: &mut dyn FnMut(Message<'_>)) {
        let mut send = |status: u8, channel: u8, data1: u8, data2: u8| {
            out(Message::Channel(ChannelMessage::new(status, channel, data1, data2)));
        };
        for (half, channel) in self.halves.iter().zip(0u8..) {
            send(status::PROGRAM_CHANGE, channel, half.program, 0);
            for number in 0..=DATA_MAX {
                let Some(param) = Param::ALL.into_iter().find(|param| param.controller() == number) else {
                    continue;
                };
                if let Some(Some(value)) = half.moved.get(param.byte()) {
                    send(status::CONTROL_CHANGE, channel, number, *value);
                }
            }
            if channel == UPPER_CHANNEL {
                self.restate_performance(&mut send);
            }
        }
    }

    fn restate_performance(&self, send: &mut dyn FnMut(u8, u8, u8, u8)) {
        let channel = UPPER_CHANNEL;
        let seven = u16::from(DATA_MAX);
        if self.bend != PITCH_BEND_CENTRE {
            let low = u8::try_from(self.bend & seven).unwrap_or(0);
            let high = u8::try_from((self.bend >> 7) & seven).unwrap_or(0);
            send(status::PITCH_BEND, channel, low, high);
        }
        let mut cc = |number: u8, value: u8| send(status::CONTROL_CHANGE, channel, number, value);
        for (number, value, start) in [
            (controller::MODULATION, self.modulation, 0),
            (PORTAMENTO_TIME, self.portamento_time, 0),
            (controller::VOLUME, self.volume, DATA_MAX),
            (BALANCE, self.balance, BALANCE_CENTRE),
            (controller::EXPRESSION, self.expression, DATA_MAX),
            (controller::SUSTAIN, if self.hold { DATA_MAX } else { 0 }, 0),
            (PORTAMENTO_SWITCH, if self.portamento { DATA_MAX } else { 0 }, 0),
        ] {
            if value != start {
                cc(number, value);
            }
        }
        match self.depth {
            Depth::Default => {}
            Depth::Controller(value) => cc(BEND_PITCH, value),
            Depth::Registered(cents) => {
                let semitones = u8::try_from(cents / CENTS_PER_SEMITONE).unwrap_or(DATA_MAX);
                let rest = u8::try_from(cents % CENTS_PER_SEMITONE).unwrap_or(0);
                cc(controller::RPN_COARSE, 0);
                cc(controller::RPN_FINE, 0);
                cc(controller::DATA_ENTRY, semitones);
                cc(controller::DATA_ENTRY_FINE, rest);
                cc(controller::RPN_COARSE, DATA_MAX);
                cc(controller::RPN_FINE, DATA_MAX);
            }
        }
    }
}
