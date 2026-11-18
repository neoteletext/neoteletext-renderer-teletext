//! The commands a group holds and the server may send directly (RULE-118,
//! RULE-119): the bodies of record types 28 to 33 (RULE-21, RULE-99,
//! RULE-100, RULE-22, RULE-102, RULE-103), the bindings of a command's
//! fields (RULE-106), and the check of a command when a table is decoded.
//!
//! A command is a record's type and body, the same bytes whether the
//! server sends the record or a group holds it. This crate reads the
//! bodies of the six records it defines and of `ANIMATE`
//! ([`crate::animate`]); the bodies of `CONTROL`, `SPRITES` and
//! `NAPLPS_OBJECTS` are read by their own crates' readers, reached through
//! the [`BodyCheck`] its caller passes (RULE-56).

use crate::animate;
use crate::limits::{
    ALL_SEQUENCES, ALL_VARIABLES, AUDIO_CHANNELS, FIRST_PAGE, LAST_PAGE, LAST_SUBCODE, MAX_COMMAND_BINDINGS,
    MAX_COMMAND_BYTES, MAX_CONTROL, MAX_CYCLE_PERIOD, MAX_FADE_MS, MAX_FRAME, MAX_GAIN, MAX_GROUPS, MAX_LOOK,
    MAX_MIDI_DATA, MAX_PITCH_BEND, MAX_SEQUENCES, MAX_TIMER_TICKS, MIDI_DATA_BITS, PAGE_TIMER, SUBCODE_MASK, TIMERS,
    VARIABLES,
};
use crate::table::{
    FIRED_P, FIRED_ROW, FIRED_TARGET, Fault, ROW_PARAMETER, SPACE_FIRED, SPACE_SEQUENCE, TargetKind, VALUE_BYTES, Value,
};
use crate::wire::{Reader, Writer};

/// Record type of the audio profile's `CONTROL` (REQ-AUD-006), which the
/// audio crate's `TYPE_CONTROL` is asserted equal to where both are
/// linked.
pub const TYPE_CONTROL: u8 = 3;
/// Record type `ANIMATE` (ANIM-6).
pub const TYPE_ANIMATE: u8 = 23;
/// Record type `SPRITES` (ANIM-7).
pub const TYPE_SPRITES: u8 = 24;
/// Record type `NAPLPS_OBJECTS` (ANIM-11).
pub const TYPE_NAPLPS_OBJECTS: u8 = 25;
/// Record type `LOCAL_MIDI` (RULE-21).
pub const TYPE_LOCAL_MIDI: u8 = 28;
/// Record type `VARIABLE` (RULE-99).
pub const TYPE_VARIABLE: u8 = 29;
/// Record type `TIMER` (RULE-100).
pub const TYPE_TIMER: u8 = 30;
/// Record type `REPORT` (RULE-22).
pub const TYPE_REPORT: u8 = 31;
/// Record type `RUN` (RULE-102).
pub const TYPE_RUN: u8 = 32;
/// Record type `SEQUENCE` (RULE-103).
pub const TYPE_SEQUENCE: u8 = 33;

/// The bytes every command's body of types 23 to 33 begins with: the page
/// and the sub-code (RULE-112).
pub const PAGE_BYTES: usize = 2 + 2;
/// The bytes of a `LOCAL_MIDI` body (RULE-21).
pub const LOCAL_MIDI_BYTES: usize = PAGE_BYTES + 1 + 1 + 2;
/// The bytes of a `VARIABLE` body before its operand (RULE-99).
pub const VARIABLE_FIXED: usize = PAGE_BYTES + 1 + 1 + 1;
/// The most bytes of a `VARIABLE` body: with a value, or with a range.
pub const MAX_VARIABLE_BYTES: usize = VARIABLE_FIXED + VALUE_BYTES;
/// The bytes of a `TIMER` body (RULE-100).
pub const TIMER_BYTES: usize = PAGE_BYTES + 1 + 1;
/// The bytes of a `REPORT` body (RULE-22).
pub const REPORT_BYTES: usize = PAGE_BYTES + 1 + VALUE_BYTES;
/// The bytes of a `RUN` body (RULE-102): the page, the sub-code and the
/// group.
pub const RUN_BYTES: usize = PAGE_BYTES + 1;
/// The bytes of a `SEQUENCE` body (RULE-103): the page, the sub-code, the
/// operation, the sequence, the start mode, the timer and `ticks`.
pub const SEQUENCE_BYTES: usize = PAGE_BYTES + 1 + 1 + 1 + 1 + 4;
/// The most bytes of a body of types 28 to 33.
pub const MAX_BODY_BYTES: usize = MAX_VARIABLE_BYTES;
/// The bytes of a binding in a command frame (RULE-45).
pub const BINDING_BYTES: usize = 3;

/// The bytes of a `CONTROL` body (REQ-AUD-006): channel, sequence,
/// operation, handle, generation, gain, fade, flags, duck and page. The
/// audio crate's `CONTROL_BYTES` is asserted equal where both are linked.
pub const CONTROL_BYTES: usize = 1 + 2 + 1 + 1 + 2 + 1 + 2 + 1 + 1 + 2;
/// The place of a `CONTROL` body's gain (RULE-106).
pub const CONTROL_GAIN_AT: usize = 1 + 2 + 1 + 1 + 2;
/// The place of a `CONTROL` body's fade, 16 bits (RULE-106).
pub const CONTROL_FADE_AT: usize = CONTROL_GAIN_AT + 1;
/// The place of a `LOCAL_MIDI` body's status byte; its two data bytes
/// follow.
pub const LOCAL_MIDI_STATUS_AT: usize = PAGE_BYTES + 1;

