//! synth53's panel and how a program of the adopted interface sets it
//! (`neoteletext-common/docs/audio/synth53-synth52-synth51.md`, S53-30 and
//! S53-35 to S53-37).

#![cfg(feature = "synth53")]
#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects, reason = "a test")]

use neoteletext_audio_instruments::synth53::interface::{
    KNOB_TOP, PROGRAM_BYTES, Program, Variant, maximum, parameter,
};
use neoteletext_audio_instruments::synth53::patch::{
    KNOB_MAX, KNOB_PARAMETERS, SWITCH_PARAMETERS, dial, knob_of_value, value_of_knob,
};
use neoteletext_audio_instruments::synth53::{Knob, Model, Patch, Switch};

#[test]
fn setting_keeps_each_knob_in_its_seven_bits_and_each_switch_in_its_one() {
    let mut patch = Patch::DEFAULT;
    for ((knob, _), switch) in KNOB_PARAMETERS
        .into_iter()
        .zip(SWITCH_PARAMETERS.map(|(switch, _)| Some(switch)).into_iter().chain([None, None]))
    {
        let before = patch;
        patch.set(knob, 200);
        assert_eq!(patch.get(knob), KNOB_MAX);
        patch.set(knob, 5);
        if let Some(switch) = switch {
            let was = patch.on(switch);
            patch.switch(switch, !was);
            assert_eq!(patch.on(switch), !was);
            patch.switch(switch, was);
        }
        assert_eq!(patch.with(knob, before.get(knob)), before);
        patch = before;
    }
    assert_eq!(Patch::default(), Patch::DEFAULT);
    assert!(Patch::DEFAULT.with_switch(Switch::Sync, true).on(Switch::Sync));
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
    assert!(core::ptr::eq(Model::Synth51.law(), Model::Synth52.law()));
    assert!(!core::ptr::eq(Model::Synth53.law(), Model::Synth52.law()));
}

/// The panel of a program of zeros but `number` at `value`.
fn alone(number: u8, value: u8, model: Model) -> Patch {
    let mut bytes = [0; PROGRAM_BYTES];
    bytes[usize::from(number)] = value;
    Patch::of_program(&Program::from_bytes(bytes), model)
}

// Verifies S53-37: each of the 46 fields is set by its parameter, and by
// nothing else.
#[test]
fn s53_37_each_field_is_set_by_exactly_its_parameter() {
    for model in [Model::Synth53, Model::Synth52] {
        let zero = alone(0, 0, model);
        for (knob, number) in KNOB_PARAMETERS {
            let patch = alone(number, 60, model);
            assert_ne!(patch.get(knob), 0, "{knob:?}");
            assert_eq!(patch.with(knob, 0), zero, "{knob:?} alone");
        }
        for (switch, number) in SWITCH_PARAMETERS {
            let patch = alone(number, 1, model);
            assert!(patch.on(switch), "{switch:?}");
            assert_eq!(patch.with_switch(switch, false), zero, "{switch:?} alone");
        }
        // FILTER KEYBOARD TRACK: off at 0, on at half and full (S53-37,
        // S53-41).
        for (value, on) in [(0, false), (1, true), (2, true)] {
            assert_eq!(alone(parameter::FILTER_KEYBOARD_TRACK, value, model).on(Switch::FilterKeyboard), on);
        }
        // Every other parameter sets no field.
        for number in 0..=parameter::RETRIGGER_AND_UNISON {
            let named = KNOB_PARAMETERS.iter().any(|&(_, found)| found == number)
                || SWITCH_PARAMETERS.iter().any(|&(_, found)| found == number);
            if !named {
                assert_eq!(alone(number, 1, model), zero, "parameter {number}");
            }
        }
    }
}

// Verifies S53-35: on synth53 a knob parameter is the panel's number.
#[test]
fn s53_35_a_knob_on_synth53_is_its_value() {
    for (knob, number) in KNOB_PARAMETERS {
        for value in [0, 60, 120] {
            assert_eq!(alone(number, value, Model::Synth53).get(knob), value, "{knob:?}");
        }
    }
    assert_eq!(alone(parameter::OSC_B_FINE_TUNE, 127, Model::Synth53).get(Knob::OscBFine), 127);
}

// Verifies S53-36: on synth52 and synth51 a knob of 0 to 120 is scaled to
// 0 to 127, never decreasing; the two parameters of 0 to 127 are not.
#[test]
fn s53_36_a_knob_on_synth52_is_scaled_to_the_converters_127() {
    let mut last = 0;
    for value in 0..=KNOB_TOP {
        let field = knob_of_value(value, KNOB_TOP, true);
        assert_eq!(u32::from(field), (u32::from(value) * 127 + 60) / 120);
        assert!(field >= last);
        last = field;
        // The writer's inverse gives back the same field.
        assert_eq!(knob_of_value(value_of_knob(field), KNOB_TOP, true), field);
    }
    assert_eq!([0, 60, 120].map(|value| knob_of_value(value, KNOB_TOP, true)), [0, 64, 127]);
    for model in [Model::Synth52, Model::Synth51] {
        assert_eq!(alone(parameter::CUTOFF, 120, model).get(Knob::Cutoff), 127);
        assert_eq!(alone(parameter::OSC_B_FINE_TUNE, 100, model).get(Knob::OscBFine), 100);
        assert_eq!(alone(parameter::POLY_MOD_FILT_ENV_AMOUNT, 100, model).get(Knob::PolyModFilterEnvelope), 100);
    }
    assert_eq!(maximum(u16::from(parameter::OSC_B_FINE_TUNE)), Some(127));
}

// Verifies S53-30 on synth53: the default program's panel is today's
// default patch, each knob as the converter plays it (120 and above are
// the dial's 10).
#[test]
fn s53_30_the_default_program_is_the_default_patch_on_synth53() {
    let panel = Patch::of_program(&Program::default_for(Variant::Synth53), Model::Synth53);
    for (knob, _) in KNOB_PARAMETERS {
        assert_eq!(panel.get(knob).min(KNOB_TOP), Patch::DEFAULT.get(knob).min(KNOB_TOP), "{knob:?}");
    }
    for (switch, _) in SWITCH_PARAMETERS {
        assert_eq!(panel.on(switch), Patch::DEFAULT.on(switch), "{switch:?}");
    }
}
