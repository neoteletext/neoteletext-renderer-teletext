//! synth53's pitch: a volt to the octave from the lowest key's C0,
//! the frequency knobs' semitones, the fine tuning, the pitch wheel and
//! the master tune, against the technical manuals' figures.

#![cfg(feature = "synth53")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod synth53_support;

use neoteletext_audio_instruments::synth53::{Knob, Model, Patch, Switch, Synth53};
use synth53_support::{CONTROL, MODELS, NOTE_ON, PITCH_BEND, cents, instrument, pitch, plain, render};

/// The frequency of a MIDI key in equal temperament from an A of 440 Hz.
fn equal(key: f64) -> f64 {
    440.0 * ((key - 69.0) / 12.0).exp2()
}

/// The pitch of a key of a patch, after the note has settled.
fn sounded(synth53: &mut Synth53, key: u8) -> f64 {
    synth53.event(NOTE_ON, key, 100);
    render(synth53, 0.2);
    pitch(&render(synth53, 1.0))
}

/// How far a voice's oscillator may stand from the note: the model's own
/// tolerance, and the cent asked of the pitch itself.
fn allowed(model: Model) -> f64 {
    1.0 + match model {
        Model::Synth53 => 1.5,
        Model::Synth52 | Model::Synth51 => 3.0,
    }
}

#[test]
fn a_key_is_its_note_within_the_oscillators_tolerance_and_a_cent() {
    for model in MODELS {
        // One voice after another takes the keys, each with its own
        // tuning; all five are heard.
        for key in [36u8, 45, 57, 60, 69, 81, 84, 96] {
            let mut synth53 = instrument(model, &plain(model));
            let found = sounded(&mut synth53, key);
            let error = cents(equal(f64::from(key)), found);
            println!("{model:?} key {key}: {found:.3} Hz, {error:+.2} cents");
            assert!(error.abs() < allowed(model), "{model:?} key {key}: {error} cents");
        }
    }
}

#[test]
fn the_five_voices_average_the_note_to_a_cent_and_two_voices_an_octave_apart_are_an_octave() {
    for model in MODELS {
        // Voice 1 plays every first key: the octave between two keys on
        // one voice is exact, whatever its tuning.
        let mut low = instrument(model, &plain(model));
        let mut high = instrument(model, &plain(model));
        let octave = cents(sounded(&mut low, 57), sounded(&mut high, 69));
        println!("{model:?}: an octave is {octave:.3} cents");
        assert!((octave - 1200.0).abs() < 1.0, "{octave}");
    }
}

#[test]
fn the_lowest_key_with_the_knob_at_zero_is_c0_and_the_knob_counts_semitones() {
    // TM3 section 2-4: nine octaves from 0-5 V of keys and 0-4 V of knob;
    // TM2 adjustment 4-18: 39 semitones up, the lowest key is 155.5 Hz.
    for model in MODELS {
        let top: u8 = if model == Model::Synth53 { 48 } else { 60 };
        for semitones in [0u8, 12, 39, top] {
            let patch = Patch::DEFAULT.with(Knob::OscAFrequency, model.frequency_knob(semitones));
            let mut synth53 = instrument(model, &patch);
            // The top key, which is 60 semitones above the lowest.
            let found = sounded(&mut synth53, 96);
            let expected = equal(12.0 + 60.0 + f64::from(semitones));
            let error = cents(expected, found);
            println!("{model:?} knob {semitones} semitones: {found:.2} Hz for {expected:.2}, {error:+.2} cents");
            assert!(error.abs() < allowed(model), "{model:?} {semitones}: {error}");
        }
        // The knob's top is its range and no more.
        let patch = Patch::DEFAULT.with(Knob::OscAFrequency, 127);
        let found = sounded(&mut instrument(model, &patch), 60);
        assert!(cents(equal(f64::from(60 - 24 + top)), found).abs() < allowed(model));
    }
    // Revision 2's own figure: 39 semitones up, the lowest key.
    let patch = Patch::DEFAULT.with(Knob::OscAFrequency, Model::Synth52.frequency_knob(39));
    let found = sounded(&mut instrument(Model::Synth52, &patch), 36);
    println!("revision 2, 39 semitones up, lowest key: {found:.2} Hz (the manual: about 155.5)");
    assert!((found - 155.5).abs() < 0.5, "{found}");
}

