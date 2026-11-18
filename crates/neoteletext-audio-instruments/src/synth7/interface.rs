//! What the server calls in synth7 (`server.md`, SRV-24, SRV-55, SRV-60,
//! SRV-68, SRV-69 and SRV-88), with no synthesis code: built under the
//! feature `interface` as well as `synth7`, so that the server checks and
//! packs with the receiver's own code (MIDI-21, DEC-6462).
//!
//! - [`accepts`]: whether synth7 acts on a message (S7-5, S7-37).
//! - [`Patch`], [`pack_patch`] and [`check_patch`]: a voice, its
//!   single-voice dump, and the check of one (S7-38 to S7-40, S7-45).
//! - [`build_bank`] and [`check_bank`]: the 32-voice dump of a bank, and
//!   its check (S7-41, S7-42; DEC-6465 answer 18).
//! - [`program_voice`]: the voice a program change chooses (S7-50), the one
//!   mapping the receiver and the tracker share.
//! - [`Tracker`]: each part's state as the sender keeps it, and its
//!   restatement in S7-49 item 4's order (MIDI-93 to MIDI-99).

use super::functions::{
    FIRST_FUNCTION_PARAMETER, FUNCTION_DEFAULTS, Functions, LAST_FUNCTION_PARAMETER, MAX_BEND_SEMITONES, number,
};
use super::patch::{ALL_OPERATORS_ON, BANK_VOICES, OPERATOR_SWITCH_PARAMETER, SYSEX_BYTES};
use super::sysex::{
    self, DumpError, Kind, Parameter, ParameterChange, VOICE_DUMP_BYTES, bank_dump, bank_voice, check_bank_dump,
    classify_event, read_parameter_change_event, read_voice_dump, voice_dump,
};
use crate::midi::common::{PEDAL_DOWN, VOLUME_FULL};
use crate::midi::{
    Banks, CHANNEL_MASK, CHANNELS, ChannelMessage, DATA_MAX, Message, NULL_PARAMETER, Outcome, PITCH_BEND_CENTRE,
    RPN_PITCH_BEND_SENSITIVITY, controller, status,
};

pub use super::patch::Patch;
pub use super::patch::{BankError, PatchError};

/// The most banks a synth7 sound names: the internal voices and the
/// cartridge's, which program changes 0 to 31 and 32 to 63 reach; the
/// server refuses a sound naming more (S7-50; `server.md`, SRV-58).
pub const MOST_BANKS: usize = 2;
/// The program numbers of one bank: 32 (S7-50).
const PROGRAMS_PER_BANK: u8 = 32;
const _: () = assert!(PROGRAMS_PER_BANK as usize == BANK_VOICES);

/// The controllers synth7 acts on (S7-10): the modulation wheel, breath,
/// foot, portamento time, data entry, volume, the fine data entry, the
/// sustain and portamento switches, increment and decrement, the
/// registered parameter number, and the channel mode messages 120, 121
/// and 123 to 127. Data entry, increment and decrement act only while
/// registered parameter number 0 is selected (S7-9, S7-18).
pub const CONTROLLERS: [u8; 20] = [
    controller::MODULATION,
    BREATH,
    FOOT,
    PORTAMENTO_TIME,
    controller::DATA_ENTRY,
    controller::VOLUME,
    controller::DATA_ENTRY_FINE,
    controller::SUSTAIN,
    controller::PORTAMENTO,
    controller::DATA_INCREMENT,
    controller::DATA_DECREMENT,
    controller::RPN_FINE,
    controller::RPN_COARSE,
    controller::ALL_SOUND_OFF,
    controller::RESET_ALL_CONTROLLERS,
    controller::ALL_NOTES_OFF,
    controller::OMNI_OFF,
    controller::OMNI_ON,
    controller::MONO_ON,
    controller::POLY_ON,
];

/// Controller 2, the breath controller (S7-12; DX7 manual, page 32).
pub const BREATH: u8 = 2;
/// Controller 4, the foot controller (S7-13; DX7 manual, page 32).
pub const FOOT: u8 = 4;
/// Controller 5, portamento time (S7-14; DX7 manual, page 32).
pub const PORTAMENTO_TIME: u8 = 5;

