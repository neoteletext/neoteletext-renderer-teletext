//! The test harness of REQ-RND-021 (REQ-396): a reference caller that
//! calls the row renderer in unusual orders and records what it draws,
//! used for the independence test of REQ-RND-017 (REQ-390) and to test
//! change tracking (REQ-RND-019) by redrawing only the rows the tracker
//! reports into a kept framebuffer and comparing it with a whole redraw.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

mod common;

use common::{
    background, build, colours, display_page, end, foreground, g0, local_data, set_position, worst_case_page, x26, x28,
};
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{PAGE_WIDTH, ROWS};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{Glyph, Grid, Size, Zoom};
use neoteletext_core::packet::TripletFunction;
use neoteletext_core::render::{
    ChangeTracker, Field, FlashPhase, IndexBuffer, IndexFormat, RenderOptions, render_line, render_row, render_screen,
};

/// Every row, as the tracker's and `render_screen`'s bitmask.
const ALL_ROWS: u32 = (1 << ROWS) - 1;
/// Deterministic shuffles of the rows tried for each page.
const SHUFFLES: u64 = 8;

/// The reference caller: the renderer, called in the ways a port may call
/// it, each giving a whole screen of 8-bit indices.
struct Harness {
    glyphs: GlyphSet<'static>,
}

impl Harness {
    fn new() -> Self {
        Self { glyphs: GlyphSet::noto_teletext() }
    }

    fn screen_bytes(options: RenderOptions) -> usize {
        PAGE_WIDTH * options.row_height() * ROWS
    }

    /// The whole screen in one call.
    fn screen(&self, grid: &Grid, options: RenderOptions) -> Vec<u8> {
        let mut frame = vec![0u8; Self::screen_bytes(options)];
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut frame, stride: PAGE_WIDTH };
        render_screen(grid, ALL_ROWS, &self.glyphs, options, &mut buffer).unwrap();
        frame
    }

    /// Each row drawn on its own, into a fresh buffer, in `order`, and put
    /// in its place.
    fn rows_in(&self, grid: &Grid, options: RenderOptions, order: &[usize]) -> Vec<u8> {
        let row_bytes = PAGE_WIDTH * options.row_height();
        let mut frame = vec![0u8; Self::screen_bytes(options)];
        for &row in order {
            let mut single = vec![0u8; row_bytes];
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut single, stride: PAGE_WIDTH };
            render_row(grid, row, Field::Both, &self.glyphs, options, &mut buffer).unwrap();
            frame[row * row_bytes..][..row_bytes].copy_from_slice(&single);
        }
        frame
    }

    /// One screen line at a time.
    fn lines(&self, grid: &Grid, options: RenderOptions) -> Vec<u8> {
        let mut frame = vec![0u8; Self::screen_bytes(options)];
        for (y, line) in frame.chunks_mut(PAGE_WIDTH).enumerate() {
            render_line(grid, y, Zoom::Off, &self.glyphs, options, IndexFormat::Bits8, line).unwrap();
        }
        frame
    }

    /// One field at a time, the second before the first, each row's lines
    /// woven back into place.
    fn fields(&self, grid: &Grid, options: RenderOptions) -> Vec<u8> {
        let height = options.row_height();
        let mut frame = vec![0u8; Self::screen_bytes(options)];
        for (field, first_line) in [(Field::Second, 1), (Field::First, 0)] {
            for row in 0..ROWS {
                let mut half = vec![0u8; PAGE_WIDTH * height / 2];
                let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut half, stride: PAGE_WIDTH };
                render_row(grid, row, field, &self.glyphs, options, &mut buffer).unwrap();
                for (index, line) in half.chunks(PAGE_WIDTH).enumerate() {
                    let y = row * height + first_line + 2 * index;
                    frame[y * PAGE_WIDTH..][..PAGE_WIDTH].copy_from_slice(line);
                }
            }
        }
        frame
    }
}

/// A change made to a grid between two redraws.
type Change = Box<dyn Fn(&mut Grid)>;

/// A port's kept framebuffer and its change tracker: after each change it
/// redraws only the rows the tracker reports.
struct KeptScreen {
    frame: Vec<u8>,
    tracker: ChangeTracker,
}

impl KeptScreen {
    fn new(options: RenderOptions) -> Self {
        Self { frame: vec![0u8; Harness::screen_bytes(options)], tracker: ChangeTracker::new() }
    }

