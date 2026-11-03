//! The picture converter's pages as a receiver draws them (TODO-1184,
//! TODO-1764): each page is encoded as T42 packets, received by the core's
//! decoder, composed and rendered as `neottx render` renders it, and every
//! sixel the page draws must be the colour [`Page::drawn`] predicts. The
//! page's structure is checked too: where its Termination Markers stand,
//! and the sub-pages its X/27/4 link names.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

use std::io::Cursor;

use image::{ImageFormat, Rgb, RgbImage};
use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementStore, EnhancementSubPage, StoredPage};
use neoteletext_core::compose::compose_level;
use neoteletext_core::config::Config;
use neoteletext_core::consts::{
    CELL_HEIGHT, LINK_SUBPAGES_SHIFT, MODE_TERMINATION, MOSAIC_ROW_TENTHS, PACKET_BYTES, PACKET_ENHANCEMENT,
    PACKET_LINKS, RECEIVER_COLUMNS, ROW_ADDRESS_LAST, TERMINATION_FINAL_LAST, TERMINATION_INTERMEDIATE_LAST,
    TERMINATION_LOCAL_DATA_END, TRIPLET_ADDRESS_BITS, TRIPLET_MODE_BITS,
};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::packet::{Packet, PacketMeta};
use neoteletext_core::render::{FlashPhase, RenderOptions};
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_formats::consts::TTI_PACKED_CHARACTER_BASE;
use neoteletext_image::palette::Rgba;
use neoteletext_image::raster::{Scale, raster};
use neoteletext_picture::{Level, Options, PAGE_NUMBER, Page, convert};

/// Sixels across and down the page.
const WIDTH: usize = 80;
const HEIGHT: usize = 75;
/// Dots across a sixel as the engine draws a cell of 12.
const SIXEL_DOTS: usize = 6;
/// The object page the converter writes: 1A0.
const OBJECT_PAGE: &str = "PN,1A0";

/// A picture with detail and colour everywhere.
fn busy(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| Rgb([(x * 7 + y * 3) as u8, (x * x + y) as u8, (x * y * 5) as u8]))
}

/// Smooth gradients, the kind of picture the palette fitting is for.
fn gradients(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x * 255 / width) as u8, (y * 255 / height) as u8, ((x + y) * 255 / (width + height)) as u8])
    })
}

fn png(picture: &RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    picture.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
    bytes
}

/// The page's packets received, composed and rendered still, as `neottx
/// render` draws a page, with every capability compiled in.
fn rendered(page: &Page) -> (usize, Vec<[u8; 3]>) {
    let bytes = page.t42().unwrap();
    let config = Config::default();
    // Room for page 100 and the sixteen sub-pages an object page can have,
    // with their packets.
    let mut slots = vec![StoredPage::EMPTY; 32];
    let mut sub_pages = vec![EnhancementSubPage::EMPTY; 32];
    let mut enhancement_packets = vec![EnhancementPacket::EMPTY; 1200];
    let store = EnhancementStore::new(&mut sub_pages, &mut enhancement_packets);
    let mut decoder = Decoder::with_enhancement_store(&mut slots, store, config);
    let (packets, rest) = bytes.as_chunks::<PACKET_BYTES>();
    assert_eq!(rest, [] as [u8; 0], "whole packets");
    for (index, &bytes) in packets.iter().enumerate() {
        let packet = Packet { bytes };
        decoder.push(&packet, &PacketMeta::default(), Instant(index as u64 * 64), &mut |_| {});
    }
    decoder.tick(Instant(1_000_000_000), &mut |_| {});
    let address = PageAddress::from_mpp(PAGE_NUMBER).unwrap();
    let stored = decoder.store().get(address, SubCode::ANY).expect("page 100 is stored");
    let magazine = decoder.magazine_data(address.magazine);
    let options = DisplayOptions::default();
    let mut grid = Box::new(Grid::EMPTY);
    build_grid(&mut grid, Some(stored), &magazine, decoder.config(), &options);
    let _ = compose_level(&mut grid, stored, &magazine, &decoder.enhancement_lookup(), decoder.config(), &options);
    let render = RenderOptions::for_level(&config, false, FlashPhase::STEADY);
    let raster = raster(
        &grid,
        render,
        true,
        &GlyphSet::noto_teletext(),
        Scale { x: 1, y: 1, aspect: false },
        Rgba::opaque(0, 0, 0),
    );
    let colours = raster
        .indices
        .iter()
        .map(|&index| {
            let colour = raster.palette[usize::from(index)];
            [colour.r, colour.g, colour.b]
        })
        .collect();
    (raster.width, colours)
}

