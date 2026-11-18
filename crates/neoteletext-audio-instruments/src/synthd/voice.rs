//! synthD's voice: the sound engine and its reader of channel messages.

use crate::analog::allocator::{Allocator, Mode, Priority, Stage as KeyStage, Trigger};
use crate::analog::decimator::Decimator;
use crate::analog::drift::Drift;
use crate::analog::envelope::{ENVELOPE_ONE, IMMEDIATE, RcEnvelope, Stage, apply, time_constant};
use crate::analog::filter::{Ladder, ONE_Q28, OnePole, saturate};
use crate::analog::fixed::{accumulate, amplify, mul_high, scale};
use crate::analog::oscillator::{Noise, Oscillator, Waves, waveform};
use crate::analog::volume::{self, VolumeGain};
use crate::analog::{frequency, to_sample};

use super::interface::{self, PATCH_BYTES};
use super::law::{self, OCTAVE, RATE};
use super::patch::{self, OscillatorPatch, Patch, Waveform};

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

use crate::midi::common::PEDAL_DOWN;
use crate::midi::{DATA_MAX, KIND_MASK, PITCH_BEND_CENTRE, PITCH_BEND_MAX, controller, status};

/// The modulation wheel's whole travel squared: the denominator of its
/// square law (SD-9).
const WHEEL_TOP_SQUARED: i32 = (PITCH_BEND_MAX as i32) * (PITCH_BEND_MAX as i32);

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
/// Source: own choice, measured (`tests/synthd_control_sweep.rs`, on
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

/// synthD.
///
/// It holds no reference and allocates nothing. Two instruments given
/// the same patch, drift and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct SynthD {
    patch: Patch,
    settings: Settings,
    /// The potentiometers as they stand, on their way to where the patch
    /// has them.
    pots: Pots,
    /// The 60 controllers' values the panel stands at (SD-17, SD-22).
    values: [u8; PATCH_BYTES],
    /// The panel moved since the voice last took it: the voice takes it
    /// before its next sample, so that the controls moved at one instant
    /// reach it together, and a control moved and moved back reaches it
    /// as nothing (MIDI-97). Whether the potentiometers go there at once:
    /// no note was sounding when the panel first moved.
    pending: Option<bool>,
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
    /// The pitch wheel, in octaves, from its 14-bit value and its range
    /// in octaves (SD-6); the modulation wheel's share in Q31, from its
    /// 14-bit number (SD-9).
    bend: i32,
    bend_value: u16,
    bend_range: i32,
    wheel: i32,
    wheel_number: u16,
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

impl Default for SynthD {
    fn default() -> Self {
        Self::new()
    }
}

impl SynthD {
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
        let mut synthd = Self {
            values: interface::DEFAULT_PATCH,
            pending: None,
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
            bend_value: PITCH_BEND_CENTRE,
            bend_range: law::BEND,
            wheel: 0,
            wheel_number: 0,
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
        synthd.set_drift(law::DRIFT_DEFAULT_CENTS);
        synthd.apply_patch();
        synthd
    }

    /// Puts the instrument back as [`SynthD::new`] made it, but for
    /// the patch and the drift, which it keeps: silent, its controllers
    /// at rest, the count of ignored messages at zero.
    pub fn reset(&mut self) {
        let (values, drift) = (self.values, self.drift_cents);
        *self = Self::new();
        self.set_drift(drift);
        self.set_values(&values);
    }

    /// Gives the instrument a patch, which it copies. It takes effect on
    /// a note that is sounding too, as moving the panel's controls does:
    /// the switches at once, and the knobs that set a level or the
    /// filter over five thousandths of a second at the most, the CUTOFF
    /// FREQUENCY over 85, as a hand turns them through every place
    /// between. With no note sounding, all of it is there at once.
    ///
    /// The patch is held as its 60 values (SD-22), each field at its
    /// canonical value: [`SynthD::patch`] then gives it back as those
    /// values read.
    pub fn set_patch(&mut self, patch: &Patch) {
        self.set_values(&patch.to_values());
    }

