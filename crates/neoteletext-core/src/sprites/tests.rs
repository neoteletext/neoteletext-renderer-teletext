//! Tests of sprites and the animation state (animation.md ANIM-6, ANIM-7,
//! ANIM-13, ANIM-14 and ANIM-16; REQ-6529).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use super::*;

use std::vec::Vec;

fn passive(number: u16) -> FrameRef {
    FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(number) }
}

fn sprite(number: u16) -> Sprite {
    Sprite::new(number, (5, 10), (3, 4), passive(0))
}

fn with_frames(number: u16, count: u16) -> Sprite {
    let mut sprite = sprite(number);
    sprite.frames.clear();
    for frame in 0..count {
        assert!(sprite.frames.push(passive(frame)));
    }
    sprite
}

fn object(number: u16) -> Target {
    Target::Object(number)
}

/// ANIM-7 (REQ-6529): a frame's two bytes hold the source in bits 12 and
/// 11, the type in bits 10 and 9 and the number in bits 8 to 0; a reserved
/// bit, source 0 and type 0 are refused.
#[test]
fn a_frame_reference_is_the_fields_of_an_object_invocation() {
    let sources = [(1, ObjectSource::Local), (2, ObjectSource::Public), (3, ObjectSource::Global)];
    let kinds = [(1, ObjectKind::Active), (2, ObjectKind::Adaptive), (3, ObjectKind::Passive)];
    for (source_bits, source) in sources {
        for (kind_bits, kind) in kinds {
            for number in [0u16, 1, 0x0FF, 0x1FF] {
                let reference = source_bits << 11 | kind_bits << 9 | number;
                let frame = FrameRef::from_reference(reference).unwrap();
                assert_eq!(frame, FrameRef { source, kind, number: ObjectNumber(number) });
                assert_eq!(frame.reference(), reference);
            }
        }
    }
    assert_eq!(FrameRef::from_reference(0b011 << 9 | 5), None, "source 0");
    assert_eq!(FrameRef::from_reference(0b01 << 11 | 5), None, "type 0");
    for reserved in [13, 14, 15] {
        assert_eq!(FrameRef::from_reference(1 << reserved | 0b0111 << 9), None, "bit {reserved}");
    }
}

/// ANIM-7, ANIM-16 (REQ-6529): each field of a sprite at its bounds, and a
/// set refused whole for a repeated number or a 33rd sprite.
#[test]
fn sprites_are_checked_at_each_bound() {
    let good = |change: fn(&mut Sprite)| {
        let mut sprite = sprite(1);
        change(&mut sprite);
        sprite.check()
    };
    assert_eq!(good(|_| {}), Ok(()));
    assert_eq!(good(|sprite| sprite.number = 0), Err(SpriteError::Number));
    assert_eq!(good(|sprite| sprite.number = u16::MAX), Ok(()));
    assert_eq!(good(|sprite| sprite.layer = 0), Err(SpriteError::Layer), "layer 0 is the page");
    assert_eq!(good(|sprite| sprite.layer = 1), Ok(()));
    assert_eq!(good(|sprite| sprite.layer = 3), Ok(()));
    assert_eq!(good(|sprite| sprite.layer = 4), Err(SpriteError::Layer));
    assert_eq!(good(|sprite| sprite.column = -39), Ok(()));
    assert_eq!(good(|sprite| sprite.column = 39), Ok(()));
    assert_eq!(good(|sprite| sprite.column = -40), Err(SpriteError::Origin));
    assert_eq!(good(|sprite| sprite.column = 40), Err(SpriteError::Origin));
    assert_eq!(good(|sprite| sprite.row = -24), Ok(()));
    assert_eq!(good(|sprite| sprite.row = 24), Ok(()));
    assert_eq!(good(|sprite| sprite.row = -25), Err(SpriteError::Origin));
    assert_eq!(good(|sprite| sprite.row = 25), Err(SpriteError::Origin));
    assert_eq!(good(|sprite| sprite.width = 0), Err(SpriteError::Size));
    assert_eq!(good(|sprite| sprite.width = 40), Ok(()));
    assert_eq!(good(|sprite| sprite.width = 41), Err(SpriteError::Size));
    assert_eq!(good(|sprite| sprite.height = 0), Err(SpriteError::Size));
    assert_eq!(good(|sprite| sprite.height = 25), Ok(()));
    assert_eq!(good(|sprite| sprite.height = 26), Err(SpriteError::Size));
    assert_eq!(good(|sprite| sprite.frames.clear()), Err(SpriteError::Frames));
    assert_eq!(with_frames(1, 16).check(), Ok(()));
    assert_eq!(good(|sprite| sprite.frames.as_mut_slice()[0].source = ObjectSource::Illegal), Err(SpriteError::Frames));

    let many: Vec<Sprite> = (1..=33).map(sprite).collect();
    assert_eq!(Sprites::new(&many[..32]).unwrap().as_slice().len(), MAX_SPRITES);
    assert_eq!(Sprites::new(&many), Err(SpriteError::TooMany));
    assert_eq!(Sprites::new(&[sprite(7), sprite(8), sprite(7)]), Err(SpriteError::RepeatedNumber));
    assert_eq!(Sprites::new(&[]).unwrap(), Sprites::NONE);
    let set = Sprites::new(&[sprite(7), sprite(8)]).unwrap();
    assert_eq!(set.get(8).map(|sprite| sprite.number), Some(8));
    assert!(set.get(9).is_none());
}