/// The dot at the middle of each sixel, row by row: the bands of a cell are
/// the core's mosaic rows, 6, 8 and 6 lines of 20.
fn sixel_middles(width: usize, colours: &[[u8; 3]]) -> Vec<[u8; 3]> {
    let lines: Vec<usize> = MOSAIC_ROW_TENTHS.iter().map(|tenths| tenths * CELL_HEIGHT / 10).collect();
    let mut out = Vec::with_capacity(WIDTH * HEIGHT);
    for sy in 0..HEIGHT {
        let (row, band) = (sy / lines.len(), sy % lines.len());
        let y = row * CELL_HEIGHT + lines[..band].iter().sum::<usize>() + lines[band] / 2;
        for sx in 0..WIDTH {
            out.push(colours[y * width + sx * SIXEL_DOTS + SIXEL_DOTS / 2]);
        }
    }
    out
}

/// Whether the page draws sixel `index`: not the receiver's eight columns
/// of row 0, and not row 0 at all when the header is kept.
fn page_draws(index: usize, keep_header: bool) -> bool {
    let (sy, sx) = (index / WIDTH, index % WIDTH);
    let row_0 = sy < MOSAIC_ROW_TENTHS.len();
    !(row_0 && (keep_header || sx < RECEIVER_COLUMNS * 2))
}

/// The sixels that differ between the page as drawn by the receiver and
/// as the converter predicts it, as (column, row) of the sixel.
fn differences(page: &Page, keep_header: bool) -> Vec<(usize, usize)> {
    let (width, colours) = rendered(page);
    sixel_middles(width, &colours)
        .iter()
        .zip(&page.drawn)
        .enumerate()
        .filter(|&(index, (seen, predicted))| page_draws(index, keep_header) && seen != predicted)
        .map(|(index, _)| (index % WIDTH, index / WIDTH))
        .collect()
}

/// Every mode the converter writes: each level, Hold Mosaics, objects or
/// the page's own packets, and the header kept.
fn modes() -> Vec<(&'static str, Options)> {
    let at = |level| Options { level, ..Options::default() };
    vec![
        ("1", at(Level::One)),
        ("1.5", at(Level::OnePointFive)),
        ("1.5 hold", Options { hold_mosaics: true, ..at(Level::OnePointFive) }),
        ("1.5 header", Options { keep_header: true, ..at(Level::OnePointFive) }),
        ("2.5", at(Level::TwoPointFive)),
        ("2.5 objects", Options { objects: true, ..at(Level::TwoPointFive) }),
        ("3.5", at(Level::ThreePointFive)),
        ("3.5 objects", Options { objects: true, ..at(Level::ThreePointFive) }),
        ("3.5 header", Options { keep_header: true, ..at(Level::ThreePointFive) }),
    ]
}

/// REQ-2335 and REQ-2327: at every level and in every mode, row 0
/// included, the receiver draws each sixel of the page in the colour the
/// converter predicts.
#[test]
fn every_page_draws_as_predicted() {
    for (name, picture) in [("busy", busy(120, 90)), ("gradients", gradients(160, 120))] {
        for (mode, options) in modes() {
            let page = convert(&png(&picture), &options).unwrap();
            let wrong = differences(&page, options.keep_header);
            assert!(wrong.is_empty(), "{name} at {mode}: {} sixels differ, first {:?}", wrong.len(), &wrong[..1]);
        }
    }
}

/// The negative control of the round trip: a page with one sixel's bit
/// flipped in its row 5 is caught, so the comparison above can fail.
#[test]
fn a_flipped_sixel_is_caught() {
    let page = convert(&png(&busy(120, 90)), &Options { level: Level::OnePointFive, ..Options::default() }).unwrap();
    let row = page.tti.lines().position(|line| line.starts_with("OL,5,")).unwrap();
    let mut lines: Vec<String> = page.tti.lines().map(str::to_string).collect();
    // The first mosaic cell after column 20; an attribute is written as two
    // characters, an escape and the code. Bit 0 is the top-left sixel.
    let mut chars: Vec<char> = lines[row].chars().collect();
    let (mut column, mut at) = (0, "OL,5,".len());
    while column < 20 || u32::from(chars[at]) < 0x20 || chars[at] == '\u{1b}' {
        at += if chars[at] == '\u{1b}' { 2 } else { 1 };
        column += 1;
    }
    chars[at] = char::from_u32(u32::from(chars[at]) ^ 1).unwrap();
    lines[row] = chars.into_iter().collect();
    let flipped = Page { tti: lines.join("\n") + "\n", ..page };
    assert_ne!(differences(&flipped, false), [] as [(usize, usize); 0]);
}

