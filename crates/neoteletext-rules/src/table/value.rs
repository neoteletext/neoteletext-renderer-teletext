//! The values a condition can read (RULE-23 to RULE-28): each named by 8
//! bytes, a space, a byte `a` and three 16-bit fields `b`, `c` and `d`
//! (RULE-44).

use super::{Fault, TargetKind};
use crate::limits::{
    AUDIO_CHANNELS, FIRST_FAMILY, LAST_FAMILY, MAX_BUTTON, MAX_CONTROL, MAX_MIDI_NUMBER, MAX_PARAMETER, MAX_PART,
    MAX_SYNTH_CLASS, MAX_VARIANT, PAGE_TIMER,
};
use crate::wire::{Reader, Writer};

/// The bytes of a value in a record (RULE-44).
pub const VALUE_BYTES: usize = 8;

/// Space 1: page variables (RULE-23).
pub const SPACE_VARIABLE: u8 = 1;
/// Space 2: key and pointer state (RULE-24).
pub const SPACE_INPUT: u8 = 2;
/// Space 3: timers (RULE-25).
pub const SPACE_TIMER: u8 = 3;
/// Space 4: target state (RULE-26).
pub const SPACE_TARGET: u8 = 4;
/// Space 5: a sound on an audio channel (RULE-27).
pub const SPACE_SOUND: u8 = 5;
/// Space 6: a synthesizer's state (RULE-28).
pub const SPACE_SYNTH: u8 = 6;

/// Space 2, `a`: a character key's state (RULE-24).
pub const INPUT_CHARACTER: u8 = 1;
/// Space 2, `a`: a named key's state.
pub const INPUT_NAMED: u8 = 2;
/// Space 2, `a`: a physical key's state.
pub const INPUT_PHYSICAL: u8 = 3;
/// Space 2, `a`: a pointer button's state, `b` the button.
pub const INPUT_BUTTON: u8 = 4;
/// Space 2, `a`: the pointer, `b` one of [`POINTER_OVER`], [`POINTER_X`]
/// and [`POINTER_Y`].
pub const INPUT_POINTER: u8 = 5;
/// Space 2, `a`: the count of keys down.
pub const INPUT_KEYS_DOWN: u8 = 6;
/// The pointer is over the page.
pub const POINTER_OVER: u16 = 0;
/// The pointer's x pixel.
pub const POINTER_X: u16 = 1;
/// The pointer's y pixel.
pub const POINTER_Y: u16 = 2;

/// Space 3, `b`: whether the timer runs (RULE-25).
pub const TIMER_RUNNING: u16 = 0;
/// Space 3, `b`: the ticks since the timer started.
pub const TIMER_ELAPSED: u16 = 1;

/// Space 4, `c`: the target exists (RULE-26).
pub const TARGET_EXISTS: u16 = 0;
/// Space 4, `c`: it is shown.
pub const TARGET_SHOWN: u16 = 1;
/// Space 4, `c`: its frame.
pub const TARGET_FRAME: u16 = 2;
/// Space 4, `c`: its x.
pub const TARGET_X: u16 = 3;
/// Space 4, `c`: its y.
pub const TARGET_Y: u16 = 4;
/// Space 4, `c`: the look in force.
pub const TARGET_LOOK: u16 = 5;
/// Space 4, `c`: its layer.
pub const TARGET_LAYER: u16 = 6;
/// Space 4, `c`: its order.
pub const TARGET_ORDER: u16 = 7;
/// Space 4, `c`: it is cycling.
pub const TARGET_CYCLING: u16 = 8;
/// Space 4, `c`: the pointer is inside it. This one is input state
/// (RULE-8), not the snapshot.
pub const TARGET_INSIDE: u16 = 9;

/// Space 5, `b`: a sound plays on the channel (RULE-27).
pub const SOUND_PLAYING: u16 = 0;
/// Space 5, `b`: the channel's level, 0 to 255 (RULE-32).
pub const SOUND_LEVEL: u16 = 1;
/// Space 5, `b`: the handle playing, or −1.
pub const SOUND_HANDLE: u16 = 2;
/// Space 5, `b`: the gain last commanded.
pub const SOUND_GAIN: u16 = 3;
/// Space 5, `b`: the sound playing is a local sound.
pub const SOUND_LOCAL: u16 = 4;

