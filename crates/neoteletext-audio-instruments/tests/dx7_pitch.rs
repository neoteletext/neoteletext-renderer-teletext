//! The DX7's pitch at 48,000 samples a second, measured from the rendered
//! samples (REQ-6412 item 6).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::dx7::{Bank, Patch, Rack, VoiceParams};

/// A rack whose part 0 plays the default sine, changed by `change`.
fn rack_with(change: impl FnOnce(&mut VoiceParams)) -> Rack {
    let mut params = *Patch::DEFAULT.params();
    change(&mut params);
    let mut bank = Bank::new();
    bank.set(0, Patch::new(params).unwrap());
    let mut rack = Rack::new();
    rack.set_bank(&bank);
    rack
}

/// The frequency of what `rack` renders over two seconds, in hertz: the
/// number of rising zero crossings less one, over the time between the
/// first and the last, each placed between two samples by a straight line.
/// The first tenth of a second is left out.
fn frequency(rack: &mut Rack) -> f64 {
    let mut samples = vec![0_i16; 96_000];
    rack.render(&mut samples);
    let mut crossings = Vec::new();
    for (index, pair) in samples.windows(2).enumerate().skip(4800) {
        let (before, after) = (f64::from(pair[0]), f64::from(pair[1]));
        if before < 0.0 && after >= 0.0 {
            crossings.push(index as f64 + before / (before - after));
        }
    }
    assert!(crossings.len() > 10, "{} crossings", crossings.len());
    (crossings.len() - 1) as f64 / (crossings[crossings.len() - 1] - crossings[0]) * 48_000.0
}

fn cents(frequency: f64, expected: f64) -> f64 {
    1200.0 * (frequency / expected).log2()
}

/// The frequency of a key of part 0.
fn key_frequency(rack: &mut Rack, key: u8) -> f64 {
    rack.event(0x90, key, 100);
    frequency(rack)
}

fn assert_cents(measured: f64, expected: f64, tolerance: f64) {
    let error = cents(measured, expected);
    assert!(error.abs() < tolerance, "{measured} Hz against {expected} Hz: {error} cents");
}

/// Note 69 at a ratio of 1.00 with no detune is 440 Hz within a tenth of a
/// cent.
#[test]
fn a4_is_440_hertz() {
    let measured = key_frequency(&mut rack_with(|_| ()), 69);
    println!("A4: {measured} Hz, {} cents from 440", cents(measured, 440.0));
    assert_cents(measured, 440.0, 0.1);
}

#[test]
fn every_octave_of_a_is_in_tune() {
    for (key, expected) in [(21, 27.5), (33, 55.0), (45, 110.0), (57, 220.0), (81, 880.0), (93, 1760.0), (105, 3520.0)]
    {
        assert_cents(key_frequency(&mut rack_with(|_| ()), key), expected, 0.1);
    }
}

#[test]
fn keys_are_equal_tempered() {
    for key in [36_u8, 60, 61, 66, 72, 100] {
        let expected = 440.0 * 2.0_f64.powf((f64::from(key) - 69.0) / 12.0);
        assert_cents(key_frequency(&mut rack_with(|_| ()), key), expected, 0.1);
    }
}

/// Transpose moves pitch by semitones, 24 being none.
#[test]
fn transpose_moves_pitch_by_semitones() {
    for (transpose, expected) in [(0_u8, 110.0), (12, 220.0), (24, 440.0), (36, 880.0), (48, 1760.0)] {
        let mut rack = rack_with(|voice| voice.transpose = transpose);
        assert_cents(key_frequency(&mut rack, 69), expected, 0.1);
    }
    let mut rack = rack_with(|voice| voice.transpose = 31);
    assert_cents(key_frequency(&mut rack, 69), 440.0 * 2.0_f64.powf(7.0 / 12.0), 0.1);
}

/// A coarse frequency of 0 is a ratio of a half, and of `n` a ratio of `n`;
/// fine adds hundredths of the coarse ratio.
#[test]
fn coarse_and_fine_set_the_ratio() {
    for (coarse, fine, ratio) in [
        (0_u8, 0_u8, 0.5),
        (1, 0, 1.0),
        (2, 0, 2.0),
        (3, 0, 3.0),
        (7, 0, 7.0),
        (31, 0, 31.0),
        (1, 50, 1.5),
        (2, 99, 3.98),
    ] {
        let mut rack = rack_with(|voice| {
            voice.operators[0].frequency_coarse = coarse;
            voice.operators[0].frequency_fine = fine;
        });
        assert_cents(key_frequency(&mut rack, 57), 220.0 * ratio, 0.1);
    }
}

