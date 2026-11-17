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
//! # A frequency that changes within a sample
//!
//! [`Oscillator::tick`] makes a sample at one frequency. Where another
//! oscillator moves this one's frequency at audio rate, the frequency
//! slides through every sample and jumps within some, and a sample is
//! made in stretches instead: [`Oscillator::run`] for each stretch, with
//! the frequency at each of its ends, [`Oscillator::synchronise`] and
//! [`Oscillator::jump_width`] between stretches for a cycle started or a
//! width changed at that instant, and [`Oscillator::finish`] for the
//! sample. Each edge is then put where the moving frequency puts it, and
//! the corner that a jump of the frequency makes in the sawtooth and the
//! triangle is rounded. `tests/analog_oscillator.rs` measures a carrier
//! moved half an octave either way by a sine, a sawtooth and a pulse of
//! 880 Hz: 111 dB down at twice the output rate, where one frequency for
//! each sample leaves 80 dB under the sine, 38 dB under the sawtooth and
//! 29 dB under the pulse.
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
    /// For a sample made in stretches: whether one is being made, how
    /// long before it the cycle has been run to, and the step the last
    /// stretch ended at.
    midway: bool,
    now: u32,
    ran: u32,
    sawtooth: Ring,
    pulse: Ring,
    triangle: Ring,
    sub: Ring,
}

/// What the square of a stretch's bow is divided by to give the share by
/// which an exponential's mean is below the middle of its two ends: with
/// last / first = (1 + b) / (1 - b), ln(last / first) = 2 b (1 + b² / 3 +
/// ...), so the mean is the middle times 1 - b² / 3.
const EXPONENTIAL_MEAN: u32 = 3;

/// A stretch of a sample in which the cycle does not start again, the
/// frequency moving from one step to another through it.
#[derive(Clone, Copy)]
struct Stretch {
    /// When it ends, as how long before the sample, and how long it is,
    /// in Q32 of a sample.
    until: u32,
    span: u32,
    /// The step at each end, and what the phase runs between them.
    first: u32,
    last: u32,
    advance: u32,
    /// Half of what the step moves by over its mean, in Q32: how far the
    /// phase bows off a straight line, as a share of the advance and of
    /// s (1 - s) at a share s of the stretch.
    bow: u32,
}

impl Stretch {
    /// The stretch from `from` to `until` before the sample.
    #[inline]
    const fn new(from: u32, until: u32, first: u32, last: u32) -> Self {
        let span = from.saturating_sub(until);
        // Both are within the bound of a step, so the sum is in its word.
        let middle = (first >> 1).wrapping_add(last >> 1).wrapping_add(first & last & 1);
        let bow = if first == last { 0 } else { ratio(first.abs_diff(last) >> 1, middle) };
        // The frequency is an exponential converter's, under a control
        // that moves in a straight line: its mean over the stretch is
        // (last - first) / ln(last / first), which is the middle of the
        // two less a third of the bow's square of it, to the bow's
        // fourth power.
        let mean = middle.wrapping_sub(mul_high_unsigned(middle, mul_high_unsigned(bow, bow)) / EXPONENTIAL_MEAN);
        // Rounded to the nearest, so that a sample cut into stretches
        // runs what the whole sample would, give or take, and not less.
        let advance = if span == u32::MAX {
            mean
        } else {
            mul_high_unsigned(mean, span).wrapping_add(mean.wrapping_mul(span) >> (u32::BITS - 1))
        };
        Self { until, span, first, last, advance, bow }
    }

    /// `after / (before + after)` in Q32; the halves keep the sum in its
    /// word.
    #[inline]
    const fn share(before: u32, after: u32) -> u32 {
        ratio(after >> 1, (before >> 1).wrapping_add(after >> 1).wrapping_add(1))
    }

    /// Where in the stretch the phase met a mark, as the share of the
    /// stretch since: the two were `before` apart at its start and
    /// `after` at its end, and the phase passed the mark if it
    /// `overtakes`, the mark the phase if not.
    ///
    /// Were the frequency steady they met `after / (before + after)` of
    /// the stretch ago. A frequency that rises through the stretch
    /// leaves the phase behind that straight line, by the advance times
    /// the bow times s (1 - s) to the first order of the bow, and one
    /// that falls puts it ahead: the
    /// meeting is taken again on the straight line from where the phase
    /// then really was to the end it is nearer to.
    #[inline]
    const fn met(&self, before: u32, after: u32, overtakes: bool) -> u32 {
        let ago = Self::share(before, after);
        if self.bow == 0 {
            return ago;
        }
        let off = mul_high_unsigned(self.advance, mul_high_unsigned(mul_high_unsigned(ago, !ago), self.bow));
        if off == 0 {
            return ago;
        }
        if (self.last > self.first) == overtakes {
            // They had not yet met: later, between there and the end.
            ago.wrapping_sub(mul_high_unsigned(ago, Self::share(after, off)))
        } else {
            ago.wrapping_add(mul_high_unsigned(!ago, Self::share(before, off)))
        }
    }

