//! synth6581's voices, heard at the output: pitch, the shape of each
//! waveform, synchronisation and ring modulation, the envelope's times and
//! the noise's rate (REQ-6412 item 4).

// Without synth6581's feature there is nothing here to test.
#![cfg(feature = "synth6581")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test"
)]

#[path = "synth6581_support.rs"]
mod support;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth6581::{ChipClock, ENVELOPE_3, OSCILLATOR_3, Synth6581, Variant};
use support::{
    CLOCKS, MODELS, amplitude, decibels, frequency_register, pitch, render, set_frequency, start, zero_crossing_pitch,
};

const TRIANGLE: u8 = 0x10;
const SAWTOOTH: u8 = 0x20;
const PULSE: u8 = 0x40;
const NOISE: u8 = 0x80;
const SYNC: u8 = 0x02;
const RING: u8 = 0x04;
const FULL_VOLUME: u8 = 0x0F;
const VOLUME_REGISTER: u8 = 24;

/// A chip with the volume up and voice 1 started.
fn playing(model: Variant, clock: ChipClock, hz: f64, control: u8) -> Synth6581 {
    let mut synth6581 = Synth6581::new(model, clock);
    synth6581.write(VOLUME_REGISTER, FULL_VOLUME);
    start(&mut synth6581, 0, frequency_register(hz, clock), control);
    synth6581
}

/// A second of the chip's output after it has settled.
fn settled(synth6581: &mut Synth6581) -> Vec<i16> {
    let _ = render(synth6581, 0.5);
    render(synth6581, 1.0)
}

#[test]
fn concert_a_is_440_hz_on_both_clocks() {
    for model in MODELS {
        for clock in CLOCKS {
            let mut synth6581 = playing(model, clock, 440.0, TRIANGLE);
            let _ = render(&mut synth6581, 0.5);
            let measured = zero_crossing_pitch(&render(&mut synth6581, 2.0));
            // The register's own step is 0.06 Hz; the requirement is 0.1
            // per cent.
            assert!((measured - 440.0).abs() < 0.44, "{model:?} {clock:?}: {measured} Hz");
            let exact = pitch(frequency_register(440.0, clock), clock);
            assert!((measured - exact).abs() < 0.01, "{model:?} {clock:?}: {measured} Hz for {exact}");
        }
    }
}

/// The amplitudes of the first `count` harmonics of `hz`, the first as 1.
fn harmonics(samples: &[i16], hz: f64, count: usize) -> Vec<f64> {
    let first = amplitude(samples, hz);
    (1..=count).map(|harmonic| amplitude(samples, hz * harmonic as f64) / first).collect()
}

#[test]
fn the_sawtooth_has_every_harmonic_at_one_over_its_number() {
    for model in MODELS {
        let mut synth6581 = playing(model, ChipClock::Pal, 220.0, SAWTOOTH);
        let found = harmonics(&settled(&mut synth6581), 220.0, 8);
        for (index, &level) in found.iter().enumerate() {
            let expected = 1.0 / (index + 1) as f64;
            assert!((level / expected - 1.0).abs() < 0.03, "{model:?} harmonic {}: {level}", index + 1);
        }
    }
}

#[test]
fn the_triangle_has_odd_harmonics_at_one_over_their_square() {
    for model in MODELS {
        let mut synth6581 = playing(model, ChipClock::Pal, 220.0, TRIANGLE);
        let found = harmonics(&settled(&mut synth6581), 220.0, 7);
        for (index, &level) in found.iter().enumerate() {
            let harmonic = index + 1;
            if harmonic % 2 == 1 {
                let expected = 1.0 / (harmonic * harmonic) as f64;
                assert!((level / expected - 1.0).abs() < 0.05, "{model:?} harmonic {harmonic}: {level}");
            } else {
                assert!(level < 0.002, "{model:?} harmonic {harmonic}: {level}");
            }
        }
    }
}

#[test]
fn the_pulse_has_the_harmonics_of_its_width() {
    for (width, duty) in [(0x800u16, 0.5f64), (0x400, 0.25), (0x200, 0.125)] {
        let mut synth6581 = playing(Variant::Synth8580, ChipClock::Pal, 220.0, PULSE);
        synth6581.write(2, width as u8);
        synth6581.write(3, (width >> 8) as u8);
        let found = harmonics(&settled(&mut synth6581), 220.0, 8);
        // A pulse of duty d has harmonic n at |sin(π n d)| / n.
        let shape = |harmonic: usize| (std::f64::consts::PI * harmonic as f64 * duty).sin().abs() / harmonic as f64;
        for (index, &level) in found.iter().enumerate() {
            let expected = shape(index + 1) / shape(1);
            assert!((level - expected).abs() < 0.01, "width {width:#x} harmonic {}: {level} for {expected}", index + 1);
        }
    }
}

