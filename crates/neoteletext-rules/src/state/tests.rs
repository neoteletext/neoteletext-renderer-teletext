//! The `PAGE_STATE` payload (REQ-6526, RULE-47).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, and indexing and arithmetic on data it built to a known size"
)]

use super::*;
use crate::table::{SOUND_LEVEL, TargetKind};

fn report(number: u16) -> TargetReport {
    TargetReport {
        target: Target::object(number),
        exists: true,
        shown: true,
        cycling: false,
        look: 0,
        frame: 0,
        x: 700,
        y: 380,
        layer: 2,
        order: 0,
    }
}

/// RULE-47 (REQ-6526): the answer's bytes, field by field, which read
/// back as it.
#[test]
fn the_answer_is_its_bytes() {
    let mut table = TableState::new(3, 9120, 0b111);
    table.variables.push(5).unwrap();
    table.timers.push(TimerState { running: true, elapsed: 18 }).unwrap();
    table.targets.push(report(301)).unwrap();
    table.values.push((Value::sound(0, SOUND_LEVEL), 41)).unwrap();
    let state = PageState { identifier: 42, table: Some(table) };
    let mut out = [0_u8; MAX_BODY_BYTES];
    let length = encode(&state, &mut out).unwrap();
    #[rustfmt::skip]
    let expected: &[u8] = &[
        0, 0, 0, 42, 0, 3, 0, 0, 0x23, 0xA0, 0, 0, 0, 0, 0, 0, 0, 7,
        // One variable, 5.
        1, 0, 0, 0, 5,
        // One timer: running, 18 ticks.
        1, 1, 0, 0, 0, 18,
        // One target: object 301, exists and shown, look, frame, x 700,
        // y 380, layer 2, order 0.
        1, 2, 0x01, 0x2D, 3, 0, 0, 0x02, 0xBC, 0x01, 0x7C, 2, 0,
        // One value: the level of audio channel 0, reading 41.
        1, 5, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 41,
    ];
    assert_eq!(&out[..length], expected);
    assert_eq!(decode(expected), Ok(state));
    for cut in 0..expected.len() {
        assert_eq!(decode(&expected[..cut]), Err(Fault::Short), "{cut} bytes");
    }
    let mut longer = expected.to_vec();
    longer.push(0);
    assert_eq!(decode(&longer), Err(Fault::Trailing));
}

/// RULE-47 (REQ-6526): a page with no table answers generation 0 and a
/// tick of 0, and nothing more.
#[test]
fn a_page_with_no_table_answers_so() {
    let state = PageState { identifier: 0xFFFF_FFFF, table: None };
    let mut out = [0_u8; 16];
    let length = encode(&state, &mut out).unwrap();
    assert_eq!(out[..length], [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0]);
    assert_eq!(decode(&out[..length]), Ok(state));
    assert_eq!(decode(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 1]), Err(Fault::Range("tick without a table")));
    assert_eq!(decode(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]), Err(Fault::Trailing));
}

/// RULE-47, RULE-57 (REQ-6526): the fullest answer is within
/// `MAX_STATE_BYTES` with its stamp, and a list over its limit or a field
/// out of range is refused.
#[test]
fn the_answer_is_bounded() {
    let mut table = TableState::new(65_535, u32::MAX, u64::MAX);
    for number in 0..MAX_VARIABLES {
        table.variables.push(i32::try_from(number).unwrap() - 16).unwrap();
    }
    for _ in 0..MAX_TIMERS {
        table.timers.push(TimerState { running: true, elapsed: u32::MAX }).unwrap();
    }
    for number in 0..MAX_TABLE_TARGETS {
        table.targets.push(report(u16::try_from(number + 1).unwrap())).unwrap();
    }
    for number in 0..MAX_TABLE_VALUES {
        table.values.push((Value::synth(0, 14, 0, 0, 2, u16::try_from(number).unwrap()), -1)).unwrap();
    }
    let state = PageState { identifier: 1, table: Some(table) };
    let mut out = [0_u8; MAX_BODY_BYTES];
    let length = encode(&state, &mut out).unwrap();
    assert_eq!(length, 1726);
    assert!(length + 4 <= MAX_STATE_BYTES);
    assert_eq!(decode(&out[..length]), Ok(state));
    assert_eq!(decode(&[0_u8; MAX_BODY_BYTES + 1]), Err(Fault::Limit("MAX_STATE_BYTES")));
    // The variable count is byte 18.
    let mut over = out;
    over[18] = 33;
    assert_eq!(decode(&over[..length]), Err(Fault::Limit("MAX_VARIABLES")));
    let refused = |change: &dyn Fn(&mut TableState)| {
        let mut table = TableState::new(1, 0, 0);
        change(&mut table);
        encode(&PageState { identifier: 1, table: Some(table) }, &mut [0_u8; 256]).unwrap_err()
    };
    let target = |change: &dyn Fn(&mut TargetReport)| {
        refused(&|table| {
            let mut bad = report(1);
            change(&mut bad);
            table.targets.push(bad).unwrap();
        })
    };
    assert_eq!(target(&|bad| bad.look = 4), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.frame = 16), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.layer = 4), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.target = Target::PAGE), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.target = Target::rect(1)), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.target = Target::control(33)), Fault::Range("target"));
    assert_eq!(target(&|bad| bad.target = Target { kind: TargetKind::Object, number: 0 }), Fault::Range("target"));
    assert_eq!(
        refused(&|table| _ = table.timers.push(TimerState { running: false, elapsed: 1 })),
        Fault::Range("elapsed while stopped")
    );
    assert!(matches!(refused(&|table| _ = table.values.push((Value::NONE, 0))), Fault::Range(_)));
    assert_eq!(encode(&state, &mut [0_u8; 100]), Err(Fault::Full));
}
