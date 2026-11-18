//! synthD's patch: its 60 values read and written by SD-18 to SD-20's
//! laws, exact both ways, and every 60 bytes of 0 to 127 a patch
//! (`synthd.md`, SD-17 to SD-22; `server.md`, SRV-34 and SRV-35).

// Without the feature there is nothing here to test.
#![cfg(feature = "synthd")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "synthd_charts.rs"]
mod charts;

use neoteletext_audio_instruments::synthd::interface::{
    self, BankFault, DEFAULT_PATCH, LAYOUT, PATCH_BYTES, PatchFault, Role, byte,
};
use neoteletext_audio_instruments::synthd::{Patch, Range, SynthD, Waveform};

/// The value of the pair at `at` set to `number`, in a copy of `values`.
fn with_pair(values: &[u8; PATCH_BYTES], at: usize, number: u16) -> [u8; PATCH_BYTES] {
    let mut values = *values;
    values[at] = (number >> 7) as u8;
    values[at + 1] = (number & 0x7F) as u8;
    values
}

/// What each pair of table A is, written out here from SD-18's table and
/// not from the code: its byte, its lowest and highest field, and the
/// field as read from a patch.
type Read = fn(&Patch) -> i32;
const PAIRS: [(&str, usize, i32, i32, Read); 17] = [
    ("TUNE", byte::TUNE, -242, 242, |patch| i32::from(patch.tune)),
    ("GLIDE", byte::GLIDE, 0, 100, |patch| i32::from(patch.glide)),
    ("oscillator 2 FREQUENCY", byte::OSCILLATOR_2_FREQUENCY, -700, 700, |patch| {
        i32::from(patch.oscillators[1].frequency)
    }),
    ("oscillator 3 FREQUENCY", byte::OSCILLATOR_3_FREQUENCY, -700, 700, |patch| {
        i32::from(patch.oscillators[2].frequency)
    }),
    ("oscillator 1 VOLUME", byte::OSCILLATOR_1_VOLUME, 0, 100, |patch| i32::from(patch.oscillators[0].volume)),
    ("oscillator 2 VOLUME", byte::OSCILLATOR_2_VOLUME, 0, 100, |patch| i32::from(patch.oscillators[1].volume)),
    ("oscillator 3 VOLUME", byte::OSCILLATOR_3_VOLUME, 0, 100, |patch| i32::from(patch.oscillators[2].volume)),
    ("EXTERNAL INPUT VOLUME", byte::EXTERNAL_INPUT_VOLUME, 0, 100, |patch| i32::from(patch.external_volume)),
    ("NOISE VOLUME", byte::NOISE_VOLUME, 0, 100, |patch| i32::from(patch.noise_volume)),
    ("CUTOFF FREQUENCY", byte::CUTOFF_FREQUENCY, -500, 500, |patch| i32::from(patch.cutoff)),
    ("EMPHASIS", byte::EMPHASIS, 0, 100, |patch| i32::from(patch.emphasis)),
    ("filter ATTACK TIME", byte::FILTER_ATTACK_TIME, 0, 10_000, |patch| i32::from(patch.filter_contour.attack)),
    ("filter DECAY TIME", byte::FILTER_DECAY_TIME, 0, 10_000, |patch| i32::from(patch.filter_contour.decay)),
    ("filter SUSTAIN LEVEL", byte::FILTER_SUSTAIN_LEVEL, 0, 100, |patch| i32::from(patch.filter_contour.sustain)),
    ("loudness ATTACK TIME", byte::LOUDNESS_ATTACK_TIME, 0, 10_000, |patch| i32::from(patch.loudness_contour.attack)),
    ("loudness DECAY TIME", byte::LOUDNESS_DECAY_TIME, 0, 10_000, |patch| i32::from(patch.loudness_contour.decay)),
    ("loudness SUSTAIN LEVEL", byte::LOUDNESS_SUSTAIN_LEVEL, 0, 100, |patch| i32::from(patch.loudness_contour.sustain)),
];

/// SD-18 item 1's canonical number, written out from the document.
fn canonical(field: i32, lowest: i32, highest: i32) -> u16 {
    let span = highest - lowest;
    (((field - lowest) * 16_383 + span / 2) / span) as u16
}