/// MIDI status bytes a `LOCAL_MIDI` may carry (RULE-21; MIDI 1.0 Detailed
/// Specification, "Channel Voice Messages").
pub mod midi_status {
    /// Note off, `0x8n`.
    pub const NOTE_OFF: u8 = 0x80;
    /// Note on, `0x9n`.
    pub const NOTE_ON: u8 = 0x90;
    /// Control change, `0xBn`.
    pub const CONTROL_CHANGE: u8 = 0xB0;
    /// Program change, `0xCn`.
    pub const PROGRAM_CHANGE: u8 = 0xC0;
    /// Pitch bend, `0xEn`.
    pub const PITCH_BEND: u8 = 0xE0;
    /// The bits of a status byte that say which message it is.
    pub const KIND_MASK: u8 = 0xF0;
}

/// The fields of a command that accept a binding (RULE-106), by record.
pub mod field {
    /// `LOCAL_MIDI`: data 1, the key, the controller or the program.
    pub const MIDI_DATA1: u8 = 1;
    /// `LOCAL_MIDI`: data 2, the velocity or the controller's value.
    pub const MIDI_DATA2: u8 = 2;
    /// `LOCAL_MIDI`: the 14-bit pitch bend value.
    pub const MIDI_BEND: u8 = 3;
    /// `CONTROL`: the gain.
    pub const CONTROL_GAIN: u8 = 1;
    /// `CONTROL`: the fade, in milliseconds.
    pub const CONTROL_FADE: u8 = 2;
    /// `ANIMATE`, every operation: the target's number.
    pub const TARGET: u8 = 1;
    /// `ANIMATE`, `move`: x.
    pub const X: u8 = 2;
    /// `ANIMATE`, `move`: y.
    pub const Y: u8 = 3;
    /// `ANIMATE`, `frame`: the frame.
    pub const FRAME: u8 = 4;
    /// `ANIMATE`, `look`: the look.
    pub const LOOK: u8 = 5;
    /// `ANIMATE`, `order`: the order within the layer.
    pub const ORDER: u8 = 6;
    /// `ANIMATE`, `cycle`: the period, in ticks.
    pub const PERIOD: u8 = 7;
}

/// Whether a record of type `kind` is a sound command (RULE-96): a
/// `CONTROL` or a `LOCAL_MIDI`.
#[must_use]
pub const fn is_sound(kind: u8) -> bool {
    kind == TYPE_CONTROL || kind == TYPE_LOCAL_MIDI
}

/// Whether a group may hold a record of type `kind` (RULE-119).
#[must_use]
pub const fn is_allowed(kind: u8) -> bool {
    matches!(kind, TYPE_CONTROL | TYPE_ANIMATE | TYPE_SPRITES | TYPE_NAPLPS_OBJECTS | TYPE_LOCAL_MIDI..=TYPE_SEQUENCE)
}

/// What the reader of a `CONTROL`, `SPRITES` or `NAPLPS_OBJECTS` record
/// found in a body it accepted (RULE-56).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BodyFacts {
    /// The page number the body names; `None` for a `CONTROL` of the
    /// whole service.
    pub page: Option<u16>,
    /// The sub-code the body names; `None` for a `CONTROL`, which names a
    /// page number only (RULE-112).
    pub sub: Option<u16>,
    /// A `CONTROL`'s sequence field; 0 for the others.
    pub sequence: u16,
    /// Whether a `CONTROL`'s flags include "after the current sound".
    pub after_current: bool,
}

/// The body check a caller passes to [`crate::decode`] and
/// [`crate::check`] (RULE-56): from a record type (`CONTROL`, `SPRITES` or
/// `NAPLPS_OBJECTS`) and a body to what its own reader found, or `None`
/// when that reader refuses the body. `neoteletext-web` provides the one
/// such function.
pub type BodyCheck = fn(kind: u8, body: &[u8]) -> Option<BodyFacts>;

/// A `VARIABLE` command's operation (RULE-99), its byte in the body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VariableOperation {
    /// The variable becomes the operand: 1.
    Set,
    /// One more, saturating: 2.
    Inc,
    /// One less, saturating: 3.
    Dec,
    /// Plus the operand, saturating: 4.
    Add,
    /// Minus the operand, saturating: 5.
    Sub,
    /// 0 becomes 1, anything else 0: 6.
    Toggle,
    /// The lesser of itself and the operand: 7.
    Min,
    /// The greater of itself and the operand: 8.
    Max,
    /// Above the range becomes its least, below it its greatest: 9.
    Wrap,
    /// 0; with variable 255, all sixteen: 10.
    Clear,
}

impl VariableOperation {
    /// Every operation, in code order.
    pub const ALL: [Self; 10] = [
        Self::Set,
        Self::Inc,
        Self::Dec,
        Self::Add,
        Self::Sub,
        Self::Toggle,
        Self::Min,
        Self::Max,
        Self::Wrap,
        Self::Clear,
    ];

    /// The operation's byte.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Set => 1,
            Self::Inc => 2,
            Self::Dec => 3,
            Self::Add => 4,
            Self::Sub => 5,
            Self::Toggle => 6,
            Self::Min => 7,
            Self::Max => 8,
            Self::Wrap => 9,
            Self::Clear => 10,
        }
    }

    /// The operation of `code`, or `None` for a byte no operation has.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|operation| operation.code() == code)
    }

    /// Whether the operation takes an operand of kind `kind` (RULE-99's
    /// second table).
    #[must_use]
    pub const fn takes(self, kind: u8) -> bool {
        match self {
            Self::Set => kind >= operand_kind::CONSTANT && kind <= operand_kind::VALUE,
            Self::Add | Self::Sub | Self::Min | Self::Max => {
                kind >= operand_kind::CONSTANT && kind <= operand_kind::FIRED
            }
            Self::Inc | Self::Dec | Self::Toggle | Self::Clear => kind == operand_kind::NONE,
            Self::Wrap => kind == operand_kind::RANGE,
        }
    }
}

