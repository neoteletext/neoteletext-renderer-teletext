//! The `PAGE_STATE` payload after its stamp (RULE-47): a page's local
//! state, once, in answer to a state query.
//!
//! The input protocol writes the stamp, the page and sub-code on screen,
//! before these bytes; this crate has no dependency (RULE-56), so the
//! stamp's form is not its own.

use crate::Fault;
use crate::bounded::Bounded;
use crate::limits::{
    MAX_FRAME, MAX_LAYER, MAX_LOOK, MAX_STATE_BYTES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES, MAX_TIMERS, MAX_VARIABLES,
};
use crate::table::{Target, Value};
use crate::wire::{Reader, Writer};

/// The bytes of one target in the answer (RULE-47): kind, number, flags,
/// look, frame, x, y, layer and order.
pub const TARGET_BYTES: usize = 1 + 2 + 1 + 1 + 1 + 2 + 2 + 1 + 1;
/// The most bytes of the payload after its stamp: the stamp is at most
/// four (the input protocol's `STAMP_BYTES`).
pub const MAX_BODY_BYTES: usize = MAX_STATE_BYTES - 4;

/// A target's flag: it exists (RULE-47).
const EXISTS: u8 = 1;
/// A target's flag: it is shown.
const SHOWN: u8 = 1 << 1;
/// A target's flag: it is cycling.
const CYCLING: u8 = 1 << 2;
/// A timer's flag: it runs.
const RUNNING: u8 = 1;

/// A timer in the answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerState {
    /// Whether it runs.
    pub running: bool,
    /// The ticks since it started; 0 while stopped.
    pub elapsed: u32,
}

/// A control or an object in the answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetReport {
    /// Which control or object.
    pub target: Target,
    /// Whether it exists on the sub-page shown.
    pub exists: bool,
    /// Whether it is shown.
    pub shown: bool,
    /// Whether it is cycling.
    pub cycling: bool,
    /// The look in force, 0 to 3.
    pub look: u8,
    /// The frame, 0 to 15.
    pub frame: u8,
    /// Its position, in its renderer's units.
    pub x: i16,
    /// See `x`.
    pub y: i16,
    /// Its layer, 0 to 3.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
}

/// What a page with a table answers (RULE-47).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableState {
    /// The table's generation, 1 to 65,535.
    pub generation: u16,
    /// The page tick of the snapshot reported.
    pub tick: u32,
    /// The rules enabled: bit `n` for rule `n`.
    pub enabled: u64,
    /// Each variable's value, in the table's order.
    pub variables: Bounded<i32, MAX_VARIABLES>,
    /// Each timer, in the table's order.
    pub timers: Bounded<TimerState, MAX_TIMERS>,
    /// Every control and object the table names, in the order first named.
    pub targets: Bounded<TargetReport, MAX_TABLE_TARGETS>,
    /// Every distinct value the table reads, in the order first read, and
    /// its reading.
    pub values: Bounded<(Value, i32), MAX_TABLE_VALUES>,
}

