//! The input state (RULE-8): which keys and buttons are down, where the
//! pointer is and which targets it is inside. It follows the events, one
//! by one, and no command can change it.

use crate::bounded::Bounded;
use crate::limits::{MAX_BUTTON, MAX_KEYS_DOWN, MAX_POINTER_OBJECTS, MAX_RECTS};
use crate::table::{
    INPUT_BUTTON, INPUT_CHARACTER, INPUT_KEYS_DOWN, INPUT_NAMED, INPUT_PHYSICAL, INPUT_POINTER, KeyKind, KeyMatch,
    POINTER_OVER, POINTER_X, POINTER_Y, Rect, SPACE_INPUT, Target, TargetKind, Value,
};

/// What a key event says happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyAction {
    /// The key went down.
    Press,
    /// An automatic repeat of a key held down.
    Repeat,
    /// The key came up, or was lost (RULE-8).
    Release,
}

/// Which key a key event is of, as the input protocol's KEY event names
/// it (DOC-1935).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyIdentity {
    /// A character: its Unicode scalar value.
    Character(u32),
    /// A functional key: its code in the input protocol's table.
    Named(u32),
}

impl KeyIdentity {
    /// The key's code (RULE-116): its Unicode scalar value or its
    /// functional code.
    #[must_use]
    pub const fn code(self) -> u32 {
        match self {
            Self::Character(code) | Self::Named(code) => code,
        }
    }
}

/// What a pointer event says happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerAction {
    /// A button or a contact went down.
    Press,
    /// A button or a contact came up, or was lost.
    Release,
    /// The pointer moved.
    Move,
    /// The pointer left the page.
    Gone,
}

/// One input event, as the evaluator needs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    /// A key event.
    Key {
        /// What happened.
        action: KeyAction,
        /// Which key.
        key: KeyIdentity,
        /// The character the key carries on a United States layout, when
        /// the event gives one (DOC-1937).
        base: Option<u32>,
        /// The modifier bits (DOC-1935).
        mods: u8,
    },
    /// Text that came from no key: a paste, a composition or its commit.
    Text,
    /// A pointer event, with where the pointer is.
    Pointer {
        /// What happened.
        action: PointerAction,
        /// The button, 0 to 2, of a press or a release; for a move, the
        /// mask of the buttons the event says are down.
        button: u8,
        /// The pixel across.
        x: u16,
        /// The pixel down.
        y: u16,
        /// The control under the pointer's cell, or 0.
        control: u16,
    },
    /// The wheel turned.
    Wheel {
        /// The pixel across.
        x: u16,
        /// The pixel down.
        y: u16,
        /// The control under the pointer's cell, or 0.
        control: u16,
    },
    /// The page gained the keyboard.
    Focus,
    /// The page lost the keyboard.
    Blur,
}

/// An input event with the number its giver knows it by (RULE-52).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// The giver's sequence number, which a report of the event names.
    pub sequence: u32,
    /// The event.
    pub input: Input,
}

/// The targets the pointer is inside (RULE-12).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Inside {
    pub(super) page: bool,
    pub(super) control: u16,
    pub(super) objects: Bounded<u16, MAX_POINTER_OBJECTS>,
    /// Bit `n` for declared rectangle `n`.
    pub(super) rects: u16,
}

// A bit for every rectangle.
const _: () = assert!(MAX_RECTS <= u16::BITS as usize);

impl Inside {
    pub(super) const NOTHING: Self = Self { page: false, control: 0, objects: Bounded::new(0), rects: 0 };

    /// Whether `target` is among them; "any" never is.
    pub(super) fn contains(&self, target: Target) -> bool {
        match target.kind {
            TargetKind::Page => self.page,
            TargetKind::Control => target.number != 0 && self.control == target.number,
            TargetKind::Object => target.number != 0 && self.objects.contains(&target.number),
            TargetKind::Rect => {
                u32::from(target.number) < u16::BITS
                    && self.rects.checked_shr(u32::from(target.number)).unwrap_or(0) & 1 != 0
            }
        }
    }

    /// What is in this set and not in `other`.
    pub(super) fn minus(&self, other: &Self) -> Self {
        let mut objects = Bounded::new(0);
        for object in self.objects.iter().filter(|object| !other.objects.contains(object)) {
            // No more than this set holds.
            let _ = objects.push(*object);
        }
        Self {
            page: self.page && !other.page,
            control: if self.control == other.control { 0 } else { self.control },
            objects,
            rects: self.rects & !other.rects,
        }
    }

