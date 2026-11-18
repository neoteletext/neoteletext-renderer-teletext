//! One output level for every instrument: the reference note of each
//! stands at the crate's reference level, and each instrument's whole
//! polyphony of that note goes through the output's limiter, which keeps
//! it under full scale and unwrapped. The limiter's own tests are beside
//! it, in `src/output.rs`.

// Without every instrument there is nothing here to compare.
#![cfg(all(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

use neoteletext_audio_instruments::output::{LIMIT_CEILING, LIMIT_KNEE};
use neoteletext_audio_instruments::synth6581::{ChipClock, Synth6581, Variant};
use neoteletext_audio_instruments::{
    REFERENCE_LEVEL_TENTHS, SAMPLE_RATE, synth7, synth8, synth53, synth80, synth106, synthd,
};

/// MIDI's note on.
const NOTE_ON: u8 = 0x90;
/// The velocity every key is struck with.
const VELOCITY: u8 = 100;
/// The reference note: the A below middle C, 220 Hz.
const KEY: u8 = 57;
/// Seconds rendered, and the seconds left out at the start while the
/// note rises and the output's coupling settles.
const SECONDS: f64 = 2.0;
const SETTLING: f64 = 1.0;

/// What renders an instrument's next samples.
type Render = Box<dyn FnMut(&mut [i16])>;

/// Something that takes keys and renders samples.
struct Player {
    name: &'static str,
    /// Keys it sounds at once.
    polyphony: usize,
    key_down: Box<dyn FnMut(u8)>,
    render: Render,
}

/// An instrument behind MIDI events, shared by its two closures.
fn midi<I: 'static>(
    name: &'static str,
    polyphony: usize,
    instrument: I,
    event: fn(&mut I, u8, u8, u8),
    render: fn(&mut I, &mut [i16]),
) -> Player {
    let shared = std::rc::Rc::new(std::cell::RefCell::new(instrument));
    let (keys, samples) = (shared.clone(), shared);
    Player {
        name,
        polyphony,
        key_down: Box::new(move |key| event(&mut keys.borrow_mut(), NOTE_ON, key, VELOCITY)),
        render: Box::new(move |out| render(&mut samples.borrow_mut(), out)),
    }
}

/// synth6581: one voice, or all three, each a sawtooth at full volume
/// through no filter.
fn synth6581(name: &'static str, model: Variant) -> Player {
    /// The registers of a voice, and the volume register.
    const VOICE_REGISTERS: u8 = 7;
    const VOLUME: u8 = 0x18;
    /// The control register's sawtooth and gate.
    const SAWTOOTH_GATE: u8 = 0x21;
    let clock = ChipClock::Pal;
    let shared = std::rc::Rc::new(std::cell::RefCell::new((Synth6581::new(model, clock), 0u8)));
    shared.borrow_mut().0.write(VOLUME, 0x0F);
    let (keys, samples) = (shared.clone(), shared);
    Player {
        name,
        polyphony: 3,
        key_down: Box::new(move |key| {
            let (synth6581, used) = &mut *keys.borrow_mut();
            let hertz = 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
            // Data sheet: Fout = Fn * Fclk / 16777216.
            let register = (hertz * 16_777_216.0 / f64::from(clock.hz())).round() as u16;
            let base = *used * VOICE_REGISTERS;
            synth6581.write(base, register as u8);
            synth6581.write(base + 1, (register >> 8) as u8);
            synth6581.write(base + 5, 0x00);
            synth6581.write(base + 6, 0xF0);
            synth6581.write(base + 4, SAWTOOTH_GATE);
            *used += 1;
        }),
        render: Box::new(move |out| samples.borrow_mut().0.render(out)),
    }
}

