//! The decoded rule table (RULE-3, RULE-9 to RULE-28), in fixed arrays,
//! and the codec of the RULES record's body (RULE-41 to RULE-45, RULE-70).
//!
//! [`decode`] accepts a body whole or refuses it whole; [`encode`] writes
//! one and reads it back, so nothing is written that a receiver refuses;
//! [`check`] is the decoder applied to encoded bytes, which is what the
//! server runs before it sends a table (RULE-39, RULE-59).

use crate::animate::Operation;
use crate::bounded::Bounded;
use crate::limits::{
    MAX_ACTIONS, MAX_CONDITION_NODES, MAX_KEYS_PER_TRIGGER, MAX_RECTS, MAX_RULES, MAX_SET_MEMBERS, MAX_SUBPAGE_MASKS,
    MAX_TIMERS, MAX_VARIABLES,
};

mod decode;
mod encode;
mod index;
mod value;

pub use decode::{check, decode};
pub use encode::encode;
pub use index::{
    INTEREST_FOCUS, INTEREST_KEYS, INTEREST_MOVES, INTEREST_POINTER_BUTTONS, INTEREST_TEXT, INTEREST_WHEEL, Index,
    WATCH_ENVELOPE_LEVEL, WATCH_ENVELOPE_STAGE, WATCH_LEVEL, WATCH_VOICES, WatchSlot,
};
pub use value::{
    CLASS_BEND, CLASS_CONTROLLER, CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_FIELD, CLASS_NOTE, CLASS_NOTES,
    CLASS_PROGRAM, CLASS_VELOCITY, CLASS_VOICES, INPUT_BUTTON, INPUT_CHARACTER, INPUT_KEYS_DOWN, INPUT_NAMED,
    INPUT_PHYSICAL, INPUT_POINTER, POINTER_OVER, POINTER_X, POINTER_Y, SOUND_GAIN, SOUND_HANDLE, SOUND_LEVEL,
    SOUND_LOCAL, SOUND_PLAYING, SPACE_INPUT, SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET, SPACE_TIMER, SPACE_VARIABLE,
    TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME, TARGET_INSIDE, TARGET_LAYER, TARGET_LOOK, TARGET_ORDER, TARGET_SHOWN,
    TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_RUNNING, VALUE_BYTES, Value,
};

/// The bytes of the RULES body's header (RULE-41).
pub const HEADER_BYTES: usize = 12;
/// The bytes of a target in a record (RULE-43): its kind and its number.
pub const TARGET_BYTES: usize = 3;

/// Trigger codes (RULE-9), the trigger's byte in the record.
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
}

/// Action codes (RULE-20), the action's byte in the record. The codes of
/// the actions on controls and objects are the animation operations' own
/// ([`crate::animate::code`]).
pub mod action_code {
    /// `midi`.
    pub const MIDI: u8 = 1;
    /// `play`.
    pub const PLAY: u8 = 2;
    /// `stop`.
    pub const STOP: u8 = 3;
    /// `gain`.
    pub const GAIN: u8 = 4;
    /// `set`.
    pub const SET: u8 = 8;
    /// `add`.
    pub const ADD: u8 = 9;
    /// `start_timer`.
    pub const START_TIMER: u8 = 10;
    /// `stop_timer`.
    pub const STOP_TIMER: u8 = 11;
    /// `enable`.
    pub const ENABLE: u8 = 12;
    /// `disable`.
    pub const DISABLE: u8 = 13;
    /// `send`.
    pub const SEND: u8 = 14;
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

/// MIDI status nibbles a `midi` action may carry (RULE-21; MIDI 1.0
/// Detailed Specification, "Channel Voice Messages").
pub mod midi_status {
    /// Note off, `0x8n`.
    pub const NOTE_OFF: u8 = 0x80;
    /// Note on, `0x9n`.
    pub const NOTE_ON: u8 = 0x90;
    /// Control change, `0xBn`.
    pub const CONTROL_CHANGE: u8 = 0xB0;
    /// Program change, `0xCn`.
    pub const PROGRAM_CHANGE: u8 = 0xC0;
    /// Pitch bend, `0xEn`.
    pub const PITCH_BEND: u8 = 0xE0;
    /// The bits of a status byte that say which message it is.
    pub const KIND_MASK: u8 = 0xF0;
}

/// Why a table was refused (RULE-70): it is refused whole, and nothing of
/// it is kept.
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
    /// [`crate::limits`].
    Limit(&'static str),
    /// A number that refers to a variable, timer, rectangle or rule the
    /// table does not have, and which.
    Reference(&'static str),
    /// A condition whose nodes are not exactly one tree (RULE-44).
    Tree,
    /// A shape RULE-3 refuses, and which.
    Shape(&'static str),
    /// The output buffer is too small.
    Full,
    /// What was written does not read back as it was given.
    Unreadable,
}

/// A page variable's declaration (RULE-14): `min ≤ initial ≤ max`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Variable {
    /// The least value it holds.
    pub min: i32,
    /// The greatest value it holds.
    pub max: i32,
    /// Its value when the page opens.
    pub initial: i32,
}

/// A rectangle a table declares (RULE-13), in the pixels of the sub-page's
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

/// The rules switched off while one sub-page is shown (RULE-17).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubpageMask {
    /// The sub-code, `0x0000` to `0x3F7E`.
    pub sub: u16,
    /// Bit `n` switches rule `n` off.
    pub off: u64,
}

/// The kind of a target (RULE-12), its byte in the record.
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
/// control or an object means any, in the six pointer triggers only.
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

/// How a key is named (RULE-10), its byte in the record.
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

/// One key of a key set (RULE-10).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    /// How it is named.
    pub kind: KeyKind,
    /// A Unicode scalar value for a character or a physical key; a
    /// functional code for a named key, which the caller that knows the
    /// input protocol's table checks (RULE-56).
    pub code: u32,
}

