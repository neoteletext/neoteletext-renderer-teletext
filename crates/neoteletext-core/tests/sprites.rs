//! Teletext sprites through the enhancement processor and the renderer
//! (animation.md ANIM-4, ANIM-6, ANIM-8, ANIM-12 to ANIM-14 and ANIM-17;
//! REQ-6529): a sprite of each object type over text and mosaics, clipped
//! at each edge, overlapping in each order, cycling its frames; the looks
//! of controls; bounding rectangles against the cells written; and the
//! damage property, that drawing only the rectangle reported gives, dot
//! for dot, the whole rendering.
//!
//! `tests/golden/sprites.txt` holds a checksum of each scene's raster and
//! carries a format version: a deliberate change to what a sprite draws
//! raises it and regenerates the file in the same change, with
//! `NEOTELETEXT_UPDATE_GOLDEN=1` (DEVELOPMENT.md section 8).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

mod common;

use common::{
    DISPLAY, ObjectDef, Receiver, background, define, display_page, end, foreground, g0, invoke, link, local_number,
    null_link, object_page, public_number, set_position, text, text_at, x26, x27,
};
use neoteletext_core::acq::Decoder;
use neoteletext_core::colour::colour_index;
use neoteletext_core::compose::{SpritesDrawn, compose_level, compose_sprites};
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{
    COLUMNS, CONTROL_JOINED, LINK_FUNCTION_GPOP, LINK_FUNCTION_POP, MODE_CONTROL, MODE_DRCS_CHARACTER, PAGE_HEIGHT,
    PAGE_WIDTH, ROWS,
};
use neoteletext_core::controls::{self, Controls};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{Cell, DisplayOptions, Glyph, Grid, build_grid};
use neoteletext_core::packet::{ObjectKind, ObjectSource, Packet, TripletFunction};
use neoteletext_core::render::{
    ALL_ROWS, DamageTracker, FlashPhase, IndexBuffer, IndexFormat, RenderOptions, render_rect, render_screen,
};
use neoteletext_core::sprites::{
    Animation, Applied, ForcedLook, FrameRef, Operation, Placements, Sprite, SpriteCells, Sprites, Target,
};
use neoteletext_core::types::{CellRect, PageAddress, SubCode};
use std::fmt::Write as _;
use std::path::PathBuf;

/// Raise when what a sprite draws changes on purpose.
const FORMAT_VERSION: u32 = 1;

const L25: PresentationLevel = PresentationLevel::Level25;
const POP: u16 = 0x1A0;
const GPOP: u16 = 0x1A1;
const RED: u8 = colour_index(0, 1);
const GREEN: u8 = colour_index(0, 2);
const YELLOW: u8 = colour_index(0, 3);
const BLUE: u8 = colour_index(0, 4);
const WHITE: u8 = colour_index(0, 7);
const MOSAIC_CYAN: u8 = 0x16;
const FLASH: u8 = 0x08;
const CONCEAL: u8 = 0x18;

/// The local objects of the scene, by the designation code of the packet
/// X/26 each starts: the frames of the tests' sprites.
const BLOCK: u8 = 1;
/// The block is longer than a packet's thirteen triplets and runs on into
/// the next, so the object after it starts two packets on.
const ACTIVE: u8 = 3;
const ADAPTIVE: u8 = 4;
const LETTER_A: u8 = 5;
const LETTER_B: u8 = 6;
const LETTER_C: u8 = 7;
const WITH_DRCS: u8 = 8;
const BIG: u8 = 9;

fn local(kind: ObjectKind, designation: u8) -> FrameRef {
    FrameRef { source: ObjectSource::Local, kind, number: local_number(designation, 0) }
}

/// A block of three rows of four letters, each row in its own colours.
fn block_body(letters: [&[u8; 4]; 3]) -> Vec<TripletFunction> {
    let mut body = Vec::new();
    for (row, (letters, colour)) in letters.into_iter().zip([RED, GREEN, YELLOW]).enumerate() {
        if row > 0 {
            body.push(set_position(row as u8, 0));
        }
        body.push(foreground(0, colour));
        body.push(background(0, BLUE));
        body.extend(text_at(0, letters));
    }
    body
}

/// An object of one row: red letters from column 0, as the example of
/// EN 300 706 clause 13.9 colours its words.
fn word_body() -> Vec<TripletFunction> {
    let mut body = vec![foreground(0, RED)];
    body.extend(text_at(1, b"ox"));
    body
}

fn letter_body(letter: u8) -> Vec<TripletFunction> {
    vec![foreground(0, YELLOW), background(0, RED), g0(0, letter), g0(1, letter)]
}

