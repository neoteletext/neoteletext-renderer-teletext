//! One voice: two oscillators with cross modulation and sync, the source
//! mixer, the high-pass filter, the low-pass filter with its two slopes,
//! two envelopes and the amplifier; and the low-frequency oscillator the
//! voices of one half share.
//!
//! Under cross modulation the oscillators run at four times the output
//! rate, where VCO-1's frequency is set afresh for every sample from
//! VCO-2's waveform, and their mix is brought down to twice the output
//! rate for the filters and the amplifier; without it they run at twice
//! the output rate themselves, where the building block's edges are
//! 91 dB clean. What changes slowly is set every [`TICK_SAMPLES`] samples
//! of that rate: the envelopes, the pitches, the pulse's width, the
//! cutoff and the amplifier's slope.
//!
//! # Cross modulation and sync
//!
//! VCO-2's waveform moves VCO-1's frequency exponentially, by up to six
//! octaves either way ([`law::CROSS_MODULATION`]), and VCO-1's cycle
//! starts VCO-2's again when the sync is on: with both, each oscillator
//! steers the other. Here:
//!
//! - VCO-1's frequency in each sample of 5.2 µs is its own times two to
//!   the power of VCO-2's waveform in that same sample, taken from
//!   VCO-2's phase and not from its band-limited output. Where an edge
//!   of VCO-2 falls within a sample, VCO-1 has the frequency of each
//!   side for that side's share of the sample, so its phase moves by
//!   exactly what it is due wherever the edge falls.
//! - When VCO-1's cycle starts VCO-2's again within a sample, VCO-1 has
//!   run the rest of that sample at the frequency of the cycle that
//!   ended. The start itself is placed where it falls; what VCO-1's
//!   phase then ran too far is taken back before the next sample, and
//!   what it ran too little is added in it, and the one sample that
//!   stood where the phase had run to is moved to where it should have.
//! - VCO-1's frequency is held to a quarter of the oscillators' rate,
//!   48 kHz ([`STEP_LIMIT`](crate::analog::STEP_LIMIT)), so it is bounded
//!   whatever the two do to each other.
//! - Each oscillator's edges are band-limited as the building block
//!   makes them, and a synchronised start is an edge like any other.
//! - The building block's oscillator takes one frequency for a sample,
//!   and puts an edge where that frequency would. Where VCO-2's sine or
//!   sawtooth moves VCO-1's frequency through the sample, the edge
//!   falls a little earlier or later: each edge of VCO-1 is moved to
//!   where a frequency that changes in a straight line through the
//!   sample puts it ([`Voice::placed`]), and VCO-2 is started again
//!   from there.
//! - Where VCO-1's frequency jumps (VCO-2's sawtooth starting again, its
//!   pulse rising or falling, its cycle cut short by the sync) VCO-1's
//!   sawtooth and triangle turn a corner, which is rounded as the
//!   building block rounds a triangle's ([`Corners`]).
//!
//! What this leaves: exponential modulation at audio rate has partials
//! without end, of which those beyond 172 kHz fold back below 20 kHz;
//! an edge of VCO-1 in a sample in which VCO-2's waveform jumps is
//! placed by the sample's mean frequency; and VCO-1's pulse turns no
//! corner but its edges move with the pulse's width, which is set at
//! each tick.
//!
//! # What is left of the aliasing
//!
//! `tests/jupiter8_oscillator.rs` measures it with the sync on, where
//! the sound has one period and what is no harmonic of it can be told
//! apart: the strongest such thing below 20 kHz against the strongest
//! harmonic, on the five As from 110 Hz to 1,760 Hz, VCO-2 an octave
//! above VCO-1. With VCO-1's sawtooth, and VCO-2's sine or sawtooth
//! moving it (the worse of the two):
//!
//! | CROSS MOD | on the A of 440 Hz | the A of 880 Hz | the A of 1,760 Hz |
//! |---|---|---|---|
//! | 1 (0.6 octave) | 100 dB down | 94 dB | 89 dB |
//! | 2.5 (1.5 octaves) | 92 dB | 93 dB | 87 dB |
//! | 5 (3 octaves) | 91 dB | 86 dB | 79 dB |
//! | 10 (6 octaves) | 84 dB | 78 dB | 46 dB |
//!
//! Before the edges were moved and the corners rounded, the slider at 5
//! left 78 dB, 70 dB and 56 dB, and at 10 56 dB, 43 dB and 32 dB. With
//! VCO-1's triangle the slider to 5 and keys to the A of 880 Hz leave
//! 91 dB or better (64 dB before), and with its square 82 dB (69 dB). At 10 on the highest A, VCO-1 is
//! driven to its bound of 48 kHz for part of every cycle: the
//! instrument's own oscillator has a bound there too, which no document
//! read gives.
//!
//! With VCO-2's pulse and the sync, VCO-1 at the slider's 5 is a
//! sawtooth three octaves above its key and nothing else, the pulse
//! never falling before VCO-1 starts it again: what is measured there is
//! the building block's sawtooth at 3.5 kHz, 7 kHz and 14 kHz, 84 dB,
//! 77 dB and 68 dB; a key of 3.5 kHz or 7 kHz with no cross modulation
//! measures the same.
//!
//! Without the sync the partials are sums of multiples of two unrelated
//! frequencies and lie everywhere, so nothing can be told apart and
//! nothing was measured; the arithmetic is the same without the start
//! to place.
//!
//! # What it costs
//!
//! Eight voices render 14 times faster than real time with one
//! oscillator each, 12.5 times with a factory patch of two oscillators
//! and a moving pulse, and 8.3 times under cross modulation with sync
//! (8.8 before its edges were moved and its corners rounded), on one
//! core of a 3.7 GHz Intel Core i9-10900KF
//! (`tests/jupiter8_instrument.rs`, `rendering_speed`, 17 November
//! 2026).

use crate::analog::decimator::Decimator;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, REST_LEVEL, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{ONE_Q28, OnePole, OtaCascade};
use crate::analog::fixed::{amplify, mul_high, mul_high_unsigned, ratio, scale};
use crate::analog::oscillator::{Noise, Oscillator, waveform};
use crate::analog::tables::{BLAMP, BLEP, BLEP_PHASES_SHIFT, BLEP_TAPS};
use crate::analog::{LEVEL, LIMIT, Oversampling, frequency};

use super::law::{self, OCTAVE};
use super::patch::{LfoWave, Param, Patch, PwmSource, Vco1Wave, Vco2Wave};

