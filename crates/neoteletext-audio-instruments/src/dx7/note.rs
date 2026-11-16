//! One sounding note: the parameter layer's envelopes and frequencies for
//! a key, driving a voice of the sound kernel.
//!
//! This is where the two meet, and where the units of one become the units
//! of the other:
//!
//! - The parameter layer's level is a logarithm to base two in Q24 in which
//!   15 doublings is nominal full scale and 16 is its ceiling; the kernel's
//!   attenuation is the same logarithm counted down from 16
//!   ([`LEVEL_CEILING`]). At that layer's nominal full scale an operator's
//!   sample is ±8,186 and it swings the phase of what it modulates by two
//!   turns each way, which is what the original's own kernel does with the
//!   same level.
//! - The parameter layer's frequency, a logarithm to base two of hertz in
//!   Q24, becomes the kernel's phase increment through
//!   `msfa::freqlut::lookup`, whose table is for 48,000 samples a second.

use super::kernel::{
    self, ALGORITHMS, ATTENUATION_MAX, ATTENUATION_ONE, ATTENUATION_SILENT, Algorithm, OPERATORS, Voice,
};
use super::msfa::dx7note::{self, LevelScaling};
use super::msfa::env::{Env, STAGE_FINISHED};
use super::msfa::pitchenv::PitchEnv;
use super::msfa::{Q24_ONE, freqlut};
use super::patch::{Patch, TRANSPOSE_CENTRE};

/// The level of the parameter layer at which the kernel's attenuation is
/// zero: 16 doublings. That layer's levels stay below it: its largest is
/// 4,064 steps of 1/256 of a doubling.
///
/// Source: derived. The parameter layer's kernel gives a level of `L`
/// doublings a gain of `2^(L - 14)` where 1.0 is one turn of phase
/// (`dx7note.cc`: `Exp2::lookup(level - (14 * (1 << 24)))`); this kernel's
/// sample at no attenuation is 2^14 against a turn of 2^12, four turns,
/// which is that gain at `L = 16`.
const LEVEL_CEILING: i32 = 16 * Q24_ONE;

/// The right shift that turns the product of the amplitude modulation depth
/// and the oscillator's swing, a fraction in Q32, into attenuation, for
/// each amplitude modulation sensitivity above 0: at full depth the
/// oscillator takes a half, two and eight doublings (3, 12 and 48 decibels)
/// from an operator at sensitivities 1, 2 and 3.
///
/// Source: own choice. The parameter layer translated here has no amplitude
/// modulation, and no source permitted to this crate gives the
/// instrument's depths.
const AMP_MOD_SHIFTS: [u32; 3] = [9, 7, 5];

/// What is added each block to the attenuation of the carriers of a
/// released note whose envelopes have all come to rest above silence (a
/// voice whose fourth level is not zero): it fades to silence in 750
/// blocks, half a second, where the instrument would sound it until the
/// voice was needed.
///
/// Source: own choice, so that every note ends and its voice is freed.
const REST_FADE_STEP: u32 = ATTENUATION_SILENT / 750;

/// What is added each block to the attenuation of the carriers of a note
/// that is cut (all sound off): silence in 8 blocks, 5.3 milliseconds,
/// which is short and makes no click.
///
/// Source: own choice.
const CUT_FADE_STEP: u32 = ATTENUATION_SILENT / 8;

/// What the part gives each of its notes for a block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Controls {
    /// The low frequency oscillator's sample, 0..1 in Q24.
    pub lfo_value: i32,
    /// The low frequency oscillator's delay, 0..1 in Q24.
    pub lfo_delay: i32,
    /// The pitch bend, in Q24 of an octave.
    pub bend: i32,
    /// The modulation wheel, 0 to 127.
    pub wheel: u8,
    /// The attenuation of the part's volume, in the kernel's units.
    pub volume: u32,
}

/// One note.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Note {
    voice: Voice,
    envelopes: [Env; OPERATORS],
    pitch_envelope: PitchEnv,
    /// Each operator's frequency, as the parameter layer's logarithm.
    base_pitch: [i32; OPERATORS],
    amp_mod_sensitivity: [u8; OPERATORS],
    algorithm: u8,
    feedback: u8,
    operators_on: u8,
    pitch_mod_depth: u8,
    pitch_mod_sens: u8,
    amp_mod_depth: u8,
    /// The attenuation of each carrier for the number of carriers.
    compensation: u32,
    /// The attenuation added to the carriers by a fade.
    fade: u32,
    fade_step: u32,
    released: bool,
    /// Whether the carriers were all silent at the end of the last block.
    silent: bool,
}