/// The scene: page 100 with text on rows 1 to 23, mosaics on row 9, a
/// flashing and a concealed word, two controls, and the local objects.
/// `local_data` is the page's own enhancement data before them.
fn scene(local_data: &[TripletFunction]) -> Vec<Packet> {
    let mut rows: Vec<(u8, Vec<u8>)> = (1..=23u8)
        .map(|number| (number, format!(" ROW {number:02} the quick brown fox jumps over it").into_bytes()))
        .collect();
    rows[8].1 = core::iter::once(MOSAIC_CYAN).chain((0..39).map(|column| 0x20 | (column % 0x20) | 0x40)).collect();
    rows[11].1 = [&[FLASH][..], b"flashing ", &[CONCEAL], b"concealed"].concat();
    let borrowed: Vec<(u8, &[u8])> =
        rows.iter().map(|(number, codes)| (*number, &codes[..COLUMNS.min(codes.len())])).collect();

    let links = x27(
        4,
        [
            link(LINK_FUNCTION_GPOP, 0b11, GPOP),
            link(LINK_FUNCTION_POP, 0b11, POP),
            null_link(0b10),
            null_link(0b11),
            null_link(0),
            null_link(0),
        ],
    );
    let mut data = local_data.to_vec();
    // Two controls: a button on row 20 and a list of two rows on rows 21 and 22.
    data.push(set_position(20, 0));
    data.push(TripletFunction::ReservedColumn { column: 2, mode: MODE_CONTROL, data: 6 });
    data.push(set_position(21, 0));
    data.push(TripletFunction::ReservedColumn { column: 10, mode: MODE_CONTROL, data: 5 });
    data.push(set_position(22, 0));
    data.push(TripletFunction::ReservedColumn { column: 10, mode: MODE_CONTROL, data: 5 | CONTROL_JOINED });
    data.push(end());
    let mut others = vec![links];
    for chunk in data.chunks(13) {
        others.push(x26(others.len() as u8 - 1, chunk));
    }
    assert!(others.len() <= 2, "the page's own data fits packet X/26/0, so the objects start at X/26/1");
    let mut big = vec![TripletFunction::DisplayAttributes {
        column: 0,
        attributes: neoteletext_core::packet::DisplayAttributes {
            double_height: true,
            boxing: false,
            conceal: false,
            invert: false,
            underline: false,
            double_width: true,
        },
    }];
    big.push(g0(0, b'Z'));
    let objects: [(u8, ObjectKind, Vec<TripletFunction>); 8] = [
        (BLOCK, ObjectKind::Passive, block_body([b"abcd", b"efgh", b"ijkl"])),
        (ACTIVE, ObjectKind::Active, word_body()),
        (ADAPTIVE, ObjectKind::Adaptive, word_body()),
        (LETTER_A, ObjectKind::Passive, letter_body(b'A')),
        (LETTER_B, ObjectKind::Passive, letter_body(b'B')),
        (LETTER_C, ObjectKind::Passive, letter_body(b'C')),
        (
            WITH_DRCS,
            ObjectKind::Passive,
            vec![
                g0(0, b'<'),
                TripletFunction::UnsupportedColumn { column: 1, mode: MODE_DRCS_CHARACTER, data: 3 },
                g0(2, b'>'),
            ],
        ),
        (BIG, ObjectKind::Passive, big),
    ];
    for (designation, kind, body) in objects {
        let mut all = vec![define(kind, 0b11, local_number(designation, 0))];
        all.extend(body);
        all.push(TripletFunction::Termination { kind: 0b101 });
        assert_eq!(others.len(), usize::from(designation) + 1, "each object starts the packet its number names");
        for chunk in all.chunks(13) {
            others.push(x26(others.len() as u8 - 1, chunk));
        }
    }
    let mut packets = display_page(0, &borrowed, others);
    packets.extend(object_page(POP, 0, &[ObjectDef::new(ObjectKind::Passive, 0, letter_body(b'P'))]));
    packets.extend(object_page(GPOP, 0, &[ObjectDef::new(ObjectKind::Passive, 0, letter_body(b'G'))]));
    packets
}

/// A page with its own grid kept apart, as a receiver holds it.
struct Shown<'d, 's> {
    decoder: &'d Decoder<'s>,
    config: Config,
    page_grid: Box<Grid>,
}

impl<'d, 's> Shown<'d, 's> {
    fn new(decoder: &'d Decoder<'s>, config: Config) -> Self {
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let page = decoder.store().get(address, SubCode::ANY).unwrap();
        let magazine = decoder.magazine_data(address.magazine);
        let options = DisplayOptions::default();
        let mut page_grid = Box::new(Grid::EMPTY);
        build_grid(&mut page_grid, Some(page), &magazine, &config, &options);
        let _ = compose_level(&mut page_grid, page, &magazine, &decoder.enhancement_lookup(), &config, &options);
        Self { decoder, config, page_grid }
    }

    fn compose(&self, sprites: &Sprites, animation: &Animation) -> (Box<Grid>, SpritesDrawn) {
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let page = self.decoder.store().get(address, SubCode::ANY).unwrap();
        let magazine = self.decoder.magazine_data(address.magazine);
        let mut grid = Box::new(Grid::EMPTY);
        let drawn = compose_sprites(
            &mut grid,
            &self.page_grid,
            page,
            &magazine,
            &self.decoder.enhancement_lookup(),
            &self.config,
            &DisplayOptions::default(),
            &Placements::of(sprites, animation),
        );
        (grid, drawn)
    }
}

