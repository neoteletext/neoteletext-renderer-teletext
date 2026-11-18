//! synth8's pitch: every key of both oscillators in every range
//! against the scale, the fine tune, the bender, the modulations'
//! documented depths and the portamento (TODO-6435).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth8")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "synth8_support.rs"]
mod support;

use neoteletext_audio_instruments::synth8::{AssignMode, LfoWave, VcoModulator};
use neoteletext_audio_instruments::synth8::{Param, Patch, Performance, Portamento, Synth8};
use support::{
    A2, C0, C5, CONTROL, NOTE_ON, ON, PITCH_BEND, cents, instrument, pitch, play, render, samples, second, square,
    vco1_range, vco2_range,
};

/// The scale's frequency of a key, an interval in octaves away.
fn equal(key: u8, octaves: f64) -> f64 {
    440.0 * ((f64::from(key) - 69.0) / 12.0 + octaves).exp2()
}

/// The pitch a patch plays a key at, once it has settled.
fn played(patch: &Patch, key: u8) -> f64 {
    pitch(&play(patch, key, 1.2, 0.0)[samples(0.2)..])
}

#[test]
fn every_key_of_both_oscillators_is_within_a_cent_of_the_scale() {
    // Service notes, adjustment 6: A3 at 8' is twice the reference's A,
    // A1 half of it, and A3 at 2' eight times it; owner's manual, printed
    // page 9: the reference is A-440 with the master tune at its middle.
    // The oscillators' own tolerance is 0.8 cent.
    let mut worst = 0.0f64;
    for range in 0..4u8 {
        for (name, patch) in [
            ("VCO-1", square().with(Param::Vco1Range, vco1_range(range))),
            ("VCO-2", second(1).with(Param::Vco2Range, vco2_range(range * 12))),
        ] {
            for key in (C0..=C5).step_by(7).chain([C5]) {
                // The highest keys at 2' are above what a crossing count
                // of a 48 kHz signal can time.
                let wanted = equal(key, f64::from(range) - 1.0);
                if wanted > 6_000.0 {
                    continue;
                }
                let error = cents(wanted, played(&patch, key));
                assert!(error.abs() < 1.0, "{name} range {range} key {key}: {error} cents");
                worst = worst.max(error.abs());
            }
        }
    }
    println!("worst error against the scale: {worst:.3} cents");
}

#[test]
fn the_service_notes_tuning_points_are_met() {
    // Adjustment 6 against a reference of 442 Hz: 884 Hz, 221 Hz and
    // 3,536 Hz; here against 440 Hz.
    let scale = 440.0 / 442.0;
    for (key, range, hertz) in [(A2 + 12, 1u8, 884.0), (A2 - 12, 1, 221.0), (A2 + 12, 3, 3_536.0)] {
        let found = played(&square().with(Param::Vco1Range, vco1_range(range)), key);
        let error = cents(hertz * scale, found);
        println!("key {key} range {range}: {found:.2} Hz for {:.2} Hz, {error:.3} cents", hertz * scale);
        assert!(error.abs() < 1.0);
    }
}

#[test]
fn the_fine_tune_is_fifty_cents_either_way_and_the_range_moves_by_semitones() {
    let centre = played(&second(1), A2);
    let sharp = cents(centre, played(&second(1).with(Param::FineTune, 127), A2));
    let flat = cents(centre, played(&second(1).with(Param::FineTune, 0), A2));
    println!("fine tune: {flat:.2} and {sharp:.2} cents");
    assert!((sharp - 50.0).abs() < 0.5 && (flat + 50.0).abs() < 0.5);
    let fifth = cents(centre, played(&second(1).with(Param::Vco2Range, vco2_range(19)), A2));
    assert!((fifth - 700.0).abs() < 0.2, "{fifth}");
}

#[test]
fn the_bender_bends_by_its_sensitivity_and_only_the_oscillators_switched_to_it() {
    let bent = |patch: &Patch, performance: &Performance, value: u16| {
        let mut synth8 = instrument(patch);
        synth8.set_performance(performance);
        let _ = synth8.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
        let _ = synth8.event(NOTE_ON, A2, 100);
        pitch(&render(&mut synth8, 1.2)[samples(0.2)..])
    };
    let default = Performance::DEFAULT;
    let centre = bent(&square(), &default, 0x2000);
    let down = cents(centre, bent(&square(), &default, 0));
    let up = cents(centre, bent(&square(), &default, 0x3fff));
    println!("the bender at its ends: {down:.2} and {up:.2} cents");
    assert!((down + 200.0).abs() < 0.5 && (up - 200.0).abs() < 0.5);
    // Service notes, adjustment 25-1: an octave at the sensitivity's top.
    let octave = Performance { bend_vco: Performance::BEND_MAX, ..default };
    let down = cents(centre, bent(&square(), &octave, 0));
    assert!((down + 1200.0).abs() < 0.5, "{down}");
    let unbent = cents(centre, bent(&square().with(Param::Vco1Bend, 0), &octave, 0));
    assert!(unbent.abs() < 0.1, "{unbent}");
}

