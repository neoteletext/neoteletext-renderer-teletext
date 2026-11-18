//! The RULES record's body (RULE-70, RULE-83; REQ-6526, REQ-TST-040): any
//! bytes decode to a table within every limit or are refused whole, and
//! what decodes encodes to the same bytes and decodes to the same table.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_rules::limits::{
    MAX_ACTIONS, MAX_CONDITION_NODES, MAX_KEYS_PER_TRIGGER, MAX_RECTS, MAX_RULES, MAX_SUBPAGE_MASKS, MAX_TABLE_BYTES,
    MAX_TABLE_TARGETS, MAX_TABLE_VALUES, MAX_TIMERS, MAX_VARIABLES, MAX_WATCHED,
};
use neoteletext_rules::table::{Index, Table, Trigger, check, check_condition, decode, encode};

fuzz_target!(|data: &[u8]| {
    let mut table = Box::new(Table::new());
    let Ok(header) = decode(data, &mut table) else {
        assert_eq!(*table, Table::new(), "nothing of a refused table is kept");
        assert!(check(data).is_err());
        return;
    };
    assert_eq!(check(data), Ok(header));
    assert!(data.len() <= MAX_TABLE_BYTES);
    assert!(table.variables.len() <= MAX_VARIABLES && table.timers.len() <= MAX_TIMERS);
    assert!(table.rects.len() <= MAX_RECTS && table.rules.len() <= MAX_RULES);
    assert!(table.subpages.len() <= MAX_SUBPAGE_MASKS);
    for rule in table.rules.iter() {
        assert!(rule.actions.len() <= MAX_ACTIONS && rule.condition.len() <= MAX_CONDITION_NODES);
        assert!(!rule.actions.is_empty() || rule.echo);
        check_condition(&rule.condition).expect("a decoded condition is one tree within its limits");
        if let Trigger::Key { keys, .. } | Trigger::KeyUp { keys, .. } = &rule.trigger {
            assert!(keys.len() <= MAX_KEYS_PER_TRIGGER);
        }
    }
    let index = Index::of(&table).expect("a decoded table is within the limits on what it reads");
    assert!(index.values.len() <= MAX_TABLE_VALUES && index.targets.len() <= MAX_TABLE_TARGETS);
    assert!(index.watch.len() <= MAX_WATCHED);
    // The encoding is the one encoding of its table: the bytes back.
    let mut out = [0_u8; MAX_TABLE_BYTES];
    let length = encode(header.page, header.generation, header.has_table.then_some(&*table), &mut out)
        .expect("what was decoded is encoded");
    assert_eq!(&out[..length], data);
    let mut again = Box::new(Table::new());
    assert_eq!(decode(&out[..length], &mut again), Ok(header));
    assert_eq!(again, table);
});
