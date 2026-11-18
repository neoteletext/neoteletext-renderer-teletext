//! The decoded rule table (RULE-3, RULE-9 to RULE-28, RULE-105 to
//! RULE-120), in fixed arrays, and the codec of its image (RULE-41 to
//! RULE-45, RULE-70).
//!
//! [`decode`] accepts an image whole or refuses it whole; [`encode`]
//! writes one and reads it back, so nothing is written that a receiver
//! refuses; [`check`] is the decoder applied to an image, which is what
//! the server runs before it cuts a table into records (RULE-39, RULE-59).
//!
//! A table belongs to one page, a page number and a sub-code together
//! (RULE-112). The image holds neither: the RULES record that carries it
//! does, and the caller gives them to [`decode`].

use crate::bounded::Bounded;
use crate::command::{Binding, BodyCheck};
use crate::limits::{
    ANY_BUTTON, MAX_CONDITION_NODES, MAX_GROUP_COMMANDS, MAX_GROUPS, MAX_GROUPS_BYTES, MAX_KEYS_PER_TRIGGER, MAX_RECTS,
    MAX_ROWS, MAX_RULES, MAX_SEQUENCE_STEPS, MAX_SEQUENCES, MAX_SET_MEMBERS, MAX_TRIGGERS_PER_RULE, ROW_PARAMETERS,
};

mod checks;
mod decode;
mod encode;
mod groups;
mod index;
mod value;

pub use decode::{check, decode};
pub use encode::encode;
pub use groups::{CommandRef, Framed, Groups};
pub use index::{
    INTEREST_FOCUS, INTEREST_KEYS, INTEREST_MOVES, INTEREST_POINTER_BUTTONS, INTEREST_TEXT, INTEREST_WHEEL, Index,
    WATCH_ENVELOPE_LEVEL, WATCH_ENVELOPE_STAGE, WATCH_LEVEL, WATCH_VOICES, WatchSlot,
};
pub use value::{
    CLASS_BEND, CLASS_CONTROLLER, CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_FIELD, CLASS_NOTE, CLASS_NOTES,
    CLASS_PROGRAM, CLASS_VELOCITY, CLASS_VOICES, FIRED_CUE, FIRED_KEY, FIRED_P0, FIRED_P3, FIRED_ROW, FIRED_TARGET,
    INPUT_BUTTON, INPUT_CHARACTER, INPUT_KEYS_DOWN, INPUT_NAMED, INPUT_PHYSICAL, INPUT_POINTER, POINTER_OVER,
    POINTER_X, POINTER_Y, SEQUENCE_PASSES, SEQUENCE_POSITION, SEQUENCE_RUNNING, SOUND_GAIN, SOUND_HANDLE, SOUND_LEVEL,
    SOUND_LOCAL, SOUND_PLAYING, SPACE_FIRED, SPACE_INPUT, SPACE_SEQUENCE, SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET,
    SPACE_TIMER, SPACE_VARIABLE, TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME, TARGET_INSIDE, TARGET_LAYER, TARGET_LOOK,
    TARGET_ORDER, TARGET_SHOWN, TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_RUNNING, VALUE_BYTES, Value,
};

/// The bytes of the image's header (RULE-41).
pub const HEADER_BYTES: usize = 12;
/// The bytes of a target in a record (RULE-43): its kind and its number.
pub const TARGET_BYTES: usize = 3;
/// The bytes of a rectangle in the image (RULE-42).
pub const RECT_BYTES: usize = 8;
/// The bytes of a binding row in the image (RULE-42).
pub const ROW_BYTES: usize = ROW_PARAMETERS * 4;
/// The bit of a key's or a target's kind byte that says its code or
/// number is a row parameter's index (RULE-43).
pub const FROM_ROW: u8 = 0x80;
/// The parameter byte of a compare node that adds no parameter (RULE-44).
pub const NO_PARAMETER: u8 = 0;
/// The parameter byte of a compare node that adds `p0` (RULE-44); `p1` to
/// `p3` follow it. They are the binding sources of the same parameters
/// (RULE-106).
pub const FIRST_PARAMETER: u8 = 0x20;

