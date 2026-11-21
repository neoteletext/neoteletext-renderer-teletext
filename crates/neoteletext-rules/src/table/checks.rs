//! The static checks of a table (RULE-70): everything beyond the image's
//! own framing that makes a table one a receiver accepts. They run when a
//! table is decoded and never when it runs.

use super::{
    Fault, Groups, Index, Key, KeyKind, KeyMatch, Node, Rule, Table, Target, TargetKind, Trigger, TriggerTarget,
};
use crate::command::{self, Binding, BodyCheck, Command, Context, TYPE_RUN};
use crate::limits::{
    ANY_BUTTON, ANY_CUE, AUDIO_CHANNELS, FIRST_PAGE, LAST_PAGE, LAST_SUBCODE, MAX_AND_OR_CHILDREN, MAX_BUTTON,
    MAX_COMMAND_BINDINGS, MAX_CONDITION_DEPTH, MAX_CONDITION_TERMS, MAX_CONTROL, MAX_CUE, MAX_GROUP_DEPTH, MAX_GROUPS,
    MAX_RULE_ROWS, MAX_RUN_COMMANDS, MAX_SEQUENCE_TICKS, MAX_TIMER_TICKS, MIN_AND_OR_CHILDREN, PAGE_TIMER,
    SUBCODE_MASK, TIMERS,
};
use crate::table::NO_BINDING;

/// Where a target is used, which decides what it may be (RULE-12,
/// RULE-19).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    /// A pointer trigger: any kind, and "any control" or "any object".
    Trigger,
    /// The first operand of `overlaps` or `inside`: one control, object
    /// or rectangle.
    First,
    /// The second operand: the page as well.
    Second,
}

/// Checks that a condition's nodes, in prefix order, are exactly one tree
/// (RULE-44) within RULE-18's limits.
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

/// Checks a target written in the table.
fn check_target(target: Target, rects: usize, used: Use) -> Result<(), Fault> {
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
    if in_range { Ok(()) } else { Err(Fault::Range("target")) }
}

/// The rows a rule names, each its parameter `p`.
fn rows_of<'a>(table: &'a Table, rule: &Rule) -> impl Iterator<Item = super::Row> + 'a {
    let count = rule.rows.map_or(0, |rows| usize::from(rows.count));
    let first = rule.rows.map_or(0, |rows| usize::from(rows.first));
    (0..count).map(move |place| table.rows.get(first.saturating_add(place)).copied().unwrap_or(super::NO_ROW))
}

/// Checks a trigger's target (RULE-12, RULE-43, RULE-105): written in the
/// table, or a constant that with every row's `p` is a valid number.
fn check_trigger_target(target: TriggerTarget, table: &Table, rule: &Rule) -> Result<(), Fault> {
    if !target.from_row {
        return check_target(Target { kind: target.kind, number: target.number }, table.rects.len(), Use::Trigger);
    }
    if rule.rows.is_none() {
        return Err(Fault::Shape("row parameter in a rule without rows"));
    }
    if target.kind == TargetKind::Page {
        return Err(Fault::Range("target's row parameter"));
    }
    for row in rows_of(table, rule) {
        // One control, object or rectangle, as an operand must be.
        let valid =
            target.resolve(&row).is_some_and(|named| check_target(named, table.rects.len(), Use::First).is_ok());
        if !valid {
            return Err(Fault::Range("row's parameter as a target's number"));
        }
    }
    Ok(())
}

/// Whether a `painted` trigger may name `target` (DEC-7031 item 4): the
/// page's whole picture, or one object, a ReGIS numbered drawing. "Any
/// object", a control and a rectangle are refused.
fn is_paintable(target: Target) -> bool {
    match target.kind {
        TargetKind::Page => target.number == 0,
        TargetKind::Object => target.number >= 1,
        TargetKind::Control | TargetKind::Rect => false,
    }
}

