//! The Juno's patch: the Juno-106's 18 bytes read and checked, for every
//! value of every byte (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test"
)]

use std::cmp::Ordering;

use neoteletext_audio_instruments::juno::patch::{PATCH_BYTES, SLIDERS};
use neoteletext_audio_instruments::juno::{ChorusMode, Patch, PatchError, PwmSource, Range, Slider};

/// The patch of the owner's manual's example of the Manual button's
/// message (MIDI Implementation 3.2).
const MANUAL: [u8; PATCH_BYTES] = [0x03, 0, 0, 0, 0, 0x3f, 0x3c, 0, 0, 0x7f, 0x45, 0, 0, 0x7f, 0, 0, 0x2a, 0x19];
/// The patch of its example of a patch change's message (3.1).
const CHANGED: [u8; PATCH_BYTES] =
    [0x39, 0x2d, 0, 0x37, 0, 0x55, 0, 0, 0, 0x19, 0x34, 0x3b, 0x20, 0x56, 0x28, 0, 0x1a, 0x18];

#[test]
fn the_manuals_two_examples_read_as_the_manual_says() {
    let manual = Patch::from_bytes(&MANUAL).expect("the manual's example");
    // 0x2A: chorus off, pulse on, sawtooth off, 8'. 0x19: high-pass at
    // 0, the amplifier on the envelope, the envelope upright, the pulse
    // width by hand.
    assert_eq!(
        (manual.chorus, manual.pulse, manual.sawtooth, manual.range),
        (ChorusMode::Off, true, false, Range::Eight)
    );
    assert_eq!(
        (manual.hpf, manual.vca_gate, manual.envelope_inverted, manual.pwm),
        (0, false, false, PwmSource::Manual)
    );
    assert_eq!(
        (manual.get(Slider::LfoRate), manual.get(Slider::VcfFrequency), manual.get(Slider::VcfKeyboard)),
        (3, 0x3f, 0x7f)
    );
    assert_eq!((manual.get(Slider::VcaLevel), manual.get(Slider::Sustain), manual.get(Slider::Sub)), (0x45, 0x7f, 0));

    let changed = Patch::from_bytes(&CHANGED).expect("the manual's example");
    // 0x1A: chorus II, sawtooth and pulse, 8'. 0x18: high-pass at 0,
    // the pulse width by the oscillator.
    assert_eq!(
        (changed.chorus, changed.pulse, changed.sawtooth, changed.range),
        (ChorusMode::Two, true, true, Range::Eight)
    );
    assert_eq!((changed.hpf, changed.pwm), (0, PwmSource::Lfo));
    assert_eq!(
        (changed.get(Slider::Attack), changed.get(Slider::Decay), changed.get(Slider::Release)),
        (0x3b, 0x20, 0x28)
    );
}

#[test]
fn every_value_of_every_byte_is_a_patch_or_is_refused_for_its_reason() {
    for position in 0..PATCH_BYTES {
        for value in 0..=u8::MAX {
            let mut bytes = MANUAL;
            bytes[position] = value;
            let expected = match position.cmp(&SLIDERS) {
                Ordering::Less => (value > 127).then_some(PatchError::Slider(position as u8)),
                Ordering::Equal => (value > 127 || ![1, 2, 4].contains(&(value & 7))).then_some(PatchError::Range),
                Ordering::Greater => (value > 31).then_some(PatchError::Switches),
            };
            match (Patch::from_bytes(&bytes), expected) {
                (Ok(patch), None) => {
                    // What was read is what is written back, but for the
                    // bit that says which chorus is off.
                    let written = patch.to_bytes();
                    assert_eq!(Patch::from_bytes(&written), Ok(patch), "byte {position} = {value}");
                    let mut same = bytes;
                    if patch.chorus == ChorusMode::Off {
                        same[SLIDERS] &= !0x40;
                    }
                    assert_eq!(written, same, "byte {position} = {value}");
                }
                (Err(found), Some(expected)) => assert_eq!(found, expected, "byte {position} = {value}"),
                (found, expected) => panic!("byte {position} = {value}: {found:?} for {expected:?}"),
            }
        }
    }
}

#[test]
fn the_switch_bytes_mean_what_the_table_says() {
    let read = |first: u8, second: u8| {
        let mut bytes = MANUAL;
        bytes[16] = first;
        bytes[17] = second;
        Patch::from_bytes(&bytes).expect("a patch")
    };
    assert_eq!(read(0b000_0001, 0).range, Range::Sixteen);
    assert_eq!(read(0b000_0100, 0).range, Range::Four);
    assert_eq!(read(0b100_0010, 0).chorus, ChorusMode::One);
    assert_eq!(read(0b000_0010, 0).chorus, ChorusMode::Two);
    assert_eq!(read(0b110_0010, 0).chorus, ChorusMode::Off);
    // The high-pass switch counts down: 11 is position 0, 00 position 3.
    assert_eq!([0b11_000, 0b10_000, 0b01_000, 0].map(|second| read(2, second).hpf), [0, 1, 2, 3]);
    let all = read(2, 0b00_111);
    assert_eq!((all.vca_gate, all.envelope_inverted, all.pwm), (true, true, PwmSource::Manual));
}

#[test]
fn what_the_juno_106_has_not_is_written_as_the_nearest_it_has() {
    let mut patch = Patch::DEFAULT.with(Slider::Sub, 90).with(Slider::Noise, 200);
    assert_eq!(patch.get(Slider::Noise), 127);
    patch.chorus = ChorusMode::Both;
    patch.pwm = PwmSource::Envelope;
    patch.sub = false;
    patch.hpf = 9;
    let read = Patch::from_bytes(&patch.to_bytes()).expect("a patch");
    assert_eq!((read.chorus, read.pwm, read.get(Slider::Sub), read.hpf), (ChorusMode::Two, PwmSource::Manual, 0, 3));
}
