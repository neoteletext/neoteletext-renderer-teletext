//! Each rule of decisions E.43 to E.48 and E.50 in a test of its own, at
//! Level 2.5 and at Level 3.5 (REQ-468, REQ-TST-015). The tests are named
//! after the decision and the rule, and each builds its page by function
//! with the tests' common builder (`common/mod.rs`, which encodes through
//! `packet::encode`), as DEC-2495 settles. Rules that only the renderer or
//! the font generator can show are tested beside them, and REQ-468 lists
//! each rule with its test.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

mod common;

use common::{
    background, build, build_both, char_of, colours, damage, display_page, end, foreground, g0, local_data, mag, row_0,
    set_of, set_position, x26, x28,
};
use neoteletext_core::charset::{CharSet, G0, G0Set};
use neoteletext_core::colour::{ColourMap, colour_index};
use neoteletext_core::config::PresentationLevel;
use neoteletext_core::consts::{
    ALPHA_RED, BLACK_BACKGROUND, CONCEAL, DEFAULT_SCREEN_COLOUR_SHIFT, FLASH, HOLD_MOSAICS, MOSAIC_RED, NEW_BACKGROUND,
    PACKET_PAGE_ENHANCEMENT, SEPARATED_MOSAICS, START_BOX, STEADY, TRIPLETS_PER_PACKET,
};
use neoteletext_core::level1::{FlashMode, FlashRate, Glyph, Grid};
use neoteletext_core::packet::{
    ColourData, DisplayAttributes, ObjectKind, ObjectSource, Packet, Triplet, TripletFunction, encode,
};
use neoteletext_core::types::{Designations, PresentationFields, Rgb12};

const L1: PresentationLevel = PresentationLevel::Level1;
const L25: PresentationLevel = PresentationLevel::Level25;
const L35: PresentationLevel = PresentationLevel::Level35;
/// Header control bit C14 alone: national option 1 of the header's group,
/// German in the Latin group 0000 (Table 32).
const C14_GERMAN: u16 = 1 << 14;
/// Table 32's designations in group 0000, Latin: German and Swedish
/// (national options 1 and 2).
const LATIN_GERMAN: u8 = 0b000_0001;
const LATIN_SWEDISH: u8 = 0b000_0010;
/// No second G0 set (Table 33: the value that names none).
const NO_SECOND_SET: u8 = 0x7F;
/// Position 4/0, the first national position of Table 36: "@" in English,
/// "§" in German, "É" in Swedish.
const NATIONAL_4_0: u8 = 0x40;
/// Address Display Row 0 and Full Row Colour row selections (Table 28,
/// data bits D6 D5): this row only, and down to the bottom.
const THIS_ROW: u8 = 0b00;
const DOWN_TO_BOTTOM: u8 = 0b11;
/// A reserved row selection.
const RESERVED_SELECTION: u8 = 0b01;
/// Font Style data with proportional spacing, bold and italic (D0 to D2)
/// for two more rows (D6 to D4), Table 29.
const EVERY_STYLE_TWO_ROWS: u8 = 0b010_0111;
/// Table 29 column mode 01110, Font Style.
const FONT_STYLE_MODE: u8 = 0b01110;

/// Page 100 with Level 1 rows and local enhancement data.
fn page(rows: &[(u8, &[u8])], local: &[TripletFunction]) -> Vec<Packet> {
    display_page(0, rows, local_data(local))
}

fn display(column: u8, attributes: DisplayAttributes) -> TripletFunction {
    TripletFunction::DisplayAttributes { column, attributes }
}

/// Packet M/29/0 or M/29/4 of magazine 1.
fn m29(designation: u8, fields: &PresentationFields) -> Packet {
    encode::magazine_enhancement(mag(1), designation, fields)
}

/// Sixteen colour words whose red is `red` and whose green counts the entry.
fn words(red: u8) -> [Rgb12; 16] {
    core::array::from_fn(|entry| Rgb12::new(red, entry as u8, 0))
}

/// X/28 or M/29 fields with the colour words only.
fn colour_words(red: u8) -> PresentationFields {
    PresentationFields { colour_words: Some(words(red)), ..colours(0, 0, false, 0) }
}

/// X/28 or M/29 fields with the default G0 and G2 designation only.
fn designation(default: u8) -> PresentationFields {
    PresentationFields { designations: Some(Designations { default, second: NO_SECOND_SET }), ..Default::default() }
}

fn latin() -> CharSet {
    CharSet::G0(G0 { set: G0Set::Latin, national: None })
}

/// A local Passive object that writes "X" at its origin, defined in packet
/// X/26/1, and the triplet that invokes it: where its "X" lands shows the
/// active position at the invocation.
fn marker_object() -> ([TripletFunction; 3], TripletFunction) {
    let number = common::local_number(1, 0);
    let object = [common::define(ObjectKind::Passive, 0b11, number), g0(0, b'X'), end()];
    (object, common::invoke(ObjectSource::Local, ObjectKind::Passive, number))
}

