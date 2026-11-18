//! The animation state of a control or an object, the operations that
//! change it, and frame cycling (ANIM-6, ANIM-14).
//!
//! Operations are carried by the `ANIMATE` record alone, whether the
//! server sends it or a group of a page's rule table holds it as a command
//! (RULE-119), and [`read`] is the one function that reads them;
//! [`read_record`] reads the record's body. [`AnimationState::apply`]
//! folds an operation into a state; [`AnimationStore`] keeps the states of
//! a page's objects.

use crate::Fault;
use crate::limits::{KEEP_LAYER, MAX_CONTROL, MAX_CYCLE_PERIOD, MAX_FRAME, MAX_LAYER, MAX_LOOK};
use crate::table::{TARGET_BYTES, Target, TargetKind};
use crate::wire::{Reader, Writer};

/// Operation codes (ANIM-6).
pub mod code {
    /// `show`.
    pub const SHOW: u8 = 5;
    /// `hide`.
    pub const HIDE: u8 = 6;
    /// `look`.
    pub const LOOK: u8 = 7;
    /// `move`.
    pub const MOVE: u8 = 15;
    /// `order`.
    pub const ORDER: u8 = 16;
    /// `frame`.
    pub const FRAME: u8 = 17;
    /// `cycle`.
    pub const CYCLE: u8 = 18;
    /// `stop_cycle`.
    pub const STOP_CYCLE: u8 = 19;
    /// `reset`.
    pub const RESET: u8 = 20;
}

/// The most bytes an operation takes: `cycle`, its code, its target, two
/// frames, a period and its flags.
pub const MAX_OPERATION_BYTES: usize = 1 + TARGET_BYTES + 1 + 1 + 2 + 1;

/// The flag of `move` that makes it relative, and of `cycle` that makes
/// it run once: bit 0. Every other bit is 0.
const FLAG: u8 = 1;

/// What an operation does to its target's animation state (ANIM-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OperationKind {
    /// Makes it shown.
    Show,
    /// Makes it hidden.
    Hide,
    /// Forces a look: 0 normal (the pointer decides again), 1 hover, 2
    /// pressed, 3 alternative.
    Look(u8),
    /// Puts an object at a position, or moves it by an amount, in its
    /// renderer's units.
    Move {
        /// Whether `x` and `y` are added to the position in force.
        relative: bool,
        /// Across.
        x: i16,
        /// Down.
        y: i16,
    },
    /// Sets an object's layer and its order within the layer (ANIM-12).
    Order {
        /// 0 to 3, or 255 to keep the layer in force.
        layer: u8,
        /// −128 to 127.
        order: i8,
    },
    /// Shows one frame of an object, 0 to 15, and stops any cycle.
    Frame(u8),
    /// Starts cycling an object's frames (ANIM-14).
    Cycle {
        /// The first frame.
        first: u8,
        /// The last frame: `first ≤ last ≤ 15`.
        last: u8,
        /// Ticks a frame, 1 to 3,600.
        period: u16,
        /// Whether it stops at `last` instead of looping.
        once: bool,
    },
    /// Stops the cycle at the frame shown.
    StopCycle,
    /// Forgets the whole animation state.
    Reset,
}

/// One operation on one control or object (ANIM-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Operation {
    /// The control or object: never the page, a rectangle or "any".
    pub target: Target,
    /// What is done to it.
    pub kind: OperationKind,
}

impl Operation {
    /// The operation's code byte (ANIM-6).
    #[must_use]
    pub const fn code(&self) -> u8 {
        match self.kind {
            OperationKind::Show => code::SHOW,
            OperationKind::Hide => code::HIDE,
            OperationKind::Look(_) => code::LOOK,
            OperationKind::Move { .. } => code::MOVE,
            OperationKind::Order { .. } => code::ORDER,
            OperationKind::Frame(_) => code::FRAME,
            OperationKind::Cycle { .. } => code::CYCLE,
            OperationKind::StopCycle => code::STOP_CYCLE,
            OperationKind::Reset => code::RESET,
        }
    }

