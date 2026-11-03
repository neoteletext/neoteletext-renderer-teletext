//! Constants of the slicer, each defined once (REQ-GEN-014).
//!
//! Teletext values already in `neoteletext_core::consts` are reused, never
//! restated; the derived values below are computed from them. Four groups
//! follow:
//!
//! - **Signal values of EN 300 706**, re-exported from the core, where
//!   decision 6 of `IMPLEMENTATION-LOG.md` puts them.
//! - **Video standard values** (ITU-R BT.470 / BT.1700 for 625-line PAL, and
//!   ITU-R BT.601 sample coding), used to find the back porch and by the
//!   test signal generator.
//! - **Capture devices** of the raw sampled line files (REQ-INP-011): the
//!   rates and line counts of their drivers and datasheets (DEC-2472).
//! - **Tuning values** of this slicer, each with its reason.

use neoteletext_core::consts::{PACKET_BITS, PACKET_BYTES, PACKET_BYTES_WITH_FRAMING, PACKET_BYTES_WITH_RUN_IN};

// ---------------------------------------------------------------------------
// Derived from neoteletext-core
// ---------------------------------------------------------------------------

/// Bits per byte of the packet, derived from the core's packet size.
pub const BITS_PER_BYTE: usize = PACKET_BITS / PACKET_BYTES;
/// Bits of the clock run-in: its two bytes (clause 6.1, 16 bits).
pub const RUN_IN_BITS: usize = (PACKET_BYTES_WITH_RUN_IN - PACKET_BYTES_WITH_FRAMING) * BITS_PER_BYTE;
/// Bits of the framing code: its one byte (clause 6.2).
pub const FRAMING_BITS: usize = (PACKET_BYTES_WITH_FRAMING - PACKET_BYTES) * BITS_PER_BYTE;
/// Bits of the whole data line, run-in to the last packet byte: 360
/// (ITU-R BT.653 Table 1a item 1.7).
pub const LINE_DATA_BITS: usize = PACKET_BYTES_WITH_RUN_IN * BITS_PER_BYTE;

// ---------------------------------------------------------------------------
// EN 300 706 signal values, defined in neoteletext-core
// ---------------------------------------------------------------------------

pub use neoteletext_core::consts::{
    BITS_PER_LINE, ONE_LEVEL_PERCENT_OF_WHITE, RUN_IN_UNRELIABLE_BITS, TIMING_EARLY_NS, TIMING_LATE_NS,
    TIMING_REFERENCE_NS,
};

/// Run-in bits the search matches: bits 3 to 16 (REQ-SLC-004).
pub const RUN_IN_MATCH_BITS: usize = RUN_IN_BITS - RUN_IN_UNRELIABLE_BITS;
/// The timing reference bit, counted from 1: the penultimate "1" of the
/// run-in (clause 6.3). The run-in's "1"s are its odd bits, the last being
/// bit 15, so the penultimate one is bit 13.
pub const TIMING_REFERENCE_BIT: usize = RUN_IN_BITS - 3;

// ---------------------------------------------------------------------------
// Video standard values (not teletext)
// ---------------------------------------------------------------------------

/// Line synchronising pulse width, nanoseconds (ITU-R BT.470 Table 1, 625
/// lines: 4.7 µs).
pub const SYNC_WIDTH_NS: u32 = 4_700;
/// Start of the colour burst after the synchronising edge, nanoseconds
/// (ITU-R BT.470 PAL: 5.6 µs).
pub const BURST_START_NS: u32 = 5_600;
/// Cycles in the colour burst (ITU-R BT.470 PAL: 10 ± 1).
pub const BURST_CYCLES: u32 = 10;
/// One more burst cycle than nominal, the upper tolerance of BT.470.
pub const BURST_CYCLES_MAX: u32 = BURST_CYCLES + 1;
/// PAL colour subcarrier frequency, hertz (ITU-R BT.470: 4,433,618.75).
pub const PAL_SUBCARRIER_HZ: f32 = 4_433_618.75;
/// Synchronising pulse depth below black, millivolts (BT.470: 300 mV).
pub const SYNC_DEPTH_MV: u32 = 300;
/// Black to peak white, millivolts (BT.470: 700 mV).
pub const BLACK_TO_WHITE_MV: u32 = 700;
/// Colour burst peak-to-peak amplitude, millivolts (BT.470 PAL: 300 mV).
pub const BURST_PEAK_TO_PEAK_MV: u32 = 300;
/// Black level of 8-bit studio coding (ITU-R BT.601: code 16).
pub const BT601_BLACK_8BIT: u32 = 16;
/// Peak white of 8-bit studio coding (ITU-R BT.601: code 235).
pub const BT601_WHITE_8BIT: u32 = 235;
/// Scale from 8-bit to 16-bit samples: one more byte of resolution.
pub const SAMPLE_16_PER_8: u32 = 256;
/// Percent scale.
pub const PERCENT: f32 = 100.0;
/// The nominal "1" level above black in 8-bit studio coding: 66 percent of
/// black-to-white (EN 300 706 clause 5.2) with BT.601 black at 16 and white
/// at 235, about 144.5 codes.
// The product is 14,454, which f32 holds exactly.
#[allow(clippy::cast_precision_loss)]
pub const NOMINAL_AMPLITUDE_8BIT: f32 =
    ((BT601_WHITE_8BIT - BT601_BLACK_8BIT) * ONE_LEVEL_PERCENT_OF_WHITE) as f32 / PERCENT;