/// How far above the output rate the oscillators run under cross
/// modulation.
///
/// Source: own choice, from the aliasing measured under cross modulation
/// (`tests/jupiter8_oscillator.rs`).
pub const OSCILLATOR_OVERSAMPLING: Oversampling = Oversampling::FourTimes;
/// The oscillators' rate in hertz.
pub const OSCILLATOR_RATE: u32 = OSCILLATOR_OVERSAMPLING.rate();
/// How far above the output rate the filters and the amplifier run.
///
/// Source: own choice: the filter distorts, as the Juno's does at the
/// same rate.
pub const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// The filters' rate in hertz.
pub const RATE: u32 = OVERSAMPLING.rate();
/// Samples of the oscillators in one of the filters.
pub const OSCILLATOR_SAMPLES: usize = (OSCILLATOR_RATE / RATE) as usize;
/// Bits of the samples in a tick.
const TICK_BITS: u32 = 4;
/// Samples of the filters between two ticks: a sixth of a millisecond.
///
/// Source: own choice: short against the envelopes' shortest millisecond,
/// and a power of two for the amplifier's slope.
pub const TICK_SAMPLES: u32 = 1 << TICK_BITS;
/// Ticks a second.
pub const TICK_RATE: u32 = RATE / TICK_SAMPLES;
const _: () = assert!(TICK_RATE == 6_000 && RATE.is_multiple_of(TICK_SAMPLES) && OSCILLATOR_SAMPLES == 2);

/// The frequency step of the A above middle C at the oscillators' rate
/// under cross modulation.
const A4_STEP: u32 = frequency(law::A4.0, OSCILLATOR_RATE);
/// The same at the rate the oscillators run at without it.
const A4_STEP_PLAIN: u32 = frequency(law::A4.0, RATE);

/// Half a cycle: the square's width.
const HALF_CYCLE: u32 = 1 << 31;
/// How much wider than a square the pulse is with its slider at the top:
/// 49.215% of a cycle, which leaves 0.785% of it low.
///
/// Source: service notes, adjustment 11, PULSE WIDTH MOD LEVEL: "Press
/// C2 key, adjust ... for 30µs space width". Own reading: at 8', where
/// the key C2 is middle C, 261.63 Hz, a cycle of 3,822 µs.
const PULSE_SPAN: u32 = ((49_215u64 << 32) / 100_000) as u32;

/// The resonance at the slider's top, in Q28, as the feedback of the
/// filter's loop, which oscillates from four: the filter rings, and does
/// not oscillate.
///
/// Source: service notes, adjustment 17, RESONANCE LEVEL: with VCO-1's
/// square wave on the key A2 (E3 on later instruments) and the cutoff
/// at 10, the resonance trimmer is set "for the figure", which shows a
/// burst of ringing after each edge of the square wave dying from 8
/// divisions to 2 before the next edge; adjustment 13 turns that
/// trimmer "fully clockwise" to make "the VCFs resonate" for tuning, so
/// at its factory setting they do not. Own reading of the figure: a
/// burst of 10 to 20 cycles falling to a quarter. Own choice of the
/// figure: just short of four. Driven by a square wave as the adjustment
/// drives it, the square's own level stands in the feedback amplifier
/// and lowers its gain by a twelfth, and the ringing falls to a quarter
/// in 8.5 cycles (`tests/jupiter8_filter.rs`); a higher figure would meet
/// the drawing better and oscillate when nothing drives it. Own choice:
/// the slider is a straight line from none.
const RESONANCE_TOP: i32 = (ONE_Q28 as i64 * 395 / 100) as i32;
/// Half: the share of the resonance that is added to the filter's input
/// gain, so that the passband falls less as the resonance rises.
///
/// Source: Electric Druid, "Roland filter designs with the IR3109 or
/// AS3109": of the Jupiter-8's filter, "the slightly-later Juno 6 ...
/// uses basically the same design, except without the option of a 12dB
/// output", and of the Juno-6's, the input is also fed to the resonance
/// amplifier, so "as you increase the amount of resonance, you also add
/// more input signal". Own choice of the amount, as for this crate's
/// Juno: the resistors that set it were not read.
const COMPENSATION_SHIFT: u32 = 1;
/// The right shift of the mixer's sum into the filter: a waveform at
/// full level stands at an eighth of the level where the filter's stages
/// saturate.
///
/// Source: own choice, as for this crate's Juno, whose filter is the
/// same chip. Service notes, adjustments 12 and 18, trim an oscillator
/// to 10 V and the amplifier's output to 3 V, peak to peak; the levels
/// inside the chip between them were not worked out.
const DRIVE_SHIFT: u32 = 2;
/// The most a voice gives: eight of them add inside a word.
const VOICE_LIMIT: i32 = LIMIT >> 3;
const _: () = assert!(super::assign::VOICES == 8);
/// The right shift of the noise when it is VCO-2's waveform: half an
/// oscillator's level.
///
/// Source: service notes, adjustment 24, NOISE: "dense signal peaks are
/// approx. 5V p-p", against the oscillators' 10 V of adjustment 12.
const NOISE_SHIFT: u32 = 1;

/// Each oscillator's tuning is off by up to this, fixed for the voice:
/// 0.8 cent.
///
/// Source: own choice. The instrument's computer tunes each of the
/// sixteen oscillators (owner's manual, printed page 9), through a
/// converter whose 12 bits over five octaves are steps of 1.5 cents
/// (service notes, page 32: "DAC for KCVs is changed from 12-bit to
/// 14-bit" "to have JP-8 more stable in pitch"); what is left is taken
/// as within half such a step, and is what keeps the voices of a unison
/// from sounding as one.
pub const TUNE_TOLERANCE: i32 = law::octaves(8, 12_000);
/// Each voice's cutoff is off by up to this, fixed for the voice.
///
/// Source: own choice, as for this crate's Juno, whose service notes
/// trim the same filter "within ±10 cents".
pub const CUTOFF_TOLERANCE: i32 = law::cents(10);

/// The decay's and the release's time is the time to fall to a tenth of
/// the way: 1 / ln 10 time constants, in ten-thousandths.
///
/// Source: own choice of what the specifications' "10s" measures, as for
/// this crate's Juno: an exponential never arrives, and a tenth is what
/// is left when a trace on an oscilloscope has visibly landed.
const FALL_TIME_CONSTANT: u64 = 4_343;
/// Ten thousand, of which [`FALL_TIME_CONSTANT`] is a share.
const TEN_THOUSAND: u64 = 10_000;
/// Thousandths, of which the attack's time constant is a share of its
/// time.
const THOUSAND: u64 = 1_000;
/// The attack's length in time constants, in thousandths: the envelope
/// charges towards a level above its peak.
///
/// Source: own choice. Service notes, adjustment 20, draws the attack as
/// a curve still rising when it ends, and `AMSynths` ("Roland IR3R01
/// Design & Replica") gives the chip a reference pin "tied to ... +10V
/// (Jupiter)" without the level the attack aims at; the building block's
/// asymptote, the CEM3310's 1.3 times the peak, is used.
const ATTACK_TIME_CONSTANTS_THOUSANDTHS: u64 = RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS as u64;
const _: () = assert!(ATTACK_TIME_CONSTANTS_THOUSANDTHS > 0);