/// ANIM-6 (REQ-6529): each operation writes its part of the state.
#[test]
fn each_operation_changes_its_part_of_the_state() {
    let sprites = Sprites::new(&[with_frames(10, 4)]).unwrap();
    let mut animation = Animation::NONE;
    let mut apply = |operation| animation.apply(object(10), operation, 100, &sprites, 0);
    assert_eq!(apply(Operation::Hide), Applied::Done);
    assert_eq!(apply(Operation::Look(ForcedLook::Pressed)), Applied::Done);
    assert_eq!(apply(Operation::Move { relative: false, x: 3, y: 7 }), Applied::Done);
    assert_eq!(apply(Operation::order(2, -5).unwrap()), Applied::Done);
    assert_eq!(apply(Operation::Frame(2)), Applied::Done);
    let state = *animation.state(object(10)).unwrap();
    let expected = AnimationState {
        shown: Some(false),
        position: Some((3, 7)),
        layer: Some(2),
        order: Some(-5),
        frame: 2,
        cycle: None,
        look: ForcedLook::Pressed,
    };
    assert_eq!(state, expected);

    let mut apply = |operation| animation.apply(object(10), operation, 100, &sprites, 0);
    assert_eq!(apply(Operation::Show), Applied::Done);
    assert_eq!(apply(Operation::order(255, 9).unwrap()), Applied::Done, "255 keeps the layer");
    assert_eq!(apply(Operation::Cycle { first: 1, last: 3, period: 6, once: false }), Applied::Done);
    let state = *animation.state(object(10)).unwrap();
    assert_eq!((state.shown, state.layer, state.order), (Some(true), Some(2), Some(9)));
    let cycle = Cycle { first: 1, last: 3, period: 6, once: false, start: 101 };
    assert_eq!((state.frame, state.cycle), (1, Some(cycle)), "a cycle starts at the tick after its operation");
    assert_eq!(animation.apply(object(10), Operation::Frame(0), 100, &sprites, 0), Applied::Done);
    assert_eq!(animation.state(object(10)).unwrap().cycle, None, "frame stops a cycle");
    let cycle = Operation::Cycle { first: 0, last: 3, period: 1, once: true };
    assert_eq!(animation.apply(object(10), cycle, 100, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(10), Operation::StopCycle, 100, &sprites, 0), Applied::Done);
    assert_eq!(animation.state(object(10)).unwrap().cycle, None);
    assert_eq!(animation.apply(object(10), Operation::Reset, 100, &sprites, 0), Applied::Done);
    assert_eq!(animation.state(object(10)), None, "reset forgets the whole state");
    assert_eq!(animation.skipped, 0);
}