    /// Redraws the reported rows and returns them.
    fn update(&mut self, harness: &Harness, grid: &Grid, options: RenderOptions) -> u32 {
        let rows = self.tracker.changed_rows(grid, options);
        let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut self.frame, stride: PAGE_WIDTH };
        render_screen(grid, rows, &harness.glyphs, options, &mut buffer).unwrap();
        rows
    }
}

/// The rows in a Fisher-Yates shuffle drawn from `seed` by a linear
/// congruential generator (Knuth's MMIX constants), the same on every run.
fn shuffled_rows(seed: u64) -> Vec<usize> {
    let mut rows: Vec<usize> = (0..ROWS).collect();
    let mut state = seed;
    for last in (1..rows.len()).rev() {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        rows.swap(last, ((state >> 32) % (last as u64 + 1)) as usize);
    }
    rows
}

/// A Level 1 page: text, mosaics held and separated, double height,
/// flash, conceal and a box, in colour.
fn level_1_page() -> Box<Grid> {
    let rows: &[(u8, &[u8])] = &[
        (1, b"\x0dDOUBLE HEIGHT TITLE"),
        (4, b"\x01RED \x08FLASHING\x09 STEADY \x18HIDDEN"),
        (6, b"\x12\x7f\x2c\x1e\x14\x7f\x1a\x7f\x7f\x19\x7f"),
        (8, b"\x0b\x0bBOXED\x0a\x0a OUTSIDE"),
        (12, b"\x1d\x04\x0dBLUE ON YELLOW"),
        (20, b"abcdefghijklmnopqrstuvwxyz 0123456789"),
        (23, b"\x16\x7f\x7f\x7f\x15\x6a\x35"),
    ];
    build(&display_page(0, rows, vec![]), PresentationLevel::Level1).0
}

/// A Level 1.5 page: G0 characters with diacritical marks and a G2
/// character placed by packet X/26.
fn level_15_page() -> Box<Grid> {
    let local = [
        set_position(3, 0),
        TripletFunction::G0Diacritic { column: 2, code: b'e', diacritic: 2 },
        TripletFunction::G0Diacritic { column: 3, code: b'c', diacritic: 15 },
        TripletFunction::G2Character { column: 5, code: 0x41 },
    ];
    let packets = display_page(0, &[(3, b"  .. . DIACRITICS")], local_data(&local));
    build(&packets, PresentationLevel::Level15).0
}

/// A Level 2.5 page: screen and row colours, and local colour and
/// character triplets.
fn level_25_page() -> Box<Grid> {
    let local = [set_position(5, 0), foreground(0, 3), background(1, 12), g0(2, b'X'), end()];
    let others = vec![x28(0, &colours(4, 2, true, 0)), x26(0, &local)];
    build(&display_page(0, &[(5, b"   COLOURS"), (9, b"\x08FLASH")], others), PresentationLevel::Level25).0
}

fn palette_options() -> RenderOptions {
    RenderOptions::from_config(&Config::default(), false)
}

fn resolved_options(unit: u8) -> RenderOptions {
    RenderOptions::resolved(&Config::default(), false, FlashPhase::at_unit(unit))
}

/// The set of pages, each with the options its level draws with.
fn pages() -> Vec<(&'static str, Box<Grid>, RenderOptions)> {
    vec![
        ("Level 1", level_1_page(), palette_options()),
        ("Level 1.5", level_15_page(), palette_options()),
        ("Level 2.5", level_25_page(), resolved_options(0)),
        ("Level 3.5 worst case", build(&worst_case_page(), PresentationLevel::Level35).0, resolved_options(3)),
    ]
}