// ---------------------------------------------------------------------------
// Decision E.43 (DEC-567, REQ-274): the Colour Map and colour indices.
// ---------------------------------------------------------------------------

// REQ-274 item 1, decision E.43 rule 1: every colour is a Colour Map index
// 8 × CLUT + entry, and the Level 1 colours are CLUT 0 entries 0 to 7.
#[test]
fn e43_every_colour_is_a_colour_map_index() {
    let grid = build_both(&page(&[(1, &[ALPHA_RED, b'A'])], &[set_position(1, 0), foreground(3, colour_index(2, 5))]));
    assert_eq!(grid.cells[1][1].foreground, colour_index(0, 1), "Level 1 red is CLUT 0 entry 1");
    assert_eq!(grid.cells[1][3].foreground, 21, "CLUT 2 entry 5 is index 21");
}

// REQ-274 item 2, decision E.43 rule 2: the defaults are Table 30's,
// with CLUT 1 entry 5 half magenta and entry 6 half cyan.
#[test]
fn e43_the_defaults_are_those_of_table_30() {
    const HALF: u8 = 7;
    let grid = build_both(&page(&[(1, b"A")], &[]));
    assert_eq!(grid.colour_map, ColourMap::DEFAULT);
    assert_eq!(grid.colour_map.entry(colour_index(1, 5)), Rgb12::new(HALF, 0, HALF), "half magenta");
    assert_eq!(grid.colour_map.entry(colour_index(1, 6)), Rgb12::new(0, HALF, HALF), "half cyan");
}

// REQ-274 item 3, decision E.43 rule 3: X/28/0 format 1 redefines CLUTs 2
// and 3 at both levels.
#[test]
fn e43_x28_0_redefines_cluts_2_and_3_at_both_levels() {
    let packets = display_page(0, &[(1, b"A")], vec![x28(0, &colour_words(9))]);
    for level in [L25, L35] {
        let grid = build(&packets, level).0;
        assert_eq!(grid.colour_map.entry(colour_index(2, 0)), Rgb12::new(9, 0, 0), "{level:?}");
        assert_eq!(grid.colour_map.entry(colour_index(3, 7)), Rgb12::new(9, 15, 0), "{level:?}");
        assert_eq!(grid.colour_map.entry(colour_index(0, 1)), ColourMap::DEFAULT.entry(1), "{level:?}: CLUT 0 kept");
    }
}

// REQ-274 item 4, decision E.43 rule 3: X/28/4 redefines CLUTs 0 and 1 at
// Level 3.5 only.
#[test]
fn e43_x28_4_redefines_cluts_0_and_1_at_level_3_5_only() {
    let packets = display_page(0, &[(1, b"A")], vec![x28(4, &colour_words(4))]);
    assert_eq!(build(&packets, L35).0.colour_map.entry(colour_index(0, 1)), Rgb12::new(4, 1, 0));
    assert_eq!(build(&packets, L25).0.colour_map, ColourMap::DEFAULT, "ignored at Level 2.5");
}

// REQ-274 item 4, decision E.43 rule 3: CLUT 1 entry 0 stays transparent;
// its twelve bits in X/28/4 are ignored.
#[test]
fn e43_clut_1_entry_0_stays_transparent() {
    let packets = display_page(0, &[(1, b"A")], vec![x28(4, &colour_words(4))]);
    let grid = build(&packets, L35).0;
    assert_eq!(grid.colour_map.entry(colour_index(1, 1)), Rgb12::new(4, 9, 0), "the rest of CLUT 1 is redefined");
    assert_eq!(grid.colour_map.entry(colour_index(1, 0)), ColourMap::DEFAULT.entry(colour_index(1, 0)));
    assert!(ColourMap::is_transparent(colour_index(1, 0)));
}

// REQ-274 item 5, decision E.43 rule 4: in a multi-bit colour field the
// lowest-numbered bit is the least significant: the default screen colour
// of X/28/0 triplet 13 (Table 4) with only its first bit, bit 5, set is
// Colour Map entry 1. The packet is written triplet by triplet, so the test
// does not rest on the core's own encoder of the field.
#[test]
fn e43_the_lowest_numbered_bit_is_the_least_significant() {
    const TRIPLET_13: usize = TRIPLETS_PER_PACKET - 1;
    let mut triplets = [Triplet::from_value(0); TRIPLETS_PER_PACKET];
    triplets[TRIPLET_13] = Triplet::from_value(1 << DEFAULT_SCREEN_COLOUR_SHIFT);
    let x28_0 = encode::triplet_packet(mag(1), PACKET_PAGE_ENHANCEMENT, 0, &triplets);
    assert_eq!(build_both(&display_page(0, &[(1, b"A")], vec![x28_0])).screen_colour, 1);
}

