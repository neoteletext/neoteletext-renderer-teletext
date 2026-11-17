//! The oscillators: sawtooth, pulse of any width, triangle and a
//! sub-oscillator from one phase, hard sync between two, and white noise.
//!
//! # How the edges are made
//!
//! A sawtooth's fall and a pulse's edges have harmonics without end, and
//! sampled as they stand everything above half the sampling rate folds
//! back as tones that belong to no note. Here each edge is put where it
//! falls between two samples, and what a low-pass filter would have made
//! of it is added around it: the difference between a step seen through
//! the filter and the step itself, read from a table
//! ([`BLEP`]) at the edge's position. A triangle's
//! corners get the same from the table's integral
//! ([`BLAMP`]).
//!
//! Source: the method is the band-limited step of Stilson and Smith,
//! "Alias-free digital synthesis of classic analog waveforms" (ICMC 1996),
//! and Brandt, "Hard sync without aliasing" (ICMC 2001), reviewed in
//! Välimäki and Huovilainen, "Antialiasing oscillators in subtractive
//! synthesis" (IEEE Signal Processing Magazine, March 2007); the corner's
//! residual is the integrated step of that family. The table and the code
//! are this crate's own.
//!
//! The residual reaches [`Oscillator::DELAY`] samples either side of an
//! edge, so every waveform leaves that many samples late.
//!
//! # What is left of the aliasing
//!
//! `tests/analog_oscillator.rs` measures, for fundamentals from 80 Hz to
//! 4 kHz, the strongest tone below 20 kHz that is no harmonic of the note
//! against the strongest harmonic. At twice and at four times the output
//! rate, through the decimators, it is 91 dB down at worst for the
//! sawtooth, the pulse, the triangle and the sub-oscillator; 106 dB down
//! for a pulse whose width swings at audio rate; and 111 dB down for an
//! oscillator synchronised to another.
//!
//! At the output rate itself it is 48 dB down for fundamentals to 1 kHz
//! and 34 dB down at 4 kHz, the narrow pulse being the worst: the table's
//! low-pass falls between 0.3 and 0.7 of the sampling rate, so harmonics
//! between 28 kHz and 33 kHz come back below 20 kHz only partly reduced.
//! An instrument that wants the 80 dB runs its voices at twice the output
//! rate or more.

use super::fixed::{mul_high_unsigned, ratio, reciprocal, scale};
use super::tables::{BLAMP, BLEP, BLEP_PHASES_SHIFT, BLEP_TAPS};
use super::{LEVEL, STEP_LIMIT};

/// The mask that wraps a position in a waveform's ring.
const RING_MASK: usize = BLEP_TAPS - 1;
const _: () = assert!(BLEP_TAPS.is_power_of_two());

/// Bits of the weight between two rows of a residual table.
const BLEND_BITS: u32 = 8;
/// The mask of that weight.
const BLEND_MASK: u32 = (1 << BLEND_BITS) - 1;
/// The shift from a fraction of a sample in Q32 to a row of the table.
const ROW_SHIFT: u32 = 32 - BLEP_PHASES_SHIFT;
/// The shift from that fraction to the row with the weight below it.
const BLEND_SHIFT: u32 = ROW_SHIFT - BLEND_BITS;

/// A step's height gives up these bits before it meets a table entry, and
/// the product the rest of the table's Q15: a height of 2²⁶ at most
/// times an entry below 2¹⁵ stays inside 32 bits.
const STEP_SHIFTS: (u32, u32) = (11, 4);
/// The same for a corner's change of slope, against the corner table's
/// Q17.
const CORNER_SHIFTS: (u32, u32) = (10, 7);

/// Half a cycle of phase, where the triangle turns down.
const HALF_CYCLE: u32 = 1 << 31;
/// The shift from a phase to the sawtooth's rise across [`LEVEL`] either
/// way: 2³² of phase are 2²⁵ of signal.
const SAWTOOTH_SHIFT: u32 = 7;
/// The shift from a phase to the triangle's rise, twice as steep.
const TRIANGLE_SHIFT: u32 = SAWTOOTH_SHIFT - 1;
/// The shift from a frequency step to the change of slope at a
/// triangle's corner, twice its slope.
const CORNER_SHIFT: u32 = TRIANGLE_SHIFT - 1;
const _: () = assert!(LEVEL == 1 << (32 - SAWTOOTH_SHIFT - 1));

