//! Plays about ten seconds of music on the DX7 rack and writes it as a
//! 48 kHz, 16-bit, mono WAV file, for listening:
//!
//! ```text
//! cargo run --release -p neoteletext-audio-instruments --example dx7_demo -- out.wav
//! ```
//!
//! Three voices made here, in code: an electric piano on part 0 (chords),
//! a bass with feedback on part 1 and a bell on part 2 (the melody, with a
//! pitch bend and vibrato from the modulation wheel). It prints the peak
//! and the RMS level of what it wrote, and fails if that is silent or
//! reaches full scale.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::print_stdout,
    reason = "an example run by hand, which may panic on a bad argument and prints its result"
)]

use std::io::Write as _;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::dx7::{Bank, OperatorParams, Patch, Rack, VoiceParams};

/// An operator at a ratio and an output level, with an envelope.
fn operator(coarse: u8, fine: u8, level: u8, rates: [u8; 4], levels: [u8; 4]) -> OperatorParams {
    OperatorParams {
        eg_rates: rates,
        eg_levels: levels,
        output_level: level,
        frequency_coarse: coarse,
        frequency_fine: fine,
        ..Patch::DEFAULT.params().operators[1]
    }
}

/// A voice from six operators (operator 1 first).
fn voice(name: &[u8; 10], algorithm: u8, feedback: u8, operators: [OperatorParams; 6]) -> VoiceParams {
    VoiceParams { operators, algorithm: algorithm - 1, feedback, name: *name, ..*Patch::DEFAULT.params() }
}

/// An electric piano: algorithm 5, three pairs of a carrier and its
/// modulator. Pair 1 and 2 is the body, pair 3 and 4 a second body a few
/// cents away, and pair 5 and 6 the tine: a high modulator that dies fast.
fn electric_piano() -> Patch {
    let body = [95, 30, 22, 55];
    let mut params = voice(
        b"EL PIANO  ",
        5,
        3,
        [
            operator(1, 0, 99, body, [99, 92, 0, 0]),
            operator(1, 0, 72, [95, 45, 25, 55], [99, 80, 0, 0]),
            operator(1, 0, 97, body, [99, 92, 0, 0]),
            operator(1, 0, 68, [95, 45, 25, 55], [99, 80, 0, 0]),
            operator(1, 0, 90, [96, 40, 30, 60], [99, 60, 0, 0]),
            operator(14, 0, 62, [97, 62, 40, 60], [99, 30, 0, 0]),
        ],
    );
    // The two bodies beat slowly; harder keys are brighter and louder.
    params.operators[0].detune = 8;
    params.operators[2].detune = 5;
    for (operator, sensitivity) in params.operators.iter_mut().zip([2, 5, 2, 5, 3, 6]) {
        operator.key_velocity_sensitivity = sensitivity;
        operator.rate_scaling = 3;
    }
    Patch::new(params).unwrap()
}

/// A bass: algorithm 16, one carrier with two modulators on, one of them
/// (operator 6, through operator 5) fed back to itself for an edge.
fn bass() -> Patch {
    let off = operator(1, 0, 0, [99; 4], [99, 99, 99, 0]);
    let mut params = voice(
        b"FB BASS   ",
        16,
        6,
        [
            operator(1, 0, 99, [96, 40, 30, 70], [99, 90, 70, 0]),
            operator(1, 0, 78, [96, 55, 30, 70], [99, 70, 40, 0]),
            off,
            off,
            operator(0, 0, 70, [96, 60, 30, 70], [99, 50, 20, 0]),
            operator(1, 0, 75, [96, 50, 30, 70], [99, 60, 0, 0]),
        ],
    );
    params.transpose = 12;
    Patch::new(params).unwrap()
}

/// A bell: algorithm 5 again, with modulators at ratios that are not whole
/// (3.5 and 2.76), long decays and a vibrato for the modulation wheel.
fn bell() -> Patch {
    let ring = [98, 28, 20, 38];
    let mut params = voice(
        b"BELL      ",
        5,
        0,
        [
            operator(1, 0, 99, ring, [99, 80, 0, 0]),
            operator(3, 50, 78, [98, 35, 25, 40], [99, 70, 0, 0]),
            operator(1, 0, 92, ring, [99, 80, 0, 0]),
            operator(2, 38, 74, [98, 40, 25, 40], [99, 60, 0, 0]),
            operator(4, 0, 80, [98, 45, 30, 45], [99, 50, 0, 0]),
            operator(11, 0, 60, [98, 55, 30, 45], [99, 0, 0, 0]),
        ],
    );
    params.operators[2].detune = 9;
    params.lfo_speed = 34;
    params.lfo_wave = 4;
    params.pitch_mod_sensitivity = 2;
    Patch::new(params).unwrap()
}

/// Samples in a beat: 100 beats a minute.
const BEAT: usize = SAMPLE_RATE as usize * 60 / 100;

/// A message at a time in samples.
type Timed = (usize, [u8; 3]);

/// A note of `part` from `start` for `beats`, in beats.
fn note(score: &mut Vec<Timed>, part: u8, key: u8, velocity: u8, start: f64, beats: f64) {
    let on = (start * BEAT as f64) as usize;
    let off = ((start + beats) * BEAT as f64) as usize;
    score.push((on, [0x90 | part, key, velocity]));
    score.push((off, [0x80 | part, key, 0]));
}

