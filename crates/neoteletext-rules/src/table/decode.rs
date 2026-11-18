//! Decoding the RULES record's body (RULE-41 to RULE-45): accepted whole
//! or refused whole (RULE-70).

use super::{
    Action, Condition, Fault, HEADER_BYTES, Header, Index, Key, KeyKind, KeySet, Node, Operator, Rect, Rule,
    SetMembers, SubpageMask, Table, Target, TargetKind, Trigger, Value, Variable, action_code, any_key, midi_status,
    no_actions, no_condition, node_code, trigger_code,
};
use crate::animate::{self, Operation, Place};
use crate::limits::{
    ANY_BUTTON, AUDIO_CHANNELS, FIRST_PAGE, FORMAT, LAST_PAGE, LAST_SUBCODE, MAX_ACTIONS, MAX_AND_OR_CHILDREN,
    MAX_BUTTON, MAX_CONDITION_DEPTH, MAX_CONDITION_NODES, MAX_CONDITION_TERMS, MAX_CONTROL, MAX_FADE_MS, MAX_GAIN,
    MAX_KEYS_PER_TRIGGER, MAX_MIDI_DATA, MAX_RECTS, MAX_RULES, MAX_SET_MEMBERS, MAX_SUBPAGE_MASKS, MAX_TABLE_BYTES,
    MAX_TIMER_TICKS, MAX_TIMERS, MAX_VARIABLES, MIN_AND_OR_CHILDREN, PAGE_TIMER, SUBCODE_MASK,
};
use crate::wire::Reader;

/// The header's flag that says a table follows (RULE-41).
pub(super) const HAS_TABLE: u8 = 1;
/// A timer's flag: it runs from opening (RULE-42).
pub(super) const TIMER_RUNS: u8 = 1;
/// A rule's flag: enabled at opening (RULE-43).
pub(super) const RULE_ENABLED: u8 = 1;
/// A rule's flag: `echo`.
pub(super) const RULE_ECHO: u8 = 1 << 1;
/// A `key` trigger's flag: `repeat` (RULE-43).
pub(super) const KEY_REPEAT: u8 = 1;
/// A `move` trigger's flag: `held`.
pub(super) const MOVE_HELD: u8 = 1;
/// `play`'s flag: loop (RULE-45).
pub(super) const PLAY_LOOP: u8 = 1;
/// `send`'s `what`: the trigger's own event (RULE-22).
pub(super) const SEND_EVENT: u8 = 0;
/// `send`'s `what`: one value.
pub(super) const SEND_VALUE: u8 = 1;

/// How many of each thing the table declares, which numbers in it must
/// stay below (RULE-70).
#[derive(Clone, Copy)]
struct Counts {
    variables: usize,
    timers: usize,
    rects: usize,
    rules: usize,
}

/// How a target is used, which decides what it may name (RULE-12).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// In a pointer trigger: any kind, and "any".
    Trigger,
    /// The first operand of `overlaps` or `inside`: a control, an object
    /// or a rectangle (RULE-19).
    First,
    /// The second operand: the page too.
    Second,
}

/// Decodes a RULES record's body into `table`, and gives its header. When
/// the header says no table follows, `table` is left empty.
///
/// # Errors
/// Every fault of RULE-70: a field outside its range, a reserved bit or
/// byte that is not 0, a count over its limit, a number that refers to
/// what the table does not have, a condition that is not one tree, a body
/// cut short, or bytes after its end. `table` is then empty: nothing of a
/// refused table is kept.
pub fn decode(body: &[u8], table: &mut Table) -> Result<Header, Fault> {
    table.clear();
    let result = decode_into(body, table);
    if result.is_err() {
        table.clear();
    }
    result
}

/// Checks a RULES record's body with every rule the decoder applies
/// (RULE-39, RULE-59): what this accepts, every receiver accepts.
///
/// # Errors
/// As [`decode`].
pub fn check(body: &[u8]) -> Result<Header, Fault> {
    let mut table = Table::new();
    decode(body, &mut table)
}

