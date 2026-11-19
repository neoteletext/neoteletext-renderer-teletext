//! Picture files into the picture converter: decoding within its bounds,
//! the fit and placement options, and the sixels (REQ-2327, REQ-2330,
//! REQ-TST-040). Every input must give a page or an error, never a panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_picture::{Align, Display, Fit, Focus, Level, Options, Region, Smooth, convert};

/// Bytes at the start that choose the options; the rest is the picture.
const OPTION_BYTES: usize = 6;

fuzz_target!(|data: &[u8]| {
    let Some((choice, picture)) = data.split_at_checked(OPTION_BYTES) else { return };
    let bit = |byte: usize, bit: u8| choice[byte] >> bit & 1 == 1;
    let options = Options {
        // Level 1.5 keeps each run short: the encoder's work is fixed by the
        // page, not the input, so the input's paths are all before it.
        level: Level::OnePointFive,
        hold_mosaics: bit(0, 0),
        display: if bit(0, 1) { Display::Television } else { Display::Square },
        smooth: if bit(0, 2) { Smooth::Lanczos } else { Smooth::Area },
        fit: [Fit::Crop, Fit::Contain, Fit::Stretch, Fit::Crop][usize::from(choice[0] >> 3 & 3)],
        align: (bit(0, 5)).then(|| Align::NAMED[usize::from(choice[1]) % Align::NAMED.len()].1),
        background: [choice[1], choice[2], choice[3]],
        pixel_art: bit(0, 6),
        auto_orient: !bit(0, 7),
        crop: bit(2, 0).then(|| Region {
            x: usize::from(choice[4] & 0x0F),
            y: usize::from(choice[4] >> 4),
            width: usize::from(choice[5] & 0x0F) + 1,
            height: usize::from(choice[5] >> 4) + 1,
        }),
        upscale: !bit(2, 1),
        focus: if bit(2, 2) { Focus::Detail } else { Focus::Align },
        keep_header: bit(2, 3),
        keep_footer: bit(2, 4),
        objects: false,
    };
    if let Ok(page) = convert(picture, &options) {
        assert!(page.tti.starts_with("DE,"));
    }
});
