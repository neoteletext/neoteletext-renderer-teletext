//! The CS-80's control sweep: every lever, tab and selector of both
//! tones and of the rest of the panel, and every message the instrument
//! takes, changed under sounding notes, and the step each makes in the
//! signal (`control_sweep_support.rs` says how it is measured).

#![cfg(feature = "cs80")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    reason = "a test's table of controls, each value within its control's range"
)]

mod control_sweep_support;

use control_sweep_support::{
    AFTER, Allowed, CHANNEL_PRESSURE, CONTROL, Change, Clock, Instrument, KEY_PRESSURE, NOTE_OFF, NOTE_ON, PITCH_BEND,
    PROGRAM, Sound, all_sound_off_silences, check, continuous, messages_after_all_sound_off_are_taken, on_off, run,
    sweep, switch, to_crest,
};
use neoteletext_audio_instruments::cs80::patch::{Control, Global};
use neoteletext_audio_instruments::cs80::{Cs80, Patch};

impl Instrument for Cs80 {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

const CONTROLS: [Control; 26] = [
    Control::PwmSpeed,
    Control::Pwm,
    Control::PulseWidth,
    Control::Square,
    Control::Sawtooth,
    Control::Noise,
    Control::Hpf,
    Control::HpfResonance,
    Control::Lpf,
    Control::LpfResonance,
    Control::InitialLevel,
    Control::AttackLevel,
    Control::FilterAttack,
    Control::FilterDecay,
    Control::FilterRelease,
    Control::VcfLevel,
    Control::Sine,
    Control::Attack,
    Control::Decay,
    Control::Sustain,
    Control::Release,
    Control::Level,
    Control::TouchBrilliance,
    Control::TouchLevel,
    Control::PressureBrilliance,
    Control::PressureLevel,
];
const GLOBALS: [Global; 33] = [
    Global::FeetOne,
    Global::FeetTwo,
    Global::Detune,
    Global::Mix,
    Global::Brilliance,
    Global::Resonance,
    Global::KeyboardBrillianceLow,
    Global::KeyboardBrillianceHigh,
    Global::KeyboardLevelLow,
    Global::KeyboardLevelHigh,
    Global::SubFunction,
    Global::SubSpeed,
    Global::SubVco,
    Global::SubVcf,
    Global::SubVca,
    Global::TouchPitchBend,
    Global::TouchSpeed,
    Global::TouchVco,
    Global::TouchVcf,
    Global::RingAttack,
    Global::RingDecay,
    Global::RingDepth,
    Global::RingSpeed,
    Global::RingModulation,
    Global::SustainMode,
    Global::Sustain,
    Global::Glissando,
    Global::GlideTime,
    Global::Effect,
    Global::Chorus,
    Global::Tremolo,
    Global::EffectSpeed,
    Global::EffectDepth,
];

/// An instrument playing a patch, its modulation wheel at `wheel`.
fn instrument(patch: &Patch, wheel: u8) -> Cs80 {
    let mut cs80 = Cs80::new();
    cs80.set_patch(patch);
    cs80.event(CONTROL, 1, wheel);
    cs80
}

/// The panel at rest: channel I's sawtooth.
fn bright() -> Patch {
    Patch::DEFAULT
}

/// Channel I's sine alone, past the filters, the amplifier a gate: the
/// sound whose own steps are smallest, under which the least step of a
/// level shows.
fn pure() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.set(Global::Resonance, 0);
    for (control, value) in [(Control::Sawtooth, 0), (Control::VcfLevel, 0), (Control::Sine, 127)] {
        patch.tones[0].set(control, value);
    }
    patch
}

