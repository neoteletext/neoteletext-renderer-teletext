//! Teletext sprites and the animation state (animation.md ANIM-6, ANIM-7,
//! ANIM-13 and ANIM-14; REQ-6529; layer L3).
//!
//! A sprite is a numbered placement of teletext objects (EN 300 706 clause
//! 13) on a sub-page: a rectangle of cells, an origin, a layer and an order,
//! and 1 to 16 frames, each an object reference with the fields of an Object
//! Invocation triplet (Table 28). The receiver's **animation state** says,
//! for a sprite or a control, what stands in place of its own visibility,
//! position, layer, order and frame, and which look is forced on it; it is
//! the receiver's own, and nothing a page sends reads it.
//!
//! This module holds the values and their rules: [`Sprites`] checks what a
//! SPRITES record gives, [`Animation`] folds [`Operation`]s into the state
//! and advances frame cycles by the tick, and [`Placements`] is the two
//! together: the shown sprites in drawing order, each with its window of
//! cells and its bounding rectangle in dots. The enhancement processor
//! draws them (`enhance::apply_sprites`, ANIM-8); nothing here reads a page.
//!
//! The core keeps no clock (REQ-GEN-005): every tick is the caller's
//! presentation tick (ANIM-1).

use crate::consts::{COLUMNS, OBJECT_NUMBER_BITS, OBJECT_SOURCE_MASK, ROWS};
use crate::limits::{
    FIRST_SPRITE_LAYER, FIRST_SPRITE_ROW, KEEP_LAYER, LAST_FRAME, LAST_LAYER, LAST_SPRITE_ROW, MAX_ANIMATED_OBJECTS,
    MAX_CYCLE_PERIOD, MAX_CYCLING_OBJECTS, MAX_OBJECT_FRAMES, MAX_SPRITE_HEIGHT, MAX_SPRITE_WIDTH, MAX_SPRITES,
    SPRITE_COLUMN_REACH, SPRITE_ROW_REACH,
};
use crate::packet::{ObjectKind, ObjectNumber, ObjectSource};
use crate::types::{CellRect, DotRect, FixedVec};

/// Where the source lies in a sprite's 16-bit object reference: bits 12 and
/// 11 (ANIM-7).
const REFERENCE_SOURCE_SHIFT: u32 = 11;
/// Where the type lies: bits 10 and 9, above the nine bits of the number.
const REFERENCE_KIND_SHIFT: u32 = OBJECT_NUMBER_BITS;
/// The nine bits N8 to N0 of the object number.
const REFERENCE_NUMBER_MASK: u16 = (1 << OBJECT_NUMBER_BITS) - 1;
/// Bits 15 to 13, which are zero.
const REFERENCE_RESERVED: u16 = !((1 << (REFERENCE_SOURCE_SHIFT + OBJECT_SOURCE_MASK.count_ones())) - 1);

/// The cells a sprite may write: columns 0 to 39 of rows 1 to 23 (ANIM-8).
const SPRITE_AREA: CellRect = CellRect {
    row: FIRST_SPRITE_ROW,
    column: 0,
    rows: LAST_SPRITE_ROW - FIRST_SPRITE_ROW + 1,
    columns: CellRect::PAGE.columns,
};

/// One frame of a sprite: the object it invokes, with the fields of an
/// Object Invocation triplet (EN 300 706 Table 28; ANIM-7).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct FrameRef {
    /// A local object of the page's own packets X/26, or an object of its
    /// Public or Global Public Object Page; never [`ObjectSource::Illegal`].
    pub source: ObjectSource,
    /// Active, Adaptive or Passive.
    pub kind: ObjectKind,
    /// The object number, N8 to N0.
    pub number: ObjectNumber,
}

impl FrameRef {
    /// The filler of an unused frame: never read.
    const NONE: Self = Self { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(0) };