/// ANIM-6 (REQ-6529): a relative move adds to the position in force, the
/// sprite's own when the state has none, saturating at the 16-bit bounds.
#[test]
fn a_relative_move_adds_to_the_position_in_force() {
    let sprites = Sprites::new(&[sprite(10)]).unwrap();
    let mut animation = Animation::NONE;
    let mut by = |x, y| {
        assert_eq!(
            animation.apply(object(10), Operation::Move { relative: true, x, y }, 0, &sprites, 0),
            Applied::Done
        );
        animation.state(object(10)).unwrap().position
    };
    assert_eq!(by(8, -2), Some((18, 3)), "from the sprite's own column 10 and row 5");
    assert_eq!(by(1, 1), Some((19, 4)));
    assert_eq!(by(i16::MAX, i16::MIN), Some((i16::MAX, 4 + i16::MIN)));
    assert_eq!(by(1, -5), Some((i16::MAX, i16::MIN)), "saturating");
}

/// ANIM-6 (REQ-6529): an operation on a target that does not exist does
/// nothing and is counted; one that does not act on a control, or is out
/// of range, is refused.
#[test]
fn operations_on_missing_targets_are_counted_and_others_refused() {
    let sprites = Sprites::new(&[sprite(10)]).unwrap();
    let mut animation = Animation::NONE;
    assert_eq!(animation.apply(object(11), Operation::Show, 0, &sprites, 2), Applied::NoTarget);
    assert_eq!(animation.apply(Target::Control(3), Operation::Hide, 0, &sprites, 2), Applied::NoTarget);
    assert_eq!(animation.apply(Target::Control(0), Operation::Hide, 0, &sprites, 2), Applied::NoTarget);
    assert_eq!(animation.skipped, 3);
    assert_eq!(animation.states(), &[]);
    assert_eq!(animation.apply(Target::Control(2), Operation::Hide, 0, &sprites, 2), Applied::Done);
    assert!(!animation.control_shown(2) && animation.control_shown(1));
    assert_eq!(animation.apply(Target::Control(2), Operation::Look(ForcedLook::Hover), 0, &sprites, 2), Applied::Done);
    assert_eq!(animation.forced_look(Target::Control(2)), ForcedLook::Hover);
    assert_eq!(animation.forced_look(Target::Control(1)), ForcedLook::None);
    for operation in [
        Operation::Move { relative: false, x: 0, y: 0 },
        Operation::Order { layer: None, order: 0 },
        Operation::Frame(0),
        Operation::Cycle { first: 0, last: 1, period: 1, once: false },
        Operation::StopCycle,
    ] {
        assert_eq!(animation.apply(Target::Control(1), operation, 0, &sprites, 2), Applied::Refused, "{operation:?}");
    }
    for operation in [
        Operation::Frame(16),
        Operation::Order { layer: Some(4), order: 0 },
        Operation::Cycle { first: 2, last: 1, period: 1, once: false },
        Operation::Cycle { first: 0, last: 16, period: 1, once: false },
        Operation::Cycle { first: 0, last: 1, period: 0, once: false },
        Operation::Cycle { first: 0, last: 1, period: 3_601, once: false },
    ] {
        assert_eq!(animation.apply(object(10), operation, 0, &sprites, 2), Applied::Refused, "{operation:?}");
    }
    assert_eq!(Operation::order(4, 0), None);
    assert_eq!(Operation::order(254, 0), None);
    assert_eq!(animation.skipped, 3, "a refused operation is the reader's to refuse, and is not counted");
    assert_eq!(ForcedLook::from_code(4), None);
    for code in 0..=3 {
        assert_eq!(ForcedLook::from_code(code).unwrap().code(), code);
    }
}

