//! The evaluator (RULE-4 to RULE-8, RULE-11, RULE-14, RULE-15, RULE-96,
//! RULE-102, RULE-103, RULE-105 to RULE-109, RULE-116 to RULE-118): a
//! page's rule state and its tick, in the fixed order of RULE-5.
//!
//! The receiver gives it the viewer's input events ([`Evaluator::push`] or
//! [`Evaluator::push_now`]), the cues heard ([`Evaluator::push_cue`]), the
//! commands the server sent directly ([`Evaluator::push_command`]), the
//! metered values ([`Evaluator::set_metered`]) and, each tick, a
//! [`World`]: what the evaluator reads of the page beyond its own state,
//! and where every command it does not carry out itself goes, as its
//! record type and body with its amounts resolved (RULE-56).
//!
//! Nothing a command does is seen in the tick that runs it (RULE-6): the
//! world is read once for a tick, when its snapshot is taken, and every
//! condition reads that snapshot. Commands are given to the world in the
//! order they run, each at its step of the tick.
//!
//! An input event given by [`Evaluator::push_now`] is decided at once
//! (RULE-96, DEC-6580): its rules are matched and their conditions read
//! the coming tick's snapshot, and the sound commands of the groups it
//! fires are given to the world before the call returns. Everything else
//! those groups do happens at step 3 of the event's tick, so both ways of
//! giving an event have the same firings, reports, commands, counters and
//! state at every tick.

mod input;

use crate::bounded::Bounded;
use crate::command::{
    self, Binding, BodyCheck, Command, Operand, RunMode, Source, TYPE_RUN, TYPE_VARIABLE, VariableOperation,
};
use crate::limits::{
    ALL_SEQUENCES, ALL_VARIABLES, ANY_BUTTON, ANY_CUE, AUDIO_CHANNELS, MAX_ARM_TICKS, MAX_ARMED_RUNS, MAX_BUTTON,
    MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES, MAX_CONDITION_DEPTH, MAX_CONDITION_NODES, MAX_CUES_PER_TICK,
    MAX_EVENTS_PER_TICK, MAX_FRAME, MAX_GROUP_DEPTH, MAX_LAYER, MAX_LOOK, MAX_POINTER_OBJECTS, MAX_QUEUED_COMMANDS,
    MAX_QUEUED_EVENTS, MAX_RECTS, MAX_RULE_ROWS, MAX_RULES, MAX_SEQUENCES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES,
    MAX_WATCHED, NO_RULE, PAGE_TIMER, TIMERS, VARIABLES,
};
use crate::state::{PageState, SequenceState, TableState, TargetReport, TimerState};
use crate::table::{
    CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_VOICES, FIRED_CUE, FIRED_KEY, FIRED_P0, FIRED_ROW, FIRED_TARGET,
    Fault, Index, NO_BINDING, NO_ROW, Node, Rect, Row, SEQUENCE_PASSES, SEQUENCE_POSITION, SEQUENCE_RUNNING,
    SOUND_GAIN, SOUND_HANDLE, SOUND_LEVEL, SOUND_LOCAL, SOUND_PLAYING, SPACE_FIRED, SPACE_SEQUENCE, SPACE_SOUND,
    SPACE_SYNTH, SPACE_TARGET, SPACE_TIMER, SPACE_VARIABLE, TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME, TARGET_LAYER,
    TARGET_LOOK, TARGET_ORDER, TARGET_SHOWN, TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_RUNNING, Table, Target,
    TargetKind, Trigger, Value,
};
use input::{InputState, Inside, Pressed, Seen, Transition, keys_match};

pub use input::{Input, InputEvent, KeyAction, KeyIdentity, PointerAction};

/// A rectangle of the picture's pixels (ANIM-13): from `left` and `top`
/// up to, and not including, `right` and `bottom`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bounds {
    /// The first column.
    pub left: i32,
    /// The first row.
    pub top: i32,
    /// The column after the last.
    pub right: i32,
    /// The row after the last.
    pub bottom: i32,
}

impl Bounds {
    /// The rectangle a table declares (RULE-13).
    #[must_use]
    pub fn of_rect(rect: &Rect) -> Self {
        let (left, top) = (i32::from(rect.x), i32::from(rect.y));
        Self {
            left,
            top,
            right: left.saturating_add(i32::from(rect.width)),
            bottom: top.saturating_add(i32::from(rect.height)),
        }
    }

    /// Whether it holds no pixel.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.left >= self.right || self.top >= self.bottom
    }

    /// Whether the two share at least one pixel (RULE-19): rectangles that
    /// touch at an edge do not.
    #[must_use]
    pub const fn overlaps(&self, other: &Self) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }

    /// Whether this lies wholly within `outer` (RULE-19).
    #[must_use]
    pub const fn within(&self, outer: &Self) -> bool {
        !self.is_empty()
            && self.left >= outer.left
            && self.right <= outer.right
            && self.top >= outer.top
            && self.bottom <= outer.bottom
    }

    /// The part of it inside a picture of `width` by `height` pixels
    /// (RULE-13), or `None` when no pixel of it is.
    #[must_use]
    pub fn cut_to(&self, width: u16, height: u16) -> Option<Self> {
        let cut = Self {
            left: self.left.max(0),
            top: self.top.max(0),
            right: self.right.min(i32::from(width)),
            bottom: self.bottom.min(i32::from(height)),
        };
        if cut.is_empty() { None } else { Some(cut) }
    }
}

/// A control's or an object's state as its renderer and its animation
/// state give it when a tick's snapshot is taken (RULE-26, ANIM-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TargetState {
    /// Whether it is shown.
    pub shown: bool,
    /// Whether it is cycling.
    pub cycling: bool,
    /// The frame in force, 0 to 15.
    pub frame: u8,
    /// The look in force: 0 normal, 1 hover, 2 pressed, 3 alternative.
    pub look: u8,
    /// Its layer, 0 to 3.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
    /// Its position in its renderer's units (ANIM-6); a control's first
    /// column.
    pub x: i16,
    /// See `x`; a control's row.
    pub y: i16,
    /// Its bounding rectangle in the picture's pixels (ANIM-13), or `None`
    /// while it is not shown.
    pub bounds: Option<Bounds>,
}

/// The sound on an audio channel when a tick's snapshot is taken
/// (RULE-27).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SoundState {
    /// Whether any sound plays on the channel in this view.
    pub playing: bool,
    /// Whether the sound playing is a local sound (RULE-60).
    pub local: bool,
    /// The handle of the sound playing.
    pub handle: u8,
    /// The channel's gain as last commanded, 0 to 100.
    pub gain: u8,
    /// The codec code of the sound playing, which for a synthesizer is its
    /// family's code (RULE-28).
    pub family: u8,
    /// Its variant (RULE-28).
    pub variant: u8,
}

/// What a report says (RULE-22, RULE-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reported {
    /// The rule or the run fired, with no event.
    Fired,
    /// The rule fired on the input event its giver numbered `sequence`.
    Input {
        /// The event's sequence number, as it was given.
        sequence: u32,
    },
    /// One value: a variable as the working state holds it, any other as
    /// the snapshot holds it.
    Value {
        /// The value.
        value: Value,
        /// Its reading.
        reading: i32,
    },
}

/// One `REPORT` applied (RULE-22): what the receiver reports to the
/// page's channel as a `RULE_EVENT` (RULE-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// The rule whose firing made the run, 0 to 63; 255 when the run was
    /// not made by a rule.
    pub rule: u8,
    /// The place of the firing row within the rule's rows; 255 when none.
    pub row: u8,
    /// The page tick of the firing.
    pub tick: u32,
    /// The control, object or rectangle the pointer was inside, for a
    /// report of an input event whose rule's pointer trigger named one;
    /// else [`Target::NONE`].
    pub target: Target,
    /// What is reported.
    pub what: Reported,
}