// REQ-274 item 6, decision E.43 rule 5: "black" as a default screen or row
// colour is Colour Map entry 0, black unless Level 3.5 redefines it.
#[test]
fn e43_default_black_is_colour_map_entry_0() {
    let packets = display_page(0, &[(1, b"A")], vec![x28(4, &colour_words(5))]);
    for level in [L25, L35] {
        let grid = build(&packets, level).0;
        assert_eq!((grid.screen_colour, grid.row_colours[1], grid.cells[1][0].background), (0, 0, 0), "{level:?}");
    }
    assert_eq!(build(&packets, L35).0.colour_map.entry(0), Rgb12::new(5, 0, 0), "entry 0 redefined at Level 3.5");
}

// REQ-274 item 7, decision E.43 rule 6: Colour Table Re-mapping applies to
// the Level 1 colour codes and to the start-of-row white and black.
#[test]
fn e43_remapping_applies_to_level_1_colours_and_the_row_defaults() {
    // Table 4 re-mapping value 4: foreground CLUT 1, background CLUT 2.
    const REMAP_1_2: u8 = 4;
    let packets = display_page(0, &[(1, &[b'A', ALPHA_RED, b'B'])], vec![x28(0, &colours(0, 0, false, REMAP_1_2))]);
    let grid = build_both(&packets);
    let cells = &grid.cells[1];
    assert_eq!(cells[0].foreground, colour_index(1, 7), "start-of-row white");
    assert_eq!(cells[0].background, colour_index(2, 0), "start-of-row black");
    assert_eq!(cells[2].foreground, colour_index(1, 1), "alpha red");
}

// REQ-274 item 7, decision E.43 rule 6: Black Background Colour
// Substitution replaces a black background, the start-of-row one included,
// with the row colour; a triplet's black is never substituted.
#[test]
fn e43_black_background_substitution_takes_the_row_colour() {
    const ROW: u8 = 20;
    let row: &[u8] = &[b'A', ALPHA_RED, NEW_BACKGROUND, BLACK_BACKGROUND, b'B', b' ', b'C'];
    let others = vec![x28(0, &colours(0, ROW, true, 0)), x26(0, &[set_position(1, 0), background(6, 0), end()])];
    let grid = build_both(&display_page(0, &[(1, row)], others));
    let cells = &grid.cells[1];
    assert_eq!(cells[0].background, ROW, "start-of-row black");
    assert_eq!(cells[4].background, ROW, "1/C");
    assert_eq!(cells[6].background, 0, "a triplet's black");
}

// ---------------------------------------------------------------------------
// Decision E.44 (DEC-568, REQ-275): page-level presentation data.
// ---------------------------------------------------------------------------

// REQ-275, decision E.44 rule 2: the page's X/28/0 beats the magazine's
// M/29/0.
#[test]
fn e44_the_page_beats_the_magazine() {
    let packets =
        display_page(0, &[(1, b"A")], vec![m29(0, &colours(6, 6, false, 0)), x28(0, &colours(5, 5, false, 0))]);
    assert_eq!(build_both(&packets).screen_colour, 5);
}

// REQ-275, decision E.44 rule 2: the magazine's M/29/0 beats the defaults.
#[test]
fn e44_the_magazine_beats_the_defaults() {
    let packets = display_page(0, &[(1, b"A")], vec![m29(0, &colours(6, 6, false, 0))]);
    let grid = build_both(&packets);
    assert_eq!((grid.screen_colour, grid.row_colours[1]), (6, 6));
}

// REQ-275, decision E.44 rule 2: at Level 3.5 the page's X/28/4 beats the
// magazine's M/29/0; at Level 2.5 X/28/4 is not read.
#[test]
fn e44_the_pages_x28_4_beats_the_magazines_m29_0() {
    let packets =
        display_page(0, &[(1, b"A")], vec![m29(0, &colours(6, 6, false, 0)), x28(4, &colours(7, 7, false, 0))]);
    assert_eq!(build(&packets, L35).0.screen_colour, 7);
    assert_eq!(build(&packets, L25).0.screen_colour, 6);
}

// REQ-275, decision E.44 rule 2: within the page, X/28/0 beats X/28/4.
#[test]
fn e44_within_the_page_x28_0_beats_x28_4() {
    let packets =
        display_page(0, &[(1, b"A")], vec![x28(4, &colours(7, 7, false, 0)), x28(0, &colours(5, 5, false, 0))]);
    assert_eq!(build(&packets, L35).0.screen_colour, 5);
}

// REQ-275, decision E.44 rule 2: within the magazine, M/29/0 beats M/29/4.
#[test]
fn e44_within_the_magazine_m29_0_beats_m29_4() {
    let packets =
        display_page(0, &[(1, b"A")], vec![m29(4, &colours(7, 7, false, 0)), m29(0, &colours(6, 6, false, 0))]);
    assert_eq!(build(&packets, L35).0.screen_colour, 6);
}

// REQ-275, decision E.44 rule 2: the colour tables are the exception: each
// packet defines its own CLUTs, so the page's X/28/0 and the magazine's
// M/29/4 both apply at Level 3.5.
#[test]
fn e44_each_packet_defines_its_own_cluts() {
    let packets = display_page(0, &[(1, b"A")], vec![m29(4, &colour_words(4)), x28(0, &colour_words(9))]);
    let grid = build(&packets, L35).0;
    assert_eq!(grid.colour_map.entry(colour_index(2, 3)), Rgb12::new(9, 3, 0), "CLUT 2 from X/28/0");
    assert_eq!(grid.colour_map.entry(colour_index(0, 1)), Rgb12::new(4, 1, 0), "CLUT 0 from M/29/4");
}

