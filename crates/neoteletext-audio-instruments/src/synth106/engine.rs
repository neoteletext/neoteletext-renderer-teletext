//! The sound engine of synth106 and synth60: the voices, the switch
//! filters, the amplifier and the chorus, and the keys and performance
//! controls that play them ([`Synth106`]). What the messages of the
//! adopted interfaces mean is the family's interface
//! ([`super::interface`]); the receiver plays the engine through
//! [`super::midi::Midi`].

use crate::SAMPLE_RATE;
use crate::analog::allocator::{Allocator, Assign, Mode, Stage};
use crate::analog::chorus::Chorus;
use crate::analog::decimator::Decimator;
use crate::analog::drift::Variation;
use crate::analog::filter::{ONE_Q28, OnePole};
use crate::analog::fixed::{amplify, limit, mul_high, scale};
use crate::analog::lfo::{LFO_ONE, Lfo, Shape};
use crate::analog::oscillator::Noise;
use crate::analog::volume::VolumeGain;
use crate::analog::{LEVEL, frequency, to_sample};
use crate::midi::common::PEDAL_DOWN;
use crate::midi::{DATA_MAX, KIND_MASK, controller, status};

use super::law::{self, Law, OCTAVE};
use super::patch::{self, ChorusMode, Model, Patch, Slider};
use super::voice::{self, CUTOFF_TOLERANCE, Controls, FRAME_TICKS, Settings, TICK_RATE, TICK_SAMPLES, Voice};

/// Voices of the instrument.
///
/// Source: JUNO-106 service notes, page 1: "6 Voice"; JUNO-60 service
/// notes, page 14, "Key assignment": "Six channels".
pub const VOICES: usize = 6;

/// How far the pitch bend wheel bends at either end before controller 87
/// or registered parameter 0 sets it, in semitones (S106-26).
///
/// Source: S106-26: "Until either message arrives the range is two
/// semitones", as built. On the instruments it is a knob of up to an
/// octave (synth106) or 700 cents (synth60), outside the patch.
pub const PITCH_BEND_SEMITONES: i32 = 2;
const _: () = assert!(
    PITCH_BEND_SEMITONES * crate::midi::common::CENTS_PER_SEMITONE as i32
        == super::interface::DEFAULT_BEND_RANGE_CENTS as i32
);
/// How far the modulation wheel at its top makes the low-frequency
/// oscillator move the pitch either way, in cents.
///
/// Source: S106-8, "The full depth is ±50 cents", as built (own choice).
/// On the instruments it is the bender panel's LFO knob, outside the
/// patch.
pub const WHEEL_CENTS: i32 = 50;

/// The pitch bend value that bends nothing (MIDI-91).
const PITCH_BEND_CENTRE: i32 = crate::midi::PITCH_BEND_CENTRE as i32;
/// The lowest key that sounds as itself; a lower one sounds whole octaves
/// up (S106-5).
///
/// Source: Juno-106 manual, page 34, note number recognised "True voice
/// 24-108", and page 29.
pub const LOWEST_KEY: u8 = 24;
/// The highest key that sounds as itself; a higher one sounds whole
/// octaves down (S106-5).
///
/// Source: as [`LOWEST_KEY`].
pub const HIGHEST_KEY: u8 = 108;
/// Keys in an octave.
const OCTAVE_KEYS: u8 = 12;
// Seven octaves and a key: every key folds into the range in whole
// octaves, from either side.
const _: () = assert!(HIGHEST_KEY - LOWEST_KEY >= OCTAVE_KEYS);

/// The key a received key sounds: moved by the fewest whole octaves that
/// bring it into [`LOWEST_KEY`] to [`HIGHEST_KEY`] (S106-5).
#[must_use]
pub const fn sounding_key(key: u8) -> u8 {
    let mut key = key;
    while key < LOWEST_KEY {
        key = key.saturating_add(OCTAVE_KEYS);
    }
    while key > HIGHEST_KEY {
        key = key.saturating_sub(OCTAVE_KEYS);
    }
    key
}

/// The seed of the instrument's noise, of its voices' tolerances and of
/// its oscillators that hold a random level.
///
/// Source: own choice: any number; the same instrument every time.
const SEED: u32 = 0x4a75_6e6f;