/// ANIM-14 (REQ-6529): the frame at tick t is first + ((t − s) ÷ p) mod c,
/// and with `once` min(first + (t − s) ÷ p, last), at their edges.
#[test]
fn both_cycle_formulas_at_their_edges() {
    let looping = Cycle { first: 2, last: 5, period: 6, once: false, start: 100 };
    for (tick, frame) in
        [(0, 2), (100, 2), (105, 2), (106, 3), (117, 4), (118, 5), (123, 5), (124, 2), (129, 2), (130, 3)]
    {
        assert_eq!(looping.frame_at(tick), frame, "looping, tick {tick}");
        assert!(!looping.ended_at(tick));
    }
    assert_eq!(looping.frame_at(u32::MAX), 2 + ((u32::MAX - 100) / 6 % 4) as u8);
    let once = Cycle { once: true, ..looping };
    for (tick, frame, ended) in
        [(100, 2, false), (106, 3, false), (117, 4, false), (118, 5, true), (124, 5, true), (u32::MAX, 5, true)]
    {
        assert_eq!((once.frame_at(tick), once.ended_at(tick)), (frame, ended), "once, tick {tick}");
    }
    let single = Cycle { first: 15, last: 15, period: 1, once: false, start: 0 };
    assert_eq!((single.frame_at(0), single.frame_at(77)), (15, 15));
    let slow = Cycle { first: 0, last: 15, period: 3_600, once: false, start: 0 };
    assert_eq!((slow.frame_at(3_599), slow.frame_at(3_600), slow.frame_at(16 * 3_600)), (0, 1, 0));
    assert_eq!(looping.next_change(50), Some(106), "before its start");
    assert_eq!(
        (looping.next_change(100), looping.next_change(105), looping.next_change(106)),
        (Some(106), Some(106), Some(112))
    );
    assert_eq!((once.next_change(117), once.next_change(118)), (Some(118), None));
    assert_eq!(single.next_change(0), None, "one frame never changes");
    assert_eq!(looping.next_change(u32::MAX), None);
}

/// ANIM-14 (REQ-6529): cycles advance by the tick; one that runs once ends
/// at its last frame.
#[test]
fn cycles_advance_by_the_tick() {
    let sprites = Sprites::new(&[with_frames(1, 4), with_frames(2, 4)]).unwrap();
    let mut animation = Animation::NONE;
    let looping = Operation::Cycle { first: 0, last: 3, period: 2, once: false };
    let once = Operation::Cycle { first: 1, last: 2, period: 5, once: true };
    assert_eq!(animation.apply(object(1), looping, 9, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(2), once, 9, &sprites, 0), Applied::Done);
    assert_eq!(animation.cycling(), 2);
    assert_eq!(animation.next_frame_tick(9), Some(12));
    let mut frames = Vec::new();
    for tick in 10..=20 {
        let changed = animation.advance(tick);
        let frame = |number| animation.state(object(number)).unwrap().frame;
        frames.push((frame(1), frame(2), changed));
    }
    let expected = [
        (0, 1, false),
        (0, 1, false),
        (1, 1, true),
        (1, 1, false),
        (2, 1, true),
        (2, 2, true),
        (3, 2, true),
        (3, 2, false),
        (0, 2, true),
        (0, 2, false),
        (1, 2, true),
    ];
    assert_eq!(frames, expected);
    assert_eq!(animation.cycling(), 1, "the cycle that runs once has ended");
    assert_eq!(animation.state(object(2)).unwrap().cycle, None);
    assert_eq!(animation.next_frame_tick(20), Some(22));
}

/// ANIM-6, ANIM-14, ANIM-16 (REQ-6529): at most 32 targets hold state and
/// 16 cycle; one more does nothing and is counted.
#[test]
fn the_limits_on_state_and_cycles_hold() {
    let many: Vec<Sprite> = (1..=32).map(|number| with_frames(number, 2)).collect();
    let sprites = Sprites::new(&many).unwrap();
    let mut animation = Animation::NONE;
    let cycle = Operation::Cycle { first: 0, last: 1, period: 1, once: false };
    for number in 1..=16 {
        assert_eq!(animation.apply(object(number), cycle, 0, &sprites, 4), Applied::Done);
    }
    assert_eq!(animation.apply(object(17), cycle, 0, &sprites, 4), Applied::Full, "a 17th cycle");
    assert_eq!((animation.cycling(), animation.skipped), (MAX_CYCLING_OBJECTS, 1));
    assert_eq!(animation.state(object(17)), None);
    assert_eq!(animation.apply(object(16), cycle, 5, &sprites, 4), Applied::Done, "an object cycling already");
    for number in 17..=32 {
        assert_eq!(animation.apply(object(number), Operation::Hide, 0, &sprites, 4), Applied::Done);
    }
    assert_eq!(animation.states().len(), MAX_ANIMATED_OBJECTS);
    assert_eq!(animation.apply(Target::Control(1), Operation::Hide, 0, &sprites, 4), Applied::Full, "a 33rd state");
    assert_eq!(animation.skipped, 2);
    assert!(animation.control_shown(1));
    animation.clear_objects();
    assert_eq!(animation.states(), &[], "a SPRITES record forgets the state of the sprites it replaces");
    assert_eq!(animation.apply(Target::Control(1), Operation::Hide, 0, &sprites, 4), Applied::Done);
    assert_eq!(animation.apply(object(1), Operation::Hide, 0, &sprites, 4), Applied::Done);
    animation.clear_objects();
    assert_eq!(animation.states().len(), 1, "controls keep theirs");
    animation.clear_all();
    assert_eq!(animation.states(), &[]);
}

