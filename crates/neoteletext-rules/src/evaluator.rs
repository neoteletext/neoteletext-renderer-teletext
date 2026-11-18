//! The evaluator (RULE-4 to RULE-8, RULE-11, RULE-14 to RULE-17): a
//! page's rule state and its tick, in the fixed order of RULE-5.
//!
//! The receiver gives it the viewer's input events ([`Evaluator::push`] or
//! [`Evaluator::push_now`]), the metered values
//! ([`Evaluator::set_metered`]) and, each tick, a [`World`]: what the
//! evaluator reads of the page beyond its own state, and where its outputs
//! go. Nothing an action does is seen in the tick that runs it (RULE-6):
//! the world is read once for a tick, when its snapshot is taken, and every
//! output is kept until the tick ends, with one exception.
//!
//! The exception is evaluation on arrival (RULE-96, DEC-6580): an input
//! event given by [`Evaluator::push_now`] is handled at once as an event of
//! the next tick, and the sound operations of the rules it fires are given
//! to the world at once, so that a key's note does not wait for the tick.
//! The snapshot, the order, the caps and every other output are the
//! tick's, so both ways of giving an event have the same firings, reports,
//! counts and state at every tick.

mod input;

use crate::animate::Operation;
use crate::bounded::Bounded;
use crate::limits::{
    AUDIO_CHANNELS, MAX_ACTIONS, MAX_ACTIONS_PER_TICK, MAX_CONDITION_DEPTH, MAX_CONDITION_NODES, MAX_EVENTS_PER_TICK,
    MAX_LAYER, MAX_POINTER_OBJECTS, MAX_QUEUED_EVENTS, MAX_RECTS, MAX_RULES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES,
    MAX_TIMERS, MAX_VARIABLES, MAX_WATCHED, PAGE_TIMER,
};
use crate::state::{PageState, TableState, TargetReport, TimerState};
use crate::table::{
    Action, CLASS_ENVELOPE_LEVEL, CLASS_ENVELOPE_STAGE, CLASS_VOICES, Fault, Index, Node, Rect, SOUND_GAIN,
    SOUND_HANDLE, SOUND_LEVEL, SOUND_LOCAL, SOUND_PLAYING, SPACE_SOUND, SPACE_SYNTH, SPACE_TARGET, SPACE_TIMER,
    SPACE_VARIABLE, TARGET_CYCLING, TARGET_EXISTS, TARGET_FRAME, TARGET_LAYER, TARGET_LOOK, TARGET_ORDER, TARGET_SHOWN,
    TARGET_X, TARGET_Y, TIMER_ELAPSED, TIMER_RUNNING, Table, Target, TargetKind, Trigger, Value,
};
use input::{InputState, Inside, Transition, key_names};

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
/// state give it when a tick begins (RULE-26, ANIM-6).
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

/// The sound on an audio channel when a tick begins (RULE-27).
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

/// A sound operation an action asks for (RULE-20).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundOperation {
    /// One MIDI channel voice message for the local sound on an audio
    /// channel (RULE-21).
    Midi {
        /// The audio channel.
        channel: u8,
        /// The status byte.
        status: u8,
        /// The first data byte.
        data1: u8,
        /// The second; not given to the instrument for a program change.
        data2: u8,
    },
    /// Start a sound the page already has.
    Play {
        /// The audio channel.
        channel: u8,
        /// The sound's handle.
        handle: u8,
        /// The sound's generation.
        generation: u16,
        /// 0 to 100.
        gain: u8,
        /// Milliseconds; below the audio profile's least it is played as
        /// the least (RULE-45).
        fade_ms: u16,
        /// Whether it loops.
        looped: bool,
    },
    /// Ramp an audio channel to silence and stop it.
    Stop {
        /// The audio channel.
        channel: u8,
        /// Milliseconds.
        fade_ms: u16,
    },
    /// Ramp an audio channel to a gain.
    Gain {
        /// The audio channel.
        channel: u8,
        /// 0 to 100.
        gain: u8,
        /// Milliseconds.
        fade_ms: u16,
    },
}

/// What a report says (RULE-22, RULE-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reported {
    /// The rule fired, with no event: `open`, `close`, `becomes`, `every`
    /// and `after`.
    Fired,
    /// The rule fired on the input event its giver numbered `sequence`.
    Input {
        /// The event's sequence number, as it was given.
        sequence: u32,
    },
    /// One value as the snapshot holds it.
    Value {
        /// The value.
        value: Value,
        /// Its reading.
        reading: i32,
    },
}

/// One `send` or `echo` (RULE-22): what the receiver reports to the
/// page's channel as a `RULE_EVENT` (RULE-46).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Report {
    /// The rule that sent it, 0 to 63.
    pub rule: u8,
    /// The page tick at which the rule fired.
    pub tick: u32,
    /// The control, object or rectangle the pointer was inside for a rule
    /// whose pointer trigger named one; else [`Target::NONE`].
    pub target: Target,
    /// What is reported.
    pub what: Reported,
}

