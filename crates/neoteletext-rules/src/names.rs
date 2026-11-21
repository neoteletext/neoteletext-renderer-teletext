//! The words the program protocol writes each code of a rule table as
//! (page-rules.md section 7, RULE-35, RULE-114), each beside its code.
//!
//! A receiver parses no text (RULE-36): these tables are for the callers
//! that read the protocol's JSON, the server and `neottx rules-run`, so
//! that both turn a word into the same number from one list. Rule, group,
//! sequence, row-set and rectangle names are a program's own and are not
//! here; neither are key names (`neoteletext_input::keys`) nor a family's
//! parameter names (`neoteletext-audio-instruments`), which this crate
//! does not know (RULE-56).

use crate::animate::code as operation;
use crate::command::{VariableOperation, midi_status};
use crate::limits::{PAGE_TIMER, TIMERS, VARIABLES};
use crate::table::{
    CLASS_BEND, CLASS_CONTROLLER, CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_FIELD, CLASS_NOTE, CLASS_NOTES,
    CLASS_PROGRAM, CLASS_VELOCITY, CLASS_VOICES, FIRED_CUE, FIRED_KEY, FIRED_P, FIRED_ROW, FIRED_TARGET, POINTER_OVER,
    POINTER_X, POINTER_Y, SEQUENCE_PASSES, SEQUENCE_POSITION, SEQUENCE_RUNNING, SEQUENCE_WAITING, SOUND_GAIN,
    SOUND_HANDLE, SOUND_LEVEL, SOUND_LOCAL, SOUND_PLAYING, TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME, TARGET_INSIDE,
    TARGET_LAYER, TARGET_LOOK, TARGET_ORDER, TARGET_SHOWN, TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_PHASE,
    TIMER_RUNNING, trigger_code,
};

/// A trigger's `event` (RULE-35) and its code (RULE-9).
pub const TRIGGERS: [(&str, u8); 18] = [
    ("key", trigger_code::KEY),
    ("key_up", trigger_code::KEY_UP),
    ("press", trigger_code::PRESS),
    ("release", trigger_code::RELEASE),
    ("enter", trigger_code::ENTER),
    ("leave", trigger_code::LEAVE),
    ("open", trigger_code::OPEN),
    ("close", trigger_code::CLOSE),
    ("becomes", trigger_code::BECOMES),
    ("every", trigger_code::EVERY),
    ("after", trigger_code::AFTER),
    ("text", trigger_code::TEXT),
    ("move", trigger_code::MOVE),
    ("wheel", trigger_code::WHEEL),
    ("focus", trigger_code::FOCUS),
    ("blur", trigger_code::BLUR),
    ("cue", trigger_code::CUE),
    // DEC-7031 item 4.
    ("painted", trigger_code::PAINTED),
];

/// A comparison's `op` (RULE-35) and its code (RULE-18).
pub const OPERATORS: [(&str, u8); 6] = [("==", 1), ("!=", 2), ("<", 3), ("<=", 4), (">", 5), (">=", 6)];

/// A `variable` message's `op` (RULE-114) and its code (RULE-99).
pub const VARIABLE_OPERATIONS: [(&str, u8); 10] = [
    ("set", VariableOperation::Set.code()),
    ("inc", VariableOperation::Inc.code()),
    ("dec", VariableOperation::Dec.code()),
    ("add", VariableOperation::Add.code()),
    ("sub", VariableOperation::Sub.code()),
    ("toggle", VariableOperation::Toggle.code()),
    ("min", VariableOperation::Min.code()),
    ("max", VariableOperation::Max.code()),
    ("wrap", VariableOperation::Wrap.code()),
    ("clear", VariableOperation::Clear.code()),
];

/// A `local_midi` message's `event` (RULE-114) and its status byte's
/// kind (RULE-21).
pub const MIDI_EVENTS: [(&str, u8); 5] = [
    ("note_off", midi_status::NOTE_OFF),
    ("note_on", midi_status::NOTE_ON),
    ("control_change", midi_status::CONTROL_CHANGE),
    ("program_change", midi_status::PROGRAM_CHANGE),
    ("pitch_bend", midi_status::PITCH_BEND),
];

/// An `animate` operation's `action` (ANIM-21) and its code (ANIM-6).
pub const OPERATIONS: [(&str, u8); 9] = [
    ("show", operation::SHOW),
    ("hide", operation::HIDE),
    ("look", operation::LOOK),
    ("move", operation::MOVE),
    ("order", operation::ORDER),
    ("frame", operation::FRAME),
    ("cycle", operation::CYCLE),
    ("stop_cycle", operation::STOP_CYCLE),
    ("reset", operation::RESET),
];

/// A look's name (ANIM-6) and its number.
pub const LOOKS: [(&str, u8); 4] = [("normal", 0), ("hover", 1), ("pressed", 2), ("alternative", 3)];

/// A `pointer` value's `property` (RULE-24).
pub const POINTER_PROPERTIES: [(&str, u16); 3] = [("over", POINTER_OVER), ("x", POINTER_X), ("y", POINTER_Y)];

/// A `timer` value's `property` (RULE-25).
pub const TIMER_PROPERTIES: [(&str, u16); 3] =
    [("running", TIMER_RUNNING), ("elapsed", TIMER_ELAPSED), ("phase", TIMER_PHASE)];

/// A `target` value's `property` (RULE-26).
pub const TARGET_PROPERTIES: [(&str, u16); 10] = [
    ("exists", TARGET_EXISTS),
    ("shown", TARGET_SHOWN),
    ("frame", TARGET_FRAME),
    ("x", TARGET_X),
    ("y", TARGET_Y),
    ("look", TARGET_LOOK),
    ("layer", TARGET_LAYER),
    ("order", TARGET_ORDER),
    ("cycling", TARGET_CYCLING),
    ("inside", TARGET_INSIDE),
];

