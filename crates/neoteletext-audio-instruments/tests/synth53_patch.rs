//! synth53's program record: the 24 bytes of the operation
//! manual's section 6-7, every value of every byte.

#![cfg(feature = "synth53")]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "a test")]

use neoteletext_audio_instruments::synth53::patch::{KNOB_MAX, PATCH_BYTES, dial};
use neoteletext_audio_instruments::synth53::{Knob, Patch, Switch};

/// The operation manual's table: each byte's switch and knob.
const TABLE: [(Option<Switch>, Knob); PATCH_BYTES] = [
    (Some(Switch::OscAPulse), Knob::FilterAttack),
    (Some(Switch::OscASawtooth), Knob::FilterDecay),
    (Some(Switch::Sync), Knob::FilterSustain),
    (Some(Switch::OscBSawtooth), Knob::FilterRelease),
    (Some(Switch::OscBTriangle), Knob::AmplifierAttack),
    (Some(Switch::OscBPulse), Knob::AmplifierDecay),
    (Some(Switch::OscBKeyboard), Knob::AmplifierSustain),
    (Some(Switch::Unison), Knob::AmplifierRelease),
    (Some(Switch::PolyModFrequencyA), Knob::Cutoff),
    (Some(Switch::PolyModPulseWidthA), Knob::FilterEnvelopeAmount),
    (Some(Switch::PolyModFilter), Knob::MixerOscB),
    (Some(Switch::LfoSawtooth), Knob::OscBPulseWidth),
    (Some(Switch::LfoTriangle), Knob::MixerOscA),
    (Some(Switch::LfoSquare), Knob::OscAPulseWidth),
    (Some(Switch::FilterKeyboard), Knob::MixerNoise),
    (Some(Switch::Release), Knob::Resonance),
    (Some(Switch::WheelFrequencyA), Knob::Glide),
    (Some(Switch::WheelFrequencyB), Knob::LfoFrequency),
    (Some(Switch::WheelPulseWidthA), Knob::SourceMix),
    (Some(Switch::WheelPulseWidthB), Knob::PolyModOscB),
    (Some(Switch::WheelFilter), Knob::PolyModFilterEnvelope),
    (Some(Switch::OscBLowFrequency), Knob::OscAFrequency),
    (None, Knob::OscBFrequency),
    (None, Knob::OscBFine),
];

#[test]
fn every_value_of_every_byte_is_the_knob_and_the_switch_of_the_manuals_table() {
    for (index, (switch, knob)) in TABLE.into_iter().enumerate() {
        for value in 0..=u8::MAX {
            let mut bytes = [0u8; PATCH_BYTES];
            bytes[index] = value;
            let patch = Patch::from_bytes(&bytes);
            assert_eq!(patch.get(knob), value & 0x7f, "byte {index} value {value}");
            // Every other knob and switch is as the other bytes say.
            for (other, (other_switch, other_knob)) in TABLE.into_iter().enumerate() {
                if other != index {
                    assert_eq!(patch.get(other_knob), 0);
                    assert!(other_switch.is_none_or(|switch| !patch.on(switch)));
                }
            }
            // What comes back is what went in, but for the two top bits
            // that hold no switch.
            let mut expected = bytes;
            match switch {
                Some(switch) => assert_eq!(patch.on(switch), value & 0x80 != 0, "byte {index} value {value}"),
                None => expected[index] &= 0x7f,
            }
            assert_eq!(patch.to_bytes(), expected);
            assert_eq!(Patch::from_bytes(&patch.to_bytes()), patch);
        }
    }
}

#[test]
fn setting_keeps_each_knob_in_its_seven_bits_and_each_switch_in_its_one() {
    let mut patch = Patch::DEFAULT;
    for (switch, knob) in TABLE {
        let before = patch;
        patch.set(knob, 200);
        assert_eq!(patch.get(knob), KNOB_MAX);
        patch.set(knob, 5);
        if let Some(switch) = switch {
            let was = patch.on(switch);
            patch.switch(switch, !was);
            assert_eq!((patch.on(switch), patch.get(knob)), (!was, 5));
            patch.switch(switch, was);
        }
        assert_eq!(patch.with(knob, before.get(knob)), before);
        patch = before;
    }
    assert_eq!(Patch::default(), Patch::DEFAULT);
    assert_eq!(Patch::DEFAULT.with_switch(Switch::Sync, true).to_bytes()[2] & 0x80, 0x80);
}

#[test]
fn a_mark_of_the_dial_is_twelve_steps() {
    // TM3 adjustment 4-14: the dial's 10 is 10.000 V, 120 steps of a
    // twelfth of a volt.
    assert_eq!((dial(0), dial(5), dial(50), dial(100), dial(255)), (0, 6, 60, 120, KNOB_MAX));
}

// Verifies DEC-6461 item 6: synth51 and synth52 are one model, so each
// name selects the same laws, and synth53 selects others.
#[test]
fn synth51_plays_the_model_of_synth52() {
    use neoteletext_audio_instruments::synth53::Model;
    assert!(core::ptr::eq(Model::Synth51.law(), Model::Synth52.law()));
    assert!(!core::ptr::eq(Model::Synth53.law(), Model::Synth52.law()));
}
