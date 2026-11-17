//! Renders a Standard MIDI File on the Minimoog to a WAV file, so that
//! the same score can be played here and on a reference program and the
//! two measured (DEC-6429, "Reference harness"):
//!
//! `cargo run --release -p neoteletext-audio-instruments --features minimoog --example minimoog_render -- in.mid out.wav`
//!
//! The file's first track is played: its note, controller and pitch bend
//! messages. After the files, `patch=HEX` sets the patch from the 72
//! hexadecimal digits of its 36 bytes (the layout is in the module
//! `minimoog::patch`), `drift=CENTS` sets the oscillators' drift (0 is
//! none) and `tail=SECONDS` how long the output runs after the track (1
//! by default). The output is 48 kHz, 16 bits, mono.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, reading a file it trusts in f64 and indices"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::minimoog::{Minimoog, PATCH_BYTES, Patch};

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
    let [input, output, options @ ..] = arguments.as_slice() else {
        panic!("usage: minimoog_render IN.mid OUT.wav [patch=HEX] [drift=CENTS] [tail=SECONDS]");
    };
    let mut tail = 1.0;
    let mut minimoog = Minimoog::new();
    for option in options {
        if let Some(digits) = option.strip_prefix("patch=") {
            assert_eq!(digits.len(), PATCH_BYTES * 2, "a patch is 72 hexadecimal digits");
            let mut bytes = [0u8; PATCH_BYTES];
            for (byte, at) in bytes.iter_mut().zip((0..digits.len()).step_by(2)) {
                *byte = u8::from_str_radix(&digits[at..at + 2], 16).expect("hexadecimal digits");
            }
            minimoog.set_patch(&Patch::from_bytes(&bytes).expect("a patch"));
        } else if let Some(cents) = option.strip_prefix("drift=") {
            minimoog.set_drift(cents.parse().expect("the drift in cents"));
        } else {
            tail = option.strip_prefix("tail=").and_then(|text| text.parse().ok()).expect("an option");
        }
    }
    let (events, length) = read(&std::fs::read(input).expect("the MIDI file is read"));
    let mut samples = vec![0i16; ((length + tail) * f64::from(SAMPLE_RATE)).round() as usize];
    let mut done = 0;
    for (time, bytes) in events {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        minimoog.render(&mut samples[done..until]);
        done = until;
        if let &[status, first, second] = bytes.as_slice() {
            minimoog.event(status, first, second);
        }
    }
    minimoog.render(&mut samples[done..]);
    std::fs::write(output, wav(&samples)).expect("the file is written");
}

/// A WAV file of 16-bit mono samples at the output rate.
fn wav(samples: &[i16]) -> Vec<u8> {
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
    file
}
