//! Enhancement faults in the decoder's statistics (REQ-L25-051,
//! REQ-ARCH-007), and the transition trace of the Level 1.5 walk and the
//! Level 2.5 and 3.5 walk (REQ-FSM-010).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

mod common;

use common::{
    DISPLAY, Receiver, damage, display_page, feed, g0, invoke, local_data, public_number, row_0, set_position, x26,
};
use neoteletext_core::acq::EnhancementReport;
use neoteletext_core::acq::{Decoder, EnhancementFaults, PageBuild};
use neoteletext_core::compose::compose_level_traced;
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{HEADER_DISPLAY_BYTES, OBJECT_INVOCATIONS_PER_TYPE, SPACE};
use neoteletext_core::fsm::PageKey;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid, build_grid_traced};
use neoteletext_core::packet::{ObjectKind, ObjectSource, Packet, TripletFunction};
use neoteletext_core::trace::{Machine, TraceEntry, TraceSink};
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_core::viewer::{ComposeContext, Composed, Viewer, ViewerEvent};

/// The second feed starts after the first has finished and timed out.
const SECOND_FEED_US: u64 = 2_000_000_000;

/// The time the traced grids are built at.
const BUILT_AT: Instant = Instant(1234);

/// Page 100 with one fault of most kinds in its local enhancement data,
/// and after them as many absent public objects as the Annex A.8 limit
/// lets through, and one more.
fn faulty_page() -> Vec<Packet> {
    let absent = invoke(ObjectSource::Public, ObjectKind::Active, public_number(0, 0));
    let mut local = vec![
        set_position(1, 0),
        // Damaged below: one damaged triplet.
        g0(9, b'Z'),
        // A reserved designation (REQ-L25-012).
        TripletFunction::ModifiedDesignation { column: 2, designation: 0b111_0000 },
        // Undefined G3 and G2 positions (decision E.50).
        TripletFunction::G3Character { column: 3, code: 0x6E, level_15_form: false },
        TripletFunction::G2Character { column: 4, code: 0x59 },
        // A DRCS character is not acted on, and is no fault (decision E.95).
        TripletFunction::UnsupportedColumn { column: 5, mode: 0b01101, data: 0x41 },
        // Row 0 columns 0 to 7 are the receiver's (REQ-L25-050).
        row_0(0, 0),
        g0(3, b'Q'),
    ];
    local.extend(core::iter::repeat_n(absent, usize::from(OBJECT_INVOCATIONS_PER_TYPE) + 1));
    let mut packets = local_data(&local);
    packets[0] = damage(packets[0], 1);
    display_page(0, &[(1, b"LEVEL ONE")], packets)
}

/// The faults [`faulty_page`] has.
const FAULTY_PAGE: EnhancementFaults = EnhancementFaults {
    damaged_triplets: 1,
    forbidden_invocations: 0,
    absent_objects: OBJECT_INVOCATIONS_PER_TYPE as u32,
    limited_invocations: 1,
    discarded_writes: 1,
    ignored_designations: 1,
    undefined_characters: 2,
};

fn address() -> PageAddress {
    PageAddress::from_mpp(DISPLAY).unwrap()
}

/// Composes page 100 as the viewer does, and returns its report.
fn compose(decoder: &Decoder<'_>, config: &Config) -> Option<EnhancementReport> {
    let mut viewer = Viewer::new(address());
    let _ = viewer.event(ViewerEvent::Started);
    let _ = viewer.event(ViewerEvent::PagePresent);
    let magazine = decoder.magazine_data(address().magazine);
    let rolling = [SPACE; HEADER_DISPLAY_BYTES];
    let context = ComposeContext {
        magazine: &magazine,
        config,
        rolling_header: &rolling,
        store: decoder.store(),
        row24: None,
        lookup: decoder.enhancement_lookup(),
    };
    let page = decoder.store().get(address(), SubCode::ANY);
    let mut grid = Box::new(Grid::EMPTY);
    match viewer.compose(&mut grid, page, &context) {
        Composed::Built(report) => report,
        Composed::Held => panic!("the viewer is not holding"),
    }
}

