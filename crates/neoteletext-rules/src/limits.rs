//! Every bound of the page rule table (RULE-57), each defined once here.
//! They are maximums: a table over any of them is refused whole (RULE-70),
//! and no receiver holds a lower or a higher one.

/// Ticks a second of the presentation clock (RULE-4, ANIM-1). The engine
/// core's `PRESENTATION_TICKS_PER_SECOND` is asserted equal where both are
/// linked.
pub const TICKS_PER_SECOND: u32 = 60;
/// The microseconds of [`TICKS_PER_TICK_SPAN`] ticks (RULE-4): tick `n` is
/// due `n × 50,000 ÷ 3` microseconds after the clock's start, rounded
/// down, so sixty ticks are exactly one second.
pub const TICK_SPAN_MICROSECONDS: u64 = 50_000;
/// The ticks that last [`TICK_SPAN_MICROSECONDS`] (RULE-4).
pub const TICKS_PER_TICK_SPAN: u64 = 3;
/// Rules a table (RULE-3; DEC-6522 item 10).
pub const MAX_RULES: usize = 64;
/// Actions a rule (RULE-3).
pub const MAX_ACTIONS: usize = 16;
/// Timers a table, beside the page timer (RULE-15).
pub const MAX_TIMERS: usize = 8;
/// Variables a table (RULE-14).
pub const MAX_VARIABLES: usize = 32;
/// Rectangles a table (RULE-13).
pub const MAX_RECTS: usize = 16;
/// Sub-pages with rules switched off (RULE-17).
pub const MAX_SUBPAGE_MASKS: usize = 16;
/// Keys in a key set (RULE-10).
pub const MAX_KEYS_PER_TRIGGER: usize = 16;
/// Terms a condition (RULE-18).
pub const MAX_CONDITION_TERMS: usize = 8;
/// Nodes a condition (RULE-18).
pub const MAX_CONDITION_NODES: usize = 16;
/// Operator nesting of a condition (RULE-18): a term directly under three
/// operators is allowed, a fourth operator level is refused.
pub const MAX_CONDITION_DEPTH: usize = 3;
/// Constants in an `in_set` (RULE-18).
pub const MAX_SET_MEMBERS: usize = 8;
/// Children of `and` or `or` (RULE-18).
pub const MAX_AND_OR_CHILDREN: usize = 8;
/// The fewest children of `and` or `or` (RULE-18).
pub const MIN_AND_OR_CHILDREN: usize = 2;
/// Distinct values a table reads (RULE-57).
pub const MAX_TABLE_VALUES: usize = 64;
/// Distinct controls and objects a table names (RULE-57).
pub const MAX_TABLE_TARGETS: usize = 64;
/// Distinct metered values a table reads (RULE-33).
pub const MAX_WATCHED: usize = 32;
/// The bytes of an encoded table: one record body (RULE-41).
pub const MAX_TABLE_BYTES: usize = 8184;
/// `ticks` of `every` and `after`: one hour (RULE-9).
pub const MAX_TIMER_TICKS: u32 = 216_000;
/// Ticks a frame of a cycle: one minute (RULE-20, ANIM-6).
pub const MAX_CYCLE_PERIOD: u16 = 3600;
/// The highest frame number (ANIM-14's `MAX_OBJECT_FRAMES` is 16).
pub const MAX_FRAME: u8 = 15;
/// Input events handled a tick (RULE-5).
pub const MAX_EVENTS_PER_TICK: usize = 16;
/// Input events waiting (RULE-5).
pub const MAX_QUEUED_EVENTS: usize = 64;
/// Actions run a tick (RULE-6).
pub const MAX_ACTIONS_PER_TICK: usize = 256;
/// Keys tracked as down (RULE-8).
pub const MAX_KEYS_DOWN: usize = 16;
/// Overdue ticks run in one wake (RULE-4).
pub const MAX_CATCH_UP_TICKS: u32 = 6;
/// The bytes of a `PAGE_STATE` payload (RULE-47).
pub const MAX_STATE_BYTES: usize = 1800;

/// The table's format byte (RULE-41).
pub const FORMAT: u8 = 1;
/// The first page a table may be for (RULE-41): magazine 1, page 00.
pub const FIRST_PAGE: u16 = 0x100;
/// The last page a table may be for (RULE-41): magazine 8, page FF.
pub const LAST_PAGE: u16 = 0x8FF;
/// The bits a sub-code has (EN 300 706 clause 9.3.1.2): thirteen, the two
/// of S4, the four of S3, the three of S2 and the four of S1. The engine
/// core's `SUBCODE_MASK` is asserted equal where both are linked.
pub const SUBCODE_MASK: u16 = (0x3 << 12) | (0xF << 8) | (0x7 << 4) | 0xF;
/// The sub-code that names whichever sub-page is shown (ANIM-6; EN 300 706
/// clause 9.6.1): every bit of a sub-code.
pub const SUBCODE_ANY: u16 = SUBCODE_MASK;
/// The last sub-code a sub-page has (RULE-17, EN 300 706 clause 9.3.1.2).
pub const LAST_SUBCODE: u16 = SUBCODE_ANY - 1;
/// The highest teletext control a target names (RULE-12): the engine
/// core's control limit, asserted equal where both are linked.
pub const MAX_CONTROL: u16 = 32;
/// The timer identifier of the page timer (RULE-15).
pub const PAGE_TIMER: u8 = 255;
/// The button that matches any button (RULE-9).
pub const ANY_BUTTON: u8 = 255;
/// The highest pointer button a trigger or a value names (RULE-9): 0 left,
/// 1 middle, 2 right.
pub const MAX_BUTTON: u8 = 2;
/// The audio channels of the audio profile (RULE-20; REQ-AUD-010): 0 to 3.
pub const AUDIO_CHANNELS: usize = 4;
/// The greatest gain of `play` and `gain` (RULE-20).
pub const MAX_GAIN: u8 = 100;
/// The longest fade of `play`, `stop` and `gain`, in milliseconds
/// (RULE-20).
pub const MAX_FADE_MS: u16 = 10_000;
/// The greatest MIDI data byte (RULE-21; MIDI 1.0, "Channel Voice
/// Messages").
pub const MAX_MIDI_DATA: u8 = 127;
/// The highest forced look (RULE-20, ANIM-6): 0 normal, 1 hover, 2
/// pressed, 3 alternative.
pub const MAX_LOOK: u8 = 3;
/// The highest layer (ANIM-12): 0 to 3.
pub const MAX_LAYER: u8 = 3;
/// The layer byte of `order` that keeps the layer in force (RULE-20).
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
/// The objects the pointer is followed inside at once (RULE-12): a ReGIS
/// drawing has at most sixteen ancestors, and no page needs more sprites
/// or objects stacked on one pixel. Those beyond it are not entered. The
/// document names no figure; this one is this crate's.
pub const MAX_POINTER_OBJECTS: usize = 16;

// RULE-47: the answer at its fullest fits its bound, with the stamp's four
// bytes the input protocol adds before it.
const _: () = assert!(
    4 + 4
        + 2
        + 4
        + 8
        + 1
        + MAX_VARIABLES * 4
        + 1
        + MAX_TIMERS * 5
        + 1
        + MAX_TABLE_TARGETS * crate::state::TARGET_BYTES
        + 1
        + MAX_TABLE_VALUES * (crate::table::VALUE_BYTES + 4)
        <= MAX_STATE_BYTES
);
// RULE-16: a rule mask has a bit for every rule.
const _: () = assert!(MAX_RULES == u64::BITS as usize);
