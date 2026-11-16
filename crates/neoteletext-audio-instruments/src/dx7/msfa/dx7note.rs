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

// This file was translated to Rust from dx7note.cc and dx7note.h of
// music-synthesizer-for-android (https://github.com/google/music-synthesizer-for-android)
// at commit f67d41d313b7dc85f6fb99e79e515cc9d208cfff. What was changed: the
// `Dx7Note` class is not here, since the note that holds the operators is
// built on this crate's own sound kernel; what `Dx7Note::init` and
// `Dx7Note::compute` work out for a note is here as functions
// (`operator_outlevel`, `feedback_shift`, `pitch_mod_depth`,
// `pitch_mod_sens`, `lfo_pitch_mod`) that take their values by name, not
// as bytes of a patch; the fine frequency's logarithm is a constant table
// (`tables.rs`), not computed in floating point; the pitch bend, which the
// original fixes at three semitones, is left to the caller; integer types
// have their widths written out, every operation that could overflow
// saturates, table lookups cannot go out of range and nothing can panic;
// the `SUPER_PRECISE` and `VERBOSE` branches are left out.

//! This is the logic to put together a note from the MIDI description:
//! what a key and a velocity make of each operator's level, rates and
//! frequency.

use super::Q24_ONE;
use super::env::Env;
use super::tables::FINE_LOGFREQ;

/// The logarithm to base two of the frequency of MIDI note 0, in Q24:
/// `(1 << 24) * (log(440) / log(2) - 69/12)`.
///
/// Source: music-synthesizer-for-android `dx7note.cc`,
/// `midinote_to_logfreq`, `base`.
const LOGFREQ_NOTE_ZERO: i32 = 50_857_777;

/// A semitone as a logarithm of frequency, in Q24.
///
/// Source: music-synthesizer-for-android `dx7note.cc`,
/// `midinote_to_logfreq`, `step`.
const LOGFREQ_SEMITONE: i32 = Q24_ONE / 12;

/// The logarithm to base two of the frequency of a MIDI note, in Q24.
#[must_use]
pub fn midinote_to_logfreq(midinote: i32) -> i32 {
    LOGFREQ_NOTE_ZERO.saturating_add(LOGFREQ_SEMITONE.saturating_mul(midinote))
}

/// The coarse frequency setting as a logarithm of the ratio, in Q24:
/// setting 0 is a ratio of one half, and setting `n` a ratio of `n`.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `coarsemul`.
#[rustfmt::skip]
pub const COARSEMUL: [i32; 32] = [
    -16_777_216, 0, 16_777_216, 26_591_258, 33_554_432, 38_955_489, 43_368_474, 47_099_600,
    50_331_648, 53_182_516, 55_732_705, 58_039_632, 60_145_690, 62_083_076, 63_876_816,
    65_546_747, 67_108_864, 68_576_247, 69_959_732, 71_268_397, 72_509_921, 73_690_858,
    74_816_848, 75_892_776, 76_922_906, 77_910_978, 78_860_292, 79_773_775, 80_654_032,
    81_503_396, 82_323_963, 83_117_622,
];

/// One step of detune as a logarithm of frequency, in Q24, for an
/// oscillator that follows the keyboard.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `osc_freq`: "This
/// was measured at 7.213Hz per count at 9600Hz, but the exact value is
/// somewhat dependent on midinote. Close enough for now."
const DETUNE_STEP: i32 = 12_606;

/// One step of detune above the centre, for an oscillator of fixed
/// frequency.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `osc_freq`:
/// `13457 * (detune - 7)`.
const DETUNE_STEP_FIXED: i32 = 13_457;

/// The detune setting that changes nothing.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `osc_freq`:
/// `detune - 7`.
const DETUNE_CENTRE: i32 = 7;

/// One step of a fixed frequency's setting (a hundredth of a decade) as a
/// logarithm of frequency, in Q24 and shifted left by three:
/// `((1 << 24) * log(10) / log(2) * .01) << 3`.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `osc_freq`.
const FIXED_STEP_Q27: i32 = 4_458_616;

