//! synth80's patch: its 85 bytes read and checked, for every value of
//! every byte, the controllers and defaults of `synth80.md`'s S80-33, the
//! bands of S80-14, S80-15 and S80-27, and the presets (TODO-6434).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth80")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

use neoteletext_audio_instruments::synth80::patch::{
    CONTROLLERS, GLIDE_BANDS, GLOBAL_BYTES, GlideMode, KINDS, Kind, OFF, ON, SELECTOR_BANDS, controller_of, place_of,
};
use neoteletext_audio_instruments::synth80::presets::PRESETS;
use neoteletext_audio_instruments::synth80::{
    Control, Feet, Function, Global, PATCH_BYTES, Patch, PatchError, TONE_BYTES, Tone,
};

/// S80-33's table: each byte's controller and default value, in the
/// patch's order, as the document prints them.
const TABLE: [(u8, u8); PATCH_BYTES] = [
    (40, 0),
    (41, 0),
    (42, 0),
    (43, 0),
    (44, 127),
    (45, 0),
    (46, 0),
    (47, 0),
    (48, 127),
    (49, 0),
    (50, 0),
    (51, 0),
    (52, 0),
    (53, 0),
    (54, 0),
    (55, 127),
    (56, 0),
    (57, 0),
    (58, 0),
    (59, 127),
    (60, 0),
    (61, 127),
    (62, 0),
    (63, 0),
    (65, 0),
    (66, 0),
    (67, 0),
    (68, 0),
    (69, 0),
    (70, 0),
    (71, 127),
    (72, 0),
    (73, 0),
    (119, 0),
    (75, 127),
    (76, 0),
    (77, 0),
    (78, 0),
    (79, 0),
    (80, 0),
    (81, 0),
    (82, 127),
    (83, 0),
    (84, 0),
    (85, 0),
    (86, 127),
    (87, 0),
    (88, 127),
    (89, 0),
    (90, 0),
    (91, 0),
    (92, 0),
    (102, 22),
    (103, 22),
    (95, 64),
    (8, 0),
    (109, 64),
    (110, 0),
    (115, 64),
    (116, 64),
    (117, 64),
    (118, 64),
    (104, 0),
    (105, 0),
    (106, 0),
    (107, 0),
    (108, 0),
    (111, 0),
    (112, 0),
    (113, 0),
    (114, 0),
    (20, 0),
    (21, 0),
    (22, 0),
    (23, 0),
    (24, 0),
    (25, 0),
    (26, 0),
    (3, 43),
    (5, 0),
    (27, 0),
    (28, 0),
    (29, 0),
    (30, 0),
    (31, 0),
];

#[test]
fn s80_33_and_s80_45_the_patch_is_the_documents_85_controllers_and_defaults() {
    assert_eq!((PATCH_BYTES, TONE_BYTES, GLOBAL_BYTES), (85, 26, 33));
    let defaults = Patch::DEFAULT.to_bytes();
    for (place, &(controller, default)) in TABLE.iter().enumerate() {
        assert_eq!(CONTROLLERS[place], controller, "byte {place}'s controller");
        assert_eq!(controller_of(place), Some(controller));
        assert_eq!(place_of(controller), Some(place));
        assert_eq!(defaults[place], default, "byte {place}'s default");
    }
    // S80-10, S80-11, S80-18, S80-20: no other controller moves a byte;
    // controller 120 is all sound off and GLIDE MODE is on 3.
    let moved: Vec<u8> = (0..=127u8).filter(|&number| place_of(number).is_some()).collect();
    let mut listed: Vec<u8> = TABLE.iter().map(|&(controller, _)| controller).collect();
    listed.sort_unstable();
    assert_eq!(moved, listed);
    assert_eq!((place_of(120), place_of(3)), (None, Some(78)));
    assert_eq!(controller_of(PATCH_BYTES), None);
}

#[test]
fn s80_33_every_value_of_every_byte_is_a_patch_and_only_a_data_byte() {
    let rest = Patch::DEFAULT.to_bytes();
    assert_eq!(Patch::from_bytes(&rest), Ok(Patch::DEFAULT));
    for place in 0..PATCH_BYTES {
        for value in 0..=u8::MAX {
            let mut bytes = rest;
            bytes[place] = value;
            match Patch::from_bytes(&bytes) {
                Ok(patch) => {
                    assert!(value <= 127, "byte {place} = {value} is taken");
                    assert_eq!(patch.to_bytes(), bytes, "byte {place} = {value}");
                    assert_eq!(patch.byte(place), value);
                }
                Err(error) => {
                    assert!(value > 127, "byte {place} = {value} is refused");
                    assert_eq!(error, PatchError::Value(place));
                }
            }
        }
    }
    // The first byte out of range is the one named; a patch of 84 or 86
    // bytes is not a patch.
    let mut bytes = rest;
    bytes[60] = 200;
    bytes[30] = 128;
    assert_eq!(Patch::from_bytes(&bytes), Err(PatchError::Value(30)));
    assert_eq!(Patch::read(&rest[..84]), Err(PatchError::Length(84)));
    assert_eq!(Patch::read(&[0; 86]), Err(PatchError::Length(86)));
    assert_eq!(Patch::read(&rest), Ok(Patch::DEFAULT));
}

