//! The bucket-brigade chorus: two delay lines of 256 stages whose clocks
//! one slow oscillator sweeps in opposite directions, as in Roland's
//! chorus ("two delay lines are modulated by the same LFO, but the
//! modulation is inverted for one of the delay lines": Florian Anwander,
//! "Roland choruses and ensemble effects").
//!
//! # The model
//!
//! It follows Raffel and Smith, "Practical modeling of bucket-brigade
//! device circuits" (DAFx-10, 2010), with the figures of the MN3009's
//! data sheet:
//!
//! - **The delay is the clock's.** A line of N stages clocked at f delays
//!   by N / (2 f) (Raffel and Smith, section 2.1): 256 stages and a clock
//!   of 10 kHz to 200 kHz give the data sheet's 12.8 ms to 0.64 ms, and a
//!   delay asked for outside that range is held to it. The line itself
//!   is "a digital delay line in place of the bucket-brigade chip", read
//!   between samples by "an interpolating delay line" (section 4.1): a
//!   cubic through four samples.
//! - **A low-pass before and one after each line**, the anti-aliasing
//!   and reconstruction filters, each of the third order (section 2.2).
//!   Their cutoff is set a third of the way to the clock at the longest
//!   delay in use, the bottom of the range the paper gives ("between 1/3
//!   and 1/2 of the sampling frequency"), unless the instrument sets it.
//! - **The chip's own loss with frequency follows the clock** (section
//!   4.2, "frequency-dependent insertion gain"): a one-pole low-pass at
//!   0.3 of the clock, from the data sheet's "3 dB down" at 12 kHz with a
//!   clock of 40 kHz. It is 5.8 dB down at half the clock, inside the 4
//!   to 6 dB the paper gives.
//!
//! # What is left out
//!
//! - The chip samples at its clock, and so aliases; with the filters in
//!   place the paper leaves that out, and so does this.
//! - The delay is taken from the clock as it stands, not from the clock
//!   integrated over a signal's passage: exact for a steady clock, and
//!   off by the change of the delay during one passage, a few millionths
//!   of a second at a chorus's rates.
//! - The chip's distortion and noise, which the paper finds "less
//!   important for choruses" of this length (section 4.5), and any
//!   compander, which these instruments' choruses do not have.
//! - Which of the clock's frequency and period the oscillator sweeps in
//!   a straight line: here the delay, the period, moves in a straight
//!   line with the oscillator.

use super::filter::{CUTOFF_LIMIT, ONE_Q28, OnePole, StateVariable};
use super::fixed::{mul_high, ratio, scale};
use super::lfo::{Lfo, Shape};
use super::{LEVEL, STEP_LIMIT};

/// Stages of a line.
///
/// Source: MN3009 data sheet: "256-stage low noise BBD".
pub const STAGES: u32 = 256;
/// The clock's range in hertz.
///
/// Source: MN3009 data sheet, Operating Conditions: "Clock Frequency fcp:
/// min. 10, max. 200 kHz".
pub const CLOCK_HZ: (u32, u32) = (10_000, 200_000);
/// Millionths of a second in a second.
const MICROSECONDS: u64 = 1_000_000;
/// The delay's range in millionths of a second: [`STAGES`] / (2 * clock).
///
/// Source: Raffel and Smith, section 2.1, "Delay Time (s) = N / (2
/// fcp)"; the data sheet gives the same figures, "Signal Delay Time tD:
/// 0.64 to 12.8 ms".
pub const DELAY_MICROSECONDS: (u32, u32) = (
    (STAGES as u64 * MICROSECONDS / (2 * CLOCK_HZ.1 as u64)) as u32,
    (STAGES as u64 * MICROSECONDS / (2 * CLOCK_HZ.0 as u64)) as u32,
);
const _: () = assert!(DELAY_MICROSECONDS.0 == 640 && DELAY_MICROSECONDS.1 == 12_800);

/// The clock in hertz that gives a delay in millionths of a second.
#[must_use]
pub const fn clock_hz(delay_microseconds: u32) -> u32 {
    match (STAGES as u64 * MICROSECONDS / 2).checked_div(delay_microseconds as u64) {
        Some(clock) if clock <= u32::MAX as u64 => clock as u32,
        _ => u32::MAX,
    }
}

/// The chip's own cutoff as a share of its clock, in thousandths.
///
/// Source: MN3009 data sheet, Electrical Characteristics: "Input Signal
/// Frequency fi: fcp = 40 kHz, 3 dB down: 12 kHz" (minimum).
const INSERTION_CUTOFF_THOUSANDTHS: u64 = 300;
/// The filters' cutoff as the reciprocal of its share of the clock at
/// the longest delay in use.
///
/// Source: Raffel and Smith, section 2.2: "typically chosen to be between
/// 1/3 and 1/2 of the sampling frequency".
const FILTER_CLOCK_SHARE: u64 = 3;

