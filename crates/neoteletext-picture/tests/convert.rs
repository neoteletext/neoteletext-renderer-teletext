//! The picture converter from bytes to page, each test naming the
//! requirement it verifies.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

use std::io::Cursor;

use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, Frame, ImageEncoder, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use neoteletext_picture::{
    Align, Display, Error, Fit, Focus, Level, MAX_INPUT_BYTES, Options, Place, Region, convert, parse_colour,
};

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const GREEN: [u8; 3] = [0, 255, 0];
const BLACK: [u8; 3] = [0, 0, 0];

/// A picture of `width` by `height` whose pixels `colour` gives.
fn picture(width: u32, height: u32, colour: impl Fn(u32, u32) -> [u8; 3]) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| Rgb(colour(x, y)))
}

/// A picture with detail everywhere: no two neighbours alike.
fn busy(width: u32, height: u32) -> RgbImage {
    picture(width, height, |x, y| [(x * 7 + y * 3) as u8, (x * x + y) as u8, (x * y * 5) as u8])
}

fn encoded(picture: &DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    picture.write_to(&mut Cursor::new(&mut bytes), format).unwrap();
    bytes
}

fn png(picture: &RgbImage) -> Vec<u8> {
    encoded(&DynamicImage::ImageRgb8(picture.clone()), ImageFormat::Png)
}

/// The page of a PNG of `picture` with `options`.
fn page_of(picture: &RgbImage, options: &Options) -> String {
    convert(&png(picture), options).unwrap().tti
}

fn level_15() -> Options {
    Options { level: Level::OnePointFive, ..Options::default() }
}

/// The text of row `row`'s line, if the page has one.
fn row_line(tti: &str, row: usize) -> Option<&str> {
    let start = format!("OL,{row},");
    tti.lines().find_map(|line| line.strip_prefix(start.as_str()))
}

/// Whether a Level 1.5 row shows nothing but black: after its Graphics
/// colour code, every cell is the empty mosaic.
fn row_is_black(tti: &str, row: usize) -> bool {
    // Row 0 starts after the receiver's eight columns.
    let text = row_line(tti, row).unwrap().trim_start_matches(' ');
    text.as_bytes()[2..].iter().all(|&byte| byte == b' ')
}

/// A PNG chunk with its CRC (ISO/IEC 15948 clause 5.3).
fn chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in kind.iter().chain(data) {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&(!crc).to_be_bytes());
    out
}

