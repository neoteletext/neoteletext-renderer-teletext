//! synth8's adopted interface as the receiver plays it
//! (`neoteletext-common/docs/audio/synth8.md`, cited "S8-n"): the messages
//! it takes and on which channels, the panel's controllers, program change
//! from the bank, the common behaviours, the bend depth by controller 41
//! and by registered parameter number 0, a lost block, and the server's
//! restatement.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth8")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "a test"
)]

#[path = "synth8_support.rs"]
mod support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, status};
use neoteletext_audio_instruments::synth8::interface::{Tracker, accepts, build_bank};
use neoteletext_audio_instruments::synth8::midi::Midi;
use neoteletext_audio_instruments::synth8::{KeyMode, PATCH_BYTES, Param, Patch, Synth8};
use support::{A2, CONTROL, NOTE_OFF, NOTE_ON, PITCH_BEND, cents, pitch, rms, samples, square};

/// Program change.
const PROGRAM: u8 = 0xc0;

/// A message on a channel, 0 to 15.
fn message(kind: u8, channel: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, channel, data1, data2))
}

/// A bank of `count` patches, patch k's LFO RATE at k, which with no
/// modulation is not heard: each tells which it is.
fn bank(count: usize) -> Vec<u8> {
    let patches: Vec<Patch> = (0..count).map(|number| square().with(Param::LfoRate, number as u8)).collect();
    let mut image = vec![0u8; count * PATCH_BYTES];
    assert_eq!(build_bank(&patches, &mut image), Ok(image.len()));
    image
}

/// synth8 in a key mode, started from `image` as bank index 0.
fn started(key_mode: KeyMode, image: &[u8]) -> Midi {
    let mut synth = Synth8::new();
    synth.set_key_mode(key_mode);
    let mut midi = Midi::new(synth);
    let _ = midi.start(&Banks::new(&[image]));
    midi
}

/// Sends one message.
fn send(midi: &mut Midi, image: &[u8], message: &Message<'_>) -> Outcome {
    midi.midi(message, &Banks::new(&[image]))
}

/// Renders so many seconds, as full scale 1.
fn play(midi: &mut Midi, seconds: f64) -> Vec<f64> {
    let mut out = vec![0i16; samples(seconds)];
    midi.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

#[test]
fn s8_1_the_messages_synth8_takes_and_no_others() {
    // Every status from 0x80 to 0xEF on every channel, with data that a
    // taken message acts on: which are taken is S8-1's list.
    let image = bank(4);
    for status_byte in 0x80..=0xefu8 {
        let mut midi = started(KeyMode::Dual, &image);
        let kind = status_byte & 0xf0;
        let channel = status_byte & 0x0f;
        let (data1, data2) = match kind {
            0xb0 => (3, 20),
            0xc0 => (1, 0),
            _ => (60, 100),
        };
        let found = send(&mut midi, &image, &message(kind, channel, data1, data2));
        let half = channel < 2;
        let wanted = match kind {
            0x80 | 0x90 | 0xe0 => Outcome::Applied,
            0xb0 | 0xc0 if half => Outcome::Applied,
            _ => Outcome::PassedOver,
        };
        assert_eq!(found, wanted, "status {status_byte:#04x}");
        assert_eq!(accepts(&message(kind, channel, data1, data2)).is_ok(), wanted == Outcome::Applied);
    }
    // System exclusive is never taken (S8-29 (c)).
    let mut midi = started(KeyMode::Whole, &image);
    assert_eq!(send(&mut midi, &image, &Message::SystemExclusive(&[0x41, 0x10, 0xf7])), Outcome::PassedOver);
    assert!(accepts(&Message::SystemExclusive(&[0x41, 0xf7])).is_err());
}

#[test]
fn s8_2_notes_on_every_channel_play_one_keyboard() {
    let image = bank(1);
    let played = |channel: u8| {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(status::NOTE_ON, channel, A2, 100));
        play(&mut midi, 0.2)
    };
    let first = played(0);
    for channel in 1..16 {
        assert_eq!(played(channel), first, "channel {}", channel + 1);
    }
}