/// REQ-390 (REQ-RND-017), REQ-396: every row of a set of pages, rendered
/// in order, in reverse order, in random orders, individually, one line at
/// a time and one field at a time, gives identical pixels; and the rows of
/// all the pages drawn interleaved in a random order give each page's
/// screen, so no call keeps anything from another.
#[test]
fn rows_of_a_set_of_pages_render_identically_in_every_order() {
    let harness = Harness::new();
    let pages = pages();
    for (name, grid, options) in &pages {
        let whole = harness.screen(grid, *options);
        assert!(whole.iter().any(|&index| index != whole[0]), "{name}: the page draws something");
        let forward: Vec<usize> = (0..ROWS).collect();
        let reverse: Vec<usize> = (0..ROWS).rev().collect();
        assert_eq!(harness.rows_in(grid, *options, &forward), whole, "{name}: in order");
        assert_eq!(harness.rows_in(grid, *options, &reverse), whole, "{name}: in reverse order");
        for seed in 1..=SHUFFLES {
            assert_eq!(harness.rows_in(grid, *options, &shuffled_rows(seed)), whole, "{name}: shuffle {seed}");
        }
        assert_eq!(harness.lines(grid, *options), whole, "{name}: one line at a time");
        assert_eq!(harness.fields(grid, *options), whole, "{name}: one field at a time");
    }
    // Rows of every page, interleaved in a random order.
    let mut frames: Vec<Vec<u8>> =
        pages.iter().map(|(_, _, options)| vec![0u8; Harness::screen_bytes(*options)]).collect();
    let mut jobs: Vec<(usize, usize)> =
        (0..pages.len()).flat_map(|page| (0..ROWS).map(move |row| (page, row))).collect();
    let order = shuffled_rows(SHUFFLES + 1);
    jobs.sort_by_key(|&(page, row)| (order[row], page));
    for (page, row) in jobs {
        let (_, grid, options) = &pages[page];
        let single = harness.rows_in(grid, *options, &[row]);
        let row_bytes = PAGE_WIDTH * options.row_height();
        frames[page][row * row_bytes..][..row_bytes].copy_from_slice(&single[row * row_bytes..][..row_bytes]);
    }
    for ((name, grid, options), frame) in pages.iter().zip(&frames) {
        assert_eq!(*frame, harness.screen(grid, *options), "{name}: rows of all the pages interleaved");
    }
}

/// REQ-396, REQ-RND-019 (REQ-395): after each change of a page, of reveal
/// and of the flash phase, redrawing only the rows the change tracker
/// reports into a kept framebuffer gives the pixels of a whole redraw.
#[test]
fn redrawing_the_reported_rows_gives_a_whole_redraw() {
    let harness = Harness::new();
    // The palette mode, on the Level 1 page.
    let mut grid = level_1_page();
    let options = palette_options();
    let mut kept = KeptScreen::new(options);
    assert_eq!(kept.update(&harness, &grid, options), ALL_ROWS, "the first call draws everything");
    assert_eq!(kept.frame, harness.screen(&grid, options));
    let revealed = RenderOptions { reveal: true, ..options };
    let steps: Vec<(&str, Change, RenderOptions)> = vec![
        ("a cell's colour", Box::new(|grid: &mut Grid| grid.cells[4][2].foreground = 5), options),
        ("a cell's glyph", Box::new(|grid: &mut Grid| grid.cells[20][0].glyph = Glyph::Space), options),
        // A concealed letter of "HIDDEN" made to flash, then reveal on and
        // off: in the palette mode its index range follows reveal (REQ-398).
        ("a concealed letter flashing", Box::new(|grid: &mut Grid| grid.cells[4][25].flash = true), options),
        ("reveal on", Box::new(|_: &mut Grid| {}), revealed),
        ("reveal off", Box::new(|_: &mut Grid| {}), options),
        // The top half of a double-height letter changed, and then the
        // whole row made normal height.
        ("a double-height letter", Box::new(|grid: &mut Grid| grid.cells[1][3] = grid.cells[1][5]), options),
        (
            "double height ended",
            Box::new(|grid: &mut Grid| {
                for cell in &mut grid.cells[1] {
                    cell.size = Size::Normal;
                }
            }),
            options,
        ),
        ("nothing", Box::new(|_: &mut Grid| {}), options),
    ];
    for (name, change, options) in &steps {
        change(&mut grid);
        kept.update(&harness, &grid, *options);
        assert_eq!(kept.frame, harness.screen(&grid, *options), "palette mode after {name}");
    }
    // The resolved mode, on the Level 2.5 page, through the flash phases.
    let grid = level_25_page();
    let mut kept = KeptScreen::new(resolved_options(0));
    for unit in 0..12 {
        let options = resolved_options(unit);
        kept.update(&harness, &grid, options);
        assert_eq!(kept.frame, harness.screen(&grid, options), "resolved mode at flash unit {unit}");
    }
    let revealed = RenderOptions { reveal: true, ..resolved_options(0) };
    kept.update(&harness, &grid, revealed);
    assert_eq!(kept.frame, harness.screen(&grid, revealed), "resolved mode, reveal on");
}