/// Sixteen beats: four bars of four, a chord to the bar (A minor, F, C, G).
fn score() -> Vec<Timed> {
    let mut score = vec![(0, [0xc0, 0, 0]), (0, [0xc1, 1, 0]), (0, [0xc2, 2, 0])];
    let bars: [([u8; 4], u8); 4] =
        [([57, 60, 64, 67], 45), ([53, 57, 60, 65], 41), ([55, 60, 64, 67], 48), ([55, 59, 62, 65], 43)];
    for (bar, (chord, root)) in bars.into_iter().enumerate() {
        let start = 4.0 * bar as f64;
        // The chord, spread a little like a hand, then again off the beat.
        for (finger, key) in chord.into_iter().enumerate() {
            note(&mut score, 0, key, 88, start + 0.03 * finger as f64, 2.4);
            note(&mut score, 0, key, 70, start + 2.5, 1.3);
        }
        // The bass: root, root, fifth, octave, walking into the next bar.
        for (step, (interval, beats)) in [(0, 0.9), (0, 0.4), (7, 0.9), (12, 0.4), (7, 0.4)].into_iter().enumerate() {
            let at = [0.0, 1.5, 2.0, 3.0, 3.5][step];
            note(&mut score, 1, root + interval, 105, start + at, beats);
        }
    }
    // The melody, on the bell.
    let melody: [(f64, f64, u8); 14] = [
        (0.0, 1.0, 76),
        (1.0, 0.5, 72),
        (1.5, 0.5, 74),
        (2.0, 2.0, 76),
        (4.0, 1.0, 77),
        (5.0, 1.0, 72),
        (6.0, 2.0, 69),
        (8.0, 0.5, 72),
        (8.5, 0.5, 76),
        (9.0, 1.0, 79),
        (10.0, 2.0, 76),
        (12.0, 1.0, 74),
        (13.0, 1.0, 71),
        (14.0, 2.0, 67),
    ];
    for (start, beats, key) in melody {
        note(&mut score, 2, key, 100, start, beats);
    }
    // Vibrato swells on the long notes; a bend up into the high G.
    for step in 0..=16_u8 {
        let wheel = step * 5;
        score.push((((2.4 + 0.09 * f64::from(step)) * BEAT as f64) as usize, [0xb2, 1, wheel]));
        score.push((((10.4 + 0.09 * f64::from(step)) * BEAT as f64) as usize, [0xb2, 1, wheel]));
        // From two semitones below (bend 0) up to the note (bend 0x2000).
        let bend = u16::from(step) * 0x200;
        score.push((
            ((9.0 + 0.03 * f64::from(step)) * BEAT as f64) as usize,
            [0xe2, (bend & 0x7f) as u8, (bend >> 7) as u8],
        ));
    }
    for beat in [4.0, 12.0] {
        score.push(((beat * BEAT as f64) as usize, [0xb2, 1, 0]));
    }
    score.sort_by_key(|&(at, _)| at);
    score
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: dx7_demo OUTPUT.wav");

    let mut bank = Bank::new();
    bank.set(0, electric_piano());
    bank.set(1, bass());
    bank.set(2, bell());
    let mut rack = Rack::new();
    rack.set_bank(&bank);

    // Sixteen beats and a second and a half for the last notes to ring.
    let total = 16 * BEAT + SAMPLE_RATE as usize * 3 / 2;
    let mut samples = vec![0_i16; total];
    let mut at = 0;
    for (time, [status, data1, data2]) in score() {
        let time = time.min(total);
        rack.render(&mut samples[at..time]);
        at = time;
        rack.event(status, data1, data2);
    }
    rack.release_all();
    rack.render(&mut samples[at..]);

    let peak = samples.iter().map(|&sample| i32::from(sample).abs()).max().unwrap();
    let rms = (samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / total as f64).sqrt();
    let decibels = |level: f64| 20.0 * (level / 32_768.0).log10();
    println!(
        "{path}: {:.2} s, peak {peak} ({:.1} dBFS), RMS {rms:.0} ({:.1} dBFS), {} messages ignored",
        total as f64 / f64::from(SAMPLE_RATE),
        decibels(f64::from(peak)),
        decibels(rms),
        rack.ignored_events()
    );
    assert!(rms > 100.0, "the demo is nearly silent");
    assert!(peak < 32_767, "the demo reaches full scale");

    let data_bytes = (total * 2) as u32;
    let mut file = std::io::BufWriter::new(std::fs::File::create(&path).expect("cannot create the output file"));
    let mut header = Vec::new();
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    header.extend_from_slice(&1_u16.to_le_bytes()); // mono
    header.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    header.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes a second
    header.extend_from_slice(&2_u16.to_le_bytes()); // bytes a sample
    header.extend_from_slice(&16_u16.to_le_bytes()); // bits a sample
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    file.write_all(&header).expect("cannot write the output file");
    for sample in samples {
        file.write_all(&sample.to_le_bytes()).expect("cannot write the output file");
    }
    file.flush().expect("cannot write the output file");
}
