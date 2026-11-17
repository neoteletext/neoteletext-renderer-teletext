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
//! VCO-2's waveform moves VCO-1's frequency exponentially, by its own
//! voltage as it stands: ten volts are six octaves with the slider at 10
//! ([`law::CROSS_MODULATION`]), so the sawtooth and the pulse, which
//! run from nothing to ten volts, move VCO-1 from its own pitch to six
//! octaves above, the sine three octaves either way and the noise one
//! and a half ([`Voice::crossed_at`]). VCO-1's cycle starts VCO-2's
//! again when the sync is on: with both, each oscillator steers the
//! other. Here:
//!
//! - Each sample of VCO-1, 5.2 µs, is made in stretches
//!   ([`Oscillator::run_to_start`]): a stretch ends where VCO-2's
//!   waveform jumps (a sawtooth starting again, a pulse rising or
//!   falling) and where VCO-1's own cycle starts. VCO-1's frequency at
//!   each end of a stretch is its own times two to the power of VCO-2's
//!   waveform there, taken from VCO-2's phase and not from its
//!   band-limited output, and between the ends it moves as an
//!   exponential converter moves it. The building block's oscillator
//!   puts every edge of VCO-1 where that moving frequency does, and
//!   rounds the corner that each jump of the frequency makes in VCO-1's
//!   sawtooth and triangle.
//! - When VCO-1's cycle starts VCO-2's again, the stretch ends there,
//!   and the stretches after it take VCO-2's waveform from its new
//!   cycle: nothing of VCO-1 runs at a frequency that the start has
//!   already changed.
//! - VCO-1's frequency is held to a quarter of the oscillators' rate,
//!   48 kHz ([`STEP_LIMIT`](crate::analog::STEP_LIMIT)), so it is bounded
//!   whatever the two do to each other.
//!
//! What this leaves: exponential modulation at audio rate has partials
//! without end, of which those beyond 172 kHz fold back below 20 kHz;
//! VCO-2's sine is taken as a straight line of control voltage through
//! each stretch; and VCO-1's pulse width is set at each tick.
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
//! | 1 (0.6 octave for ten volts) | 99 dB down | 95 dB | 89 dB |
//! | 2.5 (1.5 octaves) | 92 dB | 92 dB | 86 dB |
//! | 5 (3 octaves) | 93 dB | 87 dB | 80 dB |
//! | 10 (6 octaves) | 90 dB | 84 dB | 77 dB |
//!
//! With one frequency for each sample and nothing rounded, and every
//! waveform swinging the whole depth either way as it was then taken
//! to, the slider at 5 left 78 dB, 70 dB and 56 dB, and at 10 56 dB,
//! 43 dB and 32 dB. With VCO-1's triangle the slider to 5 and keys to
//! the A of 880 Hz leave 92 dB or better, and with its square 86 dB. At
//! 10 on the highest A, VCO-2's sawtooth drives VCO-1 to its bound of
//! 48 kHz for part of every cycle: the instrument's own oscillator has
//! a bound there too, which no document read gives.
//!
//! These figures are of the whole voice, and the filter after the
//! oscillators bends what it is given: measured before the filter, the
//! two oscillators leave 86 dB or better at every position of the
//! slider on every one of these keys, for each of VCO-1's and VCO-2's
//! waveforms, and 92 dB, which is as low as that measurement reads, on
//! all but the highest (the test in this file).
//!
//! With VCO-2's pulse and the sync, VCO-1 at the slider's 5 is a
//! sawtooth three octaves above its key and nothing else, the pulse
//! never falling before VCO-1 starts it again: what is measured there is
//! a plain sawtooth at 3.5 kHz, 7 kHz and 14 kHz through the voice, 84 dB,
//! 77 dB and 68 dB; a key of 3.5 kHz or 7 kHz with no cross modulation
//! measures the same. It is the filter's doing, not the oscillator's:
//! the building block's sawtooth through its decimator leaves 100 dB to
//! 12.5 kHz (`tests/analog_oscillator.rs`), and the filter by itself,
//! its cutoff at the slider's 20 kHz, folds back 85 dB, 78 dB and 70 dB
//! of the same three sawtooths (`tests/analog_filter_aliasing.rs`).
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
//! and a moving pulse, and 7.7 times under cross modulation with sync
//! (8.8 with one frequency for each sample and nothing rounded), on one
//! core of a 3.7 GHz Intel Core i9-10900KF
//! (`tests/jupiter8_instrument.rs`, `rendering_speed`, 17 November
//! 2026).

