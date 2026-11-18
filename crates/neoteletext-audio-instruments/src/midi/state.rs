//! What a page's rules read of a synthesizer sound
//! (`neoteletext-common/docs/page-rules.md`, RULE-28 to RULE-31): the
//! classes of value, the entry of a family's table of readable patch fields
//! ([`PatchField`], RULE-29), and the performance state every family that
//! plays notes shares ([`Performance`]): each part's program, its keys and
//! their velocities, its controllers and its pitch bend, kept from the
//! channel messages given to the sound.
//!
//! A family's patch fields are read from its own tracker, by the function
//! beside its table in its interface module; `watch::Watched`
//! holds both for one sound.

use super::{CHANNEL_MASK, CHANNELS, ChannelMessage, DATA_MAX, PITCH_BEND_CENTRE, controller, status};
use crate::midi::common::EXPRESSION_FULL;

/// The families, each by the code of its codec in the audio profile
/// (MIDI-107), which a rule names a family by (RULE-28: "its high byte the
/// family's codec code"). The audio crate asserts they are its codes.
pub mod family {
    /// The synth6581 family: synth6581 and synth8580.
    pub const SYNTH6581: u8 = 12;
    /// synth7.
    pub const SYNTH7: u8 = 13;
    /// The synth106 family: synth106 and synth60.
    pub const SYNTH106: u8 = 14;
    /// synthD.
    pub const SYNTHD: u8 = 15;
    /// The synth53 family: synth53, synth52 and synth51.
    pub const SYNTH53: u8 = 16;
    /// synth8.
    pub const SYNTH8: u8 = 17;
    /// synth80.
    pub const SYNTH80: u8 = 18;
}

/// The classes of value a rule reads of a synthesizer's state, the low byte
/// of field `c` of a value of space 6 (RULE-28).
pub mod class {
    /// The part's program, 0 to 127.
    pub const PROGRAM: u8 = 0;
    /// How many of the part's keys are down, 0 to 128.
    pub const NOTES: u8 = 1;
    /// 1 while key `index` is down on the part, else 0.
    pub const NOTE: u8 = 2;
    /// The velocity of key `index`'s note on, 1 to 127; 0 while it is up.
    pub const VELOCITY: u8 = 3;
    /// Patch field `index` of the family's table, on the field's own scale.
    pub const FIELD: u8 = 4;
    /// The stage of the amplitude envelope of the part's newest voice
    /// ([`crate::midi::MeterStage::code`]).
    pub const ENVELOPE_STAGE: u8 = 5;
    /// That envelope's level, 0 to 255.
    pub const ENVELOPE_LEVEL: u8 = 6;
    /// How many of the instrument's voices are not idle.
    pub const VOICES: u8 = 7;
    /// The last value of controller `index` on the part, 0 to 127.
    pub const CONTROLLER: u8 = 8;
    /// The part's pitch bend, 0 to 16,383, 8,192 at rest.
    pub const BEND: u8 = 9;
}

/// The channel message of a rule's `midi` action (RULE-21), from the three
/// bytes a table and a receiver's pipes carry it as: a note off, a note on,
/// a control change, a program change (whose second data byte is not given
/// to the instrument, and is 0 here) or a pitch bend, each data byte 0 to
/// 127. `None` for any other status (polyphonic key pressure, channel
/// pressure, every system message, a data byte where a status belongs) and
/// for a data byte above 127: a table's decoder refuses them, and neither
/// an instrument nor a tracker is trusted to it.
#[must_use]
pub const fn rule_message(status_byte: u8, data1: u8, data2: u8) -> Option<ChannelMessage> {
    let kind = status_byte & super::KIND_MASK;
    let taken = matches!(
        kind,
        status::NOTE_OFF | status::NOTE_ON | status::CONTROL_CHANGE | status::PROGRAM_CHANGE | status::PITCH_BEND
    );
    if !taken || data1 > DATA_MAX || data2 > DATA_MAX {
        return None;
    }
    let data2 = if kind == status::PROGRAM_CHANGE { 0 } else { data2 };
    Some(ChannelMessage { status: status_byte, data1, data2 })
}

/// One readable patch field of a family (RULE-29): its name, as the program
/// protocol's parameter type of the family names it, and the least and the
/// greatest value it reads. A field that is an enumeration there reads as
/// the ordinal of its value, from 0, and names its values in order; a
/// boolean is the enumeration `false`, `true`. A field's number is its
/// place in its family's table, from 0, and is never reused or reordered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PatchField {
    /// The field's name.
    pub name: &'static str,
    /// The least value it reads.
    pub least: i32,
    /// The greatest value it reads.
    pub greatest: i32,
    /// An enumeration's values in order, the first reading as 0; empty for
    /// a number.
    pub values: &'static [&'static str],
}

impl PatchField {
    /// A number from `least` to `greatest`.
    #[must_use]
    pub const fn range(name: &'static str, least: i32, greatest: i32) -> Self {
        Self { name, least, greatest, values: &[] }
    }

