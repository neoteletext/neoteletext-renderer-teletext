//! Moog's Minimoog Model D (1970): one voice of three oscillators, noise
//! and an external input mixed into the four-pole transistor ladder
//! filter, two contour generators and an amplifier, rendered from MIDI
//! messages to 16-bit mono samples at 48,000 Hz.
//!
//! # The instrument
//!
//! - **Oscillators.** Three, each with a range switch of six positions
//!   (LO, 32' to 2') and a waveform switch of six (triangle, the triangle
//!   with a little sawtooth, sawtooth, square, and a wide and a narrow
//!   rectangular wave; oscillator 3 has a reversed sawtooth in the second
//!   place). Oscillators 2 and 3 have a FREQUENCY knob of seven semitones
//!   either way. Oscillator 3 may be taken off the keyboard, when the
//!   same knob covers 2.75 octaves either way, and at LO it is the
//!   instrument's slow oscillator. Each drifts a little, from a seed.
//! - **Mixer.** The three oscillators, white or pink noise, and the
//!   external input, which here is fed from the instrument's own output,
//!   as the well-known patch feeds it with a cord: its volume is how much
//!   returns. What the mixer sums drives the filter and the amplifier
//!   after it into their curve: one oscillator at 10 a little, three
//!   clearly.
//! - **Filter.** The ladder ([`Ladder`]): 24 dB an octave, its EMPHASIS
//!   bringing it to oscillation from 7.5 on the dial, its cutoff set by
//!   the knob over ten octaves, by a third, two thirds or all of the
//!   keyboard's voltage, by its contour and by the modulation.
//! - **Contours.** Two, of attack, decay and sustain, each a capacitor
//!   charged and discharged ([`RcEnvelope`]); the DECAY switch makes the
//!   decay time the release too, and without it a released note is cut
//!   in a few milliseconds.
//! - **Modulation.** Oscillator 3 and noise, mixed by MODULATION MIX and
//!   let through by the modulation wheel, to the oscillators' pitch, the
//!   cutoff, or both.
//! - **Keyboard.** One note, the lowest key held; the contours start only
//!   when a key goes down with none held; GLIDE slides the pitch.
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428, from Moog's
//! service manual and its schematics, R. J. Folkman's Technical Service
//! Manual (1973), the owner's manual, the 1974 sound charts and Timothy
//! Stinchcombe's analysis of the ladder, with no code and no table from
//! any emulator or plugin. One reference program was run as a sealed
//! program and its output measured; no figure here was taken from it.
//! Each constant names its source where it is defined, in [`law`] and
//! here.
//!
//! # What is not here, or is weaker than the instrument
//!
//! - **The ladder is the building block's**: the textbook hyperbolic
//!   tangent in each stage at one saturation level, which passes what
//!   is well below its cutoff unbent at any level. What bends a loud
//!   mixer is here the amplifier after the ladder, taken as one more
//!   such curve at the same level ([`law::OUTPUT_STAGE`]): the level is
//!   an own choice, and so is the feedback above oscillation. The
//!   filter's input and output coupling, its bias and its transistors'
//!   mismatch are not modelled.
//! - **The contours' peak, the attack's asymptote and the cut of a
//!   released note** are own readings of a schematic that was hard to
//!   read ([`law::CONTOUR`], [`law::ATTACK_TARGET`],
//!   [`law::DUMP_TIME`]).
//! - **The knobs' laws between their documented ends** are own choices
//!   for the EMPHASIS, the GLIDE, the volumes and the modulation wheel.
//! - **The external input** is only the instrument's own output, through
//!   a preamplifier whose gain is an own choice.
//! - **The pink and red noise** are an own design, not the instrument's
//!   three filters; the level of the noise is an own reading of "-5dB".
//! - **The oscillators' waveforms are ideal**: the sawtooth's reset
//!   time, the triangle's and the rectangular waves' shaping errors and
//!   the levels' small differences between positions are left out, and
//!   so are the tracking errors the HI END COMP trimmers correct.
//! - **Oscillator 3's pitch off the keyboard** is an own choice
//!   ([`law::FREE_KEY`]).
//! - **What the ladder's curve folds back.** The oscillators by
//!   themselves leave nothing below 20 kHz that is no harmonic within
//!   91 dB (`analog::oscillator`). The ladder's stages bend their edges,
//!   and at four times the output rate, where the voice runs, what that
//!   makes above 172 kHz comes back below 20 kHz. Against the note's
//!   strongest harmonic, with the filter open: one sawtooth at 10 leaves
//!   92 dB at 110 Hz, 74 dB at 880 Hz and 61 dB at 4,186 Hz, the top key
//!   at 2'; at 5, 80 dB or better to 1,760 Hz and 72 dB at the top key;
//!   the narrow rectangular wave 4 to 6 dB less than the sawtooth; three
//!   sawtooths at 10 in step, 78 dB at 110 Hz and 46 dB at 4,186 Hz; a
//!   triangle, 99 dB at worst. From 880 Hz up at 10 that is short of the
//!   80 dB the oscillators are held to; at twice the output rate every
//!   figure was 6 to 9 dB worse. The amplifier's curve after the ladder
//!   is not the cause: without it one oscillator's figures move by less
//!   than 1 dB (`tests/minimoog_aliasing.rs` holds the figures).
//! - **The noise is made at twice the output rate** and each of its
//!   samples held for two of the voice's, which takes 0.6 dB off it at
//!   20 kHz.
//! - **Velocity is ignored**, as the keyboard has none; there is no
//!   A-440 tone and no external control input.

