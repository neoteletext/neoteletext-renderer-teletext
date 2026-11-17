//! synth7's control sweep: every message the rack takes, a change of
//! program and a change of bank, under sounding notes, and the step each
//! makes in the signal (`control_sweep_support.rs` says how it is
//! measured). A voice's parameters are a note's from its start, so
//! nothing of a patch can change under a note but through these.

#![cfg(feature = "synth7")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    reason = "a test's table of controls, each value within its control's range"
)]

mod control_sweep_support;

use control_sweep_support::{
    AFTER, Allowed, CHANNEL_PRESSURE, CONTROL, Change, Clock, Instrument, KEY_PRESSURE, NOTE_OFF, NOTE_ON, PITCH_BEND,
    PROGRAM, Sound, all_sound_off_leaves_the_pedal, all_sound_off_silences, check, continuous, on_off, run, sweep,
    switch, to_crest,
};
use neoteletext_audio_instruments::synth7::{Bank, Patch, Rack, VoiceParams};

impl Instrument for Rack {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

/// The plain sine with something changed.
fn patch(change: impl FnOnce(&mut VoiceParams)) -> Patch {
    let mut params = *Patch::DEFAULT.params();
    change(&mut params);
    Patch::new(params).expect("a patch of the test")
}

/// Operator 2 into operator 1 with feedback: a bright sound.
fn bright() -> Patch {
    patch(|params| {
        params.algorithm = 0;
        params.feedback = 7;
        params.operators[1].output_level = 85;
    })
}

/// The oscillator at work on pitch and on level, with a release that
/// lasts: every controller has something to change.
fn moving(wave: u8) -> Patch {
    patch(|params| {
        params.lfo_wave = wave;
        params.lfo_speed = 60;
        params.lfo_pitch_mod_depth = 20;
        params.lfo_amp_mod_depth = 60;
        // The least sensitivity: the level moves by 3 dB, so that a
        // note struck at one place of the oscillator's cycle is about
        // as loud as one struck at another.
        params.operators[0].amp_mod_sensitivity = 1;
        params.operators[0].eg_rates = [80, 60, 50, 45];
        params.operators[0].eg_levels = [99, 90, 85, 0];
    })
}

/// The programs of the bank: the plain sine, the bright sound, the
/// moving sound with the oscillator's sine and with its square.
fn bank() -> Bank {
    let mut bank = Bank::new();
    for (index, patch) in [Patch::DEFAULT, bright(), moving(4), moving(3)].into_iter().enumerate() {
        bank.set(index, patch);
    }
    bank
}

/// Another bank: the same programs the other way round.
fn other_bank() -> Bank {
    let mut bank = Bank::new();
    for (index, patch) in [moving(3), moving(4), bright(), Patch::DEFAULT].into_iter().enumerate() {
        bank.set(index, patch);
    }
    bank
}

/// A rack with the bank, part 1 at a program and its wheel at `wheel`.
fn rack(program: u8, wheel: u8) -> Rack {
    let mut rack = Rack::new();
    rack.set_bank(&bank());
    rack.event(PROGRAM, program, 0);
    rack.event(CONTROL, 1, wheel);
    rack
}

/// The modulation wheel of the moving sound.
const WHEEL: u8 = 40;
/// The keys of the chord: no two share a harmonic below the fifth.
const CHORD: [u8; 5] = [45, 52, 59, 66, 73];

fn sounds() -> Vec<Sound<Rack>> {
    let held = |name, program: u8, keys: &'static [u8], wheel: u8| Sound {
        name,
        make: Box::new(move || rack(program, wheel)),
        play: Box::new(move |rack: &mut Rack, clock: &mut Clock| {
            for &key in keys {
                rack.event(NOTE_ON, key, 100);
            }
            to_crest(rack, 4 * AFTER, clock)
        }),
    };
    vec![
        held("low bright", 1, &[36], 0),
        held("middle", 2, &[60], WHEEL),
        held("high", 1, &[96], 0),
        held("pure", 0, &[60], 0),
        held("chord", 0, &CHORD, 0),
        Sound {
            name: "release",
            make: Box::new(|| rack(2, WHEEL)),
            play: Box::new(|rack: &mut Rack, clock: &mut Clock| {
                rack.event(NOTE_ON, 60, 100);
                to_crest(rack, 4 * AFTER, clock);
                rack.event(NOTE_OFF, 60, 0);
                to_crest(rack, AFTER / 2, clock)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(|| rack(0, 0)),
            play: Box::new(|rack: &mut Rack, _clock: &mut Clock| {
                rack.event(NOTE_ON, 60, 100);
                run(rack, 2 * AFTER);
                rack.event(NOTE_OFF, 60, 0);
                run(rack, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is. The wheel's place before its change is where
/// the sound has it.
fn changes(wheel: i32) -> Vec<Change<Rack>> {
    let mut all = Vec::new();
    let controller = |number: u8| move |rack: &mut Rack, value: i32| rack.event(CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(switch("pedal (64)", 2, 0, on_off, |rack: &mut Rack, value| {
        rack.event(CONTROL, 64, if value == 0 { 0 } else { 127 });
    }));
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |rack: &mut Rack, value: i32| {
        rack.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |rack: &mut Rack| rack.event(status, data1, data2))
    };
    // A change of program is the change of voice: the notes that sound
    // keep theirs, and the part's oscillator takes the new one's.
    for program in 0..4 {
        all.push(message(&format!("program change to {program}"), PROGRAM, program, 0));
    }
    all.push(Change::new("set_bank", "another bank", |rack: &mut Rack| rack.set_bank(&other_bank())));
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(Change::starting("all sound off (120), then a key", "sent", |rack: &mut Rack| {
        rack.event(CONTROL, 120, 0);
        rack.event(NOTE_ON, 64, 100);
    }));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike =
        |name: &str, key: u8| Change::starting(name, "sent", move |rack: &mut Rack| rack.event(NOTE_ON, key, 100));
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    all.push(Change::starting("note on, seventeen more keys", "sent", |rack: &mut Rack| {
        for key in 30..47 {
            rack.event(NOTE_ON, key * 2, 100);
        }
    }));
    // What the rack does not take, and what is another part's: nothing
    // may move.
    all.push(message("key pressure (ignored)", KEY_PRESSURE, 60, 127));
    all.push(message("channel pressure (ignored)", CHANNEL_PRESSURE, 127, 0));
    all.push(message("program change to 40 (ignored)", PROGRAM, 40, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(message("volume of another part", CONTROL | 1, 7, 0));
    all.push(message("all sound off of another part", CONTROL | 1, 120, 0));
    all.push(message("pitch bend of another part", PITCH_BEND | 1, 0, 0));
    all.push(Change::new("release_all", "called", Rack::release_all));
    all.push(Change::new("reset", "called", Rack::reset));
    all
}

#[test]
fn no_message_steps_the_signal() {
    // The sounds of program 2 are the ones with the wheel up.
    let rows = sweep(&sounds(), &|rack: &Rack| {
        let mut probe = rack.clone();
        probe.event(CONTROL, 1, 0);
        changes(if probe.clone().eq_wheel(rack) { 0 } else { i32::from(WHEEL) })
    });
    check(
        "synth7",
        &rows,
        &[
            Allowed {
                controls: &["all notes off (123)", "release_all"],
                why: "the sound's own release, at rate 99: the envelope at its quickest",
            },
            Allowed {
                controls: &["reset"],
                why: "no control: the caller's own cut, which puts the rack back as new at once",
            },
        ],
    );
}

/// Whether two racks render the same next samples: here, whether
/// putting the wheel at nothing changed anything.
trait SameWheel {
    fn eq_wheel(self, other: &Rack) -> bool;
}

impl SameWheel for Rack {
    fn eq_wheel(mut self, other: &Rack) -> bool {
        let mut other = other.clone();
        self.event(NOTE_ON, 60, 100);
        other.event(NOTE_ON, 60, 100);
        run(&mut self, 8 * AFTER) == run(&mut other, 8 * AFTER)
    }
}

#[test]
fn all_sound_off_takes_the_sound_out_and_leaves_silence() {
    // The cut's 32 blocks, and one more for the block it waits for.
    all_sound_off_silences(&sounds(), &|rack: &mut Rack| rack.event(CONTROL, 120, 0), 33 * 32);
}

#[test]
fn all_sound_off_leaves_the_pedal_where_it_is() {
    all_sound_off_leaves_the_pedal(&|| rack(0, 0), &Rack::event);
}