/// The left shift from a signal to the fixed point of the sine's
/// shaping, Q28.
const SHAPE_SHIFT: u32 = 28 - LEVEL.trailing_zeros();
/// The odd quintic nearest sin(π t / 2) for t from -1 to 1, in Q28: the
/// coefficients of t, t³ and t⁵. It is within 0.0001 of the sine.
///
/// Source: computed by `validation/derive_tables.py` of the reference
/// folder: least squares with the value at one held to one. The
/// instrument makes its sine from the triangle too, which it makes from
/// the sawtooth (Mod Wiggler, "An in depth look at the Roland Jupiter
/// 8's VCO": "the triangle waveform convertor").
const SINE: [i32; 3] = [421_534_070, -172_364_298, 19_265_684];

/// The sine of a triangle: `LEVEL * sin(π t / 2)` for a triangle t
/// `LEVEL`, held to its peaks.
#[inline]
fn sine(triangle: i32) -> i32 {
    let [first, third, fifth] = SINE;
    let t = triangle.clamp(-LEVEL, LEVEL) << SHAPE_SHIFT;
    // Every factor is shifted to Q30, so each upper word is in Q28.
    let square = mul_high(t << 2, t << 2);
    let inner = third.wrapping_add(mul_high(square << 2, fifth << 2));
    let outer = first.wrapping_add(mul_high(square << 2, inner << 2));
    mul_high(t << 2, outer << 2) >> SHAPE_SHIFT
}

/// The shift from a phase to a sawtooth's rise across [`LEVEL`] either
/// way.
const SAWTOOTH_SHIFT: u32 = 32 - LEVEL.trailing_zeros() - 1;

/// The triangle at a phase, as the oscillator makes it: from -[`LEVEL`]
/// at the cycle's start to [`LEVEL`] at its middle.
#[inline]
const fn triangle(phase: u32) -> i32 {
    let folded = if phase < HALF_CYCLE { phase } else { phase.wrapping_neg() };
    (folded >> (SAWTOOTH_SHIFT - 1)).cast_signed().wrapping_sub(LEVEL)
}

/// The mask that wraps a position in the corners' ring.
const RING_MASK: usize = BLEP_TAPS - 1;
const _: () = assert!(BLEP_TAPS.is_power_of_two() && Oscillator::DELAY == BLEP_TAPS / 2);
/// Bits of the weight between two rows of the corner table, the shift
/// from a fraction of a sample in Q32 to a row, and the shift to the row
/// with the weight below it: as the building block reads the table.
const BLEND_BITS: u32 = 8;
/// See [`BLEND_BITS`].
const ROW_SHIFT: u32 = 32 - BLEP_PHASES_SHIFT;
/// See [`BLEND_BITS`].
const BLEND_SHIFT: u32 = ROW_SHIFT - BLEND_BITS;
/// A change of slope gives up these bits before it meets an entry of the
/// corner table's Q17, and the product the rest: a change of 2²⁵ at most
/// times an entry below 2¹⁵ stays inside 32 bits.
const CORNER_SHIFTS: (u32, u32) = (10, 7);
/// The same for a step's height, against the step table's Q15.
const STEP_SHIFTS: (u32, u32) = (11, 4);

/// What rounds the corners that a jump of VCO-1's frequency makes in its
/// sawtooth and its triangle under cross modulation: the building
/// block's oscillator rounds the corners it makes itself, and knows
/// nothing of a frequency that jumps between two of its samples. Each
/// corner's residual is read from the building block's table and kept
/// here until the oscillator's own sample of the same instant comes out,
/// [`Oscillator::DELAY`] samples late.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Corners {
    samples: [i32; BLEP_TAPS],
    position: usize,
}

impl Corners {
    const fn new() -> Self {
        Self { samples: [0; BLEP_TAPS], position: 0 }
    }

    /// A corner: the slope changed by `change` a sample, `fraction` of a
    /// sample ago.
    #[inline]
    fn corner(&mut self, fraction: u32, change: i32) {
        self.add(&BLAMP, fraction, change >> CORNER_SHIFTS.0, CORNER_SHIFTS.1);
    }

    /// A step of `height`, `fraction` of a sample ago.
    #[inline]
    fn step(&mut self, fraction: u32, height: i32) {
        self.add(&BLEP, fraction, height >> STEP_SHIFTS.0, STEP_SHIFTS.1);
    }

    #[inline]
    fn add(&mut self, table: &[[i16; BLEP_TAPS]], fraction: u32, amount: i32, shift: u32) {
        let row = (fraction >> ROW_SHIFT) as usize;
        let weight = ((fraction >> BLEND_SHIFT) & ((1 << BLEND_BITS) - 1)).cast_signed();
        let (Some(before), Some(after)) = (table.get(row), table.get(row.wrapping_add(1))) else {
            return;
        };
        for (tap, (&before, &after)) in before.iter().zip(after).enumerate() {
            let (before, after) = (i32::from(before), i32::from(after));
            let entry = before.wrapping_add(after.wrapping_sub(before).wrapping_mul(weight) >> BLEND_BITS);
            if let Some(slot) = self.samples.get_mut(self.position.wrapping_add(tap) & RING_MASK) {
                *slot = slot.wrapping_add(amount.wrapping_mul(entry) >> shift);
            }
        }
    }

    /// Moves the sample of this instant by `amount`.
    #[inline]
    fn shift(&mut self, amount: i32) {
        if let Some(slot) = self.samples.get_mut(self.position.wrapping_add(BLEP_TAPS / 2) & RING_MASK) {
            *slot = slot.wrapping_add(amount);
        }
    }

    /// Gives what is to be added to the oscillator's sample of this turn.
    #[inline]
    fn turn(&mut self) -> i32 {
        let complete = self.samples.get_mut(self.position).map_or(0, core::mem::take);
        self.position = self.position.wrapping_add(1) & RING_MASK;
        complete
    }
}

/// An envelope's settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct EnvelopeSettings {
    /// Times in millionths of a second.
    attack: u32,
    decay: u32,
    release: u32,
    sustain: i32,
    key_follow: bool,
}

impl EnvelopeSettings {
    fn new(patch: &Patch, first: Param, sustain: Param, follow: Param) -> Self {
        let slider = |offset: u8| {
            // The four sliders of an envelope are consecutive bytes.
            patch.to_bytes().get(usize::from((first as u8).saturating_add(offset))).copied().unwrap_or(0)
        };
        Self {
            attack: law::time(&law::ATTACK_LAW, slider(0)),
            decay: law::time(&law::FALL_LAW, slider(1)),
            release: law::time(&law::FALL_LAW, slider(3)),
            sustain: law::level(patch.get(sustain)),
            key_follow: patch.on(follow),
        }
    }

    /// Sets an envelope for a key.
    fn apply(&self, envelope: &mut RcEnvelope, key: u8) {
        let followed = |time: u32| if self.key_follow { law::key_followed(time, key) } else { time };
        let share = |time: u32, numerator: u64, denominator: u64| {
            let time = u64::from(followed(time)).saturating_mul(numerator).checked_div(denominator);
            u32::try_from(time.unwrap_or(0)).unwrap_or(u32::MAX)
        };
        envelope.set_attack(time_constant(share(self.attack, THOUSAND, ATTACK_TIME_CONSTANTS_THOUSANDTHS), TICK_RATE));
        envelope.set_decay(time_constant(share(self.decay, FALL_TIME_CONSTANT, TEN_THOUSAND), TICK_RATE));
        envelope.set_release(time_constant(share(self.release, FALL_TIME_CONSTANT, TEN_THOUSAND), TICK_RATE));
        envelope.set_sustain(self.sustain);
    }
}

