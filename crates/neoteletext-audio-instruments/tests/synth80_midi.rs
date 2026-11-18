//! synth80 played and programmed through its adopted interface
//! (`neoteletext-common/docs/audio/synth80.md`, cited as "S80-n"): the
//! messages it takes, the channels, pitch bend and its range, coarse and
//! fine pitch, the common messages, program change and banks, the patch
//! as control changes, a lost block, and the server's interface: the
//! check of messages, patches and banks, the presets and the restatement.

// Without the feature there is nothing here to test.
#![cfg(feature = "synth80")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "a test, in f64 and on indices it derives itself"
)]

mod synth80_support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, status};
use neoteletext_audio_instruments::synth80::interface::{
    self, BankError, MOST_BANK_IMAGE_BYTES, Tracker, accepts, build_bank, check_bank, check_patch, pack_patch,
    patch_messages,
};
use neoteletext_audio_instruments::synth80::midi::Midi;
use neoteletext_audio_instruments::synth80::patch::ON;
use neoteletext_audio_instruments::synth80::presets::PRESETS;
use neoteletext_audio_instruments::synth80::{Control, Global, PATCH_BYTES, Patch, PatchError, Synth80};
use synth80_support::{CONTROL, NOTE_OFF, NOTE_ON, PITCH_BEND, SINE, cents, panel, pitch, render, samples};

/// A channel message.
const fn channel(status: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage { status, data1, data2 })
}

/// A control change on channel 1.
const fn cc(number: u8, value: u8) -> Message<'static> {
    channel(CONTROL, number, value)
}

/// Gives `synth80` each message with `banks`, and answers the outcomes.
fn give(synth80: &mut Synth80, messages: &[Message<'_>], banks: &Banks<'_>) -> Vec<Outcome> {
    messages.iter().map(|message| synth80.midi(message, banks)).collect()
}

/// `seconds` of raw samples.
fn raw(synth80: &mut Synth80, seconds: f64) -> Vec<i16> {
    let mut out = vec![0; samples(seconds)];
    synth80.render(&mut out);
    out
}

/// An instrument playing the sine of S80's tests, with a held key.
fn sounding(key: u8) -> Synth80 {
    let mut synth80 = Synth80::new();
    synth80.set_patch(&panel(&SINE));
    synth80.event(NOTE_ON, key, 100);
    synth80
}

/// The pitch of key 69 held a second, after `messages`.
fn pitch_after(messages: &[Message<'_>]) -> f64 {
    let mut synth80 = Synth80::new();
    synth80.set_patch(&panel(&SINE));
    give(&mut synth80, messages, &Banks::none());
    synth80.event(NOTE_ON, 69, 100);
    pitch(&render(&mut synth80, 1.0)[4_800..])
}

/// The registered parameter 0 sequence that sets the bend range.
fn bend_range(semitones: u8, cents: u8) -> [Message<'static>; 4] {
    [cc(101, 0), cc(100, 0), cc(6, semitones), cc(38, cents)]
}

/// A pitch bend of a 14-bit value.
const fn bend(value: u16) -> Message<'static> {
    channel(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8)
}

/// The controllers S80-1 does not name: counted and changing nothing.
fn not_taken(number: u8) -> bool {
    matches!(number, 2 | 4 | 9..=19 | 32..=37 | 39 | 74 | 122)
}

#[test]
fn s80_1_only_the_listed_controllers_are_taken_and_the_rest_change_nothing() {
    let base = |synth80: &mut Synth80| raw(synth80, 0.05);
    let mut alone = sounding(60);
    let without = base(&mut alone);
    for number in 0..=127u8 {
        let mut synth80 = sounding(60);
        let outcome = synth80.midi(&cc(number, 127), &Banks::none());
        let counted = synth80.ignored_events() == 1;
        assert_eq!(counted, not_taken(number), "controller {number}");
        assert_eq!(outcome == Outcome::PassedOver, not_taken(number), "controller {number}");
        // The server's check agrees with the receiver (SRV-24).
        assert_eq!(accepts(&cc(number, 127)), !not_taken(number), "controller {number}");
        if not_taken(number) {
            assert_eq!(base(&mut synth80), without, "controller {number} changed the sound");
        }
    }
    // Every channel message's status is taken; system exclusive is not
    // (S80-9).
    for kind in [0x80u8, 0x90, 0xA0, 0xC0, 0xD0, 0xE0] {
        for number in 0..16 {
            assert!(accepts(&channel(kind | number, 60, 0)), "{kind:#x}");
        }
    }
    let mut synth80 = sounding(60);
    let sysex: &[u8] = &[0x43, 0x10, 0x01, 0xF7];
    assert_eq!(synth80.midi(&Message::SystemExclusive(sysex), &Banks::none()), Outcome::PassedOver);
    assert_eq!(synth80.ignored_events(), 1);
    assert!(!accepts(&Message::SystemExclusive(sysex)));
    assert_eq!(base(&mut synth80), without);
    // A data byte above 127 is no message.
    assert!(!accepts(&channel(NOTE_ON, 128, 1)));
}

#[test]
fn s80_2_and_s80_23_every_channel_is_one_keyboard_and_one_state() {
    let score = |on: &dyn Fn(usize) -> u8| {
        let mut synth80 = Synth80::new();
        let messages = [
            bend_range(12, 0)[0],
            bend_range(12, 0)[1],
            bend_range(12, 0)[2],
            bend_range(12, 0)[3],
            cc(48, 40),
            cc(1, 90),
            channel(NOTE_ON, 60, 100),
            channel(NOTE_ON, 64, 90),
            bend(0x3000),
            cc(93, 80),
            channel(0xD0, 70, 0),
            channel(NOTE_OFF, 60, 0),
        ];
        let mut out = Vec::new();
        for (index, message) in messages.iter().enumerate() {
            let Message::Channel(message) = message else { unreachable!() };
            let moved = ChannelMessage { status: message.status | on(index), ..*message };
            synth80.midi(&Message::Channel(moved), &Banks::none());
            out.extend(raw(&mut synth80, 0.02));
        }
        out
    };
    let first = score(&|_| 0);
    for number in 1..16 {
        assert_eq!(score(&|_| number), first, "channel {}", number + 1);
    }
    assert_eq!(score(&|index| (index % 16) as u8), first, "spread over the sixteen");
}

#[test]
fn s80_3_and_s80_4_a_velocity_of_0_is_a_note_off_and_release_velocity_is_nothing() {
    let run = |off: Message<'_>| {
        let mut synth80 = sounding(60);
        let mut out = raw(&mut synth80, 0.1);
        synth80.midi(&off, &Banks::none());
        out.extend(raw(&mut synth80, 0.3));
        out
    };
    let plain = run(channel(NOTE_OFF, 60, 0));
    for velocity in [64, 127] {
        assert_eq!(run(channel(NOTE_OFF, 60, velocity)), plain, "release velocity {velocity}");
    }
    assert_eq!(run(channel(NOTE_ON, 60, 0)), plain);
}

