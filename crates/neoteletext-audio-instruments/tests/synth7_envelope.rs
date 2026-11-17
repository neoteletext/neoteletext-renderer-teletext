//! synth7's operator envelope: how long each stage takes at 48,000
//! samples a second in blocks of 32, against the time the original's own
//! arithmetic gives (REQ-6412 item 6).
//!
//! # The formulas, from music-synthesizer-for-android `env.cc`
//!
//! A level is a logarithm to base two in Q24. A patch's level `l` (0 to
//! 99) with an output level of `outlevel` microsteps becomes the target
//! `max(16, ((scaleoutlevel(l) >> 1) << 6) + outlevel - 4256) << 16`. A
//! rate `r` (0 to 99) becomes `qrate = min(63, (r * 41 >> 6) +
//! rate_scaling)`, and one sample of the instrument moves the level by
//! `inc = (4 + (qrate & 3)) << (2 + (qrate >> 2))`: down by `inc` in a
//! decay; in an attack, first up to `1716 << 16` at once if it is below
//! that, then up by `inc * ((17 << 24) - level >> 24)`. The original
//! applies 64 samples' worth at a time (`<< LG_N`), once a block.
//!
//! The instrument's sample is 1/49,096 of a second (the original's wiki,
//! `Dx7Hardware.wiki`). So the time of a stage is the number of the
//! original's blocks it takes, times 64, over 49,096; and this crate's is
//! its number of blocks, times 32, over 48,000.
//!
//! # The tolerance
//!
//! A stage ends on a block, so each time is uncertain by one block of its
//! own: 64/49,096 s there and 32/48,000 s here, 2 ms together. The
//! conversion of the increment to a block of 32 samples at 48,000 rounds a
//! 16-bit fraction, under 0.01%. A decay is linear, so nothing else
//! differs. An attack depends on the level at the start of each block, so
//! a block half as long follows the curve more closely and ends a little
//! sooner or later: within 2% over the grid below, plus those 2 ms and,
//! for an attack, one more block of each.
#![cfg(feature = "synth7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::synth7::msfa::env::{Env, NATIVE_SAMPLE_RATE};

/// `env.cc`, word for word, in the original's blocks of 64 samples.
struct Original {
    rates: [i32; 4],
    levels: [i32; 4],
    outlevel: i32,
    rate_scaling: i32,
    level: i32,
    targetlevel: i32,
    rising: bool,
    ix: i32,
    inc: i32,
    down: bool,
}

impl Original {
    const LG_N: i32 = 6;

    fn init(rates: [u8; 4], levels: [u8; 4], outlevel: i32, rate_scaling: i32) -> Self {
        let mut env = Self {
            rates: rates.map(i32::from),
            levels: levels.map(i32::from),
            outlevel,
            rate_scaling,
            level: 0,
            targetlevel: 0,
            rising: false,
            ix: 0,
            inc: 0,
            down: true,
        };
        env.advance(0);
        env
    }

    fn getsample(&mut self) -> i32 {
        if self.ix < 3 || (self.ix < 4 && !self.down) {
            if self.rising {
                let jumptarget = 1716;
                if self.level < (jumptarget << 16) {
                    self.level = jumptarget << 16;
                }
                self.level += (((17 << 24) - self.level) >> 24) * self.inc;
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            } else {
                self.level -= self.inc;
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix + 1);
                }
            }
        }
        self.level
    }

    fn keydown(&mut self, down: bool) {
        if self.down != down {
            self.down = down;
            self.advance(if down { 0 } else { 3 });
        }
    }

    fn scaleoutlevel(outlevel: i32) -> i32 {
        const LEVELLUT: [i32; 20] = [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46];
        if outlevel >= 20 { 28 + outlevel } else { LEVELLUT[outlevel as usize] }
    }

    fn advance(&mut self, newix: i32) {
        self.ix = newix;
        if self.ix < 4 {
            let newlevel = self.levels[self.ix as usize];
            let mut actuallevel = Self::scaleoutlevel(newlevel) >> 1;
            actuallevel = (actuallevel << 6) + self.outlevel - 4256;
            actuallevel = if actuallevel < 16 { 16 } else { actuallevel };
            self.targetlevel = actuallevel << 16;
            self.rising = self.targetlevel > self.level;
            let mut qrate = (self.rates[self.ix as usize] * 41) >> 6;
            qrate += self.rate_scaling;
            qrate = qrate.min(63);
            self.inc = (4 + (qrate & 3)) << (2 + Self::LG_N + (qrate >> 2));
        }
    }
}

/// The seconds each of the four stages takes: the first three with the key
/// down, the fourth from the key going up (after the third has ended).
/// `sample` advances one block and gives the stage (0 to 4) and the level.
fn stage_times(
    seconds_a_block: f64,
    mut sample: impl FnMut() -> (i32, i32),
    mut key_up: impl FnMut(),
) -> ([f64; 4], [i32; 4]) {
    let mut times = [0.0; 4];
    let mut levels = [0; 4];
    let mut blocks = 0_u64;
    let mut started = 0_u64;
    let mut stage = 0;
    while stage < 4 {
        if stage == 3 && blocks == started {
            key_up();
        }
        let (now, level) = sample();
        blocks += 1;
        assert!(blocks < 40_000_000, "stage {stage} never ends");
        while stage < now.min(4) {
            times[stage as usize] = (blocks - started) as f64 * seconds_a_block;
            levels[stage as usize] = level;
            started = blocks;
            stage += 1;
        }
    }
    (times, levels)
}

