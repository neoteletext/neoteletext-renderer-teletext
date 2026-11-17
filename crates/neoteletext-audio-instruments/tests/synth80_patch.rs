//! synth80's patch: its 85 bytes read and checked, for every value of
//! every byte, and the presets (TODO-6434).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth80")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

use neoteletext_audio_instruments::synth80::patch::GLOBAL_BYTES;
use neoteletext_audio_instruments::synth80::presets::PRESETS;
use neoteletext_audio_instruments::synth80::{
    Control, Feet, Function, Global, PATCH_BYTES, Patch, PatchError, TONE_BYTES, Tone,
};

/// The largest value of each byte of a patch: 127 for a lever, 1 for a
/// switch, 5 for a selector of six.
fn maxima() -> [u8; PATCH_BYTES] {
    let mut maxima = [127u8; PATCH_BYTES];
    for tone in 0..2 {
        // Owner's manual, page 36: the square and the sawtooth are the
        // fourth and fifth controls of a row.
        maxima[tone * TONE_BYTES + 3] = 1;
        maxima[tone * TONE_BYTES + 4] = 1;
    }
    let globals = 2 * TONE_BYTES;
    for selector in [0, 1, 10] {
        maxima[globals + selector] = 5;
    }
    for switch in [24, 26, 28, 29, 30] {
        maxima[globals + switch] = 1;
    }
    maxima
}

#[test]
fn every_value_of_every_byte_is_a_patch_or_is_refused_by_its_place() {
    assert_eq!(PATCH_BYTES, 85);
    assert_eq!(GLOBAL_BYTES, 33);
    let maxima = maxima();
    let rest = Patch::DEFAULT.to_bytes();
    assert_eq!(Patch::from_bytes(&rest), Ok(Patch::DEFAULT));
    for place in 0..PATCH_BYTES {
        for value in 0..=u8::MAX {
            let mut bytes = rest;
            bytes[place] = value;
            match Patch::from_bytes(&bytes) {
                Ok(patch) => {
                    assert!(value <= maxima[place], "byte {place} = {value} is taken");
                    assert_eq!(patch.to_bytes(), bytes, "byte {place} = {value}");
                }
                Err(error) => {
                    assert!(value > maxima[place], "byte {place} = {value} is refused");
                    assert_eq!(error, PatchError::Value(place));
                }
            }
        }
    }
    // The first byte out of range is the one named.
    let mut bytes = rest;
    bytes[60] = 200;
    bytes[30] = 2;
    assert_eq!(Patch::from_bytes(&bytes), Err(PatchError::Value(30)));
}

#[test]
fn the_bytes_are_the_controls_in_the_panels_order() {
    let mut patch = Patch::DEFAULT;
    patch.tones[0].set(Control::PwmSpeed, 11);
    patch.tones[0].set(Control::PressureLevel, 12);
    patch.tones[1].set(Control::Lpf, 13);
    patch.tones[1].set(Control::Square, 9);
    patch.set(Global::FeetOne, Feet::Two as u8);
    patch.set(Global::SubFunction, Function::Noise as u8);
    patch.set(Global::EffectDepth, 200);
    patch.set(Global::Tremolo, 7);
    let bytes = patch.to_bytes();
    assert_eq!((bytes[0], bytes[25], bytes[26 + 8], bytes[26 + 3]), (11, 12, 13, 1));
    assert_eq!((bytes[52], bytes[62], bytes[84], bytes[82]), (5, 4, 127, 1));
    assert!(patch.tones[1].on(Control::Square) && patch.on(Global::Tremolo));
    assert_eq!(Feet::from_position(patch.get(Global::FeetOne)), Feet::Two);
    assert_eq!(Function::from_position(patch.get(Global::SubFunction)), Function::Noise);
    // A tone alone is its 26 bytes.
    let tone = patch.tones[0];
    assert_eq!(Tone::from_bytes(&tone.to_bytes()), Ok(tone));
    let mut wrong = tone.to_bytes();
    wrong[4] = 2;
    assert_eq!(Tone::from_bytes(&wrong), Err(PatchError::Value(4)));
    assert_eq!(Tone::from_levers([255; TONE_BYTES]).to_bytes()[..6], [127, 127, 127, 1, 1, 127]);
}

#[test]
fn the_presets_are_twenty_two_tones_eleven_for_each_channel() {
    // Service manual, page 1, Specifications, "Tone selectors".
    let names: Vec<&str> = PRESETS.iter().map(|&(name, _)| name).collect();
    assert_eq!(
        names,
        [
            "STRING 1",
            "STRING 3",
            "BRASS 1",
            "FLUTE",
            "ELECTRIC PIANO",
            "CLAVICHORD 1",
            "HARPSICHORD 1",
            "ORGAN 1",
            "GUITAR 1",
            "FUNKY 1",
            "FUNKY 3",
            "STRING 2",
            "STRING 4",
            "BRASS 2",
            "BRASS 3",
            "BASS",
            "CLAVICHORD 2",
            "HARPSICHORD 2",
            "ORGAN 2",
            "GUITAR 2",
            "FUNKY 2",
            "FUNKY 4",
        ]
    );
    for (name, tone) in PRESETS {
        assert_eq!(Tone::from_bytes(&tone.to_bytes()), Ok(tone), "{name}");
        // Every preset sounds: something reaches the amplifier, and its
        // level is up.
        let sounds = (tone.on(Control::Square) || tone.on(Control::Sawtooth) || tone.get(Control::Noise) > 0)
            && tone.get(Control::VcfLevel) > 0
            || tone.get(Control::Sine) > 0;
        assert!(sounds && tone.get(Control::Level) > 0, "{name}");
    }
    let patch = Patch::with_tones(PRESETS[2].1, PRESETS[13].1);
    assert_eq!((patch.tones[0], patch.get(Global::Mix)), (PRESETS[2].1, 64));
}

#[test]
fn the_panel_at_rest_has_the_overall_resonance_lever_up() {
    // Owner's manual, page 38, item 8: "Nominal setting is always with
    // the lever up for minimum additional resonance"; page 31 draws the
    // nominal panel with MIX and BRILLIANCE at their centres and
    // RESONANCE at its top. Up is the least, 0 here.
    let patch = Patch::DEFAULT;
    assert_eq!((patch.get(Global::Resonance), patch.get(Global::Brilliance)), (0, 64));
}