/// The waveforms an oscillator makes, as bits of [`Oscillator::enable`].
pub mod waveform {
    /// The sawtooth, rising.
    pub const SAWTOOTH: u8 = 1;
    /// The pulse, high for the width's share of each cycle.
    pub const PULSE: u8 = 2;
    /// The triangle.
    pub const TRIANGLE: u8 = 4;
    /// The sub-oscillator: a square an octave below.
    pub const SUB: u8 = 8;
    /// All four.
    pub const ALL: u8 = 15;
}

/// One waveform's next samples: the naive wave, late by half the
/// residual's length, with the residuals of its edges added.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Ring {
    samples: [i32; BLEP_TAPS],
    position: usize,
}

impl Ring {
    const fn new() -> Self {
        Self { samples: [0; BLEP_TAPS], position: 0 }
    }

    /// Adds a residual: `table` at `fraction` of a sample since the edge,
    /// times `amount` already reduced by the first of `shifts`.
    #[inline]
    fn add(&mut self, table: &[[i16; BLEP_TAPS]], fraction: u32, amount: i32, shift: u32) {
        let row = (fraction >> ROW_SHIFT) as usize;
        let weight = ((fraction >> BLEND_SHIFT) & BLEND_MASK).cast_signed();
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

    /// A step of `height` that happened `fraction` of a sample ago.
    #[inline]
    fn step(&mut self, fraction: u32, height: i32) {
        self.add(&BLEP, fraction, height >> STEP_SHIFTS.0, STEP_SHIFTS.1);
    }

    /// A corner: the slope changed by `change` a sample, `fraction` of a
    /// sample ago.
    #[inline]
    fn corner(&mut self, fraction: u32, change: i32) {
        self.add(&BLAMP, fraction, change >> CORNER_SHIFTS.0, CORNER_SHIFTS.1);
    }

    /// Puts the naive wave's value now in its place and gives the sample
    /// that is complete.
    #[inline]
    fn turn(&mut self, naive: i32) -> i32 {
        if let Some(slot) = self.samples.get_mut(self.position.wrapping_add(BLEP_TAPS / 2) & RING_MASK) {
            *slot = slot.wrapping_add(naive);
        }
        let complete = self.samples.get_mut(self.position).map_or(0, core::mem::take);
        self.position = self.position.wrapping_add(1) & RING_MASK;
        complete
    }
}

/// One sample of each waveform; a waveform that is not enabled is zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Waves {
    /// The sawtooth: it rises from -[`LEVEL`] to [`LEVEL`] over a cycle.
    pub sawtooth: i32,
    /// The pulse: [`LEVEL`] for the width's share of a cycle from its
    /// start, -[`LEVEL`] for the rest.
    pub pulse: i32,
    /// The triangle: from -[`LEVEL`] at the cycle's start to [`LEVEL`] at
    /// its middle.
    pub triangle: i32,
    /// The sub-oscillator: a square between -[`LEVEL`] and [`LEVEL`] that
    /// turns at each start of a cycle.
    pub sub: i32,
}

/// An oscillator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Oscillator {
    phase: u32,
    step: u32,
    /// The left shift that brings the step's top bit to bit 31, and 2⁶³
    /// over the step so shifted: what divides by the step with a product.
    normalise: u32,
    reciprocal: u32,
    /// The pulse is high while the phase is below this.
    width: u32,
    /// The width at the last sample: between two samples the width is
    /// taken to move in a straight line.
    width_before: u32,
    pulse_high: bool,
    sub_high: bool,
    enabled: u8,
    /// How long before the last sample the cycle last started, if it did
    /// within that sample: what a synchronised oscillator is given.
    started: Option<u32>,
    sawtooth: Ring,
    pulse: Ring,
    triangle: Ring,
    sub: Ring,
}

impl Default for Oscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl Oscillator {
    /// Samples by which every waveform is late: half the length of an
    /// edge's residual.
    pub const DELAY: usize = BLEP_TAPS / 2;