pub mod law;
pub mod patch;

use crate::analog::allocator::{Allocator, HELD_KEYS, Mode, Priority, Stage as KeyStage, Trigger};
use crate::analog::decimator::Decimator;
use crate::analog::drift::Drift;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{Ladder, ONE_Q28, OnePole, saturate};
use crate::analog::fixed::{accumulate, amplify, mul_high, scale};
use crate::analog::oscillator::{Noise, Oscillator, Waves, waveform};
use crate::analog::volume::{self, VolumeGain};
use crate::analog::{frequency, to_sample};

use law::{OCTAVE, RATE};
pub use patch::{ContourPatch, OscillatorPatch, PATCH_BYTES, Patch, PatchError, Range, Waveform};

/// Oscillators of the instrument.
///
/// Source: owner's manual, page 1: "three oscillators for the production
/// of pitched tones".
pub const OSCILLATORS: usize = 3;

/// Messages kept while all sound off takes the sound out, to be taken
/// when it has gone: more than come in the 5.3 ms that lasts but from
/// a stream that means to flood, whose next message ends the wait.
///
/// Source: own choice.
const DEFERRED: usize = 16;

/// The pitch bend value that bends nothing.
///
/// Source: MIDI 1.0: a pitch bend is 14 bits and its centre is 0x2000.
const PITCH_BEND_CENTRE: i32 = 0x2000;
/// The largest value of a MIDI data byte.
const DATA_MAX: u8 = 0x7f;
/// The sustain pedal is down at this value and above.
///
/// Source: MIDI 1.0: a switch controller is on from 64.
const PEDAL_DOWN: u8 = 64;
/// MIDI status: note off.
const STATUS_NOTE_OFF: u8 = 0x80;
/// MIDI status: note on.
const STATUS_NOTE_ON: u8 = 0x90;
/// MIDI status: control change.
const STATUS_CONTROL_CHANGE: u8 = 0xb0;
/// MIDI status: pitch bend.
const STATUS_PITCH_BEND: u8 = 0xe0;
/// Controller: modulation wheel.
const CONTROL_MODULATION_WHEEL: u8 = 1;
/// Controller: volume.
const CONTROL_VOLUME: u8 = 7;
/// Controller: sustain pedal.
const CONTROL_SUSTAIN: u8 = 64;
/// Controller: all sound off.
const CONTROL_ALL_SOUND_OFF: u8 = 120;
/// Controller: reset all controllers.
const CONTROL_RESET_CONTROLLERS: u8 = 121;
/// Controller: all notes off.
const CONTROL_ALL_NOTES_OFF: u8 = 123;

/// The seed of the instrument's noise and of its oscillators' drift.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4d6f_6f67;
/// What a noise generator's number is multiplied by before it is mixed
/// into the seed.
///
/// Source: own choice: the golden ratio's fraction in 32 bits, odd.
const SEED_STRIDE: u32 = 0x9E37_79B9;

/// The step of [`law::PITCH`] at the voice's rate: what the oscillators'
/// intervals are raised from.
const PITCH_STEP: u32 = frequency(law::PITCH, RATE);
/// The step of [`law::CUTOFF_ANCHOR`], likewise for the filter.
const CUTOFF_STEP: u32 = frequency(law::CUTOFF_ANCHOR, RATE);
/// Bits of a contour's and of the glide's coefficient.
const COEFFICIENT_BITS: u32 = 31;
/// The share of the mixed output that is a sample, in Q28: 1296 / 10000.
/// One oscillator's sawtooth through the open filter then peaks 18.0 dB
/// below full scale; the instrument has one voice, and no more of it
/// adds.
/// What nears full scale is bent under it by the output's limiter
/// ([`crate::output::limit`]).
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's quarter, as it was first, the note stood 5.7 dB above it.
pub const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 1296 / 10000) as i32;
/// The rate the noise generators run at, in hertz: twice the output
/// rate, each of their samples held for two of the voice's.
///
/// Source: own choice. A generator that gives a new number for each
/// sample spreads its power evenly to half the rate it is run at, and
/// the levels of the noise were set with the generators at this rate:
/// run at the voice's four times the output rate, half as much of the
/// same power would lie below 20 kHz. Held, the noise is as it was to
/// within 0.6 dB at 20 kHz.
const NOISE_RATE: u32 = RATE / 2;

/// Millionths of a second in a thousandth.
const MILLISECOND: u32 = 1_000;

