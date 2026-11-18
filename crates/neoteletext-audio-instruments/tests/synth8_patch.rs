//! synth8's patch: 42 control-change values in a fixed order
//! (`neoteletext-common/docs/audio/synth8.md`, S8-23), each read by the law
//! of its kind (S8-21, S8-30); the default patch (S8-22); the bank and the
//! program numbers (S8-28, S8-29); the factory patches as values.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth8")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

#[path = "synth8_factory.rs"]
mod factory;

use neoteletext_audio_instruments::synth8::interface::{
    BankError, MOST_PATCHES, build_bank, check_bank, check_patch, pack_patch, patch_number_of_program,
    program_of_patch_number,
};
use neoteletext_audio_instruments::synth8::patch::{
    canonical, fine_tune_signed, position, slider_byte, vca_lfo_mod_value, vco2_range_value,
};
use neoteletext_audio_instruments::synth8::{
    AssignMode, Kind, LfoWave, PATCH_BYTES, Param, Patch, PatchError, PwmSource, Synth8, Vco1Range, Vco1Wave, Vco2Wave,
    VcoModulator,
};

/// S8-16's table: each byte's controller, in byte order.
const CONTROLLERS: [u8; PATCH_BYTES] = [
    29, 27, 35, 26, 25, 60, 18, 19, 59, 20, 46, 87, 57, 62, 56, 61, 16, 17, 79, 3, 9, 13, 81, 84, 28, 82, 110, 109, 83,
    80, 85, 86, 106, 89, 90, 102, 103, 119, 104, 105, 107, 108,
];

#[test]
fn s8_16_every_byte_has_its_controller_and_no_controller_two_bytes() {
    for (param, &controller) in Param::ALL.iter().zip(&CONTROLLERS) {
        assert_eq!(param.controller(), controller, "{param:?}");
        assert_eq!(Param::of_controller(controller), Some(*param));
    }
    // Controller 63 moves VCO-2 RANGE as 62 does.
    assert_eq!(Param::of_controller(63), Some(Param::Vco2Range));
    let named: Vec<u8> = (0..=127).filter(|&number| Param::of_controller(number).is_some()).collect();
    let mut wanted = CONTROLLERS.to_vec();
    wanted.push(63);
    wanted.sort_unstable();
    assert_eq!(named, wanted);
}

#[test]
fn s8_21_every_value_of_every_selector_and_switch_is_in_its_band() {
    for param in Param::ALL {
        let positions = match param.kind() {
            Kind::Selector(positions) => positions,
            Kind::Switch => 2,
            Kind::Slider | Kind::Modulation | Kind::Centred => continue,
        };
        for value in 0..=127u8 {
            let patch = Patch::DEFAULT.with(param, value);
            let found = patch.position(param);
            // Each band holds the values floor(v × n ÷ 128) maps to it,
            // and its lowest value is the canonical one.
            assert_eq!(u16::from(found), u16::from(value) * u16::from(positions) / 128, "{param:?} {value}");
            assert!(canonical(found, positions) <= value, "{param:?} {value}");
            if found + 1 < positions {
                assert!(value < canonical(found + 1, positions), "{param:?} {value}");
            }
        }
        // Every canonical value returns its own position.
        for place in 0..positions {
            assert_eq!(position(canonical(place, positions), positions), place, "{param:?} {place}");
        }
    }
    // The bands of S8-21 item 4, by their edges.
    let edges = |positions: u8| (0..positions).map(|place| canonical(place, positions)).collect::<Vec<_>>();
    assert_eq!(edges(3), [0, 43, 86]);
    assert_eq!(edges(4), [0, 32, 64, 96]);
    assert_eq!(edges(6), [0, 22, 43, 64, 86, 107]);
    assert_eq!((position(42, 3), position(43, 3), position(85, 3), position(86, 3)), (0, 1, 1, 2));
    assert_eq!((position(21, 6), position(22, 6), position(106, 6), position(107, 6)), (0, 1, 4, 5));
    // A switch is on from 64 (S8-21 item 5).
    assert!(!Patch::DEFAULT.with(Param::Sync, 63).on(Param::Sync));
    assert!(Patch::DEFAULT.with(Param::Sync, 64).on(Param::Sync));
}