fn with_scene<R>(local_data: &[TripletFunction], level: PresentationLevel, f: impl FnOnce(&Shown<'_, '_>) -> R) -> R {
    let config = Config { level, ..Config::default() };
    Receiver::new().run(&scene(local_data), config, |decoder| f(&Shown::new(decoder, config)))
}

fn sprite(number: u16, at: (i8, i8), size: (u8, u8), frame: FrameRef) -> Sprite {
    Sprite::new(number, at, size, frame)
}

fn block(number: u16, at: (i8, i8)) -> Sprite {
    sprite(number, at, (3, 4), local(ObjectKind::Passive, BLOCK))
}

fn one(sprite: Sprite) -> Sprites {
    Sprites::new(&[sprite]).unwrap()
}

/// Asserts that every cell outside `windows` is the page's own, that every
/// cell marked as a sprite's lies inside them and differs from the page's,
/// and that no other cell differs.
fn assert_only_inside(page: &Grid, grid: &Grid, drawn: &SpritesDrawn, windows: &[CellRect], label: &str) {
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let inside = windows.iter().any(|window| window.contains(row, column));
            let differs = grid.cells[row][column] != page.cells[row][column];
            assert!(inside || !differs, "{label}: the cell at row {row}, column {column} is outside every sprite");
            assert_eq!(drawn.cells.contains(row, column), differs, "{label}: row {row}, column {column}");
        }
    }
    assert_eq!(
        (grid.screen_colour, grid.row_colours, grid.colour_map),
        (page.screen_colour, page.row_colours, page.colour_map)
    );
}

fn cells(row: u8, column: u8, rows: u8, columns: u8) -> CellRect {
    CellRect { row, column, rows, columns }
}

/// ANIM-8 (REQ-6529): a sprite of each object type, from each source, is
/// drawn as an Object Invocation triplet at its origin draws the object:
/// inside the sprite, the grid is the grid of the page with that
/// invocation added to its own data; outside it, the page's own.
#[test]
fn a_sprite_of_each_type_is_drawn_as_an_invocation_at_its_origin() {
    let frames = [
        local(ObjectKind::Passive, BLOCK),
        local(ObjectKind::Active, ACTIVE),
        local(ObjectKind::Adaptive, ADAPTIVE),
        FrameRef { source: ObjectSource::Public, kind: ObjectKind::Passive, number: public_number(0, 0) },
        FrameRef { source: ObjectSource::Global, kind: ObjectKind::Passive, number: public_number(0, 0) },
    ];
    // Over text on row 5, and over the mosaics of row 9.
    for (row, column) in [(5i8, 10i8), (9, 20)] {
        for frame in frames {
            let label = format!("{frame:?} at row {row}");
            let (top, left) = (row.unsigned_abs(), column.unsigned_abs());
            let invocation = [set_position(top, left), invoke(frame.source, frame.kind, frame.number)];
            let invoked = with_scene(&invocation, L25, |shown| shown.page_grid.clone());
            with_scene(&[], L25, |shown| {
                let sprites = one(sprite(1, (row, column), (3, 4), frame));
                let (grid, drawn) = shown.compose(&sprites, &Animation::NONE);
                let window = cells(top, left, 3, 4);
                assert_only_inside(&shown.page_grid, &grid, &drawn, &[window], &label);
                assert_eq!(drawn.covered, Some(window));
                assert!(!drawn.cells.is_empty(), "{label}: the sprite draws something");
                for r in top..top + 3 {
                    for c in left..left + 4 {
                        let (r, c) = (usize::from(r), usize::from(c));
                        assert_eq!(grid.cells[r][c], invoked.cells[r][c], "{label}: row {r}, column {c}");
                    }
                }
                assert_eq!(drawn.report.unwrap().objects, 1, "{label}");
            });
        }
    }
}