#[test]
fn s80_7_registered_parameter_0_sets_the_bend_range_and_nothing_else_does() {
    // The full bend at each range: 16,383 is 8,191 ÷ 8,192 of it.
    let rest = pitch_after(&[]);
    for (semitones, expected) in [(1u8, 1.0), (12, 12.0), (99, 99.0), (127, 99.0)] {
        let mut messages = bend_range(semitones, 0).to_vec();
        messages.push(bend(0x3FFF));
        let found = cents(rest, pitch_after(&messages));
        let wanted = expected * 100.0 * 8_191.0 / 8_192.0;
        println!("range {semitones}: a full bend is {found:.2} cents");
        // A bend past the top key's octave leaves the oscillators' range,
        // so the widest is checked by the range kept.
        if expected < 50.0 {
            assert!((found - wanted).abs() < 0.1, "range {semitones}: {found}");
        }
        let mut synth80 = Synth80::new();
        give(&mut synth80, &bend_range(semitones, 0), &Banks::none());
        assert_eq!(synth80.common().channel(0).bend_range_cents, (expected * 100.0) as u16);
    }
    // A program change and reset all controllers leave the range;
    // increment and decrement step it by a cent.
    let mut synth80 = Synth80::new();
    give(&mut synth80, &bend_range(12, 0), &Banks::none());
    give(&mut synth80, &[channel(0xC0, 3, 0), cc(121, 0)], &Banks::none());
    assert_eq!(synth80.common().channel(0).bend_range_cents, 300 + 900);
    give(&mut synth80, &[cc(101, 0), cc(100, 0), cc(96, 1), cc(96, 1), cc(97, 1)], &Banks::none());
    assert_eq!(synth80.common().channel(0).bend_range_cents, 1_201);
    // Any other registered parameter, and every non-registered one,
    // selects nothing: a data entry after it is counted.
    let mut synth80 = Synth80::new();
    let before = synth80.ignored_events();
    let outcomes =
        give(&mut synth80, &[cc(101, 0), cc(100, 2), cc(6, 40), cc(99, 1), cc(98, 2), cc(6, 9)], &Banks::none());
    assert_eq!(outcomes[2], Outcome::PassedOver);
    assert_eq!(outcomes[5], Outcome::PassedOver);
    assert_eq!(synth80.ignored_events(), before + 2);
    assert_eq!(synth80.common().channel(0).bend_range_cents, 300);
}