fn compare(rates: [u8; 4], levels: [u8; 4], outlevel: i32, rate_scaling: i32) -> f64 {
    let original_block = 64.0 / f64::from(NATIVE_SAMPLE_RATE);
    let block = 32.0 / 48_000.0;

    let original = std::cell::RefCell::new(Original::init(rates, levels, outlevel, rate_scaling));
    let (expected, expected_levels) = stage_times(
        original_block,
        || {
            let level = original.borrow_mut().getsample();
            (original.borrow().ix, level)
        },
        || original.borrow_mut().keydown(false),
    );

    let env = std::cell::RefCell::new(Env::new());
    env.borrow_mut().init(rates, levels, outlevel, rate_scaling, 0);
    let (measured, measured_levels) = stage_times(
        block,
        || {
            let level = env.borrow_mut().getsample();
            (i32::from(env.borrow().stage()), level)
        },
        || env.borrow_mut().keydown(false),
    );

    // Each stage ends at the same level exactly.
    assert_eq!(measured_levels, expected_levels, "rates {rates:?} levels {levels:?}");
    let mut worst = 0.0_f64;
    for stage in 0..4 {
        let (expected, measured) = (expected[stage], measured[stage]);
        let allowed = 0.02 * expected + 2.0 * (original_block + block);
        let error = (measured - expected).abs();
        assert!(
            error <= allowed,
            "rates {rates:?} levels {levels:?} outlevel {outlevel} scaling {rate_scaling} stage {stage}: \
             {measured} s against {expected} s"
        );
        if expected > 0.05 {
            worst = worst.max(error / expected);
        }
    }
    worst
}

#[test]
fn stage_times_are_the_originals() {
    let rates = [0_u8, 10, 25, 37, 50, 62, 75, 87, 99];
    let shapes: [[u8; 4]; 6] =
        [[99, 80, 60, 0], [99, 99, 99, 0], [70, 99, 50, 20], [50, 0, 99, 0], [99, 0, 0, 0], [20, 40, 60, 80]];
    let mut worst = 0.0_f64;
    let mut cases = 0;
    for rate in rates {
        for levels in shapes {
            // Output levels of 99 and 60 as `scaleoutlevel` and `Dx7Note::init`
            // make them (127 and 88, times 32), and the most velocity adds.
            for (outlevel, rate_scaling) in [(127 * 32, 0), (127 * 32, 20), (88 * 32, 0), (127 * 32 + 224, 7)] {
                worst = worst.max(compare([rate; 4], levels, outlevel, rate_scaling));
                cases += 1;
            }
        }
    }
    // Different rates in each stage.
    for rates in [[99, 50, 25, 60], [30, 70, 45, 80], [80, 20, 35, 50]] {
        for levels in shapes {
            worst = worst.max(compare(rates, levels, 127 * 32, 0));
            cases += 1;
        }
    }
    println!("{cases} envelopes; the worst stage longer than 50 ms differs by {:.3}%", worst * 100.0);
    assert!(worst < 0.02);
}

/// The decay the original's wiki gives for a qrate of 0: 0.2819 decibels a
/// second, a halving every 2^20 of the instrument's samples. A block's
/// increment is a whole number, 524 at this rate where 523.69 is exact, so
/// the slowest decays are up to 0.1% out.
#[test]
fn the_slowest_decay_is_the_instruments() {
    let doublings_a_block = f64::from(Env::increment(0)) / f64::from(1 << 24);
    let decibels_a_second = doublings_a_block * 20.0 * 2.0_f64.log10() * 48_000.0 / 32.0;
    assert!((decibels_a_second - 0.2819).abs() < 0.0003, "{decibels_a_second} dB/s");
    // And each four steps of qrate double it.
    for qrate in 0..60 {
        let ratio = f64::from(Env::increment(qrate + 4)) / f64::from(Env::increment(qrate));
        assert!((ratio - 2.0).abs() < 0.003, "qrate {qrate}: {ratio}");
    }
    assert_eq!(Env::increment(64), Env::increment(63));
    assert_eq!(Env::increment(-1), Env::increment(0));
}

/// Full scale is 15 doublings: levels of 99 with an output level of 99,
/// which is 127 * 32 microsteps.
#[test]
fn full_scale_is_fifteen_doublings() {
    let mut env = Env::new();
    env.init([99; 4], [99, 99, 99, 0], 127 * 32, 0, 0);
    let mut level = 0;
    for _ in 0..100 {
        level = env.getsample();
    }
    assert_eq!(level, 15 << 24);
    assert_eq!(env.stage(), 3);
    env.keydown(false);
    for _ in 0..1000 {
        level = env.getsample();
    }
    assert_eq!(level, 16 << 16);
    assert_eq!(env.stage(), 4);
}