/// Samples a line remembers: at the output rate the longest delay is 615.
const RING: usize = 1024;
/// The mask that wraps a position in the ring.
const RING_MASK: usize = RING - 1;
/// Fraction bits of a delay in samples.
const DELAY_BITS: u32 = 16;
/// The shortest delay a line can read, in samples: the cubic needs a
/// sample after the instant it reads.
const DELAY_FLOOR: u32 = 1 << DELAY_BITS;
/// The longest, which leaves the cubic its two samples before.
const DELAY_CEILING: u32 = ((RING - 4) as u32) << DELAY_BITS;

/// The largest signal a line holds: 12 dB above an oscillator. Beyond it
/// the input is clipped, as the chip clips at its rails.
///
/// Source: own choice of the level; the cubic's sums of four such samples
/// stay inside 32 bits.
const LINE_LIMIT: i32 = 4 * LEVEL;

/// The chip's cutoff as a frequency step, times its delay in samples with
/// [`DELAY_BITS`] of fraction: 0.3 of the clock is 0.3 * 128 cycles in a
/// delay, whatever the delay and the rate.
const INSERTION_CYCLES: u32 = (((STAGES / 2) as u64 * INSERTION_CUTOFF_THOUSANDTHS) << DELAY_BITS) as u32 / 1000;

/// A delay of so many millionths of a second in samples of a rate, with
/// [`DELAY_BITS`] of fraction, held to a word.
const fn delay_samples(microseconds: u32, rate: u32) -> u32 {
    let delay = ((microseconds as u64).saturating_mul(rate as u64) << DELAY_BITS) / MICROSECONDS;
    if delay > u32::MAX as u64 { u32::MAX } else { delay as u32 }
}

/// Samples between two settings of the lines' clock-following filters:
/// the delay moves too slowly for more.
///
/// Source: own choice: a third of a millisecond at the output rate.
const CONTROL_INTERVAL: u32 = 16;

/// A third-order low-pass with no peak: a one-pole stage and a two-pole
/// stage whose damping is one (Butterworth's alignment).
///
/// Source: own choice of the alignment. Raffel and Smith's capacitor
/// values are those of an echo, and the instruments' own are not taken
/// here.
#[derive(Clone, PartialEq, Eq, Debug)]
struct ThirdOrder {
    first: OnePole,
    second: StateVariable,
}

impl ThirdOrder {
    const fn new() -> Self {
        Self { first: OnePole::new(), second: StateVariable::new() }
    }

    fn set_cutoff(&mut self, step: u32) {
        // Both stages at the two-pole stage's bound, a quarter of the
        // rate.
        let step = step.min(STEP_LIMIT);
        self.first.set_cutoff(step);
        self.second.set(step, ONE_Q28);
    }

    #[inline]
    fn tick(&mut self, input: i32) -> i32 {
        self.second.tick(self.first.low_pass(input)).low
    }
}

/// One delay line with what follows it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Line {
    ring: [i32; RING],
    next: usize,
    insertion: OnePole,
    reconstruction: ThirdOrder,
}

impl Line {
    const fn new() -> Self {
        Self { ring: [0; RING], next: 0, insertion: OnePole::new(), reconstruction: ThirdOrder::new() }
    }

    /// Takes a sample, and gives the line's output for a delay in samples
    /// with [`DELAY_BITS`] of fraction.
    #[inline]
    fn tick(&mut self, input: i32, delay: u32) -> i32 {
        if let Some(slot) = self.ring.get_mut(self.next) {
            *slot = input.clamp(-LINE_LIMIT, LINE_LIMIT);
        }
        let whole = (delay >> DELAY_BITS) as usize;
        // The fraction of the way from the sample `whole` back to the one
        // before it, in Q31.
        let fraction = ((delay & (DELAY_FLOOR - 1)) << (31 - DELAY_BITS)).cast_signed();
        let at = |back: usize| self.ring.get(self.next.wrapping_sub(back) & RING_MASK).copied().unwrap_or(0);
        let (after, newer, older, before) =
            (at(whole.wrapping_sub(1)), at(whole), at(whole.wrapping_add(1)), at(whole.wrapping_add(2)));
        self.next = self.next.wrapping_add(1) & RING_MASK;
        // The cubic through four samples that has, at each of the middle
        // two, the slope of the line between its neighbours (Catmull and
        // Rom, "A class of local interpolating splines", 1974).
        let third = newer.wrapping_sub(older).wrapping_mul(3).wrapping_add(before).wrapping_sub(after) >> 1;
        let second = after
            .wrapping_sub(newer.wrapping_mul(5) >> 1)
            .wrapping_add(older.wrapping_mul(2))
            .wrapping_sub(before >> 1);
        let first = older.wrapping_sub(after) >> 1;
        let curve = scale(scale(third, fraction).wrapping_add(second), fraction).wrapping_add(first);
        let read = newer.wrapping_add(scale(curve, fraction));
        self.reconstruction.tick(self.insertion.low_pass(read))
    }
}

