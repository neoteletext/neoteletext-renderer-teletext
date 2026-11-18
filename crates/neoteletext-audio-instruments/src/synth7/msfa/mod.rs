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

// This file was translated to Rust from synth.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. What was changed: only the
// block size is kept (the memory barrier, the `min` and `max` templates and
// the NEON detection have no use here); the block is 32 samples and not 64,
// so that the 960 samples of an instrument block are a whole number of
// them; the sample rate, which the original passes to each `init`
// function when the program starts, is a constant; and `share` and
// `share_u32` are added, for the part of a block that synth7 advances
// when a message splits it.

//! The DX7's parameter layer: what turns a patch, a key and a velocity
//! into the levels and frequencies of six operators as time passes.
//!
//! Every file in this directory is translated from Google's
//! music-synthesizer-for-android and stays under the Apache License,
//! version 2.0 (`LICENSE-APACHE-2.0` in the crate's folder); each begins
//! with the original's notice and says what was changed. They keep the
//! original's names and its fixed point, so that each can be read beside
//! its source: a level is a logarithm to base two with 24 fractional bits
//! ("Q24"), a frequency likewise, and an envelope is advanced once for each
//! block of [`N`] samples.

pub mod env;
pub mod freqlut;
pub mod lfo;
pub mod note;
pub mod pitchenv;
pub mod tables;

/// The logarithm to base two of [`N`].
///
/// Source: music-synthesizer-for-android `synth.h`, `LG_N`, which is 6
/// there. Own choice of 5: 960 samples are thirty blocks of 32, and a
/// shorter block follows the instrument, which advances its envelopes every
/// sample, more closely.
pub const LG_N: u32 = 5;

/// Samples in a block: envelopes, the low frequency oscillator and pitch
/// are advanced once for each.
///
/// Source: music-synthesizer-for-android `synth.h`, `N`.
pub const N: u32 = 1 << LG_N;

/// Samples a second rendered, which the original takes as a parameter.
///
/// Source: this crate's `SAMPLE_RATE` (REQ-6411 item 1).
pub const SAMPLE_RATE: u32 = crate::SAMPLE_RATE;

/// One doubling of a level, or one octave of a frequency, in the fixed
/// point of this layer.
///
/// Source: music-synthesizer-for-android `env.h`: "Result is in
/// Q24/doubling log format".
pub const Q24_ONE: i32 = 1 << 24;

/// What an amount that a whole block advances by, `per_block`, advances by
/// in `samples` of a block: `per_block × samples ÷ N`, rounded down, and
/// `per_block` itself, unchanged, for [`N`] samples or more. Added for
/// synth7, not in the original: synth7 splits a block at a message's
/// sample (`synth7.md`, S7-52), and each part of the block advances by its
/// share of the block's time.
#[must_use]
pub const fn share(per_block: i32, samples: u32) -> i32 {
    if samples >= N {
        return per_block;
    }
    // `per_block` is `whole × N + rest`, with `rest` in 0..N; with
    // `samples` below N neither product can be larger than `per_block`.
    let samples = samples.cast_signed();
    let whole = per_block >> LG_N;
    let rest = per_block & N.cast_signed().wrapping_sub(1);
    whole.wrapping_mul(samples).wrapping_add(rest.wrapping_mul(samples) >> LG_N)
}

/// [`share`] of an unsigned amount.
#[must_use]
pub const fn share_u32(per_block: u32, samples: u32) -> u32 {
    if samples >= N {
        return per_block;
    }
    let whole = per_block >> LG_N;
    let rest = per_block & (N - 1);
    whole.wrapping_mul(samples).wrapping_add(rest.wrapping_mul(samples) >> LG_N)
}