/// What the evaluator reads of the page, and where its outputs go
/// (RULE-56).
///
/// The reading methods are called when a tick's snapshot is taken (as the
/// tick begins, or when an event of its interval is decided on arrival,
/// RULE-96) and while input events are followed. [`World::command`] and
/// [`World::report`] are called as the commands run, in order, at their
/// step of the tick; [`World::command`] also for a sound command on its
/// event's arrival. What a command changes is first read at the next
/// snapshot (RULE-6).
pub trait World {
    /// The picture's width and height in pixels (RULE-13): 480 by 500 for
    /// a teletext page, 800 by 480 for ReGIS, 640 by 480 for NAPLPS.
    fn picture(&self) -> (u16, u16);
    /// The state of one control or object of the page, or `None` when it
    /// does not exist there (RULE-12).
    fn target(&self, target: Target) -> Option<TargetState>;
    /// The objects a pixel is inside (RULE-12), written to the front of
    /// `objects`; the count written.
    fn objects_at(&self, x: u16, y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize;
    /// The sound on an audio channel, 0 to 3 (RULE-27).
    fn sound(&self, channel: u8) -> SoundState;
    /// A tracked value of the synthesizer sound on an audio channel
    /// (RULE-28, RULE-30): classes 0 to 4, 8 and 9, read from the family's
    /// tracker. It is asked only while a sound of the family and variant
    /// the table expects plays there.
    fn synth(&self, channel: u8, part: u8, class: u8, index: u16) -> i32;
    /// A command of a run that the evaluator does not carry out itself
    /// (RULE-56): a `CONTROL`, `LOCAL_MIDI`, `ANIMATE`, `SPRITES` or
    /// `NAPLPS_OBJECTS` record's type and body, amounts resolved, to be
    /// applied by the code that applies that record from the server, in
    /// page tick `tick`. False when it did nothing useful, which is
    /// counted as skipped (RULE-118).
    fn command(&mut self, kind: u8, body: &[u8], tick: u32) -> bool;
    /// A report for the page's channel (RULE-22).
    fn report(&mut self, report: Report);
    /// Advance each cycling object whose frame is due at page tick `tick`
    /// (RULE-5 step 11, ANIM-14).
    fn advance_cycles(&mut self, tick: u32);
    /// The answer to a state query (RULE-47), at the end of the tick after
    /// [`Evaluator::query`].
    fn page_state(&mut self, state: &PageState);
}

/// The receiver's own counts for the open page (RULE-72). Each stops at
/// its greatest value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counters {
    /// Ticks evaluated.
    pub ticks: u32,
    /// Rules fired.
    pub fired: u32,
    /// Groups run, those a group runs among them.
    pub groups: u32,
    /// Commands applied.
    pub commands: u32,
    /// Commands, and operations of an `ANIMATE`, that did nothing for a
    /// missing target, sound, group or sequence (RULE-118).
    pub skipped: u32,
    /// Input events dropped from a full queue (RULE-5), and firings of
    /// one event beyond `MAX_RULE_ROWS`.
    pub events_dropped: u32,
    /// Runs armed (RULE-102).
    pub armed: u32,
    /// Runs armed for a tick that had passed, and run at the next.
    pub armed_late: u32,
    /// Runs not armed because `MAX_ARMED_RUNS` waited.
    pub not_armed: u32,
    /// Commands the server sent that were dropped at a full queue.
    pub commands_dropped: u32,
    /// Cues acted on (RULE-109).
    pub cues: u32,
    /// Cues dropped beyond `MAX_CUES_PER_TICK`.
    pub cues_dropped: u32,
    /// Reports asked for.
    pub reports: u32,
}

/// One sequence's running state (RULE-103).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Playing {
    running: bool,
    /// The tick it runs since: the tick after its start was applied.
    since: u32,
    position: u32,
    passes: u32,
    /// The row parameters in force when it was started.
    parameters: Row,
}

const STOPPED: Playing = Playing { running: false, since: 0, position: 0, passes: 0, parameters: NO_ROW };

/// The rule state commands write (RULE-7): two copies are kept, the
/// snapshot and the working state.
#[derive(Clone, Copy)]
struct RuleState {
    variables: [i32; VARIABLES],
    /// Each timer's start tick while it runs.
    timers: [Option<u32>; TIMERS],
    sequences: [Playing; MAX_SEQUENCES],
}

/// An armed run (RULE-102).
#[derive(Clone, Copy)]
struct Armed {
    due: u32,
    group: u8,
    parameters: Row,
}

/// What a run is made by, and the values of the firing (RULE-116).
#[derive(Clone, Copy)]
struct Fired {
    /// The rule, or [`NO_RULE`].
    rule: u8,
    /// The place of the firing row, or [`NO_RULE`].
    row: u8,
    key: u32,
    target: Target,
    cue: u8,
    parameters: Row,
    /// The input event's sequence number, in a run an input's rule made.
    sequence: Option<u32>,
}

impl Fired {
    /// A run made by no rule, with `parameters` held for it.
    const fn held(parameters: Row) -> Self {
        Self { rule: NO_RULE, row: NO_RULE, key: 0, target: Target::NONE, cue: 0, parameters, sequence: None }
    }

    /// A value of the firing (RULE-116).
    fn read(&self, property: u16) -> i32 {
        match property {
            FIRED_KEY => i32::try_from(self.key).unwrap_or(i32::MAX),
            FIRED_TARGET => i32::from(self.target.number),
            FIRED_ROW => i32::from(if self.row == NO_RULE { 0 } else { self.row }),
            FIRED_CUE => i32::from(self.cue),
            _ => {
                let parameter = usize::from(property.saturating_sub(FIRED_P0));
                self.parameters.get(parameter).copied().unwrap_or(0)
            }
        }
    }
}

/// One firing of an input event, decided when the event was (RULE-96).
#[derive(Clone, Copy)]
struct Firing {
    rule: u8,
    row: u8,
    target: Target,
}

const NO_FIRING: Firing = Firing { rule: 0, row: 0, target: Target::NONE };

/// An input event decided for the coming tick (RULE-96).
#[derive(Clone, Copy)]
struct Decided {
    event: InputEvent,
    /// What the input state's values read just after the event (RULE-8).
    input: Seen,
    /// Its firings' place in the tick's list.
    first: usize,
    count: usize,
    /// Whether its sound commands were delivered on arrival.
    delivered: bool,
}

/// Which commands a walk of a group applies (RULE-96).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    /// On an event's arrival: the sound commands are delivered and the
    /// `VARIABLE` commands applied to the arrival copy; nothing else.
    Arrival,
    /// At the tick, for an event whose sound commands left on arrival:
    /// everything but them.
    Rest,
    /// At the tick: everything.
    Whole,
}

/// What a firing with no input event is of: the code of the key that is
/// held, the target a held button's press was inside, and the cue heard.
type Occurred = (u32, Target, u8);

/// Says whether a trigger occurs for one row at the step being run.
type Occurs<'a> = &'a dyn Fn(&Trigger, &Row) -> Option<Occurred>;

/// A place in a list of 64 or fewer, kept in a byte.
const NO_SLOT: u8 = u8::MAX;
const _: () = assert!(MAX_TABLE_VALUES < NO_SLOT as usize);

/// The most firings the input events of one tick have together.
const MAX_TICK_FIRINGS: usize = MAX_EVENTS_PER_TICK * MAX_RULE_ROWS;

/// A page's rule state and its tick (RULE-5, RULE-7).
#[derive(Clone)]
pub struct Evaluator {
    table: Table,
    index: Index,
    generation: u16,
    /// Where each condition node's value is in the index, by rule and
    /// node; [`NO_SLOT`] for a node that reads none there.
    node_slots: [[u8; MAX_CONDITION_NODES]; MAX_RULES],
    /// The page tick of the next evaluation.
    tick: u32,
    closed: bool,
    /// Whether the snapshot of tick `tick` is taken (RULE-96): by an
    /// event of its interval decided on arrival.
    opened: bool,
    snapshot: RuleState,
    working: RuleState,
    /// The arrival copy of the variables (RULE-96): the working variables
    /// as the last tick left them, then as the walks on arrival changed
    /// them.
    arrival: [i32; VARIABLES],
    armed: Bounded<Armed, MAX_ARMED_RUNS>,
    /// The armed runs waiting when the snapshot was taken.
    armed_then: u8,
    /// Each `becomes` rule's result at its previous evaluation, a bit for
    /// each of its rows (RULE-11).
    becomes: [u64; MAX_RULES],
    input: InputState,
    /// The input state a command's value reads (RULE-8): as it was just
    /// after the event whose rule runs the command.
    view: Seen,
    queue: [(InputEvent, bool); MAX_QUEUED_EVENTS],
    queue_head: usize,
    queue_length: usize,
    decided: [Decided; MAX_EVENTS_PER_TICK],
    decided_count: usize,
    firings: [Firing; MAX_TICK_FIRINGS],
    firing_count: usize,
    commands: Bounded<(u8, [u8; command::MAX_BODY_BYTES], u8), MAX_QUEUED_COMMANDS>,
    cues: Bounded<(u8, u8), MAX_CUES_PER_TICK>,
    metered: [u8; MAX_WATCHED],
    /// The snapshot's reading of each value of the index that is not
    /// input state.
    readings: [i32; MAX_TABLE_VALUES],
    /// The snapshot's state of each target of the index.
    targets: [Option<TargetState>; MAX_TABLE_TARGETS],
    sounds: [SoundState; AUDIO_CHANNELS],
    picture: (u16, u16),
    counters: Counters,
    query: Option<u32>,
}

