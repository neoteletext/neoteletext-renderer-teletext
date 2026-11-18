//! Renders a short piece on synth8 to a WAV file, for listening:
//!
//! `cargo run --release -p neoteletext-audio-instruments --features synth8 --example synth8_demo -- out.wav`
//!
//! The file is 48 kHz, 16 bits, mono, about 17 seconds. The piece is
//! written here, and played on five of Roland's factory patches as they
//! were read from the patch sheets (`tests/synth8_factory.rs`): three
//! bars with the keyboard split, MID STRINGS above and LO BRASS below;
//! a line on SYNC SWEEP with all eight voices in unison and a little
//! portamento; and three strokes of GONG, which is cross modulation. It
//! prints each part's peak and RMS level.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, composing a piece in f64 on indices it derives itself"
)]

#[path = "../tests/synth8_factory.rs"]
mod factory;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth8::{KeyMode, Param, Patch, Performance, Portamento, Synth8};

/// Seconds in a beat: 100 beats a minute.
const BEAT: f64 = 0.6;
/// Beats in a bar.
const BEATS: f64 = 4.0;

/// The chords, as keys from middle C up, each with a bass note below it:
/// A minor, F major, G major.
const CHORDS: [([u8; 4], u8); 3] = [([60, 64, 69, 72], 45), ([60, 65, 69, 72], 41), ([62, 67, 71, 74], 43)];
/// The line: keys and their lengths in beats.
const LINE: [(u8, f64); 8] = [(57, 1.0), (60, 0.5), (64, 0.5), (69, 1.5), (67, 0.5), (64, 1.0), (62, 1.0), (57, 2.0)];
/// The strokes of the gong: keys and when, in beats.
const STROKES: [(u8, f64); 3] = [(45, 0.0), (52, 2.5), (40, 4.0)];

/// What happens at an instant.
enum Change {
    Midi([u8; 3]),
    Upper(Box<Patch>),
    Lower(Box<Patch>),
    Mode(KeyMode),
    Panel(Performance),
}

/// A change and its time in seconds.
struct Timed(f64, Change);

fn note(score: &mut Vec<Timed>, key: u8, start: f64, length: f64) {
    score.push(Timed(start, Change::Midi([0x90, key, 100])));
    score.push(Timed(start + length, Change::Midi([0x80, key, 0])));
}

/// The three parts, each with its name, its score from its own start
/// and its length in seconds.
fn parts() -> Vec<(&'static str, Vec<Timed>, f64)> {
    let bar = BEAT * BEATS;

    // The strings above the split point and the brass below it.
    let mut split = vec![
        Timed(0.0, Change::Mode(KeyMode::Split)),
        Timed(0.0, Change::Panel(Performance::DEFAULT)),
        Timed(0.0, Change::Upper(Box::new(factory::mid_strings()))),
        Timed(0.0, Change::Lower(Box::new(factory::lo_brass()))),
    ];
    for (index, (chord, bass)) in CHORDS.iter().enumerate() {
        let start = index as f64 * bar;
        for &key in chord {
            note(&mut split, key, start, bar * 0.95);
        }
        for (beat, length) in [(0.0, 1.4), (1.5, 0.4), (2.0, 0.9), (3.0, 0.9)] {
            note(&mut split, *bass, start + beat * BEAT, length * BEAT);
        }
    }

    // The line, all eight voices on each key, sliding from key to key.
    let lead = factory::sync_sweep().with(Param::Env2Release, 45);
    let glide = Performance { portamento: Portamento::On, portamento_time: 12, ..Performance::DEFAULT };
    let mut line = vec![
        Timed(0.0, Change::Mode(KeyMode::Whole)),
        Timed(0.0, Change::Panel(glide)),
        Timed(0.0, Change::Upper(Box::new(lead))),
    ];
    let mut start = 0.0;
    for (key, beats) in LINE {
        note(&mut line, key, start, beats * BEAT * 0.9);
        start += beats * BEAT;
    }

    let mut gong = vec![
        Timed(0.0, Change::Panel(Performance::DEFAULT)),
        // The sheet has the gong in unison, for single notes; in poly 1
        // here each stroke rings on under the next.
        Timed(
            0.0,
            Change::Upper(Box::new(
                factory::gong().with(Param::AssignMode, 0).with(Param::Env2Sustain, 0).with(Param::Env2Decay, 95),
            )),
        ),
    ];
    for (key, beat) in STROKES {
        note(&mut gong, key, beat * BEAT, 0.3);
    }

    vec![
        ("MID STRINGS over LO BRASS, split", split, 3.0 * bar + 0.6),
        ("SYNC SWEEP, unison", line, start + 0.7),
        ("GONG", gong, 4.9),
    ]
}

/// Renders a score for a length on an instrument, which goes on from
/// where the part before left it.
fn render(synth8: &mut Synth8, mut score: Vec<Timed>, length: f64) -> Vec<i16> {
    score.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut samples = vec![0i16; (length * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    for Timed(time, change) in score {
        let until = ((time * f64::from(SAMPLE_RATE)) as usize).clamp(done, samples.len());
        synth8.render(&mut samples[done..until]);
        done = until;
        match change {
            Change::Midi([status, first, second]) => {
                let _ = synth8.event(status, first, second);
            }
            Change::Upper(patch) => synth8.set_patch(&patch),
            Change::Lower(patch) => synth8.set_lower_patch(&patch),
            Change::Mode(mode) => synth8.set_key_mode(mode),
            Change::Panel(panel) => synth8.set_performance(&panel),
        }
    }
    synth8.render(&mut samples[done..]);
    samples
}

fn levels(name: &str, samples: &[i16]) {
    let peak = samples.iter().map(|&sample| f64::from(sample).abs()).fold(0.0, f64::max) / 32_768.0;
    let power = samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / samples.len() as f64;
    let rms = power.sqrt() / 32_768.0;
    println!("{name}: peak {peak:.4} ({:.1} dBFS), RMS {rms:.4} ({:.1} dBFS)", 20.0 * peak.log10(), 20.0 * rms.log10());
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: synth8_demo OUTPUT.wav");
    let mut synth8 = Synth8::new();
    let mut samples = Vec::new();
    for (name, score, length) in parts() {
        let part = render(&mut synth8, score, length);
        levels(name, &part);
        samples.extend(part);
        synth8.release_all();
    }
    levels("the whole", &samples);

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
    std::fs::write(&path, file).expect("the file is written");
    println!("wrote {path}");
}