    /// Checks the operation's target and fields (ANIM-6).
    ///
    /// # Errors
    /// [`Fault::Range`] for a target the operation does not apply to or a
    /// field outside its range.
    pub fn check(&self) -> Result<(), Fault> {
        let objects_only = !matches!(
            self.kind,
            OperationKind::Show | OperationKind::Hide | OperationKind::Look(_) | OperationKind::Reset
        );
        let target = match self.target.kind {
            TargetKind::Control => !objects_only && self.target.number >= 1 && self.target.number <= MAX_CONTROL,
            TargetKind::Object => self.target.number >= 1,
            TargetKind::Page | TargetKind::Rect => false,
        };
        if !target {
            return Err(Fault::Range("operation's target"));
        }
        let fields = match self.kind {
            OperationKind::Look(look) => look <= MAX_LOOK,
            OperationKind::Order { layer, .. } => layer <= MAX_LAYER || layer == KEEP_LAYER,
            OperationKind::Frame(frame) => frame <= MAX_FRAME,
            OperationKind::Cycle { first, last, period, .. } => {
                first <= last && last <= MAX_FRAME && (1..=MAX_CYCLE_PERIOD).contains(&period)
            }
            OperationKind::Show
            | OperationKind::Hide
            | OperationKind::Move { .. }
            | OperationKind::StopCycle
            | OperationKind::Reset => true,
        };
        if fields { Ok(()) } else { Err(Fault::Range("operation's field")) }
    }

    /// Reads the operation of code `code`, after its code byte.
    pub(crate) fn read_after_code(code: u8, reader: &mut Reader<'_>) -> Result<Self, Fault> {
        let kind = TargetKind::from_code(reader.u8()?).ok_or(Fault::Range("operation's target"))?;
        let target = Target { kind, number: reader.u16()? };
        let flag =
            |flags: u8, what| if flags & !FLAG == 0 { Ok(flags & FLAG != 0) } else { Err(Fault::Reserved(what)) };
        let kind = match code {
            code::SHOW => OperationKind::Show,
            code::HIDE => OperationKind::Hide,
            code::LOOK => OperationKind::Look(reader.u8()?),
            code::MOVE => {
                let relative = flag(reader.u8()?, "move flags")?;
                OperationKind::Move { relative, x: reader.i16()?, y: reader.i16()? }
            }
            code::ORDER => OperationKind::Order { layer: reader.u8()?, order: reader.i8()? },
            code::FRAME => OperationKind::Frame(reader.u8()?),
            code::CYCLE => {
                let (first, last, period) = (reader.u8()?, reader.u8()?, reader.u16()?);
                OperationKind::Cycle { first, last, period, once: flag(reader.u8()?, "cycle flags")? }
            }
            code::STOP_CYCLE => OperationKind::StopCycle,
            code::RESET => OperationKind::Reset,
            _ => return Err(Fault::Range("operation code")),
        };
        let operation = Self { target, kind };
        operation.check()?;
        Ok(operation)
    }

    /// Writes the operation, its code byte first.
    pub(crate) fn write(&self, writer: &mut Writer<'_>) -> Result<(), Fault> {
        self.check()?;
        writer.u8(self.code())?;
        writer.u8(self.target.kind.code())?;
        writer.u16(self.target.number)?;
        match self.kind {
            OperationKind::Show | OperationKind::Hide | OperationKind::StopCycle | OperationKind::Reset => Ok(()),
            OperationKind::Look(look) => writer.u8(look),
            OperationKind::Move { relative, x, y } => {
                writer.u8(if relative { FLAG } else { 0 })?;
                writer.i16(x)?;
                writer.i16(y)
            }
            OperationKind::Order { layer, order } => {
                writer.u8(layer)?;
                writer.i8(order)
            }
            OperationKind::Frame(frame) => writer.u8(frame),
            OperationKind::Cycle { first, last, period, once } => {
                writer.u8(first)?;
                writer.u8(last)?;
                writer.u16(period)?;
                writer.u8(if once { FLAG } else { 0 })
            }
        }
    }
}

/// Whether `code` is an operation's code (ANIM-6).
#[must_use]
pub const fn is_code(code: u8) -> bool {
    matches!(code, code::SHOW..=code::LOOK | code::MOVE..=code::RESET)
}

/// The bytes of an operation before its own fields: its code and its
/// target.
pub const OPERATION_FIXED: usize = 1 + TARGET_BYTES;

/// The bytes the operation of code `code` takes, its code byte and its
/// target among them (ANIM-6), or `None` for a code no operation has.
#[must_use]
pub const fn length_of(code: u8) -> Option<usize> {
    match code {
        code::SHOW | code::HIDE | code::STOP_CYCLE | code::RESET => Some(OPERATION_FIXED),
        code::LOOK | code::FRAME => Some(OPERATION_FIXED + 1),
        code::ORDER => Some(OPERATION_FIXED + 2),
        code::MOVE => Some(OPERATION_FIXED + 1 + 2 + 2),
        code::CYCLE => Some(OPERATION_FIXED + 1 + 1 + 2 + 1),
        _ => None,
    }
}