/// ANIM-8, ANIM-12 (REQ-6529): the shown sprites in order of layer, then
/// order, then number, each with its animation state in force.
#[test]
fn placements_are_the_shown_sprites_in_drawing_order() {
    let mut list = Vec::new();
    for (number, layer, order) in [(5, 2, 0), (4, 2, 0), (3, 3, -128), (2, 1, 127), (1, 2, -1), (6, 1, 0)] {
        list.push(Sprite { layer, order, ..with_frames(number, 3) });
    }
    list[5].shown = false;
    let sprites = Sprites::new(&list).unwrap();
    let mut animation = Animation::NONE;
    let numbers = |animation: &Animation| -> Vec<u16> {
        Placements::of(&sprites, animation).as_slice().iter().map(|placement| placement.number).collect()
    };
    assert_eq!(numbers(&animation), [2, 1, 4, 5, 3], "sprite 6 is not shown");
    assert_eq!(animation.apply(object(6), Operation::Show, 0, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(4), Operation::Hide, 0, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(3), Operation::order(0, 0).unwrap(), 0, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(5), Operation::order(255, -2).unwrap(), 0, &sprites, 0), Applied::Done);
    assert_eq!(numbers(&animation), [3, 6, 2, 5, 1]);

    assert_eq!(animation.apply(object(1), Operation::Frame(1), 0, &sprites, 0), Applied::Done);
    assert_eq!(animation.apply(object(2), Operation::Frame(9), 0, &sprites, 0), Applied::Done);
    assert_eq!(
        animation.apply(object(2), Operation::Move { relative: false, x: -3, y: 30 }, 0, &sprites, 0),
        Applied::Done
    );
    let placements = Placements::of(&sprites, &animation);
    let of = |number| *placements.as_slice().iter().find(|placement| placement.number == number).unwrap();
    assert_eq!(of(1).frame, passive(1));
    assert_eq!(of(2).frame, passive(2), "a frame beyond a sprite's frames shows its last");
    assert_eq!((of(2).column, of(2).row), (-3, 30));
    assert_eq!((of(1).column, of(1).row, of(1).width, of(1).height), (10, 5, 4, 3));
    assert!(Placements::NONE.is_empty() && !placements.is_empty());
}