/// A PNG that claims `width` by `height` RGBA pixels and holds none.
fn claimed_png(width: u32, height: u32) -> Vec<u8> {
    let mut header = width.to_be_bytes().to_vec();
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(chunk(*b"IHDR", &header));
    bytes.extend(chunk(*b"IDAT", &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
    bytes.extend(chunk(*b"IEND", &[]));
    bytes
}

/// An EXIF block, little-endian TIFF, with one orientation tag.
fn exif_orientation(orientation: u16) -> Vec<u8> {
    let mut exif = b"II*\0".to_vec();
    exif.extend(8u32.to_le_bytes());
    exif.extend(1u16.to_le_bytes());
    exif.extend(0x0112u16.to_le_bytes());
    exif.extend(3u16.to_le_bytes());
    exif.extend(1u32.to_le_bytes());
    exif.extend(orientation.to_le_bytes());
    exif.extend(0u16.to_le_bytes());
    exif.extend(0u32.to_le_bytes());
    exif
}

/// Four flat quadrants, 16-pixel aligned, so JPEG keeps them exact in any
/// orientation.
fn quadrants() -> RgbImage {
    picture(64, 48, |x, y| match (x < 32, y < 32) {
        (true, true) => RED,
        (false, true) => GREEN,
        (true, false) => BLUE,
        (false, false) => [255, 255, 255],
    })
}

// ---------------------------------------------------------------------------
// REQ-2330: formats, bounds, orientation, animation
// ---------------------------------------------------------------------------

/// REQ-2330: the lossless formats give the same page as PNG, and JPEG and
/// lossless WebP convert.
#[test]
fn each_of_the_five_formats_converts() {
    // Few colours, so GIF's palette holds them exactly.
    let source = DynamicImage::ImageRgb8(picture(96, 64, |x, y| if (x / 8 + y / 8) % 2 == 0 { RED } else { BLUE }));
    let options = level_15();
    let reference = convert(&encoded(&source, ImageFormat::Png), &options).unwrap();
    for format in [ImageFormat::Bmp, ImageFormat::Gif, ImageFormat::WebP] {
        assert_eq!(convert(&encoded(&source, format), &options).unwrap(), reference, "{format:?}");
    }
    assert!(convert(&encoded(&source, ImageFormat::Jpeg), &options).is_ok());
}

/// REQ-2330: a side over 32,768 pixels is refused from the header.
#[test]
fn a_side_over_the_bound_is_refused() {
    assert_eq!(convert(&claimed_png(40_000, 10), &Options::default()), Err(Error::PictureTooLarge));
}

/// REQ-2330: sides within the bound whose pixels would need more memory
/// than the decoder may take are refused before decoding.
#[test]
fn a_picture_over_the_memory_bound_is_refused() {
    assert_eq!(convert(&claimed_png(30_000, 30_000), &Options::default()), Err(Error::PictureTooLarge));
}

/// REQ-2330: a file one byte over the bound is refused unread.
#[test]
fn a_file_over_the_bound_is_refused() {
    let bytes = vec![0u8; MAX_INPUT_BYTES + 1];
    assert_eq!(convert(&bytes, &Options::default()), Err(Error::InputTooLarge { bytes: MAX_INPUT_BYTES + 1 }));
}

/// REQ-2330: bytes of no format, and a truncated picture, are decode errors.
#[test]
fn bytes_of_no_format_and_a_truncated_file_are_refused() {
    assert!(matches!(convert(b"not a picture at all", &Options::default()), Err(Error::Decode(_))));
    let whole = png(&busy(50, 40));
    assert!(matches!(convert(&whole[..whole.len() / 2], &Options::default()), Err(Error::Decode(_))));
    assert!(matches!(convert(&[], &Options::default()), Err(Error::Decode(_))));
}

/// REQ-2330: a picture stored turned and tagged to be turned back gives the
/// upright picture's page, in PNG and JPEG; taken as stored it does not.
#[test]
fn the_orientation_tag_turns_the_picture_upright() {
    let upright = DynamicImage::ImageRgb8(quadrants());
    // Orientation 6: the stored picture is turned back clockwise for display.
    let stored = upright.rotate270();
    let options = level_15();
    for jpeg in [false, true] {
        let mut bytes = Vec::new();
        let rgb = stored.to_rgb8();
        let (width, height) = rgb.dimensions();
        if jpeg {
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 100);
            encoder.set_exif_metadata(exif_orientation(6)).unwrap();
            encoder.write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8).unwrap();
        } else {
            let mut encoder = PngEncoder::new(&mut bytes);
            encoder.set_exif_metadata(exif_orientation(6)).unwrap();
            encoder.write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8).unwrap();
        }
        let format = if jpeg { ImageFormat::Jpeg } else { ImageFormat::Png };
        let expected = convert(&encoded(&upright, format), &options).unwrap();
        assert_eq!(convert(&bytes, &options).unwrap(), expected, "jpeg: {jpeg}");
        let as_stored = Options { auto_orient: false, ..options.clone() };
        assert_ne!(convert(&bytes, &as_stored).unwrap(), expected, "jpeg: {jpeg}");
    }
}

/// REQ-2330: an animated GIF gives its first frame.
#[test]
fn an_animation_gives_its_first_frame() {
    let frame =
        |colour: [u8; 3]| Frame::new(RgbaImage::from_pixel(40, 30, Rgba([colour[0], colour[1], colour[2], 255])));
    let mut bytes = Vec::new();
    GifEncoder::new(&mut bytes).encode_frames([frame(RED), frame(BLUE)]).unwrap();
    let first = DynamicImage::ImageRgb8(picture(40, 30, |_, _| RED));
    let options = level_15();
    assert_eq!(convert(&bytes, &options).unwrap(), convert(&encoded(&first, ImageFormat::Png), &options).unwrap());
}

