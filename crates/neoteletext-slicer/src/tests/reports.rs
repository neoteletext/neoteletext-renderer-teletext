//! Measurements for the report, run with `--ignored` in release mode.
#![allow(
    clippy::cast_precision_loss,
    reason = "test code: counts and rates made floating point for measurements, all below 2^24 or within the measurement's tolerance"
)]

use super::rejection::biphase_false_packets;
use super::*;

#[test]
#[ignore = "measurement report; run in release mode"]
fn report_noise() {
    std::println!("REQ-SLC-013 noise (5000 lines each):");
    for rate in RATES {
        for snr in [20.0_f32, 15.0, 10.0] {
            let nominal = SignalParams::nominal(rate as f32);
            let params = SignalParams { noise_rms: nominal.noise_for_snr_db(snr), ..nominal };
            let t = run_batch(rate, &params, 5000, 2, Config::new());
            std::println!(
                "  {:>6.3} MHz {snr:>4} dB: addresses {:6.2}%  whole packets {:6.2}%  {:?}",
                f64::from(rate) / 1e6,
                t.address_percent(),
                t.exact_percent(),
                t.counters
            );
        }
    }
}

#[test]
#[ignore = "measurement report; run in release mode"]
fn report_impairments() {
    std::println!("Bit-rate error, fixed bit period (500 lines each):");
    for rate in RATES {
        for error in [0.0005_f32, 0.001, 0.0015, 0.002, 0.003, 0.005] {
            for sign in [-1.0_f32, 1.0] {
                let params = SignalParams { bit_rate_error: sign * error, ..SignalParams::nominal(rate as f32) };
                let t = run_batch(rate, &params, 500, 4, Config::new());
                std::println!(
                    "  {:>6.3} MHz {:+.2}%: addresses {:6.2}%  whole packets {:6.2}%",
                    f64::from(rate) / 1e6,
                    sign * error * 100.0,
                    t.address_percent(),
                    t.exact_percent()
                );
            }
        }
    }
    std::println!("High-frequency loss, first-order low-pass (500 lines each):");
    for rate in RATES {
        for corner in [5.0e6_f32, 4.0e6, 3.0e6, 2.5e6, 2.0e6] {
            let params = SignalParams { low_pass_hz: Some(corner), ..SignalParams::nominal(rate as f32) };
            let t = run_batch(rate, &params, 500, 5, Config::new());
            std::println!(
                "  {:>6.3} MHz corner {:.1} MHz: addresses {:6.2}%  whole packets {:6.2}%",
                f64::from(rate) / 1e6,
                corner / 1e6,
                t.address_percent(),
                t.exact_percent()
            );
        }
    }
    std::println!("Echo (500 lines each):");
    for rate in RATES {
        for (delay, gain) in
            [(200.0_f32, 0.2_f32), (500.0, 0.2), (800.0, 0.2), (200.0, 0.4), (500.0, 0.4), (800.0, -0.3)]
        {
            let params =
                SignalParams { echo: Some(Echo { delay_ns: delay, gain }), ..SignalParams::nominal(rate as f32) };
            let t = run_batch(rate, &params, 500, 6, Config::new());
            std::println!(
                "  {:>6.3} MHz {delay} ns x {gain}: addresses {:6.2}%  whole packets {:6.2}%",
                f64::from(rate) / 1e6,
                t.address_percent(),
                t.exact_percent()
            );
        }
    }
    std::println!("Black-level drift across the line, percent of data amplitude (500 lines each, 35.47 MHz):");
    for drift in [0.3_f32, 0.4, 0.45, 0.5, 0.6] {
        for sign in [-1.0_f32, 1.0] {
            let params = SignalParams {
                black_level: 80.0,
                black_drift: sign * drift * NOMINAL_AMPLITUDE_8BIT,
                ..SignalParams::nominal(RATE as f32)
            };
            let t = run_batch(RATE, &params, 500, 7, Config::new());
            std::println!(
                "  {:+.0}%: addresses {:6.2}%  whole packets {:6.2}%",
                sign * drift * 100.0,
                t.address_percent(),
                t.exact_percent()
            );
        }
    }
    std::println!("Random data after a valid run-in (false packets, 20000 lines, 35.47 MHz):");
    for tolerance in [true, false] {
        let mut generator = Generator::new(8);
        let mut slicer = Slicer::new(Config { framing_tolerance: tolerance, ..Config::new() });
        let mut accepted = 0;
        for _ in 0..20_000 {
            let mut bytes = [0u8; PACKET_BYTES_WITH_RUN_IN];
            for byte in &mut bytes {
                *byte = generator.rng().byte();
            }
            bytes[0] = CLOCK_RUN_IN;
            bytes[1] = CLOCK_RUN_IN;
            let samples = render(&mut generator, &SignalParams::nominal(RATE as f32), &bytes, samples_per_line(RATE));
            if slice_bytes(&mut slicer, &samples, RATE).is_ok() {
                accepted += 1;
            }
        }
        std::println!("  framing tolerance {tolerance}: {accepted} accepted, {:?}", slicer.counters());
    }
    std::println!("Biphase lines of random contents (false packets, 20000 lines, 35.47 MHz):");
    for (name, element_rate) in [("VPS shape", 10.0e6_f32), ("WSS shape", 5.0e6)] {
        for tolerance in [true, false] {
            let config = Config { framing_tolerance: tolerance, ..Config::new() };
            let (accepted, lines) = biphase_false_packets(element_rate, 20_000, config);
            std::println!("  {name}, framing tolerance {tolerance}: {accepted} of {lines} accepted");
        }
    }
}

