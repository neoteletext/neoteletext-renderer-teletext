//! The Prophet-5's envelopes against the chips' data sheets and the
//! technical manuals: the attack's time and its curve towards 1.3 times
//! the peak, the decay's and the release's time constant, the sustain
//! level, and the release switch.

#![cfg(feature = "prophet5")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod prophet5_support;

use neoteletext_audio_instruments::prophet5::patch::dial;
use neoteletext_audio_instruments::prophet5::{Knob, Model, Switch};
use prophet5_support::{
    CONTROL, MODELS, NOTE_OFF, NOTE_ON, instrument, peaks, plain, play, pure, render, rms, samples,
};

/// The window the level is read in, in seconds: four cycles of the key
/// played.
const WINDOW: f64 = 0.005;
/// A high key, so that a window holds whole cycles.
const KEY: u8 = 84;

/// The level in the window that holds a time.
fn at(levels: &[f64], seconds: f64) -> f64 {
    levels[(seconds / WINDOW) as usize]
}

/// The attack's documented time and the time constant of the falls for a
/// knob's number: revision 3, one second of attack at 72 steps and four
/// decades over 120 (TM3 test table, CEM3310 data sheet); revision 2,
/// 0.9 s at 75 steps and a decade to 24 (TM2 adjustment 4-10, SSM2050
/// data sheet). An attack is ln(13 / 3) time constants on both.
fn documented(model: Model, steps: f64) -> (f64, f64) {
    let attack = match model {
        Model::Rev3 => 10f64.powf(0.4 * (steps / 12.0 - 6.0)),
        Model::Rev2 => 0.9 * 10f64.powf((steps - 75.0) / 24.0),
    };
    (attack, attack / (13.0f64 / 3.0).ln())
}

#[test]
fn the_attack_takes_its_time_along_the_chips_curve() {
    for model in MODELS {
        for steps in [48u8, 72, 84] {
            let (time, constant) = documented(model, f64::from(steps));
            let patch = pure(model).with(Knob::AmplifierAttack, steps);
            let levels = peaks(&play(model, &patch, KEY, time * 1.5, 0.0), WINDOW);
            let peak = at(&levels, time * 1.3);
            // When the level passes nine tenths of the peak, which the
            // curve does after ln(1.3 / 0.4) time constants, and from it
            // the whole attack; and where the level stands at half the
            // time: 1.3 (1 - exp(-t / constant)) of the peak.
            let passed = levels.iter().position(|&level| level > 0.9 * peak).unwrap() as f64 * WINDOW;
            let reached = passed * (13.0f64 / 3.0).ln() / (1.3f64 / 0.4).ln();
            let half = at(&levels, time / 2.0) / peak;
            let curve = 1.3 * (1.0 - (-time / 2.0 / constant).exp());
            println!(
                "{model:?} attack at {steps} steps: {reached:.3} s (the documents: {time:.3}); at half the time {half:.3} of the peak (the curve: {curve:.3})"
            );
            // A window is as near as a time is read.
            assert!((reached - time).abs() < 0.03 * time + 2.0 * WINDOW, "{model:?} {steps}: {reached} for {time}");
            assert!((half - curve).abs() < 0.02, "{model:?} {steps}: {half} for {curve}");
        }
    }
}

#[test]
fn the_decay_and_the_release_fall_by_e_in_their_time_constant() {
    for model in MODELS {
        let steps = 60u8;
        let (_, constant) = documented(model, f64::from(steps));
        // Decay to no sustain, from the peak the shortest attack reaches
        // at once.
        let decaying = pure(model).with(Knob::AmplifierSustain, 0).with(Knob::AmplifierDecay, steps);
        let levels = peaks(&play(model, &decaying, KEY, constant * 3.0, 0.0), WINDOW);
        let start = 0.05;
        let decay = at(&levels, start + constant) / at(&levels, start);
        // Release from full sustain.
        let releasing = pure(model).with(Knob::AmplifierRelease, steps);
        let levels = peaks(&play(model, &releasing, KEY, 0.2, constant * 3.0), WINDOW);
        let release = at(&levels, 0.2 + start + constant) / at(&levels, 0.2 + start);
        let expected = (-1.0f64).exp();
        println!(
            "{model:?} at {steps} steps, time constant {constant:.3} s: after it the decay is at {decay:.3} and the release at {release:.3} (1/e: {expected:.3})"
        );
        assert!((decay - expected).abs() < 0.02 && (release - expected).abs() < 0.02, "{model:?}");
    }
}