    /// Gives `each` every target of this set that `target` names: the
    /// control or each object for "any", [`Target::NONE`] for the page.
    pub(super) fn matching(&self, target: Target, each: &mut dyn FnMut(Target)) {
        match target.kind {
            TargetKind::Control if target.number == 0 => {
                if self.control != 0 {
                    each(Target::control(self.control));
                }
            }
            TargetKind::Object if target.number == 0 => {
                for object in self.objects.iter() {
                    each(Target::object(*object));
                }
            }
            TargetKind::Page => {
                if self.page {
                    each(Target::NONE);
                }
            }
            _ => {
                if self.contains(target) {
                    each(target);
                }
            }
        }
    }
}

/// What one pointer event changed.
#[derive(Clone, Copy)]
pub(super) struct Transition {
    pub(super) entered: Inside,
    pub(super) left: Inside,
    /// Whether the pointer moved to another pixel.
    pub(super) moved: bool,
    /// For a release: what the button's press was inside.
    pub(super) released: Inside,
}

impl Transition {
    pub(super) const NONE: Self =
        Self { entered: Inside::NOTHING, left: Inside::NOTHING, moved: false, released: Inside::NOTHING };
}

/// A key that is down (RULE-8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct KeyDown {
    pub(super) key: KeyIdentity,
    pub(super) base: Option<u32>,
}

/// A pointer button (RULE-8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct ButtonDown {
    pub(super) down: bool,
    /// What its press was inside, which its release fires on (RULE-9).
    pub(super) inside: Inside,
}

/// What a value of the input state reads (RULE-24, RULE-26): the keys
/// down, the buttons down, where the pointer is and what it is inside.
#[derive(Clone, Copy)]
pub(super) struct Seen {
    pub(super) keys: Bounded<KeyDown, MAX_KEYS_DOWN>,
    /// Bit `n` while button `n` is down.
    pub(super) buttons: u8,
    over: bool,
    x: u16,
    y: u16,
    pub(super) inside: Inside,
}

/// The input state.
#[derive(Clone, Copy)]
pub(super) struct InputState {
    pub(super) seen: Seen,
    pub(super) pressed: [ButtonDown; MAX_BUTTON as usize + 1],
}

/// Whether a key of `kind` and `code` names the key of an event
/// (RULE-10).
pub(super) fn key_names(kind: KeyKind, code: u32, identity: KeyIdentity, base: Option<u32>) -> bool {
    match kind {
        KeyKind::Character => identity == KeyIdentity::Character(code),
        KeyKind::Named => identity == KeyIdentity::Named(code),
        // The KEY event's `base` when it has one, else its `key` when that
        // is a character (DOC-1937).
        KeyKind::Physical => base.map_or(identity == KeyIdentity::Character(code), |base| base == code),
    }
}

/// Whether a key trigger's set and modifiers match a key (RULE-9,
/// RULE-10): any member of the set, or with `by_row` the member at the
/// place of the row being matched among the rule's rows (RULE-105).
pub(super) fn keys_match(keys: &KeyMatch, place: usize, identity: KeyIdentity, base: Option<u32>, mods: u8) -> bool {
    let names_it = |key: &crate::table::Key| key_names(key.kind, key.code, identity, base);
    let named = if keys.by_row {
        keys.keys.get(place).is_some_and(names_it)
    } else {
        keys.keys.is_empty() || keys.keys.iter().any(names_it)
    };
    named && mods & keys.mods_mask == keys.mods_value
}

const NO_BUTTON: ButtonDown = ButtonDown { down: false, inside: Inside::NOTHING };
const NO_KEY: KeyDown = KeyDown { key: KeyIdentity::Character(0), base: None };

impl Seen {
    pub(super) const fn new() -> Self {
        Self { keys: Bounded::new(NO_KEY), buttons: 0, over: false, x: 0, y: 0, inside: Inside::NOTHING }
    }
}

impl InputState {
    pub(super) const fn new() -> Self {
        Self { seen: Seen::new(), pressed: [NO_BUTTON; MAX_BUTTON as usize + 1] }
    }