    /// The frame of a SPRITES record's two bytes (ANIM-7): bits 12 and 11
    /// the source, bits 10 and 9 the type, bits 8 to 0 the number. `None`
    /// for a reserved bit, source 0 or type 0, which a record's reader
    /// refuses.
    #[must_use]
    pub const fn from_reference(reference: u16) -> Option<Self> {
        if reference & REFERENCE_RESERVED != 0 {
            return None;
        }
        let source = ObjectSource::from_bits((reference >> REFERENCE_SOURCE_SHIFT) as u8);
        let Some(kind) = ObjectKind::from_bits((reference >> REFERENCE_KIND_SHIFT) as u8) else { return None };
        if matches!(source, ObjectSource::Illegal) {
            return None;
        }
        Some(Self { source, kind, number: ObjectNumber(reference & REFERENCE_NUMBER_MASK) })
    }

    /// The two bytes of the record, the inverse of
    /// [`FrameRef::from_reference`].
    #[must_use]
    pub const fn reference(self) -> u16 {
        ((self.source.bits() as u16) << REFERENCE_SOURCE_SHIFT)
            | ((self.kind.bits() as u16) << REFERENCE_KIND_SHIFT)
            | (self.number.0 & REFERENCE_NUMBER_MASK)
    }
}

/// Why a sprite, or a sub-page's set of them, is refused (ANIM-7,
/// ANIM-16): a record that defines more than a maximum, or a field outside
/// its range, is refused whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpriteError {
    /// More than [`MAX_SPRITES`] sprites.
    TooMany,
    /// A sprite's number is 0.
    Number,
    /// Two sprites have one number.
    RepeatedNumber,
    /// A layer outside 1 to 3.
    Layer,
    /// An origin column outside −39 to 39, or row outside −24 to 24.
    Origin,
    /// A width outside 1 to 40, or a height outside 1 to 25.
    Size,
    /// No frame, or a frame whose source is illegal.
    Frames,
}

/// One sprite, as a SPRITES record gives it (ANIM-7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sprite {
    /// Its number, 1 to 65,535, distinct within the sub-page.
    pub number: u16,
    /// Drawn, unless the animation state says otherwise.
    pub shown: bool,
    /// Pointer triggers see it.
    pub detectable: bool,
    /// Its layer, 1 to 3: layer 0 is the page itself (ANIM-12).
    pub layer: u8,
    /// Its order within the layer, lowest drawn first.
    pub order: i8,
    /// The column of its origin, −39 to 39.
    pub column: i8,
    /// The row of its origin, −24 to 24.
    pub row: i8,
    /// Cells across, 1 to 40.
    pub width: u8,
    /// Rows down, 1 to 25.
    pub height: u8,
    /// Its frames, 1 to [`MAX_OBJECT_FRAMES`].
    pub frames: FixedVec<FrameRef, MAX_OBJECT_FRAMES>,
}

impl Sprite {
    /// The filler of an unused slot: never read.
    const NONE: Self = Self {
        number: 0,
        shown: false,
        detectable: false,
        layer: FIRST_SPRITE_LAYER,
        order: 0,
        column: 0,
        row: 0,
        width: 1,
        height: 1,
        frames: FixedVec::new(FrameRef::NONE),
    };

    /// A shown sprite of one frame, on layer 2 (the sprites' layer, ANIM-12)
    /// at order 0, not detectable.
    #[must_use]
    pub fn new(number: u16, (row, column): (i8, i8), (height, width): (u8, u8), frame: FrameRef) -> Self {
        let mut frames = FixedVec::new(FrameRef::NONE);
        let _ = frames.push(frame);
        Self { number, shown: true, layer: 2, row, column, width, height, frames, ..Self::NONE }
    }