/// The bytes of an `ANIMATE` record's body before its operations
/// (ANIM-6): the page, the sub-code, the count and a reserved byte.
pub const RECORD_FIXED: usize = 2 + 2 + 1 + 1;
/// The place of the count of operations in an `ANIMATE` record's body.
pub const RECORD_COUNT_AT: usize = 4;
/// The most bytes of an `ANIMATE` record's body.
pub const MAX_RECORD_BYTES: usize = RECORD_FIXED + crate::limits::MAX_ANIMATE_OPERATIONS * MAX_OPERATION_BYTES;

/// What an `ANIMATE` record's body says before its operations (ANIM-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordHeader {
    /// The page number, `0x100` to `0x8FF`.
    pub page: u16,
    /// The sub-code, `0x0000` to `0x3F7E`: the first form's `0x3F7F`,
    /// "whichever sub-page is shown", is refused (DEC-6599).
    pub sub: u16,
    /// How many operations, 1 to 64.
    pub count: u8,
}

/// Reads an `ANIMATE` record's body (ANIM-6), checking every operation,
/// and gives its header and the operations' bytes, which [`read`] reads
/// one after another.
///
/// # Errors
/// A page or a sub-code outside its range, a count of 0 or over
/// `MAX_ANIMATE_OPERATIONS`, a reserved byte that is not 0, an operation
/// [`read`] refuses, or bytes after the last operation.
pub fn read_record(body: &[u8]) -> Result<(RecordHeader, &[u8]), Fault> {
    let mut reader = Reader::new(body);
    let (page, sub) = crate::command::read_page(&mut reader)?;
    let count = reader.u8()?;
    if count == 0 || usize::from(count) > crate::limits::MAX_ANIMATE_OPERATIONS {
        return Err(Fault::Range("animate count"));
    }
    if reader.u8()? != 0 {
        return Err(Fault::Reserved("animate"));
    }
    let operations = body.get(reader.used()..).ok_or(Fault::Short)?;
    let mut rest = operations;
    for _ in 0..count {
        let (_, used) = read(rest)?;
        rest = rest.get(used..).ok_or(Fault::Short)?;
    }
    if rest.is_empty() { Ok((RecordHeader { page, sub, count }, operations)) } else { Err(Fault::Trailing) }
}

/// Writes an `ANIMATE` record's body of `operations` for the page `page`
/// and `sub` at the front of `out`, and how many bytes it took.
///
/// # Errors
/// What [`read_record`] refuses, and [`Fault::Full`] when `out` is too
/// small.
pub fn write_record(page: u16, sub: u16, operations: &[Operation], out: &mut [u8]) -> Result<usize, Fault> {
    let count = u8::try_from(operations.len()).map_err(|_| Fault::Range("animate count"))?;
    let mut writer = Writer::new(out);
    writer.u16(page)?;
    writer.u16(sub)?;
    writer.u8(count)?;
    writer.u8(0)?;
    for operation in operations {
        operation.write(&mut writer)?;
    }
    let length = writer.used();
    read_record(out.get(..length).ok_or(Fault::Full)?)?;
    Ok(length)
}

/// The operations of an `ANIMATE` record's body [`read_record`] accepted,
/// in order.
pub fn operations(bytes: &[u8]) -> impl Iterator<Item = Operation> + '_ {
    let mut rest = bytes;
    core::iter::from_fn(move || {
        let (operation, used) = read(rest).ok()?;
        rest = rest.get(used..).unwrap_or(&[]);
        Some(operation)
    })
}

/// Reads one operation from the front of `bytes`, and how many bytes it
/// took.
///
/// # Errors
/// [`Fault::Short`] when the bytes end inside it, and otherwise what
/// [`Operation::check`] refuses, a code that is no operation's, or a
/// reserved flag bit.
pub fn read(bytes: &[u8]) -> Result<(Operation, usize), Fault> {
    let mut reader = Reader::new(bytes);
    let code = reader.u8()?;
    let operation = Operation::read_after_code(code, &mut reader)?;
    Ok((operation, reader.used()))
}

/// Writes one operation at the front of `out`, and how many bytes it took.
///
/// # Errors
/// What [`Operation::check`] refuses, and [`Fault::Full`] when `out` is
/// too small.
pub fn write(operation: &Operation, out: &mut [u8]) -> Result<usize, Fault> {
    let mut writer = Writer::new(out);
    operation.write(&mut writer)?;
    Ok(writer.used())
}

