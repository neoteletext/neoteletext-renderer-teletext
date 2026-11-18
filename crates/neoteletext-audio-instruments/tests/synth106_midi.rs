//! synth106 and synth60 as the receiver plays them, through
//! `synth106::midi::Midi` (`neoteletext-common/docs/audio/synth106-synth60.md`,
//! "S106-n"): what each message does to the sound.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth106")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "synth106_support.rs"]
mod support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, status};
use neoteletext_audio_instruments::synth106::interface::{self, MOST_PATCH_BYTES, ju06a};
use neoteletext_audio_instruments::synth106::midi::Midi;
use neoteletext_audio_instruments::synth106::{Model, Patch, Slider, Synth106};
use support::{MODELS, cents, flat, pitch, samples, square};

/// A bank of one patch, program 0, of `model`.
fn bank(model: Model, patch: &Patch) -> Vec<u8> {
    let mut bytes = [0; MOST_PATCH_BYTES];
    let length = interface::pack_patch(model, patch, 0, 0, &mut bytes);
    bytes[..length].to_vec()
}

/// An instrument started on a bank whose program 0 is `patch`.
fn started(model: Model, image: &[u8]) -> Midi {
    let mut midi = Midi::new(Synth106::new(model));
    assert_eq!(midi.start(&Banks::new(&[image])), Outcome::Applied);
    midi
}

fn channel(kind: u8, channel: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, channel, data1, data2))
}

fn note_on(key: u8, velocity: u8) -> Message<'static> {
    channel(status::NOTE_ON, 0, key, velocity)
}

fn cc(number: u8, value: u8) -> Message<'static> {
    channel(status::CONTROL_CHANGE, 0, number, value)
}

/// Renders `count` samples.
fn render(midi: &mut Midi, count: usize) -> Vec<i16> {
    let mut out = vec![0; count];
    midi.render(&mut out);
    out
}