/// The operand kinds of a `VARIABLE` body (RULE-99's first table).
pub mod operand_kind {
    /// No operand.
    pub const NONE: u8 = 0;
    /// A constant.
    pub const CONSTANT: u8 = 1;
    /// A constant plus a variable.
    pub const VARIABLE: u8 = 2;
    /// A constant plus a value of the firing.
    pub const FIRED: u8 = 3;
    /// A value.
    pub const VALUE: u8 = 4;
    /// A range.
    pub const RANGE: u8 = 5;
}

/// A `VARIABLE` command's operand (RULE-99).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operand {
    /// None: `inc`, `dec`, `toggle`, `clear`.
    None,
    /// A constant.
    Constant(i32),
    /// A constant plus a variable's working value, saturating.
    Variable {
        /// The constant.
        constant: i32,
        /// The variable, 0 to 15.
        variable: u8,
    },
    /// A constant plus a value of the firing, saturating (RULE-116).
    Fired {
        /// The constant.
        constant: i32,
        /// The property, 0 to 7.
        property: u8,
    },
    /// A value, as the snapshot holds it; `set` only.
    Value(Value),
    /// A range `low ≤ high`; `wrap` only.
    Range {
        /// The least.
        low: i32,
        /// The greatest.
        high: i32,
    },
}

impl Operand {
    /// The operand's kind byte.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        match self {
            Self::None => operand_kind::NONE,
            Self::Constant(_) => operand_kind::CONSTANT,
            Self::Variable { .. } => operand_kind::VARIABLE,
            Self::Fired { .. } => operand_kind::FIRED,
            Self::Value(_) => operand_kind::VALUE,
            Self::Range { .. } => operand_kind::RANGE,
        }
    }
}

/// When a `SEQUENCE` start makes the sequence begin (RULE-103): its start
/// tick `S`, for a start applied in a handling of tick `t`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartMode {
    /// The first tick step after the handling, `t + 1`: mode 0.
    Next,
    /// A page tick: mode 1. One earlier than `t + 1` has passed, and the
    /// start is at `t + 1`, counted late; one more than `MAX_START_TICKS`
    /// after `t` is not started, and is counted as skipped.
    At(u32),
    /// The named timer's next expiry of a period (RULE-15): the first tick
    /// from `t + 1` on at which an `every` of that period on it would be
    /// due; `t + 1` while it is stopped. Mode 2.
    Beat {
        /// The timer, 0 to 7, or 255 for the page timer.
        timer: u8,
        /// The period, 1 to `MAX_TIMER_TICKS`.
        ticks: u32,
    },
}

/// A command of one of the six record types this crate defines, without
/// its page.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    /// `LOCAL_MIDI` (RULE-21): one MIDI channel voice message to the local
    /// sound on an audio channel.
    LocalMidi {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// The status byte: note off, note on, control change, program
        /// change or pitch bend, with the part's MIDI channel less one.
        status: u8,
        /// The first data byte, 0 to 127.
        data1: u8,
        /// The second, 0 to 127; 0 for a program change.
        data2: u8,
    },
    /// `VARIABLE` (RULE-99): changes one variable.
    Variable {
        /// What is done.
        operation: VariableOperation,
        /// The variable, 0 to 15; 255 for all sixteen, with `clear` only.
        variable: u8,
        /// With what.
        operand: Operand,
    },
    /// `TIMER` (RULE-100): starts or stops a timer.
    Timer {
        /// Start, or stop.
        start: bool,
        /// The timer, 0 to 7.
        timer: u8,
    },
    /// `REPORT` (RULE-22): the firing for `None`, or one named value.
    Report {
        /// The value reported.
        value: Option<Value>,
    },
    /// `RUN` (RULE-102): runs a group now, in place.
    Run {
        /// The group.
        group: u8,
    },
    /// `SEQUENCE` (RULE-103): starts or stops a sequence.
    Sequence {
        /// Start, or stop.
        start: bool,
        /// The sequence, 0 to 7; 255 for every sequence, with stop only.
        sequence: u8,
        /// When a start begins; [`StartMode::Next`] with stop.
        mode: StartMode,
    },
}

impl Command {
    /// The command's record type.
    #[must_use]
    pub const fn kind(&self) -> u8 {
        match self {
            Self::LocalMidi { .. } => TYPE_LOCAL_MIDI,
            Self::Variable { .. } => TYPE_VARIABLE,
            Self::Timer { .. } => TYPE_TIMER,
            Self::Report { .. } => TYPE_REPORT,
            Self::Run { .. } => TYPE_RUN,
            Self::Sequence { .. } => TYPE_SEQUENCE,
        }
    }
}

/// The operation bytes of `TIMER` and `SEQUENCE` (RULE-100, RULE-103).
const START: u8 = 1;
const STOP: u8 = 2;
/// The `what` bytes of `REPORT` (RULE-22).
const WHAT_FIRING: u8 = 0;
const WHAT_VALUE: u8 = 1;
/// The start mode bytes of `SEQUENCE` (RULE-103).
const MODE_NEXT: u8 = 0;
const MODE_AT: u8 = 1;
const MODE_BEAT: u8 = 2;

