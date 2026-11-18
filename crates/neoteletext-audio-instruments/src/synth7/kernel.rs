//! The sound kernel: six operators combined by one of 32 algorithms, built
//! the way the instrument's sound chip is.
//!
//! Each operator has a phase that advances every sample. The phase, plus
//! whatever modulates it, addresses a quarter-wave table of the *logarithm*
//! of a sine; the operator's attenuation, also a logarithm, is added; and a
//! table of powers of two with a shift turns the sum back into a sample. An
//! operator therefore costs two table lookups and no multiplication.
//!
//! Source: Ken Shirriff's articles on the YM21280 operator chip
//! (righto.com, November 2021 to February 2022): "Reverse-engineering the
//! Yamaha DX7 synthesizer's sound chip", "part II: the exponential ROM",
//! "part III: Inside the log-sine ROM", "part IV: how algorithms are
//! implemented" and "part V: the output circuitry".
//!
//! # Units
//!
//! - **Phase** is a `u32` in which 2^32 is a whole turn. The chip keeps 23
//!   bits and uses the top 12 to address the sine; so does this, with more
//!   bits below for exact pitch at 48,000 samples a second.
//! - **Attenuation** is a logarithm to base two with 24 fractional bits:
//!   [`ATTENUATION_ONE`] halves the amplitude. The log-sine table's ten
//!   fractional bits are its top ten; the rest make a level glide between
//!   two blocks without steps.
//! - **A sample** of an operator is a signed integer of at most
//!   [`SINE_BITS`] bits of magnitude: ±16,372 at no attenuation.
//! - **Modulation** is the modulating operators' samples added to the top
//!   12 bits of the phase with no scaling: an operator at no attenuation
//!   swings the phase of what it modulates by four turns each way.

use super::msfa::LG_N;
use super::tables::{EXPONENT, LOG_ONE, LOG_SINE, ROM_ENTRIES};

// The voice's figures are the interface's, defined once with the voice
// (`synth7.md`, S7-28), which the server reads without the kernel.
pub use super::patch::{ALGORITHM_COUNT, FEEDBACK_MAX, OPERATORS};

/// The logarithm to base two of [`BLOCK_SAMPLES`].
///
/// Source: the parameter layer's block (`msfa::LG_N`), so that a level
/// reaches its target exactly when the next one is computed.
pub const BLOCK_SAMPLES_LOG2: u32 = LG_N;

/// Samples over which an operator's attenuation glides from one level to
/// the next.
pub const BLOCK_SAMPLES: usize = 1 << BLOCK_SAMPLES_LOG2;

/// [`BLOCK_SAMPLES`] as the divisor of a signed difference.
const BLOCK_SAMPLES_I32: i32 = 1 << BLOCK_SAMPLES_LOG2;

/// Bits of an operator's attenuation below one doubling.
///
/// Source: own choice: the parameter layer's fixed point, taken unchanged.
pub const ATTENUATION_FRACTION_BITS: u32 = 24;

/// The attenuation that halves an operator's amplitude.
pub const ATTENUATION_ONE: u32 = 1 << ATTENUATION_FRACTION_BITS;

/// The largest attenuation: 16 doublings, the range of the chip's 14-bit
/// logarithm (4 bits of integer, 10 of fraction).
///
/// Source: Shirriff, part II: "The circuit takes a 14-bit input value that
/// consists of a 4-bit integer part and 10 fractional bits".
pub const ATTENUATION_MAX: u32 = 16 << ATTENUATION_FRACTION_BITS;

/// The attenuation at and above which an operator's sample is exactly
/// zero: 14 doublings. The exponent table's largest entry is 4,095, which
/// shifted left by two is under 2^14, and the log-sine adds only more
/// attenuation.
///
/// Source: derived from [`operator_sample`].
pub const ATTENUATION_SILENT: u32 = 14 << ATTENUATION_FRACTION_BITS;

/// Bits of magnitude in an operator's sample.
///
/// Source: Shirriff, part II, note 2: the signal exponent "has just 14 bits
/// of output", `round(2^frac × 2048) << int >> 13`.
pub const SINE_BITS: u32 = 14;

/// The shift that takes an attenuation to the units of the log-sine table.
const ATTENUATION_TO_LOG_SHIFT: u32 = ATTENUATION_FRACTION_BITS - LOG_ONE.trailing_zeros();