    /// Sets the whole panel at one instant from a patch's 60 values, as
    /// their 60 control changes would (SD-12, SD-22).
    pub fn set_values(&mut self, values: &[u8; PATCH_BYTES]) {
        let values = values.map(|value| value & DATA_MAX);
        if values != self.values {
            self.values = values;
            self.moved();
        }
    }

    /// The panel's 60 values.
    #[must_use]
    pub const fn values(&self) -> &[u8; PATCH_BYTES] {
        &self.values
    }

    /// The modulation wheel's 14-bit number (SD-9).
    #[must_use]
    pub const fn modulation_wheel(&self) -> u16 {
        self.wheel_number
    }

    /// The pitch wheel's 14-bit value, centre 8,192 (SD-6).
    #[must_use]
    pub const fn pitch_bend(&self) -> u16 {
        self.bend_value
    }

    /// Whether the sustain pedal is down.
    #[must_use]
    pub const fn pedal(&self) -> bool {
        self.pedal
    }

    /// Controller 7's value (SD-10).
    #[must_use]
    pub const fn volume(&self) -> u8 {
        self.volume
    }

    /// The panel moved: it is read again, and the voice takes it before
    /// its next sample.
    fn moved(&mut self) {
        self.patch = Patch::from_values(&self.values);
        if self.pending.is_none() {
            self.pending = Some(!self.sounding());
        }
    }

