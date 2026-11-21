//! The evaluator (RULE-4 to RULE-8, RULE-11, RULE-14, RULE-15, RULE-96,
//! RULE-102, RULE-103, RULE-105 to RULE-107, RULE-109, RULE-116 to
//! RULE-118; DEC-6635): a page's one rule state, changed in **handlings**,
//! one at a time.
//!
//! - **An event** ([`Evaluator::handle`]: an input event, a record the
//!   server sent that the evaluator carries out, a cue heard, a painting
//!   finished (DEC-7031 item 4) or a state query) is handled at once, in
//!   arrival order, while the budgets of the present tick allow; otherwise
//!   it waits, with every event after it, for the next tick step (RULE-5,
//!   DEC-6635 items 1 and 4).
//! - **The tick step** ([`Evaluator::tick`]) handles what time drives: the
//!   `open` rules at page tick 0, the timers, the sequences, the `becomes`
//!   rules and the cycling of frames (DEC-6635 item 2).
//!
//! Every handling has two phases: it **decides** its firings against the
//! state as it stands, and then **applies** each firing's group, every
//! command taking effect at once as it is reached — sound, animation and
//! reports through the [`World`], variables, timers and sequences here.
//! No command of a handling changes which rules that handling fires
//! (RULE-6). An event belongs to the present tick, the tick of the last
//! tick step run; a timer or a sequence started in a handling of tick `t`
//! runs from tick `t + 1`. A recording of the events with their ticks and
//! their order, replayed in that order with the tick steps between, gives
//! the same outputs (DEC-6635 item 5).

mod input;

use crate::bounded::Bounded;
use crate::command::{self, Binding, BodyCheck, Command, Operand, Source, StartMode, TYPE_VARIABLE, VariableOperation};
use crate::limits::{
    ALL_SEQUENCES, ALL_VARIABLES, ANY_BUTTON, ANY_CUE, MAX_BUTTON, MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES,
    MAX_CONDITION_DEPTH, MAX_EVENT_COMMANDS_PER_TICK, MAX_EVENT_REPORTS_PER_TICK, MAX_EVENTS_PER_TICK, MAX_FRAME,
    MAX_GROUP_DEPTH, MAX_GROUPS, MAX_LAYER, MAX_LOOK, MAX_POINTER_OBJECTS, MAX_QUEUED_EVENTS, MAX_RECTS, MAX_RULE_ROWS,
    MAX_RULES, MAX_SEQUENCE_STEPS, MAX_SEQUENCES, MAX_START_TICKS, MAX_WATCHED, NO_RULE, PAGE_TIMER, TIMERS, VARIABLES,
};
use crate::state::{PageState, SequenceState, TableState, TargetReport, TimerState};
use crate::table::{
    CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_VOICES, FIRED_CUE, FIRED_KEY, FIRED_P, FIRED_ROW, FIRED_TARGET,
    Fault, Index, NO_BINDING, NO_ROW, Node, Rect, Row, SEQUENCE_PASSES, SEQUENCE_POSITION, SEQUENCE_RUNNING,
    SEQUENCE_WAITING, SOUND_GAIN, SOUND_HANDLE, SOUND_LEVEL, SOUND_LOCAL, SOUND_PLAYING, SPACE_FIRED, SPACE_SEQUENCE,
    SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET, SPACE_TIMER, SPACE_VARIABLE, TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME,
    TARGET_LAYER, TARGET_LOOK, TARGET_ORDER, TARGET_SHOWN, TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_PHASE,
    TIMER_RUNNING, Table, Target, TargetKind, Trigger, Value,
};
use input::{InputState, Inside, Transition, keys_match};

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
/// state give it at the moment it is read (RULE-26, ANIM-6).
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

/// The sound on an audio channel at the moment it is read (RULE-27).
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
    /// One value and its reading as the state held it when the `REPORT`
    /// was applied.
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
    /// The page tick the firing belongs to.
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
/// The reading methods are called whenever a condition or a command
/// reads the page: the world is the page as it is at that moment, with
/// every command already given to it applied (DEC-6635 item 1).
/// [`World::command`] and [`World::report`] are called as the commands
/// run, in order.
pub trait World {
    /// The picture's width and height in pixels (RULE-13): 480 by 500 dots
    /// for a teletext page and for NAPLPS, whose every coordinate is a dot
    /// of the same teletext raster (neoteletext-common
    /// `docs/naplps-raster.md` RAS-5, DEC-6853), and 800 by 480 for ReGIS.
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
    /// applied at once by the code that applies that record from the
    /// server; `tick` is the page tick it belongs to. False when it did
    /// nothing useful, which is counted as skipped (RULE-118).
    fn command(&mut self, kind: u8, body: &[u8], tick: u32) -> bool;
    /// A report for the page's channel (RULE-22).
    fn report(&mut self, report: Report);
    /// Advance each cycling object whose frame is due at page tick `tick`
    /// (ANIM-14), at the end of the tick step.
    fn advance_cycles(&mut self, tick: u32);
    /// The answer to a state query (RULE-47), as the query is handled.
    fn page_state(&mut self, state: &PageState);
}

/// One event for the evaluator to handle (RULE-5, RULE-96).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event<'a> {
    /// An input event of the viewer's for the open page.
    Input(InputEvent),
    /// A release or a leave the receiver gives when the page loses the
    /// keyboard, the pointer leaves, the page is paused or it closes
    /// (RULE-8): handled at once whatever the budget, after any event that
    /// waits.
    Lost(InputEvent),
    /// A record the server sent directly that the evaluator carries out:
    /// a `VARIABLE`, `TIMER`, `REPORT`, `RUN` or `SEQUENCE` record's type
    /// and body (RULE-114). Its run has no firing.
    Command {
        /// The record type, 29 to 33.
        kind: u8,
        /// The record's body.
        body: &'a [u8],
    },
    /// A cue the sound helper reported heard (RULE-109).
    Cue {
        /// The audio channel, 0 to 3.
        channel: u8,
        /// The cue's number.
        number: u8,
    },
    /// The painting of the page's whole picture ([`Target::PAGE`]) or of
    /// one ReGIS numbered drawing ([`Target::object`]) finished (DEC-7031
    /// item 4, REQ-7026). The receiver gives it; no command causes it.
    Painted {
        /// What was painted.
        target: Target,
    },
    /// A state query (RULE-47), answered at once through
    /// [`World::page_state`].
    Query(u32),
}

/// An event as it waits in the queue, its body held.
#[derive(Clone, Copy)]
enum Queued {
    Input(InputEvent),
    Command { kind: u8, body: [u8; command::MAX_BODY_BYTES], length: u8 },
    Cue { channel: u8, number: u8 },
    Painted(Target),
    Query(u32),
}

