//! The `PAGE_STATE` payload after its stamp (RULE-47): a page's local
//! state, once, in answer to a state query.
//!
//! The input protocol writes the stamp, the page and sub-code on screen,
//! before these bytes; this crate has no dependency (RULE-56), so the
//! stamp's form is not its own.

use crate::Fault;
use crate::bounded::Bounded;
use crate::limits::{
    MAX_ARMED_RUNS, MAX_CONTROL, MAX_FRAME, MAX_LAYER, MAX_LOOK, MAX_SEQUENCES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES,
    TIMERS, VARIABLES,
};
use crate::table::{TARGET_BYTES as TARGET_NAME_BYTES, Target, TargetKind, VALUE_BYTES, Value};
use crate::wire::{Reader, Writer};

/// The bytes of one target's report: its kind and number, its flags, its
/// look, its frame, x, y, its layer and its order.
pub const TARGET_BYTES: usize = TARGET_NAME_BYTES + 1 + 1 + 1 + 2 + 2 + 1 + 1;
/// The bytes of one timer's entry: its flags and its elapsed ticks.
pub const TIMER_BYTES: usize = 1 + 4;
/// The bytes of one sequence's entry: its flags, its passes and its
/// position.
pub const SEQUENCE_BYTES: usize = 1 + 4 + 4;
/// The most bytes of the payload after the stamp: the identifier, the
/// generation, the tick, the sixteen variables, the eight timers, and the
/// sequences, the count of armed runs, the targets and the values, each
/// list with its count byte.
pub const MAX_PAYLOAD_BYTES: usize = 4
    + 2
    + 4
    + VARIABLES * 4
    + TIMERS * TIMER_BYTES
    + 1
    + MAX_SEQUENCES * SEQUENCE_BYTES
    + 1
    + 1
    + MAX_TABLE_TARGETS * TARGET_BYTES
    + 1
    + MAX_TABLE_VALUES * (VALUE_BYTES + 4);

const EXISTS: u8 = 1;
const SHOWN: u8 = 1 << 1;
const CYCLING: u8 = 1 << 2;
const RUNNING: u8 = 1;

/// A timer in a state answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TimerState {
    /// Whether it runs.
    pub running: bool,
    /// The ticks since its start; 0 while stopped.
    pub elapsed: u32,
}

/// A sequence in a state answer (RULE-117).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SequenceState {
    /// Whether it runs.
    pub running: bool,
    /// The passes completed since its start.
    pub passes: u32,
    /// Its position in ticks within the pass; 0 while stopped.
    pub position: u32,
}

/// A control or an object in a state answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetReport {
    /// Which control or object.
    pub target: Target,
    /// Whether it exists on the page.
    pub exists: bool,
    /// Whether it is shown.
    pub shown: bool,
    /// Whether it is cycling.
    pub cycling: bool,
    /// The look in force, 0 to 3.
    pub look: u8,
    /// The frame in force, 0 to 15.
    pub frame: u8,
    /// Its position.
    pub x: i16,
    /// See `x`.
    pub y: i16,
    /// Its layer, 0 to 3.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
}

/// The state of a page that has a table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableState {
    /// The table's generation, 1 to 65,535.
    pub generation: u16,
    /// The page tick of the snapshot reported.
    pub tick: u32,
    /// The sixteen variables, `v0` first.
    pub variables: [i32; VARIABLES],
    /// The eight timers, `t0` first.
    pub timers: [TimerState; TIMERS],
    /// The table's sequences, in number order.
    pub sequences: Bounded<SequenceState, MAX_SEQUENCES>,
    /// The armed runs waiting, 0 to `MAX_ARMED_RUNS`.
    pub armed: u8,
    /// Each control and object the table names by a constant, in the
    /// order first named.
    pub targets: Bounded<TargetReport, MAX_TABLE_TARGETS>,
    /// Each distinct value the table reads, in the order first read, with
    /// its reading.
    pub values: Bounded<(Value, i32), MAX_TABLE_VALUES>,
}

impl TableState {
    /// A state with every variable 0, every timer stopped and every list
    /// empty.
    #[must_use]
    pub const fn new(generation: u16, tick: u32) -> Self {
        Self {
            generation,
            tick,
            variables: [0; VARIABLES],
            timers: [TimerState { running: false, elapsed: 0 }; TIMERS],
            sequences: Bounded::new(SequenceState { running: false, passes: 0, position: 0 }),
            armed: 0,
            targets: Bounded::new(TargetReport {
                target: Target::NONE,
                exists: false,
                shown: false,
                cycling: false,
                look: 0,
                frame: 0,
                x: 0,
                y: 0,
                layer: 0,
                order: 0,
            }),
            values: Bounded::new((Value::NONE, 0)),
        }
    }
}

/// The answer to a state query (RULE-47).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageState {
    /// The query's identifier.
    pub identifier: u32,
    /// The page's state; `None` when the page has no table in the view.
    pub table: Option<TableState>,
}

/// Writes the payload after the stamp at the front of `out`, and how many
/// bytes it took. What is written is read back.
///
/// # Errors
/// What [`decode`] refuses, and [`Fault::Full`] when `out` is too small.
pub fn encode(state: &PageState, out: &mut [u8]) -> Result<usize, Fault> {
    let mut writer = Writer::new(out);
    writer.u32(state.identifier)?;
    match &state.table {
        None => {
            // RULE-47: generation 0, and nothing but a tick of 0.
            writer.u16(0)?;
            writer.u32(0)?;
        }
        Some(table) => write_table(table, &mut writer)?,
    }
    let length = writer.used();
    // The writer is checked by the reader, so the two cannot disagree.
    if decode(out.get(..length).ok_or(Fault::Full)?)? == *state { Ok(length) } else { Err(Fault::Unreadable) }
}