/// Bits of phase that address the sine: a sign, a mirror and ten bits of
/// address.
///
/// Source: Shirriff, part III: "The top bit is used as the sign bit ... The
/// second bit implements the mirror symmetry ... The ROM and associated
/// logic take a 10-bit input address".
const PHASE_ADDRESS_BITS: u32 = 12;

/// The shift that takes a phase to its sine address.
const PHASE_SHIFT: u32 = u32::BITS - PHASE_ADDRESS_BITS;

/// The address bit that says the second half of the wave: negative.
const ADDRESS_SIGN: u32 = 1 << (PHASE_ADDRESS_BITS - 1);

/// The address bit that says the second half of a half wave: mirrored.
const ADDRESS_MIRROR: u32 = 1 << (PHASE_ADDRESS_BITS - 2);

/// The address within a quarter wave.
const ADDRESS_MASK: u32 = (ROM_ENTRIES as u32) - 1;

/// The exponent table's entries, of 12 bits, are shifted left by this to
/// make a sample at no attenuation: the chip's `<< 15 >> 13`.
///
/// Source: Shirriff, part II, note 2, as [`SINE_BITS`].
const MANTISSA_SHIFT: u32 = 2;

/// The sum of the feedback operator's last two samples is shifted left by
/// this plus the feedback setting to make a phase.
///
/// Source: the chip averages the previous two samples and scales the mean
/// by a power of two (Shirriff, part IV, note 4). How far is the parameter
/// layer's: at feedback 7 it shifts the sum of two samples right by two
/// where 2^24 is a turn (`msfa::note::feedback_shift`, plus one for the
/// mean), and each setting below halves that. This kernel's samples are 12
/// bits smaller than that layer's and its turn is 8 bits larger, so the
/// sum is shifted left by `12 + 8 - 2 = 18` at feedback 7, and by
/// `11 + setting` in general.
const FEEDBACK_BASE_SHIFT: u32 = 11;

/// One of the 32 ways the six operators are combined.
///
/// Operators are numbered as the instrument numbers them, 1 to 6, and
/// operator `n` is bit `n - 1` of a mask and index `n - 1` of an array. An
/// operator is modulated only by operators of higher number, which the
/// kernel computes first, as the chip does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    /// For each operator, the mask of the operators whose samples are added
    /// to its phase.
    pub modulators: [u8; OPERATORS],
    /// The mask of the operators whose samples are added to make the
    /// voice's output.
    pub carriers: u8,
    /// The index of the operator whose last two samples are the feedback.
    pub feedback_from: u8,
    /// The index of the operator whose phase the feedback is added to. It
    /// is the same operator in all algorithms but 4 and 6, whose feedback
    /// loops pass through three and two operators.
    pub feedback_to: u8,
}

impl Algorithm {
    /// How many operators are carriers.
    #[must_use]
    pub const fn carrier_count(&self) -> u32 {
        self.carriers.count_ones()
    }

    /// Whether the operator at `index` (0 for operator 1) is a carrier.
    #[must_use]
    pub const fn is_carrier(&self, index: usize) -> bool {
        index < OPERATORS && (self.carriers >> index) & 1 == 1
    }
}

