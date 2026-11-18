//! synthD as the receiver plays it: the Voyager's controller map and
//! NeoTeletext's own, the common layer, the bank and the restatement
//! (`synthd.md`, SD-1 to SD-28; `server.md`, SRV-24, SRV-68 and SRV-69).

// Without the feature there is nothing here to test.
#![cfg(feature = "synthd")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_panics_doc,
    reason = "a test"
)]

#[path = "synthd_support.rs"]
mod support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, status};
use neoteletext_audio_instruments::synthd::interface::{self, DEFAULT_PATCH, LAYOUT, PATCH_BYTES, Tracker, byte};
use neoteletext_audio_instruments::synthd::midi::Midi;
use neoteletext_audio_instruments::synthd::{SynthD, Waveform};

/// A control change on channel 0.
fn cc(number: u8, value: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, 0, number, value))
}

/// A channel message of `kind` on channel 0.
fn message(kind: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, 0, data1, data2))
}

/// A synthD behind its front end, its oscillators not drifting.
fn fresh() -> Midi {
    let mut synthd = SynthD::new();
    synthd.set_drift(0);
    Midi::new(synthd)
}

fn send(midi: &mut Midi, message: Message<'_>) -> Outcome {
    midi.midi(&message, &Banks::none())
}

fn render(midi: &mut Midi, samples: usize) -> Vec<i16> {
    let mut out = vec![0i16; samples];
    midi.render(&mut out);
    out
}

/// Registered parameter 0 selected, its semitones set by data entry.
fn set_bend_range(midi: &mut Midi, semitones: u8) {
    for (number, value) in [(101, 0), (100, 0), (6, semitones)] {
        assert_eq!(send(midi, cc(number, value)), Outcome::Applied);
    }
}

/// The frequency of a held note, from its samples.
fn heard(samples: &[i16]) -> f64 {
    let signal: Vec<f64> = samples.iter().map(|&sample| f64::from(sample)).collect();
    support::pitch(&signal[samples.len() / 4..])
}

/// The controllers SD-1 names, written out from the document: tables A to
/// E of SD-17, the modulation wheel's 1 and 33 (SD-9), volume 7 (SD-10),
/// the parameter number controllers 98 to 101 (SD-6), and 64, 120, 121 and
/// 123 to 127 (SD-11, SD-13).
fn taken() -> Vec<u8> {
    let mut taken = vec![
        5, 37, 10, 42, 12, 44, 14, 46, 15, 47, 16, 48, 17, 49, 18, 50, 19, 51, 21, 53, 23, 55, 24, 56, 25, 57, 27, 59,
        28, 60, 29, 61, 30, 62, 65, 74, 75, 76, 79, 80, 81, 82, 83, 84, 85, 88, 89, 90, 102, 103, 104, 105, 106, 107,
        108, 109, 110, 111, 112, 113,
    ];
    taken.extend([1, 33, 7, 98, 99, 100, 101, 64, 120, 121, 123, 124, 125, 126, 127]);
    taken
}

#[test]
fn sd_1_sd_17_the_controllers_synthd_takes_and_those_it_counts_as_ignored() {
    let taken = taken();
    // A held note, after which the messages passed over must change
    // nothing heard.
    let baseline = {
        let mut midi = fresh();
        send(&mut midi, message(status::NOTE_ON, 60, 100));
        render(&mut midi, 480);
        render(&mut midi, 960)
    };
    for number in 0..=127u8 {
        for value in [0u8, 64, 127] {
            let mut midi = fresh();
            send(&mut midi, message(status::NOTE_ON, 60, 100));
            render(&mut midi, 480);
            let outcome = send(&mut midi, cc(number, value));
            let expected = if taken.contains(&number) { Outcome::Applied } else { Outcome::PassedOver };
            assert_eq!(outcome, expected, "controller {number} at {value}");
            // SD-1's server rule: the server refuses what the receiver does
            // not take, but data entry, which it may send after a selection.
            let accepted = interface::accepts(&cc(number, value));
            assert_eq!(accepted, taken.contains(&number) || [6, 38, 96, 97].contains(&number), "{number}");
            if outcome == Outcome::PassedOver {
                assert_eq!(render(&mut midi, 960), baseline, "controller {number} at {value} changed the sound");
            }
        }
    }
    // Every table F controller is among those passed over.
    for number in interface::IGNORED_VOYAGER {
        assert!(!taken.contains(&number));
    }
    // Pressure, system exclusive: passed over and not taken by the server.
    let mut midi = fresh();
    for other in [message(status::KEY_PRESSURE, 60, 100), message(status::CHANNEL_PRESSURE, 100, 0)] {
        assert_eq!(send(&mut midi, other), Outcome::PassedOver);
        assert!(!interface::accepts(&other));
    }
    let sysex = Message::SystemExclusive(&[0x7D, 0x01, 0xF7]);
    assert_eq!(send(&mut midi, sysex), Outcome::PassedOver);
    assert!(!interface::accepts(&sysex));
    for kind in [status::NOTE_ON, status::NOTE_OFF, status::PROGRAM_CHANGE, status::PITCH_BEND] {
        assert!(interface::accepts(&message(kind, 1, 2)));
    }
}