/// Trigger codes (RULE-9), the trigger's byte in the image.
pub mod trigger_code {
    /// `key`.
    pub const KEY: u8 = 1;
    /// `key_up`.
    pub const KEY_UP: u8 = 2;
    /// `press`.
    pub const PRESS: u8 = 3;
    /// `release`.
    pub const RELEASE: u8 = 4;
    /// `enter`.
    pub const ENTER: u8 = 5;
    /// `leave`.
    pub const LEAVE: u8 = 6;
    /// `open`.
    pub const OPEN: u8 = 7;
    /// `close`.
    pub const CLOSE: u8 = 8;
    /// `becomes`.
    pub const BECOMES: u8 = 9;
    /// `every`.
    pub const EVERY: u8 = 10;
    /// `after`.
    pub const AFTER: u8 = 11;
    /// `text`.
    pub const TEXT: u8 = 12;
    /// `move`.
    pub const MOVE: u8 = 13;
    /// `wheel`.
    pub const WHEEL: u8 = 14;
    /// `focus`.
    pub const FOCUS: u8 = 15;
    /// `blur`.
    pub const BLUR: u8 = 16;
    /// `held`.
    pub const HELD: u8 = 17;
    /// `double`.
    pub const DOUBLE: u8 = 18;
    /// `press_held`.
    pub const PRESS_HELD: u8 = 19;
    /// `double_press`.
    pub const DOUBLE_PRESS: u8 = 20;
    /// `cue`.
    pub const CUE: u8 = 21;
}

/// Condition node codes (RULE-18).
pub mod node_code {
    /// `and`.
    pub const AND: u8 = 1;
    /// `or`.
    pub const OR: u8 = 2;
    /// `not`.
    pub const NOT: u8 = 3;
    /// A comparison of a value with a constant.
    pub const COMPARE: u8 = 4;
    /// A value within a range.
    pub const IN_RANGE: u8 = 5;
    /// A value in a set.
    pub const IN_SET: u8 = 6;
    /// Two targets' rectangles share a pixel.
    pub const OVERLAPS: u8 = 7;
    /// The first target's rectangle lies within the second's.
    pub const INSIDE: u8 = 8;
}

/// Why a table, a command or a state answer was refused (RULE-70): it is
/// refused whole, and nothing of it is kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// The bytes end before a field does.
    Short,
    /// Bytes follow the last field.
    Trailing,
    /// A reserved bit or byte that is not 0, and where.
    Reserved(&'static str),
    /// A field outside its range, and which.
    Range(&'static str),
    /// A count over a limit of RULE-57, named by its constant in
    /// [`crate::limits`]; [`crate::limits::value_of`] gives its value.
    Limit(&'static str),
    /// A number that refers to a rectangle, row, group or sequence the
    /// table does not have, and which.
    Reference(&'static str),
    /// A condition whose nodes are not exactly one tree (RULE-44).
    Tree,
    /// A shape RULE-3, RULE-43 or RULE-107 refuses, and which.
    Shape(&'static str),
    /// A command a group may not hold, or whose body its record's reader
    /// refuses, or that names another page (RULE-119), and which.
    Command(&'static str),
    /// A binding RULE-106 does not allow, and why.
    Binding(&'static str),
    /// Groups that run each other in a ring (RULE-120).
    Ring,
    /// The output buffer is too small.
    Full,
    /// What was written does not read back as it was given.
    Unreadable,
}

/// A rectangle a table declares (RULE-13), in the pixels of the page's
/// picture, origin at the top left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    /// The left edge.
    pub x: u16,
    /// The top edge.
    pub y: u16,
    /// The width, at least 1.
    pub width: u16,
    /// The height, at least 1.
    pub height: u16,
}

/// A binding row (RULE-105): the four constants `p0` to `p3`.
pub type Row = [i32; ROW_PARAMETERS];

/// The parameters in force where no row is (RULE-105): all 0.
pub const NO_ROW: Row = [0; ROW_PARAMETERS];

/// The kind of a target (RULE-12), its byte in a record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TargetKind {
    /// The whole page's picture: 0.
    Page,
    /// A teletext control: 1.
    Control,
    /// An animation object: 2.
    Object,
    /// A rectangle the table declares: 3.
    Rect,
}

impl TargetKind {
    /// The kind's byte in a record.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Page => 0,
            Self::Control => 1,
            Self::Object => 2,
            Self::Rect => 3,
        }
    }

    /// The kind of `code`, or `None` for a byte no kind has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Page),
            1 => Some(Self::Control),
            2 => Some(Self::Object),
            3 => Some(Self::Rect),
            _ => None,
        }
    }
}