/// The chorus: one input, two outputs. An instrument adds its dry signal
/// to each output, or to their sum for its mono output.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Chorus {
    rate: u32,
    /// The middle delay and the sweep either side of it, in samples with
    /// [`DELAY_BITS`] of fraction, and the bounds the data sheet's clock
    /// range puts on the delay at this rate.
    base: u32,
    depth: u32,
    bounds: (u32, u32),
    /// Whether the instrument has set the filters' cutoff itself.
    cutoff_set: bool,
    lfo: Lfo,
    anti_aliasing: ThirdOrder,
    lines: [Line; 2],
    /// Samples until the lines' clock-following filters are set again.
    wait: u32,
}

impl Chorus {
    /// A chorus for a rate in hertz, at rest: the shortest delay, no
    /// sweep, a triangle that does not move.
    #[must_use]
    pub fn new(rate: u32) -> Self {
        let bound = |microseconds: u32| delay_samples(microseconds, rate).clamp(DELAY_FLOOR, DELAY_CEILING);
        let mut chorus = Self {
            rate,
            base: 0,
            depth: 0,
            bounds: (bound(DELAY_MICROSECONDS.0), bound(DELAY_MICROSECONDS.1)),
            cutoff_set: false,
            lfo: Lfo::new(1),
            anti_aliasing: ThirdOrder::new(),
            lines: [Line::new(), Line::new()],
            wait: 0,
        };
        chorus.set_delay(DELAY_MICROSECONDS.0, 0);
        chorus
    }

    /// Sets the middle delay and how far the oscillator sweeps it either
    /// way, in millionths of a second. The delay is held within
    /// [`DELAY_MICROSECONDS`] at every instant of the sweep, and within
    /// what the line remembers, 1,020 samples.
    ///
    /// Unless [`Chorus::set_filter_cutoff`] has been called, the filters'
    /// cutoff follows: a third of the clock at the longest delay of the
    /// sweep. It divides, so it belongs where a control changes.
    pub fn set_delay(&mut self, base_microseconds: u32, depth_microseconds: u32) {
        self.base = delay_samples(base_microseconds, self.rate).clamp(self.bounds.0, self.bounds.1);
        self.depth = delay_samples(depth_microseconds, self.rate).min(self.bounds.1);
        if !self.cutoff_set {
            let longest =
                base_microseconds.saturating_add(depth_microseconds).clamp(DELAY_MICROSECONDS.0, DELAY_MICROSECONDS.1);
            let cutoff = u64::from(clock_hz(longest)) / FILTER_CLOCK_SHARE;
            let step = (cutoff << 32).checked_div(u64::from(self.rate)).unwrap_or(0);
            self.set_filters(u32::try_from(step).unwrap_or(u32::MAX));
        }
        self.wait = 0;
    }

    fn set_filters(&mut self, step: u32) {
        self.anti_aliasing.set_cutoff(step);
        for line in &mut self.lines {
            line.reconstruction.set_cutoff(step);
        }
    }

    /// Sets the cutoff of the anti-aliasing and reconstruction filters as
    /// a frequency step, held to a quarter of the rate, in place of the
    /// one [`Chorus::set_delay`] derives.
    pub fn set_filter_cutoff(&mut self, step: u32) {
        self.cutoff_set = true;
        self.set_filters(step);
    }

    /// Sets the sweeping oscillator's rate as a frequency step.
    pub const fn set_rate(&mut self, step: u32) {
        self.lfo.set_step(step);
    }

    /// Sets the sweeping oscillator's shape.
    pub const fn set_shape(&mut self, shape: Shape) {
        self.lfo.set_shape(shape);
    }

    /// One sample: the two lines' outputs, the second swept opposite to
    /// the first.
    #[inline]
    pub fn process(&mut self, input: i32) -> [i32; 2] {
        // The sweep in samples: the depth times the oscillator, in Q30.
        let sweep = mul_high(self.depth.cast_signed(), self.lfo.tick()) << 2;
        let (shortest, longest) = self.bounds;
        let delays = [
            self.base.saturating_add_signed(sweep).clamp(shortest, longest),
            self.base.saturating_add_signed(sweep.wrapping_neg()).clamp(shortest, longest),
        ];
        if self.wait == 0 {
            self.wait = CONTROL_INTERVAL;
            for (line, &delay) in self.lines.iter_mut().zip(&delays) {
                // The chip's cutoff is a fixed number of cycles in its
                // delay: over the delay, it is a frequency step.
                line.insertion.set_cutoff(ratio(INSERTION_CYCLES, delay).min(CUTOFF_LIMIT));
            }
        }
        self.wait = self.wait.wrapping_sub(1);
        let filtered = self.anti_aliasing.tick(input);
        let [first, second] = &mut self.lines;
        let [first_delay, second_delay] = delays;
        [first.tick(filtered, first_delay), second.tick(filtered, second_delay)]
    }
}