#[test]
fn s8_21_positions_the_instrument_lacks_fold_to_the_nearest_it_has() {
    let read = |param: Param, value: u8| Patch::DEFAULT.with(param, value);
    // LFO WAVE FORM: SINE, TRI, SAW-DW, SQR, RANDOM, CHAOS.
    let lfo: Vec<LfoWave> = [0, 22, 43, 64, 86, 107].map(|value| read(Param::LfoWave, value).lfo_wave()).to_vec();
    assert_eq!(
        lfo,
        [LfoWave::Sine, LfoWave::Sine, LfoWave::Sawtooth, LfoWave::Square, LfoWave::Random, LfoWave::Random]
    );
    // VCO-1 RANGE: 32′, 16′, 8′, 4′, 2′, 1′.
    let range: Vec<Vco1Range> =
        [0, 22, 43, 64, 86, 107].map(|value| read(Param::Vco1Range, value).vco1_range()).to_vec();
    assert_eq!(
        range,
        [Vco1Range::Sixteen, Vco1Range::Sixteen, Vco1Range::Eight, Vco1Range::Four, Vco1Range::Two, Vco1Range::Two]
    );
    // VCO-1 WAVEFORM: SINE, TRI, SAW, PW, SQR, NOISE.
    let wave1: Vec<Vco1Wave> = [0, 22, 43, 64, 86, 107].map(|value| read(Param::Vco1Wave, value).vco1_wave()).to_vec();
    assert_eq!(
        wave1,
        [
            Vco1Wave::Triangle,
            Vco1Wave::Triangle,
            Vco1Wave::Sawtooth,
            Vco1Wave::Pulse,
            Vco1Wave::Square,
            Vco1Wave::Square
        ]
    );
    // VCO-2 WAVEFORM: SINE, TRI, SAW, PW, SQR, NOISE.
    let wave2: Vec<Vco2Wave> = [0, 22, 43, 64, 86, 107].map(|value| read(Param::Vco2Wave, value).vco2_wave()).to_vec();
    assert_eq!(
        wave2,
        [Vco2Wave::Sine, Vco2Wave::Sine, Vco2Wave::Sawtooth, Vco2Wave::Pulse, Vco2Wave::Pulse, Vco2Wave::Noise]
    );
    // The canonical values S8-21 item 4 lists: a fold's band is never
    // written.
    assert_eq!(
        [LfoWave::Sine, LfoWave::Sawtooth, LfoWave::Square, LfoWave::Random].map(LfoWave::value),
        [0, 43, 64, 86]
    );
    assert_eq!(
        [Vco1Range::Sixteen, Vco1Range::Eight, Vco1Range::Four, Vco1Range::Two].map(Vco1Range::value),
        [22, 43, 64, 86]
    );
    assert_eq!(
        [Vco1Wave::Triangle, Vco1Wave::Sawtooth, Vco1Wave::Pulse, Vco1Wave::Square].map(Vco1Wave::value),
        [22, 43, 64, 86]
    );
    assert_eq!(
        [Vco2Wave::Sine, Vco2Wave::Sawtooth, Vco2Wave::Pulse, Vco2Wave::Noise].map(Vco2Wave::value),
        [0, 43, 64, 107]
    );
    // The assign mode in the table's order, SOLO UNISON's band Poly-2.
    assert_eq!(
        [0, 32, 64, 96].map(|value| read(Param::AssignMode, value).assign_mode()),
        [AssignMode::Poly1, AssignMode::Solo, AssignMode::Unison, AssignMode::Poly2]
    );
    assert_eq!(
        [0, 43, 86].map(|value| read(Param::VcoModulator, value).vco_modulator()),
        [VcoModulator::Vco1, VcoModulator::Both, VcoModulator::Vco2]
    );
    assert_eq!(
        [0, 43, 86].map(|value| read(Param::PwmSource, value).pwm_source()),
        [PwmSource::Lfo, PwmSource::Manual, PwmSource::Envelope]
    );
    assert_eq!([0, 32, 64, 96].map(|value| read(Param::VcaLfoMod, value).vca_lfo_mod()), [0, 1, 2, 3]);
    assert_eq!((0..4).map(vca_lfo_mod_value).collect::<Vec<_>>(), [0, 32, 64, 96]);
}

#[test]
fn s8_30_vco2_range_has_37_bands() {
    // S8-30's table, each position's band.
    let bands: [(u8, u8); 37] = [
        (0, 3),
        (4, 6),
        (7, 10),
        (11, 13),
        (14, 17),
        (18, 20),
        (21, 24),
        (25, 27),
        (28, 31),
        (32, 34),
        (35, 38),
        (39, 41),
        (42, 44),
        (45, 48),
        (49, 51),
        (52, 55),
        (56, 58),
        (59, 62),
        (63, 65),
        (66, 69),
        (70, 72),
        (73, 76),
        (77, 79),
        (80, 83),
        (84, 86),
        (87, 89),
        (90, 93),
        (94, 96),
        (97, 100),
        (101, 103),
        (104, 107),
        (108, 110),
        (111, 114),
        (115, 117),
        (118, 121),
        (122, 124),
        (125, 127),
    ];
    for (semitones, &(low, high)) in bands.iter().enumerate() {
        for value in low..=high {
            assert_eq!(usize::from(Patch::DEFAULT.with(Param::Vco2Range, value).vco2_range()), semitones);
        }
        assert_eq!(vco2_range_value(semitones as u8), low);
    }
}