/// What a patch asks of every voice, worked out once when it is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::struct_excessive_bools, reason = "each is a switch of the panel, on or off")]
pub struct Settings {
    wave1: Vco1Wave,
    wave2: Vco2Wave,
    /// Octaves of each oscillator's range from 8'.
    range1: i32,
    range2: i32,
    /// VCO-2's fine tune, in octaves.
    fine: i32,
    low: bool,
    sync: bool,
    /// Whether each oscillator is heard, or is needed by the other.
    first_runs: bool,
    second_runs: bool,
    /// Octaves VCO-2 at its peak moves VCO-1.
    cross: i32,
    mix1: i32,
    mix2: i32,
    /// Octaves the low-frequency oscillator and envelope 1 at their peaks
    /// move the pitch, signed, and which oscillators they reach.
    pitch_lfo: i32,
    pitch_envelope: i32,
    modulate1: bool,
    modulate2: bool,
    bend1: bool,
    bend2: bool,
    pwm: PwmSource,
    pulse_width: i32,
    /// The high-pass filter's cutoff as a step; zero is no filter.
    hpf: u32,
    /// The low-pass filter's feedback in Q28 and the input gain that goes
    /// with it.
    resonance: i32,
    compensation: i32,
    two_pole: bool,
    /// Octaves from the law's base to the cutoff, before the key, the
    /// envelope and the oscillator.
    cutoff: i32,
    cutoff_envelope: i32,
    cutoff_from_second: bool,
    cutoff_lfo: i32,
    /// The key follow in hundredths, signed.
    key_follow: i32,
    level: i32,
    /// The share of the amplifier's level the low-frequency oscillator
    /// moves either way, in Q30, signed.
    level_lfo: i32,
    inverted: bool,
    envelopes: [EnvelopeSettings; 2],
}

/// The share of its level that the low-frequency oscillator moves the
/// amplifier either way at the switch's position 3: all of it.
///
/// Source: own choice. Owner's manual, printed page 20: "three
/// progressively deeper Levels of modulation appearing at switch
/// positions '1', '2' and '3'"; no document gives the depths. The three
/// are taken as a third, two thirds and all.
const LEVEL_LFO: i32 = ENVELOPE_ONE;
/// The most the amplifier's level is multiplied by: twice, less a unit
/// of Q30.
const SWING_MOST: i32 = i32::MAX;
/// The left shift from a signal to a level in Q30.
const LEVEL_SHIFT: u32 = 30 - LEVEL.trailing_zeros();

impl Settings {
    /// The settings of a patch.
    pub fn new(patch: &Patch) -> Self {
        let range = |position: u8| law::octaves(i32::from(position), 1).saturating_sub(OCTAVE);
        let resonance = law::share(RESONANCE_TOP, patch.get(Param::VcfResonance));
        let hpf = match patch.get(Param::HpfCutoff) {
            0 => 0,
            slider => {
                let below = law::share(law::HPF_TOP.1, law::SLIDER_MAX.saturating_sub(slider));
                frequency(law::raise(law::HPF_TOP.0, below.saturating_neg()), RATE)
            }
        };
        Self {
            wave1: patch.vco1_wave(),
            wave2: patch.vco2_wave(),
            range1: range(patch.get(Param::Vco1Range)),
            range2: law::keys(0, patch.get(Param::Vco2Range)).saturating_sub(OCTAVE),
            fine: law::signed_share(law::FINE_TUNE, patch.signed(Param::FineTune)),
            low: patch.on(Param::LowFrequency),
            sync: patch.on(Param::Sync),
            // VCO-1 starts VCO-2's cycles; VCO-2 moves VCO-1's frequency.
            first_runs: patch.get(Param::MixVco1) != 0 || patch.on(Param::Sync),
            second_runs: patch.get(Param::MixVco2) != 0 || patch.get(Param::CrossMod) != 0,
            cross: law::share(law::CROSS_MODULATION, patch.get(Param::CrossMod)),
            mix1: law::level(patch.get(Param::MixVco1)),
            mix2: law::level(patch.get(Param::MixVco2)),
            pitch_lfo: law::signed_share(law::PITCH_LFO, patch.signed(Param::VcoLfoMod)),
            pitch_envelope: law::signed_share(law::PITCH_ENVELOPE, patch.signed(Param::VcoEnvMod)),
            modulate1: patch.on(Param::Vco1Mod),
            modulate2: patch.on(Param::Vco2Mod),
            bend1: patch.on(Param::Vco1Bend),
            bend2: patch.on(Param::Vco2Bend),
            pwm: patch.pwm_source(),
            pulse_width: law::level(patch.get(Param::PwmDepth)),
            hpf,
            resonance,
            compensation: ONE_Q28.saturating_add(resonance >> COMPENSATION_SHIFT),
            two_pole: patch.on(Param::VcfSlope),
            cutoff: law::share(law::CUTOFF_SPAN, patch.get(Param::VcfCutoff)),
            cutoff_envelope: law::signed_share(law::CUTOFF_ENVELOPE, patch.signed(Param::VcfEnvMod)),
            cutoff_from_second: patch.on(Param::VcfEnvSource),
            cutoff_lfo: law::signed_share(law::CUTOFF_LFO, patch.signed(Param::VcfLfoMod)),
            key_follow: law::signed_share(law::KEY_FOLLOW_PERCENT, patch.signed(Param::VcfKeyFollow)),
            level: law::level(patch.get(Param::VcaLevel)),
            level_lfo: law::signed_share(LEVEL_LFO, patch.signed(Param::VcaLfoMod)),
            inverted: patch.on(Param::Env1Polarity),
            envelopes: [
                EnvelopeSettings::new(patch, Param::Env1Attack, Param::Env1Sustain, Param::Env1KeyFollow),
                EnvelopeSettings::new(patch, Param::Env2Attack, Param::Env2Sustain, Param::Env2KeyFollow),
            ],
        }
    }

    /// The waveforms each oscillator must make.
    const fn waveforms(&self) -> (u8, u8) {
        let first = match self.wave1 {
            Vco1Wave::Triangle => waveform::TRIANGLE,
            Vco1Wave::Sawtooth => waveform::SAWTOOTH,
            Vco1Wave::Pulse | Vco1Wave::Square => waveform::PULSE,
        };
        let second = match self.wave2 {
            Vco2Wave::Sine => waveform::TRIANGLE,
            Vco2Wave::Sawtooth => waveform::SAWTOOTH,
            Vco2Wave::Pulse => waveform::PULSE,
            Vco2Wave::Noise => 0,
        };
        (first, second)
    }
}