#[test]
fn s80_22_no_controllers_pair_and_32_to_39_but_38_are_counted() {
    let mut synth80 = Synth80::new();
    give(&mut synth80, &[cc(8, 64), cc(40, 100)], &Banks::none());
    assert_eq!((synth80.patch().get(Global::Mix), synth80.patch().tones[0].get(Control::PwmSpeed)), (64, 100));
    for number in [32, 33, 34, 35, 36, 37, 39] {
        let before = synth80.ignored_events();
        assert_eq!(synth80.midi(&cc(number, 5), &Banks::none()), Outcome::PassedOver, "{number}");
        assert_eq!(synth80.ignored_events(), before + 1);
    }
}

#[test]
fn s80_10_s80_11_s80_18_s80_20_each_controller_moves_its_one_byte() {
    for place in 0..PATCH_BYTES {
        let number = interface::controller_of(place).unwrap();
        for value in [0u8, 127] {
            let mut synth80 = Synth80::new();
            let mut start = Patch::DEFAULT;
            start.set_byte(place, 127 - value);
            synth80.set_patch(&start);
            synth80.midi(&cc(number, value), &Banks::none());
            let mut wanted = start;
            wanted.set_byte(place, value);
            assert_eq!(*synth80.patch(), wanted, "controller {number}, byte {place}");
        }
    }
    // GLIDE MODE on 3 with its three positions; 120 does not move it.
    let mut synth80 = Synth80::new();
    for value in [0u8, 43, 85] {
        synth80.midi(&cc(3, value), &Banks::none());
        assert_eq!(synth80.patch().get(Global::GlideMode), value);
    }
    synth80.event(NOTE_ON, 60, 100);
    synth80.midi(&cc(120, 0), &Banks::none());
    assert_eq!(synth80.patch().get(Global::GlideMode), 85);
}

#[test]
fn s80_27_glide_mode_off_goes_to_the_new_key_at_once() {
    let second_pitch = |mode: u8| {
        let mut synth80 = Synth80::new();
        synth80.set_patch(&panel(&SINE));
        // Every voice plays a key two octaves down first, with no glide,
        // so that the next key glides from it.
        for key in 40..48 {
            synth80.event(NOTE_ON, key, 100);
            render(&mut synth80, 0.01);
            synth80.event(NOTE_OFF, key, 0);
        }
        render(&mut synth80, 0.3);
        give(&mut synth80, &[cc(5, 127), cc(3, mode)], &Banks::none());
        synth80.event(NOTE_ON, 69, 100);
        pitch(&render(&mut synth80, 0.1)[960..])
    };
    let off = second_pitch(43);
    assert!(cents(440.0, off).abs() < 2.0, "off: {off} Hz");
    let portamento = second_pitch(0);
    assert!(cents(440.0, portamento) < -300.0, "portamento: {portamento} Hz");
}