    /// A share of the stretch ago as how long before the sample.
    #[inline]
    const fn ago(&self, share: u32) -> u32 {
        self.until.saturating_add(mul_high_unsigned(self.span, share))
    }

    /// The step at a share of the stretch ago.
    #[inline]
    const fn step_at(&self, share: u32) -> u32 {
        let moved = mul_high_unsigned(self.first.abs_diff(self.last), share);
        if self.last > self.first { self.last.wrapping_sub(moved) } else { self.last.wrapping_add(moved) }
    }
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
            midway: false,
            now: u32::MAX,
            ran: 0,
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
        self.ran = step;
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
            if since >= self.width {
                // A pulse narrower than what the cycle has run since has
                // fallen again already, where the phase passed its width.
                self.pulse_edge(self.fraction(since.wrapping_sub(self.width)), false);
            }
            self.start(fraction);
            self.started = Some(fraction);
            new = since;
        }
        // Whatever the above left out of step turns the pulse now.
        self.pulse_edge(0, new < self.width);
        self.width_before = self.width;
        self.phase = new;
        self.emit()
    }

    /// The sample of the phase as it stands: each waveform's value put
    /// in its place, and the sample that is complete given.
    #[inline]
    fn emit(&mut self) -> Waves {
        let phase = self.phase;
        let mut waves = Waves::default();
        if self.enabled & waveform::SAWTOOTH != 0 {
            waves.sawtooth = self.sawtooth.turn((phase >> SAWTOOTH_SHIFT).cast_signed().wrapping_sub(LEVEL));
        }
        if self.enabled & waveform::PULSE != 0 {
            waves.pulse = self.pulse.turn(if self.pulse_high { LEVEL } else { -LEVEL });
        }
        if self.enabled & waveform::TRIANGLE != 0 {
            waves.triangle = self.triangle.turn(triangle(phase));
        }
        if self.enabled & waveform::SUB != 0 {
            waves.sub = self.sub.turn(if self.sub_high { LEVEL } else { -LEVEL });
        }
        waves
    }

    /// Begins a sample made in stretches, if none is being made.
    #[inline]
    const fn enter(&mut self) {
        if !self.midway {
            self.midway = true;
            self.now = u32::MAX;
            self.started = None;
        }
    }

    /// How long before the sample being made in stretches the cycle has
    /// been run to, in units of 2⁻³² sample: a whole sample, less a
    /// unit, when none is being made.
    #[must_use]
    pub const fn now(&self) -> u32 {
        if self.midway { self.now } else { u32::MAX }
    }

    /// The frequency jumps to `step` at this instant: the sawtooth and
    /// the triangle turn a corner, which is rounded as the triangle's
    /// own are.
    #[inline]
    fn turn_to(&mut self, step: u32) {
        let change = step.cast_signed().wrapping_sub(self.ran.cast_signed());
        self.ran = step;
        if self.enabled & waveform::SAWTOOTH != 0 {
            let change = change >> SAWTOOTH_SHIFT;
            if change >> CORNER_SHIFTS.0 != 0 {
                self.sawtooth.corner(self.now, change);
            }
        }
        if self.enabled & waveform::TRIANGLE != 0 {
            let change = change >> TRIANGLE_SHIFT;
            if change >> CORNER_SHIFTS.0 != 0 {
                self.triangle.corner(self.now, if self.phase < HALF_CYCLE { change } else { change.wrapping_neg() });
            }
        }
    }

    /// What happens within a stretch in which the cycle does not start:
    /// the triangle's turn and the pulse's edge, the phase running from
    /// `old` to `new` and the width in a straight line from where it was
    /// to `width`.
    #[inline]
    fn cross(&mut self, stretch: &Stretch, old: u32, new: u32, width: u32) {
        if self.enabled & waveform::TRIANGLE != 0 && old < HALF_CYCLE && new >= HALF_CYCLE {
            let share = stretch.met(HALF_CYCLE.wrapping_sub(old), new.wrapping_sub(HALF_CYCLE), true);
            let change = (stretch.step_at(share) >> CORNER_SHIFT).cast_signed().wrapping_neg();
            self.triangle.corner(stretch.ago(share), change);
        }
        let high = new < width;
        if high != self.pulse_high {
            let share = stretch.met(old.abs_diff(self.width_before), new.abs_diff(width), !high);
            self.pulse_edge(stretch.ago(share), high);
        }
    }

    /// Runs the cycle on to `until` before the sample; with `stop`, no
    /// further than its next start. Gives when the cycle started, if it
    /// did.
    #[inline]
    fn stretch(&mut self, until: u32, first: u32, last: u32, stop: bool) -> Option<u32> {
        self.enter();
        let (first, last) = (first.min(STEP_LIMIT), last.min(STEP_LIMIT));
        let until = until.min(self.now);
        if first != self.ran {
            self.turn_to(first);
        }
        let whole = Stretch::new(self.now, until, first, last);
        let old = self.phase;
        let new = old.wrapping_add(whole.advance);
        if new >= old {
            self.cross(&whole, old, new, self.width);
            (self.phase, self.now, self.ran, self.width_before) = (new, until, last, self.width);
            return None;
        }
        // The cycle starts in the stretch: what comes before the start,
        // the start, and what comes after.
        let share = whole.met(old.wrapping_neg(), new, true);
        let (at, reached) = (whole.ago(share), whole.step_at(share));
        // The width when the cycle started, on its line.
        let moved = self.width.wrapping_sub(self.width_before).cast_signed();
        let width = self.width.wrapping_sub(scale(moved, (share >> 1).cast_signed()).cast_unsigned());
        let before = Stretch { advance: old.wrapping_neg(), ..Stretch::new(self.now, at, first, reached) };
        // No width is above the top of the cycle, which the phase reaches.
        self.cross(&before, old, u32::MAX, width);
        if width > 0 {
            self.pulse_edge(at, true);
        }
        if self.enabled & waveform::SAWTOOTH != 0 {
            self.sawtooth.step(at, -2 * LEVEL);
        }
        if self.enabled & waveform::TRIANGLE != 0 {
            self.triangle.corner(at, (reached >> CORNER_SHIFT).cast_signed());
        }
        self.start(at);
        self.started = Some(at);
        (self.phase, self.now, self.ran, self.width_before) = (0, at, reached, width);
        if !stop {
            let after = Stretch::new(at, until, reached, last);
            self.cross(&after, 0, after.advance, self.width);
            (self.phase, self.now, self.ran, self.width_before) = (after.advance, until, last, self.width);
        }
        Some(at)
    }

    /// Runs the cycle through a stretch of the sample being made, for a
    /// frequency that changes within a sample: from where the cycle has
    /// been run to ([`Oscillator::now`], a whole sample before it when
    /// the sample is begun) to `until` before the sample, in units of
    /// 2⁻³² sample, the frequency moving from the step `first` to the
    /// step `last` over the stretch as an exponential converter moves it
    /// under a control that moves in a straight line, each held to
    /// [`STEP_LIMIT`]. The stretches of a sample end at zero, and
    /// [`Oscillator::finish`] then gives the sample; [`Oscillator::tick`]
    /// is one stretch of one frequency and that.
    ///
    /// Every edge is put where it falls under the moving frequency. Where
    /// `first` is not the step the stretch before ended at, or the step
    /// last set, the frequency jumps, and the corner that makes in the
    /// sawtooth and the triangle is rounded. The width set by
    /// [`Oscillator::set_width`] is reached at the stretch's end, in a
    /// straight line from what it was.
    ///
    /// A stretch whose frequency moves, and each edge in any stretch,
    /// takes a reciprocal, made without a division.
    #[inline]
    pub fn run(&mut self, until: u32, first: u32, last: u32) {
        let _ = self.stretch(until, first, last, false);
    }

    /// As [`Oscillator::run`], but no further than the next start of the
    /// cycle: if the cycle starts in the stretch it is run to that
    /// instant alone, which is given as how long before the sample, and
    /// is then [`Oscillator::now`]. It is for a caller whose frequency
    /// from there on depends on the start, as when this oscillator
    /// starts the cycle of the one that modulates it; the caller runs
    /// the rest of the stretch from there.
    #[inline]
    pub fn run_to_start(&mut self, until: u32, first: u32, last: u32) -> Option<u32> {
        self.stretch(until, first, last, true)
    }

    /// Starts the cycle again at the instant the sample being made in
    /// stretches has been run to, the jump that makes in each waveform
    /// band-limited: the synchronisation of [`Oscillator::tick`], for a
    /// sample made in stretches.
    pub fn synchronise(&mut self) {
        self.enter();
        let (at, reached) = (self.now, self.phase);
        if self.enabled & waveform::SAWTOOTH != 0 {
            self.sawtooth.step(at, (reached >> SAWTOOTH_SHIFT).cast_signed().wrapping_neg());
        }
        if self.enabled & waveform::TRIANGLE != 0 {
            self.triangle.step(at, (-LEVEL).wrapping_sub(triangle(reached)));
            if reached >= HALF_CYCLE {
                // From falling to rising at the step it had.
                self.triangle.corner(at, (self.ran >> CORNER_SHIFT).cast_signed());
            }
        }
        self.pulse_edge(at, self.width > 0);
        self.start(at);
        self.started = Some(at);
        self.phase = 0;
    }

    /// Sets the pulse's width at the instant the sample being made in
    /// stretches has been run to: a width that jumps, where
    /// [`Oscillator::set_width`] gives one that the next stretch moves
    /// to. If the jump turns the pulse, the edge is put at that instant.
    pub fn jump_width(&mut self, width: u32) {
        self.enter();
        self.width = width;
        self.width_before = width;
        self.pulse_edge(self.now, self.phase < width);
    }

    /// Ends a sample made in stretches and gives it.
    #[inline]
    pub fn finish(&mut self) -> Waves {
        self.enter();
        // Whatever the stretches left out of step turns the pulse now.
        self.pulse_edge(0, self.phase < self.width);
        self.width_before = self.width;
        self.midway = false;
        self.emit()
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
