//! Teletext sprites from their records to the picture (animation.md
//! ANIM-4, ANIM-6 to ANIM-8, ANIM-23; REQ-6529, REQ-TST-040): a SPRITES
//! record and ANIMATE records, read by the readers a receiver uses, given
//! to the sprite set, the animation state and the enhancement processor
//! over a page, its objects and its object pages, all made from the
//! fuzzer's input.
//!
//! The input is a settings byte; a SPRITES record's body with its length
//! before it; then steps, each a count of ticks to pass and an ANIMATE
//! record's body with its length before it; a step of length `0xFFFF` ends
//! them; then the page's packets, as the target `enhancement` reads them.
//!
//! After every step it holds what `crates/neoteletext-core/tests/sprites.rs`
//! holds for chosen scenes: no cell outside the shown sprites' rectangles
//! differs from the page's own; rows 0 and 24 are never written; the cells
//! reported as a sprite's are exactly those that differ; and drawing only
//! the rectangle of damage reported gives, dot for dot, the whole
//! rendering.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementStore, EnhancementSubPage, StoredPage};
use neoteletext_core::compose::{compose_level, compose_sprites};
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{
    COLUMNS, DATA_BYTES, ENHANCEMENT_PACKETS_LEVEL_35, HEADER_DISPLAY_BYTES, MAGAZINES, MOT_PACKETS, PAGE_HEIGHT,
    PAGE_WIDTH, ROWS, TRIPLETS_PER_PACKET,
};
use neoteletext_core::controls;
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::limits::MAX_SPRITES;
use neoteletext_core::packet::{Packet, PacketMeta, Triplet, encode};
use neoteletext_core::render::{
    ALL_ROWS, DamageTracker, FlashPhase, IndexBuffer, IndexFormat, RenderOptions, render_rect, render_screen,
};
use neoteletext_core::sprites::{Animation, ForcedLook, FrameRef, Operation, Placements, Sprite, Sprites, Target};
use neoteletext_core::types::{ControlBits, Instant, Magazine, PageAddress, PageNumber, SubCode};
use neoteletext_rules::animate::{self, OperationKind};
use neoteletext_rules::table::TargetKind;
use neoteletext_web::records::{animate as animate_record, sprites as sprites_record};

/// Microseconds between packets: one television line.
const LINE_US: u64 = 64;
/// The pages a record can name, all in magazine 1: the displayed page, a
/// second display page, a POP and a GPOP, a second of each, the Magazine
/// Organization Table and page FF, which carries M/29.
const PAGES: [u8; 8] = [0x00, 0x01, 0xA0, 0xE0, 0xA1, 0xE1, 0xFE, 0xFF];
/// The displayed page.
const DISPLAYED: u16 = 0x100;
/// Small stores, so that pages are evicted and replaced.
const PAGE_SLOTS: usize = 16;
const SUB_PAGE_SLOTS: usize = 32;
/// Bytes of a triplet value taken from the input: 18 bits from three.
const TRIPLET_INPUT_BYTES: usize = 3;
const TRIPLET_VALUE_MASK: u32 = 0x3_FFFF;
/// The length that ends the steps.
const END_OF_STEPS: usize = 0xFFFF;
/// The most steps of one input, so that one input's time is bounded.
const MAX_STEPS: usize = 32;
/// The first and last rows a sprite may write (ANIM-8): rows 0 and 24 are
/// the receiver's.
const TOP_ROW: usize = 0;
const BOTTOM_ROW: usize = ROWS - 1;

/// Takes `n` bytes from the front of `data`.
fn take<'a>(data: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, tail) = data.split_at_checked(n)?;
    *data = tail;
    Some(head)
}

/// Takes a body with its two-byte length before it; `None` at the length
/// that ends the steps, or when the input ends.
fn take_body<'a>(data: &mut &'a [u8]) -> Option<&'a [u8]> {
    let [high, low] = *take(data, 2)? else { return None };
    let length = usize::from(u16::from_be_bytes([high, low]));
    if length == END_OF_STEPS {
        return None;
    }
    take(data, length)
}