/// Every part of the instrument at work on a mellow sound, so that every
/// lever has something to change: both channels, their filters part
/// closed with their envelopes, the sub oscillator into all three, a
/// little ring modulation, the touch responses, the sustain and the
/// chorus.
fn everything() -> Patch {
    let mut patch = Patch::DEFAULT;
    for tone in &mut patch.tones {
        for (control, value) in [
            (Control::PwmSpeed, 60),
            (Control::Pwm, 40),
            (Control::PulseWidth, 50),
            (Control::Square, 1),
            (Control::Sawtooth, 0),
            (Control::Noise, 5),
            (Control::Hpf, 20),
            (Control::HpfResonance, 20),
            (Control::Lpf, 70),
            (Control::LpfResonance, 40),
            (Control::InitialLevel, 20),
            (Control::AttackLevel, 60),
            (Control::FilterAttack, 30),
            (Control::FilterDecay, 70),
            (Control::FilterRelease, 70),
            (Control::VcfLevel, 100),
            (Control::Sine, 60),
            (Control::Attack, 10),
            (Control::Decay, 70),
            (Control::Sustain, 100),
            (Control::Release, 70),
            (Control::Level, 100),
            (Control::TouchBrilliance, 40),
            (Control::TouchLevel, 40),
            (Control::PressureBrilliance, 40),
            (Control::PressureLevel, 40),
        ] {
            tone.set(control, value);
        }
    }
    for (global, value) in [
        (Global::Detune, 70),
        (Global::Mix, 64),
        (Global::Resonance, 30),
        (Global::SubSpeed, 80),
        (Global::SubVco, 10),
        (Global::SubVcf, 30),
        (Global::SubVca, 30),
        (Global::TouchPitchBend, 20),
        (Global::TouchSpeed, 30),
        (Global::TouchVco, 20),
        (Global::TouchVcf, 30),
        (Global::RingAttack, 30),
        (Global::RingDecay, 60),
        (Global::RingDepth, 30),
        (Global::RingSpeed, 60),
        (Global::RingModulation, 20),
        (Global::Sustain, 60),
        (Global::GlideTime, 20),
        (Global::Effect, 1),
        (Global::Chorus, 1),
        (Global::EffectSpeed, 60),
        (Global::EffectDepth, 80),
    ] {
        patch.set(global, value);
    }
    patch
}

/// The modulation wheel of the sound with everything at work.
const WHEEL: u8 = 40;
/// The keys of the chord: no two share a harmonic below the fifth.
const CHORD: [u8; 5] = [45, 52, 59, 66, 73];