#[test]
fn sd_21_sd_22_the_default_patch_is_the_last_column_of_the_table() {
    assert_eq!(Patch::DEFAULT.to_values(), DEFAULT_PATCH);
    assert_eq!(Patch::from_values(&DEFAULT_PATCH), Patch::DEFAULT);
    assert_eq!(SynthD::new().values(), &DEFAULT_PATCH);
    assert_eq!(SynthD::new().patch(), &Patch::DEFAULT);
    // The table's column, written out again from SD-22.
    let column: [u8; PATCH_BYTES] = [
        0, 0, 64, 0, 64, 0, 0, 0, 127, 127, 127, 127, 127, 127, 0, 0, 127, 127, 0, 0, 0, 0, 0, 0, 127, 127, 64, 0, 0,
        0, 0, 0, 127, 127, 0, 32, 32, 32, 127, 0, 0, 127, 0, 0, 0, 0, 64, 0, 0, 127, 43, 127, 43, 0, 127, 0, 0, 0, 0,
        0,
    ];
    assert_eq!(DEFAULT_PATCH, column);
}

#[test]
fn sd_22_the_patch_is_the_controllers_of_its_table_in_order() {
    let controllers: Vec<u8> = LAYOUT.iter().map(|&(number, _)| number).collect();
    let table = [
        5, 37, 10, 42, 12, 44, 14, 46, 15, 47, 16, 48, 17, 49, 18, 50, 19, 51, 21, 53, 23, 55, 24, 56, 25, 57, 27, 59,
        28, 60, 29, 61, 30, 62, 65, 74, 75, 76, 79, 80, 81, 82, 83, 84, 85, 88, 89, 90, 102, 103, 104, 105, 106, 107,
        108, 109, 110, 111, 112, 113,
    ];
    assert_eq!(controllers, table);
    let fine: Vec<usize> = (0..PATCH_BYTES).filter(|&at| LAYOUT[at].1 == Role::Fine).collect();
    let mut expected: Vec<usize> = (1..34).step_by(2).collect();
    expected.push(47);
    assert_eq!(fine, expected);
}

#[test]
fn sd_22_the_charts_survive_their_60_values() {
    for (name, patch) in charts::CHARTS {
        let values = patch.to_values();
        assert!(values.iter().all(|&value| value <= 127), "{name}");
        let mut expected = patch;
        // Oscillator 1 has no FREQUENCY knob, and the patch no byte for it.
        expected.oscillators[0].frequency = 0;
        assert_eq!(Patch::from_values(&values), expected, "{name}");
        assert_eq!(Patch::from_bytes(&values), Ok(expected), "{name}");
    }
}

#[test]
fn sd_18_every_value_of_every_pair_is_carried_exactly_and_the_ends_and_middle_are_numbers_0_8192_16383() {
    for (name, at, lowest, highest, read) in PAIRS {
        for field in lowest..=highest {
            let number = canonical(field, lowest, highest);
            let values = with_pair(&DEFAULT_PATCH, at, number);
            assert_eq!(read(&Patch::from_values(&values)), field, "{name} {field}");
        }
        let at_number = |number: u16| read(&Patch::from_values(&with_pair(&DEFAULT_PATCH, at, number)));
        assert_eq!(at_number(0), lowest, "{name}");
        assert_eq!(at_number(16_383), highest, "{name}");
        if lowest < 0 {
            assert_eq!(at_number(8_192), 0, "{name}");
        }
        let mut last = i32::MIN;
        for number in 0..=16_383 {
            let field = at_number(number);
            assert!(field >= last, "{name} falls at {number}");
            last = field;
        }
    }
}

#[test]
fn sd_18_amount_of_contour_is_the_upper_half_of_its_pair() {
    let amount =
        |number: u16| Patch::from_values(&with_pair(&DEFAULT_PATCH, byte::AMOUNT_OF_CONTOUR, number)).contour_amount;
    assert_eq!((amount(0), amount(8_191), amount(8_192), amount(16_383)), (0, 0, 0, 100));
    for field in 0..=100u8 {
        let number = 8_192 + ((u32::from(field) * 8_191 + 50) / 100) as u16;
        assert_eq!(amount(number), field);
        let mut patch = Patch::DEFAULT;
        patch.contour_amount = field;
        let values = patch.to_values();
        assert_eq!(u16::from(values[26]) << 7 | u16::from(values[27]), number);
    }
    let mut last = 0;
    for number in 0..=16_383 {
        assert!(amount(number) >= last);
        last = amount(number);
    }
}

