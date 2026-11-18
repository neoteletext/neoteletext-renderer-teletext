//! synth53, synth52 and synth51 played as the receiver plays them, through
//! the Prophet-5's MIDI interface
//! (`neoteletext-common/docs/audio/synth53-synth52-synth51.md`, cited as
//! "S53-n").

#![cfg(feature = "synth53")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    reason = "a test, in f64"
)]

mod synth53_support;

use neoteletext_audio_instruments::midi::{Banks, ChannelMessage, Instrument, Message, Outcome, status};
use neoteletext_audio_instruments::synth53::interface::{Program, Variant, edit_buffer_dump, parameter, program_dump};
use neoteletext_audio_instruments::synth53::midi::Midi;
use neoteletext_audio_instruments::synth53::{Model, Synth53};
use synth53_support::{cents, pitch, samples};

/// An instrument of `model` started on `program` as its bank's program 0.
fn started(model: Model, program: &Program) -> Midi {
    let image = program_dump(0, 0, program).unwrap();
    let mut midi = Midi::new(Synth53::new(model));
    assert_eq!(midi.start(&Banks::new(&[&image])), Outcome::Applied);
    midi
}

fn channel(kind: u8, data1: u8, data2: u8) -> Message<'static> {
    Message::Channel(ChannelMessage::new(kind, 0, data1, data2))
}

fn cc(number: u8, value: u8) -> Message<'static> {
    channel(status::CONTROL_CHANGE, number, value)
}

fn take(midi: &mut Midi, message: &Message<'_>) -> Outcome {
    midi.midi(message, &Banks::none())
}

/// Renders so many seconds, as full scale 1.
fn render(midi: &mut Midi, seconds: f64) -> Vec<f64> {
    let mut out = vec![0i16; samples(seconds)];
    midi.render(&mut out);
    out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect()
}

/// The default program with both frequency knobs two octaves up, where
/// key 69 is 440 Hz on synth53 (S53-6).
fn plain() -> Program {
    let mut program = Program::default_for(Variant::Synth53);
    let knob = Model::Synth53.frequency_knob(24);
    program.set(parameter::OSC_A_FREQUENCY, knob);
    program.set(parameter::OSC_B_FREQUENCY, knob);
    program
}

/// The program with one parameter changed.
fn with(mut program: Program, number: u8, value: u8) -> Program {
    assert!(program.set(number, value));
    program
}

/// One note of `program` held for `seconds`, after `before`.
fn note(model: Model, program: &Program, before: &[Message<'_>], key: u8, seconds: f64) -> Vec<f64> {
    let mut midi = started(model, program);
    for message in before {
        take(&mut midi, message);
    }
    take(&mut midi, &channel(status::NOTE_ON, key, 100));
    render(&mut midi, seconds)
}

/// The frequency of a MIDI key.
fn equal(key: u8) -> f64 {
    440.0 * ((f64::from(key) - 69.0) / 12.0).exp2()
}

// Verifies S53-1: synth52 and synth51 are one model and synth53 another.
#[test]
fn s53_1_synth52_and_synth51_sound_alike_and_synth53_does_not() {
    let program = Program::default_for(Variant::Synth52);
    let [three, two, one] =
        [Model::Synth53, Model::Synth52, Model::Synth51].map(|model| note(model, &program, &[], 60, 0.3));
    assert_eq!(two, one);
    assert_ne!(three, two);
}

// Verifies S53-3 and S53-4: every channel alike; velocity does not change
// the sound; a note on of velocity 0 is a note off.
#[test]
fn s53_3_s53_4_channels_and_velocity_change_nothing() {
    let play = |channel_number: u8, velocity: u8, off: Message<'static>| {
        let mut midi = started(Model::Synth53, &plain());
        take(&mut midi, &Message::Channel(ChannelMessage::new(status::NOTE_ON, channel_number, 60, velocity)));
        let mut signal = render(&mut midi, 0.2);
        take(&mut midi, &off);
        signal.extend(render(&mut midi, 0.3));
        signal
    };
    let first = play(0, 100, channel(status::NOTE_OFF, 60, 0));
    for channel_number in 1..16 {
        assert_eq!(play(channel_number, 100, channel(status::NOTE_OFF, 60, 0)), first, "channel {channel_number}");
    }
    for velocity in [1, 64, 127] {
        assert_eq!(play(0, velocity, channel(status::NOTE_OFF, 60, 64)), first, "velocity {velocity}");
    }
    assert_eq!(play(0, 100, channel(status::NOTE_ON, 60, 0)), first);
}

