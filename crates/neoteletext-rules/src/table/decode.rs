//! Decoding a table's image (RULE-41 to RULE-45): accepted whole or
//! refused whole (RULE-70).

use super::{
    Condition, FIRST_PARAMETER, FROM_ROW, Fault, HEADER_BYTES, Key, KeyKind, KeyMatch, NO_PARAMETER, Node, Operator,
    Rect, RowRun, Rule, Sequence, SetMembers, Step, Table, Target, TargetKind, Trigger, TriggerTarget, Triggers, Value,
    any_key, no_condition, node_code, trigger_code,
};
use crate::bounded::Bounded;
use crate::command::BodyCheck;
use crate::limits::{
    FORMAT, MAX_CONDITION_NODES, MAX_GROUPS, MAX_KEYS_PER_TRIGGER, MAX_RECTS, MAX_ROWS, MAX_RULES, MAX_SEQUENCE_STEPS,
    MAX_SEQUENCES, MAX_SET_MEMBERS, MAX_TABLE_BYTES, MAX_TRIGGERS_PER_RULE, ROW_PARAMETERS,
};
use crate::wire::Reader;

/// Bit 0 of a rule's flags: it names rows (RULE-43).
pub(super) const RULE_ROWS: u8 = 1;
/// Bit 0 of a `key` trigger's flags: `repeat` (RULE-43).
pub(super) const KEY_REPEAT: u8 = 1;
/// Bit 0 of a `move` trigger's flags: `held` (RULE-43).
pub(super) const MOVE_HELD: u8 = 1;

/// Decodes the image of a table for the page `page` and `sub` into
/// `table`, checking every command's body with `body_check` (RULE-56).
/// The image holds neither the page nor the sub-code: the RULES record
/// that carried it does (RULE-41).
///
/// # Errors
/// Any fault of RULE-70; `table` is then left empty.
pub fn decode(image: &[u8], page: u16, sub: u16, body_check: BodyCheck, table: &mut Table) -> Result<(), Fault> {
    *table = Table::new(page, sub);
    let result = decode_into(image, table).and_then(|()| table.check(body_check));
    if result.is_err() {
        table.clear();
    }
    result
}

/// Checks an image as [`decode`] does, keeping nothing: what the server
/// runs before it cuts a table into records (RULE-39, RULE-59).
///
/// # Errors
/// Any fault of RULE-70.
pub fn check(image: &[u8], page: u16, sub: u16, body_check: BodyCheck) -> Result<(), Fault> {
    let mut table = Table::new(page, sub);
    decode(image, page, sub, body_check, &mut table)
}

/// Reads a count byte held to `limit`, the constant named `name`.
fn count(reader: &mut Reader<'_>, limit: usize, name: &'static str) -> Result<usize, Fault> {
    let count = usize::from(reader.u8()?);
    if count > limit { Err(Fault::Limit(name)) } else { Ok(count) }
}

fn reserved(reader: &mut Reader<'_>, what: &'static str) -> Result<(), Fault> {
    if reader.u8()? == 0 { Ok(()) } else { Err(Fault::Reserved(what)) }
}

fn decode_into(image: &[u8], table: &mut Table) -> Result<(), Fault> {
    if image.len() > MAX_TABLE_BYTES {
        return Err(Fault::Limit("MAX_TABLE_BYTES"));
    }
    let mut reader = Reader::new(image);
    if reader.u8()? != FORMAT {
        return Err(Fault::Range("format"));
    }
    reserved(&mut reader, "header")?;
    let rects = count(&mut reader, MAX_RECTS, "MAX_RECTS")?;
    let rows = count(&mut reader, MAX_ROWS, "MAX_ROWS")?;
    let rules = count(&mut reader, MAX_RULES, "MAX_RULES")?;
    let groups = count(&mut reader, MAX_GROUPS, "MAX_GROUPS")?;
    let sequences = count(&mut reader, MAX_SEQUENCES, "MAX_SEQUENCES")?;
    reserved(&mut reader, "header")?;
    let groups_bytes = usize::from(reader.u16()?);
    if reader.u16()? != 0 {
        return Err(Fault::Reserved("header"));
    }
    debug_assert_eq!(reader.used(), HEADER_BYTES, "RULE-41: the header is twelve bytes");
    for _ in 0..rects {
        let rect = Rect { x: reader.u16()?, y: reader.u16()?, width: reader.u16()?, height: reader.u16()? };
        table.rects.push(rect).map_err(|_| Fault::Limit("MAX_RECTS"))?;
    }
    for _ in 0..rows {
        let mut row = [0_i32; ROW_PARAMETERS];
        for parameter in &mut row {
            *parameter = reader.i32()?;
        }
        table.rows.push(row).map_err(|_| Fault::Limit("MAX_ROWS"))?;
    }
    for _ in 0..rules {
        let rule = read_rule(&mut reader)?;
        table.rules.push(rule).map_err(|_| Fault::Limit("MAX_RULES"))?;
    }
    table.groups.read(&mut reader, groups, groups_bytes)?;
    for _ in 0..sequences {
        let sequence = read_sequence(&mut reader)?;
        table.sequences.push(sequence).map_err(|_| Fault::Limit("MAX_SEQUENCES"))?;
    }
    if reader.finished() { Ok(()) } else { Err(Fault::Trailing) }
}