/// ANIM-8 (REQ-6529): what the three types draw. A Passive object carries
/// its own colours; an Active object's foreground runs on along the row,
/// here to the sprite's edge and no further; an Adaptive object's applies
/// to the cells it addresses.
#[test]
fn the_three_object_types_draw_what_the_standard_says() {
    with_scene(&[], L25, |shown| {
        let (grid, _) = shown.compose(&one(block(1, (5, 10))), &Animation::NONE);
        assert_eq!(&text(&grid, 5)[..16], " ROW 05 thabcdic", "the scene's text, with the block from column 10");
        assert_eq!(&text(&grid, 6)[10..14], "efgh");
        assert_eq!(&text(&grid, 7)[10..14], "ijkl");
        let colours = |row: usize| (grid.cells[row][10].foreground, grid.cells[row][13].background);
        assert_eq!((colours(5), colours(6), colours(7)), ((RED, BLUE), (GREEN, BLUE), (YELLOW, BLUE)));

        let active = one(sprite(1, (5, 10), (1, 6), local(ObjectKind::Active, ACTIVE)));
        let (grid, _) = shown.compose(&active, &Animation::NONE);
        assert_eq!(&text(&grid, 5)[10..16], "eoxuic", "the object's two letters over the row's own");
        assert!((10..16).all(|column| grid.cells[5][column].foreground == RED), "red from column 0 of the sprite");
        assert_eq!(grid.cells[5][16].foreground, WHITE, "and the page's own past its edge");

        let adaptive = one(sprite(1, (5, 10), (1, 6), local(ObjectKind::Adaptive, ADAPTIVE)));
        let (grid, _) = shown.compose(&adaptive, &Animation::NONE);
        assert_eq!(&text(&grid, 5)[10..16], "eoxuic");
        assert!((10..13).all(|column| grid.cells[5][column].foreground == RED), "its range is columns 0 to 2");
        assert_eq!(grid.cells[5][13].foreground, WHITE);
    });
}

/// ANIM-8 (REQ-6529): cells the object addresses outside the sprite's
/// width and height, outside columns 0 to 39 or outside rows 1 to 23 are
/// not written.
#[test]
fn a_sprite_is_clipped_at_each_edge_and_to_its_own_size() {
    with_scene(&[], L25, |shown| {
        let draw = |sprite: Sprite, window: Option<CellRect>, label: &str| {
            let (grid, drawn) = shown.compose(&one(sprite), &Animation::NONE);
            assert_only_inside(&shown.page_grid, &grid, &drawn, window.as_slice(), label);
            assert_eq!(drawn.covered, window, "{label}");
            grid
        };
        let grid = draw(block(1, (-1, 5)), Some(cells(1, 5, 1, 4)), "the top");
        assert_eq!(&text(&grid, 1)[5..9], "ijkl", "only the block's last row is on rows 1 to 23");
        let grid = draw(block(1, (0, 5)), Some(cells(1, 5, 2, 4)), "row 0");
        assert_eq!((&text(&grid, 1)[5..9], &text(&grid, 2)[5..9]), ("efgh", "ijkl"));
        let grid = draw(block(1, (22, 5)), Some(cells(22, 5, 2, 4)), "the bottom");
        assert_eq!((&text(&grid, 22)[5..9], &text(&grid, 23)[5..9]), ("abcd", "efgh"));
        assert_eq!(text(&grid, 24), text(&shown.page_grid, 24), "row 24 is never written");
        let grid = draw(block(1, (5, -2)), Some(cells(5, 0, 3, 2)), "the left");
        assert_eq!((&text(&grid, 5)[..2], &text(&grid, 7)[..2]), ("cd", "kl"));
        let grid = draw(block(1, (5, 38)), Some(cells(5, 38, 3, 2)), "the right");
        assert_eq!((&text(&grid, 5)[38..], &text(&grid, 7)[38..]), ("ab", "ij"));
        let _ = draw(block(1, (-3, 5)), None, "above the page");
        let _ = draw(block(1, (24, 5)), None, "on row 24");
        let _ = draw(block(1, (5, -4)), None, "left of the page");
        // A sprite smaller than its object shows the part inside it.
        let small = sprite(1, (5, 10), (2, 3), local(ObjectKind::Passive, BLOCK));
        let grid = draw(small, Some(cells(5, 10, 2, 3)), "its own size");
        assert_eq!((&text(&grid, 5)[10..14], &text(&grid, 6)[10..14]), ("abcu", "efgu"));
        assert_eq!(text(&grid, 7), text(&shown.page_grid, 7));
        // An enlarged character does not reach past the sprite either.
        let big = sprite(1, (5, 10), (1, 1), local(ObjectKind::Passive, BIG));
        let _ = draw(big, Some(cells(5, 10, 1, 1)), "a double-size character in a sprite of one cell");
    });
}

/// ANIM-8, ANIM-12 (REQ-6529): sprites are drawn in order of layer, then
/// order, then number, so of two that overlap the later is on top; the
/// `order` operation changes it.
#[test]
fn two_overlapping_sprites_in_each_order() {
    with_scene(&[], L25, |shown| {
        let letter = |number, column, designation, layer, order| Sprite {
            layer,
            order,
            ..sprite(number, (5, column), (1, 2), local(ObjectKind::Passive, designation))
        };
        let top = |sprites: &[Sprite], animation: &Animation| {
            let (grid, _) = shown.compose(&Sprites::new(sprites).unwrap(), animation);
            text(&grid, 5)[10..13].to_owned()
        };
        let none = Animation::NONE;
        assert_eq!(top(&[letter(1, 10, LETTER_A, 2, 0), letter(2, 11, LETTER_B, 2, 0)], &none), "ABB", "by number");
        assert_eq!(top(&[letter(2, 10, LETTER_A, 2, 0), letter(1, 11, LETTER_B, 2, 0)], &none), "AAB");
        assert_eq!(top(&[letter(1, 10, LETTER_A, 2, 1), letter(2, 11, LETTER_B, 2, 0)], &none), "AAB", "by order");
        assert_eq!(top(&[letter(1, 10, LETTER_A, 3, -128), letter(2, 11, LETTER_B, 2, 127)], &none), "AAB", "by layer");
        let pair = [letter(1, 10, LETTER_A, 2, 0), letter(2, 11, LETTER_B, 2, 0)];
        let sprites = Sprites::new(&pair).unwrap();
        let mut animation = Animation::NONE;
        let raise = Operation::order(255, 5).unwrap();
        assert_eq!(animation.apply(Target::Object(1), raise, 0, &sprites, 0), Applied::Done);
        assert_eq!(top(&pair, &animation), "AAB", "the order operation puts sprite 1 on top");
        let lower = Operation::order(1, 5).unwrap();
        assert_eq!(animation.apply(Target::Object(1), lower, 0, &sprites, 0), Applied::Done);
        assert_eq!(top(&pair, &animation), "ABB", "and a lower layer beneath again");
    });
}

