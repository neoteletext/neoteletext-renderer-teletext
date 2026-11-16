//! From twice or four times the output rate down to it: half-band
//! low-pass filters, each taking two samples and giving one.
//!
//! A voice runs above the output rate where its filter distorts, so that
//! what the distortion and the oscillators' edges put above the hearing
//! range is removed here, not folded into it. An instrument adds its
//! voices and decimates the sum once.
//!
//! [`Decimator::sharp`] takes 96 kHz to 48 kHz: it is flat to 20 kHz and
//! removes everything from 28 kHz, which is what would fold below 20 kHz.
//! [`Decimator::wide`] takes 192 kHz to 96 kHz, where only what lies above
//! 76 kHz matters, and is a quarter of the length. `tests/analog_tables.rs`
//! measures both.

use super::fixed::{limit, mul_high};
use super::tables::{HALF_BAND_SHARP, HALF_BAND_WIDE};

/// Samples the filter remembers: a power of two at least the longest
/// filter's length, twice [`HALF_BAND_SHARP`]'s reach and one.
const HISTORY: usize = 128;
/// The mask that wraps a position in the history.
const HISTORY_MASK: usize = HISTORY - 1;

// The longest filter spans four taps for each one kept, less one.
const _: () = assert!(4 * HALF_BAND_SHARP.len() - 1 <= HISTORY);
const _: () = assert!(HALF_BAND_WIDE.len() <= HALF_BAND_SHARP.len());

/// A half-band decimator: two samples in, one out.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decimator {
    /// The taps 1, 3, 5 ... samples from the centre, in Q32.
    taps: &'static [i32],
    history: [i32; HISTORY],
    /// Where the next sample goes.
    next: usize,
}

impl Decimator {
    /// The decimator from twice the output rate to the output rate.
    #[must_use]
    pub const fn sharp() -> Self {
        Self { taps: &HALF_BAND_SHARP, history: [0; HISTORY], next: 0 }
    }

    /// The decimator from four times the output rate to twice.
    #[must_use]
    pub const fn wide() -> Self {
        Self { taps: &HALF_BAND_WIDE, history: [0; HISTORY], next: 0 }
    }

    /// Samples by which the output lags the input, counted at the input's
    /// rate: the filter's reach.
    #[must_use]
    pub const fn delay(&self) -> usize {
        self.taps.len().saturating_mul(2).saturating_sub(1)
    }

    fn push(&mut self, sample: i32) {
        if let Some(slot) = self.history.get_mut(self.next) {
            // Two samples at the bound still add inside 32 bits.
            *slot = limit(sample);
        }
        self.next = self.next.wrapping_add(1) & HISTORY_MASK;
    }

    /// Takes two samples, the earlier first, and gives the one sample of
    /// half the rate at the instant of the second less [`Self::delay`].
    /// Samples beyond [`LIMIT`](super::LIMIT) are held to it.
    #[inline]
    pub fn process(&mut self, earlier: i32, later: i32) -> i32 {
        self.push(earlier);
        self.push(later);
        let reach = self.delay();
        // The centre is `reach` samples before the newest.
        let centre = self.next.wrapping_sub(1).wrapping_sub(reach);
        let at = |offset: usize| self.history.get(offset & HISTORY_MASK).copied().unwrap_or(0);
        let mut sum = at(centre) >> 1;
        let mut distance = 1usize;
        for &tap in self.taps {
            let pair = at(centre.wrapping_sub(distance)).wrapping_add(at(centre.wrapping_add(distance)));
            sum = sum.wrapping_add(mul_high(pair, tap));
            distance = distance.wrapping_add(2);
        }
        sum
    }
}
