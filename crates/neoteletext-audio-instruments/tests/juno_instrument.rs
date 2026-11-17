//! The Juno as an instrument: its MIDI messages, its six voices, any
//! patch and any messages without a panic, the same samples from the
//! same input, and what it costs (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod analog;
#[path = "juno_support.rs"]
mod support;

use std::time::Instant;

use analog::Random;
use neoteletext_audio_instruments::juno::patch::PATCH_BYTES;
use neoteletext_audio_instruments::juno::{ChorusMode, Juno, Model, Patch, PwmSource, Slider, VOICES};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{CONTROL, MODELS, NOTE_OFF, NOTE_ON, PITCH_BEND, flat, instrument, render, rms, square, whistle};

/// A patch from random bytes: whatever the Juno-106's 18 bytes can hold,
/// and at random what only the Juno-60's panel has.
fn random_patch(random: &mut Random) -> Patch {
    loop {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = random.word() as u8;
        }
        // Most random bytes are no patch: that they are refused, and not
        // a panic, is part of the test.
        if random.word().is_multiple_of(4) {
            for byte in &mut bytes[..16] {
                *byte &= 0x7f;
            }
            bytes[16] = (bytes[16] & 0x78) | [1, 2, 4][random.word() as usize % 3];
            bytes[17] &= 0x1f;
        }
        if let Ok(mut patch) = Patch::from_bytes(&bytes) {
            match random.word() % 4 {
                0 => patch.chorus = ChorusMode::Both,
                1 => patch.pwm = PwmSource::Envelope,
                2 => patch.sub = false,
                _ => patch.hpf = random.word() as u8,
            }
            return patch;
        }
    }
}

/// A random message: mostly ones the instrument takes, some it ignores,
/// some with bytes no message has.
fn random_event(random: &mut Random) -> (u8, u8, u8) {
    let word = random.word();
    let [a, b, c, kind] = word.to_le_bytes();
    match kind % 16 {
        0..=6 => (NOTE_ON | (a & 15), 24 + b % 85, c & 0x7f),
        7..=10 => (NOTE_OFF | (a & 15), 24 + b % 85, c & 0x7f),
        11 => (CONTROL, [1, 7, 64, 120, 121, 123, 124, 125, 126, 127, 10, 91][usize::from(b % 12)], c & 0x7f),
        12 => (PITCH_BEND, b & 0x7f, c & 0x7f),
        13 => (0xc0 | (a & 15), b & 0x7f, c & 0x7f),
        _ => (a, b, c),
    }
}

/// Plays random patches and messages for `blocks` blocks of 20 ms, the
/// calls to render cut at `cut` samples, and gives every sample.
fn random_run(model: Model, seed: u64, blocks: usize, cut: usize) -> Vec<i16> {
    let mut random = Random(seed);
    let mut juno = Juno::new(model);
    let mut out = vec![0i16; blocks * BLOCK_SAMPLES];
    for block in out.chunks_mut(BLOCK_SAMPLES) {
        if random.word().is_multiple_of(8) {
            juno.set_patch(&random_patch(&mut random));
        }
        for _ in 0..random.word() % 4 {
            let (status, first, second) = random_event(&mut random);
            juno.event(status, first, second);
        }
        for piece in block.chunks_mut(cut) {
            juno.render(piece);
        }
        assert!(juno.sounding_voices() <= VOICES);
    }
    out
}

#[test]
fn random_patches_and_messages_never_panic_and_the_output_stays_alive() {
    for model in MODELS {
        for seed in 1..=6 {
            let out = random_run(model, seed, 400, BLOCK_SAMPLES);
            // Not stuck at a rail, and not silent throughout. A random
            // patch may ask for more than full scale (six voices, the
            // level slider's 6 dB, the bass lift's 10 dB, the chorus), and
            // what is beyond is saturated.
            let clipped = out.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
            let loud = out.iter().filter(|&&sample| sample.unsigned_abs() > 300).count();
            println!("{model:?} seed {seed}: {clipped} samples at a rail and {loud} above 1% of {}", out.len());
            assert!(clipped < out.len() / 5 && loud > out.len() / 100, "{model:?} seed {seed}");
        }
    }
}

