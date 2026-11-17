//! Plays about sixteen seconds on the Prophet-5 and writes them as a WAV
//! file: five of Sequential's original factory programs, entered from
//! the patch sheets, each with a phrase written for it.
//!
//! `cargo run --release -p neoteletext-audio-instruments --features prophet5 --example prophet5_demo -- out.wav [2]`
//!
//! The second argument `2` plays revisions 1 and 2, the SSM chips;
//! without it revision 3 plays. The output is 48 kHz, 16 bits, mono; the
//! peak and the level are printed.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, in f64 and indices"
)]

#[path = "../tests/prophet5_programs.rs"]
mod programs;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::prophet5::{Model, Prophet5};
use programs::{BELLS, BRASS, LOW_STRINGS, SYNC, Sheet, UNISON_GLIDE};

/// What happens at a time: a program, a key down or up, or the
/// modulation wheel.
enum What {
    Program(&'static Sheet),
    Down(u8),
    Up(u8),
    Wheel(u8),
}

/// A chord: its keys down at a time and up after a length.
fn chord(events: &mut Vec<(f64, What)>, at: f64, length: f64, keys: &[u8]) {
    for &key in keys {
        events.push((at, What::Down(key)));
        events.push((at + length, What::Up(key)));
    }
}

/// The score: seconds, and what happens then.
fn score() -> Vec<(f64, What)> {
    let mut events = Vec::new();
    // Brass: a fanfare in fourths, the wheel a little up for vibrato.
    events.push((0.0, What::Program(&BRASS)));
    events.push((0.0, What::Wheel(24)));
    chord(&mut events, 0.0, 0.45, &[55, 62, 67]);
    chord(&mut events, 0.5, 0.2, &[55, 62, 67]);
    chord(&mut events, 0.75, 0.45, &[57, 64, 69]);
    chord(&mut events, 1.25, 1.5, &[60, 65, 72, 76]);
    // The sync sweep: a line of single notes, each swept by the filter
    // envelope through Poly-Mod.
    events.push((3.1, What::Program(&SYNC)));
    events.push((3.1, What::Wheel(0)));
    for (index, key) in [45u8, 52, 57, 55, 52, 48, 50, 45].into_iter().enumerate() {
        chord(&mut events, 3.2 + 0.36 * index as f64, 0.3, &[key]);
    }
    // Strings: two slow chords, the pulse widths moved by the wheel.
    events.push((6.2, What::Program(&LOW_STRINGS)));
    events.push((6.2, What::Wheel(40)));
    chord(&mut events, 6.3, 1.7, &[41, 48, 53, 57, 60]);
    chord(&mut events, 8.1, 1.9, &[43, 50, 55, 59, 62]);
    // Bells: oscillator B into oscillator A's frequency.
    events.push((10.3, What::Program(&BELLS)));
    events.push((10.3, What::Wheel(0)));
    for (index, key) in [72u8, 79, 76, 84, 81, 77].into_iter().enumerate() {
        chord(&mut events, 10.4 + 0.33 * index as f64, 0.25, &[key]);
    }
    // Unison: five voices on one key, gliding.
    events.push((12.6, What::Program(&UNISON_GLIDE)));
    for (index, key) in [36u8, 48, 43, 39, 41, 36].into_iter().enumerate() {
        chord(&mut events, 12.7 + 0.45 * index as f64, 0.5, &[key]);
    }
    events.push((15.4, What::Wheel(30)));
    events.sort_by(|first, second| first.0.total_cmp(&second.0));
    events
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let output = arguments.first().expect("usage: prophet5_demo OUT.wav [2]");
    let model = if arguments.get(1).is_some_and(|model| model == "2") { Model::Rev2 } else { Model::Rev3 };
    let mut prophet = Prophet5::new(model);
    let mut samples = vec![0i16; (16.5 * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    for (time, what) in score() {
        let until = ((time * f64::from(SAMPLE_RATE)) as usize).clamp(done, samples.len());
        prophet.render(&mut samples[done..until]);
        done = until;
        match what {
            What::Program(sheet) => {
                // A program is changed with no key down.
                prophet.release_all();
                prophet.set_patch(&sheet.program(model));
            }
            What::Down(key) => prophet.event(0x90, key, 100),
            What::Up(key) => prophet.event(0x80, key, 0),
            What::Wheel(value) => prophet.event(0xb0, 1, value),
        }
    }
    prophet.render(&mut samples[done..]);

    let peak = samples.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
    let square = samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / samples.len() as f64;
    println!(
        "{model:?}: {} samples, peak {:.1} dB, level {:.1} dB below full scale",
        samples.len(),
        20.0 * (f64::from(peak) / 32_768.0).log10(),
        10.0 * (square / 32_768.0f64.powi(2)).log10()
    );

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