// ---------------------------------------------------------------------------
// REQ-2328: transparency and the background
// ---------------------------------------------------------------------------

/// REQ-2328: a transparent half shows the background, black by default,
/// never the colour hidden behind it.
#[test]
fn transparency_shows_the_background() {
    let mut transparent = RgbaImage::new(96, 64);
    for (x, _, pixel) in transparent.enumerate_pixels_mut() {
        // The left half is red but fully transparent.
        *pixel = if x < 48 { Rgba([255, 0, 0, 0]) } else { Rgba([255, 255, 255, 255]) };
    }
    let bytes = encoded(&DynamicImage::ImageRgba8(transparent), ImageFormat::Png);
    for (background, name) in [(BLACK, None), (BLUE, Some("blue"))] {
        let options = Options { background, ..level_15() };
        let expected = page_of(&picture(96, 64, |x, _| if x < 48 { background } else { [255, 255, 255] }), &options);
        let options = Options { background: name.map_or(BLACK, |name| parse_colour(name).unwrap()), ..level_15() };
        assert_eq!(convert(&bytes, &options).unwrap().tti, expected);
    }
}

/// REQ-2328: each colour name and `#RRGGBB` are read, and other text is not.
#[test]
fn background_colours_are_read() {
    assert_eq!(parse_colour("magenta"), Some([255, 0, 255]));
    assert_eq!(parse_colour("#0a10FF"), Some([10, 16, 255]));
    for refused in ["purple", "#12345", "#1234567", "#12345g", "0a10ff", "#é1234"] {
        assert_eq!(parse_colour(refused), None, "{refused}");
    }
}

// ---------------------------------------------------------------------------
// REQ-2331 and REQ-2332: fits and placement
// ---------------------------------------------------------------------------

/// A picture three times as wide as tall, red on the left third, green in
/// the middle and blue on the right.
fn thirds() -> RgbImage {
    picture(300, 100, |x, _| match x / 100 {
        0 => RED,
        1 => GREEN,
        _ => BLUE,
    })
}

/// REQ-2332: crop keeps the part the alignment names; centred by default.
#[test]
fn crop_keeps_the_aligned_part() {
    let solid = |colour: [u8; 3]| page_of(&picture(100, 100, |_, _| colour), &level_15());
    let aligned = |across: Place| Options { align: Some(Align { across, down: Place::Middle }), ..level_15() };
    assert_eq!(page_of(&thirds(), &aligned(Place::Start)), solid(RED));
    assert_eq!(page_of(&thirds(), &level_15()), solid(GREEN));
    assert_eq!(page_of(&thirds(), &aligned(Place::End)), solid(BLUE));
}

/// REQ-2331: contain leaves bars of the background above and below a wide
/// picture; stretch and crop fill every row.
#[test]
fn contain_leaves_bars_and_stretch_fills() {
    let wide = picture(300, 100, |_, _| GREEN);
    let contain = page_of(&wide, &Options { fit: Fit::Contain, ..level_15() });
    assert!(row_is_black(&contain, 2) && row_is_black(&contain, 22), "{contain}");
    assert!(!row_is_black(&contain, 12), "{contain}");
    for fit in [Fit::Crop, Fit::Stretch] {
        let page = page_of(&wide, &Options { fit, ..level_15() });
        assert!((1..25).all(|row| !row_is_black(&page, row)), "{fit:?}: {page}");
    }
}

/// REQ-2331: a television's wider dots make the crop window narrower, so
/// the page differs from the square display's.
#[test]
fn the_television_display_crops_differently() {
    let source = busy(300, 100);
    let television = Options { display: Display::Television, ..level_15() };
    assert_ne!(page_of(&source, &television), page_of(&source, &level_15()));
}