#[test]
fn sd_18_a_coarse_controller_alone_clears_the_fine_part() {
    let mut values = DEFAULT_PATCH;
    values[byte::GLIDE + 1] = 99;
    assert_eq!(interface::set_value(&mut values, 5, 64), Some(true));
    assert_eq!((values[byte::GLIDE], values[byte::GLIDE + 1]), (64, 0));
    assert_eq!(interface::set_value(&mut values, 37, 5), Some(true));
    assert_eq!((values[byte::GLIDE], values[byte::GLIDE + 1]), (64, 5));
    // Alone, a coarse controller reaches 16,256 ÷ 16,383 of the travel.
    let mut patch_values = DEFAULT_PATCH;
    interface::set_value(&mut patch_values, 5, 127);
    assert_eq!(Patch::from_values(&patch_values).glide, 99);
    interface::set_value(&mut patch_values, 27, 127);
    assert_eq!(Patch::from_values(&patch_values).contour_amount, 98);
}

#[test]
fn sd_19_the_knobs_of_one_controller() {
    for at in [byte::MODULATION_MIX, byte::MAIN_OUTPUT_VOLUME] {
        let read = |value: u8| {
            let mut values = DEFAULT_PATCH;
            values[at] = value;
            let patch = Patch::from_values(&values);
            if at == byte::MODULATION_MIX { patch.modulation_mix } else { patch.volume }
        };
        let mut last = 0;
        for value in 0..=127u8 {
            let field = read(value);
            assert_eq!(u32::from(field), (u32::from(value) * 100 + 63) / 127);
            assert!(field >= last);
            last = field;
        }
        assert_eq!((read(0), read(64), read(127)), (0, 50, 100));
        for field in 0..=100u32 {
            assert_eq!(u32::from(read(((field * 127 + 50) / 100) as u8)), field);
        }
    }
}

#[test]
fn sd_20_the_bands_of_the_selectors_and_switches() {
    let footage = [Range::ThirtyTwo, Range::Sixteen, Range::Eight, Range::Four, Range::Two, Range::Two];
    let ranges = [
        (byte::OSCILLATOR_1_RANGE_OCTAVE, byte::OSCILLATOR_1_RANGE_LO_HI, 0),
        (byte::OSCILLATOR_2_RANGE_OCTAVE, byte::OSCILLATOR_2_RANGE_LO_HI, 1),
        (byte::OSCILLATOR_3_RANGE_OCTAVE, byte::OSCILLATOR_3_RANGE_LO_HI, 2),
    ];
    for (octave, lo_hi, oscillator) in ranges {
        for value in 0..=127u8 {
            let mut values = DEFAULT_PATCH;
            values[octave] = value;
            values[lo_hi] = 127;
            let expected = footage[usize::from(value.min(80) / 16)];
            assert_eq!(Patch::from_values(&values).oscillators[oscillator].range, expected, "octave {value}");
            values[lo_hi] = value;
            values[octave] = 48;
            let expected = if value >= 64 { Range::Four } else { Range::Lo };
            assert_eq!(Patch::from_values(&values).oscillators[oscillator].range, expected, "LO/HI {value}");
        }
    }
    // The edges by name.
    let edges = [(15, Range::ThirtyTwo), (16, Range::Sixteen), (31, Range::Sixteen), (32, Range::Eight)];
    for (value, range) in
        edges.into_iter().chain([(47, Range::Eight), (48, Range::Four), (79, Range::Two), (80, Range::Two)])
    {
        let mut values = DEFAULT_PATCH;
        values[byte::OSCILLATOR_1_RANGE_OCTAVE] = value;
        assert_eq!(Patch::from_values(&values).oscillators[0].range, range);
    }
    let waveforms =
        [Waveform::Triangle, Waveform::Shark, Waveform::Sawtooth, Waveform::Square, Waveform::Wide, Waveform::Narrow];
    let bands = [(0, 21), (22, 42), (43, 63), (64, 85), (86, 106), (107, 127)];
    let selectors =
        [(byte::OSCILLATOR_1_WAVEFORM, 0), (byte::OSCILLATOR_2_WAVEFORM, 1), (byte::OSCILLATOR_3_WAVEFORM, 2)];
    for (at, oscillator) in selectors {
        for (position, (low, high)) in bands.into_iter().enumerate() {
            for value in low..=high {
                let mut values = DEFAULT_PATCH;
                values[at] = value;
                let patch = Patch::from_values(&values);
                assert_eq!(patch.oscillators[oscillator].waveform, waveforms[position], "{at} {value}");
            }
        }
    }
    for (number, _) in LAYOUT
        .iter()
        .filter(|(number, _)| [65, 79, 81, 82, 83, 84, 85, 88].contains(number) || (109..=113).contains(number))
    {
        let at = LAYOUT.iter().position(|(found, _)| found == number).unwrap();
        for value in 0..=127u8 {
            let mut on = DEFAULT_PATCH;
            on[at] = value;
            let off = {
                let mut values = DEFAULT_PATCH;
                values[at] = if value >= 64 { 0 } else { 127 };
                values
            };
            assert_ne!(Patch::from_values(&on), Patch::from_values(&off), "controller {number} at {value}");
            assert_eq!(Patch::from_values(&on).to_values()[at], if value >= 64 { 127 } else { 0 });
        }
    }
}