// REQ-275, decision E.44 rule 3: the character set designation follows
// Annex C.4 rule 1: the page's X/28/0 before the magazine's M/29/0.
#[test]
fn e44_designation_follows_annex_c4() {
    let row: &[(u8, &[u8])] = &[(1, &[NATIONAL_4_0])];
    let page_and_magazine =
        display_page(0, row, vec![m29(0, &designation(LATIN_SWEDISH)), x28(0, &designation(LATIN_GERMAN))]);
    assert_eq!(char_of(&build_both(&page_and_magazine).cells[1][0]), '§', "the page's German");
    let magazine = display_page(0, row, vec![m29(0, &designation(LATIN_SWEDISH))]);
    assert_eq!(char_of(&build_both(&magazine).cells[1][0]), 'É', "the magazine's Swedish");
}

// REQ-275, decision E.44 rule 5: a field group with an uncorrectable
// triplet is absent, and the next source supplies that group only.
#[test]
fn e44_a_damaged_field_group_falls_to_the_next_source() {
    // Triplet 13 carries the default colours (and the last colour words);
    // triplet 1 carries the designation, which stays good.
    const TRIPLET_13: usize = 12;
    let page_fields = PresentationFields { colours: colours(5, 5, false, 0).colours, ..designation(LATIN_GERMAN) };
    let magazine_fields = PresentationFields { colours: colours(6, 6, false, 0).colours, ..designation(LATIN_SWEDISH) };
    let packets = display_page(
        0,
        &[(1, &[NATIONAL_4_0])],
        vec![m29(0, &magazine_fields), damage(x28(0, &page_fields), TRIPLET_13)],
    );
    let grid = build_both(&packets);
    assert_eq!(grid.screen_colour, 6, "the colours from M/29/0");
    assert_eq!(char_of(&grid.cells[1][0]), '§', "the designation still from X/28/0");
}

// ---------------------------------------------------------------------------
// Decision E.45 (DEC-569): the screen colour, row colours and borders.
// ---------------------------------------------------------------------------

// Decision E.45 rule 1: a Full Screen Colour triplet beats the page's
// default screen colour.
#[test]
fn e45_a_full_screen_colour_triplet_comes_first() {
    let full_screen = TripletFunction::FullScreenColour(ColourData { index: 17, high: 0 });
    let packets = display_page(0, &[], vec![x28(0, &colours(11, 12, false, 0)), x26(0, &[full_screen, end()])]);
    assert_eq!(build_both(&packets).screen_colour, 17);
}

// Decision E.45 rule 1: the page's default screen colour beats the
// magazine's.
#[test]
fn e45_the_pages_default_screen_colour_beats_the_magazines() {
    let packets = display_page(0, &[], vec![m29(0, &colours(6, 0, false, 0)), x28(0, &colours(11, 0, false, 0))]);
    assert_eq!(build_both(&packets).screen_colour, 11);
}

// Decision E.45 rule 2: an explicit Full Screen Colour colours the rows,
// above the page's default row colour.
#[test]
fn e45_an_explicit_screen_colour_colours_the_rows() {
    let full_screen = TripletFunction::FullScreenColour(ColourData { index: 17, high: 0 });
    let packets = display_page(0, &[], vec![x28(0, &colours(11, 12, false, 0)), x26(0, &[full_screen, end()])]);
    assert!(build_both(&packets).row_colours.iter().all(|&colour| colour == 17));
}

// Decision E.45 rule 2: a default screen colour from X/28 does not replace
// the default row colour.
#[test]
fn e45_a_default_screen_colour_leaves_the_default_row_colour() {
    let packets = display_page(0, &[], vec![x28(0, &colours(11, 12, false, 0))]);
    let grid = build_both(&packets);
    assert_eq!((grid.screen_colour, grid.row_colours[3]), (11, 12));
}

/// Full Row Colour for `row` with a row selection.
fn full_row(row: u8, index: u8, selection: u8) -> TripletFunction {
    let address = if row == 24 { 40 } else { 40 + row };
    TripletFunction::FullRowColour { address, colour: ColourData { index, high: selection } }
}

// Decision E.45 rule 3: "this row only" colours its own row only.
#[test]
fn e45_full_row_colour_this_row_only() {
    let grid = build_both(&page(&[], &[full_row(5, 18, THIS_ROW)]));
    assert_eq!((grid.row_colours[4], grid.row_colours[5], grid.row_colours[6]), (0, 18, 0));
}

