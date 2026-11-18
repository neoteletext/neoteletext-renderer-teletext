/*
 * Copyright 2012 Google Inc.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *      http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

// This file was translated to Rust from env.cc and env.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. What was changed:
// integer types have their widths written out and every operation that
// could overflow saturates; table lookups cannot go out of range and
// nothing can panic; `init` takes the level to start from, where the
// original always starts from zero, so that a note that takes over a
// sounding voice continues from that voice's level; `setparam` is left
// out; the comment on `init` is corrected where it gave nominal full scale as
// 99 * 32; the increment of a stage is scaled from the instrument's own
// sample rate to the rate and block size rendered here, where the
// original uses the instrument's increments unchanged at whatever rate it
// runs; `stage`, `level`, `new` and `set_outlevel` are added;
// `getsample` is `getsample_for` a whole block, which advances the
// envelope by the share of a block's increment that a part of a block
// takes, for synth7's control steps split at a message's sample; and
// `Progress`, `progress` and `set_progress` are added, to put an
// envelope back where it was when such a step began.

//! The envelope generator of one operator.

use super::{N, SAMPLE_RATE, share};

/// The stages of an envelope: three while the key is down, the fourth
/// after it is released.
///
/// Source: music-synthesizer-for-android `env.h`: `rates_[4]`,
/// `levels_[4]`.
pub const STAGES: usize = 4;

/// The stage number of an envelope that has finished its last stage.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::getsample`, which
/// advances no envelope whose `ix_` is 4.
pub const STAGE_FINISHED: u8 = 4;

/// The stage an envelope holds while the key stays down: its third level.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::getsample`:
/// `ix_ < 3 || (ix_ < 4) && !down_`.
const STAGE_SUSTAIN: u8 = 3;

/// Samples a second of the instrument, in which the envelope's increments
/// are measured.
///
/// Source: music-synthesizer-for-android wiki, `Dx7Hardware.wiki`: "at a
/// sampling rate of 49096 Hz", and `Dx7Envelope.wiki`: "At a qrate of 0,
/// the amplitude decreases by one step every 4096 samples", which is the
/// increment `Env::advance` computes; that page's decay rate of 0.2819
/// decibels a second at qrate 0 is that increment at 49,096 samples a
/// second.
pub const NATIVE_SAMPLE_RATE: u32 = 49_096;

/// The instrument's samples that pass in one block rendered here, with 16
/// fractional bits: `N × 49096 / 48000`, rounded.
///
/// Source: derived from [`NATIVE_SAMPLE_RATE`], [`N`] and
/// [`SAMPLE_RATE`]. The original multiplies by its block size alone
/// (`<< LG_N`).
const NATIVE_SAMPLES_IN_BLOCK_Q16: i64 =
    (((N as i64 * NATIVE_SAMPLE_RATE as i64) << 16) + (SAMPLE_RATE as i64 / 2)) / SAMPLE_RATE as i64;

/// The level an attack jumps to at once, in steps of 1/256 of a doubling.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::getsample`,
/// `jumptarget`.
const JUMP_TARGET: i32 = 1716;

/// The level an attack rises towards, in Q24: its speed is in proportion to
/// the whole doublings left below this.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::getsample`:
/// `(17 << 24) - level_`.
const ATTACK_CEILING: i32 = 17 << 24;

/// What full scale of level and output level sum to, in steps of 1/256 of a
/// doubling, less the 3,840 steps (15 doublings) that full scale is.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::advance`:
/// `(actuallevel << 6) + outlevel_ - 4256`.
const LEVEL_OFFSET: i32 = 4256;

/// The lowest level an envelope takes, in steps of 1/256 of a doubling.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::advance`:
/// `actuallevel < 16 ? 16 : actuallevel`.
pub const MIN_LEVEL: i32 = 16;

/// The largest rate of an envelope, after rate scaling.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::advance`:
/// `min(qrate, 63)`.
const MAX_QRATE: i32 = 63;

/// A rate of 0 to 99 becomes a rate of 0 to 63 by multiplying by this and
/// dividing by 64.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::advance`:
/// `(rates_[ix_] * 41) >> 6`.
const QRATE_MULTIPLIER: i32 = 41;

/// Output levels and envelope levels below 20, as the instrument scales
/// them.
///
/// Source: music-synthesizer-for-android `env.cc`, `levellut`.
pub const LEVEL_LUT: [u8; 20] = [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46];

/// What is added to an output level of 20 or more.
///
/// Source: music-synthesizer-for-android `env.cc`, `Env::scaleoutlevel`:
/// `28 + outlevel`.
const LEVEL_LINEAR_OFFSET: i32 = 28;

/// Where an envelope is on its way: what [`Env::getsample_for`] changes,
/// and nothing else. Added for synth7, not in the original: synth7 puts an
/// envelope back where it stood when a control step began, to bring it to
/// a message's sample within the step (`synth7.md`, S7-52).
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    level: i32,
    targetlevel: i32,
    inc: i32,
    ix: u8,
    rising: bool,
}

impl Progress {
    /// Where [`Env::new`] stands.
    #[must_use]
    pub const fn new() -> Self {
        Self { level: 0, targetlevel: 0, inc: 0, ix: STAGE_FINISHED, rising: false }
    }

    /// The progress of the pitch envelope, which has the same five parts.
    pub(crate) const fn at(level: i32, targetlevel: i32, inc: i32, ix: u8, rising: bool) -> Self {
        Self { level, targetlevel, inc, ix, rising }
    }

    /// The five parts, as [`Progress::at`] takes them.
    pub(crate) const fn parts(&self) -> (i32, i32, i32, u8, bool) {
        (self.level, self.targetlevel, self.inc, self.ix, self.rising)
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

/// DX7 envelope generation.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    rates: [u8; STAGES],
    levels: [u8; STAGES],
    outlevel: i32,
    rate_scaling: i32,
    /// Level is stored so that 2^24 is one doubling, ie 16 more bits than
    /// the DX7 itself (fraction is stored in level rather than separate
    /// counter).
    level: i32,
    targetlevel: i32,
    rising: bool,
    ix: u8,
    inc: i32,
    down: bool,
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

impl Env {
    /// An envelope that has finished, at level zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rates: [0; STAGES],
            levels: [0; STAGES],
            outlevel: 0,
            rate_scaling: 0,
            level: 0,
            targetlevel: 0,
            rising: false,
            ix: STAGE_FINISHED,
            inc: 0,
            down: false,
        }
    }

    /// Starts the envelope with the key down. The rates and levels arrays
    /// are calibrated to match the Dx7 parameters (ie, value 0..99). The
    /// outlevel parameter is calibrated in microsteps (ie units of approx
    /// .023 dB), with 127 * 32 = nominal full scale (127 being what
    /// [`Env::scaleoutlevel`] makes of 99; the original's comment says
    /// 99 * 32, which its own caller does not pass). The `rate_scaling`
    /// parameter is in qRate units (ie 0..63). `level` is the level to start
    /// from, as [`Env::level`] gives it: zero for a voice that was silent.
    pub fn init(&mut self, rates: [u8; STAGES], levels: [u8; STAGES], outlevel: i32, rate_scaling: i32, level: i32) {
        self.rates = rates;
        self.levels = levels;
        self.outlevel = outlevel;
        self.rate_scaling = rate_scaling;
        self.level = level;
        self.down = true;
        self.advance(0);
    }

    /// Advances the envelope by one block and gives its level. Result is in
    /// Q24/doubling log format: 2^24 is one doubling, and nominal full
    /// scale is 15 doublings.
    pub fn getsample(&mut self) -> i32 {
        self.getsample_for(N)
    }

    /// As [`Env::getsample`], for `samples` of a block: the level moves by
    /// that share of a block's increment ([`share`]); [`N`] or more is a
    /// whole block, and 0 advances nothing. Added for synth7, not in the
    /// original.
    pub fn getsample_for(&mut self, samples: u32) -> i32 {
        if samples == 0 {
            return self.level;
        }
        if self.ix < STAGE_SUSTAIN || (self.ix < STAGE_FINISHED && !self.down) {
            if self.rising {
                let jumptarget = JUMP_TARGET << 16;
                if self.level < jumptarget {
                    self.level = jumptarget;
                }
                let factor = ATTACK_CEILING.saturating_sub(self.level) >> 24;
                self.level = self.level.saturating_add(share(factor.saturating_mul(self.inc), samples));
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix.saturating_add(1));
                }
            } else {
                self.level = self.level.saturating_sub(share(self.inc, samples));
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix.saturating_add(1));
                }
            }
        }
        self.level
    }

    /// The key goes down or up: up starts the fourth stage.
    pub fn keydown(&mut self, down: bool) {
        if self.down != down {
            self.down = down;
            self.advance(if down { 0 } else { STAGE_SUSTAIN });
        }
    }

    /// Sets the output level of a sounding envelope, as [`Env::init`] takes
    /// it, and aims its stage at the level that makes: an output level
    /// changed while a key is held (`synth7.md`, S7-44). Added for synth7,
    /// not in the original.
    pub fn set_outlevel(&mut self, outlevel: i32) {
        if self.outlevel != outlevel {
            self.outlevel = outlevel;
            // Held at the third level, the envelope goes back to its third
            // stage to reach the third level the new output level makes;
            // in any other stage it aims that stage at its new level.
            let stage = if self.down && self.ix == STAGE_SUSTAIN { STAGE_SUSTAIN.saturating_sub(1) } else { self.ix };
            if stage < STAGE_FINISHED {
                self.advance(stage);
            }
        }
    }

    /// Where the envelope is on its way, for [`Env::set_progress`].
    #[must_use]
    pub const fn progress(&self) -> Progress {
        Progress { level: self.level, targetlevel: self.targetlevel, inc: self.inc, ix: self.ix, rising: self.rising }
    }

    /// Puts the envelope back where [`Env::progress`] found it. Its rates,
    /// levels, output level and key are left as they are: only a message
    /// changes them.
    pub const fn set_progress(&mut self, progress: &Progress) {
        self.level = progress.level;
        self.targetlevel = progress.targetlevel;
        self.inc = progress.inc;
        self.ix = progress.ix;
        self.rising = progress.rising;
    }

    /// The level now, in Q24, without advancing.
    #[must_use]
    pub const fn level(&self) -> i32 {
        self.level
    }

    /// The stage the envelope is in: 0 to 3, or [`STAGE_FINISHED`] once the
    /// fourth stage has reached its level.
    #[must_use]
    pub const fn stage(&self) -> u8 {
        self.ix
    }

    /// An output level or envelope level of 0 to 99 as the instrument
    /// scales it: 0 to 127, in steps of about 0.75 decibels.
    #[must_use]
    pub fn scaleoutlevel(outlevel: i32) -> i32 {
        if outlevel >= 20 {
            LEVEL_LINEAR_OFFSET.saturating_add(outlevel)
        } else {
            usize::try_from(outlevel).ok().and_then(|index| LEVEL_LUT.get(index)).map_or(0, |&level| i32::from(level))
        }
    }

    fn advance(&mut self, newix: u8) {
        self.ix = newix;
        let stage = usize::from(newix);
        if let (Some(&newlevel), Some(&rate)) = (self.levels.get(stage), self.rates.get(stage)) {
            let mut actuallevel = Self::scaleoutlevel(i32::from(newlevel)) >> 1;
            actuallevel = (actuallevel << 6).saturating_add(self.outlevel).saturating_sub(LEVEL_OFFSET);
            actuallevel = actuallevel.clamp(MIN_LEVEL, i32::MAX >> 16);
            // level here is same as Java impl
            self.targetlevel = actuallevel << 16;
            self.rising = self.targetlevel > self.level;

            // rate
            let mut qrate = i32::from(rate).saturating_mul(QRATE_MULTIPLIER) >> 6;
            qrate = qrate.saturating_add(self.rate_scaling).clamp(0, MAX_QRATE);
            self.inc = Self::increment(qrate);
        }
    }

    /// What a decay at `qrate` (0 to 63) takes from the level in one block,
    /// in Q24: the instrument's `(4 + (qrate & 3)) << (2 + (qrate >> 2))` a
    /// sample, for the instrument's samples that pass in a block, rounded.
    #[must_use]
    pub fn increment(qrate: i32) -> i32 {
        let qrate = qrate.clamp(0, MAX_QRATE);
        let native: i32 = (4 | (qrate & 3)) << ((qrate >> 2).saturating_add(2));
        let block = i64::from(native).saturating_mul(NATIVE_SAMPLES_IN_BLOCK_Q16).saturating_add(1 << 15) >> 16;
        i32::try_from(block).unwrap_or(i32::MAX)
    }
}