#[test]
fn s8_21_sliders_and_the_fine_tune_scale_as_the_laws_say() {
    // Item 1: floor((v × 255 + 63) ÷ 127).
    assert_eq!([0, 1, 63, 64, 126, 127].map(slider_byte), [0, 2, 126, 129, 253, 255]);
    // Item 3: −50 cents at 0, the centre at 64, +50 at 127, as the signed
    // byte −128, 0 and 127.
    assert_eq!([0, 32, 63, 64, 96, 127].map(fine_tune_signed), [-128, -64, -2, 0, 65, 127]);
    // The original's scale of 0 to 10, in tenths.
    let scaled = Patch::DEFAULT.at(Param::VcfEnvMod, 100).at(Param::VcfCutoff, 50).at(Param::VcoLfoMod, 0);
    assert_eq!(
        (scaled.get(Param::VcfEnvMod), scaled.get(Param::VcfCutoff), scaled.get(Param::VcoLfoMod)),
        (127, 64, 0)
    );
}

#[test]
fn s8_22_the_default_patch_is_the_table_s_last_column() {
    let patch = Patch::DEFAULT;
    assert_eq!(
        patch.to_bytes(),
        [
            0, 0, 0, 0, 0, 43, 0, 43, 0, 43, 43, 0, 0, 42, 64, 43, 127, 0, 0, 127, 0, 127, 0, 0, 0, 0, 96, 0, 0, 0,
            127, 0, 0, 0, 0, 127, 0, 0, 0, 0, 127, 127
        ]
    );
    assert_eq!(patch.lfo_wave(), LfoWave::Sine);
    assert_eq!(patch.vco_modulator(), VcoModulator::Both);
    assert_eq!(patch.pwm_source(), PwmSource::Manual);
    assert_eq!((patch.vco1_range(), patch.vco1_wave()), (Vco1Range::Eight, Vco1Wave::Sawtooth));
    assert_eq!((patch.vco2_range(), patch.vco2_wave()), (12, Vco2Wave::Sawtooth));
    assert_eq!(fine_tune_signed(patch.get(Param::FineTune)), 0);
    assert!(!patch.on(Param::Sync) && !patch.on(Param::LowFrequency));
    // VCF SLOPE at −24 dB, envelope 1, ENV-1 normal, Poly-1, both bend
    // switches on.
    assert!(patch.on(Param::VcfSlope) && !patch.on(Param::VcfEnvSource) && !patch.on(Param::Env1Polarity));
    assert_eq!(patch.assign_mode(), AssignMode::Poly1);
    assert!(patch.on(Param::Vco1Bend) && patch.on(Param::Vco2Bend));
    assert_eq!(slider_byte(patch.get(Param::VcaLevel)), 193);
    // A new instrument's two halves stand at it.
    let synth8 = Synth8::new();
    assert_eq!((*synth8.patch(), *synth8.lower_patch()), (Patch::DEFAULT, Patch::DEFAULT));
    assert_eq!(Patch::default(), Patch::DEFAULT);
}

#[test]
fn s8_23_every_value_0_to_127_is_a_patch_and_nothing_else_is() {
    let base = Patch::DEFAULT.to_bytes();
    let mut synth8 = Synth8::new();
    let mut out = [0i16; 64];
    for place in 0..PATCH_BYTES {
        for value in 0..=255u8 {
            let mut bytes = base;
            bytes[place] = value;
            match check_patch(&bytes) {
                Ok(patch) => {
                    assert!(value <= 127, "byte {place} took {value}");
                    assert_eq!(pack_patch(&patch), bytes);
                    // And the instrument plays it.
                    synth8.set_patch(&patch);
                    let _ = synth8.event(0x90, 60, 100);
                    synth8.render(&mut out);
                }
                Err(error) => {
                    assert!(value > 127, "byte {place} refused {value}");
                    assert_eq!(error, PatchError::Value(place as u8));
                }
            }
        }
    }
    // The first byte that is wrong is the one named; a patch of 41 or 43
    // bytes is none.
    let mut bytes = base;
    bytes[22] = 128;
    bytes[38] = 200;
    assert_eq!(Patch::from_bytes(&bytes), Err(PatchError::Value(22)));
    assert_eq!(check_patch(&base[..41]), Err(PatchError::Length(41)));
    assert_eq!(check_patch(&[base.as_slice(), &[0]].concat()), Err(PatchError::Length(43)));
}

