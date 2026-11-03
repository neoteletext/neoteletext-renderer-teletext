//! Tests of REQ-SLC-002, REQ-SLC-007, REQ-SLC-009, REQ-SLC-013: rates and noise; REQ-SLC-004 and REQ-SLC-006: timing, run-in, bit rate.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::similar_names
)]

use super::*;

#[test]
fn clean_lines_decode_at_every_rate() {
    for rate in RATES {
        let tally = run_batch(rate, &SignalParams::nominal(rate as f32), 200, 1, Config::new());
        assert_eq!(tally.exact, tally.lines, "rate {rate}: {tally:?}");
    }
}

#[test]
fn twenty_db_noise_keeps_addresses() {
    for rate in RATES {
        let params = SignalParams::nominal(rate as f32);
        let params = SignalParams { noise_rms: params.noise_for_snr_db(20.0), ..params };
        let tally = run_batch(rate, &params, 500, 3, Config::new());
        assert!(tally.address_percent() >= 99.0, "rate {rate}: {tally:?}");
    }
}

/// REQ-452, held at the tolerance's worst corner as measured on 2 November
/// 2026 (TODO-2077 item 6): 20 dB of noise at the highest rate and the
/// earliest timing offset of REQ-SLC-004 gave 99.2 per cent of addresses,
/// against the target of 99, so a change that loses the margin fails here.
#[test]
fn twenty_db_noise_keeps_addresses_at_the_worst_corner() {
    let rate = RATES[3];
    let params = SignalParams { timing_offset_ns: -1000.0, ..SignalParams::nominal(rate as f32) };
    let params = SignalParams { noise_rms: params.noise_for_snr_db(20.0), ..params };
    let tally = run_batch(rate, &params, 1000, 7, Config::new());
    assert!(tally.address_percent() >= 99.0, "{tally:?}");
}

/// REQ-441 (REQ-SLC-002): the slicer works at the lowest rate the
/// requirement names, 13.5 MHz (ITU-R BT.601), at nominal timing and at the
/// timing offsets the other rates are tested at.
#[test]
fn the_lowest_sample_rate_slices() {
    const LOWEST_RATE: u32 = 13_500_000;
    for timing_offset_ns in [-1000.0, -500.0, 0.0, 400.0] {
        let params = SignalParams { timing_offset_ns, ..SignalParams::nominal(LOWEST_RATE as f32) };
        let tally = run_batch(LOWEST_RATE, &params, 100, 11, Config::new());
        assert_eq!(tally.address_percent(), 100.0, "{timing_offset_ns} ns: {tally:?}");
    }
}

#[test]
fn sixteen_bit_samples() {
    let rate = RATES[0];
    let params = SignalParams::nominal(rate as f32);
    let mut generator = Generator::new(5);
    let mut slicer = Slicer::new(Config::new());
    let len = samples_per_line(rate);
    for _ in 0..50 {
        let packet = random_packet(generator.rng());
        let mut values = vec![0.0_f32; len];
        generator.line(&params, &line_bytes(&packet), &mut values);
        let mut bytes = vec![0u8; len * 2];
        quantise_u16le(&values, SAMPLE_16_PER_8 as f32, &mut bytes);
        let sliced = slicer.slice(&Line::new(&bytes, SampleFormat::U16Le, rate)).unwrap();
        assert_eq!(sliced.packet, packet);
        let amplitude = sliced.meta.quality.unwrap().amplitude_percent;
        assert!((97..=103).contains(&amplitude), "amplitude {amplitude}");
    }
}

// ---------------------------------------------------------------------------
// REQ-SLC-004 and REQ-SLC-006: timing, run-in, bit rate
// ---------------------------------------------------------------------------

#[test]
fn timing_across_the_full_tolerance() {
    let early = -(TIMING_EARLY_NS as i32);
    let late = TIMING_LATE_NS as i32;
    for rate in RATES {
        let mut generator = Generator::new(11);
        let mut slicer = Slicer::new(Config::new());
        for offset in (early..=late).step_by(25) {
            let params = SignalParams { timing_offset_ns: offset as f32, ..SignalParams::nominal(rate as f32) };
            let packet = random_packet(generator.rng());
            let samples = render(&mut generator, &params, &line_bytes(&packet), samples_per_line(rate));
            let sliced = slice_bytes(&mut slicer, &samples, rate).unwrap_or_else(|e| panic!("{rate} {offset}: {e:?}"));
            assert_eq!(sliced.packet, packet, "rate {rate} offset {offset}");
        }
    }
}

#[test]
fn capture_windows_starting_anywhere() {
    // Lines whose first sample is at various times after the
    // synchronising edge, as different capture devices produce, cut to
    // 57.7 µs as in the bt8x8 capture. The window must still hold the whole
    // packet, from 9.2 µs at the earliest to 62.5 µs at the latest.
    for rate in RATES {
        let mut generator = Generator::new(12);
        let mut slicer = Slicer::new(Config::new());
        let len = (f64::from(rate) * 57_700.0 / 1e9) as usize;
        for start in [5_000.0, 6_000.0, 7_000.0, 8_000.0, 9_000.0] {
            let params = SignalParams { start_ns: start, ..SignalParams::nominal(rate as f32) };
            let packet = random_packet(generator.rng());
            let samples = render(&mut generator, &params, &line_bytes(&packet), len);
            let sliced = slice_bytes(&mut slicer, &samples, rate).unwrap_or_else(|e| panic!("{rate} {start}: {e:?}"));
            assert_eq!(sliced.packet, packet, "rate {rate} start {start}");
        }
    }
}

#[test]
fn first_two_run_in_bits_reduced_or_absent() {
    for rate in RATES {
        for scale in [0.0, 0.3, 0.6] {
            let params = SignalParams { first_bits_scale: scale, ..SignalParams::nominal(rate as f32) };
            let tally = run_batch(rate, &params, 20, 13, Config::new());
            assert_eq!(tally.exact, tally.lines, "rate {rate} scale {scale}");
        }
    }
}

#[test]
fn bit_rate_error_with_fixed_period() {
    // The Phase 1 clock has a fixed period, so a bit-rate error moves the
    // late bits off their centres: whole packets survive up to 0.05
    // percent, and addresses (just after the run-in) up to 0.5 percent.
    // Following larger errors needs the Phase 2 tracking loop.
    for rate in RATES {
        for error in [-0.0005_f32, 0.0005] {
            let params = SignalParams { bit_rate_error: error, ..SignalParams::nominal(rate as f32) };
            let tally = run_batch(rate, &params, 20, 14, Config::new());
            assert_eq!(tally.exact, tally.lines, "rate {rate} error {error}");
        }
        for error in [-0.005_f32, 0.005] {
            let params = SignalParams { bit_rate_error: error, ..SignalParams::nominal(rate as f32) };
            let tally = run_batch(rate, &params, 20, 14, Config::new());
            assert_eq!(tally.address_correct, tally.lines, "rate {rate} error {error}");
        }
    }
}

#[test]
fn search_window_limits_the_search() {
    let samples = line_with(&line_bytes(&known_packet()), &nominal(), RATE);
    let mut slicer = Slicer::new(Config { search_end: Some(100), ..Config::new() });
    assert_eq!(slice_bytes(&mut slicer, &samples, RATE), Err(Rejection::NoSignal));
    let mut slicer = Slicer::new(Config { search_start: 300, search_end: Some(400), ..Config::new() });
    assert_eq!(slice_bytes(&mut slicer, &samples, RATE).unwrap().packet, known_packet());
}