/// Whether synth7 acts on `message` (S7-5): a note off or on, a control
/// change of [`CONTROLLERS`], a program change, channel pressure, a pitch
/// bend, every data byte at most 127; or a single-voice dump or parameter
/// change that is whole and in range (S7-37 to S7-45). Everything else,
/// the 32-voice dump in a block among it, is what the server refuses with
/// `not_taken` (`server.md`, SRV-24).
#[must_use]
pub fn accepts(message: &Message<'_>) -> bool {
    match message {
        Message::Channel(ChannelMessage { status, data1, data2 }) => {
            if *data1 > DATA_MAX || *data2 > DATA_MAX {
                return false;
            }
            match status & crate::midi::KIND_MASK {
                status::NOTE_OFF
                | status::NOTE_ON
                | status::PROGRAM_CHANGE
                | status::CHANNEL_PRESSURE
                | status::PITCH_BEND => true,
                status::CONTROL_CHANGE => CONTROLLERS.contains(data1),
                _ => false,
            }
        }
        Message::SystemExclusive(after_start) => match classify_event(after_start).0 {
            Kind::VoiceDump => check_voice_event(after_start).is_ok(),
            Kind::ParameterChange => read_parameter_change_event(after_start).is_ok(),
            Kind::BankDump | Kind::Other => false,
        },
    }
}

/// Checks a single-voice dump given as a block's event holds it, the bytes
/// after its `F0`.
///
/// # Errors
///
/// As [`check_patch`].
pub fn check_voice_event(after_start: &[u8]) -> Result<(u8, Patch), DumpError> {
    let mut message = [0_u8; VOICE_DUMP_BYTES];
    if after_start.len() != VOICE_DUMP_BYTES.saturating_sub(1) {
        return Err(DumpError::Length);
    }
    for (place, &byte) in message.iter_mut().zip(core::iter::once(&sysex::START).chain(after_start)) {
        *place = byte;
    }
    read_voice_dump(&message)
}

/// The single-voice dump of `patch` on `channel` (0 to 15), as the server
/// writes an inline patch (S7-38; `server.md`, SRV-29 and SRV-55).
#[must_use]
pub fn pack_patch(patch: &Patch, channel: u8) -> [u8; VOICE_DUMP_BYTES] {
    voice_dump(patch, channel)
}

/// Checks a single-voice dump of exactly 163 bytes, `F0` to `F7`, any
/// channel, as the receiver does (S7-29, S7-38, S7-39, S7-45; SRV-27), and
/// gives its voice.
///
/// # Errors
///
/// A [`DumpError`] naming the fault, and the byte where it has one.
pub fn check_patch(message: &[u8]) -> Result<Patch, DumpError> {
    read_voice_dump(message).map(|(_, patch)| patch)
}

/// Why a bank could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    /// No voice was given.
    Empty,
    /// More than 32 voices were given (DEC-6465 answer 18; SRV-27).
    TooLong,
}

/// The 32-voice dump of `voices`, voice k at program k, on channel 0;
/// places past the voices given hold the default voice (S7-51; DEC-6465
/// answer 18; `server.md`, SRV-56).
///
/// # Errors
///
/// [`BuildError::Empty`] for no voice, [`BuildError::TooLong`] for more
/// than 32.
pub fn build_bank(voices: &[Patch]) -> Result<[u8; SYSEX_BYTES], BuildError> {
    if voices.is_empty() {
        return Err(BuildError::Empty);
    }
    if voices.len() > BANK_VOICES {
        return Err(BuildError::TooLong);
    }
    let mut all = [Patch::DEFAULT; BANK_VOICES];
    for (place, voice) in all.iter_mut().zip(voices) {
        *place = *voice;
    }
    Ok(bank_dump(&all))
}

/// Checks a bank image: exactly one 32-voice dump of 4,104 bytes, any
/// channel, whose every voice is in range (S7-41, S7-42, S7-45 to S7-48;
/// SRV-27, SRV-60).
///
/// # Errors
///
/// A [`BankError`] naming the fault, and the voice where it is in one.
pub fn check_bank(image: &[u8]) -> Result<(), BankError> {
    super::Bank::from_sysex(image).map(|_| ())
}

