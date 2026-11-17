//! One voice: the oscillator's divider and waveforms, the mixer, the
//! low-pass filter with its resonance, the envelope and the amplifier.
//!
//! A voice runs at twice the output rate. What changes slowly is set at
//! intervals: every [`TICK_SAMPLES`] samples a tick sets the filter and
//! the amplifier's slope, and every frame (one tick on the Juno-60,
//! [`FRAME_TICKS`] on the Juno-106, whose processor writes each control
//! voltage once in its cycle) the envelope, the pitch and the cutoff are
//! worked out afresh.

use crate::analog::envelope::{
    ENVELOPE_ONE, Fall, IMMEDIATE, REST_LEVEL, RcEnvelope, Stage, SteppedEnvelope, apply, linear_step, time_constant,
};
use crate::analog::filter::{ONE_Q28, OtaCascade};
use crate::analog::fixed::{amplify, mul_high, mul_high_unsigned, scale};
use crate::analog::oscillator::{Oscillator, waveform};
use crate::analog::{Oversampling, STEP_LIMIT, frequency};

use super::Model;
use super::law::{self, Law, OCTAVE};
use super::patch::{Patch, PwmSource, Range, Slider};

/// How far above the output rate the voices run.
///
/// Source: own choice: the oscillators' edges are 91 dB clean at twice
/// the output rate and 48 dB at the rate itself
/// ([`crate::analog::oscillator`]), and the filter distorts.
pub const OVERSAMPLING: Oversampling = Oversampling::Twice;
/// The voices' rate in hertz.
pub const RATE: u32 = OVERSAMPLING.rate();
/// Bits of the samples in a tick.
const TICK_BITS: u32 = 4;
/// Samples of the voices between two ticks: a sixth of a millisecond.
///
/// Source: own choice: short against the 1 ms of the instrument's own
/// control smoothing, and a power of two for the amplifier's slope.
pub const TICK_SAMPLES: u32 = 1 << TICK_BITS;
/// Ticks a second.
pub const TICK_RATE: u32 = RATE / TICK_SAMPLES;
/// Ticks in a frame of the Juno-106: 25 are 4.17 ms.
///
/// Source: JUNO-106 service notes, page 8, "D/A & S/H timing chart": one
/// cycle of the converter through every sample-and-hold is marked
/// "4.2ms".
pub const FRAME_TICKS: u32 = 25;
const _: () = assert!(TICK_RATE == 6_000 && RATE.is_multiple_of(TICK_SAMPLES));

/// The time constant of what follows a sample-and-hold of the Juno-106,
/// in millionths of a second.
///
/// Source: JUNO-106 service notes, module board schematic (page 13): the
/// filter's control passes R113, 10 kΩ, onto C61, 0.1 µF, and the
/// amplifier's R106, 10 kΩ, onto C58, 0.1 µF: 1 ms each.
const HOLD_SMOOTHING_MICROSECONDS: u32 = 1_000;

/// The Juno-106's master clock in hertz, and what divides it for 8'.
///
/// Source: JUNO-106 service notes, page 8, "OSC": "a master oscillator
/// (8MHz) and a divider IC35. The IC35 divides 8MHz by two, four or
/// eight according to a position of RANGE (4', 8', 16')".
const CLOCK_106: u64 = 8_000_000;
/// See [`CLOCK_106`].
const CLOCK_106_DIVIDER_EIGHT: u64 = 4;
/// The largest divisor of a 16-bit programmable timer.
///
/// Source: JUNO-106 service notes, page 8: the DCOs "are 16-bit
/// Programmable Interval Timers"; JUNO-60 service notes, page 14: "by up
/// to 65535".
const DIVISOR_MAX: u64 = 65_535;
/// The Juno-60's master oscillator at rest, in hertz: 4,305 times 440.
///
/// Source: JUNO-60 service notes, page 14, "Programmable Counters":
/// "Assume that the master oscillator runs at 1902810Hz and a divisor is
/// 4305, the counter develops 442Hz". Own choice: the instrument is
/// tuned to an A of 440 Hz, not the notes' 442 Hz, so the oscillator is
/// the notes' divisor times 440.
const MASTER_60: u32 = 4_305 * 440;
/// The Juno-60's master oscillator's range in hertz.
///
/// Source: JUNO-60 service notes, page 14, "Master Oscillators": "from
/// 1MHz to 3.5MHz".
const MASTER_60_BOUNDS: (u32, u32) = (1_000_000, 3_500_000);
/// The pitch of the A above middle C, in thousandths of a hertz, and its
/// key.
///
/// Source: own choice of the standard pitch; MIDI 1.0 for the key.
const A4: (u32, u8) = (440_000, 69);
/// Thousandths of a hertz in a hertz.
const MILLIHERTZ: u64 = 1_000;

