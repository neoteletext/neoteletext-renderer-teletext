/*
 * Copyright 2013 Google Inc.
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

// This file was translated to Rust from lfo.cc and lfo.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. What was changed:
// integer types have their widths written out and every operation wraps
// or saturates as the original's unsigned arithmetic does; nothing can
// panic; the unit of rate, which `Lfo::init` computes in floating point
// from the sample rate when the program starts and keeps in a static
// variable, is a constant computed in integers for 48,000 samples a second
// and blocks of 32; `reset` takes its four parameters by name, not as six
// bytes of a patch; the sine waveform comes from the sound kernel's
// log-sine and exponent tables, not from the original's sine table; the
// state of the sample-and-hold waveform, which the original leaves
// uninitialised, is given to `new`; an unknown waveform cannot be set.

//! Low frequency oscillator, compatible with DX7.

use super::{N, Q24_ONE, SAMPLE_RATE};
use crate::synth7::kernel;

/// What a rate of 1 adds to the phase in one block, a whole turn being
/// 2^32: `N × 25190424 / sample rate`, rounded.
///
/// Source: music-synthesizer-for-android `lfo.cc`, `Lfo::init`:
/// `N * 25190424 / sample_rate + 0.5`, where "constant is 1 << 32 / 15.5s
/// / 11".
pub const UNIT: u32 = ((2 * N as u64 * 25_190_424 + SAMPLE_RATE as u64) / (2 * SAMPLE_RATE as u64)) as u32;

/// The waveforms of the oscillator, by their numbers in a patch.
///
/// Source: music-synthesizer-for-android `lfo.cc`, `Lfo::getsample`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    /// Number 0.
    Triangle,
    /// Number 1.
    SawtoothDown,
    /// Number 2.
    SawtoothUp,
    /// Number 3.
    Square,
    /// Number 4.
    Sine,
    /// Number 5: a new random level each period.
    SampleAndHold,
}

impl Waveform {
    /// The waveform of a number in a patch; none for a number above 5.
    #[must_use]
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            0 => Some(Self::Triangle),
            1 => Some(Self::SawtoothDown),
            2 => Some(Self::SawtoothUp),
            3 => Some(Self::Square),
            4 => Some(Self::Sine),
            5 => Some(Self::SampleAndHold),
            _ => None,
        }
    }
}

/// The multiplier of the sample-and-hold waveform's random sequence.
///
/// Source: music-synthesizer-for-android `lfo.cc`, `Lfo::getsample`:
/// `(randstate_ * 179 + 17) & 0xff`.
const RANDOM_MULTIPLIER: u8 = 179;

/// The increment of the sample-and-hold waveform's random sequence.
///
/// Source: as [`RANDOM_MULTIPLIER`].
const RANDOM_INCREMENT: u8 = 17;

/// The largest speed and delay of a patch.
///
/// Source: music-synthesizer-for-android `lfo.cc`, `Lfo::reset`: "0..99".
const PARAMETER_MAX: u8 = 99;

/// Half a turn of phase, and half the range of the delay's state.
const HALF: u32 = 1 << 31;

/// Low frequency oscillator, compatible with DX7.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    /// Q32.
    phase: u32,
    delta: u32,
    waveform: Waveform,
    randstate: u8,
    sync: bool,
    delaystate: u32,
    delayinc: u32,
    delayinc2: u32,
}

impl Lfo {
    /// An oscillator at speed 0 with no delay, whose sample-and-hold
    /// sequence starts from `randstate`.
    #[must_use]
    pub const fn new(randstate: u8) -> Self {
        Self {
            phase: 0,
            delta: UNIT,
            waveform: Waveform::Triangle,
            randstate,
            sync: false,
            delaystate: 0,
            delayinc: u32::MAX,
            delayinc2: u32::MAX,
        }
    }

    /// Sets the oscillator from a patch: `speed` and `delay` are 0 to 99
    /// (more is taken as 99).
    pub fn reset(&mut self, speed: u8, delay: u8, sync: bool, waveform: Waveform) {
        let rate = u32::from(speed.min(PARAMETER_MAX));
        let mut sr = if rate == 0 { 1 } else { rate.saturating_mul(165) >> 6 };
        sr = sr.saturating_mul(if sr < 160 { 11 } else { 11_u32.saturating_add(sr.saturating_sub(160) >> 4) });
        self.delta = UNIT.wrapping_mul(sr);
        let mut a = u32::from(PARAMETER_MAX.saturating_sub(delay.min(PARAMETER_MAX)));
        if a == u32::from(PARAMETER_MAX) {
            self.delayinc = u32::MAX;
            self.delayinc2 = u32::MAX;
        } else {
            a = (16 | (a & 15)) << (a >> 4).saturating_add(1);
            self.delayinc = UNIT.wrapping_mul(a);
            a &= 0xff80;
            a = a.max(0x80);
            self.delayinc2 = UNIT.wrapping_mul(a);
        }
        self.waveform = waveform;
        self.sync = sync;
    }

    /// Advances the oscillator by one block. Result is 0..1 in Q24.
    pub fn getsample(&mut self) -> i32 {
        self.phase = self.phase.wrapping_add(self.delta);
        let phase = self.phase;
        let value = match self.waveform {
            Waveform::Triangle => {
                let x = phase >> 7;
                (if phase >= HALF { !x } else { x }) & ((1 << 24) - 1)
            }
            Waveform::SawtoothDown => (!phase ^ HALF) >> 8,
            Waveform::SawtoothUp => (phase ^ HALF) >> 8,
            Waveform::Square => (!phase >> 7) & (1 << 24),
            Waveform::Sine => {
                // The original's sine is ±2^24 and is halved; the kernel's is
                // ±2^14 at most, so it is shifted to ±2^23.
                let sine = kernel::sine(phase) << (23 - kernel::SINE_BITS);
                return (Q24_ONE >> 1).saturating_add(sine);
            }
            Waveform::SampleAndHold => {
                if phase < self.delta {
                    self.randstate = self.randstate.wrapping_mul(RANDOM_MULTIPLIER).wrapping_add(RANDOM_INCREMENT);
                }
                let x = u32::from(self.randstate ^ 0x80);
                x.saturating_add(1) << 16
            }
        };
        i32::try_from(value).unwrap_or(Q24_ONE)
    }

    /// Advances the delay by one block: how much of the oscillator's depth
    /// has faded in since the key went down. Result is 0..1 in Q24.
    pub fn getdelay(&mut self) -> i32 {
        let delta = if self.delaystate < HALF { self.delayinc } else { self.delayinc2 };
        let d = self.delaystate.wrapping_add(delta);
        if d < self.delayinc {
            return Q24_ONE;
        }
        self.delaystate = d;
        if d < HALF { 0 } else { i32::try_from((d >> 7) & ((1 << 24) - 1)).unwrap_or(Q24_ONE) }
    }

    /// A key goes down: the delay starts again, and so does the waveform if
    /// the patch asks for key sync.
    pub fn keydown(&mut self) {
        if self.sync {
            self.phase = HALF - 1;
        }
        self.delaystate = 0;
    }
}
