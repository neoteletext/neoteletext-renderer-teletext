//! The envelope generators: the analogue one, a capacitor charged and
//! discharged through resistors, and the one a processor computes at
//! intervals.
//!
//! An envelope's level runs from zero to [`ENVELOPE_ONE`]; [`apply`]
//! multiplies a signal by it.

use super::fixed::{accumulate, mul_high};

/// The envelope's peak: one in Q30.
pub const ENVELOPE_ONE: i32 = 1 << 30;

/// The level the attack of an analogue envelope charges towards, which it
/// never reaches: the attack ends when the level passes the peak, and so
/// in a finite time and still rising.
///
/// Source: Curtis Electromusic Specialties, CEM3310 Voltage Controlled
/// Envelope Generator data sheet (1979), Electrical Characteristics:
/// "Attack Asymptote Voltage 6.5 V", "Attack Peak Voltage 5.0 V" (typical).
pub const ATTACK_TARGET: i32 = (ENVELOPE_ONE as i64 * 65 / 50) as i32;

/// The lowest level [`RcEnvelope::set_attack_target`] takes: a sixty-fourth
/// above the peak, so that an attack always ends.
///
/// Source: own choice.
pub const ATTACK_TARGET_FLOOR: i32 = ENVELOPE_ONE + ENVELOPE_ONE / 64;

/// The level below which a released envelope is at rest: 90 dB below the
/// peak.
///
/// Source: own choice: below a 16-bit sample's last bit for a voice at
/// full level.
pub const REST_LEVEL: i32 = ENVELOPE_ONE >> 15;

/// One in Q32, the fixed point the coefficient is worked out in.
const ONE_Q32: u64 = 1 << 32;
/// Millionths of a second in a second.
const MICROSECONDS: u64 = 1_000_000;
/// Terms of the series of 1 - e⁻ˣ kept: for x up to one the next is below
/// 2⁻³².
const SERIES_TERMS: u64 = 13;
/// Halvings of x beyond which the coefficient is one: e⁻³² is below 2⁻³².
const HALVINGS_LIMIT: u32 = 5;

/// One as a coefficient: the whole way at once, in Q31 less a unit.
pub const IMMEDIATE: i32 = i32::MAX;
/// Bits of a coefficient's fraction.
const COEFFICIENT_BITS: u32 = 31;

/// The share of the way to its target that a capacitor moves in one
/// sample, in Q31: 1 - exp(-1 / (time constant * rate)), for a time
/// constant in millionths of a second and a rate in hertz, within 2⁻³⁰
/// of it. A time constant of zero, or of less than a thirty-second of a
/// sample, gives [`IMMEDIATE`]. The longest time constants have the
/// fewest bits: 20 s at 192 kHz is set to within a tenth of a per cent.
///
/// It divides, so it belongs where a control changes.
#[must_use]
pub const fn time_constant(microseconds: u32, rate: u32) -> i32 {
    let samples = (microseconds as u64).saturating_mul(rate as u64);
    // x = 1 / (time constant in samples), in Q32.
    let Some(mut x) = (MICROSECONDS << 32).checked_div(samples) else {
        return IMMEDIATE;
    };
    // The series needs x below one: halve it until it is, and square
    // e⁻ˣ back afterwards. At one exactly the first product below would
    // be 2⁶⁴ and wrap to nothing.
    let mut halvings = 0u32;
    while x >= ONE_Q32 {
        x >>= 1;
        halvings = halvings.saturating_add(1);
        if halvings > HALVINGS_LIMIT {
            return IMMEDIATE;
        }
    }
    // 1 - e⁻ˣ = x - x²/2 + x³/6 - ...: each term is the last times x / n,
    // and with x below one in Q32 no product leaves 64 bits.
    let mut term = x;
    let mut sum = x;
    let mut index = 2u64;
    while index <= SERIES_TERMS {
        term = match (term.wrapping_mul(x) >> 32).checked_div(index) {
            Some(term) => term,
            None => 0,
        };
        sum = if index.is_multiple_of(2) { sum.wrapping_sub(term) } else { sum.wrapping_add(term) };
        index = index.wrapping_add(1);
    }
    // e⁻ˣ, squared once for each halving.
    let mut decay = ONE_Q32.saturating_sub(sum);
    while halvings > 0 {
        decay = decay.wrapping_mul(decay) >> 32;
        halvings = halvings.wrapping_sub(1);
    }
    let coefficient = ONE_Q32.saturating_sub(decay) >> 1;
    if coefficient >= IMMEDIATE as u64 { IMMEDIATE } else { coefficient as i32 }
}

/// A signal times an envelope's level.
#[inline]
#[must_use]
pub const fn apply(signal: i32, level: i32) -> i32 {
    mul_high(signal, level) << 2
}

/// Where an envelope is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// At rest, at zero.
    Rest,
    /// The gate is on and the level rises to the peak.
    Attack,
    /// The gate is on and the level goes to the sustain level and stays.
    Decay,
    /// The gate is off and the level falls to zero.
    Release,
}

