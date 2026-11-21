//! Every bound of the page rule table (RULE-57, REQ-6608), each defined
//! once here and used by name everywhere else. They are maximums: a table
//! over any of them is refused whole (RULE-70), and no receiver holds a
//! lower or a higher one. No other file states one of these figures.
//!
//! [`RULE_57`] lists each constant of the requirement's table by name with
//! its value, so that a refusal can give the value beside the name
//! ([`value_of`]) and a document can be generated from, or compared with,
//! the constants.

/// Commands in one group (RULE-118).
pub const MAX_GROUP_COMMANDS: usize = 32;
/// Groups a page, the unnamed ones the server makes among them (RULE-118).
pub const MAX_GROUPS: usize = 64;
/// The depth to which a group may run other groups; no cycle is allowed at
/// any depth (RULE-120).
pub const MAX_GROUP_DEPTH: usize = 4;
/// Commands applied by one run of a group, counting every group it runs
/// (RULE-120).
pub const MAX_RUN_COMMANDS: usize = 128;
/// Bytes of one command's body in a group (RULE-118).
pub const MAX_COMMAND_BYTES: usize = 256;
/// Bytes of all of a page's groups together, as framed (RULE-120).
pub const MAX_GROUPS_BYTES: usize = 16_384;
/// Rules a page (RULE-3).
pub const MAX_RULES: usize = 64;
/// Binding rows a page (RULE-105).
pub const MAX_ROWS: usize = 64;
/// Sequences a page (RULE-103).
pub const MAX_SEQUENCES: usize = 8;
/// Steps in one sequence (RULE-103).
pub const MAX_SEQUENCE_STEPS: usize = 64;
/// The stated repeats of a sequence, 1 to this; or until stopped
/// (RULE-103).
pub const MAX_SEQUENCE_REPEAT: u8 = 255;
/// Timers a page, exactly, `t0` to `t7`, beside the page timer (RULE-15).
pub const TIMERS: usize = 8;
/// Variables a page, exactly, `v0` to `v15`, each a saturating 32-bit
/// signed integer (RULE-14).
pub const VARIABLES: usize = 16;
/// Terms in one rule's condition (RULE-18).
pub const MAX_CONDITION_TERMS: usize = 8;
/// Depth of nesting in one rule's condition (RULE-18): a term directly
/// under this many operators is allowed, one operator level more is
/// refused.
pub const MAX_CONDITION_DEPTH: usize = 3;
/// Nodes in one rule's condition, terms and operators together (RULE-18).
pub const MAX_CONDITION_NODES: usize = 16;
/// Constants in an `in_set` (RULE-18).
pub const MAX_SET_MEMBERS: usize = 8;
/// Children of an `and` or an `or` (RULE-18).
pub const MAX_AND_OR_CHILDREN: usize = 8;
/// Triggers on one rule, any of which fires it (RULE-107).
pub const MAX_TRIGGERS_PER_RULE: usize = 4;
/// Keys in one trigger's key set (RULE-10).
pub const MAX_KEYS_PER_TRIGGER: usize = 16;
/// The sum, over a page's rules, of the rows each names, a rule without
/// rows counting one: the most firings one event can cause (RULE-43).
pub const MAX_RULE_ROWS: usize = 128;
/// Bindings on one command (RULE-106).
pub const MAX_COMMAND_BINDINGS: usize = 8;
/// Rectangles a page's table declares (RULE-13).
pub const MAX_RECTS: usize = 16;
/// A sequence's length in ticks: one hour (RULE-103).
pub const MAX_SEQUENCE_TICKS: u32 = 216_000;
/// `ticks` of `every` and `after`, the period of a timer's phase and of a
/// sequence's `beat` start: one hour (RULE-9, RULE-25, RULE-103).
pub const MAX_TIMER_TICKS: u32 = 216_000;
/// How far after the present tick a sequence's start `at` a page tick may
/// be: one hour (RULE-103).
pub const MAX_START_TICKS: u32 = 216_000;
/// The highest cue number (RULE-109).
pub const MAX_CUE: u8 = 127;
/// Events handled in one tick: input events, the server's records the
/// evaluator carries out, cues, paintings finished (DEC-7031 item 4) and
/// state queries together (a per-tick budget, RULE-5).
pub const MAX_EVENTS_PER_TICK: usize = 16;
/// Commands applied by the runs of the events handled in one tick (a
/// per-tick budget, RULE-5): `MAX_EVENTS_PER_TICK × MAX_RUN_COMMANDS`,
/// asserted below.
pub const MAX_EVENT_COMMANDS_PER_TICK: usize = 2048;
/// `REPORT` commands applied by the runs of the events handled in one tick
/// (a per-tick budget, RULE-5): `2 × MAX_EVENTS_PER_TICK`, asserted below,
/// so that every event of a full tick may report its firing and one value.
pub const MAX_EVENT_REPORTS_PER_TICK: usize = 32;
/// Events waiting for a later tick, in arrival order (RULE-5).
pub const MAX_QUEUED_EVENTS: usize = 64;
/// Keys tracked as down (RULE-8).
pub const MAX_KEYS_DOWN: usize = 16;
/// Objects the pointer is followed inside at once (RULE-12).
pub const MAX_POINTER_OBJECTS: usize = 16;
/// Distinct values a table reads (RULE-47).
pub const MAX_TABLE_VALUES: usize = 64;
/// Distinct controls and objects a table names by a constant (RULE-47).
pub const MAX_TABLE_TARGETS: usize = 64;
/// Distinct metered values a table reads (RULE-33).
pub const MAX_WATCHED: usize = 32;
/// Distinct drawings a table's `painted` triggers name, by a constant or
/// by a row's `p` (DEC-7031 item 4): what the ReGIS renderer watches at
/// once, asserted equal to its limit where both are linked.
pub const MAX_PAINT_WATCHED: usize = 32;
/// A table's image (RULE-41).
pub const MAX_TABLE_BYTES: usize = 32_768;
/// Image bytes in one RULES record (RULE-41): the record frame's largest
/// body less the fixed fields before them, asserted below.
pub const MAX_TABLE_FRAGMENT: usize = 8172;
/// A `PAGE_STATE` payload (RULE-47), asserted from its parts below.
pub const MAX_STATE_BYTES: usize = 1800;
/// Ticks a frame of a cycle (ANIM-14).
pub const MAX_CYCLE_PERIOD: u16 = 3600;
/// The highest frame number (ANIM-14).
pub const MAX_FRAME: u8 = 15;
/// Overdue ticks run in one wake (RULE-4).
pub const MAX_CATCH_UP_TICKS: u32 = 6;
/// Ticks a second of the presentation clock (RULE-4, ANIM-1). The engine
/// core's `PRESENTATION_TICKS_PER_SECOND` is asserted equal where both are
/// linked.
pub const TICKS_PER_SECOND: u32 = 60;