/// The 32 algorithms; entry 0 is the instrument's algorithm 1.
///
/// Source: own table, written from the algorithm chart printed on the
/// instrument and in its manual. `tests/synth7_algorithms.rs` checks it
/// against the routing of music-synthesizer-for-android's `fm_core.cc`
/// (`algorithms`) and against the output counts in the chip's algorithm
/// ROM (Shirriff, part IV, note 5).
#[rustfmt::skip]
pub const ALGORITHMS: [Algorithm; ALGORITHM_COUNT] = [
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b01_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 5, feedback_to: 5 }, // 1
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b01_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 1, feedback_to: 1 }, // 2
    Algorithm { modulators: [0b00_0010, 0b00_0100, 0b00_0000, 0b01_0000, 0b10_0000, 0b00_0000], carriers: 0b00_1001, feedback_from: 5, feedback_to: 5 }, // 3
    Algorithm { modulators: [0b00_0010, 0b00_0100, 0b00_0000, 0b01_0000, 0b10_0000, 0b00_0000], carriers: 0b00_1001, feedback_from: 3, feedback_to: 5 }, // 4
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b01_0101, feedback_from: 5, feedback_to: 5 }, // 5
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b01_0101, feedback_from: 4, feedback_to: 5 }, // 6
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b01_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 5, feedback_to: 5 }, // 7
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b01_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 3, feedback_to: 3 }, // 8
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b01_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 1, feedback_to: 1 }, // 9
    Algorithm { modulators: [0b00_0010, 0b00_0100, 0b00_0000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_1001, feedback_from: 2, feedback_to: 2 }, // 10
    Algorithm { modulators: [0b00_0010, 0b00_0100, 0b00_0000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_1001, feedback_from: 5, feedback_to: 5 }, // 11
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b11_1000, 0b00_0000, 0b00_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 1, feedback_to: 1 }, // 12
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b11_1000, 0b00_0000, 0b00_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 5, feedback_to: 5 }, // 13
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 5, feedback_to: 5 }, // 14
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_0101, feedback_from: 1, feedback_to: 1 }, // 15
    Algorithm { modulators: [0b01_0110, 0b00_0000, 0b00_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0001, feedback_from: 5, feedback_to: 5 }, // 16
    Algorithm { modulators: [0b01_0110, 0b00_0000, 0b00_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0001, feedback_from: 1, feedback_to: 1 }, // 17
    Algorithm { modulators: [0b00_1110, 0b00_0000, 0b00_0000, 0b01_0000, 0b10_0000, 0b00_0000], carriers: 0b00_0001, feedback_from: 2, feedback_to: 2 }, // 18
    Algorithm { modulators: [0b00_0010, 0b00_0100, 0b00_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1001, feedback_from: 5, feedback_to: 5 }, // 19
    Algorithm { modulators: [0b00_0100, 0b00_0100, 0b00_0000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_1011, feedback_from: 2, feedback_to: 2 }, // 20
    Algorithm { modulators: [0b00_0100, 0b00_0100, 0b00_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1011, feedback_from: 2, feedback_to: 2 }, // 21
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b10_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1101, feedback_from: 5, feedback_to: 5 }, // 22
    Algorithm { modulators: [0b00_0000, 0b00_0100, 0b00_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1011, feedback_from: 5, feedback_to: 5 }, // 23
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b10_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1111, feedback_from: 5, feedback_to: 5 }, // 24
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b00_0000, 0b10_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1111, feedback_from: 5, feedback_to: 5 }, // 25
    Algorithm { modulators: [0b00_0000, 0b00_0100, 0b00_0000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_1011, feedback_from: 5, feedback_to: 5 }, // 26
    Algorithm { modulators: [0b00_0000, 0b00_0100, 0b00_0000, 0b11_0000, 0b00_0000, 0b00_0000], carriers: 0b00_1011, feedback_from: 2, feedback_to: 2 }, // 27
    Algorithm { modulators: [0b00_0010, 0b00_0000, 0b00_1000, 0b01_0000, 0b00_0000, 0b00_0000], carriers: 0b10_0101, feedback_from: 4, feedback_to: 4 }, // 28
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b00_1000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b01_0111, feedback_from: 5, feedback_to: 5 }, // 29
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b00_1000, 0b01_0000, 0b00_0000, 0b00_0000], carriers: 0b10_0111, feedback_from: 4, feedback_to: 4 }, // 30
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b00_0000, 0b00_0000, 0b10_0000, 0b00_0000], carriers: 0b01_1111, feedback_from: 5, feedback_to: 5 }, // 31
    Algorithm { modulators: [0b00_0000, 0b00_0000, 0b00_0000, 0b00_0000, 0b00_0000, 0b00_0000], carriers: 0b11_1111, feedback_from: 5, feedback_to: 5 }, // 32
];

/// What is added to the attenuation of each carrier of an algorithm with
/// `n` carriers, at index `n`, in units of 1/8 of a doubling: the
/// logarithms to base two of 1 to 6 as the chip's small ROM holds them
/// (binary 01.000, 01.101, 10.000, 10.011 and 10.101 for 2 to 6). It makes
/// an algorithm of six carriers about as loud as one of one.
///
/// Source: Shirriff, part IV, "The compensation (COM) computation" and its
/// note 8.
pub const CARRIER_COMPENSATION_EIGHTHS: [u8; OPERATORS + 1] = [0, 0, 0b01_000, 0b01_101, 0b10_000, 0b10_011, 0b10_101];

