//! Tests of the render options taken from the configuration and the level.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, `panic!` on outcomes it rules out, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

// The module is compiled for tests only (see the parent file); the
// attribute also marks this file as test code for `cargo xtask check-layers`.
#[cfg(test)]
use crate::render::*;

use crate::config::{CellMode, Config};

/// Decision E.55, REQ-RND-002, REQ-RND-003, REQ-RND-035: the options take
/// the cell size and rounding from the configuration; Levels 1 and 1.5
/// draw in the palette mode and Levels 2.5 and 3.5 in the resolved mode at
/// the flash phase given; the underline is drawn unless turned off.
/// REQ-398: Levels 1 and 1.5 draw in the palette mode, and Levels 2.5 and 3.5 in the resolved mode.
#[test]
fn render_options_follow_the_configuration_and_level() {
    let phase = FlashPhase::at_unit(9);
    let enhanced = RenderOptions::for_level(&Config::default(), true, phase);
    assert_eq!(
        enhanced,
        RenderOptions {
            mode: CellMode::Saa5050,
            rounding: true,
            reveal: true,
            colour: ColourMode::Resolved(phase),
            underline: true
        }
    );
    assert_eq!(enhanced.row_height(), crate::consts::CELL_HEIGHT);
    let level_15 = RenderOptions::for_level(&Config::saa5246a(), false, phase);
    assert_eq!(
        level_15,
        RenderOptions {
            mode: CellMode::Saa5246a,
            rounding: false,
            reveal: false,
            colour: ColourMode::Palette,
            underline: true
        }
    );
    assert_eq!(level_15.row_height(), crate::consts::CELL_HEIGHT_SMALL);
    assert_eq!(RenderOptions::for_level(&Config::saa5050(), false, phase).colour, ColourMode::Palette);
    assert!(!level_15.with_underline(false).underline);
}