/// The limits of RULE-57's table, each by its constant's name with its
/// value, in the table's order (REQ-6608).
pub const RULE_57: [(&str, u32); 43] = [
    ("MAX_GROUP_COMMANDS", MAX_GROUP_COMMANDS as u32),
    ("MAX_GROUPS", MAX_GROUPS as u32),
    ("MAX_GROUP_DEPTH", MAX_GROUP_DEPTH as u32),
    ("MAX_RUN_COMMANDS", MAX_RUN_COMMANDS as u32),
    ("MAX_COMMAND_BYTES", MAX_COMMAND_BYTES as u32),
    ("MAX_GROUPS_BYTES", MAX_GROUPS_BYTES as u32),
    ("MAX_RULES", MAX_RULES as u32),
    ("MAX_ROWS", MAX_ROWS as u32),
    ("MAX_SEQUENCES", MAX_SEQUENCES as u32),
    ("MAX_SEQUENCE_STEPS", MAX_SEQUENCE_STEPS as u32),
    ("MAX_SEQUENCE_REPEAT", MAX_SEQUENCE_REPEAT as u32),
    ("TIMERS", TIMERS as u32),
    ("VARIABLES", VARIABLES as u32),
    ("MAX_CONDITION_TERMS", MAX_CONDITION_TERMS as u32),
    ("MAX_CONDITION_DEPTH", MAX_CONDITION_DEPTH as u32),
    ("MAX_CONDITION_NODES", MAX_CONDITION_NODES as u32),
    ("MAX_SET_MEMBERS", MAX_SET_MEMBERS as u32),
    ("MAX_AND_OR_CHILDREN", MAX_AND_OR_CHILDREN as u32),
    ("MAX_TRIGGERS_PER_RULE", MAX_TRIGGERS_PER_RULE as u32),
    ("MAX_KEYS_PER_TRIGGER", MAX_KEYS_PER_TRIGGER as u32),
    ("MAX_RULE_ROWS", MAX_RULE_ROWS as u32),
    ("MAX_COMMAND_BINDINGS", MAX_COMMAND_BINDINGS as u32),
    ("MAX_RECTS", MAX_RECTS as u32),
    ("MAX_SEQUENCE_TICKS", MAX_SEQUENCE_TICKS),
    ("MAX_TIMER_TICKS", MAX_TIMER_TICKS),
    ("MAX_START_TICKS", MAX_START_TICKS),
    ("MAX_CUE", MAX_CUE as u32),
    ("MAX_EVENTS_PER_TICK", MAX_EVENTS_PER_TICK as u32),
    ("MAX_EVENT_COMMANDS_PER_TICK", MAX_EVENT_COMMANDS_PER_TICK as u32),
    ("MAX_EVENT_REPORTS_PER_TICK", MAX_EVENT_REPORTS_PER_TICK as u32),
    ("MAX_QUEUED_EVENTS", MAX_QUEUED_EVENTS as u32),
    ("MAX_KEYS_DOWN", MAX_KEYS_DOWN as u32),
    ("MAX_POINTER_OBJECTS", MAX_POINTER_OBJECTS as u32),
    ("MAX_TABLE_VALUES", MAX_TABLE_VALUES as u32),
    ("MAX_TABLE_TARGETS", MAX_TABLE_TARGETS as u32),
    ("MAX_WATCHED", MAX_WATCHED as u32),
    ("MAX_PAINT_WATCHED", MAX_PAINT_WATCHED as u32),
    ("MAX_TABLE_BYTES", MAX_TABLE_BYTES as u32),
    ("MAX_TABLE_FRAGMENT", MAX_TABLE_FRAGMENT as u32),
    ("MAX_STATE_BYTES", MAX_STATE_BYTES as u32),
    ("MAX_CYCLE_PERIOD", MAX_CYCLE_PERIOD as u32),
    ("MAX_FRAME", MAX_FRAME as u32),
    ("MAX_CATCH_UP_TICKS", MAX_CATCH_UP_TICKS),
];