#[test]
fn s80_28_reset_all_controllers_follows_rp_015_and_leaves_the_tuning_and_the_patch() {
    let mut synth80 = sounding(60);
    let mut messages = bend_range(7, 0).to_vec();
    messages.extend([cc(1, 90), cc(64, 127), bend(0x3000), cc(7, 50), cc(93, 20), cc(94, 100), cc(65, 77)]);
    messages.push(channel(0xD0, 90, 0));
    give(&mut synth80, &messages, &Banks::none());
    synth80.event(NOTE_ON, 64, 100);
    render(&mut synth80, 0.05);
    synth80.midi(&cc(121, 0), &Banks::none());
    let state = *synth80.common().channel(0);
    assert_eq!((state.modulation, state.sustain, state.bend, state.channel_pressure), (0, false, 0x2000, 0));
    assert_eq!((state.volume, state.bend_range_cents), (50, 700));
    assert_eq!(synth80.wheel_and_volume(), (0, 50));
    assert_eq!(synth80.tuning(), (20, 100));
    assert_eq!(synth80.patch().tones[0].get(Control::PressureBrilliance), 77);
    // It releases no held key.
    render(&mut synth80, 0.05);
    assert_eq!(synth80.sounding_voices(), 2);
    // Controllers 123 to 127 each release every held key as a note off
    // would: with the pedal up they fall; with it down the pedal holds
    // them, and what it held already, until it goes up (MIDI-85).
    for number in 123..=127u8 {
        let mut synth80 = sounding(60);
        synth80.event(NOTE_ON, 67, 100);
        synth80.midi(&cc(number, 0), &Banks::none());
        render(&mut synth80, 0.3);
        assert_eq!(synth80.sounding_voices(), 0, "controller {number}");
        let mut synth80 = sounding(60);
        synth80.event(NOTE_ON, 64, 100);
        synth80.event(CONTROL, 64, 127);
        synth80.event(NOTE_OFF, 64, 0);
        synth80.event(NOTE_ON, 67, 100);
        synth80.midi(&cc(number, 0), &Banks::none());
        render(&mut synth80, 0.3);
        assert_eq!(synth80.sounding_voices(), 3, "controller {number}: the pedal holds");
        synth80.event(CONTROL, 64, 0);
        render(&mut synth80, 0.3);
        assert_eq!(synth80.sounding_voices(), 0, "controller {number}");
    }
}

#[test]
fn s80_29_all_sound_off_silences_in_256_samples_and_leaves_the_controllers() {
    // The fade is 256 samples long where the voices are mixed; the
    // decimator after them delays what comes out by its own 22 samples
    // (`analog::decimator`, `Decimator::sharp`), within MIDI-86's 1,024.
    const DECIMATOR_DELAY: usize = 22;
    let mut patch = panel(&SINE);
    patch.tones[0].set(Control::Release, 127);
    let mut synth80 = Synth80::new();
    synth80.set_patch(&patch);
    give(&mut synth80, &[cc(64, 127), cc(93, 70), cc(1, 30)], &Banks::none());
    for key in [60, 64, 67] {
        synth80.event(NOTE_ON, key, 100);
    }
    render(&mut synth80, 0.2);
    synth80.midi(&cc(120, 0), &Banks::none());
    let out = raw(&mut synth80, 0.05);
    let last = out.iter().rposition(|&sample| sample != 0).unwrap_or(0);
    println!("all sound off: the last sound at sample {last}");
    assert!(last <= 256 + DECIMATOR_DELAY, "silent after sample 278: the last sound at {last}");
    assert_eq!(synth80.sounding_voices(), 0);
    assert!(synth80.common().channel(0).sustain);
    assert_eq!((synth80.tuning().0, synth80.wheel_and_volume().0), (70, 30));
    assert_eq!(*synth80.patch(), patch);
}

/// A bank of `count` patches, patch k with byte 0 at k and channel I's
/// LPF at 20 + k ÷ 2.
fn distinct(count: usize) -> Vec<Patch> {
    (0..count)
        .map(|number| {
            let mut patch = Patch::DEFAULT;
            patch.set_byte(0, number as u8);
            patch.tones[0].set(Control::Lpf, 20 + (number / 2) as u8);
            patch
        })
        .collect()
}

/// The image of a bank.
fn image(patches: &[Patch]) -> Vec<u8> {
    let mut out = [0; MOST_BANK_IMAGE_BYTES];
    let length = build_bank(patches, &mut out).unwrap();
    out[..length].to_vec()
}