#[test]
fn s8_2_panel_controllers_on_channel_1_move_the_upper_half_and_on_2_the_lower() {
    let image = bank(1);
    let mut midi = started(KeyMode::Dual, &image);
    let cutoff =
        |midi: &Midi| (midi.synth().patch().get(Param::VcfCutoff), midi.synth().lower_patch().get(Param::VcfCutoff));
    assert_eq!(cutoff(&midi), (127, 127));
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 3, 50)), Outcome::Applied);
    assert_eq!(cutoff(&midi), (50, 127));
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 1, 3, 70)), Outcome::Applied);
    assert_eq!(cutoff(&midi), (50, 70));
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 2, 3, 90)), Outcome::PassedOver);
    assert_eq!(cutoff(&midi), (50, 70));
    assert_eq!(midi.synth().ignored_events(), 1);
}

#[test]
fn s8_16_each_controller_moves_exactly_its_control() {
    for (channel, upper) in [(0u8, true), (1, false)] {
        for param in Param::ALL {
            let image = bank(1);
            let mut midi = started(KeyMode::Dual, &image);
            let before = (*midi.synth().patch(), *midi.synth().lower_patch());
            let value = if before.0.get(param) == 100 { 101 } else { 100 };
            assert_eq!(
                send(&mut midi, &image, &message(CONTROL, channel, param.controller(), value)),
                Outcome::Applied
            );
            let (moved, kept) = if upper {
                (*midi.synth().patch(), *midi.synth().lower_patch())
            } else {
                (*midi.synth().lower_patch(), *midi.synth().patch())
            };
            let original = if upper { before.0 } else { before.1 };
            assert_eq!(moved, original.with(param, value), "{param:?} on channel {}", channel + 1);
            assert_eq!(kept, if upper { before.1 } else { before.0 });
        }
    }
    // Controller 63 moves VCO-2 RANGE as 62 does, in either position of
    // the NORMAL / LOW FREQ switch.
    for low in [0u8, 127] {
        let image = bank(1);
        let mut by_62 = started(KeyMode::Whole, &image);
        let mut by_63 = started(KeyMode::Whole, &image);
        for midi in [&mut by_62, &mut by_63] {
            let _ = send(midi, &image, &message(CONTROL, 0, 57, low));
        }
        let _ = send(&mut by_62, &image, &message(CONTROL, 0, 62, 90));
        let _ = send(&mut by_63, &image, &message(CONTROL, 0, 63, 90));
        assert_eq!(by_62.synth().patch(), by_63.synth().patch());
        assert_eq!(by_63.synth().patch().vco2_range(), 26);
    }
    // Every controller no requirement names is counted.
    let named = |number: u8| {
        Param::of_controller(number).is_some()
            || [1, 5, 6, 7, 8, 11, 38, 41, 64, 96, 97, 98, 99, 100, 101, 118, 120, 121, 123, 124, 125, 126, 127]
                .contains(&number)
    };
    for number in (0..=127).filter(|&number| !named(number)) {
        let image = bank(1);
        let mut midi = started(KeyMode::Whole, &image);
        assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, number, 64)), Outcome::PassedOver, "{number}");
        assert_eq!(*midi.synth().patch(), *started(KeyMode::Whole, &image).synth().patch());
    }
}

#[test]
fn s8_17_no_controller_is_another_s_fine_part() {
    let image = bank(1);
    let mut midi = started(KeyMode::Whole, &image);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 3, 40));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 35, 86));
    let patch = midi.synth().patch();
    assert_eq!((patch.get(Param::VcfCutoff), patch.get(Param::LfoWave)), (40, 86));
}

#[test]
fn s8_3_velocity_changes_nothing_and_velocity_0_is_a_note_off() {
    let image = bank(1);
    let chord = |velocity: u8| {
        let mut midi = started(KeyMode::Whole, &image);
        for key in [57, 61, 64] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, velocity));
        }
        play(&mut midi, 0.2)
    };
    assert_eq!(chord(1), chord(64));
    assert_eq!(chord(64), chord(127));
    let released = |off: Message<'static>| {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, A2, 100));
        play(&mut midi, 0.1);
        let _ = send(&mut midi, &image, &off);
        play(&mut midi, 0.2)
    };
    assert_eq!(released(message(status::NOTE_ON, 0, A2, 0)), released(message(status::NOTE_OFF, 0, A2, 64)));
}