/// The analogue envelope: a capacitor that the attack charges towards
/// [`ATTACK_TARGET`] until it passes the peak, that the decay then
/// brings to the sustain level, and that the release discharges to zero,
/// each along an exponential with its own time constant.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RcEnvelope {
    level: i32,
    /// What the level holds below a unit.
    fraction: u32,
    stage: Stage,
    attack: i32,
    decay: i32,
    release: i32,
    sustain: i32,
    /// The level the attack charges towards.
    target: i32,
}

impl Default for RcEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl RcEnvelope {
    /// The attack's length in time constants: the natural logarithm of
    /// [`ATTACK_TARGET`] over what is left of it at the peak, 13 / 3, in
    /// thousandths. An attack that is to take a time T has a time
    /// constant of T * 1000 / this.
    pub const ATTACK_TIME_CONSTANTS_THOUSANDTHS: u32 = 1_466;

    /// An envelope at rest, whose every stage is immediate and whose
    /// sustain level is the peak.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            level: 0,
            fraction: 0,
            stage: Stage::Rest,
            attack: IMMEDIATE,
            decay: IMMEDIATE,
            release: IMMEDIATE,
            sustain: ENVELOPE_ONE,
            target: ATTACK_TARGET,
        }
    }

    /// Sets the level the attack charges towards, for an instrument whose
    /// circuit is not the CEM3310's: [`ATTACK_TARGET`] until it is set.
    /// It is held between [`ATTACK_TARGET_FLOOR`] and the largest level,
    /// one unit short of twice the peak. The attack then takes the
    /// natural logarithm of target / (target - peak) time constants.
    pub const fn set_attack_target(&mut self, target: i32) {
        self.target = if target < ATTACK_TARGET_FLOOR { ATTACK_TARGET_FLOOR } else { target };
    }

    /// Sets the attack, as [`time_constant`] gives it; a negative
    /// coefficient is taken as zero, which never moves.
    pub const fn set_attack(&mut self, coefficient: i32) {
        self.attack = positive(coefficient);
    }

    /// Sets the decay, as [`time_constant`] gives it.
    pub const fn set_decay(&mut self, coefficient: i32) {
        self.decay = positive(coefficient);
    }

    /// Sets the release, as [`time_constant`] gives it.
    pub const fn set_release(&mut self, coefficient: i32) {
        self.release = positive(coefficient);
    }

    /// Sets the sustain level, from zero to [`ENVELOPE_ONE`].
    pub const fn set_sustain(&mut self, level: i32) {
        self.sustain = clamp_level(level);
    }

    /// The gate: on starts the attack from wherever the level is, and off
    /// starts the release.
    pub const fn gate(&mut self, on: bool) {
        self.stage = match (on, self.stage) {
            (true, Stage::Attack | Stage::Decay) => self.stage,
            (true, Stage::Rest | Stage::Release) => Stage::Attack,
            (false, Stage::Rest) => Stage::Rest,
            (false, _) => Stage::Release,
        };
    }

    /// Starts the attack again, though the gate is already on: a new
    /// trigger.
    pub const fn retrigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Where the envelope is.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// The level, without moving on.
    #[must_use]
    pub const fn level(&self) -> i32 {
        self.level
    }

    /// One sample: moves on, and gives the level.
    #[inline]
    pub const fn tick(&mut self) -> i32 {
        let (target, coefficient) = match self.stage {
            Stage::Rest => return 0,
            Stage::Attack => (self.target, self.attack),
            Stage::Decay => (self.sustain, self.decay),
            Stage::Release => (0, self.release),
        };
        let moved = accumulate(target.wrapping_sub(self.level), coefficient, COEFFICIENT_BITS, &mut self.fraction);
        self.level = self.level.wrapping_add(moved);
        match self.stage {
            Stage::Attack if self.level >= ENVELOPE_ONE => {
                self.level = ENVELOPE_ONE;
                self.stage = Stage::Decay;
            }
            Stage::Release if self.level < REST_LEVEL => {
                self.level = 0;
                self.fraction = 0;
                self.stage = Stage::Rest;
            }
            _ => {}
        }
        self.level
    }
}

/// A coefficient that is not negative.
const fn positive(coefficient: i32) -> i32 {
    if coefficient < 0 { 0 } else { coefficient }
}

/// A level held between zero and one in Q30.
const fn clamp_level(value: i32) -> i32 {
    if value < 0 {
        0
    } else if value > ENVELOPE_ONE {
        ENVELOPE_ONE
    } else {
        value
    }
}

/// How a computed envelope's decay and release fall.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fall {
    /// By the same amount at each update, in Q30: a straight line.
    Linear(i32),
    /// By the same share of the distance left at each update, as
    /// [`time_constant`] gives it for the update rate: an exponential in
    /// steps.
    Proportional(i32),
}