#[test]
fn sd_2_the_sixteen_channels_are_one_panel() {
    let score = |channel: &dyn Fn(usize) -> u8| {
        let mut midi = fresh();
        let messages = [
            (status::CONTROL_CHANGE, 19, 40),
            (status::NOTE_ON, 60, 100),
            (status::PITCH_BEND, 0, 80),
            (status::CONTROL_CHANGE, 1, 90),
            (status::NOTE_ON, 55, 100),
            (status::NOTE_OFF, 55, 0),
        ];
        let mut out = Vec::new();
        for (index, (kind, data1, data2)) in messages.into_iter().enumerate() {
            let message = Message::Channel(ChannelMessage::new(kind, channel(index), data1, data2));
            midi.midi(&message, &Banks::none());
            out.extend(render(&mut midi, 480));
        }
        out
    };
    let first = score(&|_| 0);
    for channel in 1..16u8 {
        assert_eq!(score(&|_| channel), first, "channel {channel}");
    }
    assert_eq!(score(&|index| (index * 5 % 16) as u8), first, "spread");
}

#[test]
fn sd_6_the_bend_range_by_registered_parameter_0() {
    let mut patch = support::triangle();
    patch.oscillators[0].waveform = Waveform::Triangle;
    let up = |semitones: Option<u8>| {
        let mut midi = fresh();
        midi.synthd_mut().set_patch(&patch);
        if let Some(semitones) = semitones {
            set_bend_range(&mut midi, semitones);
        }
        send(&mut midi, message(status::PITCH_BEND, 0x7F, 0x7F));
        send(&mut midi, message(status::NOTE_ON, 57, 100));
        heard(&render(&mut midi, 48_000))
    };
    let base = 220.0;
    for (semitones, cents) in
        [(None, 1200.0 * 200.0 / 301.0), (Some(1), 100.0), (Some(7), 700.0), (Some(24), 2400.0), (Some(40), 3100.0)]
    {
        let expected = cents * 8191.0 / 8192.0;
        let moved = support::cents(base, up(semitones));
        println!("range {semitones:?}: {moved:.2} cents for {expected:.2}");
        assert!((moved - expected).abs() < 1.0, "{semitones:?}");
    }
    let mut midi = fresh();
    assert_eq!(midi.common().channel(0).bend_range_cents, 797);
    set_bend_range(&mut midi, 40);
    assert_eq!(midi.common().channel(0).bend_range_cents, 3_100);
    set_bend_range(&mut midi, 2);
    assert_eq!(send(&mut midi, cc(38, 50)), Outcome::Applied);
    assert_eq!(midi.common().channel(0).bend_range_cents, 250);
    // Increment and decrement step one cent.
    assert_eq!(send(&mut midi, cc(96, 0)), Outcome::Applied);
    assert_eq!(midi.common().channel(0).bend_range_cents, 251);
    assert_eq!(send(&mut midi, cc(97, 99)), Outcome::Applied);
    send(&mut midi, cc(97, 0));
    assert_eq!(midi.common().channel(0).bend_range_cents, 249);
    // A program change and controller 121 leave the range.
    send(&mut midi, message(status::PROGRAM_CHANGE, 3, 0));
    send(&mut midi, cc(121, 0));
    assert_eq!(midi.common().channel(0).bend_range_cents, 249);
    // Data entry with no parameter, after the null number, after a
    // non-registered selection: nothing changes, and it is counted.
    for selection in [&[][..], &[(101, 127), (100, 127)][..], &[(99, 0), (98, 5)][..], &[(101, 0), (100, 1)][..]] {
        let mut midi = fresh();
        for &(number, value) in selection {
            assert_eq!(send(&mut midi, cc(number, value)), Outcome::Applied);
        }
        for number in [6, 38, 96, 97] {
            assert_eq!(send(&mut midi, cc(number, 12)), Outcome::PassedOver, "{selection:?} {number}");
        }
        assert_eq!(midi.common().channel(0).bend_range_cents, 797);
    }
}