/// The number of the Juno-106's converter that gives a sawtooth of the
/// full 12 V at a divisor of one: 12 V times R C f over the converter's
/// step, with R C f = 400 / divisor (200 kΩ and 1,000 pF against 2 MHz
/// over the divisor) and a step of 10 V / 4,096.
///
/// Source: JUNO-106 service notes, page 9, "Sawtooth": the DCO CV charges
/// C54 through R85 to R87 "and discharges through transistor E on the
/// positive going edges of DCO", keeping the amplitude "nearly constant
/// (approx. 12Vp-p)"; module board schematic (page 13): R85 399 kΩ, R87
/// 200 kΩ, R86 100 kΩ, C54 0.001 µF; page 8, timing chart: "DCO CV, SUB
/// LEVEL (0 to -10V)" from a 12-bit converter. Own derivation: a ramp of
/// slope V / (R C) for a period 1 / f peaks at V / (R C f).
const SAWTOOTH_CODE: u64 = 12 * 400 * 4_096 / 10;
/// The converter's largest number. Above the pitch that needs it the
/// sawtooth shrinks, as the owner's manual says of keys above C8 (printed
/// page 22: "the sound will become lower (if it is saw-tooth wave)").
const CONVERTER_MAX: u64 = 4_095;
/// The 16' resistor against four times the 4' one: the sawtooth at 16'
/// is larger by 400 / 399.
///
/// Source: the same schematic: R85 is 399 kΩ where 400 kΩ would match.
const SIXTEEN_FOOT_RATIO: (u64, u64) = (400, 399);
/// The second of [`SIXTEEN_FOOT_RATIO`], which divides.
const SIXTEEN_FOOT: u64 = SIXTEEN_FOOT_RATIO.1;

/// The pulse's width at the slider's foot, a square, and how much wider
/// the slider's top makes it: 50% and 45% of a cycle.
///
/// Source: JUNO-106 service notes, adjustment 10 (page 19): "50% duty
/// cycle" at PWM 0 and "93 - 97%" with PWM at 10; JUNO-60 service notes,
/// adjustments 9-1 and 9-2: 50% and 95%.
const PULSE_WIDTH: (u32, u32) = (1 << 31, ((45u64 << 32) / 100) as u32);