/// Space 6 class: the part's program (RULE-28).
pub const CLASS_PROGRAM: u8 = 0;
/// Space 6 class: how many of the part's keys are down.
pub const CLASS_NOTES: u8 = 1;
/// Space 6 class: whether key `d` is down.
pub const CLASS_NOTE: u8 = 2;
/// Space 6 class: the velocity of key `d`.
pub const CLASS_VELOCITY: u8 = 3;
/// Space 6 class: patch field `d`'s present value (appendix A).
pub const CLASS_FIELD: u8 = 4;
/// Space 6 class: the envelope stage of the part's newest voice.
pub const CLASS_ENVELOPE_STAGE: u8 = 5;
/// Space 6 class: the envelope level of the same voice.
pub const CLASS_ENVELOPE_LEVEL: u8 = 6;
/// Space 6 class: how many of the instrument's voices are not idle.
pub const CLASS_VOICES: u8 = 7;
/// Space 6 class: the last value of controller `d`.
pub const CLASS_CONTROLLER: u8 = 8;
/// Space 6 class: the part's pitch bend.
pub const CLASS_BEND: u8 = 9;

/// The bits of half a 32-bit key code (RULE-24: `b` the high half, `c`
/// the low).
const HALF_BITS: u32 = 16;
/// The bits of a byte, for the two bytes of `b` and of `c` in space 6.
const BYTE_BITS: u32 = 8;
/// The low byte of a 16-bit field.
const LOW_BYTE: u16 = 0x00FF;

/// A value's name (RULE-44). Every value reads as a signed 32-bit integer;
/// one that cannot be read reads as 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Value {
    /// The space, 1 to 6.
    pub space: u8,
    /// The space's first field.
    pub a: u8,
    /// Its second.
    pub b: u16,
    /// Its third.
    pub c: u16,
    /// Its fourth.
    pub d: u16,
}

impl Value {
    /// What a `send` of the trigger's own event carries in place of a
    /// value (RULE-45): all zero. It names no value.
    pub const NONE: Self = Self { space: 0, a: 0, b: 0, c: 0, d: 0 };

    /// Variable `number` (RULE-23).
    #[must_use]
    pub const fn variable(number: u8) -> Self {
        Self { space: SPACE_VARIABLE, a: number, b: 0, c: 0, d: 0 }
    }

    /// Whether a key is down (RULE-24): `kind` one of [`INPUT_CHARACTER`],
    /// [`INPUT_NAMED`] and [`INPUT_PHYSICAL`], `code` the key's 32-bit
    /// code.
    #[must_use]
    pub const fn key(kind: u8, code: u32) -> Self {
        Self { space: SPACE_INPUT, a: kind, b: (code >> HALF_BITS) as u16, c: code as u16, d: 0 }
    }

    /// Whether pointer button `button` is down over the page (RULE-24).
    #[must_use]
    pub const fn button(button: u8) -> Self {
        Self { space: SPACE_INPUT, a: INPUT_BUTTON, b: button as u16, c: 0, d: 0 }
    }

    /// The pointer (RULE-24): `property` one of [`POINTER_OVER`],
    /// [`POINTER_X`] and [`POINTER_Y`].
    #[must_use]
    pub const fn pointer(property: u16) -> Self {
        Self { space: SPACE_INPUT, a: INPUT_POINTER, b: property, c: 0, d: 0 }
    }

    /// The count of keys down (RULE-24).
    #[must_use]
    pub const fn keys_down() -> Self {
        Self { space: SPACE_INPUT, a: INPUT_KEYS_DOWN, b: 0, c: 0, d: 0 }
    }