/// ANIM-6, ANIM-8 (REQ-6529): the animation state moves, hides and shows a
/// sprite with nothing of the page sent again, and the cells it left are
/// the page's own.
#[test]
fn the_animation_state_moves_hides_and_shows_a_sprite() {
    with_scene(&[], L25, |shown| {
        let sprites = one(block(7, (5, 10)));
        let mut animation = Animation::NONE;
        let mut apply = |operation| {
            assert_eq!(animation.apply(Target::Object(7), operation, 0, &sprites, 0), Applied::Done);
            let (grid, drawn) = shown.compose(&sprites, &animation);
            (grid, drawn, Placements::of(&sprites, &animation))
        };
        let (grid, drawn, placements) = apply(Operation::Move { relative: false, x: 20, y: 12 });
        assert_only_inside(&shown.page_grid, &grid, &drawn, &[cells(12, 20, 3, 4)], "moved");
        assert_eq!(&text(&grid, 12)[20..24], "abcd");
        assert_eq!(placements.bounds(7).unwrap(), cells(12, 20, 3, 4).dots());
        let (grid, drawn, _) = apply(Operation::Move { relative: true, x: -1, y: 1 });
        assert_only_inside(&shown.page_grid, &grid, &drawn, &[cells(13, 19, 3, 4)], "moved by one cell");
        let (grid, drawn, placements) = apply(Operation::Hide);
        assert_eq!(*grid, *shown.page_grid, "hidden");
        assert_eq!((drawn, placements.bounds(7)), (SpritesDrawn::NONE, None));
        let (grid, _, _) = apply(Operation::Show);
        assert_eq!(&text(&grid, 13)[19..23], "abcd");
        let (grid, _, _) = apply(Operation::Reset);
        assert_eq!(&text(&grid, 5)[10..14], "abcd", "reset: the sprite as the record gave it");
    });
}

/// ANIM-14 (REQ-6529): a sprite with several frames shows the frame in
/// force; a cycle changes it by the tick; a frame beyond its frames shows
/// the last. Its frames are objects of characters, and a DRCS character an
/// object invokes is not drawn (REQ-2196).
#[test]
fn a_sprite_cycles_its_frames_and_never_draws_drcs() {
    with_scene(&[], L25, |shown| {
        let mut three = sprite(1, (5, 10), (1, 2), local(ObjectKind::Passive, LETTER_A));
        assert!(three.frames.push(local(ObjectKind::Passive, LETTER_B)));
        assert!(three.frames.push(local(ObjectKind::Passive, LETTER_C)));
        let sprites = one(three);
        let mut animation = Animation::NONE;
        let cycle = Operation::Cycle { first: 0, last: 2, period: 3, once: false };
        assert_eq!(animation.apply(Target::Object(1), cycle, 0, &sprites, 0), Applied::Done);
        let mut seen = String::new();
        for tick in 1..=12 {
            let _ = animation.advance(tick);
            let (grid, _) = shown.compose(&sprites, &animation);
            seen.push_str(&text(&grid, 5)[10..11]);
        }
        assert_eq!(seen, "AAABBBCCCAAA", "one frame every three ticks from tick 1");
        assert_eq!(animation.apply(Target::Object(1), Operation::Frame(15), 12, &sprites, 0), Applied::Done);
        let (grid, _) = shown.compose(&sprites, &animation);
        assert_eq!(&text(&grid, 5)[10..12], "CC", "a frame beyond its frames shows its last");

        let drcs = one(sprite(2, (5, 12), (1, 3), local(ObjectKind::Passive, WITH_DRCS)));
        let (grid, drawn) = shown.compose(&drcs, &Animation::NONE);
        assert_eq!(&text(&grid, 5)[12..15], "<u>", "the DRCS character's cell is left as the page has it");
        assert_eq!(grid.cells[5][13], shown.page_grid.cells[5][13]);
        assert!(matches!(grid.cells[5][13].glyph, Glyph::Char(_)));
        assert_eq!(drawn.cells.count(), 2);
    });
}

