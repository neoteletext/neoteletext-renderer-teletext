//! Renders a short piece on synth106 or synth60 to a WAV file,
//! for listening:
//!
//! `cargo run --release -p neoteletext-audio-instruments --features synth106 --example synth106_demo -- 106 out.wav`
//!
//! The first argument is the model, `106` or `60`. The file is 48 kHz, 16
//! bits, mono, about 16 seconds. The piece is written here: a pad of
//! four chords through chorus I with a bass under it, the pad's cutoff
//! swept open by hand over the last two chords; then the same chords as
//! a plucked arpeggio whose filter the envelope opens, through chorus
//! II, over a pulse bass with its width swept, and a last chord with
//! both chorus buttons down (on synth106, which has no such
//! setting, chorus II). The cutoffs are given in hertz and each model's
//! slider is set from its own law, so both play the same piece. It
//! prints each half's peak and RMS level.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "an example, composing a piece in f64 on indices it derives itself"
)]

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::synth106::{ChorusMode, Model, Patch, PwmSource, Range, Slider, Synth106};

/// Seconds in a beat: 100 beats a minute.
const BEAT: f64 = 0.6;
/// Beats in a bar, and bars in each half.
const BEATS: usize = 4;
const BARS: usize = 3;
/// Seconds rendered after each half's last note, for its release.
const TAIL: f64 = 0.8;

/// The chords, as keys: A minor, F major, C major, each with a bass note.
const CHORDS: [([u8; 4], u8); BARS] = [([57, 60, 64, 69], 33), ([57, 60, 65, 69], 29), ([55, 60, 64, 67], 36)];

/// The position of a model's cutoff slider that puts the cutoff at so
/// many hertz at middle C, by the model's law (`synth106::law`): the
/// synth106's 5 Hz at 0 and 50 kHz at 127, synth60's 6.94 Hz at 0
/// and 17.2 octaves to 127.
fn slider(model: Model, hertz: f64) -> u8 {
    let position = match model {
        Model::Synth106 => 127.0 * (hertz / 5.0).log2() / 10_000.0f64.log2(),
        Model::Synth60 => 127.0 * (hertz / 6.94).log2() / 17.2,
    };
    position.round().clamp(0.0, 127.0) as u8
}

/// What happens at an instant: a MIDI message or a new patch.
enum Change {
    Midi([u8; 3]),
    Patch(Box<Patch>),
}

/// A change and its time in seconds.
struct Timed(f64, Change);

fn note(score: &mut Vec<Timed>, key: u8, start: f64, length: f64) {
    score.push(Timed(start, Change::Midi([0x90, key, 100])));
    score.push(Timed(start + length, Change::Midi([0x80, key, 0])));
}

/// The position of a model's high-pass switch at which nothing is cut
/// and nothing lifted: 1 on synth106, whose 0 lifts the bass, and 0
/// on synth60, whose 1 already cuts below 235 Hz (`synth106::law`). With
/// one position for both, synth60 played the piece without its bass.
fn flat(model: Model) -> u8 {
    Patch::default_for(model).hpf
}

/// The pad: a sawtooth and a quiet sub-oscillator, slow to rise and to
/// die, the filter half closed, chorus I.
fn pad(model: Model, cutoff: u8) -> Patch {
    let mut patch = Patch::DEFAULT
        .with(Slider::VcfFrequency, cutoff)
        .with(Slider::VcfResonance, 30)
        .with(Slider::VcfKeyboard, 80)
        .with(Slider::Sub, 50)
        .with(Slider::Attack, 70)
        .with(Slider::Decay, 80)
        .with(Slider::Sustain, 100)
        .with(Slider::Release, 75)
        .with(Slider::LfoRate, 70)
        .with(Slider::LfoDelay, 60)
        .with(Slider::DcoLfo, 8);
    patch.chorus = ChorusMode::One;
    patch.hpf = flat(model);
    patch
}

/// The pluck: a pulse whose width the oscillator sweeps and a sawtooth,
/// the filter opened by a short envelope, chorus II.
fn pluck(model: Model, chorus: ChorusMode) -> Patch {
    let mut patch = Patch::DEFAULT
        .with(Slider::DcoPwm, 90)
        .with(Slider::LfoRate, 55)
        .with(Slider::VcfFrequency, slider(model, PLUCK_CUTOFF))
        .with(Slider::VcfResonance, 70)
        .with(Slider::VcfEnvelope, 60)
        .with(Slider::VcfKeyboard, 100)
        .with(Slider::Decay, 62)
        .with(Slider::Sustain, 30)
        .with(Slider::Release, 60);
    patch.pulse = true;
    patch.pwm = PwmSource::Lfo;
    patch.chorus = chorus;
    patch.range = Range::Eight;
    patch.hpf = flat(model);
    patch
}