    /// An oscillator at rest: no frequency, a square pulse, every
    /// waveform enabled.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: 0,
            step: 0,
            normalise: 0,
            reciprocal: 0,
            width: HALF_CYCLE,
            width_before: HALF_CYCLE,
            pulse_high: true,
            sub_high: true,
            enabled: waveform::ALL,
            started: None,
            sawtooth: Ring::new(),
            pulse: Ring::new(),
            triangle: Ring::new(),
            sub: Ring::new(),
        }
    }

    /// Sets the frequency as a [`frequency`](super::frequency) step, held
    /// to [`STEP_LIMIT`]. It does not divide, and may be called for every
    /// sample.
    pub const fn set_step(&mut self, step: u32) {
        let step = if step > STEP_LIMIT { STEP_LIMIT } else { step };
        self.step = step;
        if step == 0 {
            self.normalise = 0;
            self.reciprocal = 0;
            return;
        }
        self.normalise = step.leading_zeros();
        self.reciprocal = reciprocal(step << self.normalise);
    }

    /// The frequency step.
    #[must_use]
    pub const fn step(&self) -> u32 {
        self.step
    }

    /// Sets the pulse's width as its high share of a cycle in units of
    /// 2⁻³² cycle: 2³¹ is a square, and 0 is low throughout. It may change
    /// at every sample: the pulse turns when the phase and the width
    /// cross, however they come to, and each turn is a band-limited edge
    /// placed where the two met, the width taken to move in a straight
    /// line between samples. A width that moves by half a cycle or more in
    /// one sample is beyond that.
    pub const fn set_width(&mut self, width: u32) {
        self.width = width;
    }

    /// Chooses the waveforms to make, as [`waveform`] bits; the others
    /// cost nothing and read zero.
    pub const fn enable(&mut self, waveforms: u8) {
        self.enabled = waveforms;
    }

    /// Starts the cycle again at once, with no band-limiting: for a note's
    /// start, when the oscillator is silent.
    pub const fn restart(&mut self) {
        self.phase = 0;
        self.width_before = self.width;
        self.pulse_high = self.width > 0;
    }

    /// Moves the phase back by `amount`, in units of 2⁻³² cycle and held
    /// to the phase itself, with no band-limiting: for a caller that
    /// changed the frequency part of the way through the last sample of
    /// [`Oscillator::tick`], and takes back what the cycle has run too
    /// far since. Every waveform steps by what the amount is of it, so
    /// it is for small amounts.
    pub const fn set_back(&mut self, amount: u32) {
        self.phase = self.phase.saturating_sub(amount);
    }

    /// The phase, in units of 2⁻³² cycle.
    #[must_use]
    pub const fn phase(&self) -> u32 {
        self.phase
    }

    /// How long before the last sample of [`Oscillator::tick`] the cycle
    /// started, in units of 2⁻³² sample, if it started within that sample:
    /// what [`Oscillator::tick`] of an oscillator synchronised to this one
    /// takes.
    #[must_use]
    pub const fn started(&self) -> Option<u32> {
        self.started
    }

    /// `phase / step` in Q32, for a phase below the step; one, less a
    /// unit, for anything larger.
    #[inline]
    const fn fraction(&self, phase: u32) -> u32 {
        if phase >= self.step {
            return u32::MAX;
        }
        let half = mul_high_unsigned(phase << self.normalise, self.reciprocal);
        if half >= HALF_CYCLE { u32::MAX } else { half << 1 }
    }

    /// If the phase passed `threshold` on its way from `old` to `new`, how
    /// long ago in Q32 of a sample.
    #[inline]
    const fn crossing(&self, old: u32, new: u32, threshold: u32) -> Option<u32> {
        if threshold.wrapping_sub(old).wrapping_sub(1) < self.step {
            Some(self.fraction(new.wrapping_sub(threshold)))
        } else {
            None
        }
    }

    fn pulse_edge(&mut self, fraction: u32, high: bool) {
        if self.pulse_high != high {
            self.pulse_high = high;
            if self.enabled & waveform::PULSE != 0 {
                self.pulse.step(fraction, if high { 2 * LEVEL } else { -2 * LEVEL });
            }
        }
    }

    /// The pulse's edges within one sample while its width moves: the
    /// phase runs from `old` to `new`, starting again `wrap` of a sample
    /// ago if it did, and the width in a straight line from its last
    /// value to this one. The pulse turns where the two lines cross:
    /// with distances `a` before and `b` after between phase and width, a
    /// share b / (a + b) of the stretch ago. Edges later than `floor`
    /// ago are left to the caller.
    fn moving_pulse(&mut self, old: u32, new: u32, wrap: Option<u32>, floor: u32) {
        /// `b / (a + b)` in Q32; the halves keep the sum in its word.
        const fn share(a: u32, b: u32) -> u32 {
            ratio(b >> 1, (a >> 1).wrapping_add(b >> 1).wrapping_add(1))
        }
        let (before, after) = (self.width_before, self.width);
        let Some(wrap) = wrap else {
            let high = new < after;
            if high != self.pulse_high {
                let fraction = share(old.abs_diff(before), new.abs_diff(after));
                if fraction >= floor {
                    self.pulse_edge(fraction, high);
                }
            }
            return;
        };
        // The width when the cycle started, on the line between the two.
        let moved = after.wrapping_sub(before).cast_signed();
        let at_wrap = after.wrapping_sub(scale(moved, (wrap >> 1).cast_signed()).cast_unsigned());
        if self.pulse_high {
            // It fell before the cycle ended: the phase reached the top,
            // which no width is above.
            let stretch = share(before.wrapping_sub(old), at_wrap.wrapping_neg().wrapping_sub(1));
            let fraction = wrap.saturating_add(mul_high_unsigned(u32::MAX.wrapping_sub(wrap), stretch));
            if fraction >= floor {
                self.pulse_edge(fraction, false);
            }
        }
        if wrap < floor {
            return;
        }
        if at_wrap > 0 {
            self.pulse_edge(wrap, true);
        }
        let high = new < after;
        if high != self.pulse_high {
            let fraction = mul_high_unsigned(wrap, share(at_wrap, new.abs_diff(after)));
            if fraction >= floor {
                self.pulse_edge(fraction, high);
            }
        }
    }

    /// What a start of the cycle does to every waveform but the pulse.
    fn start(&mut self, fraction: u32) {
        if self.enabled & waveform::SUB != 0 {
            self.sub_high = !self.sub_high;
            self.sub.step(fraction, if self.sub_high { 2 * LEVEL } else { -2 * LEVEL });
        }
    }

    /// One sample. `sync` is [`Oscillator::started`] of the oscillator
    /// this one is synchronised to, or `None` for an oscillator that runs
    /// free: when it is `Some`, this oscillator's cycle starts again at
    /// that instant, and the jump that makes in each waveform is
    /// band-limited as any other edge.
    #[inline]
    pub fn tick(&mut self, sync: Option<u32>) -> Waves {
        let old = self.phase;
        let mut new = old.wrapping_add(self.step);
        let wrapped = new < old;
        let wrap_fraction = self.fraction(new);
        // With a sync, only what happened before it happened at all.
        let floor = sync.unwrap_or(0);
        let wrap = wrapped && wrap_fraction >= floor;
        let moving = self.width != self.width_before;
        let fall = if moving { None } else { self.crossing(old, new, self.width) };
        let fall = fall.filter(|&fraction| fraction >= floor);
        let before_wrap = |fraction: u32| !wrapped || fraction > wrap_fraction;

        if moving {
            self.moving_pulse(old, new, wrapped.then_some(wrap_fraction), floor);
        } else if let Some(fraction) = fall.filter(|&fraction| before_wrap(fraction)) {
            self.pulse_edge(fraction, false);
        }
        if wrap {
            if self.width > 0 && !moving {
                self.pulse_edge(wrap_fraction, true);
            }
            if self.enabled & waveform::SAWTOOTH != 0 {
                self.sawtooth.step(wrap_fraction, -2 * LEVEL);
            }
            if self.enabled & waveform::TRIANGLE != 0 {
                self.triangle.corner(wrap_fraction, (self.step >> CORNER_SHIFT).cast_signed());
            }
            self.start(wrap_fraction);
            if let Some(fraction) = fall.filter(|&fraction| !before_wrap(fraction)) {
                self.pulse_edge(fraction, false);
            }
        } else if self.enabled & waveform::TRIANGLE != 0
            && let Some(fraction) = self.crossing(old, new, HALF_CYCLE).filter(|&fraction| fraction >= floor)
        {
            self.triangle.corner(fraction, (self.step >> CORNER_SHIFT).cast_signed().wrapping_neg());
        }

        self.started = if wrap { Some(wrap_fraction) } else { None };
        if let Some(fraction) = sync {
            // The phase the cycle had reached when it was cut short, and
            // what it has run since.
            let since = mul_high_unsigned(self.step, fraction);
            let reached = new.wrapping_sub(since);
            if self.enabled & waveform::SAWTOOTH != 0 {
                self.sawtooth.step(fraction, (reached >> SAWTOOTH_SHIFT).cast_signed().wrapping_neg());
            }
            if self.enabled & waveform::TRIANGLE != 0 {
                self.triangle.step(fraction, (-LEVEL).wrapping_sub(triangle(reached)));
                if reached >= HALF_CYCLE {
                    self.triangle.corner(fraction, (self.step >> CORNER_SHIFT).cast_signed());
                }
            }
            self.pulse_edge(fraction, self.width > 0);
            self.start(fraction);
            self.started = Some(fraction);
            new = since;
        }
        // Whatever the above left out of step turns the pulse now.
        self.pulse_edge(0, new < self.width);
        self.width_before = self.width;
        self.phase = new;

        let mut waves = Waves::default();
        if self.enabled & waveform::SAWTOOTH != 0 {
            waves.sawtooth = self.sawtooth.turn((new >> SAWTOOTH_SHIFT).cast_signed().wrapping_sub(LEVEL));
        }
        if self.enabled & waveform::PULSE != 0 {
            waves.pulse = self.pulse.turn(if self.pulse_high { LEVEL } else { -LEVEL });
        }
        if self.enabled & waveform::TRIANGLE != 0 {
            waves.triangle = self.triangle.turn(triangle(new));
        }
        if self.enabled & waveform::SUB != 0 {
            waves.sub = self.sub.turn(if self.sub_high { LEVEL } else { -LEVEL });
        }
        waves
    }
}