#[test]
fn sd_9_the_modulation_wheel_is_the_pair_1_and_33() {
    let mut midi = fresh();
    send(&mut midi, cc(1, 64));
    assert_eq!(midi.synthd().modulation_wheel(), 8_192);
    send(&mut midi, cc(33, 5));
    assert_eq!(midi.synthd().modulation_wheel(), 8_197);
    send(&mut midi, cc(1, 127));
    assert_eq!(midi.synthd().modulation_wheel(), 16_256);
    send(&mut midi, cc(33, 127));
    assert_eq!(midi.synthd().modulation_wheel(), 16_383);
    send(&mut midi, cc(121, 0));
    assert_eq!(midi.synthd().modulation_wheel(), 0);
}

#[test]
fn sd_10_controller_7_is_the_volume_and_39_is_ignored() {
    let mut midi = fresh();
    assert_eq!(midi.synthd().volume(), 127);
    assert_eq!(send(&mut midi, cc(7, 64)), Outcome::Applied);
    assert_eq!(send(&mut midi, cc(39, 64)), Outcome::PassedOver);
    assert_eq!(midi.synthd().volume(), 64);
    assert_eq!(midi.synthd().values()[byte::MAIN_OUTPUT_VOLUME], 127);
}

/// A bank of `count` patches, patch k with its GLIDE coarse byte k and its
/// CUTOFF FREQUENCY's coarse byte 127 − k.
fn bank(count: usize) -> Vec<u8> {
    let mut image = Vec::new();
    for program in 0..count {
        let mut values = DEFAULT_PATCH;
        values[byte::GLIDE] = program as u8;
        values[byte::CUTOFF_FREQUENCY] = 127 - program as u8;
        image.extend(values);
    }
    image
}

#[test]
fn sd_12_a_program_change_sets_the_panel_from_the_bank() {
    let image = bank(128);
    let banks = Banks::new(&[&image]);
    let mut midi = fresh();
    assert_eq!(midi.start(&banks), Outcome::Applied);
    assert_eq!(midi.synthd().values()[..], image[..PATCH_BYTES]);
    midi.midi(&message(status::NOTE_ON, 60, 100), &banks);
    midi.midi(&cc(1, 70), &banks);
    midi.midi(&message(status::PITCH_BEND, 0, 70), &banks);
    midi.midi(&cc(64, 127), &banks);
    for program in [0u8, 1, 31, 32, 127] {
        assert_eq!(midi.midi(&message(status::PROGRAM_CHANGE, program, 0), &banks), Outcome::Applied);
        let at = usize::from(program) * PATCH_BYTES;
        assert_eq!(midi.synthd().values()[..], image[at..at + PATCH_BYTES], "program {program}");
        assert!(midi.synthd().sounding());
        assert_eq!(midi.synthd().modulation_wheel(), 70 << 7);
        assert_eq!(midi.synthd().pitch_bend(), 70 << 7);
        assert!(midi.synthd().pedal());
    }
    // Beyond a bank of 4: the default patch, counted.
    let small = bank(4);
    let banks = Banks::new(&[&small]);
    assert_eq!(midi.midi(&message(status::PROGRAM_CHANGE, 10, 0), &banks), Outcome::PassedOver);
    assert_eq!(midi.synthd().values(), &DEFAULT_PATCH);
    // A faulty patch: the default, refused (MIDI-63).
    let mut faulty = bank(2);
    faulty[PATCH_BYTES + 3] = 0x80;
    let banks = Banks::new(&[&faulty]);
    midi.midi(&message(status::PROGRAM_CHANGE, 0, 0), &banks);
    assert_eq!(midi.midi(&message(status::PROGRAM_CHANGE, 1, 0), &banks), Outcome::PatchRefused);
    assert_eq!(midi.synthd().values(), &DEFAULT_PATCH);
    // Patch 0 faulty at the start, or no bank: the default.
    let mut faulty = bank(1);
    faulty[0] = 0xFF;
    let mut midi = fresh();
    assert_eq!(midi.start(&Banks::new(&[&faulty])), Outcome::PatchRefused);
    assert_eq!(midi.synthd().values(), &DEFAULT_PATCH);
    assert_eq!(fresh().start(&Banks::none()), Outcome::Applied);
    // No bank select: 0 and 32 are counted as ignored.
    assert_eq!(send(&mut midi, cc(0, 1)), Outcome::PassedOver);
    assert_eq!(send(&mut midi, cc(32, 1)), Outcome::PassedOver);
}