/// The pad's cutoff at middle C before the sweep and after it, and the
/// pluck's before its envelope opens it, in hertz.
const PAD_CUTOFF: (f64, f64) = (390.0, 14_500.0);
const PLUCK_CUTOFF: f64 = 96.0;

/// The first half: the pad and its bass, the cutoff opened by hand over
/// the last two bars.
fn first_half(model: Model) -> Vec<Timed> {
    let mut score = vec![Timed(0.0, Change::Patch(Box::new(pad(model, slider(model, PAD_CUTOFF.0)))))];
    let bar = BEAT * BEATS as f64;
    for (index, (chord, bass)) in CHORDS.iter().enumerate() {
        let start = index as f64 * bar;
        for &key in chord {
            note(&mut score, key, start, bar * 0.95);
        }
        note(&mut score, *bass, start, bar * 0.9);
    }
    // The sweep: a new patch thirty times a second, as a hand on the
    // slider would send.
    let (from, to) = (bar, bar * BARS as f64);
    let steps = ((to - from) * 30.0) as usize;
    for step in 0..steps {
        let share = step as f64 / steps as f64;
        // Octaves in proportion to the time, as a hand on the slider.
        let hertz = PAD_CUTOFF.0 * (PAD_CUTOFF.1 / PAD_CUTOFF.0).powf(share);
        score.push(Timed(from + (to - from) * share, Change::Patch(Box::new(pad(model, slider(model, hertz))))));
    }
    score
}

/// The second half: the arpeggio over held bass notes, and a last chord.
fn second_half(model: Model) -> Vec<Timed> {
    let mut score = vec![Timed(0.0, Change::Patch(Box::new(pluck(model, ChorusMode::Two))))];
    let bar = BEAT * BEATS as f64;
    for (index, (chord, bass)) in CHORDS.iter().enumerate().take(BARS - 1) {
        let start = index as f64 * bar;
        note(&mut score, *bass + 12, start, bar * 0.9);
        for step in 0..BEATS * 4 {
            let key = chord[step % chord.len()] + if step % 8 >= 4 { 12 } else { 0 };
            note(&mut score, key, start + step as f64 * BEAT / 4.0, BEAT / 5.0);
        }
    }
    let last = (BARS - 1) as f64 * bar;
    score.push(Timed(last, Change::Patch(Box::new(pluck(model, ChorusMode::Both)))));
    for &key in &CHORDS[0].0 {
        note(&mut score, key, last, bar * 0.8);
    }
    score
}

/// Renders a score on a model, with a tail.
fn render(model: Model, mut score: Vec<Timed>) -> Vec<i16> {
    score.sort_by(|a, b| a.0.total_cmp(&b.0));
    let length = BARS as f64 * BEAT * BEATS as f64 + TAIL;
    let mut synth106 = Synth106::new(model);
    let mut samples = vec![0i16; (length * f64::from(SAMPLE_RATE)) as usize];
    let mut done = 0;
    for Timed(time, change) in score {
        let until = ((time * f64::from(SAMPLE_RATE)) as usize).clamp(done, samples.len());
        synth106.render(&mut samples[done..until]);
        done = until;
        match change {
            Change::Midi([status, first, second]) => synth106.event(status, first, second),
            Change::Patch(patch) => synth106.set_patch(&patch),
        }
    }
    synth106.render(&mut samples[done..]);
    samples
}

fn levels(name: &str, samples: &[i16]) {
    let peak = samples.iter().map(|&sample| f64::from(sample).abs()).fold(0.0, f64::max) / 32_768.0;
    let power = samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / samples.len() as f64;
    let rms = power.sqrt() / 32_768.0;
    println!("{name}: peak {peak:.4} ({:.1} dBFS), RMS {rms:.4} ({:.1} dBFS)", 20.0 * peak.log10(), 20.0 * rms.log10());
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (model, path) = match arguments.as_slice() {
        [model, path] if model == "106" => (Model::Synth106, path.clone()),
        [model, path] if model == "60" => (Model::Synth60, path.clone()),
        _ => {
            eprintln!("usage: synth106_demo 106|60 OUTPUT.wav");
            std::process::exit(2);
        }
    };
    let first = render(model, first_half(model));
    let second = render(model, second_half(model));
    levels("the pad", &first);
    levels("the pluck", &second);
    let samples: Vec<i16> = first.into_iter().chain(second).collect();

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