/// Composes page 100 and records the report, as a port does on a redraw.
fn redraw(decoder: &mut Decoder<'_>, config: &Config) -> Option<EnhancementReport> {
    let report = compose(decoder, config);
    if let Some(report) = report {
        decoder.record_enhancement(report.build, &report.faults);
    }
    report
}

fn config(level: PresentationLevel) -> Config {
    Config { level, ..Config::default() }
}

/// REQ-L25-051, REQ-ARCH-007: every kind of fault the report counts is
/// added to its own count in the statistics.
#[test]
fn every_kind_of_fault_reaches_the_statistics() {
    let mut pages = vec![neoteletext_core::acq::StoredPage::EMPTY; 1];
    let mut decoder = Decoder::new(&mut pages, Config::default());
    // A different count for each kind, so that a field added to the wrong
    // count shows.
    let faults = EnhancementFaults {
        damaged_triplets: 1,
        forbidden_invocations: 2,
        absent_objects: 3,
        limited_invocations: 4,
        discarded_writes: 5,
        ignored_designations: 6,
        undefined_characters: 7,
    };
    let key = PageKey { address: address(), subcode: SubCode::new(0) };
    let build = PageBuild { key, receptions: 1, level: PresentationLevel::Level25 };
    assert!(decoder.record_enhancement(build, &faults));
    assert_eq!(decoder.statistics().enhancement, faults);
    let next = PageBuild { receptions: 2, ..build };
    assert!(decoder.record_enhancement(next, &faults));
    let mut twice = faults;
    twice.add(&faults);
    assert_eq!(decoder.statistics().enhancement, twice);
    assert_eq!(twice.undefined_characters, 14);
    decoder.reset_statistics();
    assert_eq!(decoder.statistics().enhancement, EnhancementFaults::NONE);
}

/// REQ-L25-051: the faults a real build meets reach the statistics through
/// the viewer, while the Level 1 page is still shown.
#[test]
fn faults_of_a_built_page_reach_the_statistics() {
    let config = config(PresentationLevel::Level25);
    Receiver::new().run_mut(&faulty_page(), config, |decoder| {
        assert_eq!(decoder.statistics().enhancement, EnhancementFaults::NONE, "nothing counted before a build");
        let report = redraw(decoder, &config).unwrap();
        assert_eq!(report.faults, FAULTY_PAGE);
        assert_eq!(decoder.statistics().enhancement, FAULTY_PAGE);
    });
}

/// The decoder counts a build once however often the viewer redraws it,
/// and again for a new reception or a new level.
#[test]
fn redraws_do_not_count_again() {
    let level_25 = config(PresentationLevel::Level25);
    let packets = faulty_page();
    Receiver::new().run_mut(&packets, level_25, |decoder| {
        for _ in 0..3 {
            let _ = redraw(decoder, &level_25);
        }
        assert_eq!(decoder.statistics().enhancement, FAULTY_PAGE, "three redraws, one build");
        let mut twice = FAULTY_PAGE;
        twice.add(&FAULTY_PAGE);
        // The page received again is a new build.
        feed(decoder, &packets, SECOND_FEED_US);
        let _ = redraw(decoder, &level_25);
        let _ = redraw(decoder, &level_25);
        assert_eq!(decoder.statistics().enhancement, twice, "a new reception");
        // So is the same reception at another level.
        let level_35 = config(PresentationLevel::Level35);
        let _ = redraw(decoder, &level_35);
        twice.add(&FAULTY_PAGE);
        assert_eq!(decoder.statistics().enhancement, twice, "another level");
        // At Levels 1 and 1.5 there is no report to record.
        assert!(compose(decoder, &config(PresentationLevel::Level15)).is_none());
    });
}

/// Builds page 100 from `packets` with the walk traced, and returns the
/// entries.
fn trace_of(packets: &[Packet], config: Config) -> Vec<TraceEntry> {
    Receiver::new().run(packets, config, |decoder| {
        let page = decoder.store().get(address(), SubCode::ANY).unwrap();
        let magazine = decoder.magazine_data(address().magazine);
        let options = DisplayOptions::default();
        let mut grid = Box::new(Grid::EMPTY);
        build_grid(&mut grid, Some(page), &magazine, &config, &options);
        let mut entries = Vec::new();
        let mut keep = |entry| entries.push(entry);
        let mut trace = TraceSink { time: BUILT_AT, entries: &mut keep };
        let lookup = decoder.enhancement_lookup();
        let _ = compose_level_traced(&mut grid, page, &magazine, &lookup, &config, &options, &mut trace);
        entries
    })
}