#[test]
fn a_pulse_width_of_zero_is_silent() {
    let mut synth6581 = playing(Variant::Synth8580, ChipClock::Pal, 220.0, PULSE);
    let samples = settled(&mut synth6581);
    assert!(samples.iter().all(|&sample| sample.abs() <= 1), "{:?}", &samples[..8]);
}

#[test]
fn sync_gives_voice_1_the_period_of_voice_3() {
    let clock = ChipClock::Pal;
    let (slave, master) = (700.0, 300.0);
    let mut free = playing(Variant::Synth8580, clock, slave, SAWTOOTH);
    set_frequency(&mut free, 2, frequency_register(master, clock));
    let mut synced = playing(Variant::Synth8580, clock, slave, SAWTOOTH | SYNC);
    set_frequency(&mut synced, 2, frequency_register(master, clock));
    let (free, synced) = (settled(&mut free), settled(&mut synced));
    // Free, the voice sounds at its own pitch; synchronised, its spectrum
    // is the harmonics of voice 3, and its own pitch is none of them.
    let own = amplitude(&free, slave);
    assert!(amplitude(&free, master) < own / 100.0);
    assert!(amplitude(&synced, master) > own / 10.0);
    assert!(amplitude(&synced, 2.0 * master) > own / 10.0);
    assert!(amplitude(&synced, slave) < own / 20.0, "{} of {own}", amplitude(&synced, slave));
}

#[test]
fn ring_modulation_moves_the_triangle_to_the_sum_and_difference() {
    let clock = ChipClock::Pal;
    let (carrier, modulator) = (440.0, 1000.0);
    let mut plain = playing(Variant::Synth8580, clock, carrier, TRIANGLE);
    set_frequency(&mut plain, 2, frequency_register(modulator, clock));
    let mut ring = playing(Variant::Synth8580, clock, carrier, TRIANGLE | RING);
    set_frequency(&mut ring, 2, frequency_register(modulator, clock));
    let (plain, ring) = (settled(&mut plain), settled(&mut ring));
    let own = amplitude(&plain, carrier);
    assert!(amplitude(&plain, modulator - carrier) < own / 100.0);
    // The triangle times a square wave: its fundamental moves to the
    // square's fundamental plus and minus it, at 4/π of half its level.
    let expected = own * 2.0 / std::f64::consts::PI;
    for hz in [modulator - carrier, modulator + carrier] {
        let found = amplitude(&ring, hz);
        assert!((found / expected - 1.0).abs() < 0.05, "{hz} Hz: {found} for {expected}");
    }
    assert!(amplitude(&ring, carrier) < own / 50.0);
}

/// Samples until register `ENVELOPE_3` first reads `level`.
fn samples_until(synth6581: &mut Synth6581, level: u8, limit: usize) -> usize {
    let mut sample = [0i16; 1];
    for count in 0..limit {
        if synth6581.read(ENVELOPE_3) == level {
            return count;
        }
        synth6581.render(&mut sample);
    }
    panic!("the envelope never read {level}");
}

/// Samples until register `ENVELOPE_3` first reads above zero. The
/// fastest attack takes more than one step in a sample, so no one level
/// can be waited for.
fn samples_until_sounding(synth6581: &mut Synth6581, limit: usize) -> usize {
    let mut sample = [0i16; 1];
    for count in 0..limit {
        if synth6581.read(ENVELOPE_3) != 0 {
            return count;
        }
        synth6581.render(&mut sample);
    }
    panic!("the envelope never rose");
}

