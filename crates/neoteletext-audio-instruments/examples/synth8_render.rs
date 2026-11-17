//! Renders a Standard MIDI File on synth8 to a WAV file, so that
//! the same score can be played here and on a reference program and the
//! two measured (DEC-6429, "Reference harness"):
//!
//! `cargo run --release -p neoteletext-audio-instruments --features synth8 --example synth8_render -- in.mid out.wav [OPTIONS]`
//!
//! The file's first track is played: its note, controller and pitch bend
//! messages. The instrument has no MIDI of its own and no patch message,
//! so the patches and the panel are options after the files:
//!
//! - `patch=P` and `lower=P`: the upper and the lower patch. `P` is the
//!   number of one of the factory patches read from Roland's sheets (14,
//!   31, 32, 35 or 58), or a patch's 43 bytes as 86 hexadecimal digits,
//!   or the path of a file of those 43 bytes.
//! - `mode=whole`, `mode=dual` or `mode=split`, and `split=KEY`.
//! - `bend=CENTS`: the bender's range (200 by default).
//! - `tail=SECONDS`: what is rendered after the track ends (1 by
//!   default).
//!
//! The output is 48 kHz, 16 bits, mono.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, reading a file it trusts in f64 and indices"
)]

#[path = "../tests/synth8_factory.rs"]
mod factory;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth8::{KeyMode, PATCH_BYTES, Patch, Performance, Synth8};

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

/// A patch from an option's value: a factory patch's number, 86
/// hexadecimal digits, or a file of 43 bytes.
fn patch(text: &str) -> Patch {
    if let Some((_, _, patch)) = factory::all().into_iter().find(|(number, _, _)| number.to_string() == text) {
        return patch;
    }
    let bytes = if text.len() == 2 * PATCH_BYTES && text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        (0..PATCH_BYTES)
            .map(|place| u8::from_str_radix(&text[2 * place..2 * place + 2], 16).expect("two digits"))
            .collect()
    } else {
        std::fs::read(text).expect("the patch file is read")
    };
    let bytes: [u8; PATCH_BYTES] = bytes.as_slice().try_into().expect("a patch is 43 bytes");
    Patch::from_bytes(&bytes).expect("a patch of synth8")
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, options @ ..] = arguments.as_slice() else {
        panic!(
            "usage: synth8_render IN.mid OUT.wav [patch=P] [lower=P] [mode=whole|dual|split] [split=KEY] [bend=CENTS] [tail=SECONDS]"
        );
    };
    let mut tail = 1.0;
    let mut synth8 = Synth8::new();
    for option in options {
        let (name, value) = option.split_once('=').expect("an option is NAME=VALUE");
        match name {
            "patch" => synth8.set_patch(&patch(value)),
            "lower" => synth8.set_lower_patch(&patch(value)),
            "mode" => synth8.set_key_mode(match value {
                "whole" => KeyMode::Whole,
                "dual" => KeyMode::Dual,
                "split" => KeyMode::Split,
                other => panic!("no key mode {other}"),
            }),
            "split" => synth8.set_split(value.parse().expect("a key")),
            "bend" => {
                synth8
                    .set_performance(&Performance { bend_vco: value.parse().expect("cents"), ..*synth8.performance() });
            }
            "tail" => tail = value.parse().expect("seconds"),
            other => panic!("no option {other}"),
        }
    }
    let (events, length) = read(&std::fs::read(input).expect("the MIDI file is read"));
    let mut samples = vec![0i16; ((length + tail) * f64::from(SAMPLE_RATE)).round() as usize];
    let mut done = 0;
    for (time, bytes) in events {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        synth8.render(&mut samples[done..until]);
        done = until;
        if let &[status, first, second] = bytes.as_slice() {
            synth8.event(status, first, second);
        }
    }
    synth8.render(&mut samples[done..]);

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