impl TableState {
    /// An answer with nothing in its lists.
    #[must_use]
    pub const fn new(generation: u16, tick: u32, enabled: u64) -> Self {
        Self {
            generation,
            tick,
            enabled,
            variables: Bounded::new(0),
            timers: Bounded::new(TimerState { running: false, elapsed: 0 }),
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

/// A `PAGE_STATE`, after its stamp (RULE-47).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PageState {
    /// The query's identifier.
    pub identifier: u32,
    /// The page's state, or `None` when it has no table: the answer then
    /// carries generation 0 and a tick of 0, and nothing more.
    pub table: Option<TableState>,
}

/// Writes a `PAGE_STATE`'s bytes after its stamp, and returns how many.
///
/// # Errors
/// What [`decode`] would refuse, [`Fault::Full`] when `out` is too small,
/// and [`Fault::Unreadable`] when the bytes do not read back as `state`.
pub fn encode(state: &PageState, out: &mut [u8]) -> Result<usize, Fault> {
    let mut writer = Writer::new(out);
    writer.u32(state.identifier)?;
    match &state.table {
        None => {
            writer.u16(0)?;
            writer.u32(0)?;
        }
        Some(table) => write_table(table, &mut writer)?,
    }
    let length = writer.used();
    // The writer is checked by the reader, so the two cannot disagree.
    if decode(out.get(..length).ok_or(Fault::Full)?)? == *state { Ok(length) } else { Err(Fault::Unreadable) }
}

/// A count byte: every list of the answer holds fewer than 256.
fn write_count(count: usize, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(u8::try_from(count).map_err(|_| Fault::Range("count"))?)
}

fn write_table(table: &TableState, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u16(table.generation)?;
    writer.u32(table.tick)?;
    writer.u64(table.enabled)?;
    write_count(table.variables.len(), writer)?;
    for value in table.variables.iter() {
        writer.i32(*value)?;
    }
    write_count(table.timers.len(), writer)?;
    for timer in table.timers.iter() {
        writer.u8(if timer.running { RUNNING } else { 0 })?;
        writer.u32(timer.elapsed)?;
    }
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

/// A count byte, refused over `limit` by the name of its constant.
fn count(reader: &mut Reader<'_>, limit: usize, name: &'static str) -> Result<usize, Fault> {
    let count = usize::from(reader.u8()?);
    if count > limit { Err(Fault::Limit(name)) } else { Ok(count) }
}

/// Reads a `PAGE_STATE`'s bytes after its stamp.
///
/// # Errors
/// [`Fault::Short`] and [`Fault::Trailing`] for bytes that end early or
/// go on; [`Fault::Limit`] for a list or a payload over its bound;
/// [`Fault::Range`] and [`Fault::Reserved`] for a field outside its range.
/// A value's fields are checked for their space's ranges, but not against
/// a table, which the reader of an answer need not hold.
pub fn decode(bytes: &[u8]) -> Result<PageState, Fault> {
    if bytes.len() > MAX_BODY_BYTES {
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
    // RULE-16: with 64 rules every bit of the mask is a rule's (limits
    // asserts it), so the mask needs no check.
    let mut table = TableState::new(generation, tick, reader.u64()?);
    for _ in 0..count(&mut reader, MAX_VARIABLES, "MAX_VARIABLES")? {
        table.variables.push(reader.i32()?).map_err(|_| Fault::Limit("MAX_VARIABLES"))?;
    }
    for _ in 0..count(&mut reader, MAX_TIMERS, "MAX_TIMERS")? {
        let flags = reader.u8()?;
        if flags & !RUNNING != 0 {
            return Err(Fault::Reserved("timer flags"));
        }
        let timer = TimerState { running: flags & RUNNING != 0, elapsed: reader.u32()? };
        if !timer.running && timer.elapsed != 0 {
            return Err(Fault::Range("elapsed while stopped"));
        }
        table.timers.push(timer).map_err(|_| Fault::Limit("MAX_TIMERS"))?;
    }
    for _ in 0..count(&mut reader, MAX_TABLE_TARGETS, "MAX_TABLE_TARGETS")? {
        table.targets.push(read_target(&mut reader)?).map_err(|_| Fault::Limit("MAX_TABLE_TARGETS"))?;
    }
    for _ in 0..count(&mut reader, MAX_TABLE_VALUES, "MAX_TABLE_VALUES")? {
        let value = Value::read(&mut reader)?;
        // Whichever table it was, a value names a variable below the most
        // a table has and a timer likewise.
        value.check(MAX_VARIABLES, MAX_TIMERS)?;
        table.values.push((value, reader.i32()?)).map_err(|_| Fault::Limit("MAX_TABLE_VALUES"))?;
    }
    if reader.finished() { Ok(PageState { identifier, table: Some(table) }) } else { Err(Fault::Trailing) }
}

fn read_target(reader: &mut Reader<'_>) -> Result<TargetReport, Fault> {
    let kind = crate::table::TargetKind::from_code(reader.u8()?).ok_or(Fault::Range("target kind"))?;
    let target = Target { kind, number: reader.u16()? };
    let (flags, look, frame) = (reader.u8()?, reader.u8()?, reader.u8()?);
    let (x, y, layer, order) = (reader.i16()?, reader.i16()?, reader.u8()?, reader.i8()?);
    if flags & !(EXISTS | SHOWN | CYCLING) != 0 {
        return Err(Fault::Reserved("target flags"));
    }
    let in_range = target.is_thing()
        && (target.kind != crate::table::TargetKind::Control || target.number <= crate::limits::MAX_CONTROL)
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