#[test]
fn the_envelope_moves_the_pitch_three_octaves_and_only_where_it_is_switched() {
    // Service notes, adjustment 8: ENV-1's sustain at 10, VCO MOD ENV at
    // 10: VCO-1 at 16' sounds as it did at 2' with none.
    let high = played(&square().with(Param::Vco1Range, vco1_range(3)), C0 + 9);
    let moved = played(&square().with(Param::Vco1Range, vco1_range(0)).with(Param::VcoEnvMod, ON), C0 + 9);
    let error = cents(high, moved);
    println!("16' with ENV MOD at 10 against 2': {error:.2} cents");
    assert!(error.abs() < 2.0);
    let unmoved =
        played(&square().with(Param::VcoEnvMod, ON).with(Param::VcoModulator, VcoModulator::Vco2.value()), A2);
    assert!(cents(440.0, unmoved).abs() < 1.0);
    // Upside down, it moves the pitch down as far.
    let down = played(
        &square().with(Param::Vco1Range, vco1_range(3)).with(Param::VcoEnvMod, ON).with(Param::Env1Polarity, ON),
        A2,
    );
    assert!(cents(equal(A2, -1.0), down).abs() < 2.0, "{down}");
}

#[test]
fn the_low_frequency_oscillator_swings_the_pitch_an_octave_either_way() {
    // Service notes, adjustment 22: at VCO LFO MOD 10 the highest pitch
    // is twice the note's: "LFO modulated VCO swing equals 2 oct's". A
    // square at the lowest rate holds each end for ten seconds.
    let patch =
        square().with(Param::VcoLfoMod, ON).with(Param::LfoWave, LfoWave::Square.value()).with(Param::LfoRate, 0);
    let mut synth8 = instrument(&patch);
    let _ = synth8.event(NOTE_ON, A2, 100);
    let upper = pitch(&render(&mut synth8, 9.0)[samples(0.5)..]);
    render(&mut synth8, 2.0);
    let lower = pitch(&render(&mut synth8, 8.0)[samples(0.5)..]);
    println!("the swing's ends: {upper:.2} Hz and {lower:.2} Hz");
    assert!(cents(880.0, upper).abs() < 2.0 && cents(220.0, lower).abs() < 2.0);
}

#[test]
fn the_low_range_leaves_the_keyboard() {
    // Owner's manual, printed page 16: "the frequencies produced in this
    // range are not effected by the keyboard"; own choice of 8.2 Hz at 2'.
    let patch = second(1).with(Param::LowFrequency, ON).with(Param::Vco2Range, vco2_range(36));
    let (low, high) = (played(&patch, C0), played(&patch, C5));
    println!("the low range at 2': {low:.3} Hz");
    assert!(cents(low, high).abs() < 0.1 && cents(261.626 * 4.0 / 128.0, low).abs() < 2.0);
}

#[test]
fn the_portamento_glides_as_a_capacitor_charges() {
    let mut synth8 = Synth8::new();
    // One voice, so that the glide starts from the key before.
    synth8.set_patch(&square().with(Param::AssignMode, AssignMode::Solo.value()));
    synth8.set_performance(&Performance { portamento: Portamento::On, portamento_time: 64, ..Performance::DEFAULT });
    let _ = synth8.event(NOTE_ON, A2 - 12, 100);
    render(&mut synth8, 3.0);
    let _ = synth8.event(0x80, A2 - 12, 0);
    let _ = synth8.event(NOTE_ON, A2, 100);
    // One time constant, 504 ms at a time of 64, leaves 1 / e of the
    // octave: 441 cents.
    render(&mut synth8, 0.504 - 0.05);
    let left = cents(pitch(&render(&mut synth8, 0.1)), 440.0);
    println!("after one time constant: {left:.1} cents to go");
    assert!((left - 441.0).abs() < 25.0);
    render(&mut synth8, 6.0);
    assert!(cents(pitch(&render(&mut synth8, 0.5)), 440.0).abs() < 1.0);
    // Controller 118 switches it off: the next key is there at once
    // (S8-15).
    let _ = synth8.event(CONTROL, 118, 0);
    let _ = synth8.event(0x80, A2, 0);
    let _ = synth8.event(NOTE_ON, A2 + 12, 100);
    assert!(cents(pitch(&render(&mut synth8, 0.5)[samples(0.1)..]), 880.0).abs() < 1.0);
}
