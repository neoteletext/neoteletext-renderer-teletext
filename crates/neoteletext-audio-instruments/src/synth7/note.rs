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
    self, ALGORITHMS, ATTENUATION_MAX, ATTENUATION_ONE, ATTENUATION_SILENT, Algorithm, BLOCK_SAMPLES, OPERATORS, Voice,
};
use super::msfa::env::{Env, Progress, STAGE_FINISHED};
use super::msfa::note::{self, LevelScaling};
use super::msfa::pitchenv::PitchEnv;
use super::msfa::{N, Q24_ONE, freqlut, share, share_u32};
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
/// that is cut (all sound off): silence in 32 blocks, 21 milliseconds,
/// and a 250th of the level in 12.
///
/// Source: own choice, measured (`tests/synth7_control_sweep.rs`). The
/// attenuation is a logarithm, so the level falls by the same share at
/// every sample: in 8 blocks, as it first was, by 3.7% a sample, which
/// under a plain sine on middle C was a step 2.3 times the sine's own
/// largest; in 32 blocks by 0.9%.
const CUT_FADE_STEP: u32 = ATTENUATION_SILENT / 32;

/// The full depth of a modulation, as the parameter layer scales a
/// patch's depth of 99 ([`note::pitch_mod_depth`]: 99 is 255).
pub(crate) const FULL_DEPTH: u32 = u8::MAX as u32;

/// The shift that makes a depth of 0 to [`FULL_DEPTH`] the parameter
/// layer's depth in Q32, as a patch's depth times the oscillator's delay
/// (0..1 in Q24) is.
const DEPTH_SHIFT: u32 = 24;

/// A semitone as a logarithm of frequency, in Q24: what a glissando moves
/// by (S7-27).
const SEMITONE: i32 = Q24_ONE / 12;

/// How a control step sets the note's voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// A whole block, from its boundary: each level glides from the last
    /// step's target to this one's.
    Block,
    /// The rest of a block, from a message part way through it (S7-52):
    /// each level glides from where it stands to this step's target.
    Rest,
    /// The part of a block already rendered, brought up to the message's
    /// sample: the envelopes and glides advance, and the voice, which has
    /// sounded those samples, is left as it is.
    Elapsed,
}

/// `samples` of a block as the parameter layer counts them; a whole block
/// at most.
pub(crate) fn layer_samples(samples: usize) -> u32 {
    u32::try_from(samples.min(BLOCK_SAMPLES)).unwrap_or(N)
}

/// What the part gives each of its notes for a block.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Controls {
    /// The low frequency oscillator's sample, 0..1 in Q24.
    pub lfo_value: i32,
    /// The low frequency oscillator's delay, 0..1 in Q24.
    pub lfo_delay: i32,
    /// The pitch bend, in Q24 of an octave.
    pub bend: i32,
    /// The pitch modulation depth the part's controllers add, 0 to
    /// [`FULL_DEPTH`] (S7-36 item 1).
    pub pitch_depth: u32,
    /// The amplitude modulation depth they add, 0 to [`FULL_DEPTH`] (S7-36
    /// item 2).
    pub amplitude_depth: u32,
    /// The EG bias they take, 0 to [`FULL_DEPTH`] of the full amplitude
    /// modulation depth at its deepest (S7-36 item 3).
    pub bias: u32,
    /// Whether a glide moves in semitones (S7-27).
    pub glissando: bool,
    /// The attenuation of the part's volume, in the kernel's units.
    pub volume: u32,
}

/// What of a note a control step changes, and nothing else: as it stood
/// when a step began, for a message within the step to bring the note to
/// its own sample (S7-52). Everything else of a note changes only by a
/// message, and a message brings the note to its sample before it acts.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Course {
    envelopes: [Progress; OPERATORS],
    pitch_envelope: Progress,
    glide: i32,
    fade: u32,
    fade_step: u32,
    silent: bool,
}

