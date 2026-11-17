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

/// The 8580's cutoff register for a cutoff in hertz: 30 Hz and 7.25 Hz a
/// step.
fn register_8580(hz: f64) -> u16 {
    ((hz - 30.0) / 7.25).round() as u16
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
        // Open, the filter passes the tone as it came. For the 6581 that
        // is what two real chips do: noise through the low-pass with no
        // resonance lay 0 to 1 dB above the same noise sent past the
        // filter on Stone Oakvalley's R2 and 1 dB below it on his R4
        // (his recordings of Plogue's benchmark, measured 17 November
        // 2026). TODO-6427: the filter's input was multiplied by 0.67
        // before, 3.5 dB down, as one emulator has it.
        let direct = through(model, 2000.0, 0, 0, 0);
        // Closed, the tone is five times the 6581's floor of 400 Hz, two
        // poles down.
        assert!(decibels(levels[0] / direct) < -25.0, "{model:?} closed: {}", decibels(levels[0] / direct));
        assert!(decibels(last / direct).abs() < 1.0, "{model:?} open: {}", decibels(last / direct));
    }
}

/// The first cutoff register, in steps of eight, at which a tone of `hz`
/// through the low-pass is less than 3 dB below the same tone with the
/// filter wide open: the register whose cutoff is `hz`.
fn register_reaching(model: ChipModel, hz: f64) -> u16 {
    let open = through(model, hz, 2047, 0, LOW_PASS);
    (0..=2047u16).step_by(8).find(|&cutoff| decibels(through(model, hz, cutoff, 0, LOW_PASS) / open) > -3.0).unwrap()
}

#[test]
fn the_8580_cutoff_is_a_straight_line_and_the_6581_is_not() {
    let straight = register_reaching(ChipModel::Mos8580, 2000.0);
    assert!(straight.abs_diff(register_8580(2000.0)) <= 16, "8580: {straight}");
    // The 6581 lies at its floor first, and reaches 500 Hz far later than
    // the 8580 does: at register 391 by its curve, against 65.
    let (bent, line) = (register_reaching(ChipModel::Mos6581, 500.0), register_reaching(ChipModel::Mos8580, 500.0));
    assert!(bent > 2 * line && bent.abs_diff(391) <= 16, "6581: {bent} against {line}");
}

#[test]
fn the_6581_cutoff_is_the_middle_chips() {
    // The 6581's cutoff follows the middle one of seven real chips, Stone
    // Oakvalley's 6581 R2, whose low-pass for noise had its corner at
    // 1,348 Hz for register $220 and 2,723 Hz for $2A0 (measured 17
    // November 2026 from his recording of Plogue's benchmark; the table
    // at the curve). TODO-6427 fault 2: the curve fitted to one
    // emulator's output had its cutoff at 2,826 and 4,554 Hz there, and
    // the one before it at 526 and 1,376 Hz.
    let open = |tone: f64| through(ChipModel::Mos6581, tone, 2047, 0, LOW_PASS);
    let at = |register: u16, tone: f64| decibels(through(ChipModel::Mos6581, tone, register, 0, LOW_PASS) / open(tone));
    // The low-pass's corner, 3 dB down, is within an octave of each:
    // above half of it, and below 1.7 times it or the highest tone a
    // voice can play, 3.8 kHz.
    for (register, hz) in [(0x220, 1348.0), (0x2A0, 2723.0)] {
        let (below, above) = (at(register, hz / 2.0), at(register, (hz * 1.7_f64).min(3800.0)));
        assert!(below > -3.0 && above < -3.0, "${register:03X}: {below} dB and {above} dB about {hz} Hz");
    }
}

/// How far a tone at the cutoff is raised by the low-pass at a resonance
/// setting, against the tone sent past the filter.
fn peak(model: ChipModel, resonance: u8, cutoff: u16, hz: f64) -> f64 {
    through(model, hz, cutoff, resonance, LOW_PASS) / through(model, hz, cutoff, resonance, 0)
}

#[test]
fn resonance_raises_the_cutoff_and_the_8580_has_more_of_it() {
    // The cutoff register for 1 kHz: on the 6581, by its curve, 495.
    let at_1_khz = |model: ChipModel, resonance: u8| {
        let cutoff = match model {
            ChipModel::Mos6581 => 495,
            ChipModel::Mos8580 => register_8580(1000.0),
        };
        peak(model, resonance, cutoff, 1000.0)
    };
    for model in MODELS {
        let mut last = 0.0;
        for resonance in 0..16 {
            let level = at_1_khz(model, resonance);
            assert!(level > last, "{model:?} resonance {resonance}: {level} after {last}");
            last = level;
        }
    }
    // No resonance is -3 dB at the cutoff on both chips; the highest is
    // +8 dB on an 8580 (TODO-6427 fault 6: +12 dB at first).
    assert!((decibels(at_1_khz(ChipModel::Mos8580, 0)) + 3.0).abs() < 0.5);
    let found = decibels(at_1_khz(ChipModel::Mos8580, 15));
    assert!((found - 8.0).abs() < 1.0, "{found}");
    assert!((decibels(at_1_khz(ChipModel::Mos6581, 0)) + 3.0).abs() < 1.0);
}