/// The chorus's sweep rates in thousandths of a hertz: I, II, and both
/// buttons of synth60.
///
/// Source: Roland's Juno-60 chorus LFO schematic (as scanned by Florian
/// Anwander), simulated in ngspice on 18 November 2026: a Schmitt trigger
/// (R7 33k, R12 47k, R14 3.3k, R102 270k) around an integrator (R8 100k,
/// C3 0.1 uF) fed through R11 270k and a 4066 shunt of R9 3.9k (I) or
/// R10 6.8k (II), none for I+II, where R6 150k and C2 1 uF also follow
/// it. I 0.506 Hz and II 0.854 Hz agree with the schematic's own notes
/// (one cycle in 2 s and 1.2 s). I+II is 9.64 Hz: only that rate leaves
/// the 1.6 V the schematic notes after R6 and C2.
const CHORUS_RATES: [u32; 3] = [506, 854, 9_640];
/// The chorus's middle delay and its sweep either way with one button
/// down, in millionths of a second.
///
/// Source: Roland's Juno-60 chorus board schematic (as scanned by
/// Florian Anwander), simulated in ngspice on 18 November 2026. The
/// MN3009's clock comes from a transistor oscillator (TR1 to TR5, C3
/// 150 pF, R5 6.8k, R7 8.2k, R11 1.8k, R12 10k) driving the MN3101's
/// OX1; the MN3101 halves it (its datasheet: "Clock signal frequency is
/// 1/2 of oscillation frequency") and the MN3009's 256 stages delay by
/// 256 / (2 x clock) (its datasheet), so the delay is 256 over the
/// oscillator's frequency. Over the chorus LFO's 18.9 V swing it runs
/// from 1.44 to 5.24 ms, linearly in the LFO's voltage. The longest
/// delay is the least certain: a slower switching MN3101 than modelled
/// would shorten it, to 3.5 ms at worst.
const CHORUS_DELAY: (u32, u32) = (3_340, 1_900);
/// The sweep with both buttons down, as a share of the sweep with one,
/// in hundredths.
///
/// Source: the same schematic: 1.6 V against 20 V.
const CHORUS_BOTH_PERCENT: u32 = 8;
/// The cutoff of the filters before and after each delay line, in
/// thousandths of a hertz.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): each
/// is two stages of 22 kΩ twice with 820 pF and 680 pF, then 1,800 pF and
/// 270 pF: 1 / (2 π R √(C1 C2)) is 9.7 kHz and 10.4 kHz, with the two
/// stages' peaks of a fourth-order filter with none overall.
const CHORUS_FILTER: u32 = 10_000_000;
/// The share of each delay line against the dry signal in the mono
/// output: 47 / 78, in Q31.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): each
/// output amplifier (IC6) adds the dry signal through 47 kΩ (R71, R73)
/// and one line through 39 kΩ (R72, R74), a line being 47 / 39 of the
/// dry signal; the mono jack takes the mean of the two outputs, in which
/// the dry signal is whole and each line half that.
const CHORUS_LINE_GAIN: i32 = ((47i64 << 31) / 78) as i32;

/// Samples over which the lines' share of the output falls to nothing or
/// rises from it when the chorus's switches move while a note sounds: 20
/// ms. A line cut off or let in at once is a step as large as the line's
/// signal at that instant, a click; so is a sweep that changes its depth
/// or its direction at once, which moves the lines' delays in one step,
/// and that change is made while the lines are not heard.
///
/// Source: own choice of the time and of the straight line; 20 ms, as the
/// synth106's all sound off fades, because at 10 ms a switch from I+II to I
/// under a held pulse stepped 1.5 times the instrument's own largest step
/// once the sweep took the rates derived from the schematic. JUNO-60
/// service notes, chorus board schematic (page 11), "MUTE": each line
/// reaches its output amplifier through a FET (TR8, TR16) whose gate the
/// CHORUS OFF line moves through resistors and a capacitor, so the
/// instrument's chorus comes and goes softly too; that circuit's time
/// was not worked out.
const CHORUS_FADE_SAMPLES: u32 = SAMPLE_RATE / 50;
/// Samples the lines are given to fill, and their filters to settle,
/// after the sweep is set and before the lines are let in: 6 ms.
///
/// Source: own choice: just over the longest delay of the sweep, 5.24 ms
/// ([`CHORUS_DELAY`]).
const CHORUS_FILL_SAMPLES: u32 = SAMPLE_RATE * 6 / 1000;
/// What one step of the fade is of the lines' whole share, in Q31.
const CHORUS_FADE_UNIT: i32 = i32::MAX / CHORUS_FADE_SAMPLES.cast_signed();
const _: () = assert!(
    CHORUS_FILL_SAMPLES as u64 * 1_000_000 > (CHORUS_DELAY.0 + CHORUS_DELAY.1) as u64 * SAMPLE_RATE as u64
        && CHORUS_FADE_SAMPLES > 0
);
/// What the signal into the chorus stays under while nothing in it could
/// be heard to step: half a unit of a 16-bit sample before the output's
/// gain. Once it has stayed under for [`CHORUS_FILL_SAMPLES`], longer
/// than the longest delay, the lines hold nothing either, and the
/// chorus's switches may move at once.
///
/// Source: own choice: under the sample's last bit.
const QUIET_LEVEL: i32 = LEVEL >> 16;

/// The corner below which the voices' sum does not pass, in thousandths
/// of a hertz: what takes away the steady part of a pulse that is not a
/// square, which would otherwise stand in the output for as long as the
/// note.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): the
/// voices' sum enters the board through C14, 10 µF, across R39, 33 kΩ,
/// before the high-pass switch IC3: 1 / (2 π R C) is 0.48 Hz. JUNO-60
/// service notes, chorus board schematic (page 11): the same parts
/// before its high-pass switch IC5, C47, 10 µF, across R70, 33 kΩ.
const COUPLING_CORNER: u32 = 482;

/// The corner of the bass lift, in thousandths of a hertz.
///
/// Source: JUNO-106 service notes, jack board schematic (page 15): in
/// the path that position 0 of the high-pass switch adds, R22, 47 kΩ,
/// with C9, 0.047 µF: 1 / (2 π R C) is 72 Hz.
const BASS_BOOST_CORNER: u32 = 72_000;
/// The gain of the low frequencies the lift adds, in Q28: 100 / 47.
///
/// Source: the same schematic, as far as it was read: in that path the
/// amplifier has R18, 100 kΩ, against R20, 47 kΩ. Below the corner
/// the lift is then 1 + 100 / 47, 9.9 dB, and at 55 Hz 8.2 dB; one
/// reference program, measured, lifts 55 Hz by 8.3 dB.
const BASS_BOOST_GAIN: i32 = ((100i64 << 28) / 47) as i32;

