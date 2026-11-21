//! Encoding a table's image (RULE-41 to RULE-45). What is written is
//! checked and read back, so nothing is written that a receiver refuses.

use super::decode::{KEY_BY_ROW, KEY_REPEAT, MOVE_HELD, RULE_ROWS, decode};
use super::{
    FROM_ROW, Fault, KeyMatch, NO_PARAMETER, Node, ROW_PARAMETER, Rule, Table, Target, Trigger, TriggerTarget,
    node_code,
};
use crate::command::BodyCheck;
use crate::limits::{FORMAT, MAX_TABLE_BYTES};
use crate::wire::Writer;

/// Writes the image of `table` at the front of `out`, and how many bytes
/// it took. The table's page and sub-code are not in the image: the RULES
/// record carries them (RULE-41).
///
/// # Errors
/// What [`Table::check`] refuses; [`Fault::Limit`] for an image over
/// `MAX_TABLE_BYTES`; [`Fault::Full`] when `out` is too small; and
/// [`Fault::Unreadable`] when the image does not read back as the table.
pub fn encode(table: &Table, body_check: BodyCheck, out: &mut [u8]) -> Result<usize, Fault> {
    table.check(body_check)?;
    let count = |count: usize| u8::try_from(count).map_err(|_| Fault::Range("count"));
    let mut writer = Writer::new(out);
    writer.bytes(&[FORMAT, 0, count(table.rects.len())?, count(table.rows.len())?, count(table.rules.len())?])?;
    writer.bytes(&[count(table.groups.len())?, count(table.sequences.len())?, 0])?;
    writer.u16(u16::try_from(table.groups.as_bytes().len()).map_err(|_| Fault::Limit("MAX_GROUPS_BYTES"))?)?;
    writer.u16(0)?;
    for rect in table.rects.iter() {
        for field in [rect.x, rect.y, rect.width, rect.height] {
            writer.u16(field)?;
        }
    }
    for row in table.rows.iter() {
        writer.i32(*row)?;
    }
    for rule in table.rules.iter() {
        write_rule(rule, &mut writer)?;
    }
    writer.bytes(table.groups.as_bytes())?;
    for sequence in table.sequences.iter() {
        writer.bytes(&[sequence.repeat, count(sequence.steps.len())?])?;
        writer.u32(sequence.length)?;
        for step in sequence.steps.iter() {
            writer.u32(step.offset)?;
            writer.u8(step.group)?;
        }
    }
    let length = writer.used();
    if length > MAX_TABLE_BYTES {
        return Err(Fault::Limit("MAX_TABLE_BYTES"));
    }
    // The writer is checked by the reader, so the two cannot disagree.
    let mut back = Table::new(table.page, table.sub);
    decode(out.get(..length).ok_or(Fault::Full)?, table.page, table.sub, body_check, &mut back)?;
    if back == *table { Ok(length) } else { Err(Fault::Unreadable) }
}

fn write_target(target: Target, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(target.kind.code())?;
    writer.u16(target.number)
}

fn write_trigger_target(target: TriggerTarget, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(target.kind.code() | if target.from_row { FROM_ROW } else { 0 })?;
    writer.u16(target.number)
}

fn write_keys(keys: &KeyMatch, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.bytes(&[keys.mods_mask, keys.mods_value])?;
    writer.u8(u8::try_from(keys.keys.len()).map_err(|_| Fault::Limit("MAX_KEYS_PER_TRIGGER"))?)?;
    for key in keys.keys.iter() {
        writer.u8(key.kind.code())?;
        writer.u32(key.code)?;
    }
    Ok(())
}

fn write_trigger(trigger: &Trigger, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.u8(trigger.code())?;
    match trigger {
        Trigger::Key { keys, repeat } => {
            writer.u8(if *repeat { KEY_REPEAT } else { 0 } | if keys.by_row { KEY_BY_ROW } else { 0 })?;
            write_keys(keys, writer)
        }
        Trigger::KeyUp { keys } => {
            writer.u8(if keys.by_row { KEY_BY_ROW } else { 0 })?;
            write_keys(keys, writer)
        }
        Trigger::Press { target, button } | Trigger::Release { target, button } => {
            write_trigger_target(*target, writer)?;
            writer.u8(*button)
        }
        Trigger::Enter { target }
        | Trigger::Leave { target }
        | Trigger::Wheel { target }
        | Trigger::Painted { target } => write_trigger_target(*target, writer),
        Trigger::Move { target, held } => {
            write_trigger_target(*target, writer)?;
            writer.u8(if *held { MOVE_HELD } else { 0 })
        }
        Trigger::Every { timer, ticks } | Trigger::After { timer, ticks } => {
            writer.u8(*timer)?;
            writer.u32(*ticks)
        }
        Trigger::Cue { channel, number } => writer.bytes(&[*channel, *number]),
        Trigger::Open | Trigger::Close | Trigger::Becomes | Trigger::Text | Trigger::Focus | Trigger::Blur => Ok(()),
    }
}

fn write_node(node: &Node, writer: &mut Writer<'_>) -> Result<(), Fault> {
    match node {
        Node::And(children) => writer.bytes(&[node_code::AND, *children]),
        Node::Or(children) => writer.bytes(&[node_code::OR, *children]),
        Node::Not => writer.u8(node_code::NOT),
        Node::Compare { value, operator, constant, plus_p } => {
            writer.u8(node_code::COMPARE)?;
            value.write(writer)?;
            writer.u8(operator.code())?;
            writer.i32(*constant)?;
            writer.u8(if *plus_p { ROW_PARAMETER } else { NO_PARAMETER })
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
            writer.u8(u8::try_from(members.len()).map_err(|_| Fault::Limit("MAX_SET_MEMBERS"))?)?;
            members.iter().try_for_each(|member| writer.i32(*member))
        }
        Node::Overlaps(first, second) | Node::Inside(first, second) => {
            let code = if matches!(node, Node::Overlaps(..)) { node_code::OVERLAPS } else { node_code::INSIDE };
            writer.u8(code)?;
            write_target(*first, writer)?;
            write_target(*second, writer)
        }
    }
}

fn write_rule(rule: &Rule, writer: &mut Writer<'_>) -> Result<(), Fault> {
    writer.bytes(&[if rule.rows.is_some() { RULE_ROWS } else { 0 }, rule.group])?;
    if let Some(rows) = rule.rows {
        writer.bytes(&[rows.first, rows.count])?;
    }
    writer.u8(u8::try_from(rule.triggers.len()).map_err(|_| Fault::Limit("MAX_TRIGGERS_PER_RULE"))?)?;
    for trigger in rule.triggers.iter() {
        write_trigger(trigger, writer)?;
    }
    writer.u8(u8::try_from(rule.condition.len()).map_err(|_| Fault::Limit("MAX_CONDITION_NODES"))?)?;
    rule.condition.iter().try_for_each(|node| write_node(node, writer))
}
