//! The animation operations, their fold and frame cycling (REQ-6526,
//! REQ-6529; ANIM-6, ANIM-14).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;

const OBJECT: Target = Target::object(0x1234);

fn bytes(operation: &Operation, place: Place) -> std::vec::Vec<u8> {
    let mut out = [0_u8; MAX_OPERATION_BYTES];
    let length = write(operation, place, &mut out).unwrap();
    out[..length].to_vec()
}

/// ANIM-6 (REQ-6529): each operation's bytes, which read back as it.
#[test]
fn each_operation_is_its_bytes() {
    let cases: [(OperationKind, &[u8]); 9] = [
        (OperationKind::Show, &[5, 2, 0x12, 0x34]),
        (OperationKind::Hide, &[6, 2, 0x12, 0x34]),
        (OperationKind::Look(2), &[7, 2, 0x12, 0x34, 2]),
        (OperationKind::Move { relative: true, x: -8, y: 300 }, &[15, 2, 0x12, 0x34, 1, 0xFF, 0xF8, 0x01, 0x2C]),
        (OperationKind::Order { layer: 255, order: -1 }, &[16, 2, 0x12, 0x34, 255, 0xFF]),
        (OperationKind::Frame(15), &[17, 2, 0x12, 0x34, 15]),
        (
            OperationKind::Cycle { first: 1, last: 4, period: 3600, once: true },
            &[18, 2, 0x12, 0x34, 1, 4, 0x0E, 0x10, 1],
        ),
        (OperationKind::StopCycle, &[19, 2, 0x12, 0x34]),
        (OperationKind::Reset, &[20, 2, 0x12, 0x34]),
    ];
    for (kind, expected) in cases {
        let operation = Operation { target: OBJECT, kind };
        assert_eq!(bytes(&operation, Place::Record), expected, "{kind:?}");
        assert_eq!(read(expected, Place::Record), Ok((operation, expected.len())));
        // Cut short anywhere, it is short; bytes after it are the next
        // operation's.
        for length in 0..expected.len() {
            assert_eq!(read(&expected[..length], Place::Record), Err(Fault::Short));
        }
        assert!(is_code(expected[0]));
    }
    assert_eq!(MAX_OPERATION_BYTES, 9);
    for code in [0, 4, 8, 14, 21] {
        assert!(!is_code(code));
        assert_eq!(read(&[code, 2, 0, 1, 0, 0, 0, 0, 0], Place::Record), Err(Fault::Range("operation code")));
    }
}

/// ANIM-6 (REQ-6529): what each operation applies to, each field's
/// bounds, the reserved flag bits, and `reset` in a record only.
#[test]
fn operations_are_bounded() {
    let reset = Operation { target: Target::control(1), kind: OperationKind::Reset };
    assert_eq!(reset.check(Place::Record), Ok(()));
    assert_eq!(reset.check(Place::Rule), Err(Fault::Range("operation's field")));
    assert_eq!(read(&[20, 2, 0, 1], Place::Rule), Err(Fault::Range("operation's field")));
    // A control takes show, hide, look and reset only.
    assert_eq!(read(&[5, 1, 0, 32], Place::Rule).map(|read| read.0.target), Ok(Target::control(32)));
    for refused in
        [[5, 1, 0, 33, 0], [5, 1, 0, 0, 0], [5, 2, 0, 0, 0], [5, 0, 0, 0, 0], [5, 3, 0, 0, 0], [17, 1, 0, 1, 0]]
    {
        assert_eq!(read(&refused, Place::Record), Err(Fault::Range("operation's target")), "{refused:?}");
    }
    assert_eq!(read(&[5, 4, 0, 1], Place::Record), Err(Fault::Range("operation's target")));
    assert_eq!(read(&[15, 2, 0, 1, 2, 0, 0, 0, 0], Place::Record), Err(Fault::Reserved("move flags")));
    assert_eq!(read(&[18, 2, 0, 1, 0, 1, 0, 1, 0x80], Place::Record), Err(Fault::Reserved("cycle flags")));
    for bad in [
        OperationKind::Look(4),
        OperationKind::Order { layer: 4, order: 0 },
        OperationKind::Frame(16),
        OperationKind::Cycle { first: 2, last: 1, period: 1, once: false },
        OperationKind::Cycle { first: 0, last: 16, period: 1, once: false },
        OperationKind::Cycle { first: 0, last: 1, period: 0, once: false },
        OperationKind::Cycle { first: 0, last: 1, period: 3601, once: false },
    ] {
        let operation = Operation { target: OBJECT, kind: bad };
        assert_eq!(operation.check(Place::Record), Err(Fault::Range("operation's field")), "{bad:?}");
        let mut out = [0_u8; MAX_OPERATION_BYTES];
        assert!(write(&operation, Place::Record, &mut out).is_err());
    }
    assert_eq!(write(&reset, Place::Record, &mut [0_u8; 3]), Err(Fault::Full));
}