#[test]
fn the_envelope_keeps_the_data_sheets_times() {
    // Data sheet Table 2 in milliseconds at 1 MHz: attack, then decay or
    // release.
    let table =
        [(0u8, 2.0, 6.0), (2, 16.0, 48.0), (4, 38.0, 114.0), (8, 100.0, 300.0), (9, 250.0, 750.0), (10, 500.0, 1500.0)];
    for clock in CLOCKS {
        let scale = 1_000_000.0 / f64::from(clock.hz()) * f64::from(SAMPLE_RATE) / 1000.0;
        for &(rate, attack, fall) in &table {
            let mut synth6581 = Synth6581::new(Variant::Synth6581, clock);
            let third = support::voice(2);
            synth6581.write(third + 5, (rate << 4) | rate);
            synth6581.write(third + 6, rate);
            synth6581.write(third + 4, 0x01);
            let rise = samples_until(&mut synth6581, 255, 100_000) as f64;
            // The sustain level is zero: the decay runs to silence.
            let decay = samples_until(&mut synth6581, 0, 200_000) as f64;
            // Within 4 per cent and a sample, but for the fastest rate,
            // which the chip's table makes 9 cycles a step, not 8.
            let tolerance = if rate == 0 { 0.2 } else { 0.04 };
            for (found, nominal) in [(rise, attack * scale), (decay, fall * scale)] {
                assert!(
                    (found - nominal).abs() <= nominal * tolerance + 1.0,
                    "{clock:?} rate {rate}: {found} for {nominal}"
                );
            }

            // The release from the peak takes the decay's time.
            let mut synth6581 = Synth6581::new(Variant::Synth6581, clock);
            synth6581.write(third + 6, 0xF0 | rate);
            synth6581.write(third + 4, 0x01);
            samples_until(&mut synth6581, 255, 1_000);
            synth6581.write(third + 4, 0x00);
            let release = samples_until(&mut synth6581, 0, 200_000) as f64;
            assert!((release - decay).abs() <= 2.0, "{clock:?} rate {rate}: {release} against {decay}");
        }
    }
}

#[test]
fn the_envelope_holds_each_sustain_level() {
    for nibble in 0u8..16 {
        let mut synth6581 = Synth6581::new(Variant::Synth8580, ChipClock::Pal);
        let third = support::voice(2);
        synth6581.write(third + 6, nibble << 4);
        synth6581.write(third + 4, 0x01);
        let _ = render(&mut synth6581, 0.1);
        assert_eq!(synth6581.read(ENVELOPE_3), nibble * 17);
        // And the level heard follows it: a sawtooth at that envelope.
        synth6581.write(VOLUME_REGISTER, FULL_VOLUME);
        set_frequency(&mut synth6581, 2, frequency_register(440.0, ChipClock::Pal));
        synth6581.write(third + 4, SAWTOOTH | 0x01);
        let level = amplitude(&settled(&mut synth6581), 440.0);
        // A sawtooth of 4,095 * 255 units is 2/π of half that at its
        // fundamental, and 65,536 / 26 units, synth8580's output gain, are
        // one of a sample at each of the volume's fifteen steps.
        let full = 4095.0 * 255.0 / std::f64::consts::PI * 15.0 * 26.0 / 65536.0;
        let expected = full * f64::from(nibble) / 15.0;
        assert!((level - expected).abs() < full * 0.01, "sustain {nibble}: {level} for {expected}");
    }
}

#[test]
fn a_faster_rate_written_late_waits_for_the_divider_to_come_round() {
    let third = support::voice(2);
    // Gate at the slowest attack, and let the rate divider run 49 samples,
    // about a thousand cycles, towards its 31,251.
    let mut synth6581 = Synth6581::new(Variant::Synth6581, ChipClock::Pal);
    synth6581.write(third + 5, 0xF0);
    synth6581.write(third + 6, 0xF0);
    synth6581.write(third + 4, 0x01);
    let mut samples = [0i16; 49];
    synth6581.render(&mut samples);
    assert_eq!(synth6581.read(ENVELOPE_3), 0);
    let passed = synth6581.cycles();
    // The fastest attack's 9 cycles are now behind the divider: the first
    // step waits for the divider's 32,767 states to come round.
    synth6581.write(third + 5, 0x00);
    let waited = samples_until_sounding(&mut synth6581, 2_000) as f64;
    let expected = (32_767.0 - passed as f64 + 9.0) * f64::from(SAMPLE_RATE) / f64::from(ChipClock::Pal.hz());
    assert!((waited - expected).abs() <= 1.0, "{waited} samples for {expected}");
    // Once round, the attack runs at its rate: 254 steps of 9 cycles, less
    // the two that fit in the sample that saw the first.
    let rise = samples_until(&mut synth6581, 255, 1_000) as f64;
    assert!((rise - 254.0 * 9.0 * 48_000.0 / 985_248.0).abs() <= 2.0, "{rise}");

    // Written before the gate, the same attack starts within a sample.
    let mut synth6581 = Synth6581::new(Variant::Synth6581, ChipClock::Pal);
    synth6581.write(third + 6, 0xF0);
    synth6581.write(third + 4, 0x01);
    assert!(samples_until_sounding(&mut synth6581, 10) <= 1);
}