/// Samples over which the high-pass switch's filter and the bass lift come
/// and go when the switch moves while anything passes through them: 10
/// ms, the chorus's ([`CHORUS_FADE_SAMPLES`]). A filter emptied or cut out
/// in one sample is a step as large as what it held: under a low note
/// 0.15 of full scale, and 0.06 in silence after a wide pulse, whose
/// steady part the capacitor before the switch still gives up and a
/// filter at the switch's top keeps from the output.
///
/// Source: own choice of the time. On the instruments an analogue switch
/// (IC3 of synth106's jack board, IC5 of synth60's chorus board)
/// puts another capacitor in the path at once; how the capacitors then
/// settle was not worked out.
const HIGH_PASS_FADE_SAMPLES: u32 = SAMPLE_RATE / 100;
/// What one step of the bass lift's fade is of its whole, in Q31.
const HIGH_PASS_FADE_UNIT: i32 = i32::MAX / HIGH_PASS_FADE_SAMPLES.cast_signed();
/// The highest corner of either model's high-pass switch, in thousandths
/// of a hertz.
const HIGH_PASS_TOP: u32 = {
    /// The larger of two corners.
    const fn larger(one: u32, other: u32) -> u32 {
        if one > other { one } else { other }
    }
    let ([a, b, c, d], [e, f, g, h]) = (law::SYNTH106.hpf, law::SYNTH60.hpf);
    larger(larger(larger(a, b), larger(c, d)), larger(larger(e, f), larger(g, h)))
};
/// The most the high-pass filter's corner moves in a sample, as a
/// frequency step: the whole of the highest corner in
/// [`HIGH_PASS_FADE_SAMPLES`].
const HIGH_PASS_SLEW: u32 = frequency(HIGH_PASS_TOP, SAMPLE_RATE).div_ceil(HIGH_PASS_FADE_SAMPLES);
const _: () = assert!(HIGH_PASS_SLEW > 0);
/// The corner at which the high-pass filter's capacitor, once the switch
/// has left it out of the path, gives up what it still holds, in
/// thousandths of a hertz: a time constant of 10 ms.
///
/// Source: own choice: the fade's time ([`HIGH_PASS_FADE_SAMPLES`]).
const HIGH_PASS_DISCHARGE: u32 = 15_915;
/// [`HIGH_PASS_DISCHARGE`] as a frequency step.
const HIGH_PASS_DISCHARGE_STEP: u32 = frequency(HIGH_PASS_DISCHARGE, SAMPLE_RATE);

/// The amplifier's level slider moves the level by this either way, in
/// octaves of amplitude: 6 dB.
///
/// Source: own choice: the slider is marked -5 to +5 and no document
/// gives its range.
const VCA_LEVEL_RANGE: i32 = OCTAVE;
/// The slider's middle, where the level is one.
///
/// Source: own choice with [`VCA_LEVEL_RANGE`]: 64 of 0 to 127, the
/// slider's 0 between -5 and +5, where [`Patch::DEFAULT`] has it.
const VCA_LEVEL_CENTRE: i32 = 64;
/// The right shift of the share of the way the amplifier's level moves
/// in a sample towards a level a patch has newly set: a time constant of
/// 64 samples, 1.3 ms. A level set in one sample under a note is a step
/// as large as the note's signal at that instant times the change.
///
/// Source: own choice: the nearest power of two to the 1 ms that follows
/// each of synth106's held control voltages (service notes, module
/// board schematic, page 13: 10 kΩ onto 0.1 µF); the level's own path
/// was not read.
const VCA_LEVEL_SLEW_SHIFT: u32 = 6;
/// The share of the mixed output that is a sample, in Q28: 494 / 1000.
/// One voice's sawtooth through an open filter then peaks 16.8 dB below
/// full scale, and six struck together 3.4 dB below.
/// What nears full scale is bent under it by the output's limiter
/// ([`crate::output::limit`]).
///
/// Source: own choice: the gain at which the reference note stands at
/// [`crate::REFERENCE_LEVEL_TENTHS`] (`tests/levels.rs`); with the
/// output's half, as it was first, the note stood 0.1 dB above it.
const OUTPUT_GAIN: i32 = (ONE_Q28 as i64 * 494 / 1000) as i32;

/// The whole of the low-frequency oscillator's delay that is silence;
/// the rise to full depth then takes half as long again.
///
/// Source: own choice of the rise. JUNO-60 service notes, adjustment 7-4
/// and figure 31: with the delay at 10 the modulation "2 seconds later,
/// begins to reappear" and grows; the specifications' 1.5 s and 3 s are
/// taken as the silence.
const LFO_FADE_SHARE: u32 = 2;
/// Millionths of a second in a second.
const MICROSECONDS: u64 = 1_000_000;