impl Evaluator {
    /// An evaluator for `table`, as the page opens (RULE-2): its sixteen
    /// variables 0, its timers stopped, nothing running or armed, page
    /// tick 0 next. `generation` is the table's, which a state query's
    /// answer names. The table is checked whole first
    /// ([`Table::check`]), so that one built by hand is held to what
    /// [`crate::decode`] holds an image to.
    ///
    /// # Errors
    /// What [`Table::check`] refuses. A decoded table is never refused.
    pub fn new(table: &Table, generation: u16, body_check: BodyCheck) -> Result<Self, Fault> {
        table.check(body_check)?;
        let index = Index::of(table)?;
        let mut node_slots = [[NO_SLOT; MAX_CONDITION_NODES]; MAX_RULES];
        for (rule, nodes) in table.rules.iter().zip(node_slots.iter_mut()) {
            for (node, slot) in rule.condition.iter().zip(nodes.iter_mut()) {
                let place = node.value().and_then(|value| index.value_slot(&value));
                *slot = place.and_then(|place| u8::try_from(place).ok()).unwrap_or(NO_SLOT);
            }
        }
        let state =
            RuleState { variables: [0; VARIABLES], timers: [None; TIMERS], sequences: [STOPPED; MAX_SEQUENCES] };
        let no_event = InputEvent { sequence: 0, input: Input::Text };
        Ok(Self {
            table: table.clone(),
            index,
            generation,
            node_slots,
            tick: 0,
            closed: false,
            opened: false,
            snapshot: state,
            working: state,
            arrival: [0; VARIABLES],
            armed: Bounded::new(Armed { due: 0, group: 0, parameters: NO_ROW }),
            armed_then: 0,
            becomes: [0; MAX_RULES],
            input: InputState::new(),
            view: Seen::new(),
            queue: [(no_event, false); MAX_QUEUED_EVENTS],
            queue_head: 0,
            queue_length: 0,
            decided: [Decided { event: no_event, input: Seen::new(), first: 0, count: 0, delivered: false };
                MAX_EVENTS_PER_TICK],
            decided_count: 0,
            firings: [NO_FIRING; MAX_TICK_FIRINGS],
            firing_count: 0,
            commands: Bounded::new((0, [0; command::MAX_BODY_BYTES], 0)),
            cues: Bounded::new((0, 0)),
            metered: [0; MAX_WATCHED],
            readings: [0; MAX_TABLE_VALUES],
            targets: [None; MAX_TABLE_TARGETS],
            sounds: [SoundState::default(); AUDIO_CHANNELS],
            picture: (0, 0),
            counters: Counters::default(),
            query: None,
        })
    }

    /// The table evaluated.
    #[must_use]
    pub const fn table(&self) -> &Table {
        &self.table
    }

    /// The table's generation.
    #[must_use]
    pub const fn generation(&self) -> u16 {
        self.generation
    }

    /// What the table reads and names: its watch list (RULE-33 item 1),
    /// its interest mask (RULE-52 item 1), and the targets and values a
    /// state query reports.
    #[must_use]
    pub const fn index(&self) -> &Index {
        &self.index
    }

    /// The page tick of the next evaluation: 0 until the first.
    #[must_use]
    pub const fn page_tick(&self) -> u32 {
        self.tick
    }

    /// The receiver's counts (RULE-72).
    #[must_use]
    pub const fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The sixteen variables as the last completed tick left them
    /// (RULE-72).
    #[must_use]
    pub const fn variables(&self) -> &[i32; VARIABLES] {
        &self.snapshot.variables
    }

    /// Whether the final tick has run ([`Evaluator::close`]).
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// How many events wait: those not yet decided and those decided for
    /// the coming tick (RULE-96).
    fn waiting(&self) -> usize {
        self.queue_length.saturating_add(self.decided_count)
    }

    /// Puts an event at the queue's end; false when the queue is full and
    /// the event was dropped, which is counted (RULE-5 step 3).
    fn enqueue(&mut self, event: InputEvent, now: bool) -> bool {
        let place = self.queue_head.saturating_add(self.queue_length).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        let slot = if self.waiting() < MAX_QUEUED_EVENTS { self.queue.get_mut(place) } else { None };
        let Some(slot) = slot else {
            self.counters.events_dropped = self.counters.events_dropped.saturating_add(1);
            return false;
        };
        *slot = (event, now);
        self.queue_length = self.queue_length.saturating_add(1);
        true
    }

    /// Gives the evaluator an input event (RULE-5 step 3), to be decided
    /// at its tick (RULE-96, the first way). It waits in a queue of at
    /// most `MAX_QUEUED_EVENTS`; false when the queue is full and the
    /// event was dropped, which is counted.
    pub fn push(&mut self, event: InputEvent) -> bool {
        self.enqueue(event, false)
    }

    /// Gives the evaluator an input event the moment it arrives (RULE-96,
    /// the second way; DEC-6580 item 1): when fewer than
    /// `MAX_EVENTS_PER_TICK` events already belong to the coming tick, it
    /// is stamped with that tick and decided at once, against the tick's
    /// one snapshot (taken now if no earlier arrival of the interval took
    /// it), and the world is given at once, with their amounts resolved,
    /// the sound commands of the groups it fires. Otherwise it waits, and
    /// is decided as soon as the tick before its own has completed; so
    /// does an event that arrives before page tick 0 has been evaluated,
    /// which belongs to page tick 1 (RULE-5 step 2). An event given
    /// earlier by [`Evaluator::push`] and still waiting is decided first,
    /// in arrival order. False when the event was dropped from a full
    /// queue, which is counted.
    pub fn push_now(&mut self, event: InputEvent, world: &mut dyn World) -> bool {
        if self.closed {
            return self.push(event);
        }
        let taken = self.enqueue(event, true);
        self.decide_waiting(world);
        taken
    }

    /// Gives the evaluator a cue the sound helper reported heard
    /// (RULE-109): step 9 of the next tick fires its `cue` rules. False
    /// when `MAX_CUES_PER_TICK` already wait and the cue was dropped,
    /// which is counted.
    pub fn push_cue(&mut self, channel: u8, number: u8) -> bool {
        if self.cues.push((channel, number)).is_ok() {
            return true;
        }
        self.counters.cues_dropped = self.counters.cues_dropped.saturating_add(1);
        false
    }

    /// Gives the evaluator a command the server sent directly for this
    /// page (RULE-114): a `VARIABLE`, `TIMER`, `REPORT`, `RUN` or
    /// `SEQUENCE` record's type and body, applied at step 5 of the next
    /// tick. False, and counted, when the body is not one its reader
    /// accepts or names another page (skipped), or when
    /// `MAX_QUEUED_COMMANDS` already wait (dropped).
    pub fn push_command(&mut self, kind: u8, body: &[u8]) -> bool {
        let here = (self.table.page, self.table.sub);
        let ours = matches!(command::decode(kind, body), Ok((page, sub, _)) if (page, sub) == here);
        if !ours || !(TYPE_VARIABLE..=command::TYPE_SEQUENCE).contains(&kind) {
            self.skip(1);
            return false;
        }
        let mut held = [0_u8; command::MAX_BODY_BYTES];
        for (to, from) in held.iter_mut().zip(body) {
            *to = *from;
        }
        let length = u8::try_from(body.len()).unwrap_or(0);
        if self.commands.push((kind, held, length)).is_ok() {
            return true;
        }
        self.counters.commands_dropped = self.counters.commands_dropped.saturating_add(1);
        false
    }

    /// Gives the newest metered report (RULE-33 item 5): one byte for each
    /// slot of the watch list, in its order. A slot not given reads 0. The
    /// next snapshot takes it.
    pub fn set_metered(&mut self, values: &[u8]) {
        let mut given = values.iter().copied();
        for slot in &mut self.metered {
            *slot = given.next().unwrap_or(0);
        }
    }

    /// Asks for the page's state (RULE-47): [`World::page_state`] is given
    /// it once, at the end of the next tick. A second query before then
    /// replaces the first; the receiver holds the rate of queries.
    pub fn query(&mut self, identifier: u32) {
        self.query = Some(identifier);
    }

    /// Decides the waiting events that belong to the coming tick, when one
    /// of them was given by [`Evaluator::push_now`] (RULE-96): in arrival
    /// order, up to the last such, while fewer than `MAX_EVENTS_PER_TICK`
    /// are decided; and delivers their sound commands.
    fn decide_waiting(&mut self, world: &mut dyn World) {
        // RULE-5 step 2: no input event belongs to page tick 0.
        if self.closed || self.tick == 0 {
            return;
        }
        loop {
            let asked = (0..self.queue_length).any(|offset| {
                let place = self.queue_head.saturating_add(offset).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
                self.queue.get(place).is_some_and(|(_, now)| *now)
            });
            if !asked || self.decided_count >= MAX_EVENTS_PER_TICK {
                return;
            }
            self.open(world);
            let Some(event) = self.next_event() else { return };
            let place = self.decide(&event, world, true);
            self.walk_arrival(place, world);
        }
    }

    /// Takes the event that has waited longest.
    fn next_event(&mut self) -> Option<InputEvent> {
        if self.queue_length == 0 {
            return None;
        }
        let (event, _) = self.queue.get(self.queue_head).copied()?;
        self.queue_head = self.queue_head.saturating_add(1).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        self.queue_length = self.queue_length.saturating_sub(1);
        Some(event)
    }

    /// Takes the snapshot of the tick to be evaluated, unless an event
    /// decided on arrival already made it be taken (RULE-96).
    fn open(&mut self, world: &dyn World) {
        if !self.opened {
            self.take_snapshot(world);
            self.opened = true;
        }
    }

    /// Evaluates one tick (RULE-5), in its fixed order of twelve steps.
    /// The events decided on arrival since the last tick are this tick's
    /// first events and are not decided again (RULE-96). After
    /// [`Evaluator::close`] it does nothing.
    pub fn tick(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        // Step 1.
        let early = self.opened;
        self.open(world);
        if self.tick == 0 {
            // Step 2.
            self.run_simple(&Trigger::Open, world);
        } else {
            // Step 3.
            self.run_events(world);
        }
        if early {
            // RULE-7: a snapshot an arrival took is a tick old for the
            // meters; the steps after the input events read the newest.
            self.refresh_metered();
        }
        self.view = self.input.seen;
        self.run_holds(world);
        self.run_server_commands(world);
        self.run_armed(world);
        self.run_timers(world);
        self.run_sequences(world);
        self.run_cues(world);
        self.run_becomes(world);
        // Step 11.
        world.advance_cycles(self.tick);
        self.apply(world);
        self.counters.ticks = self.counters.ticks.saturating_add(1);
        self.tick = self.tick.saturating_add(1);
        // RULE-96: an event that waited is decided as soon as the tick
        // before its own has completed.
        self.decide_waiting(world);
    }