    /// Follows a key event. A press beyond `MAX_KEYS_DOWN` is not tracked
    /// (RULE-8).
    pub(super) fn key(&mut self, action: KeyAction, key: KeyIdentity, base: Option<u32>) {
        let held = self.seen.keys.iter().position(|down| down.key == key);
        match (action, held) {
            (KeyAction::Press, None) => {
                // One press over the limit finds no room, and is not down
                // for conditions.
                let _ = self.seen.keys.push(KeyDown { key, base });
            }
            (KeyAction::Release, Some(place)) => {
                let mut kept = Bounded::new(NO_KEY);
                for (_, down) in self.seen.keys.iter().enumerate().filter(|(at, _)| *at != place) {
                    // One fewer than the list held.
                    let _ = kept.push(*down);
                }
                self.seen.keys = kept;
            }
            _ => {}
        }
    }

    /// Follows a pointer event at a position: what the pointer entered
    /// and left, whether it moved, and for a release what the press was
    /// inside.
    pub(super) fn pointer(&mut self, action: PointerAction, button: u8, now: Inside, (x, y): (u16, u16)) -> Transition {
        let was = self.seen.inside;
        let moved = action == PointerAction::Move && (!self.seen.over || (self.seen.x, self.seen.y) != (x, y));
        (self.seen.over, self.seen.x, self.seen.y, self.seen.inside) = (true, x, y, now);
        let mut released = Inside::NOTHING;
        if button <= MAX_BUTTON && matches!(action, PointerAction::Press | PointerAction::Release) {
            let bit = 1_u8.checked_shl(u32::from(button)).unwrap_or(0);
            if let Some(slot) = self.pressed.get_mut(usize::from(button)) {
                if action == PointerAction::Press {
                    self.seen.buttons |= bit;
                    *slot = ButtonDown { down: true, inside: now };
                } else {
                    self.seen.buttons &= !bit;
                    released = core::mem::replace(slot, NO_BUTTON).inside;
                }
            }
        }
        Transition { entered: now.minus(&was), left: was.minus(&now), moved, released }
    }

    /// The pointer left the page: it is inside nothing.
    pub(super) fn gone(&mut self) -> Transition {
        let was = self.seen.inside;
        (self.seen.over, self.seen.x, self.seen.y, self.seen.inside) = (false, 0, 0, Inside::NOTHING);
        Transition { entered: Inside::NOTHING, left: was, moved: false, released: Inside::NOTHING }
    }

    /// The declared rectangles a pixel is within: bit `n` for rectangle
    /// `n` (RULE-13).
    pub(super) fn rects_at(rects: &[Rect], x: u16, y: u16) -> u16 {
        let mut mask = 0_u16;
        for (rect, bit) in rects.iter().zip(0_u32..) {
            let across = x >= rect.x && u32::from(x) < u32::from(rect.x).saturating_add(u32::from(rect.width));
            let down = y >= rect.y && u32::from(y) < u32::from(rect.y).saturating_add(u32::from(rect.height));
            if across && down {
                mask |= 1_u16.checked_shl(bit).unwrap_or(0);
            }
        }
        mask
    }
}

impl Seen {
    /// Reads a value of space 2 (RULE-24).
    pub(super) fn read(&self, value: Value) -> i32 {
        if value.space != SPACE_INPUT {
            return 0;
        }
        let kind = match value.a {
            INPUT_CHARACTER => Some(KeyKind::Character),
            INPUT_NAMED => Some(KeyKind::Named),
            INPUT_PHYSICAL => Some(KeyKind::Physical),
            _ => None,
        };
        if let Some(kind) = kind {
            let code = value.key_code();
            return i32::from(self.keys.iter().any(|down| key_names(kind, code, down.key, down.base)));
        }
        match (value.a, value.b) {
            (INPUT_BUTTON, button) => i32::from(
                u32::from(button) < u8::BITS && self.buttons.checked_shr(u32::from(button)).unwrap_or(0) & 1 != 0,
            ),
            (INPUT_POINTER, POINTER_OVER) => i32::from(self.over),
            (INPUT_POINTER, POINTER_X) => i32::from(self.x),
            (INPUT_POINTER, POINTER_Y) => i32::from(self.y),
            (INPUT_KEYS_DOWN, _) => i32::try_from(self.keys.len()).unwrap_or(i32::MAX),
            _ => 0,
        }
    }
}