fn read_sequence(reader: &mut Reader<'_>) -> Result<Sequence, Fault> {
    let repeat = reader.u8()?;
    let steps = count(reader, MAX_SEQUENCE_STEPS, "MAX_SEQUENCE_STEPS")?;
    let mut sequence = Sequence::new(reader.u32()?, repeat);
    for _ in 0..steps {
        let step = Step { offset: reader.u32()?, group: reader.u8()? };
        sequence.steps.push(step).map_err(|_| Fault::Limit("MAX_SEQUENCE_STEPS"))?;
    }
    Ok(sequence)
}

/// Reads a target written in a condition: no row parameter (RULE-44).
fn read_target(reader: &mut Reader<'_>) -> Result<Target, Fault> {
    let kind = TargetKind::from_code(reader.u8()?).ok_or(Fault::Range("target kind"))?;
    Ok(Target { kind, number: reader.u16()? })
}

/// Reads a trigger's target: bit 7 of its kind byte says its number is a
/// row parameter's index (RULE-43).
fn read_trigger_target(reader: &mut Reader<'_>) -> Result<TriggerTarget, Fault> {
    let byte = reader.u8()?;
    let kind = TargetKind::from_code(byte & !FROM_ROW).ok_or(Fault::Range("target kind"))?;
    Ok(TriggerTarget { kind, number: reader.u16()?, from_row: byte & FROM_ROW != 0 })
}

fn read_rule(reader: &mut Reader<'_>) -> Result<Rule, Fault> {
    let flags = reader.u8()?;
    if flags & !RULE_ROWS != 0 {
        return Err(Fault::Reserved("rule flags"));
    }
    let group = reader.u8()?;
    let rows = if flags & RULE_ROWS == 0 { None } else { Some(RowRun { first: reader.u8()?, count: reader.u8()? }) };
    let trigger_count = count(reader, MAX_TRIGGERS_PER_RULE, "MAX_TRIGGERS_PER_RULE")?;
    if trigger_count == 0 {
        return Err(Fault::Shape("rule without a trigger"));
    }
    let mut triggers: Triggers = Bounded::new(Trigger::Open);
    for _ in 0..trigger_count {
        triggers.push(read_trigger(reader)?).map_err(|_| Fault::Limit("MAX_TRIGGERS_PER_RULE"))?;
    }
    let condition = read_condition(reader)?;
    Ok(Rule { group, rows, triggers, condition })
}

/// Reads a key trigger's modifiers, and after `between` its keys.
fn read_keys(reader: &mut Reader<'_>, ticks: bool) -> Result<(KeyMatch, u16), Fault> {
    let (mods_mask, mods_value) = (reader.u8()?, reader.u8()?);
    let ticks = if ticks { reader.u16()? } else { 0 };
    let key_count = count(reader, MAX_KEYS_PER_TRIGGER, "MAX_KEYS_PER_TRIGGER")?;
    let mut keys = any_key();
    for _ in 0..key_count {
        let byte = reader.u8()?;
        let kind = KeyKind::from_code(byte & !FROM_ROW).ok_or(Fault::Range("key kind"))?;
        let key = Key { kind, code: reader.u32()?, from_row: byte & FROM_ROW != 0 };
        keys.push(key).map_err(|_| Fault::Limit("MAX_KEYS_PER_TRIGGER"))?;
    }
    Ok((KeyMatch { keys, mods_mask, mods_value }, ticks))
}

