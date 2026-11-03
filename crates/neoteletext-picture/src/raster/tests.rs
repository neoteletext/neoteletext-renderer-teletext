#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::unwrap_used, reason = "tests")]

use super::*;

const WIDTH: usize = 37;
const HEIGHT: usize = 23;

/// A synthetic picture whose every pixel differs from its neighbours, the
/// same one the stored outputs were made from.
fn picture() -> Raster {
    let mut data = Vec::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            data.extend([(x * 7 + y * 3) % 256, (x * x + y) % 256, (x * y * 5) % 256].map(|v| v as u8));
        }
    }
    Raster::new(WIDTH, HEIGHT, data).unwrap()
}

/// FNV-1a, 64 bits: a stable digest to compare outputs by.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// REQ-2331: the resampler matches Pillow 13.0.0 byte for byte. Each digest
/// is of `Image.resize((width, height), filter).tobytes()` of the same
/// picture, made with `uvx --with pillow==13.0.0` on 3 November 2026:
/// shrinking both ways, enlarging, one axis only, and to the page's sixels.
#[test]
fn the_resampler_matches_the_stored_output_of_the_library_it_follows() {
    let expected = [
        (11, 7, Smooth::Area, 0xe35a_6006_0de9_43b9),
        (11, 7, Smooth::Lanczos, 0xe6c0_8e1c_3c84_4d2e),
        (53, 31, Smooth::Area, 0xf2fd_f2d3_4422_d33f),
        (53, 31, Smooth::Lanczos, 0xd857_f0d8_6ae5_ba2d),
        (37, 9, Smooth::Area, 0x0705_6afb_34d2_7ed2),
        (37, 9, Smooth::Lanczos, 0x1873_75fb_1286_23ce),
        (5, 23, Smooth::Area, 0xe50b_bfd6_de52_12af),
        (5, 23, Smooth::Lanczos, 0x92d7_7a59_e35f_d1eb),
        (80, 75, Smooth::Area, 0x668d_cb85_6400_af03),
        (80, 75, Smooth::Lanczos, 0x0dc2_d3a8_c676_e11f),
    ];
    let source = picture();
    for (width, height, smooth, wanted) in expected {
        let out = resample(&source, width, height, smooth);
        assert_eq!((out.width(), out.height()), (width, height));
        assert_eq!(digest(&out.data), wanted, "{width} by {height}, {smooth:?}");
    }
}

/// REQ-2330: a region outside the picture is refused, not read past.
#[test]
fn a_region_outside_is_refused() {
    let source = picture();
    assert!(source.cropped(30, 0, 8, 1).is_none());
    assert!(source.cropped(0, 20, 1, 4).is_none());
    assert!(source.cropped(usize::MAX, 0, 2, 1).is_none());
    assert_eq!(source.cropped(36, 22, 1, 1).unwrap().pixel(0, 0), source.pixel(36, 22));
}
