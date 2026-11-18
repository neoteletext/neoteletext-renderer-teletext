//! synth106 as an instrument: its MIDI messages, its six voices, any
//! patch and any messages without a panic, the same samples from the
//! same input, and what it costs (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth106")]
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
#[path = "synth106_support.rs"]
mod support;

use std::time::Instant;

use analog::Random;
use neoteletext_audio_instruments::synth106::patch::PATCH_BYTES;
use neoteletext_audio_instruments::synth106::{ChorusMode, Model, Patch, PwmSource, Slider, Synth106, VOICES};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{CONTROL, MODELS, NOTE_OFF, NOTE_ON, PITCH_BEND, flat, instrument, render, rms, square, whistle};

/// A patch from random bytes: whatever synth106's 18 bytes can hold,
/// and at random what only synth60's panel has.
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
    let mut synth106 = Synth106::new(model);
    let mut out = vec![0i16; blocks * BLOCK_SAMPLES];
    for block in out.chunks_mut(BLOCK_SAMPLES) {
        if random.word().is_multiple_of(8) {
            synth106.set_patch(&random_patch(&mut random));
        }
        for _ in 0..random.word() % 4 {
            let (status, first, second) = random_event(&mut random);
            synth106.event(status, first, second);
        }
        for piece in block.chunks_mut(cut) {
            synth106.render(piece);
        }
        assert!(synth106.sounding_voices() <= VOICES);
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
    assert_ne!(random_run(Model::Synth106, 42, 150, BLOCK_SAMPLES), random_run(Model::Synth60, 42, 150, BLOCK_SAMPLES));
}

// Verifies S106-34: six voices, one a key round the six; a seventh key
// held does not sound (DEC-6465 answer 22); a key when fewer are held takes
// the voice whose release began first.
#[test]
fn s106_34_six_voices_and_a_seventh_key_held_does_not_sound() {
    for model in MODELS {
        let mut long = flat(model, square()).with(Slider::Release, 127);
        long.vca_gate = false;
        let mut synth106 = instrument(model, &long);
        for (count, key) in (60..67u8).enumerate() {
            synth106.event(NOTE_ON, key, 100);
            render(&mut synth106, 0.02);
            assert_eq!(synth106.sounding_voices(), (count + 1).min(VOICES), "{model:?}");
        }
        // The seventh, key 66, does not sound: its note off finds no voice,
        // and the six sound on.
        synth106.event(NOTE_OFF, 66, 0);
        render(&mut synth106, 0.02);
        assert_eq!(synth106.sounding_voices(), VOICES, "{model:?}: the seventh key took a voice");
        // Two keys up, 62 then 61, release slowly; a new key takes 62's
        // voice, whose release began first, and leaves 61's ringing.
        synth106.event(NOTE_OFF, 62, 0);
        render(&mut synth106, 0.01);
        synth106.event(NOTE_OFF, 61, 0);
        render(&mut synth106, 0.01);
        synth106.event(NOTE_ON, 70, 100);
        render(&mut synth106, 0.02);
        assert_eq!(synth106.sounding_voices(), VOICES, "{model:?}");
        // Key 61 is still sounding: a note on of it takes its own voice
        // again, so no voice is added and none taken from another key.
        synth106.event(NOTE_ON, 61, 100);
        render(&mut synth106, 0.02);
        assert_eq!(synth106.sounding_voices(), VOICES, "{model:?}");
    }
}

#[test]
fn the_pedal_holds_and_the_controllers_do_what_midi_says() {
    for model in MODELS {
        let mut synth106 = instrument(model, &flat(model, square()));
        let level = |synth106: &mut Synth106| rms(&render(synth106, 0.1));
        synth106.event(CONTROL, 64, 127);
        synth106.event(NOTE_ON, 60, 100);
        let held = level(&mut synth106);
        assert!(held > 0.05, "{model:?}: {held}");
        synth106.event(NOTE_OFF, 60, 0);
        assert!((level(&mut synth106) / held - 1.0).abs() < 0.05, "{model:?}: the pedal lets go");
        // All notes off is a note off for every key: the pedal holds on.
        synth106.event(NOTE_ON, 64, 100);
        synth106.event(CONTROL, 123, 0);
        render(&mut synth106, 0.1);
        assert_eq!(synth106.sounding_voices(), 2, "{model:?}");
        // The volume: half the controller is a quarter of the level.
        let both = level(&mut synth106);
        synth106.event(CONTROL, 7, 64);
        render(&mut synth106, 0.01);
        let quarter = level(&mut synth106) / both;
        assert!((quarter - (64.0f64 / 127.0).powi(2)).abs() < 0.02, "{model:?}: {quarter}");
        // Reset all controllers lifts the pedal and leaves the volume.
        synth106.event(CONTROL, 121, 0);
        render(&mut synth106, 0.1);
        assert_eq!(synth106.sounding_voices(), 0, "{model:?}");
        // All sound off silences at once what a long release would hold;
        // what is left is the chorus's and the filters' last samples.
        let mut long = flat(model, square()).with(Slider::Release, 127);
        long.vca_gate = false;
        synth106.set_patch(&long);
        synth106.event(NOTE_ON, 60, 100);
        render(&mut synth106, 0.1);
        synth106.event(NOTE_OFF, 60, 0);
        assert_eq!(synth106.sounding_voices(), 1);
        synth106.event(CONTROL, 120, 0);
        render(&mut synth106, 0.05);
        assert_eq!(synth106.sounding_voices(), 0, "{model:?}");
        assert!(level(&mut synth106) < 1.0e-4, "{model:?}: sound after all sound off");
    }
}