/// The fractional bits of [`CARRIER_COMPENSATION_EIGHTHS`].
const COMPENSATION_FRACTION_BITS: u32 = 3;

/// The attenuation added to each carrier of `algorithm`, so that its
/// carriers together are about as loud as a single one.
#[must_use]
pub fn carrier_compensation(algorithm: &Algorithm) -> u32 {
    let count = usize::try_from(algorithm.carrier_count()).unwrap_or(OPERATORS);
    let eighths = CARRIER_COMPENSATION_EIGHTHS.get(count).copied().unwrap_or(0);
    u32::from(eighths) << (ATTENUATION_FRACTION_BITS - COMPENSATION_FRACTION_BITS)
}

/// One sample of an operator: the sine of `phase` (2^32 is a turn),
/// attenuated by `attenuation` (see [`ATTENUATION_ONE`]). The result is
/// within ±16,372.
///
/// Source: Shirriff, parts II and III: the log-sine of the phase, plus the
/// attenuation, through the exponent table and a shift.
#[inline]
#[must_use]
pub fn operator_sample(phase: u32, attenuation: u32) -> i32 {
    let angle = phase >> PHASE_SHIFT;
    let mut address = angle & ADDRESS_MASK;
    if angle & ADDRESS_MIRROR != 0 {
        address ^= ADDRESS_MASK;
    }
    let log_sine = u32::from(LOG_SINE.get(address as usize).copied().unwrap_or(0));
    let total = log_sine.saturating_add(attenuation >> ATTENUATION_TO_LOG_SHIFT);
    let integer = total >> LOG_ONE.trailing_zeros();
    // The chip's exponent takes the complement of the attenuation: no
    // attenuation is its largest input.
    let fraction = !total & ADDRESS_MASK;
    let mantissa = u32::from(EXPONENT.get(fraction as usize).copied().unwrap_or(0));
    let magnitude = (mantissa << MANTISSA_SHIFT).checked_shr(integer).unwrap_or(0).cast_signed();
    if angle & ADDRESS_SIGN == 0 { magnitude } else { magnitude.wrapping_neg() }
}

/// The sine of `phase` (2^32 is a turn) at no attenuation: within ±16,372,
/// that is [`SINE_BITS`] bits of magnitude.
#[must_use]
pub fn sine(phase: u32) -> i32 {
    operator_sample(phase, 0)
}

/// The state of one operator.
#[derive(Clone, Copy, Debug)]
struct Operator {
    /// The phase now.
    phase: u32,
    /// What is added to the phase each sample.
    increment: u32,
    /// The attenuation now.
    attenuation: u32,
    /// The attenuation at the end of the block.
    target: u32,
    /// What is added to the attenuation each sample of the block.
    step: i32,
}

impl Operator {
    const fn new() -> Self {
        Self { phase: 0, increment: 0, attenuation: ATTENUATION_MAX, target: ATTENUATION_MAX, step: 0 }
    }
}

/// The sound of one note: six operators and the feedback register.
#[derive(Clone, Copy, Debug)]
pub struct Voice {
    operators: [Operator; OPERATORS],
    /// The feedback operator's last two samples, older first.
    feedback: [i32; 2],
    /// Samples of the block not yet rendered.
    remaining: usize,
    /// Silent since [`Voice::silence`]: the next block starts at its levels
    /// and does not glide to them.
    fresh: bool,
    /// Aimed part way through the block ([`Voice::aim_operator`]): the next
    /// block's glides start where the levels stand, not at the targets
    /// they were aimed at, which they need not have reached.
    carried: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

impl Voice {
    /// A silent voice.
    #[must_use]
    pub const fn new() -> Self {
        Self { operators: [Operator::new(); OPERATORS], feedback: [0; 2], remaining: 0, fresh: true, carried: false }
    }

    /// Makes every operator silent at once and clears the feedback
    /// register. Phases are kept. The block after it starts at the levels
    /// it is given, as a key going down on the instrument starts a note at
    /// its envelope's first level, with no glide from silence.
    pub fn silence(&mut self) {
        for operator in &mut self.operators {
            operator.attenuation = ATTENUATION_MAX;
            operator.target = ATTENUATION_MAX;
            operator.step = 0;
        }
        self.feedback = [0; 2];
        self.remaining = 0;
        self.fresh = true;
        self.carried = false;
    }

