//! What a whole table reads and names (RULE-57): its distinct values, its
//! distinct controls and objects, its watch list (RULE-33 item 1) and its
//! interest mask (RULE-52 item 1).
//!
//! "First named" and "first read" follow RULE-47: the rules in rule
//! order and, within a rule, its triggers' targets, then its condition's
//! nodes in order, then the commands of its group in order; and after the
//! rules the groups no rule names, in group order.

use super::value::{
    CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_VOICES, INPUT_BUTTON, INPUT_POINTER, SOUND_LEVEL, SPACE_FIRED,
    SPACE_INPUT, SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET, TARGET_INSIDE,
};
use super::{CommandRef, Fault, Node, Table, Target, TargetKind, Trigger, Value};
use crate::animate;
use crate::bounded::Bounded;
use crate::command::{self, Command, Operand, TYPE_ANIMATE, TYPE_REPORT, TYPE_VARIABLE, field};
use crate::limits::{MAX_GROUPS, MAX_TABLE_TARGETS, MAX_TABLE_VALUES, MAX_WATCHED};

/// Interest mask bit 0: keys (RULE-52 item 1).
pub const INTEREST_KEYS: u8 = 1;
/// Interest mask bit 1: text.
pub const INTEREST_TEXT: u8 = 1 << 1;
/// Interest mask bit 2: pointer buttons.
pub const INTEREST_POINTER_BUTTONS: u8 = 1 << 2;
/// Interest mask bit 3: pointer moves.
pub const INTEREST_MOVES: u8 = 1 << 3;
/// Interest mask bit 4: the wheel.
pub const INTEREST_WHEEL: u8 = 1 << 4;
/// Interest mask bit 5: focus changes.
pub const INTEREST_FOCUS: u8 = 1 << 5;

/// Watch kind 1: a channel's `level` (RULE-33).
pub const WATCH_LEVEL: u8 = 1;
/// Watch kind 2: a part's `envelope_stage`.
pub const WATCH_ENVELOPE_STAGE: u8 = 2;
/// Watch kind 3: a part's `envelope_level`.
pub const WATCH_ENVELOPE_LEVEL: u8 = 3;
/// Watch kind 4: a part's `voices`.
pub const WATCH_VOICES: u8 = 4;

/// One slot of a watch list (RULE-33 item 1): a metered value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchSlot {
    /// The audio channel, 0 to 3.
    pub channel: u8,
    /// What is metered: 1 `level`, 2 `envelope_stage`, 3
    /// `envelope_level`, 4 `voices`.
    pub kind: u8,
    /// The part, 0 to 15; 0 for `level`.
    pub part: u8,
}

impl WatchSlot {
    /// The slot a value is metered in, or `None` for a value that is not
    /// metered.
    #[must_use]
    pub const fn of(value: &Value) -> Option<Self> {
        if value.space == SPACE_SOUND && value.b == SOUND_LEVEL {
            return Some(Self { channel: value.a, kind: WATCH_LEVEL, part: 0 });
        }
        if value.space != SPACE_SYNTH {
            return None;
        }
        let kind = match value.class() {
            CLASS_ENVELOPE_STAGE => WATCH_ENVELOPE_STAGE,
            CLASS_ENVELOPE_LEVEL => WATCH_ENVELOPE_LEVEL,
            CLASS_VOICES => WATCH_VOICES,
            _ => return None,
        };
        Some(Self { channel: value.a, kind, part: value.part() })
    }
}

/// What a table reads and names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Index {
    /// Each distinct value the table reads, in the order first read
    /// (RULE-47). A value of the firing (RULE-116) is not among them: it
    /// is a firing's, and no state of the page.
    pub values: Bounded<Value, MAX_TABLE_VALUES>,
    /// Each control and object the table names by a constant, in the
    /// order first named (RULE-47).
    pub targets: Bounded<Target, MAX_TABLE_TARGETS>,
    /// The distinct metered values the table's conditions and its groups'
    /// `REPORT` and `VARIABLE` commands name (RULE-33 item 1).
    pub watch: Bounded<WatchSlot, MAX_WATCHED>,
    /// The classes of input the table's triggers and key-state values
    /// need (RULE-52 item 1).
    pub interest: u8,
}

impl Index {
    /// The index of `table`.
    ///
    /// # Errors
    /// [`Fault::Limit`] when the table reads more than `MAX_TABLE_VALUES`
    /// distinct values, names more than `MAX_TABLE_TARGETS` distinct
    /// controls and objects, or meters more than `MAX_WATCHED` values.
    pub fn of(table: &Table) -> Result<Self, Fault> {
        let mut index = Self {
            values: Bounded::new(Value::NONE),
            targets: Bounded::new(Target::NONE),
            watch: Bounded::new(WatchSlot { channel: 0, kind: WATCH_LEVEL, part: 0 }),
            interest: 0,
        };
        let mut named = [false; MAX_GROUPS];
        for rule in table.rules.iter() {
            for trigger in rule.triggers.iter() {
                index.interest |= trigger_interest(trigger);
                if let Some(target) = trigger.target().filter(|target| !target.from_row) {
                    index.name(Target { kind: target.kind, number: target.number })?;
                }
            }
            for node in rule.condition.iter() {
                if let Node::Overlaps(first, second) | Node::Inside(first, second) = node {
                    index.name(*first)?;
                    index.name(*second)?;
                }
                if let Some(value) = node.value() {
                    index.read(value)?;
                }
            }
            index.group(table, usize::from(rule.group))?;
            if let Some(slot) = named.get_mut(usize::from(rule.group)) {
                *slot = true;
            }
        }
        for group in 0..table.groups.len() {
            if !named.get(group).copied().unwrap_or(false) {
                index.group(table, group)?;
            }
        }
        Ok(index)
    }