    /// Checks every field against its range in ANIM-7.
    ///
    /// # Errors
    /// The [`SpriteError`] of the first field out of range.
    pub fn check(&self) -> Result<(), SpriteError> {
        if self.number == 0 {
            return Err(SpriteError::Number);
        }
        if !(FIRST_SPRITE_LAYER..=LAST_LAYER).contains(&self.layer) {
            return Err(SpriteError::Layer);
        }
        if self.column.unsigned_abs() > SPRITE_COLUMN_REACH.unsigned_abs()
            || self.row.unsigned_abs() > SPRITE_ROW_REACH.unsigned_abs()
        {
            return Err(SpriteError::Origin);
        }
        if !(1..=MAX_SPRITE_WIDTH).contains(&self.width) || !(1..=MAX_SPRITE_HEIGHT).contains(&self.height) {
            return Err(SpriteError::Size);
        }
        let frames = self.frames.as_slice();
        if frames.is_empty() || frames.iter().any(|frame| frame.source == ObjectSource::Illegal) {
            return Err(SpriteError::Frames);
        }
        Ok(())
    }
}

/// All the sprites of a sub-page (ANIM-7): what its newest SPRITES record
/// gave, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sprites {
    list: FixedVec<Sprite, MAX_SPRITES>,
}

impl Default for Sprites {
    fn default() -> Self {
        Self::NONE
    }
}

impl Sprites {
    /// No sprites.
    pub const NONE: Self = Self { list: FixedVec::new(Sprite::NONE) };

    /// The sprites of a record, which replace any the sub-page had. The
    /// caller forgets the animation state of the sprites replaced
    /// ([`Animation::clear_objects`], ANIM-6).
    ///
    /// # Errors
    /// [`SpriteError::TooMany`] for more than [`MAX_SPRITES`],
    /// [`SpriteError::RepeatedNumber`] for a number given twice, and
    /// [`Sprite::check`]'s error for a field out of range. Nothing of a
    /// refused set is kept (ANIM-16).
    pub fn new(sprites: &[Sprite]) -> Result<Self, SpriteError> {
        let mut list = FixedVec::new(Sprite::NONE);
        for (index, sprite) in sprites.iter().enumerate() {
            sprite.check()?;
            if sprites.iter().take(index).any(|earlier| earlier.number == sprite.number) {
                return Err(SpriteError::RepeatedNumber);
            }
            if !list.push(*sprite) {
                return Err(SpriteError::TooMany);
            }
        }
        Ok(Self { list })
    }

    /// The sprites, in the record's order.
    #[must_use]
    pub fn as_slice(&self) -> &[Sprite] {
        self.list.as_slice()
    }

    /// The sprite numbered `number`.
    #[must_use]
    pub fn get(&self, number: u16) -> Option<&Sprite> {
        self.as_slice().iter().find(|sprite| sprite.number == number)
    }
}

/// What an operation or a state names (ANIM-6; page-rules.md RULE-14): a
/// control of the page, numbered from 1 in the order the page declares
/// them, or an animation object, here a sprite by its number.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Target {
    /// A teletext control (`controls::Controls`), 1 to 32.
    Control(u16),
    /// A sprite, 1 to 65,535.
    Object(u16),
}

/// The look forced on a control or an object (ANIM-6).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum ForcedLook {
    /// 0: none, the pointer decides the look.
    #[default]
    None,
    /// 1: the hover look.
    Hover,
    /// 2: the pressed look.
    Pressed,
    /// 3: the alternative look, which a teletext control draws as its
    /// pressed look, having two looks (decision E.127).
    Alternative,
}

impl ForcedLook {
    /// The look of an operation's look byte, 0 to 3.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::None),
            1 => Some(Self::Hover),
            2 => Some(Self::Pressed),
            3 => Some(Self::Alternative),
            _ => None,
        }
    }

    /// The look byte.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Hover => 1,
            Self::Pressed => 2,
            Self::Alternative => 3,
        }
    }
}

/// A cycle of an object's frames (ANIM-14).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Cycle {
    /// The first frame.
    pub first: u8,
    /// The last frame, `first` to 15.
    pub last: u8,
    /// Ticks a frame is shown, 1 to 3,600.
    pub period: u16,
    /// Run from `first` to `last` once and end there, instead of looping.
    pub once: bool,
    /// The tick the cycle started: the tick after its operation ran.
    pub start: u32,
}