/// The right shift from a potentiometer's whole travel to the most of it
/// that a sample of the voice moves it: the whole travel in 1,024
/// samples of the voice, which are 256 of the output's, 5.3 ms, the
/// time a volume controller's gain takes
/// ([`VOLUME_SLEW`](crate::analog::volume::VOLUME_SLEW)).
///
/// Source: own choice. A knob of the panel is a potentiometer under a
/// hand, which passes every place between two; a patch that is set
/// under a sounding note puts the knob at its new place in no time,
/// which no hand does and which steps the signal. Long against a
/// sample, short against a hand.
const POT_SLEW_SHIFT: u32 = 10;
/// The filter's two knobs, CUTOFF FREQUENCY and EMPHASIS, go more
/// slowly, by this shift: their whole travel in 4,096 samples of the
/// voice, 21 ms.
///
/// Source: own choice, measured (`tests/minimoog_control_sweep.rs`, on
/// a triangle through the open filter, against the triangle's own
/// largest step). The CUTOFF FREQUENCY's travel is ten octaves and the
/// filter follows it at once: crossed in 5.3 ms, the filter opens over
/// a steady tone within a few samples, a step 1.42 times the tone's
/// own, and in 21 ms 1.16 times. The EMPHASIS takes most of the level
/// it takes in the first of its travel (the gain below the cutoff is
/// `1 / (1 + k)`): one division of its dial in 5.3 ms of the whole is a
/// step 1.28 times the tone's own.
const FILTER_SLEW_SHIFT: u32 = 12;
/// The most a sample moves each potentiometer of [`Pots`].
const LEVEL_SLEW: i32 = i32::MAX >> POT_SLEW_SHIFT;
/// Likewise the noise's level, whose top is [`law::NOISE_LEVEL`].
const NOISE_SLEW: i32 = law::NOISE_LEVEL >> POT_SLEW_SHIFT;
/// Likewise the external input's gain, whose top is
/// [`law::EXTERNAL_GAIN`].
const EXTERNAL_SLEW: i32 = law::EXTERNAL_GAIN >> POT_SLEW_SHIFT;
/// Likewise the EMPHASIS, whose top is [`law::EMPHASIS_TOP`].
const EMPHASIS_SLEW: i32 = law::EMPHASIS_TOP >> FILTER_SLEW_SHIFT;
/// Likewise a contour's SUSTAIN LEVEL, whose top is one in Q30.
const SUSTAIN_SLEW: i32 = ENVELOPE_ONE >> POT_SLEW_SHIFT;
/// Likewise the AMOUNT OF CONTOUR, whose top is [`law::CONTOUR`].
const CONTOUR_SLEW: i32 = law::CONTOUR >> POT_SLEW_SHIFT;
/// Likewise the CUTOFF FREQUENCY, whose travel is the dial's ten
/// divisions.
const CUTOFF_SLEW: i32 = (law::CUTOFF_DIVISION * CUTOFF_DIVISIONS) >> FILTER_SLEW_SHIFT;
/// Divisions of the CUTOFF FREQUENCY dial, from -5 to 5.
const CUTOFF_DIVISIONS: i32 = (2 * patch::CUTOFF_MAX / 100) as i32;

/// A potentiometer's place a sample on: `most` nearer to `wanted` than
/// `now`, or `wanted` itself once it is that near.
#[inline]
const fn approach(now: i32, wanted: i32, most: i32) -> i32 {
    let distance = wanted.saturating_sub(now);
    let step = if distance > most {
        most
    } else if distance < most.saturating_neg() {
        most.saturating_neg()
    } else {
        distance
    };
    now.saturating_add(step)
}

/// Pink noise, and red: white noise through one-pole filters two octaves
/// apart, each half as strong as the one below ([`law::PINK_CORNER`]).
#[derive(Clone, Debug)]
struct Pink {
    sources: [Noise; law::PINK_STAGES],
    stages: [OnePole; law::PINK_STAGES],
}

impl Pink {
    fn new() -> Self {
        let mut number = 0u32;
        let mut corner = law::PINK_CORNER;
        let sources = core::array::from_fn(|_| {
            number = number.wrapping_add(1);
            Noise::new(SEED ^ number.wrapping_mul(SEED_STRIDE))
        });
        let stages = core::array::from_fn(|_| {
            let mut stage = OnePole::new();
            stage.set_cutoff(frequency(corner, NOISE_RATE));
            // Two octaves to the next.
            corner = corner.saturating_mul(4);
            stage
        });
        Self { sources, stages }
    }

    /// One sample of the pink noise and of the red.
    #[inline]
    fn tick(&mut self) -> (i32, i32) {
        let mut sum = 0i32;
        let mut lowest = 0i32;
        for ((source, stage), halvings) in self.sources.iter_mut().zip(&mut self.stages).zip(0u32..) {
            let part = stage.low_pass(source.tick());
            if halvings == 0 {
                lowest = part;
            }
            sum = sum.wrapping_add(part >> halvings);
        }
        (sum << law::PINK_SHIFT, lowest << law::RED_SHIFT)
    }
}

/// What a patch asks of one oscillator.
#[derive(Clone, Copy, Debug)]
struct OscillatorSettings {
    /// What its range and FREQUENCY knob add to its pitch, in octaves.
    offset: i32,
    waveform: Waveform,
    /// Its switch in the mixer.
    on: bool,
}