/// Something on the page (RULE-12): a kind and a number. Number 0 of a
/// control or an object means any, in the pointer triggers only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    /// What kind of thing.
    pub kind: TargetKind,
    /// Which: 0 for the page; a control 1 to 32; an object 1 to 65,535; a
    /// rectangle 0 to 15.
    pub number: u16,
}

impl Target {
    /// The whole page's picture.
    pub const PAGE: Self = Self { kind: TargetKind::Page, number: 0 };
    /// Any control, in a pointer trigger.
    pub const ANY_CONTROL: Self = Self { kind: TargetKind::Control, number: 0 };
    /// Any object, in a pointer trigger.
    pub const ANY_OBJECT: Self = Self { kind: TargetKind::Object, number: 0 };
    /// What a report carries when no target is named (RULE-46): all zero.
    pub const NONE: Self = Self::PAGE;

    /// Control `number`.
    #[must_use]
    pub const fn control(number: u16) -> Self {
        Self { kind: TargetKind::Control, number }
    }

    /// Object `number`.
    #[must_use]
    pub const fn object(number: u16) -> Self {
        Self { kind: TargetKind::Object, number }
    }

    /// Declared rectangle `number`.
    #[must_use]
    pub const fn rect(number: u16) -> Self {
        Self { kind: TargetKind::Rect, number }
    }

    /// Whether this is "any control" or "any object".
    #[must_use]
    pub const fn is_any(self) -> bool {
        matches!(self.kind, TargetKind::Control | TargetKind::Object) && self.number == 0
    }

    /// Whether this is one control or one object: what holds animation
    /// state and what a state query reports (RULE-47).
    #[must_use]
    pub const fn is_thing(self) -> bool {
        matches!(self.kind, TargetKind::Control | TargetKind::Object) && self.number != 0
    }
}

/// A trigger's target (RULE-12, RULE-105): a target, or in a rule that
/// names rows a kind whose number each row gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TriggerTarget {
    /// What kind of thing.
    pub kind: TargetKind,
    /// The number; or, with `from_row`, the index 0 to 3 of the row
    /// parameter that holds it.
    pub number: u16,
    /// Whether the number is a row parameter's (RULE-43: bit 7 of the
    /// kind byte).
    pub from_row: bool,
}

impl TriggerTarget {
    /// The whole page's picture.
    pub const PAGE: Self = Self::of(Target::PAGE);

    /// A target written in the table.
    #[must_use]
    pub const fn of(target: Target) -> Self {
        Self { kind: target.kind, number: target.number, from_row: false }
    }

    /// A target of `kind` whose number is row parameter `parameter`.
    #[must_use]
    pub const fn row(kind: TargetKind, parameter: u8) -> Self {
        Self { kind, number: parameter as u16, from_row: true }
    }

    /// The target for one row (RULE-105), or `None` when the row's
    /// parameter is no number of the kind, which [`decode`] refuses.
    #[must_use]
    pub fn resolve(&self, row: &Row) -> Option<Target> {
        if !self.from_row {
            return Some(Target { kind: self.kind, number: self.number });
        }
        let parameter = row.get(usize::from(self.number)).copied()?;
        let number = u16::try_from(parameter).ok()?;
        Some(Target { kind: self.kind, number })
    }
}

/// How a key is named (RULE-10), its byte in a record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyKind {
    /// The character the active layout gives the key without Shift: 1.
    Character,
    /// A functional key by its code in the input protocol's table: 2.
    Named,
    /// The key that carries the character on a United States layout: 3.
    Physical,
}

impl KeyKind {
    /// The kind's byte in a record.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Character => 1,
            Self::Named => 2,
            Self::Physical => 3,
        }
    }

    /// The kind of `code`, or `None` for a byte no kind has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Character),
            2 => Some(Self::Named),
            3 => Some(Self::Physical),
            _ => None,
        }
    }
}

/// One key of a key set (RULE-10, RULE-105).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// How it is named.
    pub kind: KeyKind,
    /// A Unicode scalar value for a character or a physical key; a
    /// functional code for a named key, which the caller that knows the
    /// input protocol's table checks (RULE-56). With `from_row`, the index
    /// 0 to 3 of the row parameter that holds the code.
    pub code: u32,
    /// Whether the code is a row parameter's (RULE-43: bit 7 of the kind
    /// byte).
    pub from_row: bool,
}