fn sounds() -> Vec<Sound<Cs80>> {
    let held = |name, patch: fn() -> Patch, keys: &'static [u8], wheel: u8| Sound {
        name,
        make: Box::new(move || instrument(&patch(), wheel)),
        play: Box::new(move |cs80: &mut Cs80, clock: &mut Clock| {
            for &key in keys {
                cs80.event(NOTE_ON, key, 100);
            }
            to_crest(cs80, 4 * AFTER, clock)
        }),
    };
    vec![
        held("low bright", bright, &[36], 0),
        held("middle", everything, &[60], WHEEL),
        held("high", bright, &[96], 0),
        held("pure", pure, &[60], 0),
        held("chord", pure, &CHORD, 0),
        Sound {
            name: "release",
            make: Box::new(|| instrument(&everything(), WHEEL)),
            play: Box::new(|cs80: &mut Cs80, clock: &mut Clock| {
                cs80.event(NOTE_ON, 60, 100);
                to_crest(cs80, 4 * AFTER, clock);
                cs80.event(NOTE_OFF, 60, 0);
                to_crest(cs80, AFTER / 2, clock)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(|| instrument(&bright(), 0)),
            play: Box::new(|cs80: &mut Cs80, _clock: &mut Clock| {
                cs80.event(NOTE_ON, 60, 100);
                run(cs80, 2 * AFTER);
                cs80.event(NOTE_OFF, 60, 0);
                run(cs80, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is, from where `cs80` has its controls.
fn changes(cs80: &Cs80) -> Vec<Change<Cs80>> {
    let patch = *cs80.patch();
    let wheel = if patch == everything() { i32::from(WHEEL) } else { 0 };
    let mut all = Vec::new();
    let position = |position: i32| position.to_string();
    for channel in 0..2 {
        for control in CONTROLS {
            let set = move |cs80: &mut Cs80, value: i32| {
                let mut patch = *cs80.patch();
                patch.tones[channel].set(control, value as u8);
                cs80.set_patch(&patch);
            };
            let name = format!("{} {control:?}", ["I", "II"][channel]);
            let (most, at) = (i32::from(control.max()), patch.tones[channel].get(control).into());
            if most > 5 {
                all.extend(continuous(&name, (0, most), at, set));
            } else {
                all.extend(switch(&name, most + 1, at, position, set));
            }
        }
    }
    for global in GLOBALS {
        let set = move |cs80: &mut Cs80, value: i32| {
            let mut patch = *cs80.patch();
            patch.set(global, value as u8);
            cs80.set_patch(&patch);
        };
        let (name, most, at) = (format!("{global:?}"), i32::from(global.max()), patch.get(global).into());
        if most > 5 {
            all.extend(continuous(&name, (0, most), at, set));
        } else {
            all.extend(switch(&name, most + 1, at, position, set));
        }
    }

    // The messages. A controller's place before the change is given by
    // a message before the keys, as a lever's is by the patch.
    let controller = |number: u8| move |cs80: &mut Cs80, value: i32| cs80.event(CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(switch("pedal (64)", 2, 0, on_off, |cs80: &mut Cs80, value| {
        cs80.event(CONTROL, 64, if value == 0 { 0 } else { 127 });
    }));
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |cs80: &mut Cs80, value: i32| {
        cs80.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    // The pressure of a key that is down: none before the change, since
    // a key newly down is not yet pressed.
    all.extend(continuous("key pressure", (0, 127), 0, |cs80: &mut Cs80, value: i32| {
        cs80.event(KEY_PRESSURE, 60, value as u8);
    }));
    all.extend(continuous("channel pressure", (0, 127), 0, |cs80: &mut Cs80, value: i32| {
        cs80.event(CHANNEL_PRESSURE, value as u8, 0);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |cs80: &mut Cs80| cs80.event(status, data1, data2))
    };
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(Change::starting("all sound off (120), then a key", "sent", |cs80: &mut Cs80| {
        cs80.event(CONTROL, 120, 0);
        cs80.event(NOTE_ON, 64, 100);
    }));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike =
        |name: &str, key: u8| Change::starting(name, "sent", move |cs80: &mut Cs80| cs80.event(NOTE_ON, key, 100));
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    all.push(Change::starting("note on, nine more keys", "sent", |cs80: &mut Cs80| {
        for key in [40, 47, 54, 61, 68, 75, 82, 89, 94] {
            cs80.event(NOTE_ON, key, 100);
        }
    }));
    // What the instrument does not take: nothing may move.
    all.push(message("program change (ignored)", PROGRAM, 5, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(Change::new("release_all", "called", Cs80::release_all));
    all.push(Change::new("reset", "called", Cs80::reset));
    all
}

#[test]
fn no_control_steps_the_signal_but_the_switches_that_do() {
    let rows = sweep(&sounds(), &changes);
    check(
        "CS-80",
        &rows,
        &[Allowed {
            controls: &["reset"],
            why: "no control: the caller's own cut, which puts the instrument back as new at once",
        }],
    );
}

#[test]
fn all_sound_off_takes_the_sound_out_and_leaves_silence() {
    // The effect's lines hold 30 ms of the sound and let it out after.
    all_sound_off_silences(&sounds(), &|cs80: &mut Cs80| cs80.event(CONTROL, 120, 0), 4_800);
}

#[test]
fn messages_that_come_during_all_sound_off_are_taken_after_it() {
    messages_after_all_sound_off_are_taken(&|| instrument(&bright(), 0), &Cs80::event);
}