/// The logarithm to base two of an oscillator's frequency in hertz, in
/// Q24. `mode` is 0 for a ratio of the note's frequency and otherwise a
/// fixed frequency; `coarse` is 0 to 31, `fine` 0 to 99 and `detune` 0 to
/// 14.
#[must_use]
pub fn osc_freq(midinote: i32, mode: u8, coarse: u8, fine: u8, detune: u8) -> i32 {
    // TODO: pitch randomization
    let detune = i32::from(detune);
    if mode == 0 {
        let mut logfreq = midinote_to_logfreq(midinote);
        logfreq = logfreq.saturating_add(COARSEMUL.get(usize::from(coarse & 31)).copied().unwrap_or(0));
        logfreq = logfreq.saturating_add(FINE_LOGFREQ.get(usize::from(fine)).copied().unwrap_or(0));
        logfreq.saturating_add(DETUNE_STEP.saturating_mul(detune.saturating_sub(DETUNE_CENTRE)))
    } else {
        let setting = i32::from(coarse & 3).saturating_mul(100).saturating_add(i32::from(fine));
        let logfreq = FIXED_STEP_Q27.saturating_mul(setting) >> 3;
        if detune > DETUNE_CENTRE {
            logfreq.saturating_add(DETUNE_STEP_FIXED.saturating_mul(detune.saturating_sub(DETUNE_CENTRE)))
        } else {
            logfreq
        }
    }
}

/// The instrument's response to key velocity, for velocities two at a
/// time.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `velocity_data`.
#[rustfmt::skip]
pub const VELOCITY_DATA: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163,
    166, 170, 173, 174, 178, 181, 184, 186, 189, 190, 194, 196, 198, 200, 202,
    205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232,
    233, 235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251,
    252, 253, 254,
];

/// The [`VELOCITY_DATA`] value at which velocity changes nothing.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `ScaleVelocity`:
/// `velocity_data[clamped_vel >> 1] - 239`.
const VELOCITY_CENTRE: i32 = 239;

/// See "velocity" section of notes. Returns velocity delta in microsteps.
/// `sensitivity` is 0 to 7.
#[must_use]
pub fn scale_velocity(velocity: i32, sensitivity: u8) -> i32 {
    let clamped_vel = usize::try_from(velocity.clamp(0, 127)).unwrap_or(0);
    let vel_value =
        VELOCITY_DATA.get(clamped_vel >> 1).map_or(0, |&value| i32::from(value)).saturating_sub(VELOCITY_CENTRE);
    (i32::from(sensitivity).saturating_mul(vel_value).saturating_add(7) >> 3) << 4
}

/// What keyboard rate scaling adds to an envelope's rates for a key, in
/// qrate units. `sensitivity` is 0 to 7.
#[must_use]
pub fn scale_rate(midinote: i32, sensitivity: u8) -> i32 {
    let x = (midinote / 3).saturating_sub(7).clamp(0, 31);
    i32::from(sensitivity).saturating_mul(x) >> 3
}

/// The exponential curve of keyboard level scaling, by groups of three
/// keys from the break point.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `exp_scale_data`.
#[rustfmt::skip]
pub const EXP_SCALE_DATA: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66,
    80, 94, 110, 126, 142, 158, 174, 190, 206, 222, 238, 250,
];

/// The multiplier that makes a depth of 99 over a group of 12, or over a
/// curve value of 250 after a further division by 8, about 99 steps of
/// output level.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `ScaleCurve`:
/// `(group * depth * 329) >> 12` and `(raw_exp * depth * 329) >> 15`.
const SCALE_CURVE_MULTIPLIER: i32 = 329;

/// What one side of keyboard level scaling adds to the output level, for a
/// key `group` groups of three keys from the break point. `depth` is 0 to
/// 99; `curve` is 0 for a linear fall, 1 for an exponential fall, 2 for an
/// exponential rise and 3 for a linear rise.
#[must_use]
pub fn scale_curve(group: i32, depth: u8, curve: u8) -> i32 {
    let depth = i32::from(depth);
    let scale = if curve == 0 || curve == 3 {
        // linear
        group.saturating_mul(depth).saturating_mul(SCALE_CURVE_MULTIPLIER) >> 12
    } else {
        // exponential
        let last = EXP_SCALE_DATA.len().saturating_sub(1);
        let index = usize::try_from(group).unwrap_or(0).min(last);
        let raw_exp = EXP_SCALE_DATA.get(index).map_or(0, |&value| i32::from(value));
        raw_exp.saturating_mul(depth).saturating_mul(SCALE_CURVE_MULTIPLIER) >> 15
    };
    if curve < 2 { scale.saturating_neg() } else { scale }
}

/// The keys between the break point's number and the key it stands for.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `ScaleLevel`:
/// `midinote - break_pt - 17`.
const BREAK_POINT_OFFSET: i32 = 17;

/// What keyboard level scaling adds to an operator's output level for a
/// key.
#[must_use]
pub fn scale_level(
    midinote: i32,
    break_pt: u8,
    left_depth: u8,
    right_depth: u8,
    left_curve: u8,
    right_curve: u8,
) -> i32 {
    let offset = midinote.saturating_sub(i32::from(break_pt)).saturating_sub(BREAK_POINT_OFFSET);
    if offset >= 0 {
        scale_curve(offset / 3, right_depth, right_curve)
    } else {
        scale_curve(offset.saturating_neg() / 3, left_depth, left_curve)
    }
}