/// Checks a `painted` trigger's target (DEC-7031 item 4): written in the
/// table, or an object whose number is a constant that with every row's
/// `p` is 1 to 65,535.
fn check_painted_target(target: TriggerTarget, table: &Table, rule: &Rule) -> Result<(), Fault> {
    if !target.from_row {
        let named = Target { kind: target.kind, number: target.number };
        return if is_paintable(named) { Ok(()) } else { Err(Fault::Range("painted target")) };
    }
    if rule.rows.is_none() {
        return Err(Fault::Shape("row parameter in a rule without rows"));
    }
    if target.kind != TargetKind::Object {
        return Err(Fault::Range("painted target"));
    }
    for row in rows_of(table, rule) {
        if !target.resolve(&row).is_some_and(is_paintable) {
            return Err(Fault::Range("row's parameter as a target's number"));
        }
    }
    Ok(())
}

/// Whether `code` can be the code of a key of `kind`: a Unicode scalar
/// value for a character or a physical key. A named key's code is the
/// caller's to check (RULE-56).
fn is_key_code(kind: KeyKind, code: u32) -> bool {
    kind == KeyKind::Named || char::from_u32(code).is_some()
}

/// Checks a key (RULE-10, RULE-43).
fn check_key(key: Key) -> Result<(), Fault> {
    if is_key_code(key.kind, key.code) { Ok(()) } else { Err(Fault::Range("key")) }
}

/// Checks a key set (RULE-10, RULE-105): one key a row only in a rule
/// that names rows, with exactly as many keys as it names rows.
fn check_keys(keys: &KeyMatch, rule: &Rule) -> Result<(), Fault> {
    if keys.by_row {
        let Some(rows) = rule.rows else { return Err(Fault::Shape("one key a row in a rule without rows")) };
        if keys.keys.len() != usize::from(rows.count) {
            return Err(Fault::Shape("one key a row, and not one key for each row"));
        }
    }
    keys.keys.iter().try_for_each(|key| check_key(*key))
}

fn check_button(button: u8) -> Result<(), Fault> {
    if button <= MAX_BUTTON || button == ANY_BUTTON { Ok(()) } else { Err(Fault::Range("button")) }
}

fn check_ticks(ticks: u32, limit: u32, name: &'static str) -> Result<(), Fault> {
    if (1..=limit).contains(&ticks) { Ok(()) } else { Err(Fault::Limit(name)) }
}

fn check_trigger(trigger: &Trigger, table: &Table, rule: &Rule) -> Result<(), Fault> {
    if let Some(keys) = trigger.keys() {
        check_keys(keys, rule)?;
    }
    if let Some(target) = trigger.target() {
        check_trigger_target(target, table, rule)?;
    }
    match *trigger {
        Trigger::Painted { target } => check_painted_target(target, table, rule),
        Trigger::Press { button, .. } | Trigger::Release { button, .. } => check_button(button),
        Trigger::Every { timer, ticks } | Trigger::After { timer, ticks } => {
            if usize::from(timer) >= TIMERS && timer != PAGE_TIMER {
                return Err(Fault::Range("trigger's timer"));
            }
            check_ticks(ticks, MAX_TIMER_TICKS, "MAX_TIMER_TICKS")
        }
        Trigger::Cue { channel, number } => {
            if usize::from(channel) < AUDIO_CHANNELS && (number <= MAX_CUE || number == ANY_CUE) {
                Ok(())
            } else {
                Err(Fault::Range("cue"))
            }
        }
        _ => Ok(()),
    }
}