/// In fixed mode the frequency is `10^(coarse mod 4 + fine/100)` Hz
/// whatever the key (`dx7note.cc`, `osc_freq`).
#[test]
fn a_fixed_frequency_ignores_the_key() {
    for (coarse, fine, expected) in
        [(1_u8, 0_u8, 10.0), (2, 0, 100.0), (3, 0, 1000.0), (2, 50, 316.227_766), (7, 0, 1000.0)]
    {
        for key in [40_u8, 90] {
            let mut rack = rack_with(|voice| {
                voice.operators[0].oscillator_mode = 1;
                voice.operators[0].frequency_coarse = coarse;
                voice.operators[0].frequency_fine = fine;
            });
            assert_cents(key_frequency(&mut rack, key), expected, 0.1);
        }
    }
}

/// Detune moves a ratio oscillator by 12,606 parts in 2^24 of an octave
/// each step either side of 7 (`dx7note.cc`, `osc_freq`): 0.9 cents.
#[test]
fn detune_moves_pitch_a_little_each_way() {
    for detune in [0_u8, 6, 8, 14] {
        let mut rack = rack_with(|voice| voice.operators[0].detune = detune);
        let octaves = 12_606.0 * (f64::from(detune) - 7.0) / 16_777_216.0;
        assert_cents(key_frequency(&mut rack, 69), 440.0 * 2.0_f64.powf(octaves), 0.1);
    }
}

/// The pitch bend wheel bends two semitones each way.
#[test]
fn pitch_bend_is_two_semitones() {
    for (low, high, semitones) in
        [(0_u8, 0_u8, -2.0), (0, 32, -1.0), (0, 64, 0.0), (0, 96, 1.0), (127, 127, 2.0 * 8191.0 / 8192.0)]
    {
        let mut rack = rack_with(|_| ());
        rack.event(0xe0, low, high);
        assert_cents(key_frequency(&mut rack, 69), 440.0 * 2.0_f64.powf(semitones / 12.0), 0.1);
    }
}

/// The pitch envelope's level 50 is no change, and its levels are the
/// table's steps of 1/32 of an octave, held at its third level while the
/// key is down.
#[test]
fn the_pitch_envelope_holds_its_third_level() {
    for (level, steps) in [(50_u8, 0.0), (66, 16.0), (34, -16.0), (99, 127.0), (0, -128.0)] {
        let mut rack = rack_with(|voice| {
            voice.pitch_eg_levels = [level; 4];
        });
        assert_cents(key_frequency(&mut rack, 57), 220.0 * 2.0_f64.powf(steps / 32.0), 0.1);
    }
}

/// The modulation wheel adds vibrato: the pitch swings about the note, by
/// more with more wheel, and not at all with a pitch modulation
/// sensitivity of 0.
#[test]
fn the_modulation_wheel_adds_vibrato() {
    let spread = |sensitivity: u8, wheel: u8| {
        let mut rack = rack_with(|voice| {
            voice.pitch_mod_sensitivity = sensitivity;
            voice.lfo_speed = 30;
        });
        rack.event(0xb0, 1, wheel);
        rack.event(0x90, 69, 100);
        let mut samples = vec![0_i16; 96_000];
        rack.render(&mut samples);
        // The longest and shortest periods between rising zero crossings.
        let mut last = None;
        let (mut shortest, mut longest) = (f64::MAX, 0.0_f64);
        for (index, pair) in samples.windows(2).enumerate().skip(4800) {
            let (before, after) = (f64::from(pair[0]), f64::from(pair[1]));
            if before < 0.0 && after >= 0.0 {
                let at = index as f64 + before / (before - after);
                if let Some(last) = last {
                    shortest = shortest.min(at - last);
                    longest = longest.max(at - last);
                }
                last = Some(at);
            }
        }
        1200.0 * (longest / shortest).log2()
    };
    assert!(spread(3, 0) < 1.0, "no wheel: {} cents", spread(3, 0));
    assert!(spread(0, 127) < 1.0, "no sensitivity: {} cents", spread(0, 127));
    let (half, full) = (spread(3, 64), spread(3, 127));
    println!("vibrato at sensitivity 3: {half} cents at half wheel, {full} at full");
    assert!(half > 10.0 && full > 1.8 * half, "{half} then {full} cents");
}