    /// An enumeration of `values`, read as 0 to one less than their count.
    #[must_use]
    pub const fn choice(name: &'static str, values: &'static [&'static str]) -> Self {
        let mut greatest: i32 = 0;
        let mut counted: usize = 1;
        while counted < values.len() {
            greatest = greatest.saturating_add(1);
            counted = counted.saturating_add(1);
        }
        Self { name, least: 0, greatest, values }
    }

    /// `value` held to the field's range: what a read function answers for
    /// a stored byte beyond it.
    #[must_use]
    pub const fn hold(&self, value: i32) -> i32 {
        if value < self.least {
            self.least
        } else if value > self.greatest {
            self.greatest
        } else {
            value
        }
    }
}

/// MIDI keys, 0 to 127.
const KEYS: usize = DATA_MAX as usize + 1;
/// Controllers, 0 to 127.
const CONTROLLERS: usize = DATA_MAX as usize + 1;

/// One part's performance state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Part {
    program: u8,
    /// Each key's note-on velocity while it is down, 0 while it is up.
    velocities: [u8; KEYS],
    /// Each controller's last value, 0 before any.
    controllers: [u8; CONTROLLERS],
    bend: u16,
}

impl Part {
    const NEW: Self =
        Self { program: 0, velocities: [0; KEYS], controllers: [0; CONTROLLERS], bend: PITCH_BEND_CENTRE };

    fn control(&mut self, number: u8, value: u8) {
        if let Some(place) = self.controllers.get_mut(usize::from(number)) {
            *place = value;
        }
        match number {
            // RP-015 (MIDI-84): the controllers a reset sets go back to
            // what a sound starts with, and the bend to its centre.
            controller::RESET_ALL_CONTROLLERS => {
                self.bend = PITCH_BEND_CENTRE;
                for (number, rest) in [
                    (controller::MODULATION, 0),
                    (controller::EXPRESSION, EXPRESSION_FULL),
                    (controller::SUSTAIN, 0),
                    (controller::PORTAMENTO, 0),
                    (controller::SOSTENUTO, 0),
                    (controller::SOFT, 0),
                ] {
                    if let Some(place) = self.controllers.get_mut(usize::from(number)) {
                        *place = rest;
                    }
                }
            }
            // All sound off, all notes off and the mode messages, which
            // act as all notes off (MIDI 1.0 4.2, Table IV): no key of the
            // part is down afterwards.
            controller::ALL_SOUND_OFF | controller::ALL_NOTES_OFF..=controller::POLY_ON => {
                self.velocities = [0; KEYS];
            }
            _ => {}
        }
    }
}

/// The performance state of the sixteen parts of one sound, as the channel
/// messages given to it leave it (RULE-28 classes 0 to 3, 8 and 9). It is
/// the sender's view, as a family's tracker is (MIDI-94): a key is down
/// from its note on to its note off, whatever the instrument's voices do
/// with it. Its size is fixed: sixteen parts of 128 keys and 128
/// controllers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Performance {
    parts: [Part; CHANNELS],
}

impl Default for Performance {
    fn default() -> Self {
        Self::new()
    }
}

impl Performance {
    /// Every part as a sound starts it: program 0, no key down, every
    /// controller 0 and the bend at its centre.
    #[must_use]
    pub const fn new() -> Self {
        Self { parts: [Part::NEW; CHANNELS] }
    }

    /// Takes one channel message given to the sound. A data byte above 127,
    /// which no message carries, changes nothing.
    pub fn take(&mut self, message: ChannelMessage) {
        let ChannelMessage { data1, data2, .. } = message;
        if data1 > DATA_MAX || data2 > DATA_MAX {
            return;
        }
        let Some(part) = self.parts.get_mut(usize::from(message.channel())) else { return };
        match message.kind() {
            status::NOTE_ON | status::NOTE_OFF => {
                // A note on of velocity 0 is a note off (MIDI 1.0 4.2).
                let velocity = if message.kind() == status::NOTE_ON { data2 } else { 0 };
                if let Some(place) = part.velocities.get_mut(usize::from(data1)) {
                    *place = velocity;
                }
            }
            status::CONTROL_CHANGE => part.control(data1, data2),
            status::PROGRAM_CHANGE => part.program = data1,
            status::PITCH_BEND => part.bend = message.bend(),
            _ => {}
        }
    }

    /// Input was lost, or the sound's notes were all released: no key of
    /// any part is down.
    pub fn release_all(&mut self) {
        for part in &mut self.parts {
            part.velocities = [0; KEYS];
        }
    }

    /// The value of `class` for `part` (0 to 15) and `index`, for the
    /// classes kept here ([`class::PROGRAM`], [`class::NOTES`],
    /// [`class::NOTE`], [`class::VELOCITY`], [`class::CONTROLLER`] and
    /// [`class::BEND`]); `None` for another class, and for a key or a
    /// controller above 127.
    #[must_use]
    pub fn read(&self, class: u8, part: u8, index: u16) -> Option<i32> {
        let part = self.parts.get(usize::from(part & CHANNEL_MASK))?;
        let at = usize::from(index);
        match class {
            class::PROGRAM => Some(i32::from(part.program)),
            class::NOTES => {
                let down = part.velocities.iter().filter(|&&velocity| velocity != 0).count();
                i32::try_from(down).ok()
            }
            class::NOTE => part.velocities.get(at).map(|&velocity| i32::from(velocity != 0)),
            class::VELOCITY => part.velocities.get(at).map(|&velocity| i32::from(velocity)),
            class::CONTROLLER => part.controllers.get(at).map(|&value| i32::from(value)),
            class::BEND => Some(i32::from(part.bend)),
            _ => None,
        }
    }
}