/// The potentiometers of the panel that set a level in the signal's
/// path or the filter: where a patch puts them, or where they stand on
/// their way there ([`POT_SLEW_SHIFT`]). The mixer's switches are no
/// part of them: a switch opens its path at once, on the instrument
/// too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Pots {
    /// The mixer's VOLUME of each oscillator, in Q31.
    levels: [i32; OSCILLATORS],
    /// The NOISE VOLUME, as the noise's level in Q31.
    noise: i32,
    /// The EXTERNAL INPUT VOLUME, as its gain in Q28.
    external: i32,
    /// The cutoff with nothing added, in octaves from the anchor.
    cutoff: i32,
    /// The EMPHASIS, as the filter's feedback.
    feedback: i32,
    /// The AMOUNT OF CONTOUR, in octaves.
    contour: i32,
    /// The MODULATION MIX: the noise's share of the modulation, in Q31.
    mix: i32,
    /// The main output's VOLUME, in Q31.
    volume: i32,
    /// The SUSTAIN LEVEL of the filter's contour and of the loudness
    /// contour, in Q30.
    sustains: [i32; 2],
}

impl Pots {
    /// Each potentiometer a sample nearer to where `wanted` has it.
    #[inline]
    fn approach(&mut self, wanted: &Self) {
        for (level, &wanted) in self.levels.iter_mut().zip(&wanted.levels) {
            *level = approach(*level, wanted, LEVEL_SLEW);
        }
        self.noise = approach(self.noise, wanted.noise, NOISE_SLEW);
        self.external = approach(self.external, wanted.external, EXTERNAL_SLEW);
        self.cutoff = approach(self.cutoff, wanted.cutoff, CUTOFF_SLEW);
        self.feedback = approach(self.feedback, wanted.feedback, EMPHASIS_SLEW);
        self.contour = approach(self.contour, wanted.contour, CONTOUR_SLEW);
        self.mix = approach(self.mix, wanted.mix, LEVEL_SLEW);
        self.volume = approach(self.volume, wanted.volume, LEVEL_SLEW);
        for (sustain, &wanted) in self.sustains.iter_mut().zip(&wanted.sustains) {
            *sustain = approach(*sustain, wanted, SUSTAIN_SLEW);
        }
    }
}

/// What a patch asks of the voice, worked out once when it is set.
#[allow(clippy::struct_excessive_bools, reason = "each is a switch of the panel, on or off")]
#[derive(Clone, Copy, Debug)]
struct Settings {
    oscillators: [OscillatorSettings; OSCILLATORS],
    /// Oscillator 3 follows the keyboard.
    follow: bool,
    tune: i32,
    /// The mixer's switches of the noise and of the external input.
    noise_on: bool,
    external_on: bool,
    pink: bool,
    keyboard: i32,
    /// How far the modulation moves the pitch and the cutoff, in octaves
    /// in Q28: nothing with its switch off.
    oscillator_modulation: i32,
    filter_modulation: i32,
    glide: i32,
    /// Where the patch has the potentiometers.
    pots: Pots,
}

impl Settings {
    fn new(patch: &Patch) -> Self {
        let [first, second, third] = patch.oscillators;
        let range = |oscillator: OscillatorPatch| law::RANGES.get(oscillator.range as usize).copied().unwrap_or(0);
        let third_frequency = if patch.oscillator_3_control {
            law::cents(i32::from(third.frequency))
        } else {
            law::share_of(law::FREE_FREQUENCY, i32::from(third.frequency), i32::from(law::FREQUENCY_CENTS))
        };
        let feedback = law::emphasis(patch.emphasis);
        // Above oscillation the block's pitch falls below its cutoff: the
        // cutoff is raised to keep it (law::REGENERATION_TRIM).
        let above = feedback.saturating_sub(law::REGENERATION).max(0);
        let trim = mul_high(above, law::REGENERATION_TRIM) << 4;
        let dial = i32::from(patch.cutoff).saturating_sub(i32::from(law::CUTOFF_ANCHOR_DIAL));
        let [first_control, second_control] = patch.keyboard_control;
        let [third_share, two_thirds] = law::KEYBOARD_CONTROL;
        let keyboard =
            if first_control { third_share } else { 0 }.saturating_add(if second_control { two_thirds } else { 0 });
        Self {
            oscillators: [
                OscillatorSettings { offset: range(first), waveform: first.waveform, on: first.on },
                OscillatorSettings {
                    offset: range(second).saturating_add(law::cents(i32::from(second.frequency))),
                    waveform: second.waveform,
                    on: second.on,
                },
                OscillatorSettings {
                    offset: range(third).saturating_add(third_frequency),
                    waveform: third.waveform,
                    on: third.on,
                },
            ],
            follow: patch.oscillator_3_control,
            tune: law::cents(i32::from(patch.tune)),
            noise_on: patch.noise_on,
            external_on: patch.external_on,
            pink: patch.noise_pink,
            keyboard,
            oscillator_modulation: if patch.oscillator_modulation { law::OSCILLATOR_MODULATION } else { 0 },
            filter_modulation: if patch.filter_modulation { law::FILTER_MODULATION } else { 0 },
            glide: if patch.glide_on && patch.glide > 0 {
                time_constant(law::glide(patch.glide), RATE)
            } else {
                IMMEDIATE
            },
            pots: Pots {
                levels: [law::share(first.volume), law::share(second.volume), law::share(third.volume)],
                noise: scale(law::NOISE_LEVEL, law::share(patch.noise_volume)),
                external: scale(law::EXTERNAL_GAIN, law::square(patch.external_volume)),
                cutoff: law::share_of(law::CUTOFF_DIVISION, dial, 100).saturating_add(trim),
                feedback,
                contour: scale(law::CONTOUR, law::share(patch.contour_amount)),
                mix: law::share(patch.modulation_mix),
                volume: law::share(patch.volume),
                sustains: [law::level(patch.filter_contour.sustain), law::level(patch.loudness_contour.sustain)],
            },
        }
    }
}