impl Note {
    pub(crate) const fn new() -> Self {
        Self {
            voice: Voice::new(),
            envelopes: [Env::new(); OPERATORS],
            pitch_envelope: PitchEnv::new(),
            base_pitch: [0; OPERATORS],
            amp_mod_sensitivity: [0; OPERATORS],
            algorithm: 0,
            feedback: 0,
            operators_on: 0,
            pitch_mod_depth: 0,
            pitch_mod_sens: 0,
            amp_mod_depth: 0,
            compensation: 0,
            fade: 0,
            fade_step: 0,
            released: true,
            silent: true,
        }
    }

    /// Makes the note silent at once and forgets its levels.
    pub(crate) fn clear(&mut self) {
        let voice = self.voice;
        *self = Self::new();
        // The phases run on, as a free voice's do in the instrument.
        self.voice = voice;
        self.voice.silence();
    }

    /// Starts the note for a key. With `continues`, the voice was sounding
    /// and each envelope starts from the level it had; otherwise from
    /// silence.
    pub(crate) fn start(&mut self, patch: &Patch, key: u8, velocity: u8, continues: bool) {
        let params = patch.params();
        // Own choice: transpose moves the key itself, so keyboard scaling
        // follows the transposed key as pitch does.
        let midinote =
            i32::from(key).saturating_add(i32::from(params.transpose)).saturating_sub(i32::from(TRANSPOSE_CENTRE));
        if !continues {
            self.voice.silence();
        }
        if params.oscillator_key_sync != 0 {
            self.voice.reset_phases();
        }
        let operators = self.envelopes.iter_mut().zip(&mut self.base_pitch).zip(&mut self.amp_mod_sensitivity);
        for (((envelope, base_pitch), sensitivity), operator) in operators.zip(&params.operators) {
            let scaling = LevelScaling {
                break_pt: operator.break_point,
                left_depth: operator.left_depth,
                right_depth: operator.right_depth,
                left_curve: operator.left_curve,
                right_curve: operator.right_curve,
            };
            let outlevel = dx7note::operator_outlevel(
                midinote,
                i32::from(velocity),
                operator.output_level,
                scaling,
                operator.key_velocity_sensitivity,
            );
            let rate_scaling = dx7note::scale_rate(midinote, operator.rate_scaling);
            let level = if continues { envelope.level() } else { 0 };
            envelope.init(operator.eg_rates, operator.eg_levels, outlevel, rate_scaling, level);
            *base_pitch = dx7note::osc_freq(
                midinote,
                operator.oscillator_mode,
                operator.frequency_coarse,
                operator.frequency_fine,
                operator.detune,
            );
            *sensitivity = operator.amp_mod_sensitivity;
        }
        self.pitch_envelope.set(params.pitch_eg_rates, params.pitch_eg_levels);
        self.algorithm = params.algorithm;
        self.feedback = params.feedback;
        self.operators_on = params.operators_on;
        self.pitch_mod_depth = dx7note::pitch_mod_depth(params.lfo_pitch_mod_depth);
        self.pitch_mod_sens = dx7note::pitch_mod_sens(params.pitch_mod_sensitivity);
        // Own choice: the amplitude depth is scaled to 0..255 as the
        // parameter layer scales the pitch depth.
        self.amp_mod_depth = dx7note::pitch_mod_depth(params.lfo_amp_mod_depth);
        self.compensation = kernel::carrier_compensation(self.algorithm());
        self.fade = 0;
        self.fade_step = 0;
        self.released = false;
        self.silent = false;
    }

    /// The key goes up: every envelope starts its last stage.
    pub(crate) fn release(&mut self) {
        if !self.released {
            self.released = true;
            for envelope in &mut self.envelopes {
                envelope.keydown(false);
            }
            self.pitch_envelope.keydown(false);
        }
    }