/// A cycle of frames in force (ANIM-14).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cycle {
    /// The first frame.
    pub first: u8,
    /// The last frame.
    pub last: u8,
    /// Ticks a frame.
    pub period: u16,
    /// Whether it stops at `last`.
    pub once: bool,
    /// The tick it started: the tick after its operation ran.
    pub start: u32,
}

impl Cycle {
    /// The frame at tick `tick`, and whether the cycle has ended (ANIM-14).
    /// With `s` the start, `p` the period and `c = last − first + 1`: the
    /// frame is `first + ((t − s) ÷ p) mod c`; with `once` it is
    /// `min(first + (t − s) ÷ p, last)`, and the cycle ends when `last` is
    /// reached. Before the start the frame is `first`.
    #[must_use]
    pub fn frame_at(&self, tick: u32) -> (u8, bool) {
        let elapsed = tick.saturating_sub(self.start);
        let steps = elapsed.checked_div(u32::from(self.period)).unwrap_or(0);
        let span = u32::from(self.last.saturating_sub(self.first));
        let count = span.saturating_add(1);
        let (offset, ended) =
            if self.once { (steps.min(span), steps >= span) } else { (steps.checked_rem(count).unwrap_or(0), false) };
        (self.first.saturating_add(u8::try_from(offset).unwrap_or(u8::MAX)).min(self.last), ended)
    }
}

/// The animation state of one control or object (ANIM-6): the receiver's
/// own, which nothing in a page's stream can read. A part that is `None`
/// leaves the object's own in force.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AnimationState {
    /// Shown or hidden; `None` for the object's own visibility.
    pub shown: Option<bool>,
    /// The position, in the renderer's units; `None` for the object's own.
    pub position: Option<(i16, i16)>,
    /// The layer, 0 to 3; `None` for the object's own.
    pub layer: Option<u8>,
    /// The order within the layer; `None` for the object's own.
    pub order: Option<i8>,
    /// The frame shown, 0 to 15, while no cycle runs.
    pub frame: u8,
    /// The cycle in force.
    pub cycle: Option<Cycle>,
    /// The forced look: 0 none, 1 hover, 2 pressed, 3 alternative.
    pub look: u8,
}

impl AnimationState {
    /// Applies one operation (ANIM-6). `own` is the object's own position,
    /// which a relative `move` adds to when no position is in force;
    /// `tick` is the tick the operation runs in, so that a cycle starts at
    /// the tick after it.
    pub fn apply(&mut self, kind: OperationKind, own: (i16, i16), tick: u32) {
        match kind {
            OperationKind::Show => self.shown = Some(true),
            OperationKind::Hide => self.shown = Some(false),
            OperationKind::Look(look) => self.look = look,
            OperationKind::Move { relative, x, y } => {
                let (from_x, from_y) = if relative { self.position.unwrap_or(own) } else { (0, 0) };
                self.position = Some((from_x.saturating_add(x), from_y.saturating_add(y)));
            }
            OperationKind::Order { layer, order } => {
                if layer != KEEP_LAYER {
                    self.layer = Some(layer);
                }
                self.order = Some(order);
            }
            OperationKind::Frame(frame) => {
                self.frame = frame;
                self.cycle = None;
            }
            OperationKind::Cycle { first, last, period, once } => {
                self.frame = first;
                self.cycle = Some(Cycle { first, last, period, once, start: tick.saturating_add(1) });
            }
            OperationKind::StopCycle => {
                self.frame = self.frame_at(tick);
                self.cycle = None;
            }
            OperationKind::Reset => *self = Self::default(),
        }
    }

    /// The frame in force at tick `tick` (ANIM-14).
    #[must_use]
    pub fn frame_at(&self, tick: u32) -> u8 {
        self.cycle.map_or(self.frame, |cycle| cycle.frame_at(tick).0)
    }

    /// Advances the cycle to tick `tick` (RULE-5 step 6): the frame it
    /// shows becomes the frame in force, and a cycle that ran once and
    /// reached its last frame ends. True when the frame changed.
    pub fn advance(&mut self, tick: u32) -> bool {
        let Some(cycle) = self.cycle else { return false };
        let (frame, ended) = cycle.frame_at(tick);
        let changed = frame != self.frame;
        self.frame = frame;
        if ended {
            self.cycle = None;
        }
        changed
    }