#[test]
fn s80_14_s80_15_s80_27_the_bands_of_switches_and_selectors() {
    for value in 0..=127u8 {
        let mut tone = Tone::DEFAULT;
        tone.set(Control::Square, value);
        assert_eq!(tone.on(Control::Square), value >= 65, "switch at {value}");
        let feet = Feet::from_value(value);
        let function = Function::from_value(value);
        let expected = match value {
            0..=21 => (Feet::Sixteen, Function::Sine),
            22..=42 => (Feet::Eight, Function::Sawtooth),
            43..=63 => (Feet::FiveAndThird, Function::InvertedSawtooth),
            64..=84 => (Feet::Four, Function::Square),
            85..=105 => (Feet::TwoAndTwoThirds, Function::Noise),
            _ => (Feet::Two, Function::External),
        };
        assert_eq!((feet, function), expected, "selector at {value}");
        let glide = match value {
            0..=42 => GlideMode::Portamento,
            43..=84 => GlideMode::Off,
            _ => GlideMode::Glissando,
        };
        assert_eq!(GlideMode::from_value(value), glide, "glide mode at {value}");
    }
    // The edges by name, and the canonical values.
    assert!(!Tone::from_levers([64; TONE_BYTES]).on(Control::Sawtooth));
    assert!(Tone::from_levers([65; TONE_BYTES]).on(Control::Sawtooth));
    assert_eq!((OFF, ON), (0, 127));
    assert_eq!(SELECTOR_BANDS, [0, 22, 43, 64, 85, 106]);
    assert_eq!(GLIDE_BANDS, [0, 43, 85]);
    assert_eq!([Feet::Sixteen as u8, Feet::Eight as u8, Feet::Two as u8], [0, 22, 106]);
    assert_eq!([GlideMode::Portamento as u8, GlideMode::Off as u8, GlideMode::Glissando as u8], [0, 43, 85]);
    // Which bytes are which kind: the four tone switches, the three
    // selectors, the four master switches and GLIDE MODE.
    let kinds = |kind: Kind| (0..PATCH_BYTES).filter(|&place| KINDS[place] == kind).collect::<Vec<_>>();
    assert_eq!(kinds(Kind::Switch), [3, 4, 29, 30, 76, 80, 81, 82]);
    assert_eq!(kinds(Kind::Selector), [52, 53, 62]);
    assert_eq!(kinds(Kind::Glide), [78]);
}

#[test]
fn the_bytes_are_the_controls_in_the_panels_order() {
    let mut patch = Patch::DEFAULT;
    patch.tones[0].set(Control::PwmSpeed, 11);
    patch.tones[0].set(Control::PressureLevel, 12);
    patch.tones[1].set(Control::Lpf, 13);
    patch.tones[1].set(Control::Square, 99);
    patch.set(Global::FeetOne, Feet::Two as u8);
    patch.set(Global::SubFunction, Function::Noise as u8);
    patch.set(Global::EffectDepth, 200);
    patch.set(Global::Tremolo, 70);
    let bytes = patch.to_bytes();
    assert_eq!((bytes[0], bytes[25], bytes[26 + 8], bytes[26 + 3]), (11, 12, 13, 99));
    assert_eq!((bytes[52], bytes[62], bytes[84], bytes[82]), (106, 85, 127, 70));
    assert!(patch.tones[1].on(Control::Square) && patch.on(Global::Tremolo));
    assert_eq!(Feet::from_value(patch.get(Global::FeetOne)), Feet::Two);
    assert_eq!(Function::from_value(patch.get(Global::SubFunction)), Function::Noise);
    // A tone alone is its 26 bytes.
    let tone = patch.tones[0];
    assert_eq!(Tone::from_bytes(&tone.to_bytes()), Ok(tone));
    let mut wrong = tone.to_bytes();
    wrong[4] = 128;
    assert_eq!(Tone::from_bytes(&wrong), Err(PatchError::Value(4)));
    assert_eq!(Tone::from_levers([255; TONE_BYTES]).to_bytes()[..6], [127; 6]);
    // A byte set by its place is the same as by its name.
    let mut by_place = Patch::DEFAULT;
    by_place.set_byte(26 + 8, 13);
    by_place.set_byte(52 + 30, 70);
    assert_eq!((by_place.tones[1].get(Control::Lpf), by_place.get(Global::Tremolo)), (13, 70));
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
        // A switch is written at its canonical values (S80-14).
        for control in [Control::Square, Control::Sawtooth] {
            assert!([OFF, ON].contains(&tone.get(control)), "{name}");
        }
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