/// A count byte, refused over `limit` by the name of its constant.
fn count(reader: &mut Reader<'_>, limit: usize, name: &'static str) -> Result<usize, Fault> {
    let count = usize::from(reader.u8()?);
    if count > limit { Err(Fault::Limit(name)) } else { Ok(count) }
}

fn decode_into(body: &[u8], table: &mut Table) -> Result<Header, Fault> {
    if body.len() > MAX_TABLE_BYTES {
        return Err(Fault::Limit("MAX_TABLE_BYTES"));
    }
    let mut reader = Reader::new(body);
    let page = reader.u16()?;
    if !(FIRST_PAGE..=LAST_PAGE).contains(&page) {
        return Err(Fault::Range("page"));
    }
    let generation = reader.u16()?;
    if generation == 0 {
        return Err(Fault::Range("generation"));
    }
    if reader.u8()? != FORMAT {
        return Err(Fault::Range("format"));
    }
    let flags = reader.u8()?;
    if flags & !HAS_TABLE != 0 {
        return Err(Fault::Reserved("header flags"));
    }
    let has_table = flags & HAS_TABLE != 0;
    let counts = Counts {
        variables: count(&mut reader, MAX_VARIABLES, "MAX_VARIABLES")?,
        timers: count(&mut reader, MAX_TIMERS, "MAX_TIMERS")?,
        rects: count(&mut reader, MAX_RECTS, "MAX_RECTS")?,
        rules: count(&mut reader, MAX_RULES, "MAX_RULES")?,
    };
    let subpages = count(&mut reader, MAX_SUBPAGE_MASKS, "MAX_SUBPAGE_MASKS")?;
    if reader.u8()? != 0 {
        return Err(Fault::Reserved("header"));
    }
    debug_assert_eq!(reader.used(), HEADER_BYTES, "RULE-41: the header is twelve bytes");
    let header = Header { page, generation, has_table };
    if !has_table {
        let none = counts.variables == 0 && counts.timers == 0 && counts.rects == 0 && counts.rules == 0;
        if !none || subpages != 0 {
            return Err(Fault::Range("counts without a table"));
        }
        return if reader.finished() { Ok(header) } else { Err(Fault::Trailing) };
    }
    for _ in 0..counts.variables {
        let variable = Variable { min: reader.i32()?, max: reader.i32()?, initial: reader.i32()? };
        if variable.min > variable.initial || variable.initial > variable.max {
            return Err(Fault::Range("variable"));
        }
        table.variables.push(variable).map_err(|_| Fault::Limit("MAX_VARIABLES"))?;
    }
    for _ in 0..counts.timers {
        let flags = reader.u8()?;
        if flags & !TIMER_RUNS != 0 {
            return Err(Fault::Reserved("timer flags"));
        }
        table.timers.push(flags & TIMER_RUNS != 0).map_err(|_| Fault::Limit("MAX_TIMERS"))?;
    }
    for _ in 0..counts.rects {
        let rect = Rect { x: reader.u16()?, y: reader.u16()?, width: reader.u16()?, height: reader.u16()? };
        if rect.width == 0 || rect.height == 0 {
            return Err(Fault::Range("rectangle"));
        }
        table.rects.push(rect).map_err(|_| Fault::Limit("MAX_RECTS"))?;
    }
    for _ in 0..counts.rules {
        let rule = read_rule(&mut reader, counts)?;
        table.rules.push(rule).map_err(|_| Fault::Limit("MAX_RULES"))?;
    }
    for _ in 0..subpages {
        let mask = SubpageMask { sub: reader.u16()?, off: reader.u64()? };
        if mask.sub > LAST_SUBCODE || mask.sub & !SUBCODE_MASK != 0 {
            return Err(Fault::Range("sub-code"));
        }
        if table.subpages.iter().any(|held| held.sub == mask.sub) {
            return Err(Fault::Range("sub-code repeated"));
        }
        check_rule_mask(mask.off, counts.rules)?;
        table.subpages.push(mask).map_err(|_| Fault::Limit("MAX_SUBPAGE_MASKS"))?;
    }
    if !reader.finished() {
        return Err(Fault::Trailing);
    }
    // RULE-57: the limits on what the whole table reads and names.
    Index::of(table)?;
    Ok(header)
}