    /// The place of `value` among the values read.
    #[must_use]
    pub fn value_slot(&self, value: &Value) -> Option<usize> {
        self.values.iter().position(|held| held == value)
    }

    /// The place of `target` among the targets named.
    #[must_use]
    pub fn target_slot(&self, target: Target) -> Option<usize> {
        self.targets.iter().position(|held| *held == target)
    }

    /// The place of a metered value in the watch list, which is its place
    /// in each metered report (RULE-33 item 3).
    #[must_use]
    pub fn watch_place(&self, value: &Value) -> Option<usize> {
        let slot = WatchSlot::of(value)?;
        self.watch.iter().position(|held| *held == slot)
    }

    /// What the commands of one group read and name.
    fn group(&mut self, table: &Table, group: usize) -> Result<(), Fault> {
        for command in table.groups.commands(group) {
            match command.kind {
                TYPE_ANIMATE => self.operations(&command)?,
                TYPE_REPORT | TYPE_VARIABLE => {
                    if let Ok((
                        _,
                        _,
                        Command::Report { value: Some(value) }
                        | Command::Variable { operand: Operand::Value(value), .. },
                    )) = command::decode(command.kind, command.body)
                    {
                        self.read(value)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The controls and objects an `ANIMATE` command names by a constant:
    /// each operation's target whose number no binding gives. Only each
    /// operation's length is read, since a bound field's constant may be
    /// anything (RULE-106).
    fn operations(&mut self, command: &CommandRef<'_>) -> Result<(), Fault> {
        let count = command.body.get(animate::RECORD_COUNT_AT).copied().unwrap_or(0);
        let mut at = animate::RECORD_FIXED;
        for item in 0..count {
            let byte = |offset: usize| command.body.get(at.saturating_add(offset)).copied();
            let (Some(code), Some(kind), Some(high), Some(low)) = (byte(0), byte(1), byte(2), byte(3)) else { break };
            let Some(length) = animate::length_of(code) else { break };
            let bound = command.bindings().any(|binding| binding.item == item && binding.field == field::TARGET);
            if let (false, Some(kind)) = (bound, TargetKind::from_code(kind)) {
                self.name(Target { kind, number: u16::from_be_bytes([high, low]) })?;
            }
            at = at.saturating_add(length);
        }
        Ok(())
    }

    fn name(&mut self, target: Target) -> Result<(), Fault> {
        if !target.is_thing() || self.target_slot(target).is_some() {
            return Ok(());
        }
        self.targets.push(target).map(|_| ()).map_err(|_| Fault::Limit("MAX_TABLE_TARGETS"))
    }

    fn read(&mut self, value: Value) -> Result<(), Fault> {
        if value.space == SPACE_FIRED {
            return Ok(());
        }
        self.interest |= value_interest(value);
        if let Some(target) = value.named_target() {
            self.name(target)?;
        }
        if let Some(slot) = WatchSlot::of(&value)
            && !self.watch.contains(&slot)
        {
            self.watch.push(slot).map_err(|_| Fault::Limit("MAX_WATCHED"))?;
        }
        if self.value_slot(&value).is_some() {
            return Ok(());
        }
        self.values.push(value).map(|_| ()).map_err(|_| Fault::Limit("MAX_TABLE_VALUES"))
    }
}

/// The class of input a trigger needs (RULE-52 item 1).
const fn trigger_interest(trigger: &Trigger) -> u8 {
    match trigger {
        Trigger::Key { .. } | Trigger::KeyUp { .. } => INTEREST_KEYS,
        Trigger::Text => INTEREST_TEXT,
        Trigger::Press { .. } | Trigger::Release { .. } => INTEREST_POINTER_BUTTONS,
        Trigger::Enter { .. } | Trigger::Leave { .. } | Trigger::Move { .. } => INTEREST_MOVES,
        Trigger::Wheel { .. } => INTEREST_WHEEL,
        Trigger::Focus | Trigger::Blur => INTEREST_FOCUS,
        Trigger::Open
        | Trigger::Close
        | Trigger::Becomes
        | Trigger::Every { .. }
        | Trigger::After { .. }
        | Trigger::Cue { .. } => 0,
    }
}

/// The class of input a value needs.
const fn value_interest(value: Value) -> u8 {
    match (value.space, value.a) {
        (SPACE_INPUT, INPUT_BUTTON) => INTEREST_POINTER_BUTTONS,
        (SPACE_INPUT, INPUT_POINTER) => INTEREST_MOVES,
        (SPACE_INPUT, _) => INTEREST_KEYS,
        (SPACE_TARGET, _) if value.c == TARGET_INSIDE => INTEREST_MOVES,
        _ => 0,
    }
}