/// What every voice of a half is given at a tick: the instrument's
/// modulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// The low-frequency oscillator after its delay, in Q30 either way.
    pub lfo: i32,
    /// Octaves the bender moves the oscillators it reaches.
    pub bend: i32,
    /// Octaves the bender panel's oscillator moves both oscillators.
    pub vibrato: i32,
    /// Octaves the bender panel moves the cutoff.
    pub cutoff: i32,
    /// The share of the way to a new key that the pitch glides in a
    /// tick; [`IMMEDIATE`] for no portamento.
    pub glide: i32,
}

/// One of the eight voices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Voice {
    first: Oscillator,
    second: Oscillator,
    decimator: Decimator,
    corners: Corners,
    high_pass: OnePole,
    filter: OtaCascade,
    envelopes: [RcEnvelope; 2],
    key: u8,
    gate: bool,
    /// Whether the voice sounds: from its key until it has died away.
    active: bool,
    /// The key's pitch as it glides, in octaves from the A, and whether
    /// it has been anywhere yet.
    pitch: i32,
    placed: bool,
    /// VCO-1's step before VCO-2 moves it.
    step: u32,
    /// The phase VCO-1 is owed from the sample before.
    owed: u32,
    /// The pulse's width as VCO-2 was last given it.
    width: u32,
    /// The amplifier's gain in Q30 and what it gains each sample.
    gain: i32,
    gain_step: i32,
    /// The voice's own offsets, in octaves: each oscillator's tuning and
    /// the cutoff.
    tune: [i32; 2],
    trim: i32,
}

impl Voice {
    /// A voice at rest, with its tolerances.
    pub fn new(tune: [i32; 2], trim: i32) -> Self {
        Self {
            first: Oscillator::new(),
            second: Oscillator::new(),
            decimator: Decimator::wide(),
            corners: Corners::new(),
            high_pass: OnePole::new(),
            filter: OtaCascade::new(),
            envelopes: [RcEnvelope::new(), RcEnvelope::new()],
            key: law::A4.1,
            gate: false,
            active: false,
            pitch: 0,
            placed: false,
            step: 0,
            owed: 0,
            width: HALF_CYCLE,
            gain: 0,
            gain_step: 0,
            tune,
            trim,
        }
    }

    /// Takes a patch's settings.
    pub fn configure(&mut self, settings: &Settings) {
        let (first, second) = settings.waveforms();
        self.first.enable(first);
        self.second.enable(second);
        self.high_pass.set_cutoff(settings.hpf);
        if settings.hpf == 0 {
            self.high_pass.reset();
        }
        self.owed = 0;
        if settings.cross == 0 {
            self.corners = Corners::new();
        }
        self.set_envelopes(settings);
    }

    fn set_envelopes(&mut self, settings: &Settings) {
        for (envelope, times) in self.envelopes.iter_mut().zip(&settings.envelopes) {
            times.apply(envelope, self.key);
        }
    }

    /// A key for this voice. With `trigger` the envelopes start again;
    /// without, the voice slides to the key.
    pub fn start(&mut self, key: u8, trigger: bool, settings: &Settings) {
        self.key = key;
        self.gate = true;
        self.active = true;
        self.set_envelopes(settings);
        if trigger {
            for envelope in &mut self.envelopes {
                envelope.retrigger();
            }
        }
    }

    /// The key is up. A damped voice dies away at once.
    pub fn release(&mut self, damped: bool) {
        self.gate = false;
        if damped {
            // The shortest release the sliders have.
            let shortest = u64::from(law::TIME_BASE).saturating_mul(FALL_TIME_CONSTANT) / TEN_THOUSAND;
            let coefficient = time_constant(u32::try_from(shortest).unwrap_or(0), TICK_RATE);
            for envelope in &mut self.envelopes {
                envelope.set_release(coefficient);
            }
        }
    }

    /// Silences the voice at once, keeping its tolerances and where its
    /// oscillators are in their cycles.
    pub fn cut(&mut self, settings: &Settings) {
        let (first, second) = (self.first.clone(), self.second.clone());
        *self = Self::new(self.tune, self.trim);
        self.first = first;
        self.second = second;
        self.configure(settings);
    }

    /// Whether the voice sounds.
    pub const fn active(&self) -> bool {
        self.active
    }

    /// A tick: the envelopes move on, and the pitches, the pulse's
    /// width, the cutoff and the amplifier's slope are set.
    pub fn tick(&mut self, settings: &Settings, controls: Controls) {
        let [first, second] = &mut self.envelopes;
        first.gate(self.gate);
        second.gate(self.gate);
        let (first, second) = (first.tick(), second.tick());
        // The first envelope upside down runs from zero downwards
        // (service notes, module board schematic, the envelope's test
        // point: "NOR 0 to +10V, INV 0 to -10V").
        let first = if settings.inverted { first.saturating_neg() } else { first };
        let lfo = controls.lfo;

        let target = law::keys(law::A4.1, self.key);
        let moved = scale(target.saturating_sub(self.pitch), controls.glide);
        self.pitch = if controls.glide == IMMEDIATE || !self.placed || moved == 0 {
            target
        } else {
            self.pitch.saturating_add(moved)
        };
        self.placed = true;

        let modulation = depth(settings.pitch_lfo, lfo).saturating_add(depth(settings.pitch_envelope, first));
        let moved = |modulated: bool, bent: bool| {
            controls.vibrato.saturating_add(if modulated { modulation } else { 0 }).saturating_add(if bent {
                controls.bend
            } else {
                0
            })
        };
        let [tune_first, tune_second] = self.tune;
        let pitch = self.pitch.saturating_add(settings.range1).saturating_add(tune_first);
        // The oscillators' rate is the filters' when nothing modulates
        // VCO-1 at audio rate.
        let a4 = if settings.cross == 0 { A4_STEP_PLAIN } else { A4_STEP };
        self.step = law::raise_step(a4, pitch.saturating_add(moved(settings.modulate1, settings.bend1)));
        if settings.cross == 0 {
            self.first.set_step(self.step);
        }
        // In its low range VCO-2 leaves the keyboard.
        let own =
            if settings.low { law::keys(law::A4.1, law::MIDDLE_C).saturating_sub(law::LOW_RANGE) } else { self.pitch };
        let pitch = own.saturating_add(settings.range2).saturating_add(settings.fine).saturating_add(tune_second);
        self.second.set_step(law::raise_step(a4, pitch.saturating_add(moved(settings.modulate2, settings.bend2))));

        let position = match settings.pwm {
            PwmSource::Manual => settings.pulse_width,
            // The oscillator from the square to the slider's width and
            // back (owner's manual, printed page 17: "pivoting from the
            // basic Square wave ... to the extreme limit set by the
            // slider and back again").
            PwmSource::Lfo => depth(settings.pulse_width, (ENVELOPE_ONE >> 1).saturating_add(lfo >> 1)),
            PwmSource::Envelope => depth(settings.pulse_width, first.saturating_abs()),
        };
        // The position, one at most in Q30, as a share of a word.
        let share = position.clamp(0, ENVELOPE_ONE - 1).cast_unsigned() << 2;
        let width = HALF_CYCLE.saturating_add(mul_high_unsigned(PULSE_SPAN, share));
        self.first.set_width(if settings.wave1 == Vco1Wave::Square { HALF_CYCLE } else { width });
        self.second.set_width(width);
        self.width = width;

        let keyboard = law::share_of(law::keys(law::MIDDLE_C, self.key), settings.key_follow, 100);
        let envelope = if settings.cutoff_from_second { second } else { first };
        let octaves = settings
            .cutoff
            .saturating_add(self.trim)
            .saturating_add(keyboard)
            .saturating_add(depth(settings.cutoff_envelope, envelope))
            .saturating_add(depth(settings.cutoff_lfo, lfo))
            .saturating_add(controls.cutoff);
        let hertz = law::raise(law::CUTOFF_BASE, octaves).clamp(law::CUTOFF_BOUNDS.0, law::CUTOFF_BOUNDS.1);
        self.filter.set(frequency(hertz, RATE), settings.resonance);

        // The amplifier follows envelope 2 (owner's manual, printed page
        // 20), and the low-frequency oscillator moves its level about
        // where the slider put it.
        let swing = ENVELOPE_ONE.saturating_add(depth(settings.level_lfo, lfo)).clamp(0, SWING_MOST);
        let level = depth(depth(second, settings.level), swing >> 1) << 1;
        self.gain_step = level.saturating_sub(self.gain) >> TICK_BITS;

        let at_rest = self.envelopes.iter().all(|envelope| envelope.stage() == Stage::Rest);
        if !self.gate && at_rest && self.gain < REST_LEVEL {
            self.active = false;
            self.gain = 0;
            self.gain_step = 0;
        }
    }