    /// Whether the voice is silent since [`Voice::silence`] and has not yet
    /// begun a block: its first levels are taken at once, not glided to.
    #[must_use]
    pub const fn is_fresh(&self) -> bool {
        self.fresh
    }

    /// Sets every operator's phase to zero: the instrument's oscillator key
    /// sync.
    pub fn reset_phases(&mut self) {
        for operator in &mut self.operators {
            operator.phase = 0;
        }
    }

    /// Sets what the operator at `index` (0 for operator 1) does over the
    /// block that [`Voice::begin_block`] then starts: its phase advances by
    /// `increment` each sample, and its attenuation glides from the last
    /// block's target to `attenuation`, which is limited to
    /// [`ATTENUATION_MAX`]; in the first block after [`Voice::silence`] it
    /// is at `attenuation` from the start. An index of 6 or more does
    /// nothing.
    pub fn set_operator(&mut self, index: usize, increment: u32, attenuation: u32) {
        if let Some(operator) = self.operators.get_mut(index) {
            operator.increment = increment;
            let previous = operator.target;
            operator.target = attenuation.min(ATTENUATION_MAX);
            operator.attenuation = if self.fresh {
                operator.target
            } else if self.carried {
                operator.attenuation
            } else {
                previous
            };
            // Both are at most 2^28, so the difference fits an i32.
            let difference = operator.target.cast_signed().wrapping_sub(operator.attenuation.cast_signed());
            // Rounded towards zero, so that the glide never passes its
            // target: rounded down, a glide down to no attenuation would
            // pass zero within the block, and wrap round to silence.
            operator.step = difference / BLOCK_SAMPLES_I32;
        }
    }

    /// Sets what the operator at `index` (0 for operator 1) does over the
    /// rest of a block, the `samples` that [`Voice::begin_rest`] then
    /// starts, from part way through it: for a message that arrives there
    /// (`synth7.md`, S7-52). Its phase advances by `increment` from the
    /// sample it has reached. Its attenuation goes on gliding as it was,
    /// and the change of its target to `attenuation` (limited to
    /// [`ATTENUATION_MAX`]) is glided over [`BLOCK_SAMPLES`] from here, as
    /// a block's boundary glides it: by the block's end it has gone the
    /// share of the change that the samples left are of a block, and the
    /// next block's glide starts where it stands. Gliding the change over
    /// only the samples left would step a level that the envelopes move at
    /// once (an attack at its quickest, a key struck again) by as much as
    /// all of it in one sample. After [`Voice::silence`] the attenuation is
    /// `attenuation` from the start. An index of 6 or more does nothing.
    pub fn aim_operator(&mut self, index: usize, increment: u32, attenuation: u32, samples: usize) {
        let fresh = self.fresh;
        let Some(operator) = self.operators.get_mut(index) else { return };
        operator.increment = increment;
        let target = attenuation.min(ATTENUATION_MAX);
        if fresh {
            operator.attenuation = target;
            operator.target = target;
            operator.step = 0;
            return;
        }
        // Every attenuation is at most 2^28, so each difference and each
        // product below, of at most a block's samples, fits an i32.
        let samples = i32::try_from(samples.clamp(1, BLOCK_SAMPLES)).unwrap_or(BLOCK_SAMPLES_I32);
        let change = target.cast_signed().wrapping_sub(operator.target.cast_signed()) / BLOCK_SAMPLES_I32;
        let now = operator.attenuation.cast_signed();
        let path = operator.step.saturating_add(change).saturating_mul(samples);
        // Where the level stands at the block's end, held within the range:
        // a glide that left it would wrap round to silence.
        let end = now.saturating_add(path).clamp(0, ATTENUATION_MAX.cast_signed());
        // Rounded towards zero, as in `set_operator`, so that the glide
        // never passes its end.
        operator.step = end.wrapping_sub(now).checked_div(samples).unwrap_or(0);
        operator.target = target;
        self.carried = true;
    }

    /// Starts a block of [`BLOCK_SAMPLES`] samples.
    pub fn begin_block(&mut self) {
        self.carried = false;
        self.begin_rest(BLOCK_SAMPLES);
    }

    /// Starts the last `samples` of a block, at most [`BLOCK_SAMPLES`]: for
    /// a voice whose operators [`Voice::aim_operator`] set part way through
    /// its rack's block, so that it ends the block with the others.
    pub fn begin_rest(&mut self, samples: usize) {
        self.fresh = false;
        self.remaining = samples.min(BLOCK_SAMPLES);
    }