/// The resonance at the slider's top, in Q28, as the feedback of the
/// filter's loop: the loop oscillates from four, which the slider reaches
/// at 117 of 127.
///
/// Source: own choice. The documents say only "0 to self oscillation";
/// the amplifier that sets it is linear in its control current. The two
/// reference programs begin to oscillate between 112 and 120 of 127 and
/// between 0.90 and 0.95 of the travel.
const RESONANCE_TOP: i32 = (ONE_Q28 as i64 * 435 / 100) as i32;
/// Half: the share of the resonance that is added to the filter's input
/// gain, so that the passband falls less as the resonance rises.
///
/// Source: Electric Druid, "Roland filter designs with the IR3109 or
/// AS3109", on the Juno-6: the input is also fed to the resonance
/// amplifier's other input, so "as you increase the amount of resonance,
/// you also add more input signal"; the write-up of the chip by
/// `AMSynths` ("All about the IR3109 chip"): the Juno-106's module is the same circuit "with Q compensation".
/// Own choice of the amount: the resistors that set it were not read.
const COMPENSATION_SHIFT: u32 = 1;
/// How flat the filter oscillates at [`RESONANCE_TOP`], in octaves: the
/// cutoff is raised by this at the slider's top, and by its share above
/// the onset, so that the oscillation is in tune where the factory trims
/// it.
///
/// Source: measured on this filter (`tests/juno_filter.rs`). JUNO-106
/// service notes, adjustment 7, and JUNO-60 service notes, adjustment
/// 8-2: the cutoff's trimmer is set with the resonance at 10, by the
/// pitch of the oscillation.
const OSCILLATION_FLAT: i32 = law::cents(31);
/// The feedback at which the loop begins to oscillate, in Q28.
///
/// Source: Zavalishin, "The Art of VA Filter Design", section 5.1.
const OSCILLATION_ONSET: i32 = 4 * ONE_Q28;
/// The feedback from the onset to the slider's top.
const OSCILLATION_SPAN: i64 = (RESONANCE_TOP - OSCILLATION_ONSET) as i64;
const _: () = assert!(OSCILLATION_SPAN > 0);
/// The right shift of the noise that is always in the filter's input: 78
/// dB below a waveform. An amplifier's own noise is what starts a real
/// filter oscillating; without any, this one would stay at rest for ever.
///
/// Source: own choice of the level.
const HISS_SHIFT: u32 = 13;
/// The right shift of the mixer's sum into the filter. A waveform at
/// full level then stands at an eighth of the level where the filter's
/// stages saturate, and the filter's own oscillation at the slider's
/// top, whose size that level sets, comes out as large as a square wave:
/// 0.9 dB more, peak to peak (`tests/juno_filter.rs`). It is also what
/// lets six voices at the filter's bound add inside a word.
///
/// Source: JUNO-60 service notes, adjustments 5-1 and 8-1: a square wave
/// and the oscillation at RES 10 are each trimmed to 4 V peak to peak at
/// the amplifier's output; JUNO-106 service notes, adjustments 5 and 6.
const DRIVE_SHIFT: u32 = 2;
/// Each voice's cutoff is off by up to this, fixed for the voice.
///
/// Source: JUNO-106 service notes, adjustments 7 and 8: each voice's
/// filter is trimmed "within ±10 cents on the tuner".
pub const CUTOFF_TOLERANCE: i32 = law::cents(10);

/// The decay's and the release's time is the time to fall to a tenth of
/// the way: 1 / ln 10 time constants, in ten-thousandths.
///
/// Source: own choice of what the specifications' "12s" measures. An
/// exponential never arrives; a tenth is what is left when a trace on an
/// oscilloscope has visibly landed. The two reference programs' longest
/// releases have time constants of 4.8 s and 12.6 s; this gives 5.2 s.
const FALL_TIME_CONSTANT: u64 = 4_343;
/// Ten thousand, of which [`FALL_TIME_CONSTANT`] is a share.
const TEN_THOUSAND: u64 = 10_000;
/// Thousandths, of which the analogue attack's time constant is a share
/// of its time.
const THOUSAND: u64 = 1_000;

/// What a patch asks of every voice, worked out once when it is set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Settings {
    /// The oscillator's waveforms in use.
    waves: u8,
    range: Range,
    sub_level: i32,
    noise_level: i32,
    /// The filter's feedback in Q28, and the input gain that goes with
    /// it.
    resonance: i32,
    compensation: i32,
    /// Octaves from the law's base to the cutoff, before the key, the
    /// envelope and the oscillator.
    cutoff: i32,
    keyboard: u8,
    /// Octaves the envelope at its peak moves the cutoff, signed.
    cutoff_envelope: i32,
    /// Octaves the low-frequency oscillator at its peak moves the cutoff.
    cutoff_lfo: i32,
    /// Octaves the low-frequency oscillator at its peak moves the pitch.
    pitch_lfo: i32,
    pwm: PwmSource,
    pulse_width: i32,
    vca_gate: bool,
    attack: u32,
    decay: u32,
    release: u32,
    sustain: i32,
}