/// What the evaluator reads of the page, and where its outputs go
/// (RULE-56).
///
/// The reading methods are called when a tick's snapshot is taken (as the
/// tick begins, or when the first event of its interval is given by
/// [`Evaluator::push_now`], RULE-96) and while its input events are
/// followed; the output methods only when the tick ends, in the order the
/// actions ran (RULE-5 step 7), but for `sound_operation`, which
/// [`Evaluator::push_now`] also calls on an event's arrival. An output
/// method answers whether the operation did anything: `false` counts it as
/// skipped (RULE-20).
pub trait World {
    /// The picture's width and height in pixels (RULE-13): 480 by 500 for
    /// a teletext page, 800 by 480 for ReGIS, 640 by 480 for NAPLPS.
    fn picture(&self) -> (u16, u16);
    /// The state of one control or object on the sub-page shown, or
    /// `None` when it does not exist there (RULE-12).
    fn target(&self, target: Target) -> Option<TargetState>;
    /// The objects a pixel is inside (RULE-12), written to the front of
    /// `objects`; the count written.
    fn objects_at(&self, x: u16, y: u16, objects: &mut [u16; MAX_POINTER_OBJECTS]) -> usize;
    /// The sound on an audio channel, 0 to 3 (RULE-27).
    fn sound(&self, channel: u8) -> SoundState;
    /// A tracked value of the synthesizer sound on an audio channel
    /// (RULE-28, RULE-30): classes 0 to 4, 8 and 9, read from the family's
    /// tracker. It is asked only while a sound of the family and variant
    /// the rule expects plays there.
    fn synth(&self, channel: u8, part: u8, class: u8, index: u16) -> i32;
    /// A sound operation (RULE-20).
    fn sound_operation(&mut self, operation: SoundOperation) -> bool;
    /// An animation operation (ANIM-6), run in page tick `tick`.
    fn animate(&mut self, operation: Operation, tick: u32) -> bool;
    /// A report for the page's channel (RULE-22).
    fn report(&mut self, report: Report);
    /// Advance each cycling object whose frame is due at page tick `tick`
    /// (RULE-5 step 6, ANIM-14).
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
    /// Actions run, an `echo` among them.
    pub actions: u32,
    /// Actions skipped for a missing target or sound (RULE-20).
    pub skipped: u32,
    /// Input events dropped from a full queue (RULE-5).
    pub events_dropped: u32,
    /// Reports asked for.
    pub sends: u32,
    /// Rule firings dropped whole by the cap on a tick's actions (RULE-6).
    pub overruns: u32,
}

/// The rule state actions write (RULE-7): two copies are kept, the
/// snapshot and the working state.
#[derive(Clone, Copy)]
struct RuleState {
    variables: [i32; MAX_VARIABLES],
    /// Each timer's start tick while it runs.
    timers: [Option<u32>; MAX_TIMERS],
    /// The rules enabled: bit `n` for rule `n`.
    enabled: u64,
}

/// One output, kept until its tick ends (RULE-5 step 7).
#[derive(Clone, Copy)]
enum Output {
    Sound(SoundOperation),
    Animation(Operation),
    Report(Report),
}

/// What made a rule fire, for its reports (RULE-22).
#[derive(Clone, Copy)]
struct Cause {
    /// The input event's sequence number, for an input trigger.
    sequence: Option<u32>,
    /// The target the pointer was inside.
    target: Target,
}

impl Cause {
    /// A firing with no event.
    const NONE: Self = Self { sequence: None, target: Target::NONE };
}

/// A place in a list of 64 or fewer, kept in a byte.
const NO_SLOT: u8 = u8::MAX;
const _: () = assert!(MAX_TABLE_VALUES < NO_SLOT as usize);

/// Bit `index` of a rule mask.
fn rule_bit(index: usize) -> u64 {
    u32::try_from(index).ok().and_then(|index| 1_u64.checked_shl(index)).unwrap_or(0)
}