    /// A timer's property (RULE-25): `timer` 0 to 7 or 255 for the page
    /// timer, `property` [`TIMER_RUNNING`] or [`TIMER_ELAPSED`].
    #[must_use]
    pub const fn timer(timer: u8, property: u16) -> Self {
        Self { space: SPACE_TIMER, a: timer, b: property, c: 0, d: 0 }
    }

    /// A control's or an object's property (RULE-26), one of
    /// [`TARGET_EXISTS`] to [`TARGET_INSIDE`].
    #[must_use]
    pub const fn target(kind: TargetKind, number: u16, property: u16) -> Self {
        Self { space: SPACE_TARGET, a: kind.code(), b: number, c: property, d: 0 }
    }

    /// A property of the sound on an audio channel (RULE-27), one of
    /// [`SOUND_PLAYING`] to [`SOUND_LOCAL`].
    #[must_use]
    pub const fn sound(channel: u8, property: u16) -> Self {
        Self { space: SPACE_SOUND, a: channel, b: property, c: 0, d: 0 }
    }

    /// A synthesizer's state (RULE-28): the audio channel, the family's
    /// codec code and variant the rule expects, the part (a MIDI channel
    /// less one), the class and the index within it.
    #[must_use]
    pub const fn synth(channel: u8, family: u8, variant: u8, part: u8, class: u8, index: u16) -> Self {
        Self {
            space: SPACE_SYNTH,
            a: channel,
            b: ((family as u16) << BYTE_BITS) | variant as u16,
            c: ((part as u16) << BYTE_BITS) | class as u16,
            d: index,
        }
    }

    /// Space 2's key code (RULE-24): `b` the high half, `c` the low.
    #[must_use]
    pub const fn key_code(&self) -> u32 {
        ((self.b as u32) << HALF_BITS) | self.c as u32
    }

    /// Space 6's family code, the high byte of `b` (RULE-28).
    #[must_use]
    pub const fn family(&self) -> u8 {
        (self.b >> BYTE_BITS) as u8
    }

    /// Space 6's variant, the low byte of `b`.
    #[must_use]
    pub const fn variant(&self) -> u8 {
        (self.b & LOW_BYTE) as u8
    }

    /// Space 6's part, the high byte of `c`.
    #[must_use]
    pub const fn part(&self) -> u8 {
        (self.c >> BYTE_BITS) as u8
    }

    /// Space 6's class, the low byte of `c`.
    #[must_use]
    pub const fn class(&self) -> u8 {
        (self.c & LOW_BYTE) as u8
    }

    /// Whether the value is input state (RULE-8), read as the events leave
    /// it, and not from the snapshot (RULE-24, RULE-26 property 9).
    #[must_use]
    pub const fn is_input_state(&self) -> bool {
        self.space == SPACE_INPUT || (self.space == SPACE_TARGET && self.c == TARGET_INSIDE)
    }

    /// The target a space 4 value names.
    #[must_use]
    pub const fn named_target(&self) -> Option<super::Target> {
        if self.space != SPACE_TARGET {
            return None;
        }
        match TargetKind::from_code(self.a) {
            Some(kind) => Some(super::Target { kind, number: self.b }),
            None => None,
        }
    }

    /// Checks every field against its space's ranges (RULE-44), in a table
    /// of `variables` variables and `timers` timers.
    ///
    /// # Errors
    /// [`Fault::Range`] for a field outside its range or an unused field
    /// that is not 0; [`Fault::Reference`] for a variable or a timer the
    /// table does not have.
    pub fn check(&self, variables: usize, timers: usize) -> Result<(), Fault> {
        let unused = |fields: &[u16]| {
            if fields.iter().all(|field| *field == 0) { Ok(()) } else { Err(Fault::Range("value's unused field")) }
        };
        match self.space {
            SPACE_VARIABLE => {
                unused(&[self.b, self.c, self.d])?;
                if usize::from(self.a) < variables { Ok(()) } else { Err(Fault::Reference("value's variable")) }
            }
            SPACE_INPUT => self.check_input(),
            SPACE_TIMER => {
                unused(&[self.c, self.d])?;
                if self.b > TIMER_ELAPSED {
                    return Err(Fault::Range("timer property"));
                }
                if usize::from(self.a) < timers || self.a == PAGE_TIMER {
                    Ok(())
                } else {
                    Err(Fault::Reference("value's timer"))
                }
            }
            SPACE_TARGET => {
                unused(&[self.d])?;
                let thing = match TargetKind::from_code(self.a) {
                    Some(TargetKind::Control) => self.b >= 1 && self.b <= MAX_CONTROL,
                    Some(TargetKind::Object) => self.b >= 1,
                    _ => false,
                };
                if thing && self.c <= TARGET_INSIDE { Ok(()) } else { Err(Fault::Range("target value")) }
            }
            SPACE_SOUND => {
                unused(&[self.c, self.d])?;
                if usize::from(self.a) < AUDIO_CHANNELS && self.b <= SOUND_LOCAL {
                    Ok(())
                } else {
                    Err(Fault::Range("sound value"))
                }
            }
            SPACE_SYNTH => self.check_synth(),
            _ => Err(Fault::Range("value's space")),
        }
    }