/// ANIM-6 (REQ-6529): each operation's effect on the state, and the fold
/// of a list of them in order.
#[test]
fn operations_fold_into_the_state() {
    let own = (10, 20);
    let mut state = AnimationState::default();
    assert!(state.is_default());
    state.apply(OperationKind::Hide, own, 0);
    assert_eq!(state.shown, Some(false));
    state.apply(OperationKind::Show, own, 0);
    assert_eq!(state.shown, Some(true));
    state.apply(OperationKind::Look(3), own, 0);
    assert_eq!(state.look, 3);
    // A relative move adds to the object's own position until one is in
    // force, then to that; it saturates at the 16-bit bounds.
    state.apply(OperationKind::Move { relative: true, x: 5, y: -5 }, own, 0);
    assert_eq!(state.position, Some((15, 15)));
    state.apply(OperationKind::Move { relative: true, x: 5, y: -5 }, own, 0);
    assert_eq!(state.position, Some((20, 10)));
    state.apply(OperationKind::Move { relative: true, x: i16::MAX, y: i16::MIN }, own, 0);
    state.apply(OperationKind::Move { relative: true, x: i16::MAX, y: i16::MIN }, own, 0);
    assert_eq!(state.position, Some((i16::MAX, i16::MIN)));
    state.apply(OperationKind::Move { relative: false, x: -3, y: 4 }, own, 0);
    assert_eq!(state.position, Some((-3, 4)));
    // Layer 255 keeps the layer in force.
    state.apply(OperationKind::Order { layer: 2, order: -7 }, own, 0);
    state.apply(OperationKind::Order { layer: 255, order: 9 }, own, 0);
    assert_eq!((state.layer, state.order), (Some(2), Some(9)));
    // A cycle starts at the tick after its operation; `frame` stops it.
    state.apply(OperationKind::Cycle { first: 2, last: 5, period: 10, once: false }, own, 99);
    assert_eq!(state.cycle, Some(Cycle { first: 2, last: 5, period: 10, once: false, start: 100 }));
    assert_eq!((state.frame_at(99), state.frame_at(100), state.frame_at(110)), (2, 2, 3));
    state.apply(OperationKind::Frame(7), own, 120);
    assert_eq!((state.cycle, state.frame), (None, 7));
    // `stop_cycle` stops at the frame shown.
    state.apply(OperationKind::Cycle { first: 0, last: 3, period: 1, once: false }, own, 0);
    state.apply(OperationKind::StopCycle, own, 3);
    assert_eq!((state.cycle, state.frame), (None, 2));
    state.apply(OperationKind::Reset, own, 0);
    assert!(state.is_default());
    let list = [
        OperationKind::Hide,
        OperationKind::Move { relative: true, x: 1, y: 1 },
        OperationKind::Show,
        OperationKind::Frame(4),
    ];
    let folded = fold(AnimationState::default(), list, own, 0);
    let expected = AnimationState { shown: Some(true), position: Some((11, 21)), frame: 4, ..Default::default() };
    assert_eq!(folded, expected);
}