/// How far the low frequency oscillator bends pitch at each pitch
/// modulation sensitivity, 0 to 7.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `pitchmodsenstab`.
pub const PITCHMODSENSTAB: [u8; 8] = [0, 10, 20, 33, 55, 92, 153, 255];

/// The largest scaled output level.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `Dx7Note::init`:
/// `outlevel = min(127, outlevel)`.
const MAX_OUTLEVEL: i32 = 127;

/// One operator's part of the keyboard level scaling of a patch: the
/// arguments of [`scale_level`] after the key.
#[derive(Clone, Copy, Debug)]
pub struct LevelScaling {
    /// The break point, 0 to 99.
    pub break_pt: u8,
    /// The depth below the break point, 0 to 99.
    pub left_depth: u8,
    /// The depth above the break point, 0 to 99.
    pub right_depth: u8,
    /// The curve below the break point, 0 to 3.
    pub left_curve: u8,
    /// The curve above the break point, 0 to 3.
    pub right_curve: u8,
}

/// An operator's output level for a key and a velocity, in microsteps (ie
/// units of approx .023 dB), as [`Env::init`] takes it. `output_level` is
/// the patch's, 0 to 99; `velocity_sensitivity` is 0 to 7.
#[must_use]
pub fn operator_outlevel(
    midinote: i32,
    velocity: i32,
    output_level: u8,
    scaling: LevelScaling,
    velocity_sensitivity: u8,
) -> i32 {
    let mut outlevel = Env::scaleoutlevel(i32::from(output_level));
    let level_scaling = scale_level(
        midinote,
        scaling.break_pt,
        scaling.left_depth,
        scaling.right_depth,
        scaling.left_curve,
        scaling.right_curve,
    );
    outlevel = outlevel.saturating_add(level_scaling);
    outlevel = outlevel.min(MAX_OUTLEVEL);
    outlevel <<= 5;
    outlevel = outlevel.saturating_add(scale_velocity(velocity, velocity_sensitivity));
    outlevel.max(0)
}

/// The shift that stands for no feedback.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `Dx7Note::init`:
/// `fb_shift_ = feedback != 0 ? 8 - feedback : 16`.
pub const FEEDBACK_SHIFT_OFF: u32 = 16;

/// The right shift the original applies to the mean of an operator's last
/// two outputs to make its feedback, for a feedback setting of 0 to 7;
/// [`FEEDBACK_SHIFT_OFF`] for no feedback. With a shift of 1 (feedback 7),
/// an operator at nominal full scale feeds back one whole turn of phase.
#[must_use]
pub fn feedback_shift(feedback: u8) -> u32 {
    if feedback == 0 { FEEDBACK_SHIFT_OFF } else { 8_u32.saturating_sub(u32::from(feedback)) }
}

/// A patch's pitch modulation depth of 0 to 99 as 0 to 255.
///
/// Source: music-synthesizer-for-android `dx7note.cc`, `Dx7Note::init`:
/// `pitchmoddepth_ = (patch[139] * 165) >> 6`.
#[must_use]
pub fn pitch_mod_depth(depth: u8) -> u8 {
    u8::try_from(u32::from(depth).saturating_mul(165) >> 6).unwrap_or(u8::MAX)
}

/// The [`PITCHMODSENSTAB`] entry of a pitch modulation sensitivity.
#[must_use]
pub fn pitch_mod_sens(sensitivity: u8) -> u8 {
    PITCHMODSENSTAB.get(usize::from(sensitivity & 7)).copied().unwrap_or(0)
}

/// What the low frequency oscillator adds to pitch in one block, in Q24 of
/// an octave. `pmd` is the depth in Q32 (the value of [`pitch_mod_depth`]
/// times the oscillator's delay, which is 0..1 in Q24); `pitchmodsens` is
/// the value of [`pitch_mod_sens`]; `lfo_val` is the oscillator's sample,
/// 0..1 in Q24.
#[must_use]
pub fn lfo_pitch_mod(pmd: u32, pitchmodsens: u8, lfo_val: i32) -> i32 {
    // TODO: add modulation sources (mod wheel, etc)
    let senslfo = i32::from(pitchmodsens).saturating_mul(lfo_val.saturating_sub(Q24_ONE >> 1));
    i32::try_from(i64::from(pmd).saturating_mul(i64::from(senslfo)) >> 39).unwrap_or(0)
}