/// RULE-42, RULE-45: a rule mask names no rule at or above the table's
/// count.
fn check_rule_mask(mask: u64, rules: usize) -> Result<(), Fault> {
    let above = u32::try_from(rules).ok().and_then(|rules| mask.checked_shr(rules)).unwrap_or(0);
    if above == 0 { Ok(()) } else { Err(Fault::Reference("rule in a mask")) }
}

/// A target's three bytes, its kind checked.
fn read_target(reader: &mut Reader<'_>) -> Result<Target, Fault> {
    let kind = TargetKind::from_code(reader.u8()?).ok_or(Fault::Range("target kind"))?;
    Ok(Target { kind, number: reader.u16()? })
}

/// A target checked for how it is used (RULE-12, RULE-19, RULE-43).
fn check_target(target: Target, rects: usize, used: Use) -> Result<Target, Fault> {
    let any_allowed = used == Use::Trigger;
    let in_range = match target.kind {
        TargetKind::Page => used != Use::First && target.number == 0,
        TargetKind::Control => target.number <= MAX_CONTROL && (any_allowed || target.number >= 1),
        TargetKind::Object => any_allowed || target.number >= 1,
        TargetKind::Rect => {
            if usize::from(target.number) >= rects {
                return Err(Fault::Reference("rectangle"));
            }
            true
        }
    };
    if in_range { Ok(target) } else { Err(Fault::Range("target")) }
}

fn read_rule(reader: &mut Reader<'_>, counts: Counts) -> Result<Rule, Fault> {
    let flags = reader.u8()?;
    if flags & !(RULE_ENABLED | RULE_ECHO) != 0 {
        return Err(Fault::Reserved("rule flags"));
    }
    let trigger = read_trigger(reader, counts)?;
    let condition = read_condition(reader, counts)?;
    if trigger == Trigger::Becomes && condition.is_empty() {
        return Err(Fault::Shape("becomes without a condition"));
    }
    let action_count = count(reader, MAX_ACTIONS, "MAX_ACTIONS")?;
    let mut actions = no_actions();
    for _ in 0..action_count {
        actions.push(read_action(reader, counts)?).map_err(|_| Fault::Limit("MAX_ACTIONS"))?;
    }
    let echo = flags & RULE_ECHO != 0;
    if actions.is_empty() && !echo {
        return Err(Fault::Shape("no actions and no echo"));
    }
    Ok(Rule { enabled: flags & RULE_ENABLED != 0, echo, trigger, condition, actions })
}

/// A key trigger's body after its flags: the modifiers and the key set.
fn read_keys(reader: &mut Reader<'_>) -> Result<(u8, u8, KeySet), Fault> {
    let (mods_mask, mods_value) = (reader.u8()?, reader.u8()?);
    let key_count = count(reader, MAX_KEYS_PER_TRIGGER, "MAX_KEYS_PER_TRIGGER")?;
    let mut keys = any_key();
    for _ in 0..key_count {
        let kind = KeyKind::from_code(reader.u8()?).ok_or(Fault::Range("key kind"))?;
        let code = reader.u32()?;
        // A named key's code is the caller's to check (RULE-56).
        if kind != KeyKind::Named && char::from_u32(code).is_none() {
            return Err(Fault::Range("key"));
        }
        keys.push(Key { kind, code }).map_err(|_| Fault::Limit("MAX_KEYS_PER_TRIGGER"))?;
    }
    Ok((mods_mask, mods_value, keys))
}

/// A timer trigger's body: the timer and its ticks.
fn read_timer_trigger(reader: &mut Reader<'_>, timers: usize) -> Result<(u8, u32), Fault> {
    let timer = reader.u8()?;
    if usize::from(timer) >= timers && timer != PAGE_TIMER {
        return Err(Fault::Reference("trigger's timer"));
    }
    let ticks = reader.u32()?;
    if ticks == 0 || ticks > MAX_TIMER_TICKS {
        return Err(Fault::Limit("MAX_TIMER_TICKS"));
    }
    Ok((timer, ticks))
}