// Decision E.45 rule 3: "down to the bottom" colours its row and every
// displayed row below it (rows to 23; row 24 keeps the screen colour, as
// `screen_and_row_colours` in `level25.rs` shows), and continues below a
// later "this row only" row.
#[test]
fn e45_full_row_colour_down_to_the_bottom() {
    let grid = build_both(&page(&[], &[full_row(10, 19, DOWN_TO_BOTTOM), full_row(12, 20, THIS_ROW)]));
    let rows = &grid.row_colours;
    assert_eq!((rows[9], rows[10], rows[11]), (0, 19, 19));
    assert_eq!((rows[12], rows[13], rows[23]), (20, 19, 19), "it continues below the later row");
}

// Decision E.45 rule 4: Address Display Row 0 and Full Row Colour with a
// reserved selection set the active position but no colour.
#[test]
fn e45_a_reserved_selection_sets_the_position_only() {
    let grid = build_both(&page(
        &[],
        &[full_row(14, 22, RESERVED_SELECTION), g0(1, b'x'), row_0(21, RESERVED_SELECTION), g0(9, b'y')],
    ));
    assert_eq!((grid.row_colours[14], grid.row_colours[0]), (0, 0));
    assert_eq!((char_of(&grid.cells[14][1]), char_of(&grid.cells[0][9])), ('x', 'y'));
}

// ---------------------------------------------------------------------------
// Decision E.46 (DEC-570): the active position, triplet order and damage.
// ---------------------------------------------------------------------------

// Decision E.46 rule 1: only Full Row Colour, Set Active Position and
// Address Display Row 0 (address 63) set the active row.
#[test]
fn e46_only_three_row_triplets_set_the_active_row() {
    let local = [
        full_row(5, 0, THIS_ROW),
        g0(1, b'a'),
        TripletFunction::ProgrammeDeliveryRow { mode: 0b01000, address: 50, data: 0 },
        TripletFunction::ReservedRow { mode: 0b00011, address: 52, data: 0 },
        TripletFunction::AddressDisplayRow0 { address: 50, colour: ColourData { index: 0, high: THIS_ROW } },
        g0(2, b'b'),
        set_position(7, 0),
        g0(3, b'c'),
        row_0(0, THIS_ROW),
        g0(9, b'd'),
    ];
    let grid = build_both(&page(&[], &local));
    assert_eq!((char_of(&grid.cells[5][1]), char_of(&grid.cells[5][2])), ('a', 'b'), "other row triplets keep row 5");
    assert_eq!((char_of(&grid.cells[7][3]), char_of(&grid.cells[0][9])), ('c', 'd'));
}

// Decision E.46 rule 2: Set Active Position with a column of 40 or more
// sets the row only.
#[test]
fn e46_a_column_of_40_or_more_sets_the_row_only() {
    let (object, invoke) = marker_object();
    let local = [set_position(4, 12), TripletFunction::SetActivePosition { address: 47, column: 45 }, invoke, end()];
    let grid = build_both(&display_page(0, &[], vec![x26(0, &local), x26(1, &object)]));
    assert_eq!(char_of(&grid.cells[7][12]), 'X', "row 7, column 12 kept");
}

// Decision E.46 rule 3: every column triplet sets the column, a character
// whose data is below 2/0 included; Programme Delivery Control and the
// reserved modes do not.
#[test]
fn e46_column_triplets_set_the_column() {
    let (object, invoke) = marker_object();
    let cases = [
        (g0(30, 0x10), 30),
        (TripletFunction::ProgrammeDeliveryColumn { column: 30, data: 0 }, 2),
        (TripletFunction::ReservedColumn { column: 30, mode: 0b00100, data: 0 }, 2),
    ];
    for (function, column) in cases {
        let local = [set_position(4, 2), function, invoke, end()];
        let grid = build_both(&display_page(0, &[], vec![x26(0, &local), x26(1, &object)]));
        assert_eq!(char_of(&grid.cells[4][column]), 'X', "{function:?}");
    }
}

// Decision E.46 rule 4: a triplet addressing a position earlier than the
// previous one is applied where it points.
#[test]
fn e46_an_out_of_order_triplet_is_applied_where_it_points() {
    let grid = build_both(&page(&[], &[set_position(2, 0), g0(6, b'b'), g0(3, b'c')]));
    assert_eq!((char_of(&grid.cells[2][6]), char_of(&grid.cells[2][3])), ('b', 'c'));
}

// Decision E.46 rule 5: any triplet with address 63 and mode 11111 ends the
// data, whatever its type bits.
#[test]
fn e46_a_termination_marker_ends_the_data_whatever_its_type() {
    for kind in 0..=0b111 {
        let local = [set_position(1, 0), g0(1, b'a'), TripletFunction::Termination { kind }, g0(2, b'b')];
        let grid = build_both(&page(&[], &local));
        assert_eq!((char_of(&grid.cells[1][1]), char_of(&grid.cells[1][2])), ('a', ' '), "type {kind:03b}");
    }
}

// Decision E.46 rule 5: an Object Definition met in the local enhancement
// data ends it.
#[test]
fn e46_an_object_definition_ends_the_local_data() {
    let definition = common::define(ObjectKind::Active, 0b11, common::local_number(0, 2));
    let grid =
        build_both(&display_page(0, &[], vec![x26(0, &[set_position(1, 0), g0(1, b'a'), definition, g0(2, b'b')])]));
    assert_eq!((char_of(&grid.cells[1][1]), char_of(&grid.cells[1][2])), ('a', ' '));
}