    /// Ends the note within a few milliseconds.
    pub(crate) fn cut(&mut self) {
        self.release();
        self.fade_step = CUT_FADE_STEP;
    }

    fn algorithm(&self) -> &'static Algorithm {
        ALGORITHMS.get(usize::from(self.algorithm)).unwrap_or(&ALGORITHMS[0])
    }

    /// Advances the envelopes by one block and sets the voice for it.
    /// Returns whether the note still sounds: false once it is released and
    /// its carriers are silent for the whole block, after which it need not
    /// be rendered again.
    pub(crate) fn control(&mut self, controls: &Controls) -> bool {
        let algorithm = self.algorithm();
        let delay = controls.lfo_delay.clamp(0, Q24_ONE).cast_unsigned();

        // Own choice: the wheel adds vibrato depth, at full travel as much
        // as a patch's depth of 99, without the oscillator's delay.
        let wheel = u32::from(controls.wheel);
        let wheel_depth = (wheel << 1 | wheel >> 6) << 24;
        let depth = u32::from(self.pitch_mod_depth)
            .saturating_mul(delay)
            .saturating_add(wheel_depth)
            .min(u32::from(u8::MAX) << 24);
        let pitch = self
            .pitch_envelope
            .getsample()
            .saturating_add(dx7note::lfo_pitch_mod(depth, self.pitch_mod_sens, controls.lfo_value))
            .saturating_add(controls.bend);

        // The fraction of the full amplitude modulation depth taken now, in
        // Q32: the patch's depth, times the delay, times how far the
        // oscillator is below its top.
        let swing = Q24_ONE.saturating_sub(controls.lfo_value).clamp(0, Q24_ONE).cast_unsigned();
        let amp_fraction = (u32::from(self.amp_mod_depth).saturating_mul(delay) >> 16).saturating_mul(swing >> 8);

        if self.released
            && self.fade_step == 0
            && self.carrier_envelopes(algorithm).all(|envelope| envelope.stage() == STAGE_FINISHED)
        {
            self.fade_step = REST_FADE_STEP;
        }
        self.fade = self.fade.saturating_add(self.fade_step).min(ATTENUATION_MAX);

        let mut silent = true;
        let operators = self.envelopes.iter_mut().zip(&self.base_pitch).zip(&self.amp_mod_sensitivity);
        for (index, ((envelope, &base_pitch), &sensitivity)) in operators.enumerate() {
            let level = envelope.getsample();
            let mut attenuation = LEVEL_CEILING.saturating_sub(level).max(0).cast_unsigned();
            let on = (self.operators_on >> index) & 1 == 1;
            if algorithm.is_carrier(index) {
                attenuation = attenuation.saturating_add(self.compensation).saturating_add(self.fade);
                // Whether the note has ended is asked of the envelope and
                // the fade alone: tremolo and volume may bring a level back.
                silent &= !on || attenuation >= ATTENUATION_SILENT;
                attenuation = attenuation.saturating_add(controls.volume);
            }
            if let Some(&shift) = usize::from(sensitivity).checked_sub(1).and_then(|at| AMP_MOD_SHIFTS.get(at)) {
                attenuation = attenuation.saturating_add(amp_fraction >> shift);
            }
            if !on {
                attenuation = ATTENUATION_MAX;
            }
            self.voice.set_operator(index, freqlut::lookup(base_pitch.saturating_add(pitch)), attenuation);
        }
        self.voice.begin_block();
        let ended = self.released && silent && self.silent;
        self.silent = silent;
        !ended
    }

    fn carrier_envelopes<'a>(&'a self, algorithm: &'a Algorithm) -> impl Iterator<Item = &'a Env> {
        let on = self.operators_on;
        self.envelopes
            .iter()
            .enumerate()
            .filter(move |&(index, _)| algorithm.is_carrier(index) && (on >> index) & 1 == 1)
            .map(|(_, envelope)| envelope)
    }

    /// Adds the note's next samples to `out`, up to the end of the block.
    pub(crate) fn render(&mut self, out: &mut [i32]) {
        let algorithm = self.algorithm();
        self.voice.render(algorithm, self.feedback, out);
    }
}

/// One doubling of attenuation is one doubling of level.
const _: () = assert!(ATTENUATION_ONE == Q24_ONE.cast_unsigned());