/// The voice program change `program` chooses from `banks` (S7-50,
/// MIDI-59): programs 0 to 31 voice `p` of bank index 0, programs 32 to 63
/// voice `p − 32` of bank index 1. Where the mapping reaches no voice (64
/// to 127, or a bank the sound does not name) the default voice and
/// [`Outcome::PassedOver`]; where the dump or the voice is faulty, the
/// default voice and [`Outcome::PatchRefused`] (MIDI-63). The whole dump's
/// frame and checksum are checked before its voice is read, so the work is
/// bounded by the image's 4,104 bytes (MIDI-66).
#[must_use]
pub fn program_voice(banks: &Banks<'_>, program: u8) -> (Patch, Outcome) {
    let bank = usize::from(program / PROGRAMS_PER_BANK);
    let voice = usize::from(program % PROGRAMS_PER_BANK);
    if bank >= MOST_BANKS {
        return (Patch::DEFAULT, Outcome::PassedOver);
    }
    let Some(image) = banks.get(bank) else { return (Patch::DEFAULT, Outcome::PassedOver) };
    match check_bank_dump(image).and_then(|data| bank_voice(data, voice)) {
        Ok(patch) => (patch, Outcome::Applied),
        Err(_) => (Patch::DEFAULT, Outcome::PatchRefused),
    }
}

/// One part's state as the sender keeps it (S7-49 item 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrackedPart {
    program: u8,
    /// The current voice, as program changes, dumps and parameter changes
    /// left it.
    voice: Patch,
    /// Whether the current voice differs from the voice its program chose.
    edited: bool,
    operators_on: u8,
    functions: Functions,
    bend: u16,
    pressure: u8,
    wheel: u8,
    breath: u8,
    foot: u8,
    volume: u8,
    sustain: u8,
    /// Controller 65's last value since the start, or none.
    portamento: Option<u8>,
    /// The registered parameter number selected, two bytes.
    registered: [u8; 2],
}

impl TrackedPart {
    const fn new(voice: Patch) -> Self {
        Self {
            program: 0,
            voice,
            edited: false,
            operators_on: ALL_OPERATORS_ON,
            functions: Functions::DEFAULT,
            bend: PITCH_BEND_CENTRE,
            pressure: 0,
            wheel: 0,
            breath: 0,
            foot: 0,
            volume: VOLUME_FULL,
            sustain: 0,
            portamento: None,
            registered: [DATA_MAX, DATA_MAX],
        }
    }

    fn rpn_zero(&self) -> bool {
        let [coarse, fine] = self.registered;
        let selected = u16::from(coarse) << 7 | u16::from(fine);
        selected != NULL_PARAMETER && selected == RPN_PITCH_BEND_SENSITIVITY
    }

    fn set_function(&mut self, number: u16, value: u8) {
        // A value out of range is not tracked, as synth7 does not take it.
        let _changed = self.functions.set(number, value);
    }

    fn control(&mut self, number: u8, value: u8) {
        let bend_range = self.functions.bend_range();
        match number {
            controller::MODULATION => self.wheel = value,
            BREATH => self.breath = value,
            FOOT => self.foot = value,
            PORTAMENTO_TIME => self.set_function(number::PORTAMENTO_TIME, portamento_time(value)),
            controller::VOLUME => self.volume = value,
            controller::SUSTAIN => self.sustain = value,
            controller::PORTAMENTO => self.portamento = Some(value),
            controller::RPN_COARSE => {
                let [_, fine] = self.registered;
                self.registered = [value, fine];
            }
            controller::RPN_FINE => {
                let [coarse, _] = self.registered;
                self.registered = [coarse, value];
            }
            controller::DATA_ENTRY if self.rpn_zero() => {
                self.set_function(number::PITCH_BEND_RANGE, value.min(MAX_BEND_SEMITONES));
            }
            controller::DATA_INCREMENT if self.rpn_zero() => {
                self.set_function(number::PITCH_BEND_RANGE, bend_range.saturating_add(1).min(MAX_BEND_SEMITONES));
            }
            controller::DATA_DECREMENT if self.rpn_zero() => {
                self.set_function(number::PITCH_BEND_RANGE, bend_range.saturating_sub(1));
            }
            controller::RESET_ALL_CONTROLLERS => {
                self.wheel = 0;
                self.breath = 0;
                self.foot = 0;
                self.pressure = 0;
                self.bend = PITCH_BEND_CENTRE;
                self.sustain = 0;
                self.portamento = Some(0);
                self.registered = [DATA_MAX, DATA_MAX];
            }
            _ => {}
        }
    }
}