#[test]
fn sd_13_reset_all_controllers_sets_rp_015s_list_and_the_glide_switch() {
    let mut midi = fresh();
    send(&mut midi, cc(1, 90));
    send(&mut midi, cc(33, 9));
    send(&mut midi, message(status::PITCH_BEND, 3, 100));
    send(&mut midi, cc(64, 127));
    send(&mut midi, cc(7, 50));
    send(&mut midi, cc(19, 20));
    send(&mut midi, cc(65, 127));
    send(&mut midi, cc(5, 90));
    set_bend_range(&mut midi, 12);
    let before = *midi.synthd().values();
    assert_eq!(before[byte::GLIDE_SWITCH], 127);
    for channel in [0u8, 9] {
        let reset = Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, channel, 121, 0));
        assert_eq!(midi.midi(&reset, &Banks::none()), Outcome::Applied);
    }
    let synthd = midi.synthd();
    assert_eq!((synthd.modulation_wheel(), synthd.pitch_bend(), synthd.pedal()), (0, 8_192, false));
    assert_eq!(synthd.volume(), 50);
    let mut expected = before;
    expected[byte::GLIDE_SWITCH] = 0;
    assert_eq!(synthd.values(), &expected);
    assert_eq!(midi.common().channel(0).bend_range_cents, 1_200);
    // The selection is none: a data entry now changes nothing.
    assert_eq!(send(&mut midi, cc(6, 2)), Outcome::PassedOver);
    assert_eq!(midi.common().channel(0).bend_range_cents, 1_200);
}

#[test]
fn sd_11_sd_13_all_notes_off_and_the_mode_messages_release_the_keys() {
    for number in [123u8, 124, 125, 126, 127] {
        for pedal in [false, true] {
            let mut midi = fresh();
            if pedal {
                send(&mut midi, cc(64, 127));
            }
            for key in [60, 64, 67] {
                send(&mut midi, message(status::NOTE_ON, key, 100));
            }
            let values = *midi.synthd().values();
            assert_eq!(send(&mut midi, cc(number, 0)), Outcome::Applied);
            render(&mut midi, 4_800);
            assert_eq!(midi.synthd().sounding(), pedal, "controller {number}, pedal {pedal}");
            assert_eq!(midi.synthd().values(), &values);
            if pedal {
                send(&mut midi, cc(64, 0));
                // The contours come to rest within 0.2 s of the release.
                render(&mut midi, 9_600);
                assert!(!midi.synthd().sounding());
            }
        }
    }
}

#[test]
fn sd_17_each_controller_moves_its_own_control_and_no_other() {
    for (at, &(number, _)) in LAYOUT.iter().enumerate() {
        let mut midi = fresh();
        let value = if DEFAULT_PATCH[at] == 100 { 101 } else { 100 };
        assert_eq!(send(&mut midi, cc(number, value)), Outcome::Applied);
        let changed: Vec<usize> =
            (0..PATCH_BYTES).filter(|&place| midi.synthd().values()[place] != DEFAULT_PATCH[place]).collect();
        assert!(changed.contains(&at), "controller {number}");
        // A coarse controller also clears its fine byte, which the default
        // may hold at 127.
        assert!(changed.iter().all(|&place| place == at || place == at + 1), "controller {number}: {changed:?}");
    }
}

