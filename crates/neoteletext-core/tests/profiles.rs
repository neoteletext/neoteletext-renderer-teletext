//! The alternatives of REQ-PRF-004 (REQ-370) under each profile: each
//! takes effect whichever profile it is set on.
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;

use common::{Receiver, display_page, grid_of, local_data, set_position, text};
use neoteletext_core::config::{Config, PresentationLevel};
use neoteletext_core::consts::{C5_NEWSFLASH, START_BOX};
use neoteletext_core::packet::TripletFunction;

/// The three profiles, by the names `neottx --profile` takes.
const PROFILES: [&str; 3] = ["en300706", "saa5050", "saa5246a"];

fn profile(name: &str) -> Config {
    Config::profile(name).unwrap()
}

/// The profile's level, raised to Level 1.5 where it is Level 1.0, at which
/// packets X/26 are not applied and an alternative about them cannot show
/// (REQ-PRF-005 lets any setting be changed after a profile).
fn enhanced(config: Config) -> PresentationLevel {
    config.level.max(PresentationLevel::Level15)
}

/// A G0 character written by column mode 10000 with no diacritic, which
/// every level from 1.5 applies (Table 29).
fn g0(column: u8, code: u8) -> TripletFunction {
    TripletFunction::G0Diacritic { column, code, diacritic: 0 }
}

// Verifies REQ-370: ignoring out-of-order triplets takes effect from each
// profile: the triplet writing column 2 after column 5 is applied, or
// ignored when asked.
#[test]
fn out_of_order_triplets_from_each_profile() {
    let packets = display_page(0, &[(4, b"0123456")], local_data(&[set_position(4, 5), g0(5, b'x'), g0(2, b'y')]));
    for name in PROFILES {
        for (ignore, expected) in [(false, "01y34x6"), (true, "01234x6")] {
            let base = profile(name);
            let config = Config { level: enhanced(base), ignore_out_of_order_triplets: ignore, ..base };
            let grid = Receiver::new().run(&packets, config, |decoder| grid_of(decoder, &config)).0;
            assert_eq!(&text(&grid, 4)[..7], expected, "{name}, ignoring {ignore}");
        }
    }
}

// Verifies REQ-370: the clause 12.3.2 row-setting rule takes effect from
// each profile at Level 1.5, where Annex A.3's rule is the other choice: a
// reserved row mode moves the active row under Annex A.3 and not under the
// strict rule. At Levels 2.5 and 3.5 the rule is always the strict one
// (DEC-570).
#[test]
fn strict_row_setting_from_each_profile() {
    let reserved_row_mode = 0b00010;
    let row_6_address = 46;
    let local = [
        set_position(4, 0),
        TripletFunction::ReservedRow { mode: reserved_row_mode, address: row_6_address, data: 0 },
        g0(2, b'x'),
    ];
    let packets = display_page(0, &[(4, b"0123"), (6, b"0123")], local_data(&local));
    for name in PROFILES {
        for (strict, row) in [(false, 6), (true, 4)] {
            let config = Config { level: PresentationLevel::Level15, strict_row_setting: strict, ..profile(name) };
            let grid = Receiver::new().run(&packets, config, |decoder| grid_of(decoder, &config)).0;
            assert_eq!(&text(&grid, row)[..4], "01x3", "{name}, strict {strict}");
        }
    }
}

// Verifies REQ-370: a single Start Box takes effect from each profile, at
// the profile's own level: on a newsflash page, one Start Box boxes the
// cell after it only when a single one is accepted.
#[test]
fn single_start_box_from_each_profile() {
    let packets = display_page(C5_NEWSFLASH, &[(2, &[START_BOX, b'a'])], Vec::new());
    for name in PROFILES {
        for (double, boxed) in [(true, false), (false, true)] {
            let config = Config { double_start_box: double, ..profile(name) };
            let grid = Receiver::new().run(&packets, config, |decoder| grid_of(decoder, &config)).0;
            assert_eq!(grid.cells[2][1].boxed, boxed, "{name}, two required {double}");
        }
    }
}