/// Every instrument with its reference patch, new each time.
fn players() -> Vec<Player> {
    let synth106 = |name, model| {
        let mut patch = synth106::Patch::DEFAULT;
        // The switch's flat position: 1 on synth106, 0 on synth60.
        patch.hpf = u8::from(model == synth106::Model::Synth106);
        let mut instrument = synth106::Synth106::new(model);
        instrument.set_patch(&patch);
        midi(name, synth106::VOICES, instrument, synth106::Synth106::event, synth106::Synth106::render)
    };
    let synth53 = |name, model: synth53::Model| {
        let knob = model.frequency_knob(24);
        let patch =
            synth53::Patch::DEFAULT.with(synth53::Knob::OscAFrequency, knob).with(synth53::Knob::OscBFrequency, knob);
        let mut instrument = synth53::Synth53::new(model);
        instrument.set_patch(&patch);
        midi(name, synth53::VOICES, instrument, synth53::Synth53::event, synth53::Synth53::render)
    };
    let mut one_voice = synthd::SynthD::new();
    one_voice.set_drift(0);
    one_voice.set_patch(&synthd::Patch::DEFAULT);
    let mut eight = synth8::Synth8::new();
    eight.set_patch(&synth8::Patch::DEFAULT);
    let mut touch = synth80::Synth80::new();
    touch.set_patch(&synth80::Patch::DEFAULT);
    vec![
        synth6581("synth6581", Variant::Synth6581),
        synth6581("synth8580", Variant::Synth8580),
        midi("synth7", synth7::rack::NOTES, synth7::Rack::new(), synth7::Rack::event, synth7::Rack::render),
        synth106("synth106", synth106::Model::Synth106),
        synth106("synth60", synth106::Model::Synth60),
        midi("synthD", 1, one_voice, synthd::SynthD::event, synthd::SynthD::render),
        synth53("synth53", synth53::Model::Synth53),
        synth53("synth52", synth53::Model::Synth52),
        midi(
            "synth8",
            synth8::VOICES,
            eight,
            // synth8's reader answers what it did, which this table does not want.
            |synth, status, data1, data2| {
                let _ = synth8::Synth8::event(synth, status, data1, data2);
            },
            synth8::Synth8::render,
        ),
        midi("synth80", synth80::VOICES, touch, synth80::Synth80::event, synth80::Synth80::render),
    ]
}

/// What a player renders, from the keys' going down.
fn rendered(player: &mut Player) -> Vec<i16> {
    let mut out = vec![0i16; (SECONDS * f64::from(SAMPLE_RATE)) as usize];
    (player.render)(&mut out);
    out
}

/// A level in decibels of full scale.
fn decibels(level: f64) -> f64 {
    20.0 * (level / 32_768.0).log10()
}

/// The RMS of what was rendered after its settling, and the peak of all
/// of it, in decibels of full scale.
fn levels(out: &[i16]) -> (f64, f64) {
    let steady = &out[(SETTLING * f64::from(SAMPLE_RATE)) as usize..];
    let power = steady.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>() / steady.len() as f64;
    (decibels(power.sqrt()), decibels(f64::from(peak(out))))
}

/// The largest magnitude of what was rendered.
fn peak(out: &[i16]) -> u16 {
    out.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0)
}

/// How near the reference level every reference note is, in decibels.
const NEAR: f64 = 1.0;
/// How far synth8580 stands below synth6581, in decibels: its output gain
/// is 17 to synth6581's 22, and its sawtooth is the cleaner one.
const QUIETER_CHIP: f64 = 2.2;
/// synth8580's name here.
const QUIETER_CHIP_NAME: &str = "synth8580";

#[test]
fn every_instrument_plays_its_reference_note_at_the_reference_level() {
    let reference = f64::from(REFERENCE_LEVEL_TENTHS) / 10.0;
    for mut player in players() {
        (player.key_down)(KEY);
        let out = rendered(&mut player);
        let (rms, peak) = levels(&out);
        // The one exception: synth8580 keeps its level against synth6581.
        let expected = if player.name == QUIETER_CHIP_NAME { reference - QUIETER_CHIP } else { reference };
        println!("{}: one note {rms:.1} dBFS RMS for {expected:.1}, peak {peak:.1} dBFS", player.name);
        assert!((rms - expected).abs() < NEAR, "{}: {rms} dBFS", player.name);
    }
}

/// The largest step from one sample to the next that the whole polyphony
/// may take. Sawtooths struck together all fall back in the same sample,
/// and the largest such fall measured is 17,054 (synth106's six); a
/// sample that wrapped would step by the whole of the 16 bits, 65,536,
/// less what it had passed full scale by.
const LARGEST_STEP: i32 = 30_000;

#[test]
fn every_instruments_whole_polyphony_stays_under_full_scale_and_unwrapped() {
    for mut player in players() {
        // A cluster of semitones about the reference note, struck
        // together: the notes start in step, which is the worst they do.
        for index in 0..player.polyphony {
            (player.key_down)(KEY - 4 + index as u8);
        }
        let out = rendered(&mut player);
        let (rms, peak_decibels) = levels(&out);
        let bent = out.iter().filter(|sample| sample.unsigned_abs() > LIMIT_KNEE.unsigned_abs()).count();
        let step = out.windows(2).map(|pair| (i32::from(pair[1]) - i32::from(pair[0])).abs()).max().unwrap_or(0);
        println!(
            "{}: {} notes {rms:.1} dBFS RMS, peak {peak_decibels:.1} dBFS ({}), {bent} of {} samples above the knee, largest step {step}",
            player.name,
            player.polyphony,
            peak(&out),
            out.len()
        );
        // The limiter's ceiling is the largest positive sample, so the
        // most negative sample is never made.
        assert!(peak(&out) <= LIMIT_CEILING.unsigned_abs(), "{}: peak {}", player.name, peak(&out));
        assert!(step < LARGEST_STEP, "{}: a step of {step}", player.name);
    }
}
