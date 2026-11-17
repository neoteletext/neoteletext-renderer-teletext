//! The CS-80 as a whole: its voices, its MIDI messages, that nothing
//! makes it panic or leave its bounds, that it is the same every time,
//! and what it costs.

// Without the feature there is nothing here to test.
#![cfg(feature = "cs80")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64 and on indices it derives itself"
)]

#[path = "analog_support.rs"]
mod analog;
mod cs80_support;

use std::time::Instant;

use analog::Random;
use cs80_support::{
    CHANNEL_PRESSURE, CONTROL, KEY_PRESSURE, NOTE_OFF, NOTE_ON, PITCH_BEND, SINE, instrument, panel, render, rms,
};
use neoteletext_audio_instruments::cs80::presets::PRESETS;
use neoteletext_audio_instruments::cs80::{Control, Cs80, Global, PATCH_BYTES, Patch, VOICES};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};

/// A patch from random bytes: whatever the 85 bytes can hold.
fn random_patch(random: &mut Random) -> Patch {
    loop {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = random.word() as u8;
        }
        // Most random bytes are no patch: that they are refused, and not
        // a panic, is part of the test. One time in four each byte is
        // brought within what its place takes.
        if random.word().is_multiple_of(4) {
            let rest = Patch::DEFAULT.to_bytes();
            for (place, byte) in bytes.iter_mut().enumerate() {
                let mut probe = rest;
                probe[place] = *byte & 0x7f;
                *byte = if Patch::from_bytes(&probe).is_ok() { *byte & 0x7f } else { *byte & 1 };
            }
        }
        if let Ok(patch) = Patch::from_bytes(&bytes) {
            return patch;
        }
    }
}

/// A random message: mostly ones the instrument takes, some it ignores,
/// some with bytes no message has.
fn random_event(random: &mut Random) -> (u8, u8, u8) {
    let word = random.word();
    let [a, b, c, kind] = word.to_le_bytes();
    match kind % 16 {
        0..=5 => (NOTE_ON | (a & 15), 24 + b % 85, c & 0x7f),
        6..=8 => (NOTE_OFF | (a & 15), 24 + b % 85, c & 0x7f),
        9 => (KEY_PRESSURE, 24 + b % 85, c & 0x7f),
        10 => (CHANNEL_PRESSURE, b & 0x7f, 0),
        11 => (CONTROL, [1, 7, 64, 120, 121, 123, 10, 91][usize::from(b % 8)], c & 0x7f),
        12 => (PITCH_BEND, b & 0x7f, c & 0x7f),
        13 => (0xc0 | (a & 15), b & 0x7f, c & 0x7f),
        _ => (a, b, c),
    }
}

/// Plays random patches and messages for `blocks` blocks of 20 ms, the
/// calls to render cut at `cut` samples, and gives every sample.
fn random_run(seed: u64, blocks: usize, cut: usize) -> Vec<i16> {
    let mut random = Random(seed);
    let mut cs80 = Cs80::new();
    let mut out = vec![0i16; blocks * BLOCK_SAMPLES];
    for block in out.chunks_mut(BLOCK_SAMPLES) {
        if random.word().is_multiple_of(8) {
            cs80.set_patch(&random_patch(&mut random));
        }
        for _ in 0..random.word() % 4 {
            let (status, first, second) = random_event(&mut random);
            cs80.event(status, first, second);
        }
        for piece in block.chunks_mut(cut) {
            cs80.render(piece);
        }
        assert!(cs80.sounding_voices() <= VOICES);
    }
    out
}

#[test]
fn random_patches_and_messages_never_panic_and_the_output_stays_alive() {
    for seed in 1..=8 {
        let out = random_run(seed, 400, BLOCK_SAMPLES);
        // Not stuck at a rail, and not silent throughout. A random patch
        // may ask for more than full scale (sixteen lines, two resonances,
        // 18 dB of pressure), and what is beyond is saturated.
        let clipped = out.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
        let loud = out.iter().filter(|&&sample| sample.unsigned_abs() > 300).count();
        println!("seed {seed}: {clipped} samples at a rail and {loud} above 1% of {}", out.len());
        assert!(clipped < out.len() / 4 && loud > out.len() / 100, "seed {seed}");
    }
}

