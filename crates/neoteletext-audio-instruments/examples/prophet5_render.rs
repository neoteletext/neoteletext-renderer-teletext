//! Renders a Standard MIDI File on the Prophet-5 to a WAV file, so that
//! the same score can be played here and elsewhere and the two measured
//! (DEC-6429, "Reference harness"):
//!
//! `cargo run --release -p neoteletext-audio-instruments --features prophet5 --example prophet5_render -- 3 in.mid out.wav`
//!
//! The first argument is the revision, `3` (the Curtis chips) or `2`
//! (the SSM chips of revisions 1 and 2). The file's first track is
//! played: its note, controller and pitch bend messages. The original
//! instrument has no MIDI and so no message that carries a program:
//! after the files, `patch=FILE` names a file of a program's 24 bytes
//! (the operation manual's section 6-7), and `tail=SECONDS` how long to
//! go on after the track (1 by default). The output is 48 kHz, 16 bits,
//! mono.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, reading a file it trusts in f64 and indices"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::prophet5::patch::PATCH_BYTES;
use neoteletext_audio_instruments::prophet5::{Model, Patch, Prophet5};

/// A variable-length quantity at `at`, which it moves past it.
fn quantity(data: &[u8], at: &mut usize) -> usize {
    let mut value = 0;
    loop {
        let byte = data[*at];
        *at += 1;
        value = value << 7 | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return value;
        }
    }
}

/// What the track holds: each event's time in seconds and its bytes,
/// status first; and the track's length.
fn read(data: &[u8]) -> (Vec<(f64, Vec<u8>)>, f64) {
    assert_eq!(&data[..4], b"MThd", "a Standard MIDI File");
    let division = f64::from(u16::from_be_bytes([data[12], data[13]]));
    assert_eq!(&data[14..18], b"MTrk", "one track after the header");
    let end = 22 + u32::from_be_bytes([data[18], data[19], data[20], data[21]]) as usize;
    let (mut at, mut seconds, mut beat, mut status) = (22, 0.0, 0.5, 0u8);
    let mut events = Vec::new();
    while at < end {
        seconds += quantity(data, &mut at) as f64 / division * beat;
        if data[at] & 0x80 != 0 {
            status = data[at];
            at += 1;
        }
        match status {
            0xff => {
                let kind = data[at];
                at += 1;
                let length = quantity(data, &mut at);
                if kind == 0x51 {
                    beat = f64::from(u32::from_be_bytes([0, data[at], data[at + 1], data[at + 2]])) / 1.0e6;
                }
                at += length;
            }
            0xf0 | 0xf7 => {
                let length = quantity(data, &mut at);
                events.push((seconds, [&[status][..], &data[at..at + length]].concat()));
                at += length;
            }
            _ => {
                let length = if matches!(status & 0xf0, 0xc0 | 0xd0) { 1 } else { 2 };
                events.push((seconds, [&[status][..], &data[at..at + length]].concat()));
                at += length;
            }
        }
    }
    (events, seconds)
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [model, input, output, options @ ..] = arguments.as_slice() else {
        panic!("usage: prophet5_render 3|2 IN.mid OUT.wav [patch=FILE] [tail=SECONDS]");
    };
    let model = if model == "2" { Model::Rev2 } else { Model::Rev3 };
    let (mut tail, mut patch) = (1.0, Patch::DEFAULT);
    for option in options {
        if let Some(file) = option.strip_prefix("patch=") {
            let bytes = std::fs::read(file).expect("the patch file is read");
            let bytes: &[u8; PATCH_BYTES] = bytes.as_slice().try_into().expect("24 bytes");
            patch = Patch::from_bytes(bytes);
        } else {
            tail = option.strip_prefix("tail=").and_then(|text| text.parse().ok()).expect("an option");
        }
    }
    let (events, length) = read(&std::fs::read(input).expect("the MIDI file is read"));
    let mut prophet = Prophet5::new(model);
    prophet.set_patch(&patch);
    let mut samples = vec![0i16; ((length + tail + 1.0) * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    for (time, bytes) in events {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        prophet.render(&mut samples[done..until]);
        done = until;
        if let &[status, first, second] = bytes.as_slice() {
            prophet.event(status, first, second);
        }
    }
    samples.truncate((((length + tail) * f64::from(SAMPLE_RATE)).round() as usize).max(done));
    prophet.render(&mut samples[done..]);

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