impl Cycle {
    /// The frame at tick `tick` (ANIM-14): with `s` the start, `p` the
    /// period and `c = last − first + 1`, `first + ((t − s) ÷ p) mod c`,
    /// or with `once`, `min(first + (t − s) ÷ p, last)`. A tick before the
    /// start shows `first`.
    #[must_use]
    pub const fn frame_at(&self, tick: u32) -> u8 {
        let steps = match (tick.saturating_sub(self.start)).checked_div(self.period as u32) {
            Some(steps) => steps,
            None => 0,
        };
        let span = self.last.saturating_sub(self.first) as u32;
        let ahead = if self.once {
            if steps < span { steps } else { span }
        } else {
            match steps.checked_rem(span.saturating_add(1)) {
                Some(ahead) => ahead,
                None => 0,
            }
        };
        // `ahead` is at most `span`, so the sum is at most `last`.
        self.first.saturating_add(ahead as u8)
    }

    /// True when a cycle that runs once has reached its last frame, and so
    /// has ended.
    #[must_use]
    pub const fn ended_at(&self, tick: u32) -> bool {
        self.once && self.frame_at(tick) >= self.last
    }

    /// The first tick after `tick` at which the frame changes; `None` for
    /// a cycle of one frame, one that has ended, or when the tick counter
    /// ends first.
    #[must_use]
    pub fn next_change(&self, tick: u32) -> Option<u32> {
        if self.first >= self.last || self.ended_at(tick) || self.period == 0 {
            return None;
        }
        let period = u32::from(self.period);
        let Some(elapsed) = tick.checked_sub(self.start) else { return Some(self.start.saturating_add(period)) };
        let into = elapsed.checked_rem(period).unwrap_or(0);
        tick.checked_add(period.saturating_sub(into))
    }
}

/// The animation state of one control or object (ANIM-6). A part that is
/// `None` leaves the object's own value in force.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct AnimationState {
    /// Shown or hidden; `None`, the object's own visibility.
    pub shown: Option<bool>,
    /// The position, x the column and y the row, in cells; `None`, the
    /// object's own.
    pub position: Option<(i16, i16)>,
    /// The layer, 0 to 3; `None`, the object's own.
    pub layer: Option<u8>,
    /// The order within the layer; `None`, the object's own.
    pub order: Option<i8>,
    /// The frame in force, 0 to 15: a frame beyond an object's frames
    /// shows its last (ANIM-14).
    pub frame: u8,
    /// The cycle that changes the frame by itself; `None`, not cycling.
    pub cycle: Option<Cycle>,
    /// The forced look.
    pub look: ForcedLook,
}

impl AnimationState {
    /// No state: the object as it is without one.
    pub const NONE: Self =
        Self { shown: None, position: None, layer: None, order: None, frame: 0, cycle: None, look: ForcedLook::None };
}

/// An operation on the animation state (ANIM-6), decoded: what a rule's
/// action or an ANIMATE record holds, whose bytes `neoteletext-rules`
/// reads. The code in brackets is the operation's code byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Operation {
    /// `show` (5): controls and objects.
    Show,
    /// `hide` (6): controls and objects.
    Hide,
    /// `look` (7): controls and objects.
    Look(ForcedLook),
    /// `move` (15): objects. A relative move adds to the position in
    /// force, saturating at the signed 16-bit bounds.
    Move {
        /// Add to the position in force instead of replacing it.
        relative: bool,
        /// The column, or the columns to move by.
        x: i16,
        /// The row, or the rows to move by.
        y: i16,
    },
    /// `order` (16): objects.
    Order {
        /// The layer, 0 to 3; `None` keeps it (the byte 255).
        layer: Option<u8>,
        /// The order within the layer.
        order: i8,
    },
    /// `frame` (17): objects; 0 to 15, and stops a cycle.
    Frame(u8),
    /// `cycle` (18): objects.
    Cycle {
        /// The first frame.
        first: u8,
        /// The last frame, `first` to 15.
        last: u8,
        /// Ticks a frame, 1 to 3,600.
        period: u16,
        /// Run once and end at `last`.
        once: bool,
    },
    /// `stop_cycle` (19): objects.
    StopCycle,
    /// `reset` (20): forgets the whole animation state; controls and
    /// objects, in an ANIMATE record only.
    Reset,
}