#[test]
fn s8_12_program_change_chooses_a_patch_of_the_bank() {
    let image = bank(64);
    let mut midi = started(KeyMode::Dual, &image);
    // At the start both halves play program 0.
    assert_eq!(midi.synth().patch().get(Param::LfoRate), 0);
    for program in [0u8, 7, 8, 63] {
        assert_eq!(send(&mut midi, &image, &message(PROGRAM, 0, program, 0)), Outcome::Applied);
        assert_eq!(midi.synth().patch().get(Param::LfoRate), program);
        assert_eq!(send(&mut midi, &image, &message(PROGRAM, 1, program, 0)), Outcome::Applied);
        assert_eq!(midi.synth().lower_patch().get(Param::LfoRate), program);
    }
    // Channel 16 addresses no half.
    assert_eq!(send(&mut midi, &image, &message(PROGRAM, 15, 5, 0)), Outcome::PassedOver);
    assert_eq!(midi.synth().patch().get(Param::LfoRate), 63);
    // 64 and a program past the bank set the default and are counted.
    assert_eq!(send(&mut midi, &image, &message(PROGRAM, 0, 64, 0)), Outcome::PassedOver);
    assert_eq!(*midi.synth().patch(), Patch::DEFAULT);
    let small = bank(4);
    let mut midi = started(KeyMode::Whole, &small);
    assert_eq!(send(&mut midi, &small, &message(PROGRAM, 0, 4, 0)), Outcome::PassedOver);
    assert_eq!(*midi.synth().patch(), Patch::DEFAULT);
    // A faulty patch is refused and the default set (S8-23, MIDI-63).
    let mut faulty = bank(2);
    faulty[PATCH_BYTES + 3] = 0x80;
    let mut midi = started(KeyMode::Whole, &faulty);
    assert_eq!(send(&mut midi, &faulty, &message(PROGRAM, 0, 1, 0)), Outcome::PatchRefused);
    assert_eq!(*midi.synth().patch(), Patch::DEFAULT);
    // No bank at all: the default patch, no fault at the start.
    let mut midi = Midi::new(Synth8::new());
    assert_eq!(midi.start(&Banks::none()), Outcome::Applied);
    assert_eq!(*midi.synth().patch(), Patch::DEFAULT);
}

