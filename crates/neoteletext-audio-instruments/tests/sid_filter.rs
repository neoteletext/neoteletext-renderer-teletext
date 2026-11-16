//! The SID's analogue half, heard at the output: the filter's three
//! outputs, its cutoff and resonance on both chips, the 6581's offsets and
//! distortion, the board's output stage, and stability under any register
//! contents (REQ-6412 item 4).

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

use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, REGISTERS, Sid};
use support::{CLOCKS, MODELS, Random, amplitude, decibels, frequency_register, pitch, render, rms, start};

const TRIANGLE: u8 = 0x10;
const SAWTOOTH: u8 = 0x20;
const CUTOFF_LOW: u8 = 21;
const CUTOFF_HIGH: u8 = 22;
const RESONANCE_ROUTING: u8 = 23;
const MODE_VOLUME: u8 = 24;
const LOW_PASS: u8 = 0x10;
const BAND_PASS: u8 = 0x20;
const HIGH_PASS: u8 = 0x40;
const FULL_VOLUME: u8 = 0x0F;

/// The level, at its own pitch, of a quiet triangle on voice 1 sent
/// through the filter. Quiet, so that the 6581's distortion stays out of
/// it: sustain level 4 of 15.
fn through(model: ChipModel, hz: f64, cutoff: u16, resonance: u8, mode: u8) -> f64 {
    let clock = ChipClock::Pal;
    let mut sid = Sid::new(model, clock);
    sid.write(CUTOFF_LOW, (cutoff & 7) as u8);
    sid.write(CUTOFF_HIGH, (cutoff >> 3) as u8);
    sid.write(RESONANCE_ROUTING, (resonance << 4) | u8::from(mode & 0x70 != 0));
    sid.write(MODE_VOLUME, mode | FULL_VOLUME);
    let register = frequency_register(hz, clock);
    start(&mut sid, 0, register, TRIANGLE);
    sid.write(6, 0x40);
    let _ = render(&mut sid, 0.3);
    amplitude(&render(&mut sid, 0.5), pitch(register, clock))
}

/// The 8580's cutoff register for a cutoff in hertz: 30 Hz and 12,500 Hz
/// over 2,048 steps.
fn register_8580(hz: f64) -> u16 {
    ((hz - 30.0) * 2048.0 / 12_500.0).round() as u16
}

#[test]
fn the_low_pass_passes_a_low_tone_and_cuts_a_high_one() {
    let cutoff = register_8580(1000.0);
    for (hz, expected) in [(100.0, 0.0), (250.0, 0.0), (2000.0, -12.3), (3900.0, -23.7)] {
        // A filter of 12 dB an octave with no resonance: 1 / sqrt(1 + (f /
        // fc)^4).
        let direct = through(ChipModel::Mos8580, hz, cutoff, 0, 0);
        let found = decibels(through(ChipModel::Mos8580, hz, cutoff, 0, LOW_PASS) / direct);
        assert!((found - expected).abs() < 1.0, "{hz} Hz: {found} dB for {expected}");
    }
}

#[test]
fn the_high_pass_cuts_a_low_tone_and_passes_a_high_one() {
    let cutoff = register_8580(1000.0);
    for (hz, expected) in [(250.0, -24.1), (500.0, -12.3), (3900.0, 0.0)] {
        let direct = through(ChipModel::Mos8580, hz, cutoff, 0, 0);
        let found = decibels(through(ChipModel::Mos8580, hz, cutoff, 0, HIGH_PASS) / direct);
        assert!((found - expected).abs() < 1.0, "{hz} Hz: {found} dB for {expected}");
    }
}

#[test]
fn the_band_pass_is_loudest_at_the_cutoff() {
    let cutoff = register_8580(1000.0);
    let at =
        |hz: f64| through(ChipModel::Mos8580, hz, cutoff, 8, BAND_PASS) / through(ChipModel::Mos8580, hz, cutoff, 8, 0);
    let (below, centre, above) = (at(250.0), at(1000.0), at(3900.0));
    assert!(centre > 2.0 * below && centre > 2.0 * above, "{below} {centre} {above}");
}

#[test]
fn low_and_high_pass_together_are_a_notch() {
    let cutoff = register_8580(1000.0);
    let at = |hz: f64| {
        through(ChipModel::Mos8580, hz, cutoff, 0, LOW_PASS | HIGH_PASS) / through(ChipModel::Mos8580, hz, cutoff, 0, 0)
    };
    assert!(decibels(at(1000.0)) < -20.0, "{} dB at the cutoff", decibels(at(1000.0)));
    assert!(decibels(at(100.0)).abs() < 1.0 && decibels(at(3900.0)).abs() < 2.0);
}