/// The same for 16-bit samples: the 8-bit coding scaled by 256.
#[allow(clippy::cast_precision_loss)]
pub const NOMINAL_AMPLITUDE_16BIT: f32 = NOMINAL_AMPLITUDE_8BIT * SAMPLE_16_PER_8 as f32;

// ---------------------------------------------------------------------------
// Capture devices of the raw sampled line files (REQ-INP-011): their drivers'
// and datasheets' rates and line counts, not teletext's
// ---------------------------------------------------------------------------

/// Fields per frame of 625-line television (ITU-R BT.470).
pub const FIELDS_PER_FRAME: usize = 2;
/// The PAL sampling rate of the bt8x8 and cx88 drivers: 8 × the PAL colour
/// subcarrier, hertz (REQ-INP-011 table).
pub const RATE_8_FSC_PAL: u32 = 35_468_950;
/// The saa7131 sampling rate, hertz (REQ-INP-011 table).
pub const RATE_SAA7131: u32 = 27_000_000;
/// The ld-decode time-base-corrected rate, hertz (REQ-INP-011 table).
pub const RATE_TBC: u32 = 17_730_000;
/// Samples per line of the bt8x8 and cx88 captures (REQ-INP-011 table).
pub const SAMPLES_8_FSC: usize = 2048;
/// Samples per line of the saa7131 capture (REQ-INP-011 table).
pub const SAMPLES_SAA7131: usize = 1440;
/// Samples per line of the tbc captures (REQ-INP-011 table).
pub const SAMPLES_TBC: usize = 1135;
/// Lines per field of the bt8x8, saa7131 and tbc-vbi captures.
pub const LINES_16: usize = 16;
/// Lines per field of the cx88 capture.
pub const LINES_CX88: usize = 18;
/// Lines per field of a full tbc field: the longer field of a frame.
pub const LINES_TBC: usize = MAX_LINES_PER_FIELD;
/// Lines used of the cx88 capture: indices 1 to 16.
pub const CX88_USED: (usize, usize) = (1, 16);
/// Lines used of a full tbc field: indices 6 to 21.
pub const TBC_USED: (usize, usize) = (6, 21);
/// Television line numbers of the first bt8x8 line of each field: lines 7
/// to 22 and 320 to 335 (REQ-INP-011 table).
pub const BT8X8_FIRST_LINES: [u16; 2] = [7, 320];
/// Bytes of the frame sequence number the Linux bt8x8 driver may write at
/// the end of each frame (REQ-INP-011).
pub const BT8X8_SEQUENCE_BYTES: usize = 4;

// ---------------------------------------------------------------------------
// Slicer tuning
// ---------------------------------------------------------------------------