fn check_node(node: &Node, table: &Table, rule: &Rule) -> Result<(), Fault> {
    let rects = table.rects.len();
    if let Some(value) = node.value() {
        value.check(table.sequences.len())?;
    }
    match node {
        Node::And(children) | Node::Or(children) => {
            if usize::from(*children) > MAX_AND_OR_CHILDREN {
                Err(Fault::Limit("MAX_AND_OR_CHILDREN"))
            } else if usize::from(*children) < MIN_AND_OR_CHILDREN {
                Err(Fault::Range("children of and or or"))
            } else {
                Ok(())
            }
        }
        Node::Compare { plus_p: true, .. } if rule.rows.is_none() => {
            Err(Fault::Shape("row parameter in a rule without rows"))
        }
        Node::InRange { low, high, .. } if low > high => Err(Fault::Range("range")),
        Node::InSet { members, .. } if members.is_empty() => Err(Fault::Range("empty set")),
        Node::Overlaps(first, second) | Node::Inside(first, second) => {
            check_target(*first, rects, Use::First)?;
            check_target(*second, rects, Use::Second)
        }
        _ => Ok(()),
    }
}

fn check_rule(rule: &Rule, table: &Table) -> Result<(), Fault> {
    if usize::from(rule.group) >= table.groups.len() {
        return Err(Fault::Reference("rule's group"));
    }
    if let Some(rows) = rule.rows {
        let end = usize::from(rows.first).saturating_add(usize::from(rows.count));
        if rows.count == 0 {
            return Err(Fault::Range("rule's row count"));
        }
        if end > table.rows.len() {
            return Err(Fault::Reference("rule's rows"));
        }
    }
    if rule.triggers.is_empty() {
        return Err(Fault::Shape("rule without a trigger"));
    }
    let becomes = rule.triggers.contains(&Trigger::Becomes);
    if becomes && rule.triggers.len() != 1 {
        return Err(Fault::Shape("becomes beside another trigger"));
    }
    if becomes && rule.condition.is_empty() {
        return Err(Fault::Shape("becomes without a condition"));
    }
    rule.triggers.iter().try_for_each(|trigger| check_trigger(trigger, table, rule))?;
    rule.condition.iter().try_for_each(|node| check_node(node, table, rule))?;
    check_tree(&rule.condition)
}

/// What is known of a group's nesting: the commands one run of it
/// applies, and how deep it runs other groups, itself counting 1. A
/// height of 0 is a group not yet walked.
#[derive(Clone, Copy)]
struct Nest {
    total: u16,
    height: u8,
}

/// The group a command runs in place, if it is a `RUN` (RULE-102).
fn runs_now(kind: u8, body: &[u8]) -> Option<u8> {
    if kind != TYPE_RUN {
        return None;
    }
    match command::decode(kind, body) {
        Ok((_, _, Command::Run { group })) => Some(group),
        _ => None,
    }
}

/// Walks group `group` and the groups it runs, refusing a ring, a depth
/// over `MAX_GROUP_DEPTH` and a run over `MAX_RUN_COMMANDS` (RULE-120).
/// `path` holds the groups being walked above this one.
fn nest(
    groups: &Groups,
    group: u8,
    path: &mut crate::Bounded<u8, MAX_GROUP_DEPTH>,
    known: &mut [Nest; MAX_GROUPS],
) -> Result<Nest, Fault> {
    if path.contains(&group) {
        return Err(Fault::Ring);
    }
    let above = path.len();
    if let Some(found) = known.get(usize::from(group)).copied().filter(|found| found.height != 0) {
        return if above.saturating_add(usize::from(found.height)) > MAX_GROUP_DEPTH {
            Err(Fault::Limit("MAX_GROUP_DEPTH"))
        } else {
            Ok(found)
        };
    }
    path.push(group).map_err(|_| Fault::Limit("MAX_GROUP_DEPTH"))?;
    let mut found = Nest { total: 0, height: 1 };
    for command in groups.commands(usize::from(group)) {
        found.total = found.total.saturating_add(1);
        if let Some(inner) = runs_now(command.kind, command.body) {
            let below = nest(groups, inner, path, known)?;
            found.total = found.total.saturating_add(below.total);
            found.height = found.height.max(below.height.saturating_add(1));
        }
    }
    path.pop();
    if usize::from(found.total) > MAX_RUN_COMMANDS {
        return Err(Fault::Limit("MAX_RUN_COMMANDS"));
    }
    if let Some(slot) = known.get_mut(usize::from(group)) {
        *slot = found;
    }
    Ok(found)
}