/// The value of the limit named `name` (RULE-57): what a refusal gives
/// beside the constant's name. `TICKS_PER_SECOND`, the table's last line,
/// is the clock and no limit of a table, but is answered too.
#[must_use]
pub fn value_of(name: &str) -> Option<u32> {
    if name == "TICKS_PER_SECOND" {
        return Some(TICKS_PER_SECOND);
    }
    RULE_57.iter().find(|(held, _)| *held == name).map(|(_, value)| *value)
}

/// The microseconds of [`TICKS_PER_TICK_SPAN`] ticks (RULE-4): tick `n` is
/// due `n × 50,000 ÷ 3` microseconds after the clock's start, rounded
/// down, so sixty ticks are exactly one second.
pub const TICK_SPAN_MICROSECONDS: u64 = 50_000;
/// The ticks that last [`TICK_SPAN_MICROSECONDS`] (RULE-4).
pub const TICKS_PER_TICK_SPAN: u64 = 3;
/// The microseconds of a second.
const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
// RULE-4: the span is the clock's rate written without a fraction.
const _: () =
    assert!(TICK_SPAN_MICROSECONDS * TICKS_PER_SECOND as u64 == MICROSECONDS_PER_SECOND * TICKS_PER_TICK_SPAN);

/// The fewest children of `and` or `or` (RULE-18).
pub const MIN_AND_OR_CHILDREN: usize = 2;
/// The table image's format byte (RULE-41): the second form.
pub const FORMAT: u8 = 2;
/// The first page a table may be for (RULE-41): magazine 1, page 00.
pub const FIRST_PAGE: u16 = 0x100;
/// The last page a table may be for (RULE-41): magazine 8, page FF.
pub const LAST_PAGE: u16 = 0x8FF;
/// The bits a sub-code has (EN 300 706 clause 9.3.1.2): thirteen, the two
/// of S4, the four of S3, the three of S2 and the four of S1. The engine
/// core's `SUBCODE_MASK` is asserted equal where both are linked.
pub const SUBCODE_MASK: u16 = (0x3 << 12) | (0xF << 8) | (0x7 << 4) | 0xF;
/// The last sub-code a page has (RULE-112, EN 300 706 clause 9.3.1.2):
/// one below the code with every bit set, which names no page.
pub const LAST_SUBCODE: u16 = SUBCODE_MASK - 1;
/// The first generation of a table (RULE-40); 0 is no table.
pub const FIRST_GENERATION: u16 = 1;
/// The highest teletext control a target names (RULE-12): the engine
/// core's control limit, asserted equal where both are linked.
pub const MAX_CONTROL: u16 = 32;
/// The timer identifier of the page timer (RULE-15).
pub const PAGE_TIMER: u8 = 255;
/// The button that matches any button (RULE-9).
pub const ANY_BUTTON: u8 = 255;
/// The cue number that matches any cue (RULE-109).
pub const ANY_CUE: u8 = 255;
/// The variable number that names all of them, with `clear` (RULE-99).
pub const ALL_VARIABLES: u8 = 255;
/// The sequence number that names all of them, with stop (RULE-103).
pub const ALL_SEQUENCES: u8 = 255;
/// The rule and the row of a report made by no rule or with no row
/// (RULE-46).
pub const NO_RULE: u8 = 255;
/// The highest pointer button a trigger or a value names (RULE-9): 0 left,
/// 1 middle, 2 right.
pub const MAX_BUTTON: u8 = 2;
/// The audio channels of the audio profile (REQ-AUD-010): 0 to 3.
pub const AUDIO_CHANNELS: usize = 4;
/// The greatest gain of a `CONTROL` record (RULE-106; REQ-AUD-006).
pub const MAX_GAIN: u8 = 100;
/// The longest fade of a `CONTROL` record, in milliseconds (RULE-106).
pub const MAX_FADE_MS: u16 = 10_000;
/// The greatest MIDI data byte (RULE-21; MIDI 1.0, "Channel Voice
/// Messages").
pub const MAX_MIDI_DATA: u8 = 127;
/// The bits of a MIDI data byte.
pub const MIDI_DATA_BITS: u32 = 7;
/// The greatest 14-bit pitch bend value (RULE-21): two data bytes of
/// seven bits.
pub const MAX_PITCH_BEND: u16 = ((MAX_MIDI_DATA as u16) << MIDI_DATA_BITS) | MAX_MIDI_DATA as u16;
/// The highest forced look (ANIM-6): 0 normal, 1 hover, 2 pressed, 3
/// alternative.
pub const MAX_LOOK: u8 = 3;
/// The highest layer (ANIM-12): 0 to 3.
pub const MAX_LAYER: u8 = 3;
/// The layer byte of `order` that keeps the layer in force (ANIM-6).
pub const KEEP_LAYER: u8 = 255;
/// The first family code a synthesizer value names (RULE-28): the
/// synth6581 family's codec code.
pub const FIRST_FAMILY: u8 = 12;
/// The last family code (RULE-28): synth80's.
pub const LAST_FAMILY: u8 = 18;
/// The highest variant of a family (RULE-28).
pub const MAX_VARIANT: u8 = 2;
/// The highest part, a MIDI channel less one (RULE-28).
pub const MAX_PART: u8 = 15;
/// The highest class of a synthesizer value (RULE-28).
pub const MAX_SYNTH_CLASS: u8 = 9;
/// The highest parameter number of a `field` (RULE-44).
pub const MAX_PARAMETER: u16 = 255;
/// The highest key or controller number of a synthesizer value (RULE-28).
pub const MAX_MIDI_NUMBER: u16 = 127;
/// Operations an `ANIMATE` record carries (ANIM-6).
pub const MAX_ANIMATE_OPERATIONS: usize = 64;
/// The largest body of a record in the audio profile's frame (REQ-AUD-003;
/// page-rules.md section 8). The audio crate's `MAX_FRAMED_BODY` is
/// asserted equal where both are linked.
pub const MAX_FRAMED_BODY: usize = 8184;
/// The bytes of a RULES record before its image data (RULE-41): page,
/// sub-code, generation, flags, reserved, total and offset.
pub const RULES_RECORD_FIXED: usize = 2 + 2 + 2 + 1 + 1 + 2 + 2;
/// The bytes the input protocol's stamp takes at most before a
/// `PAGE_STATE` payload (DOC-1934): two varints, of a page and a sub-code.
pub const STAMP_BYTES: usize = 4;

