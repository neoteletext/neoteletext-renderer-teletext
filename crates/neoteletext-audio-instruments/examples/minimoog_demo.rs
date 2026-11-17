//! Renders a short piece on the Minimoog to a WAV file, for listening:
//!
//! `cargo run --release -p neoteletext-audio-instruments --features minimoog --example minimoog_demo -- out.wav`
//!
//! The file is 48 kHz, 16 bits, mono, about 16 seconds. The piece is
//! written here. The instrument plays one note at a time, so the piece is
//! five takes of it laid over one another, as a player with a tape
//! recorder would: a bass with the output patched back into the external
//! input; the sound charts' electric piano in broken chords; the sound
//! charts' violin with glide, the modulation wheel brought up on its long
//! notes; and at the end the sound charts' shamisen, and the filter
//! oscillating by itself, swept by its contour. It prints each take's
//! peak and RMS level, and the mix's.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, composing a piece in f64 on indices it derives itself"
)]

#[path = "../tests/minimoog_charts.rs"]
mod charts;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::minimoog::{ContourPatch, Minimoog, OscillatorPatch, Patch, Range, Waveform};

/// Seconds in a beat: 120 beats a minute.
const BEAT: f64 = 0.5;
/// Beats in a bar, and the piece's bars.
const BEATS: f64 = 4.0;
const BARS: usize = 7;
/// Seconds rendered after the last bar, for the last note's release.
const TAIL: f64 = 1.5;

/// The bars' bass notes and chords, as keys: A minor, F major, G major,
/// A minor, and round again.
const HARMONY: [(u8, [u8; 4]); 4] =
    [(45, [57, 60, 64, 69]), (41, [57, 60, 65, 69]), (43, [59, 62, 67, 71]), (45, [60, 64, 69, 72])];

/// A MIDI message and its time in seconds.
struct Timed(f64, [u8; 3]);

fn note(score: &mut Vec<Timed>, key: u8, start: f64, length: f64) {
    score.push(Timed(start, [0x90, key, 100]));
    score.push(Timed(start + length, [0x80, key, 0]));
}

/// The start of a beat of a bar, in seconds.
fn at(bar: usize, beat: f64) -> f64 {
    (bar as f64 * BEATS + beat) * BEAT
}

/// The bass: two sawtooths a few cents apart and a square an octave
/// below, the filter low with a short contour, and a third of the output
/// returned to the external input.
fn bass_patch() -> Patch {
    let oscillator =
        |range, waveform, frequency, volume| OscillatorPatch { range, waveform, frequency, volume, on: true };
    Patch {
        oscillators: [
            oscillator(Range::Sixteen, Waveform::Sawtooth, 0, 80),
            oscillator(Range::Sixteen, Waveform::Sawtooth, 7, 80),
            oscillator(Range::ThirtyTwo, Waveform::Square, -6, 60),
        ],
        external_volume: 35,
        external_on: true,
        cutoff: -250,
        emphasis: 30,
        contour_amount: 45,
        filter_contour: ContourPatch { attack: 0, decay: 250, sustain: 30 },
        loudness_contour: ContourPatch { attack: 0, decay: 400, sustain: 80 },
        keyboard_control: [true, false],
        decay_on: true,
        volume: 60,
        ..Patch::DEFAULT
    }
}

fn bass() -> Vec<Timed> {
    let mut score = Vec::new();
    for bar in 0..BARS - 1 {
        let root = HARMONY[bar % HARMONY.len()].0;
        let figure =
            [(0.0, root, 0.9), (1.5, root, 0.4), (2.0, root + 12, 0.4), (3.0, root + 7, 0.4), (3.5, root + 10, 0.4)];
        for (beat, key, length) in figure {
            note(&mut score, key, at(bar, beat), length * BEAT);
        }
    }
    note(&mut score, 45, at(BARS - 1, 0.0), 3.0 * BEAT);
    score
}

/// The electric piano: each bar's chord broken into eighths, up and
/// down.
fn piano() -> Vec<Timed> {
    let mut score = Vec::new();
    for (bar, (_, chord)) in HARMONY.iter().enumerate() {
        for (step, index) in [0, 1, 2, 3, 2, 1, 2, 3].into_iter().enumerate() {
            note(&mut score, chord[index] + 12, at(bar, step as f64 / 2.0), 0.3 * BEAT);
        }
    }
    score
}