/// ANIM-8 (REQ-6529): below Level 2.5 sprites are not drawn; an object
/// that is absent draws nothing, and is counted in the report.
#[test]
fn no_sprite_below_level_25_and_none_of_an_absent_object() {
    for level in [PresentationLevel::Level1, PresentationLevel::Level15] {
        with_scene(&[], level, |shown| {
            let (grid, drawn) = shown.compose(&one(block(1, (5, 10))), &Animation::NONE);
            assert_eq!((*grid == *shown.page_grid, drawn), (true, SpritesDrawn::NONE), "{level:?}");
        });
    }
    with_scene(&[], PresentationLevel::Level35, |shown| {
        let (grid, _) = shown.compose(&one(block(1, (5, 10))), &Animation::NONE);
        assert_eq!(&text(&grid, 5)[10..14], "abcd", "Level 3.5");
    });
    with_scene(&[], L25, |shown| {
        let absent = [
            local(ObjectKind::Passive, 12),
            local(ObjectKind::Active, BLOCK),
            FrameRef { source: ObjectSource::Public, kind: ObjectKind::Passive, number: public_number(3, 0) },
            FrameRef { source: ObjectSource::Global, kind: ObjectKind::Adaptive, number: public_number(0, 0) },
        ];
        for frame in absent {
            let (grid, drawn) = shown.compose(&one(sprite(1, (5, 10), (3, 4), frame)), &Animation::NONE);
            assert_eq!(*grid, *shown.page_grid, "{frame:?}");
            assert!(drawn.cells.is_empty());
            assert_eq!(drawn.report.unwrap().faults.absent_objects, 1, "{frame:?}");
        }
    });
}

/// ANIM-8, ANIM-16 (REQ-6529): 32 sprites of a sub-page are all drawn.
#[test]
fn thirty_two_sprites_are_drawn() {
    with_scene(&[], L25, |shown| {
        let list: Vec<Sprite> = (0..32u16)
            .map(|index| {
                let at = (1 + (index / 16) as i8 * 3, (index % 16) as i8 * 2);
                sprite(index + 1, at, (1, 2), local(ObjectKind::Passive, LETTER_A))
            })
            .collect();
        let (grid, drawn) = shown.compose(&Sprites::new(&list).unwrap(), &Animation::NONE);
        assert_eq!((&text(&grid, 1)[..32], &text(&grid, 4)[..32]), (&"A".repeat(32)[..], &"A".repeat(32)[..]));
        assert_eq!(drawn.cells.count(), 64);
        assert_eq!(drawn.report.unwrap().objects, 32);
    });
}

/// ANIM-13 (REQ-6529): every cell a sprite changed lies inside its bounding
/// rectangle, wherever it is put, and so does every dot it changed.
#[test]
fn every_cell_written_lies_inside_the_bounding_rectangle() {
    with_scene(&[], L25, |shown| {
        let sprites = one(block(1, (0, 0)));
        let glyphs = GlyphSet::noto_teletext();
        let options = RenderOptions::resolved(&shown.config, false, FlashPhase::STEADY);
        let own = raster(&shown.page_grid, &glyphs, options);
        for row in (-4i16..=26).step_by(3) {
            for column in (-5i16..=41).step_by(5) {
                let mut animation = Animation::NONE;
                let moved = Operation::Move { relative: false, x: column, y: row };
                assert_eq!(animation.apply(Target::Object(1), moved, 0, &sprites, 0), Applied::Done);
                let (grid, drawn) = shown.compose(&sprites, &animation);
                let bounds = Placements::of(&sprites, &animation).bounds(1);
                let pixels = raster(&grid, &glyphs, options);
                for (index, (now, before)) in pixels.iter().zip(&own).enumerate() {
                    let (x, y) = ((index % PAGE_WIDTH) as u16, (index / PAGE_WIDTH) as u16);
                    let inside = bounds.is_some_and(|bounds| bounds.contains(x, y));
                    assert!(now == before || inside, "at {row}, {column}: the dot at {x}, {y} is outside {bounds:?}");
                }
                assert!(bounds.is_some() || drawn.cells.is_empty());
            }
        }
    });
}

