//! Whole hostile pages, built by the Level 1 pass, the Level 1.5 pass and
//! the enhancement processor as a receiver builds them, that try to write
//! the receiver's own cells: the status area (REQ-418, REQ-SEC-005) and
//! row 24 with its link prompts (REQ-437, REQ-SEC-024).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

mod common;

use common::{
    DISPLAY, Receiver, background, char_of, define, display_page, end, foreground, invoke, local_number, row_0,
    set_position, text_at, x26,
};
use neoteletext_core::compose::compose_level;
use neoteletext_core::config::{Config, PresentationLevel, Row24Policy};
use neoteletext_core::consts::{
    ALPHA_RED, C5_NEWSFLASH, C7_SUPPRESS_HEADER, COLOUR_RED_BIT, COLOUR_WHITE, COLUMNS, RECEIVER_COLUMNS,
    STATUS_COLUMNS,
};
use neoteletext_core::enhance;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::packet::{ObjectKind, ObjectSource, Packet, TripletFunction};
use neoteletext_core::types::{PageAddress, SubCode};

const LEVELS: [PresentationLevel; 3] =
    [PresentationLevel::Level15, PresentationLevel::Level25, PresentationLevel::Level35];
/// The last display row, row 24.
const ROW_24: usize = 24;
/// Row 24's number as a packet number and in [`set_position`].
const PACKET_24: u8 = 24;

/// Page 100 built at `level` with the viewer's display options, as a
/// receiver builds it: the Level 1 pass, then the Level 1.5 pass or the
/// enhancement processor.
fn build(packets: &[Packet], level: PresentationLevel, row24: Row24Policy, options: &DisplayOptions) -> Box<Grid> {
    let config = Config { level, row24, ..Config::default() };
    Receiver::new().run(packets, config, |decoder| {
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let page = decoder.store().get(address, SubCode::ANY).expect("the page is stored");
        let magazine = decoder.magazine_data(address.magazine);
        let lookup = decoder.enhancement_lookup();
        let mut grid = Box::new(Grid::EMPTY);
        build_grid(&mut grid, Some(page), &magazine, &config, options);
        if level.is_enhanced() {
            let _ = enhance::apply(&mut grid, page, &magazine, &lookup, &config, options);
        } else {
            let _ = compose_level(&mut grid, page, &magazine, &lookup, &config, options);
        }
        grid
    })
}

/// A local Active object, defined in packet X/26/1, that writes `text`
/// from its origin.
fn local_object(text: &[u8]) -> Packet {
    let mut body = vec![define(ObjectKind::Active, 0b11, local_number(1, 0))];
    body.extend(text_at(0, text));
    body.push(TripletFunction::Termination { kind: 0b101 });
    x26(1, &body)
}

/// The invocation of [`local_object`] at the active position.
fn invoke_local_object() -> TripletFunction {
    invoke(ObjectSource::Local, ObjectKind::Active, local_number(1, 0))
}

/// REQ-418 (REQ-SEC-005): a page that writes to row 0, columns 0 to 7, by
/// every route the levels give it leaves the status area as the receiver
/// drew it: a Level 1.5 or 2.5 character written there through X/26
/// Address Display Row 0, a colour change there, an object invoked there,
/// and a full row colour for row 0; on an ordinary page, on a page with
/// C7 (suppress header) while a security indicator is on, and on a
/// newsflash page. The status area is compared with the same page built
/// without the hostile data. Its colours, which a page may also try to
/// redefine through its Colour Map, are the row renderer's fixed receiver
/// colours, tested by `the_receivers_cells_are_drawn_in_its_own_colours`
/// and, for the terminal, by `a_redefined_clut_0_leaves_the_receivers_cells_alone`.
#[test]
fn a_hostile_page_cannot_write_the_status_area_by_any_route() {
    let mut local: Vec<TripletFunction> = vec![row_0(COLOUR_RED_BIT, 0)];
    local.extend(text_at(0, b"FORGED!!"));
    local.extend([background(0, COLOUR_RED_BIT), foreground(1, COLOUR_WHITE), invoke_local_object(), end()]);
    let hostile_data = vec![x26(0, &local), local_object(b"OBJECT!!")];
    let mut status = [b' '; STATUS_COLUMNS];
    status[..4].copy_from_slice(b"P100");
    for control in [0, C7_SUPPRESS_HEADER, C5_NEWSFLASH | C7_SUPPRESS_HEADER] {
        for security_indicator in [false, true] {
            let options = DisplayOptions { status, security_indicator, ..DisplayOptions::default() };
            let plain = display_page(control, &[(1, b"PAGE")], vec![]);
            let hostile = display_page(control, &[(1, b"PAGE")], hostile_data.clone());
            for level in LEVELS {
                let expected = build(&plain, level, Row24Policy::AnnexH, &options);
                let grid = build(&hostile, level, Row24Policy::AnnexH, &options);
                let label = format!("control {control:#06X}, indicator {security_indicator}, {level:?}");
                assert_eq!(grid.cells[0][..RECEIVER_COLUMNS], expected.cells[0][..RECEIVER_COLUMNS], "{label}");
                assert!((0..RECEIVER_COLUMNS).all(|column| grid.is_receiver_cell(0, column)), "{label}");
                // The routes are live: past the status area the object,
                // invoked at column 1 (the active position the colour
                // change left), writes the header's row.
                if control == 0 && level.is_enhanced() {
                    assert_eq!(char_of(&grid.cells[0][RECEIVER_COLUMNS]), '!', "{label}");
                }
            }
        }
    }
}