#[test]
fn noise_changes_at_the_oscillators_rate() {
    for clock in CLOCKS {
        let mut synth6581 = Synth6581::new(Variant::Synth6581, clock);
        let third = support::voice(2);
        // Frequency $0400: bit 19 of the accumulator rises every 1,024
        // cycles, some 50 samples.
        set_frequency(&mut synth6581, 2, 0x0400);
        synth6581.write(third + 4, NOISE);
        let mut sample = [0i16; 1];
        let mut last = synth6581.read(OSCILLATOR_3);
        let mut changes = 0u32;
        for _ in 0..4 * SAMPLE_RATE {
            synth6581.render(&mut sample);
            let value = synth6581.read(OSCILLATOR_3);
            changes += u32::from(value != last);
            last = value;
        }
        // One shift in 1,024 cycles, and all but a few shifts change one
        // of the eight bits read.
        let shifts = 4 * clock.hz() / 1024;
        assert!(changes <= shifts && changes > shifts * 9 / 10, "{clock:?}: {changes} changes in {shifts} shifts");
    }
}

#[test]
fn noise_is_broadband_and_unpitched() {
    let mut synth6581 = playing(Variant::Synth8580, ChipClock::Pal, 3000.0, NOISE);
    let samples = settled(&mut synth6581);
    let overall = support::rms(&samples);
    assert!(overall > 500.0, "rms {overall}");
    // No component stands out as a tone would.
    for hz in [250.0, 1000.0, 3000.0, 6000.0] {
        assert!(decibels(amplitude(&samples, hz) / overall) < -10.0, "{hz} Hz");
    }
}

/// Pulse combined with triangle or sawtooth sounds at about the level of
/// the triangle alone on an synth8580 at width $200: two emulators run as
/// separate programs give +1.2 and +1.5 dB for triangle and pulse and
/// +1.0 and +1.2 dB for sawtooth and pulse (sidplayfp 2.16.0, both its
/// engines, measured 17 November 2026; TODO-6427 fault 1). The pulse
/// comparison was the wrong way round: the chip's waveform output is
/// inverted, so the bits set by the pulse are the share of the period
/// above the width, and combined with another waveform the wrong share
/// all but silenced the voice (-15.6 and -21.6 dB). Real 6581s settle
/// which share it is: the test
/// `the_6581s_combined_waveforms_are_as_quiet_as_two_real_chips`.
#[test]
fn pulse_combined_with_another_waveform_keeps_its_level() {
    const WIDTH: u16 = 0x200;
    const LEAST_DB: f64 = -3.0;
    let level = |control: u8| {
        let mut synth6581 = Synth6581::new(Variant::Synth8580, ChipClock::Pal);
        synth6581.write(VOLUME_REGISTER, FULL_VOLUME);
        synth6581.write(2, (WIDTH & 0xFF) as u8);
        synth6581.write(3, (WIDTH >> 8) as u8);
        start(&mut synth6581, 0, frequency_register(440.0, ChipClock::Pal), control);
        let _ = render(&mut synth6581, 0.1);
        support::rms(&render(&mut synth6581, 0.5))
    };
    let triangle = level(TRIANGLE);
    for (name, control) in [("triangle and pulse", TRIANGLE | PULSE), ("sawtooth and pulse", SAWTOOTH | PULSE)] {
        let relative = decibels(level(control) / triangle);
        assert!(relative > LEAST_DB, "{name}: {relative:.1} dB against the triangle alone");
    }
}