/// The violin: a melody over bars 3 to 6, the wheel brought up on each
/// long note and let down before the next.
fn violin() -> Vec<Timed> {
    let mut score = Vec::new();
    let melody = [
        (2, 0.0, 76, 3.0),
        (2, 3.0, 74, 1.0),
        (3, 0.0, 72, 2.5),
        (3, 2.5, 69, 1.5),
        (4, 0.0, 77, 3.0),
        (4, 3.0, 76, 1.0),
        (5, 0.0, 74, 2.0),
        (5, 2.0, 71, 2.0),
    ];
    for (bar, beat, key, beats) in melody {
        note(&mut score, key, at(bar, beat), (beats - 0.1) * BEAT);
        if beats >= 2.0 {
            for step in 0..=8u8 {
                score.push(Timed(at(bar, beat + 0.8 + f64::from(step) * 0.1), [0xb0, 1, step * 8]));
            }
            score.push(Timed(at(bar, beat + beats - 0.1), [0xb0, 1, 0]));
        }
    }
    score
}

/// The shamisen's answer in bar 6, and the last bar's whistle: the
/// filter alone, oscillating, its contour sweeping it up and back.
fn ending() -> [(Patch, Vec<Timed>); 2] {
    let mut plucked = Vec::new();
    for (step, key) in [69, 72, 76, 81, 76, 72, 69, 64].into_iter().enumerate() {
        note(&mut plucked, key, at(5, step as f64 / 2.0), 0.2 * BEAT);
    }
    let mut whistled = Vec::new();
    note(&mut whistled, 57, at(6, 0.0), 2.5 * BEAT);
    let whistle = Patch {
        oscillators: [OscillatorPatch { on: false, ..Patch::DEFAULT.oscillators[0] }; 3],
        cutoff: -100,
        emphasis: 100,
        contour_amount: 25,
        filter_contour: ContourPatch { attack: 300, decay: 500, sustain: 0 },
        loudness_contour: ContourPatch { attack: 100, decay: 800, sustain: 100 },
        keyboard_control: [true, true],
        decay_on: true,
        ..Patch::DEFAULT
    };
    [(Patch { volume: 100, ..charts::SHAMISEN }, plucked), (whistle, whistled)]
}

/// One take: a patch and its score, rendered alone.
fn take(patch: &Patch, mut score: Vec<Timed>, length: usize) -> Vec<i16> {
    score.sort_by(|first, second| first.0.total_cmp(&second.0));
    let mut minimoog = Minimoog::new();
    minimoog.set_patch(patch);
    let mut samples = vec![0i16; length];
    let mut done = 0;
    for Timed(time, [status, first, second]) in score {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, length);
        minimoog.render(&mut samples[done..until]);
        done = until;
        minimoog.event(status, first, second);
    }
    minimoog.render(&mut samples[done..]);
    samples
}

/// The peak and the root mean square of samples, in decibels from full
/// scale.
fn levels(samples: &[i16]) -> (f64, f64) {
    let decibels = |value: f64| 20.0 * (value / 32_768.0).log10();
    let peak = samples.iter().map(|&sample| f64::from(sample).abs()).fold(0.0, f64::max);
    let power = samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / samples.len() as f64;
    (decibels(peak), decibels(power.sqrt()))
}

fn main() {
    let output = std::env::args().nth(1).expect("usage: minimoog_demo OUT.wav");
    let length = ((BARS as f64 * BEATS * BEAT + TAIL) * f64::from(SAMPLE_RATE)).round() as usize;
    let [shamisen, whistle] = ending();
    // Each take, and its gain in the mix.
    let takes = [
        ("bass, the output returned", bass_patch(), bass(), 2.0),
        ("electric piano", Patch { volume: 100, ..charts::ELECTRIC_PIANO }, piano(), 4.0),
        ("violin", Patch { volume: 100, ..charts::VIOLIN }, violin(), 2.8),
        ("shamisen", shamisen.0, shamisen.1, 2.8),
        ("the filter alone", whistle.0, whistle.1, 2.8),
    ];
    let mut mix = vec![0.0f64; length];
    for (name, patch, score, gain) in takes {
        let samples = take(&patch, score, length);
        let (peak, rms) = levels(&samples);
        println!("{name}: peak {peak:.1} dBFS, RMS {rms:.1} dBFS, mixed at {gain}");
        for (sum, &sample) in mix.iter_mut().zip(&samples) {
            *sum += gain * f64::from(sample);
        }
    }
    let samples: Vec<i16> = mix.iter().map(|&sum| sum.round().clamp(-32_768.0, 32_767.0) as i16).collect();
    let (peak, rms) = levels(&samples);
    println!("the mix: peak {peak:.1} dBFS, RMS {rms:.1} dBFS, {:.1} s", length as f64 / f64::from(SAMPLE_RATE));

    let bytes = u32::try_from(samples.len() * 2).expect("a short file");
    let mut file = Vec::with_capacity(samples.len() * 2 + 44);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + bytes).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    // PCM, one channel, 16 bits.
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    file.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    file.extend_from_slice(&2u16.to_le_bytes());
    file.extend_from_slice(&16u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&bytes.to_le_bytes());
    for sample in samples {
        file.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(output, file).expect("the file is written");
}
