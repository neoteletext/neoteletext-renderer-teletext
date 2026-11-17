//! One output level for every instrument: the reference note of each
//! stands at the crate's reference level, and each instrument's whole
//! polyphony of that note stays under full scale.

// Without every instrument there is nothing here to compare.
#![cfg(all(
    feature = "sid",
    feature = "dx7",
    feature = "juno",
    feature = "minimoog",
    feature = "prophet5",
    feature = "jupiter8",
    feature = "cs80"
))]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

use neoteletext_audio_instruments::sid::{ChipClock, ChipModel, Sid};
use neoteletext_audio_instruments::{
    POLYPHONY_HEADROOM_TENTHS, REFERENCE_LEVEL_TENTHS, SAMPLE_RATE, cs80, dx7, juno, jupiter8, minimoog, prophet5,
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

/// The SID: one voice, or all three, each a sawtooth at full volume
/// through no filter.
fn sid(name: &'static str, model: ChipModel) -> Player {
    /// The registers of a voice, and the volume register.
    const VOICE_REGISTERS: u8 = 7;
    const VOLUME: u8 = 0x18;
    /// The control register's sawtooth and gate.
    const SAWTOOTH_GATE: u8 = 0x21;
    let clock = ChipClock::Pal;
    let shared = std::rc::Rc::new(std::cell::RefCell::new((Sid::new(model, clock), 0u8)));
    shared.borrow_mut().0.write(VOLUME, 0x0F);
    let (keys, samples) = (shared.clone(), shared);
    Player {
        name,
        polyphony: 3,
        key_down: Box::new(move |key| {
            let (sid, used) = &mut *keys.borrow_mut();
            let hertz = 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
            // Data sheet: Fout = Fn * Fclk / 16777216.
            let register = (hertz * 16_777_216.0 / f64::from(clock.hz())).round() as u16;
            let base = *used * VOICE_REGISTERS;
            sid.write(base, register as u8);
            sid.write(base + 1, (register >> 8) as u8);
            sid.write(base + 5, 0x00);
            sid.write(base + 6, 0xF0);
            sid.write(base + 4, SAWTOOTH_GATE);
            *used += 1;
        }),
        render: Box::new(move |out| samples.borrow_mut().0.render(out)),
    }
}

/// Every instrument with its reference patch, new each time.
fn players() -> Vec<Player> {
    let juno = |name, model| {
        let mut patch = juno::Patch::DEFAULT;
        // The switch's flat position: 1 on the Juno-106, 0 on the Juno-60.
        patch.hpf = u8::from(model == juno::Model::Juno106);
        let mut instrument = juno::Juno::new(model);
        instrument.set_patch(&patch);
        midi(name, juno::VOICES, instrument, juno::Juno::event, juno::Juno::render)
    };
    let prophet = |name, model: prophet5::Model| {
        let knob = model.frequency_knob(24);
        let patch = prophet5::Patch::DEFAULT
            .with(prophet5::Knob::OscAFrequency, knob)
            .with(prophet5::Knob::OscBFrequency, knob);
        let mut instrument = prophet5::Prophet5::new(model);
        instrument.set_patch(&patch);
        midi(name, prophet5::VOICES, instrument, prophet5::Prophet5::event, prophet5::Prophet5::render)
    };
    let mut moog = minimoog::Minimoog::new();
    moog.set_drift(0);
    moog.set_patch(&minimoog::Patch::DEFAULT);
    let mut jupiter = jupiter8::Jupiter8::new();
    jupiter.set_patch(&jupiter8::Patch::DEFAULT);
    let mut yamaha = cs80::Cs80::new();
    yamaha.set_patch(&cs80::Patch::DEFAULT);
    vec![
        sid("SID 6581", ChipModel::Mos6581),
        sid("SID 8580", ChipModel::Mos8580),
        midi("DX7", dx7::rack::NOTES, dx7::Rack::new(), dx7::Rack::event, dx7::Rack::render),
        juno("Juno-106", juno::Model::Juno106),
        juno("Juno-60", juno::Model::Juno60),
        midi("Minimoog", 1, moog, minimoog::Minimoog::event, minimoog::Minimoog::render),
        prophet("Prophet-5 rev 3", prophet5::Model::Rev3),
        prophet("Prophet-5 rev 2", prophet5::Model::Rev2),
        midi("Jupiter-8", jupiter8::VOICES, jupiter, jupiter8::Jupiter8::event, jupiter8::Jupiter8::render),
        midi("CS-80", cs80::VOICES, yamaha, cs80::Cs80::event, cs80::Cs80::render),
    ]
}

/// The RMS and the peak of what a player renders after its settling,
/// in decibels of full scale.
fn levels(player: &mut Player) -> (f64, f64) {
    let mut out = vec![0i16; (SECONDS * f64::from(SAMPLE_RATE)) as usize];
    (player.render)(&mut out);
    let steady = &out[(SETTLING * f64::from(SAMPLE_RATE)) as usize..];
    let power = steady.iter().map(|&sample| (f64::from(sample) / 32_768.0).powi(2)).sum::<f64>() / steady.len() as f64;
    let peak = out.iter().map(|&sample| f64::from(sample).abs() / 32_768.0).fold(0.0, f64::max);
    (10.0 * power.log10(), 20.0 * peak.log10())
}

/// How near the reference level every reference note is, in decibels.
const NEAR: f64 = 1.0;
/// How far the 8580 stands below the 6581, in decibels: its output gain
/// is 17 to the 6581's 22, and its sawtooth is the cleaner one.
const QUIETER_CHIP: f64 = 2.2;
/// The 8580's name here.
const QUIETER_CHIP_NAME: &str = "SID 8580";

#[test]
fn every_instrument_plays_its_reference_note_at_the_reference_level() {
    let reference = f64::from(REFERENCE_LEVEL_TENTHS) / 10.0;
    for mut player in players() {
        (player.key_down)(KEY);
        let (rms, peak) = levels(&mut player);
        // The one exception: the 8580 keeps its level against the 6581.
        let expected = if player.name == QUIETER_CHIP_NAME { reference - QUIETER_CHIP } else { reference };
        println!("{}: one note {rms:.1} dBFS RMS for {expected:.1}, peak {peak:.1} dBFS", player.name);
        assert!((rms - expected).abs() < NEAR, "{}: {rms} dBFS", player.name);
    }
}

#[test]
fn every_instruments_whole_polyphony_stays_under_full_scale() {
    let ceiling = -f64::from(POLYPHONY_HEADROOM_TENTHS) / 10.0;
    for mut player in players() {
        // A cluster of semitones about the reference note, struck
        // together: the notes start in step, which is the worst they do.
        for index in 0..player.polyphony {
            (player.key_down)(KEY - 4 + index as u8);
        }
        let (rms, peak) = levels(&mut player);
        println!("{}: {} notes {rms:.1} dBFS RMS, peak {peak:.1} dBFS", player.name, player.polyphony);
        assert!(peak < ceiling, "{}: {} notes peak at {peak} dBFS", player.name, player.polyphony);
    }
}
