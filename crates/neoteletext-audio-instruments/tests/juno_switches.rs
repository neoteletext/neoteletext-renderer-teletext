//! The Juno while its panel moves: a switch or a whole patch changed
//! under a note that sounds or dies away, and what a voice keeps from
//! one note to the next (TODO-6438).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

#[path = "juno_support.rs"]
mod support;

use neoteletext_audio_instruments::juno::{Patch, Slider, VOICES};
use support::{MODELS, NOTE_OFF, NOTE_ON, flat, instrument, render, rms};

#[test]
fn a_voice_on_the_gate_starts_its_next_note_with_its_envelope_at_rest() {
    for model in MODELS {
        // The amplifier on the gate, the envelope opening a nearly closed
        // filter over a second, and a release of a fifth of a second: a
        // note is silent a few thousandths of a second after its key, and
        // its envelope at rest long before its voice's turn comes again.
        let mut patch = flat(model, Patch::DEFAULT)
            .with(Slider::VcfFrequency, 30)
            .with(Slider::VcfEnvelope, 100)
            .with(Slider::Attack, 90)
            .with(Slider::Release, 32);
        patch.vca_gate = true;
        let mut juno = instrument(model, &patch);
        let mut heads = Vec::new();
        for _ in 0..=2 * VOICES {
            juno.event(NOTE_ON, 48, 100);
            heads.push(rms(&render(&mut juno, 0.05)));
            render(&mut juno, 1.0);
            juno.event(NOTE_OFF, 48, 0);
            render(&mut juno, 1.0);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: the envelope has come to rest");
        }
        println!("{model:?}: the first 50 ms of each note: {heads:.4?}");
        // Each voice's second note begins as dull as its first, where one
        // that kept its envelope begins some thirty times louder. The
        // two are not alike to the sample: a voice's first note of all
        // starts its oscillator and its filter from nothing.
        for (index, head) in heads.iter().enumerate() {
            let ratio = head / heads[0];
            assert!((0.5..2.0).contains(&ratio), "{model:?}: note {index} begins at {head}, the first at {}", heads[0]);
        }
    }
}