#[test]
fn the_same_input_gives_the_same_samples_however_the_calls_are_cut() {
    let whole = random_run(42, 150, BLOCK_SAMPLES);
    assert_eq!(whole, random_run(42, 150, BLOCK_SAMPLES), "two instruments differ");
    assert_eq!(whole, random_run(42, 150, 7), "calls of 7 samples differ");
    assert_ne!(whole, random_run(43, 150, BLOCK_SAMPLES));
}

#[test]
fn eight_keys_sound_and_the_ninth_takes_the_oldest_voice() {
    let mut cs80 = instrument(&panel(&SINE));
    for (count, key) in (48..56u8).enumerate() {
        cs80.event(NOTE_ON, key, 100);
        render(&mut cs80, 0.02);
        assert_eq!(cs80.sounding_voices(), count + 1);
    }
    cs80.event(NOTE_ON, 72, 100);
    render(&mut cs80, 0.3);
    assert_eq!(cs80.sounding_voices(), VOICES);
    let all = render(&mut cs80, 0.5);
    // The first key is gone, the ninth is there.
    let level = |hertz: f64| cs80_support::component(&all, hertz);
    assert!(level(130.81) < 1.0e-3 && level(523.25) > 0.1 && level(138.59) > 0.1);
    // Every key up: with the shortest release the voices are free within
    // a tenth of a second.
    for key in (48..56u8).chain([72]) {
        cs80.event(NOTE_OFF, key, 0);
    }
    render(&mut cs80, 0.1);
    assert_eq!(cs80.sounding_voices(), 0);
    assert!(render(&mut cs80, 0.1).iter().all(|&sample| sample.abs() < 1.0e-4));
}

#[test]
fn the_controllers_do_what_the_instrument_says_of_them() {
    let level = |cs80: &mut Cs80| rms(&render(cs80, 0.1));
    let mut cs80 = instrument(&panel(&SINE));
    cs80.event(NOTE_ON, 60, 100);
    render(&mut cs80, 0.1);
    // The volume: half the controller is a quarter of the level.
    let full = level(&mut cs80);
    cs80.event(CONTROL, 7, 64);
    render(&mut cs80, 0.01);
    let quarter = level(&mut cs80) / full;
    assert!((quarter - (64.0f64 / 127.0).powi(2)).abs() < 0.02, "{quarter}");
    // The foot switch does not hold a note: it lengthens its release by
    // the SUSTAIN lever, which is at its foot here.
    cs80.event(CONTROL, 64, 127);
    cs80.event(NOTE_OFF, 60, 0);
    render(&mut cs80, 0.1);
    assert_eq!(cs80.sounding_voices(), 0);
    // All notes off lets go of every key; reset all controllers puts the
    // ribbon, the wheel and the pressure back, and leaves the volume.
    cs80.event(CONTROL, 7, 127);
    cs80.event(NOTE_ON, 60, 100);
    cs80.event(PITCH_BEND, 0, 0x30);
    cs80.event(CONTROL, 1, 127);
    render(&mut cs80, 0.1);
    cs80.event(CONTROL, 121, 0);
    let plain = cs80_support::pitch(&render(&mut cs80, 1.0)[4_800..]);
    assert!(cs80_support::cents(261.63, plain).abs() < 2.0, "{plain}");
    cs80.event(CONTROL, 123, 0);
    render(&mut cs80, 0.1);
    assert_eq!(cs80.sounding_voices(), 0);
    // All sound off silences at once what a long release would hold.
    let mut long = panel(&SINE);
    long.tones[0].set(Control::Release, 127);
    cs80.set_patch(&long);
    cs80.event(NOTE_ON, 60, 100);
    render(&mut cs80, 0.1);
    cs80.event(NOTE_OFF, 60, 0);
    render(&mut cs80, 0.1);
    assert_eq!(cs80.sounding_voices(), 1);
    cs80.event(CONTROL, 120, 0);
    render(&mut cs80, 0.05);
    assert_eq!(cs80.sounding_voices(), 0);
    assert!(level(&mut cs80) < 1.0e-4);
}

#[test]
fn what_is_no_message_of_the_instrument_is_counted_and_changes_nothing() {
    let mut cs80 = Cs80::new();
    let mut same = Cs80::new();
    for (status, first, second) in [(0xc0, 5, 0), (0xf8, 0, 0), (0x40, 1, 1), (CONTROL, 10, 5)] {
        cs80.event(status, first, second);
    }
    // A data byte above 127 is no data byte.
    cs80.event(NOTE_ON, 200, 100);
    cs80.event(NOTE_ON, 60, 128);
    assert_eq!((cs80.ignored_events(), cs80.sounding_voices()), (6, 0));
    // Every channel is heard.
    cs80.event(NOTE_ON, 60, 90);
    same.event(NOTE_ON | 15, 60, 90);
    assert_eq!(render(&mut cs80, 0.2), render(&mut same, 0.2));
    cs80.reset();
    assert_eq!((cs80.ignored_events(), cs80.sounding_voices()), (0, 0));
    assert!(render(&mut cs80, 0.1).iter().all(|&sample| sample == 0.0));
}