/// A button of `press` or `release`: 0, 1, 2 or any.
fn read_button(reader: &mut Reader<'_>) -> Result<u8, Fault> {
    let button = reader.u8()?;
    if button <= MAX_BUTTON || button == ANY_BUTTON { Ok(button) } else { Err(Fault::Range("button")) }
}

fn read_trigger(reader: &mut Reader<'_>, counts: Counts) -> Result<Trigger, Fault> {
    let target = |reader: &mut Reader<'_>| check_target(read_target(reader)?, counts.rects, Use::Trigger);
    Ok(match reader.u8()? {
        trigger_code::KEY => {
            let flags = reader.u8()?;
            if flags & !KEY_REPEAT != 0 {
                return Err(Fault::Reserved("key flags"));
            }
            let (mods_mask, mods_value, keys) = read_keys(reader)?;
            Trigger::Key { keys, repeat: flags & KEY_REPEAT != 0, mods_mask, mods_value }
        }
        trigger_code::KEY_UP => {
            if reader.u8()? != 0 {
                return Err(Fault::Reserved("key_up flags"));
            }
            let (mods_mask, mods_value, keys) = read_keys(reader)?;
            Trigger::KeyUp { keys, mods_mask, mods_value }
        }
        trigger_code::PRESS => Trigger::Press { target: target(reader)?, button: read_button(reader)? },
        trigger_code::RELEASE => Trigger::Release { target: target(reader)?, button: read_button(reader)? },
        trigger_code::ENTER => Trigger::Enter { target: target(reader)? },
        trigger_code::LEAVE => Trigger::Leave { target: target(reader)? },
        trigger_code::OPEN => Trigger::Open,
        trigger_code::CLOSE => Trigger::Close,
        trigger_code::BECOMES => Trigger::Becomes,
        trigger_code::EVERY => {
            let (timer, ticks) = read_timer_trigger(reader, counts.timers)?;
            Trigger::Every { timer, ticks }
        }
        trigger_code::AFTER => {
            let (timer, ticks) = read_timer_trigger(reader, counts.timers)?;
            Trigger::After { timer, ticks }
        }
        trigger_code::TEXT => Trigger::Text,
        trigger_code::MOVE => {
            let target = target(reader)?;
            let flags = reader.u8()?;
            if flags & !MOVE_HELD != 0 {
                return Err(Fault::Reserved("move flags"));
            }
            Trigger::Move { target, held: flags & MOVE_HELD != 0 }
        }
        trigger_code::WHEEL => Trigger::Wheel { target: target(reader)? },
        trigger_code::FOCUS => Trigger::Focus,
        trigger_code::BLUR => Trigger::Blur,
        _ => return Err(Fault::Range("trigger kind")),
    })
}

/// A value's 8 bytes, checked against the table (RULE-44).
fn read_value(reader: &mut Reader<'_>, counts: Counts) -> Result<Value, Fault> {
    let value = Value::read(reader)?;
    value.check(counts.variables, counts.timers)?;
    Ok(value)
}

