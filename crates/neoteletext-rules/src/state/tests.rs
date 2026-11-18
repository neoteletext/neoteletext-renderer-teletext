//! The `PAGE_STATE` payload (REQ-6528; RULE-47).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::limits::{MAX_STATE_BYTES, STAMP_BYTES};

fn full() -> PageState {
    let mut table = TableState::new(3, 9120);
    table.variables = [4, -1, i32::MAX, i32::MIN, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7];
    table.timers[0] = TimerState { running: true, elapsed: 18 };
    table.sequences.push(SequenceState { running: true, passes: 3, position: 12 }).unwrap();
    table.sequences.push(SequenceState::default()).unwrap();
    table.armed = 1;
    table
        .targets
        .push(TargetReport {
            target: Target::object(301),
            exists: true,
            shown: true,
            cycling: false,
            look: 0,
            frame: 2,
            x: 700,
            y: -380,
            layer: 2,
            order: -1,
        })
        .unwrap();
    table.values.push((Value::sound(0, crate::table::SOUND_LEVEL), 41)).unwrap();
    PageState { identifier: 42, table: Some(table) }
}

/// RULE-47 (REQ-6528): the payload's bytes, field by field, which read
/// back as the state.
#[test]
fn a_state_is_its_bytes() {
    let state = full();
    let mut out = [0_u8; MAX_PAYLOAD_BYTES];
    let length = encode(&state, &mut out).unwrap();
    let bytes = &out[..length];
    assert_eq!(decode(bytes), Ok(state));
    // The identifier, the generation and the tick.
    assert_eq!(bytes[..10], [0, 0, 0, 42, 0, 3, 0, 0, 0x23, 0xA0]);
    // Sixteen variables of four bytes, then eight timers of five.
    assert_eq!(bytes[10..14], [0, 0, 0, 4]);
    assert_eq!(bytes[14..18], [0xFF; 4]);
    let timers = 10 + 16 * 4;
    assert_eq!(bytes[timers..timers + 5], [1, 0, 0, 0, 18]);
    // The sequences with their count, then the armed runs.
    let sequences = timers + 8 * TIMER_BYTES;
    assert_eq!(bytes[sequences..sequences + 10], [2, 1, 0, 0, 0, 3, 0, 0, 0, 12]);
    let armed = sequences + 1 + 2 * SEQUENCE_BYTES;
    assert_eq!(bytes[armed], 1);
    // One target of twelve bytes, one value of twelve.
    assert_eq!(bytes[armed + 1..armed + 14], [1, 2, 0x01, 0x2D, 3, 0, 2, 0x02, 0xBC, 0xFE, 0x84, 2, 0xFF]);
    assert_eq!(bytes[armed + 14..], [1, 5, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 41]);
    assert_eq!(TARGET_BYTES, 12);
    // Cut short anywhere, it is refused; a byte more is trailing.
    for cut in 0..length {
        assert!(decode(&bytes[..cut]).is_err(), "{cut}");
    }
    let mut longer = bytes.to_vec();
    longer.push(0);
    assert_eq!(decode(&longer), Err(Fault::Trailing));
}

/// RULE-47 (REQ-6528): a page with no table answers generation 0 and a
/// tick of 0 and nothing more.
#[test]
fn no_table_is_ten_bytes() {
    let state = PageState { identifier: 5, table: None };
    let mut out = [0_u8; 16];
    let length = encode(&state, &mut out).unwrap();
    assert_eq!(out[..length], [0, 0, 0, 5, 0, 0, 0, 0, 0, 0]);
    assert_eq!(decode(&out[..length]), Ok(state));
    assert_eq!(decode(&[0, 0, 0, 5, 0, 0, 0, 0, 0, 1]), Err(Fault::Range("tick without a table")));
    assert_eq!(decode(&[0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0]), Err(Fault::Trailing));
}

/// RULE-47, RULE-57 (REQ-6528, REQ-6608): the fullest answer is within
/// `MAX_STATE_BYTES` with its stamp, and each count over its limit, each
/// reserved bit and each field out of range is refused.
#[test]
fn a_state_is_bounded() {
    let mut table = TableState::new(1, 0);
    for _ in 0..MAX_SEQUENCES {
        table.sequences.push(SequenceState { running: true, passes: u32::MAX, position: 1 }).unwrap();
    }
    table.armed = u8::try_from(MAX_ARMED_RUNS).unwrap();
    for number in 1..=u16::try_from(MAX_TABLE_TARGETS).unwrap() {
        let report = TargetReport { target: Target::object(number), ..full().table.unwrap().targets[0] };
        table.targets.push(report).unwrap();
    }
    for number in 0..u16::try_from(MAX_TABLE_VALUES).unwrap() {
        table.values.push((Value::synth(0, 13, 0, 0, crate::table::CLASS_NOTE, number), 1)).unwrap();
    }
    let state = PageState { identifier: u32::MAX, table: Some(table) };
    let mut out = [0_u8; MAX_PAYLOAD_BYTES + 1];
    let length = encode(&state, &mut out).unwrap();
    assert_eq!(length, MAX_PAYLOAD_BYTES);
    assert!(STAMP_BYTES + length <= MAX_STATE_BYTES);
    assert_eq!(decode(&out), Err(Fault::Limit("MAX_STATE_BYTES")));

    let good = {
        let mut out = [0_u8; MAX_PAYLOAD_BYTES];
        let length = encode(&full(), &mut out).unwrap();
        out[..length].to_vec()
    };
    let timers = 10 + 16 * 4;
    let sequences = timers + 8 * TIMER_BYTES;
    let armed = sequences + 1 + 2 * SEQUENCE_BYTES;
    let refused = |at: usize, byte: u8| {
        let mut bytes = good.clone();
        bytes[at] = byte;
        decode(&bytes)
    };
    assert_eq!(refused(timers, 2), Err(Fault::Reserved("timer flags")));
    assert_eq!(refused(timers + 5 + 4, 1), Err(Fault::Range("elapsed while stopped")));
    assert_eq!(refused(sequences, 9), Err(Fault::Limit("MAX_SEQUENCES")));
    assert_eq!(refused(sequences + 1, 3), Err(Fault::Reserved("sequence flags")));
    assert_eq!(refused(armed, 17), Err(Fault::Limit("MAX_ARMED_RUNS")));
    assert_eq!(refused(armed + 1, 65), Err(Fault::Limit("MAX_TABLE_TARGETS")));
    assert_eq!(refused(armed + 2, 3), Err(Fault::Range("target")));
    assert_eq!(refused(armed + 5, 8), Err(Fault::Reserved("target flags")));
    assert_eq!(refused(armed + 6, 4), Err(Fault::Range("target")));
    assert_eq!(refused(armed + 14, 65), Err(Fault::Limit("MAX_TABLE_VALUES")));
    assert_eq!(refused(armed + 15, 9), Err(Fault::Range("value's space")));
}