impl Settings {
    /// The settings of a patch on a model.
    pub fn new(patch: &Patch, model: Model, law: &Law) -> Self {
        let mut waves = 0;
        if patch.sawtooth {
            waves |= waveform::SAWTOOTH;
        }
        if patch.pulse {
            waves |= waveform::PULSE;
        }
        let sub = patch.get(Slider::Sub);
        if patch.sub && sub > 0 {
            waves |= waveform::SUB;
        }
        let resonance = law::share(RESONANCE_TOP, patch.get(Slider::VcfResonance));
        // Above the onset the oscillation is flat by its share of the
        // most; the cutoff is raised to meet it.
        let above = resonance.saturating_sub(OSCILLATION_ONSET).max(0);
        let tuning = i64::from(OSCILLATION_FLAT).saturating_mul(i64::from(above)) / OSCILLATION_SPAN;
        let envelope = law::share(law.cutoff_envelope, patch.get(Slider::VcfEnvelope));
        Self {
            waves,
            range: patch.range,
            sub_level: law::level(sub),
            noise_level: law::level(patch.get(Slider::Noise)),
            resonance,
            compensation: ONE_Q28.saturating_add(resonance >> COMPENSATION_SHIFT),
            cutoff: law::share(law.cutoff_span, patch.get(Slider::VcfFrequency))
                .saturating_add(i32::try_from(tuning).unwrap_or(0)),
            keyboard: patch.get(Slider::VcfKeyboard),
            cutoff_envelope: if patch.envelope_inverted { envelope.saturating_neg() } else { envelope },
            cutoff_lfo: law::share(law.cutoff_lfo, patch.get(Slider::VcfLfo)),
            pitch_lfo: law::share(law.pitch_lfo, patch.get(Slider::DcoLfo)),
            pwm: match (patch.pwm, model) {
                (PwmSource::Envelope, Model::Juno106) => PwmSource::Manual,
                (source, _) => source,
            },
            pulse_width: law::level(patch.get(Slider::DcoPwm)),
            vca_gate: patch.vca_gate,
            attack: law::cubic(law.attack.0, law.attack.1, patch.get(Slider::Attack)),
            decay: law::cubic(law.decay.0, law.decay.1, patch.get(Slider::Decay)),
            release: law::cubic(law.decay.0, law.decay.1, patch.get(Slider::Release)),
            sustain: law::level(patch.get(Slider::Sustain)),
        }
    }
}

/// What every voice is given at a frame: the instrument's modulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controls {
    /// The low-frequency oscillator after its delay, in Q30 either way.
    pub lfo: i32,
    /// Octaves the bender and the modulation wheel move the pitch.
    pub pitch: i32,
}

/// The time constant of a fall that takes a time, in millionths of a
/// second.
fn fall_time_constant(time: u32) -> u32 {
    u32::try_from(u64::from(time).saturating_mul(FALL_TIME_CONSTANT) / TEN_THOUSAND).unwrap_or(u32::MAX)
}

/// A value that follows its target as a capacitor follows a voltage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Smoothed {
    value: i32,
    target: i32,
}

impl Smoothed {
    const fn new() -> Self {
        Self { value: 0, target: 0 }
    }

    /// One tick: a share of the way, or all of it at once.
    fn tick(&mut self, coefficient: i32) -> i32 {
        let distance = self.target.saturating_sub(self.value);
        let step = scale(distance, coefficient);
        // A share cut to nothing would never arrive.
        self.value = if coefficient == IMMEDIATE || step == 0 { self.target } else { self.value.saturating_add(step) };
        self.value
    }
}

/// One of the six voices.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Voice {
    model: Model,
    oscillator: Oscillator,
    filter: OtaCascade,
    /// The Juno-106's envelope, which its processor computes each frame.
    stepped: SteppedEnvelope,
    /// The Juno-60's envelope, a chip that charges a capacitor.
    analogue: RcEnvelope,
    key: u8,
    gate: bool,
    /// The Juno-60's divisor for the key, fixed while it sounds.
    divisor: u32,
    sawtooth_gain: i32,
    cutoff: Smoothed,
    level: Smoothed,
    /// The amplifier's gain in Q30 and what it gains each sample.
    gain: i32,
    gain_step: i32,
    /// The voice's own offset of the cutoff, in octaves.
    trim: i32,
}