/// What controller 5's value, 0 to 127, sets PORTAMENTO TIME to, 0 to 99:
/// `floor((value × 99 + 63) ÷ 127)` (S7-14; NeoTeletext's own,
/// `synth7.md` section 15 item 7).
#[must_use]
pub fn portamento_time(value: u8) -> u8 {
    let scaled = u16::from(value.min(DATA_MAX))
        .saturating_mul(u16::from(super::patch::MAX_99))
        .saturating_add(u16::from(DATA_MAX / 2))
        .checked_div(u16::from(DATA_MAX))
        .unwrap_or(0);
    u8::try_from(scaled).unwrap_or(super::patch::MAX_99)
}

/// The sixteen parts of a synth7 sound as the sender keeps them (S7-49
/// item 4; `server.md`, SRV-68): the server gives it every message it
/// writes, with the sound's banks, and writes each part's restatement from
/// it. Its size is fixed: sixteen parts of one voice each.
#[derive(Clone, Debug)]
pub struct Tracker {
    parts: [TrackedPart; CHANNELS],
}

impl Tracker {
    /// Every part as a sound starts it (S7-54): program 0 of bank index 0,
    /// or the default voice.
    #[must_use]
    pub fn new(banks: &Banks<'_>) -> Self {
        let (voice, _) = program_voice(banks, 0);
        Self { parts: [TrackedPart::new(voice); CHANNELS] }
    }

    /// Takes one message the server wrote into the sound, with the sound's
    /// banks. A message synth7 does not take changes nothing.
    pub fn take(&mut self, message: &Message<'_>, banks: &Banks<'_>) {
        match message {
            Message::Channel(message) => self.channel_message(*message, banks),
            Message::SystemExclusive(after_start) => self.system_exclusive(after_start),
        }
    }

    fn channel_message(&mut self, message: ChannelMessage, banks: &Banks<'_>) {
        if message.data1 > DATA_MAX || message.data2 > DATA_MAX {
            return;
        }
        let Some(part) = self.parts.get_mut(usize::from(message.channel())) else { return };
        match message.kind() {
            status::PROGRAM_CHANGE => {
                part.program = message.data1;
                part.voice = program_voice(banks, message.data1).0;
                part.edited = false;
                part.operators_on = ALL_OPERATORS_ON;
            }
            status::CHANNEL_PRESSURE => part.pressure = message.data1,
            status::PITCH_BEND => part.bend = message.bend(),
            status::CONTROL_CHANGE => part.control(message.data1, message.data2),
            _ => {}
        }
    }

    fn system_exclusive(&mut self, after_start: &[u8]) {
        match classify_event(after_start).0 {
            Kind::VoiceDump => {
                if let Ok((channel, voice)) = check_voice_event(after_start)
                    && let Some(part) = self.parts.get_mut(usize::from(channel))
                {
                    part.voice = voice;
                    part.edited = true;
                    part.operators_on = ALL_OPERATORS_ON;
                }
            }
            Kind::ParameterChange => {
                let Ok(ParameterChange { channel, parameter, value }) = read_parameter_change_event(after_start) else {
                    return;
                };
                let Some(part) = self.parts.get_mut(usize::from(channel)) else { return };
                match parameter {
                    Parameter::Voice(OPERATOR_SWITCH_PARAMETER) => part.operators_on = value,
                    Parameter::Voice(number) => {
                        if let Ok(voice) = part.voice.with_parameter(number, value)
                            && voice != part.voice
                        {
                            part.voice = voice;
                            part.edited = true;
                        }
                    }
                    Parameter::Function(number) => part.set_function(number, value),
                }
            }
            Kind::BankDump | Kind::Other => {}
        }
    }

    /// Part `channel`'s current voice and whether it differs from its
    /// program's (0 to 15).
    #[must_use]
    pub fn voice(&self, channel: u8) -> Option<(&Patch, bool)> {
        self.parts.get(usize::from(channel & CHANNEL_MASK)).map(|part| (&part.voice, part.edited))
    }