#[test]
fn s80_35_and_s80_36_program_change_and_bank_select_choose_among_three_banks() {
    let full = image(&distinct(128));
    let mut other = distinct(8);
    for patch in &mut other {
        patch.set(Global::Mix, 99);
    }
    let second = image(&other);
    let four = image(&distinct(4));
    let banks = Banks::new(&[&full, &second, &four]);
    let mut synth80 = Synth80::new();
    assert_eq!(synth80.start(&banks), Outcome::Applied);
    assert_eq!(*synth80.patch(), distinct(1)[0]);
    for program in [0u8, 1, 64, 127] {
        assert_eq!(synth80.midi(&channel(0xC0, program, 0), &banks), Outcome::Applied);
        assert_eq!(*synth80.patch(), distinct(128)[usize::from(program)], "program {program}");
    }
    // Bank select alone changes nothing; the next program change reads it.
    synth80.midi(&cc(0, 1), &banks);
    assert_eq!(*synth80.patch(), distinct(128)[127]);
    synth80.midi(&channel(0xC0, 5, 0), &banks);
    assert_eq!(*synth80.patch(), other[5]);
    // A program the bank lacks, and a bank past the third or past the
    // sound's, give the default patch and are counted.
    for (select, program, names) in [(2u8, 4u8, 3usize), (2, 1, 2), (3, 0, 3), (127, 0, 3)] {
        let named = [&full[..], &second[..], &four[..]];
        let banks = Banks::new(&named[..names]);
        let mut synth80 = Synth80::new();
        synth80.midi(&cc(0, select), &banks);
        let before = synth80.ignored_events();
        assert_eq!(synth80.midi(&channel(0xC0, program, 0), &banks), Outcome::PassedOver);
        assert_eq!((*synth80.patch(), synth80.ignored_events()), (Patch::DEFAULT, before + 1));
    }
    // Controller 32 is counted and selects nothing.
    let mut synth80 = Synth80::new();
    assert_eq!(synth80.midi(&cc(32, 1), &banks), Outcome::PassedOver);
    // A faulty patch is refused and the default set.
    let mut faulty = full.clone();
    faulty[85 + 10] = 0x80;
    let banks = Banks::new(&[&faulty]);
    assert_eq!(synth80.midi(&channel(0xC0, 1, 0), &banks), Outcome::PatchRefused);
    assert_eq!(*synth80.patch(), Patch::DEFAULT);
    // S80-21: a program change leaves the wheel, the volume, the pedal and
    // the tuning.
    let mut synth80 = sounding(60);
    give(&mut synth80, &[cc(1, 9), cc(7, 99), cc(64, 127), cc(93, 3), cc(94, 4), bend(100)], &banks);
    synth80.midi(&channel(0xC0, 0, 0), &banks);
    assert_eq!((synth80.wheel_and_volume(), synth80.tuning()), ((9, 99), (3, 4)));
    assert!(synth80.common().channel(0).sustain && synth80.common().channel(0).bend == 100);
    assert_eq!(synth80.sounding_voices(), 1);
}