#[test]
fn s8_12_a_program_change_to_the_patch_playing_changes_nothing_heard() {
    let image = bank(8);
    let run = |again: bool| {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(PROGRAM, 0, 5, 0));
        let _ = send(&mut midi, &image, &message(CONTROL, 1, 64, 127));
        let _ = send(&mut midi, &image, &message(PITCH_BEND, 0, 0, 80));
        for key in [57, 61, 64] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
        let mut out = play(&mut midi, 0.3);
        if again {
            let _ = send(&mut midi, &image, &message(PROGRAM, 0, 5, 0));
        }
        out.extend(play(&mut midi, 0.3));
        out
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn s8_13_reset_all_controllers_is_rp_015_and_leaves_what_the_list_leaves() {
    let image = bank(1);
    // Set everything the list names; reset on channel 5; then compare
    // with an instrument that had the left controls set alone.
    let set = |midi: &mut Midi, all: bool| {
        let _ = send(midi, &image, &message(CONTROL, 0, 3, 40));
        let _ = send(midi, &image, &message(CONTROL, 0, 7, 90));
        let _ = send(midi, &image, &message(CONTROL, 0, 8, 100));
        let _ = send(midi, &image, &message(CONTROL, 0, 5, 30));
        let _ = send(midi, &image, &message(CONTROL, 0, 118, 127));
        let _ = send(midi, &image, &message(CONTROL, 0, 41, 127));
        if all {
            let _ = send(midi, &image, &message(CONTROL, 0, 1, 127));
            let _ = send(midi, &image, &message(CONTROL, 0, 11, 30));
            let _ = send(midi, &image, &message(PITCH_BEND, 0, 0, 0));
            let _ = send(midi, &image, &message(CONTROL, 3, 101, 0));
            let _ = send(midi, &image, &message(CONTROL, 3, 100, 0));
        }
    };
    let mut reset = started(KeyMode::Dual, &image);
    set(&mut reset, true);
    assert_eq!(send(&mut reset, &image, &message(CONTROL, 4, 121, 0)), Outcome::Applied);
    let mut plain = started(KeyMode::Dual, &image);
    set(&mut plain, false);
    assert_eq!(reset.synth().patch(), plain.synth().patch());
    assert_eq!(reset.synth().performance(), plain.synth().performance());
    assert_eq!(reset.synth().performance().bend_vco, 1_200);
    for midi in [&mut reset, &mut plain] {
        let _ = send(midi, &image, &message(status::NOTE_ON, 0, A2, 100));
    }
    assert_eq!(play(&mut reset, 0.3), play(&mut plain, 0.3), "wheel, expression and bender put back");
    // The selection of channel 4 is none: a data entry there changes
    // nothing.
    assert_eq!(send(&mut reset, &image, &message(CONTROL, 3, 6, 2)), Outcome::PassedOver);
    assert_eq!(reset.synth().performance().bend_vco, 1_200);
    // The hold goes off and lets go what it held; keys held stay.
    let mut held = started(KeyMode::Whole, &image);
    let _ = send(&mut held, &image, &message(CONTROL, 0, 64, 127));
    let _ = send(&mut held, &image, &message(status::NOTE_ON, 0, 60, 100));
    let _ = send(&mut held, &image, &message(status::NOTE_OFF, 0, 60, 0));
    let _ = send(&mut held, &image, &message(status::NOTE_ON, 0, 67, 100));
    play(&mut held, 0.2);
    assert_eq!(held.synth().sounding_voices(), 2);
    let _ = send(&mut held, &image, &message(CONTROL, 0, 121, 0));
    play(&mut held, 0.3);
    assert_eq!(held.synth().sounding_voices(), 1);
}

#[test]
fn s8_13_all_notes_off_and_the_mode_messages_release_the_keys() {
    let image = bank(1);
    for number in [123u8, 124, 125, 126, 127] {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 64, 127));
        let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, 60, 100));
        let _ = send(&mut midi, &image, &message(status::NOTE_OFF, 0, 60, 0));
        for key in [64, 67] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
        play(&mut midi, 0.1);
        assert_eq!(send(&mut midi, &image, &message(CONTROL, 9, number, 0)), Outcome::Applied);
        play(&mut midi, 0.3);
        // As note offs: the hold holds them all, until it goes off
        // (MIDI-85).
        assert_eq!(midi.synth().sounding_voices(), 3, "controller {number}");
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 64, 0));
        play(&mut midi, 0.3);
        assert_eq!(midi.synth().sounding_voices(), 0, "controller {number}");
        // Without the hold, the keys under fingers are let go.
        for key in [64, 67] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
        play(&mut midi, 0.1);
        let _ = send(&mut midi, &image, &message(CONTROL, 9, number, 0));
        play(&mut midi, 0.3);
        assert_eq!(midi.synth().sounding_voices(), 0, "controller {number}");
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 64, 127));
        let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, 60, 100));
        play(&mut midi, 0.1);
        // All sound off silences them all within 1,024 samples.
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 120, 0));
        let mut out = vec![0i16; 2_048];
        midi.render(&mut out);
        assert!(out[1_024..].iter().all(|&sample| sample == 0), "controller {number}");
        assert_eq!(midi.synth().sounding_voices(), 0);
    }
}

/// The bend depth an instrument has, in cents, measured by a full bend
/// down of the square's A.
fn bent_cents(midi: &mut Midi, image: &[u8]) -> f64 {
    let _ = send(midi, image, &message(PITCH_BEND, 0, 0, 0));
    let _ = send(midi, image, &message(status::NOTE_ON, 0, A2, 100));
    let found = pitch(&play(midi, 0.8)[samples(0.2)..]);
    let _ = send(midi, image, &message(CONTROL, 0, 120, 0));
    play(midi, 0.05);
    -cents(440.0, found)
}

#[test]
fn s8_14_controller_41_sets_the_vco_bend_depth() {
    let image = bank(1);
    for (value, wanted) in [(0u8, 0u16), (64, 605), (127, 1_200)] {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(CONTROL, 6, 41, value));
        assert_eq!(midi.synth().performance().bend_vco, wanted, "{value}");
        let found = bent_cents(&mut midi, &image);
        assert!((found - f64::from(wanted)).abs() < 2.0, "{value}: {found} cents");
    }
    // The last of controller 41 and registered parameter number 0 holds.
    let mut midi = started(KeyMode::Whole, &image);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 41, 64));
    for (number, value) in [(101u8, 0u8), (100, 0), (6, 3)] {
        let _ = send(&mut midi, &image, &message(CONTROL, 2, number, value));
    }
    assert_eq!(midi.synth().performance().bend_vco, 300);
    assert!((bent_cents(&mut midi, &image) - 300.0).abs() < 2.0);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 41, 127));
    assert_eq!(midi.synth().performance().bend_vco, 1_200);
    // Neither a program change nor reset all controllers changes it.
    let _ = send(&mut midi, &image, &message(PROGRAM, 0, 0, 0));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 121, 0));
    assert_eq!(midi.synth().performance().bend_vco, 1_200);
}

