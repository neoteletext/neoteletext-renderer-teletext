//! The Jupiter-8's patch record: every value of every byte is a patch or
//! is refused, and what is read is what was written (TODO-6435).

// Without the feature there is nothing here to test.
#![cfg(feature = "jupiter8")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

#[path = "jupiter8_factory.rs"]
mod factory;

use neoteletext_audio_instruments::jupiter8::{
    AssignMode, Jupiter8, LfoWave, PATCH_BYTES, Param, Patch, PatchError, PwmSource, Vco1Wave, Vco2Wave,
};

/// The largest value of each byte: Roland's parameter table gives each
/// control's values, and the module's documentation the bytes.
const MAXIMA: [u8; PATCH_BYTES] = [
    255, 255, 3, 255, 255, 1, 1, 255, 2, 255, 3, 3, 1, 1, 36, 255, 3, 255, 255, 255, 255, 255, 1, 255, 1, 255, 255,
    255, 255, 255, 255, 255, 255, 1, 255, 255, 255, 255, 3, 1, 1, 1, 1,
];

#[test]
fn every_value_of_every_byte_is_a_patch_or_is_refused() {
    let base = Patch::DEFAULT.to_bytes();
    let mut jupiter = Jupiter8::new();
    let mut out = [0i16; 64];
    for place in 0..PATCH_BYTES {
        for value in 0..=255u8 {
            let mut bytes = base;
            bytes[place] = value;
            match Patch::from_bytes(&bytes) {
                Ok(patch) => {
                    assert!(value <= MAXIMA[place], "byte {place} took {value}");
                    assert_eq!(patch.to_bytes(), bytes);
                    // And the instrument plays it.
                    jupiter.set_patch(&patch);
                    jupiter.event(0x90, 60, 100);
                    jupiter.render(&mut out);
                }
                Err(error) => {
                    assert!(value > MAXIMA[place], "byte {place} refused {value}");
                    assert_eq!(error, PatchError::Value(place as u8));
                }
            }
        }
    }
    // The first byte that is wrong is the one named.
    let mut bytes = base;
    bytes[22] = 2;
    bytes[38] = 9;
    assert_eq!(Patch::from_bytes(&bytes), Err(PatchError::Value(22)));
}

#[test]
fn a_control_is_held_to_its_range_and_read_as_it_was_set() {
    let mut patch = Patch::DEFAULT;
    patch.set(Param::Vco2Range, 200);
    patch.set(Param::VcfSlope, 7);
    assert_eq!((patch.get(Param::Vco2Range), patch.get(Param::VcfSlope)), (36, 1));
    assert_eq!((Patch::max(Param::PwmSource), Patch::max(Param::LfoRate)), (2, 255));
    let patch = Patch::DEFAULT
        .with(Param::LfoWave, 1)
        .with(Param::Vco1Wave, 0)
        .with(Param::Vco2Wave, 3)
        .with(Param::PwmSource, 2)
        .with(Param::AssignMode, 2)
        .with(Param::FineTune, 0);
    assert_eq!(
        (patch.lfo_wave(), patch.vco1_wave(), patch.vco2_wave(), patch.pwm_source(), patch.assign_mode()),
        (LfoWave::Sawtooth, Vco1Wave::Triangle, Vco2Wave::Noise, PwmSource::Envelope, AssignMode::Unison)
    );
    assert_eq!((patch.signed(Param::FineTune), Patch::DEFAULT.signed(Param::VcfEnvMod)), (-128, 0));
    assert!(patch.on(Param::Vco1Bend) && !patch.on(Param::Sync));
    // The original's scale of 0 to 10: a signed control from none to its
    // most, any other from 0 to 255.
    let scaled = Patch::DEFAULT.at(Param::VcfEnvMod, 100).at(Param::VcfCutoff, 50).at(Param::VcoLfoMod, 0);
    assert_eq!(
        (scaled.get(Param::VcfEnvMod), scaled.get(Param::VcfCutoff), scaled.get(Param::VcoLfoMod)),
        (255, 128, 128)
    );
    assert_eq!(Patch::default(), Patch::DEFAULT);
}

#[test]
fn the_source_mixer_is_one_knob() {
    // Owner's manual, printed page 18: VCO-1 alone at one end, "an equal
    // mix at the 12 o'clock position", VCO-2 alone at the other.
    let mix = |knob: u8| {
        let mut patch = Patch::DEFAULT;
        patch.set_source_mix(knob);
        (patch.get(Param::MixVco1), patch.get(Param::MixVco2))
    };
    assert_eq!((mix(0), mix(128), mix(255), mix(64)), ((255, 0), (254, 255), (0, 255), (255, 128)));
}

#[test]
fn the_factory_patches_are_patches_and_sound() {
    for (number, name, patch) in factory::all() {
        assert_eq!(Patch::from_bytes(&patch.to_bytes()), Ok(patch), "{number} {name}");
        let mut jupiter = Jupiter8::new();
        jupiter.set_patch(&patch);
        for key in [48, 55, 64] {
            jupiter.event(0x90, key, 100);
        }
        let mut out = vec![0i16; 48_000];
        jupiter.render(&mut out);
        let peak = out.iter().map(|sample| sample.unsigned_abs()).max().unwrap_or(0);
        let clipped = out.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
        println!("{number} {name}: peak {peak} of 32768, {clipped} samples at a rail");
        assert!(peak > 1_000 && clipped == 0, "{number} {name}");
    }
    // What each sheet names under its panel: the two patches in unison,
    // and the one whose bender reaches VCO-2 alone.
    let numbers = |wanted: &dyn Fn(&Patch) -> bool| -> Vec<u8> {
        factory::all().iter().filter(|(_, _, patch)| wanted(patch)).map(|&(number, _, _)| number).collect()
    };
    assert_eq!(numbers(&|patch| patch.get(Param::AssignMode) == 2), [14, 58]);
    assert_eq!(numbers(&|patch| patch.get(Param::AssignMode) != 0 && patch.get(Param::AssignMode) != 2), []);
    assert_eq!(numbers(&|patch| !patch.on(Param::Vco1Bend) && patch.on(Param::Vco2Bend)), [14]);
}