/// The envelope a processor computes: it is worked out once every so many
/// samples and held between, so its output is a staircase. The attack is
/// a straight line to the peak; the decay and the release are each a
/// straight line or an exponential in steps ([`Fall`]).
///
/// Source: own design of the general case. The instruments whose
/// envelopes are made by a processor and a converter set the interval and
/// the shapes from their own documents.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SteppedEnvelope {
    level: i32,
    fraction: u32,
    stage: Stage,
    /// Samples between updates, and those left before the next.
    interval: u32,
    wait: u32,
    attack: i32,
    decay: Fall,
    release: Fall,
    sustain: i32,
}

impl Default for SteppedEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

impl SteppedEnvelope {
    /// An envelope at rest, updated at every sample, whose every stage is
    /// immediate and whose sustain level is the peak.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            level: 0,
            fraction: 0,
            stage: Stage::Rest,
            interval: 1,
            wait: 0,
            attack: ENVELOPE_ONE,
            decay: Fall::Linear(ENVELOPE_ONE),
            release: Fall::Linear(ENVELOPE_ONE),
            sustain: ENVELOPE_ONE,
        }
    }

    /// Sets the samples between updates; zero is taken as one. A wait
    /// that is running is held to the new interval.
    pub const fn set_interval(&mut self, samples: u32) {
        self.interval = if samples == 0 { 1 } else { samples };
        if self.wait > self.interval {
            self.wait = self.interval;
        }
    }

    /// Sets the attack's rise at each update, in Q30.
    pub const fn set_attack(&mut self, rise: i32) {
        self.attack = clamp_level(rise);
    }

    /// Sets the decay.
    pub const fn set_decay(&mut self, fall: Fall) {
        self.decay = clamp_fall(fall);
    }

    /// Sets the release.
    pub const fn set_release(&mut self, fall: Fall) {
        self.release = clamp_fall(fall);
    }

    /// Sets the sustain level, from zero to [`ENVELOPE_ONE`].
    pub const fn set_sustain(&mut self, level: i32) {
        self.sustain = clamp_level(level);
    }

    /// The gate: on starts the attack from wherever the level is, and off
    /// starts the release. Either takes effect at the next update.
    pub const fn gate(&mut self, on: bool) {
        self.stage = match (on, self.stage) {
            (true, Stage::Attack | Stage::Decay) => self.stage,
            (true, Stage::Rest | Stage::Release) => Stage::Attack,
            (false, Stage::Rest) => Stage::Rest,
            (false, _) => Stage::Release,
        };
    }

    /// Starts the attack again, though the gate is already on.
    pub const fn retrigger(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Where the envelope is.
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// One sample: updates the level if an interval has passed, and gives
    /// it.
    #[inline]
    pub const fn tick(&mut self) -> i32 {
        if self.wait == 0 {
            self.wait = self.interval;
            self.update();
        }
        self.wait = self.wait.wrapping_sub(1);
        self.level
    }

    const fn fall(&mut self, fall: Fall, target: i32) {
        // The distance and the step have one sign: towards the target
        // from either side, and never past it.
        let distance = self.level.wrapping_sub(target);
        let step = match fall {
            Fall::Linear(step) if distance >= 0 => step,
            Fall::Linear(step) => step.wrapping_neg(),
            Fall::Proportional(share) => accumulate(distance, share, COEFFICIENT_BITS, &mut self.fraction),
        };
        let arrives = if distance >= 0 { step >= distance } else { step <= distance };
        self.level = if arrives { target } else { self.level.wrapping_sub(step) };
    }

    const fn update(&mut self) {
        match self.stage {
            Stage::Rest => {}
            Stage::Attack => {
                self.level = self.level.saturating_add(self.attack);
                if self.level >= ENVELOPE_ONE {
                    self.level = ENVELOPE_ONE;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => self.fall(self.decay, self.sustain),
            Stage::Release => {
                self.fall(self.release, 0);
                if self.level < REST_LEVEL {
                    self.level = 0;
                    self.fraction = 0;
                    self.stage = Stage::Rest;
                }
            }
        }
    }
}

/// A fall whose amount or share is held between zero and one in Q30.
const fn clamp_fall(fall: Fall) -> Fall {
    match fall {
        Fall::Linear(step) => Fall::Linear(clamp_level(step)),
        Fall::Proportional(share) => Fall::Proportional(positive(share)),
    }
}

/// The rise or fall at each update that crosses the whole range in a
/// time: [`ENVELOPE_ONE`] over the updates in that time, for a time in
/// millionths of a second and updates a second, rounded up so that the
/// time is never exceeded. A time of zero gives the whole range.
///
/// It divides, so it belongs where a control changes.
#[must_use]
pub const fn linear_step(microseconds: u32, updates_a_second: u32) -> i32 {
    let updates = match (microseconds as u64).saturating_mul(updates_a_second as u64).checked_div(MICROSECONDS) {
        Some(updates) => updates,
        None => 0,
    };
    match (ENVELOPE_ONE as u64).checked_div(updates) {
        Some(step) if step < ENVELOPE_ONE as u64 => (step as i32).saturating_add(1),
        _ => ENVELOPE_ONE,
    }
}