#[test]
fn sd_22_every_byte_value_is_a_patch_and_sets_what_its_control_change_sets() {
    for at in 0..PATCH_BYTES {
        let (number, role) = LAYOUT[at];
        for value in 0..=127u8 {
            let mut bytes = DEFAULT_PATCH;
            bytes[at] = value;
            if role == Role::Coarse {
                bytes[at + 1] = 0;
            }
            let patch = Patch::from_bytes(&bytes).expect("every value is a patch");
            let mut synthd = SynthD::new();
            synthd.event(0xB0, number, value);
            assert_eq!(synthd.values(), &bytes, "controller {number} at {value}");
            assert_eq!(synthd.patch(), &patch, "controller {number} at {value}");
        }
    }
}

#[test]
fn sd_22_srv_35_a_patch_of_another_length_or_with_a_high_byte_is_refused() {
    assert_eq!(Patch::from_bytes(&[0; 59]), Err(PatchFault::Length(59)));
    assert_eq!(Patch::from_bytes(&[0; 61]), Err(PatchFault::Length(61)));
    for at in [0, 30, 59] {
        let mut bytes = DEFAULT_PATCH;
        bytes[at] = 128;
        assert_eq!(Patch::from_bytes(&bytes), Err(PatchFault::Byte { at, value: 128 }));
        assert_eq!(interface::check_patch(&bytes), Err(PatchFault::Byte { at, value: 128 }));
    }
    // A bank: 1 and 128 patches; 129, 61 bytes and a high byte refused.
    let one = interface::Patch::default_patch();
    let mut image = [0u8; 7_740];
    assert_eq!(interface::build_bank(&[one], &mut image), Ok(60));
    assert_eq!(interface::check_bank(&image[..60]), Ok(1));
    assert_eq!(interface::build_bank(&[one; 128], &mut image), Ok(7_680));
    assert_eq!(interface::check_bank(&image[..7_680]), Ok(128));
    assert_eq!(interface::build_bank(&[one; 129], &mut image), Err(BankFault::TooManyPatches(129)));
    assert_eq!(interface::build_bank(&[], &mut image), Err(BankFault::Empty));
    assert_eq!(interface::build_bank(&[one; 2], &mut image[..100]), Err(BankFault::Room(120)));
    assert_eq!(interface::check_bank(&image[..61]), Err(BankFault::Length(61)));
    assert_eq!(interface::check_bank(&image[..7_740]), Err(BankFault::Length(7_740)));
    assert_eq!(interface::check_bank(&[]), Err(BankFault::Empty));
    image[60 + 7] = 200;
    assert_eq!(interface::check_bank(&image[..120]), Err(BankFault::Byte { program: 1, at: 7, value: 200 }));
}

#[test]
fn srv_34_the_named_fields_round_trip_with_the_60_bytes() {
    let fields = interface::Patch::from_values(&DEFAULT_PATCH);
    assert_eq!(interface::pack_patch(&fields), DEFAULT_PATCH);
    assert_eq!(fields.cutoff_frequency, 16_383);
    assert_eq!(fields.tune, 8_192);
    assert_eq!(fields.oscillator_1_waveform, 43);
    // Every field at its lowest and highest lands on the bytes the table
    // names: each byte position in turn at 0 and at 127.
    for at in 0..PATCH_BYTES {
        for value in [0u8, 127] {
            let mut bytes = DEFAULT_PATCH;
            bytes[at] = value;
            let fields = interface::check_patch(&bytes).unwrap();
            assert_eq!(interface::pack_patch(&fields), bytes, "byte {at} at {value}");
        }
    }
    // A field above its top is held to it.
    let mut high = fields;
    high.glide = u16::MAX;
    high.decay_switch = 255;
    let bytes = interface::pack_patch(&high);
    assert_eq!((bytes[0], bytes[1], bytes[45]), (127, 127, 127));
}
