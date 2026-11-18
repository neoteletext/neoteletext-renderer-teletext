//! What a whole table reads and names (RULE-57): its distinct values, its
//! distinct controls and objects, its watch list (RULE-33 item 1) and its
//! interest mask (RULE-52 item 1).

use super::value::{
    CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_VOICES, INPUT_BUTTON, INPUT_POINTER, SOUND_LEVEL, SPACE_INPUT,
    SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET, TARGET_INSIDE,
};
use super::{Action, Fault, Node, Table, Target, Trigger, Value};
use crate::bounded::Bounded;
use crate::limits::{MAX_TABLE_TARGETS, MAX_TABLE_VALUES, MAX_WATCHED};

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

/// A watch slot's kind: an audio channel's `level` (RULE-33 item 1).
pub const WATCH_LEVEL: u8 = 1;
/// A watch slot's kind: a part's `envelope_stage`.
pub const WATCH_ENVELOPE_STAGE: u8 = 2;
/// A watch slot's kind: a part's `envelope_level`.
pub const WATCH_ENVELOPE_LEVEL: u8 = 3;
/// A watch slot's kind: `voices`.
pub const WATCH_VOICES: u8 = 4;

/// One metered value a table reads (RULE-33 item 1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WatchSlot {
    /// The audio channel, 0 to 3.
    pub channel: u8,
    /// [`WATCH_LEVEL`], [`WATCH_ENVELOPE_STAGE`], [`WATCH_ENVELOPE_LEVEL`]
    /// or [`WATCH_VOICES`].
    pub kind: u8,
    /// The part, 0 to 15; 0 for a level.
    pub part: u8,
}

impl WatchSlot {
    /// The slot that meters `value`, when the sound helper meters it
    /// (RULE-33): a channel's level, and a synthesizer's envelope stage,
    /// envelope level and voice count.
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

/// What a table reads and names. The order of each list is the order of
/// first use, going through the rules in rule order, and within a rule
/// through its trigger, its condition's nodes and its actions as written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Index {
    /// Every distinct value the table's conditions and `send` actions
    /// read, in the order first read (RULE-47).
    pub values: Bounded<Value, MAX_TABLE_VALUES>,
    /// Every distinct control and object the table names, in the order
    /// first named (RULE-47).
    pub targets: Bounded<Target, MAX_TABLE_TARGETS>,
    /// The watch list: the distinct metered values the table reads, in
    /// the order first read (RULE-33 item 1). A metered report gives one
    /// byte for each, in this order.
    pub watch: Bounded<WatchSlot, MAX_WATCHED>,
    /// The classes of input the table's triggers and input-state values
    /// need (RULE-52 item 1): the `INTEREST_` bits.
    pub interest: u8,
}

impl Index {
    /// The index of `table`.
    ///
    /// # Errors
    /// [`Fault::Limit`] when the table reads more than `MAX_TABLE_VALUES`
    /// distinct values or `MAX_WATCHED` distinct metered values, or names
    /// more than `MAX_TABLE_TARGETS` distinct controls and objects
    /// (RULE-57).
    pub fn of(table: &Table) -> Result<Self, Fault> {
        let mut index = Self {
            values: Bounded::new(Value::NONE),
            targets: Bounded::new(Target::NONE),
            watch: Bounded::new(WatchSlot { channel: 0, kind: WATCH_LEVEL, part: 0 }),
            interest: 0,
        };
        for rule in table.rules.iter() {
            index.interest |= trigger_interest(&rule.trigger);
            if let Some(target) = rule.trigger.target() {
                index.name(target)?;
            }
            for node in rule.condition.iter() {
                match node {
                    Node::Overlaps(first, second) | Node::Inside(first, second) => {
                        index.name(*first)?;
                        index.name(*second)?;
                    }
                    _ => {}
                }
                if let Some(value) = node.value() {
                    index.read(value)?;
                }
            }
            for action in rule.actions.iter() {
                match action {
                    Action::Animate(operation) => index.name(operation.target)?,
                    Action::Send(Some(value)) => index.read(*value)?,
                    _ => {}
                }
            }
        }
        Ok(index)
    }

    /// Where `value` is in [`Index::values`].
    #[must_use]
    pub fn value_slot(&self, value: &Value) -> Option<usize> {
        self.values.iter().position(|held| held == value)
    }

    /// Where `target` is in [`Index::targets`].
    #[must_use]
    pub fn target_slot(&self, target: Target) -> Option<usize> {
        self.targets.iter().position(|held| *held == target)
    }

    /// Where the slot that meters `value` is in [`Index::watch`], and so
    /// in a metered report.
    #[must_use]
    pub fn watch_place(&self, value: &Value) -> Option<usize> {
        let slot = WatchSlot::of(value)?;
        self.watch.iter().position(|held| *held == slot)
    }

    /// Notes one control or object the table names; the page, a
    /// rectangle and "any" hold no state and are not listed.
    fn name(&mut self, target: Target) -> Result<(), Fault> {
        if !target.is_thing() || self.target_slot(target).is_some() {
            return Ok(());
        }
        self.targets.push(target).map(|_| ()).map_err(|_| Fault::Limit("MAX_TABLE_TARGETS"))
    }

    /// Notes one value the table reads, with the target it names, the
    /// slot that meters it and the input it needs.
    fn read(&mut self, value: Value) -> Result<(), Fault> {
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
        Trigger::Open | Trigger::Close | Trigger::Becomes | Trigger::Every { .. } | Trigger::After { .. } => 0,
    }
}

/// The class of input an input-state value needs (RULE-52 item 1): a
/// key's state the keys, a button's the buttons, the pointer's place and
/// what it is inside the moves.
const fn value_interest(value: Value) -> u8 {
    match (value.space, value.a) {
        (SPACE_INPUT, INPUT_BUTTON) => INTEREST_POINTER_BUTTONS,
        (SPACE_INPUT, INPUT_POINTER) => INTEREST_MOVES,
        (SPACE_INPUT, _) => INTEREST_KEYS,
        (SPACE_TARGET, _) if value.c == TARGET_INSIDE => INTEREST_MOVES,
        _ => 0,
    }
}