/// The pitch of key 69 under a bend of `value` after `range`.
fn bent(range: &[Message<'_>], value: u16) -> f64 {
    let bend = channel(status::PITCH_BEND, (value & 0x7F) as u8, (value >> 7) as u8);
    let signal = note(Model::Synth53, &plain(), &[range, &[bend]].concat(), 69, 0.6);
    pitch(&signal[samples(0.1)..])
}

// Verifies S53-8: controller 70 and registered parameter 0 set the range,
// held to an octave; seven semitones before either; a program change
// leaves it.
#[test]
fn s53_8_the_pitch_wheel_bends_by_its_range() {
    let centre = bent(&[], 8_192);
    let check = |range: &[Message<'_>], semitones: f64| {
        let (down, up) = (cents(centre, bent(range, 0)), cents(centre, bent(range, 16_383)));
        let whole = semitones * 100.0;
        assert!((down + whole).abs() < 1.5, "{semitones}: down {down}");
        assert!((up - whole * 8_191.0 / 8_192.0).abs() < 1.5, "{semitones}: up {up}");
    };
    check(&[], 7.0);
    for (value, semitones) in [(0, 1.0), (6, 7.0), (11, 12.0)] {
        check(&[cc(70, value)], semitones);
    }
    for (semitones, held) in [(1, 1.0), (7, 7.0), (24, 12.0)] {
        check(&[cc(101, 0), cc(100, 0), cc(6, semitones)], held);
    }
    let image = program_dump(0, 0, &with(plain(), parameter::PITCH_WHEEL_RANGE, 0)).unwrap();
    let mut midi = started(Model::Synth53, &plain());
    take(&mut midi, &cc(70, 2));
    midi.midi(&channel(status::PROGRAM_CHANGE, 0, 0), &Banks::new(&[&image]));
    assert_eq!(midi.family().bend_range_cents(), 300);
}

// Verifies S53-12: brightness turns the cutoff knob so many steps higher,
// as far as its top, and reset all controllers leaves it (S53-16).
#[test]
fn s53_12_brightness_is_steps_of_the_cutoff() {
    let at = |cutoff: u8, before: &[Message<'_>]| {
        note(Model::Synth53, &with(plain(), parameter::CUTOFF, cutoff), before, 57, 0.3)
    };
    assert_eq!(at(40, &[cc(74, 30)]), at(70, &[]));
    assert_eq!(at(100, &[cc(74, 127)]), at(120, &[]));
    assert_ne!(at(40, &[cc(74, 30)]), at(40, &[]));
    assert_eq!(at(40, &[cc(74, 30), cc(121, 0)]), at(70, &[]));
}

// Verifies S53-15: controllers 123 to 127 each release every key held,
// the pedal still holding, and change neither unison nor the channels.
#[test]
fn s53_15_all_notes_off_and_the_mode_messages() {
    for number in 123..=127 {
        for pedal in [false, true] {
            let mut midi = started(Model::Synth53, &plain());
            if pedal {
                take(&mut midi, &cc(64, 127));
            }
            for key in [60, 64, 67] {
                take(&mut midi, &channel(status::NOTE_ON, key, 100));
            }
            render(&mut midi, 0.1);
            take(&mut midi, &cc(number, 0));
            render(&mut midi, 1.0);
            let sounding = midi.family().sounding_voices();
            assert_eq!(sounding, if pedal { 3 } else { 0 }, "controller {number}, pedal {pedal}");
            assert_eq!(midi.part().program(), &plain());
            // Every channel is still heard.
            take(&mut midi, &Message::Channel(ChannelMessage::new(status::NOTE_ON, 9, 72, 100)));
            render(&mut midi, 0.1);
            assert_eq!(midi.family().sounding_voices(), sounding + 1);
        }
    }
}

// Verifies S53-38 and S53-39: FILTER REV SELECT, Vintage and velocity's
// switches are carried and not heard.
#[test]
fn s53_38_s53_39_the_revision_and_vintage_are_carried_and_not_heard() {
    for model in [Model::Synth53, Model::Synth52, Model::Synth51] {
        let heard = |program: Program| note(model, &program, &[], 60, 0.2);
        let base = heard(plain());
        assert_eq!(heard(with(plain(), parameter::FILTER_REV_SELECT, 0)), base, "{model:?}");
        assert_eq!(heard(with(plain(), parameter::FILTER_REV_SELECT, 1)), base, "{model:?}");
        assert_eq!(heard(with(plain(), parameter::VINTAGE, 127)), base, "{model:?}");
        assert_eq!(heard(with(plain(), parameter::VELOCITY_TO_AMP, 1)), base, "{model:?}");
    }
}

// Verifies S53-41: the cutoff follows the key not at all, by half or
// wholly: a self-oscillating filter rises 0, 12 and 24 semitones from key
// 36 to key 60.
#[test]
fn s53_41_half_keyboard_tracking() {
    let whistle = |track: u8, key: u8| {
        let mut program = with(plain(), parameter::FILTER_KEYBOARD_TRACK, track);
        program.set(parameter::OSC_A_SAW, 0);
        program.set(parameter::RESONANCE, 120);
        program.set(parameter::CUTOFF, 48);
        let signal = note(Model::Synth53, &program, &[], key, 1.0);
        pitch(&signal[samples(0.5)..])
    };
    for (track, semitones) in [(0, 0.0), (1, 12.0), (2, 24.0)] {
        let rise = cents(whistle(track, 36), whistle(track, 60)) / 100.0;
        assert!((rise - semitones).abs() < 1.0, "track {track}: {rise}");
    }
}

// Verifies S53-42 items 1 and 3: the unison count stacks v + 1 voices,
// five from 4 up; the key priority chooses the lowest or the last key.
#[test]
fn s53_42_unison_count_and_key_priority() {
    for count in 0..=10u8 {
        let program = with(with(plain(), parameter::UNISON, 1), parameter::UNISON_VOICE_COUNT, count);
        let mut midi = started(Model::Synth53, &program);
        take(&mut midi, &channel(status::NOTE_ON, 60, 100));
        render(&mut midi, 0.5);
        assert_eq!(midi.family().sounding_voices(), usize::from(count.min(4) + 1), "count {count}");
    }
    for mode in 0..=3u8 {
        // The filter closed to the note's own pitch and following the keys
        // leaves little but the fundamental, whose pitch reads cleanly.
        let program = with(with(plain(), parameter::UNISON, 1), parameter::RETRIGGER_AND_UNISON, mode);
        let mut midi = started(Model::Synth53, &with(program, parameter::CUTOFF, 36));
        let mut heard = Vec::new();
        for (key, down) in [(60, true), (64, true), (55, true), (55, false)] {
            take(&mut midi, &channel(if down { status::NOTE_ON } else { status::NOTE_OFF }, key, 100));
            let signal = render(&mut midi, 0.4);
            heard.push(pitch(&signal[samples(0.2)..]));
        }
        let expected = if mode < 2 { [60, 60, 55, 60] } else { [60, 64, 55, 64] };
        for (found, wanted) in heard.iter().zip(expected) {
            assert!(cents(equal(wanted), *found).abs() < 5.0, "mode {mode}: {heard:?}");
        }
    }
}

// Verifies S53-42 item 3: whether a key that takes the note restarts the
// envelopes. With the amplifier's sustain at 0 a note dies away, and only
// a restart is heard again.
#[test]
fn s53_42_retrigger_restarts_the_envelopes() {
    for (mode, restarts) in [
        (0, [true, false, false, false]),
        (1, [true, false, true, false]),
        (2, [true, false, false, false]),
        (3, [true, true, true, false]),
    ] {
        let mut program = with(with(plain(), parameter::UNISON, 1), parameter::RETRIGGER_AND_UNISON, mode);
        program.set(parameter::SUSTAIN_VCA, 0);
        program.set(parameter::DECAY_VCA, 40);
        let mut midi = started(Model::Synth53, &program);
        let mut heard = Vec::new();
        for (key, down) in [(60, true), (64, true), (55, true), (55, false)] {
            take(&mut midi, &channel(if down { status::NOTE_ON } else { status::NOTE_OFF }, key, 100));
            let signal = render(&mut midi, 1.5);
            heard.push(synth53_support::rms(&signal[..samples(0.05)]));
        }
        let loud: Vec<bool> = heard.iter().map(|&level| level > 0.01).collect();
        assert_eq!(loud, restarts, "mode {mode}: {heard:?}");
    }
}

// Verifies S53-42 item 2, in part: one voice is not detuned, and five
// are.
#[test]
fn s53_42_detune_spreads_the_voices_about_the_note() {
    let unison = |count: u8, detune: u8| {
        let mut program = with(with(plain(), parameter::UNISON, 1), parameter::UNISON_VOICE_COUNT, count);
        program.set(parameter::UNISON_DETUNE, detune);
        note(Model::Synth53, &with(program, parameter::CUTOFF, 36), &[], 69, 1.0)
    };
    assert_eq!(unison(0, 7), unison(0, 0));
    assert_ne!(unison(4, 7), unison(4, 0));
}

// Verifies S53-43: the initial amount and the wheel add, held to the whole.
#[test]
fn s53_43_the_initial_amount_adds_to_the_wheel() {
    let vibrato = |initial: u8, wheel: u8| {
        let program = with(with(plain(), parameter::LFO_FREQ_A, 1), parameter::LFO_INITIAL_AMOUNT, initial);
        note(Model::Synth53, &program, &[cc(1, wheel)], 69, 0.5)
    };
    let still = note(Model::Synth53, &with(plain(), parameter::LFO_FREQ_A, 1), &[], 69, 0.5);
    assert_eq!(vibrato(0, 0), still);
    assert_ne!(vibrato(60, 0), still);
    assert_eq!(vibrato(120, 0), vibrato(0, 127));
    assert_eq!(vibrato(120, 127), vibrato(120, 0));
    assert_eq!(vibrato(60, 127), vibrato(0, 127));
}

// Verifies S53-29 and S53-28: an edit buffer dump sets the program; one of
// another length sets the default and is refused; a program dump in a
// block is passed over.
#[test]
fn s53_29_an_edit_buffer_dump_sets_the_program() {
    let dull = with(plain(), parameter::CUTOFF, 20);
    let dump = edit_buffer_dump(&dull);
    let mut midi = started(Model::Synth53, &plain());
    assert_eq!(take(&mut midi, &Message::SystemExclusive(&dump[1..])), Outcome::Applied);
    assert_eq!(midi.family().program(), &dull);
    let short = [&dump[1..155], &[0xF7]].concat();
    assert_eq!(take(&mut midi, &Message::SystemExclusive(&short)), Outcome::PatchRefused);
    assert_eq!(midi.family().program(), &Program::default_for(Variant::Synth53));
    let program = program_dump(0, 0, &dull).unwrap();
    assert_eq!(take(&mut midi, &Message::SystemExclusive(&program[1..])), Outcome::PassedOver);
    assert_eq!(midi.family().program(), &Program::default_for(Variant::Synth53));
}

/// Plays `events` (a sample and a message each) for `length` samples,
/// rendered in calls of at most `chunk` samples.
fn played(events: &[(usize, Message<'_>)], length: usize, chunk: usize) -> Vec<i16> {
    let mut midi = started(Model::Synth53, &plain());
    let mut out = vec![0i16; length];
    let mut done = 0;
    for (at, message) in events.iter().map(|(at, message)| (*at, Some(message))).chain([(length, None)]) {
        while done < at {
            let next = (done + chunk).min(at);
            midi.render(&mut out[done..next]);
            done = next;
        }
        if let Some(message) = message {
            take(&mut midi, message);
        }
    }
    out
}

/// A tune with parameter messages, a program's dump, the wheels, the
/// pedal and unison switched.
fn tune(dump: &[u8]) -> Vec<(usize, Message<'_>)> {
    vec![
        (0, channel(status::NOTE_ON, 60, 100)),
        (500, cc(99, 0)),
        (500, cc(98, parameter::CUTOFF)),
        (500, cc(6, 0)),
        (500, cc(38, 50)),
        (3_000, channel(status::NOTE_ON, 64, 100)),
        (4_100, cc(1, 90)),
        (5_000, Message::SystemExclusive(dump)),
        (6_200, channel(status::PITCH_BEND, 0, 70)),
        (7_000, cc(64, 127)),
        (7_500, channel(status::NOTE_OFF, 60, 0)),
        (9_000, cc(112, 1)),
        (9_000, channel(status::NOTE_ON, 67, 100)),
        (12_000, cc(64, 0)),
    ]
}

// Verifies S53-46: the same events give the same samples however the
// rendering is divided.
#[test]
fn s53_46_the_same_events_give_the_same_samples_in_any_calls() {
    let dump = edit_buffer_dump(&with(plain(), parameter::RESONANCE, 80));
    let events = tune(&dump[1..]);
    let length = 16_000;
    let whole = played(&events, length, length);
    for chunk in [1, 7, 960, 4_800] {
        assert!(played(&events, length, chunk) == whole, "calls of {chunk}");
    }
}

// Verifies S53-46 and S53-19: a restatement written at a tick changes
// nothing that is heard.
#[test]
fn s53_46_a_restatement_is_inaudible() {
    let dump = edit_buffer_dump(&with(plain(), parameter::RESONANCE, 80));
    let events = tune(&dump[1..]);
    let length = 16_000;
    let without = played(&events, length, length);
    for at in [1_000, 4_500, 6_000, 8_000, 10_000, 13_000] {
        // The part as it stands at `at`, from the events before.
        let mut midi = started(Model::Synth53, &plain());
        for (_, message) in events.iter().filter(|(time, _)| *time < at) {
            take(&mut midi, message);
        }
        let mut restatement: Vec<(Vec<u8>, Option<ChannelMessage>)> = Vec::new();
        midi.part().restate(0, &mut |message| match message {
            Message::Channel(channel) => restatement.push((Vec::new(), Some(channel))),
            Message::SystemExclusive(bytes) => restatement.push((bytes.to_vec(), None)),
        });
        let mut with_it: Vec<(usize, Message<'_>)> =
            events.iter().filter(|(time, _)| *time < at).map(|(time, message)| (*time, *message)).collect();
        for (bytes, channel) in &restatement {
            with_it.push((at, channel.map_or(Message::SystemExclusive(bytes), Message::Channel)));
        }
        with_it.extend(events.iter().filter(|(time, _)| *time >= at).map(|(time, message)| (*time, *message)));
        assert!(played(&with_it, length, length) == without, "a restatement at {at}");
    }
}

// Verifies S53-19: a program change whose unison is off followed at the
// same tick by unison on, with unison on before, lets go of nothing.
#[test]
fn s53_19_a_unison_switched_off_and_on_in_one_tick_lets_go_of_nothing() {
    let unison = with(plain(), parameter::UNISON, 1);
    let image = [program_dump(0, 0, &unison).unwrap(), program_dump(0, 1, &plain()).unwrap()].concat();
    let banks = Banks::new(&[&image]);
    let run = |switch: bool| {
        let mut midi = Midi::new(Synth53::new(Model::Synth53));
        midi.start(&banks);
        midi.midi(&channel(status::NOTE_ON, 60, 100), &banks);
        let mut signal = render(&mut midi, 0.2);
        if switch {
            midi.midi(&channel(status::PROGRAM_CHANGE, 1, 0), &banks);
            midi.midi(&cc(112, 1), &banks);
        }
        signal.extend(render(&mut midi, 0.3));
        signal
    };
    assert_eq!(run(true), run(false));
}

// Verifies S53-46: after lost input, a data entry changes nothing.
#[test]
fn s53_46_after_lost_input_a_data_entry_changes_nothing() {
    let mut midi = started(Model::Synth53, &plain());
    take(&mut midi, &cc(99, 0));
    take(&mut midi, &cc(98, parameter::CUTOFF));
    midi.lost();
    assert_eq!(take(&mut midi, &cc(38, 3)), Outcome::PassedOver);
    assert_eq!(midi.family().program(), &plain());
}