/// The entries as (from, event, action, to), checking that each is of
/// `machine` and stamped with the build time.
fn transitions(entries: &[TraceEntry], machine: Machine) -> Vec<[&'static str; 4]> {
    assert!(entries.iter().all(|entry| entry.machine == machine && entry.time == BUILT_AT), "{entries:#?}");
    entries.iter().map(|entry| [entry.from, entry.event, entry.action, entry.to]).collect()
}

fn traced(level: PresentationLevel) -> Config {
    Config { level, trace: true, ..Config::default() }
}

/// Page 100 whose local enhancement data has a damaged triplet, an absent
/// object and a write into the receiver's columns, for Level 3.5.
fn enhanced_page() -> Vec<Packet> {
    let absent = invoke(ObjectSource::Public, ObjectKind::Active, public_number(0, 0));
    let local = [set_position(1, 0), g0(3, b'A'), g0(4, b'B'), absent, row_0(0, 0), g0(3, b'Q')];
    let mut packets = local_data(&local);
    packets[0] = damage(packets[0], 2);
    display_page(0, &[(1, b"LEVEL ONE")], packets)
}

/// REQ-FSM-010, REQ-FSM-011, REQ-L25-051: a known page gives the expected
/// trace of the enhancement walk, its faults named as the actions of the
/// transitions that met them. Only the page-level run is traced, though
/// row 1 and row 0 each have a run of their own.
#[test]
fn enhancement_walk_trace() {
    let entries = trace_of(&enhanced_page(), traced(PresentationLevel::Level35));
    let place = "set the column; place the character or attribute";
    assert_eq!(
        transitions(&entries, Machine::EnhancementWalk),
        [
            ["Start", "walk begins, with a display triplet", "begin the local enhancement data", "Walking"],
            ["Walking", "Set Active Position", "set the active position", "Walking"],
            ["Walking", "G0 character", place, "Walking"],
            ["Walking", "damaged triplet", "skip the damaged triplet", "Walking"],
            ["Walking", "Object Invocation", "skip the invocation: the object is absent", "Walking"],
            ["Walking", "Address Display Row 0", "move to the row; row colour", "Walking"],
            ["Walking", "G0 character", "discard the write (REQ-OBJ-014, REQ-L25-050)", "Walking"],
            ["Walking", "Termination Marker", "end the source", "Returning"],
            ["Returning", "the local enhancement data ended", "none", "Finished"],
        ]
    );
}

/// REQ-FSM-010, REQ-FSM-011: a page without display triplets places the
/// default objects, of which there are none here.
#[test]
fn enhancement_walk_trace_without_local_data() {
    let entries = trace_of(&display_page(0, &[(1, b"LEVEL ONE")], vec![]), traced(PresentationLevel::Level25));
    assert_eq!(
        transitions(&entries, Machine::EnhancementWalk),
        [
            ["Start", "walk begins, with no display triplet", "none", "Placing default objects"],
            ["Placing default objects", "no more default objects", "none", "Finished"],
        ]
    );
}