#[test]
fn the_same_input_gives_the_same_samples_however_the_calls_are_cut() {
    for model in MODELS {
        let whole = random_run(model, 42, 150, BLOCK_SAMPLES);
        assert_eq!(whole, random_run(model, 42, 150, BLOCK_SAMPLES), "{model:?}: two instruments differ");
        assert_eq!(whole, random_run(model, 42, 150, 7), "{model:?}: calls of 7 samples differ");
        assert_ne!(whole, random_run(model, 43, 150, BLOCK_SAMPLES));
    }
    assert_ne!(random_run(Model::Juno106, 42, 150, BLOCK_SAMPLES), random_run(Model::Juno60, 42, 150, BLOCK_SAMPLES));
}

#[test]
fn six_voices_and_the_seventh_key_takes_the_first() {
    for model in MODELS {
        let mut juno = instrument(model, &flat(model, square()).with(Slider::Release, 0));
        for (count, key) in (60..67u8).enumerate() {
            juno.event(NOTE_ON, key, 100);
            render(&mut juno, 0.02);
            assert_eq!(juno.sounding_voices(), (count + 1).min(VOICES), "{model:?}");
        }
        // Six keys up leave the seventh's voice, and then none.
        for key in 61..66u8 {
            juno.event(NOTE_OFF, key, 0);
        }
        render(&mut juno, 0.1);
        assert_eq!(juno.sounding_voices(), 1, "{model:?}");
        juno.event(NOTE_ON | 9, 66, 0);
        render(&mut juno, 0.1);
        assert_eq!(juno.sounding_voices(), 0, "{model:?}");
        // But for the coupling capacitor's last charge, some five units
        // of a 16-bit sample, which runs out over a second.
        assert!(rms(&render(&mut juno, 0.1)) < 3.0e-4, "{model:?}: not silent");
    }
}

#[test]
fn the_pedal_holds_and_the_controllers_do_what_midi_says() {
    for model in MODELS {
        let mut juno = instrument(model, &flat(model, square()));
        let level = |juno: &mut Juno| rms(&render(juno, 0.1));
        juno.event(CONTROL, 64, 127);
        juno.event(NOTE_ON, 60, 100);
        let held = level(&mut juno);
        assert!(held > 0.05, "{model:?}: {held}");
        juno.event(NOTE_OFF, 60, 0);
        assert!((level(&mut juno) / held - 1.0).abs() < 0.05, "{model:?}: the pedal lets go");
        // All notes off is a note off for every key: the pedal holds on.
        juno.event(NOTE_ON, 64, 100);
        juno.event(CONTROL, 123, 0);
        render(&mut juno, 0.1);
        assert_eq!(juno.sounding_voices(), 2, "{model:?}");
        // The volume: half the controller is a quarter of the level.
        let both = level(&mut juno);
        juno.event(CONTROL, 7, 64);
        render(&mut juno, 0.01);
        let quarter = level(&mut juno) / both;
        assert!((quarter - (64.0f64 / 127.0).powi(2)).abs() < 0.02, "{model:?}: {quarter}");
        // Reset all controllers lifts the pedal and leaves the volume.
        juno.event(CONTROL, 121, 0);
        render(&mut juno, 0.1);
        assert_eq!(juno.sounding_voices(), 0, "{model:?}");
        // All sound off silences at once what a long release would hold;
        // what is left is the chorus's and the filters' last samples.
        let mut long = flat(model, square()).with(Slider::Release, 127);
        long.vca_gate = false;
        juno.set_patch(&long);
        juno.event(NOTE_ON, 60, 100);
        render(&mut juno, 0.1);
        juno.event(NOTE_OFF, 60, 0);
        assert_eq!(juno.sounding_voices(), 1);
        juno.event(CONTROL, 120, 0);
        render(&mut juno, 0.05);
        assert_eq!(juno.sounding_voices(), 0, "{model:?}");
        assert!(level(&mut juno) < 1.0e-4, "{model:?}: sound after all sound off");
    }
}