use crate::analog::decimator::Decimator;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, REST_LEVEL, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{ONE_Q28, OnePole, OtaCascade};
use crate::analog::fixed::{amplify, mul_high, mul_high_unsigned, ratio, scale};
use crate::analog::oscillator::{Noise, Oscillator, Waves, waveform};
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

/// Stretches a sample of VCO-1 is made in under cross modulation, at
/// most: VCO-2's pulse rises and falls, VCO-1 starts its own cycle and
/// VCO-2's, the pulse falls again, and the sample ends; one to spare.
const STRETCHES: u32 = 6;

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
            key_follow: law::key_follow(patch.signed(Param::VcfKeyFollow)),
            level: law::level(patch.get(Param::VcaLevel)),
            level_lfo: law::signed_share(LEVEL_LFO, patch.signed(Param::VcaLfoMod)),
            inverted: patch.on(Param::Env1Polarity),
            envelopes: [
                EnvelopeSettings::new(patch, Param::Env1Attack, Param::Env1Sustain, Param::Env1KeyFollow),
                EnvelopeSettings::new(patch, Param::Env2Attack, Param::Env2Sustain, Param::Env2KeyFollow),
            ],
        }
    }

    /// How far above the output rate the oscillators run.
    const fn oscillators_at(&self) -> Oversampling {
        if self.cross == 0 { OVERSAMPLING } else { OSCILLATOR_OVERSAMPLING }
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
    /// VCO-1's step as VCO-2's waveform left it at the end of the last
    /// sample, while nothing but VCO-2's cycle has moved it since.
    carried: Option<u32>,
    /// How far above the output rate the oscillators run: what cross
    /// modulation asks, or the filters' rate.
    oscillators_at: Oversampling,
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
            high_pass: OnePole::new(),
            filter: OtaCascade::new(),
            envelopes: [RcEnvelope::new(), RcEnvelope::new()],
            key: law::A4.1,
            gate: false,
            active: false,
            pitch: 0,
            placed: false,
            step: 0,
            carried: None,
            oscillators_at: OVERSAMPLING,
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
        if self.oscillators_at != settings.oscillators_at() {
            self.change_rate(settings);
        }
        self.high_pass.set_cutoff(settings.hpf);
        if settings.hpf == 0 {
            self.high_pass.reset();
        }
        self.carried = None;
        self.set_envelopes(settings);
    }

    /// Takes the oscillators from the rate they run at without cross
    /// modulation to the rate they run at with it, or back, so that a
    /// note that is sounding goes on where it was.
    ///
    /// Their steps are for the old rate until the next tick, and are
    /// doubled or halved here. What they have on its way out is spaced
    /// for the old rate, and the decimator's samples are of an earlier
    /// note or of none: both are forgotten, each oscillator is set back
    /// by what the forgetting and the decimator's delay cost, and the
    /// two are run unheard until what comes next follows the last
    /// sample heard.
    fn change_rate(&mut self, settings: &Settings) {
        /// Samples by which an oscillator's output is behind its phase.
        const DELAY: u32 = Oscillator::DELAY as u32;
        /// Samples the oscillators run unheard after they are set back:
        /// those that are of nothing, and as many again, in which an
        /// edge from before would still have been dying away.
        const WARM: u32 = 2 * DELAY;
        let crossing = settings.cross != 0;
        self.oscillators_at = settings.oscillators_at();
        self.carried = None;
        self.decimator = Decimator::wide();
        // Samples of the oscillators' higher rate by which the wide
        // decimator's output is behind its newest input.
        let behind = u32::try_from(self.decimator.delay()).unwrap_or(0);
        // To the higher rate: samples to fill the decimator either side
        // of its first output's centre, unheard too.
        let fill = if crossing { behind.saturating_mul(2) } else { 0 };
        // All in steps of the higher rate, of which the lower rate's
        // step is two. How far behind its phase the last sample heard
        // was: at the lower rate the oscillator's delay; at the higher,
        // that and the decimator's.
        let heard = if crossing { DELAY.saturating_mul(2) } else { DELAY.saturating_add(behind) };
        // How far past the phase set here the first sample heard will
        // be. To the higher rate it is the decimator's output for the
        // first pair after the unheard samples, centred `behind` before
        // the pair's second; to the lower, the sample after the unheard
        // ones.
        let unheard = WARM.saturating_sub(DELAY);
        let ahead = if crossing {
            unheard.saturating_add(fill).saturating_add(2).saturating_sub(behind)
        } else {
            unheard.saturating_add(1).saturating_mul(2)
        };
        // The first sample heard is to follow the last by one sample of
        // the rate heard, two steps.
        let back = ahead.saturating_add(heard).saturating_sub(2);
        let rescale = |step: u32| if crossing { step >> 1 } else { step.saturating_mul(2) };
        // VCO-1's own step is the voice's: under cross modulation the
        // oscillator is given a step for each stretch and keeps none.
        let second = self.second.step();
        for (oscillator, old) in [(&mut self.first, self.step), (&mut self.second, second)] {
            let higher = if crossing { old >> 1 } else { old };
            oscillator.rewind(higher.wrapping_mul(back));
            oscillator.set_step(rescale(old));
        }
        self.step = rescale(self.step);
        for _ in 0..WARM {
            let _ = self.oscillators(settings, 0);
        }
        for _ in 0..fill / 2 {
            let earlier = self.oscillators(settings, 0);
            let later = self.oscillators(settings, 0);
            let _ = self.decimator.process(earlier, later);
        }
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
        // What a tick sets may move VCO-1's step under VCO-2's waveform.
        self.carried = None;
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

    /// VCO-1's step under a voltage of VCO-2's waveform, as a share of
    /// the ten volts that move it [`law::CROSS_MODULATION`] with the
    /// slider at 10, [`LEVEL`] being the ten.
    #[inline]
    fn crossed(&self, settings: &Settings, voltage: i32) -> u32 {
        // Octaves in Q24 from a depth in Q24 and a share whose one is
        // 2²⁴.
        let octaves = mul_high(settings.cross << 4, voltage.clamp(-LEVEL, LEVEL) << 4);
        law::raise_step(self.step, octaves)
    }

    /// VCO-1's step with VCO-2 at a phase of its cycle, or under the
    /// noise, which has no phase.
    ///
    /// Source: service notes, module board schematic (page 15 of the
    /// file): VCO-2's waveform selector IC14 feeds the X-MOD amplifier
    /// IC18A through R102A, 100 kΩ, with no capacitor, so each waveform
    /// moves VCO-1 by its voltage as it stands. The waveforms are drawn
    /// there with their levels: the sine "+5v" to "-5v", the sawtooth
    /// and the pulse "10v" to "0v"; adjustment 24 sets the noise to
    /// "approx. 5V p-p". Adjustment 10 makes the pulse's ten volts three
    /// octaves with CROSS MOD at 5. So the sine moves VCO-1 half the
    /// depth either way, the sawtooth and the pulse from nothing to the
    /// whole depth upwards, and the noise a quarter either way. Own
    /// reading: the noise is taken as centred on zero.
    #[inline]
    fn crossed_at(&self, settings: &Settings, phase: u32, noise: i32) -> u32 {
        let voltage = match settings.wave2 {
            Vco2Wave::Sine => sine(triangle(phase)) >> 1,
            Vco2Wave::Sawtooth => (phase >> (SAWTOOTH_SHIFT + 1)).cast_signed(),
            Vco2Wave::Pulse => {
                if phase < self.width {
                    LEVEL
                } else {
                    0
                }
            }
            // The noise comes at the level it has in the mixer, half an
            // oscillator's swing of ten volts.
            Vco2Wave::Noise => noise >> 1,
        };
        self.crossed(settings, voltage)
    }

    /// What VCO-2 runs from a phase of its cycle to where its waveform
    /// next jumps: the start of a sawtooth's or a pulse's cycle, and
    /// where a pulse falls. Nothing for a sine and for the noise, which
    /// jump nowhere within a sample.
    #[inline]
    const fn to_the_jump(&self, settings: &Settings, phase: u32) -> Option<u32> {
        let to_the_start = phase.wrapping_neg();
        let distance = match settings.wave2 {
            Vco2Wave::Pulse if phase < self.width => self.width.wrapping_sub(phase),
            Vco2Wave::Sawtooth | Vco2Wave::Pulse => to_the_start,
            Vco2Wave::Sine | Vco2Wave::Noise => 0,
        };
        // At the very start of its cycle the next start is a cycle away.
        if distance == 0 { None } else { Some(distance) }
    }

    /// One sample of VCO-1 under cross modulation, and how long before
    /// it VCO-1's cycle started, if it did.
    ///
    /// The sample is made in stretches ([`Oscillator::run_to_start`]):
    /// each runs to where VCO-2's waveform next jumps or to where
    /// VCO-1's own cycle starts, whichever is first, with VCO-1's
    /// frequency at each end of it as VCO-2's waveform there makes it.
    /// With the sync, VCO-2's cycle starts with VCO-1's, and the
    /// stretches that follow take VCO-2's waveform from its new cycle.
    #[inline]
    fn cross_modulated(&mut self, settings: &Settings, noise: i32) -> (Waves, Option<u32>) {
        let step = self.second.step();
        let mut phase = self.second.phase();
        let mut started = None;
        let mut from = match self.carried {
            Some(carried) if settings.wave2 != Vco2Wave::Noise => carried,
            _ => self.crossed_at(settings, phase, noise),
        };
        for _ in 0..STRETCHES {
            let now = self.first.now();
            // What VCO-2 runs in what is left of the sample.
            let left = if now == u32::MAX { step } else { mul_high_unsigned(step, now) };
            let jump = self.to_the_jump(settings, phase).filter(|&distance| distance <= left);
            let run = jump.unwrap_or(left);
            let until = if run >= left { 0 } else { ratio(left.wrapping_sub(run), step) };
            let end = phase.wrapping_add(run);
            // Before a jump the waveform is as it was a unit of phase
            // short of it.
            let last = self.crossed_at(settings, if jump.is_some() { end.wrapping_sub(1) } else { end }, noise);
            if let Some(at) = self.first.run_to_start(until, from, last) {
                started = Some(at);
                // VCO-1 starts VCO-2's cycle (owner's manual, printed
                // page 16: "forces the pitch of that oscillator to
                // synchronize itself to that of VCO-1"); without the
                // sync VCO-2 has run to that instant.
                let ran = mul_high_unsigned(step, now.wrapping_sub(at)).min(run);
                phase = if settings.sync { 0 } else { phase.wrapping_add(ran) };
                from = self.crossed_at(settings, phase, noise);
            } else {
                phase = end;
                from = if jump.is_some() { self.crossed_at(settings, phase, noise) } else { last };
            }
            if self.first.now() == 0 {
                break;
            }
        }
        self.carried = Some(from);
        (self.first.finish(), started)
    }

    /// One sample of the two oscillators, mixed, at the rate they are
    /// run at: four times the output rate under cross modulation, and
    /// twice without. An oscillator that is not heard and that the other
    /// does not need stands still.
    #[inline]
    fn oscillators(&mut self, settings: &Settings, noise: i32) -> i32 {
        let (waves, started) = if !settings.first_runs {
            (Waves::default(), None)
        } else if settings.cross != 0 {
            self.cross_modulated(settings, noise)
        } else {
            let waves = self.first.tick(None);
            (waves, self.first.started())
        };
        let first = match settings.wave1 {
            Vco1Wave::Triangle => waves.triangle,
            Vco1Wave::Sawtooth => waves.sawtooth,
            Vco1Wave::Pulse | Vco1Wave::Square => waves.pulse,
        };
        if !settings.second_runs {
            return apply(first, settings.mix1);
        }
        let waves = self.second.tick(if settings.sync { started } else { None });
        let second = match settings.wave2 {
            Vco2Wave::Sine => sine(waves.triangle),
            Vco2Wave::Sawtooth => waves.sawtooth,
            Vco2Wave::Pulse => waves.pulse,
            Vco2Wave::Noise => noise,
        };
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

    /// Takes a patch's rate, waveform and delay. A delay that is
    /// shortened while it runs shortens what is left of it.
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
        self.waiting = self.waiting.min(self.delay);
        if self.delay == 0 {
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

#[cfg(test)]
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]
mod tests {
    extern crate std;
    use super::{
        Controls, Decimator, IMMEDIATE, OSCILLATOR_RATE, OSCILLATOR_SAMPLES, Param, Patch, Settings, TICK_SAMPLES,
        Voice,
    };
    use std::{println, vec, vec::Vec};

    /// Samples of the spectrum at the output rate, and those left out
    /// before it.
    const POINTS: usize = 1 << 16;
    const SETTLE: usize = 4_096;
    /// Bins either side of a harmonic that are the harmonic's.
    const LOBE: f64 = 7.0;

    /// The magnitudes of a signal's spectrum under a Blackman-Harris
    /// window, whose side lobes are 92 dB down.
    fn spectrum(signal: &[f64]) -> Vec<f64> {
        let count = signal.len();
        let turn = |index: usize| core::f64::consts::TAU * index as f64 / count as f64;
        let window = |index: usize| {
            0.358_75 - 0.488_29 * turn(index).cos() + 0.141_28 * turn(2 * index).cos()
                - 0.011_68 * turn(3 * index).cos()
        };
        let mut real: Vec<f64> = signal.iter().enumerate().map(|(index, &value)| value * window(index)).collect();
        let mut imaginary = vec![0.0f64; count];
        let bits = count.trailing_zeros();
        for index in 0..count {
            let reversed = index.reverse_bits() >> (usize::BITS - bits);
            if index < reversed {
                real.swap(index, reversed);
            }
        }
        let mut length = 2;
        while length <= count {
            for start in (0..count).step_by(length) {
                for offset in 0..length / 2 {
                    let (sine, cosine) = (-core::f64::consts::TAU * offset as f64 / length as f64).sin_cos();
                    let (low, high) = (start + offset, start + offset + length / 2);
                    let (re, im) =
                        (real[high] * cosine - imaginary[high] * sine, real[high] * sine + imaginary[high] * cosine);
                    (real[high], imaginary[high]) = (real[low] - re, imaginary[low] - im);
                    (real[low], imaginary[low]) = (real[low] + re, imaginary[low] + im);
                }
            }
            length *= 2;
        }
        real.iter().zip(&imaginary).take(count / 2).map(|(re, im)| re.hypot(*im)).collect()
    }

    /// What the two oscillators make under cross modulation with the
    /// sync, before the filter, brought down to the output rate: the
    /// strongest thing below 20 kHz that is no harmonic of VCO-1's
    /// period, in decibels against the strongest harmonic. The period is
    /// counted from the starts of VCO-1's cycle.
    fn before_the_filter(key: u8, depth: u8, wave: u8, first: u8) -> f64 {
        let patch = Patch::DEFAULT
            .with(Param::Vco1Wave, first)
            .with(Param::CrossMod, depth)
            .with(Param::Vco2Wave, wave)
            .with(Param::Sync, 1)
            .with(Param::Vco2Range, 24);
        let settings = Settings::new(&patch);
        let mut voice = Voice::new([0, 0], 0);
        voice.configure(&settings);
        voice.start(key, true, &settings);
        let controls = Controls { lfo: 0, bend: 0, vibrato: 0, cutoff: 0, glide: IMMEDIATE };
        let (mut wide, mut sharp) = (Decimator::wide(), Decimator::sharp());
        let (mut starts, mut first_start, mut last_start) = (0u32, 0.0f64, 0.0f64);
        let mut signal = Vec::new();
        let mut count = 0usize;
        while signal.len() < POINTS + SETTLE {
            if count.is_multiple_of(TICK_SAMPLES as usize * OSCILLATOR_SAMPLES) {
                voice.tick(&settings, controls);
            }
            let mut four = [0i32; 4];
            for sample in &mut four {
                *sample = voice.oscillators(&settings, 0);
                count += 1;
                if let Some(ago) = voice.first.started() {
                    last_start = count as f64 - f64::from(ago) / 4_294_967_296.0;
                    if starts == 0 {
                        first_start = last_start;
                    }
                    starts += 1;
                }
            }
            let [a, b, c, d] = four;
            signal.push(f64::from(sharp.process(wide.process(a, b), wide.process(c, d))));
        }
        let fundamental = f64::from(starts - 1) / (last_start - first_start) * f64::from(OSCILLATOR_RATE);
        let magnitudes = spectrum(&signal[SETTLE..]);
        let bin = 48_000.0 / POINTS as f64;
        let (mut on, mut off) = (0.0f64, 0.0f64);
        for (index, &magnitude) in magnitudes.iter().enumerate().skip(8) {
            let hertz = index as f64 * bin;
            if hertz > 20_000.0 {
                break;
            }
            let harmonic = (hertz / fundamental).round();
            if (hertz - harmonic * fundamental).abs() <= LOBE * bin {
                on = on.max(magnitude);
            } else {
                off = off.max(magnitude);
            }
        }
        20.0 * (off / on).log10()
    }

    #[test]
    fn the_oscillators_under_cross_modulation_are_clean_before_the_filter() {
        // The instrument's own figures are of the whole voice, whose
        // filter bends what it is given. Before it, at every position of
        // the slider and keys to the A of 1,760 Hz, nothing that is no
        // harmonic is within 86 dB, for VCO-1's sawtooth, triangle and
        // square under VCO-2's sine, sawtooth and pulse: 92 dB, which
        // is as low as the window reads, but for the sine at 2.5 and 10
        // on the highest key.
        let keys: &[u8] = if cfg!(debug_assertions) { &[81] } else { &[45, 69, 81, 93] };
        for first in [1u8, 0, 3] {
            let mut worst = f64::MIN;
            for &key in keys {
                for depth in [26u8, 64, 128, 255] {
                    for wave in [0u8, 1, 2] {
                        let found = before_the_filter(key, depth, wave, first);
                        if found.is_finite() {
                            worst = worst.max(found);
                        }
                        // Where VCO-1 is driven out of hearing nothing is left
                        // to measure against.
                        assert!(
                            !found.is_finite() || found < -86.0,
                            "VCO-1 {first}, VCO-2 {wave}, key {key}, depth {depth}: {found}"
                        );
                    }
                }
            }
            println!("VCO-1's waveform {first}, before the filter: {worst:.1} dB at worst");
        }
    }
}