#[test]
fn s80_33_and_s80_40_a_patch_is_its_85_control_changes_at_one_instant() {
    let mut target = Patch::with_tones(PRESETS[2].1, PRESETS[13].1);
    target.set(Global::Effect, ON);
    target.set(Global::Chorus, ON);
    target.set(Global::GlideMode, 85);
    let bank = image(&[Patch::DEFAULT, target]);
    let banks = Banks::new(&[&bank]);
    let run = |by_messages: bool| {
        let mut synth80 = Synth80::new();
        synth80.start(&banks);
        synth80.event(NOTE_ON, 57, 100);
        let mut out = raw(&mut synth80, 0.1);
        if by_messages {
            let messages: Vec<_> = patch_messages(&target, 0).collect();
            assert_eq!(messages.len(), PATCH_BYTES);
            give(&mut synth80, &messages, &banks);
        } else {
            synth80.midi(&channel(0xC0, 1, 0), &banks);
        }
        assert_eq!(*synth80.patch(), target);
        out.extend(raw(&mut synth80, 0.3));
        out
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn s80_39_a_lost_block_releases_every_key_lifts_the_pedal_and_leaves_the_rest() {
    let mut midi = Midi::new(Synth80::new());
    let none = Banks::none();
    let mut messages = bend_range(5, 0).to_vec();
    messages.extend([cc(64, 127), cc(93, 30), cc(94, 90), cc(7, 70), cc(1, 50), bend(0x2800), cc(48, 33)]);
    messages.extend([channel(NOTE_ON, 60, 100), cc(101, 0), cc(100, 0)]);
    for message in &messages {
        midi.midi(message, &none);
    }
    midi.lost();
    let mut out = vec![0; samples(0.5)];
    midi.render(&mut out);
    let synth80 = midi.synth80();
    assert_eq!(synth80.sounding_voices(), 0);
    let state = *synth80.common().channel(0);
    assert!(!state.sustain);
    assert_eq!((state.bend, state.bend_range_cents, state.volume, state.modulation), (0x2800, 500, 70, 50));
    assert_eq!((synth80.tuning(), synth80.patch().tones[0].get(Control::Lpf)), ((30, 90), 33));
    // No parameter is selected: a data entry changes nothing.
    midi.midi(&cc(6, 20), &none);
    assert_eq!(midi.synth80().common().channel(0).bend_range_cents, 500);
}

#[test]
fn s80_47_coarse_and_fine_pitch_tune_outside_the_patch() {
    let rest = pitch_after(&[]);
    for (number, reach) in [(93u8, 500.0), (94, 100.0)] {
        for (value, share) in [(0u8, -1.0), (32, -0.5), (64, 0.0), (96, 32.0 / 63.0), (127, 1.0)] {
            let found = cents(rest, pitch_after(&[cc(number, value)]));
            println!("controller {number} at {value}: {found:.2} cents");
            assert!((found - share * reach).abs() < 0.1, "controller {number} at {value}: {found}");
        }
    }
    let both = cents(rest, pitch_after(&[cc(93, 127), cc(94, 127), bend(0x3FFF)]));
    assert!((both - (600.0 + 300.0 * 8_191.0 / 8_192.0)).abs() < 0.1, "{both}");
    // A program change, a patch inline, controllers 121 and 120 and a
    // lost block leave both; the patch is still 85 values.
    let mut synth80 = sounding(60);
    give(&mut synth80, &[cc(93, 10), cc(94, 120)], &Banks::none());
    let inline: Vec<_> = patch_messages(&Patch::DEFAULT, 0).collect();
    for message in [channel(0xC0, 0, 0), cc(121, 0), cc(120, 0)].into_iter().chain(inline) {
        synth80.midi(&message, &Banks::none());
        assert_eq!(synth80.tuning(), (10, 120));
    }
    synth80.lost();
    assert_eq!(synth80.tuning(), (10, 120));
    assert_eq!(pack_patch(synth80.patch()).len(), 85);
    assert!(interface::place_of(93).is_none() && interface::place_of(94).is_none());
}

#[test]
fn s80_41_and_s80_42_the_server_builds_and_checks_patches_banks_and_presets() {
    // Patches.
    let patch = Patch::with_tones(interface::preset("BRASS 1").unwrap(), interface::preset("BRASS 3").unwrap());
    let bytes = pack_patch(&patch);
    assert_eq!(bytes[..26], PRESETS[2].1.to_bytes());
    assert_eq!(bytes[26..52], PRESETS[14].1.to_bytes());
    assert_eq!(check_patch(&bytes), Ok(patch));
    assert_eq!(check_patch(&bytes[..84]), Err(PatchError::Length(84)));
    assert!(interface::preset("BRASS 9").is_none());
    for (name, tone) in PRESETS {
        assert_eq!(interface::preset(name), Some(tone));
    }
    // Banks of 1 and 128 patches; 0 and 129 refused.
    for count in [1, 128] {
        let image = image(&distinct(count));
        assert_eq!((image.len(), check_bank(&image)), (count * 85, Ok(count)));
    }
    let mut out = [0; MOST_BANK_IMAGE_BYTES];
    assert_eq!(build_bank(&[], &mut out), Err(BankError::Length(0)));
    assert_eq!(build_bank(&distinct(129), &mut out), Err(BankError::Length(129 * 85)));
    assert_eq!(check_bank(&[0; 86]), Err(BankError::Length(86)));
    assert_eq!(check_bank(&[]), Err(BankError::Length(0)));
    let mut faulty = image(&distinct(3));
    faulty[2 * 85 + 7] = 0x90;
    assert_eq!(check_bank(&faulty), Err(BankError::Patch(2, PatchError::Value(7))));
}

/// The messages of a run that moves every kind of state S80-46 restates.
fn busy_score() -> Vec<Message<'static>> {
    let mut messages = vec![cc(0, 1), channel(0xC0, 3, 0)];
    for (number, value) in
        [(40u8, 10), (48, 90), (3, 0), (5, 60), (25, 127), (27, 127), (28, 127), (95, 80), (8, 40), (110, 70)]
    {
        messages.push(cc(number, value));
    }
    messages.extend(bend_range(7, 25));
    messages.extend([bend(0x2400), channel(0xD0, 30, 0), cc(7, 90), cc(1, 20), cc(64, 127), cc(93, 70), cc(94, 50)]);
    messages
}

/// Restates `tracker` as a list.
fn restatement(tracker: &Tracker) -> Vec<Message<'static>> {
    let mut out = Vec::new();
    tracker.restate(&mut |message| {
        let Message::Channel(message) = message else { panic!("synth80 restates no system exclusive") };
        out.push(Message::Channel(message));
    });
    out
}