    /// Part `channel`'s operator switch, voice parameter 155, as the last
    /// parameter change of it left it: bit 5 is operator 1 (S7-28).
    #[must_use]
    pub fn operator_switch(&self, channel: u8) -> Option<u8> {
        self.parts.get(usize::from(channel & CHANNEL_MASK)).map(|part| part.operators_on)
    }

    /// Part `channel`'s function parameters.
    #[must_use]
    pub fn functions(&self, channel: u8) -> Option<&Functions> {
        self.parts.get(usize::from(channel & CHANNEL_MASK)).map(|part| &part.functions)
    }

    /// Gives part `channel`'s restatement to `out`, in S7-49 item 4's
    /// order, every message on the part's channel: (a) its program change,
    /// or one single-voice dump of its current voice where that differs
    /// from its program's; (b) a parameter change of voice parameter 155
    /// where the operator switch is not all on; (c) a parameter change of
    /// every function parameter that differs from S7-35, in ascending
    /// order; (d) pitch bend, channel pressure, then controllers 1, 2, 4, 7,
    /// 64 and 65 where each is not at its value at a sound's start or
    /// after reset all controllers (controller 65 whenever it has been
    /// sent or reset). At most 332 bytes as a block's events (S7-49); the
    /// server writes it whole at one tick (MIDI-98).
    pub fn restate(&self, channel: u8, out: &mut dyn FnMut(Message<'_>)) {
        let channel = channel & CHANNEL_MASK;
        let Some(part) = self.parts.get(usize::from(channel)) else { return };
        let cc = |out: &mut dyn FnMut(Message<'_>), number: u8, value: u8| {
            out(Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, number, value)));
        };
        if part.edited {
            let dump = voice_dump(&part.voice, channel);
            if let Some((_, after_start)) = dump.split_first() {
                out(Message::SystemExclusive(after_start));
            }
        } else {
            out(Message::Channel(ChannelMessage::new(status::PROGRAM_CHANGE, channel, part.program, 0)));
        }
        let parameter = |out: &mut dyn FnMut(Message<'_>), parameter: Parameter, value: u8| {
            if let Some(bytes) = sysex::parameter_change(ParameterChange { channel, parameter, value })
                && let Some((_, after_start)) = bytes.split_first()
            {
                out(Message::SystemExclusive(after_start));
            }
        };
        if part.operators_on != ALL_OPERATORS_ON {
            parameter(out, Parameter::Voice(OPERATOR_SWITCH_PARAMETER), part.operators_on);
        }
        let numbers = FIRST_FUNCTION_PARAMETER..=LAST_FUNCTION_PARAMETER;
        for ((number, &value), &default) in numbers.zip(part.functions.values()).zip(&FUNCTION_DEFAULTS) {
            if value != default {
                parameter(out, Parameter::Function(number), value);
            }
        }
        if part.bend != PITCH_BEND_CENTRE {
            let seven = u16::from(DATA_MAX);
            let low = u8::try_from(part.bend & seven).unwrap_or(0);
            let high = u8::try_from((part.bend >> 7) & seven).unwrap_or(0);
            out(Message::Channel(ChannelMessage::new(status::PITCH_BEND, channel, low, high)));
        }
        if part.pressure != 0 {
            out(Message::Channel(ChannelMessage::new(status::CHANNEL_PRESSURE, channel, part.pressure, 0)));
        }
        for (number, value, rest) in [
            (controller::MODULATION, part.wheel, 0),
            (BREATH, part.breath, 0),
            (FOOT, part.foot, 0),
            (controller::VOLUME, part.volume, VOLUME_FULL),
        ] {
            if value != rest {
                cc(out, number, value);
            }
        }
        if part.sustain >= PEDAL_DOWN {
            cc(out, controller::SUSTAIN, part.sustain);
        }
        if let Some(value) = part.portamento {
            cc(out, controller::PORTAMENTO, value);
        }
    }
}

// Named by its path so that it is found both where this module is a file
// of its own and where the feature `interface` alone reads it through a
// path from the crate root (lib.rs).
#[path = "interface/parameters.rs"]
mod parameters;
pub use parameters::{PARAMETER_COUNT, PARAMETERS, parameter_of, read_parameter};