/// DEC-1092, REQ-447: false packets from random data after a valid run-in,
/// and from VPS-shaped lines, on a line where no teletext was seen (held to
/// the exact framing code) and on a line expected to carry teletext (one
/// wrong bit tolerated). Each false line is preceded, in the field before,
/// by nothing on its line or by an exact packet, so that the expectation is
/// as stated.
#[test]
#[ignore = "measurement report; run in release mode"]
fn report_false_packets_by_line() {
    const LINES: u32 = 20_000;
    std::println!("Random data after a valid run-in ({LINES} lines, 35.47 MHz):");
    let exact = line_with(&line_bytes(&known_packet()), &nominal(), RATE);
    for expected in [false, true] {
        let mut generator = Generator::new(8);
        let mut slicer = Slicer::new(Config::new());
        let mut accepted = 0;
        for _ in 0..LINES {
            let mut bytes = [0u8; PACKET_BYTES_WITH_RUN_IN];
            for byte in &mut bytes {
                *byte = generator.rng().byte();
            }
            bytes[0] = CLOCK_RUN_IN;
            bytes[1] = CLOCK_RUN_IN;
            let samples = render(&mut generator, &SignalParams::nominal(RATE as f32), &bytes, samples_per_line(RATE));
            // Each field: when the line measured is expected, exact packets
            // on it in the fields before (it is then measured on the next
            // field's line); otherwise teletext on the line above it only.
            let line = if expected {
                for _ in 0..crate::consts::EXPECTED_LINE_EXACT_PACKETS {
                    assert!(slicer.slice(&placed(&exact, RATE, TELETEXT_LINE)).is_ok());
                }
                TELETEXT_LINE
            } else {
                assert!(slicer.slice(&placed(&exact, RATE, TELETEXT_LINE)).is_ok());
                TELETEXT_LINE + 1
            };
            if slicer.slice(&placed(&samples, RATE, line)).is_ok() {
                accepted += 1;
            }
        }
        let percent = 100.0 * f64::from(accepted) / f64::from(LINES);
        std::println!("  expected line {expected}: {accepted} accepted ({percent:.2}%)");
    }
    std::println!("VPS-shaped lines of random contents ({LINES} lines, 35.47 MHz), on a line no teletext was seen on:");
    let (accepted, lines) = biphase_false_packets(10.0e6, LINES, Config::new());
    let percent = 100.0 * f64::from(accepted) / f64::from(lines);
    std::println!("  {accepted} of {lines} accepted ({percent:.2}%)");
}