    /// Adds the voice's next samples to `out`: as many as `out` holds, but
    /// no more than are left of the block; the samples of `out` beyond that
    /// are left as they are. `feedback` is the patch's setting, 0 to 7
    /// (more is taken as 7). A sample is within ±16,372 times the number of
    /// carriers, before their compensation.
    pub fn render(&mut self, algorithm: &Algorithm, feedback: u8, out: &mut [i32]) {
        let count = out.len().min(self.remaining);
        self.remaining = self.remaining.saturating_sub(count);

        // Each operator's modulators as places in an array of this sample's
        // values: at most three, the rest pointing at a place that stays
        // zero. The loop below then has no branch on the algorithm.
        let mut sources = [[ZERO_PLACE; MODULATORS_MAX]; OPERATORS];
        for (places, &modulators) in sources.iter_mut().zip(&algorithm.modulators) {
            let mut modulating = (0..OPERATORS).filter(|&source| (modulators >> source) & 1 == 1);
            for place in places {
                *place = modulating.next().unwrap_or(ZERO_PLACE);
            }
        }
        let mut carrier_masks = [0_i32; OPERATORS];
        let mut feedback_masks = [0_u32; OPERATORS];
        for (index, (carrier, to)) in carrier_masks.iter_mut().zip(&mut feedback_masks).enumerate() {
            *carrier = mask_of(algorithm.is_carrier(index));
            *to = mask_of(index == usize::from(algorithm.feedback_to)).cast_unsigned();
        }
        let feedback_from = usize::from(algorithm.feedback_from) & PLACE_MASK;
        let feedback_shift = if feedback == 0 {
            None
        } else {
            Some(FEEDBACK_BASE_SHIFT.saturating_add(u32::from(feedback.min(FEEDBACK_MAX))))
        };

        let [mut older, mut newer] = self.feedback;
        for sample in out.iter_mut().take(count) {
            let feedback_phase = match feedback_shift {
                Some(shift) => older.wrapping_add(newer).cast_unsigned().wrapping_shl(shift),
                None => 0,
            };
            let mut values = [0_i32; PLACES];
            let mut mixed = 0_i32;
            // Operator 6 first, as the chip computes them, so that every
            // modulator's sample is ready when it is needed.
            let routes = sources.iter().zip(&carrier_masks).zip(&feedback_masks);
            for (index, (operator, ((places, &carrier_mask), &to_mask))) in
                self.operators.iter_mut().zip(routes).enumerate().rev()
            {
                let modulation = places.iter().fold(0_i32, |sum, &place| sum.wrapping_add(value_at(&values, place)));
                let phase = operator
                    .phase
                    .wrapping_add(modulation.cast_unsigned().wrapping_shl(PHASE_SHIFT))
                    .wrapping_add(feedback_phase & to_mask);
                let value = operator_sample(phase, operator.attenuation);
                if let Some(slot) = values.get_mut(index) {
                    *slot = value;
                }
                mixed = mixed.wrapping_add(value & carrier_mask);
                operator.phase = operator.phase.wrapping_add(operator.increment);
                operator.attenuation = operator.attenuation.wrapping_add_signed(operator.step);
            }
            older = newer;
            newer = value_at(&values, feedback_from);
            *sample = sample.wrapping_add(mixed);
        }
        self.feedback = [older, newer];
    }
}

/// The most operators that modulate one operator in any algorithm: three,
/// in algorithms 12, 13, 16, 17 and 18.
///
/// Source: [`ALGORITHMS`]; `tests/synth7_algorithms.rs` checks it.
pub const MODULATORS_MAX: usize = 3;

/// Places in the array of one sample's values: the six operators', and
/// two that stay zero to make a power of two.
const PLACES: usize = 8;

/// A place that stays zero.
const ZERO_PLACE: usize = PLACES - 1;

/// The bits of a place.
const PLACE_MASK: usize = PLACES - 1;

/// The value at a place; the mask makes any place a valid one.
#[inline]
fn value_at(values: &[i32; PLACES], place: usize) -> i32 {
    values.get(place & PLACE_MASK).copied().unwrap_or(0)
}

/// All ones for true, all zeros for false.
const fn mask_of(set: bool) -> i32 {
    if set { -1 } else { 0 }
}