    /// Space 2's ranges (RULE-24).
    fn check_input(self) -> Result<(), Fault> {
        let refused = Err(Fault::Range("input value"));
        match self.a {
            // A character or a physical key is a Unicode scalar value; a
            // named key's code is the caller's to check (RULE-56).
            INPUT_CHARACTER | INPUT_PHYSICAL if self.d == 0 && char::from_u32(self.key_code()).is_some() => Ok(()),
            INPUT_NAMED if self.d == 0 => Ok(()),
            INPUT_BUTTON if self.b <= u16::from(MAX_BUTTON) && self.c == 0 && self.d == 0 => Ok(()),
            INPUT_POINTER if self.b <= POINTER_Y && self.c == 0 && self.d == 0 => Ok(()),
            INPUT_KEYS_DOWN if self.b == 0 && self.c == 0 && self.d == 0 => Ok(()),
            _ => refused,
        }
    }

    /// Space 6's ranges (RULE-28, RULE-44).
    fn check_synth(self) -> Result<(), Fault> {
        let family = self.family();
        let in_range = usize::from(self.a) < AUDIO_CHANNELS
            && (FIRST_FAMILY..=LAST_FAMILY).contains(&family)
            && self.variant() <= MAX_VARIANT
            && self.part() <= MAX_PART
            && self.class() <= MAX_SYNTH_CLASS;
        let index = match self.class() {
            CLASS_NOTE | CLASS_VELOCITY | CLASS_CONTROLLER => self.d <= MAX_MIDI_NUMBER,
            CLASS_FIELD => self.d <= MAX_PARAMETER,
            _ => self.d == 0,
        };
        if in_range && index { Ok(()) } else { Err(Fault::Range("synthesizer value")) }
    }

    /// Reads a value's 8 bytes, unchecked: [`Value::check`] checks it.
    pub(crate) fn read(reader: &mut Reader<'_>) -> Result<Self, Fault> {
        Ok(Self { space: reader.u8()?, a: reader.u8()?, b: reader.u16()?, c: reader.u16()?, d: reader.u16()? })
    }

    /// Writes a value's 8 bytes.
    pub(crate) fn write(self, writer: &mut Writer<'_>) -> Result<(), Fault> {
        writer.u8(self.space)?;
        writer.u8(self.a)?;
        writer.u16(self.b)?;
        writer.u16(self.c)?;
        writer.u16(self.d)
    }

    /// Reads a value's 8 bytes from the front of `bytes`.
    ///
    /// # Errors
    /// [`Fault::Short`] when fewer than 8 bytes are given. The fields are
    /// not checked: [`Value::check`] does that.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Fault> {
        Self::read(&mut Reader::new(bytes))
    }

    /// The value's 8 bytes (RULE-44).
    #[must_use]
    pub fn to_bytes(&self) -> [u8; VALUE_BYTES] {
        let mut bytes = [0_u8; VALUE_BYTES];
        // Eight bytes into eight: the writer has room.
        let _ = self.write(&mut Writer::new(&mut bytes));
        bytes
    }
}