/// synth106.
///
/// It holds no reference and allocates nothing. Two instruments of one
/// model given the same patch and messages and rendered in calls of any
/// lengths give the same samples.
#[derive(Clone, Debug)]
pub struct Synth106 {
    model: Model,
    law: &'static Law,
    patch: Patch,
    settings: Settings,
    allocator: Allocator<VOICES>,
    voices: [Voice; VOICES],
    /// The trigger count each voice last acted on.
    triggers: [u32; VOICES],
    noise: Noise,
    /// The oscillator after its delay, and the same without, for the
    /// wheel.
    lfo: Lfo,
    vibrato: Lfo,
    /// The pitch bend, less its centre.
    bend: i32,
    /// The range of a full bend either way, in cents (S106-26).
    bend_range_cents: u16,
    wheel: u8,
    /// Whether the sustain pedal is down.
    pedal: bool,
    volume: u8,
    /// The volume's gain as it stands, in Q28, on its way to what the
    /// controller asks.
    volume_gain: VolumeGain,
    /// Samples of the voices until the next tick, and ticks until the
    /// next frame.
    tick_wait: u32,
    frame_wait: u32,
    /// The share of the way a held control voltage moves in a tick.
    smoothing: i32,
    decimator: Decimator,
    coupling: OnePole,
    high_pass: OnePole,
    bass: OnePole,
    /// The high-pass filter's corner as it stands, a frequency step, on
    /// its way to the corner of the patch's switch; at zero the filter is
    /// out of the path and only gives up what it holds.
    corner: u32,
    corner_wanted: u32,
    /// Steps of its fade the bass lift has risen, 0 to
    /// [`HIGH_PASS_FADE_SAMPLES`].
    lift: u32,
    /// Samples for which the signal into the high-pass switch and the
    /// signal out of it have both stayed under [`QUIET_LEVEL`], counted
    /// no further than [`CHORUS_FILL_SAMPLES`].
    still: u32,
    /// The amplifier's level in Q28, and the level the patch asks, which
    /// it follows.
    level: i32,
    level_wanted: i32,
    chorus: Chorus,
    /// The setting the lines are swept by now, which follows the patch's
    /// at once or by way of a fade; off, the lines are not run.
    sweep: ChorusMode,
    /// Steps of the fade the lines' share of the output has risen, 0 to
    /// [`CHORUS_FADE_SAMPLES`], and samples the lines still have to fill
    /// before it rises.
    wet: u32,
    fill: u32,
    /// Samples for which the signal into the chorus has stayed under
    /// [`QUIET_LEVEL`], counted no further than [`CHORUS_FILL_SAMPLES`].
    quiet: u32,
    ignored: u64,
}

impl Synth106 {
    /// An instrument at rest playing its model's plain patch
    /// ([`Patch::default_for`]).
    #[must_use]
    pub fn new(model: Model) -> Self {
        let law = match model {
            Model::Synth106 => &law::SYNTH106,
            Model::Synth60 => &law::SYNTH60,
        };
        let mut voice = 0u32;
        let voices = core::array::from_fn(|_| {
            let trim = Variation::new(SEED, voice).offset(CUTOFF_TOLERANCE);
            voice = voice.wrapping_add(1);
            Voice::new(model, trim)
        });
        let patch = Patch::default_for(model);
        let mut synth106 = Self {
            model,
            law,
            patch,
            settings: Settings::new(&patch, model, law),
            // Each key takes the next voice round the six (S106-34):
            // JUNO-60 service notes, page 14, "Key assignment"; JUNO-106
            // service notes, page 18, the test mode's "ROTARY". A seventh
            // key held does not sound ([`Synth106::key_down`]); a key when
            // fewer are held takes the voice released longest ago.
            allocator: Allocator::new(Mode::Poly(Assign::Cyclic)),
            voices,
            triggers: [0; VOICES],
            noise: Noise::new(SEED),
            lfo: Lfo::new(SEED),
            vibrato: Lfo::new(SEED),
            bend: 0,
            bend_range_cents: super::interface::DEFAULT_BEND_RANGE_CENTS,
            wheel: 0,
            pedal: false,
            volume: DATA_MAX,
            volume_gain: VolumeGain::full(),
            tick_wait: 0,
            frame_wait: 0,
            smoothing: voice::hold_smoothing(),
            decimator: Decimator::sharp(),
            coupling: OnePole::new(),
            high_pass: OnePole::new(),
            bass: OnePole::new(),
            corner: 0,
            corner_wanted: 0,
            lift: 0,
            still: CHORUS_FILL_SAMPLES,
            level: ONE_Q28,
            level_wanted: ONE_Q28,
            chorus: Self::empty_chorus(),
            sweep: ChorusMode::Off,
            wet: 0,
            fill: 0,
            quiet: CHORUS_FILL_SAMPLES,
            ignored: 0,
        };
        synth106.coupling.set_cutoff(frequency(COUPLING_CORNER, SAMPLE_RATE));
        synth106.bass.set_cutoff(frequency(BASS_BOOST_CORNER, SAMPLE_RATE));
        synth106.apply_patch();
        synth106
    }

    /// The chorus with nothing in its lines, before a patch sets its
    /// delay and its sweep.
    fn empty_chorus() -> Chorus {
        let mut chorus = Chorus::new(SAMPLE_RATE);
        chorus.set_filter_cutoff(frequency(CHORUS_FILTER, SAMPLE_RATE));
        chorus
    }

    /// Which instrument this is.
    #[must_use]
    pub const fn model(&self) -> Model {
        self.model
    }

    /// Puts the instrument back as [`Synth106::new`] made it, but for the
    /// patch, which it keeps: silent, its controllers at rest, the count
    /// of ignored messages at zero.
    pub fn reset(&mut self) {
        let patch = self.patch;
        *self = Self::new(self.model);
        self.set_patch(&patch);
    }