// RULE-41, RULE-57: a fragment's data fills the frame's largest body.
const _: () = assert!(MAX_TABLE_FRAGMENT == MAX_FRAMED_BODY - RULES_RECORD_FIXED);
// RULE-57: the per-tick budgets of commands and reports are derived.
const _: () = assert!(
    MAX_EVENT_COMMANDS_PER_TICK == MAX_EVENTS_PER_TICK * MAX_RUN_COMMANDS
        && MAX_EVENT_REPORTS_PER_TICK == 2 * MAX_EVENTS_PER_TICK
);
// RULE-47, RULE-57: the answer at its fullest fits its bound, with the
// stamp the input protocol writes before it.
const _: () = assert!(STAMP_BYTES + crate::state::MAX_PAYLOAD_BYTES <= MAX_STATE_BYTES);
// RULE-41: the image's total and a fragment's offset are 16-bit fields,
// and so is the bytes-of-groups field of the header, which counts a part
// of the image.
const _: () = assert!(MAX_TABLE_BYTES <= u16::MAX as usize);
const _: () = assert!(MAX_GROUPS_BYTES <= MAX_TABLE_BYTES);
// RULE-11: a rule's rows have a bit each in one word of the `becomes`
// bitmap, and a rule or a row is a byte in a report (RULE-46).
const _: () = assert!(MAX_ROWS <= u64::BITS as usize && MAX_RULES < NO_RULE as usize);
// RULE-106: a variable is named by the low nibble of a binding's source
// byte; RULE-15: a timer by a byte below the page timer's.
const _: () = assert!(VARIABLES == 1 << 4 && TIMERS <= PAGE_TIMER as usize);