/// Oscillator B alone.
fn second(model: Model) -> Patch {
    plain(model)
        .with(Knob::MixerOscA, 0)
        .with(Knob::MixerOscB, 127)
        .with_switch(Switch::OscASawtooth, false)
        .with_switch(Switch::OscBSawtooth, true)
}

#[test]
fn the_fine_tuning_raises_oscillator_b_by_a_semitone_at_most() {
    for (model, top) in [(Model::Synth53, 100.0), (Model::Synth52, 127.0)] {
        let base = sounded(&mut instrument(model, &second(model)), 69);
        let raised = sounded(&mut instrument(model, &second(model).with(Knob::OscBFine, 127)), 69);
        let found = cents(base, raised);
        println!("{model:?}: the fine tuning's top is {found:.2} cents (the documents: {top})");
        assert!((found - top).abs() < 1.0, "{found}");
    }
}

#[test]
fn oscillator_b_off_the_keyboard_keeps_its_pitch_and_low_is_low() {
    for model in MODELS {
        let patch = second(model).with_switch(Switch::OscBKeyboard, false);
        let low = sounded(&mut instrument(model, &patch), 48);
        let high = sounded(&mut instrument(model, &patch), 84);
        println!("{model:?}: off the keyboard, {low:.2} Hz and {high:.2} Hz");
        assert!(cents(low, high).abs() < 0.1);
        // No key's voltage: the lowest key's pitch, two octaves up.
        assert!(cents(equal(36.0), low).abs() < allowed(model));
    }
    // TM3 test table: low frequency, off the keyboard, the knob from 0 to
    // 10: ".1-40 HZ".
    let slow =
        second(Model::Synth53).with_switch(Switch::OscBKeyboard, false).with_switch(Switch::OscBLowFrequency, true);
    let mut synth53 = instrument(Model::Synth53, &slow.with(Knob::OscBFrequency, 120));
    synth53.event(NOTE_ON, 60, 100);
    let top = pitch(&render(&mut synth53, 2.0));
    println!("revision 3, low frequency, knob at 10: {top:.2} Hz (the manual: 40; C0 less 7.5 octaves plus 9: 46.2)");
    assert!((top - 46.25).abs() < 0.3, "{top}");
}

#[test]
fn the_pitch_wheel_bends_a_fifth_and_the_master_tune_a_semitone() {
    for model in MODELS {
        let mut synth53 = instrument(model, &plain(model));
        let base = sounded(&mut synth53, 69);
        synth53.event(PITCH_BEND, 0x7f, 0x7f);
        render(&mut synth53, 0.1);
        let up = cents(base, pitch(&render(&mut synth53, 1.0)));
        synth53.event(PITCH_BEND, 0, 0);
        render(&mut synth53, 0.1);
        let down = cents(base, pitch(&render(&mut synth53, 1.0)));
        synth53.event(PITCH_BEND, 0, 0x40);
        synth53.set_master_tune(500);
        render(&mut synth53, 0.1);
        let tuned = cents(base, pitch(&render(&mut synth53, 1.0)));
        println!("{model:?}: wheel {up:+.2} and {down:+.2} cents, master tune {tuned:+.2}");
        assert!((up - 700.0).abs() < 1.0 && (down + 700.0).abs() < 1.0 && (tuned - 100.0).abs() < 0.5);
        // The wheel at rest and the controllers reset bend nothing.
        synth53.set_master_tune(0);
        synth53.event(PITCH_BEND, 0x7f, 0x7f);
        synth53.event(CONTROL, 121, 0);
        render(&mut synth53, 0.1);
        assert!(cents(base, pitch(&render(&mut synth53, 1.0))).abs() < 0.1);
    }
}

#[test]
fn sync_holds_oscillator_a_to_oscillator_bs_pitch() {
    // OM section 2-3: A "will therefore tune only to harmonic
    // frequencies of OSC B".
    for model in MODELS {
        // The filter is closed to just above the note, so that the
        // period is read and not the harmonic the synchronisation
        // makes strong.
        let patch = plain(model)
            .with(Knob::OscAFrequency, model.frequency_knob(31))
            .with(Knob::Cutoff, 40)
            .with_switch(Switch::Sync, true);
        let found = sounded(&mut instrument(model, &patch), 57);
        let error = cents(equal(57.0), found);
        println!("{model:?}: synchronised, a fifth above, sounds {found:.2} Hz ({error:+.2} cents from B)");
        assert!(error.abs() < allowed(model), "{error}");
    }
}