// Decision E.46 rule 6: an Origin Modifier not immediately followed by an
// object invocation is discarded.
#[test]
fn e46_an_origin_modifier_without_its_invocation_is_discarded() {
    let (object, invoke) = marker_object();
    let local = [set_position(3, 2), common::origin(1, 10), g0(5, b'l'), invoke, end()];
    let grid = build_both(&display_page(0, &[], vec![x26(0, &local), x26(1, &object)]));
    assert_eq!(char_of(&grid.cells[3][5]), 'X', "placed at the active position, no offsets");
    assert_eq!(char_of(&grid.cells[4][15]), ' ');
}

// Decision E.46 rule 7: an uncorrectable triplet is skipped, and does not
// move the active position.
#[test]
fn e46_a_damaged_triplet_is_skipped_without_moving() {
    const SECOND_SET_POSITION: usize = 2;
    let local = [set_position(2, 0), g0(5, b'a'), set_position(9, 0), g0(6, b'b'), end()];
    let packets = display_page(0, &[], vec![damage(x26(0, &local), SECOND_SET_POSITION)]);
    for level in [L25, L35] {
        let (grid, report) = build(&packets, level);
        assert_eq!(char_of(&grid.cells[2][6]), 'b', "{level:?}: still row 2");
        assert_eq!(report.unwrap().faults.damaged_triplets, 1);
    }
}

// ---------------------------------------------------------------------------
// Decision E.47 (DEC-571): attribute contention and the display attributes.
// ---------------------------------------------------------------------------

// Decision E.47 rule 1: foreground and background colour are attribute
// types of their own: one does not end the other.
#[test]
fn e47_foreground_and_background_are_separate_types() {
    let grid = build_both(&page(&[(4, b"ABCDEF")], &[set_position(4, 0), foreground(2, 20), background(4, 27)]));
    let cell = grid.cells[4][5];
    assert_eq!((cell.foreground, cell.background), (20, 27));
}

const CONTENTION_ROW: &[u8] = &[b' ', b' ', b' ', b' ', ALPHA_RED, b'A'];

// Decision E.47 rule 2, Annex C.2 example 1: the attribute that takes
// effect later wins: Foreground Blue at n − 1 is overtaken by alpha red.
#[test]
fn e47_the_later_effect_wins() {
    let grid = build_both(&page(&[(1, CONTENTION_ROW)], &[set_position(1, 0), foreground(4, colour_index(0, 4))]));
    assert_eq!(grid.cells[1][5].foreground, colour_index(0, 1));
}

// Decision E.47 rule 2, Annex C.2 example 2: both take effect at n; the one
// placed later, the triplet at n, wins.
#[test]
fn e47_the_later_placement_wins() {
    let grid = build_both(&page(&[(1, CONTENTION_ROW)], &[set_position(1, 0), foreground(5, colour_index(0, 4))]));
    assert_eq!(grid.cells[1][5].foreground, colour_index(0, 4));
}

// Decision E.47 rule 2, Annex C.2 example 3: placed at the same cell, the
// later transmitted wins.
#[test]
fn e47_the_later_transmission_wins() {
    let packets = display_page(
        0,
        &[(1, CONTENTION_ROW)],
        vec![
            x26(0, &[set_position(1, 0), foreground(5, colour_index(0, 4))]),
            x26(1, &[set_position(1, 0), foreground(5, colour_index(0, 3)), end()]),
        ],
    );
    assert_eq!(build_both(&packets).cells[1][5].foreground, colour_index(0, 3));
}

// Decision E.47 rule 3: a set-at Level 1 attribute and an X/26 attribute of
// the same type at the same cell: the X/26 attribute wins.
#[test]
fn e47_an_x26_attribute_beats_a_level_1_attribute_at_the_same_cell() {
    let blue = colour_index(0, 4);
    let grid =
        build_both(&page(&[(1, &[ALPHA_RED, NEW_BACKGROUND, b'A'])], &[set_position(1, 0), background(1, blue)]));
    assert_eq!((grid.cells[1][1].background, grid.cells[1][2].background), (blue, blue));
}

// Decision E.47 rule 4: a 0 bit of a Display Attributes triplet cancels its
// function from that cell on, a Level 1 conceal included.
#[test]
fn e47_a_zero_bit_cancels_a_level_1_function() {
    let grid = build_both(&page(
        &[(1, &[CONCEAL, b'A', b'B'])],
        &[set_position(1, 0), display(1, DisplayAttributes::default())],
    ));
    let concealed: Vec<bool> = grid.cells[1][..3].iter().map(|cell| cell.conceal).collect();
    assert_eq!(concealed, [true, false, false]);
}

