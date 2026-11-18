//! Encoding the RULES record's body (RULE-41 to RULE-45). What is written
//! is read back with the decoder before it is given out, so a server can
//! never send what a receiver refuses (RULE-59).

use super::decode::{
    HAS_TABLE, KEY_REPEAT, MOVE_HELD, PLAY_LOOP, RULE_ECHO, RULE_ENABLED, SEND_EVENT, SEND_VALUE, TIMER_RUNS, decode,
};
use super::{Action, Fault, Header, KeySet, Node, Rule, Table, Target, Trigger, Value, action_code, node_code};
use crate::animate::Place;
use crate::limits::FORMAT;
use crate::wire::Writer;

/// Writes the RULES record's body for `page` at `generation`: `table`, or
/// with `None` the body that says the page has no table (RULE-41). Returns
/// the bytes written.
///
/// # Errors
/// Everything [`decode`] refuses, since the body is read back before it is
/// given out; [`Fault::Unreadable`] when it reads back as another table;
/// [`Fault::Full`] when `out` is too small, and [`Fault::Limit`] of
/// `MAX_TABLE_BYTES` when no buffer could hold it.
pub fn encode(page: u16, generation: u16, table: Option<&Table>, out: &mut [u8]) -> Result<usize, Fault> {
    let mut writer = Writer::new(out);
    writer.u16(page)?;
    writer.u16(generation)?;
    writer.u8(FORMAT)?;
    writer.u8(if table.is_some() { HAS_TABLE } else { 0 })?;
    let counts = table.map_or([0; 5], |table| {
        [table.variables.len(), table.timers.len(), table.rects.len(), table.rules.len(), table.subpages.len()]
    });
    for count in counts {
        writer.u8(u8::try_from(count).map_err(|_| Fault::Range("count"))?)?;
    }
    writer.u8(0)?;
    if let Some(table) = table {
        write_table(table, &mut writer)?;
    }
    let length = writer.used();
    let body = out.get(..length).ok_or(Fault::Full)?;
    let mut back = Table::new();
    let header = decode(body, &mut back)?;
    let same = header == Header { page, generation, has_table: table.is_some() } && table.is_none_or(|t| *t == back);
    if same { Ok(length) } else { Err(Fault::Unreadable) }
}

fn write_table(table: &Table, writer: &mut Writer<'_>) -> Result<(), Fault> {
    for variable in table.variables.iter() {
        writer.i32(variable.min)?;
        writer.i32(variable.max)?;
        writer.i32(variable.initial)?;
    }
    for runs in table.timers.iter() {
        writer.u8(if *runs { TIMER_RUNS } else { 0 })?;
    }
    for rect in table.rects.iter() {
        for field in [rect.x, rect.y, rect.width, rect.height] {
            writer.u16(field)?;
        }
    }
    for rule in table.rules.iter() {
        write_rule(rule, writer)?;
    }
    for mask in table.subpages.iter() {
        writer.u16(mask.sub)?;
        writer.u64(mask.off)?;
    }
    Ok(())
}

fn write_target(target: Target, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(target.kind.code())?;
    writer.u16(target.number)
}

/// A count byte: every list of a table holds fewer than 256.
fn write_count(count: usize, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(u8::try_from(count).map_err(|_| Fault::Range("count"))?)
}

fn write_keys(mods_mask: u8, mods_value: u8, keys: &KeySet, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(mods_mask)?;
    writer.u8(mods_value)?;
    write_count(keys.len(), writer)?;
    for key in keys.iter() {
        writer.u8(key.kind.code())?;
        writer.u32(key.code)?;
    }
    Ok(())
}

fn write_trigger(trigger: &Trigger, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(trigger.code())?;
    match trigger {
        Trigger::Key { keys, repeat, mods_mask, mods_value } => {
            writer.u8(if *repeat { KEY_REPEAT } else { 0 })?;
            write_keys(*mods_mask, *mods_value, keys, writer)
        }
        Trigger::KeyUp { keys, mods_mask, mods_value } => {
            writer.u8(0)?;
            write_keys(*mods_mask, *mods_value, keys, writer)
        }
        Trigger::Press { target, button } | Trigger::Release { target, button } => {
            write_target(*target, writer)?;
            writer.u8(*button)
        }
        Trigger::Enter { target } | Trigger::Leave { target } | Trigger::Wheel { target } => {
            write_target(*target, writer)
        }
        Trigger::Move { target, held } => {
            write_target(*target, writer)?;
            writer.u8(if *held { MOVE_HELD } else { 0 })
        }
        Trigger::Every { timer, ticks } | Trigger::After { timer, ticks } => {
            writer.u8(*timer)?;
            writer.u32(*ticks)
        }
        Trigger::Open | Trigger::Close | Trigger::Becomes | Trigger::Text | Trigger::Focus | Trigger::Blur => Ok(()),
    }
}