/// A page's rule state and its tick (RULE-5, RULE-7).
#[derive(Clone)]
pub struct Evaluator {
    table: Table,
    index: Index,
    generation: u16,
    /// Where each condition node's value is in the index, by rule and
    /// node; [`NO_SLOT`] for a node that reads none.
    node_slots: [[u8; MAX_CONDITION_NODES]; MAX_RULES],
    /// Where each `send` action's value is in the index.
    send_slots: [[u8; MAX_ACTIONS]; MAX_RULES],
    /// The page tick of the next evaluation.
    tick: u32,
    closed: bool,
    /// The rules switched off for the sub-page shown (RULE-17), as the
    /// snapshot of the tick being evaluated took them.
    off: u64,
    /// The same for the sub-page shown now, which the next snapshot takes
    /// (RULE-7): a change of sub-page after a snapshot is seen by the
    /// next tick, like every other change to the world.
    off_shown: u64,
    /// Whether the snapshot of tick `tick` is taken (RULE-96): by the
    /// first event of its interval given with [`Evaluator::push_now`].
    opened: bool,
    /// The input events of tick `tick` already handled on arrival
    /// (RULE-96), which count against `MAX_EVENTS_PER_TICK` and, until
    /// their tick ends, against `MAX_QUEUED_EVENTS`.
    handled: usize,
    snapshot: RuleState,
    working: RuleState,
    /// Each `becomes` rule's result at its previous evaluation (RULE-11).
    becomes: u64,
    input: InputState,
    queue: [InputEvent; MAX_QUEUED_EVENTS],
    queue_head: usize,
    queue_length: usize,
    metered: [u8; MAX_WATCHED],
    /// The snapshot's reading of each value of the index that is not
    /// input state.
    readings: [i32; MAX_TABLE_VALUES],
    /// The snapshot's state of each target of the index.
    targets: [Option<TargetState>; MAX_TABLE_TARGETS],
    sounds: [SoundState; AUDIO_CHANNELS],
    picture: (u16, u16),
    outputs: [Output; MAX_ACTIONS_PER_TICK],
    output_count: usize,
    /// The actions run in the tick being evaluated.
    actions_run: usize,
    counters: Counters,
    query: Option<u32>,
}

impl Evaluator {
    /// An evaluator for `table`, as the page opens (RULE-2): its variables
    /// at their initial values, its timers and rules as declared, page
    /// tick 0 next. `generation` is the table's, which a state query's
    /// answer names. `sub` is the sub-code of the sub-page shown.
    ///
    /// # Errors
    /// [`Fault::Limit`] and [`Fault::Tree`] for a table built by hand that
    /// [`crate::table::decode`] would refuse for what it reads, names or
    /// nests. A decoded table is never refused.
    pub fn new(table: &Table, generation: u16, sub: u16) -> Result<Self, Fault> {
        let index = Index::of(table)?;
        let mut node_slots = [[NO_SLOT; MAX_CONDITION_NODES]; MAX_RULES];
        let mut send_slots = [[NO_SLOT; MAX_ACTIONS]; MAX_RULES];
        let slot_of = |value: &Value| index.value_slot(value).and_then(|slot| u8::try_from(slot).ok());
        for ((rule, nodes), sends) in table.rules.iter().zip(node_slots.iter_mut()).zip(send_slots.iter_mut()) {
            crate::table::check_condition(&rule.condition)?;
            for (node, slot) in rule.condition.iter().zip(nodes.iter_mut()) {
                *slot = node.value().and_then(|value| slot_of(&value)).unwrap_or(NO_SLOT);
            }
            for (action, slot) in rule.actions.iter().zip(sends.iter_mut()) {
                if let Action::Send(Some(value)) = action {
                    *slot = slot_of(value).unwrap_or(NO_SLOT);
                }
            }
        }
        let mut state = RuleState {
            variables: [0; MAX_VARIABLES],
            timers: [None; MAX_TIMERS],
            enabled: table.enabled_at_opening(),
        };
        for (variable, slot) in table.variables.iter().zip(state.variables.iter_mut()) {
            *slot = variable.initial;
        }
        for (runs, slot) in table.timers.iter().zip(state.timers.iter_mut()) {
            // RULE-15: a timer that starts at opening runs since tick 0.
            *slot = runs.then_some(0);
        }
        let no_event = InputEvent { sequence: 0, input: Input::Text };
        let no_output = Output::Report(Report { rule: 0, tick: 0, target: Target::NONE, what: Reported::Fired });
        Ok(Self {
            table: *table,
            index,
            generation,
            node_slots,
            send_slots,
            tick: 0,
            closed: false,
            off: table.off_for(sub),
            off_shown: table.off_for(sub),
            opened: false,
            handled: 0,
            snapshot: state,
            working: state,
            becomes: 0,
            input: InputState::new(),
            queue: [no_event; MAX_QUEUED_EVENTS],
            queue_head: 0,
            queue_length: 0,
            metered: [0; MAX_WATCHED],
            readings: [0; MAX_TABLE_VALUES],
            targets: [None; MAX_TABLE_TARGETS],
            sounds: [SoundState::default(); AUDIO_CHANNELS],
            picture: (0, 0),
            outputs: [no_output; MAX_ACTIONS_PER_TICK],
            output_count: 0,
            actions_run: 0,
            counters: Counters::default(),
            query: None,
        })
    }