/// The Minimoog.
///
/// It holds no reference and allocates nothing. Two instruments given
/// the same patch, drift and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Minimoog {
    patch: Patch,
    settings: Settings,
    /// The potentiometers as they stand, on their way to where the patch
    /// has them.
    pots: Pots,
    allocator: Allocator<1>,
    /// The trigger count the contours last acted on.
    triggers: u32,
    oscillators: [Oscillator; OSCILLATORS],
    drifts: [Drift; OSCILLATORS],
    drift_cents: u8,
    noise: Noise,
    pink: Pink,
    /// The noises' last samples, white, pink and red, and whether the
    /// next sample of the voice takes new ones ([`NOISE_RATE`]).
    noises: (i32, i32, i32),
    noise_due: bool,
    coupling: OnePole,
    ladder: Ladder,
    /// The filter's contour and the loudness contour.
    contours: [RcEnvelope; 2],
    /// The output a sample ago, and the filter that takes its steady part
    /// away on its return to the mixer.
    returned: i32,
    return_coupling: OnePole,
    /// The modulation mix a sample ago, before the wheel: oscillator 3's
    /// wave and the noise.
    modulation: (i32, i32),
    /// The key's pitch, and the pitch as the glide has brought it, in
    /// octaves from [`law::PITCH_KEY`]; what the glide holds below a
    /// unit.
    target: i32,
    pitch: i32,
    pitch_fraction: u32,
    /// The pitch wheel, in octaves, and the modulation wheel's share in
    /// Q31.
    bend: i32,
    wheel: i32,
    volume: u8,
    /// Whether the sustain pedal is down.
    pedal: bool,
    /// The volume's gain as it stands, in Q28, on its way to what the
    /// controller asks.
    volume_gain: VolumeGain,
    /// All sound off has come and the sound is on its way out: the gain
    /// that takes it there, which the note is cut at the end of.
    cut: Option<VolumeGain>,
    /// The messages that came meanwhile, in order, and how many.
    deferred: [(u8, u8, u8); DEFERRED],
    deferred_count: usize,
    /// From the voice's rate to twice the output rate, and from there to
    /// the output rate.
    wide: Decimator,
    decimator: Decimator,
    ignored: u64,
}

impl Default for Minimoog {
    fn default() -> Self {
        Self::new()
    }
}

impl Minimoog {
    /// An instrument at rest playing [`Patch::DEFAULT`], its oscillators
    /// drifting by [`law::DRIFT_DEFAULT_CENTS`].
    #[must_use]
    pub fn new() -> Self {
        let interval = law::DRIFT_SECONDS.saturating_mul(RATE);
        let mut number = 0u32;
        let drifts = core::array::from_fn(|_| {
            number = number.wrapping_add(1);
            Drift::new(SEED, number, interval)
        });
        let mut coupling = OnePole::new();
        coupling.set_cutoff(frequency(law::MIXER_CORNER, RATE));
        let mut return_coupling = OnePole::new();
        return_coupling.set_cutoff(frequency(law::EXTERNAL_CORNER, RATE));
        let mut minimoog = Self {
            patch: Patch::DEFAULT,
            settings: Settings::new(&Patch::DEFAULT),
            pots: Settings::new(&Patch::DEFAULT).pots,
            // Owner's manual, page 8: "If more than one key is held down,
            // only the lowest one has effect"; page 2: "A timing signal
            // begins whenever a key on the keyboard is depressed, and
            // stops when all keys are released."
            allocator: Allocator::new(Mode::Mono(Priority::Low, Trigger::Single)),
            triggers: 0,
            oscillators: core::array::from_fn(|_| Oscillator::new()),
            drifts,
            drift_cents: law::DRIFT_DEFAULT_CENTS,
            noise: Noise::new(SEED),
            pink: Pink::new(),
            noises: (0, 0, 0),
            noise_due: false,
            coupling,
            ladder: Ladder::new(),
            contours: [RcEnvelope::new(), RcEnvelope::new()],
            returned: 0,
            return_coupling,
            modulation: (0, 0),
            target: 0,
            pitch: 0,
            pitch_fraction: 0,
            bend: 0,
            wheel: 0,
            volume: DATA_MAX,
            pedal: false,
            volume_gain: VolumeGain::full(),
            cut: None,
            deferred: [(0, 0, 0); DEFERRED],
            deferred_count: 0,
            wide: Decimator::wide(),
            decimator: Decimator::sharp(),
            ignored: 0,
        };
        minimoog.set_drift(law::DRIFT_DEFAULT_CENTS);
        minimoog.apply_patch();
        minimoog
    }

