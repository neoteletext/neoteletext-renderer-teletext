//! The cost of one flashing cell and of one moved sprite, as a port pays
//! it (animation.md ANIM-4, REQ-6529): drawing the whole page at each
//! change, against drawing only the rectangle [`DamageTracker`] reports.
//! Run with `cargo test -p neoteletext-core --release --features objects
//! --test damage_cost -- --ignored --nocapture`.
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;

use common::{DISPLAY, Receiver, define, display_page, g0, local_number, x26};
use neoteletext_core::compose::{compose_level, compose_sprites};
use neoteletext_core::config::Config;
use neoteletext_core::consts::{COLUMNS, FLASH, PAGE_HEIGHT, PAGE_WIDTH, STEADY};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::packet::{ObjectKind, ObjectSource, TripletFunction};
use neoteletext_core::render::{
    ALL_ROWS, DamageTracker, FlashPhase, IndexBuffer, IndexFormat, RenderOptions, render_rect, render_screen,
};
use neoteletext_core::sprites::{Animation, FrameRef, Operation, Placements, Sprite, Sprites, Target};
use neoteletext_core::types::{PageAddress, SubCode};
use std::time::Instant as Clock;

/// Changes timed in each batch, and batches: the fastest batch is
/// reported, as the one least disturbed by the rest of the computer.
const ROUNDS: u32 = 500;
const BATCHES: u32 = 15;
/// Display rows 1 to 23 hold text; row 24 is Fastext.
const LAST_TEXT_ROW: u8 = 23;
/// The row whose first word flashes.
const FLASHING_ROW: u8 = 12;
/// Ticks on either side of the Level 1 flash's change (ANIM-1: shown in
/// ticks 0 to 56 of each 76).
const SHOWN_TICK: u32 = 0;
const HIDDEN_TICK: u32 = 57;

/// Microseconds per change of `step`: the fastest of `BATCHES` batches of
/// `ROUNDS`; and the cells drawn in the last change.
fn time(mut step: impl FnMut(u32) -> u32) -> (f64, u32) {
    let mut cells = 0;
    let mut best = f64::MAX;
    for _ in 0..BATCHES {
        let start = Clock::now();
        for round in 0..ROUNDS {
            cells = step(round);
        }
        best = best.min(start.elapsed().as_secs_f64() * 1e6 / f64::from(ROUNDS));
    }
    (best, cells)
}

#[test]
#[ignore = "measurement; run in release mode"]
fn one_flashing_cell_and_one_moved_sprite_against_a_whole_page() {
    let config = Config::default();
    let glyphs = GlyphSet::noto_teletext();
    let rows: Vec<(u8, Vec<u8>)> = (1..=LAST_TEXT_ROW)
        .map(|number| {
            // One cell flashes: the code, the letter, then steady again.
            let mut codes = if number == FLASHING_ROW { vec![FLASH, b'X', STEADY] } else { vec![STEADY] };
            codes.extend(format!("ROW {number:02} THE QUICK BROWN FOX JUMPS OVER").bytes());
            codes.truncate(COLUMNS);
            (number, codes)
        })
        .collect();
    let borrowed: Vec<(u8, &[u8])> = rows.iter().map(|(number, codes)| (*number, codes.as_slice())).collect();
    let object = [
        define(ObjectKind::Passive, 0b11, local_number(1, 0)),
        g0(0, b'<'),
        g0(1, b'>'),
        TripletFunction::Termination { kind: 0b101 },
    ];
    let end = [TripletFunction::Termination { kind: 0b111 }];
    let packets = display_page(0, &borrowed, vec![x26(0, &end), x26(1, &object)]);
    Receiver::new().run(&packets, config, |decoder| {
        let address = PageAddress::from_mpp(DISPLAY).unwrap();
        let page = decoder.store().get(address, SubCode::ANY).unwrap();
        let magazine = decoder.magazine_data(address.magazine);
        let lookup = decoder.enhancement_lookup();
        let display = DisplayOptions::default();
        let mut own = Grid::EMPTY;
        build_grid(&mut own, Some(page), &magazine, &config, &display);
        let _ = compose_level(&mut own, page, &magazine, &lookup, &config, &display);
        let mut pixels = vec![0_u8; PAGE_WIDTH * PAGE_HEIGHT];
        let options = |tick| RenderOptions::resolved(&config, false, FlashPhase::at_tick(tick, &config));
        let phase = |round: u32| if round.is_multiple_of(2) { HIDDEN_TICK } else { SHOWN_TICK };

        // One flashing cell.
        let (whole, _) = time(|round| {
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
            render_screen(&own, ALL_ROWS, &glyphs, options(phase(round)), &mut buffer).unwrap();
            1000
        });
        let mut tracker = DamageTracker::new();
        let _ = tracker.damage(&own, options(SHOWN_TICK), None);
        let (damaged, cells) = time(|round| {
            let options = options(phase(round));
            let damage = tracker.damage(&own, options, None).unwrap();
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
            render_rect(&own, damage, &glyphs, options, &mut buffer).unwrap();
            damage.area()
        });
        println!("one flashing cell: the whole page {whole:.1} us; the damage ({cells} cell) {damaged:.1} us");

        // One sprite of two cells, moved one cell a change.
        let frame = FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: local_number(1, 0) };
        let sprites = Sprites::new(&[Sprite::new(1, (5, 10), (1, 2), frame)]).unwrap();
        let mut animation = Animation::NONE;
        let mut grid = Grid::EMPTY;
        let steady = options(SHOWN_TICK);
        let mut step = |round: u32, grid: &mut Grid| {
            let by = if round.is_multiple_of(2) { 1 } else { -1 };
            let _ = animation.apply(Target::Object(1), Operation::Move { relative: true, x: by, y: 0 }, round, &sprites, 0);
            let placements = Placements::of(&sprites, &animation);
            compose_sprites(grid, &own, page, &magazine, &lookup, &config, &display, &placements).covered
        };
        let (model, _) = time(|round| {
            let _ = step(round, &mut grid);
            0
        });
        let (whole, _) = time(|round| {
            let _ = step(round, &mut grid);
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
            render_screen(&grid, ALL_ROWS, &glyphs, steady, &mut buffer).unwrap();
            1000
        });
        let mut tracker = DamageTracker::new();
        let covered = step(1, &mut grid);
        let _ = tracker.damage(&grid, steady, covered);
        let (damaged, cells) = time(|round| {
            let covered = step(round, &mut grid);
            let damage = tracker.damage(&grid, steady, covered).unwrap();
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
            render_rect(&grid, damage, &glyphs, steady, &mut buffer).unwrap();
            damage.area()
        });
        println!(
            "one moved sprite: composing the grid {model:.1} us; with the whole page drawn {whole:.1} us; with the damage ({cells} cells) drawn {damaged:.1} us"
        );
    });
}