/// REQ-2332: a crop region is cut out first; one outside is refused.
#[test]
fn a_crop_region_selects_and_is_checked() {
    let source = picture(100, 100, |x, y| if x >= 50 && y < 50 { RED } else { BLUE });
    let region = Some(Region { x: 50, y: 0, width: 50, height: 50 });
    let solid_red = page_of(&picture(50, 50, |_, _| RED), &level_15());
    assert_eq!(page_of(&source, &Options { crop: region, ..level_15() }), solid_red);
    let outside = Some(Region { x: 60, y: 0, width: 50, height: 50 });
    assert_eq!(
        convert(&png(&source), &Options { crop: outside, ..level_15() }),
        Err(Error::CropOutside { width: 100, height: 100 })
    );
    assert_eq!(Region::parse("1,2,3,4"), Some(Region { x: 1, y: 2, width: 3, height: 4 }));
    assert_eq!(Region::parse("1,2,3"), None);
}

/// REQ-2332: without enlarging, a small picture keeps its own size on bars.
#[test]
fn without_enlarging_a_small_picture_keeps_its_size() {
    let small = picture(24, 20, |_, _| RED);
    let page = page_of(&small, &Options { upscale: false, ..level_15() });
    // 24 by 20 dots is two cells by one row in the middle; every other row
    // is the background.
    let filled: Vec<usize> = (0..25).filter(|&row| !row_is_black(&page, row)).collect();
    assert_eq!(filled, [12], "{page}");
}

/// REQ-2332: focus on detail places the crop window where the detail is.
#[test]
fn focus_on_detail_finds_the_detail() {
    let source = picture(300, 100, |x, y| if x >= 200 && (x + y) % 2 == 0 { [255, 255, 255] } else { [90, 90, 90] });
    let right = Options { align: Some(Align { across: Place::End, down: Place::Middle }), ..level_15() };
    let focused = Options { focus: Focus::Detail, ..level_15() };
    assert_eq!(page_of(&source, &focused), page_of(&source, &right));
    assert_ne!(page_of(&source, &level_15()), page_of(&source, &right));
}

// ---------------------------------------------------------------------------
// REQ-2333: pixel art
// ---------------------------------------------------------------------------

/// REQ-2333: pixel art is enlarged by the largest whole number and drawn
/// sixel for sixel, centred on the background.
#[test]
fn pixel_art_is_enlarged_whole() {
    let art = busy(40, 25);
    let mut expected = RgbImage::new(80, 75);
    for (x, y, pixel) in expected.enumerate_pixels_mut() {
        // Twice: 80 by 50, its 25 spare rows split 12 above and 13 below.
        if (12..62).contains(&y) {
            *pixel = *art.get_pixel(x / 2, (y - 12) / 2);
        }
    }
    let options = Options { pixel_art: true, ..level_15() };
    assert_eq!(page_of(&art, &options), page_of(&expected, &level_15()));
}

/// REQ-2333: pixel art larger than the page's sixels is refused.
#[test]
fn pixel_art_too_large_is_refused() {
    let options = Options { pixel_art: true, ..level_15() };
    assert_eq!(
        convert(&png(&busy(81, 10)), &options),
        Err(Error::PixelArtTooLarge { width: 81, height: 10, page_width: 80, page_height: 75 })
    );
    let header = Options { keep_header: true, ..options };
    assert!(matches!(convert(&png(&busy(80, 73)), &header), Err(Error::PixelArtTooLarge { page_height: 72, .. })));
}

// ---------------------------------------------------------------------------
// REQ-2334: the header
// ---------------------------------------------------------------------------

/// REQ-2334: keeping the header writes rows 1 to 24 and no row 0, at Level
/// 1.5 and Level 3.5; without it row 0 starts after the receiver's eight
/// columns.
#[test]
fn keeping_the_header_leaves_row_0() {
    let source = busy(120, 90);
    for level in [Level::OnePointFive, Level::ThreePointFive] {
        let kept = page_of(&source, &Options { level, keep_header: true, ..Options::default() });
        assert!(row_line(&kept, 0).is_none(), "{level:?}");
        assert!((1..25).all(|row| row_line(&kept, row).is_some()), "{level:?}");
        let whole = page_of(&source, &Options { level, ..Options::default() });
        assert!(row_line(&whole, 0).unwrap().starts_with("        "), "{level:?}");
    }
}

// ---------------------------------------------------------------------------
// REQ-2335: levels
// ---------------------------------------------------------------------------