/// How many commands one run of `group` applies (RULE-120); 0 for a group
/// there is not, or for groups [`check_table`] would refuse.
pub(super) fn run_commands(groups: &Groups, group: u8) -> usize {
    let mut known = [Nest { total: 0, height: 0 }; MAX_GROUPS];
    let mut path = crate::Bounded::new(0);
    nest(groups, group, &mut path, &mut known).map_or(0, |found| usize::from(found.total))
}

/// The `REPORT` commands one run of `group` applies, with every group it
/// runs in place (RULE-5's budget): a constant of a checked table, whose
/// nesting has no ring and is at most `MAX_GROUP_DEPTH` deep.
pub(super) fn run_reports(groups: &Groups, group: u8, depth: usize) -> usize {
    if depth >= MAX_GROUP_DEPTH {
        return 0;
    }
    groups
        .commands(usize::from(group))
        .map(|command| match runs_now(command.kind, command.body) {
            Some(inner) => run_reports(groups, inner, depth.saturating_add(1)),
            None => usize::from(command.kind == command::TYPE_REPORT),
        })
        .fold(0_usize, usize::saturating_add)
}

/// Checks every command of every group, then how the groups nest.
fn check_groups(table: &Table, body_check: BodyCheck) -> Result<(), Fault> {
    let context =
        Context { page: table.page, sub: table.sub, groups: table.groups.len(), sequences: table.sequences.len() };
    for group in 0..table.groups.len() {
        for command in table.groups.commands(group) {
            let mut bindings = [NO_BINDING; MAX_COMMAND_BINDINGS];
            let mut count = 0_usize;
            for (binding, slot) in command.bindings().zip(bindings.iter_mut()) {
                *slot = binding;
                count = count.saturating_add(1);
            }
            let bindings: &[Binding] = bindings.get(..count).unwrap_or(&[]);
            command::check(command.kind, command.body, bindings, &context, body_check)?;
        }
    }
    let mut known = [Nest { total: 0, height: 0 }; MAX_GROUPS];
    for group in (0_u8..).take(table.groups.len()) {
        let mut path = crate::Bounded::new(0);
        nest(&table.groups, group, &mut path, &mut known)?;
    }
    Ok(())
}

/// Everything [`super::decode`] checks beyond the image's framing.
pub(super) fn check_table(table: &Table, body_check: BodyCheck) -> Result<(), Fault> {
    if !(FIRST_PAGE..=LAST_PAGE).contains(&table.page) {
        return Err(Fault::Range("page"));
    }
    if table.sub > LAST_SUBCODE || table.sub & !SUBCODE_MASK != 0 {
        return Err(Fault::Range("sub-code"));
    }
    if table.rects.iter().any(|rect| rect.width == 0 || rect.height == 0) {
        return Err(Fault::Range("rectangle"));
    }
    // The groups first: the nesting reads each `RUN` as checked.
    check_groups(table, body_check)?;
    let mut rule_rows = 0_usize;
    for rule in table.rules.iter() {
        check_rule(rule, table)?;
        rule_rows = rule_rows.saturating_add(rule.row_count());
    }
    if rule_rows > MAX_RULE_ROWS {
        return Err(Fault::Limit("MAX_RULE_ROWS"));
    }
    for sequence in table.sequences.iter() {
        check_ticks(sequence.length, MAX_SEQUENCE_TICKS, "MAX_SEQUENCE_TICKS")?;
        if sequence.steps.is_empty() {
            return Err(Fault::Range("sequence with no step"));
        }
        let mut before = 0_u32;
        for step in sequence.steps.iter() {
            if step.offset >= sequence.length || step.offset < before {
                return Err(Fault::Range("step's offset"));
            }
            if usize::from(step.group) >= table.groups.len() {
                return Err(Fault::Reference("step's group"));
            }
            before = step.offset;
        }
    }
    // RULE-57: the limits on what the whole table reads and names.
    Index::of(table).map(|_| ())
}
