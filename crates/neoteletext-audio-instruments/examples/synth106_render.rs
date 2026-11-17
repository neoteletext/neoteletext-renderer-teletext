//! Renders a Standard MIDI File on synth106 or synth60 to a WAV
//! file, so that the same score can be played here and on a reference
//! program and the two measured (DEC-6429, "Reference harness"):
//!
//! `cargo run --release -p neoteletext-audio-instruments --features synth106 --example synth106_render -- 106 in.mid out.wav`
//!
//! The first argument is the model, `106` or `60`. The file's first
//! track is played: its note, controller and pitch bend messages, and
//! each synth106 patch message in it (system exclusive, the maker's number, function
//! 0x30 or 0x31, 18 values: owner's manual, MIDI Implementation 3.1 and
//! 3.2) sets the patch. After the files, `chorus=both`, `pwm=envelope`
//! and `sub=off` set what synth60's panel has and those 18 bytes
//! have not. The output is 48 kHz, 16 bits, mono, the length of the
//! track and `tail=SECONDS` more (1 by default).

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, reading a file it trusts in f64 and indices"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth106::patch::PATCH_BYTES;
use neoteletext_audio_instruments::synth106::{ChorusMode, Model, Patch, PwmSource, Synth106};

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
        panic!("usage: synth106_render 106|60 IN.mid OUT.wav [chorus=both] [pwm=envelope] [sub=off] [tail=SECONDS]");
    };
    let model = if model == "60" { Model::Synth60 } else { Model::Synth106 };
    let mut tail = 1.0;
    let (events, length) = read(&std::fs::read(input).expect("the MIDI file is read"));
    let mut synth106 = Synth106::new(model);
    let mut samples = vec![0i16; ((length + 10.0) * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    let adjust = |patch: &mut Patch, tail: &mut f64| {
        for option in options {
            match option.as_str() {
                "chorus=both" => patch.chorus = ChorusMode::Both,
                "pwm=envelope" => patch.pwm = PwmSource::Envelope,
                "sub=off" => patch.sub = false,
                other => *tail = other.strip_prefix("tail=").and_then(|text| text.parse().ok()).expect("an option"),
            }
        }
    };
    // Until the file sets a patch, the model's plain one, whose
    // high-pass switch is where the model is flat.
    let mut patch = Patch::default_for(model);
    adjust(&mut patch, &mut tail);
    synth106.set_patch(&patch);
    for (time, bytes) in events {
        let until = ((time * f64::from(SAMPLE_RATE)).round() as usize).clamp(done, samples.len());
        synth106.render(&mut samples[done..until]);
        done = until;
        match bytes.as_slice() {
            [0xf0, 0x41, 0x30 | 0x31, _, _, values @ .., 0xf7] if values.len() == PATCH_BYTES => {
                let values: &[u8; PATCH_BYTES] = values.try_into().expect("eighteen bytes");
                let mut patch = Patch::from_bytes(values).expect("a patch of synth106");
                adjust(&mut patch, &mut tail);
                synth106.set_patch(&patch);
            }
            &[status, first, second] => synth106.event(status, first, second),
            _ => {}
        }
    }
    // The track and its tail, however long the tail was asked to be.
    samples.resize((((length + tail) * f64::from(SAMPLE_RATE)).round() as usize).max(done), 0);
    synth106.render(&mut samples[done..]);

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