fn read_node(reader: &mut Reader<'_>, counts: Counts) -> Result<Node, Fault> {
    let children = |reader: &mut Reader<'_>| {
        let children = reader.u8()?;
        if usize::from(children) > MAX_AND_OR_CHILDREN {
            Err(Fault::Limit("MAX_AND_OR_CHILDREN"))
        } else if usize::from(children) < MIN_AND_OR_CHILDREN {
            Err(Fault::Range("children of and or or"))
        } else {
            Ok(children)
        }
    };
    let pair = |reader: &mut Reader<'_>| {
        let first = check_target(read_target(reader)?, counts.rects, Use::First)?;
        let second = check_target(read_target(reader)?, counts.rects, Use::Second)?;
        Ok::<_, Fault>((first, second))
    };
    Ok(match reader.u8()? {
        node_code::AND => Node::And(children(reader)?),
        node_code::OR => Node::Or(children(reader)?),
        node_code::NOT => Node::Not,
        node_code::COMPARE => {
            let value = read_value(reader, counts)?;
            let operator = Operator::from_code(reader.u8()?).ok_or(Fault::Range("operator"))?;
            Node::Compare { value, operator, constant: reader.i32()? }
        }
        node_code::IN_RANGE => {
            let value = read_value(reader, counts)?;
            let (low, high) = (reader.i32()?, reader.i32()?);
            if low > high {
                return Err(Fault::Range("range"));
            }
            Node::InRange { value, low, high }
        }
        node_code::IN_SET => {
            let value = read_value(reader, counts)?;
            let member_count = count(reader, MAX_SET_MEMBERS, "MAX_SET_MEMBERS")?;
            if member_count == 0 {
                return Err(Fault::Range("empty set"));
            }
            let mut members: SetMembers = SetMembers::new(0);
            for _ in 0..member_count {
                members.push(reader.i32()?).map_err(|_| Fault::Limit("MAX_SET_MEMBERS"))?;
            }
            Node::InSet { value, members }
        }
        node_code::OVERLAPS => {
            let (first, second) = pair(reader)?;
            Node::Overlaps(first, second)
        }
        node_code::INSIDE => {
            let (first, second) = pair(reader)?;
            Node::Inside(first, second)
        }
        _ => return Err(Fault::Range("condition node")),
    })
}

fn read_condition(reader: &mut Reader<'_>, counts: Counts) -> Result<Condition, Fault> {
    let node_count = count(reader, MAX_CONDITION_NODES, "MAX_CONDITION_NODES")?;
    let mut condition = no_condition();
    for _ in 0..node_count {
        condition.push(read_node(reader, counts)?).map_err(|_| Fault::Limit("MAX_CONDITION_NODES"))?;
    }
    check_tree(&condition)?;
    Ok(condition)
}

/// RULE-18 and RULE-44: the nodes, in prefix order, are exactly one tree
/// that uses every node, of at most 8 terms, its operators nested at most
/// 3 deep. No node is no condition.
pub(super) fn check_tree(nodes: &[Node]) -> Result<(), Fault> {
    if nodes.is_empty() {
        return Ok(());
    }
    // The children each open operator still waits for.
    let mut open = [0_u8; MAX_CONDITION_DEPTH];
    let mut depth = 0_usize;
    let mut terms = 0_usize;
    let mut complete = false;
    for node in nodes {
        if complete {
            // Nodes are left over after the tree ended.
            return Err(Fault::Tree);
        }
        let children = match node {
            Node::And(children) | Node::Or(children) => Some(*children),
            Node::Not => Some(1),
            _ => None,
        };
        if let Some(children) = children {
            let slot = open.get_mut(depth).ok_or(Fault::Limit("MAX_CONDITION_DEPTH"))?;
            *slot = children;
            depth = depth.saturating_add(1);
            continue;
        }
        terms = terms.saturating_add(1);
        if terms > MAX_CONDITION_TERMS {
            return Err(Fault::Limit("MAX_CONDITION_TERMS"));
        }
        // A term ends a child of each operator it is the last child of.
        loop {
            let Some(top) = depth.checked_sub(1) else {
                complete = true;
                break;
            };
            let Some(waiting) = open.get_mut(top) else { return Err(Fault::Tree) };
            *waiting = waiting.saturating_sub(1);
            if *waiting > 0 {
                break;
            }
            depth = top;
        }
    }
    if complete { Ok(()) } else { Err(Fault::Tree) }
}

/// An audio channel, 0 to 3 (RULE-20).
fn read_channel(reader: &mut Reader<'_>) -> Result<u8, Fault> {
    let channel = reader.u8()?;
    if usize::from(channel) < AUDIO_CHANNELS { Ok(channel) } else { Err(Fault::Range("audio channel")) }
}