/// The naive triangle at a phase.
#[inline]
const fn triangle(phase: u32) -> i32 {
    if phase < HALF_CYCLE {
        (phase >> TRIANGLE_SHIFT).cast_signed().wrapping_sub(LEVEL)
    } else {
        LEVEL.wrapping_sub((phase.wrapping_sub(HALF_CYCLE) >> TRIANGLE_SHIFT).cast_signed())
    }
}

/// The seed a noise generator takes in place of zero, which its sequence
/// never leaves.
///
/// Source: own choice: the golden ratio's fraction in 32 bits, a word
/// with no pattern.
const NOISE_SEED: u32 = 0x9E37_79B9;
/// The shifts of the generator.
///
/// Source: Marsaglia, "Xorshift RNGs" (Journal of Statistical Software 8,
/// 2003), the triple (13, 17, 5) of full period 2³² - 1.
const NOISE_SHIFTS: (u32, u32, u32) = (13, 17, 5);
/// The shift from the generator's word to a signal within [`LEVEL`].
const NOISE_SHIFT: u32 = 7;

/// White noise: a generator of its own for each instance, the same
/// sequence from the same seed every time.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Noise {
    state: u32,
}

impl Noise {
    /// A generator from a seed; every seed is valid.
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { state: if seed == 0 { NOISE_SEED } else { seed } }
    }

    /// The next word of the sequence, all 32 bits of it.
    #[inline]
    pub const fn word(&mut self) -> u32 {
        self.state ^= self.state << NOISE_SHIFTS.0;
        self.state ^= self.state >> NOISE_SHIFTS.1;
        self.state ^= self.state << NOISE_SHIFTS.2;
        self.state
    }

    /// The next sample: uniform between -[`LEVEL`] and [`LEVEL`].
    #[inline]
    pub const fn tick(&mut self) -> i32 {
        self.word().cast_signed() >> NOISE_SHIFT
    }
    /// The next word as a control level: uniform between -2³⁰ and 2³⁰.
    #[inline]
    pub const fn tick_level(&mut self) -> i32 {
        self.word().cast_signed() >> 1
    }
}