    /// Gives the instrument a patch, which it copies. It takes effect at
    /// once, on the voices that are sounding too, as moving the panel's
    /// controls does; but the chorus, switched while anything passes
    /// into it, goes and comes over a hundredth of a second, so that its
    /// lines are not cut into the output or out of it in one step; the
    /// high-pass switch's filter and bass lift, moved while anything
    /// passes through them, take as long to come to their new places,
    /// so that nothing they hold is let out or cut off in one step; and
    /// the amplifier's level then moves to its new place over a
    /// thousandth of a second or so. A setting the model has not is
    /// taken as the nearest it has
    /// ([`ChorusMode::Both`], [`super::PwmSource::Envelope`]).
    ///
    /// The patch the instrument already plays changes nothing at all: no
    /// envelope, oscillator, low-frequency oscillator or filter is touched,
    /// so a restatement of the state a receiver holds is not heard
    /// (MIDI-97, S106-40).
    pub fn set_patch(&mut self, patch: &Patch) {
        if *patch == self.patch {
            return;
        }
        self.patch = *patch;
        self.apply_patch();
    }

    /// The patch being played.
    #[must_use]
    pub const fn patch(&self) -> &Patch {
        &self.patch
    }

    fn apply_patch(&mut self) {
        let patch = self.patch;
        let law = self.law;
        self.settings = Settings::new(&patch, self.model, law);
        for voice in &mut self.voices {
            voice.configure(&self.settings);
        }

        let rate = law::exponential(law.lfo_rate.0, law.lfo_rate.1, patch.get(Slider::LfoRate));
        self.lfo.set_step(frequency(rate, TICK_RATE));
        self.vibrato.set_step(frequency(rate, TICK_RATE));
        let delay = law::share(i32::try_from(law.lfo_delay).unwrap_or(i32::MAX), patch.get(Slider::LfoDelay));
        let ticks = u64::from(delay.cast_unsigned()).saturating_mul(u64::from(TICK_RATE)) / MICROSECONDS;
        let ticks = u32::try_from(ticks).unwrap_or(u32::MAX);
        self.lfo.set_delay(ticks, ticks / LFO_FADE_SHARE);

        let corner = law.hpf.get(usize::from(patch.hpf.min(patch::HPF_MAX))).copied().unwrap_or(0);
        self.corner_wanted = frequency(corner, SAMPLE_RATE);
        if self.still >= CHORUS_FILL_SAMPLES {
            // Nothing passes into the switch's filters and they hold
            // nothing: at once, and empty. That nothing comes out of them
            // is not enough: a filter at the switch's top keeps from the
            // output the charge the capacitor before it gives up for
            // seconds after a wide pulse's note, and taken out of the
            // path in one sample it lets all of that through at once.
            self.corner = self.corner_wanted;
            self.set_high_pass();
            self.high_pass.reset();
            self.bass.reset();
            self.lift = if self.bass_lift() { HIGH_PASS_FADE_SAMPLES } else { 0 };
        }
        // Otherwise the corner and the lift move to their places in
        // `render`, a sample at a time.
        let slider = i32::from(patch.get(Slider::VcaLevel)).saturating_sub(VCA_LEVEL_CENTRE);
        let octaves = slider.saturating_mul(VCA_LEVEL_RANGE) / VCA_LEVEL_CENTRE;
        self.level_wanted = law::raise(ONE_Q28.cast_unsigned(), octaves).cast_signed();
        if self.quiet >= CHORUS_FILL_SAMPLES {
            // Nothing passes through the amplifier: at once.
            self.level = self.level_wanted;
        }

        let wanted = self.chorus_mode();
        if self.quiet >= CHORUS_FILL_SAMPLES {
            // Nothing is in the lines or on its way into them, so nothing
            // can click: at once. That no voice sounds is not enough: for
            // seconds after a wide pulse's note the coupling capacitor
            // gives up the charge the pulse's steady part left on it, and
            // a line cut out of that, or let into it, is a step as large
            // as what is left.
            self.sweep_by(wanted);
            self.fill = 0;
            self.wet = if wanted == ChorusMode::Off { 0 } else { CHORUS_FADE_SAMPLES };
        } else if self.sweep != wanted
            && let (Some((_, now)), Some((_, then))) = (Self::sweep_of(self.sweep), Self::sweep_of(wanted))
            && now == then
        {
            // I to II or back: the same sweep at another rate, which the
            // lines' delays follow without a step. A patch that leaves
            // the sweep as it is leaves the chorus alone: set again, the
            // lines' filters would be put to their clocks at another
            // sample than they otherwise are, and a chorus switched off
            // and on again before its lines had gone would not come back
            // to the sample.
            self.sweep_by(wanted);
        }
        // Any other change is made in `render`, while the lines are not
        // heard.
    }

    /// Whether the patch's high-pass switch lifts the bass.
    fn bass_lift(&self) -> bool {
        self.law.bass_boost && self.patch.hpf == 0
    }

    /// Sets the high-pass filter to its corner as it stands; with the
    /// filter out of the path, to the corner its capacitor empties at.
    fn set_high_pass(&mut self) {
        self.high_pass.set_cutoff(if self.corner == 0 { HIGH_PASS_DISCHARGE_STEP } else { self.corner });
    }