#[test]
fn sd_20_a_range_kept_under_lo_comes_back_at_hi() {
    let mut midi = fresh();
    send(&mut midi, cc(74, 48));
    assert_eq!(midi.synthd().patch().oscillators[0].range, neoteletext_audio_instruments::synthd::Range::Four);
    send(&mut midi, cc(103, 0));
    assert_eq!(midi.synthd().patch().oscillators[0].range, neoteletext_audio_instruments::synthd::Range::Lo);
    send(&mut midi, cc(103, 127));
    assert_eq!(midi.synthd().patch().oscillators[0].range, neoteletext_audio_instruments::synthd::Range::Four);
    send(&mut midi, cc(74, 100));
    assert_eq!(midi.synthd().patch().oscillators[0].range, neoteletext_audio_instruments::synthd::Range::Two);
    // Oscillator 3's LO is the Voyager's 3 FREQ, controller 80.
    send(&mut midi, cc(80, 127));
    assert_eq!(midi.synthd().patch().oscillators[2].range, neoteletext_audio_instruments::synthd::Range::Eight);
}

#[test]
fn sd_25_a_lost_block_releases_the_note_lifts_the_pedal_and_clears_the_selection() {
    let mut midi = fresh();
    send(&mut midi, cc(101, 0));
    send(&mut midi, cc(100, 0));
    send(&mut midi, cc(64, 127));
    send(&mut midi, cc(1, 40));
    send(&mut midi, cc(19, 30));
    send(&mut midi, message(status::NOTE_ON, 60, 100));
    let values = *midi.synthd().values();
    midi.lost();
    render(&mut midi, 4_800);
    assert!(!midi.synthd().sounding());
    assert!(!midi.synthd().pedal());
    assert_eq!(midi.synthd().values(), &values);
    assert_eq!(midi.synthd().modulation_wheel(), 40 << 7);
    assert_eq!(send(&mut midi, cc(6, 12)), Outcome::PassedOver);
    assert_eq!(midi.common().channel(0).bend_range_cents, 797);
}

#[test]
fn sd_26_a_patch_sounds_as_its_60_control_changes() {
    let mut values = DEFAULT_PATCH;
    for (at, value) in [
        (byte::CUTOFF_FREQUENCY, 70),
        (byte::EMPHASIS, 60),
        (byte::OSCILLATOR_2_SWITCH, 127),
        (byte::OSCILLATOR_2_FREQUENCY, 70),
    ] {
        values[at] = value;
    }
    let image = values.to_vec();
    let banks = Banks::new(&[&image]);
    let play = |inline: bool| {
        let mut midi = fresh();
        midi.start(&Banks::none());
        if inline {
            let mut messages = Vec::new();
            interface::control_changes(&values, 0, &mut |message| messages.push(owned(message)));
            assert_eq!(messages.len(), PATCH_BYTES);
            for message in messages {
                assert_eq!(midi.midi(&message, &banks), Outcome::Applied);
            }
        } else {
            midi.midi(&message(status::PROGRAM_CHANGE, 0, 0), &banks);
        }
        midi.midi(&message(status::NOTE_ON, 48, 100), &banks);
        render(&mut midi, 9_600)
    };
    assert_eq!(play(true), play(false));
}

/// The events of SD-28's verification: a program change, an inline patch's
/// worth of panel changes, a bend range and a bend, the wheel, the volume
/// and the pedal.
fn performance() -> Vec<Message<'static>> {
    let mut messages = vec![message(status::PROGRAM_CHANGE, 1, 0)];
    for (number, value) in
        [(19, 30), (51, 9), (21, 70), (89, 50), (90, 3), (104, 90), (110, 127), (74, 64), (103, 0), (37, 20)]
    {
        messages.push(cc(number, value));
    }
    for (number, value) in [(101, 0), (100, 0), (6, 12), (38, 30)] {
        messages.push(cc(number, value));
    }
    messages.push(message(status::PITCH_BEND, 5, 90));
    messages.extend([cc(7, 90), cc(1, 50), cc(33, 7), cc(64, 127)]);
    messages
}

/// A channel message, kept beyond the call that gave it.
fn owned(message: Message<'_>) -> Message<'static> {
    match message {
        Message::Channel(message) => Message::Channel(message),
        Message::SystemExclusive(_) => panic!("synthD writes no system exclusive"),
    }
}

fn restatement(tracker: &Tracker) -> Vec<Message<'static>> {
    let mut out = Vec::new();
    tracker.restate(&mut |message| out.push(owned(message)));
    out
}