/// ANIM-6 (REQ-6529): a hidden control is drawn as spaces on its row's
/// background and takes no pointer event; a forced look draws the hover
/// look for 1 and the pressed look for 2 and 3; a sprite over a control is
/// left alone.
#[test]
fn hidden_controls_and_forced_looks() {
    with_scene(&[], L25, |shown| {
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let page = shown.decoder.store().get(address, SubCode::ANY).unwrap();
        let found = controls::controls(page);
        let list: Vec<_> = found.list.as_slice().iter().map(|c| (c.row, c.column, c.width, c.height)).collect();
        assert_eq!(list, [(20, 2, 6, 1), (21, 10, 5, 2)]);
        assert_eq!(found.numbered(2).unwrap().bounds(), cells(21, 10, 2, 5).dots());
        assert_eq!((found.numbered(0), found.numbered(3)), (None, None));

        let none = Sprites::NONE;
        let mut animation = Animation::NONE;
        let with = |animation: &Animation, sprites: &SpriteCells| {
            let mut grid = shown.page_grid.clone();
            controls::apply_animation(&mut grid, &found, animation, sprites);
            grid
        };
        assert_eq!(*with(&animation, &SpriteCells::NONE), *shown.page_grid, "no state changes nothing");

        assert_eq!(animation.apply(Target::Control(1), Operation::Hide, 0, &none, 2), Applied::Done);
        let grid = with(&animation, &SpriteCells::NONE);
        let hidden = Cell { background: shown.page_grid.row_colours[20], ..Cell::BLANK };
        assert!((2..8).all(|column| grid.cells[20][column] == hidden), "spaces on the row's background");
        assert_eq!(
            (grid.cells[20][1], grid.cells[20][8]),
            (shown.page_grid.cells[20][1], shown.page_grid.cells[20][8])
        );
        assert_eq!(found.shown_at(20, 3, &animation), None, "a hidden control takes no pointer event");
        assert_eq!(found.shown_at(21, 10, &animation), Some(2));
        assert_eq!(found.shown_at(19, 3, &animation), None);
        assert_eq!(
            (found.bounds(1, &animation), found.bounds(2, &animation)),
            (None, Some(cells(21, 10, 2, 5).dots()))
        );

        for (look, expected) in [
            (ForcedLook::Hover, controls::Look::Hover),
            (ForcedLook::Pressed, controls::Look::Pressed),
            (ForcedLook::Alternative, controls::Look::Pressed),
        ] {
            assert_eq!(animation.apply(Target::Control(2), Operation::Look(look), 0, &none, 2), Applied::Done);
            let mut by_hand = shown.page_grid.clone();
            controls::apply_look(&mut by_hand, found.numbered(2).unwrap(), expected);
            let grid = with(&animation, &SpriteCells::NONE);
            assert_eq!(grid.cells[21..23], by_hand.cells[21..23], "{look:?}");
            assert_ne!(grid.cells[21][10], shown.page_grid.cells[21][10]);
        }
        assert_eq!(animation.apply(Target::Control(2), Operation::Look(ForcedLook::None), 0, &none, 2), Applied::Done);
        assert_eq!(
            with(&animation, &SpriteCells::NONE).cells[21],
            shown.page_grid.cells[21],
            "look 0: the pointer decides"
        );

        // A sprite over the hidden control and the pressed one stays as drawn.
        assert_eq!(
            animation.apply(Target::Control(2), Operation::Look(ForcedLook::Pressed), 0, &none, 2),
            Applied::Done
        );
        let over = one(block(1, (20, 6)));
        let (mut grid, drawn) = shown.compose(&over, &animation);
        let composed = grid.clone();
        controls::apply_animation(&mut grid, &found, &animation, &drawn.cells);
        for row in 20..23 {
            assert_eq!(grid.cells[row][6..10], composed.cells[row][6..10], "row {row}: the sprite's cells");
        }
        assert_eq!(grid.cells[20][5], hidden);
        assert_ne!(grid.cells[21][10], composed.cells[21][10], "the control beside the sprite has its look");
        assert_eq!(found, Controls { list: found.list, dropped: 0 });
    });
}

fn raster(grid: &Grid, glyphs: &GlyphSet, options: RenderOptions) -> Vec<u8> {
    let mut pixels = vec![0u8; PAGE_WIDTH * PAGE_HEIGHT];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
    render_screen(grid, ALL_ROWS, glyphs, options, &mut buffer).unwrap();
    pixels
}

/// A small deterministic generator (a linear congruential one), so that
/// the property test below is the same on every run and every machine.
struct Random(u64);

impl Random {
    fn next(&mut self, below: u32) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) as u32) % below
    }
}

