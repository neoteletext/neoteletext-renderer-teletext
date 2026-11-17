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

// This file was translated to Rust from freqlut.cc and freqlut.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. What was changed: the
// table is a constant for 48,000 samples a second (`tables.rs`), not built
// in floating point when the program starts; the result is the phase
// advanced in a sample where a whole turn is 2^32, eight bits finer than
// the original's 2^24; the interpolation between two entries uses the top
// 12 of the 14 low bits, so that its product fits 32 bits where the
// original's takes 64; a frequency above the table's range wraps, as the
// phase it would be added to does, where the original's shift by a
// negative count is undefined; nothing can panic.

//! Resolve frequency signal (1.0 in Q24 format = 1 octave) to phase delta.

use super::tables::FREQUENCY;

/// The logarithm to base two of the entries in an octave of
/// [`FREQUENCY`].
///
/// Source: music-synthesizer-for-android `freqlut.cc`, `LG_N_SAMPLES`.
const LG_N_SAMPLES: u32 = 10;

/// The bits of a frequency below one entry of the table.
///
/// Source: music-synthesizer-for-android `freqlut.cc`, `SAMPLE_SHIFT`.
const SAMPLE_SHIFT: u32 = 24 - LG_N_SAMPLES;

/// The low bits dropped before interpolating, so that the product of the
/// largest difference of two entries (under 2^19) and the rest (12 bits)
/// fits 32 bits.
///
/// Source: own choice, as the notice above says.
const INTERPOLATION_DROPPED_BITS: u32 = 2;

/// The octave whose phase increments the table holds for a turn of 2^24.
///
/// Source: music-synthesizer-for-android `freqlut.cc`, `MAX_LOGFREQ_INT`.
const MAX_LOGFREQ_INT: i32 = 20;

/// The bits by which a turn of 2^32 is finer than the original's 2^24.
const TURN_EXTRA_BITS: i32 = 8;

/// The phase a sine of frequency `logfreq` advances in one sample, a whole
/// turn being 2^32. `logfreq` is the logarithm to base two of the frequency
/// in hertz, in Q24.
#[must_use]
pub fn lookup(logfreq: i32) -> u32 {
    let ix = usize::try_from((logfreq & 0x00ff_ffff) >> SAMPLE_SHIFT).unwrap_or(0);
    let y0 = FREQUENCY.get(ix).copied().unwrap_or(0);
    let y1 = FREQUENCY.get(ix.saturating_add(1)).copied().unwrap_or(y0);
    let lowbits = (logfreq & ((1 << SAMPLE_SHIFT) - 1)).cast_unsigned() >> INTERPOLATION_DROPPED_BITS;
    let y =
        y0.saturating_add(y1.saturating_sub(y0).saturating_mul(lowbits) >> (SAMPLE_SHIFT - INTERPOLATION_DROPPED_BITS));
    let hibits = logfreq >> 24;
    let shift = (MAX_LOGFREQ_INT - TURN_EXTRA_BITS).saturating_sub(hibits);
    match u32::try_from(shift) {
        Ok(right) => y.checked_shr(right).unwrap_or(0),
        Err(_) => y.checked_shl(shift.unsigned_abs()).unwrap_or(0),
    }
}