impl Key {
    /// A key written in the table.
    #[must_use]
    pub const fn of(kind: KeyKind, code: u32) -> Self {
        Self { kind, code, from_row: false }
    }

    /// A key of `kind` whose code is row parameter `parameter`.
    #[must_use]
    pub const fn row(kind: KeyKind, parameter: u8) -> Self {
        Self { kind, code: parameter as u32, from_row: true }
    }

    /// The key for one row (RULE-105): its kind and code, or `None` when
    /// the row's parameter is no code, which [`decode`] refuses.
    #[must_use]
    pub fn resolve(&self, row: &Row) -> Option<(KeyKind, u32)> {
        if !self.from_row {
            return Some((self.kind, self.code));
        }
        let parameter = usize::try_from(self.code).ok().and_then(|index| row.get(index)).copied()?;
        Some((self.kind, u32::try_from(parameter).ok()?))
    }
}

/// A key set (RULE-10): empty for any key, else 1 to
/// `MAX_KEYS_PER_TRIGGER` keys.
pub type KeySet = Bounded<Key, MAX_KEYS_PER_TRIGGER>;

/// What a list of keys not yet filled holds.
const NO_KEY: Key = Key::of(KeyKind::Character, 0);

/// A key set that matches any key.
#[must_use]
pub const fn any_key() -> KeySet {
    Bounded::new(NO_KEY)
}

/// The keys a key trigger names and the modifiers it compares (RULE-9).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyMatch {
    /// The keys; empty for any.
    pub keys: KeySet,
    /// The modifier bits compared.
    pub mods_mask: u8,
    /// What they must be: the trigger matches when
    /// `(mods AND mask) = value`.
    pub mods_value: u8,
}

impl KeyMatch {
    /// Any key, whatever the modifiers.
    #[must_use]
    pub const fn any() -> Self {
        Self { keys: any_key(), mods_mask: 0, mods_value: 0 }
    }

    /// The keys given, whatever the modifiers. Keys beyond the limit are
    /// left out, which [`encode`] cannot see: give no more.
    #[must_use]
    pub fn of(keys: &[Key]) -> Self {
        let mut set = any_key();
        for key in keys {
            let _ = set.push(*key);
        }
        Self { keys: set, mods_mask: 0, mods_value: 0 }
    }
}

/// A trigger (RULE-9): an event that can make a rule fire.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// A key of the set is pressed.
    Key {
        /// The keys and modifiers.
        keys: KeyMatch,
        /// Whether each automatic repeat fires too.
        repeat: bool,
    },
    /// A key of the set is released, or lost.
    KeyUp {
        /// The keys and modifiers.
        keys: KeyMatch,
    },
    /// A button or a contact goes down inside the target.
    Press {
        /// Where.
        target: TriggerTarget,
        /// 0 left, 1 middle, 2 right, or 255 for any.
        button: u8,
    },
    /// A button that went down inside the target comes up.
    Release {
        /// Where it went down.
        target: TriggerTarget,
        /// 0, 1, 2, or 255 for any.
        button: u8,
    },
    /// The pointer comes inside the target.
    Enter {
        /// Which.
        target: TriggerTarget,
    },
    /// The pointer goes outside the target, or leaves the page.
    Leave {
        /// Which.
        target: TriggerTarget,
    },
    /// Page tick 0.
    Open,
    /// The page is closed while its connection stands.
    Close,
    /// The rule's condition holds now and did not before (RULE-11).
    Becomes,
    /// Each `ticks` ticks after the timer started.
    Every {
        /// The timer, 0 to 7, or 255 for the page timer.
        timer: u8,
        /// 1 to `MAX_TIMER_TICKS`.
        ticks: u32,
    },
    /// Once, `ticks` ticks after the timer started.
    After {
        /// The timer, 0 to 7, or 255 for the page timer.
        timer: u8,
        /// 1 to `MAX_TIMER_TICKS`.
        ticks: u32,
    },
    /// Text arrives that is not a key.
    Text,
    /// The pointer moves to another pixel inside the target.
    Move {
        /// Where.
        target: TriggerTarget,
        /// Only while a button is down.
        held: bool,
    },
    /// The wheel turns with the pointer inside the target.
    Wheel {
        /// Where.
        target: TriggerTarget,
    },
    /// The page gains the keyboard.
    Focus,
    /// The page loses the keyboard.
    Blur,
    /// A key of the set has been down for exactly `ticks` ticks
    /// (RULE-108).
    Held {
        /// The keys and modifiers, as they were at the press.
        keys: KeyMatch,
        /// 1 to `MAX_HOLD_TICKS`.
        ticks: u16,
    },
    /// A key of the set is pressed at most `ticks` ticks after its
    /// previous press (RULE-108).
    Double {
        /// The keys and modifiers.
        keys: KeyMatch,
        /// 1 to `MAX_DOUBLE_TICKS`.
        ticks: u16,
    },
    /// A button that went down inside the target has been down for
    /// exactly `ticks` ticks (RULE-108).
    PressHeld {
        /// Where it went down.
        target: TriggerTarget,
        /// 0, 1, 2, or 255 for any.
        button: u8,
        /// 1 to `MAX_HOLD_TICKS`.
        ticks: u16,
    },
    /// A button goes down inside the target at most `ticks` ticks after
    /// its previous press there (RULE-108).
    DoublePress {
        /// Where.
        target: TriggerTarget,
        /// 0, 1, 2, or 255 for any.
        button: u8,
        /// 1 to `MAX_DOUBLE_TICKS`.
        ticks: u16,
    },
    /// A cue of the sound playing on an audio channel is heard
    /// (RULE-109).
    Cue {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// The cue, 0 to `MAX_CUE`, or 255 for any.
        number: u8,
    },
}