    /// Evaluates the final tick, when the page is closed while its
    /// connection stands (RULE-5, RULE-111): the snapshot, each `close`
    /// rule whose condition holds, and apply. The receiver first gives a
    /// release for every key and button still down and runs a tick for
    /// them (RULE-8). When events were decided on arrival and their tick
    /// has not run, it is run first: their sound has left, so what their
    /// groups wrote and reported must too (RULE-96). On a disconnect the
    /// receiver drops the evaluator and calls nothing.
    pub fn close(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        while self.decided_count > 0 {
            self.tick(world);
        }
        self.open(world);
        self.view = self.input.seen;
        self.run_simple(&Trigger::Close, world);
        self.apply(world);
        self.closed = true;
    }

    /// The rectangles of the picture inside which a `wheel` rule applies
    /// (RULE-51 item 4), whatever its condition, as the world holds them
    /// now, written to the front of `out`; the count written. "Any
    /// control" and "any object" give the whole picture.
    pub fn wheel_bounds(&self, world: &dyn World, out: &mut [Bounds; MAX_RECTS]) -> usize {
        let (width, height) = world.picture();
        let whole = Bounds { left: 0, top: 0, right: i32::from(width), bottom: i32::from(height) };
        let mut count = 0_usize;
        for rule in self.table.rules.iter() {
            for place in 0..rule.row_count() {
                let row = self.table.row_of(rule, place);
                for trigger in rule.triggers.iter() {
                    let Trigger::Wheel { target } = trigger else { continue };
                    let Some(target) = target.resolve(&row) else { continue };
                    let bounds = match target.kind {
                        TargetKind::Page => Some(whole),
                        TargetKind::Rect => self.table.rects.get(usize::from(target.number)).map(Bounds::of_rect),
                        _ if target.is_any() => Some(whole),
                        _ => world.target(target).filter(|state| state.shown).and_then(|state| state.bounds),
                    };
                    let Some(bounds) = bounds.and_then(|bounds| bounds.cut_to(width, height)) else { continue };
                    let known = out.get(..count).is_some_and(|held| held.contains(&bounds));
                    if let (false, Some(slot)) = (known, out.get_mut(count)) {
                        *slot = bounds;
                        count = count.saturating_add(1);
                    }
                }
            }
        }
        count
    }

    /// Step 1 (RULE-5, RULE-7): the rule state as the last tick left it,
    /// and the world as it is now.
    fn take_snapshot(&mut self, world: &dyn World) {
        self.working = self.snapshot;
        self.armed_then = u8::try_from(self.armed.len()).unwrap_or(u8::MAX);
        self.picture = world.picture();
        for (sound, channel) in self.sounds.iter_mut().zip(0_u8..) {
            *sound = world.sound(channel);
        }
        for (state, target) in self.targets.iter_mut().zip(self.index.targets.iter()) {
            *state = world.target(*target);
        }
        for slot in 0..self.index.values.len() {
            let Some(value) = self.index.values.get(slot).copied() else { continue };
            let reading = if value.is_input_state() { 0 } else { self.read_snapshot(value, world) };
            if let Some(held) = self.readings.get_mut(slot) {
                *held = reading;
            }
        }
    }

    /// Reads one value that is not input state, as a snapshot is taken.
    fn read_snapshot(&self, value: Value, world: &dyn World) -> i32 {
        if value.space == SPACE_SYNTH && !Self::is_metered(value) {
            let sound = self.sounds.get(usize::from(value.a)).copied().unwrap_or_default();
            // RULE-28: another family's sound, or none, reads 0.
            if !sound.playing || sound.family != value.family() || sound.variant != value.variant() {
                return 0;
            }
            return world.synth(value.a, value.part(), value.class(), value.d);
        }
        self.read_held(value)
    }