impl Voice {
    /// A voice at rest, whose filter is off by `trim` octaves.
    pub fn new(model: Model, trim: i32) -> Self {
        Self {
            model,
            oscillator: Oscillator::new(),
            filter: OtaCascade::new(),
            stepped: SteppedEnvelope::new(),
            analogue: RcEnvelope::new(),
            key: A4.1,
            gate: false,
            divisor: 1,
            sawtooth_gain: ENVELOPE_ONE,
            cutoff: Smoothed::new(),
            level: Smoothed::new(),
            gain: 0,
            gain_step: 0,
            trim,
        }
    }

    /// Takes a patch's settings: the envelope's times and the
    /// oscillator's waveforms.
    pub fn configure(&mut self, settings: &Settings) {
        self.oscillator.enable(settings.waves);
        let fall = |time: u32| fall_time_constant(time);
        match self.model {
            Model::Juno106 => {
                // The attack is a straight line to the peak, the decay
                // and the release exponentials (owner's manual, printed
                // pages 20 and 21, the figures of the envelope), in the
                // frames of the processor: a time constant in frames is
                // FRAME_TICKS times shorter than in ticks.
                let frames = TICK_RATE / FRAME_TICKS;
                self.stepped.set_attack(linear_step(settings.attack, frames));
                self.stepped.set_decay(Fall::Proportional(time_constant(fall(settings.decay), frames)));
                self.stepped.set_release(Fall::Proportional(time_constant(fall(settings.release), frames)));
                self.stepped.set_sustain(settings.sustain);
            }
            Model::Juno60 => {
                // The chip charges towards a level above its peak
                // (JUNO-60 service notes, adjustment 10, figure 38: the
                // attack is a curve that is still rising at its end).
                let attack = u64::from(settings.attack).saturating_mul(THOUSAND) / ATTACK_TIME_CONSTANTS_THOUSANDTHS;
                self.analogue.set_attack(time_constant(u32::try_from(attack).unwrap_or(u32::MAX), TICK_RATE));
                self.analogue.set_decay(time_constant(fall(settings.decay), TICK_RATE));
                self.analogue.set_release(time_constant(fall(settings.release), TICK_RATE));
                self.analogue.set_sustain(settings.sustain);
            }
        }
    }

    /// A key for this voice: the envelope starts again.
    pub fn start(&mut self, key: u8, settings: &Settings) {
        self.key = key;
        self.gate = true;
        self.stepped.retrigger();
        self.analogue.retrigger();
        if self.model == Model::Juno60 {
            // The divisor is the nearest to the master oscillator at
            // rest over the key's frequency in its range: the A's 4,305
            // lowered by the key's interval from it.
            let pitch = law::keys(A4.1, key).saturating_add(range_octaves(settings.range));
            self.divisor = divisor(DIVISOR_OF_A_60, pitch) as u32;
        }
    }

    /// The key is up.
    pub const fn release(&mut self) {
        self.gate = false;
    }

    /// Silences the voice at once.
    pub fn cut(&mut self, settings: &Settings) {
        *self = Self::new(self.model, self.trim);
        self.configure(settings);
    }

    /// Whether the voice, its key up, has fallen silent.
    pub fn silent(&self, settings: &Settings) -> bool {
        let at_rest = match self.model {
            Model::Juno106 => self.stepped.stage() == Stage::Rest,
            Model::Juno60 => self.analogue.stage() == Stage::Rest,
        };
        !self.gate && self.level.value < REST_LEVEL && self.gain < REST_LEVEL && (settings.vca_gate || at_rest)
    }