impl Trigger {
    /// The trigger's byte in the image (RULE-9).
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self {
            Self::Key { .. } => trigger_code::KEY,
            Self::KeyUp { .. } => trigger_code::KEY_UP,
            Self::Press { .. } => trigger_code::PRESS,
            Self::Release { .. } => trigger_code::RELEASE,
            Self::Enter { .. } => trigger_code::ENTER,
            Self::Leave { .. } => trigger_code::LEAVE,
            Self::Open => trigger_code::OPEN,
            Self::Close => trigger_code::CLOSE,
            Self::Becomes => trigger_code::BECOMES,
            Self::Every { .. } => trigger_code::EVERY,
            Self::After { .. } => trigger_code::AFTER,
            Self::Text => trigger_code::TEXT,
            Self::Move { .. } => trigger_code::MOVE,
            Self::Wheel { .. } => trigger_code::WHEEL,
            Self::Focus => trigger_code::FOCUS,
            Self::Blur => trigger_code::BLUR,
            Self::Held { .. } => trigger_code::HELD,
            Self::Double { .. } => trigger_code::DOUBLE,
            Self::PressHeld { .. } => trigger_code::PRESS_HELD,
            Self::DoublePress { .. } => trigger_code::DOUBLE_PRESS,
            Self::Cue { .. } => trigger_code::CUE,
        }
    }

    /// The target a pointer trigger names.
    #[must_use]
    pub const fn target(&self) -> Option<TriggerTarget> {
        match self {
            Self::Press { target, .. }
            | Self::Release { target, .. }
            | Self::Enter { target }
            | Self::Leave { target }
            | Self::Move { target, .. }
            | Self::Wheel { target }
            | Self::PressHeld { target, .. }
            | Self::DoublePress { target, .. } => Some(*target),
            _ => None,
        }
    }

    /// The keys a key trigger names.
    #[must_use]
    pub const fn keys(&self) -> Option<&KeyMatch> {
        match self {
            Self::Key { keys, .. } | Self::KeyUp { keys } | Self::Held { keys, .. } | Self::Double { keys, .. } => {
                Some(keys)
            }
            _ => None,
        }
    }

    /// Whether an input event can make the trigger occur (RULE-96): these
    /// are decided on the event's arrival. `held` and `press_held` belong
    /// to step 4 of the tick and are not.
    #[must_use]
    pub const fn is_input(&self) -> bool {
        !matches!(
            self,
            Self::Open
                | Self::Close
                | Self::Becomes
                | Self::Every { .. }
                | Self::After { .. }
                | Self::Held { .. }
                | Self::PressHeld { .. }
                | Self::Cue { .. }
        )
    }
}

/// A comparison's operator (RULE-18), its byte in the image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Operator {
    /// `==`: 1.
    Equal,
    /// `!=`: 2.
    NotEqual,
    /// `<`: 3.
    Less,
    /// `<=`: 4.
    LessOrEqual,
    /// `>`: 5.
    Greater,
    /// `>=`: 6.
    GreaterOrEqual,
}

