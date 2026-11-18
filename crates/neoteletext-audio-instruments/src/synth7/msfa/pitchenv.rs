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

// This file was translated to Rust from pitchenv.cc and pitchenv.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff; the notice above is
// pitchenv.cc's, and pitchenv.h's is the same but for its year, 2013. What
// was changed:
// integer types have their widths written out and every operation that
// could overflow saturates; table lookups cannot go out of range and
// nothing can panic; the unit of rate, which `PitchEnv::init` computes in
// floating point from the sample rate when the program starts and keeps in
// a static variable, is a constant computed in integers for 48,000 samples
// a second and blocks of 32; `new` is added; `getsample` is
// `getsample_for` a whole block, which advances the envelope by the share
// of a block's increment that a part of a block takes, for synth7's
// control steps split at a message's sample; and `progress` and
// `set_progress` are added, to put the envelope back where it was when
// such a step began.

//! Computation of the DX7 pitch envelope.

use super::env::{Progress, STAGE_FINISHED, STAGES};
use super::{N, SAMPLE_RATE, share};

/// The stage an envelope holds while the key stays down.
///
/// Source: music-synthesizer-for-android `pitchenv.cc`,
/// `PitchEnv::getsample`: `ix_ < 3 || (ix_ < 4) && !down_`.
const STAGE_SUSTAIN: u8 = 3;

/// What a rate of 1 in [`RATETAB`] adds to the level in one block, in Q24
/// of an octave: `N × 2^24 / (21.3 × sample rate)`, rounded.
///
/// Source: music-synthesizer-for-android `pitchenv.cc`, `PitchEnv::init`:
/// `unit_ = N * (1 << 24) / (21.3 * sample_rate) + 0.5`. The 21.3 is
/// written here as 213 / 10.
pub const UNIT: i32 =
    ((2 * 10 * N as i64 * (1 << 24) + 213 * SAMPLE_RATE as i64) / (2 * 213 * SAMPLE_RATE as i64)) as i32;

/// The speed of each rate, 0 to 99.
///
/// Source: music-synthesizer-for-android `pitchenv.cc`, `ratetab`.
#[rustfmt::skip]
pub const RATETAB: [u8; 100] = [
    1, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12,
    12, 13, 13, 14, 14, 15, 16, 16, 17, 18, 18, 19, 20, 21, 22, 23, 24,
    25, 26, 27, 28, 30, 31, 33, 34, 36, 37, 38, 39, 41, 42, 44, 46, 47,
    49, 51, 53, 54, 56, 58, 60, 62, 64, 66, 68, 70, 72, 74, 76, 79, 82,
    85, 88, 91, 94, 98, 102, 106, 110, 115, 120, 125, 130, 135, 141, 147,
    153, 159, 165, 171, 178, 185, 193, 202, 211, 232, 243, 254, 255,
];

/// The pitch of each level, 0 to 99, in steps of 1/32 of an octave; level
/// 50 is no change.
///
/// Source: music-synthesizer-for-android `pitchenv.cc`, `pitchtab`.
#[rustfmt::skip]
pub const PITCHTAB: [i8; 100] = [
    -128, -116, -104, -95, -85, -76, -68, -61, -56, -52, -49, -46, -43,
    -41, -39, -37, -35, -33, -32, -31, -30, -29, -28, -27, -26, -25, -24,
    -23, -22, -21, -20, -19, -18, -17, -16, -15, -14, -13, -12, -11, -10,
    -9, -8, -7, -6, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
    28, 29, 30, 31, 32, 33, 34, 35, 38, 40, 43, 46, 49, 53, 58, 65, 73,
    82, 92, 103, 115, 127,
];

/// A [`PITCHTAB`] entry becomes Q24 of an octave by this shift.
///
/// Source: music-synthesizer-for-android `pitchenv.cc`, `PitchEnv::set`:
/// `pitchtab[l[3]] << 19`.
const PITCH_SHIFT: u32 = 19;

/// Computation of the DX7 pitch envelope.
#[derive(Clone, Copy, Debug)]
pub struct PitchEnv {
    rates: [u8; STAGES],
    levels: [u8; STAGES],
    level: i32,
    targetlevel: i32,
    rising: bool,
    ix: u8,
    inc: i32,
    down: bool,
}

impl Default for PitchEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchEnv {
    /// A pitch envelope that has finished, at no change of pitch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rates: [0; STAGES],
            levels: [0; STAGES],
            level: 0,
            targetlevel: 0,
            rising: false,
            ix: STAGE_FINISHED,
            inc: 0,
            down: false,
        }
    }

    /// Starts the envelope with the key down. The rates and levels arrays
    /// are calibrated to match the Dx7 parameters (ie, value 0..99).
    pub fn set(&mut self, rates: [u8; STAGES], levels: [u8; STAGES]) {
        self.rates = rates;
        self.levels = levels;
        self.level = Self::pitch(levels[3]);
        self.down = true;
        self.advance(0);
    }

    /// Advances the envelope by one block. Result is in Q24/octave.
    pub fn getsample(&mut self) -> i32 {
        self.getsample_for(N)
    }

    /// As [`PitchEnv::getsample`], for `samples` of a block: the level
    /// moves by that share of a block's increment ([`share`]); [`N`] or
    /// more is a whole block, and 0 advances nothing. Added for synth7, not
    /// in the original.
    pub fn getsample_for(&mut self, samples: u32) -> i32 {
        if samples == 0 {
            return self.level;
        }
        if self.ix < STAGE_SUSTAIN || (self.ix < STAGE_FINISHED && !self.down) {
            let inc = share(self.inc, samples);
            if self.rising {
                self.level = self.level.saturating_add(inc);
                if self.level >= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix.saturating_add(1));
                }
            } else {
                self.level = self.level.saturating_sub(inc);
                if self.level <= self.targetlevel {
                    self.level = self.targetlevel;
                    self.advance(self.ix.saturating_add(1));
                }
            }
        }
        self.level
    }

    /// Where the envelope is on its way, as [`super::env::Env::progress`]
    /// gives it. Added for synth7, not in the original.
    #[must_use]
    pub const fn progress(&self) -> Progress {
        Progress::at(self.level, self.targetlevel, self.inc, self.ix, self.rising)
    }

    /// Puts the envelope back where [`PitchEnv::progress`] found it; its
    /// rates, levels and key are left as they are. Added for synth7, not in
    /// the original.
    pub const fn set_progress(&mut self, progress: &Progress) {
        (self.level, self.targetlevel, self.inc, self.ix, self.rising) = progress.parts();
    }

    /// The key goes down or up: up starts the fourth stage.
    pub fn keydown(&mut self, down: bool) {
        if self.down != down {
            self.down = down;
            self.advance(if down { 0 } else { STAGE_SUSTAIN });
        }
    }

    /// The pitch of a level of 0 to 99, in Q24 of an octave.
    fn pitch(level: u8) -> i32 {
        PITCHTAB.get(usize::from(level)).map_or(0, |&pitch| i32::from(pitch) << PITCH_SHIFT)
    }

    fn advance(&mut self, newix: u8) {
        self.ix = newix;
        let stage = usize::from(newix);
        if let (Some(&newlevel), Some(&rate)) = (self.levels.get(stage), self.rates.get(stage)) {
            self.targetlevel = Self::pitch(newlevel);
            self.rising = self.targetlevel > self.level;

            self.inc = RATETAB.get(usize::from(rate)).map_or(0, |&speed| i32::from(speed).saturating_mul(UNIT));
        }
    }
}