    /// A frame: the envelope moves on, and the pitch, the pulse's width
    /// and the targets of the cutoff and of the amplifier are set.
    pub fn frame(&mut self, settings: &Settings, controls: Controls) {
        let envelope = match self.model {
            Model::Juno106 => {
                self.stepped.gate(self.gate);
                self.stepped.tick()
            }
            Model::Juno60 => {
                self.analogue.gate(self.gate);
                self.analogue.tick()
            }
        };
        let lfo = controls.lfo;
        let pitch = controls.pitch.saturating_add(mul_high(settings.pitch_lfo, lfo) << 2);
        match self.model {
            Model::Juno106 => self.divide_106(pitch, settings.range),
            Model::Juno60 => self.divide_60(pitch),
        }

        let position = match settings.pwm {
            PwmSource::Manual => settings.pulse_width,
            // The oscillator from the square to the slider's width and
            // back: its swing made one-sided.
            PwmSource::Lfo => mul_high(settings.pulse_width, (ENVELOPE_ONE >> 1).saturating_add(lfo >> 1)) << 2,
            PwmSource::Envelope => mul_high(settings.pulse_width, envelope) << 2,
        };
        let wider = mul_high_unsigned(PULSE_WIDTH.1, position.max(0).cast_unsigned() << 2);
        self.oscillator.set_width(PULSE_WIDTH.0.saturating_add(wider));

        // The key follow turns about middle C: JUNO-60 service notes,
        // adjustments 8-3 to 8-5 (C4 is trimmed to the same 248 Hz with
        // KYBD at 10 as at 0, and C6 to 992 Hz); JUNO-106 service notes,
        // page 1: "KEY FOLLOW +3/-2 octaves" on a keyboard of C2 to C7.
        let keyboard = law::share(law::keys(MIDDLE_C, self.key), settings.keyboard);
        self.cutoff.target = settings
            .cutoff
            .saturating_add(self.trim)
            .saturating_add(keyboard)
            .saturating_add(mul_high(settings.cutoff_envelope, envelope) << 2)
            .saturating_add(mul_high(settings.cutoff_lfo, lfo) << 2);
        self.level.target = match (settings.vca_gate, self.gate) {
            (true, true) => ENVELOPE_ONE,
            (true, false) => 0,
            (false, _) => envelope,
        };
    }

    /// The Juno-106's oscillator: the clock over the nearest divisor to
    /// the pitch, which holds the key, the bender and the low-frequency
    /// oscillator alike (service notes, page 8: "divide data is the sum
    /// of a key number and the outputs from LFO, Bender, Portamento and
    /// Tune").
    fn divide_106(&mut self, pitch: i32, range: Range) {
        // At 8' the timer counts 2 MHz; the range moves the clock, not
        // the divisor.
        let clock = CLOCK_106 / CLOCK_106_DIVIDER_EIGHT;
        let divisor = divisor(DIVISOR_OF_A_106, law::keys(A4.1, self.key).saturating_add(pitch));
        let step = (clock << 32).checked_div(divisor.saturating_mul(u64::from(RATE))).unwrap_or(0);
        let step = match range {
            Range::Sixteen => step >> 1,
            Range::Eight => step,
            Range::Four => step << 1,
        };
        self.oscillator.set_step(u32::try_from(step).unwrap_or(STEP_LIMIT));
        // The converter's nearest number to the voltage that gives 12 V,
        // and the 12 V it then gives.
        let code = nearest(SAWTOOTH_CODE, divisor).min(CONVERTER_MAX);
        let mut gain = (code.saturating_mul(divisor) << 30) / SAWTOOTH_CODE;
        if range == Range::Sixteen {
            gain = gain.saturating_mul(SIXTEEN_FOOT_RATIO.0) / SIXTEEN_FOOT;
        }
        self.sawtooth_gain = i32::try_from(gain).unwrap_or(ENVELOPE_ONE);
    }

    /// The Juno-60's oscillator: the master oscillator, which the bender
    /// and the low-frequency oscillator move, over the key's divisor. Its
    /// sawtooth's charging current follows the pitch through an
    /// exponential amplifier, "maintaining the sawtooth amplitude
    /// constant over the frequency range" (service notes, page 14).
    fn divide_60(&mut self, pitch: i32) {
        let master = law::raise(MASTER_60, pitch).clamp(MASTER_60_BOUNDS.0, MASTER_60_BOUNDS.1);
        let step = (u64::from(master) << 32).checked_div(u64::from(self.divisor).saturating_mul(u64::from(RATE)));
        self.oscillator.set_step(u32::try_from(step.unwrap_or(0)).unwrap_or(STEP_LIMIT));
    }