    /// Puts the instrument back as [`Minimoog::new`] made it, but for
    /// the patch and the drift, which it keeps: silent, its controllers
    /// at rest, the count of ignored messages at zero.
    pub fn reset(&mut self) {
        let (patch, drift) = (self.patch, self.drift_cents);
        *self = Self::new();
        self.set_drift(drift);
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect on
    /// a note that is sounding too, as moving the panel's controls does:
    /// the switches at once, and the knobs that set a level or the
    /// filter over five thousandths of a second at the most, the CUTOFF
    /// FREQUENCY over 85, as a hand turns them through every place
    /// between. With no note sounding, all of it is there at once.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.patch = *patch;
        self.apply_patch();
        if !self.sounding() {
            self.pots = self.settings.pots;
        }
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    /// Sets how far each oscillator's pitch wanders either way, in cents,
    /// held to [`law::DRIFT_LIMIT_CENTS`]. Zero stops the drift at
    /// nothing at once: the oscillators are then exactly in tune.
    pub fn set_drift(&mut self, cents: u8) {
        self.drift_cents = cents.min(law::DRIFT_LIMIT_CENTS);
        for drift in &mut self.drifts {
            drift.set_amount(law::cents(i32::from(self.drift_cents)));
        }
    }

    fn apply_patch(&mut self) {
        let patch = self.patch;
        self.settings = Settings::new(&patch);
        for ((oscillator, source), number) in self.oscillators.iter_mut().zip(patch.oscillators).zip(0u8..) {
            let (waves, width) = match source.waveform {
                Waveform::Triangle => (waveform::TRIANGLE, None),
                // Oscillator 3 has the reversed sawtooth there.
                Waveform::Shark if number == 2 => (waveform::SAWTOOTH, None),
                Waveform::Shark => (waveform::TRIANGLE | waveform::SAWTOOTH, None),
                Waveform::Sawtooth => (waveform::SAWTOOTH, None),
                Waveform::Square => (waveform::PULSE, law::WIDTHS.first()),
                Waveform::Wide => (waveform::PULSE, law::WIDTHS.get(1)),
                Waveform::Narrow => (waveform::PULSE, law::WIDTHS.get(2)),
            };
            oscillator.enable(waves);
            if let Some(&width) = width {
                oscillator.set_width(width);
            }
        }
        let time = |milliseconds: u16| {
            let time = u32::from(milliseconds).saturating_mul(MILLISECOND).max(law::CONTOUR_TIMES.0);
            time_constant(time, RATE)
        };
        let dump = time_constant(law::DUMP_TIME, RATE);
        for (contour, source) in self.contours.iter_mut().zip([patch.filter_contour, patch.loudness_contour]) {
            let decay = time(source.decay);
            contour.set_attack_target(law::ATTACK_TARGET);
            contour.set_attack(time(source.attack));
            contour.set_decay(decay);
            contour.set_release(if patch.decay_on { decay } else { dump });
        }
    }

    /// How many messages [`Minimoog::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// Whether a note is sounding: a key is held, or the loudness
    /// contour of a released one has not yet fallen to rest.
    #[must_use]
    pub fn sounding(&self) -> bool {
        self.allocator.sounding() > 0
    }

    /// Takes one MIDI 1.0 channel voice message. The instrument is one
    /// part and hears every channel.
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0.
    /// - Note on (0x9n); the velocity is not used. At 8' a key sounds its
    ///   MIDI pitch; the instrument's own keyboard is keys 41 to 84, and
    ///   the keys beyond it go on as it would.
    /// - Control change (0xBn) of the modulation wheel (1), volume (7,
    ///   whose gain moves to its new place over five thousandths of a
    ///   second at the most),
    ///   the sustain pedal (64, down from 64, which holds the note as a
    ///   key does), all sound off (120, which takes the sound out over
    ///   five thousandths of a second, then empties the voice, and
    ///   leaves the pedal where it is; a message that comes before then
    ///   is taken when the sound has gone), reset all controllers
    ///   (121, which leaves the volume) and all notes off (123).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven: the pitch
    ///   wheel, [`law::BEND`] either way.
    ///
    /// Anything else is ignored and counted
    /// ([`Minimoog::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        // While all sound off takes the sound out, a message waits for
        // it to have gone; with no room to wait, the sound is cut now.
        if self.cut.is_some() {
            if let Some(slot) = self.deferred.get_mut(self.deferred_count) {
                *slot = (status, data1, data2);
                self.deferred_count = self.deferred_count.saturating_add(1);
                return;
            }
            self.cut();
        }
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & 0xf0 {
            STATUS_NOTE_OFF => self.allocator.key_up(data1),
            STATUS_NOTE_ON if data2 == 0 => self.allocator.key_up(data1),
            STATUS_NOTE_ON => self.allocator.key_down(data1, data2),
            STATUS_CONTROL_CHANGE => self.control_change(data1, data2),
            STATUS_PITCH_BEND => {
                let bend = (i32::from(data2) << 7 | i32::from(data1)).saturating_sub(PITCH_BEND_CENTRE);
                self.bend = law::share_of(law::BEND, bend, PITCH_BEND_CENTRE);
            }
            _ => self.ignore(),
        }
        self.keys();
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, controller: u8, value: u8) {
        match controller {
            // The wheel is an audio taper (service manual, figure 9-12:
            // "50K AUDIO"), taken as the square of its travel.
            CONTROL_MODULATION_WHEEL => {
                let travel = i32::from(value);
                let full = i32::from(DATA_MAX);
                self.wheel = law::share_of(i32::MAX, travel.saturating_mul(travel), full.saturating_mul(full));
            }
            CONTROL_VOLUME => self.volume = value,
            CONTROL_SUSTAIN => {
                self.pedal = value >= PEDAL_DOWN;
                self.allocator.sustain(self.pedal);
            }
            // The sound goes out over the 5.3 ms a volume's gain takes,
            // and the note is cut when it has gone: cut where it stood,
            // the signal would fall to nothing in one sample.
            // The key is let go then and not before: the DECAY switch
            // off ends a note more quickly than the fade.
            CONTROL_ALL_SOUND_OFF => {
                if self.sounding() {
                    self.cut = Some(VolumeGain::full());
                } else {
                    self.cut();
                }
            }
            CONTROL_RESET_CONTROLLERS => {
                self.wheel = 0;
                self.bend = 0;
                self.pedal = false;
                self.allocator.sustain(false);
            }
            CONTROL_ALL_NOTES_OFF => {
                // As a note off for every key: the pedal still holds. Each
                // key up hands the note to another key still held.
                for _ in 0..=HELD_KEYS {
                    match self.allocator.voices().first() {
                        Some(&key) if key.stage == KeyStage::Held => self.allocator.key_up(key.key),
                        _ => break,
                    }
                }
            }
            _ => self.ignore(),
        }
    }