/// Reads the page and the sub-code a body begins with (RULE-112).
pub(crate) fn read_page(reader: &mut Reader<'_>) -> Result<(u16, u16), Fault> {
    let page = reader.u16()?;
    if !(FIRST_PAGE..=LAST_PAGE).contains(&page) {
        return Err(Fault::Range("page"));
    }
    let sub = reader.u16()?;
    if sub > LAST_SUBCODE || sub & !SUBCODE_MASK != 0 {
        return Err(Fault::Range("sub-code"));
    }
    Ok((page, sub))
}

/// Checks a MIDI message a `LOCAL_MIDI` carries (RULE-21): one channel
/// voice message, with each data byte 0 to 127.
fn check_midi(status: u8, data1: u8, data2: u8) -> Result<(), Fault> {
    let kind = status & midi_status::KIND_MASK;
    let allowed = matches!(
        kind,
        midi_status::NOTE_OFF
            | midi_status::NOTE_ON
            | midi_status::CONTROL_CHANGE
            | midi_status::PROGRAM_CHANGE
            | midi_status::PITCH_BEND
    );
    if !allowed {
        return Err(Fault::Range("midi status"));
    }
    if data1 > MAX_MIDI_DATA || data2 > MAX_MIDI_DATA {
        return Err(Fault::Range("midi data"));
    }
    if kind == midi_status::PROGRAM_CHANGE && data2 != 0 {
        return Err(Fault::Range("program change's second byte"));
    }
    Ok(())
}

/// Reads the body of a record of type `kind`, 28 to 33: its page, its
/// sub-code and the command.
///
/// # Errors
/// [`Fault::Command`] for another type; [`Fault::Short`] and
/// [`Fault::Trailing`] for a body that is not exactly its length; and
/// [`Fault::Range`] or [`Fault::Reserved`] for a field outside its range
/// or a pairing RULE-99 does not give.
pub fn decode(kind: u8, body: &[u8]) -> Result<(u16, u16, Command), Fault> {
    let mut reader = Reader::new(body);
    let (page, sub) = read_page(&mut reader)?;
    let command = match kind {
        TYPE_LOCAL_MIDI => {
            let channel = reader.u8()?;
            if usize::from(channel) >= AUDIO_CHANNELS {
                return Err(Fault::Range("audio channel"));
            }
            let (status, data1, data2) = (reader.u8()?, reader.u8()?, reader.u8()?);
            check_midi(status, data1, data2)?;
            Command::LocalMidi { channel, status, data1, data2 }
        }
        TYPE_VARIABLE => read_variable(&mut reader)?,
        TYPE_TIMER => {
            let start = read_start(&mut reader)?;
            let timer = reader.u8()?;
            if usize::from(timer) >= TIMERS {
                return Err(Fault::Range("timer"));
            }
            Command::Timer { start, timer }
        }
        TYPE_REPORT => {
            let what = reader.u8()?;
            let bytes = reader.take(VALUE_BYTES)?;
            match what {
                WHAT_FIRING if bytes.iter().all(|byte| *byte == 0) => Command::Report { value: None },
                WHAT_FIRING => return Err(Fault::Reserved("report's value")),
                WHAT_VALUE => {
                    let value = Value::from_bytes(bytes)?;
                    value.check(MAX_SEQUENCES)?;
                    Command::Report { value: Some(value) }
                }
                _ => return Err(Fault::Range("report's what")),
            }
        }
        TYPE_RUN => read_run(&mut reader)?,
        TYPE_SEQUENCE => read_sequence(&mut reader)?,
        _ => return Err(Fault::Command("not a record of types 28 to 33")),
    };
    if reader.finished() { Ok((page, sub, command)) } else { Err(Fault::Trailing) }
}

fn read_start(reader: &mut Reader<'_>) -> Result<bool, Fault> {
    match reader.u8()? {
        START => Ok(true),
        STOP => Ok(false),
        _ => Err(Fault::Range("start or stop")),
    }
}

fn read_variable(reader: &mut Reader<'_>) -> Result<Command, Fault> {
    let operation = VariableOperation::from_code(reader.u8()?).ok_or(Fault::Range("variable operation"))?;
    let variable = reader.u8()?;
    let named =
        usize::from(variable) < VARIABLES || (variable == ALL_VARIABLES && operation == VariableOperation::Clear);
    if !named {
        return Err(Fault::Range("variable"));
    }
    let kind = reader.u8()?;
    if !operation.takes(kind) {
        return Err(Fault::Range("operand kind"));
    }
    let operand = match kind {
        operand_kind::NONE => Operand::None,
        operand_kind::CONSTANT => Operand::Constant(reader.i32()?),
        operand_kind::VARIABLE => {
            let (constant, variable) = (reader.i32()?, reader.u8()?);
            if usize::from(variable) >= VARIABLES {
                return Err(Fault::Range("operand's variable"));
            }
            Operand::Variable { constant, variable }
        }
        operand_kind::FIRED => {
            let (constant, property) = (reader.i32()?, reader.u8()?);
            if u16::from(property) > FIRED_P {
                return Err(Fault::Range("operand's fired property"));
            }
            Operand::Fired { constant, property }
        }
        operand_kind::VALUE => {
            let value = Value::read(reader)?;
            value.check(MAX_SEQUENCES)?;
            Operand::Value(value)
        }
        operand_kind::RANGE => {
            let (low, high) = (reader.i32()?, reader.i32()?);
            if low > high {
                return Err(Fault::Range("wrap's range"));
            }
            Operand::Range { low, high }
        }
        _ => return Err(Fault::Range("operand kind")),
    };
    Ok(Command::Variable { operation, variable, operand })
}