/// A key set (RULE-10): empty for any key, else 1 to 16 keys.
pub type KeySet = Bounded<Key, MAX_KEYS_PER_TRIGGER>;

/// What a list of keys not yet filled holds.
const NO_KEY: Key = Key { kind: KeyKind::Character, code: 0 };

/// A key set that matches any key.
#[must_use]
pub const fn any_key() -> KeySet {
    Bounded::new(NO_KEY)
}

/// A trigger (RULE-9): the event that can make a rule fire.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    /// A key of the set is pressed.
    Key {
        /// The keys; empty for any.
        keys: KeySet,
        /// Whether each automatic repeat fires too.
        repeat: bool,
        /// The modifier bits compared.
        mods_mask: u8,
        /// What they must be: the trigger matches when
        /// `(mods AND mask) = value`.
        mods_value: u8,
    },
    /// A key of the set is released, or lost.
    KeyUp {
        /// The keys; empty for any.
        keys: KeySet,
        /// The modifier bits compared.
        mods_mask: u8,
        /// What they must be.
        mods_value: u8,
    },
    /// A button or a contact goes down inside the target.
    Press {
        /// Where.
        target: Target,
        /// 0 left, 1 middle, 2 right, or 255 for any.
        button: u8,
    },
    /// A button that went down inside the target comes up.
    Release {
        /// Where it went down.
        target: Target,
        /// 0, 1, 2, or 255 for any.
        button: u8,
    },
    /// The pointer comes inside the target.
    Enter {
        /// Which.
        target: Target,
    },
    /// The pointer goes outside the target, or leaves the page.
    Leave {
        /// Which.
        target: Target,
    },
    /// Page tick 0.
    Open,
    /// The page stops being shown.
    Close,
    /// The rule's condition holds now and did not before (RULE-11).
    Becomes,
    /// Each `ticks` ticks after the timer started.
    Every {
        /// The timer, 0 to 7, or 255 for the page timer.
        timer: u8,
        /// 1 to 216,000.
        ticks: u32,
    },
    /// Once, `ticks` ticks after the timer started.
    After {
        /// The timer, 0 to 7, or 255 for the page timer.
        timer: u8,
        /// 1 to 216,000.
        ticks: u32,
    },
    /// Text arrives that is not a key.
    Text,
    /// The pointer moves to another pixel inside the target.
    Move {
        /// Where.
        target: Target,
        /// Only while a button is down.
        held: bool,
    },
    /// The wheel turns with the pointer inside the target.
    Wheel {
        /// Where.
        target: Target,
    },
    /// The page gains the keyboard.
    Focus,
    /// The page loses the keyboard.
    Blur,
}

impl Trigger {
    /// The trigger's byte in the record (RULE-9).
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
        }
    }

    /// The target a pointer trigger names.
    #[must_use]
    pub const fn target(&self) -> Option<Target> {
        match self {
            Self::Press { target, .. }
            | Self::Release { target, .. }
            | Self::Enter { target }
            | Self::Leave { target }
            | Self::Move { target, .. }
            | Self::Wheel { target } => Some(*target),
            _ => None,
        }
    }
}