/// The link prompts of the tests: a red prompt "NEWS" from column 0.
fn prompt_row() -> [u8; COLUMNS] {
    let mut row = [b' '; COLUMNS];
    row[0] = ALPHA_RED;
    row[1..5].copy_from_slice(b"NEWS");
    row
}

/// Local data that overlaps the prompt of [`prompt_row`] in row 24: a red
/// background behind it, other characters over it, and an object written
/// over it.
fn overlapping_data() -> Vec<Packet> {
    let mut local = vec![set_position(PACKET_24, 0), background(1, COLOUR_RED_BIT), foreground(3, COLOUR_WHITE)];
    local.extend(text_at(2, b"EVIL"));
    local.extend([set_position(PACKET_24, 6), invoke_local_object(), end()]);
    vec![x26(0, &local), local_object(b"FAKE PROMPT")]
}

/// REQ-437 (REQ-SEC-024): enhancement data that overlaps a row 24 the
/// receiver draws from a page's link lines changes none of it, at any
/// level, so the red prompt stays whole and its key keeps its link.
#[test]
fn an_enhancement_cannot_overlap_a_receiver_drawn_row_24() {
    let options = DisplayOptions { row24: Some(prompt_row()), ..DisplayOptions::default() };
    let plain = display_page(0, &[(1, b"PAGE")], vec![]);
    let hostile = display_page(0, &[(1, b"PAGE")], overlapping_data());
    for level in LEVELS {
        let expected = build(&plain, level, Row24Policy::WhenPresent, &options);
        let grid = build(&hostile, level, Row24Policy::WhenPresent, &options);
        assert_eq!(grid.cells[ROW_24], expected.cells[ROW_24], "{level:?}");
        assert_eq!(grid.prompts_shown(), [true, false, false, false], "{level:?}");
    }
}

/// REQ-437 (REQ-SEC-024): on a page that sends its own row 24 (a broadcast
/// page), enhancement data that overlaps the red prompt so that it is
/// drawn on its own colour takes the red key's link away, at Levels 2.5
/// and 3.5, where a page can change colours; without the overlap the key
/// works.
#[test]
fn an_enhancement_that_overlaps_a_pages_prompt_takes_its_key_away() {
    let options = DisplayOptions::default();
    let prompt = prompt_row();
    let plain = display_page(0, &[(1, b"PAGE"), (PACKET_24, &prompt)], vec![]);
    let hostile = display_page(0, &[(1, b"PAGE"), (PACKET_24, &prompt)], overlapping_data());
    for level in [PresentationLevel::Level25, PresentationLevel::Level35] {
        let shown = build(&plain, level, Row24Policy::WhenPresent, &options).prompts_shown();
        assert_eq!(shown, [true, false, false, false], "{level:?}: the prompt alone");
        let overlapped = build(&hostile, level, Row24Policy::WhenPresent, &options).prompts_shown();
        assert_eq!(overlapped, [false; 4], "{level:?}: the prompt overlapped");
    }
}