/// The triplets of the packed enhancement lines `OL,<packet>,` of one page
/// of the file, in order: the designation character dropped, three
/// characters of six bits a triplet, low bits first.
fn packed_triplets(lines: &[&str], packet: u8) -> Vec<u32> {
    let start = format!("OL,{packet},");
    lines
        .iter()
        .filter_map(|line| line.strip_prefix(start.as_str()))
        .flat_map(|text| {
            let bits: Vec<u32> =
                text.chars().skip(1).map(|c| u32::from(c) & !u32::from(TTI_PACKED_CHARACTER_BASE)).collect();
            bits.chunks(3).map(|chunk| chunk[0] | chunk[1] << 6 | chunk[2] << 12).collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `triplet` is a Termination Marker of `kind` (Table 28: address
/// 63, mode 11111, the kind in the data's low bits).
fn is_termination(triplet: u32, kind: u8) -> bool {
    let address = triplet & ((1 << TRIPLET_ADDRESS_BITS) - 1);
    let mode = triplet >> TRIPLET_ADDRESS_BITS & ((1 << TRIPLET_MODE_BITS) - 1);
    let data = triplet >> (TRIPLET_ADDRESS_BITS + TRIPLET_MODE_BITS);
    address == u32::from(ROW_ADDRESS_LAST) && mode == u32::from(MODE_TERMINATION) && data == u32::from(kind)
}

/// The lines of page 100 and of each object sub-page, in the file's order.
fn pages_of(tti: &str) -> Vec<Vec<&str>> {
    let mut pages: Vec<Vec<&str>> = Vec::new();
    for line in tti.lines() {
        if line.starts_with("PN,") {
            pages.push(Vec::new());
        }
        if let Some(page) = pages.last_mut() {
            page.push(line);
        }
    }
    pages
}

/// EN 300 706 clause 12.3.1 and Table 28: the page's own enhancement data
/// ends with the Termination Marker "local enhancement data end", and with
/// nothing else after it but more of the same; each object sub-page's
/// definition ends with "intermediate, last", the last sub-page's with
/// "final, last".
#[test]
fn termination_markers_end_each_definition() {
    let source = png(&busy(120, 90));
    for objects in [false, true] {
        let options = Options { objects, ..Options::default() };
        let tti = convert(&source, &options).unwrap().tti;
        let pages = pages_of(&tti);
        let local = packed_triplets(&pages[0], PACKET_ENHANCEMENT);
        let end = local.iter().position(|&t| is_termination(t, TERMINATION_LOCAL_DATA_END)).unwrap();
        assert!(local[end..].iter().all(|&t| is_termination(t, TERMINATION_LOCAL_DATA_END)), "objects {objects}");
        assert!(local[..end].iter().all(|&t| !is_termination(t, TERMINATION_LOCAL_DATA_END)), "objects {objects}");
        let object_pages: Vec<&Vec<&str>> = pages.iter().filter(|page| page[0].starts_with(OBJECT_PAGE)).collect();
        assert_eq!(object_pages.is_empty(), !objects);
        for (index, page) in object_pages.iter().enumerate() {
            let kind =
                if index + 1 == object_pages.len() { TERMINATION_FINAL_LAST } else { TERMINATION_INTERMEDIATE_LAST };
            // The definitions are rows 3 to 25 and then X/26 (clause
            // 10.5.1.2); the first marker of any kind ends the object.
            let mut triplets: Vec<u32> = (3..=25).flat_map(|row| packed_triplets(page, row)).collect();
            triplets.extend(packed_triplets(page, PACKET_ENHANCEMENT));
            let first = triplets.iter().position(|&t| (0..8).any(|any_kind| is_termination(t, any_kind))).unwrap();
            assert!(is_termination(triplets[first], kind), "sub-page {index}");
        }
    }
}

/// EN 300 706 Table 15: the POP link of X/27/4 names exactly the object
/// sub-pages the file carries, one bit each from sub-page 0; without
/// objects the page has no X/27/4.
#[test]
fn the_pop_link_names_the_sub_pages_sent() {
    let source = png(&busy(120, 90));
    let own = convert(&source, &Options::default()).unwrap().tti;
    assert_eq!(packed_triplets(&pages_of(&own)[0], PACKET_LINKS), [] as [u32; 0]);
    for level in [Level::TwoPointFive, Level::ThreePointFive] {
        let tti = convert(&source, &Options { level, objects: true, ..Options::default() }).unwrap().tti;
        let pages = pages_of(&tti);
        let sent = pages.iter().filter(|page| page[0].starts_with(OBJECT_PAGE)).count();
        let links = packed_triplets(&pages[0], PACKET_LINKS);
        // Link 1 is the POP link: triplets 2 and 3 (Table 16).
        let subpages = links[3] >> LINK_SUBPAGES_SHIFT;
        assert_eq!(subpages, (1 << sent) - 1, "{level:?}: {sent} sub-pages sent");
    }
}