#[test]
fn s80_46_the_restatement_brings_a_fresh_instrument_to_the_state() {
    let first = image(&distinct(8));
    let second = image(&distinct(8).into_iter().rev().collect::<Vec<_>>());
    let banks = Banks::new(&[&first, &second]);
    let mut tracker = Tracker::new();
    let mut synth80 = Synth80::new();
    synth80.start(&banks);
    for message in busy_score() {
        tracker.take(&message);
        synth80.midi(&message, &banks);
    }
    let said = restatement(&tracker);
    // Its order: bank select and the program, the controls by number, the
    // range, the performance state, then the tuning.
    assert_eq!(said[..2], [cc(0, 1), channel(0xC0, 3, 0)]);
    assert_eq!(
        said[2..12]
            .iter()
            .map(|message| match message {
                Message::Channel(message) => message.data1,
                Message::SystemExclusive(_) => 0,
            })
            .collect::<Vec<_>>(),
        [3, 5, 8, 25, 27, 28, 40, 48, 95, 110]
    );
    assert_eq!(said[said.len() - 2..], [cc(93, 70), cc(94, 50)]);
    let mut fresh = Synth80::new();
    fresh.start(&banks);
    for message in &said {
        fresh.midi(message, &banks);
    }
    assert_eq!(*fresh.patch(), *synth80.patch());
    assert_eq!(fresh.common().channel(0), synth80.common().channel(0));
    assert_eq!((fresh.tuning(), fresh.wheel_and_volume()), (synth80.tuning(), synth80.wheel_and_volume()));

    // After an inline patch the patch is restated as its 85 controls, in
    // byte order, then what moved since.
    let mut tracker = Tracker::new();
    let mut synth80 = Synth80::new();
    let mut inline = Patch::with_tones(PRESETS[0].1, PRESETS[12].1);
    inline.set(Global::SubVco, 30);
    let messages: Vec<_> = patch_messages(&inline, 0).collect();
    tracker.inline_patch(&inline);
    give(&mut synth80, &messages, &banks);
    for message in [cc(48, 12), cc(0, 2)] {
        tracker.take(&message);
        synth80.midi(&message, &banks);
    }
    let said = restatement(&tracker);
    assert_eq!(said[..PATCH_BYTES], messages[..]);
    let mut fresh = Synth80::new();
    give(&mut fresh, &said, &banks);
    assert_eq!(*fresh.patch(), *synth80.patch());
    assert_eq!(fresh.common().channel(0), synth80.common().channel(0));
}

#[test]
fn s80_46_and_midi_97_a_restatement_changes_nothing_where_the_state_is_held() {
    let first = image(&distinct(8));
    let banks = Banks::new(&[&first]);
    let run = |restate: bool| {
        let mut tracker = Tracker::new();
        let mut synth80 = Synth80::new();
        synth80.start(&banks);
        let mut out = Vec::new();
        let notes = [(60u8, 100u8), (64, 90), (67, 80), (72, 70)];
        for (index, message) in busy_score().into_iter().enumerate() {
            tracker.take(&message);
            synth80.midi(&message, &banks);
            if let Some(&(key, velocity)) = notes.get(index % 7) {
                synth80.event(NOTE_ON, key, velocity);
            }
            out.extend(raw(&mut synth80, 0.01));
            if restate {
                for message in restatement(&tracker) {
                    synth80.midi(&message, &banks);
                }
            }
            out.extend(raw(&mut synth80, 0.01));
        }
        out.extend(raw(&mut synth80, 0.2));
        out
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn midi_97_a_program_change_to_the_patch_held_changes_nothing_audible() {
    let bank = image(&distinct(4));
    let banks = Banks::new(&[&bank]);
    let run = |again: bool| {
        let mut synth80 = Synth80::new();
        synth80.start(&banks);
        synth80.midi(&channel(0xC0, 2, 0), &banks);
        synth80.event(NOTE_ON, 60, 100);
        let mut out = raw(&mut synth80, 0.1);
        if again {
            synth80.midi(&channel(0xC0, 2, 0), &banks);
        }
        out.extend(raw(&mut synth80, 0.2));
        out
    };
    assert_eq!(run(true), run(false));
    // And a status byte of another kind is never one (S80-1).
    assert!(!accepts(&channel(status::SYSTEM_EXCLUSIVE, 0, 0)));
}
