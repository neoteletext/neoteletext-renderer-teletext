//! The command records, types 28 to 33 (RULE-21, RULE-22, RULE-99,
//! RULE-100, RULE-102, RULE-103, RULE-83; REQ-6612, REQ-TST-040): the
//! readers and the writers agree and refuse what is out of range; a
//! command with any bindings is checked and bound without a panic, and
//! what it is bound to is a body its record's reader still accepts.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::command::{
    self, Binding, Context, MAX_BODY_BYTES, Source, TYPE_LOCAL_MIDI, TYPE_SEQUENCE, apply_bindings,
};
use neoteletext_rules::limits::{MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES};
use neoteletext_web::records::{body_check, commands};

fuzz_target!(|data: &[u8]| {
    let Some((&kind, body)) = data.split_first() else { return };
    // One record of the six: what reads writes back to the same bytes,
    // and the record's reader agrees with the body's.
    if let Ok((page, sub, read)) = command::decode(kind, body) {
        assert!((TYPE_LOCAL_MIDI..=TYPE_SEQUENCE).contains(&kind) && read.kind() == kind);
        assert!(body.len() <= MAX_BODY_BYTES);
        let mut out = [0_u8; MAX_BODY_BYTES];
        let length = command::encode(&read, page, sub, &mut out).expect("what was decoded is encoded");
        assert_eq!(&out[..length], body);
        let record = commands::read(kind, body).expect("the record's reader reads what the body's does");
        assert_eq!((record.page.mpp(), record.subcode.value(), record.command), (page, sub, read));
    } else {
        assert!(commands::read(kind, body).is_err());
    }
    // A command of a group, with bindings: the first byte of the rest
    // says how many, of three bytes each.
    let Some((&count, rest)) = body.split_first() else { return };
    let count = usize::from(count) % (MAX_COMMAND_BINDINGS + 2);
    let Some((bound, body)) = rest.split_at_checked(count * 3) else { return };
    let bindings: Vec<Binding> =
        bound.as_chunks::<3>().0.iter().map(|&[item, field, source]| Binding { item, field, source }).collect();
    if body.is_empty() || body.len() > MAX_COMMAND_BYTES {
        return;
    }
    // The table the command is in is the page its own body names: a
    // CONTROL names a page number alone, in its last field.
    let word =
        |at: usize| u16::from_be_bytes([body.get(at).copied().unwrap_or(0), body.get(at + 1).copied().unwrap_or(0)]);
    let (page, sub) =
        if kind == command::TYPE_CONTROL { (word(body.len().saturating_sub(2)), 1) } else { (word(0), word(2)) };
    let context = Context { page, sub, groups: 64, sequences: 8 };
    if command::check(kind, body, &bindings, &context, body_check).is_err() {
        return;
    }
    // Whatever the sources give, the body bound is one its reader takes,
    // or nothing of it is left.
    for value in [0, 1, -1, 127, 128, i32::MAX, i32::MIN, i32::from(kind) * 257] {
        let mut out = [0_u8; MAX_COMMAND_BYTES];
        let resolve = |source: Source| match source {
            Source::Variable(_) => value,
            Source::Parameter => value.wrapping_add(1),
            Source::Fired(_) => value.wrapping_neg(),
        };
        let applied = apply_bindings(kind, body, &bindings, &resolve, &mut out);
        assert!(applied.length <= body.len());
        if applied.length == 0 {
            assert!(applied.skipped > 0);
            continue;
        }
        let checked = command::check(kind, &out[..applied.length], &[], &context, body_check);
        assert_eq!(checked, Ok(()), "bound to {value}, the body is refused");
    }
});
