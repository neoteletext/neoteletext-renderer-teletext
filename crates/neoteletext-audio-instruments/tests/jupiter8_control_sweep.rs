//! The Jupiter-8's control sweep: every slider, switch and selector of
//! both patches, the bender panel, the key mode, the split and the
//! balance, and every message the instrument takes, changed under
//! sounding notes, and the step each makes in the signal
//! (`control_sweep_support.rs` says how it is measured).

#![cfg(feature = "jupiter8")]
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
    PROGRAM, Sound, all_sound_off_leaves_the_pedal, all_sound_off_silences, check, continuous,
    messages_after_all_sound_off_are_taken, on_off, run, sweep, switch, to_crest,
};
use neoteletext_audio_instruments::jupiter8::{Jupiter8, KeyMode, Param, Patch, Performance, Portamento};

impl Instrument for Jupiter8 {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

const PARAMS: [Param; 43] = [
    Param::LfoRate,
    Param::LfoDelay,
    Param::LfoWave,
    Param::VcoLfoMod,
    Param::VcoEnvMod,
    Param::Vco1Mod,
    Param::Vco2Mod,
    Param::PwmDepth,
    Param::PwmSource,
    Param::CrossMod,
    Param::Vco1Range,
    Param::Vco1Wave,
    Param::Sync,
    Param::LowFrequency,
    Param::Vco2Range,
    Param::FineTune,
    Param::Vco2Wave,
    Param::MixVco1,
    Param::MixVco2,
    Param::HpfCutoff,
    Param::VcfCutoff,
    Param::VcfResonance,
    Param::VcfSlope,
    Param::VcfEnvMod,
    Param::VcfEnvSource,
    Param::VcfLfoMod,
    Param::VcfKeyFollow,
    Param::VcaLevel,
    Param::VcaLfoMod,
    Param::Env1Attack,
    Param::Env1Decay,
    Param::Env1Sustain,
    Param::Env1Release,
    Param::Env1Polarity,
    Param::Env2Attack,
    Param::Env2Decay,
    Param::Env2Sustain,
    Param::Env2Release,
    Param::AssignMode,
    Param::Env1KeyFollow,
    Param::Env2KeyFollow,
    Param::Vco1Bend,
    Param::Vco2Bend,
];
const KEY_MODES: [KeyMode; 3] = [KeyMode::Whole, KeyMode::Dual, KeyMode::Split];
const PORTAMENTOS: [Portamento; 3] = [Portamento::Off, Portamento::On, Portamento::UpperOnly];

/// The bender panel of every sound: the wheel and the bender reach the
/// cutoff as well as the pitch, so that both have something to move.
const PANEL: Performance = Performance {
    bend_vco: 200,
    bend_vcf: 1_200,
    lfo_vco: 50,
    lfo_vcf: 600,
    portamento: Portamento::Off,
    portamento_time: 40,
};

/// An instrument playing an upper and a lower patch in a key mode, its
/// modulation wheel at `wheel`.
fn instrument(key_mode: KeyMode, upper: &Patch, lower: &Patch, wheel: u8) -> Jupiter8 {
    let mut jupiter = Jupiter8::new();
    jupiter.set_key_mode(key_mode);
    jupiter.set_patch(upper);
    jupiter.set_lower_patch(lower);
    jupiter.set_performance(&PANEL);
    jupiter.event(CONTROL, 1, wheel);
    jupiter
}

/// Both oscillators' sawtooths through the open filter.
fn bright() -> Patch {
    Patch::DEFAULT.with(Param::MixVco2, 255).with(Param::FineTune, 140)
}

/// VCO-1's triangle alone through the open filter: the sound whose own
/// steps are smallest, under which the least step of a level shows.
fn pure() -> Patch {
    Patch::DEFAULT.with(Param::Vco1Wave, 0)
}

/// Every part of the instrument at work on a mellow sound, so that every
/// slider has something to change.
fn everything() -> Patch {
    let mut patch = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    for (param, value) in [
        (Param::LfoRate, 150),
        (Param::VcoLfoMod, 148),
        (Param::VcoEnvMod, 138),
        (Param::PwmDepth, 100),
        (Param::CrossMod, 20),
        (Param::MixVco1, 180),
        (Param::MixVco2, 150),
        (Param::Vco2Wave, 2),
        (Param::FineTune, 140),
        (Param::HpfCutoff, 40),
        (Param::VcfCutoff, 120),
        (Param::VcfResonance, 80),
        (Param::VcfEnvMod, 178),
        (Param::VcfLfoMod, 158),
        (Param::VcfKeyFollow, 188),
        (Param::VcaLevel, 180),
        (Param::VcaLfoMod, 158),
        (Param::Env1Attack, 30),
        (Param::Env1Decay, 120),
        (Param::Env1Sustain, 150),
        (Param::Env1Release, 120),
        (Param::Env2Attack, 20),
        (Param::Env2Decay, 120),
        (Param::Env2Sustain, 200),
        (Param::Env2Release, 120),
    ] {
        patch.set(param, value);
    }
    patch
}

/// The modulation wheel of the sound with everything at work.
const WHEEL: u8 = 40;
/// The keys of the chord: no two share a harmonic below the fifth.
const CHORD: [u8; 5] = [45, 52, 59, 66, 73];

fn sounds() -> Vec<Sound<Jupiter8>> {
    let held = |name, key_mode, upper: fn() -> Patch, lower: fn() -> Patch, keys: &'static [u8], wheel: u8| Sound {
        name,
        make: Box::new(move || instrument(key_mode, &upper(), &lower(), wheel)),
        play: Box::new(move |jupiter: &mut Jupiter8, clock: &mut Clock| {
            for &key in keys {
                jupiter.event(NOTE_ON, key, 100);
            }
            to_crest(jupiter, 4 * AFTER, clock)
        }),
    };
    vec![
        held("low bright", KeyMode::Whole, bright, bright, &[36], 0),
        held("middle", KeyMode::Whole, everything, everything, &[60], WHEEL),
        held("high", KeyMode::Whole, bright, bright, &[96], 0),
        held("pure", KeyMode::Whole, pure, pure, &[60], 0),
        held("chord", KeyMode::Whole, pure, pure, &CHORD, 0),
        held("dual", KeyMode::Dual, pure, everything, &[60], 0),
        held("split", KeyMode::Split, pure, pure, &[48, 67], 0),
        Sound {
            name: "release",
            make: Box::new(|| instrument(KeyMode::Whole, &everything(), &everything(), WHEEL)),
            play: Box::new(|jupiter: &mut Jupiter8, clock: &mut Clock| {
                jupiter.event(NOTE_ON, 60, 100);
                to_crest(jupiter, 4 * AFTER, clock);
                jupiter.event(NOTE_OFF, 60, 0);
                to_crest(jupiter, AFTER / 2, clock)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(|| instrument(KeyMode::Whole, &bright(), &bright(), 0)),
            play: Box::new(|jupiter: &mut Jupiter8, _clock: &mut Clock| {
                jupiter.event(NOTE_ON, 60, 100);
                run(jupiter, 2 * AFTER);
                jupiter.event(NOTE_OFF, 60, 0);
                run(jupiter, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is, from where `jupiter` has its controls.
fn changes(jupiter: &Jupiter8) -> Vec<Change<Jupiter8>> {
    let wheel = if *jupiter.patch() == everything() { i32::from(WHEEL) } else { 0 };
    let mut all = Vec::new();
    for (half, patch) in [("upper", *jupiter.patch()), ("lower", *jupiter.lower_patch())] {
        let lower = half == "lower";
        for param in PARAMS {
            let set = move |jupiter: &mut Jupiter8, value: i32| {
                if lower {
                    let patch = jupiter.lower_patch().with(param, value as u8);
                    jupiter.set_lower_patch(&patch);
                } else {
                    let patch = jupiter.patch().with(param, value as u8);
                    jupiter.set_patch(&patch);
                }
            };
            let (name, most, at) = (format!("{half} {param:?}"), i32::from(Patch::max(param)), patch.get(param).into());
            if most > 3 {
                all.extend(continuous(&name, (0, most), at, set));
            } else {
                all.extend(switch(&name, most + 1, at, |position| position.to_string(), set));
            }
        }
    }
    let panel = |set: fn(&mut Performance, i32)| {
        move |jupiter: &mut Jupiter8, value: i32| {
            let mut performance = *jupiter.performance();
            set(&mut performance, value);
            jupiter.set_performance(&performance);
        }
    };
    all.extend(continuous("bender to VCO", (0, 1_200), 200, panel(|panel, value| panel.bend_vco = value as u16)));
    all.extend(continuous("bender to VCF", (0, 4_800), 1_200, panel(|panel, value| panel.bend_vcf = value as u16)));
    all.extend(continuous("wheel to VCO", (0, 1_200), 50, panel(|panel, value| panel.lfo_vco = value as u16)));
    all.extend(continuous("wheel to VCF", (0, 4_800), 600, panel(|panel, value| panel.lfo_vcf = value as u16)));
    all.extend(continuous("portamento time", (0, 127), 40, panel(|panel, value| panel.portamento_time = value as u8)));
    all.extend(switch(
        "portamento",
        3,
        0,
        |position| format!("{:?}", PORTAMENTOS[position as usize]),
        panel(|panel, value| panel.portamento = PORTAMENTOS[value as usize]),
    ));
    all.extend(switch(
        "key mode",
        3,
        KEY_MODES.iter().zip(0i32..).find(|&(&mode, _)| mode == jupiter.key_mode()).map_or(0, |(_, position)| position),
        |position| format!("{:?}", KEY_MODES[position as usize]),
        |jupiter: &mut Jupiter8, value: i32| jupiter.set_key_mode(KEY_MODES[value as usize]),
    ));
    all.extend(continuous("split point", (0, 127), 60, |jupiter: &mut Jupiter8, value: i32| {
        jupiter.set_split(value as u8);
    }));
    all.extend(continuous("balance", (0, 127), 64, |jupiter: &mut Jupiter8, value: i32| {
        jupiter.set_balance(value as u8);
    }));

    // The messages. A controller's place before the change is given by
    // a message before the keys, as a slider's is by the patch.
    let controller = |number: u8| move |jupiter: &mut Jupiter8, value: i32| jupiter.event(CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("portamento time (5)", (0, 127), 40, controller(5)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(continuous("balance (8)", (0, 127), 64, controller(8)));
    for (name, number) in [("pedal (64)", 64), ("portamento (65)", 65)] {
        all.extend(switch(name, 2, 0, on_off, move |jupiter: &mut Jupiter8, value| {
            jupiter.event(CONTROL, number, if value == 0 { 0 } else { 127 });
        }));
    }
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |jupiter: &mut Jupiter8, value: i32| {
        jupiter.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |jupiter: &mut Jupiter8| jupiter.event(status, data1, data2))
    };
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(Change::starting("all sound off (120), then a key", "sent", |jupiter: &mut Jupiter8| {
        jupiter.event(CONTROL, 120, 0);
        jupiter.event(NOTE_ON, 64, 100);
    }));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike = |name: &str, key: u8| {
        Change::starting(name, "sent", move |jupiter: &mut Jupiter8| jupiter.event(NOTE_ON, key, 100))
    };
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    all.push(Change::starting("note on, nine more keys", "sent", |jupiter: &mut Jupiter8| {
        for key in [40, 47, 54, 61, 68, 75, 82, 89, 94] {
            jupiter.event(NOTE_ON, key, 100);
        }
    }));
    // What the instrument does not take: nothing may move.
    all.push(message("key pressure (ignored)", KEY_PRESSURE, 60, 127));
    all.push(message("channel pressure (ignored)", CHANNEL_PRESSURE, 127, 0));
    all.push(message("program change (ignored)", PROGRAM, 5, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(Change::new("release_all", "called", Jupiter8::release_all));
    all.push(Change::new("reset", "called", Jupiter8::reset));
    all
}

#[test]
fn no_control_steps_the_signal_but_the_switches_that_do() {
    let rows = sweep(&sounds(), &changes);
    check(
        "Jupiter-8",
        &rows,
        &[
            // The slope switch chooses which of the filter's two
            // outputs goes on to the amplifier (voice.rs, Voice::sample):
            // one path for the other at once.
            Allowed {
                controls: &["upper VcfSlope"],
                why: "a switch between the filter's two outputs, which changes the path at once",
            },
            // The sounds here have the release sliders at 0, where the
            // amplifier closes in a thousandth of a second, as the patch
            // asks. A change of the key mode or of an assign mode
            // releases the voices (Jupiter8::set_key_mode, set_patch),
            // and is the same release.
            Allowed {
                controls: &[
                    "note off",
                    "all notes off (123)",
                    "release_all",
                    "key mode",
                    "upper AssignMode",
                    "lower AssignMode",
                ],
                why: "the sound's own release, with its slider at 0: the envelope at its shortest",
            },
            // The slider is slid through every place between its ends,
            // and under a triangle the places between are sounds whose
            // own steps are larger than either end's: 124 units a
            // quarter of the way up against 89 at the bottom and 83 at
            // the top, as the same table's row of the quarter shows.
            Allowed {
                controls: &["upper HpfCutoff", "lower HpfCutoff"],
                why: "no step: the slider passes through sounds whose own steps are larger than either end's",
            },
            Allowed {
                controls: &["reset"],
                why: "no control: the caller's own cut, which puts the instrument back as new at once",
            },
        ],
    );
}

#[test]
fn all_sound_off_takes_the_sound_out_and_leaves_silence() {
    all_sound_off_silences(&sounds(), &|jupiter: &mut Jupiter8| jupiter.event(CONTROL, 120, 0), 256 + 64);
}

#[test]
fn all_sound_off_leaves_the_pedal_where_it_is() {
    all_sound_off_leaves_the_pedal(&|| instrument(KeyMode::Whole, &bright(), &bright(), 0), &Jupiter8::event);
}

#[test]
fn messages_that_come_during_all_sound_off_are_taken_after_it() {
    messages_after_all_sound_off_are_taken(&|| instrument(KeyMode::Whole, &bright(), &bright(), 0), &Jupiter8::event);
}
