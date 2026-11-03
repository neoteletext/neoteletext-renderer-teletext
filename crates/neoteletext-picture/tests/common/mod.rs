//! The receiver the picture converter's tests draw its pages with: the
//! core's decoder, composer and renderer, as `neottx render` uses them.
#![allow(dead_code, reason = "each test file uses the part of this module it needs")]

use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementStore, EnhancementSubPage, StoredPage};
use neoteletext_core::compose::compose_level;
use neoteletext_core::config::Config;
use neoteletext_core::consts::{CELL_HEIGHT, MOSAIC_ROW_TENTHS, PACKET_BYTES, RECEIVER_COLUMNS};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::{DisplayOptions, Grid, build_grid};
use neoteletext_core::packet::{Packet, PacketMeta};
use neoteletext_core::render::{FlashPhase, RenderOptions};
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use neoteletext_image::palette::Rgba;
use neoteletext_image::raster::{Scale, raster};
use neoteletext_picture::{PAGE_NUMBER, Page};

/// Sixels across and down the page.
pub const WIDTH: usize = 80;
pub const HEIGHT: usize = 75;
/// Dots across a sixel as the engine draws a cell of 12.
pub const SIXEL_DOTS: usize = 6;
/// The page's packets received, composed and rendered still, as `neottx
/// render` draws a page, with every capability compiled in.
pub fn rendered(page: &Page) -> (usize, Vec<[u8; 3]>) {
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
pub fn sixel_middles(width: usize, colours: &[[u8; 3]]) -> Vec<[u8; 3]> {
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
pub fn page_draws(index: usize, keep_header: bool) -> bool {
    let (sy, sx) = (index / WIDTH, index % WIDTH);
    let row_0 = sy < MOSAIC_ROW_TENTHS.len();
    !(row_0 && (keep_header || sx < RECEIVER_COLUMNS * 2))
}

/// The sixels that differ between the page as drawn by the receiver and
/// as the converter predicts it, as (column, row) of the sixel.
pub fn differences(page: &Page, keep_header: bool) -> Vec<(usize, usize)> {
    let (width, colours) = rendered(page);
    sixel_middles(width, &colours)
        .iter()
        .zip(&page.drawn)
        .enumerate()
        .filter(|&(index, (seen, predicted))| page_draws(index, keep_header) && seen != predicted)
        .map(|(index, _)| (index % WIDTH, index / WIDTH))
        .collect()
}