    /// VCO-1's step under VCO-2's waveform at a level.
    #[inline]
    fn crossed(&self, settings: &Settings, modulator: i32) -> u32 {
        // Octaves in Q24 from a depth in Q24 and a signal whose peak is
        // 2²⁴.
        let octaves = mul_high(settings.cross << 4, modulator.clamp(-LEVEL, LEVEL) << 4);
        law::raise_step(self.step, octaves)
    }

    /// VCO-1's step averaged over a stretch of VCO-2's cycle in which
    /// VCO-2 does not start again, from VCO-2's phase at either end. It
    /// is the frequencies that are averaged, not the waveform: either
    /// side of a pulse's edge VCO-1 runs at one of two frequencies, each
    /// for its share of the stretch.
    #[inline]
    fn stretch(&self, settings: &Settings, from: u32, to: u32, noise: i32) -> u32 {
        let middle = from.wrapping_add(to.wrapping_sub(from) >> 1);
        match settings.wave2 {
            Vco2Wave::Sine => self.crossed(settings, sine(triangle(middle))),
            Vco2Wave::Sawtooth => self.crossed(settings, (middle >> SAWTOOTH_SHIFT).cast_signed().wrapping_sub(LEVEL)),
            Vco2Wave::Pulse => {
                // High from the cycle's start to the width.
                let high = self.width.min(to).saturating_sub(from);
                let share = if to <= from {
                    u32::from(from < self.width).wrapping_neg()
                } else {
                    ratio(high, to.wrapping_sub(from))
                };
                blend(self.crossed(settings, -LEVEL), self.crossed(settings, LEVEL), share)
            }
            Vco2Wave::Noise => self.crossed(settings, noise),
        }
    }

    /// VCO-1's step for the sample to come: its own, moved by VCO-2's
    /// waveform over that sample as VCO-2 will run it if nothing starts
    /// its cycle again.
    #[inline]
    fn cross_modulated(&self, settings: &Settings, noise: i32) -> u32 {
        let (old, step) = (self.second.phase(), self.second.step());
        let new = old.wrapping_add(step);
        if new < old {
            // The cycle starts again by itself: each side of the start
            // for its share of the sample.
            let before = self.stretch(settings, old, u32::MAX, noise);
            blend(before, self.stretch(settings, 0, new, noise), ratio(new, step))
        } else {
            self.stretch(settings, old, new, noise)
        }
    }

    /// VCO-2's waveform at a phase, as it moves VCO-1; none for the
    /// noise, which has no phase.
    #[inline]
    fn modulator(&self, settings: &Settings, phase: u32) -> Option<i32> {
        match settings.wave2 {
            Vco2Wave::Sine => Some(sine(triangle(phase))),
            Vco2Wave::Sawtooth => Some((phase >> SAWTOOTH_SHIFT).cast_signed().wrapping_sub(LEVEL)),
            Vco2Wave::Pulse => Some(if phase < self.width { LEVEL } else { -LEVEL }),
            Vco2Wave::Noise => None,
        }
    }

    /// The slope of VCO-1's waveform for a frequency step, signed, where
    /// its phase now is; none for the pulse, which has no slope.
    #[inline]
    const fn slope(&self, settings: &Settings, step: i32) -> Option<i32> {
        match settings.wave1 {
            Vco1Wave::Sawtooth => Some(step >> SAWTOOTH_SHIFT),
            Vco1Wave::Triangle => {
                let slope = step >> (SAWTOOTH_SHIFT - 1);
                Some(if self.first.phase() < HALF_CYCLE { slope } else { slope.wrapping_neg() })
            }
            Vco1Wave::Pulse | Vco1Wave::Square => None,
        }
    }

    /// Rounds the corners that the edges of VCO-2's own cycle made in
    /// VCO-1's waveform in the sample just run: where a sawtooth of VCO-2
    /// started again and where its pulse rose and fell, VCO-1's frequency
    /// jumped, and its waveform turned a corner. `old` and `step` are
    /// VCO-2's phase and step before the sample, and `floor` is how long
    /// ago VCO-1 started VCO-2 again, before which alone VCO-2's own
    /// edges happened.
    #[inline]
    fn round_edges(&mut self, settings: &Settings, old: u32, step: u32, floor: u32) {
        let pulse = match settings.wave2 {
            Vco2Wave::Sawtooth => false,
            Vco2Wave::Pulse => true,
            Vco2Wave::Sine | Vco2Wave::Noise => return,
        };
        let new = old.wrapping_add(step);
        let wrapped = new < old;
        let width = self.width;
        // Whether the pulse turned down in the sample.
        let down = pulse && old < width && (wrapped || new >= width);
        if !wrapped && !down {
            return;
        }
        // VCO-1's step with VCO-2's waveform at its top, less at its
        // bottom: the pulse jumps up by it where its cycle starts and
        // down where it turns, and the sawtooth down where it starts.
        let jump =
            self.crossed(settings, LEVEL).cast_signed().wrapping_sub(self.crossed(settings, -LEVEL).cast_signed());
        if down {
            let fraction = ratio(new.wrapping_sub(width), step);
            if fraction >= floor
                && let Some(change) = self.slope(settings, jump.wrapping_neg())
            {
                self.corners.corner(fraction, change);
            }
        }
        if wrapped {
            let fraction = ratio(new, step);
            if fraction >= floor
                && let Some(change) = self.slope(settings, if pulse { jump } else { jump.wrapping_neg() })
            {
                self.corners.corner(fraction, change);
            }
        }
    }