/// REQ-2335: with hold mosaics every row starts with Hold Mosaics, written
/// escaped.
#[test]
fn hold_mosaics_starts_every_row() {
    let page = page_of(&busy(120, 90), &Options { hold_mosaics: true, ..level_15() });
    assert!((1..25).all(|row| row_line(&page, row).unwrap().starts_with("\u{1b}^")), "{page}");
}

/// REQ-2335: without objects the triplets fit the page's own sixteen X/26
/// packets; with them they are on the object page 1A0.
#[test]
fn objects_or_the_pages_own_packets() {
    let source = busy(120, 90);
    let own = page_of(&source, &Options::default());
    assert!(own.lines().filter(|line| line.starts_with("OL,26,")).count() <= 16);
    assert!(!own.contains("PN,1A0"));
    let objects = page_of(&source, &Options { objects: true, ..Options::default() });
    assert!(objects.contains("PN,1A000"));
}

/// The prototype's pages of the same picture, written on 3 November 2026
/// by `tmp/image-to-teletext-rs` at its last state, which were rendered
/// with `neottx` when it was measured (TODO-2132).
const GOLDEN: [(&str, &str); 5] = [
    ("1.5", include_str!("data/busy-l15.tti")),
    ("1.5 hold", include_str!("data/busy-l15-hold.tti")),
    ("3.5", include_str!("data/busy-l35.tti")),
    ("3.5 objects", include_str!("data/busy-l35-objects.tti")),
    ("2.5 objects", include_str!("data/busy-l25-objects.tti")),
];

/// REQ-2335: the encoder writes the pages the measured prototype wrote.
#[test]
fn the_pages_are_the_measured_prototypes() {
    let source = busy(120, 90);
    for (mode, expected) in GOLDEN {
        let mut words = mode.split(' ');
        let level = Level::parse(words.next().unwrap()).unwrap();
        let mut options = Options { level, ..Options::default() };
        for word in words {
            match word {
                "hold" => options.hold_mosaics = true,
                _ => options.objects = true,
            }
        }
        assert_eq!(page_of(&source, &options), expected, "{mode}");
    }
}

// ---------------------------------------------------------------------------
// REQ-2336: conflicts; REQ-2327: the page's forms
// ---------------------------------------------------------------------------

/// REQ-2336: each contradictory combination is refused, and no option alone
/// is.
#[test]
fn contradictory_options_are_refused() {
    let base = Options::default();
    let refused = [
        Options { hold_mosaics: true, ..base.clone() },
        Options { objects: true, level: Level::One, ..base.clone() },
        Options { pixel_art: true, fit: Fit::Contain, ..base.clone() },
        Options { pixel_art: true, focus: Focus::Detail, ..base.clone() },
        Options { upscale: false, fit: Fit::Stretch, ..base.clone() },
        Options { focus: Focus::Detail, fit: Fit::Contain, ..base.clone() },
        Options { focus: Focus::Detail, align: Some(Align::CENTRE), ..base.clone() },
        Options { focus: Focus::Detail, upscale: false, ..base.clone() },
    ];
    for options in refused {
        assert!(matches!(options.check(), Err(Error::Conflict(_))), "{options:?}");
    }
    let alone = [
        base.clone(),
        Options { hold_mosaics: true, level: Level::One, ..base.clone() },
        Options { objects: true, ..base.clone() },
        Options { pixel_art: true, ..base.clone() },
        Options { upscale: false, ..base.clone() },
        Options { focus: Focus::Detail, ..base.clone() },
        Options { fit: Fit::Stretch, ..base.clone() },
        Options { keep_header: true, ..base },
    ];
    for options in alone {
        assert_eq!(options.check(), Ok(()), "{options:?}");
    }
}

/// REQ-2327: the T42 form is whole packets of 42 bytes.
#[test]
fn the_page_as_t42_is_whole_packets() {
    let page = convert(&png(&busy(120, 90)), &Options { objects: true, ..Options::default() }).unwrap();
    let packets = page.t42().unwrap();
    assert!(!packets.is_empty() && packets.len().is_multiple_of(42), "{} bytes", packets.len());
}