#[test]
fn s8_29_a_bank_is_1_to_64_patches_back_to_back() {
    let patches: Vec<Patch> = (0..64u8).map(|number| Patch::DEFAULT.with(Param::VcfCutoff, number)).collect();
    let mut image = [0u8; 64 * PATCH_BYTES];
    assert_eq!(build_bank(&patches, &mut image), Ok(2_688));
    assert_eq!(check_bank(&image), Ok(64));
    for (number, patch) in patches.iter().enumerate() {
        assert_eq!(check_patch(&image[number * PATCH_BYTES..][..PATCH_BYTES]), Ok(*patch));
    }
    assert_eq!(build_bank(&patches[..1], &mut image), Ok(42));
    assert_eq!(check_bank(&image[..42]), Ok(1));
    assert_eq!(build_bank(&[], &mut image), Err(BankError::Empty));
    let many = vec![Patch::DEFAULT; MOST_PATCHES + 1];
    let mut room = vec![0u8; 65 * PATCH_BYTES];
    assert_eq!(build_bank(&many, &mut room), Err(BankError::TooMany(65)));
    assert_eq!(check_bank(&room), Err(BankError::TooMany(65)));
    assert_eq!(build_bank(&patches, &mut image[..100]), Err(BankError::Room(2_688)));
    assert_eq!(check_bank(&[]), Err(BankError::Empty));
    assert_eq!(check_bank(&image[..43]), Err(BankError::Length(43)));
    image[42 + 5] = 128;
    assert_eq!(check_bank(&image), Err(BankError::Patch { index: 1, error: PatchError::Value(5) }));
}

#[test]
fn s8_28_patch_numbers_11_to_88_are_programs_0_to_63() {
    let mut pairs = Vec::new();
    for tens in 1..=8u8 {
        for units in 1..=8u8 {
            let number = tens * 10 + units;
            let program = program_of_patch_number(number).unwrap();
            assert_eq!(patch_number_of_program(program), Some(number));
            pairs.push(program);
        }
    }
    assert_eq!(pairs, (0..64).collect::<Vec<u8>>());
    assert_eq!([11, 18, 21, 88].map(program_of_patch_number), [Some(0), Some(7), Some(8), Some(63)]);
    for refused in [19, 20, 29, 90, 10, 0, 9, 99] {
        assert_eq!(program_of_patch_number(refused), None, "{refused}");
    }
    assert_eq!(patch_number_of_program(64), None);
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
    assert_eq!((mix(0), mix(64), mix(127), mix(32)), ((127, 0), (126, 127), (0, 127), (127, 64)));
}

#[test]
fn the_factory_patches_are_patches_and_sound() {
    for (number, name, patch) in factory::all() {
        assert_eq!(Patch::from_bytes(&patch.to_bytes()), Ok(patch), "{number} {name}");
        let mut synth8 = Synth8::new();
        synth8.set_patch(&patch);
        for key in [48, 55, 64] {
            let _ = synth8.event(0x90, key, 100);
        }
        let mut out = vec![0i16; 48_000];
        synth8.render(&mut out);
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
    assert_eq!(numbers(&|patch| patch.assign_mode() == AssignMode::Unison), [14, 58]);
    assert_eq!(numbers(&|patch| !matches!(patch.assign_mode(), AssignMode::Poly1 | AssignMode::Unison)), []);
    assert_eq!(numbers(&|patch| !patch.on(Param::Vco1Bend) && patch.on(Param::Vco2Bend)), [14]);
    // And what is written on GONG's sheet by hand, at VCO-2's range
    // knob: "TUNE UP A FIFTH". VCO-1's range is in octaves from 16' and
    // VCO-2's in semitones.
    let gong = factory::gong();
    assert_eq!(i32::from(gong.vco2_range()) - 12 * i32::from(gong.vco1_range().index()), 7);
    // LO STRINGS' envelopes, measured on the scan of its sheet at 300
    // dots an inch: each knob's middle between the end marks of its
    // scale, to a quarter of a division. In tenths of the scale.
    let strings = factory::lo_strings();
    let drawn = [
        (Param::Env1Attack, 50),
        (Param::Env1Decay, 67),
        (Param::Env1Sustain, 67),
        (Param::Env1Release, 46),
        (Param::Env2Attack, 54),
        (Param::Env2Decay, 62),
        (Param::Env2Sustain, 61),
        (Param::Env2Release, 51),
    ];
    for (param, tenths) in drawn {
        let (low, high) =
            (Patch::DEFAULT.at(param, tenths - 3).get(param), Patch::DEFAULT.at(param, tenths + 3).get(param));
        assert!((low..=high).contains(&strings.get(param)), "{param:?}: {} for {tenths} tenths", strings.get(param));
    }
}
