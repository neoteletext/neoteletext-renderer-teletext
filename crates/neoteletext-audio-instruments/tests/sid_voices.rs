//! The SID's voices, heard at the output: pitch, the shape of each
//! waveform, synchronisation and ring modulation, the envelope's times and
//! the noise's rate (REQ-6412 item 4).

// Without the SID's feature there is nothing here to test.
#![cfg(feature = "sid")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test"
)]

#[path = "sid_support.rs"]
mod support;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, ENVELOPE_3, OSCILLATOR_3, Sid};
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
fn playing(model: ChipModel, clock: ChipClock, hz: f64, control: u8) -> Sid {
    let mut sid = Sid::new(model, clock);
    sid.write(VOLUME_REGISTER, FULL_VOLUME);
    start(&mut sid, 0, frequency_register(hz, clock), control);
    sid
}

/// A second of the chip's output after it has settled.
fn settled(sid: &mut Sid) -> Vec<i16> {
    let _ = render(sid, 0.5);
    render(sid, 1.0)
}

#[test]
fn concert_a_is_440_hz_on_both_clocks() {
    for model in MODELS {
        for clock in CLOCKS {
            let mut sid = playing(model, clock, 440.0, TRIANGLE);
            let _ = render(&mut sid, 0.5);
            let measured = zero_crossing_pitch(&render(&mut sid, 2.0));
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
        let mut sid = playing(model, ChipClock::Pal, 220.0, SAWTOOTH);
        let found = harmonics(&settled(&mut sid), 220.0, 8);
        for (index, &level) in found.iter().enumerate() {
            let expected = 1.0 / (index + 1) as f64;
            assert!((level / expected - 1.0).abs() < 0.03, "{model:?} harmonic {}: {level}", index + 1);
        }
    }
}

#[test]
fn the_triangle_has_odd_harmonics_at_one_over_their_square() {
    for model in MODELS {
        let mut sid = playing(model, ChipClock::Pal, 220.0, TRIANGLE);
        let found = harmonics(&settled(&mut sid), 220.0, 7);
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
        let mut sid = playing(ChipModel::Mos8580, ChipClock::Pal, 220.0, PULSE);
        sid.write(2, width as u8);
        sid.write(3, (width >> 8) as u8);
        let found = harmonics(&settled(&mut sid), 220.0, 8);
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
    let mut sid = playing(ChipModel::Mos8580, ChipClock::Pal, 220.0, PULSE);
    let samples = settled(&mut sid);
    assert!(samples.iter().all(|&sample| sample.abs() <= 1), "{:?}", &samples[..8]);
}

#[test]
fn sync_gives_voice_1_the_period_of_voice_3() {
    let clock = ChipClock::Pal;
    let (slave, master) = (700.0, 300.0);
    let mut free = playing(ChipModel::Mos8580, clock, slave, SAWTOOTH);
    set_frequency(&mut free, 2, frequency_register(master, clock));
    let mut synced = playing(ChipModel::Mos8580, clock, slave, SAWTOOTH | SYNC);
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
    let mut plain = playing(ChipModel::Mos8580, clock, carrier, TRIANGLE);
    set_frequency(&mut plain, 2, frequency_register(modulator, clock));
    let mut ring = playing(ChipModel::Mos8580, clock, carrier, TRIANGLE | RING);
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
fn samples_until(sid: &mut Sid, level: u8, limit: usize) -> usize {
    let mut sample = [0i16; 1];
    for count in 0..limit {
        if sid.read(ENVELOPE_3) == level {
            return count;
        }
        sid.render(&mut sample);
    }
    panic!("the envelope never read {level}");
}

/// Samples until register `ENVELOPE_3` first reads above zero. The
/// fastest attack takes more than one step in a sample, so no one level
/// can be waited for.
fn samples_until_sounding(sid: &mut Sid, limit: usize) -> usize {
    let mut sample = [0i16; 1];
    for count in 0..limit {
        if sid.read(ENVELOPE_3) != 0 {
            return count;
        }
        sid.render(&mut sample);
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
            let mut sid = Sid::new(ChipModel::Mos6581, clock);
            let third = support::voice(2);
            sid.write(third + 5, (rate << 4) | rate);
            sid.write(third + 6, rate);
            sid.write(third + 4, 0x01);
            let rise = samples_until(&mut sid, 255, 100_000) as f64;
            // The sustain level is zero: the decay runs to silence.
            let decay = samples_until(&mut sid, 0, 200_000) as f64;
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
            let mut sid = Sid::new(ChipModel::Mos6581, clock);
            sid.write(third + 6, 0xF0 | rate);
            sid.write(third + 4, 0x01);
            samples_until(&mut sid, 255, 1_000);
            sid.write(third + 4, 0x00);
            let release = samples_until(&mut sid, 0, 200_000) as f64;
            assert!((release - decay).abs() <= 2.0, "{clock:?} rate {rate}: {release} against {decay}");
        }
    }
}

#[test]
fn the_envelope_holds_each_sustain_level() {
    for nibble in 0u8..16 {
        let mut sid = Sid::new(ChipModel::Mos8580, ChipClock::Pal);
        let third = support::voice(2);
        sid.write(third + 6, nibble << 4);
        sid.write(third + 4, 0x01);
        let _ = render(&mut sid, 0.1);
        assert_eq!(sid.read(ENVELOPE_3), nibble * 17);
        // And the level heard follows it: a sawtooth at that envelope.
        sid.write(VOLUME_REGISTER, FULL_VOLUME);
        set_frequency(&mut sid, 2, frequency_register(440.0, ChipClock::Pal));
        sid.write(third + 4, SAWTOOTH | 0x01);
        let level = amplitude(&settled(&mut sid), 440.0);
        // A sawtooth of 4,095 * 255 units is 2/π of half that at its
        // fundamental, and 1,024 units are one of a sample at full volume.
        let full = 4095.0 * 255.0 / std::f64::consts::PI * 15.0 / 1024.0;
        let expected = full * f64::from(nibble) / 15.0;
        assert!((level - expected).abs() < full * 0.01, "sustain {nibble}: {level} for {expected}");
    }
}

#[test]
fn a_faster_rate_written_late_waits_for_the_divider_to_come_round() {
    let third = support::voice(2);
    // Gate at the slowest attack, and let the rate divider run 49 samples,
    // about a thousand cycles, towards its 31,251.
    let mut sid = Sid::new(ChipModel::Mos6581, ChipClock::Pal);
    sid.write(third + 5, 0xF0);
    sid.write(third + 6, 0xF0);
    sid.write(third + 4, 0x01);
    let mut samples = [0i16; 49];
    sid.render(&mut samples);
    assert_eq!(sid.read(ENVELOPE_3), 0);
    let passed = sid.cycles();
    // The fastest attack's 9 cycles are now behind the divider: the first
    // step waits for the divider's 32,767 states to come round.
    sid.write(third + 5, 0x00);
    let waited = samples_until_sounding(&mut sid, 2_000) as f64;
    let expected = (32_767.0 - passed as f64 + 9.0) * f64::from(SAMPLE_RATE) / f64::from(ChipClock::Pal.hz());
    assert!((waited - expected).abs() <= 1.0, "{waited} samples for {expected}");
    // Once round, the attack runs at its rate: 254 steps of 9 cycles, less
    // the two that fit in the sample that saw the first.
    let rise = samples_until(&mut sid, 255, 1_000) as f64;
    assert!((rise - 254.0 * 9.0 * 48_000.0 / 985_248.0).abs() <= 2.0, "{rise}");

    // Written before the gate, the same attack starts within a sample.
    let mut sid = Sid::new(ChipModel::Mos6581, ChipClock::Pal);
    sid.write(third + 6, 0xF0);
    sid.write(third + 4, 0x01);
    assert!(samples_until_sounding(&mut sid, 10) <= 1);
}

#[test]
fn noise_changes_at_the_oscillators_rate() {
    for clock in CLOCKS {
        let mut sid = Sid::new(ChipModel::Mos6581, clock);
        let third = support::voice(2);
        // Frequency $0400: bit 19 of the accumulator rises every 1,024
        // cycles, some 50 samples.
        set_frequency(&mut sid, 2, 0x0400);
        sid.write(third + 4, NOISE);
        let mut sample = [0i16; 1];
        let mut last = sid.read(OSCILLATOR_3);
        let mut changes = 0u32;
        for _ in 0..4 * SAMPLE_RATE {
            sid.render(&mut sample);
            let value = sid.read(OSCILLATOR_3);
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
    let mut sid = playing(ChipModel::Mos8580, ChipClock::Pal, 3000.0, NOISE);
    let samples = settled(&mut sid);
    let overall = support::rms(&samples);
    assert!(overall > 1000.0, "rms {overall}");
    // No component stands out as a tone would.
    for hz in [250.0, 1000.0, 3000.0, 6000.0] {
        assert!(decibels(amplitude(&samples, hz) / overall) < -10.0, "{hz} Hz");
    }
}