/// The level of a waveform selection on voice 1 against the triangle's,
/// in decibels, at a pulse width.
fn level_against_the_triangle(model: Variant, control: u8, width: u16) -> f64 {
    let level = |control: u8| {
        let mut synth6581 = Synth6581::new(model, ChipClock::Pal);
        synth6581.write(VOLUME_REGISTER, FULL_VOLUME);
        synth6581.write(2, (width & 0xFF) as u8);
        synth6581.write(3, (width >> 8) as u8);
        start(&mut synth6581, 0, frequency_register(240.0, ChipClock::Pal), control);
        let _ = render(&mut synth6581, 0.2);
        let samples = render(&mut synth6581, 0.7);
        let mean = samples.iter().map(|&sample| f64::from(sample)).sum::<f64>() / samples.len() as f64;
        (samples.iter().map(|&sample| (f64::from(sample) - mean).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    };
    decibels(level(control).max(1e-3) / level(TRIANGLE))
}

/// The level of a waveform selection over one sweep of Plogue's one-page
/// BASIC synth6581 benchmark (ploguechipsounds.blogspot.com, May 2010) as its
/// listing plays it on voice 1: attack 0 and decay 8, sustain 12 and
/// release 6, the gate set, then the frequency's upper byte from 0 to 254
/// in steps of 2. A step of the BASIC loop takes 8.6 ms on the machine
/// (128 steps in 1.1 s of the recordings); the level is the root mean
/// square from 35 to 95 % of the sweep, with what lies below 15 Hz taken
/// off, as the recordings were measured.
fn benchmark_level(model: Variant, control: u8, width_high: u8) -> f64 {
    const STEP_SECONDS: f64 = 0.0086;
    const HIGH_PASS_HZ: f64 = 15.0;
    let mut synth6581 = Synth6581::new(model, ChipClock::Pal);
    synth6581.write(VOLUME_REGISTER, FULL_VOLUME);
    synth6581.write(3, width_high);
    synth6581.write(5, 0x08);
    synth6581.write(6, 0xC6);
    synth6581.write(4, control | 0x01);
    let mut samples = Vec::new();
    for high in (0..=254u8).step_by(2) {
        synth6581.write(1, high);
        samples.extend(render(&mut synth6581, STEP_SECONDS));
    }
    // One pole: the level a capacitor has taken up is taken off.
    let share = 2.0 * std::f64::consts::PI * HIGH_PASS_HZ / f64::from(SAMPLE_RATE);
    let mut held = 0.0;
    let passed: Vec<f64> = samples
        .iter()
        .map(|&sample| {
            held += share * (f64::from(sample) - held);
            f64::from(sample) - held
        })
        .collect();
    let part = &passed[samples.len() * 35 / 100..samples.len() * 95 / 100];
    (part.iter().map(|value| value * value).sum::<f64>() / part.len() as f64).sqrt()
}

/// synth6581's combined waveforms are as quiet as two real chips play
/// them: Stone Oakvalley's recordings of his 6581 R2 and R4 playing the
/// benchmark (6581-8580.com, 1 September 2015), measured 17 November 2026
/// as each selection's level over its sweep against the same voice's
/// triangle, on the three voices of each chip. The pulse's width is 0
/// when the benchmark combines it. Each range is the two chips' with 2 dB
/// either side. TODO-6427 fault 3: the reach of the pull before, fitted
/// to one emulator's output, gave -24.4 dB for sawtooth with triangle,
/// -33.8 dB for pulse with sawtooth and -24.4 dB for all three by this
/// measure.
#[test]
fn the_6581s_combined_waveforms_are_as_quiet_as_two_real_chips() {
    const MARGIN_DB: f64 = 2.0;
    let triangle = benchmark_level(Variant::Synth6581, TRIANGLE, 0x08);
    for (control, width_high, quietest, loudest) in [
        (SAWTOOTH | TRIANGLE, 0x08, -31.3, -27.9),
        (PULSE | TRIANGLE, 0x00, -4.1, -3.2),
        (PULSE | SAWTOOTH, 0x00, -17.0, -9.9),
        (PULSE | SAWTOOTH | TRIANGLE, 0x00, -35.8, -33.5),
    ] {
        let found = decibels(benchmark_level(Variant::Synth6581, control, width_high) / triangle);
        assert!(
            (quietest - MARGIN_DB..=loudest + MARGIN_DB).contains(&found),
            "{control:#04x}: {found:.1} dB for {quietest} to {loudest}"
        );
    }
    // At a width of 0 the pulse masks nothing, and the real chips play it
    // with a triangle: the other share of the period would be silence.
    assert!(decibels(benchmark_level(Variant::Synth6581, PULSE | TRIANGLE, 0x00) / triangle) > -6.0);
    // No real chip was measured at other widths. Two emulators run as
    // separate programs (sidplayfp 2.16.0, both engines) give pulse with
    // triangle against a steady triangle -3.3 and -5.7 dB at width $800,
    // -0.7 and -2.9 at $200, -20.0 and -21.4 at $C00: here within 3 dB of
    // that range.
    for (width, quietest, loudest) in [(0x800, -5.7, -3.3), (0x200, -2.9, -0.7), (0xC00, -21.4, -20.0)] {
        let found = level_against_the_triangle(Variant::Synth6581, PULSE | TRIANGLE, width);
        assert!((quietest - 3.0..=loudest + 3.0).contains(&found), "pulse and triangle at {width:#05x}: {found:.1} dB");
    }
    // synth8580's stay the AND: sawtooth with triangle at -2.1 and -2.5 dB
    // in the two emulators.
    let found = level_against_the_triangle(Variant::Synth8580, SAWTOOTH | TRIANGLE, 0x800);
    assert!((found + 2.5).abs() < 1.0, "8580: {found:.1} dB");
}