/// ANIM-4 (REQ-6529): for random sequences of changes — sprites moved,
/// shown, hidden, reordered and cycled, controls hidden and given looks,
/// the flash phase and reveal — drawing only the rectangle the tracker
/// reports into the picture as last drawn gives, dot for dot, the whole
/// rendering; so no dot outside the reported rectangle differs.
#[test]
fn damaged_rendering_equals_whole_rendering() {
    with_scene(&[], L25, |shown| {
        let glyphs = GlyphSet::noto_teletext();
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let found = controls::controls(shown.decoder.store().get(address, SubCode::ANY).unwrap());
        let mut three = sprite(3, (15, 30), (1, 2), local(ObjectKind::Passive, LETTER_A));
        assert!(three.frames.push(local(ObjectKind::Passive, LETTER_B)));
        assert!(three.frames.push(local(ObjectKind::Passive, LETTER_C)));
        let sprites = Sprites::new(&[
            block(1, (5, 10)),
            sprite(2, (9, 3), (1, 6), local(ObjectKind::Active, ACTIVE)),
            three,
            sprite(4, (11, 2), (1, 6), local(ObjectKind::Adaptive, ADAPTIVE)),
            sprite(5, (3, 3), (2, 2), local(ObjectKind::Passive, BIG)),
        ])
        .unwrap();
        let mut random = Random(0x5EED);
        let mut animation = Animation::NONE;
        let mut tracker = DamageTracker::new();
        let mut pixels = vec![0xEEu8; PAGE_WIDTH * PAGE_HEIGHT];
        let (mut tick, mut reveal) = (0u32, false);
        let mut damaged_cells = 0u32;
        for step in 0..400 {
            let target = if random.next(5) == 0 {
                Target::Control(1 + random.next(2) as u16)
            } else {
                Target::Object(1 + random.next(5) as u16)
            };
            let operation = match random.next(10) {
                0 => Operation::Hide,
                1 => Operation::Show,
                2 => Operation::Look(ForcedLook::from_code(random.next(4) as u8).unwrap()),
                3 | 4 => Operation::Move { relative: true, x: random.next(5) as i16 - 2, y: random.next(5) as i16 - 2 },
                5 => Operation::Move { relative: false, x: random.next(50) as i16 - 5, y: random.next(30) as i16 - 3 },
                6 => Operation::order(random.next(4) as u8, random.next(7) as i8 - 3).unwrap(),
                7 => Operation::Frame(random.next(4) as u8),
                8 => {
                    Operation::Cycle { first: 0, last: 2, period: 1 + random.next(3) as u16, once: random.next(2) == 0 }
                }
                _ => Operation::Reset,
            };
            let _ = animation.apply(target, operation, tick, &sprites, found.list.len());
            tick += random.next(7);
            reveal ^= random.next(9) == 0;
            let _ = animation.advance(tick);

            let (mut grid, drawn) = shown.compose(&sprites, &animation);
            controls::apply_animation(&mut grid, &found, &animation, &drawn.cells);
            let options = RenderOptions::resolved(&shown.config, reveal, FlashPhase::at_tick(tick, &shown.config));
            let damage = tracker.damage(&grid, options, drawn.covered);
            if let Some(rect) = damage {
                let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
                render_rect(&grid, rect, &glyphs, options, &mut buffer).unwrap();
                if step > 0 {
                    damaged_cells += rect.area();
                }
            }
            assert!(
                pixels == raster(&grid, &glyphs, options),
                "step {step}: {operation:?} on {target:?}, damage {damage:?}"
            );
        }
        assert!(damaged_cells < 399 * 1000 / 2, "the rectangles are smaller than the page: {damaged_cells} cells");
    });
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3))
}

/// ANIM-8 (REQ-6529): the golden rasters. A sprite of each object type
/// over text and over mosaics; clipped at each edge; two overlapping in
/// each order.
#[test]
fn sprite_golden_rasters_match() {
    let actual = with_scene(&[], L25, |shown| {
        let glyphs = GlyphSet::noto_teletext();
        let options = RenderOptions::resolved(&shown.config, false, FlashPhase::STEADY);
        let mut out = format!("format {FORMAT_VERSION}\n");
        let mut scene = |name: &str, sprites: &[Sprite]| {
            let (grid, _) = shown.compose(&Sprites::new(sprites).unwrap(), &Animation::NONE);
            writeln!(out, "{name} {:016x}", fnv(&raster(&grid, &glyphs, options))).unwrap();
        };
        scene("the page's own", &[]);
        for (name, kind, designation) in [
            ("passive", ObjectKind::Passive, BLOCK),
            ("active", ObjectKind::Active, ACTIVE),
            ("adaptive", ObjectKind::Adaptive, ADAPTIVE),
        ] {
            scene(&format!("{name} over text"), &[sprite(1, (5, 10), (3, 6), local(kind, designation))]);
            scene(&format!("{name} over mosaics"), &[sprite(1, (9, 20), (3, 6), local(kind, designation))]);
        }
        for (name, at) in [("top", (-1, 5)), ("bottom", (22, 5)), ("left", (5, -2)), ("right", (5, 38))] {
            scene(&format!("clipped at the {name}"), &[block(1, at)]);
        }
        let a = sprite(1, (5, 10), (1, 2), local(ObjectKind::Passive, LETTER_A));
        let b = sprite(2, (5, 11), (1, 2), local(ObjectKind::Passive, LETTER_B));
        scene("two overlapping, the second on top", &[a, b]);
        scene("two overlapping, the first on top", &[Sprite { order: 1, ..a }, b]);
        out
    });
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/sprites.txt");
    if std::env::var_os("NEOTELETEXT_UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected =
        std::fs::read_to_string(&path).expect("tests/golden/sprites.txt; generate with NEOTELETEXT_UPDATE_GOLDEN=1");
    assert_eq!(actual, expected, "what a sprite draws changed: if deliberate, raise FORMAT_VERSION and regenerate");
}
