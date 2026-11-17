//! The Juno's patch: the Juno-106's 18 bytes and the two that carry the
//! Juno-60's settings, read and checked, for every value of every byte
//! (TODO-6431; the owner's 20-byte patch of 18 November 2026).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "a test"
)]

use std::cmp::Ordering;

use neoteletext_audio_instruments::juno::patch::{MIDI_PATCH_BYTES, PATCH_BYTES, SLIDERS};
use neoteletext_audio_instruments::juno::{ChorusMode, Juno, Model, Patch, PatchError, PwmSource, Range, Slider};

/// The patch of the owner's manual's example of the Manual button's
/// message (MIDI Implementation 3.2), with no Juno-60 setting.
const MANUAL: [u8; PATCH_BYTES] = [0x03, 0, 0, 0, 0, 0x3f, 0x3c, 0, 0, 0x7f, 0x45, 0, 0, 0x7f, 0, 0, 0x2a, 0x19, 0, 0];
/// The patch of its example of a patch change's message (3.1).
const CHANGED: [u8; PATCH_BYTES] =
    [0x39, 0x2d, 0, 0x37, 0, 0x55, 0, 0, 0, 0x19, 0x34, 0x3b, 0x20, 0x56, 0x28, 0, 0x1a, 0x18, 0, 0];
/// Byte 18: the Juno-60's settings; byte 19, reserved.
const JUNO_60: usize = MIDI_PATCH_BYTES;
const RESERVED: usize = MIDI_PATCH_BYTES + 1;

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
            // MANUAL's chorus is off, so I+II (bit 2 of byte 18) is refused.
            let expected = if position == JUNO_60 {
                (value > 3).then_some(PatchError::Juno60)
            } else if position == RESERVED {
                (value != 0).then_some(PatchError::Juno60)
            } else {
                match position.cmp(&SLIDERS) {
                    Ordering::Less => (value > 127).then_some(PatchError::Slider(position as u8)),
                    Ordering::Equal => (value > 127 || ![1, 2, 4].contains(&(value & 7))).then_some(PatchError::Range),
                    Ordering::Greater => (value > 31).then_some(PatchError::Switches),
                }
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
fn the_juno_60s_settings_are_carried_and_the_juno_106_bytes_hold_the_nearest_it_has() {
    let mut patch = Patch::DEFAULT.with(Slider::Sub, 90).with(Slider::Noise, 200);
    assert_eq!(patch.get(Slider::Noise), 127);
    patch.chorus = ChorusMode::Both;
    patch.pwm = PwmSource::Envelope;
    patch.sub = false;
    let bytes = patch.to_bytes();
    let read = Patch::from_bytes(&bytes).expect("a patch");
    // The Juno-60's settings come back whole, and the sub level is kept.
    assert_eq!(
        (read.chorus, read.pwm, read.sub, read.get(Slider::Sub)),
        (ChorusMode::Both, PwmSource::Envelope, false, 90)
    );
    // The Juno-106's bytes say II and manual, the nearest it has.
    assert_eq!((bytes[16] & 0x60, bytes[17] & 1, bytes[JUNO_60], bytes[RESERVED]), (0, 1, 0b111, 0));
    // A held-high high-pass position is written as the highest there is.
    patch.hpf = 9;
    assert_eq!(Patch::from_bytes(&patch.to_bytes()).expect("a patch").hpf, 3);
    // I+II with the chorus off is no patch.
    let mut off = MANUAL;
    off[JUNO_60] = 0b100;
    assert_eq!(Patch::from_bytes(&off), Err(PatchError::Juno60));
}

#[test]
fn each_model_starts_on_a_plain_patch_that_is_flat_on_that_model() {
    // The Juno-106's plain patch is the default one, and the Juno-60's
    // differs from it by the high-pass switch alone: 1 is flat on the
    // Juno-106 and a high-pass filter at 235 Hz on the Juno-60, whose
    // flat position is 0.
    assert_eq!(Patch::default_for(Model::Juno106), Patch::DEFAULT);
    let mut expected = Patch::DEFAULT;
    expected.hpf = 0;
    assert_eq!(Patch::default_for(Model::Juno60), expected);
    for model in [Model::Juno106, Model::Juno60] {
        let plain = Patch::default_for(model);
        assert_eq!(*Juno::new(model).patch(), plain, "{model:?}");
        // A second of the lowest A, 55 Hz, and the size of its
        // fundamental in the second half.
        let low_a = |juno: &mut Juno| {
            juno.event(0x90, 33, 100);
            let mut out = vec![0i16; 48_000];
            juno.render(&mut out);
            let (mut sine, mut cosine) = (0.0f64, 0.0f64);
            for (index, &sample) in out.iter().enumerate().skip(24_000) {
                let angle = std::f64::consts::TAU * 55.0 * index as f64 / 48_000.0;
                sine += f64::from(sample) * angle.sin();
                cosine += f64::from(sample) * angle.cos();
            }
            (out, sine.hypot(cosine))
        };
        // A new instrument plays exactly its model's plain patch.
        let (new, whole) = low_a(&mut Juno::new(model));
        let mut given = Juno::new(model);
        given.set_patch(&plain);
        assert_eq!(new, low_a(&mut given).0, "{model:?}");
        // And has its bass whole: every position of the switch that
        // cuts leaves less than a third of that fundamental (the
        // Juno-106's position 0 does not cut, it lifts).
        for position in (0..=3u8).filter(|&position| position != plain.hpf) {
            if model == Model::Juno106 && position == 0 {
                continue;
            }
            let mut cutting = Juno::new(model);
            let mut patch = plain;
            patch.hpf = position;
            cutting.set_patch(&patch);
            let left = low_a(&mut cutting).1 / whole;
            assert!(left < 0.3, "{model:?}: position {position} leaves {left} of the plain patch's 55 Hz");
        }
    }
}