#[test]
fn the_documents_figures_of_time_are_met() {
    // TM3 test table: 6 on the dial, "APPROX 1-SEC ATTACK"; 10, "GREATER
    // THAN 20-SEC RELEASE". OM section 2-6: "setting 5 ... approximately
    // 1/2 second", "approximately 1 millisecond ... to 30 seconds". TM2
    // adjustment 4-10: 6.250 V, 75 steps, "0.9-sec ATK period".
    let rise = |model: Model, steps: u8, long: f64| {
        let patch = pure(model).with(Knob::AmplifierAttack, steps);
        let levels = peaks(&play(model, &patch, KEY, long, 0.0), WINDOW);
        let peak = levels.iter().fold(0.0f64, |peak, &level| peak.max(level));
        // From nine tenths of the peak, as above.
        let passed = levels.iter().position(|&level| level > 0.9 * peak).unwrap() as f64 * WINDOW;
        passed * (13.0f64 / 3.0).ln() / (1.3f64 / 0.4).ln()
    };
    let (six, five, least) =
        (rise(Model::Rev3, dial(60), 2.0), rise(Model::Rev3, dial(50), 1.0), rise(Model::Rev3, 0, 0.1));
    let two = rise(Model::Rev2, 75, 2.0);
    println!("revision 3: attack at 6 {six:.3} s (1), at 5 {five:.3} s (about 1/2), at 0 {least:.4} s (about 0.001)");
    println!("revision 2: attack at 6.25 V {two:.3} s (0.9)");
    assert!((six - 1.0).abs() < 0.05 && (five - 0.4).abs() < 0.03 && least < 0.01 && (two - 0.9).abs() < 0.05);

    // The longest release, heard for five seconds: it has fallen as far
    // as 27 s of time constant let it, and so sounds beyond 20 s.
    let patch = pure(Model::Rev3).with(Knob::AmplifierRelease, 127);
    let levels = peaks(&play(Model::Rev3, &patch, KEY, 0.2, 5.2), WINDOW);
    let left = at(&levels, 5.2) / at(&levels, 0.2 + 0.05);
    let constant = documented(Model::Rev3, 120.0).1;
    println!(
        "revision 3: release at 10: time constant {constant:.1} s; after 5 s {left:.3} is left ({:.3})",
        (-4.95 / constant).exp()
    );
    assert!((left - (-4.95 / constant).exp()).abs() < 0.02);
}

#[test]
fn the_sustain_knob_is_the_level_and_the_release_switch_cuts_the_release() {
    for model in MODELS {
        let full = peaks(&play(model, &pure(model), KEY, 0.5, 0.0), WINDOW);
        let half = pure(model).with(Knob::AmplifierSustain, 60);
        let levels = peaks(&play(model, &half, KEY, 0.5, 0.0), WINDOW);
        let share = at(&levels, 0.45) / at(&full, 0.45);
        println!("{model:?}: sustain at 5 volts is {share:.3} of the peak");
        assert!((share - 0.5).abs() < 0.01);

        // OM section 2-8: with the switch off "the filter and amplifier
        // release times are programmed to a minimal setting"; section 1-5:
        // the footswitch then gives the programmed release back.
        let long = pure(model).with(Knob::AmplifierRelease, 96);
        let ringing = peaks(&play(model, &long, KEY, 0.2, 0.3), WINDOW);
        let cut = long.with_switch(Switch::Release, false);
        let stopped = peaks(&play(model, &cut, KEY, 0.2, 0.3), WINDOW);
        let mut prophet = instrument(model, &cut);
        prophet.event(CONTROL, 64, 127);
        prophet.event(NOTE_ON, KEY, 100);
        render(&mut prophet, 0.2);
        prophet.event(NOTE_OFF, KEY, 0);
        // The pedal also holds the note; lifted, it lets go of it and of
        // the programmed release.
        let held = peaks(&render(&mut prophet, 0.3), WINDOW);
        prophet.event(CONTROL, 64, 0);
        let after = peaks(&render(&mut prophet, 0.1), WINDOW);
        println!(
            "{model:?}: 0.25 s after the key: release on {:.3}, off {:.5}, pedal held {:.3}, pedal lifted {:.5}",
            at(&ringing, 0.45),
            at(&stopped, 0.45),
            at(&held, 0.25),
            at(&after, 0.09)
        );
        assert!(
            at(&ringing, 0.45) > 0.01
                && at(&stopped, 0.45) < 0.001
                && at(&held, 0.25) > 0.01
                && at(&after, 0.09) < 0.001
        );
    }
}

#[test]
fn the_filter_envelope_goes_on_falling_while_its_voice_is_silent() {
    // The chip's capacitor discharges whether or not the amplifier passes
    // anything. Here the amplifier's envelope has no sustain and no
    // release, so the voice is silent as soon as the key is up, while
    // the filter envelope stands at its sustain, the peak. Three seconds
    // later, a thousand of its release's time constants, the same key
    // must find the filter as shut as the first did, and open it as
    // slowly. The first hundredth of a second is left out: there the
    // first key of all has the filter's start from nothing in it. A
    // voice that stopped where it fell silent, with the filter open,
    // gave the second key 25 times the first's level here.
    for model in MODELS {
        let patch = plain(model)
            .with(Knob::Cutoff, 20)
            .with(Knob::FilterEnvelopeAmount, 100)
            .with(Knob::FilterAttack, 72)
            .with(Knob::FilterSustain, 120)
            .with(Knob::FilterRelease, 0)
            .with(Knob::AmplifierDecay, 60)
            .with(Knob::AmplifierSustain, 0)
            .with(Knob::AmplifierRelease, 0);
        let mut prophet = instrument(model, &patch);
        let start = samples(0.01)..samples(0.05);
        prophet.event(NOTE_ON, 60, 100);
        let first = rms(&render(&mut prophet, 3.0)[start.clone()]);
        prophet.event(NOTE_OFF, 60, 0);
        render(&mut prophet, 3.0);
        assert_eq!(prophet.sounding_voices(), 0);
        prophet.event(NOTE_ON, 60, 100);
        let second = rms(&render(&mut prophet, 3.0)[start]);
        println!("{model:?}: from 10 to 50 ms of the key: {first:.5}, and of the same key again {second:.5}");
        assert!(first > 0.001 && (second / first - 1.0).abs() < 0.25, "{model:?}: {first} then {second}");
    }
}