fn read_run(reader: &mut Reader<'_>) -> Result<Command, Fault> {
    let group = reader.u8()?;
    if usize::from(group) >= MAX_GROUPS {
        return Err(Fault::Limit("MAX_GROUPS"));
    }
    Ok(Command::Run { group })
}

fn read_sequence(reader: &mut Reader<'_>) -> Result<Command, Fault> {
    let start = read_start(reader)?;
    let sequence = reader.u8()?;
    let named = usize::from(sequence) < MAX_SEQUENCES || (sequence == ALL_SEQUENCES && !start);
    if !named {
        return Err(Fault::Range("sequence"));
    }
    let (mode, timer, ticks) = (reader.u8()?, reader.u8()?, reader.u32()?);
    if (!start || mode != MODE_BEAT) && timer != 0 {
        return Err(Fault::Reserved("sequence's timer"));
    }
    let mode = match mode {
        MODE_NEXT if ticks == 0 => StartMode::Next,
        MODE_NEXT => return Err(Fault::Reserved("sequence's ticks")),
        _ if !start => return Err(Fault::Reserved("a stop's start mode")),
        MODE_AT => StartMode::At(ticks),
        MODE_BEAT => {
            if usize::from(timer) >= TIMERS && timer != PAGE_TIMER {
                return Err(Fault::Range("sequence's timer"));
            }
            if !(1..=MAX_TIMER_TICKS).contains(&ticks) {
                return Err(Fault::Limit("MAX_TIMER_TICKS"));
            }
            StartMode::Beat { timer, ticks }
        }
        _ => return Err(Fault::Range("sequence's start mode")),
    };
    Ok(Command::Sequence { start, sequence, mode })
}

/// Writes the body of `command` for the page `page` and `sub` at the
/// front of `out`, and how many bytes it took. What is written is read
/// back, so nothing is written that [`decode`] refuses.
///
/// # Errors
/// What [`decode`] refuses, and [`Fault::Full`] when `out` is too small.
pub fn encode(command: &Command, page: u16, sub: u16, out: &mut [u8]) -> Result<usize, Fault> {
    let mut writer = Writer::new(out);
    writer.u16(page)?;
    writer.u16(sub)?;
    let start = |start: bool| if start { START } else { STOP };
    match *command {
        Command::LocalMidi { channel, status, data1, data2 } => writer.bytes(&[channel, status, data1, data2])?,
        Command::Variable { operation, variable, operand } => {
            writer.bytes(&[operation.code(), variable, operand.kind()])?;
            match operand {
                Operand::None => {}
                Operand::Constant(constant) => writer.i32(constant)?,
                Operand::Variable { constant, variable } => {
                    writer.i32(constant)?;
                    writer.u8(variable)?;
                }
                Operand::Fired { constant, property } => {
                    writer.i32(constant)?;
                    writer.u8(property)?;
                }
                Operand::Value(value) => value.write(&mut writer)?,
                Operand::Range { low, high } => {
                    writer.i32(low)?;
                    writer.i32(high)?;
                }
            }
        }
        Command::Timer { start: on, timer } => writer.bytes(&[start(on), timer])?,
        Command::Report { value: None } => {
            writer.u8(WHAT_FIRING)?;
            writer.bytes(&[0; VALUE_BYTES])?;
        }
        Command::Report { value: Some(value) } => {
            writer.u8(WHAT_VALUE)?;
            value.write(&mut writer)?;
        }
        Command::Run { group } => writer.u8(group)?,
        Command::Sequence { start: on, sequence, mode } => {
            let (code, timer, ticks) = match mode {
                StartMode::Next => (MODE_NEXT, 0, 0),
                StartMode::At(tick) => (MODE_AT, 0, tick),
                StartMode::Beat { timer, ticks } => (MODE_BEAT, timer, ticks),
            };
            writer.bytes(&[start(on), sequence, code, timer])?;
            writer.u32(ticks)?;
        }
    }
    let length = writer.used();
    let back = decode(command.kind(), out.get(..length).ok_or(Fault::Full)?)?;
    if back == (page, sub, *command) { Ok(length) } else { Err(Fault::Unreadable) }
}

/// One binding of a command (RULE-106): a field and the source added to
/// the constant the body holds there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Binding {
    /// The place of the operation within an `ANIMATE` record, from 0; 0
    /// for every other record.
    pub item: u8,
    /// The field's code ([`field`]).
    pub field: u8,
    /// The source's byte ([`Source`]).
    pub source: u8,
}

/// The source of a binding (RULE-106), its byte in the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// A variable `v0` to `v15`: `0x10` to `0x1F`.
    Variable(u8),
    /// The row parameter `p`: `0x20` (DEC-6635 item 8; `0x21` to `0x23`,
    /// the second form's `p1` to `p3`, are withdrawn and refused).
    Parameter,
    /// A value of the firing, the key, the target, the row or the cue:
    /// `0x30` to `0x33`.
    Fired(u8),
}

/// The source byte of variable `v0`.
const SOURCE_VARIABLE: u8 = 0x10;
/// The source byte of the firing's key.
const SOURCE_FIRED: u8 = 0x30;
/// The values of the firing a binding may name: the key, the target, the
/// row and the cue (RULE-106).
const FIRED_SOURCES: u8 = 4;

