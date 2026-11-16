//! The building blocks of the analogue synthesizers (DEC-6429): what the
//! emulations of the Juno-60 and Juno-106, the Minimoog, the Prophet-5,
//! the CS-80 and the Jupiter-8 are made from.
//!
//! # Signals, rates and frequencies
//!
//! A signal is an `i32` in which an oscillator at full level peaks at
//! [`LEVEL`], 2²⁴. Every block holds what it keeps to [`LIMIT`], 2²⁹, so
//! 30 dB above an oscillator is carried and nothing wraps; [`to_sample`]
//! makes the 16-bit sample of the crate's output.
//!
//! A voice may run at the output rate, [`SAMPLE_RATE`](crate::SAMPLE_RATE),
//! or at twice or four times it ([`Oversampling`]), where a filter that
//! distorts needs the room; a [`Decimator`](decimator::Decimator) or two
//! bring the sum of the voices down. No block knows its rate: a frequency
//! is given as a [`frequency`] step, the share of a cycle that passes in
//! one sample of whatever rate the block is run at, in units of 2⁻³²
//! cycle. Oscillators, filter cutoffs and the low-frequency oscillator
//! all take it.
//!
//! Arithmetic is in 32 bits throughout, with no division and no 64-bit
//! product in anything that runs for each sample ([`fixed`]); what sets a
//! parameter may divide, and says so.

pub mod decimator;
pub mod envelope;
pub mod filter;
pub mod fixed;
pub mod oscillator;
pub mod tables;

use crate::SAMPLE_RATE;

/// The peak of an oscillator at full level.
///
/// Source: own choice: 24 bits leave a 16-bit sample eight bits of
/// fraction for the filters' slow integrators, and five bits of headroom.
pub const LEVEL: i32 = 1 << 24;

/// The largest magnitude a block keeps or passes on: 30 dB above
/// [`LEVEL`]. Two signals at the bound add inside 32 bits.
///
/// Source: own choice.
pub const LIMIT: i32 = 1 << 29;

/// The right shift from a signal to a 16-bit sample: [`LEVEL`] is the
/// sample's full scale.
const SAMPLE_SHIFT: u32 = 9;
const _: () = assert!(LEVEL >> SAMPLE_SHIFT == 1 << 15);

/// The 16-bit sample of a signal, rounded, and saturated beyond
/// [`LEVEL`].
#[must_use]
pub const fn to_sample(signal: i32) -> i16 {
    let rounded = signal.saturating_add(1 << (SAMPLE_SHIFT - 1)) >> SAMPLE_SHIFT;
    if rounded > i16::MAX as i32 {
        i16::MAX
    } else if rounded < i16::MIN as i32 {
        i16::MIN
    } else {
        rounded as i16
    }
}

/// How many times the output rate a voice runs at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Oversampling {
    /// The output rate itself.
    None,
    /// Twice the output rate: one sharp decimator.
    Twice,
    /// Four times the output rate: a wide decimator, then the sharp one.
    FourTimes,
}

impl Oversampling {
    /// Samples of the voice for each sample of output.
    #[must_use]
    pub const fn factor(self) -> u32 {
        match self {
            Self::None => 1,
            Self::Twice => 2,
            Self::FourTimes => 4,
        }
    }

    /// The voice's rate in hertz.
    #[must_use]
    pub const fn rate(self) -> u32 {
        SAMPLE_RATE.saturating_mul(self.factor())
    }
}

/// The largest frequency step of an oscillator and of the state-variable
/// filter: a quarter of the sampling rate. An oscillator's edges are then
/// at least four samples apart, and that filter's integrator gain stays
/// below one.
pub const STEP_LIMIT: u32 = 1 << 30;

/// The step of half the sampling rate, beyond which a frequency means
/// nothing.
const HALF_RATE: u64 = 1 << 31;

/// Thousandths of a hertz in a hertz.
const MILLIHERTZ: u64 = 1_000;

/// The step of a frequency given in thousandths of a hertz, at a rate in
/// hertz: 2³² times the frequency over the rate, rounded, and held to
/// half the rate; each block holds it to its own bound besides. A rate of
/// zero gives a step of zero.
///
/// It divides, so it belongs where a note or a control changes, not in
/// the loop that renders samples.
#[must_use]
pub const fn frequency(millihertz: u32, rate: u32) -> u32 {
    let divisor = (rate as u64).saturating_mul(MILLIHERTZ);
    let rounded = ((millihertz as u64) << 32).saturating_add(divisor >> 1);
    match rounded.checked_div(divisor) {
        Some(step) if step <= HALF_RATE => step as u32,
        Some(_) => HALF_RATE as u32,
        None => 0,
    }
}