/// ANIM-8, ANIM-13 (REQ-6529): a sprite's bounding rectangle is its width
/// and height in cells at its origin, cut to the page, in dots of 12 by 20
/// a cell; it writes only columns 0 to 39 of rows 1 to 23.
#[test]
fn bounds_and_windows_are_cut_to_the_page() {
    let at = |row: i16, column: i16| {
        let sprites = Sprites::new(&[Sprite { detectable: true, ..sprite(1) }]).unwrap();
        let mut animation = Animation::NONE;
        let moved = Operation::Move { relative: false, x: column, y: row };
        assert_eq!(animation.apply(object(1), moved, 0, &sprites, 0), Applied::Done);
        Placements::of(&sprites, &animation)
    };
    let dots = |x, y, width, height| Some(DotRect { x, y, width, height });
    let cells = |row, column, rows, columns| Some(CellRect { row, column, rows, columns });
    // The sprite is 3 rows of 4 cells.
    assert_eq!(at(5, 10).bounds(1), dots(120, 100, 48, 60));
    assert_eq!(at(5, 10).covered(), cells(5, 10, 3, 4));
    assert_eq!(at(-1, -2).bounds(1), dots(0, 0, 24, 40), "cut at the top and the left");
    assert_eq!(at(-1, -2).covered(), cells(1, 0, 1, 2), "row 0 is never written");
    assert_eq!(at(23, 38).bounds(1), dots(456, 460, 24, 40), "cut at the bottom and the right");
    assert_eq!(at(23, 38).covered(), cells(23, 38, 1, 2), "row 24 is never written");
    assert_eq!(at(24, 0).bounds(1), dots(0, 480, 48, 20), "on row 24 it has a rectangle");
    assert_eq!(at(24, 0).covered(), None, "and writes nothing");
    assert_eq!(at(-2, 0).bounds(1), dots(0, 0, 48, 20));
    assert_eq!(at(-2, 0).covered(), None);
    for (row, column) in [(-3, 0), (25, 0), (0, -4), (0, 40), (i16::MIN, i16::MIN), (i16::MAX, i16::MAX)] {
        assert_eq!(at(row, column).bounds(1), None, "off the page at {row}, {column}");
        assert_eq!(at(row, column).covered(), None);
    }
    assert_eq!(at(5, 10).bounds(2), None, "no such sprite");
    let hidden = Sprites::new(&[Sprite { shown: false, ..sprite(1) }]).unwrap();
    assert_eq!(Placements::of(&hidden, &Animation::NONE).bounds(1), None, "a sprite not shown has no rectangle");

    let placements = at(5, 10);
    assert!(placements.covers(5, 10) && placements.covers(7, 13));
    assert!(
        !placements.covers(4, 10) && !placements.covers(8, 10) && !placements.covers(5, 9) && !placements.covers(5, 14)
    );
    assert_eq!(placements.detectable_at(120, 100).collect::<Vec<_>>(), [1]);
    assert_eq!(placements.detectable_at(167, 159).collect::<Vec<_>>(), [1]);
    assert_eq!(placements.detectable_at(168, 159).count(), 0);
    assert_eq!(placements.detectable_at(119, 100).count(), 0);
}

/// The cells sprites changed are a set of cells of the page.
#[test]
fn sprite_cells_hold_the_cells_marked() {
    let mut cells = SpriteCells::NONE;
    assert!(cells.is_empty());
    cells.insert(1, 0);
    cells.insert(23, 39);
    cells.insert(25, 0);
    cells.insert(0, 40);
    assert!(cells.contains(1, 0) && cells.contains(23, 39));
    assert!(!cells.contains(1, 1) && !cells.contains(25, 0) && !cells.contains(0, 40) && !cells.contains(0, 64));
    assert_eq!(cells.count(), 2);
}

/// ANIM-4, ANIM-13 (REQ-6529): rectangles of cells and of dots.
#[test]
fn rectangles_join_cut_and_overlap() {
    let one = CellRect::cell(2, 3);
    let other = CellRect { row: 5, column: 1, rows: 2, columns: 1 };
    assert_eq!(one.union(other), CellRect { row: 2, column: 1, rows: 5, columns: 3 });
    assert_eq!(CellRect::join(None, Some(one)), Some(one));
    assert_eq!(CellRect::join(Some(one), None), Some(one));
    assert_eq!(CellRect::join(None, None), None);
    assert_eq!(CellRect::PAGE.area(), 1000);
    assert_eq!(CellRect::PAGE.dots(), DotRect { x: 0, y: 0, width: 480, height: 500 });
    assert_eq!(CellRect::PAGE.dots_of(10).height, 250);
    assert_eq!(CellRect::PAGE.dots().area(), 2 * crate::limits::MAX_ANIMATION_PIXELS_PER_TICK, "half a page a tick");
    assert_eq!(CellRect::cut(-5, -5, 60, 60), Some(CellRect::PAGE));
    assert_eq!(CellRect::cut(3, 3, 2, 3), None);
    let a = DotRect { x: 10, y: 10, width: 10, height: 10 };
    assert!(a.overlaps(DotRect { x: 19, y: 19, width: 5, height: 5 }));
    assert!(!a.overlaps(DotRect { x: 20, y: 10, width: 5, height: 5 }), "edges that touch share no dot");
    assert!(a.contains(10, 10) && a.contains(19, 19) && !a.contains(20, 19) && !a.contains(9, 10));
}