/// A `sound` value's `property` (RULE-27).
pub const SOUND_PROPERTIES: [(&str, u16); 5] = [
    ("playing", SOUND_PLAYING),
    ("level", SOUND_LEVEL),
    ("handle", SOUND_HANDLE),
    ("gain", SOUND_GAIN),
    ("local", SOUND_LOCAL),
];

/// A `synth` value's `property` and its class (RULE-28).
pub const SYNTH_PROPERTIES: [(&str, u8); 10] = [
    ("program", CLASS_PROGRAM),
    ("notes", CLASS_NOTES),
    ("note", CLASS_NOTE),
    ("velocity", CLASS_VELOCITY),
    ("field", CLASS_FIELD),
    ("envelope_stage", CLASS_ENVELOPE_STAGE),
    ("envelope_level", CLASS_ENVELOPE_LEVEL),
    ("voices", CLASS_VOICES),
    ("controller", CLASS_CONTROLLER),
    ("bend", CLASS_BEND),
];

/// A `fired` value's `property` (RULE-116). The first four are also the
/// values of the firing an amount may name (RULE-106), by the low nibble
/// of the source byte.
pub const FIRED_PROPERTIES: [(&str, u16); 5] =
    [("key", FIRED_KEY), ("target", FIRED_TARGET), ("row", FIRED_ROW), ("cue", FIRED_CUE), ("p", FIRED_P)];

/// A `sequence` value's `property` (RULE-117).
pub const SEQUENCE_PROPERTIES: [(&str, u16); 4] = [
    ("running", SEQUENCE_RUNNING),
    ("passes", SEQUENCE_PASSES),
    ("position", SEQUENCE_POSITION),
    ("waiting", SEQUENCE_WAITING),
];

/// The designators of the sixteen variables (RULE-14), `v0` first.
pub const VARIABLE_NAMES: [&str; VARIABLES] =
    ["v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13", "v14", "v15"];

/// The designators of the eight timers (RULE-15), `t0` first.
pub const TIMER_NAMES: [&str; TIMERS] = ["t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7"];

/// The designator of the page timer (RULE-15).
pub const PAGE_TIMER_NAME: &str = "page";

/// The designator of a row's one parameter (RULE-105, DEC-6635 item 8).
pub const PARAMETER_NAME: &str = "p";

/// The code `name` has in `table`.
#[must_use]
pub fn code<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table.iter().find(|(held, _)| *held == name).map(|(_, code)| *code)
}

/// The name `code` has in `table`.
#[must_use]
pub fn name<T: Copy + PartialEq>(table: &[(&'static str, T)], code: T) -> Option<&'static str> {
    table.iter().find(|(_, held)| *held == code).map(|(name, _)| *name)
}

/// The place of `name` in a list of designators.
fn place(names: &[&str], name: &str) -> Option<u8> {
    names.iter().position(|held| *held == name).and_then(|place| u8::try_from(place).ok())
}

/// The number of the variable designated `name`, `v0` to `v15`.
#[must_use]
pub fn variable(name: &str) -> Option<u8> {
    place(&VARIABLE_NAMES, name)
}

/// The identifier of the timer designated `name`: `t0` to `t7`, or
/// `page` for the page timer.
#[must_use]
pub fn timer(name: &str) -> Option<u8> {
    if name == PAGE_TIMER_NAME { Some(PAGE_TIMER) } else { place(&TIMER_NAMES, name) }
}

/// Whether `name` designates the row parameter, `p`.
#[must_use]
pub fn is_parameter(name: &str) -> bool {
    name == PARAMETER_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RULE-35, RULE-36 (REQ-6526): each word has one code and each code
    /// one word, and the designators are the fixed ones of RULE-14,
    /// RULE-15 and RULE-105.
    #[test]
    fn each_word_has_one_code() {
        fn distinct<T: Copy + PartialEq + core::fmt::Debug>(table: &[(&'static str, T)]) {
            for (word, held) in table {
                assert_eq!(code(table, word), Some(*held), "{word}");
                assert_eq!(name(table, *held), Some(*word), "{word}");
            }
        }
        distinct(&TRIGGERS);
        distinct(&OPERATORS);
        distinct(&VARIABLE_OPERATIONS);
        distinct(&MIDI_EVENTS);
        distinct(&OPERATIONS);
        distinct(&LOOKS);
        distinct(&POINTER_PROPERTIES);
        distinct(&TIMER_PROPERTIES);
        distinct(&TARGET_PROPERTIES);
        distinct(&SOUND_PROPERTIES);
        distinct(&SYNTH_PROPERTIES);
        distinct(&FIRED_PROPERTIES);
        distinct(&SEQUENCE_PROPERTIES);
        assert_eq!(code(&TRIGGERS, "enable"), None);
        assert_eq!((variable("v0"), variable("v15"), variable("v16")), (Some(0), Some(15), None));
        assert_eq!((timer("t7"), timer("page"), timer("t8")), (Some(7), Some(PAGE_TIMER), None));
        assert!(is_parameter("p") && !is_parameter("p0"), "one row parameter (DEC-6635 item 8)");
        assert_eq!(code(&TRIGGERS, "held"), None, "withdrawn (DEC-6635 item 7)");
        for (operation, (_, held)) in VariableOperation::ALL.into_iter().zip(VARIABLE_OPERATIONS) {
            assert_eq!(operation.code(), held);
        }
    }
}