#[test]
fn sd_28_the_restatement_brings_a_fresh_synthd_to_the_same_state() {
    let image = bank(4);
    let banks = Banks::new(&[&image]);
    let mut midi = fresh();
    let mut tracker = Tracker::new(&banks);
    midi.start(&banks);
    for message in performance() {
        midi.midi(&message, &banks);
        tracker.take(&message, &banks);
    }
    assert_eq!(tracker.values(), midi.synthd().values());
    let restated = restatement(&tracker);
    let mut joined = fresh();
    joined.start(&banks);
    for message in &restated {
        assert_eq!(joined.midi(message, &banks), Outcome::Applied, "{message:?}");
    }
    let state = |midi: &Midi| {
        let synthd = midi.synthd();
        (
            *synthd.values(),
            synthd.modulation_wheel(),
            synthd.pitch_bend(),
            synthd.volume(),
            synthd.pedal(),
            midi.common().channel(0).bend_range_cents,
            midi.common().channel(0).selected,
        )
    };
    let mut expected = state(&midi);
    // The restatement leaves no parameter selected.
    expected.6 = neoteletext_audio_instruments::midi::common::Selected::None;
    assert_eq!(state(&joined), expected);
    // In order: the program change, the panel by number, the range and
    // the null number, bend, volume, the wheel 1 then 33, the pedal.
    let kinds: Vec<(u8, u8)> = restated
        .iter()
        .map(|message| match message {
            Message::Channel(message) => (message.kind(), message.data1),
            Message::SystemExclusive(_) => (0, 0),
        })
        .collect();
    assert_eq!(kinds[0], (status::PROGRAM_CHANGE, 1));
    let tail = &kinds[kinds.len() - 11..];
    let control = status::CONTROL_CHANGE;
    assert_eq!(
        tail,
        [
            (control, 101),
            (control, 100),
            (control, 6),
            (control, 38),
            (control, 101),
            (control, 100),
            (status::PITCH_BEND, 5),
            (control, 7),
            (control, 1),
            (control, 33),
            (control, 64)
        ]
    );
    let panel: Vec<u8> = kinds[1..kinds.len() - 11].iter().map(|&(_, number)| number).collect();
    assert_eq!(panel, [5, 37, 19, 51, 21, 53, 74, 89, 90, 103, 104, 110]);

    // A reset that turned the GLIDE switch off is restated as 65 at 0.
    let mut values = DEFAULT_PATCH;
    values[byte::GLIDE_SWITCH] = 127;
    let image = values.to_vec();
    let banks = Banks::new(&[&image]);
    let mut tracker = Tracker::new(&banks);
    tracker.take(&cc(121, 0), &banks);
    let restated = restatement(&tracker);
    assert_eq!(restated, [cc(65, 0)]);

    // An inline patch of 60 control changes stands for the program change.
    let mut tracker = Tracker::new(&banks);
    tracker.take(&message(status::PROGRAM_CHANGE, 0, 0), &banks);
    let mut inline = Vec::new();
    interface::control_changes(&DEFAULT_PATCH, 0, &mut |message| inline.push(owned(message)));
    for message in &inline {
        tracker.take(message, &banks);
    }
    assert_eq!(restatement(&tracker), inline);
    // A fresh sound restates nothing.
    assert_eq!(restatement(&Tracker::new(&banks)), []);
}

#[test]
fn sd_28_midi_97_a_restatement_of_the_state_held_changes_no_sample() {
    let image = bank(4);
    let banks = Banks::new(&[&image]);
    let play = |restate: bool| {
        let mut midi = fresh();
        let mut tracker = Tracker::new(&banks);
        midi.start(&banks);
        let mut out = Vec::new();
        let mut events = performance();
        events.insert(5, message(status::NOTE_ON, 52, 100));
        events.push(message(status::NOTE_ON, 45, 100));
        for message in events {
            midi.midi(&message, &banks);
            tracker.take(&message, &banks);
            // A registered parameter's sequence is written whole at one
            // tick (MIDI-37), so no restatement falls inside it.
            let inside = matches!(message, Message::Channel(m) if m.kind() == status::CONTROL_CHANGE && [101, 100, 6].contains(&m.data1));
            if restate && !inside {
                for message in restatement(&tracker) {
                    midi.midi(&message, &banks);
                }
            }
            out.extend(render(&mut midi, 960));
        }
        out
    };
    assert_eq!(play(true), play(false));
}