#[test]
fn the_6581s_resonance_is_weak_at_a_low_cutoff_and_grows_with_it() {
    // At its floor of 400 Hz the 6581's highest resonance is a Q of 1.2,
    // +1.6 dB: three real chips playing "$11 Heaven", which holds the
    // highest resonance with the cutoff register between 160 and 256, are
    // followed better the weaker it is (Stone Oakvalley's recordings of
    // his 6581 R2, R3 and R4, measured 17 November 2026; the figures are
    // at the constant). TODO-6427: a Q of 2.4 at every cutoff before,
    // +7.6 dB.
    let floor = decibels(peak(ChipModel::Mos6581, 15, 200, 400.0));
    assert!((floor - 1.6).abs() < 1.0, "at the floor: {floor} dB");
    // It grows with the cutoff, as two emulators have it and no real chip
    // was measured: a Q of 1.37 at register 495, where the cutoff is
    // 1 kHz, and 1.57 at 717, where it is 3.5 kHz, near the highest tone
    // a voice plays.
    let middle = decibels(peak(ChipModel::Mos6581, 15, 495, 1000.0));
    let upper = decibels(peak(ChipModel::Mos6581, 15, 717, 3500.0));
    assert!((middle - 2.7).abs() < 1.0 && (upper - 3.9).abs() < 1.0, "{middle} dB and {upper} dB");
    assert!(floor < middle && middle < upper);
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
fn the_volume_register_plays_samples_on_a_6581_and_quietly_on_an_8580() {
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
    // The 6581's three silent voices and its mixer add (3 * 0.26 + 0.08)
    // / 1.05 of a voice's full swing, 4,095 * 255 units, which the
    // volume's fifteen steps and the 6581's output gain, 34 / 65,536,
    // bring to samples; a square wave of that has 2/π of it at its
    // fundamental.
    let expected = (3.0 * 0.26 + 0.08) / 1.05 * 4095.0 * 255.0 * 15.0 * 34.0 / 65536.0 * 2.0 / std::f64::consts::PI;
    assert!((old / expected - 1.0).abs() < 0.05, "6581: {old} for {expected}");
    // The 8580's add 3 * 0.05 of it, at an output gain of 26 / 65,536.
    let expected = 0.15 * 4095.0 * 255.0 * 15.0 * 26.0 / 65536.0 * 2.0 / std::f64::consts::PI;
    assert!((new / expected - 1.0).abs() < 0.05, "8580: {new} for {expected}");
}

/// The largest change over eight samples of what `sid` plays next: the
/// height of a step.
fn step(sid: &mut Sid) -> f64 {
    let samples = render(sid, 0.01);
    samples.windows(9).map(|window| f64::from(i32::from(window[8]) - i32::from(window[0])).abs()).fold(0.0, f64::max)
}

#[test]
fn the_volume_registers_step_is_as_loud_as_real_chips_play_samples() {
    // The step heard when only the volume changes, as a share of the span
    // of one triangle at full volume. Four real 6581s have 0.57, 0.69,
    // 0.98 and 1.53 (the chip of "The C64 Digi", and Stone Oakvalley's
    // R4, R2 and R3 from his recordings of "Arkanoid"), and the middle is
    // 0.82; his 8580 R5 has 0.15 (measured 17 November 2026; the figures
    // are at the constants). With the voices out of the mixer, what is
    // left is the mixer's own: 0.08 of the span on the 6581, nothing on
    // the 8580. TODO-6427 fault 4: 0.73 and 0.05 at first, then 1.57 and
    // 0.29 when fitted to one emulator.
    const NEAR: f64 = 0.1;
    for (model, silent, mixer) in [(ChipModel::Mos6581, 0.82, 0.076), (ChipModel::Mos8580, 0.15, 0.0)] {
        let clock = ChipClock::Pal;
        let mut sid = Sid::new(model, clock);
        sid.write(MODE_VOLUME, FULL_VOLUME);
        start(&mut sid, 0, frequency_register(440.0, clock), TRIANGLE);
        let _ = render(&mut sid, 0.5);
        let triangle = render(&mut sid, 0.5);
        let span = f64::from(i32::from(*triangle.iter().max().unwrap()) - i32::from(*triangle.iter().min().unwrap()));

        let mut sid = Sid::new(model, clock);
        let _ = render(&mut sid, 0.5);
        sid.write(MODE_VOLUME, FULL_VOLUME);
        let found = step(&mut sid) / span;
        assert!((found - silent).abs() < NEAR * silent, "{model:?}: a step of {found} for {silent}");

        // The three voices sent to the filter, and no output of it heard.
        let mut sid = Sid::new(model, clock);
        sid.write(RESONANCE_ROUTING, 0x07);
        let _ = render(&mut sid, 0.5);
        sid.write(MODE_VOLUME, FULL_VOLUME);
        let found = step(&mut sid) / span;
        assert!((found - mixer).abs() < NEAR * silent, "{model:?}: the mixer's step of {found} for {mixer}");
    }
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