    /// The voice takes the panel moved since it last did.
    fn take_panel(&mut self) {
        if let Some(at_once) = self.pending.take() {
            self.apply_patch();
            if at_once {
                self.pots = self.settings.pots;
            }
        }
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    /// Sets the pitch wheel's range at either end, in cents (SD-6,
    /// MIDI-90); the wheel where it stands bends by the new range.
    pub fn set_bend_range_cents(&mut self, cents: u16) {
        self.bend_range = law::cents(i32::from(cents.min(interface::MOST_BEND_RANGE_CENTS)));
        self.bend_from_wheel();
    }

    /// The pitch wheel's bend in octaves: `(value − 8,192) ÷ 8,192` of the
    /// range (SD-6).
    fn bend_from_wheel(&mut self) {
        let centre = i32::from(PITCH_BEND_CENTRE);
        self.bend = law::share_of(self.bend_range, i32::from(self.bend_value).saturating_sub(centre), centre);
    }

    /// The modulation wheel's share from its 14-bit number: the square of
    /// its travel (SD-9; the service manual, figure 9-12: "50K AUDIO").
    fn wheel_from_number(&mut self) {
        let number = i32::from(self.wheel_number);
        self.wheel = law::share_of(i32::MAX, number.saturating_mul(number), WHEEL_TOP_SQUARED);
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

    /// How many messages [`SynthD::event`] has ignored since the
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
    /// part and hears every channel alike (SD-2).
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0 (SD-3,
    ///   SD-4); note on, whose velocity is not used. At 8' a key sounds its
    ///   MIDI pitch (SD-5); the keys are taken as section 5 of `synthd.md`
    ///   says: the lowest held sounds (SD-14), the contours start only from
    ///   silence (SD-15).
    /// - Control change (0xBn) of a panel controller of SD-17 (the
    ///   Voyager's, and NeoTeletext's 89, 90 and 102 to 113), read by
    ///   SD-18 to SD-20; the modulation wheel, 1 and 33 (SD-9); volume, 7,
    ///   whose gain moves to its new place over 5.3 ms at the most (SD-10);
    ///   the sustain pedal, 64; all sound off, 120, which takes the sound
    ///   out over 5.3 ms, then empties the voice, a message coming before
    ///   then being taken when the sound has gone; reset all controllers,
    ///   121, which centres the wheels, lifts the pedal and turns the GLIDE
    ///   switch off (SD-13 item 3); all notes off and the mode messages,
    ///   123 to 127 (SD-11, SD-13 item 4).
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven, at the range of
    ///   [`SynthD::set_bend_range_cents`] (SD-6).
    ///
    /// Anything else is ignored and counted
    /// ([`SynthD::ignored_events`]): any other status or controller, a
    /// data byte above 127. Nothing is an error. Registered parameter
    /// numbers and program changes are the receiver's front end's
    /// ([`super::midi::Midi`]).
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
        match status & KIND_MASK {
            status::NOTE_OFF => self.allocator.key_up(data1),
            status::NOTE_ON if data2 == 0 => self.allocator.key_up(data1),
            status::NOTE_ON => self.allocator.key_down(data1, data2),
            status::CONTROL_CHANGE => self.control_change(data1, data2),
            status::PITCH_BEND => {
                self.bend_value = u16::from(data2) << 7 | u16::from(data1);
                self.bend_from_wheel();
            }
            _ => self.ignore(),
        }
        self.keys();
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, number: u8, value: u8) {
        match number {
            // A coarse value sets the seven high bits and clears the low;
            // a fine value sets the low (SD-9, MIDI-75).
            controller::MODULATION => {
                self.wheel_number = u16::from(value) << 7;
                self.wheel_from_number();
            }
            interface::MODULATION_FINE => {
                self.wheel_number = (self.wheel_number & !u16::from(DATA_MAX)) | u16::from(value);
                self.wheel_from_number();
            }
            controller::VOLUME => self.volume = value,
            controller::SUSTAIN => {
                self.pedal = value >= PEDAL_DOWN;
                self.allocator.sustain(self.pedal);
            }
            // The sound goes out over the 5.3 ms a volume's gain takes,
            // and the note is cut when it has gone: cut where it stood,
            // the signal would fall to nothing in one sample.
            // The key is let go then and not before: the DECAY switch
            // off ends a note more quickly than the fade.
            controller::ALL_SOUND_OFF => {
                if self.sounding() {
                    self.cut = Some(VolumeGain::full());
                } else {
                    self.cut();
                }
            }
            // RP-015 (SD-13 item 3, DEC-6473): the wheels to rest, the
            // pedal up, and the GLIDE switch, which carries controller 65's
            // own meaning, off. Volume, the panel's other controls and the
            // bend range stay.
            controller::RESET_ALL_CONTROLLERS => {
                self.wheel_number = 0;
                self.wheel_from_number();
                self.bend_value = PITCH_BEND_CENTRE;
                self.bend_from_wheel();
                self.pedal = false;
                self.allocator.sustain(false);
                self.panel(controller::PORTAMENTO, 0);
            }
            // The mode messages are all notes off and nothing more
            // (MIDI-89).
            controller::ALL_NOTES_OFF..=controller::POLY_ON => {
                // As a note off for every key held (SD-11): the pedal still
                // holds the note. The keys that do not sound go first, so
                // that the note is not handed to one of them on its way
                // out, and the key that sounds last.
                let sounding = self.allocator.voices().first().map(|voice| voice.key);
                for key in (0..=DATA_MAX).filter(|&key| Some(key) != sounding) {
                    self.allocator.key_up(key);
                }
                if let Some(key) = sounding {
                    self.allocator.key_up(key);
                }
            }
            _ => {
                if !self.panel(number, value) {
                    self.ignore();
                }
            }
        }
    }

    /// A control change of a panel controller (SD-17): answers whether
    /// `number` is one.
    fn panel(&mut self, number: u8, value: u8) -> bool {
        match interface::set_value(&mut self.values, number, value) {
            Some(true) => {
                self.moved();
                true
            }
            Some(false) => true,
            None => false,
        }
    }

    /// Empties the voice at once: what all sound off comes to.
    fn cut(&mut self) {
        self.cut = None;
        self.pending = None;
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
        self.take_panel();
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
                    self.take_panel();
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
