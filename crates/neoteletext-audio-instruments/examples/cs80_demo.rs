//! Renders a short piece on the CS-80 to a WAV file, for listening:
//!
//! `cargo run --release -p neoteletext-audio-instruments --features cs80 --example cs80_demo -- out.wav`
//!
//! The file is 48 kHz, 16 bits, mono, about 16 seconds. The piece is
//! written here. First the two brass presets, BRASS 1 on channel I and
//! BRASS 2 on channel II a little sharp, in four slow chords: the top
//! note of each is pressed harder as it is held, which brightens it,
//! raises it and brings in the sub oscillator's vibrato, and the last
//! chord falls away under the ribbon. Then STRING 1 over STRING 4 an
//! octave below, the foot switch down so that the notes die away in the
//! sustain lever's time, a melody gliding from note to note above a held
//! chord, through the tremolo. It prints each half's peak and RMS level.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, composing a piece in f64 on indices it derives itself"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::cs80::presets::PRESETS;
use neoteletext_audio_instruments::cs80::{Cs80, Feet, Function, Global, Patch, Tone};

/// Seconds each chord of the first half is held, and between two chords.
const CHORD: f64 = 1.7;
const BAR: f64 = 2.0;
/// Seconds of each half.
const HALF: f64 = 8.0;

/// The first half's chords: D minor, C major, B flat major, A major.
const CHORDS: [[u8; 5]; 4] = [[38, 50, 57, 62, 65], [36, 48, 55, 60, 67], [34, 46, 53, 58, 65], [33, 45, 52, 57, 64]];

/// What happens at an instant: a MIDI message or a new patch.
enum Change {
    Midi([u8; 3]),
    Patch(Box<Patch>),
}

/// A change and its time in seconds.
struct Timed(f64, Change);

/// A preset by its button's name.
fn preset(name: &str) -> Tone {
    PRESETS.iter().find(|(found, _)| *found == name).expect("a preset").1
}

fn note(score: &mut Vec<Timed>, key: u8, velocity: u8, start: f64, length: f64) {
    score.push(Timed(start, Change::Midi([0x90, key, velocity])));
    score.push(Timed(start + length, Change::Midi([0x80, key, 0])));
}

/// The brass: both channels, channel II a few cents sharp, pressure
/// bringing a vibrato.
fn brass() -> Patch {
    let mut patch = Patch::with_tones(preset("BRASS 1"), preset("BRASS 2"));
    for (global, value) in [
        (Global::Detune, 70),
        // A hard stroke is bright: the overall lever takes some of it back.
        (Global::Brilliance, 24),
        (Global::SubFunction, Function::Sine as u8),
        (Global::SubSpeed, 62),
        (Global::TouchVco, 40),
        (Global::TouchSpeed, 20),
    ] {
        patch.set(global, value);
    }
    patch
}

/// The strings: channel II an octave below, a long sustain under the
/// foot switch, a slow glide, the tremolo.
fn strings() -> Patch {
    let mut patch = Patch::with_tones(preset("STRING 1"), preset("STRING 4"));
    for (global, value) in [
        (Global::FeetTwo, Feet::Sixteen as u8),
        (Global::Detune, 58),
        (Global::Brilliance, 40),
        (Global::Sustain, 96),
        (Global::GlideTime, 70),
        (Global::SubSpeed, 60),
        (Global::SubVco, 5),
        (Global::Effect, 1),
        (Global::Tremolo, 1),
        (Global::EffectSpeed, 80),
        (Global::EffectDepth, 127),
    ] {
        patch.set(global, value);
    }
    patch
}

fn score() -> Vec<Timed> {
    let mut score = vec![Timed(0.0, Change::Patch(Box::new(brass())))];
    for (bar, chord) in CHORDS.iter().enumerate() {
        let start = bar as f64 * BAR;
        for (voice, &key) in chord.iter().enumerate() {
            // The chord is rolled upwards.
            note(&mut score, key, 96 + 6 * voice as u8, start + 0.02 * voice as f64, CHORD);
        }
        // The top note is pressed harder and harder.
        for step in 0..=20u8 {
            let time = start + 0.5 + 0.05 * f64::from(step);
            score.push(Timed(time, Change::Midi([0xa0, chord[4], 3 * step])));
        }
    }
    // The last chord falls under the ribbon, which is then let go.
    for step in 0..=30u16 {
        let bend = 0x2000 - step * 0x100;
        let time = 3.0 * BAR + 1.0 + 0.02 * f64::from(step);
        score.push(Timed(time, Change::Midi([0xe0, (bend & 0x7f) as u8, (bend >> 7) as u8])));
    }
    score.push(Timed(HALF - 0.05, Change::Midi([0xe0, 0, 0x40])));

    score.push(Timed(HALF, Change::Patch(Box::new(strings()))));
    score.push(Timed(HALF, Change::Midi([0xb0, 64, 127])));
    for key in [38u8, 50, 57] {
        note(&mut score, key, 105, HALF, 5.5);
    }
    // The melody, each note let go before the next: the voices come round
    // in turn, and each glides from the note it last played.
    for (step, (key, beats)) in
        [(74u8, 2.0), (77, 1.0), (76, 1.0), (72, 2.0), (69, 1.0), (74, 4.0)].into_iter().enumerate()
    {
        let before: f64 = [2.0, 1.0, 1.0, 2.0, 1.0, 4.0][..step].iter().sum();
        note(&mut score, key, 120, 0.5f64.mul_add(before, HALF + 0.25), 0.5 * beats - 0.05);
    }
    score.sort_by(|a, b| a.0.total_cmp(&b.0));
    score
}

fn main() {
    let output = std::env::args().nth(1).expect("usage: cs80_demo OUT.wav");
    let mut cs80 = Cs80::new();
    let mut samples = vec![0i16; (2.0 * HALF * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    for Timed(time, change) in score() {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        cs80.render(&mut samples[done..until]);
        done = until;
        match change {
            Change::Midi([status, first, second]) => cs80.event(status, first, second),
            Change::Patch(patch) => cs80.set_patch(&patch),
        }
    }
    cs80.release_all();
    cs80.render(&mut samples[done..]);

    for (name, half) in ["brass", "strings"].into_iter().zip(samples.chunks(samples.len() / 2)) {
        let peak = half.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
        let power = half.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / half.len() as f64;
        println!(
            "{name}: peak {:.1} dBFS, RMS {:.1} dBFS",
            20.0 * (f64::from(peak) / 32_768.0).log10(),
            10.0 * (power / 32_768.0f64.powi(2)).log10()
        );
    }

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