#[cfg(test)]
mod tests {
    use super::*;

    /// RULE-57 (REQ-6608): each constant's value is the requirement's
    /// table's, line by line.
    #[test]
    fn the_constants_are_rule_57s() {
        let table: [(&str, u32); 44] = [
            ("MAX_GROUP_COMMANDS", 32),
            ("MAX_GROUPS", 64),
            ("MAX_GROUP_DEPTH", 4),
            ("MAX_RUN_COMMANDS", 128),
            ("MAX_COMMAND_BYTES", 256),
            ("MAX_GROUPS_BYTES", 16_384),
            ("MAX_RULES", 64),
            ("MAX_ROWS", 64),
            ("MAX_SEQUENCES", 8),
            ("MAX_SEQUENCE_STEPS", 64),
            ("MAX_SEQUENCE_REPEAT", 255),
            ("TIMERS", 8),
            ("VARIABLES", 16),
            ("MAX_CONDITION_TERMS", 8),
            ("MAX_CONDITION_DEPTH", 3),
            ("MAX_CONDITION_NODES", 16),
            ("MAX_SET_MEMBERS", 8),
            ("MAX_AND_OR_CHILDREN", 8),
            ("MAX_TRIGGERS_PER_RULE", 4),
            ("MAX_KEYS_PER_TRIGGER", 16),
            ("MAX_RULE_ROWS", 128),
            ("MAX_COMMAND_BINDINGS", 8),
            ("MAX_RECTS", 16),
            ("MAX_SEQUENCE_TICKS", 216_000),
            ("MAX_TIMER_TICKS", 216_000),
            ("MAX_START_TICKS", 216_000),
            ("MAX_CUE", 127),
            ("MAX_EVENTS_PER_TICK", 16),
            ("MAX_EVENT_COMMANDS_PER_TICK", 2048),
            ("MAX_EVENT_REPORTS_PER_TICK", 32),
            ("MAX_QUEUED_EVENTS", 64),
            ("MAX_KEYS_DOWN", 16),
            ("MAX_POINTER_OBJECTS", 16),
            ("MAX_TABLE_VALUES", 64),
            ("MAX_TABLE_TARGETS", 64),
            ("MAX_WATCHED", 32),
            ("MAX_PAINT_WATCHED", 32),
            ("MAX_TABLE_BYTES", 32_768),
            ("MAX_TABLE_FRAGMENT", 8172),
            ("MAX_STATE_BYTES", 1800),
            ("MAX_CYCLE_PERIOD", 3600),
            ("MAX_FRAME", 15),
            ("MAX_CATCH_UP_TICKS", 6),
            ("TICKS_PER_SECOND", 60),
        ];
        for (name, value) in table {
            assert_eq!(value_of(name), Some(value), "{name}");
        }
        assert_eq!(table.len(), RULE_57.len() + 1, "every line of RULE-57 is listed once");
        assert_eq!(value_of("MAX_ACTIONS_PER_TICK"), None, "the first form's work cap is withdrawn");
        for withdrawn in ["MAX_ARMED_RUNS", "MAX_ARM_TICKS", "MAX_HOLD_TICKS", "MAX_DOUBLE_TICKS", "ROW_PARAMETERS"] {
            assert_eq!(value_of(withdrawn), None, "{withdrawn} is withdrawn (DEC-6635)");
        }
    }

    /// RULE-57 (REQ-6608): the two derived figures follow from their
    /// parts: a fragment from the record frame's largest body, and the
    /// state answer from its fields at their fullest.
    #[test]
    fn the_derived_limits_follow_from_their_parts() {
        assert_eq!(MAX_TABLE_FRAGMENT, MAX_FRAMED_BODY - RULES_RECORD_FIXED);
        let targets = MAX_TABLE_TARGETS * crate::state::TARGET_BYTES;
        let values = MAX_TABLE_VALUES * (crate::table::VALUE_BYTES + 4);
        let sequences = MAX_SEQUENCES * crate::state::SEQUENCE_BYTES;
        let fixed = 4 + 2 + 4 + VARIABLES * 4 + TIMERS * crate::state::TIMER_BYTES;
        let fullest = fixed + 1 + sequences + 1 + targets + 1 + values;
        assert_eq!(crate::state::MAX_PAYLOAD_BYTES, fullest);
        assert!(STAMP_BYTES + fullest <= MAX_STATE_BYTES);
    }
}