/// Plays `score`, each entry a sample to render up to and the messages
/// given there, then renders to `total` samples.
fn play(model: Model, patch: &Patch, score: &[(usize, Vec<Message<'_>>)], total: usize) -> Vec<i16> {
    let image = bank(model, patch);
    let banks = Banks::new(&[&image]);
    let mut midi = started(model, &image);
    let mut out = Vec::new();
    for (at, messages) in score {
        out.extend(render(&mut midi, at - out.len()));
        for message in messages {
            midi.midi(message, &banks);
        }
    }
    out.extend(render(&mut midi, total - out.len()));
    out
}

fn as_float(signal: &[i16]) -> Vec<f64> {
    signal.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

/// A tenth of a second, in samples.
const TENTH: usize = 4_800;

// Verifies S106-3: the velocity does not change the sound.
#[test]
fn s106_3_velocity_changes_nothing() {
    for model in MODELS {
        let patch = flat(model, square());
        let run = |velocity| play(model, &patch, &[(0, vec![note_on(60, velocity)])], 3 * TENTH);
        let one = run(1);
        assert!(one.iter().any(|&sample| sample != 0));
        assert_eq!(one, run(64), "{model:?}");
        assert_eq!(one, run(127), "{model:?}");
    }
}

// Verifies S106-4 and MIDI-88: a note on of velocity 0 and note offs of
// release velocity 0 and 127 release alike.
#[test]
fn s106_4_note_off_in_every_form() {
    for model in MODELS {
        let patch = flat(model, square()).with(Slider::Release, 60);
        let run =
            |off: Message<'static>| play(model, &patch, &[(0, vec![note_on(60, 100)]), (TENTH, vec![off])], 5 * TENTH);
        let zero = run(note_on(60, 0));
        assert_eq!(zero, run(channel(status::NOTE_OFF, 0, 60, 0)), "{model:?}");
        assert_eq!(zero, run(channel(status::NOTE_OFF, 0, 60, 127)), "{model:?}");
    }
}

// Verifies S106-2: one score on each channel, and spread over all
// sixteen, gives the same samples.
#[test]
fn s106_2_all_channels_are_one_instrument() {
    for model in MODELS {
        let patch = flat(model, square());
        let score = |on: &dyn Fn(usize) -> u8| -> Vec<(usize, Vec<Message<'static>>)> {
            let messages = [
                (status::CONTROL_CHANGE, 101, 0),
                (status::CONTROL_CHANGE, 100, 0),
                (status::CONTROL_CHANGE, 6, 5),
                (status::NOTE_ON, 60, 100),
                (status::PITCH_BEND, 0, 80),
                (status::CONTROL_CHANGE, 64, 127),
                (status::NOTE_OFF, 60, 0),
                (status::CONTROL_CHANGE, ju06a::VCF_FREQUENCY, 40),
                (status::CONTROL_CHANGE, 64, 0),
            ];
            messages
                .iter()
                .enumerate()
                .map(|(index, &(kind, first, second))| (index * 960, vec![channel(kind, on(index), first, second)]))
                .collect()
        };
        let total = 12 * 960;
        let first = play(model, &patch, &score(&|_| 0), total);
        for one in 1..16u8 {
            assert_eq!(play(model, &patch, &score(&|_| one), total), first, "{model:?}: channel {one}");
        }
        assert_eq!(play(model, &patch, &score(&|index| (index * 7 % 16) as u8), total), first, "{model:?}: spread");
    }
}

// Verifies S106-5: keys outside 24 to 108 sound folded by whole octaves,
// and a key is known by the key received.
#[test]
fn s106_5_keys_outside_the_range_sound_folded_by_octaves() {
    let patch = flat(Model::Synth106, square());
    let alone = |key| play(Model::Synth106, &patch, &[(0, vec![note_on(key, 100)])], 2 * TENTH);
    for (key, sounds) in
        [(0, 24), (11, 35), (12, 24), (23, 35), (24, 24), (108, 108), (109, 97), (120, 108), (127, 103)]
    {
        assert_eq!(alone(key), alone(sounds), "key {key} sounds as {sounds}");
    }
    for model in MODELS {
        let image = bank(model, &flat(model, square()));
        let banks = Banks::new(&[&image]);
        let mut midi = started(model, &image);
        midi.midi(&note_on(24, 100), &banks);
        midi.midi(&note_on(12, 100), &banks);
        render(&mut midi, TENTH);
        assert_eq!(midi.engine().sounding_voices(), 2);
        midi.midi(&channel(status::NOTE_OFF, 0, 12, 0), &banks);
        render(&mut midi, 2 * TENTH);
        assert_eq!(midi.engine().sounding_voices(), 1, "{model:?}: key 12's note off released key 24");
    }
}

// Verifies S106-6: a note whose note off comes 1 ms after its note on
// sounds.
#[test]
fn s106_6_a_short_note_sounds() {
    for model in MODELS {
        let patch = flat(model, square()).with(Slider::Release, 40);
        let out = play(model, &patch, &[(0, vec![note_on(60, 100)]), (48, vec![note_on(60, 0)])], 2 * TENTH);
        assert!(out.iter().any(|&sample| sample.unsigned_abs() > 300), "{model:?}");
    }
}

/// The pitch of key 69 held under `messages`, given before it.
fn bent_pitch(model: Model, messages: Vec<Message<'static>>) -> f64 {
    let mut score = vec![(0, messages)];
    score.push((0, vec![note_on(69, 100)]));
    let out = play(model, &flat(model, square()), &score, samples(1.0));
    pitch(&as_float(&out)[samples(0.2)..])
}

// Verifies S106-7 and S106-26: the bend uses all 14 bits of its range,
// which controller 87 and registered parameter 0 set, at most twelve
// semitones. The Juno-106's divider puts a bend within a cent or so.
#[test]
fn s106_7_and_s106_26_the_bend_and_its_range() {
    for model in MODELS {
        let centre = bent_pitch(model, vec![]);
        let bend = |low, high, range: Vec<Message<'static>>| {
            let mut messages = range;
            messages.push(channel(status::PITCH_BEND, 0, low, high));
            cents(centre, bent_pitch(model, messages))
        };
        for ((low, high), expected) in [((0, 0), -200.0), ((0, 64), 0.0), ((0, 96), 100.0), ((127, 127), 199.975_6)] {
            let found = bend(low, high, vec![]);
            assert!((found - expected).abs() < 1.5, "{model:?} ({low}, {high}): {found} cents");
        }
        let rpn = |semitones| vec![cc(101, 0), cc(100, 0), cc(6, semitones)];
        for (range, expected) in [(rpn(1), 100.0), (rpn(7), 700.0), (rpn(24), 1_200.0), (vec![cc(87, 30)], 300.0)] {
            let found = bend(127, 127, range);
            let mut wanted: f64 = expected * 8_191.0 / 8_192.0;
            if model == Model::Synth60 {
                // synth60's bend moves its master oscillator, which the
                // service notes bound at 3.5 MHz from its 1.894 MHz at rest
                // (page 14, "Master Oscillators": "from 1MHz to 3.5MHz"):
                // 1,063 cents up, short of the twelve semitones S106-26
                // allows. The engine keeps the bound; the departure is
                // reported for the owner.
                wanted = wanted.min(1_200.0 * (3.5e6_f64 / (4_305.0 * 440.0)).log2());
            }
            assert!((found - wanted).abs() < 2.0, "{model:?}: {found} cents for {wanted}");
        }
    }
}

// Verifies S106-20: a patch message and a note at one tick, in either
// order, give the note the new patch: the panel moves the voices that
// sound at once (S106-13, "at that instant"), so the order at one tick is
// not heard; a note a block before the message began with the old patch.
#[test]
fn s106_20_a_patch_message_takes_effect_in_the_blocks_order() {
    for model in MODELS {
        let old = flat(model, square());
        let new = old.with(Slider::VcfFrequency, 40);
        let mut message = [0; MOST_PATCH_BYTES];
        interface::pack_patch(model, &new, 0, 0, &mut message);
        let exclusive = Message::SystemExclusive(&message[1..interface::PATCH_MESSAGE_BYTES]);
        let before = play(model, &old, &[(0, vec![exclusive, note_on(60, 100)])], 2 * TENTH);
        let reference = play(model, &new, &[(0, vec![note_on(60, 100)])], 2 * TENTH);
        assert_eq!(before, reference, "{model:?}: the note has the new patch");
        let after = play(model, &old, &[(0, vec![note_on(60, 100), exclusive])], 2 * TENTH);
        assert_eq!(after, reference, "{model:?}");
        let earlier = play(model, &old, &[(0, vec![note_on(60, 100)]), (960, vec![exclusive])], 2 * TENTH);
        let alone = play(model, &old, &[(0, vec![note_on(60, 100)])], 2 * TENTH);
        assert_eq!(earlier[..960], alone[..960], "{model:?}: the old patch until the message");
        assert_ne!(earlier[960..], alone[960..], "{model:?}: the new patch from the message");
    }
}

// Verifies S106-29 item 1 (MIDI-86): all sound off brings every voice to
// rest within 1,024 samples, held, sustained and releasing alike.
#[test]
fn s106_29_all_sound_off_within_1024_samples() {
    for model in MODELS {
        let mut patch = flat(model, square()).with(Slider::Release, 127);
        patch.vca_gate = false;
        let image = bank(model, &patch);
        let banks = Banks::new(&[&image]);
        let mut midi = started(model, &image);
        midi.midi(&note_on(60, 100), &banks);
        midi.midi(&note_on(64, 100), &banks);
        midi.midi(&cc(64, 127), &banks);
        midi.midi(&note_on(67, 100), &banks);
        render(&mut midi, TENTH);
        midi.midi(&channel(status::NOTE_OFF, 0, 64, 0), &banks);
        midi.midi(&cc(64, 0), &banks);
        render(&mut midi, 100);
        assert_eq!(midi.engine().sounding_voices(), 3);
        midi.midi(&cc(120, 0), &banks);
        render(&mut midi, 1_024);
        assert_eq!(midi.engine().sounding_voices(), 0, "{model:?}");
    }
}

// Verifies S106-35: controller 86 is counted at every value and changes
// nothing.
#[test]
fn s106_35_the_assignment_controller_is_counted() {
    for model in MODELS {
        let patch = flat(model, square());
        let image = bank(model, &patch);
        let banks = Banks::new(&[&image]);
        let mut midi = started(model, &image);
        for value in 0..=127 {
            assert_eq!(midi.midi(&cc(ju06a::ASSIGN_MODE, value), &banks), Outcome::PassedOver);
        }
        midi.midi(&note_on(60, 100), &banks);
        let plain = play(model, &patch, &[(0, vec![note_on(60, 100)])], TENTH);
        assert_eq!(render(&mut midi, TENTH), plain, "{model:?}");
    }
}

/// A score that moves the panel, the bend, its range, the volume and hold
/// while notes sound: a message every few blocks.
fn busy(block: usize) -> Vec<Message<'static>> {
    match block {
        0 => vec![note_on(60, 100), note_on(67, 100)],
        3 => vec![cc(ju06a::VCF_FREQUENCY, 70), cc(ju06a::CHORUS, 96)],
        5 => vec![cc(87, 50), channel(status::PITCH_BEND, 0, 0, 90)],
        7 => vec![cc(64, 127), channel(status::NOTE_OFF, 0, 60, 0)],
        9 => vec![cc(7, 90), cc(1, 64), cc(ju06a::DCO_PWM_SOURCE, 86)],
        11 => vec![note_on(64, 100), cc(64, 0)],
        13 => vec![cc(101, 0), cc(100, 0), cc(6, 4), cc(101, 127), cc(100, 127)],
        15 => vec![channel(status::PROGRAM_CHANGE, 0, 1, 0), note_on(72, 100)],
        17 => vec![cc(ju06a::ENV_RELEASE, 90)],
        _ => vec![],
    }
}

// Verifies S106-40 and MIDI-97: a restatement of the state an instrument
// already holds, at the start of every block, changes no sample.
#[test]
fn s106_40_a_restatement_of_the_state_held_is_not_heard() {
    for model in MODELS {
        let patches = [flat(model, square()), flat(model, square()).with(Slider::VcfFrequency, 90)];
        let mut image = vec![0; 2 * interface::patch_bytes(model)];
        interface::build_bank(model, &patches, 0, &mut image).unwrap();
        let banks = Banks::new(&[&image]);
        let run = |restate: bool| {
            let mut midi = started(model, &image);
            let mut out = Vec::new();
            for block in 0..25 {
                if restate {
                    let mut messages: Vec<Result<ChannelMessage, Vec<u8>>> = Vec::new();
                    midi.tracker().restate(0, &mut |message| {
                        messages.push(match message {
                            Message::Channel(message) => Ok(message),
                            Message::SystemExclusive(bytes) => Err(bytes.to_vec()),
                        });
                    });
                    for message in &messages {
                        let message = match message {
                            Ok(message) => Message::Channel(*message),
                            Err(bytes) => Message::SystemExclusive(bytes),
                        };
                        assert_ne!(midi.midi(&message, &banks), Outcome::PassedOver);
                    }
                }
                for message in busy(block) {
                    midi.midi(&message, &banks);
                }
                out.extend(render(&mut midi, 960));
            }
            out
        };
        let plain = run(false);
        assert!(plain.iter().any(|&sample| sample.unsigned_abs() > 300));
        assert_eq!(run(true), plain, "{model:?}");
    }
}

// Verifies S106-40 and MIDI-102: a lost block releases every note and
// lifts hold; the panel stays.
#[test]
fn s106_40_a_lost_block_releases_every_note() {
    for model in MODELS {
        let image = bank(model, &flat(model, square()));
        let banks = Banks::new(&[&image]);
        let mut midi = started(model, &image);
        midi.midi(&cc(ju06a::VCF_FREQUENCY, 50), &banks);
        midi.midi(&cc(64, 127), &banks);
        midi.midi(&note_on(60, 100), &banks);
        render(&mut midi, TENTH);
        let panel = *midi.panel();
        midi.lost();
        render(&mut midi, 2 * TENTH);
        assert_eq!(midi.engine().sounding_voices(), 0, "{model:?}");
        assert!(!midi.tracker().common().channel(0).sustain);
        assert_eq!(*midi.panel(), panel);
    }
}
