//! The input state (RULE-8): which keys and buttons are down, where the
//! pointer is and which targets it is inside. It follows the events, one
//! by one, and no action can change it.

use crate::bounded::Bounded;
use crate::limits::{MAX_BUTTON, MAX_KEYS_DOWN, MAX_POINTER_OBJECTS, MAX_RECTS};
use crate::table::{
    INPUT_BUTTON, INPUT_CHARACTER, INPUT_KEYS_DOWN, INPUT_NAMED, INPUT_PHYSICAL, INPUT_POINTER, Key, KeyKind,
    POINTER_OVER, POINTER_X, POINTER_Y, Rect, SPACE_INPUT, Target, TargetKind, Value,
};

/// What a key did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyAction {
    /// Pressed.
    Press,
    /// Repeating while held.
    Repeat,
    /// Released, or lost (RULE-8).
    Release,
}

/// Which key, as the input protocol's KEY event names it (RULE-10).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyIdentity {
    /// A character key: the Unicode scalar value the active layout gives
    /// it without Shift.
    Character(u32),
    /// A functional key, by its code in the input protocol's table.
    Named(u32),
}

/// What a pointer did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerAction {
    /// A button or a contact went down.
    Press,
    /// It came up, or was lost (RULE-8).
    Release,
    /// The pointer moved.
    Move,
    /// The pointer left the page (RULE-8): every target it was inside is
    /// left.
    Gone,
}

/// One thing the viewer did, as the receiver gives it to the evaluator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    /// A key.
    Key {
        /// Press, repeat or release.
        action: KeyAction,
        /// The KEY event's `key`.
        key: KeyIdentity,
        /// The KEY event's `base`: the character the physical key has on
        /// a United States layout, when it differs from `key`.
        base: Option<u32>,
        /// The modifier bits held.
        mods: u8,
    },
    /// Text that is not a key: a paste, an input method's composition or
    /// its committed text.
    Text,
    /// A pointer button, contact or move.
    Pointer {
        /// What it did.
        action: PointerAction,
        /// On a press or a release the button: 0 left (or a contact, or a
        /// pen touching), 1 middle, 2 right; others are not followed. On a
        /// move the bit mask of buttons held.
        button: u8,
        /// The pointer's pixel in the picture's grid (RULE-13).
        x: u16,
        /// See `x`.
        y: u16,
        /// The control under the pointer's cell, 1 to 32, or 0 for none.
        control: u16,
    },
    /// The wheel turned.
    Wheel {
        /// The pointer's pixel.
        x: u16,
        /// See `x`.
        y: u16,
        /// The control under the pointer's cell, or 0.
        control: u16,
    },
    /// The page gained the keyboard.
    Focus,
    /// The page lost the keyboard.
    Blur,
}

/// An input event with the number its giver knows it by (RULE-52): a
/// report of the event names the number, never the event's bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// The giver's sequence number of the event.
    pub sequence: u32,
    /// What the viewer did.
    pub input: Input,
}

/// The targets a pointer is inside at once (RULE-12).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Inside {
    /// The page: the pointer is over it.
    pub(super) page: bool,
    /// The control under the pointer, or 0.
    pub(super) control: u16,
    /// The objects the pointer is inside.
    pub(super) objects: Bounded<u16, MAX_POINTER_OBJECTS>,
    /// The declared rectangles the pointer is inside: bit `n` for
    /// rectangle `n`.
    pub(super) rects: u16,
}

// A bit for every rectangle.
const _: () = assert!(MAX_RECTS <= u16::BITS as usize);

impl Inside {
    /// Inside nothing.
    pub(super) const NOTHING: Self = Self { page: false, control: 0, objects: Bounded::new(0), rects: 0 };

    /// Whether `target`, one thing and never "any", is among them.
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