impl Operation {
    /// The `order` operation of its two bytes: a layer byte of 0 to 3, or
    /// 255 to keep the layer; `None` for any other layer byte.
    #[must_use]
    pub const fn order(layer: u8, order: i8) -> Option<Self> {
        match layer {
            KEEP_LAYER => Some(Self::Order { layer: None, order }),
            0..=LAST_LAYER => Some(Self::Order { layer: Some(layer), order }),
            _ => None,
        }
    }

    /// True for the operations that act on objects alone.
    const fn objects_only(self) -> bool {
        !matches!(self, Self::Show | Self::Hide | Self::Look(_) | Self::Reset)
    }

    /// True when every field is inside its range in ANIM-6.
    const fn in_range(self) -> bool {
        match self {
            Self::Order { layer: Some(layer), .. } => layer <= LAST_LAYER,
            Self::Frame(frame) => frame <= LAST_FRAME,
            Self::Cycle { first, last, period, .. } => {
                first <= last && last <= LAST_FRAME && period >= 1 && period <= MAX_CYCLE_PERIOD
            }
            _ => true,
        }
    }
}

/// What applying an operation did (ANIM-6, ANIM-14).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Applied {
    /// The state was written.
    Done,
    /// The target does not exist: nothing was done, and it was counted.
    NoTarget,
    /// The operation does not act on a control, or a field is outside its
    /// range: nothing was done. A record's reader refuses these first.
    Refused,
    /// [`MAX_ANIMATED_OBJECTS`] targets hold state already, or
    /// [`MAX_CYCLING_OBJECTS`] objects cycle already: nothing was done,
    /// and it was counted.
    Full,
}

/// The animation state of a sub-page's sprites and controls (ANIM-6): at
/// most [`MAX_ANIMATED_OBJECTS`] of them hold state at once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Animation {
    states: FixedVec<(Target, AnimationState), MAX_ANIMATED_OBJECTS>,
    /// Operations that did nothing because their target does not exist, or
    /// no room was left for its state or its cycle (ANIM-6, ANIM-14): the
    /// receiver reports them (page-rules.md RULE-48, fault 5).
    pub skipped: u32,
}

impl Default for Animation {
    fn default() -> Self {
        Self::NONE
    }
}

impl Animation {
    /// No state.
    pub const NONE: Self = Self { states: FixedVec::new((Target::Object(0), AnimationState::NONE)), skipped: 0 };

    /// The state of `target`, when it holds any.
    #[must_use]
    pub fn state(&self, target: Target) -> Option<&AnimationState> {
        self.states.as_slice().iter().find(|(held, _)| *held == target).map(|(_, state)| state)
    }

    /// Every target that holds state, with its state.
    #[must_use]
    pub fn states(&self) -> &[(Target, AnimationState)] {
        self.states.as_slice()
    }

    /// Gives `target` a whole state, replacing any it had: for a caller
    /// that keeps the state itself and hands it over (ANIM-22). False,
    /// changing nothing, when [`MAX_ANIMATED_OBJECTS`] others hold state,
    /// or the state cycles and [`MAX_CYCLING_OBJECTS`] others do.
    pub fn set(&mut self, target: Target, state: AnimationState) -> bool {
        let cycling_already = self.state(target).is_some_and(|held| held.cycle.is_some());
        if state.cycle.is_some() && !cycling_already && self.cycling() >= MAX_CYCLING_OBJECTS {
            return false;
        }
        if let Some((_, held)) = self.states.as_mut_slice().iter_mut().find(|(held, _)| *held == target) {
            *held = state;
            return true;
        }
        self.states.push((target, state))
    }