fn packet_of(selector: u8, data: &mut &[u8]) -> Option<Packet> {
    let magazine = Magazine::new(1)?;
    match (selector >> 3) & 3 {
        0 => {
            let page = PageNumber(PAGES[usize::from(selector & 7)]);
            let [s1, control_low, control_high] = *take(data, 3)? else { return None };
            let subcode = SubCode::new(u16::from(s1 & 0xF));
            let control = ControlBits(u16::from_le_bytes([control_low, control_high]));
            encode::header(magazine, page, subcode, control, &[b' '; HEADER_DISPLAY_BYTES])
        }
        1 => {
            let [row] = *take(data, 1)? else { return None };
            let bytes: [u8; DATA_BYTES] = take(data, DATA_BYTES)?.try_into().ok()?;
            Some(encode::row(magazine, row % 26, &bytes))
        }
        _ => {
            let [number, designation] = *take(data, 2)? else { return None };
            let mut triplets = [Triplet::default(); TRIPLETS_PER_PACKET];
            for triplet in &mut triplets {
                let [a, b, c] = *take(data, TRIPLET_INPUT_BYTES)? else { return None };
                *triplet = Triplet::from_value(u32::from_le_bytes([a, b, c, 0]) & TRIPLET_VALUE_MASK);
            }
            // Rows 1 to 25 of object pages, and packets 26 to 29.
            Some(encode::triplet_packet(magazine, 1 + number % 29, designation, &triplets))
        }
    }
}

/// A SPRITES record's sprites as the core draws them. A record its reader
/// accepted is one the core accepts: the two hold the same ranges.
fn sprites_of(record: &sprites_record::SpritesRecord<'_>) -> Sprites {
    let mut list = Vec::new();
    for read in record.sprites() {
        let mut frames = read.frames().map(|reference| FrameRef::from_reference(reference.bits()));
        let first = frames.next().flatten().expect("the reader checked each reference");
        let mut sprite = Sprite::new(read.number, (read.row, read.column), (read.height, read.width), first);
        for frame in frames {
            assert!(sprite.frames.push(frame.expect("the reader checked each reference")), "at most 16 frames");
        }
        list.push(Sprite {
            shown: read.shown,
            detectable: read.detectable,
            layer: read.layer,
            order: read.order,
            ..sprite
        });
    }
    assert!(list.len() <= MAX_SPRITES);
    Sprites::new(&list).expect("what the record's reader accepts, the core accepts")
}

/// An operation of an ANIMATE record as the core applies it.
fn to_core(operation: &animate::Operation) -> Option<(Target, Operation)> {
    let target = match operation.target.kind {
        TargetKind::Control => Target::Control(operation.target.number),
        TargetKind::Object => Target::Object(operation.target.number),
        TargetKind::Page | TargetKind::Rect => return None,
    };
    let applied = match operation.kind {
        OperationKind::Show => Operation::Show,
        OperationKind::Hide => Operation::Hide,
        OperationKind::Look(look) => Operation::Look(ForcedLook::from_code(look)?),
        OperationKind::Move { relative, x, y } => Operation::Move { relative, x, y },
        OperationKind::Order { layer, order } => Operation::order(layer, order)?,
        OperationKind::Frame(frame) => Operation::Frame(frame),
        OperationKind::Cycle { first, last, period, once } => Operation::Cycle { first, last, period, once },
        OperationKind::StopCycle => Operation::StopCycle,
        OperationKind::Reset => Operation::Reset,
    };
    Some((target, applied))
}

fn raster(grid: &Grid, glyphs: &GlyphSet, options: RenderOptions) -> Vec<u8> {
    let mut pixels = vec![0u8; PAGE_WIDTH * PAGE_HEIGHT];
    let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
    render_screen(grid, ALL_ROWS, glyphs, options, &mut buffer).expect("a grid renders");
    pixels
}