// Decision E.47 rule 5: a Level 1 colour code ends only a Level 1 conceal; a
// non-spacing conceal lasts until a triplet clears it or the row ends.
#[test]
fn e47_a_colour_code_ends_only_a_level_1_conceal() {
    let conceal = DisplayAttributes { conceal: true, ..DisplayAttributes::default() };
    let level_1 = build_both(&page(&[(1, &[CONCEAL, b'A', ALPHA_RED, b'B'])], &[]));
    assert!(level_1.cells[1][1].conceal && !level_1.cells[1][3].conceal, "1/8 ended by a colour code");
    let grid = build_both(&page(&[(1, &[b'A', ALPHA_RED, b'B'])], &[set_position(1, 0), display(0, conceal)]));
    assert!(grid.cells[1][2].conceal && grid.cells[1][39].conceal, "the non-spacing conceal survives it");
}

// Decision E.47 rule 6: on a page that is neither a newsflash nor a
// subtitle page, Level 1 Start Box opens no window; the boxing bit does.
#[test]
fn e47_windows_come_only_from_the_boxing_bit() {
    let boxing = DisplayAttributes { boxing: true, ..DisplayAttributes::default() };
    let row: &[u8] = &[START_BOX, START_BOX, b'A', b'B'];
    let grid = build_both(&page(&[(2, row)], &[set_position(2, 0), display(3, boxing)]));
    let boxed: Vec<bool> = grid.cells[2][..5].iter().map(|cell| cell.boxed).collect();
    assert_eq!(boxed, [false, false, false, true, true]);
}

// Decision E.47 rule 7, Table 26 notes 11 and 12: a non-spacing Underline or
// Separated attribute affects Level 1 mosaics, but the Level 1 code 1/A
// does not affect an X/26 G1 character.
#[test]
fn e47_level_1_separated_does_not_reach_enhancement_mosaics() {
    let underline = DisplayAttributes { underline: true, ..DisplayAttributes::default() };
    let row = [MOSAIC_RED, SEPARATED_MOSAICS, 0x7F, b' ', b' ', b' ', b' ', 0x7F];
    let local = [set_position(3, 0), TripletFunction::G1Character { column: 3, code: 0x7F }, display(5, underline)];
    let grid = build_both(&page(&[(3, &row)], &local));
    assert_eq!(grid.cells[3][3].glyph, Glyph::Mosaic { pattern: 0x3F, separated: false }, "1/A ignored");
    assert_eq!(grid.cells[3][7].glyph, Glyph::Mosaic { pattern: 0x3F, separated: true }, "the triplet separates");
}

// ---------------------------------------------------------------------------
// Decision E.48 (DEC-572): flash.
// ---------------------------------------------------------------------------

// Decision E.48 rule 1: the Level 1 Flash code is the same Level 1 flash at
// every level; the presentation level never changes it.
#[test]
fn e48_level_1_flash_is_the_same_at_every_level() {
    let packets = page(&[(1, &[FLASH, b'A', STEADY, b'B'])], &[]);
    let level_1 = build(&packets, L1).0;
    let grid = build_both(&packets);
    let flashing = |grid: &Grid| grid.cells[1][..4].iter().map(|cell| cell.flash).collect::<Vec<_>>();
    assert_eq!(flashing(&grid), flashing(&level_1));
    assert_eq!(flashing(&grid), [false, true, false, false], "Flash is set-after, Steady set-at");
    let attributes = grid.cells[1][1].attributes;
    assert_eq!((attributes.flash_mode(), attributes.flash_rate()), (FlashMode::Normal, FlashRate::Level1));
}

// ---------------------------------------------------------------------------
// Decision E.50 (DEC-574): the character repertoire.
// ---------------------------------------------------------------------------

// Decision E.50 rule 1: the national option comes from the highest-priority
// X/28 or M/29 designation when there is one, and from the header's C12 to
// C14 otherwise.
#[test]
fn e50_the_national_option_comes_from_x28_before_the_header() {
    let row: &[(u8, &[u8])] = &[(1, &[NATIONAL_4_0])];
    let header_only = display_page(C14_GERMAN, row, vec![]);
    assert_eq!(char_of(&build_both(&header_only).cells[1][0]), '§', "the header's German");
    let with_x28 = display_page(C14_GERMAN, row, vec![x28(0, &designation(LATIN_SWEDISH))]);
    assert_eq!(char_of(&build_both(&with_x28).cells[1][0]), 'É', "X/28/0's Swedish");
}

// Decision E.50 rule 2: at Level 2.5, Modified G0 and G2 designations beyond
// two pairs, counting the default, are ignored; Level 3.5 has no limit.
#[test]
fn e50_level_2_5_takes_one_modified_pair_and_level_3_5_any() {
    // 0100000 Cyrillic 1, 0110111 Greek (Table 32).
    let local = [
        set_position(1, 0),
        TripletFunction::ModifiedDesignation { column: 2, designation: 0b010_0000 },
        g0(3, b'B'),
        set_position(2, 0),
        TripletFunction::ModifiedDesignation { column: 4, designation: 0b011_0111 },
        g0(5, b'B'),
    ];
    let packets = page(&[], &local);
    let greek = Some(CharSet::G0(G0 { set: G0Set::Greek, national: None }));
    let cyrillic = Some(CharSet::G0(G0 { set: G0Set::Cyrillic1, national: None }));
    assert_eq!(set_of(&build(&packets, L25).0.cells[2][5]), Some(latin()), "a third pair at Level 2.5");
    assert_eq!(set_of(&build(&packets, L25).0.cells[1][3]), cyrillic, "the second pair at Level 2.5");
    assert_eq!(set_of(&build(&packets, L35).0.cells[2][5]), greek, "a third pair at Level 3.5");
}