/// The receiver's own counts for the open page (RULE-72). Each stops at
/// its greatest value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counters {
    /// Tick steps run.
    pub ticks: u32,
    /// Rules fired.
    pub fired: u32,
    /// Groups run, those a group runs among them.
    pub groups: u32,
    /// Commands applied.
    pub commands: u32,
    /// Commands, and operations of an `ANIMATE`, that did nothing for a
    /// missing target, sound, group or sequence (RULE-118), and records of
    /// the server's for another page or that their reader refuses.
    pub skipped: u32,
    /// Events dropped at a full queue (fault 4), and the events still
    /// waiting when the page closed.
    pub events_dropped: u32,
    /// Firings of one event beyond `MAX_RULE_ROWS`, when "any" names more
    /// than one control or object for its rows.
    pub firings_dropped: u32,
    /// Events handled.
    pub events_handled: u32,
    /// Events that waited for a later tick's budget (RULE-5).
    pub events_waited: u32,
    /// Sequence starts `at` a tick that had passed, begun at the next
    /// (RULE-103).
    pub starts_late: u32,
    /// Sequence starts `at` a tick more than `MAX_START_TICKS` ahead, not
    /// made, counted as skipped (RULE-103, fault 10).
    pub starts_skipped: u32,
    /// Cues handled (RULE-109).
    pub cues: u32,
    /// Paintings finished that were handled (DEC-7031 item 4).
    pub painted: u32,
    /// Reports made.
    pub reports: u32,
}

/// Where a sequence stands (RULE-103).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    Stopped,
    /// A start waits for its start tick.
    Waiting(u32),
    Running {
        position: u32,
        passes: u32,
    },
}

/// One sequence's state (RULE-103).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Playing {
    standing: Standing,
    /// The row parameter its steps' groups run with.
    parameter: Row,
    /// How many starts and stops have been applied to it: a tick step
    /// advances only a sequence no command of its own changed.
    changes: u32,
}

const STOPPED: Playing = Playing { standing: Standing::Stopped, parameter: NO_ROW, changes: 0 };

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
    parameter: Row,
    /// The input event's sequence number, in a run an input's rule made.
    sequence: Option<u32>,
}

impl Fired {
    /// A run made by no rule, with `parameter` held for it.
    const fn held(parameter: Row) -> Self {
        Self { rule: NO_RULE, row: NO_RULE, key: 0, target: Target::NONE, cue: 0, parameter, sequence: None }
    }

    /// A value of the firing (RULE-116).
    fn read(&self, property: u16) -> i32 {
        match property {
            FIRED_KEY => i32::try_from(self.key).unwrap_or(i32::MAX),
            FIRED_TARGET => i32::from(self.target.number),
            FIRED_ROW => i32::from(if self.row == NO_RULE { 0 } else { self.row }),
            FIRED_CUE => i32::from(self.cue),
            FIRED_P => self.parameter,
            _ => 0,
        }
    }
}

/// One firing a handling decided, and the group it runs.
#[derive(Clone, Copy)]
struct Firing {
    group: u8,
    fired: Fired,
}

const NO_FIRING: Firing = Firing { group: 0, fired: Fired::held(NO_ROW) };

/// One firing a tick step decided: the group, the rule and row (or
/// [`NO_RULE`] for a sequence's step) and the `p` in force. A tick step's
/// firings carry no key, target, cue or event (RULE-116).
#[derive(Clone, Copy)]
struct StepFiring {
    group: u8,
    rule: u8,
    row: u8,
    parameter: Row,
}

const NO_STEP_FIRING: StepFiring = StepFiring { group: 0, rule: NO_RULE, row: NO_RULE, parameter: NO_ROW };

impl StepFiring {
    const fn of(firing: &Firing) -> Self {
        Self { group: firing.group, rule: firing.fired.rule, row: firing.fired.row, parameter: firing.fired.parameter }
    }

    const fn firing(self) -> Firing {
        Firing { group: self.group, fired: Fired { rule: self.rule, row: self.row, ..Fired::held(self.parameter) } }
    }
}

/// The most firings one tick step decides: every rule and row once, and
/// every step of every sequence.
const MAX_STEP_FIRINGS: usize = MAX_RULE_ROWS + MAX_SEQUENCES * MAX_SEQUENCE_STEPS;

/// What a firing with no input event is of: the code of the key, the
/// target and the cue.
type Occurred = (u32, Target, u8);

/// A page's rule state and its handlings (RULE-5, DEC-6635).
#[derive(Clone)]
pub struct Evaluator {
    table: Table,
    index: Index,
    generation: u16,
    /// Each group's commands and `REPORT`s in one run, constants of the
    /// table (RULE-120), against the tick's budgets.
    run_costs: [(u16, u16); MAX_GROUPS],
    /// The present tick: that of the last tick step run, which an event
    /// belongs to.
    present: u32,
    /// Whether tick step 0 has been run: before it, events wait.
    stepped: bool,
    closed: bool,
    variables: [i32; VARIABLES],
    /// Each timer's start tick while it runs.
    timers: [Option<u32>; TIMERS],
    sequences: [Playing; MAX_SEQUENCES],
    /// Each `becomes` rule's result at its previous check, a bit for each
    /// of its rows (RULE-11).
    becomes: [u64; MAX_RULES],
    input: InputState,
    queue: [Queued; MAX_QUEUED_EVENTS],
    queue_head: usize,
    queue_length: usize,
    /// The present tick's budgets spent: events, commands, reports.
    spent: (usize, usize, usize),
    /// The firings a tick step decided.
    step_firings: [StepFiring; MAX_STEP_FIRINGS],
    metered: [u8; MAX_WATCHED],
    counters: Counters,
}