    /// Gives `each` every target of this set that the trigger's `target`
    /// names: itself when it is among them, or for "any" each control or
    /// object in the set (RULE-12). What is given is what a report names
    /// (RULE-46): all zero for the page.
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

/// What one pointer event changed (RULE-9): what `enter`, `leave`, `move`
/// and `release` triggers match against.
#[derive(Clone, Copy)]
pub(super) struct Transition {
    /// The targets the pointer came inside.
    pub(super) entered: Inside,
    /// The targets it went outside.
    pub(super) left: Inside,
    /// Whether a move took it to another pixel.
    pub(super) moved: bool,
    /// The targets the button released went down inside.
    pub(super) released: Inside,
}

impl Transition {
    /// Nothing changed: what an event that is no pointer's gives.
    pub(super) const NONE: Self =
        Self { entered: Inside::NOTHING, left: Inside::NOTHING, moved: false, released: Inside::NOTHING };
}

/// A key held down: how the event that pressed it named it.
type Held = (KeyIdentity, Option<u32>);

/// The input state (RULE-8).
#[derive(Clone, Copy)]
pub(super) struct InputState {
    /// The keys down, at most 16: a 17th press is not tracked.
    keys: Bounded<Held, MAX_KEYS_DOWN>,
    /// The buttons down over the page: bit `n` for button `n`.
    pub(super) buttons: u8,
    /// Whether the pointer is over the page.
    over: bool,
    /// The pointer's pixel; (0, 0) while it is not over the page.
    x: u16,
    y: u16,
    /// What the pointer is inside.
    pub(super) inside: Inside,
    /// What each button went down inside.
    pressed: [Inside; MAX_BUTTON as usize + 1],
}

/// Whether `key` of a key set or of a key-state value names the key an
/// event called `identity`, with `base` (RULE-10).
pub(super) fn key_names(key: Key, identity: KeyIdentity, base: Option<u32>) -> bool {
    match key.kind {
        KeyKind::Character => identity == KeyIdentity::Character(key.code),
        KeyKind::Named => identity == KeyIdentity::Named(key.code),
        // The KEY event's `base` when it has one, else its `key` when that
        // is a character (DOC-1937).
        KeyKind::Physical => base.map_or(identity == KeyIdentity::Character(key.code), |base| base == key.code),
    }
}

impl InputState {
    /// Nothing down, the pointer not over the page.
    pub(super) const fn new() -> Self {
        Self {
            keys: Bounded::new((KeyIdentity::Character(0), None)),
            buttons: 0,
            over: false,
            x: 0,
            y: 0,
            inside: Inside::NOTHING,
            pressed: [Inside::NOTHING; MAX_BUTTON as usize + 1],
        }
    }

    /// Follows a key event (RULE-8).
    pub(super) fn key(&mut self, action: KeyAction, identity: KeyIdentity, base: Option<u32>) {
        let held = self.keys.iter().position(|(down, _)| *down == identity);
        match (action, held) {
            // A 17th press finds no room, and is not down for conditions.
            (KeyAction::Press, None) => {
                let _ = self.keys.push((identity, base));
            }
            (KeyAction::Release, Some(place)) => {
                let mut kept = Bounded::new((KeyIdentity::Character(0), None));
                for (_, key) in self.keys.iter().enumerate().filter(|(at, _)| *at != place) {
                    // One fewer than the list held.
                    let _ = kept.push(*key);
                }
                self.keys = kept;
            }
            _ => {}
        }
    }

    /// Follows a pointer event at a pixel: where the pointer is and what
    /// it is inside (RULE-12), and for a press or a release the button.
    pub(super) fn pointer(&mut self, action: PointerAction, button: u8, now: Inside, x: u16, y: u16) -> Transition {
        let was = self.inside;
        let moved = action == PointerAction::Move && (!self.over || (self.x, self.y) != (x, y));
        (self.over, self.x, self.y, self.inside) = (true, x, y, now);
        let mut released = Inside::NOTHING;
        if button <= MAX_BUTTON {
            let bit = 1_u8.checked_shl(u32::from(button)).unwrap_or(0);
            let slot = self.pressed.get_mut(usize::from(button));
            match (action, slot) {
                (PointerAction::Press, Some(slot)) => {
                    self.buttons |= bit;
                    *slot = now;
                }
                (PointerAction::Release, Some(slot)) => {
                    self.buttons &= !bit;
                    released = core::mem::replace(slot, Inside::NOTHING);
                }
                _ => {}
            }
        }
        Transition { entered: now.minus(&was), left: was.minus(&now), moved, released }
    }

    /// The pointer left the page (RULE-8): it is inside nothing.
    pub(super) fn gone(&mut self) -> Transition {
        let was = self.inside;
        (self.over, self.x, self.y, self.inside) = (false, 0, 0, Inside::NOTHING);
        Transition { entered: Inside::NOTHING, left: was, moved: false, released: Inside::NOTHING }
    }

    /// The declared rectangles a pixel is inside (RULE-13).
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
            let key = Key { kind, code: value.key_code() };
            return i32::from(self.keys.iter().any(|(identity, base)| key_names(key, *identity, *base)));
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