// Decision E.50 rule 3: a G1 character written by a triplet is a mosaic,
// drawn separated by the non-spacing attribute.
#[test]
fn e50_a_g1_character_from_a_triplet_is_a_mosaic() {
    let separated = DisplayAttributes { underline: true, ..DisplayAttributes::default() };
    let local = [
        set_position(1, 0),
        TripletFunction::G1Character { column: 1, code: 0x7F },
        display(2, separated),
        TripletFunction::G1Character { column: 3, code: 0x7F },
    ];
    let grid = build_both(&page(&[], &local));
    assert_eq!(grid.cells[1][1].glyph, Glyph::Mosaic { pattern: 0x3F, separated: false });
    assert_eq!(grid.cells[1][3].glyph, Glyph::Mosaic { pattern: 0x3F, separated: true });
}

// Decision E.50 rule 3, Table 47 note 3: a G1 triplet's codes in columns 4
// and 5 are the current G0 set's characters, without national option.
#[test]
fn e50_g1_columns_4_and_5_are_g0_without_national_option() {
    let local = [set_position(1, 0), TripletFunction::G1Character { column: 1, code: NATIONAL_4_0 }];
    let grid = build_both(&display_page(C14_GERMAN, &[], local_data(&local)));
    assert_eq!(set_of(&grid.cells[1][1]), Some(latin()));
    assert_eq!(char_of(&grid.cells[1][1]), '@', "not the German §");
}

// Decision E.50 rule 4: a G3 character written by X/26 does not change the
// held mosaic.
#[test]
fn e50_a_g3_character_does_not_change_the_held_mosaic() {
    let row: &[u8] = &[MOSAIC_RED, 0x7F, HOLD_MOSAICS, MOSAIC_RED];
    let local = [set_position(1, 0), TripletFunction::G3Character { column: 2, code: 0x51, level_15_form: false }];
    let grid = build_both(&page(&[(1, row)], &local));
    let held = Glyph::Mosaic { pattern: 0x3F, separated: false };
    assert_eq!((grid.cells[1][3].glyph, grid.cells[1][3].held), (held, true), "still the mosaic of column 1");
}

// Decision E.50 rule 4: Hold Mosaics never replaces a character written by
// X/26.
#[test]
fn e50_hold_mosaics_never_replaces_an_enhancement_character() {
    let row: &[u8] = &[MOSAIC_RED, 0x7F, HOLD_MOSAICS, MOSAIC_RED];
    let local = [set_position(1, 0), TripletFunction::G3Character { column: 2, code: 0x51, level_15_form: false }];
    let grid = build_both(&page(&[(1, row)], &local));
    assert_eq!(set_of(&grid.cells[1][2]), Some(CharSet::G3));
}

// Decision E.50 rule 6: undefined G3 positions (6/E, 6/F, 7/E, 7/F) and the
// undefined Latin G2 positions show a space, and are counted.
#[test]
fn e50_undefined_positions_show_a_space_and_are_counted() {
    const UNDEFINED_G3: [u8; 4] = [0x6E, 0x6F, 0x7E, 0x7F];
    const UNDEFINED_LATIN_G2: u8 = 0x59;
    let mut local = vec![set_position(1, 0)];
    local.extend(UNDEFINED_G3.iter().zip(1..).map(|(&code, column)| TripletFunction::G3Character {
        column,
        code,
        level_15_form: false,
    }));
    local.push(TripletFunction::G2Character { column: 5, code: UNDEFINED_LATIN_G2 });
    let packets = page(&[(1, b"ABCDEF")], &local);
    for level in [L25, L35] {
        let (grid, report) = build(&packets, level);
        assert!(grid.cells[1][1..6].iter().all(|cell| cell.glyph == Glyph::Space), "{level:?}");
        assert_eq!(report.unwrap().faults.undefined_characters, 5, "{level:?}");
    }
}

// Decision E.50 rule 7 as amended by decision E.96: a Font Style triplet is
// not acted on.
#[test]
fn e50_a_font_style_triplet_is_not_acted_on() {
    let style = TripletFunction::UnsupportedColumn { column: 1, mode: FONT_STYLE_MODE, data: EVERY_STYLE_TWO_ROWS };
    let with = build_both(&page(&[(1, b"ABCD")], &[set_position(1, 0), style]));
    let without = build_both(&page(&[(1, b"ABCD")], &[]));
    common::assert_same_cells(&with, &without, "Font Style");
}