fn read_trigger(reader: &mut Reader<'_>) -> Result<Trigger, Fault> {
    let flag = |flags: u8, allowed: u8, what| {
        if flags & !allowed == 0 { Ok(flags & allowed != 0) } else { Err(Fault::Reserved(what)) }
    };
    Ok(match reader.u8()? {
        trigger_code::KEY => {
            let repeat = flag(reader.u8()?, KEY_REPEAT, "key flags")?;
            Trigger::Key { keys: read_keys(reader, false)?.0, repeat }
        }
        trigger_code::KEY_UP => {
            reserved(reader, "key_up flags")?;
            Trigger::KeyUp { keys: read_keys(reader, false)?.0 }
        }
        trigger_code::HELD => {
            reserved(reader, "held flags")?;
            let (keys, ticks) = read_keys(reader, true)?;
            Trigger::Held { keys, ticks }
        }
        trigger_code::DOUBLE => {
            reserved(reader, "double flags")?;
            let (keys, ticks) = read_keys(reader, true)?;
            Trigger::Double { keys, ticks }
        }
        trigger_code::PRESS => Trigger::Press { target: read_trigger_target(reader)?, button: reader.u8()? },
        trigger_code::RELEASE => Trigger::Release { target: read_trigger_target(reader)?, button: reader.u8()? },
        trigger_code::PRESS_HELD => {
            Trigger::PressHeld { target: read_trigger_target(reader)?, button: reader.u8()?, ticks: reader.u16()? }
        }
        trigger_code::DOUBLE_PRESS => {
            Trigger::DoublePress { target: read_trigger_target(reader)?, button: reader.u8()?, ticks: reader.u16()? }
        }
        trigger_code::ENTER => Trigger::Enter { target: read_trigger_target(reader)? },
        trigger_code::LEAVE => Trigger::Leave { target: read_trigger_target(reader)? },
        trigger_code::WHEEL => Trigger::Wheel { target: read_trigger_target(reader)? },
        trigger_code::MOVE => {
            let target = read_trigger_target(reader)?;
            Trigger::Move { target, held: flag(reader.u8()?, MOVE_HELD, "move flags")? }
        }
        trigger_code::OPEN => Trigger::Open,
        trigger_code::CLOSE => Trigger::Close,
        trigger_code::BECOMES => Trigger::Becomes,
        trigger_code::TEXT => Trigger::Text,
        trigger_code::FOCUS => Trigger::Focus,
        trigger_code::BLUR => Trigger::Blur,
        trigger_code::EVERY => Trigger::Every { timer: reader.u8()?, ticks: reader.u32()? },
        trigger_code::AFTER => Trigger::After { timer: reader.u8()?, ticks: reader.u32()? },
        trigger_code::CUE => Trigger::Cue { channel: reader.u8()?, number: reader.u8()? },
        _ => return Err(Fault::Range("trigger kind")),
    })
}

fn read_node(reader: &mut Reader<'_>) -> Result<Node, Fault> {
    Ok(match reader.u8()? {
        node_code::AND => Node::And(reader.u8()?),
        node_code::OR => Node::Or(reader.u8()?),
        node_code::NOT => Node::Not,
        node_code::COMPARE => {
            let value = Value::read(reader)?;
            let operator = Operator::from_code(reader.u8()?).ok_or(Fault::Range("operator"))?;
            let constant = reader.i32()?;
            let parameter = match reader.u8()? {
                NO_PARAMETER => None,
                byte => {
                    let index = byte.checked_sub(FIRST_PARAMETER).filter(|index| usize::from(*index) < ROW_PARAMETERS);
                    Some(index.ok_or(Fault::Range("comparison's row parameter"))?)
                }
            };
            Node::Compare { value, operator, constant, parameter }
        }
        node_code::IN_RANGE => Node::InRange { value: Value::read(reader)?, low: reader.i32()?, high: reader.i32()? },
        node_code::IN_SET => {
            let value = Value::read(reader)?;
            let member_count = count(reader, MAX_SET_MEMBERS, "MAX_SET_MEMBERS")?;
            let mut members: SetMembers = SetMembers::new(0);
            for _ in 0..member_count {
                members.push(reader.i32()?).map_err(|_| Fault::Limit("MAX_SET_MEMBERS"))?;
            }
            Node::InSet { value, members }
        }
        node_code::OVERLAPS => Node::Overlaps(read_target(reader)?, read_target(reader)?),
        node_code::INSIDE => Node::Inside(read_target(reader)?, read_target(reader)?),
        _ => return Err(Fault::Range("condition node")),
    })
}

fn read_condition(reader: &mut Reader<'_>) -> Result<Condition, Fault> {
    let node_count = count(reader, MAX_CONDITION_NODES, "MAX_CONDITION_NODES")?;
    let mut condition = no_condition();
    for _ in 0..node_count {
        condition.push(read_node(reader)?).map_err(|_| Fault::Limit("MAX_CONDITION_NODES"))?;
    }
    Ok(condition)
}