#[test]
fn the_pedal_is_down_at_any_value_but_0_on_the_juno_106_and_from_64_on_the_juno_60() {
    // JUNO-106 owner's manual, MIDI Implementation 2.1: "hold OFF" is a
    // third byte of 0 and "hold ON" is "vvvvvvv = 1 - 127". The Juno-60
    // has no MIDI of its own and keeps MIDI 1.0's rule, on from 64.
    for (model, down_from) in [(Model::Juno106, 1u8), (Model::Juno60, 64)] {
        for value in [0u8, 1, 2, 63, 64, 127] {
            let mut juno = instrument(model, &flat(model, square()));
            juno.event(CONTROL, 64, value);
            juno.event(NOTE_ON, 60, 100);
            render(&mut juno, 0.05);
            juno.event(NOTE_OFF, 60, 0);
            render(&mut juno, 0.1);
            let held = usize::from(value >= down_from);
            assert_eq!(juno.sounding_voices(), held, "{model:?}: the pedal at {value}");
            // A pedal that was down stays down at any value that is down,
            // and 0 lets go on both.
            juno.event(CONTROL, 64, down_from);
            render(&mut juno, 0.1);
            assert_eq!(juno.sounding_voices(), held, "{model:?}: the pedal from {value} to {down_from}");
            juno.event(CONTROL, 64, 0);
            render(&mut juno, 0.1);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: the pedal from {value} to 0");
        }
    }
}

#[test]
fn a_volume_message_between_two_renders_does_not_step_the_signal() {
    // The largest step from one sample to the next.
    let step = |signal: &[i16]| signal.windows(2).map(|pair| (pair[1] - pair[0]).unsigned_abs()).max().unwrap_or(0);
    for model in MODELS {
        // The filter's own oscillation near middle C, a sine: its
        // largest step is its slope, and anything sudden is larger. The
        // volume's gain was worked out once for each call to render, so
        // a volume message moved it between one sample and the next: a
        // step of 0.137 of full scale where the sine's own are 0.008
        // (TODO-6438).
        let cutoff = match model {
            Model::Juno106 => 62,
            Model::Juno60 => 44,
        };
        let sine = flat(model, whistle(cutoff));
        // Half a second for the oscillation to grow; then twenty changes
        // of the volume 3,058 samples apart, the calls to render cut at
        // `cut` samples; then the volume at its half.
        let lead = SAMPLE_RATE as usize / 2;
        let run = |cut: usize| {
            let mut juno = instrument(model, &sine);
            juno.event(NOTE_ON, 60, 100);
            let mut out = vec![0i16; 21 * 3_058 + SAMPLE_RATE as usize / 2];
            juno.render(&mut vec![0i16; lead]);
            for (turn, piece) in out.chunks_mut(3_058).enumerate() {
                juno.event(CONTROL, 7, [127, 64, 127, 0][turn.min(21) % 4]);
                for part in piece.chunks_mut(cut) {
                    juno.render(part);
                }
            }
            out
        };
        let out = run(3_058);
        assert_eq!(out, run(7), "{model:?}: the calls' lengths change the samples");
        // The note alone, at the full volume of the first 3,058 samples.
        let (alone, changes) = (&out[..3_058], &out[3_058..21 * 3_058]);
        let (steady, worst) = (step(alone), step(changes));
        println!("{model:?}: a step of {worst} units at most across the volume's changes, {steady} in the note");
        assert!(worst * 5 < steady * 6, "{model:?}: the volume clicks: {worst} against {steady}");
        // And the volume arrives: half the controller, to the sample of
        // an instrument that had it from the first.
        let mut there = instrument(model, &sine);
        there.event(CONTROL, 7, 64);
        there.event(NOTE_ON, 60, 100);
        let mut same = vec![0i16; lead + out.len()];
        there.render(&mut same);
        let settled = out.len() - SAMPLE_RATE as usize / 4;
        assert_eq!(out[settled..], same[lead + settled..], "{model:?}: the volume does not arrive");
    }
}

#[test]
fn the_mode_messages_are_all_notes_off_as_the_owners_manual_says() {
    for model in MODELS {
        for controller in 123..=127u8 {
            let mut juno = instrument(model, &flat(model, square()));
            // Seven keys, so that one voice has been taken from a key
            // that is still down; then the pedal, and one key up.
            for key in 60..67u8 {
                juno.event(NOTE_ON, key, 100);
                render(&mut juno, 0.01);
            }
            juno.event(CONTROL, 64, 127);
            juno.event(NOTE_OFF, 66, 0);
            juno.event(CONTROL | 3, controller, 0);
            render(&mut juno, 0.1);
            // As a note off for every key: the pedal holds them all.
            assert_eq!((juno.sounding_voices(), juno.ignored_events()), (VOICES, 0), "{model:?}: {controller}");
            juno.event(CONTROL, 64, 0);
            render(&mut juno, 0.1);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: {controller} left a note sounding");
            // And without the pedal, at once.
            juno.event(NOTE_ON, 60, 100);
            juno.event(CONTROL, controller, 0);
            render(&mut juno, 0.1);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: {controller} left a note sounding");
        }
    }
}