fn write_node(node: &Node, writer: &mut Writer<'_>) -> Result<(), Fault> {
    match node {
        Node::And(children) => {
            writer.u8(node_code::AND)?;
            writer.u8(*children)
        }
        Node::Or(children) => {
            writer.u8(node_code::OR)?;
            writer.u8(*children)
        }
        Node::Not => writer.u8(node_code::NOT),
        Node::Compare { value, operator, constant } => {
            writer.u8(node_code::COMPARE)?;
            value.write(writer)?;
            writer.u8(operator.code())?;
            writer.i32(*constant)
        }
        Node::InRange { value, low, high } => {
            writer.u8(node_code::IN_RANGE)?;
            value.write(writer)?;
            writer.i32(*low)?;
            writer.i32(*high)
        }
        Node::InSet { value, members } => {
            writer.u8(node_code::IN_SET)?;
            value.write(writer)?;
            write_count(members.len(), writer)?;
            for member in members.iter() {
                writer.i32(*member)?;
            }
            Ok(())
        }
        Node::Overlaps(first, second) => {
            writer.u8(node_code::OVERLAPS)?;
            write_target(*first, writer)?;
            write_target(*second, writer)
        }
        Node::Inside(first, second) => {
            writer.u8(node_code::INSIDE)?;
            write_target(*first, writer)?;
            write_target(*second, writer)
        }
    }
}

fn write_action(action: &Action, writer: &mut Writer<'_>) -> Result<(), Fault> {
    match action {
        Action::Midi { channel, status, data1, data2 } => {
            writer.bytes(&[action_code::MIDI, *channel, *status, *data1, *data2])
        }
        Action::Play { channel, handle, generation, gain, fade_ms, looped } => {
            writer.bytes(&[action_code::PLAY, *channel, *handle])?;
            writer.u16(*generation)?;
            writer.u8(*gain)?;
            writer.u16(*fade_ms)?;
            writer.u8(if *looped { PLAY_LOOP } else { 0 })
        }
        Action::Stop { channel, fade_ms } => {
            writer.bytes(&[action_code::STOP, *channel])?;
            writer.u16(*fade_ms)
        }
        Action::Gain { channel, gain, fade_ms } => {
            writer.bytes(&[action_code::GAIN, *channel, *gain])?;
            writer.u16(*fade_ms)
        }
        Action::Animate(operation) => operation.write(writer, Place::Rule),
        Action::Set { variable, value } => {
            writer.bytes(&[action_code::SET, *variable])?;
            writer.i32(*value)
        }
        Action::Add { variable, amount } => {
            writer.bytes(&[action_code::ADD, *variable])?;
            writer.i32(*amount)
        }
        Action::StartTimer(timer) => writer.bytes(&[action_code::START_TIMER, *timer]),
        Action::StopTimer(timer) => writer.bytes(&[action_code::STOP_TIMER, *timer]),
        Action::Enable(mask) => {
            writer.u8(action_code::ENABLE)?;
            writer.u64(*mask)
        }
        Action::Disable(mask) => {
            writer.u8(action_code::DISABLE)?;
            writer.u64(*mask)
        }
        Action::Send(value) => {
            writer.u8(action_code::SEND)?;
            writer.u8(if value.is_some() { SEND_VALUE } else { SEND_EVENT })?;
            value.unwrap_or(Value::NONE).write(writer)
        }
    }
}

fn write_rule(rule: &Rule, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8((if rule.enabled { RULE_ENABLED } else { 0 }) | (if rule.echo { RULE_ECHO } else { 0 }))?;
    write_trigger(&rule.trigger, writer)?;
    write_count(rule.condition.len(), writer)?;
    for node in rule.condition.iter() {
        write_node(node, writer)?;
    }
    write_count(rule.actions.len(), writer)?;
    for action in rule.actions.iter() {
        write_action(action, writer)?;
    }
    Ok(())
}