impl Operator {
    /// The operator's byte in the image.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Equal => 1,
            Self::NotEqual => 2,
            Self::Less => 3,
            Self::LessOrEqual => 4,
            Self::Greater => 5,
            Self::GreaterOrEqual => 6,
        }
    }

    /// The operator of `code`, or `None` for a byte no operator has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Equal),
            2 => Some(Self::NotEqual),
            3 => Some(Self::Less),
            4 => Some(Self::LessOrEqual),
            5 => Some(Self::Greater),
            6 => Some(Self::GreaterOrEqual),
            _ => None,
        }
    }

    /// Whether `value` stands in this relation to `constant`.
    #[must_use]
    pub const fn holds(self, value: i32, constant: i32) -> bool {
        match self {
            Self::Equal => value == constant,
            Self::NotEqual => value != constant,
            Self::Less => value < constant,
            Self::LessOrEqual => value <= constant,
            Self::Greater => value > constant,
            Self::GreaterOrEqual => value >= constant,
        }
    }
}

/// The constants of an `in_set` (RULE-18): 1 to `MAX_SET_MEMBERS`.
pub type SetMembers = Bounded<i32, MAX_SET_MEMBERS>;

/// One node of a condition (RULE-18). A condition's nodes are in prefix
/// order: a node, then each of its children in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Node {
    /// True when all of its children are.
    And(u8),
    /// True when any of its children is.
    Or(u8),
    /// True when its one child is false.
    Not,
    /// A value compared with a constant.
    Compare {
        /// What is read.
        value: Value,
        /// How it is compared.
        operator: Operator,
        /// With what.
        constant: i32,
        /// The row parameter, 0 to 3, added to the constant with
        /// saturation (RULE-105), in a rule that names rows.
        parameter: Option<u8>,
    },
    /// True when `low ≤ value ≤ high`.
    InRange {
        /// What is read.
        value: Value,
        /// The least.
        low: i32,
        /// The greatest.
        high: i32,
    },
    /// True when the value equals one of the members.
    InSet {
        /// What is read.
        value: Value,
        /// The constants.
        members: SetMembers,
    },
    /// True when the two targets' bounding rectangles share a pixel
    /// (RULE-19).
    Overlaps(Target, Target),
    /// True when the first target's bounding rectangle lies wholly within
    /// the second's (RULE-19).
    Inside(Target, Target),
}

impl Node {
    /// A value compared with a constant, with no row parameter.
    #[must_use]
    pub const fn compare(value: Value, operator: Operator, constant: i32) -> Self {
        Self::Compare { value, operator, constant, parameter: None }
    }

    /// The value a term reads, if it reads one.
    #[must_use]
    pub const fn value(&self) -> Option<Value> {
        match self {
            Self::Compare { value, .. } | Self::InRange { value, .. } | Self::InSet { value, .. } => Some(*value),
            _ => None,
        }
    }

    /// Whether the node is a term: it has no children.
    #[must_use]
    pub const fn is_term(&self) -> bool {
        !matches!(self, Self::And(_) | Self::Or(_) | Self::Not)
    }
}

/// A condition (RULE-18): no node for none, else one tree of at most
/// `MAX_CONDITION_NODES` nodes in prefix order.
pub type Condition = Bounded<Node, MAX_CONDITION_NODES>;

/// No condition: the rule fires whenever a trigger of its occurs.
#[must_use]
pub const fn no_condition() -> Condition {
    Bounded::new(Node::Not)
}

/// A condition of the nodes given, in prefix order. Nodes beyond the
/// limit are left out: give no more.
#[must_use]
pub fn condition(nodes: &[Node]) -> Condition {
    let mut condition = no_condition();
    for node in nodes {
        let _ = condition.push(*node);
    }
    condition
}

/// Checks that a condition's nodes, in prefix order, are exactly one tree
/// within RULE-18's limits. No node is no condition, and passes.
///
/// # Errors
/// [`Fault::Tree`] when the nodes are not one tree that uses every node,
/// and [`Fault::Limit`] for a term or a level over its limit.
pub fn check_condition(nodes: &[Node]) -> Result<(), Fault> {
    checks::check_tree(nodes)
}