#[test]
fn s8_31_registered_parameter_number_0_sets_the_bend_range() {
    let image = bank(1);
    let mut midi = started(KeyMode::Whole, &image);
    let select = |midi: &mut Midi, coarse: u8, fine: u8| {
        let _ = send(midi, &image, &message(CONTROL, 0, 101, coarse));
        let _ = send(midi, &image, &message(CONTROL, 0, 100, fine));
    };
    select(&mut midi, 0, 0);
    for (semitones, wanted) in [(1u8, 100u16), (7, 700), (24, 1_200)] {
        assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 6, semitones)), Outcome::Applied);
        assert_eq!(midi.synth().performance().bend_vco, wanted);
    }
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 6, 2));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 38, 50));
    assert_eq!(midi.synth().performance().bend_vco, 250);
    assert!((bent_cents(&mut midi, &image) - 250.0).abs() < 2.0);
    // Cents above 99 are 99; increment and decrement move a cent.
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 38, 120));
    assert_eq!(midi.synth().performance().bend_vco, 299);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 96, 0));
    assert_eq!(midi.synth().performance().bend_vco, 300);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 97, 0));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 97, 0));
    assert_eq!(midi.synth().performance().bend_vco, 298);
    // The null number selects none: data entry changes nothing and is
    // counted; so with a non-registered number, which synth8 has none of.
    select(&mut midi, 127, 127);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 6, 5)), Outcome::PassedOver);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 99, 1));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 98, 2));
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 6, 5)), Outcome::PassedOver);
    assert_eq!(midi.synth().performance().bend_vco, 298);
}

#[test]
fn s8_9_expression_multiplies_the_volume_by_the_one_law() {
    let image = bank(1);
    let level = |volume: u8, expression: u8| {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 7, volume));
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 11, expression));
        let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, A2, 100));
        rms(&play(&mut midi, 0.4)[samples(0.1)..])
    };
    let full = level(127, 127);
    let law = |value: f64| (value * value) / (127.0 * 127.0);
    for (volume, expression) in [(64u8, 127u8), (127, 64), (64, 64)] {
        let found = level(volume, expression) / full;
        let wanted = law(f64::from(volume)) * law(f64::from(expression));
        assert!((found / wanted - 1.0).abs() < 0.01, "{volume} {expression}: {found} for {wanted}");
    }
    assert!(level(0, 127) < 1.0e-6 && level(127, 0) < 1.0e-6);
}

#[test]
fn s8_10_the_balance_does_nothing_in_whole() {
    let image = bank(1);
    let run = |balance: u8| {
        let mut midi = started(KeyMode::Whole, &image);
        let _ = send(&mut midi, &image, &message(CONTROL, 0, 8, balance));
        for key in [57, 61, 64, 69, 72] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
        play(&mut midi, 0.2)
    };
    let centre = run(64);
    for balance in [0u8, 32, 96, 127] {
        assert_eq!(run(balance), centre, "balance {balance}");
    }
}

#[test]
fn s8_15_controller_118_switches_portamento_and_65_is_counted() {
    let image = bank(1);
    let mut midi = started(KeyMode::Whole, &image);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 65, 127)), Outcome::PassedOver);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 118, 127)), Outcome::Applied);
    assert_eq!(midi.synth().performance().portamento, neoteletext_audio_instruments::synth8::Portamento::On);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 118, 63)), Outcome::Applied);
    assert_eq!(midi.synth().performance().portamento, neoteletext_audio_instruments::synth8::Portamento::Off);
}

