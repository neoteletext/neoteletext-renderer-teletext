//! `PAGE_STATE` (RULE-47, RULE-83; REQ-6528, REQ-TST-040): the reader of
//! the answer after its stamp, and the reader of the whole event, never
//! panic; what they accept is within `MAX_STATE_BYTES`, within every
//! limit of a table, and writes back to the same bytes.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_input::MAX_MESSAGE;
use neoteletext_input::event::{Event, Item, read, write};
use neoteletext_rules::limits::{MAX_SEQUENCES, MAX_STATE_BYTES, MAX_TABLE_TARGETS, MAX_TABLE_VALUES};
use neoteletext_rules::state::{MAX_PAYLOAD_BYTES, decode, encode};

fuzz_target!(|data: &[u8]| {
    // The answer after its stamp.
    if let Ok(state) = decode(data) {
        assert!(data.len() <= MAX_PAYLOAD_BYTES);
        if let Some(table) = &state.table {
            assert!(table.generation != 0);
            assert!(table.sequences.len() <= MAX_SEQUENCES);
            assert!(table.targets.len() <= MAX_TABLE_TARGETS && table.values.len() <= MAX_TABLE_VALUES);
        }
        let mut out = [0_u8; MAX_PAYLOAD_BYTES];
        let length = encode(&state, &mut out).expect("what was decoded is encoded");
        assert_eq!(&out[..length], data);
    }
    // The whole event.
    let Ok(Some((Item::Event(event @ Event::PageState(answer)), used))) = read(data) else { return };
    assert!(answer.state().is_some(), "an answer that was read decodes");
    assert!(used <= MAX_STATE_BYTES + 4, "the event, its type and its length");
    let mut out = [0_u8; MAX_MESSAGE];
    let length = write(&Item::Event(event), &mut out).expect("what was read is written");
    assert_eq!(&out[..length], &data[..used]);
});