    /// One sample of the high-pass switch: the filter at its corner, which
    /// moves towards the switch's by [`HIGH_PASS_SLEW`] at the most, and
    /// the bass lift, which comes and goes over
    /// [`HIGH_PASS_FADE_SAMPLES`]. Nothing in it is ever emptied or cut
    /// out at once while a signal passes: a filter out of the path gives
    /// up what it holds, and the lift's low-pass is fed less and less.
    #[inline]
    fn high_pass_switch(&mut self, input: i32, lift: bool) -> i32 {
        if self.corner != self.corner_wanted {
            self.corner = if self.corner < self.corner_wanted {
                self.corner.saturating_add(HIGH_PASS_SLEW).min(self.corner_wanted)
            } else {
                self.corner.saturating_sub(HIGH_PASS_SLEW).max(self.corner_wanted)
            };
            self.set_high_pass();
        }
        let mut signal = if self.corner == 0 {
            limit(input).saturating_sub(self.high_pass.low_pass(0))
        } else {
            self.high_pass.high_pass(input)
        };
        let whole = if lift { HIGH_PASS_FADE_SAMPLES } else { 0 };
        if self.lift < whole {
            self.lift = self.lift.saturating_add(1);
        } else if self.lift > whole {
            self.lift = self.lift.saturating_sub(1);
        }
        if self.law.bass_boost {
            let into = if self.lift == HIGH_PASS_FADE_SAMPLES {
                signal
            } else {
                scale(signal, self.lift.cast_signed().saturating_mul(HIGH_PASS_FADE_UNIT))
            };
            signal = signal.saturating_add(amplify(self.bass.low_pass(into), BASS_BOOST_GAIN));
        }
        self.still = if input.saturating_abs() < QUIET_LEVEL && signal.saturating_abs() < QUIET_LEVEL {
            self.still.saturating_add(1).min(CHORUS_FILL_SAMPLES)
        } else {
            0
        };
        signal
    }

    /// The patch's chorus setting as the model has it.
    const fn chorus_mode(&self) -> ChorusMode {
        match (self.patch.chorus, self.model) {
            (ChorusMode::Both, Model::Synth106) => ChorusMode::Two,
            (mode, _) => mode,
        }
    }

    /// The sweep of a setting: its rate in thousandths of a hertz, and
    /// its depth in hundredths of [`CHORUS_DELAY`]'s, its shape and
    /// whether the second line is swept opposite to the first. Nothing
    /// for off.
    const fn sweep_of(mode: ChorusMode) -> Option<(u32, (u32, Shape, bool))> {
        match mode {
            ChorusMode::Off => None,
            ChorusMode::One => Some((CHORUS_RATES[0], (100, Shape::Triangle, true))),
            ChorusMode::Two => Some((CHORUS_RATES[1], (100, Shape::Triangle, true))),
            // With both buttons down the second line is no longer swept
            // the other way (Anwander: "In the 'fast mode' the second
            // BBD is controlled in a non inverted way"), and the sweep is
            // "sine like".
            ChorusMode::Both => Some((CHORUS_RATES[2], (CHORUS_BOTH_PERCENT, Shape::Sine, false))),
        }
    }

    /// Sets the lines' sweep to a setting's.
    fn sweep_by(&mut self, mode: ChorusMode) {
        if let Some((rate, (percent, shape, opposite))) = Self::sweep_of(mode) {
            // While the chorus is off its lines are not run, to spare
            // the work, and so keep what they held when it was switched
            // off. The instrument's lines run on and have long emptied
            // by the time they are heard again: a chorus switched on
            // starts empty.
            if self.sweep == ChorusMode::Off {
                self.chorus = Self::empty_chorus();
            }
            self.chorus.set_delay(CHORUS_DELAY.0, CHORUS_DELAY.1.saturating_mul(percent) / 100);
            self.chorus.set_rate(frequency(rate, SAMPLE_RATE));
            self.chorus.set_shape(shape);
            self.chorus.set_opposite(opposite);
        }
        self.sweep = mode;
    }

    /// One sample of the chorus's switching: towards the patch's setting
    /// by way of silence of the lines. Gives the lines' share of the
    /// output in Q31, or nothing while they are heard whole.
    #[inline]
    fn chorus_fade(&mut self, wanted: ChorusMode) -> Option<i32> {
        if self.sweep != wanted {
            if self.wet == 0 {
                self.sweep_by(wanted);
                self.fill = CHORUS_FILL_SAMPLES;
            } else {
                self.wet = self.wet.saturating_sub(1);
            }
        } else if self.fill > 0 {
            self.fill = self.fill.saturating_sub(1);
        } else if self.wet < CHORUS_FADE_SAMPLES && wanted != ChorusMode::Off {
            self.wet = self.wet.saturating_add(1);
        }
        (self.wet < CHORUS_FADE_SAMPLES).then(|| self.wet.cast_signed().saturating_mul(CHORUS_FADE_UNIT))
    }

    /// How many messages [`Synth106::event`] has ignored since the
    /// instrument was made or reset.
    #[must_use]
    pub const fn ignored_events(&self) -> u64 {
        self.ignored
    }

    /// How many voices are sounding, 0 to [`VOICES`].
    #[must_use]
    pub fn sounding_voices(&self) -> usize {
        self.allocator.sounding()
    }

    /// The envelope position of the newest voice that is not at rest and
    /// the count of voices not at rest (page-rules.md, RULE-31).
    #[must_use]
    pub fn meter(&self) -> crate::midi::Meter {
        let newest = self.allocator.newest().and_then(|(index, gate)| Some((self.voices.get(index)?, gate)));
        let position = newest.map(|(voice, gate)| crate::midi::Meter::keyed(voice.meter(), gate));
        crate::midi::Meter::of(position, self.allocator.sounding())
    }