/// A comparison's operator (RULE-18), its byte in the record.
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
    /// The operator's byte in a record.
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

/// The constants of an `in_set` (RULE-18): 1 to 8.
pub type SetMembers = Bounded<i32, MAX_SET_MEMBERS>;

/// One node of a condition (RULE-18). A condition's nodes are in prefix
/// order: a node, then each of its children in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Node {
    /// True when all of its children are: 2 to 8 of them.
    And(u8),
    /// True when any of its children is: 2 to 8 of them.
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
        /// The constants, 1 to 8.
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

/// A condition (RULE-18): no node for none, else one tree of at most 16
/// nodes in prefix order.
pub type Condition = Bounded<Node, MAX_CONDITION_NODES>;

/// No condition: the rule fires whenever its trigger occurs.
#[must_use]
pub const fn no_condition() -> Condition {
    Bounded::new(Node::Not)
}

/// An action (RULE-20): one local effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// One MIDI channel voice message to the local sound on an audio
    /// channel (RULE-21).
    Midi {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// The status byte: note off, note on, control change, program
        /// change or pitch bend, with the part's MIDI channel less one.
        status: u8,
        /// The first data byte, 0 to 127.
        data1: u8,
        /// The second, 0 to 127; 0 for a program change.
        data2: u8,
    },
    /// Starts a sound the page already has on an audio channel.
    Play {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// The sound's handle.
        handle: u8,
        /// The sound's generation.
        generation: u16,
        /// 0 to 100.
        gain: u8,
        /// 0 to 10,000 milliseconds.
        fade_ms: u16,
        /// Whether it loops.
        looped: bool,
    },
    /// Ramps an audio channel to silence and stops it.
    Stop {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// 0 to 10,000 milliseconds.
        fade_ms: u16,
    },
    /// Ramps an audio channel to a gain.
    Gain {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// 0 to 100.
        gain: u8,
        /// 0 to 10,000 milliseconds.
        fade_ms: u16,
    },
    /// An operation on a control's or an object's animation state: `show`,
    /// `hide`, `look`, `move`, `order`, `frame`, `cycle` or `stop_cycle`
    /// (ANIM-6), the same bytes an `ANIMATE` record holds.
    Animate(Operation),
    /// Sets a variable to a constant, held to its range (RULE-14).
    Set {
        /// The variable's number.
        variable: u8,
        /// The constant.
        value: i32,
    },
    /// Adds a constant to a variable, saturating at its range (RULE-14).
    Add {
        /// The variable's number.
        variable: u8,
        /// The constant.
        amount: i32,
    },
    /// Starts or restarts a timer (RULE-15).
    StartTimer(u8),
    /// Stops a timer.
    StopTimer(u8),
    /// Switches rules on: bit `n` for rule `n` (RULE-16).
    Enable(u64),
    /// Switches rules off.
    Disable(u64),
    /// Reports to the page's channel: the trigger's own event for `None`,
    /// or one value (RULE-22).
    Send(Option<Value>),
}

/// Checks that a condition's nodes, in prefix order, are exactly one tree
/// within RULE-18's limits: at most 8 terms, operators nested at most 3
/// deep. No node is no condition, and passes.
///
/// # Errors
/// [`Fault::Tree`] when the nodes are not one tree that uses every node,
/// and [`Fault::Limit`] for a ninth term or a fourth level.
pub fn check_condition(nodes: &[Node]) -> Result<(), Fault> {
    decode::check_tree(nodes)
}

/// A rule's actions (RULE-3): 0 to 16, run in written order.
pub type Actions = Bounded<Action, MAX_ACTIONS>;

/// No actions.
#[must_use]
pub const fn no_actions() -> Actions {
    Bounded::new(Action::StopTimer(0))
}

/// A rule (RULE-3): a trigger, at most one condition and at most 16
/// actions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    /// Its state when the page opens.
    pub enabled: bool,
    /// A `send` of the trigger's own event after its last action
    /// (RULE-22).
    pub echo: bool,
    /// The event that can make it fire.
    pub trigger: Trigger,
    /// What must hold for it to fire; for a `becomes` trigger, the
    /// condition whose becoming true fires it.
    pub condition: Condition,
    /// What it does.
    pub actions: Actions,
}