#[test]
fn s8_26_a_lost_block_releases_every_voice_and_clears_the_selection() {
    let image = bank(1);
    let mut midi = started(KeyMode::Whole, &image);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 101, 0));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 100, 0));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 64, 127));
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 3, 77));
    let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, 60, 100));
    play(&mut midi, 0.1);
    midi.lost();
    play(&mut midi, 0.3);
    assert_eq!(midi.synth().sounding_voices(), 0);
    // The panel stays; a data entry after the loss changes nothing.
    assert_eq!(midi.synth().patch().get(Param::VcfCutoff), 77);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 6, 9)), Outcome::PassedOver);
    assert_eq!(midi.synth().performance().bend_vco, 200);
    // The hold is off: a key released is released.
    let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, 60, 100));
    let _ = send(&mut midi, &image, &message(status::NOTE_OFF, 0, 60, 0));
    play(&mut midi, 0.3);
    assert_eq!(midi.synth().sounding_voices(), 0);
}

#[test]
fn s8_25_panel_changes_on_both_channels_give_the_same_samples_however_rendering_is_cut() {
    let image = bank(4);
    let run = |cut: usize| {
        let mut midi = started(KeyMode::Dual, &image);
        let mut out = vec![0i16; 4_800 * 4];
        let script: [(usize, Message<'static>); 6] = [
            (0, message(status::NOTE_ON, 0, 60, 100)),
            (500, message(CONTROL, 0, 3, 90)),
            (1_000, message(CONTROL, 1, 3, 20)),
            (4_000, message(PROGRAM, 1, 2, 0)),
            (9_000, message(CONTROL, 0, 35, 64)),
            (12_000, message(status::NOTE_OFF, 0, 60, 0)),
        ];
        let mut done = 0;
        for (at, event) in script {
            let mut rendered = done;
            while rendered < at {
                let to = (rendered + cut).min(at);
                midi.render(&mut out[rendered..to]);
                rendered = to;
            }
            done = at;
            let _ = send(&mut midi, &image, &event);
        }
        let mut rendered = done;
        while rendered < out.len() {
            let to = (rendered + cut).min(out.len());
            midi.render(&mut out[rendered..to]);
            rendered = to;
        }
        out
    };
    let whole = run(4_800);
    for cut in [1, 7, 960] {
        assert_eq!(run(cut), whole, "calls of {cut}");
    }
}

/// Writes `messages` into a fresh instrument and into a tracker, then
/// plays the tracker's restatement into a second fresh instrument.
fn restated(image: &[u8], messages: &[Message<'static>]) -> (Midi, Midi, Tracker) {
    let mut original = started(KeyMode::Dual, image);
    let mut tracker = Tracker::new();
    for each in messages {
        let _ = send(&mut original, image, each);
        tracker.take(each);
    }
    let mut copy = started(KeyMode::Dual, image);
    let mut restatement = Vec::new();
    tracker.restate(&mut |each| {
        if let Message::Channel(channel) = each {
            restatement.push(channel);
        }
    });
    for channel in restatement {
        let _ = send(&mut copy, image, &Message::Channel(channel));
    }
    (original, copy, tracker)
}

#[test]
fn s8_29_the_restatement_brings_a_fresh_synth8_to_the_same_state() {
    let image = bank(16);
    let messages = [
        message(PROGRAM, 0, 5, 0),
        message(PROGRAM, 1, 9, 0),
        message(CONTROL, 0, 3, 99),
        message(CONTROL, 0, 63, 70),
        message(CONTROL, 0, 35, 64),
        message(CONTROL, 1, 119, 64),
        message(CONTROL, 1, 108, 0),
        message(CONTROL, 0, 9, 30),
        message(CONTROL, 1, 81, 40),
        message(CONTROL, 0, 110, 50),
        message(CONTROL, 1, 57, 127),
        message(CONTROL, 1, 87, 127),
        message(PITCH_BEND, 4, 0, 90),
        message(CONTROL, 3, 64, 127),
        message(CONTROL, 0, 1, 33),
        message(CONTROL, 0, 5, 20),
        message(CONTROL, 0, 7, 100),
        message(CONTROL, 0, 8, 30),
        message(CONTROL, 0, 11, 90),
        message(CONTROL, 0, 118, 127),
        message(CONTROL, 0, 101, 0),
        message(CONTROL, 0, 100, 0),
        message(CONTROL, 0, 6, 5),
        message(CONTROL, 0, 38, 25),
    ];
    let (mut original, mut copy, tracker) = restated(&image, &messages);
    assert_eq!(original.synth().patch(), copy.synth().patch());
    assert_eq!(original.synth().lower_patch(), copy.synth().lower_patch());
    assert_eq!(original.synth().performance(), copy.synth().performance());
    assert_eq!(tracker.bend_depth_cents(), 525);
    // The same notes now sound the same: wheel, bend, hold, volume,
    // expression and balance were restated too.
    for midi in [&mut original, &mut copy] {
        for key in [55, 60, 67] {
            let _ = send(midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
    }
    assert_eq!(play(&mut original, 0.5), play(&mut copy, 0.5));
    // And by controller 41: restated as 41.
    let (original, copy, _) = restated(&image, &[message(CONTROL, 2, 41, 90)]);
    assert_eq!(original.synth().performance(), copy.synth().performance());
}

#[test]
fn s8_29_a_restatement_of_the_state_held_changes_nothing_heard() {
    // MIDI-97: a viewer who lost nothing hears nothing of it.
    let image = bank(8);
    let messages = [
        message(PROGRAM, 0, 3, 0),
        message(CONTROL, 0, 3, 60),
        message(CONTROL, 0, 64, 127),
        message(PITCH_BEND, 0, 0, 70),
        message(CONTROL, 0, 41, 30),
        message(CONTROL, 1, 20, 86),
    ];
    let run = |restate: bool| {
        let mut midi = started(KeyMode::Dual, &image);
        let mut tracker = Tracker::new();
        for each in &messages {
            let _ = send(&mut midi, &image, each);
            tracker.take(each);
        }
        for key in [57, 64] {
            let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, key, 100));
        }
        let mut out = play(&mut midi, 0.3);
        if restate {
            let mut restatement = Vec::new();
            tracker.restate(&mut |each| {
                if let Message::Channel(channel) = each {
                    restatement.push(channel);
                }
            });
            for channel in restatement {
                let _ = send(&mut midi, &image, &Message::Channel(channel));
            }
        }
        out.extend(play(&mut midi, 0.3));
        out
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn s8_29_the_restatement_is_in_the_document_s_order() {
    let mut tracker = Tracker::new();
    for each in [
        message(CONTROL, 1, 3, 10),
        message(CONTROL, 0, 87, 127),
        message(CONTROL, 0, 63, 70),
        message(CONTROL, 0, 3, 50),
        message(CONTROL, 0, 1, 20),
        message(CONTROL, 0, 41, 64),
        message(PROGRAM, 1, 4, 0),
        message(CONTROL, 1, 9, 20),
    ] {
        tracker.take(&each);
    }
    let mut out = Vec::new();
    tracker.restate(&mut |each| {
        if let Message::Channel(channel) = each {
            out.push((channel.status, channel.data1, channel.data2));
        }
    });
    assert_eq!(
        out,
        [
            (0xc0, 0, 0),
            (0xb0, 3, 50),
            (0xb0, 62, 70),
            (0xb0, 87, 127),
            (0xb0, 1, 20),
            (0xb0, 41, 64),
            (0xc1, 4, 0),
            (0xb1, 9, 20),
        ]
    );
    // A fresh tracker restates the two programs alone (SRV-70).
    let mut out = Vec::new();
    Tracker::new().restate(&mut |each| {
        if let Message::Channel(channel) = each {
            out.push(channel.status);
        }
    });
    assert_eq!(out, [0xc0, 0xc1]);
}

#[test]
fn s8_13_messages_during_all_sound_off_wait_program_changes_among_them() {
    let image = bank(8);
    let mut midi = started(KeyMode::Whole, &image);
    let _ = send(&mut midi, &image, &message(status::NOTE_ON, 0, 60, 100));
    play(&mut midi, 0.1);
    let _ = send(&mut midi, &image, &message(CONTROL, 0, 120, 0));
    assert_eq!(send(&mut midi, &image, &message(PROGRAM, 0, 6, 0)), Outcome::Applied);
    assert_eq!(send(&mut midi, &image, &message(CONTROL, 0, 3, 99)), Outcome::Applied);
    // Not yet: the sound is still going out.
    assert_eq!(midi.synth().patch().get(Param::LfoRate), 0);
    play(&mut midi, 0.02);
    assert_eq!(midi.synth().patch().get(Param::LfoRate), 6);
    assert_eq!(midi.synth().patch().get(Param::VcfCutoff), 99);
    assert_eq!(midi.synth().patch().get(Param::Vco1Wave), square().get(Param::Vco1Wave));
    // NOTE_OFF names the constant the support defines.
    let _ = NOTE_OFF;
    let _ = NOTE_ON;
}