// Verifies S106-9 and MIDI-87: hold is off at 0 to 63 and on at 64 to 127
// on both models; the Juno-106's on at 1 to 127 is a departure (section
// 15.4).
#[test]
fn s106_9_hold_is_down_from_64_on_both_models() {
    for model in MODELS {
        for value in [0u8, 1, 2, 63, 64, 127] {
            let mut synth106 = instrument(model, &flat(model, square()));
            synth106.event(CONTROL, 64, value);
            synth106.event(NOTE_ON, 60, 100);
            render(&mut synth106, 0.05);
            synth106.event(NOTE_OFF, 60, 0);
            render(&mut synth106, 0.1);
            let held = usize::from(value >= 64);
            assert_eq!(synth106.sounding_voices(), held, "{model:?}: the pedal at {value}");
            synth106.event(CONTROL, 64, 0);
            render(&mut synth106, 0.1);
            assert_eq!(synth106.sounding_voices(), 0, "{model:?}: the pedal from {value} to 0");
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
            Model::Synth106 => 62,
            Model::Synth60 => 44,
        };
        let sine = flat(model, whistle(cutoff));
        // Half a second for the oscillation to grow; then twenty changes
        // of the volume 3,058 samples apart, the calls to render cut at
        // `cut` samples; then the volume at its half.
        let lead = SAMPLE_RATE as usize / 2;
        let run = |cut: usize| {
            let mut synth106 = instrument(model, &sine);
            synth106.event(NOTE_ON, 60, 100);
            let mut out = vec![0i16; 21 * 3_058 + SAMPLE_RATE as usize / 2];
            synth106.render(&mut vec![0i16; lead]);
            for (turn, piece) in out.chunks_mut(3_058).enumerate() {
                synth106.event(CONTROL, 7, [127, 64, 127, 0][turn.min(21) % 4]);
                for part in piece.chunks_mut(cut) {
                    synth106.render(part);
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
            let mut synth106 = instrument(model, &flat(model, square()));
            // Seven keys, the seventh of which does not sound (S106-34);
            // then the pedal, and the seventh key up.
            for key in 60..67u8 {
                synth106.event(NOTE_ON, key, 100);
                render(&mut synth106, 0.01);
            }
            synth106.event(CONTROL, 64, 127);
            synth106.event(NOTE_OFF, 66, 0);
            synth106.event(CONTROL | 3, controller, 0);
            render(&mut synth106, 0.1);
            // As a note off for every key: the pedal holds them all.
            assert_eq!((synth106.sounding_voices(), synth106.ignored_events()), (VOICES, 0), "{model:?}: {controller}");
            synth106.event(CONTROL, 64, 0);
            render(&mut synth106, 0.1);
            assert_eq!(synth106.sounding_voices(), 0, "{model:?}: {controller} left a note sounding");
            // And without the pedal, at once.
            synth106.event(NOTE_ON, 60, 100);
            synth106.event(CONTROL, controller, 0);
            render(&mut synth106, 0.1);
            assert_eq!(synth106.sounding_voices(), 0, "{model:?}: {controller} left a note sounding");
        }
    }
}

#[test]
fn what_is_no_message_of_the_instrument_is_counted_and_changes_nothing() {
    let mut synth106 = Synth106::new(Model::Synth106);
    let mut same = Synth106::new(Model::Synth106);
    for (status, first, second) in
        [(0xc0, 5, 0), (0xa0, 60, 9), (0xd0, 3, 0), (0xf8, 0, 0), (0x40, 1, 1), (CONTROL, 10, 5)]
    {
        synth106.event(status, first, second);
    }
    // A data byte above 127 is no data byte.
    synth106.event(NOTE_ON, 200, 100);
    synth106.event(NOTE_ON, 60, 128);
    assert_eq!((synth106.ignored_events(), synth106.sounding_voices()), (8, 0));
    synth106.event(NOTE_ON, 60, 1);
    same.event(NOTE_ON | 15, 60, 127);
    // Every channel is heard, and the velocity is not used.
    assert_eq!(render(&mut synth106, 0.2), render(&mut same, 0.2));
    synth106.reset();
    assert_eq!((synth106.ignored_events(), synth106.sounding_voices()), (0, 0));
    assert!(render(&mut synth106, 0.1).iter().all(|&sample| sample == 0.0));
}

#[test]
fn the_wheel_adds_the_oscillators_swing_to_the_pitch() {
    for model in MODELS {
        let mut synth106 = instrument(model, &flat(model, square()).with(Slider::LfoRate, 100));
        synth106.event(NOTE_ON, 69, 100);
        let steady = render(&mut synth106, 0.5);
        synth106.event(CONTROL, 1, 127);
        let moved = render(&mut synth106, 0.5);
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
        let mut synth106 = instrument(model, &patch);
        for key in [36u8, 48, 55, 60, 64, 67] {
            synth106.event(NOTE_ON, key, 100);
        }
        let seconds = 20;
        let mut out = vec![0i16; BLOCK_SAMPLES];
        let start = Instant::now();
        for _ in 0..seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES {
            synth106.render(&mut out);
        }
        let taken = start.elapsed().as_secs_f64();
        assert_eq!(synth106.sounding_voices(), VOICES);
        println!("{model:?}: six voices with chorus render {:.1} times faster than real time", seconds as f64 / taken);
    }
}

/// Six voices whose keys are up, the amplifier on the gate and the
/// release at its longest: silent within thousandths of a second, and
/// their envelopes run on for 54 s, with their oscillators and filters
/// (TODO-6438).
#[test]
#[ignore = "a measurement, not a check: run it by name, in a release build, for the figure"]
fn the_cost_of_voices_that_linger_unheard() {
    for model in MODELS {
        let mut patch = flat(model, Patch::DEFAULT).with(Slider::Release, 127);
        patch.vca_gate = true;
        let seconds = 20;
        let speed = |synth106: &mut Synth106| {
            let mut out = vec![0i16; BLOCK_SAMPLES];
            let start = Instant::now();
            for _ in 0..seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES {
                synth106.render(&mut out);
            }
            seconds as f64 / start.elapsed().as_secs_f64()
        };
        let mut synth106 = instrument(model, &patch);
        let rest = speed(&mut synth106);
        for key in [36u8, 48, 55, 60, 64, 67] {
            synth106.event(NOTE_ON, key, 100);
        }
        let held = speed(&mut synth106);
        assert_eq!(synth106.sounding_voices(), VOICES);
        for key in [36u8, 48, 55, 60, 64, 67] {
            synth106.event(NOTE_OFF, key, 0);
        }
        render(&mut synth106, 0.5);
        let lingering = speed(&mut synth106);
        assert_eq!(synth106.sounding_voices(), VOICES, "{model:?}: the envelopes still run");
        println!(
            "{model:?}: times faster than real time: {rest:.0} with every voice at rest, {held:.1} with six keys held, {lingering:.1} with six voices lingering unheard"
        );
    }
}

#[test]
fn all_sound_off_leaves_the_pedal_where_it_is() {
    // All sound off silences what sounds; the pedal has not moved. It let
    // go of the pedal with the keys, so a note played after it under a
    // pedal still down ended with its key (TODO-6438).
    for model in MODELS {
        let mut synth106 = instrument(model, &flat(model, Patch::DEFAULT));
        synth106.event(CONTROL, 64, 127);
        synth106.event(NOTE_ON, 60, 100);
        render(&mut synth106, 0.1);
        synth106.event(CONTROL, 120, 0);
        render(&mut synth106, 0.1);
        assert_eq!(synth106.sounding_voices(), 0, "{model:?}: silenced");
        synth106.event(NOTE_ON, 64, 100);
        render(&mut synth106, 0.1);
        synth106.event(NOTE_OFF, 64, 0);
        render(&mut synth106, 0.5);
        assert_eq!(synth106.sounding_voices(), 1, "{model:?}: the pedal no longer holds");
        synth106.event(CONTROL, 64, 0);
        render(&mut synth106, 0.5);
        assert_eq!(synth106.sounding_voices(), 0, "{model:?}: the pedal up lets go");
    }
}