#[test]
fn every_preset_sounds_on_its_channel() {
    for (number, (name, tone)) in PRESETS.into_iter().enumerate() {
        // Channel I's presets on channel I, channel II's on channel II.
        let second = number >= PRESETS.len() / 2;
        let mut patch = Patch::DEFAULT;
        patch.tones[usize::from(second)] = tone;
        patch.set(Global::Mix, if second { 127 } else { 0 });
        let mut cs80 = instrument(&patch);
        for key in [48u8, 60, 67] {
            cs80.event(NOTE_ON, key, 100);
        }
        let held = render(&mut cs80, 1.0);
        let (peak, level) = (held.iter().fold(0.0f64, |peak, &value| peak.max(value.abs())), rms(&held));
        println!("{name}: peak {peak:.3}, RMS {level:.4}");
        assert!(level > 0.001 && peak < 0.999, "{name}");
        cs80.release_all();
        render(&mut cs80, 12.0);
        assert_eq!(cs80.sounding_voices(), 0, "{name}");
    }
}

/// A patch that uses everything a line has, on both channels: both
/// waveforms, noise, the sine, both filters with resonance and their
/// envelope, the pulse width and the sub oscillator moving, the ring
/// modulator and the effect.
fn everything() -> Patch {
    let mut patch = Patch::DEFAULT;
    for tone in &mut patch.tones {
        for (control, value) in [
            (Control::Square, 1),
            (Control::Noise, 30),
            (Control::Sine, 60),
            (Control::PulseWidth, 60),
            (Control::Pwm, 100),
            (Control::PwmSpeed, 60),
            (Control::Hpf, 30),
            (Control::HpfResonance, 80),
            (Control::Lpf, 90),
            (Control::LpfResonance, 80),
            (Control::InitialLevel, 60),
            (Control::AttackLevel, 60),
            (Control::FilterAttack, 80),
            (Control::FilterDecay, 100),
            (Control::Decay, 90),
            (Control::Sustain, 100),
        ] {
            tone.set(control, value);
        }
    }
    for (global, value) in [
        (Global::Mix, 64),
        (Global::FeetOne, 0),
        (Global::Detune, 80),
        (Global::SubSpeed, 70),
        (Global::SubVco, 20),
        (Global::SubVcf, 40),
        (Global::SubVca, 30),
        (Global::RingModulation, 40),
        (Global::RingSpeed, 90),
        (Global::Effect, 1),
        (Global::Tremolo, 1),
        (Global::EffectSpeed, 100),
        (Global::EffectDepth, 127),
    ] {
        patch.set(global, value);
    }
    patch
}

/// Eight voices of two lines each, everything on: what a second of it
/// costs. And the same with one channel alone, and with the sine alone.
#[test]
#[ignore = "a measurement, not a check: run it by name, in a release build, for the figure"]
fn rendering_speed() {
    let mut one = everything();
    one.set(Global::Mix, 0);
    let mut sines = Patch::DEFAULT;
    sines.set(Global::Mix, 64);
    for tone in &mut sines.tones {
        tone.set(Control::VcfLevel, 0);
        tone.set(Control::Sine, 127);
    }
    for (name, patch) in
        [("sixteen lines, everything on", everything()), ("eight lines", one), ("sixteen sines", sines)]
    {
        let mut cs80 = instrument(&patch);
        for key in [36u8, 48, 55, 60, 64, 67, 72, 76] {
            cs80.event(NOTE_ON, key, 100);
        }
        cs80.event(CONTROL, 1, 64);
        let seconds = 20;
        let mut out = vec![0i16; BLOCK_SAMPLES];
        let start = Instant::now();
        for _ in 0..seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES {
            cs80.render(&mut out);
        }
        let taken = start.elapsed().as_secs_f64();
        assert_eq!(cs80.sounding_voices(), VOICES);
        println!("{name}: eight voices render {:.1} times faster than real time", seconds as f64 / taken);
    }
}