    /// Forgets the state of `target` (the `reset` operation).
    pub fn clear(&mut self, target: Target) {
        if let Some(index) = self.states.as_slice().iter().position(|(held, _)| *held == target) {
            let _ = self.states.remove(index);
        }
    }

    /// Forgets the state of every object: a SPRITES record replaced the
    /// sub-page's sprites (ANIM-6, "last writer wins"). Controls keep
    /// theirs.
    pub fn clear_objects(&mut self) {
        while let Some(index) = self.states.as_slice().iter().position(|(held, _)| matches!(held, Target::Object(_))) {
            let _ = self.states.remove(index);
        }
    }

    /// Forgets everything: another page or sub-page is shown.
    pub fn clear_all(&mut self) {
        self.states.clear();
    }

    /// The objects that cycle now.
    #[must_use]
    pub fn cycling(&self) -> usize {
        self.states.as_slice().iter().filter(|(_, state)| state.cycle.is_some()).count()
    }

    /// Applies one operation to `target` at tick `tick` (ANIM-6). The
    /// target exists when it is a sprite of `sprites`, or a control
    /// numbered 1 to `controls`, the count of the page's controls. A
    /// cycle starts at the tick after this one (ANIM-14).
    pub fn apply(
        &mut self,
        target: Target,
        operation: Operation,
        tick: u32,
        sprites: &Sprites,
        controls: usize,
    ) -> Applied {
        let sprite = match target {
            Target::Control(number) => {
                if number == 0 || usize::from(number) > controls {
                    return self.skip(Applied::NoTarget);
                }
                if operation.objects_only() {
                    return Applied::Refused;
                }
                None
            }
            Target::Object(number) => match sprites.get(number) {
                Some(sprite) => Some(sprite),
                None => return self.skip(Applied::NoTarget),
            },
        };
        if !operation.in_range() {
            return Applied::Refused;
        }
        if operation == Operation::Reset {
            self.clear(target);
            return Applied::Done;
        }
        let mut state = self.state(target).copied().unwrap_or(AnimationState::NONE);
        match operation {
            Operation::Show => state.shown = Some(true),
            Operation::Hide => state.shown = Some(false),
            Operation::Look(look) => state.look = look,
            Operation::Move { relative, x, y } => {
                let own = sprite.map_or((0, 0), |sprite| (i16::from(sprite.column), i16::from(sprite.row)));
                let (from_x, from_y) = if relative { state.position.unwrap_or(own) } else { (0, 0) };
                state.position = Some((from_x.saturating_add(x), from_y.saturating_add(y)));
            }
            Operation::Order { layer, order } => {
                state.layer = layer.or(state.layer);
                state.order = Some(order);
            }
            Operation::Frame(frame) => {
                state.frame = frame;
                state.cycle = None;
            }
            Operation::Cycle { first, last, period, once } => {
                state.frame = first;
                state.cycle = Some(Cycle { first, last, period, once, start: tick.saturating_add(1) });
            }
            Operation::StopCycle => state.cycle = None,
            Operation::Reset => {}
        }
        if self.set(target, state) { Applied::Done } else { self.skip(Applied::Full) }
    }

    /// Counts an operation that did nothing.
    const fn skip(&mut self, outcome: Applied) -> Applied {
        self.skipped = self.skipped.saturating_add(1);
        outcome
    }

    /// Advances every cycle to tick `tick` (ANIM-14): each cycling
    /// object's frame becomes its cycle's frame at that tick, and a cycle
    /// that runs once ends at its last frame. True when a frame changed,
    /// so that the caller composes the sprites again. Cycles run whether
    /// or not the page has a rule table.
    pub fn advance(&mut self, tick: u32) -> bool {
        let mut changed = false;
        for (_, state) in self.states.as_mut_slice() {
            let Some(cycle) = state.cycle else { continue };
            let frame = cycle.frame_at(tick);
            changed |= frame != state.frame;
            state.frame = frame;
            if cycle.ended_at(tick) {
                state.cycle = None;
            }
        }
        changed
    }