#[test]
fn the_cutoff_register_opens_the_low_pass_step_by_step() {
    for model in MODELS {
        let mut last = 0.0;
        let mut levels = Vec::new();
        for cutoff in (0..=2047u16).step_by(93) {
            let level = through(model, 2000.0, cutoff, 0, LOW_PASS);
            // A tenth of a decibel for the measurement.
            assert!(level >= last * 0.99, "{model:?} at {cutoff}: {level} after {last}");
            last = level;
            levels.push(level);
        }
        let direct = through(model, 2000.0, 0, 0, 0);
        assert!(decibels(levels[0] / direct) < -30.0, "{model:?} closed: {}", decibels(levels[0] / direct));
        assert!(decibels(last / direct) > -1.0, "{model:?} open: {}", decibels(last / direct));
    }
}

#[test]
fn the_8580_cutoff_is_a_straight_line_and_the_6581_is_not() {
    // The register at which a 2 kHz tone through the low-pass is 3 dB down
    // is the register whose cutoff is 2 kHz.
    let crossing = |model: ChipModel| {
        let direct = through(model, 2000.0, 0, 0, 0);
        (0..=2047u16)
            .step_by(8)
            .find(|&cutoff| decibels(through(model, 2000.0, cutoff, 0, LOW_PASS) / direct) > -3.0)
            .unwrap()
    };
    let straight = crossing(ChipModel::Mos8580);
    assert!(straight.abs_diff(register_8580(2000.0)) <= 16, "8580: {straight}");
    // The 6581 here reaches 2 kHz far later: its curve lies flat first.
    let bent = crossing(ChipModel::Mos6581);
    assert!(bent > 2 * straight && bent < 1024, "6581: {bent}");
}

#[test]
fn resonance_raises_the_cutoff_and_the_8580_has_more_of_it() {
    let peak = |model: ChipModel, resonance: u8| {
        // The 6581's cutoff register for 1 kHz, by the test above's
        // method, is found here by its level instead: use the band-pass,
        // which peaks at the cutoff whatever it is, and search near it.
        let cutoff = match model {
            ChipModel::Mos6581 => 624,
            ChipModel::Mos8580 => register_8580(1000.0),
        };
        through(model, 1000.0, cutoff, resonance, LOW_PASS) / through(model, 1000.0, cutoff, resonance, 0)
    };
    for model in MODELS {
        let mut last = 0.0;
        for resonance in 0..16 {
            let level = peak(model, resonance);
            assert!(level > last, "{model:?} resonance {resonance}: {level} after {last}");
            last = level;
        }
    }
    // No resonance is -3 dB at the cutoff; the highest is +6 dB on a 6581
    // and +12 dB on an 8580.
    assert!((decibels(peak(ChipModel::Mos8580, 0)) + 3.0).abs() < 0.5);
    assert!((decibels(peak(ChipModel::Mos8580, 15)) - 12.0).abs() < 1.0, "{}", decibels(peak(ChipModel::Mos8580, 15)));
    assert!((decibels(peak(ChipModel::Mos6581, 15)) - 6.0).abs() < 1.5, "{}", decibels(peak(ChipModel::Mos6581, 15)));
}

#[test]
fn a_voice_reaches_the_output_by_one_path_only() {
    // With no filter output selected, a voice sent to the filter is not
    // heard; nor is voice 3 when it is cut from the direct path.
    for model in MODELS {
        let heard = |routing: u8, mode: u8, voice: u8| {
            let mut sid = Sid::new(model, ChipClock::Pal);
            sid.write(RESONANCE_ROUTING, routing);
            sid.write(MODE_VOLUME, mode | FULL_VOLUME);
            sid.write(CUTOFF_HIGH, 0xFF);
            start(&mut sid, voice, frequency_register(440.0, ChipClock::Pal), TRIANGLE);
            let _ = render(&mut sid, 0.3);
            amplitude(&render(&mut sid, 0.3), 440.0)
        };
        let direct = heard(0, 0, 0);
        assert!(direct > 1000.0);
        assert!(heard(0x01, 0, 0) < direct / 1000.0, "{model:?}: filtered and not selected");
        assert!(heard(0, 0x80, 2) < direct / 1000.0, "{model:?}: voice 3 off");
        assert!(heard(0, 0x80, 1) > direct * 0.99, "{model:?}: voice 2 is not voice 3");
        assert!(heard(0x04, 0x80 | LOW_PASS, 2) > direct * 0.5, "{model:?}: voice 3 through the filter");
    }
}

#[test]
fn the_volume_register_plays_samples_on_a_6581_and_barely_on_an_8580() {
    let played = |model: ChipModel| {
        let mut sid = Sid::new(model, ChipClock::Pal);
        let mut samples = Vec::new();
        // A 1 kHz square wave written to the volume alone, no voice on.
        for half in 0..2000 {
            sid.write(MODE_VOLUME, if half % 2 == 0 { 0x0F } else { 0x00 });
            let mut block = [0i16; 24];
            sid.render(&mut block);
            samples.extend_from_slice(&block);
        }
        amplitude(&samples[4800..], 1000.0)
    };
    let (old, new) = (played(ChipModel::Mos6581), played(ChipModel::Mos8580));
    // "The C64 Digi": the 6581's output rises 0.72 V from volume 0 to 15,
    // against 1.05 V for a voice's full swing: here 0.72 / 1.05 of 4,095 *
    // 255 * 15 / 1,024 samples, and a square wave of that has 2/π of it at
    // its fundamental. The offsets used are 3 * 0.26 - 0.04 = 0.74 V.
    let expected = 0.74 / 1.05 * 4095.0 * 255.0 * 15.0 / 1024.0 * 2.0 / std::f64::consts::PI;
    assert!((old / expected - 1.0).abs() < 0.05, "6581: {old} for {expected}");
    assert!(new < old / 10.0 && new > old / 100.0, "8580: {new} against {old}");
}