/// ANIM-14 (REQ-6529): both frame formulas at their edges.
#[test]
fn the_frame_formulas_hold_at_their_edges() {
    // Looping: first + ((t − s) ÷ p) mod c.
    let looping = Cycle { first: 3, last: 6, period: 30, once: false, start: 100 };
    for (tick, frame) in [(0, 3), (100, 3), (129, 3), (130, 4), (189, 5), (190, 6), (219, 6), (220, 3)] {
        assert_eq!(looping.frame_at(tick), (frame, false), "tick {tick}");
    }
    // The last tick there is.
    let steps = (u32::MAX - 100) / 30;
    assert_eq!(looping.frame_at(u32::MAX).0, 3 + u8::try_from(steps % 4).unwrap());
    // Once: min(first + (t − s) ÷ p, last), ended when last is reached.
    let once = Cycle { once: true, ..looping };
    for (tick, frame, ended) in [(100, 3, false), (130, 4, false), (189, 5, false), (190, 6, true), (5000, 6, true)] {
        assert_eq!(once.frame_at(tick), (frame, ended), "tick {tick}");
    }
    // One frame, the fastest period and the slowest.
    let single = Cycle { first: 15, last: 15, period: 1, once: false, start: 0 };
    assert_eq!((single.frame_at(0), single.frame_at(77)), ((15, false), (15, false)));
    assert_eq!(Cycle { once: true, ..single }.frame_at(0), (15, true));
    let slow = Cycle { first: 0, last: 15, period: 3600, once: false, start: 1 };
    assert_eq!((slow.frame_at(3600).0, slow.frame_at(3601).0, slow.frame_at(1 + 16 * 3600).0), (0, 1, 0));
    // Advancing a state follows the cycle, and a cycle run once ends.
    let mut state = AnimationState::default();
    state.apply(OperationKind::Cycle { first: 0, last: 2, period: 2, once: true }, (0, 0), 9);
    let changed: std::vec::Vec<bool> = (10..=15).map(|tick| state.advance(tick)).collect();
    assert_eq!(changed, [false, false, true, false, true, false]);
    assert_eq!((state.frame, state.cycle), (2, None));
}

/// ANIM-6, ANIM-14 (REQ-6529): the store holds at most `N` states, at
/// most so many cycling, and forgets what the stream replaces.
#[test]
fn a_store_keeps_the_states_of_a_sub_page() {
    let mut store: AnimationStore<2> = AnimationStore::new();
    let on = |target, kind| Operation { target, kind };
    let cycle = OperationKind::Cycle { first: 0, last: 1, period: 5, once: false };
    assert_eq!(store.apply(&on(Target::object(1), OperationKind::Hide), (0, 0), 0, 1), Applied::Done);
    assert_eq!(store.apply(&on(Target::object(2), cycle), (0, 0), 0, 1), Applied::Done);
    assert_eq!(store.apply(&on(Target::object(3), OperationKind::Show), (0, 0), 0, 1), Applied::NoRoom);
    // A second object may not cycle; the one cycling may be given a new
    // cycle.
    assert_eq!(store.apply(&on(Target::object(1), cycle), (0, 0), 0, 1), Applied::TooManyCycles);
    assert_eq!(store.apply(&on(Target::object(2), cycle), (0, 0), 7, 1), Applied::Done);
    assert_eq!((store.cycling(), store.entries().len()), (1, 2));
    assert_eq!(store.get(Target::object(1)).unwrap().shown, Some(false));
    let mut changed = std::vec::Vec::new();
    store.advance(13, &mut |target| changed.push(target));
    assert_eq!(changed, [Target::object(2)]);
    // A stream that sets the object's own position forgets only that.
    store.get_mut(Target::object(1)).unwrap().position = None;
    // `reset` and `forget` remove a state, making room.
    assert_eq!(store.apply(&on(Target::object(1), OperationKind::Reset), (0, 0), 0, 1), Applied::Done);
    assert_eq!(store.get(Target::object(1)), None);
    assert_eq!(store.apply(&on(Target::object(3), OperationKind::Show), (0, 0), 0, 1), Applied::Done);
    store.forget(Target::object(2));
    assert_eq!(store.entries().iter().map(|(target, _)| target.number).collect::<std::vec::Vec<_>>(), [3]);
    store.clear();
    assert_eq!(store.entries(), []);
}