    /// The table evaluated.
    #[must_use]
    pub const fn table(&self) -> &Table {
        &self.table
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

    /// Whether the final tick has run ([`Evaluator::close`]).
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// The sub-page shown changed (RULE-17): the rules its list switches
    /// off are off from the next tick, and nothing else changes.
    pub fn set_subpage(&mut self, sub: u16) {
        self.off_shown = self.table.off_for(sub);
    }

    /// Gives the evaluator an input event (RULE-5 step 2), to be handled
    /// at a tick. It waits in a queue of at most `MAX_QUEUED_EVENTS`;
    /// false when the queue is full and the event was dropped, which is
    /// counted.
    pub fn push(&mut self, event: InputEvent) -> bool {
        let place = self.queue_head.saturating_add(self.queue_length).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        // RULE-96: an event handled on arrival still waits for its tick,
        // so it holds its place among the `MAX_QUEUED_EVENTS` until then,
        // and the same events are dropped whichever way they are given.
        let waiting = self.queue_length.saturating_add(self.handled);
        let slot = if waiting < MAX_QUEUED_EVENTS { self.queue.get_mut(place) } else { None };
        let Some(slot) = slot else {
            self.counters.events_dropped = self.counters.events_dropped.saturating_add(1);
            return false;
        };
        *slot = event;
        self.queue_length = self.queue_length.saturating_add(1);
        true
    }

    /// Gives the newest metered report (RULE-33 item 5): one byte for each
    /// slot of the watch list, in its order. A slot not given reads 0. The
    /// next tick's snapshot takes it.
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

    /// Gives the evaluator an input event the moment it arrives, without
    /// waiting for the tick (RULE-96, DEC-6580 item 1), and gives the
    /// world at once the sound operations (`midi`, `play`, `stop`, `gain`)
    /// of the rules it fires. False when the event was dropped from a
    /// full queue, which is counted, as [`Evaluator::push`] answers.
    ///
    /// The event is an event of the next tick to be evaluated, tick `t`,
    /// and is handled exactly as step 2 of RULE-5 handles it, only
    /// earlier:
    ///
    /// - RULE-7 as revised: its rules' conditions read the snapshot of
    ///   tick `t`. That snapshot is the state the last completed tick left
    ///   (the variables, the timers, the rules enabled), with the world as
    ///   it is when the first event of the interval arrives (the targets,
    ///   the sounds, the tracked values, the newest metered report, the
    ///   sub-page shown). It is taken once: every later event of the
    ///   interval and steps 3 to 6 of tick `t` read the same one, and what
    ///   changes in the world after it is seen by tick `t + 1`.
    /// - RULE-8: the keys and the pointer are as every event that arrived
    ///   before this one left them, this one included, in arrival order.
    /// - RULE-5 step 2: events are handled in arrival order. One given
    ///   earlier by [`Evaluator::push`] and still waiting is handled
    ///   first; once `MAX_EVENTS_PER_TICK` events of tick `t` are handled,
    ///   this one waits in the queue for a later tick, where its sound
    ///   operations leave with the tick's other outputs.
    /// - RULE-6: a firing counts against tick `t`'s `MAX_ACTIONS_PER_TICK`
    ///   and is dropped whole beyond it, sound operations and all.
    /// - RULE-5 step 7: what the fired rules write, their animation
    ///   operations and their reports are held, in order, and are applied
    ///   and leave when tick `t` ends. Tick `t` does not evaluate the
    ///   event again.
    ///
    /// So a recording of each event with its tick and its place within the
    /// tick replays to the same result through either method: only the
    /// moment the world is given a sound operation differs. After
    /// [`Evaluator::close`] the event waits unhandled, as with `push`.
    pub fn push_now(&mut self, event: InputEvent, world: &mut dyn World) -> bool {
        if self.closed {
            return self.push(event);
        }
        self.open(world);
        let first_output = self.output_count;
        // Earlier arrivals first (RULE-5 step 2).
        while let Some(waiting) = self.next_event() {
            self.handle(&waiting, world);
        }
        let taken = if self.handled < MAX_EVENTS_PER_TICK
            && self.queue_length.saturating_add(self.handled) < MAX_QUEUED_EVENTS
        {
            self.handled = self.handled.saturating_add(1);
            self.handle(&event, world);
            true
        } else {
            self.push(event)
        };
        self.deliver_sounds(first_output, world);
        taken
    }

    /// Gives the world the sound operations among the outputs kept from
    /// `first` on, in order, and keeps the others for the tick's end
    /// (RULE-96). Only step 2's outputs can be there, and step 2 is the
    /// first to make any, so no sound operation overtakes an earlier one.
    fn deliver_sounds(&mut self, first: usize, world: &mut dyn World) {
        let mut kept = first;
        for place in first..self.output_count {
            let Some(output) = self.outputs.get(place).copied() else { continue };
            if let Output::Sound(operation) = output {
                if !world.sound_operation(operation) {
                    self.skip();
                }
            } else {
                if let Some(slot) = self.outputs.get_mut(kept) {
                    *slot = output;
                }
                kept = kept.saturating_add(1);
            }
        }
        self.output_count = kept;
    }

    /// Takes the snapshot of the tick to be evaluated, unless an event
    /// given on arrival already made it be taken (RULE-96).
    fn open(&mut self, world: &dyn World) {
        if !self.opened {
            self.take_snapshot(world);
            self.opened = true;
        }
    }

    /// Evaluates one tick (RULE-5), in its fixed order: the snapshot, the
    /// input events, the timers, `open` on page tick 0, `becomes`,
    /// animation, and apply. The events [`Evaluator::push_now`] handled
    /// since the last tick are this tick's first events and are not
    /// evaluated again (RULE-96). After [`Evaluator::close`] it does
    /// nothing.
    pub fn tick(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        self.open(world);
        self.run_events(world);
        self.run_timers();
        if self.tick == 0 {
            self.run_simple(&Trigger::Open);
        }
        self.run_becomes();
        world.advance_cycles(self.tick);
        self.apply(world);
        self.opened = false;
        self.handled = 0;
        self.counters.ticks = self.counters.ticks.saturating_add(1);
        self.tick = self.tick.saturating_add(1);
    }

    /// Evaluates the final tick, when the page stops being shown (RULE-5):
    /// the snapshot, each enabled `close` rule whose condition holds, and
    /// apply. The receiver first gives a release for every key and button
    /// still down and runs a tick for them (RULE-8). When events were
    /// handled on arrival and their tick has not run, it is run first:
    /// their sound operations have left, so their other effects must too
    /// (RULE-96).
    pub fn close(&mut self, world: &mut dyn World) {
        if self.closed {
            return;
        }
        if self.opened {
            self.tick(world);
        }
        self.take_snapshot(world);
        self.run_simple(&Trigger::Close);
        self.apply(world);
        self.closed = true;
    }

    /// The rectangles of the picture inside which an enabled `wheel` rule
    /// applies (RULE-51 item 4), as the last snapshot holds them, written
    /// to the front of `out`; the count written. "Any control" and "any
    /// object" give the whole picture.
    pub fn wheel_bounds(&self, out: &mut [Bounds; MAX_RECTS]) -> usize {
        let enabled = self.snapshot.enabled & !self.off;
        let mut count = 0_usize;
        for (rule, index) in self.table.rules.iter().zip(0_usize..) {
            let Trigger::Wheel { target } = rule.trigger else { continue };
            if enabled & rule_bit(index) == 0 {
                continue;
            }
            let bounds = if target.is_any() { self.bounds_of(Target::PAGE) } else { self.bounds_of(target) };
            let (Some(bounds), Some(slot)) = (bounds, out.get_mut(count)) else { continue };
            *slot = bounds;
            count = count.saturating_add(1);
        }
        count
    }

    /// Step 1 (RULE-5, RULE-7): the world as the last tick left it.
    fn take_snapshot(&mut self, world: &dyn World) {
        self.off = self.off_shown;
        self.output_count = 0;
        self.actions_run = 0;
        self.working = self.snapshot;
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

    /// Reads one value that is not input state, as a tick begins.
    fn read_snapshot(&self, value: Value, world: &dyn World) -> i32 {
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
                match value.class() {
                    CLASS_ENVELOPE_STAGE | CLASS_ENVELOPE_LEVEL | CLASS_VOICES => self.read_metered(value),
                    class => world.synth(value.a, value.part(), class, value.d),
                }
            }
            _ => 0,
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
        // A timer started in the last tick runs since this one (RULE-6).
        start.and_then(|start| self.tick.checked_sub(start))
    }

    /// A value's reading in this tick: the snapshot's, or for input state
    /// the state as the events so far leave it (RULE-8).
    fn reading(&self, value: Value, slot: u8) -> i32 {
        if value.space == SPACE_TARGET && value.is_input_state() {
            return i32::from(value.named_target().is_some_and(|target| self.input.inside.contains(target)));
        }
        if value.is_input_state() {
            return self.input.read(value);
        }
        self.readings.get(usize::from(slot)).copied().unwrap_or(0)
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

    /// Whether rule `rule`'s condition holds (RULE-18); none always does.
    fn holds(&self, rule: usize) -> bool {
        let Some(nodes) = self.table.rules.get(rule).map(|rule| &rule.condition) else { return false };
        if nodes.is_empty() {
            return true;
        }
        let mut at = 0_usize;
        self.node_holds(rule, nodes, &mut at, 0)
    }

    /// Evaluates the node at `at` and its children, leaving `at` after
    /// them. Every child is evaluated: no node has an effect, and the
    /// place after the tree is then known.
    fn node_holds(&self, rule: usize, nodes: &[Node], at: &mut usize, depth: usize) -> bool {
        let place = *at;
        *at = place.saturating_add(1);
        // The decoder holds operators to three levels (RULE-18).
        let (Some(node), true) = (nodes.get(place), depth <= MAX_CONDITION_DEPTH) else { return false };
        let read = |value: Value| {
            let slot = self.node_slots.get(rule).and_then(|slots| slots.get(place)).copied().unwrap_or(NO_SLOT);
            self.reading(value, slot)
        };
        match node {
            Node::And(children) => {
                let mut all = true;
                for _ in 0..*children {
                    all &= self.node_holds(rule, nodes, at, depth.saturating_add(1));
                }
                all
            }
            Node::Or(children) => {
                let mut any = false;
                for _ in 0..*children {
                    any |= self.node_holds(rule, nodes, at, depth.saturating_add(1));
                }
                any
            }
            Node::Not => !self.node_holds(rule, nodes, at, depth.saturating_add(1)),
            Node::Compare { value, operator, constant } => operator.holds(read(*value), *constant),
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

    /// Step 2 (RULE-5): the input events that arrived, in arrival order,
    /// at most `MAX_EVENTS_PER_TICK`, those handled on arrival among them
    /// (RULE-96).
    fn run_events(&mut self, world: &dyn World) {
        while let Some(event) = self.next_event() {
            self.handle(&event, world);
        }
    }

    /// Takes the event that has waited longest, when the tick being
    /// evaluated has handled fewer than `MAX_EVENTS_PER_TICK` (RULE-5
    /// step 2).
    fn next_event(&mut self) -> Option<InputEvent> {
        if self.handled >= MAX_EVENTS_PER_TICK || self.queue_length == 0 {
            return None;
        }
        let event = self.queue.get(self.queue_head).copied()?;
        self.queue_head = self.queue_head.saturating_add(1).checked_rem(MAX_QUEUED_EVENTS).unwrap_or(0);
        self.queue_length = self.queue_length.saturating_sub(1);
        self.handled = self.handled.saturating_add(1);
        Some(event)
    }

    /// Handles one event (RULE-5 step 2): the input state first (RULE-8),
    /// then the rules.
    fn handle(&mut self, event: &InputEvent, world: &dyn World) {
        let transition = self.follow(&event.input, world);
        self.match_event(event, &transition);
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
                self.input.pointer(action, button, now, x, y)
            }
            Input::Wheel { x, y, control } => {
                let now = at(control, x, y, &self.table.rects);
                // A wheel event moves nothing and presses nothing, but it
                // says where the pointer is.
                self.input.pointer(PointerAction::Gone, u8::MAX, now, x, y)
            }
            Input::Text | Input::Focus | Input::Blur => Transition::NONE,
        }
    }

    /// Matches one event against each enabled rule in rule order, and
    /// fires those whose condition holds (RULE-5 step 2).
    fn match_event(&mut self, event: &InputEvent, transition: &Transition) {
        let enabled = self.snapshot.enabled & !self.off;
        for index in 0..self.table.rules.len() {
            if enabled & rule_bit(index) == 0 {
                continue;
            }
            let Some(trigger) = self.table.rules.get(index).map(|rule| rule.trigger) else { continue };
            // The page, or each control or object an "any" names.
            let mut hits: Bounded<Target, { MAX_POINTER_OBJECTS + 1 }> = Bounded::new(Target::NONE);
            self.matches(&trigger, &event.input, transition, &mut |target| {
                let _ = hits.push(target);
            });
            if hits.is_empty() || !self.holds(index) {
                continue;
            }
            for target in hits.iter() {
                self.fire(index, Cause { sequence: Some(event.sequence), target: *target });
            }
        }
    }

    /// Gives `hit` each target for which `trigger` matches `input`
    /// (RULE-9): [`Target::NONE`] for a trigger that names none.
    fn matches(&self, trigger: &Trigger, input: &Input, transition: &Transition, hit: &mut dyn FnMut(Target)) {
        let inside = &self.input.inside;
        match (trigger, input) {
            (Trigger::Key { keys, repeat, mods_mask, mods_value }, Input::Key { action, key, base, mods }) => {
                let pressed = *action == KeyAction::Press || (*action == KeyAction::Repeat && *repeat);
                let named = keys.is_empty() || keys.iter().any(|each| key_names(*each, *key, *base));
                if pressed && named && mods & mods_mask == *mods_value {
                    hit(Target::NONE);
                }
            }
            (Trigger::KeyUp { keys, mods_mask, mods_value }, Input::Key { action, key, base, mods }) => {
                let named = keys.is_empty() || keys.iter().any(|each| key_names(*each, *key, *base));
                if *action == KeyAction::Release && named && mods & mods_mask == *mods_value {
                    hit(Target::NONE);
                }
            }
            (Trigger::Text, Input::Text) | (Trigger::Focus, Input::Focus) | (Trigger::Blur, Input::Blur) => {
                hit(Target::NONE);
            }
            (Trigger::Press { target, button }, Input::Pointer { action: PointerAction::Press, button: down, .. })
                if *button == crate::limits::ANY_BUTTON || button == down =>
            {
                inside.matching(*target, hit);
            }
            (
                Trigger::Release { target, button },
                Input::Pointer { action: PointerAction::Release, button: up, .. },
            ) if *button == crate::limits::ANY_BUTTON || button == up => {
                transition.released.matching(*target, hit);
            }
            (Trigger::Enter { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                transition.entered.matching(*target, hit);
            }
            (Trigger::Leave { target }, Input::Pointer { .. } | Input::Wheel { .. }) => {
                transition.left.matching(*target, hit);
            }
            (Trigger::Move { target, held }, Input::Pointer { action: PointerAction::Move, button: mask, .. })
                if transition.moved && (!held || *mask != 0 || self.input.buttons != 0) =>
            {
                inside.matching(*target, hit);
            }
            (Trigger::Wheel { target }, Input::Wheel { .. }) => inside.matching(*target, hit),
            _ => {}
        }
    }

    /// Step 3 (RULE-5): the timers in identifier order, the page timer
    /// first; for each that runs in the snapshot, each enabled rule in
    /// rule order whose `after` or `every` on it is due.
    fn run_timers(&mut self) {
        let enabled = self.snapshot.enabled & !self.off;
        let timers = core::iter::once(PAGE_TIMER).chain((0_u8..).take(self.table.timers.len()));
        for timer in timers {
            let Some(elapsed) = self.elapsed(timer) else { continue };
            for index in 0..self.table.rules.len() {
                let due = match self.table.rules.get(index).map(|rule| rule.trigger) {
                    Some(Trigger::After { timer: named, ticks }) => named == timer && elapsed == ticks,
                    Some(Trigger::Every { timer: named, ticks }) => {
                        named == timer && elapsed > 0 && elapsed.checked_rem(ticks) == Some(0)
                    }
                    _ => false,
                };
                if due && enabled & rule_bit(index) != 0 && self.holds(index) {
                    self.fire(index, Cause::NONE);
                }
            }
        }
    }

    /// Step 4, and the final tick (RULE-5): each enabled rule of `trigger`,
    /// `open` or `close`, in rule order, whose condition holds.
    fn run_simple(&mut self, trigger: &Trigger) {
        let enabled = self.snapshot.enabled & !self.off;
        for index in 0..self.table.rules.len() {
            let is = self.table.rules.get(index).is_some_and(|rule| rule.trigger == *trigger);
            if is && enabled & rule_bit(index) != 0 && self.holds(index) {
                self.fire(index, Cause::NONE);
            }
        }
    }

    /// Step 5 (RULE-5, RULE-11): each enabled `becomes` rule fires when
    /// its condition holds now and did not at its previous evaluation. A
    /// rule that is off is not evaluated, and its bit is cleared.
    fn run_becomes(&mut self) {
        let enabled = self.snapshot.enabled & !self.off;
        for index in 0..self.table.rules.len() {
            if self.table.rules.get(index).is_none_or(|rule| rule.trigger != Trigger::Becomes) {
                continue;
            }
            let bit = rule_bit(index);
            if enabled & bit == 0 {
                self.becomes &= !bit;
                continue;
            }
            let holds = self.holds(index);
            let held = self.becomes & bit != 0;
            if holds {
                self.becomes |= bit;
            } else {
                self.becomes &= !bit;
            }
            if holds && !held {
                self.fire(index, Cause::NONE);
            }
        }
    }

    /// Runs rule `index`'s actions in written order, then its `echo`
    /// (RULE-22). Once `MAX_ACTIONS_PER_TICK` actions have run in the
    /// tick, a firing that would pass it is dropped whole and counted
    /// (RULE-6).
    fn fire(&mut self, index: usize, cause: Cause) {
        let Some((actions, echo)) = self.table.rules.get(index).map(|rule| (rule.actions, rule.echo)) else { return };
        let needed = actions.len().saturating_add(usize::from(echo));
        if self.actions_run.saturating_add(needed) > MAX_ACTIONS_PER_TICK {
            self.counters.overruns = self.counters.overruns.saturating_add(1);
            return;
        }
        self.actions_run = self.actions_run.saturating_add(needed);
        self.counters.fired = self.counters.fired.saturating_add(1);
        self.counters.actions = self.counters.actions.saturating_add(u32::try_from(needed).unwrap_or(u32::MAX));
        for (action, place) in actions.iter().zip(0_usize..) {
            self.run(index, place, action, cause);
        }
        if echo {
            self.send(index, cause, None);
        }
    }

    /// One report (RULE-22): the trigger's own event, or a value.
    fn send(&mut self, rule: usize, cause: Cause, value: Option<(Value, i32)>) {
        let what = match (value, cause.sequence) {
            (Some((value, reading)), _) => Reported::Value { value, reading },
            (None, Some(sequence)) => Reported::Input { sequence },
            (None, None) => Reported::Fired,
        };
        // RULE-46: a report of a value names no target.
        let target = if value.is_some() { Target::NONE } else { cause.target };
        let report = Report { rule: u8::try_from(rule).unwrap_or(u8::MAX), tick: self.tick, target, what };
        self.counters.sends = self.counters.sends.saturating_add(1);
        self.output(Output::Report(report));
    }

    /// Keeps an output until the tick ends. The cap on a tick's actions
    /// is the array's length, so there is always room.
    fn output(&mut self, output: Output) {
        if let Some(slot) = self.outputs.get_mut(self.output_count) {
            *slot = output;
            self.output_count = self.output_count.saturating_add(1);
        }
    }

    /// Counts an action that does nothing (RULE-20).
    fn skip(&mut self) {
        self.counters.skipped = self.counters.skipped.saturating_add(1);
    }

    /// Runs one action (RULE-20): its effect goes to the working state or
    /// to the outputs, and is seen from the next tick (RULE-6).
    fn run(&mut self, rule: usize, place: usize, action: &Action, cause: Cause) {
        let sound = |channel: u8| self.sounds.get(usize::from(channel)).copied().unwrap_or_default();
        match *action {
            Action::Midi { channel, status, data1, data2 } => {
                // RULE-21: only to a local sound.
                if sound(channel).playing && sound(channel).local {
                    self.output(Output::Sound(SoundOperation::Midi { channel, status, data1, data2 }));
                } else {
                    self.skip();
                }
            }
            Action::Play { channel, handle, generation, gain, fade_ms, looped } => {
                self.output(Output::Sound(SoundOperation::Play { channel, handle, generation, gain, fade_ms, looped }));
            }
            Action::Stop { channel, fade_ms } => {
                if sound(channel).playing {
                    self.output(Output::Sound(SoundOperation::Stop { channel, fade_ms }));
                } else {
                    self.skip();
                }
            }
            Action::Gain { channel, gain, fade_ms } => {
                if sound(channel).playing {
                    self.output(Output::Sound(SoundOperation::Gain { channel, gain, fade_ms }));
                } else {
                    self.skip();
                }
            }
            Action::Animate(operation) => {
                let exists = self.index.target_slot(operation.target).and_then(|slot| self.targets.get(slot));
                if exists.is_some_and(Option::is_some) {
                    self.output(Output::Animation(operation));
                } else {
                    self.skip();
                }
            }
            Action::Set { variable, value } => self.write_variable(variable, |_| value),
            // RULE-7: `add` reads the working state, so two in one tick
            // both count.
            Action::Add { variable, amount } => self.write_variable(variable, |held| held.saturating_add(amount)),
            Action::StartTimer(timer) => {
                // RULE-15: running since the next tick.
                let start = self.tick.saturating_add(1);
                if let Some(slot) = self.working.timers.get_mut(usize::from(timer)) {
                    *slot = Some(start);
                }
            }
            Action::StopTimer(timer) => {
                if let Some(slot) = self.working.timers.get_mut(usize::from(timer)) {
                    *slot = None;
                }
            }
            Action::Enable(mask) => self.working.enabled |= mask,
            Action::Disable(mask) => self.working.enabled &= !mask,
            Action::Send(None) => self.send(rule, cause, None),
            Action::Send(Some(value)) => {
                let slot = self.send_slots.get(rule).and_then(|slots| slots.get(place)).copied().unwrap_or(NO_SLOT);
                let reading = self.reading(value, slot);
                self.send(rule, cause, Some((value, reading)));
            }
        }
    }

    /// Writes a variable in the working state, held to its declared range
    /// (RULE-14).
    fn write_variable(&mut self, variable: u8, change: impl FnOnce(i32) -> i32) {
        let declared = self.table.variables.get(usize::from(variable)).copied();
        let (Some(declared), Some(slot)) = (declared, self.working.variables.get_mut(usize::from(variable))) else {
            self.skip();
            return;
        };
        *slot = change(*slot).max(declared.min).min(declared.max);
    }

    /// Step 7 (RULE-5, RULE-7): the working state becomes the snapshot,
    /// and the outputs leave in the order their actions ran.
    fn apply(&mut self, world: &mut dyn World) {
        // The answer to a state query is of the snapshot this tick read.
        let state = self.query.take().map(|identifier| self.page_state(identifier));
        self.snapshot = self.working;
        for place in 0..self.output_count {
            let did = match self.outputs.get(place).copied() {
                Some(Output::Sound(operation)) => world.sound_operation(operation),
                Some(Output::Animation(operation)) => world.animate(operation, self.tick),
                Some(Output::Report(report)) => {
                    world.report(report);
                    true
                }
                None => true,
            };
            if !did {
                self.skip();
            }
        }
        self.output_count = 0;
        if let Some(state) = state {
            world.page_state(&state);
        }
    }

    /// The page's state as the snapshot of the tick being evaluated holds
    /// it (RULE-47).
    fn page_state(&self, identifier: u32) -> PageState {
        let mut state = TableState::new(self.generation, self.tick, self.snapshot.enabled);
        for value in self.snapshot.variables.iter().take(self.table.variables.len()) {
            let _ = state.variables.push(*value);
        }
        for timer in (0_u8..).take(self.table.timers.len()) {
            let elapsed = self.elapsed(timer);
            let _ = state.timers.push(TimerState { running: elapsed.is_some(), elapsed: elapsed.unwrap_or(0) });
        }
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
                look: found.look.min(crate::limits::MAX_LOOK),
                frame: found.frame.min(crate::limits::MAX_FRAME),
                x: found.x,
                y: found.y,
                layer: found.layer.min(MAX_LAYER),
                order: found.order,
            });
        }
        for (value, slot) in self.index.values.iter().zip(0_u8..) {
            let _ = state.values.push((*value, self.reading(*value, slot)));
        }
        PageState { identifier, table: Some(state) }
    }
}

#[cfg(test)]
mod tests;