    /// Whether nothing is in force: the state of an object never touched,
    /// which a store need not keep.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Folds a list of operations into a state, in order (ANIM-6).
#[must_use]
pub fn fold(
    state: AnimationState,
    operations: impl IntoIterator<Item = OperationKind>,
    own: (i16, i16),
    tick: u32,
) -> AnimationState {
    operations.into_iter().fold(state, |mut state, kind| {
        state.apply(kind, own, tick);
        state
    })
}

/// What became of an operation given to an [`AnimationStore`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applied {
    /// It changed the target's state.
    Done,
    /// The store already holds `N` states: the operation did nothing, and
    /// is counted (ANIM-6).
    NoRoom,
    /// As many objects cycle as may: the `cycle` did nothing, and is
    /// counted (ANIM-14).
    TooManyCycles,
}

/// The animation states of a sub-page's controls and objects, at most `N`
/// of them (ANIM-6: `MAX_ANIMATED_OBJECTS`, each renderer's own limit).
#[derive(Clone, Copy, Debug)]
pub struct AnimationStore<const N: usize> {
    entries: [(Target, AnimationState); N],
    count: usize,
}

impl<const N: usize> Default for AnimationStore<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> AnimationStore<N> {
    /// A store with no state.
    #[must_use]
    pub fn new() -> Self {
        Self { entries: [(Target::NONE, AnimationState::default()); N], count: 0 }
    }

    /// The targets that hold state, with their states.
    #[must_use]
    pub fn entries(&self) -> &[(Target, AnimationState)] {
        self.entries.get(..self.count).unwrap_or(&[])
    }

    /// The state of `target`, if it holds one.
    #[must_use]
    pub fn get(&self, target: Target) -> Option<&AnimationState> {
        self.entries().iter().find(|(held, _)| *held == target).map(|(_, state)| state)
    }

    /// How many targets are cycling.
    #[must_use]
    pub fn cycling(&self) -> usize {
        self.entries().iter().filter(|(_, state)| state.cycle.is_some()).count()
    }

    /// Applies one operation at tick `tick`. `own` is the target's own
    /// position; at most `max_cycling` targets cycle at once (ANIM-14:
    /// `MAX_CYCLING_OBJECTS`).
    pub fn apply(&mut self, operation: &Operation, own: (i16, i16), tick: u32, max_cycling: usize) -> Applied {
        let place = self.entries().iter().position(|(held, _)| *held == operation.target);
        if operation.kind == OperationKind::Reset {
            if let Some(place) = place {
                self.remove(place);
            }
            return Applied::Done;
        }
        let already_cycling = place.and_then(|place| self.entries.get(place)).is_some_and(|(_, s)| s.cycle.is_some());
        if matches!(operation.kind, OperationKind::Cycle { .. }) && !already_cycling && self.cycling() >= max_cycling {
            return Applied::TooManyCycles;
        }
        let place = if let Some(place) = place {
            place
        } else {
            let Some(slot) = self.entries.get_mut(self.count) else { return Applied::NoRoom };
            *slot = (operation.target, AnimationState::default());
            let place = self.count;
            self.count = place.saturating_add(1);
            place
        };
        if let Some((_, state)) = self.entries.get_mut(place) {
            state.apply(operation.kind, own, tick);
        }
        Applied::Done
    }

    /// Advances every cycle to tick `tick` (RULE-5 step 6), giving each
    /// target whose frame changed to `changed`.
    pub fn advance(&mut self, tick: u32, changed: &mut dyn FnMut(Target)) {
        for (target, state) in self.entries.iter_mut().take(self.count) {
            if state.advance(tick) {
                changed(*target);
            }
        }
    }

    /// Forgets the state of `target`, as a stream's own change of it, its
    /// removal or its replacement does (ANIM-6: last writer wins).
    pub fn forget(&mut self, target: Target) {
        if let Some(place) = self.entries().iter().position(|(held, _)| *held == target) {
            self.remove(place);
        }
    }

    /// Changes the state of `target` in place, if it holds one: a stream
    /// that sets a drawing's own position forgets only the position.
    pub fn get_mut(&mut self, target: Target) -> Option<&mut AnimationState> {
        self.entries.iter_mut().take(self.count).find(|(held, _)| *held == target).map(|(_, state)| state)
    }

    /// Forgets every state.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// Removes the entry at `place`, keeping the others' order.
    fn remove(&mut self, place: usize) {
        let last = self.count.saturating_sub(1);
        for at in place..last {
            let next = self.entries.get(at.saturating_add(1)).copied();
            if let (Some(slot), Some(next)) = (self.entries.get_mut(at), next) {
                *slot = next;
            }
        }
        self.count = last;
    }
}

#[cfg(test)]
mod tests;