impl Rule {
    /// A rule of `trigger` that is enabled, has no condition, no actions
    /// and no `echo`: a start to fill in.
    #[must_use]
    pub const fn new(trigger: Trigger) -> Self {
        Self { enabled: true, echo: false, trigger, condition: no_condition(), actions: no_actions() }
    }
}

/// What a list of rules not yet filled holds.
const NO_RULE: Rule = Rule::new(Trigger::Open);

/// What the RULES body's header says (RULE-41).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    /// The page the table is for, `0x100` to `0x8FF`.
    pub page: u16,
    /// 1 to 65,535 (RULE-40).
    pub generation: u16,
    /// Whether a table follows; without one the page has no table.
    pub has_table: bool,
}

/// A page's rule table (RULE-2, RULE-3), decoded into fixed arrays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Table {
    /// The variables, numbered from 0 (RULE-14).
    pub variables: Bounded<Variable, MAX_VARIABLES>,
    /// The timers, numbered from 0: whether each runs from opening
    /// (RULE-15).
    pub timers: Bounded<bool, MAX_TIMERS>,
    /// The rectangles, numbered from 0 (RULE-13).
    pub rects: Bounded<Rect, MAX_RECTS>,
    /// The rules, in rule order (RULE-3).
    pub rules: Bounded<Rule, MAX_RULES>,
    /// The rules switched off for a sub-page (RULE-17).
    pub subpages: Bounded<SubpageMask, MAX_SUBPAGE_MASKS>,
}

impl Table {
    /// A table with nothing declared and no rule.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            variables: Bounded::new(Variable { min: 0, max: 0, initial: 0 }),
            timers: Bounded::new(false),
            rects: Bounded::new(Rect { x: 0, y: 0, width: 1, height: 1 }),
            rules: Bounded::new(NO_RULE),
            subpages: Bounded::new(SubpageMask { sub: 0, off: 0 }),
        }
    }

    /// Forgets everything declared and every rule.
    pub fn clear(&mut self) {
        self.variables.clear();
        self.timers.clear();
        self.rects.clear();
        self.rules.clear();
        self.subpages.clear();
    }

    /// The rules switched off while sub-page `sub` is shown (RULE-17): 0
    /// when the table lists none for it.
    #[must_use]
    pub fn off_for(&self, sub: u16) -> u64 {
        self.subpages.iter().find(|mask| mask.sub == sub).map_or(0, |mask| mask.off)
    }

    /// The rules enabled when the page opens: bit `n` for rule `n`.
    #[must_use]
    pub fn enabled_at_opening(&self) -> u64 {
        let mut mask = 0_u64;
        for (rule, bit) in self.rules.iter().zip(0_u32..) {
            if rule.enabled {
                mask |= 1_u64.checked_shl(bit).unwrap_or(0);
            }
        }
        mask
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// The default table of a channel whose configuration says
/// `"default_rules": "send_all"` (RULE-53): every kind of input is sent,
/// as push protocol 8 sent it. The last rule, `wheel`, is present only
/// with `wheel`. Its rules, in order: `keys`, `keys_up`, `text`, `press`,
/// `release`, `move`, `hover_in`, `hover_out`, `drawing_in`,
/// `drawing_out`, `focus`, `blur` and `wheel`.
pub fn default_send_all(wheel: bool, table: &mut Table) {
    let any = crate::limits::ANY_BUTTON;
    let triggers = [
        Trigger::Key { keys: any_key(), repeat: true, mods_mask: 0, mods_value: 0 },
        Trigger::KeyUp { keys: any_key(), mods_mask: 0, mods_value: 0 },
        Trigger::Text,
        Trigger::Press { target: Target::PAGE, button: any },
        Trigger::Release { target: Target::PAGE, button: any },
        Trigger::Move { target: Target::PAGE, held: false },
        Trigger::Enter { target: Target::ANY_CONTROL },
        Trigger::Leave { target: Target::ANY_CONTROL },
        Trigger::Enter { target: Target::ANY_OBJECT },
        Trigger::Leave { target: Target::ANY_OBJECT },
        Trigger::Focus,
        Trigger::Blur,
    ];
    table.clear();
    let wheel_rule = wheel.then_some(Trigger::Wheel { target: Target::PAGE });
    for trigger in triggers.into_iter().chain(wheel_rule) {
        let mut rule = Rule::new(trigger);
        // Thirteen rules of one action each: both lists have room.
        let _ = rule.actions.push(Action::Send(None));
        let _ = table.rules.push(rule);
    }
}

#[cfg(test)]
mod tests;