/// The rows a rule names (RULE-105): a run of the table's rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowRun {
    /// The first row.
    pub first: u8,
    /// How many, at least 1.
    pub count: u8,
}

/// The triggers of a rule (RULE-107): 1 to `MAX_TRIGGERS_PER_RULE`.
pub type Triggers = Bounded<Trigger, MAX_TRIGGERS_PER_RULE>;

/// A rule (RULE-3): its triggers, at most one condition, optionally a run
/// of binding rows, and the one group it runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    /// The group it runs.
    pub group: u8,
    /// The rows it is matched once for each of (RULE-105).
    pub rows: Option<RowRun>,
    /// The events any of which can make it fire.
    pub triggers: Triggers,
    /// What must hold for it to fire; for a `becomes` trigger, the
    /// condition whose becoming true fires it.
    pub condition: Condition,
}

impl Rule {
    /// A rule of one trigger that runs `group`, with no condition and no
    /// rows: a start to fill in.
    #[must_use]
    pub fn new(trigger: Trigger, group: u8) -> Self {
        let mut triggers = Bounded::new(Trigger::Open);
        let _ = triggers.push(trigger);
        Self { group, rows: None, triggers, condition: no_condition() }
    }

    /// How many rows the rule is matched for: its rows, or one without.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.map_or(1, |rows| usize::from(rows.count))
    }
}

/// What a list of rules not yet filled holds.
const NO_RULE: Rule = Rule { group: 0, rows: None, triggers: Bounded::new(Trigger::Open), condition: no_condition() };

/// One step of a sequence (RULE-103).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Step {
    /// The tick offset within a pass, below the sequence's length.
    pub offset: u32,
    /// The group run then.
    pub group: u8,
}

/// A sequence (RULE-103): steps the receiver plays on its own clock.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sequence {
    /// How many passes, 1 to `MAX_SEQUENCE_REPEAT`; 0 for until stopped.
    pub repeat: u8,
    /// The length of a pass in ticks, 1 to `MAX_SEQUENCE_TICKS`.
    pub length: u32,
    /// The steps, in ascending offset.
    pub steps: Bounded<Step, MAX_SEQUENCE_STEPS>,
}

impl Sequence {
    /// A sequence of `length` ticks and `repeat` passes with no step yet.
    #[must_use]
    pub const fn new(length: u32, repeat: u8) -> Self {
        Self { repeat, length, steps: Bounded::new(Step { offset: 0, group: 0 }) }
    }
}

/// A page's rule table (RULE-2, RULE-3), decoded into fixed arrays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Table {
    /// The page number the table is for, `0x100` to `0x8FF` (RULE-112).
    pub page: u16,
    /// The page's sub-code, `0x0000` to `0x3F7E` (RULE-112).
    pub sub: u16,
    /// The rectangles, numbered from 0 (RULE-13).
    pub rects: Bounded<Rect, MAX_RECTS>,
    /// The binding rows, numbered from 0 (RULE-105).
    pub rows: Bounded<Row, MAX_ROWS>,
    /// The rules, in rule order (RULE-3).
    pub rules: Bounded<Rule, MAX_RULES>,
    /// The groups, numbered from 0 (RULE-118).
    pub groups: Groups,
    /// The sequences, numbered from 0 (RULE-103).
    pub sequences: Bounded<Sequence, MAX_SEQUENCES>,
}

impl Table {
    /// A table for `page` and `sub` with nothing declared and no rule.
    #[must_use]
    pub const fn new(page: u16, sub: u16) -> Self {
        Self {
            page,
            sub,
            rects: Bounded::new(Rect { x: 0, y: 0, width: 1, height: 1 }),
            rows: Bounded::new(NO_ROW),
            rules: Bounded::new(NO_RULE),
            groups: Groups::new(),
            sequences: Bounded::new(Sequence::new(1, 1)),
        }
    }

    /// Forgets everything declared and every rule; the page stays.
    pub fn clear(&mut self) {
        self.rects.clear();
        self.rows.clear();
        self.rules.clear();
        self.groups.clear();
        self.sequences.clear();
    }

