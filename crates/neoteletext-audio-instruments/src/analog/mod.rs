//! The building blocks of the analogue synthesizers (DEC-6429): what the
//! emulations of the Juno-60 and Juno-106, the Minimoog, the Prophet-5,
//! the CS-80 and the Jupiter-8 are made from. They make no instrument by
//! themselves: `tests/analog_voice.rs` puts them together as an
//! instrument would.
//!
//! - [`oscillator`]: a sawtooth, a pulse of any width, a triangle and a
//!   sub-oscillator from one phase, band-limited; hard sync; white
//!   noise.
//! - [`filter`]: a one-pole filter, the four-pole transistor ladder, the
//!   four-pole amplifier cascade with its two outputs, and the two-pole
//!   state-variable filter.
//! - [`envelope`]: the analogue envelope, and the one a processor
//!   computes in steps.
//! - [`lfo`]: the low-frequency oscillator, with its delay and fade.
//! - [`chorus`]: the bucket-brigade chorus.
//! - [`drift`]: each voice's tolerances, its drift, and detuning in
//!   cents.
//! - [`allocator`]: which voice plays which key.
//! - [`decimator`]: from twice or four times the output rate down to it.
//! - [`fixed`], [`octave`] and [`tables`]: the arithmetic, the intervals and
//!   their exponential, and the tables under them.
//!
//! # Signals, rates and frequencies
//!
//! A signal is an `i32` in which an oscillator at full level peaks at
//! [`LEVEL`], 2²⁴. Every block holds what it keeps to [`LIMIT`], 2²⁹, so
//! 30 dB above an oscillator is carried and nothing wraps; [`to_sample`]
//! makes the 16-bit sample of the crate's output. Levels of envelopes
//! and of the low-frequency oscillator are in Q30.
//!
//! A voice may run at the output rate, [`SAMPLE_RATE`],
//! or at twice or four times it ([`Oversampling`]), where a filter that
//! distorts needs the room; a [`Decimator`](decimator::Decimator) or two
//! bring the sum of the voices down. No block knows its rate: a frequency
//! is given as a [`frequency`] step, the share of a cycle that passes in
//! one sample of whatever rate the block is run at, in units of 2⁻³²
//! cycle. Oscillators, filter cutoffs and the low-frequency oscillator
//! all take it.
//!
//! Arithmetic is in 32 bits throughout, with no division in anything
//! that runs for each sample, and no 64-bit product on a processor whose
//! word is not 64 bits ([`fixed`]); what sets a parameter and divides
//! says so. Nothing allocates, nothing panics, and every value of every
//! parameter is valid: each is held to its range, as its documentation
//! says.
//!
//! # What it costs
//!
//! Eight voices, each of two oscillators, a four-pole filter that
//! distorts and an envelope, at twice the output rate, with the decimator
//! and the chorus, render 13 to 14 times faster than real time on one
//! core of a 3.7 GHz Intel Core i9-10900KF, and 5 to 6 times with the
//! products taken in halves as a 32-bit processor takes them
//! (`tests/analog_voice.rs`, `rendering_speed`, 17 November 2026).
//!
//! # Whose work this is
//!
//! It is written under the clean-room procedure of DEC-6428: from public
//! documents and our own derivations, with no code and no table from any
//! emulator or plugin. Every constant says, where it is defined, which
//! document it comes from, or that it is an own choice and why. The
//! documents:
//!
//! - Vadim Zavalishin, "The Art of VA Filter Design", rev. 2.1.2:
//!   trapezoidal integrators and their instantaneous response, cutoff
//!   prewarping, zero-delay feedback (chapter 3); the state-variable
//!   filter (chapter 4); the ladder filter (chapter 5); saturators
//!   (chapter 6). The method of [`filter`].
//! - Tim Stilson and Julius Smith, "Analyzing the Moog VCF with
//!   considerations for digital implementation" (ICMC 1996): the
//!   ladder's transfer function and its oscillation at a feedback of
//!   four.
//! - Timothy Stinchcombe, "Analysis of the Moog transistor ladder and
//!   derivative filters", section 2.1.1: the hyperbolic tangent
//!   of a differential pair, which is each ladder stage's distortion.
//! - Electric Druid, "Roland filter designs with the IR3109 or AS3109":
//!   the amplifier cascade's four stages, its feedback through a
//!   soft-clipping amplifier, and the Jupiter-8's two outputs.
//! - Curtis Electromusic Specialties, CEM3310 Voltage Controlled
//!   Envelope Generator data sheet (1979): the attack's asymptote and
//!   peak.
//! - Colin Raffel and Julius Smith, "Practical modeling of
//!   bucket-brigade device circuits" (DAFx-10, 2010), and the MN3009
//!   data sheet (Xvive Audio's, of Panasonic's chip): the chorus.
//! - Florian Anwander, "Roland choruses and ensemble effects": the two
//!   lines swept oppositely by one oscillator.
//! - For band-limited oscillators: Stilson and Smith, "Alias-free
//!   digital synthesis of classic analog waveforms" (ICMC 1996); Eli
//!   Brandt, "Hard sync without aliasing" (ICMC 2001); Vesa Välimäki
//!   and Antti Huovilainen, "Antialiasing oscillators in subtractive
//!   synthesis" (IEEE Signal Processing Magazine, March 2007). These
//!   three are cited for the method from general knowledge of them;
//!   they are not among the documents filed for this work, and the
//!   residual tables are our own design.
//! - General: Kaiser's window (Kaiser 1974, as in Oppenheim and Schafer,
//!   "Discrete-Time Signal Processing", section 7.5); Marsaglia,
//!   "Xorshift RNGs" (2003); Catmull and Rom's interpolating cubic
//!   (1974); Newton's iteration for a reciprocal.
//!
//! # What is not here, or is weaker than the circuits
//!
//! - **The instruments themselves**: their controls' laws, their ranges,
//!   their calibration, their voice structure. Every level, cutoff range
//!   and time here is a parameter.
//! - **Oscillators at the output rate** alias: 48 dB below the strongest
//!   harmonic for fundamentals to 1 kHz and 34 dB at 4 kHz. The 80 dB
//!   asked of them (they reach 91 dB) needs the voice at twice the
//!   output rate or more. There is no sine and no noise other than
//!   white; an oscillator's phase cannot be modulated, only its
//!   frequency and its pulse's width.
//! - **The four-pole filters' distortion** is the textbook hyperbolic
//!   tangent in each stage, with one saturation level and matched
//!   stages; no instrument's transistors, bias, input mixer overdrive or
//!   capacitor leakage is modelled. The implicit equations are solved by
//!   three passes, not to convergence: the error left is 34 dB below
//!   the output in the worst case measured and 64 dB at a cutoff of
//!   2 kHz ([`filter::PASSES`]). Their cutoff is limited to 0.45 of the
//!   rate.
//! - **The state-variable filter does not distort**, and its cutoff is
//!   limited to a quarter of the rate. The CS-80's own filter circuit
//!   has not been read.
//! - **Oscillation above a resonance of four is flatter than the
//!   cutoff**, as the equations are: 2 cents at 4.02, 19 at 4.2, 70 at
//!   5. No compensation is applied.
//! - **The chorus** leaves out the chip's aliasing, distortion and
//!   noise, takes the delay from the clock as it stands, and sweeps the
//!   delay, not the clock's frequency, in a straight line; its filters'
//!   alignment is Butterworth's, not any instrument's capacitors
//!   ([`chorus`]).
//! - **The stepped envelope's shapes** are general: no instrument's
//!   processor's arithmetic has been read.
//! - **Tolerances are uniform** within their amounts; no instrument's
//!   spread has been measured.
//! - **The allocator's modes** are from general knowledge of such
//!   instruments, not yet checked against the five manuals.
//! - **Envelope coefficients have 31 bits**: the longest time constants
//!   at the highest rates are set to within a tenth of a per cent.

pub mod allocator;
pub mod chorus;
pub mod decimator;
pub mod drift;
pub mod envelope;
pub mod filter;
pub mod fixed;
pub mod lfo;
pub mod octave;
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
