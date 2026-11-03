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
use neoteletext_core::consts::{
    LINK_SUBPAGES_SHIFT, MODE_TERMINATION, PACKET_ENHANCEMENT, PACKET_LINKS, ROW_ADDRESS_LAST, TERMINATION_FINAL_LAST,
    TERMINATION_INTERMEDIATE_LAST, TERMINATION_LOCAL_DATA_END, TRIPLET_ADDRESS_BITS, TRIPLET_MODE_BITS,
};
use neoteletext_formats::consts::TTI_PACKED_CHARACTER_BASE;
use neoteletext_picture::{Level, Options, Page, convert};

mod common;
use common::differences;

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