/// The lowest sample rate the slicer accepts, hertz (REQ-SLC-002: about 1.95
/// samples per bit).
pub const MIN_SAMPLE_RATE_HZ: u32 = 13_500_000;
/// The most samples of one line the slicer reads. A 64 µs line at 64 MHz
/// fits; longer lines are cut to this length so the work per line is
/// bounded.
pub const MAX_LINE_SAMPLES: usize = 4096;
/// Default minimum data amplitude, percent of nominal (REQ-SLC-005).
pub const DEFAULT_MIN_AMPLITUDE_PERCENT: u16 = 50;
/// Default minimum run-in match, percent. A run-in at 10 dB signal-to-noise
/// still scores about 85; a random waveform scores about 0. Half way keeps
/// a wide margin both ways.
pub const DEFAULT_MIN_MATCH_PERCENT: u8 = 50;
/// The run-in search reports a candidate when the window's alternating
/// component has at least this fraction of the gate's minimum amplitude, so
/// the gate, not the search, decides amplitude rejections (REQ-FSM-002).
pub const DETECT_AMPLITUDE_FRACTION: f32 = 0.5;
/// The fraction of the search window's varying energy that must lie at half
/// the bit rate for a candidate run-in. A clean run-in gives about 1, one
/// at 10 dB signal-to-noise about 0.55, and white noise about 2 divided by
/// the window length in samples (under 0.05).
pub const DETECT_CONCENTRATION: f32 = 0.3;
/// After the first candidate position, the search follows the match for
/// this many more bit periods and keeps the strongest. The detector can
/// fire when the window has only just entered the run-in, about a quarter
/// of the way; the window is then full after up to 14 more bits, and the
/// alternating pattern (run-in bits 1 to 16 and the first framing bit, 17
/// bits) keeps it full for three more.
pub const PEAK_FOLLOW_BITS: usize = RUN_IN_MATCH_BITS + 3;
/// The framing code is looked for within this many bit periods either side
/// of its expected position (REQ-SLC-008).
pub const FRAMING_SEARCH_BITS: usize = 2;
/// Histogram bins used to find the black level when the synchronising edge
/// is unknown: one per 8-bit code.
pub const BLACK_HISTOGRAM_BINS: usize = 256;
/// Half-width of the smoothing applied to the black-level histogram, in
/// bins. Five bins span about one third of the noise width at 20 dB.
pub const BLACK_HISTOGRAM_SMOOTH: usize = 2;
/// Percent of a line's samples, from the lowest, set aside as the
/// synchronising pulse when looking for black: the pulse is 4.7 µs of a
/// 64 µs line, 7.3 percent, and the lower half of the colour burst adds
/// about 1.8 percent. Black (blanking and "0" bits) is at least a quarter
/// of a line even on a row of solid mosaics, so its peak survives.
pub const BLACK_LOW_PERCENTILE: u32 = 15;
/// Percentile taken as the top of the line's range when looking for
/// black; the highest 2 percent are overshoots and noise peaks.
pub const BLACK_HIGH_PERCENTILE: u32 = 98;
/// Half-width, in bins, of the samples averaged around the histogram peak
/// to refine the black level below one bin.
pub const BLACK_REFINE_BINS: usize = 3;
/// Guard after the end of the colour burst before the back-porch window
/// used for the black level, nanoseconds: about one subcarrier cycle for the
/// burst envelope to decay.
pub const BACK_PORCH_GUARD_NS: u32 = 250;
/// Steps of the phasor recurrence between renormalisations, so rounding
/// cannot let its magnitude drift.
pub const PHASOR_RENORMALISE_STEPS: usize = 256;

/// Lines in the longer field of a 625-line frame: 625 lines in two fields,
/// the half line counted whole (ITU-R BT.470).
pub const MAX_LINES_PER_FIELD: usize = 313;
/// Lines whose expectation of teletext is tracked: every line number of a
/// 625-line frame (1 to 625, slot 0 unused), or every position of either
/// field when only the position is known (DEC-1092).
pub const EXPECTED_LINE_SLOTS: usize = FIELDS_PER_FRAME * MAX_LINES_PER_FIELD;
/// Fields for which a line stays expected to carry teletext after its last
/// packet with an exact framing code, and the most fields between the exact
/// packets that make it expected (DEC-1092, DEC-2431): one second at
/// 50 fields a second. An inserter that serves a line sends a packet on it
/// in every field, so a second spans many exact packets on any signal the
/// one-bit tolerance could help, and a line whose teletext has gone is
/// strict again within a second.
pub const EXPECTED_LINE_FIELDS: u32 = 50;
/// Exact packets in a row, each within [`EXPECTED_LINE_FIELDS`] of the one
/// before, that make a line expected (DEC-2431). Random data after a
/// run-in has an exact framing code and a valid address on about 0.34 per
/// cent of lines, so one such packet would make a noisy line expected
/// often; measured, one gave 1.25 per cent false packets on lines that never
/// carried teletext, where three keep the exact code's rate.
pub const EXPECTED_LINE_EXACT_PACKETS: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_values() {
        assert_eq!(BITS_PER_BYTE, 8);
        assert_eq!(RUN_IN_BITS, 16);
        assert_eq!(FRAMING_BITS, 8);
        assert_eq!(LINE_DATA_BITS, 360);
        assert_eq!(RUN_IN_MATCH_BITS, 14);
        assert_eq!(TIMING_REFERENCE_BIT, 13);
        // Clause 5.3: 444 bits per 64 µs line at 6.9375 Mbit/s.
        assert_eq!(u64::from(neoteletext_core::consts::BIT_RATE_HZ) * 64, u64::from(BITS_PER_LINE) * 1_000_000);
    }
}