fn write_count(count: usize, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(u8::try_from(count).map_err(|_| Fault::Range("count"))?)
}

fn write_table(table: &TableState, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u16(table.generation)?;
    writer.u32(table.tick)?;
    for value in table.variables {
        writer.i32(value)?;
    }
    for timer in table.timers {
        writer.u8(if timer.running { RUNNING } else { 0 })?;
        writer.u32(timer.elapsed)?;
    }
    write_count(table.sequences.len(), writer)?;
    for sequence in table.sequences.iter() {
        writer.u8(if sequence.running { RUNNING } else { 0 })?;
        writer.u32(sequence.passes)?;
        writer.u32(sequence.position)?;
    }
    writer.u8(table.armed)?;
    write_count(table.targets.len(), writer)?;
    for report in table.targets.iter() {
        let flags = (if report.exists { EXISTS } else { 0 })
            | (if report.shown { SHOWN } else { 0 })
            | (if report.cycling { CYCLING } else { 0 });
        writer.u8(report.target.kind.code())?;
        writer.u16(report.target.number)?;
        writer.bytes(&[flags, report.look, report.frame])?;
        writer.i16(report.x)?;
        writer.i16(report.y)?;
        writer.u8(report.layer)?;
        writer.i8(report.order)?;
    }
    write_count(table.values.len(), writer)?;
    for (value, reading) in table.values.iter() {
        writer.bytes(&value.to_bytes())?;
        writer.i32(*reading)?;
    }
    Ok(())
}

fn count(reader: &mut Reader<'_>, limit: usize, name: &'static str) -> Result<usize, Fault> {
    let count = usize::from(reader.u8()?);
    if count > limit { Err(Fault::Limit(name)) } else { Ok(count) }
}

fn running(reader: &mut Reader<'_>, what: &'static str) -> Result<bool, Fault> {
    let flags = reader.u8()?;
    if flags & !RUNNING == 0 { Ok(flags & RUNNING != 0) } else { Err(Fault::Reserved(what)) }
}

/// Reads the payload after the stamp.
///
/// # Errors
/// A payload over its bound, a field outside its range, a reserved bit
/// set, a count over its limit, or bytes after the last field.
pub fn decode(bytes: &[u8]) -> Result<PageState, Fault> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(Fault::Limit("MAX_STATE_BYTES"));
    }
    let mut reader = Reader::new(bytes);
    let identifier = reader.u32()?;
    let generation = reader.u16()?;
    let tick = reader.u32()?;
    if generation == 0 {
        if tick != 0 {
            return Err(Fault::Range("tick without a table"));
        }
        return if reader.finished() { Ok(PageState { identifier, table: None }) } else { Err(Fault::Trailing) };
    }
    let mut table = TableState::new(generation, tick);
    for variable in &mut table.variables {
        *variable = reader.i32()?;
    }
    for timer in &mut table.timers {
        *timer = TimerState { running: running(&mut reader, "timer flags")?, elapsed: reader.u32()? };
        if !timer.running && timer.elapsed != 0 {
            return Err(Fault::Range("elapsed while stopped"));
        }
    }
    for _ in 0..count(&mut reader, MAX_SEQUENCES, "MAX_SEQUENCES")? {
        let sequence = SequenceState {
            running: running(&mut reader, "sequence flags")?,
            passes: reader.u32()?,
            position: reader.u32()?,
        };
        if !sequence.running && sequence.position != 0 {
            return Err(Fault::Range("position while stopped"));
        }
        table.sequences.push(sequence).map_err(|_| Fault::Limit("MAX_SEQUENCES"))?;
    }
    table.armed = reader.u8()?;
    if usize::from(table.armed) > MAX_ARMED_RUNS {
        return Err(Fault::Limit("MAX_ARMED_RUNS"));
    }
    for _ in 0..count(&mut reader, MAX_TABLE_TARGETS, "MAX_TABLE_TARGETS")? {
        table.targets.push(read_target(&mut reader)?).map_err(|_| Fault::Limit("MAX_TABLE_TARGETS"))?;
    }
    for _ in 0..count(&mut reader, MAX_TABLE_VALUES, "MAX_TABLE_VALUES")? {
        let value = Value::read(&mut reader)?;
        // Whichever table it was, a value names a sequence below the most
        // a table has.
        value.check(MAX_SEQUENCES)?;
        table.values.push((value, reader.i32()?)).map_err(|_| Fault::Limit("MAX_TABLE_VALUES"))?;
    }
    if reader.finished() { Ok(PageState { identifier, table: Some(table) }) } else { Err(Fault::Trailing) }
}

fn read_target(reader: &mut Reader<'_>) -> Result<TargetReport, Fault> {
    let kind = TargetKind::from_code(reader.u8()?).ok_or(Fault::Range("target kind"))?;
    let target = Target { kind, number: reader.u16()? };
    let (flags, look, frame) = (reader.u8()?, reader.u8()?, reader.u8()?);
    let (x, y, layer, order) = (reader.i16()?, reader.i16()?, reader.u8()?, reader.i8()?);
    if flags & !(EXISTS | SHOWN | CYCLING) != 0 {
        return Err(Fault::Reserved("target flags"));
    }
    let in_range = target.is_thing()
        && (target.kind != TargetKind::Control || target.number <= MAX_CONTROL)
        && look <= MAX_LOOK
        && frame <= MAX_FRAME
        && layer <= MAX_LAYER;
    if !in_range {
        return Err(Fault::Range("target"));
    }
    Ok(TargetReport {
        target,
        exists: flags & EXISTS != 0,
        shown: flags & SHOWN != 0,
        cycling: flags & CYCLING != 0,
        look,
        frame,
        x,
        y,
        layer,
        order,
    })
}

#[cfg(test)]
mod tests;
