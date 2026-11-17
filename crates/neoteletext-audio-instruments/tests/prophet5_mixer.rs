//! The Prophet-5's mixer: which way round each waveform enters it.

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

use neoteletext_audio_instruments::prophet5::{Knob, Model, Switch};
use prophet5_support::{amplitude, pitch, plain, play, samples};

#[test]
fn on_revision_3_the_pulse_enters_the_mixer_upside_down() {
    // TM3 sheet SD431 (page 50): of each oscillator's mixer amplifier
    // (U464, a CA3280) the sawtooth goes to the input marked "+" (R4364,
    // R4241) and the pulse and the triangle to the other (R4366; R4250,
    // R4285). A rising sawtooth has every harmonic n at 2 / (pi n), all
    // of one sign; a square that is high first has the odd ones at
    // 4 / (pi n), of the other sign. Added as they stand they leave every
    // harmonic at 2 / (pi n): the first is twice the second, 6 dB. With
    // the square upside down the odd ones are 6 / (pi n): the first is
    // six times the second, 15.6 dB.
    // The pulse width knob at 57 of its 120 steps is 49.7% of a cycle.
    let patch = plain(Model::Rev3).with_switch(Switch::OscAPulse, true).with(Knob::OscAPulseWidth, 57);
    let signal = play(Model::Rev3, &patch, 57, 1.0, 0.0);
    let steady = &signal[samples(0.5)..];
    let hertz = pitch(steady);
    let ratio = 20.0 * (amplitude(steady, hertz) / amplitude(steady, 2.0 * hertz)).log10();
    println!("the sawtooth and the pulse of {hertz:.1} Hz: the first harmonic {ratio:.1} dB above the second");
    // The pulse is not quite square, and the filter not quite flat.
    assert!((ratio - 15.6).abs() < 1.0, "{ratio}");
}