impl Evaluator {
    /// An evaluator for `table`, as the page opens (RULE-2): its sixteen
    /// variables 0, its timers and sequences stopped, tick step 0 next.
    /// `generation` is the table's, which a state query's answer names.
    /// The table is checked whole first ([`Table::check`]), so that one
    /// built by hand is held to what [`crate::decode`] holds an image to.
    ///
    /// # Errors
    /// What [`Table::check`] refuses. A decoded table is never refused.
    pub fn new(table: &Table, generation: u16, body_check: BodyCheck) -> Result<Self, Fault> {
        table.check(body_check)?;
        let index = Index::of(table)?;
        let mut run_costs = [(0, 0); MAX_GROUPS];
        for (cost, group) in run_costs.iter_mut().zip(0_u8..).take(table.groups.len()) {
            let commands = u16::try_from(table.run_commands(group)).unwrap_or(u16::MAX);
            *cost = (commands, u16::try_from(table.run_reports(group)).unwrap_or(u16::MAX));
        }
        Ok(Self {
            table: table.clone(),
            index,
            generation,
            run_costs,
            present: 0,
            stepped: false,
            closed: false,
            variables: [0; VARIABLES],
            timers: [None; TIMERS],
            sequences: [STOPPED; MAX_SEQUENCES],
            becomes: [0; MAX_RULES],
            input: InputState::new(),
            queue: [Queued::Query(0); MAX_QUEUED_EVENTS],
            queue_head: 0,
            queue_length: 0,
            spent: (0, 0, 0),
            step_firings: [NO_STEP_FIRING; MAX_STEP_FIRINGS],
            metered: [0; MAX_WATCHED],
            counters: Counters::default(),
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

    /// The present tick: that of the last tick step run, which an event
    /// handled now belongs to; 0 before tick step 0.
    #[must_use]
    pub const fn page_tick(&self) -> u32 {
        self.present
    }

    /// Whether tick step 0 has been run.
    #[must_use]
    pub const fn has_ticked(&self) -> bool {
        self.stepped
    }

    /// The receiver's counts (RULE-72).
    #[must_use]
    pub const fn counters(&self) -> &Counters {
        &self.counters
    }

    /// The sixteen variables as they stand (RULE-72).
    #[must_use]
    pub const fn variables(&self) -> &[i32; VARIABLES] {
        &self.variables
    }

    /// Whether the `close` handling has run ([`Evaluator::close`]).
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// How many events wait for a later tick (RULE-5).
    #[must_use]
    pub const fn waiting(&self) -> usize {
        self.queue_length
    }

    /// The tick a timer or a sequence started now first counts from: the
    /// first tick step after this handling (RULE-15, RULE-103).
    const fn next_tick(&self) -> u32 {
        if self.stepped { self.present.saturating_add(1) } else { 0 }
    }

    /// Gives the newest metered report (RULE-33 item 5): one byte for each
    /// slot of the watch list, in its order. A slot not given reads 0. It
    /// is a change to the world, made between handlings (RULE-7).
    pub fn set_metered(&mut self, values: &[u8]) {
        let mut given = values.iter().copied();
        for slot in &mut self.metered {
            *slot = given.next().unwrap_or(0);
        }
    }

    /// Handles one event (RULE-5, RULE-96): at once, when tick step 0 has
    /// run, nothing waits before it and the present tick's budgets allow
    /// its firings — `MAX_EVENTS_PER_TICK` events, `MAX_EVENT_COMMANDS_PER_TICK`
    /// commands and `MAX_EVENT_REPORTS_PER_TICK` reports, the first event
    /// of a tick always handled; every command of its firings is applied,
    /// and given to the world, before this returns. Otherwise it waits, in
    /// arrival order, in a queue of at most `MAX_QUEUED_EVENTS`, for the
    /// next tick step. [`Event::Lost`] is handled at once whatever the
    /// budget, after what waits. False when the event was dropped at a
    /// full queue, or a record of the server's was refused, which are
    /// counted.
    pub fn handle(&mut self, event: Event<'_>, world: &mut dyn World) -> bool {
        if self.closed {
            return false;
        }
        let queued = match event {
            Event::Lost(input) if self.stepped => {
                while self.queue_length > 0 {
                    self.handle_next_waiting(world, false);
                }
                return self.try_handle(&Queued::Input(input), world, false);
            }
            Event::Input(input) | Event::Lost(input) => Queued::Input(input),
            Event::Command { kind, body } => {
                let here = (self.table.page, self.table.sub);
                let ours = matches!(command::decode(kind, body), Ok((page, sub, _)) if (page, sub) == here);
                let length = u8::try_from(body.len()).ok().filter(|_| ours);
                let Some(length) = length.filter(|_| (TYPE_VARIABLE..=command::TYPE_SEQUENCE).contains(&kind)) else {
                    self.skip(1);
                    return false;
                };
                let mut held = [0_u8; command::MAX_BODY_BYTES];
                for (to, from) in held.iter_mut().zip(body) {
                    *to = *from;
                }
                Queued::Command { kind, body: held, length }
            }
            Event::Cue { channel, number } => Queued::Cue { channel, number },
            Event::Painted { target } => Queued::Painted(target),
            Event::Query(identifier) => Queued::Query(identifier),
        };
        if self.stepped && self.queue_length == 0 && self.try_handle(&queued, world, true) {
            return true;
        }
        self.wait(queued)
    }

    /// Puts an event at the queue's end; false when the queue is full and
    /// the event was dropped, which is counted (RULE-5).
    fn wait(&mut self, queued: Queued) -> bool {
        let place = self.queue_head.saturating_add(self.queue_length).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        let slot = if self.queue_length < MAX_QUEUED_EVENTS { self.queue.get_mut(place) } else { None };
        let Some(slot) = slot else {
            self.counters.events_dropped = self.counters.events_dropped.saturating_add(1);
            return false;
        };
        *slot = queued;
        self.queue_length = self.queue_length.saturating_add(1);
        self.counters.events_waited = self.counters.events_waited.saturating_add(1);
        true
    }

    /// Handles the event that has waited longest, if the budgets allow or
    /// `budgeted` is false; false when it must wait on.
    fn handle_next_waiting(&mut self, world: &mut dyn World, budgeted: bool) -> bool {
        let Some(queued) = self.queue.get(self.queue_head).copied().filter(|_| self.queue_length > 0) else {
            return false;
        };
        if !self.try_handle(&queued, world, budgeted) {
            return false;
        }
        self.queue_head = self.queue_head.saturating_add(1).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        self.queue_length = self.queue_length.saturating_sub(1);
        true
    }

    /// Decides one event's firings and, when the budgets allow (or are
    /// not asked), counts them against the present tick and applies them;
    /// false, with nothing of the decide phase kept, when they do not fit.
    fn try_handle(&mut self, queued: &Queued, world: &mut dyn World, budgeted: bool) -> bool {
        let before = self.input;
        let mut firings: Bounded<Firing, MAX_RULE_ROWS> = Bounded::new(NO_FIRING);
        let mut dropped = 0_u32;
        let mut keep = |firing: Firing| {
            if firings.push(firing).is_err() {
                dropped = dropped.saturating_add(1);
            }
        };
        let (mut commands, mut reports) = (0_usize, 0_usize);
        match *queued {
            Queued::Input(event) => self.decide_input(&event, &*world, &mut keep),
            Queued::Cue { channel, number } => self.decide_cue(channel, number, &*world, &mut keep),
            Queued::Painted(target) => self.decide_painted(target, &*world, &mut keep),
            Queued::Command { kind, body, length } => {
                let body = body.get(..usize::from(length)).unwrap_or(&[]);
                commands = 1;
                match command::decode(kind, body) {
                    Ok((_, _, Command::Run { group })) => {
                        commands = commands.saturating_add(usize::from(self.cost(group).0));
                        reports = usize::from(self.cost(group).1);
                    }
                    Ok((_, _, Command::Report { .. })) => reports = 1,
                    _ => {}
                }
            }
            Queued::Query(_) => {}
        }
        for firing in firings.iter() {
            let (group_commands, group_reports) = self.cost(firing.group);
            commands = commands.saturating_add(usize::from(group_commands));
            reports = reports.saturating_add(usize::from(group_reports));
        }
        let (events, spent_commands, spent_reports) = self.spent;
        let fits = events == 0
            || (events < MAX_EVENTS_PER_TICK
                && spent_commands.saturating_add(commands) <= MAX_EVENT_COMMANDS_PER_TICK
                && spent_reports.saturating_add(reports) <= MAX_EVENT_REPORTS_PER_TICK);
        if budgeted && !fits {
            self.input = before;
            return false;
        }
        self.spent =
            (events.saturating_add(1), spent_commands.saturating_add(commands), spent_reports.saturating_add(reports));
        self.counters.firings_dropped = self.counters.firings_dropped.saturating_add(dropped);
        self.counters.events_handled = self.counters.events_handled.saturating_add(1);
        match *queued {
            Queued::Command { kind, body, length } => {
                let body = body.get(..usize::from(length)).unwrap_or(&[]);
                self.counters.commands = self.counters.commands.saturating_add(1);
                self.apply_command(kind, body, &Fired::held(NO_ROW), world, 0);
            }
            Queued::Query(identifier) => {
                let state = self.page_state(identifier, &*world);
                world.page_state(&state);
            }
            Queued::Cue { .. } => self.counters.cues = self.counters.cues.saturating_add(1),
            Queued::Painted(_) => self.counters.painted = self.counters.painted.saturating_add(1),
            Queued::Input(_) => {}
        }
        for firing in firings.iter() {
            self.fire(firing, world);
        }
        true
    }

    /// The commands and the `REPORT`s one run of `group` applies.
    fn cost(&self, group: u8) -> (u16, u16) {
        self.run_costs.get(usize::from(group)).copied().unwrap_or((0, 0))
    }

    /// Runs the next tick step (RULE-5): it decides, in this order, the
    /// `open` rules at page tick 0, the timers' rules, the sequences'
    /// steps (a start whose tick has come begins) and the `becomes` rules,
    /// each rule or row once at most, against the state as it stands;
    /// then runs their groups in that order; then advances each sequence
    /// no command of the step changed, and the cycling of frames; and then
    /// handles, at once, the events that waited, under the new tick's
    /// budgets. After [`Evaluator::close`] it does nothing.
    pub fn tick(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        if self.stepped {
            self.present = self.present.saturating_add(1);
        } else {
            self.stepped = true;
        }
        self.spent = (0, 0, 0);
        let mut count = 0_usize;
        let mut fired_rows = [0_u64; MAX_RULES];
        if self.present == 0 {
            self.decide_rules(&*world, &mut fired_rows, &mut count, &|trigger, _| {
                (*trigger == Trigger::Open).then_some((0, Target::NONE, 0))
            });
        }
        self.decide_timers(&*world, &mut fired_rows, &mut count);
        let changes = self.decide_sequences(&mut count);
        self.decide_becomes(&*world, &mut fired_rows, &mut count);
        for place in 0..count {
            let Some(firing) = self.step_firings.get(place).map(|held| held.firing()) else { continue };
            self.fire(&firing, world);
        }
        self.advance_sequences(&changes);
        world.advance_cycles(self.present);
        self.counters.ticks = self.counters.ticks.saturating_add(1);
        while self.handle_next_waiting(world, true) {}
    }

    /// Runs the `close` handling (RULE-5, RULE-111): each `close` rule
    /// whose condition holds, in rule order. The receiver first gives a
    /// release for every key and button still down ([`Event::Lost`]). The
    /// events still waiting are dropped and counted. On a disconnect the
    /// receiver drops the evaluator and calls nothing.
    pub fn close(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        let dropped = u32::try_from(self.queue_length).unwrap_or(u32::MAX);
        self.counters.events_dropped = self.counters.events_dropped.saturating_add(dropped);
        self.queue_length = 0;
        let mut count = 0_usize;
        let mut fired_rows = [0_u64; MAX_RULES];
        self.decide_rules(&*world, &mut fired_rows, &mut count, &|trigger, _| {
            (*trigger == Trigger::Close).then_some((0, Target::NONE, 0))
        });
        for place in 0..count {
            let Some(firing) = self.step_firings.get(place).map(|held| held.firing()) else { continue };
            self.fire(&firing, world);
        }
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

    /// Reads one value as the page stands (RULE-7, RULE-23 to RULE-28,
    /// RULE-116, RULE-117): the variables, timers and sequences here, the
    /// input state as the events so far leave it (RULE-8), a value of the
    /// firing that of the event and row being matched or run, the metered
    /// values of the newest report, and every other value from the world.
    fn read(&self, value: Value, fired: &Fired, world: &dyn World) -> i32 {
        if value.space == SPACE_FIRED {
            return fired.read(u16::from(value.a));
        }
        if value.space == SPACE_TARGET && value.is_input_state() {
            return i32::from(value.named_target().is_some_and(|target| self.input.seen.inside.contains(target)));
        }
        if value.is_input_state() {
            return self.input.seen.read(value);
        }
        match value.space {
            SPACE_VARIABLE => self.variables.get(usize::from(value.a)).copied().unwrap_or(0),
            SPACE_TIMER => {
                let elapsed = self.elapsed(value.a);
                let reading = match value.b {
                    TIMER_RUNNING => u32::from(elapsed.is_some()),
                    TIMER_ELAPSED => elapsed.unwrap_or(0),
                    // RULE-25: a counter of the period, never a division
                    // the table computes; the receiver keeps it as it keeps
                    // `every`'s expiries.
                    TIMER_PHASE => elapsed.and_then(|elapsed| elapsed.checked_rem(value.period())).unwrap_or(0),
                    _ => 0,
                };
                i32::try_from(reading).unwrap_or(i32::MAX)
            }
            SPACE_TARGET => Self::read_target(value, world),
            SPACE_SOUND => {
                let sound = world.sound(value.a);
                match value.b {
                    SOUND_PLAYING => i32::from(sound.playing),
                    SOUND_LEVEL => self.read_metered(value),
                    SOUND_HANDLE if sound.playing => i32::from(sound.handle),
                    SOUND_HANDLE => -1,
                    SOUND_GAIN => i32::from(sound.gain),
                    SOUND_LOCAL => i32::from(sound.playing && sound.local),
                    _ => 0,
                }
            }
            SPACE_SYNTH => {
                let sound = world.sound(value.a);
                // RULE-28: another family's sound, or none, reads 0.
                if !sound.playing || sound.family != value.family() || sound.variant != value.variant() {
                    return 0;
                }
                if matches!(value.class(), CLASS_ENVELOPE_STAGE | CLASS_ENVELOPE_LEVEL | CLASS_VOICES) {
                    self.read_metered(value)
                } else {
                    world.synth(value.a, value.part(), value.class(), value.d)
                }
            }
            SPACE_SEQUENCE => {
                let standing = self.sequences.get(usize::from(value.a)).map_or(Standing::Stopped, |held| held.standing);
                let (running, passes, position, waiting) = match standing {
                    Standing::Stopped => (false, 0, 0, false),
                    Standing::Waiting(_) => (false, 0, 0, true),
                    Standing::Running { position, passes } => (true, passes, position, false),
                };
                let reading = match value.b {
                    SEQUENCE_RUNNING => u32::from(running),
                    SEQUENCE_PASSES => passes,
                    SEQUENCE_POSITION => position,
                    SEQUENCE_WAITING => u32::from(waiting),
                    _ => 0,
                };
                i32::try_from(reading).unwrap_or(i32::MAX)
            }
            _ => 0,
        }
    }

    /// A metered value's newest report (RULE-33); 0 before any.
    fn read_metered(&self, value: Value) -> i32 {
        let place = self.index.watch_place(&value);
        i32::from(place.and_then(|place| self.metered.get(place)).copied().unwrap_or(0))
    }

    /// A value of space 4 from the world (RULE-26).
    fn read_target(value: Value, world: &dyn World) -> i32 {
        let Some(state) = value.named_target().and_then(|target| world.target(target)) else { return 0 };
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

    /// A timer's start tick, or `None` while it is stopped. The page timer
    /// runs since tick 0.
    fn start_of(&self, timer: u8) -> Option<u32> {
        if timer == PAGE_TIMER { Some(0) } else { self.timers.get(usize::from(timer)).copied().flatten() }
    }

    /// The ticks from a timer's start to the present tick (RULE-15), or
    /// `None` while it is stopped or before its start tick.
    fn elapsed(&self, timer: u8) -> Option<u32> {
        self.start_of(timer).and_then(|start| self.present.checked_sub(start))
    }

    /// A target's bounding rectangle now (RULE-19), or `None` when it does
    /// not exist or is not shown.
    fn bounds_of(&self, target: Target, world: &dyn World) -> Option<Bounds> {
        let (width, height) = world.picture();
        match target.kind {
            TargetKind::Page => {
                Bounds { left: 0, top: 0, right: i32::from(width), bottom: i32::from(height) }.cut_to(width, height)
            }
            TargetKind::Rect => {
                let rect = self.table.rects.get(usize::from(target.number))?;
                Bounds::of_rect(rect).cut_to(width, height)
            }
            TargetKind::Control | TargetKind::Object => {
                let state = world.target(target)?;
                if state.shown { state.bounds.filter(|bounds| !bounds.is_empty()) } else { None }
            }
        }
    }

    /// Whether rule `rule`'s condition holds for a firing now (RULE-18);
    /// none always does.
    fn holds(&self, rule: usize, fired: &Fired, world: &dyn World) -> bool {
        let Some(nodes) = self.table.rules.get(rule).map(|rule| &rule.condition) else { return false };
        if nodes.is_empty() {
            return true;
        }
        let mut at = 0_usize;
        self.node_holds(nodes, &mut at, 0, fired, world)
    }

    /// Evaluates the node at `at` and its children, leaving `at` after
    /// them. Every child is evaluated: no node has an effect, and the
    /// place after the tree is then known.
    fn node_holds(&self, nodes: &[Node], at: &mut usize, depth: usize, fired: &Fired, world: &dyn World) -> bool {
        let place = *at;
        *at = place.saturating_add(1);
        // The decoder holds operators to their levels (RULE-18).
        let (Some(node), true) = (nodes.get(place), depth <= MAX_CONDITION_DEPTH) else { return false };
        let below = depth.saturating_add(1);
        match node {
            Node::And(children) => {
                let mut all = true;
                for _ in 0..*children {
                    all &= self.node_holds(nodes, at, below, fired, world);
                }
                all
            }
            Node::Or(children) => {
                let mut any = false;
                for _ in 0..*children {
                    any |= self.node_holds(nodes, at, below, fired, world);
                }
                any
            }
            Node::Not => !self.node_holds(nodes, at, below, fired, world),
            Node::Compare { value, operator, constant, plus_p } => {
                // RULE-105: the row's `p` is added with saturation.
                let added = if *plus_p { fired.parameter } else { 0 };
                operator.holds(self.read(*value, fired, world), constant.saturating_add(added))
            }
            Node::InRange { value, low, high } => (*low..=*high).contains(&self.read(*value, fired, world)),
            Node::InSet { value, members } => members.contains(&self.read(*value, fired, world)),
            Node::Overlaps(first, second) => match (self.bounds_of(*first, world), self.bounds_of(*second, world)) {
                (Some(first), Some(second)) => first.overlaps(&second),
                _ => false,
            },
            Node::Inside(first, second) => match (self.bounds_of(*first, world), self.bounds_of(*second, world)) {
                (Some(first), Some(second)) => first.within(&second),
                _ => false,
            },
        }
    }

    /// The firing of rule `rule`'s row at `place` for an event or a step.
    fn fired_of(&self, rule: usize, place: usize, (key, target, cue): Occurred, sequence: Option<u32>) -> Fired {
        let held = self.table.rules.get(rule);
        let named = held.is_some_and(|held| held.rows.is_some());
        Fired {
            rule: u8::try_from(rule).unwrap_or(NO_RULE),
            row: if named { u8::try_from(place).unwrap_or(NO_RULE) } else { NO_RULE },
            key,
            target,
            cue,
            parameter: held.map_or(NO_ROW, |held| self.table.row_of(held, place)),
            sequence,
        }
    }

    /// The decide phase of an input event (RULE-5): the input state
    /// follows it (RULE-8), and each rule in rule order and each of its
    /// rows in row order is matched and its condition read; `keep` is
    /// given each firing.
    fn decide_input(&mut self, event: &InputEvent, world: &dyn World, keep: &mut dyn FnMut(Firing)) {
        let transition = self.follow(&event.input, world);
        let key = match event.input {
            Input::Key { key, .. } => key.code(),
            _ => 0,
        };
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            let group = rule.group;
            for place in 0..rule.row_count() {
                let row = self.table.row_of(rule, place);
                // The page, or each control or object an "any" names: an
                // event fires a row at most once, by the first of the rule's
                // triggers it matches (RULE-107).
                let mut hits: Bounded<Target, { MAX_POINTER_OBJECTS + 1 }> = Bounded::new(Target::NONE);
                for trigger in rule.triggers.iter() {
                    self.matches(trigger, place, row, &event.input, &transition, &mut |target| {
                        let _ = hits.push(target);
                    });
                    if !hits.is_empty() {
                        break;
                    }
                }
                for target in hits.iter() {
                    let fired = self.fired_of(index, place, (key, *target, 0), Some(event.sequence));
                    if self.holds(index, &fired, world) {
                        keep(Firing { group, fired });
                    }
                }
            }
        }
    }

    /// The decide phase of a cue (RULE-109): each rule with a `cue` trigger
    /// it matches whose condition holds.
    fn decide_cue(&self, channel: u8, number: u8, world: &dyn World, keep: &mut dyn FnMut(Firing)) {
        let occurs = |trigger: &Trigger| match trigger {
            Trigger::Cue { channel: named, number: wanted }
                if *named == channel && (*wanted == ANY_CUE || *wanted == number) =>
            {
                Some((0, Target::NONE, number))
            }
            _ => None,
        };
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            let Some(occurred) = rule.triggers.iter().find_map(occurs) else { continue };
            for place in 0..rule.row_count() {
                let fired = self.fired_of(index, place, occurred, None);
                if self.holds(index, &fired, world) {
                    keep(Firing { group: rule.group, fired });
                }
            }
        }
    }

    /// The decide phase of a painting's end (DEC-7031 item 4): each rule,
    /// in rule order, and each of its rows, in row order, with a `painted`
    /// trigger that names `target` for that row, whose condition holds. A
    /// target written as a constant names it for every row of its rule; one
    /// given by the row's `p`, for the rows whose `p` gives its number. The
    /// firing's target is the event's.
    fn decide_painted(&self, target: Target, world: &dyn World, keep: &mut dyn FnMut(Firing)) {
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            for place in 0..rule.row_count() {
                let row = self.table.row_of(rule, place);
                let names = |trigger: &Trigger| {
                    trigger.painted().and_then(|named| named.resolve(&row)).is_some_and(|named| named == target)
                };
                if !rule.triggers.iter().any(names) {
                    continue;
                }
                let fired = self.fired_of(index, place, (0, target, 0), None);
                if self.holds(index, &fired, world) {
                    keep(Firing { group: rule.group, fired });
                }
            }
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
        match *input {
            Input::Key { action, key, base, .. } => {
                self.input.key(action, key, base);
                Transition::NONE
            }
            Input::Pointer { action: PointerAction::Gone, .. } => self.input.gone(),
            Input::Pointer { action, button, x, y, control } => {
                let now = at(control, x, y, &self.table.rects);
                self.input.pointer(action, button, now, (x, y))
            }
            Input::Wheel { x, y, control } => {
                let now = at(control, x, y, &self.table.rects);
                // A wheel event moves nothing and presses nothing, but it
                // says where the pointer is.
                self.input.pointer(PointerAction::Gone, ANY_BUTTON, now, (x, y))
            }
            Input::Text | Input::Focus | Input::Blur => Transition::NONE,
        }
    }

    /// Gives `hit` each target for which `trigger` matches `input` for the
    /// row at `place` among the rule's rows, whose `p` is `row` (RULE-9):
    /// [`Target::NONE`] for a trigger that names none.
    fn matches(
        &self,
        trigger: &Trigger,
        place: usize,
        row: Row,
        input: &Input,
        transition: &Transition,
        hit: &mut dyn FnMut(Target),
    ) {
        let inside = &self.input.seen.inside;
        let button_named = |named: u8, event: u8| named == ANY_BUTTON || named == event;
        match (trigger, input) {
            (Trigger::Key { keys, repeat }, Input::Key { action, key, base, mods }) => {
                let pressed = *action == KeyAction::Press || (*action == KeyAction::Repeat && *repeat);
                if pressed && keys_match(keys, place, *key, *base, *mods) {
                    hit(Target::NONE);
                }
            }
            (Trigger::KeyUp { keys }, Input::Key { action: KeyAction::Release, key, base, mods }) => {
                if keys_match(keys, place, *key, *base, *mods) {
                    hit(Target::NONE);
                }
            }
            (Trigger::Text, Input::Text) | (Trigger::Focus, Input::Focus) | (Trigger::Blur, Input::Blur) => {
                hit(Target::NONE);
            }
            (Trigger::Press { target, button }, Input::Pointer { action: PointerAction::Press, button: down, .. })
                if button_named(*button, *down) =>
            {
                if let Some(target) = target.resolve(&row) {
                    inside.matching(target, hit);
                }
            }
            (
                Trigger::Release { target, button },
                Input::Pointer { action: PointerAction::Release, button: up, .. },
            ) if button_named(*button, *up) => {
                if let Some(target) = target.resolve(&row) {
                    transition.released.matching(target, hit);
                }
            }
            (Trigger::Enter { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(&row) {
                    transition.entered.matching(target, hit);
                }
            }
            (Trigger::Leave { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(&row) {
                    transition.left.matching(target, hit);
                }
            }
            (Trigger::Move { target, held }, Input::Pointer { action: PointerAction::Move, button: mask, .. })
                if transition.moved && (!held || *mask != 0 || self.input.seen.buttons != 0) =>
            {
                if let Some(target) = target.resolve(&row) {
                    inside.matching(target, hit);
                }
            }
            (Trigger::Wheel { target }, Input::Wheel { .. }) => {
                if let Some(target) = target.resolve(&row) {
                    inside.matching(target, hit);
                }
            }
            _ => {}
        }
    }

    /// Keeps one firing of the tick step, when there is room.
    fn keep_step(&mut self, count: &mut usize, firing: Firing) {
        if let Some(slot) = self.step_firings.get_mut(*count) {
            *slot = StepFiring::of(&firing);
            *count = count.saturating_add(1);
        }
    }

    /// Decides, in rule order and row order, each row for which `occurs`
    /// gives a firing and the rule's condition holds, and that has not
    /// fired in this tick step (RULE-5: once a tick step at most).
    fn decide_rules(
        &mut self,
        world: &dyn World,
        fired_rows: &mut [u64; MAX_RULES],
        count: &mut usize,
        occurs: &dyn Fn(&Trigger, &Row) -> Option<Occurred>,
    ) {
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            let group = rule.group;
            for place in 0..rule.row_count() {
                let Some(rule) = self.table.rules.get(index) else { continue };
                let row = self.table.row_of(rule, place);
                let bit = u32::try_from(place).ok().and_then(|place| 1_u64.checked_shl(place)).unwrap_or(0);
                if fired_rows.get(index).is_some_and(|bits| bits & bit != 0) {
                    continue;
                }
                let Some(occurred) = rule.triggers.iter().find_map(|trigger| occurs(trigger, &row)) else {
                    continue;
                };
                let fired = self.fired_of(index, place, occurred, None);
                if self.holds(index, &fired, world) {
                    if let Some(bits) = fired_rows.get_mut(index) {
                        *bits |= bit;
                    }
                    self.keep_step(count, Firing { group, fired });
                }
            }
        }
    }

    /// The timers' rules (RULE-15), the page timer first, then `t0` to
    /// `t7`: each rule whose `after` or `every` on a running timer is due
    /// at the present tick.
    fn decide_timers(&mut self, world: &dyn World, fired_rows: &mut [u64; MAX_RULES], count: &mut usize) {
        for timer in core::iter::once(PAGE_TIMER).chain((0_u8..).take(TIMERS)) {
            let Some(elapsed) = self.elapsed(timer) else { continue };
            self.decide_rules(world, fired_rows, count, &|trigger, _| {
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

    /// The sequences (RULE-103), in number order: a start whose tick has
    /// come begins at position 0, pass 0; then each step of a running
    /// sequence whose offset equals its position is decided. Gives each
    /// sequence's count of changes as the step decided it, by which the
    /// step's own starts and stops are told apart.
    fn decide_sequences(&mut self, count: &mut usize) -> [Option<u32>; MAX_SEQUENCES] {
        let mut running = [None; MAX_SEQUENCES];
        for number in 0..self.table.sequences.len() {
            let Some(held) = self.sequences.get_mut(number) else { continue };
            if held.standing == Standing::Waiting(self.present) {
                held.standing = Standing::Running { position: 0, passes: 0 };
            }
            let Playing { standing: Standing::Running { position, .. }, parameter, changes } = *held else { continue };
            if let Some(slot) = running.get_mut(number) {
                *slot = Some(changes);
            }
            let steps = self.table.sequences.get(number).map_or(0, |sequence| sequence.steps.len());
            for place in 0..steps {
                let step = self.table.sequences.get(number).and_then(|sequence| sequence.steps.get(place)).copied();
                if let Some(step) = step.filter(|step| step.offset == position) {
                    self.keep_step(count, Firing { group: step.group, fired: Fired::held(parameter) });
                }
            }
        }
        running
    }

    /// Each `becomes` rule, and each of its rows, whose condition holds
    /// now and did not at its previous check (RULE-11), checked once a
    /// tick step.
    fn decide_becomes(&mut self, world: &dyn World, fired_rows: &mut [u64; MAX_RULES], count: &mut usize) {
        for index in 0..self.table.rules.len() {
            let Some(rule) = self.table.rules.get(index) else { continue };
            if !rule.triggers.contains(&Trigger::Becomes) {
                continue;
            }
            let group = rule.group;
            for place in 0..rule.row_count() {
                let fired = self.fired_of(index, place, (0, Target::NONE, 0), None);
                let holds = self.holds(index, &fired, world);
                let bit = u32::try_from(place).ok().and_then(|place| 1_u64.checked_shl(place)).unwrap_or(0);
                let Some(bits) = self.becomes.get_mut(index) else { continue };
                let held = *bits & bit != 0;
                if holds {
                    *bits |= bit;
                } else {
                    *bits &= !bit;
                }
                let once = fired_rows.get(index).is_some_and(|fired| fired & bit != 0);
                if holds && !held && !once {
                    if let Some(fired_bits) = fired_rows.get_mut(index) {
                        *fired_bits |= bit;
                    }
                    self.keep_step(count, Firing { group, fired });
                }
            }
        }
    }

    /// After a tick step's groups: each sequence that was running when it
    /// decided, and that no command of the step started or stopped,
    /// advances; a pass ends at its length, and the sequence ends at its
    /// repeat count (RULE-103).
    fn advance_sequences(&mut self, running: &[Option<u32>; MAX_SEQUENCES]) {
        for (number, decided) in running.iter().enumerate() {
            let Some(decided) = decided else { continue };
            let Some((length, repeat)) =
                self.table.sequences.get(number).map(|sequence| (sequence.length, sequence.repeat))
            else {
                continue;
            };
            let Some(held) = self.sequences.get_mut(number).filter(|held| held.changes == *decided) else { continue };
            let Standing::Running { position, passes } = held.standing else { continue };
            let mut position = position.saturating_add(1);
            let mut passes = passes;
            if position >= length {
                position = 0;
                passes = passes.saturating_add(1);
            }
            held.standing = if repeat != 0 && passes >= u32::from(repeat) {
                Standing::Stopped
            } else {
                Standing::Running { position, passes }
            };
        }
    }

    /// Runs the group of a firing (RULE-118).
    fn fire(&mut self, firing: &Firing, world: &mut dyn World) {
        if firing.fired.rule != NO_RULE {
            self.counters.fired = self.counters.fired.saturating_add(1);
        }
        self.run_group(firing.group, &firing.fired, world, 0);
    }

    /// Counts commands that did nothing (RULE-118).
    fn skip(&mut self, count: usize) {
        self.counters.skipped = self.counters.skipped.saturating_add(u32::try_from(count).unwrap_or(u32::MAX));
    }

    /// The value of a binding's source (RULE-106): a variable's value as it
    /// stands, the row's `p` in force, or a value of the firing.
    fn source(&self, source: Source, fired: &Fired) -> i32 {
        match source {
            Source::Variable(variable) => self.variables.get(usize::from(variable)).copied().unwrap_or(0),
            Source::Parameter => fired.parameter,
            Source::Fired(property) => fired.read(u16::from(property)),
        }
    }

    /// Runs group `group` (RULE-118): its commands in written order, each
    /// with its amounts resolved first and applied at once, a `RUN`
    /// replaced by the run of its group (RULE-102). `depth` is how many
    /// groups are being run above this one; the table's check holds it
    /// below `MAX_GROUP_DEPTH`, and so does this.
    fn run_group(&mut self, group: u8, fired: &Fired, world: &mut dyn World, depth: usize) {
        let Some((mut at, count)) = self.table.groups.start(usize::from(group)).filter(|_| depth < MAX_GROUP_DEPTH)
        else {
            self.skip(1);
            return;
        };
        self.counters.groups = self.counters.groups.saturating_add(1);
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
            let resolved =
                command::apply_bindings(kind, command.body, bindings, &|source| self.source(source, fired), &mut body);
            self.counters.commands = self.counters.commands.saturating_add(1);
            self.skip(resolved.skipped);
            let Some(body) = body.get(..resolved.length).filter(|body| !body.is_empty()) else { continue };
            self.apply_command(kind, body, fired, world, depth);
        }
    }

    /// Applies one command at once (RULE-96, RULE-118): the evaluator's
    /// own (`VARIABLE`, `TIMER`, `REPORT`, `RUN`, `SEQUENCE`) itself, every
    /// other through the world.
    fn apply_command(&mut self, kind: u8, body: &[u8], fired: &Fired, world: &mut dyn World, depth: usize) {
        if !(TYPE_VARIABLE..=command::TYPE_SEQUENCE).contains(&kind) {
            if !world.command(kind, body, self.present) {
                self.skip(1);
            }
            return;
        }
        let Ok((_, _, command)) = command::decode(kind, body) else {
            self.skip(1);
            return;
        };
        match command {
            Command::Variable { operation, variable, operand } => {
                self.apply_variable(operation, variable, operand, fired, &*world);
            }
            Command::Run { group } => self.run_group(group, fired, world, depth.saturating_add(1)),
            // `LOCAL_MIDI` is the world's, and never reaches here.
            Command::LocalMidi { .. } => {}
            Command::Timer { start, timer } => {
                let since = self.next_tick();
                if let Some(slot) = self.timers.get_mut(usize::from(timer)) {
                    *slot = start.then_some(since);
                }
            }
            Command::Sequence { start, sequence, mode } => self.apply_sequence(start, sequence, mode, fired),
            Command::Report { value } => {
                let what = match (value, fired.sequence) {
                    (Some(value), _) => Reported::Value { value, reading: self.read(value, fired, &*world) },
                    (None, Some(sequence)) => Reported::Input { sequence },
                    (None, None) => Reported::Fired,
                };
                // RULE-46: only a report of an input event names a target.
                let target = if matches!(what, Reported::Input { .. }) { fired.target } else { Target::NONE };
                self.counters.reports = self.counters.reports.saturating_add(1);
                world.report(Report { rule: fired.rule, row: fired.row, tick: self.present, target, what });
            }
        }
    }

    /// Applies a `VARIABLE` command (RULE-99). Every sum saturates at the
    /// 32-bit bounds.
    fn apply_variable(
        &mut self,
        operation: VariableOperation,
        variable: u8,
        operand: Operand,
        fired: &Fired,
        world: &dyn World,
    ) {
        let given = match operand {
            Operand::None | Operand::Range { .. } => 0,
            Operand::Constant(constant) => constant,
            Operand::Variable { constant, variable } => {
                constant.saturating_add(self.variables.get(usize::from(variable)).copied().unwrap_or(0))
            }
            Operand::Fired { constant, property } => constant.saturating_add(fired.read(u16::from(property))),
            Operand::Value(value) => self.read(value, fired, world),
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
        if variable == ALL_VARIABLES {
            for slot in &mut self.variables {
                *slot = change(*slot);
            }
        } else if let Some(slot) = self.variables.get_mut(usize::from(variable)) {
            *slot = change(*slot);
        }
    }

    /// Starts or stops a sequence (RULE-103). A start replaces any run or
    /// waiting start of it, holds the `p` in force, and waits for its start
    /// tick, set by its mode; a stop stops it, or cancels a waiting start.
    fn apply_sequence(&mut self, start: bool, sequence: u8, mode: StartMode, fired: &Fired) {
        let count = self.table.sequences.len();
        if sequence == ALL_SEQUENCES {
            for held in &mut self.sequences {
                held.standing = Standing::Stopped;
                held.changes = held.changes.wrapping_add(1);
            }
            return;
        }
        let next = self.next_tick();
        let begins = match mode {
            StartMode::Next => next,
            StartMode::At(tick) if tick < next => {
                self.counters.starts_late = self.counters.starts_late.saturating_add(1);
                next
            }
            StartMode::At(tick) if tick.saturating_sub(self.present) > MAX_START_TICKS => {
                self.counters.starts_skipped = self.counters.starts_skipped.saturating_add(1);
                return;
            }
            StartMode::At(tick) => tick,
            StartMode::Beat { timer, ticks } => self.start_of(timer).map_or(next, |since| {
                // The first expiry from the next tick on: `since` plus a
                // whole number of periods, at least one (RULE-15).
                let wanted = next.saturating_sub(since).max(1);
                let periods = wanted.div_ceil(ticks.max(1));
                since.saturating_add(periods.saturating_mul(ticks))
            }),
        };
        let Some(held) = self.sequences.get_mut(usize::from(sequence)).filter(|_| usize::from(sequence) < count) else {
            self.skip(1);
            return;
        };
        held.standing = if start { Standing::Waiting(begins) } else { Standing::Stopped };
        if start {
            held.parameter = fired.parameter;
        }
        held.changes = held.changes.wrapping_add(1);
    }

    /// The page's state as it stands (RULE-47).
    fn page_state(&self, identifier: u32, world: &dyn World) -> PageState {
        let mut state = TableState::new(self.generation, self.present);
        state.variables = self.variables;
        for (timer, slot) in (0_u8..).zip(state.timers.iter_mut()) {
            let elapsed = self.elapsed(timer);
            *slot = TimerState { running: elapsed.is_some(), elapsed: elapsed.unwrap_or(0) };
        }
        for held in self.sequences.iter().take(self.table.sequences.len()) {
            let report = match held.standing {
                Standing::Stopped => SequenceState::default(),
                Standing::Waiting(_) => SequenceState { waiting: true, ..SequenceState::default() },
                Standing::Running { position, passes } => {
                    SequenceState { running: true, passes, position, waiting: false }
                }
            };
            let _ = state.sequences.push(report);
        }
        for target in self.index.targets.iter() {
            let held = world.target(*target);
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
        for value in self.index.values.iter() {
            let _ = state.values.push((*value, self.read(*value, &nothing, world)));
        }
        PageState { identifier, table: Some(state) }
    }
}

// RULE-120 with RULE-102: a group run in place is a `RUN` record, whose
// type is the evaluator's own.
const _: () = assert!(command::TYPE_RUN > TYPE_VARIABLE && command::TYPE_RUN < command::TYPE_SEQUENCE);
// RULE-8: a button is one of the three.
const _: () = assert!(MAX_BUTTON < u8::BITS as u8);
// RULE-5: a run's commands and reports are counted in 16 bits.
const _: () = assert!(crate::limits::MAX_RUN_COMMANDS <= u16::MAX as usize);

#[cfg(test)]
mod tests;