    /// Empties the voice at once: what all sound off comes to.
    fn cut(&mut self) {
        self.cut = None;
        self.allocator.all_keys_up();
        self.allocator.finished(0);
        self.contours = [RcEnvelope::new(), RcEnvelope::new()];
        self.apply_patch();
        self.pots = self.settings.pots;
        self.ladder.reset();
        self.coupling.reset();
        self.return_coupling.reset();
        self.returned = 0;
        // The allocator lets go of the pedal with the keys; the pedal
        // has not moved, and is given back.
        self.allocator.sustain(self.pedal);
        self.take_deferred();
    }

    /// Takes the messages that waited for the sound to go, in order.
    fn take_deferred(&mut self) {
        let (deferred, count) = (self.deferred, self.deferred_count);
        self.deferred_count = 0;
        for &(status, data1, data2) in deferred.iter().take(count) {
            self.event(status, data1, data2);
        }
    }

    /// Releases the note, held by a key or by the pedal, and lifts the
    /// pedal: for when the input stops and no note off will come.
    pub fn release_all(&mut self) {
        self.pedal = false;
        self.allocator.all_keys_up();
        self.keys();
    }

    /// The contours and the pitch take what the keys ask.
    fn keys(&mut self) {
        let Some(&key) = self.allocator.voices().first() else {
            return;
        };
        if key.stage != KeyStage::Rest {
            // The keyboard's voltage is held after the key is up.
            self.target = law::keys(law::PITCH_KEY, key.key);
        }
        if key.triggers != self.triggers {
            self.triggers = key.triggers;
            for contour in &mut self.contours {
                contour.gate(true);
            }
        }
        if !key.gate() {
            for contour in &mut self.contours {
                contour.gate(false);
            }
        }
    }

    /// One oscillator's wave, as its switch selects it.
    #[inline]
    fn wave(waves: Waves, selected: Waveform, third: bool) -> i32 {
        match selected {
            Waveform::Triangle => waves.triangle,
            Waveform::Shark if third => waves.sawtooth.wrapping_neg(),
            Waveform::Shark => {
                waves.triangle.wrapping_add(scale(waves.sawtooth.wrapping_sub(waves.triangle), law::SHARK_SAWTOOTH))
            }
            Waveform::Sawtooth => waves.sawtooth,
            Waveform::Square | Waveform::Wide | Waveform::Narrow => waves.pulse,
        }
    }

