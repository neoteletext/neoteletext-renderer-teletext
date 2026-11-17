//! The low-frequency oscillator: triangle, sine, square and
//! sample-and-hold, with a delay and a fade-in after a key.
//!
//! Its output runs between -[`LFO_ONE`] and [`LFO_ONE`]. It is ticked at
//! whatever rate the instrument modulates at, and its frequency is a
//! [`frequency`](super::frequency) step at that rate.

use super::fixed::{mul_high, scale};
use super::oscillator::Noise;

/// The oscillator's peak: one in Q30.
pub const LFO_ONE: i32 = 1 << 30;

/// The weight of the parabola's own square in the sine: 0.225 in Q30.
///
/// Source: general knowledge: the parabola 4x(1 - |x|) through the sine's
/// zeros and peaks, corrected by a share of its own square, is a common
/// approximation; this weight is near the one that makes the largest
/// error least. `tests/analog_lfo.rs` measures the error, 0.1% of the
/// peak, and the harmonics, 60 dB down.
const SINE_CORRECTION: i32 = (LFO_ONE as i64 * 225 / 1000) as i32;

/// Half a cycle of phase.
const HALF_CYCLE: u32 = 1 << 31;
/// A quarter cycle: the triangle and the sine start at zero, rising.
const QUARTER_CYCLE: u32 = 1 << 30;
/// The fade's full level, one in Q31 less a unit.
const FADE_FULL: i32 = i32::MAX;

/// The oscillator's waveform.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    /// A triangle, from zero, rising.
    Triangle,
    /// A sine, from zero, rising.
    Sine,
    /// A square: high for the first half of the cycle.
    Square,
    /// A new random level at the start of each cycle, held through it.
    SampleAndHold,
}

/// A low-frequency oscillator.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Lfo {
    phase: u32,
    step: u32,
    shape: Shape,
    noise: Noise,
    held: i32,
    /// Samples of silence after a key, and those left.
    delay: u32,
    waiting: u32,
    /// The fade's rise each sample, and its level, in Q31.
    fade_step: i32,
    fade: i32,
}

impl Lfo {
    /// A triangle at rest, with no delay and no fade, whose
    /// sample-and-hold levels come from `seed`.
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        let mut noise = Noise::new(seed);
        let held = noise.tick_level();
        Self {
            phase: 0,
            step: 0,
            shape: Shape::Triangle,
            noise,
            held,
            delay: 0,
            waiting: 0,
            fade_step: FADE_FULL,
            fade: FADE_FULL,
        }
    }

    /// Sets the frequency, as a step for the rate the oscillator is
    /// ticked at.
    pub const fn set_step(&mut self, step: u32) {
        self.step = step;
    }

    /// Sets the waveform.
    pub const fn set_shape(&mut self, shape: Shape) {
        self.shape = shape;
    }

    /// Sets what follows a key: `delay` samples of silence, then a rise
    /// to full depth along a straight line over `fade` samples. Zero for
    /// both is full depth at once.
    ///
    /// A wait that is running is held to the new delay, so that a delay
    /// shortened under a held note shortens what is left of the silence;
    /// and with nothing for both, whatever silence or fade is running
    /// ends at once.
    ///
    /// It divides, so it belongs where a control changes.
    pub const fn set_delay(&mut self, delay: u32, fade: u32) {
        self.delay = delay;
        self.fade_step = match FADE_FULL.checked_div(fade.cast_signed()) {
            Some(step) if step > 0 => step,
            _ => FADE_FULL,
        };
        if self.waiting > delay {
            self.waiting = delay;
        }
        if delay == 0 && self.fade_step == FADE_FULL {
            self.fade = FADE_FULL;
        }
    }

    /// A key: the delay and the fade start again. The cycle goes on
    /// where it was, as an oscillator that runs free does.
    pub const fn key(&mut self) {
        self.waiting = self.delay;
        self.fade = if self.delay == 0 && self.fade_step == FADE_FULL { FADE_FULL } else { 0 };
    }

    /// Starts the cycle again, for an instrument whose oscillator is
    /// reset by a key.
    pub const fn restart(&mut self) {
        self.phase = 0;
    }

    /// The phase, in units of 2⁻³² cycle.
    #[must_use]
    pub const fn phase(&self) -> u32 {
        self.phase
    }

    /// The waveform at full depth for the phase as it stands.
    #[inline]
    const fn wave(&self) -> i32 {
        match self.shape {
            Shape::Triangle => {
                // Folded about the half cycle, a quarter cycle on.
                let turned = self.phase.wrapping_add(QUARTER_CYCLE);
                let folded = if turned < HALF_CYCLE { turned } else { turned.wrapping_neg() };
                folded.cast_signed().wrapping_sub(LFO_ONE)
            }
            Shape::Sine => {
                // x from -1 to 1 over the cycle, in Q30: the phase as a
                // signed number.
                let x = self.phase.cast_signed() >> 1;
                let parabola = mul_high(x, LFO_ONE.wrapping_sub(x.wrapping_abs())) << 4;
                // p (1 + 0.225 (|p| - 1)).
                let correction = mul_high(SINE_CORRECTION, parabola.wrapping_abs().wrapping_sub(LFO_ONE)) << 2;
                parabola.wrapping_add(mul_high(parabola, correction) << 2)
            }
            Shape::Square => {
                if self.phase < HALF_CYCLE {
                    LFO_ONE
                } else {
                    -LFO_ONE
                }
            }
            Shape::SampleAndHold => self.held,
        }
    }

    /// One sample: gives the waveform at the depth the fade has reached,
    /// and moves on.
    #[inline]
    pub const fn tick(&mut self) -> i32 {
        let output = if self.fade == FADE_FULL { self.wave() } else { scale(self.wave(), self.fade) };
        if self.waiting > 0 {
            self.waiting = self.waiting.wrapping_sub(1);
        } else if self.fade != FADE_FULL {
            self.fade = self.fade.saturating_add(self.fade_step);
        }
        let next = self.phase.wrapping_add(self.step);
        if next < self.phase {
            self.held = self.noise.tick_level();
        }
        self.phase = next;
        output
    }
}