impl Source {
    /// The source's byte.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Variable(variable) => SOURCE_VARIABLE | variable,
            Self::Parameter => ROW_PARAMETER,
            Self::Fired(property) => SOURCE_FIRED | property,
        }
    }

    /// The source of `code`, or `None` for a byte no source has.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        let within = |first: u8, count: usize| code.checked_sub(first).filter(|index| usize::from(*index) < count);
        if let Some(variable) = within(SOURCE_VARIABLE, VARIABLES) {
            return Some(Self::Variable(variable));
        }
        if code == ROW_PARAMETER {
            return Some(Self::Parameter);
        }
        within(SOURCE_FIRED, usize::from(FIRED_SOURCES)).map(Self::Fired)
    }
}

impl Binding {
    /// A binding of field `field` of a record that is not an `ANIMATE`.
    #[must_use]
    pub const fn of(field: u8, source: Source) -> Self {
        Self { item: 0, field, source: source.code() }
    }

    /// A binding of field `field` of operation `item` of an `ANIMATE`.
    #[must_use]
    pub const fn of_operation(item: u8, field: u8, source: Source) -> Self {
        Self { item, field, source: source.code() }
    }
}

/// Where a bound field lies in a body, and the range it is held to.
#[derive(Clone, Copy)]
struct Place {
    /// The field's first byte.
    at: usize,
    /// How it is written.
    form: Form,
    /// The least and the greatest value (RULE-106's table).
    least: i32,
    greatest: i32,
}

/// How a bound field is written in a body.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    /// One unsigned byte.
    U8,
    /// One signed byte.
    I8,
    /// Sixteen unsigned bits.
    U16,
    /// Sixteen signed bits.
    I16,
    /// Two data bytes of seven bits, the low first (RULE-21).
    Bend,
    /// A target's number: an identity, not held to a range (RULE-106).
    Target,
}

impl Place {
    /// The constant the body holds in the field.
    fn read(&self, body: &[u8]) -> i32 {
        let byte = |offset: usize| body.get(self.at.saturating_add(offset)).copied().unwrap_or(0);
        match self.form {
            Form::U8 => i32::from(byte(0)),
            Form::I8 => i32::from(i8::from_be_bytes([byte(0)])),
            Form::U16 | Form::Target => i32::from(u16::from_be_bytes([byte(0), byte(1)])),
            Form::I16 => i32::from(i16::from_be_bytes([byte(0), byte(1)])),
            Form::Bend => i32::from(byte(0)) | i32::from(byte(1)).checked_shl(MIDI_DATA_BITS).unwrap_or(0),
        }
    }

    /// Writes `value`, which is within the field's range, into the field.
    fn write(&self, body: &mut [u8], value: i32) {
        let mut put = |offset: usize, byte: u8| {
            if let Some(slot) = body.get_mut(self.at.saturating_add(offset)) {
                *slot = byte;
            }
        };
        match self.form {
            Form::U8 => put(0, u8::try_from(value).unwrap_or(0)),
            Form::I8 => put(0, i8::try_from(value).unwrap_or(0).to_be_bytes()[0]),
            Form::U16 | Form::Target => {
                let [high, low] = u16::try_from(value).unwrap_or(0).to_be_bytes();
                put(0, high);
                put(1, low);
            }
            Form::I16 => {
                let [high, low] = i16::try_from(value).unwrap_or(0).to_be_bytes();
                put(0, high);
                put(1, low);
            }
            Form::Bend => {
                let value = u16::try_from(value).unwrap_or(0);
                put(0, u8::try_from(value & u16::from(MAX_MIDI_DATA)).unwrap_or(0));
                put(1, u8::try_from(value.checked_shr(MIDI_DATA_BITS).unwrap_or(0)).unwrap_or(0));
            }
        }
    }
}

/// Where operation `item` of an `ANIMATE` body begins, its code and the
/// kind byte of its target; `None` when the body has no such operation.
/// Only each operation's length is read, never its fields, so a bound
/// field's constant may be anything (RULE-106).
fn operation_at(body: &[u8], item: u8) -> Option<(usize, u8, u8)> {
    let count = body.get(animate::RECORD_COUNT_AT).copied()?;
    if item >= count {
        return None;
    }
    let mut at = animate::RECORD_FIXED;
    for _ in 0..item {
        at = at.checked_add(animate::length_of(body.get(at).copied()?)?)?;
    }
    let code = body.get(at).copied()?;
    animate::length_of(code)?;
    Some((at, code, body.get(at.checked_add(1)?).copied()?))
}