    /// Where an edge of VCO-1's cycle fell in the sample just run, as
    /// how long ago, with VCO-1's frequency taken to move in a straight
    /// line through the sample as VCO-2's waveform moved it; `since` is
    /// where the oscillator put it, which takes the sample's mean
    /// throughout. `old` and `step` are VCO-2's phase and step before the
    /// sample, and `stepped` VCO-1's step in it.
    ///
    /// With the frequency f₀ at the sample's start and f₁ at its end, an
    /// edge that the mean would put a share s of the sample ago falls
    /// (f₁ - f₀) / 2 mean × s (1 - s) later than that. Where VCO-2's
    /// waveform jumps within the sample the line is no better than the
    /// mean, and the edge is left where it was put.
    #[inline]
    fn placed(&self, settings: &Settings, old: u32, step: u32, stepped: u32, since: u32) -> u32 {
        let new = old.wrapping_add(step);
        let smooth = match settings.wave2 {
            Vco2Wave::Sine => true,
            Vco2Wave::Sawtooth => new >= old,
            Vco2Wave::Pulse | Vco2Wave::Noise => false,
        };
        let (Some(from), Some(to)) = (self.modulator(settings, old), self.modulator(settings, new)) else {
            return since;
        };
        if !smooth {
            return since;
        }
        let (from, to) = (self.crossed(settings, from), self.crossed(settings, to));
        let later = mul_high_unsigned(ratio(from.abs_diff(to) >> 1, stepped), mul_high_unsigned(since, !since));
        if to > from { since.saturating_sub(later) } else { since.saturating_add(later) }
    }

    /// Moves the edges of VCO-1's waveform in the sample just run from
    /// where the oscillator put them to where [`Voice::placed`] has
    /// them: the start of its cycle, and the pulse's fall or the
    /// triangle's turn. `before` is VCO-1's phase before the sample.
    /// Gives how long ago the cycle started, if it did.
    #[inline]
    fn place_edges(&mut self, settings: &Settings, before: u32, stepped: u32, old: u32, step: u32) -> Option<u32> {
        /// A step of the pulse, and of the sawtooth.
        const STEP: i32 = 2 * LEVEL;
        // The change of the triangle's slope where it turns.
        let turn = (stepped >> (SAWTOOTH_SHIFT - 2)).cast_signed();
        let middle = match settings.wave1 {
            Vco1Wave::Sawtooth => None,
            Vco1Wave::Triangle | Vco1Wave::Square => Some(HALF_CYCLE),
            Vco1Wave::Pulse => Some(self.width),
        };
        // As the oscillator finds a crossing: the phase passed the mark.
        if let Some(mark) = middle
            && mark.wrapping_sub(before).wrapping_sub(1) < stepped
        {
            let since = ratio(before.wrapping_add(stepped).wrapping_sub(mark), stepped);
            let placed = self.placed(settings, old, step, stepped, since);
            if settings.wave1 == Vco1Wave::Triangle {
                self.corners.corner(placed, turn.wrapping_neg());
                self.corners.corner(since, turn);
            } else {
                self.corners.step(placed, -STEP);
                self.corners.step(since, STEP);
            }
        }
        let since = self.first.started()?;
        let placed = self.placed(settings, old, step, stepped, since);
        match settings.wave1 {
            Vco1Wave::Sawtooth => {
                self.corners.step(placed, -STEP);
                self.corners.step(since, STEP);
            }
            Vco1Wave::Triangle => {
                self.corners.corner(placed, turn);
                self.corners.corner(since, turn.wrapping_neg());
            }
            Vco1Wave::Pulse | Vco1Wave::Square => {
                self.corners.step(placed, STEP);
                self.corners.step(since, -STEP);
            }
        }
        Some(placed)
    }

    /// One sample of the two oscillators, mixed, at the rate they are
    /// run at: four times the output rate under cross modulation, and
    /// twice without. An oscillator that is not heard and that the other
    /// does not need stands still.
    #[inline]
    fn oscillators(&mut self, settings: &Settings, noise: i32) -> i32 {
        let crossing = settings.cross != 0;
        let stepped = if crossing {
            // What the sample before owes is paid in this one.
            self.first.set_step(self.cross_modulated(settings, noise).saturating_add(self.owed));
            self.owed = 0;
            // As the oscillator holds it to its bound.
            self.first.step()
        } else {
            self.step
        };
        let before = self.first.phase();
        let mut first = if settings.first_runs {
            let waves = self.first.tick(None);
            match settings.wave1 {
                Vco1Wave::Triangle => waves.triangle,
                Vco1Wave::Sawtooth => waves.sawtooth,
                Vco1Wave::Pulse | Vco1Wave::Square => waves.pulse,
            }
        } else {
            0
        };
        if !settings.second_runs {
            return apply(first, settings.mix1);
        }
        // VCO-1 starts VCO-2's cycle (owner's manual, printed page 16:
        // "forces the pitch of that oscillator to synchronize itself to
        // that of VCO-1").
        let (old, step) = (self.second.phase(), self.second.step());
        let started = if crossing && settings.first_runs {
            self.place_edges(settings, before, stepped, old, step)
        } else {
            self.first.started()
        };
        let sync = if settings.sync { started } else { None };
        let waves = self.second.tick(sync);
        let second = match settings.wave2 {
            Vco2Wave::Sine => sine(waves.triangle),
            Vco2Wave::Sawtooth => waves.sawtooth,
            Vco2Wave::Pulse => waves.pulse,
            Vco2Wave::Noise => noise,
        };
        if crossing && settings.first_runs {
            self.round_edges(settings, old, step, sync.unwrap_or(0));
        }
        if crossing && let Some(since) = sync {
            // VCO-2 started again when VCO-1 did, `since` of a sample
            // ago, and VCO-1 has run since then at the frequency of the
            // cycle that ended, not at the one VCO-2's new cycle gives
            // it. What it ran too far is taken back at once; what it ran
            // too little is owed to the next sample. Either way the
            // sample just made stood where VCO-1 had run to, and is
            // moved to where it should have.
            let after = self.stretch(settings, 0, self.second.phase(), noise);
            let (ran, due) = (self.first.phase(), mul_high_unsigned(after, since));
            let moved = if due < ran {
                self.first.set_back(ran.wrapping_sub(due));
                ran.wrapping_sub(due).cast_signed().wrapping_neg()
            } else {
                self.owed = due.wrapping_sub(ran);
                self.owed.cast_signed()
            };
            if let Some(shift) = self.slope(settings, moved) {
                self.corners.shift(shift);
            }
            // And VCO-1's frequency jumped where its own cycle started:
            // from what VCO-2's waveform made it where that was cut
            // short, to what the start of VCO-2's cycle makes it.
            let reached = old.wrapping_add(step).wrapping_sub(mul_high_unsigned(step, since));
            if let (Some(before), Some(start)) = (self.modulator(settings, reached), self.modulator(settings, 0)) {
                let before = self.crossed(settings, before).cast_signed();
                let start = self.crossed(settings, start).cast_signed();
                let change = match settings.wave1 {
                    Vco1Wave::Sawtooth => start.wrapping_sub(before) >> SAWTOOTH_SHIFT,
                    // The oscillator rounded this corner as twice the
                    // slope it was given: it is the two slopes' sum.
                    Vco1Wave::Triangle => (start >> (SAWTOOTH_SHIFT - 1))
                        .wrapping_add(before >> (SAWTOOTH_SHIFT - 1))
                        .wrapping_sub((stepped >> (SAWTOOTH_SHIFT - 2)).cast_signed()),
                    Vco1Wave::Pulse | Vco1Wave::Square => 0,
                };
                self.corners.corner(since, change);
            }
        }
        if crossing && settings.first_runs {
            first = first.wrapping_add(self.corners.turn());
        }
        apply(first, settings.mix1).wrapping_add(apply(second, settings.mix2))
    }