    /// Takes one MIDI 1.0 channel message straight, without the common
    /// layer: for the tests and examples that play the engine by itself.
    /// The receiver plays it through [`super::midi::Midi`], which reads
    /// every message as the family document says; what this reads, it
    /// reads the same way. The instrument is one part and hears every
    /// channel (S106-2).
    ///
    /// - Note off (0x8n), and note on (0x9n) with a velocity of 0
    ///   (S106-4).
    /// - Note on (0x9n); the velocity is not used (S106-3).
    /// - Control change (0xBn) of the modulation wheel (1, S106-8), volume
    ///   (7, S106-28), the sustain pedal (64, down from 64 on both models,
    ///   S106-9 and MIDI-87), all sound off (120), reset all controllers
    ///   (121, as RP-015 says of what this engine has: the wheel to 0, the
    ///   bender to its centre, the pedal up; the bend range and the volume
    ///   stay, S106-29), all notes off (123) and the four mode messages
    ///   (124 to 127), which are an all notes off and nothing more.
    /// - Pitch bend (0xEn), 14 bits, `data1` the low seven (S106-7).
    ///
    /// Anything else is ignored and counted ([`Synth106::ignored_events`]):
    /// any other status or controller, a data byte above 127. Nothing is
    /// an error.
    pub fn event(&mut self, status: u8, data1: u8, data2: u8) {
        if data1 > DATA_MAX || data2 > DATA_MAX {
            self.ignore();
            return;
        }
        match status & KIND_MASK {
            status::NOTE_OFF => self.key_up(data1),
            status::NOTE_ON if data2 == 0 => self.key_up(data1),
            status::NOTE_ON => self.key_down(data1),
            status::CONTROL_CHANGE => self.control_change(data1, data2),
            status::PITCH_BEND => self.set_bend(u16::from(data2) << 7 | u16::from(data1)),
            _ => self.ignore(),
        }
    }

    fn ignore(&mut self) {
        self.ignored = self.ignored.saturating_add(1);
    }

    fn control_change(&mut self, number: u8, value: u8) {
        match number {
            controller::MODULATION => self.set_wheel(value),
            controller::VOLUME => self.set_volume(value),
            controller::SUSTAIN => self.sustain(value >= PEDAL_DOWN),
            controller::ALL_SOUND_OFF => self.all_sound_off(),
            controller::RESET_ALL_CONTROLLERS => self.reset_controllers(),
            controller::ALL_NOTES_OFF..=controller::POLY_ON => self.all_notes_off(),
            _ => self.ignore(),
        }
    }

    /// A key goes down (S106-3, S106-34). It sounds at
    /// the key `sounding_key` gives (S106-5) and is known by the key received. While
    /// six keys are held, a seventh does not sound: "If 6 keys are
    /// simultaneously pressed, no more key will sound" (Juno-106 manual,
    /// page 22; DEC-6465 answer 22). A key already sounding takes its own
    /// voice again.
    pub fn key_down(&mut self, key: u8) {
        let voices = self.allocator.voices();
        let held = voices.iter().filter(|voice| voice.stage == Stage::Held).count();
        let again = voices.iter().any(|voice| voice.stage != Stage::Rest && voice.key == key);
        if held >= VOICES && !again {
            return;
        }
        // The oscillator's delay starts when a key goes down with no other
        // held (JUNO-60 service notes, adjustment 7-4, "TRIG MODE to
        // AUTO").
        if held == 0 {
            self.lfo.key();
        }
        // The velocity is not used (S106-3).
        self.allocator.key_down(key, DATA_MAX);
    }

    /// A key goes up (S106-4); with the pedal down its voice goes on.
    pub fn key_up(&mut self, key: u8) {
        self.allocator.key_up(key);
    }