    /// Reads one value from what the evaluator itself holds of the
    /// snapshot: every value but input state, the firing's, and a
    /// synthesizer's tracked values, which the world gives.
    fn read_held(&self, value: Value) -> i32 {
        match value.space {
            SPACE_VARIABLE => self.snapshot.variables.get(usize::from(value.a)).copied().unwrap_or(0),
            SPACE_TIMER => {
                let elapsed = self.elapsed(value.a);
                match value.b {
                    TIMER_RUNNING => i32::from(elapsed.is_some()),
                    TIMER_ELAPSED => i32::try_from(elapsed.unwrap_or(0)).unwrap_or(i32::MAX),
                    _ => 0,
                }
            }
            SPACE_TARGET => self.read_target(value),
            SPACE_SOUND => {
                let sound = self.sounds.get(usize::from(value.a)).copied().unwrap_or_default();
                match value.b {
                    SOUND_PLAYING => i32::from(sound.playing),
                    SOUND_LEVEL => self.read_metered(value),
                    SOUND_HANDLE => {
                        if sound.playing {
                            i32::from(sound.handle)
                        } else {
                            -1
                        }
                    }
                    SOUND_GAIN => i32::from(sound.gain),
                    SOUND_LOCAL => i32::from(sound.playing && sound.local),
                    _ => 0,
                }
            }
            SPACE_SYNTH => {
                let sound = self.sounds.get(usize::from(value.a)).copied().unwrap_or_default();
                // RULE-28: another family's sound, or none, reads 0.
                if !sound.playing || sound.family != value.family() || sound.variant != value.variant() {
                    return 0;
                }
                if Self::is_metered(value) { self.read_metered(value) } else { 0 }
            }
            SPACE_SEQUENCE => {
                let sequence = self.snapshot.sequences.get(usize::from(value.a)).copied().unwrap_or(STOPPED);
                match value.b {
                    SEQUENCE_RUNNING => i32::from(sequence.running),
                    SEQUENCE_PASSES => i32::try_from(sequence.passes).unwrap_or(i32::MAX),
                    SEQUENCE_POSITION if sequence.running => i32::try_from(sequence.position).unwrap_or(i32::MAX),
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    /// Whether a value is metered by the sound helper (RULE-33): a
    /// channel's `level`, and a sound's envelope stage, envelope level
    /// and voice count.
    const fn is_metered(value: Value) -> bool {
        match value.space {
            SPACE_SOUND => value.b == SOUND_LEVEL,
            SPACE_SYNTH => matches!(value.class(), CLASS_ENVELOPE_STAGE | CLASS_ENVELOPE_LEVEL | CLASS_VOICES),
            _ => false,
        }
    }

    /// Reads the metered values again, of the newest report (RULE-7): at
    /// a tick whose snapshot an arrival took, the steps after the input
    /// events read the newest report at the tick. Every other value stays
    /// as the snapshot took it.
    fn refresh_metered(&mut self) {
        for slot in 0..self.index.values.len() {
            let Some(value) = self.index.values.get(slot).copied() else { continue };
            if !Self::is_metered(value) {
                continue;
            }
            let reading = self.read_held(value);
            if let Some(held) = self.readings.get_mut(slot) {
                *held = reading;
            }
        }
    }

    /// A metered value's newest report (RULE-33); 0 before any.
    fn read_metered(&self, value: Value) -> i32 {
        let place = self.index.watch_place(&value);
        i32::from(place.and_then(|place| self.metered.get(place)).copied().unwrap_or(0))
    }

    /// A value of space 4 from the snapshot (RULE-26).
    fn read_target(&self, value: Value) -> i32 {
        let state = value
            .named_target()
            .and_then(|target| self.index.target_slot(target))
            .and_then(|slot| self.targets.get(slot).copied().flatten());
        let Some(state) = state else { return 0 };
        match value.c {
            TARGET_EXISTS => 1,
            TARGET_SHOWN => i32::from(state.shown),
            TARGET_FRAME => i32::from(state.frame),
            TARGET_X => i32::from(state.x),
            TARGET_Y => i32::from(state.y),
            TARGET_LOOK => i32::from(state.look),
            TARGET_LAYER => i32::from(state.layer),
            TARGET_ORDER => i32::from(state.order),
            TARGET_CYCLING => i32::from(state.cycling),
            _ => 0,
        }
    }

    /// The ticks since a timer started, in the snapshot (RULE-15), or
    /// `None` while it is stopped. The page timer runs since tick 0.
    fn elapsed(&self, timer: u8) -> Option<u32> {
        let start =
            if timer == PAGE_TIMER { Some(0) } else { self.snapshot.timers.get(usize::from(timer)).copied().flatten() };
        start.and_then(|start| self.tick.checked_sub(start))
    }

    /// A value's reading for a condition: the snapshot's, for input state
    /// the state as the events so far leave it (RULE-8), and for a value
    /// of the firing that of the event and row being matched (RULE-116).
    fn reading(&self, value: Value, slot: u8, fired: &Fired, input: &Seen) -> i32 {
        if value.space == SPACE_FIRED {
            return fired.read(u16::from(value.a));
        }
        if value.space == SPACE_TARGET && value.is_input_state() {
            return i32::from(value.named_target().is_some_and(|target| input.inside.contains(target)));
        }
        if value.is_input_state() {
            return input.read(value);
        }
        if slot == NO_SLOT {
            return self.read_held(value);
        }
        self.readings.get(usize::from(slot)).copied().unwrap_or(0)
    }

    /// A value's reading for a command (RULE-7): a variable as the
    /// working state holds it (on arrival, the arrival copy), any other
    /// as a condition reads it. A value the table's own index does not
    /// hold, which only a command the server sent can name, reads what
    /// the evaluator holds of the snapshot, and 0 for a target's state
    /// and a synthesizer's.
    fn command_reading(&self, value: Value, fired: &Fired, pass: Pass) -> i32 {
        if value.space == SPACE_VARIABLE {
            return self.variable(value.a, pass);
        }
        let slot = self.index.value_slot(&value).and_then(|slot| u8::try_from(slot).ok()).unwrap_or(NO_SLOT);
        self.reading(value, slot, fired, &self.view)
    }

    /// A variable as a command reads it (RULE-7, RULE-96).
    fn variable(&self, variable: u8, pass: Pass) -> i32 {
        let held = if pass == Pass::Arrival { &self.arrival } else { &self.working.variables };
        held.get(usize::from(variable)).copied().unwrap_or(0)
    }

    /// A target's bounding rectangle in the snapshot (RULE-19), or `None`
    /// when it does not exist or is not shown.
    fn bounds_of(&self, target: Target) -> Option<Bounds> {
        let (width, height) = self.picture;
        match target.kind {
            TargetKind::Page => {
                Bounds { left: 0, top: 0, right: i32::from(width), bottom: i32::from(height) }.cut_to(width, height)
            }
            TargetKind::Rect => {
                let rect = self.table.rects.get(usize::from(target.number))?;
                Bounds::of_rect(rect).cut_to(width, height)
            }
            TargetKind::Control | TargetKind::Object => {
                let state = self.targets.get(self.index.target_slot(target)?).copied().flatten()?;
                if state.shown { state.bounds.filter(|bounds| !bounds.is_empty()) } else { None }
            }
        }
    }

    /// Whether rule `rule`'s condition holds for a firing (RULE-18); none
    /// always does.
    fn holds(&self, rule: usize, fired: &Fired) -> bool {
        let Some(nodes) = self.table.rules.get(rule).map(|rule| &rule.condition) else { return false };
        if nodes.is_empty() {
            return true;
        }
        let mut at = 0_usize;
        self.node_holds(rule, nodes, &mut at, 0, fired)
    }

    /// Evaluates the node at `at` and its children, leaving `at` after
    /// them. Every child is evaluated: no node has an effect, and the
    /// place after the tree is then known.
    fn node_holds(&self, rule: usize, nodes: &[Node], at: &mut usize, depth: usize, fired: &Fired) -> bool {
        let place = *at;
        *at = place.saturating_add(1);
        // The decoder holds operators to their levels (RULE-18).
        let (Some(node), true) = (nodes.get(place), depth <= MAX_CONDITION_DEPTH) else { return false };
        let read = |value: Value| {
            let slot = self.node_slots.get(rule).and_then(|slots| slots.get(place)).copied().unwrap_or(NO_SLOT);
            self.reading(value, slot, fired, &self.input.seen)
        };
        let below = depth.saturating_add(1);
        match node {
            Node::And(children) => {
                let mut all = true;
                for _ in 0..*children {
                    all &= self.node_holds(rule, nodes, at, below, fired);
                }
                all
            }
            Node::Or(children) => {
                let mut any = false;
                for _ in 0..*children {
                    any |= self.node_holds(rule, nodes, at, below, fired);
                }
                any
            }
            Node::Not => !self.node_holds(rule, nodes, at, below, fired),
            Node::Compare { value, operator, constant, parameter } => {
                // RULE-105: the row's parameter is added with saturation.
                let added = parameter.and_then(|index| fired.parameters.get(usize::from(index))).copied().unwrap_or(0);
                operator.holds(read(*value), constant.saturating_add(added))
            }
            Node::InRange { value, low, high } => (*low..=*high).contains(&read(*value)),
            Node::InSet { value, members } => members.contains(&read(*value)),
            Node::Overlaps(first, second) => match (self.bounds_of(*first), self.bounds_of(*second)) {
                (Some(first), Some(second)) => first.overlaps(&second),
                _ => false,
            },
            Node::Inside(first, second) => match (self.bounds_of(*first), self.bounds_of(*second)) {
                (Some(first), Some(second)) => first.within(&second),
                _ => false,
            },
        }
    }

    /// Step 3 (RULE-5): the input events that belong to this tick, in
    /// arrival order, at most `MAX_EVENTS_PER_TICK`: those not decided on
    /// arrival are decided now, every one before any group runs, since no
    /// command can change what a condition or the input state reads in
    /// its own tick (RULE-6); then each event's firings are run in order.
    fn run_events(&mut self, world: &mut dyn World) {
        while self.decided_count < MAX_EVENTS_PER_TICK {
            let Some(event) = self.next_event() else { break };
            self.decide(&event, world, false);
        }
        for place in 0..self.decided_count {
            let Some(decided) = self.decided.get(place).copied() else { continue };
            // RULE-8: the input state as it was just after this event.
            self.view = decided.input;
            let pass = if decided.delivered { Pass::Rest } else { Pass::Whole };
            for at in decided.first..decided.first.saturating_add(decided.count) {
                let Some(firing) = self.firings.get(at).copied() else { continue };
                let fired = self.fired_by(firing, &decided.event);
                self.fire(&fired, pass, world);
            }
        }
        self.decided_count = 0;
        self.firing_count = 0;
    }

    /// The firing of an input event's rule, as its group's commands read
    /// it (RULE-116).
    fn fired_by(&self, firing: Firing, event: &InputEvent) -> Fired {
        let rule = self.table.rules.get(usize::from(firing.rule));
        let rows = rule.is_some_and(|rule| rule.rows.is_some());
        let key = match event.input {
            Input::Key { key, .. } => key.code(),
            _ => 0,
        };
        Fired {
            rule: firing.rule,
            row: if rows { firing.row } else { NO_RULE },
            key,
            target: firing.target,
            cue: 0,
            parameters: rule.map_or(NO_ROW, |rule| self.table.row_of(rule, usize::from(firing.row))),
            sequence: Some(event.sequence),
        }
    }

    /// Decides one event for the coming tick (RULE-96): stamps it with
    /// the tick, updates the input state (RULE-8), matches each rule in
    /// rule order and each of its rows in row order, and records those
    /// that fire. Gives the event's place among the tick's decided
    /// events.
    fn decide(&mut self, event: &InputEvent, world: &dyn World, delivered: bool) -> usize {
        let transition = self.follow(&event.input, world);
        let first = self.firing_count;
        let mut found: Bounded<Firing, MAX_RULE_ROWS> = Bounded::new(NO_FIRING);
        let mut dropped = 0_u32;
        // The kept press a `double` or `double_press` that fired paired
        // with (RULE-108).
        let mut paired: Option<usize> = None;
        for index in 0..self.table.rules.len() {
            self.match_rule(index, event, &transition, &mut |firing, pair| {
                if found.push(firing).is_err() {
                    dropped = dropped.saturating_add(1);
                }
                paired = paired.or(pair);
            });
        }
        self.counters.events_dropped = self.counters.events_dropped.saturating_add(dropped);
        for firing in found.iter() {
            if let Some(slot) = self.firings.get_mut(self.firing_count) {
                *slot = *firing;
                self.firing_count = self.firing_count.saturating_add(1);
            }
        }
        self.remember_press(&event.input, paired);
        let place = self.decided_count;
        if let Some(slot) = self.decided.get_mut(place) {
            *slot = Decided {
                event: *event,
                input: self.input.seen,
                first,
                count: self.firing_count.saturating_sub(first),
                delivered,
            };
            self.decided_count = place.saturating_add(1);
        }
        place
    }

    /// Keeps a press for a later double, or forgets the one this press
    /// paired with (RULE-108): three quick presses are one double and a
    /// single. An automatic repeat is not a press.
    fn remember_press(&mut self, input: &Input, paired: Option<usize>) {
        let what = match *input {
            Input::Key { action: KeyAction::Press, key, .. } => Pressed::Key(key),
            Input::Pointer { action: PointerAction::Press, button, .. } if button <= MAX_BUTTON => {
                Pressed::Button(button, self.input.seen.inside)
            }
            _ => return,
        };
        match paired {
            Some(place) => self.input.forget_press(place),
            None => self.input.keep_press(what, self.tick),
        }
    }

    /// Delivers at once the sound commands of the groups a decided event
    /// fires, and applies their `VARIABLE` commands to the arrival copy
    /// (RULE-96).
    fn walk_arrival(&mut self, place: usize, world: &mut dyn World) {
        let Some(decided) = self.decided.get(place).copied() else { return };
        self.view = decided.input;
        for at in decided.first..decided.first.saturating_add(decided.count) {
            let Some(firing) = self.firings.get(at).copied() else { continue };
            let fired = self.fired_by(firing, &decided.event);
            let group = self.table.rules.get(usize::from(firing.rule)).map_or(0, |rule| rule.group);
            self.run_group(group, &fired, Pass::Arrival, world, 0);
        }
    }

    /// Updates the input state for one event (RULE-8), before the rules
    /// are matched against it.
    fn follow(&mut self, input: &Input, world: &dyn World) -> Transition {
        let at = |control: u16, x: u16, y: u16, rects: &[Rect]| {
            let mut found = [0_u16; MAX_POINTER_OBJECTS];
            let count = world.objects_at(x, y, &mut found).min(MAX_POINTER_OBJECTS);
            let mut objects = Bounded::new(0);
            for object in found.iter().take(count).filter(|object| **object != 0) {
                // No more than the array holds.
                let _ = objects.push(*object);
            }
            Inside { page: true, control, objects, rects: InputState::rects_at(rects, x, y) }
        };
        let tick = self.tick;
        match *input {
            Input::Key { action, key, base, mods } => {
                self.input.key(action, key, base, mods, tick);
                Transition::NONE
            }
            Input::Pointer { action: PointerAction::Gone, .. } => self.input.gone(),
            Input::Pointer { action, button, x, y, control } => {
                let now = at(control, x, y, &self.table.rects);
                self.input.pointer(action, button, now, (x, y), tick)
            }
            Input::Wheel { x, y, control } => {
                let now = at(control, x, y, &self.table.rects);
                // A wheel event moves nothing and presses nothing, but it
                // says where the pointer is.
                self.input.pointer(PointerAction::Gone, ANY_BUTTON, now, (x, y), tick)
            }
            Input::Text | Input::Focus | Input::Blur => Transition::NONE,
        }
    }

    /// Matches one event against rule `index`, each of its rows in row
    /// order, and gives `fire` each firing whose condition holds, with the
    /// kept press a double paired with (RULE-9, RULE-105, RULE-107). An
    /// event fires a row at most once: by the first of the rule's
    /// triggers it matches, once for each control or object an "any"
    /// names.
    fn match_rule(
        &self,
        index: usize,
        event: &InputEvent,
        transition: &Transition,
        fire: &mut dyn FnMut(Firing, Option<usize>),
    ) {
        let Some(rule) = self.table.rules.get(index) else { return };
        let number = u8::try_from(index).unwrap_or(NO_RULE);
        for place in 0..rule.row_count() {
            let row = self.table.row_of(rule, place);
            let row_number = u8::try_from(place).unwrap_or(NO_RULE);
            // The page, or each control or object an "any" names.
            let mut hits: Bounded<Target, { MAX_POINTER_OBJECTS + 1 }> = Bounded::new(Target::NONE);
            let mut pair = None;
            for trigger in rule.triggers.iter() {
                pair = self.matches(trigger, &row, &event.input, transition, &mut |target| {
                    let _ = hits.push(target);
                });
                if !hits.is_empty() {
                    break;
                }
            }
            for target in hits.iter() {
                let firing = Firing { rule: number, row: row_number, target: *target };
                if self.holds(index, &self.fired_by(firing, event)) {
                    fire(firing, pair);
                }
            }
        }
    }

    /// Gives `hit` each target for which `trigger` matches `input` with
    /// `row`'s parameters (RULE-9): [`Target::NONE`] for a trigger that
    /// names none. Answers the kept press a `double` or `double_press`
    /// paired with.
    fn matches(
        &self,
        trigger: &Trigger,
        row: &Row,
        input: &Input,
        transition: &Transition,
        hit: &mut dyn FnMut(Target),
    ) -> Option<usize> {
        let inside = &self.input.seen.inside;
        let button_named = |named: u8, event: u8| named == ANY_BUTTON || named == event;
        // The ticks from a kept press to this one, when within `ticks`.
        let within = |kept: Option<(usize, u32)>, ticks: u16| {
            kept.filter(|(_, then)| self.tick.saturating_sub(*then) <= u32::from(ticks)).map(|(place, _)| place)
        };
        match (trigger, input) {
            (Trigger::Key { keys, repeat }, Input::Key { action, key, base, mods }) => {
                let pressed = *action == KeyAction::Press || (*action == KeyAction::Repeat && *repeat);
                if pressed && keys_match(keys, row, *key, *base, *mods) {
                    hit(Target::NONE);
                }
            }
            (Trigger::KeyUp { keys }, Input::Key { action: KeyAction::Release, key, base, mods }) => {
                if keys_match(keys, row, *key, *base, *mods) {
                    hit(Target::NONE);
                }
            }
            (Trigger::Double { keys, ticks }, Input::Key { action: KeyAction::Press, key, base, mods }) => {
                let kept = self.input.last_press(|pressed| *pressed == Pressed::Key(*key));
                if let (true, Some(place)) = (keys_match(keys, row, *key, *base, *mods), within(kept, *ticks)) {
                    hit(Target::NONE);
                    return Some(place);
                }
            }
            (Trigger::Text, Input::Text) | (Trigger::Focus, Input::Focus) | (Trigger::Blur, Input::Blur) => {
                hit(Target::NONE);
            }
            (Trigger::Press { target, button }, Input::Pointer { action: PointerAction::Press, button: down, .. })
                if button_named(*button, *down) =>
            {
                if let Some(target) = target.resolve(row) {
                    inside.matching(target, hit);
                }
            }
            (
                Trigger::DoublePress { target, button, ticks },
                Input::Pointer { action: PointerAction::Press, button: down, .. },
            ) if button_named(*button, *down) => {
                let kept = self.input.last_press(|pressed| matches!(pressed, Pressed::Button(held, _) if held == down));
                let (Some(place), Some(target)) = (within(kept, *ticks), target.resolve(row)) else { return None };
                let Some(Pressed::Button(_, before)) = self.input.recent.get(place).map(|recent| recent.what) else {
                    return None;
                };
                // Inside the same target both times.
                let mut any = false;
                inside.matching(target, &mut |found| {
                    if before.contains(found) {
                        any = true;
                        hit(found);
                    }
                });
                return any.then_some(place);
            }
            (
                Trigger::Release { target, button },
                Input::Pointer { action: PointerAction::Release, button: up, .. },
            ) if button_named(*button, *up) => {
                if let Some(target) = target.resolve(row) {
                    transition.released.matching(target, hit);
                }
            }
            (Trigger::Enter { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(row) {
                    transition.entered.matching(target, hit);
                }
            }
            (Trigger::Leave { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(row) {
                    transition.left.matching(target, hit);
                }
            }
            (Trigger::Move { target, held }, Input::Pointer { action: PointerAction::Move, button: mask, .. })
                if transition.moved && (!held || *mask != 0 || self.input.seen.buttons != 0) =>
            {
                if let Some(target) = target.resolve(row) {
                    inside.matching(target, hit);
                }
            }
            (Trigger::Wheel { target }, Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(row) {
                    inside.matching(target, hit);
                }
            }
            _ => {}
        }
        None
    }

    /// Fires each row of each rule, in rule order and row order, for
    /// which `occurs` gives a firing and the rule's condition holds, and
    /// runs its group whole. `occurs` is asked for each trigger of a row
    /// until one answers.
    fn run_rules(&mut self, world: &mut dyn World, occurs: Occurs<'_>) {
        for index in 0..self.table.rules.len() {
            let rows = self.table.rules.get(index).map_or(0, crate::table::Rule::row_count);
            for place in 0..rows {
                let Some(rule) = self.table.rules.get(index) else { continue };
                let row = self.table.row_of(rule, place);
                let Some((key, target, cue)) = rule.triggers.iter().find_map(|trigger| occurs(trigger, &row)) else {
                    continue;
                };
                let fired = Fired {
                    rule: u8::try_from(index).unwrap_or(NO_RULE),
                    row: if rule.rows.is_some() { u8::try_from(place).unwrap_or(NO_RULE) } else { NO_RULE },
                    key,
                    target,
                    cue,
                    parameters: row,
                    sequence: None,
                };
                if self.holds(index, &fired) {
                    self.fire(&fired, Pass::Whole, world);
                }
            }
        }
    }

    /// Step 2, and the final tick (RULE-5): each rule with the trigger
    /// `open` or `close`.
    fn run_simple(&mut self, wanted: &Trigger, world: &mut dyn World) {
        self.run_rules(world, &|trigger, _| (trigger == wanted).then_some((0, Target::NONE, 0)));
    }

    /// Step 4 (RULE-5, RULE-108): for each key and each pointer button
    /// that has been down for exactly `N` ticks, in the order they went
    /// down, each rule with a `held` or `press_held` trigger of `N` ticks
    /// that it matches.
    fn run_holds(&mut self, world: &mut dyn World) {
        let input = self.input;
        let tick = self.tick;
        let mut after = 0_u32;
        loop {
            // The next key or button down, in the order they went down.
            let key = input.seen.keys.iter().filter(|down| down.order > after).min_by_key(|down| down.order).copied();
            let button = (0_u8..)
                .zip(input.pressed.iter())
                .filter(|(_, down)| down.down && down.order > after)
                .min_by_key(|(_, down)| down.order)
                .map(|(button, down)| (button, *down));
            match (key, button) {
                (Some(key), button) if button.is_none_or(|(_, down)| key.order < down.order) => {
                    after = key.order;
                    let down_for = tick.saturating_sub(key.tick);
                    self.run_rules(world, &|trigger, row| match trigger {
                        Trigger::Held { keys, ticks }
                            if u32::from(*ticks) == down_for && keys_match(keys, row, key.key, key.base, key.mods) =>
                        {
                            Some((key.key.code(), Target::NONE, 0))
                        }
                        _ => None,
                    });
                }
                (_, Some((button, down))) => {
                    after = down.order;
                    let down_for = tick.saturating_sub(down.tick);
                    self.run_rules(world, &|trigger, row| match trigger {
                        Trigger::PressHeld { target, button: named, ticks }
                            if u32::from(*ticks) == down_for && (*named == ANY_BUTTON || *named == button) =>
                        {
                            let mut found = None;
                            down.inside.matching(target.resolve(row)?, &mut |hit| found = found.or(Some(hit)));
                            found.map(|hit| (0, hit, 0))
                        }
                        _ => None,
                    });
                }
                _ => break,
            }
        }
    }

    /// Step 5 (RULE-5, RULE-114): the commands the server sent directly
    /// since the last tick, in arrival order. A value of the firing reads
    /// 0 in them, and a `REPORT` of the firing reports a bare firing with
    /// no rule (RULE-22).
    fn run_server_commands(&mut self, world: &mut dyn World) {
        let commands = self.commands;
        self.commands.clear();
        let fired = Fired::held(NO_ROW);
        for (kind, body, length) in commands.iter() {
            let body = body.get(..usize::from(*length)).unwrap_or(&[]);
            self.counters.commands = self.counters.commands.saturating_add(1);
            self.apply_command(*kind, body, &fired, Pass::Whole, world, 0);
        }
    }

    /// Step 6 (RULE-5, RULE-102): the armed runs whose due tick is this
    /// tick or earlier, in the order they were armed, each once.
    fn run_armed(&mut self, world: &mut dyn World) {
        let armed = self.armed;
        self.armed.clear();
        for run in armed.iter().filter(|run| run.due > self.tick) {
            // No more than were armed.
            let _ = self.armed.push(*run);
        }
        let tick = self.tick;
        for run in armed.iter().filter(|run| run.due <= tick) {
            self.run_group(run.group, &Fired::held(run.parameters), Pass::Whole, world, 0);
        }
    }

    /// Step 7 (RULE-5, RULE-15): the timers in identifier order, the page
    /// timer first; for each that runs in the snapshot, each rule whose
    /// `after` or `every` on it is due.
    fn run_timers(&mut self, world: &mut dyn World) {
        let timers = core::iter::once(PAGE_TIMER).chain((0_u8..).take(TIMERS));
        for timer in timers {
            let Some(elapsed) = self.elapsed(timer) else { continue };
            self.run_rules(world, &|trigger, _| {
                let due = match trigger {
                    Trigger::After { timer: named, ticks } => *named == timer && elapsed == *ticks,
                    Trigger::Every { timer: named, ticks } => {
                        *named == timer && elapsed > 0 && elapsed.checked_rem(*ticks) == Some(0)
                    }
                    _ => false,
                };
                due.then_some((0, Target::NONE, 0))
            });
        }
    }

    /// Step 8 (RULE-5, RULE-103): for each sequence running in the
    /// snapshot, in number order, each of its steps whose offset equals
    /// its position, in step order; then its position advances, a pass
    /// ends or the sequence ends.
    fn run_sequences(&mut self, world: &mut dyn World) {
        for number in 0..self.table.sequences.len() {
            let Some(running) = self.snapshot.sequences.get(number).copied().filter(|running| running.running) else {
                continue;
            };
            let Some((length, repeat, steps)) = self
                .table
                .sequences
                .get(number)
                .map(|sequence| (sequence.length, sequence.repeat, sequence.steps.len()))
            else {
                continue;
            };
            let fired = Fired::held(running.parameters);
            for place in 0..steps {
                let step = self.table.sequences.get(number).and_then(|sequence| sequence.steps.get(place)).copied();
                if let Some(step) = step.filter(|step| step.offset == running.position) {
                    self.run_group(step.group, &fired, Pass::Whole, world, 0);
                }
            }
            // A start or a stop applied in this tick is not advanced over
            // (RULE-6): the run it replaced or ended is gone.
            let Some(now) = self.working.sequences.get_mut(number) else { continue };
            if !now.running || now.since != running.since {
                continue;
            }
            now.position = now.position.saturating_add(1);
            if now.position >= length {
                now.position = 0;
                now.passes = now.passes.saturating_add(1);
                if repeat != 0 && now.passes >= u32::from(repeat) {
                    now.running = false;
                }
            }
        }
    }

    /// Step 9 (RULE-5, RULE-109): the cues reported since the last tick,
    /// in the order reported; for each, each rule with a `cue` trigger it
    /// matches.
    fn run_cues(&mut self, world: &mut dyn World) {
        let cues = self.cues;
        self.cues.clear();
        for (channel, number) in cues.iter().copied() {
            self.counters.cues = self.counters.cues.saturating_add(1);
            self.run_rules(world, &|trigger, _| match trigger {
                Trigger::Cue { channel: named, number: wanted }
                    if *named == channel && (*wanted == ANY_CUE || *wanted == number) =>
                {
                    Some((0, Target::NONE, number))
                }
                _ => None,
            });
        }
    }

    /// Step 10 (RULE-5, RULE-11): each `becomes` rule, and each of its
    /// rows, fires when its condition holds now and did not at its
    /// previous evaluation.
    fn run_becomes(&mut self, world: &mut dyn World) {
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            if !rule.triggers.contains(&Trigger::Becomes) {
                continue;
            }
            let (rows, named) = (rule.row_count(), rule.rows.is_some());
            for place in 0..rows {
                let Some(rule) = self.table.rules.get(index) else { continue };
                let fired = Fired {
                    rule: u8::try_from(index).unwrap_or(NO_RULE),
                    row: if named { u8::try_from(place).unwrap_or(NO_RULE) } else { NO_RULE },
                    key: 0,
                    target: Target::NONE,
                    cue: 0,
                    parameters: self.table.row_of(rule, place),
                    sequence: None,
                };
                let holds = self.holds(index, &fired);
                let bit = u32::try_from(place).ok().and_then(|place| 1_u64.checked_shl(place)).unwrap_or(0);
                let Some(bits) = self.becomes.get_mut(index) else { continue };
                let held = *bits & bit != 0;
                if holds {
                    *bits |= bit;
                } else {
                    *bits &= !bit;
                }
                if holds && !held {
                    self.fire(&fired, Pass::Whole, world);
                }
            }
        }
    }

    /// Runs the group of the rule that fired (RULE-118).
    fn fire(&mut self, fired: &Fired, pass: Pass, world: &mut dyn World) {
        let Some(group) = self.table.rules.get(usize::from(fired.rule)).map(|rule| rule.group) else { return };
        self.counters.fired = self.counters.fired.saturating_add(1);
        self.run_group(group, fired, pass, world, 0);
    }

    /// Counts commands that did nothing (RULE-118).
    fn skip(&mut self, count: usize) {
        self.counters.skipped = self.counters.skipped.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    /// The value of a binding's source (RULE-106): a variable's working
    /// value (on arrival, the arrival copy's), a row parameter in force,
    /// or a value of the firing.
    fn source(&self, source: Source, fired: &Fired, pass: Pass) -> i32 {
        match source {
            Source::Variable(variable) => self.variable(variable, pass),
            Source::Parameter(parameter) => fired.parameters.get(usize::from(parameter)).copied().unwrap_or(0),
            Source::Fired(property) => fired.read(u16::from(property)),
        }
    }

    /// Runs group `group` (RULE-118): its commands in written order, each
    /// with its amounts resolved first, a `RUN` with the mode "now"
    /// replaced by the run of its group (RULE-102). `depth` is how many
    /// groups are being run above this one; the table's check holds it
    /// below `MAX_GROUP_DEPTH`, and so does this.
    fn run_group(&mut self, group: u8, fired: &Fired, pass: Pass, world: &mut dyn World, depth: usize) {
        let Some((mut at, count)) = self.table.groups.start(usize::from(group)).filter(|_| depth < MAX_GROUP_DEPTH)
        else {
            self.skip(usize::from(pass != Pass::Arrival));
            return;
        };
        if pass != Pass::Arrival {
            self.counters.groups = self.counters.groups.saturating_add(1);
        }
        for _ in 0..count {
            let mut body = [0_u8; MAX_COMMAND_BYTES];
            let Some((command, next)) = self.table.groups.frame(at) else { break };
            at = next;
            let kind = command.kind;
            let mut bindings = [NO_BINDING; MAX_COMMAND_BINDINGS];
            let mut bound = 0_usize;
            for (binding, slot) in command.bindings().zip(bindings.iter_mut()) {
                *slot = binding;
                bound = bound.saturating_add(1);
            }
            let bindings: &[Binding] = bindings.get(..bound).unwrap_or(&[]);
            let resolved = command::apply_bindings(
                kind,
                command.body,
                bindings,
                &|source| self.source(source, fired, pass),
                &mut body,
            );
            if pass != Pass::Arrival {
                self.counters.commands = self.counters.commands.saturating_add(1);
                self.skip(resolved.skipped);
            }
            let Some(body) = body.get(..resolved.length).filter(|body| !body.is_empty()) else { continue };
            self.apply_command(kind, body, fired, pass, world, depth);
        }
    }

    /// Applies one command (RULE-118): the evaluator's own (`VARIABLE`,
    /// `TIMER`, `REPORT`, `RUN`, `SEQUENCE`) itself, every other through
    /// the world. On an event's arrival only the sound commands are
    /// delivered and the `VARIABLE` commands applied to the arrival copy;
    /// at that event's tick the sound commands are not delivered again
    /// (RULE-96).
    fn apply_command(&mut self, kind: u8, body: &[u8], fired: &Fired, pass: Pass, world: &mut dyn World, depth: usize) {
        if !(TYPE_VARIABLE..=command::TYPE_SEQUENCE).contains(&kind) {
            let sound = command::is_sound(kind);
            let now = match pass {
                Pass::Arrival => sound,
                Pass::Rest => !sound,
                Pass::Whole => true,
            };
            if now && !world.command(kind, body, self.tick) {
                self.skip(1);
            }
            return;
        }
        let Ok((_, _, command)) = command::decode(kind, body) else {
            self.skip(usize::from(pass != Pass::Arrival));
            return;
        };
        match (command, pass) {
            (Command::Variable { operation, variable, operand }, _) => {
                self.apply_variable(operation, variable, operand, fired, pass);
            }
            (Command::Run { group, mode: RunMode::Now }, _) => {
                self.run_group(group, fired, pass, world, depth.saturating_add(1));
            }
            // On arrival nothing else is done (RULE-96); and `LOCAL_MIDI`
            // is the world's, and never reaches here.
            (_, Pass::Arrival) | (Command::LocalMidi { .. }, _) => {}
            (Command::Run { group, mode }, _) => self.arm(group, mode, fired),
            (Command::Timer { start, timer }, _) => {
                // RULE-15: running since the next tick.
                let since = self.tick.saturating_add(1);
                if let Some(slot) = self.working.timers.get_mut(usize::from(timer)) {
                    *slot = start.then_some(since);
                }
            }
            (Command::Sequence { start, sequence }, _) => self.apply_sequence(start, sequence, fired),
            (Command::Report { value }, _) => {
                let what = match (value, fired.sequence) {
                    (Some(value), _) => Reported::Value { value, reading: self.command_reading(value, fired, pass) },
                    (None, Some(sequence)) => Reported::Input { sequence },
                    (None, None) => Reported::Fired,
                };
                // RULE-46: only a report of an input event names a target.
                let target = if matches!(what, Reported::Input { .. }) { fired.target } else { Target::NONE };
                self.counters.reports = self.counters.reports.saturating_add(1);
                world.report(Report { rule: fired.rule, row: fired.row, tick: self.tick, target, what });
            }
        }
    }

    /// Applies a `VARIABLE` command (RULE-99) to the working variables,
    /// or on an event's arrival to the arrival copy (RULE-96). Every sum
    /// saturates at the 32-bit bounds.
    fn apply_variable(
        &mut self,
        operation: VariableOperation,
        variable: u8,
        operand: Operand,
        fired: &Fired,
        pass: Pass,
    ) {
        let given = match operand {
            Operand::None | Operand::Range { .. } => 0,
            Operand::Constant(constant) => constant,
            Operand::Variable { constant, variable } => constant.saturating_add(self.variable(variable, pass)),
            Operand::Fired { constant, property } => constant.saturating_add(fired.read(u16::from(property))),
            Operand::Value(value) => self.command_reading(value, fired, pass),
        };
        let change = |held: i32| match operation {
            VariableOperation::Set => given,
            VariableOperation::Inc => held.saturating_add(1),
            VariableOperation::Dec => held.saturating_sub(1),
            VariableOperation::Add => held.saturating_add(given),
            VariableOperation::Sub => held.saturating_sub(given),
            VariableOperation::Toggle => i32::from(held == 0),
            VariableOperation::Min => held.min(given),
            VariableOperation::Max => held.max(given),
            VariableOperation::Wrap => match operand {
                Operand::Range { low, high } if held > high => low,
                Operand::Range { low, high } if held < low => high,
                _ => held,
            },
            VariableOperation::Clear => 0,
        };
        let variables = if pass == Pass::Arrival { &mut self.arrival } else { &mut self.working.variables };
        if variable == ALL_VARIABLES {
            for slot in variables.iter_mut() {
                *slot = change(*slot);
            }
        } else if let Some(slot) = variables.get_mut(usize::from(variable)) {
            *slot = change(*slot);
        }
    }

    /// Arms a run of `group` (RULE-102): for a page tick, after a delay,
    /// or on a timer's next beat. The due tick is fixed now.
    fn arm(&mut self, group: u8, mode: RunMode, fired: &Fired) {
        if usize::from(group) >= self.table.groups.len() {
            self.skip(1);
            return;
        }
        let next = self.tick.saturating_add(1);
        let due = match mode {
            RunMode::Now => return,
            RunMode::At(tick) if tick <= self.tick => {
                // The tick has passed: the run is due at the next.
                self.counters.armed_late = self.counters.armed_late.saturating_add(1);
                next
            }
            RunMode::At(tick) if tick.saturating_sub(self.tick) > MAX_ARM_TICKS => {
                self.skip(1);
                return;
            }
            RunMode::At(tick) => tick,
            RunMode::After(ticks) => self.tick.saturating_add(ticks),
            RunMode::Beat { timer, ticks } => {
                // The next tick, later than this one, at which an `every`
                // of that period on the timer would be due (RULE-15). A
                // stopped timer has no beat: the next tick.
                let start = if timer == PAGE_TIMER {
                    Some(0)
                } else {
                    self.snapshot.timers.get(usize::from(timer)).copied().flatten()
                };
                start.map_or(next, |start| {
                    let elapsed = self.tick.saturating_sub(start);
                    let beats = elapsed.checked_div(ticks).unwrap_or(0).saturating_add(1);
                    start.saturating_add(beats.saturating_mul(ticks))
                })
            }
        };
        if self.armed.push(Armed { due, group, parameters: fired.parameters }).is_ok() {
            self.counters.armed = self.counters.armed.saturating_add(1);
        } else {
            self.counters.not_armed = self.counters.not_armed.saturating_add(1);
        }
    }

    /// Starts or stops a sequence (RULE-103): a start makes it running
    /// since the next tick at position 0 and pass 0, replacing any run in
    /// progress, and holds the row parameters in force.
    fn apply_sequence(&mut self, start: bool, sequence: u8, fired: &Fired) {
        let count = self.table.sequences.len();
        if sequence == ALL_SEQUENCES {
            for running in &mut self.working.sequences {
                running.running = false;
            }
            return;
        }
        let Some(slot) =
            self.working.sequences.get_mut(usize::from(sequence)).filter(|_| usize::from(sequence) < count)
        else {
            self.skip(1);
            return;
        };
        if start {
            let since = self.tick.saturating_add(1);
            *slot = Playing { running: true, since, position: 0, passes: 0, parameters: fired.parameters };
        } else {
            slot.running = false;
        }
    }

    /// Step 12 (RULE-5, RULE-7): the working state becomes the snapshot,
    /// the arrival copy the working variables, and a state query waiting
    /// is answered.
    fn apply(&mut self, world: &mut dyn World) {
        // The answer to a state query is of the snapshot this tick read.
        let state = self.query.take().map(|identifier| self.page_state(identifier));
        self.snapshot = self.working;
        self.arrival = self.working.variables;
        self.opened = false;
        if let Some(state) = state {
            world.page_state(&state);
        }
    }

    /// The page's state as the snapshot of the tick being evaluated holds
    /// it (RULE-47).
    fn page_state(&self, identifier: u32) -> PageState {
        let mut state = TableState::new(self.generation, self.tick);
        state.variables = self.snapshot.variables;
        for (timer, slot) in (0_u8..).zip(state.timers.iter_mut()) {
            let elapsed = self.elapsed(timer);
            *slot = TimerState { running: elapsed.is_some(), elapsed: elapsed.unwrap_or(0) };
        }
        for running in self.snapshot.sequences.iter().take(self.table.sequences.len()) {
            let position = if running.running { running.position } else { 0 };
            let _ = state.sequences.push(SequenceState { running: running.running, passes: running.passes, position });
        }
        state.armed = self.armed_then;
        for (target, held) in self.index.targets.iter().zip(self.targets.iter()) {
            let found = held.unwrap_or(TargetState {
                shown: false,
                cycling: false,
                frame: 0,
                look: 0,
                layer: 0,
                order: 0,
                x: 0,
                y: 0,
                bounds: None,
            });
            let _ = state.targets.push(TargetReport {
                target: *target,
                exists: held.is_some(),
                shown: found.shown,
                cycling: found.cycling,
                look: found.look.min(MAX_LOOK),
                frame: found.frame.min(MAX_FRAME),
                x: found.x,
                y: found.y,
                layer: found.layer.min(MAX_LAYER),
                order: found.order,
            });
        }
        let nothing = Fired::held(NO_ROW);
        for (value, slot) in self.index.values.iter().zip(0_u8..) {
            let _ = state.values.push((*value, self.reading(*value, slot, &nothing, &self.input.seen)));
        }
        PageState { identifier, table: Some(state) }
    }
}

// RULE-120 with RULE-102: a group run in place is a `RUN` record, whose
// type is the evaluator's own.
const _: () = assert!(TYPE_RUN > TYPE_VARIABLE && TYPE_RUN < command::TYPE_SEQUENCE);

#[cfg(test)]
mod tests;