/// REQ-FSM-010, REQ-FSM-007: a known page gives the expected trace of the
/// Level 1.5 walk, from its first row "walk begins" to the Termination
/// Marker or its last row "no more packets" (REQ-139), and the strict
/// option of REQ-L15-009 shows as the action of the write it ignores.
#[test]
fn level_15_walk_trace() {
    let write = |column, code| TripletFunction::G0Diacritic { column, code, diacritic: 0 };
    let local = [set_position(3, 0), write(6, b'A'), write(7, b'B'), write(2, b'C')];
    let mut packets = local_data(&local);
    packets[0] = damage(packets[0], 2);
    let packets = display_page(0, &[], packets);
    let diacritic = "G0 character with diacritical mark";
    let expected = |last: &'static str| {
        [
            ["Start", "walk begins", "none", "Walking"],
            ["Walking", "next packet", "none", "Walking"],
            ["Walking", "Set Active Position", "none", "Walking"],
            ["Walking", diacritic, "write character", "Walking"],
            ["Walking", "damaged triplet", "none", "Walking"],
            ["Walking", diacritic, last, "Walking"],
            ["Walking", "Termination Marker", "none", "Terminated"],
        ]
    };
    let entries = trace_of(&packets, traced(PresentationLevel::Level15));
    assert_eq!(transitions(&entries, Machine::TripletWalk), expected("write character"));
    let strict = Config { ignore_out_of_order_triplets: true, ..traced(PresentationLevel::Level15) };
    let entries = trace_of(&packets, strict);
    assert_eq!(transitions(&entries, Machine::TripletWalk), expected("ignore out-of-order write"));
    // REQ-139's last row: data with no Termination Marker ends when the
    // packets do.
    let unterminated = display_page(0, &[], vec![x26(0, &[set_position(3, 0)])]);
    let entries = trace_of(&unterminated, traced(PresentationLevel::Level15));
    let rows = transitions(&entries, Machine::TripletWalk);
    assert_eq!(rows.first(), Some(&["Start", "walk begins", "none", "Walking"]));
    assert_eq!(rows.last(), Some(&["Walking", "no more packets", "none", "Terminated"]));
}

/// REQ-FSM-010: with tracing off in the configuration, nothing is
/// reported, at any level.
#[test]
fn tracing_off_reports_nothing() {
    for level in
        [PresentationLevel::Level1, PresentationLevel::Level15, PresentationLevel::Level25, PresentationLevel::Level35]
    {
        let config = Config { level, trace: false, ..Config::default() };
        assert_eq!(trace_of(&enhanced_page(), config), [], "{level:?}");
    }
    // Level 1 has no walk to trace even with tracing on.
    assert_eq!(trace_of(&enhanced_page(), traced(PresentationLevel::Level1)), []);
}

/// The row attribute machine's trace of `packets`' page 100 under `config`.
fn row_trace_of(packets: &[Packet], config: Config) -> Vec<TraceEntry> {
    Receiver::new().run(packets, config, |decoder| {
        let page = decoder.store().get(address(), SubCode::ANY).unwrap();
        let magazine = decoder.magazine_data(address().magazine);
        let mut grid = Box::new(Grid::EMPTY);
        let mut entries = Vec::new();
        let mut keep = |entry| entries.push(entry);
        let mut trace = TraceSink { time: BUILT_AT, entries: &mut keep };
        build_grid_traced(&mut grid, Some(page), &magazine, &config, &DisplayOptions::default(), &mut trace);
        entries
    })
}

/// REQ-142 (REQ-FSM-010), REQ-138 (REQ-FSM-006): with the cells' trace
/// asked for as well as the trace, the row attribute machine reports each
/// cell of each row, naming the Table 26 attribute or a character, what the
/// cell shows, and the mode and hold before and after; asked for alone, or
/// not at all, it reports nothing.
#[test]
fn row_attribute_trace() {
    // Mosaic Red, a solid mosaic, Hold Mosaics, Alpha White, "A".
    let codes: &[u8] = &[0x11, 0x7F, 0x1E, 0x07, b'A'];
    let packets = display_page(0, &[(1, codes)], vec![]);
    let cells = Config { trace_cells: true, ..traced(PresentationLevel::Level1) };
    let entries = row_trace_of(&packets, cells);
    let row_1: Vec<_> = entries
        .iter()
        .filter(|entry| entry.machine == Machine::RowAttributes(1))
        .map(|entry| [entry.from, entry.event, entry.action, entry.to])
        .collect();
    assert_eq!(
        row_1[..5],
        [
            ["alphanumeric", "Mosaic Red", "show space", "mosaic"],
            ["mosaic", "character", "show mosaic", "mosaic"],
            ["mosaic", "Hold Mosaics", "show held mosaic", "mosaic, hold"],
            ["mosaic, hold", "Alpha White", "show held mosaic", "alphanumeric, hold"],
            ["alphanumeric, hold", "character", "show character", "alphanumeric, hold"],
        ]
    );
    assert_eq!(row_1.len(), 40, "one entry for each cell");
    assert!(entries.iter().all(|entry| entry.time == BUILT_AT));
    for config in [traced(PresentationLevel::Level1), Config { trace_cells: true, ..Config::default() }] {
        assert_eq!(row_trace_of(&packets, config), [], "{config:?}");
    }
}