    /// One sample at the filters' rate. `noise` holds the noise of the
    /// voice's half for each sample of the oscillators.
    #[inline]
    pub fn sample(&mut self, settings: &Settings, noise: [i32; OSCILLATOR_SAMPLES]) -> i32 {
        let mut signal = if settings.cross == 0 {
            let [noise, _] = noise;
            self.oscillators(settings, noise >> NOISE_SHIFT)
        } else {
            let [earlier, later] = noise.map(|noise| self.oscillators(settings, noise >> NOISE_SHIFT));
            self.decimator.process(earlier, later)
        };
        if settings.hpf != 0 {
            signal = self.high_pass.high_pass(signal);
        }
        let slopes = self.filter.tick(amplify(signal, settings.compensation) >> DRIVE_SHIFT);
        let filtered = if settings.two_pole { slopes.two_pole } else { slopes.four_pole };
        self.gain = self.gain.wrapping_add(self.gain_step);
        apply(filtered, self.gain).clamp(-VOICE_LIMIT, VOICE_LIMIT)
    }
}

/// Two steps mixed: `second` for a share of a word, `first` for the rest.
#[inline]
const fn blend(first: u32, second: u32, share: u32) -> u32 {
    mul_high_unsigned(first, !share).wrapping_add(mul_high_unsigned(second, share))
}

/// `amount * level` for a level in Q30 either way: a modulation's depth.
#[inline]
const fn depth(amount: i32, level: i32) -> i32 {
    mul_high(amount, level) << 2
}

/// The seed a random level takes its noise from, whichever half.
///
/// Source: own choice: any number; the same instrument every time.
const LFO_SEED: u32 = 0x4a50_3831;
/// The fade's full level, one in Q31 less a unit.
const FADE_FULL: i32 = i32::MAX;
/// The share of the delay that the rise to full depth takes after it:
/// half as long again.
///
/// Source: own choice, as for this crate's Juno. Service notes,
/// adjustment 21, says only when the modulation begins.
const FADE_SHARE: u32 = 2;
/// Millionths of a second in a second.
const MICROSECONDS: u64 = 1_000_000;

/// The low-frequency oscillator of one half, ticked at [`TICK_RATE`]:
/// a sine, a falling sawtooth, a square or a random level, with the
/// delay after a key.
///
/// Source: owner's manual, printed page 17: "a Sine Wave, inverted
/// Sawtooth wave, Square wave and a Random output from the Sample & Hold
/// circuit"; service notes, adjustment 21, draws the sawtooth falling.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Lfo {
    phase: u32,
    step: u32,
    wave: LfoWave,
    noise: Noise,
    held: i32,
    /// Ticks of silence after a key, and those left.
    delay: u32,
    waiting: u32,
    /// The fade's rise each tick, and its level, in Q31.
    fade_step: i32,
    fade: i32,
}

impl Lfo {
    /// An oscillator at rest for the half numbered `half`.
    pub const fn new(half: u32) -> Self {
        Self {
            phase: 0,
            step: 0,
            wave: LfoWave::Sine,
            noise: Noise::new(LFO_SEED.wrapping_add(half)),
            held: 0,
            delay: 0,
            waiting: 0,
            fade_step: FADE_FULL,
            fade: FADE_FULL,
        }
    }

    /// Takes a patch's rate, waveform and delay.
    pub fn configure(&mut self, patch: &Patch) {
        let rate = law::raise(law::LFO_RATE_BASE, law::share(law::LFO_RATE_SPAN, patch.get(Param::LfoRate)));
        self.step = frequency(rate, TICK_RATE);
        self.wave = patch.lfo_wave();
        let delay = law::share(i32::try_from(law::LFO_DELAY).unwrap_or(i32::MAX), patch.get(Param::LfoDelay));
        let ticks = u64::from(delay.cast_unsigned()).saturating_mul(u64::from(TICK_RATE)) / MICROSECONDS;
        self.delay = u32::try_from(ticks).unwrap_or(u32::MAX);
        self.fade_step = match (FADE_FULL.cast_unsigned()).checked_div(self.delay / FADE_SHARE) {
            Some(step) => step.cast_signed(),
            None => FADE_FULL,
        };
        if self.delay == 0 {
            self.waiting = 0;
            self.fade = FADE_FULL;
        }
    }

    /// A key with no other held: the delay starts again. The cycle goes
    /// on where it was.
    pub const fn key(&mut self) {
        if self.delay > 0 {
            self.waiting = self.delay;
            self.fade = 0;
        }
    }

    /// One tick: the waveform at full depth, and at the depth the delay
    /// has let it reach; each in Q30 either way.
    pub fn tick(&mut self) -> (i32, i32) {
        let full = match self.wave {
            // The sine of a triangle that starts at zero, rising.
            LfoWave::Sine => sine(triangle(self.phase.wrapping_add(HALF_CYCLE >> 1))) << LEVEL_SHIFT,
            LfoWave::Sawtooth => (!self.phase >> 1).cast_signed().wrapping_sub(ENVELOPE_ONE),
            LfoWave::Square => {
                if self.phase < HALF_CYCLE {
                    ENVELOPE_ONE
                } else {
                    -ENVELOPE_ONE
                }
            }
            LfoWave::Random => self.held,
        };
        let delayed = if self.fade == FADE_FULL { full } else { scale(full, self.fade) };
        if self.waiting > 0 {
            self.waiting = self.waiting.wrapping_sub(1);
        } else if self.fade != FADE_FULL {
            self.fade = self.fade.saturating_add(self.fade_step);
        }
        let next = self.phase.wrapping_add(self.step);
        if next < self.phase {
            self.held = self.noise.tick_level();
        }
        self.phase = next;
        (full, delayed)
    }
}