    /// The first tick after `tick` at which a cycle changes a frame
    /// (ANIM-1: a decoder wakes for a cycle's next frame); `None` when
    /// nothing cycles.
    #[must_use]
    pub fn next_frame_tick(&self, tick: u32) -> Option<u32> {
        self.states.as_slice().iter().filter_map(|(_, state)| state.cycle?.next_change(tick)).min()
    }

    /// Whether the control numbered `number` is shown: true unless its
    /// state hides it. A hidden control is drawn as spaces and takes no
    /// pointer event (ANIM-6).
    #[must_use]
    pub fn control_shown(&self, number: u16) -> bool {
        self.state(Target::Control(number)).and_then(|state| state.shown).unwrap_or(true)
    }

    /// The look forced on `target`; [`ForcedLook::None`] leaves the look to
    /// the pointer.
    #[must_use]
    pub fn forced_look(&self, target: Target) -> ForcedLook {
        self.state(target).map_or(ForcedLook::None, |state| state.look)
    }
}

/// One shown sprite as it is drawn: the record's sprite with its animation
/// state in force (ANIM-8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    /// The sprite's number.
    pub number: u16,
    /// Its layer, 0 to 3.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
    /// The column of its origin.
    pub column: i16,
    /// The row of its origin.
    pub row: i16,
    /// Cells across.
    pub width: u8,
    /// Rows down.
    pub height: u8,
    /// The object of its frame in force.
    pub frame: FrameRef,
    /// Pointer triggers see it.
    pub detectable: bool,
}

impl Placement {
    /// The filler of an unused slot: never read.
    const NONE: Self = Self {
        number: 0,
        layer: 0,
        order: 0,
        column: 0,
        row: 0,
        width: 1,
        height: 1,
        frame: FrameRef::NONE,
        detectable: false,
    };

    /// Its cells from its origin, cut to `bounds`.
    fn cut_to(&self, bounds: CellRect) -> Option<CellRect> {
        let (top, left) = (i32::from(self.row), i32::from(self.column));
        let bottom = top.saturating_add(i32::from(self.height)).saturating_sub(1);
        let right = left.saturating_add(i32::from(self.width)).saturating_sub(1);
        CellRect::cut_to(top, left, bottom, right, bounds)
    }

    /// The cells it may write (ANIM-8): its width and height from its
    /// origin, inside columns 0 to 39 and rows 1 to 23; `None` when none
    /// of it is there.
    #[must_use]
    pub fn window(&self) -> Option<CellRect> {
        self.cut_to(SPRITE_AREA)
    }

    /// Its bounding rectangle (ANIM-8, ANIM-13): its width and height in
    /// cells at its origin, cut to the page, in dots of 12 by 20 a cell;
    /// `None` when none of it is on the page.
    #[must_use]
    pub fn bounds(&self) -> Option<DotRect> {
        self.cut_to(CellRect::PAGE).map(CellRect::dots)
    }
}

/// The shown sprites of a sub-page in drawing order (ANIM-8): by layer,
/// then order, then number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placements {
    list: FixedVec<Placement, MAX_SPRITES>,
}

impl Default for Placements {
    fn default() -> Self {
        Self::NONE
    }
}

impl Placements {
    /// Nothing to draw.
    pub const NONE: Self = Self { list: FixedVec::new(Placement::NONE) };

