//! synthD's patch: its 36 bytes read and written, and every value
//! of every byte either a patch or refused, never a panic (TODO-6432).

// Without the feature there is nothing here to test.
#![cfg(feature = "synthd")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod analog;
#[path = "synthd_charts.rs"]
mod charts;

use analog::Random;
use neoteletext_audio_instruments::synthd::{PATCH_BYTES, Patch, PatchError, Range, Waveform};

/// What the layout's table says of a patch's bytes, written out again
/// here from the table and not from the code: whether they are a patch.
fn valid(bytes: &[u8; PATCH_BYTES]) -> bool {
    let signed = |at: usize| i32::from(i16::from_le_bytes([bytes[at], bytes[at + 1]]));
    let unsigned = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let knobs = [2, 3, 14, 15, 16, 17, 18, 21, 22, 27, 32, 33];
    let positions = [4, 5, 6, 7, 10, 11];
    signed(0).abs() <= 242
        && knobs.iter().all(|&at| bytes[at] <= 100)
        && positions.iter().all(|&at| bytes[at] <= 5)
        && signed(8).abs() <= 700
        && signed(12).abs() <= 700
        && signed(19).abs() <= 500
        && [23, 25, 28, 30].iter().all(|&at| unsigned(at) <= 10_000)
        && bytes[34] < 0x80
        && bytes[35] < 0x40
}

#[test]
fn the_default_patch_and_the_charts_survive_their_bytes() {
    for (name, patch) in charts::CHARTS.into_iter().chain([("default", Patch::DEFAULT)]) {
        let bytes = patch.to_bytes();
        assert!(valid(&bytes), "{name}");
        assert_eq!(Patch::from_bytes(&bytes), Ok(patch), "{name}");
    }
}

#[test]
fn the_bytes_are_where_the_table_says() {
    let mut patch = Patch::DEFAULT;
    patch.tune = -3;
    patch.oscillators[1].range = Range::Two;
    patch.oscillators[1].frequency = 700;
    patch.oscillators[2].waveform = Waveform::Narrow;
    patch.oscillators[2].frequency = -700;
    patch.oscillators[2].on = true;
    patch.cutoff = -133;
    patch.filter_contour.decay = 10_000;
    patch.loudness_contour.attack = 600;
    patch.noise_pink = true;
    patch.decay_on = true;
    let bytes = patch.to_bytes();
    assert_eq!(bytes[..2], (-3i16).to_le_bytes());
    assert_eq!((bytes[6], bytes[8..10].to_vec()), (5, 700i16.to_le_bytes().to_vec()));
    assert_eq!((bytes[11], bytes[12..14].to_vec()), (5, (-700i16).to_le_bytes().to_vec()));
    assert_eq!(bytes[19..21], (-133i16).to_le_bytes());
    assert_eq!(bytes[25..27], 10_000u16.to_le_bytes());
    assert_eq!(bytes[28..30], 600u16.to_le_bytes());
    // Oscillator 3's control, the mixer's switches of 1 and 3; pink and
    // decay.
    assert_eq!((bytes[34], bytes[35]), (0b001_0110, 0b10_0001));
    assert_eq!(Patch::from_bytes(&bytes), Ok(patch));
}

#[test]
fn every_value_of_every_byte_is_a_patch_or_is_refused() {
    let bases = [Patch::DEFAULT.to_bytes(), charts::VIOLIN.to_bytes(), {
        // Fields at their far ends, so that a high byte alone goes
        // beyond.
        let mut patch = charts::FLUTE;
        patch.tune = -242;
        patch.cutoff = 500;
        patch.oscillators[1].frequency = -700;
        patch.oscillators[2].frequency = 700;
        patch.filter_contour.attack = 10_000;
        patch.to_bytes()
    }];
    let (mut accepted, mut refused) = (0, 0);
    for base in bases {
        for at in 0..PATCH_BYTES {
            for value in 0..=u8::MAX {
                let mut bytes = base;
                bytes[at] = value;
                if let Ok(patch) = Patch::from_bytes(&bytes) {
                    assert!(valid(&bytes), "byte {at} = {value} was taken");
                    assert_eq!(patch.to_bytes(), bytes, "byte {at} = {value}");
                    accepted += 1;
                } else {
                    assert!(!valid(&bytes), "byte {at} = {value} was refused");
                    refused += 1;
                }
            }
        }
    }
    println!("{accepted} taken, {refused} refused");
    assert!(accepted > 5_000 && refused > 5_000);
}

#[test]
fn random_bytes_are_a_patch_exactly_when_the_table_says() {
    let mut random = Random(0x6d6f_6f67);
    let mut accepted = 0;
    for round in 0..200_000 {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = random.word() as u8;
        }
        // Most random bytes are no patch; some rounds are brought near
        // one.
        if round % 2 == 0 {
            for at in [1, 9, 13, 20] {
                bytes[at] = if random.word() & 1 == 0 { 0 } else { 0xff };
            }
            for at in [24, 26, 29, 31] {
                bytes[at] %= 40;
            }
            for byte in &mut bytes[2..8] {
                *byte %= 7;
            }
            bytes[10] %= 7;
            bytes[11] %= 7;
            for at in [14, 15, 16, 17, 18, 21, 22, 27, 32, 33] {
                bytes[at] %= 104;
            }
            bytes[34] &= 0x7f | (random.word() as u8 & 0x80 & (random.word() as u8));
            bytes[35] &= 0x3f;
        }
        let found = Patch::from_bytes(&bytes);
        assert_eq!(found.is_ok(), valid(&bytes), "{bytes:?}");
        if let Ok(patch) = found {
            assert_eq!(patch.to_bytes(), bytes);
            accepted += 1;
        }
    }
    println!("{accepted} of 200,000 were patches");
    assert!(accepted > 100);
}

#[test]
fn each_error_names_its_byte() {
    let with = |at: usize, value: u8| {
        let mut bytes = Patch::DEFAULT.to_bytes();
        bytes[at] = value;
        Patch::from_bytes(&bytes)
    };
    assert_eq!(with(1, 0x7f), Err(PatchError::Tune));
    assert_eq!(with(3, 101), Err(PatchError::Knob(3)));
    assert_eq!(with(33, 255), Err(PatchError::Knob(33)));
    assert_eq!(with(7, 6), Err(PatchError::Position(7)));
    assert_eq!(with(13, 3), Err(PatchError::Frequency(12)));
    assert_eq!(with(20, 2), Err(PatchError::Cutoff));
    assert_eq!(with(31, 40), Err(PatchError::Time(30)));
    assert_eq!(with(34, 0x80), Err(PatchError::Switches));
    assert_eq!(with(35, 0x40), Err(PatchError::Switches));
}