    /// One sample of the voice, at its rate.
    #[inline]
    fn sample(&mut self) -> i32 {
        let settings = self.settings;
        self.pots.approach(&settings.pots);
        let pots = self.pots;

        // The glide: the pitch moves a share of the way to the key's.
        if settings.glide == IMMEDIATE {
            self.pitch = self.target;
        } else {
            let distance = self.target.wrapping_sub(self.pitch);
            let moved = accumulate(distance, settings.glide, COEFFICIENT_BITS, &mut self.pitch_fraction);
            self.pitch = self.pitch.wrapping_add(moved);
        }

        // The modulation: oscillator 3 and the noise, mixed, through the
        // wheel. A full swing is one in Q24.
        let (periodic, random) = self.modulation;
        let mixed = periodic.wrapping_add(scale(random.wrapping_sub(periodic), pots.mix));
        let modulation = scale(mixed, self.wheel);
        let pitch_modulation = amplify(modulation, settings.oscillator_modulation);
        let cutoff_modulation = amplify(modulation, settings.filter_modulation);

        // The oscillators, into the mixer.
        let keyed = self.pitch.wrapping_add(self.bend).wrapping_add(settings.tune).wrapping_add(pitch_modulation);
        let free = law::keys(law::PITCH_KEY, law::FREE_KEY);
        let mut sum = 0i32;
        let mut third_wave = 0i32;
        let parts = self.oscillators.iter_mut().zip(&mut self.drifts).zip(settings.oscillators).zip(pots.levels);
        for ((((oscillator, drift), setting), level), number) in parts.zip(0u8..) {
            let third = number == 2;
            let base = if third && !settings.follow { free } else { keyed };
            let pitch = base.wrapping_add(setting.offset).wrapping_add(drift.tick());
            oscillator.set_step(law::raise(PITCH_STEP, pitch));
            let wave = Self::wave(oscillator.tick(None), setting.waveform, third);
            if third {
                third_wave = wave;
            }
            if setting.on {
                sum = sum.wrapping_add(scale(wave, level));
            }
        }

        // The noise: white or pink to the mixer; to the modulation, pink
        // with the switch at white and red with it at pink (service
        // manual, figure 9-8: "PINK OR RED NOISE" to the modulation mix).
        let needs_colour = settings.pink || (pots.mix > 0 && self.wheel > 0);
        self.noise_due = !self.noise_due;
        if self.noise_due {
            let (pink, red) = if needs_colour { self.pink.tick() } else { (0, 0) };
            self.noises = (self.noise.tick(), pink, red);
        }
        let (white, pink, red) = self.noises;
        let (heard, random) = if settings.pink { (pink, red) } else { (white, pink) };
        // The circuit's own noise is what the filter's oscillation grows
        // from.
        if settings.noise_on {
            sum = sum.wrapping_add(scale(heard, pots.noise));
        }
        sum = sum.wrapping_add(white >> law::FLOOR_SHIFT);
        self.modulation = (third_wave, random);

        // The instrument's own output, back through the external input's
        // preamplifier, which clips.
        if settings.external_on && pots.external != 0 {
            let back = self.return_coupling.high_pass(self.returned);
            sum = sum.wrapping_add(saturate(amplify(back, pots.external)));
        }
        let mixed = self.coupling.high_pass(sum);

        // The filter: the knob, the keyboard, the contour, the
        // modulation.
        let [filter_contour, loudness_contour] = &mut self.contours;
        let [filter_sustain, loudness_sustain] = pots.sustains;
        filter_contour.set_sustain(filter_sustain);
        loudness_contour.set_sustain(loudness_sustain);
        let keyboard = self.pitch.wrapping_sub(law::keys(law::PITCH_KEY, law::KEYBOARD.0));
        let cutoff = pots
            .cutoff
            .wrapping_add(scale(keyboard, settings.keyboard))
            .wrapping_add(mul_high(filter_contour.tick(), pots.contour) << 2)
            .wrapping_add(cutoff_modulation);
        self.ladder.set(law::raise(CUTOFF_STEP, cutoff), pots.feedback);
        // The pair that reads the ladder's top stage bends as the ladder's
        // own do (law::OUTPUT_STAGE).
        let filtered = saturate(self.ladder.tick(mixed));

        // The amplifier and the output's volume.
        let output = scale(apply(filtered, loudness_contour.tick()), pots.volume);
        self.returned = output;
        output
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is the same whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        let mut volume = volume::wanted(self.volume);
        for sample in out {
            let mut voice: [i32; 4] = core::array::from_fn(|_| self.sample());
            // Faded before the decimators, which then end on silence:
            // what they still hold when the voice is emptied is nothing.
            if let Some(cut) = &mut self.cut {
                let gain = cut.towards(0);
                voice = voice.map(|sample| amplify(sample, gain));
                if gain == 0 {
                    // The messages that waited are taken now, the
                    // volume's among them.
                    self.cut();
                    volume = volume::wanted(self.volume);
                }
            }
            let [first, second, third, fourth] = voice;
            let (earlier, later) = (self.wide.process(first, second), self.wide.process(third, fourth));
            let signal = amplify(self.decimator.process(earlier, later), self.volume_gain.towards(volume));
            *sample = to_sample(amplify(signal, OUTPUT_GAIN));
            if self.contours.last().is_some_and(|contour| contour.stage() == Stage::Rest) {
                self.allocator.finished(0);
            }
        }
    }
}

const _: () = assert!(OCTAVE == 1 << 24);
/// The amplifier's curve is the one `saturate` applies.
const _: () = assert!(law::OUTPUT_STAGE == crate::analog::filter::SATURATION);