    /// The sprites of `sprites` that are shown, each with the parts of its
    /// animation state in force in place of its own (ANIM-6), in drawing
    /// order. A frame number beyond a sprite's frames shows its last
    /// (ANIM-14).
    #[must_use]
    pub fn of(sprites: &Sprites, animation: &Animation) -> Self {
        let mut list = FixedVec::new(Placement::NONE);
        for sprite in sprites.as_slice() {
            let state = animation.state(Target::Object(sprite.number)).copied().unwrap_or(AnimationState::NONE);
            let frames = sprite.frames.as_slice();
            let frame = frames.get(usize::from(state.frame)).or(frames.last()).copied();
            let Some(frame) = frame.filter(|_| state.shown.unwrap_or(sprite.shown)) else { continue };
            let (column, row) = state.position.unwrap_or((i16::from(sprite.column), i16::from(sprite.row)));
            let placement = Placement {
                number: sprite.number,
                layer: state.layer.unwrap_or(sprite.layer),
                order: state.order.unwrap_or(sprite.order),
                column,
                row,
                width: sprite.width,
                height: sprite.height,
                frame,
                detectable: sprite.detectable,
            };
            // The list holds as many as a sub-page has sprites.
            let _ = list.push(placement);
        }
        list.as_mut_slice().sort_unstable_by_key(|placement| (placement.layer, placement.order, placement.number));
        Self { list }
    }

    /// The placements, in drawing order.
    #[must_use]
    pub fn as_slice(&self) -> &[Placement] {
        self.list.as_slice()
    }

    /// True when no sprite is shown.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The bounding rectangle of the sprite numbered `number`, in dots
    /// (ANIM-13); `None` while it does not exist, is not shown, or lies
    /// wholly off the page.
    #[must_use]
    pub fn bounds(&self, number: u16) -> Option<DotRect> {
        self.as_slice().iter().find(|placement| placement.number == number)?.bounds()
    }

    /// The numbers of the detectable sprites whose bounding rectangle
    /// holds the dot at `x`, `y`, in drawing order (page-rules.md RULE-12:
    /// a pointer is inside each of them).
    pub fn detectable_at(&self, x: u16, y: u16) -> impl Iterator<Item = u16> + '_ {
        self.as_slice()
            .iter()
            .filter(move |placement| {
                placement.detectable && placement.bounds().is_some_and(|bounds| bounds.contains(x, y))
            })
            .map(|placement| placement.number)
    }

    /// The smallest rectangle of cells holding every cell a sprite may
    /// write: what the sprites cover now, for the damage of ANIM-4.
    #[must_use]
    pub fn covered(&self) -> Option<CellRect> {
        self.as_slice().iter().fold(None, |covered, placement| CellRect::join(covered, placement.window()))
    }

    /// Whether the cell at `row`, `column` is one a sprite may write.
    #[must_use]
    pub fn covers(&self, row: usize, column: usize) -> bool {
        self.as_slice().iter().any(|placement| placement.window().is_some_and(|window| window.contains(row, column)))
    }
}

/// The cells sprites changed on a grid: where the presented page differs
/// from the page's own rendering (ANIM-8, ANIM-12).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpriteCells {
    /// One word a row, bit c set for column c.
    rows: [u64; ROWS],
}

impl Default for SpriteCells {
    fn default() -> Self {
        Self::NONE
    }
}

impl SpriteCells {
    /// No cell.
    pub const NONE: Self = Self { rows: [0; ROWS] };

    /// Marks the cell at `row`, `column`; a cell off the page is no cell.
    pub fn insert(&mut self, row: usize, column: usize) {
        if let Some(cells) = self.rows.get_mut(row).filter(|_| column < COLUMNS) {
            *cells |= 1u64.checked_shl(column as u32).unwrap_or(0);
        }
    }

    /// Whether a sprite changed the cell at `row`, `column`.
    #[must_use]
    pub fn contains(&self, row: usize, column: usize) -> bool {
        self.rows.get(row).is_some_and(|cells| cells.checked_shr(column as u32).is_some_and(|bits| bits & 1 == 1))
    }

    /// True when no sprite changed any cell.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|&cells| cells == 0)
    }

    /// The cells changed.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.rows.iter().map(|cells| cells.count_ones()).sum()
    }
}

#[cfg(test)]
mod tests;