    /// The sustain pedal goes down or up (S106-9, MIDI-87).
    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
        self.allocator.sustain(down);
    }

    /// Every key held is released as a note off of it would release it;
    /// the pedal still holds (S106-29 item 3, MIDI-85).
    pub fn all_notes_off(&mut self) {
        let voices = *self.allocator.voices();
        for voice in voices.iter().filter(|voice| voice.stage == Stage::Held) {
            self.allocator.key_up(voice.key);
        }
    }

    /// Every voice to silence, by the amplifier's closing over a few
    /// thousandths of a second, then emptied; the pedal, the controllers
    /// and the patch stay (S106-29 item 1, MIDI-86).
    pub fn all_sound_off(&mut self) {
        self.allocator.all_keys_up();
        let keys = *self.allocator.voices();
        for (voice, key) in self.voices.iter_mut().zip(&keys) {
            if key.stage == Stage::Rest {
                voice.empty(&self.settings);
            } else {
                // Its amplifier closes over a few thousandths of a second,
                // and the voice is emptied and comes to rest at the frame
                // that finds it closed.
                voice.cut();
            }
        }
        // The allocator lets go of the pedal with the keys; the pedal has
        // not moved, and is given back.
        self.allocator.sustain(self.pedal);
    }

    /// What reset all controllers sets of this engine's controls, as
    /// RP-015 says (S106-29 item 2, MIDI-84): the wheel to 0, the bender to
    /// its centre and the pedal up, which releases what it held. The bend
    /// range, the volume and the patch stay.
    pub fn reset_controllers(&mut self) {
        self.wheel = 0;
        self.bend = 0;
        self.sustain(false);
    }

    /// The pitch bend, 14 bits, centre 8,192 (S106-7, MIDI-91).
    pub fn set_bend(&mut self, value: u16) {
        self.bend = i32::from(value.min(crate::midi::PITCH_BEND_MAX)).saturating_sub(PITCH_BEND_CENTRE);
    }

    /// The range of a full bend either way, in cents, held to
    /// [`super::interface::MOST_BEND_RANGE_CENTS`] (S106-26).
    pub fn set_bend_range_cents(&mut self, cents: u16) {
        self.bend_range_cents = cents.min(super::interface::MOST_BEND_RANGE_CENTS);
    }

    /// The range of a full bend either way, in cents.
    #[must_use]
    pub const fn bend_range_cents(&self) -> u16 {
        self.bend_range_cents
    }

    /// The modulation wheel, 0 to 127 (S106-8).
    pub fn set_wheel(&mut self, value: u8) {
        self.wheel = value.min(DATA_MAX);
    }

    /// Controller 7, 0 to 127 (S106-28).
    pub fn set_volume(&mut self, value: u8) {
        self.volume = value.min(DATA_MAX);
    }

    /// Releases every voice, held by a key or by the pedal, and lifts
    /// the pedal: for when the input stops and no note off will come.
    pub fn release_all(&mut self) {
        self.pedal = false;
        self.allocator.all_keys_up();
    }

    /// A tick, and at its turn a frame: the voices take what the keys
    /// and the controls ask.
    fn control(&mut self) {
        let lfo = self.lfo.tick();
        let vibrato = self.vibrato.tick();
        if self.frame_wait == 0 {
            self.frame_wait = match self.model {
                Model::Synth106 => FRAME_TICKS,
                Model::Synth60 => 1,
            };
            // The bender's whole travel either way is so many keys.
            let range = law::cents(i32::from(self.bend_range_cents));
            let bend = law::share_of(range, self.bend, PITCH_BEND_CENTRE);
            let wheel = law::share(law::cents(WHEEL_CENTS), self.wheel);
            let controls = Controls { lfo, pitch: bend.saturating_add(mul_high(wheel, vibrato) << 2) };
            let keys = *self.allocator.voices();
            for (index, ((voice, key), seen)) in self.voices.iter_mut().zip(&keys).zip(&mut self.triggers).enumerate() {
                if key.stage == Stage::Rest {
                    continue;
                }
                if *seen != key.triggers {
                    *seen = key.triggers;
                    voice.start(sounding_key(key.key), &self.settings);
                }
                if !key.gate() {
                    voice.release();
                }
                voice.frame(&self.settings, controls);
                if key.stage == Stage::Released && voice.silent() {
                    if voice.cutting() {
                        voice.empty(&self.settings);
                    }
                    self.allocator.finished(index);
                }
            }
        }
        self.frame_wait = self.frame_wait.saturating_sub(1);
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                voice.tick(&self.settings, self.law, self.smoothing);
            }
        }
    }

    /// One sample of the voices' sum, at their rate.
    #[inline]
    fn voices_sample(&mut self) -> i32 {
        if self.tick_wait == 0 {
            self.tick_wait = TICK_SAMPLES;
            self.control();
        }
        self.tick_wait = self.tick_wait.saturating_sub(1);
        let noise = self.noise.tick();
        let mut sum = 0i32;
        for (voice, key) in self.voices.iter_mut().zip(self.allocator.voices()) {
            if key.stage != Stage::Rest {
                sum = sum.wrapping_add(voice.sample(&self.settings, noise));
            }
        }
        sum
    }

    /// Renders the next `out.len()` samples: 16 bits, mono, 48,000 a
    /// second. The work of a sample is at most that of six voices and
    /// the chorus, whatever messages came.
    pub fn render(&mut self, out: &mut [i16]) {
        let volume = crate::analog::volume::wanted(self.volume);
        let lift = self.bass_lift();
        let chorus = self.chorus_mode();
        for sample in out {
            let earlier = self.voices_sample();
            let later = self.voices_sample();
            let mut signal = self.decimator.process(earlier, later);
            signal = self.coupling.high_pass(signal);
            signal = self.high_pass_switch(signal, lift);
            if self.level != self.level_wanted {
                let step = self.level_wanted.saturating_sub(self.level) >> VCA_LEVEL_SLEW_SHIFT;
                // A share cut to nothing would never arrive.
                self.level = if step == 0 { self.level_wanted } else { self.level.saturating_add(step) };
            }
            signal = amplify(amplify(signal, self.level), self.volume_gain.towards(volume));
            self.quiet = if signal.saturating_abs() < QUIET_LEVEL {
                self.quiet.saturating_add(1).min(CHORUS_FILL_SAMPLES)
            } else {
                0
            };
            let fade = self.chorus_fade(chorus);
            if self.sweep != ChorusMode::Off {
                let [first, second] = self.chorus.process(signal);
                let lines = scale(first.saturating_add(second), CHORUS_LINE_GAIN);
                signal = signal.saturating_add(fade.map_or(lines, |share| scale(lines, share)));
            }
            *sample = to_sample(amplify(signal, OUTPUT_GAIN));
        }
    }
}

/// The low-frequency oscillator's peak, which the laws' depths are
/// multiplied by.
const _: () = assert!(LFO_ONE == crate::analog::envelope::ENVELOPE_ONE);
