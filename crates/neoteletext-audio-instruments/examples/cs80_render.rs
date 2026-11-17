//! Renders a Standard MIDI File on the CS-80 to a WAV file, so that the
//! same score can be played here and on a reference program and the two
//! measured (DEC-6429, "Reference harness"):
//!
//! `cargo run --release -p neoteletext-audio-instruments --features cs80 --example cs80_render -- in.mid out.wav`
//!
//! The file's first track is played: its note, pressure, controller and
//! pitch bend messages. After the files come the patch's settings, taken
//! in order: `patch=FILE` reads a patch's 85 bytes; `preset1=N` and
//! `preset2=N` put preset N (0 to 21, in the order of
//! `cs80::presets::PRESETS`) on channel I or II; `1.B=V` and `2.B=V` set
//! byte B of channel I's or II's tone to V, and `g.B=V` byte B of the
//! controls outside the tones (the bytes are those of `cs80::patch`).
//! The output is 48 kHz, 16 bits, mono, the length of the track and
//! `tail=SECONDS` more (1 by default).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, reading a file it trusts in f64 and indices"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::cs80::presets::PRESETS;
use neoteletext_audio_instruments::cs80::{Cs80, PATCH_BYTES, Patch, TONE_BYTES};

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

/// The patch the options ask for, and the tail in seconds.
fn patch(options: &[String]) -> (Patch, f64) {
    let mut bytes = Patch::DEFAULT.to_bytes();
    let mut tail = 1.0;
    for option in options {
        let (name, value) = option.split_once('=').expect("an option is NAME=VALUE");
        let number = || value.parse::<u8>().expect("a number");
        match name {
            "tail" => tail = value.parse().expect("seconds"),
            "patch" => bytes = std::fs::read(value).expect("the patch is read").try_into().expect("85 bytes"),
            "preset1" => bytes[..TONE_BYTES].copy_from_slice(&PRESETS[usize::from(number())].1.to_bytes()),
            "preset2" => {
                bytes[TONE_BYTES..2 * TONE_BYTES].copy_from_slice(&PRESETS[usize::from(number())].1.to_bytes());
            }
            _ => {
                let (part, byte) = name.split_once('.').expect("a byte is PART.BYTE");
                let byte: usize = byte.parse().expect("a byte's number");
                let offset = match part {
                    "1" => 0,
                    "2" => TONE_BYTES,
                    "g" => 2 * TONE_BYTES,
                    _ => panic!("a part is 1, 2 or g"),
                };
                bytes[offset + byte] = number();
            }
        }
    }
    let bytes: [u8; PATCH_BYTES] = bytes;
    (Patch::from_bytes(&bytes).expect("a patch"), tail)
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, options @ ..] = arguments.as_slice() else {
        panic!(
            "usage: cs80_render IN.mid OUT.wav [patch=FILE] [preset1=N] [preset2=N] [1.B=V] [2.B=V] [g.B=V] [tail=SECONDS]"
        );
    };
    let (events, length) = read(&std::fs::read(input).expect("the MIDI file is read"));
    let (patch, tail) = patch(options);
    let mut cs80 = Cs80::new();
    cs80.set_patch(&patch);
    let mut samples = vec![0i16; ((length + tail) * f64::from(SAMPLE_RATE)).round() as usize];
    let mut done = 0;
    for (time, bytes) in events {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        cs80.render(&mut samples[done..until]);
        done = until;
        match *bytes.as_slice() {
            [status, first, second] => cs80.event(status, first, second),
            [status, first] => cs80.event(status, first, 0),
            _ => {}
        }
    }
    cs80.render(&mut samples[done..]);

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