    /// Adds a group of `commands` (RULE-118), and gives its number.
    ///
    /// # Errors
    /// [`Fault::Limit`] for a group, a command, a byte or a binding over
    /// its limit, and [`Fault::Range`] for a group with no command or a
    /// command with no body. The commands themselves are checked by
    /// [`Table::check`].
    pub fn push_group(&mut self, commands: &[Framed<'_>]) -> Result<u8, Fault> {
        self.groups.push(commands)
    }

    /// The parameters of row `place` of `rule`'s rows (RULE-105): all 0
    /// for a rule without rows.
    #[must_use]
    pub fn row_of(&self, rule: &Rule, place: usize) -> Row {
        let Some(rows) = rule.rows else { return NO_ROW };
        self.rows.get(usize::from(rows.first).saturating_add(place)).copied().unwrap_or(NO_ROW)
    }

    /// Checks everything [`decode`] checks beyond the image's own framing
    /// (RULE-70): every reference, every command with its record's reader
    /// and RULE-119's list, every binding (RULE-106), the nesting of
    /// groups (RULE-120), the rows where a trigger uses them (RULE-105)
    /// and what the whole table reads and names (RULE-57). A table built
    /// by hand is checked with this before it is evaluated.
    ///
    /// # Errors
    /// The first fault found.
    pub fn check(&self, body_check: BodyCheck) -> Result<(), Fault> {
        checks::check_table(self, body_check)
    }

    /// How many commands one run of group `group` applies, with every
    /// group it runs in place (RULE-120): a constant of a checked table.
    /// 0 for a group the table does not have.
    #[must_use]
    pub fn run_commands(&self, group: u8) -> usize {
        checks::run_commands(&self.groups, group)
    }
}

/// The default table of a channel whose configuration says
/// `"default_rules": "send_all"` (RULE-53), for the page `page` and
/// `sub`: every kind of input is reported, as push protocol 8 sent it. It
/// is one group, which holds one `REPORT` of the firing, and twelve rules
/// that each run it, with a thirteenth, `wheel`, only with `wheel`. Its
/// rules, in order: `keys`, `keys_up`, `text`, `press`, `release`,
/// `move`, `hover_in`, `hover_out`, `drawing_in`, `drawing_out`, `focus`,
/// `blur` and `wheel`.
pub fn default_send_all(wheel: bool, page: u16, sub: u16, table: &mut Table) {
    let over = |target: Target| TriggerTarget::of(target);
    let triggers = [
        Trigger::Key { keys: KeyMatch::any(), repeat: true },
        Trigger::KeyUp { keys: KeyMatch::any() },
        Trigger::Text,
        Trigger::Press { target: TriggerTarget::PAGE, button: ANY_BUTTON },
        Trigger::Release { target: TriggerTarget::PAGE, button: ANY_BUTTON },
        Trigger::Move { target: TriggerTarget::PAGE, held: false },
        Trigger::Enter { target: over(Target::ANY_CONTROL) },
        Trigger::Leave { target: over(Target::ANY_CONTROL) },
        Trigger::Enter { target: over(Target::ANY_OBJECT) },
        Trigger::Leave { target: over(Target::ANY_OBJECT) },
        Trigger::Focus,
        Trigger::Blur,
    ];
    *table = Table::new(page, sub);
    let report = crate::command::Command::Report { value: None };
    let mut body = [0_u8; crate::command::REPORT_BYTES];
    // One command of thirteen bytes: the buffer and the table have room.
    let length = crate::command::encode(&report, page, sub, &mut body).unwrap_or(0);
    let framed = Framed { kind: crate::command::TYPE_REPORT, body: body.get(..length).unwrap_or(&[]), bindings: &[] };
    let group = table.push_group(&[framed]).unwrap_or(0);
    let wheel_rule = wheel.then_some(Trigger::Wheel { target: TriggerTarget::PAGE });
    for trigger in triggers.into_iter().chain(wheel_rule) {
        // Thirteen rules at most: the list has room.
        let _ = table.rules.push(Rule::new(trigger, group));
    }
}

/// The most commands all of a table's groups hold together.
pub(crate) const MAX_TABLE_COMMANDS: usize = MAX_GROUPS * MAX_GROUP_COMMANDS;
// RULE-45: every group's frames fit the bytes the groups may take only
// when the table keeps to both limits; the smaller bound is the bytes'.
const _: () = assert!(MAX_TABLE_COMMANDS * groups::FRAME_FIXED <= MAX_GROUPS_BYTES);

/// A binding list not yet filled.
pub(crate) const NO_BINDING: Binding = Binding { item: 0, field: 0, source: 0 };

#[cfg(test)]
mod tests;