/// A gain, 0 to 100.
fn read_gain(reader: &mut Reader<'_>) -> Result<u8, Fault> {
    let gain = reader.u8()?;
    if gain <= MAX_GAIN { Ok(gain) } else { Err(Fault::Range("gain")) }
}

/// A fade, 0 to 10,000 milliseconds.
fn read_fade(reader: &mut Reader<'_>) -> Result<u16, Fault> {
    let fade = reader.u16()?;
    if fade <= MAX_FADE_MS { Ok(fade) } else { Err(Fault::Range("fade")) }
}

/// RULE-21: one complete channel voice message of the five kinds allowed,
/// each data byte 0 to 127, a program change's second 0.
pub(super) fn check_midi(status: u8, data1: u8, data2: u8) -> Result<(), Fault> {
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
    let program_change = kind == midi_status::PROGRAM_CHANGE;
    if data1 > MAX_MIDI_DATA || data2 > MAX_MIDI_DATA || (program_change && data2 != 0) {
        return Err(Fault::Range("midi data"));
    }
    Ok(())
}

fn read_action(reader: &mut Reader<'_>, counts: Counts) -> Result<Action, Fault> {
    let variable = |reader: &mut Reader<'_>| {
        let variable = reader.u8()?;
        if usize::from(variable) < counts.variables { Ok(variable) } else { Err(Fault::Reference("action's variable")) }
    };
    let timer = |reader: &mut Reader<'_>| {
        let timer = reader.u8()?;
        if usize::from(timer) < counts.timers { Ok(timer) } else { Err(Fault::Reference("action's timer")) }
    };
    let mask = |reader: &mut Reader<'_>| {
        let mask = reader.u64()?;
        check_rule_mask(mask, counts.rules)?;
        Ok::<_, Fault>(mask)
    };
    let code = reader.u8()?;
    Ok(match code {
        action_code::MIDI => {
            let channel = read_channel(reader)?;
            let (status, data1, data2) = (reader.u8()?, reader.u8()?, reader.u8()?);
            check_midi(status, data1, data2)?;
            Action::Midi { channel, status, data1, data2 }
        }
        action_code::PLAY => {
            let (channel, handle, generation) = (read_channel(reader)?, reader.u8()?, reader.u16()?);
            let (gain, fade_ms, flags) = (read_gain(reader)?, read_fade(reader)?, reader.u8()?);
            if flags & !PLAY_LOOP != 0 {
                return Err(Fault::Reserved("play flags"));
            }
            Action::Play { channel, handle, generation, gain, fade_ms, looped: flags & PLAY_LOOP != 0 }
        }
        action_code::STOP => Action::Stop { channel: read_channel(reader)?, fade_ms: read_fade(reader)? },
        action_code::GAIN => {
            Action::Gain { channel: read_channel(reader)?, gain: read_gain(reader)?, fade_ms: read_fade(reader)? }
        }
        action_code::SET => Action::Set { variable: variable(reader)?, value: reader.i32()? },
        action_code::ADD => Action::Add { variable: variable(reader)?, amount: reader.i32()? },
        action_code::START_TIMER => Action::StartTimer(timer(reader)?),
        action_code::STOP_TIMER => Action::StopTimer(timer(reader)?),
        action_code::ENABLE => Action::Enable(mask(reader)?),
        action_code::DISABLE => Action::Disable(mask(reader)?),
        action_code::SEND => {
            let what = reader.u8()?;
            let value = Value::read(reader)?;
            match what {
                SEND_EVENT if value == Value::NONE => Action::Send(None),
                SEND_EVENT => return Err(Fault::Reserved("send's value")),
                SEND_VALUE => {
                    value.check(counts.variables, counts.timers)?;
                    Action::Send(Some(value))
                }
                _ => return Err(Fault::Range("send's what")),
            }
        }
        // RULE-45: the actions on controls and objects are the ANIMATE
        // record's operations, read by its one function.
        code if animate::is_code(code) => Action::Animate(Operation::read_after_code(code, reader, Place::Rule)?),
        _ => return Err(Fault::Range("action kind")),
    })
}