impl Course {
    pub(crate) const fn new() -> Self {
        Self {
            envelopes: [Progress::new(); OPERATORS],
            pitch_envelope: Progress::new(),
            glide: 0,
            fade: 0,
            fade_step: 0,
            silent: true,
        }
    }
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
    /// The key, after transpose, and the velocity the note was started
    /// with, for a parameter that changes under it (S7-44).
    midinote: i32,
    velocity: u8,
    /// The glide still to go to the key's pitch, in Q24 of an octave, and
    /// what it moves by each block (S7-27).
    glide: i32,
    glide_step: i32,
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
            midinote: 0,
            velocity: 0,
            glide: 0,
            glide_step: 0,
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
        // A voice that goes on sounding keeps its phases. Set to zero
        // under a level that is there already, they would step the
        // signal by as much as all of it: a key struck again while its
        // last note dies away is the commonest case. Own choice: the
        // instrument, with a voice to spare, gives a new key another
        // voice and leaves the old one to die away.
        if params.oscillator_key_sync != 0 && !continues {
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
            let outlevel = note::operator_outlevel(
                midinote,
                i32::from(velocity),
                operator.output_level,
                scaling,
                operator.key_velocity_sensitivity,
            );
            let rate_scaling = note::scale_rate(midinote, operator.rate_scaling);
            let level = if continues { envelope.level() } else { 0 };
            envelope.init(operator.eg_rates, operator.eg_levels, outlevel, rate_scaling, level);
            *base_pitch = note::osc_freq(
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
        self.pitch_mod_depth = note::pitch_mod_depth(params.lfo_pitch_mod_depth);
        self.pitch_mod_sens = note::pitch_mod_sens(params.pitch_mod_sensitivity);
        // Own choice: the amplitude depth is scaled to 0..255 as the
        // parameter layer scales the pitch depth.
        self.amp_mod_depth = note::pitch_mod_depth(params.lfo_amp_mod_depth);
        self.compensation = kernel::carrier_compensation(self.algorithm());
        self.fade = 0;
        self.fade_step = 0;
        self.released = false;
        self.silent = false;
        self.midinote = midinote;
        self.velocity = velocity;
        self.glide = 0;
        self.glide_step = 0;
    }

    /// The pitch of the note's key now, glide included, as a logarithm of
    /// frequency in Q24: where a glide to the next key starts (S7-27).
    pub(crate) fn pitch(&self) -> i32 {
        note::midinote_to_logfreq(self.midinote).saturating_add(self.glide)
    }

    /// Glides from `from`, a pitch as [`Note::pitch`] gives it, to the
    /// note's own, by `step` each block; a step of 0 does not glide.
    pub(crate) fn glide_from(&mut self, from: i32, step: i32) {
        if step > 0 {
            self.glide = from.saturating_sub(note::midinote_to_logfreq(self.midinote));
            self.glide_step = step;
        } else {
            self.glide = 0;
        }
    }

    /// Moves a sounding note to another key without starting its envelopes
    /// again (S7-26: legato; S7-27: a sustained note that follows), gliding
    /// from where it is by `step` each block.
    pub(crate) fn retarget(&mut self, patch: &Patch, key: u8, step: i32) {
        let from = self.pitch();
        let params = patch.params();
        self.midinote =
            i32::from(key).saturating_add(i32::from(params.transpose)).saturating_sub(i32::from(TRANSPOSE_CENTRE));
        for (base_pitch, operator) in self.base_pitch.iter_mut().zip(&params.operators) {
            *base_pitch = note::osc_freq(
                self.midinote,
                operator.oscillator_mode,
                operator.frequency_coarse,
                operator.frequency_fine,
                operator.detune,
            );
        }
        self.glide_from(from, step);
    }

    /// Takes a change of the current voice into the sounding note, for the
    /// parameters the engine reads continuously (S7-44): each operator's
    /// output level, the operator switch, the modulation depths and the
    /// pitch modulation sensitivity.
    pub(crate) fn follow(&mut self, patch: &Patch) {
        let params = patch.params();
        for (envelope, operator) in self.envelopes.iter_mut().zip(&params.operators) {
            let scaling = LevelScaling {
                break_pt: operator.break_point,
                left_depth: operator.left_depth,
                right_depth: operator.right_depth,
                left_curve: operator.left_curve,
                right_curve: operator.right_curve,
            };
            envelope.set_outlevel(note::operator_outlevel(
                self.midinote,
                i32::from(self.velocity),
                operator.output_level,
                scaling,
                operator.key_velocity_sensitivity,
            ));
        }
        self.operators_on = params.operators_on;
        self.pitch_mod_depth = note::pitch_mod_depth(params.lfo_pitch_mod_depth);
        self.pitch_mod_sens = note::pitch_mod_sens(params.pitch_mod_sensitivity);
        self.amp_mod_depth = note::pitch_mod_depth(params.lfo_amp_mod_depth);
        for (sensitivity, operator) in self.amp_mod_sensitivity.iter_mut().zip(&params.operators) {
            *sensitivity = operator.amp_mod_sensitivity;
        }
    }

    /// What of the note a control step changes, as it stands.
    pub(crate) fn course(&self) -> Course {
        Course {
            envelopes: self.envelopes.map(|envelope| envelope.progress()),
            pitch_envelope: self.pitch_envelope.progress(),
            glide: self.glide,
            fade: self.fade,
            fade_step: self.fade_step,
            silent: self.silent,
        }
    }

    /// Puts back what a control step changes as it was in `begun`, the
    /// note's [`Note::course`] when the rack's control step began, for a
    /// message part way through the step to bring the note to the
    /// message's sample (S7-52). The voice has sounded the samples since,
    /// and keeps its phases and levels.
    pub(crate) fn restore(&mut self, begun: &Course) {
        for (envelope, progress) in self.envelopes.iter_mut().zip(&begun.envelopes) {
            envelope.set_progress(progress);
        }
        self.pitch_envelope.set_progress(&begun.pitch_envelope);
        self.glide = begun.glide;
        self.fade = begun.fade;
        self.fade_step = begun.fade_step;
        self.silent = begun.silent;
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

    /// Ends the note within 21 milliseconds. Its envelopes are left
    /// where they are: let go, a release at its quickest would take the
    /// level down sooner than the fade does, and more suddenly.
    pub(crate) fn cut(&mut self) {
        self.released = true;
        self.fade_step = CUT_FADE_STEP;
    }

    fn algorithm(&self) -> &'static Algorithm {
        ALGORITHMS.get(usize::from(self.algorithm)).unwrap_or(&ALGORITHMS[0])
    }

    /// Advances the envelopes by `samples` of a block (their share of a
    /// block's movement, and a whole block's for [`BLOCK_SAMPLES`]) and
    /// sets the voice for them as `step` says. Returns whether the note
    /// still sounds: false once it is released and its carriers are silent
    /// for the whole step, after which it need not be rendered again.
    pub(crate) fn control(&mut self, controls: &Controls, samples: usize, step: Step) -> bool {
        // A note from silence starts at its envelopes' levels a whole block
        // on, wherever in the block it starts, as one started on a block's
        // boundary does (`Voice::silence`): advanced by only the samples
        // left, its first levels would be far below them, and the next
        // block's glide up to them would step the signal.
        let layer = if step == Step::Rest && self.voice.is_fresh() { N } else { layer_samples(samples) };
        let algorithm = self.algorithm();
        let delay = controls.lfo_delay.clamp(0, Q24_ONE).cast_unsigned();

        // S7-36: the controllers add their depth to the patch's, without
        // the oscillator's delay, the sum not past the full depth. At full
        // travel and range the modulation wheel adds as much as a patch's
        // depth of 99, as it did before it had a range.
        let full = FULL_DEPTH << DEPTH_SHIFT;
        let depth = u32::from(self.pitch_mod_depth)
            .saturating_mul(delay)
            .saturating_add(controls.pitch_depth << DEPTH_SHIFT)
            .min(full);
        // S7-27: the glide, in semitones for a glissando, towards the key.
        let glide_step = share(self.glide_step, layer);
        self.glide = if self.glide > 0 {
            self.glide.saturating_sub(glide_step).max(0)
        } else {
            self.glide.saturating_add(glide_step).min(0)
        };
        let glide = if controls.glissando { (self.glide / SEMITONE).saturating_mul(SEMITONE) } else { self.glide };
        let pitch = self
            .pitch_envelope
            .getsample_for(layer)
            .saturating_add(note::lfo_pitch_mod(depth, self.pitch_mod_sens, controls.lfo_value))
            .saturating_add(controls.bend)
            .saturating_add(glide);

        // The fraction of the full amplitude modulation depth taken now, in
        // Q32: the patch's depth, times the delay, with the controllers'
        // (S7-36), times how far the oscillator is below its top.
        let swing = Q24_ONE.saturating_sub(controls.lfo_value).clamp(0, Q24_ONE).cast_unsigned();
        let amp_depth = u32::from(self.amp_mod_depth)
            .saturating_mul(delay)
            .saturating_add(controls.amplitude_depth << DEPTH_SHIFT)
            .min(full);
        let amp_fraction = (amp_depth >> 16).saturating_mul(swing >> 8);
        // S7-36 item 3: EG bias, the fraction of the full depth at the
        // oscillator's deepest, in Q32 as `amp_fraction` is.
        let bias_fraction = controls.bias.min(FULL_DEPTH) << DEPTH_SHIFT;

        if self.released
            && self.fade_step == 0
            && self.carrier_envelopes(algorithm).all(|envelope| envelope.stage() == STAGE_FINISHED)
        {
            self.fade_step = REST_FADE_STEP;
        }
        self.fade = self.fade.saturating_add(share_u32(self.fade_step, layer)).min(ATTENUATION_MAX);

        let mut silent = true;
        let operators = self.envelopes.iter_mut().zip(&self.base_pitch).zip(&self.amp_mod_sensitivity);
        for (index, ((envelope, &base_pitch), &sensitivity)) in operators.enumerate() {
            let level = envelope.getsample_for(layer);
            let mut attenuation = LEVEL_CEILING.saturating_sub(level).max(0).cast_unsigned();
            let on = operator_on(self.operators_on, index);
            if algorithm.is_carrier(index) {
                attenuation = attenuation.saturating_add(self.compensation).saturating_add(self.fade);
                // Whether the note has ended is asked of the envelope and
                // the fade alone: tremolo and volume may bring a level back.
                silent &= !on || attenuation >= ATTENUATION_SILENT;
                attenuation = attenuation.saturating_add(controls.volume);
            }
            if let Some(&shift) = usize::from(sensitivity).checked_sub(1).and_then(|at| AMP_MOD_SHIFTS.get(at)) {
                attenuation = attenuation.saturating_add(amp_fraction >> shift).saturating_add(bias_fraction >> shift);
            }
            if !on {
                attenuation = ATTENUATION_MAX;
            }
            match step {
                Step::Block => {
                    self.voice.set_operator(index, freqlut::lookup(base_pitch.saturating_add(pitch)), attenuation);
                }
                Step::Rest => {
                    let increment = freqlut::lookup(base_pitch.saturating_add(pitch));
                    self.voice.aim_operator(index, increment, attenuation, samples);
                }
                Step::Elapsed => {}
            }
        }
        match step {
            Step::Block => self.voice.begin_block(),
            Step::Rest => self.voice.begin_rest(samples),
            Step::Elapsed => {}
        }
        let ended = self.released && silent && self.silent;
        self.silent = silent;
        !ended
    }

    fn carrier_envelopes<'a>(&'a self, algorithm: &'a Algorithm) -> impl Iterator<Item = &'a Env> {
        let on = self.operators_on;
        self.envelopes
            .iter()
            .enumerate()
            .filter(move |&(index, _)| algorithm.is_carrier(index) && operator_on(on, index))
            .map(|(_, envelope)| envelope)
    }

    /// Adds the note's next samples to `out`, up to the end of the block.
    pub(crate) fn render(&mut self, out: &mut [i32]) {
        let algorithm = self.algorithm();
        self.voice.render(algorithm, self.feedback, out);
    }
}

/// Whether the operator at `index` (0 for operator 1) is on in `switch`,
/// voice parameter 155: bit 5 is operator 1 and bit 0 operator 6, as all
/// three of the maker's manuals draw it (`synth7.md`, S7-30; DX7 manual,
/// page 31: "D5 … OP1 … D0 OP6"; TX7 manual, printed page 39, note *7; DX7
/// II manual, printed page Add-8: "B5:OP1,..,B0:OP6").
pub(crate) fn operator_on(switch: u8, index: usize) -> bool {
    let bit = (OPERATORS - 1).checked_sub(index);
    bit.and_then(|bit| u32::try_from(bit).ok()).is_some_and(|bit| (switch >> bit) & 1 == 1)
}

/// One doubling of attenuation is one doubling of level.
const _: () = assert!(ATTENUATION_ONE == Q24_ONE.cast_unsigned());