fuzz_target!(|input: &[u8]| {
    let Some((&settings, mut data)) = input.split_first() else { return };
    let Some(sprites_body) = take_body(&mut data) else { return };
    let Ok(record) = sprites_record::read(sprites_body) else { return };
    let sprites = sprites_of(&record);
    let mut steps = Vec::new();
    while steps.len() < MAX_STEPS {
        let Some((&ticks, rest)) = data.split_first() else { break };
        let mut after = rest;
        let Some(body) = take_body(&mut after) else {
            // The length that ends the steps, or the input's end.
            data = rest.get(2..).unwrap_or(&[]);
            break;
        };
        data = after;
        steps.push((ticks, body));
    }

    let config = Config {
        level: if settings & 1 != 0 { PresentationLevel::Level35 } else { PresentationLevel::Level25 },
        ..Config::default()
    };
    let mut pages = [StoredPage::EMPTY; PAGE_SLOTS];
    let mut sub_pages = [EnhancementSubPage::EMPTY; SUB_PAGE_SLOTS];
    let mut pool = vec![EnhancementPacket::EMPTY; ENHANCEMENT_PACKETS_LEVEL_35 + MAGAZINES * MOT_PACKETS];
    let mut decoder =
        Decoder::with_enhancement_store(&mut pages, EnhancementStore::new(&mut sub_pages, &mut pool), config);
    let mut now = 0u64;
    while let Some((&selector, rest)) = data.split_first() {
        data = rest;
        let Some(packet) = packet_of(selector, &mut data) else { break };
        now += LINE_US;
        decoder.push(&packet, &PacketMeta::default(), Instant(now), &mut |_| {});
    }
    decoder.tick(Instant(u64::MAX / 2), &mut |_| {});
    let Some(address) = PageAddress::from_mpp(DISPLAYED) else { return };
    let Some(page) = decoder.store().get(address, SubCode::ANY) else { return };
    let magazine = decoder.magazine_data(address.magazine);
    let lookup = decoder.enhancement_lookup();
    let display = DisplayOptions::default();
    let mut page_grid: Box<Grid> = Box::new(Grid::empty());
    build_grid(&mut page_grid, Some(page), &magazine, &config, &display);
    let _ = compose_level(&mut page_grid, page, &magazine, &lookup, &config, &display);
    let found = controls::controls(page);

    let glyphs = GlyphSet::noto_teletext();
    let mut animation = Animation::NONE;
    let mut tracker = DamageTracker::new();
    let mut pixels = vec![0xEEu8; PAGE_WIDTH * PAGE_HEIGHT];
    let mut grid: Box<Grid> = Box::new(Grid::empty());
    let mut tick = 0u32;
    // The page as it opens, then after each step.
    for step in core::iter::once(None).chain(steps.iter().map(Some)) {
        if let Some((ticks, body)) = step {
            tick = tick.saturating_add(u32::from(*ticks));
            if let Ok(record) = animate_record::read(body) {
                for operation in record.operations() {
                    if let Some((target, applied)) = to_core(&operation) {
                        let _ = animation.apply(target, applied, tick, &sprites, found.list.len());
                    }
                }
            }
        }
        let _ = animation.advance(tick);
        let placements = Placements::of(&sprites, &animation);
        let drawn = compose_sprites(&mut grid, &page_grid, page, &magazine, &lookup, &config, &display, &placements);
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let differs = grid.cells[row][column] != page_grid.cells[row][column];
                assert!(!differs || placements.covers(row, column), "a cell outside every sprite changed");
                assert!(!differs || (row != TOP_ROW && row != BOTTOM_ROW), "ANIM-8: row {row} is the receiver's");
                assert_eq!(drawn.cells.contains(row, column), differs, "row {row}, column {column}");
            }
        }
        controls::apply_animation(&mut grid, &found, &animation, &drawn.cells);
        let options = RenderOptions::resolved(&config, settings & 2 != 0, FlashPhase::at_tick(tick, &config));
        if let Some(rect) = tracker.damage(&grid, options, drawn.covered) {
            let mut buffer = IndexBuffer { format: IndexFormat::Bits8, data: &mut pixels, stride: PAGE_WIDTH };
            render_rect(&grid, rect, &glyphs, options, &mut buffer).expect("a rectangle of the page renders");
        }
        assert!(pixels == raster(&grid, &glyphs, options), "ANIM-4: the damaged rendering is the whole rendering");
    }
});