    /// A tick: the cutoff and the amplifier follow their targets.
    pub fn tick(&mut self, settings: &Settings, law: &Law, smoothing: i32) {
        let (cutoff_smoothing, level_smoothing) = match self.model {
            Model::Juno106 => (smoothing, smoothing),
            // The Juno-60's controls are continuous; only the gate's
            // edge is rounded, by as much (own choice: a bare edge on
            // the amplifier is a click the instrument does not make).
            Model::Juno60 => (IMMEDIATE, if settings.vca_gate { smoothing } else { IMMEDIATE }),
        };
        let octaves = self.cutoff.tick(cutoff_smoothing);
        let hertz = law::raise(law.cutoff_base, octaves).clamp(law.cutoff_bounds.0, law.cutoff_bounds.1);
        self.filter.set(frequency(hertz, RATE), settings.resonance);
        let level = self.level.tick(level_smoothing);
        self.gain_step = level.saturating_sub(self.gain) >> TICK_BITS;
    }

    /// One sample. `noise` is the instrument's one noise generator.
    #[inline]
    pub fn sample(&mut self, settings: &Settings, noise: i32) -> i32 {
        let waves = self.oscillator.tick(None);
        let mixed = apply(waves.sawtooth, self.sawtooth_gain)
            .wrapping_add(waves.pulse)
            .wrapping_add(apply(waves.sub, settings.sub_level))
            .wrapping_add(apply(noise, settings.noise_level))
            .wrapping_add(noise >> HISS_SHIFT);
        let filtered = self.filter.tick(amplify(mixed, settings.compensation) >> DRIVE_SHIFT).four_pole;
        self.gain = self.gain.wrapping_add(self.gain_step);
        apply(filtered, self.gain)
    }
}

/// Fraction bits of a divisor before it is rounded.
const DIVISOR_BITS: u32 = 16;
/// The A's frequency in hertz.
const A4_HERTZ: u64 = A4.0 as u64 / MILLIHERTZ;
/// The divisor of the A above middle C on the Juno-60, with
/// [`DIVISOR_BITS`] of fraction: 4,305.
const DIVISOR_OF_A_60: u32 = ((MASTER_60 as u64) << DIVISOR_BITS).div_ceil(A4_HERTZ) as u32;
/// The same on the Juno-106 at 8': 2 MHz over 440 Hz, 4,545.45, to the
/// nearest unit.
const DIVISOR_OF_A_106: u32 =
    ((((CLOCK_106 / CLOCK_106_DIVIDER_EIGHT) << DIVISOR_BITS) + A4_HERTZ / 2) / A4_HERTZ) as u32;
const _: () = assert!(DIVISOR_OF_A_60 == 4_305 << DIVISOR_BITS);

/// The divisor of a pitch `octaves` from the A, whose own divisor is
/// `of_a`: the nearest whole number to `of_a / 2^octaves`, held to what a
/// timer divides by, 1 to [`DIVISOR_MAX`]. `of_a` has [`DIVISOR_BITS`] of
/// fraction.
fn divisor(of_a: u32, octaves: i32) -> u64 {
    let exact = u64::from(law::raise(of_a, octaves.saturating_neg()));
    (exact.saturating_add(1 << (DIVISOR_BITS - 1)) >> DIVISOR_BITS).clamp(1, DIVISOR_MAX)
}

/// The nearest whole number to a quotient, held to what a timer divides
/// by: 1 to [`DIVISOR_MAX`].
fn nearest(dividend: u64, divisor: u64) -> u64 {
    dividend.saturating_add(divisor / 2).checked_div(divisor).unwrap_or(DIVISOR_MAX).clamp(1, DIVISOR_MAX)
}

/// The key the cutoff's key follow turns about.
const MIDDLE_C: u8 = 60;

/// The analogue attack's length in time constants, in thousandths.
const ATTACK_TIME_CONSTANTS_THOUSANDTHS: u64 = RcEnvelope::ATTACK_TIME_CONSTANTS_THOUSANDTHS as u64;
const _: () = assert!(ATTACK_TIME_CONSTANTS_THOUSANDTHS > 0);

/// The interval of a range from 8'.
const fn range_octaves(range: Range) -> i32 {
    match range {
        Range::Sixteen => -OCTAVE,
        Range::Eight => 0,
        Range::Four => OCTAVE,
    }
}

/// The share of the way a sample-and-hold's capacitor moves in a tick.
pub fn hold_smoothing() -> i32 {
    time_constant(HOLD_SMOOTHING_MICROSECONDS, TICK_RATE)
}