/// Where the field a binding names lies in a body of type `kind`
/// (RULE-106's table), or why the binding is refused.
fn place_of(kind: u8, body: &[u8], binding: Binding) -> Result<Place, Fault> {
    let place = |at: usize, form: Form, least: i32, greatest: i32| Ok(Place { at, form, least, greatest });
    let data = i32::from(MAX_MIDI_DATA);
    if kind != TYPE_ANIMATE && binding.item != 0 {
        return Err(Fault::Binding("item of a record with no operations"));
    }
    match (kind, binding.field) {
        (TYPE_LOCAL_MIDI, bound) => {
            let status = body.get(LOCAL_MIDI_STATUS_AT).copied().unwrap_or(0) & midi_status::KIND_MASK;
            let first = LOCAL_MIDI_STATUS_AT.saturating_add(1);
            match bound {
                field::MIDI_DATA1 if status != midi_status::PITCH_BEND => place(first, Form::U8, 0, data),
                field::MIDI_DATA2 if status != midi_status::PITCH_BEND && status != midi_status::PROGRAM_CHANGE => {
                    place(first.saturating_add(1), Form::U8, 0, data)
                }
                field::MIDI_BEND if status == midi_status::PITCH_BEND => {
                    place(first, Form::Bend, 0, i32::from(MAX_PITCH_BEND))
                }
                _ => Err(Fault::Binding("field of this MIDI message")),
            }
        }
        (TYPE_CONTROL, field::CONTROL_GAIN) => place(CONTROL_GAIN_AT, Form::U8, 0, i32::from(MAX_GAIN)),
        (TYPE_CONTROL, field::CONTROL_FADE) => place(CONTROL_FADE_AT, Form::U16, 0, i32::from(MAX_FADE_MS)),
        (TYPE_ANIMATE, bound) => {
            let (at, code, target) =
                operation_at(body, binding.item).ok_or(Fault::Binding("item beyond the operations"))?;
            let own = at.saturating_add(animate::OPERATION_FIXED);
            match (bound, code) {
                (field::TARGET, _) => {
                    let greatest = if target == TargetKind::Control.code() { MAX_CONTROL } else { u16::MAX };
                    place(at.saturating_add(2), Form::Target, 1, i32::from(greatest))
                }
                // `move`: a flags byte, then x and y.
                (field::X, animate::code::MOVE) => {
                    place(own.saturating_add(1), Form::I16, i32::from(i16::MIN), i32::from(i16::MAX))
                }
                (field::Y, animate::code::MOVE) => {
                    place(own.saturating_add(3), Form::I16, i32::from(i16::MIN), i32::from(i16::MAX))
                }
                (field::FRAME, animate::code::FRAME) => place(own, Form::U8, 0, i32::from(MAX_FRAME)),
                (field::LOOK, animate::code::LOOK) => place(own, Form::U8, 0, i32::from(MAX_LOOK)),
                // `order`: the layer, then the order.
                (field::ORDER, animate::code::ORDER) => {
                    place(own.saturating_add(1), Form::I8, i32::from(i8::MIN), i32::from(i8::MAX))
                }
                // `cycle`: the first and the last frame, then the period.
                (field::PERIOD, animate::code::CYCLE) => {
                    place(own.saturating_add(2), Form::U16, 1, i32::from(MAX_CYCLE_PERIOD))
                }
                _ => Err(Fault::Binding("field of this operation")),
            }
        }
        _ => Err(Fault::Binding("record or field that takes none")),
    }
}

/// Checks a command's bindings against RULE-106's table: each names a
/// field its record has and a source the field allows, and no field is
/// bound twice.
///
/// # Errors
/// [`Fault::Binding`], and [`Fault::Limit`] for more than
/// `MAX_COMMAND_BINDINGS`.
pub fn check_bindings(kind: u8, body: &[u8], bindings: &[Binding]) -> Result<(), Fault> {
    if bindings.len() > MAX_COMMAND_BINDINGS {
        return Err(Fault::Limit("MAX_COMMAND_BINDINGS"));
    }
    for (binding, index) in bindings.iter().zip(0_usize..) {
        let place = place_of(kind, body, *binding)?;
        let source = Source::from_code(binding.source).ok_or(Fault::Binding("source"))?;
        // A target's number is an identity: a variable choosing it would
        // be a computed address (RULE-1). A row's `p`, the firing's row
        // and the firing's target are constants of the table or the
        // viewer's own event (RULE-105, RULE-106).
        let identity = matches!(source, Source::Parameter | Source::Fired(FIRED_TARGET_SOURCE | FIRED_ROW_SOURCE));
        if place.form == Form::Target && !identity {
            return Err(Fault::Binding("a target's number takes the row's p, the firing's row or its target"));
        }
        let twice =
            bindings.iter().take(index).any(|earlier| (earlier.item, earlier.field) == (binding.item, binding.field));
        if twice {
            return Err(Fault::Binding("field bound twice"));
        }
    }
    Ok(())
}

/// The firing's target as a binding's source (RULE-106: `0x31`).
const FIRED_TARGET_SOURCE: u8 = FIRED_TARGET as u8;
/// The firing's row as a binding's source (RULE-106: `0x32`).
const FIRED_ROW_SOURCE: u8 = FIRED_ROW as u8;

/// What became of a command when its bindings were applied.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bound {
    /// The bytes of the body to apply; 0 when nothing of it is left.
    pub length: usize,
    /// The operations of an `ANIMATE` left out because a bound target
    /// number was outside its range (RULE-106): each is counted as
    /// skipped.
    pub skipped: usize,
}

/// What a bound field is given.
enum Fill<'a> {
    /// The constant plus the source's value (RULE-106).
    Sources(&'a dyn Fn(Source) -> i32),
    /// The least value of its range, whatever the constant: what the body
    /// is checked with when a table is decoded.
    Least,
}