#[test]
fn the_6581_filter_distorts_a_loud_input_and_the_8580_does_not() {
    let second_harmonic = |model: ChipModel| {
        let clock = ChipClock::Pal;
        let mut sid = Sid::new(model, clock);
        sid.write(CUTOFF_HIGH, 0xFF);
        sid.write(RESONANCE_ROUTING, 0x07);
        sid.write(MODE_VOLUME, LOW_PASS | FULL_VOLUME);
        // Three triangles in unison: three times one voice's level.
        let register = frequency_register(220.0, clock);
        for voice in 0..3 {
            start(&mut sid, voice, register, TRIANGLE);
        }
        let _ = render(&mut sid, 0.3);
        let samples = render(&mut sid, 1.0);
        let hz = pitch(register, clock);
        decibels(amplitude(&samples, 2.0 * hz) / amplitude(&samples, hz))
    };
    // A triangle has no even harmonics; the 6581's saturating input, off
    // centre by the voices' offsets, makes some.
    assert!(second_harmonic(ChipModel::Mos8580) < -60.0, "8580: {} dB", second_harmonic(ChipModel::Mos8580));
    assert!(second_harmonic(ChipModel::Mos6581) > -40.0, "6581: {} dB", second_harmonic(ChipModel::Mos6581));
}

#[test]
fn the_output_stage_is_the_boards_low_pass() {
    // R9 = 10 kΩ into C74 = 1 nF: -3 dB at 1 / (2π * 10 µs) = 15.9 kHz. A
    // sawtooth at a fifth of that has its fifth harmonic there, a fifth of
    // its fundamental before the board.
    let corner = 1.0 / (2.0 * std::f64::consts::PI * 10e3 * 1e-9);
    for clock in CLOCKS {
        let mut sid = Sid::new(ChipModel::Mos8580, clock);
        sid.write(MODE_VOLUME, FULL_VOLUME);
        let register = frequency_register(corner / 5.0, clock);
        start(&mut sid, 0, register, SAWTOOTH);
        let _ = render(&mut sid, 0.3);
        let samples = render(&mut sid, 1.0);
        let hz = pitch(register, clock);
        let response = |f: f64| 1.0 / (1.0 + (f / corner) * (f / corner)).sqrt();
        let expected = decibels(response(5.0 * hz) / response(hz) / 5.0);
        let found = decibels(amplitude(&samples, 5.0 * hz) / amplitude(&samples, hz));
        // The decimator's first stage takes 0.2 dB more at 16 kHz.
        assert!((found - expected).abs() < 0.5, "{clock:?}: {found} dB for {expected}");
    }
}

#[test]
fn the_output_carries_no_dc() {
    for model in MODELS {
        let mut sid = Sid::new(model, ChipClock::Pal);
        sid.write(MODE_VOLUME, FULL_VOLUME);
        start(&mut sid, 0, 0, 0x40);
        let _ = render(&mut sid, 1.0);
        let samples = render(&mut sid, 0.1);
        assert!(samples.iter().all(|&sample| sample.abs() <= 1), "{model:?}: {:?}", &samples[..4]);
    }
}

#[test]
fn random_register_images_never_overflow_and_never_run_away() {
    // Debug builds check every arithmetic operation that is not explicitly
    // wrapping, so an overflow anywhere in the chip fails this test.
    for model in MODELS {
        for clock in CLOCKS {
            let mut random = Random(0x5EED_6581);
            let mut sid = Sid::new(model, clock);
            let mut block = [0i16; 64];
            let mut loud = 0usize;
            for _ in 0..1_000 {
                let mut image = [0u8; REGISTERS];
                for register in &mut image {
                    *register = random.byte();
                }
                sid.write_image(&image);
                sid.render(&mut block);
                loud += block.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
            }
            // Three voices at full level with resonance may touch the
            // rails; a filter that had run away would sit on them.
            assert!(loud < 64 * 1_000 / 20, "{model:?} {clock:?}: {loud} samples at the rails");
            // Silenced, the chip falls silent: nothing was left ringing.
            sid.write_image(&[0; REGISTERS]);
            let _ = render(&mut sid, 1.0);
            assert!(rms(&render(&mut sid, 0.1)) < 2.0, "{model:?} {clock:?}");
        }
    }
}