#[test]
fn what_is_no_message_of_the_instrument_is_counted_and_changes_nothing() {
    let mut juno = Juno::new(Model::Juno106);
    let mut same = Juno::new(Model::Juno106);
    for (status, first, second) in
        [(0xc0, 5, 0), (0xa0, 60, 9), (0xd0, 3, 0), (0xf8, 0, 0), (0x40, 1, 1), (CONTROL, 10, 5)]
    {
        juno.event(status, first, second);
    }
    // A data byte above 127 is no data byte.
    juno.event(NOTE_ON, 200, 100);
    juno.event(NOTE_ON, 60, 128);
    assert_eq!((juno.ignored_events(), juno.sounding_voices()), (8, 0));
    juno.event(NOTE_ON, 60, 1);
    same.event(NOTE_ON | 15, 60, 127);
    // Every channel is heard, and the velocity is not used.
    assert_eq!(render(&mut juno, 0.2), render(&mut same, 0.2));
    juno.reset();
    assert_eq!((juno.ignored_events(), juno.sounding_voices()), (0, 0));
    assert!(render(&mut juno, 0.1).iter().all(|&sample| sample == 0.0));
}

#[test]
fn the_wheel_adds_the_oscillators_swing_to_the_pitch() {
    for model in MODELS {
        let mut juno = instrument(model, &flat(model, square()).with(Slider::LfoRate, 100));
        juno.event(NOTE_ON, 69, 100);
        let steady = render(&mut juno, 0.5);
        juno.event(CONTROL, 1, 127);
        let moved = render(&mut juno, 0.5);
        // The spread of the tone's cycle lengths, in cents.
        let swing = |signal: &[f64]| {
            let mut crossings = Vec::new();
            for (index, pair) in signal.windows(2).enumerate() {
                if pair[0] < 0.0 && pair[1] >= 0.0 {
                    crossings.push(index as f64 + pair[0] / (pair[0] - pair[1]));
                }
            }
            let periods: Vec<f64> = crossings.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let (low, high) = periods.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &p| (low.min(p), high.max(p)));
            1200.0 * (high / low).log2()
        };
        let (before, after) = (swing(&steady[4_800..]), swing(&moved[4_800..]));
        println!("{model:?}: the wheel at its top swings the pitch {after:.1} cents, from {before:.1}");
        // 50 cents either way.
        assert!(before < 2.0 && (after - 100.0).abs() < 6.0, "{model:?}");
    }
}

/// Six voices of a sawtooth, a pulse and the sub-oscillator with the
/// filter moving, through the chorus: what a second of it costs.
#[test]
#[ignore = "a measurement, not a check: run it by name, in a release build, for the figure"]
fn rendering_speed() {
    for model in MODELS {
        let mut patch = Patch::DEFAULT
            .with(Slider::Sub, 100)
            .with(Slider::Noise, 30)
            .with(Slider::VcfFrequency, 70)
            .with(Slider::VcfResonance, 80)
            .with(Slider::VcfEnvelope, 60)
            .with(Slider::VcfLfo, 40)
            .with(Slider::LfoRate, 90)
            .with(Slider::DcoPwm, 100)
            .with(Slider::Decay, 90)
            .with(Slider::Sustain, 80);
        patch.pulse = true;
        patch.pwm = PwmSource::Lfo;
        patch.chorus = ChorusMode::One;
        let mut juno = instrument(model, &patch);
        for key in [36u8, 48, 55, 60, 64, 67] {
            juno.event(NOTE_ON, key, 100);
        }
        let seconds = 20;
        let mut out = vec![0i16; BLOCK_SAMPLES];
        let start = Instant::now();
        for _ in 0..seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES {
            juno.render(&mut out);
        }
        let taken = start.elapsed().as_secs_f64();
        assert_eq!(juno.sounding_voices(), VOICES);
        println!("{model:?}: six voices with chorus render {:.1} times faster than real time", seconds as f64 / taken);
    }
}