/// Copies `body` to `out` with each bound field filled.
fn fill(kind: u8, body: &[u8], bindings: &[Binding], fill: &Fill<'_>, out: &mut [u8; MAX_COMMAND_BYTES]) -> Bound {
    let Some(copy) = out.get_mut(..body.len()) else { return Bound { length: 0, skipped: 0 } };
    for (to, from) in copy.iter_mut().zip(body) {
        *to = *from;
    }
    // The operations whose bound target number named nothing.
    let mut dropped = 0_u64;
    for binding in bindings {
        let Ok(place) = place_of(kind, body, *binding) else { continue };
        let value = match fill {
            Fill::Least => place.least,
            Fill::Sources(resolve) => {
                let source = Source::from_code(binding.source).map_or(0, resolve);
                place.read(body).saturating_add(source)
            }
        };
        if place.form == Form::Target && !(place.least..=place.greatest).contains(&value) {
            dropped |= 1_u64.checked_shl(u32::from(binding.item)).unwrap_or(0);
            continue;
        }
        place.write(copy, value.clamp(place.least, place.greatest));
    }
    if dropped == 0 {
        return Bound { length: body.len(), skipped: 0 };
    }
    // Close the record up over the operations left out.
    let count = body.get(animate::RECORD_COUNT_AT).copied().unwrap_or(0);
    let (mut from, mut to, mut kept) = (animate::RECORD_FIXED, animate::RECORD_FIXED, 0_u8);
    for item in 0..count {
        let length = out.get(from).copied().and_then(animate::length_of).unwrap_or(0);
        if dropped.checked_shr(u32::from(item)).unwrap_or(0) & 1 == 0 {
            for offset in 0..length {
                let byte = out.get(from.saturating_add(offset)).copied().unwrap_or(0);
                if let Some(slot) = out.get_mut(to.saturating_add(offset)) {
                    *slot = byte;
                }
            }
            to = to.saturating_add(length);
            kept = kept.saturating_add(1);
        }
        from = from.saturating_add(length);
    }
    if let Some(slot) = out.get_mut(animate::RECORD_COUNT_AT) {
        *slot = kept;
    }
    let skipped = usize::from(count.saturating_sub(kept));
    Bound { length: if kept == 0 { 0 } else { to }, skipped }
}

// RULE-106: an `ANIMATE` operation's place has a bit in the word that
// marks those left out.
const _: () = assert!(crate::limits::MAX_ANIMATE_OPERATIONS <= u64::BITS as usize);

/// Applies a command's bindings (RULE-106): `out` is given the body with
/// each bound field holding its constant plus its source's value, added
/// with saturation and held to the field's range. A bound target number
/// outside its range leaves that one operation out.
pub fn apply_bindings(
    kind: u8,
    body: &[u8],
    bindings: &[Binding],
    resolve: &dyn Fn(Source) -> i32,
    out: &mut [u8; MAX_COMMAND_BYTES],
) -> Bound {
    fill(kind, body, bindings, &Fill::Sources(resolve), out)
}

/// What a command's check needs of the table it is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Context {
    /// The table's page number.
    pub page: u16,
    /// The table's sub-code.
    pub sub: u16,
    /// How many groups the table has.
    pub groups: usize,
    /// How many sequences the table has.
    pub sequences: usize,
}

/// Checks one command of a group (RULE-118, RULE-119, RULE-106): its type
/// is one a group may hold; its body is one its record's reader accepts,
/// read with each bound field at the least value of its range; its page
/// is the table's; and its bindings are RULE-106's.
///
/// # Errors
/// [`Fault::Command`], [`Fault::Binding`], [`Fault::Reference`] for a
/// group or a sequence the table does not have, and what the body's
/// reader refuses.
pub fn check(
    kind: u8,
    body: &[u8],
    bindings: &[Binding],
    context: &Context,
    body_check: BodyCheck,
) -> Result<(), Fault> {
    if !is_allowed(kind) {
        return Err(Fault::Command("record type a group may not hold"));
    }
    if body.is_empty() || body.len() > MAX_COMMAND_BYTES {
        return Err(Fault::Limit("MAX_COMMAND_BYTES"));
    }
    check_bindings(kind, body, bindings)?;
    let mut neutral = [0_u8; MAX_COMMAND_BYTES];
    let length = fill(kind, body, bindings, &Fill::Least, &mut neutral).length;
    let neutral = neutral.get(..length).unwrap_or(&[]);
    let (page, sub) = match kind {
        TYPE_CONTROL | TYPE_SPRITES | TYPE_NAPLPS_OBJECTS => {
            let facts = body_check(kind, neutral).ok_or(Fault::Command("body its record's reader refuses"))?;
            if kind == TYPE_CONTROL && (facts.sequence != 0 || facts.after_current) {
                // RULE-119: a group's `CONTROL` has no sequence and never
                // waits for the current sound.
                return Err(Fault::Command("CONTROL with a sequence or after the current sound"));
            }
            (facts.page, facts.sub.or(Some(context.sub)))
        }
        TYPE_ANIMATE => {
            let (header, _) = animate::read_record(neutral)?;
            (Some(header.page), Some(header.sub))
        }
        _ => {
            let (page, sub, command) = decode(kind, neutral)?;
            check_references(&command, context)?;
            (Some(page), Some(sub))
        }
    };
    // RULE-119: a group cannot act on another page, or on the service.
    if page == Some(context.page) && sub == Some(context.sub) {
        Ok(())
    } else {
        Err(Fault::Command("names another page"))
    }
}

/// Checks that what a command names is in its table (RULE-70).
fn check_references(command: &Command, context: &Context) -> Result<(), Fault> {
    let value = |value: &Value| {
        if value.space == SPACE_SEQUENCE && usize::from(value.a) >= context.sequences {
            Err(Fault::Reference("value's sequence"))
        } else {
            Ok(())
        }
    };
    match command {
        Command::Run { group, .. } if usize::from(*group) >= context.groups => Err(Fault::Reference("run's group")),
        Command::Sequence { sequence, .. }
            if *sequence != ALL_SEQUENCES && usize::from(*sequence) >= context.sequences =>
        {
            Err(Fault::Reference("sequence"))
        }
        Command::Report { value: Some(read) } | Command::Variable { operand: Operand::Value(read), .. } => value(read),
        _ => Ok(()),
    }
}

/// Whether a value is one of the firing's (RULE-116), which a command the
/// server sends directly reads as 0.
#[must_use]
pub const fn reads_firing(value: &Value) -> bool {
    value.space == SPACE_FIRED
}

#[cfg(test)]
mod tests;
