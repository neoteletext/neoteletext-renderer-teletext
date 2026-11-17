//! synth53's filter against the technical manuals: where it is
//! tuned while it oscillates, the cutoff's semitones, the keyboard
//! switch, the onset of oscillation on the resonance knob, the slope,
//! and how far the filter envelope opens it.

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

use neoteletext_audio_instruments::synth53::patch::dial;
use neoteletext_audio_instruments::synth53::{Knob, Model, Patch, Switch};
use synth53_support::{MODELS, cents, pitch, plain, play, rms, samples, whistle};

/// The pitch a patch's filter oscillates at with a key held.
fn whistled(model: Model, patch: &Patch, key: u8) -> f64 {
    let signal = play(model, patch, key, 5.0, 0.0);
    pitch(&signal[samples(4.0)..])
}

/// How far a voice's filter may stand from where it is tuned: the
/// model's own tolerance and three cents of the measurement and of the
/// oscillation's own flatness.
fn allowed(model: Model) -> f64 {
    3.0 + match model {
        Model::Synth53 => 10.0,
        Model::Synth52 | Model::Synth51 => 15.0,
    }
}

#[test]
fn the_oscillating_filter_is_where_the_factory_tunes_it() {
    // TM3 adjustment 4-20: cutoff voltage 2.000 V (24 steps), A3 and A4
    // tuned "for 440- and 880-Hz". TM2 adjustment 4-17: 3.083 V (37
    // steps), C3 "to resonate at A-440", C5 "exactly two octaves higher".
    // The keyboard's lowest C is MIDI's 36.
    for (model, steps, keys) in
        [(Model::Synth53, 24, [(81u8, 440.0), (93, 880.0)]), (Model::Synth52, 37, [(72, 440.0), (96, 1760.0)])]
    {
        for (key, expected) in keys {
            let found = whistled(model, &whistle(model, steps), key);
            let error = cents(expected, found);
            println!("{model:?}: cutoff {steps} steps, key {key}: {found:.2} Hz for {expected} ({error:+.1} cents)");
            assert!(error.abs() < allowed(model), "{model:?} key {key}: {error}");
        }
    }
}

#[test]
fn a_step_of_the_cutoff_is_a_semitone_and_the_keyboard_switch_a_volt_to_the_octave() {
    // TM3 section 2-6: "Its scale matches the VCO scale of 1V/OCT, so is
    // also adjusted in semitones by 83-mV DAC steps."
    for model in MODELS {
        let low = whistled(model, &whistle(model, 36), 60);
        let high = whistled(model, &whistle(model, 60), 60);
        let octaves = cents(low, high);
        let above = cents(low, whistled(model, &whistle(model, 36), 72));
        let off = whistle(model, 60).with_switch(Switch::FilterKeyboard, false);
        let fixed = cents(whistled(model, &off, 48), whistled(model, &off, 84));
        println!(
            "{model:?}: 24 steps are {octaves:.1} cents; an octave of keys {above:.1}; off the keyboard {fixed:.2}"
        );
        assert!((octaves - 2400.0).abs() < 4.0 && (above - 1200.0).abs() < 4.0 && fixed.abs() < 0.5);
    }
}

#[test]
fn the_filter_oscillates_from_eight_on_the_resonance_knob() {
    // OM section 2-6: "beyond 7, the filter breaks into oscillation"; TM3
    // test table: "BETWEEN 7 AND 9-1/2 ON DIAL".
    for model in MODELS {
        let level = |tenths: u8| {
            let patch = whistle(model, 48).with(Knob::Resonance, dial(tenths));
            let signal = play(model, &patch, 60, 3.0, 0.0);
            20.0 * rms(&signal[samples(2.0)..]).max(1.0e-9).log10()
        };
        let (below, at, above, top) = (level(70), level(78), level(85), level(100));
        println!("{model:?}: resonance 7.0 {below:.1} dB, 7.8 {at:.1} dB, 8.5 {above:.1} dB, 10 {top:.1} dB");
        assert!(below < -70.0 && at < -60.0 && above > -40.0 && top > above, "{model:?}");
    }
}

#[test]
fn the_filter_falls_as_four_poles_do() {
    // OM section 2-6: "the 24 dB/octave (4-pole) low-pass filter": four
    // equal poles pass 1 / (1 + w²)² at w times the cutoff, which nears 24
    // dB an octave far above it.
    for model in MODELS {
        // Off the keyboard, the cutoff where this voice's filter
        // whistles; then a sawtooth, whose fundamental is nearly all of
        // what is left of it, at two keys an octave apart above that.
        let patch = plain(model).with(Knob::Cutoff, 60).with_switch(Switch::FilterKeyboard, false);
        let cutoff = whistled(model, &patch.with(Knob::Resonance, 127).with_switch(Switch::OscASawtooth, false), 60);
        let level = |key: u8| {
            let signal = play(model, &patch, key, 1.0, 0.0);
            (20.0 * rms(&signal[samples(0.5)..]).log10(), pitch(&signal[samples(0.5)..]))
        };
        let ((low, low_pitch), (high, high_pitch)) = (level(60), level(72));
        let gain = |pitch: f64| -40.0 * (1.0 + (pitch / cutoff).powi(2)).log10();
        let (fall, expected) = (low - high, gain(low_pitch) - gain(high_pitch));
        println!(
            "{model:?}: cutoff {cutoff:.1} Hz; from {low_pitch:.0} to {high_pitch:.0} Hz it falls {fall:.1} dB (four poles: {expected:.1})"
        );
        assert!((fall - expected).abs() < 0.5 && expected > 12.0, "{fall} for {expected}");
    }
}

#[test]
fn the_filter_envelope_at_its_top_opens_the_cutoff_nine_octaves() {
    // Own choice of the depth (the voice's ENVELOPE_CUTOFF): 9.1 octaves.
    for model in MODELS {
        let closed = whistle(model, 12).with(Knob::FilterSustain, 0);
        let half = closed.with(Knob::FilterSustain, 60).with(Knob::FilterEnvelopeAmount, 60);
        let (from, to) = (whistled(model, &closed, 60), whistled(model, &half, 60));
        let moved = cents(from, to) / 1200.0;
        println!("{model:?}: half the envelope at half the amount opens {moved:.2} octaves (a quarter of 9.11)");
        assert!((moved - 9.107 / 4.0).abs() < 0.02, "{moved}");
    }
}
