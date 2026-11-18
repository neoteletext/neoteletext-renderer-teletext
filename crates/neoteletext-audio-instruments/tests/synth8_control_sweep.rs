//! synth8's control sweep: every slider, switch and selector of
//! both patches, the bender panel, the key mode, the split and the
//! balance, and every message the instrument takes, changed under
//! sounding notes, and the step each makes in the signal
//! (`control_sweep_support.rs` says how it is measured).

#![cfg(feature = "synth8")]
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
use neoteletext_audio_instruments::synth8::patch::canonical;
use neoteletext_audio_instruments::synth8::{
    KeyMode, Kind, Param, Patch, Performance, Portamento, Synth8, Vco1Wave, Vco2Wave,
};

impl Instrument for Synth8 {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

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
fn instrument(key_mode: KeyMode, upper: &Patch, lower: &Patch, wheel: u8) -> Synth8 {
    let mut synth8 = Synth8::new();
    synth8.set_key_mode(key_mode);
    synth8.set_patch(upper);
    synth8.set_lower_patch(lower);
    synth8.set_performance(&PANEL);
    let _ = synth8.event(CONTROL, 1, wheel);
    synth8
}

/// Sends a message, for the support's tables, which want no answer.
fn event(synth8: &mut Synth8, status: u8, data1: u8, data2: u8) {
    let _ = synth8.event(status, data1, data2);
}

/// Both oscillators' sawtooths through the open filter.
fn bright() -> Patch {
    // FINE TUNE's value 70 is the signed byte's +12 (S8-21 item 3).
    Patch::DEFAULT.with(Param::MixVco2, 127).with(Param::FineTune, 70)
}

/// VCO-1's triangle alone through the open filter: the sound whose own
/// steps are smallest, under which the least step of a level shows.
fn pure() -> Patch {
    Patch::DEFAULT.with(Param::Vco1Wave, Vco1Wave::Triangle.value())
}

/// Every part of the instrument at work on a mellow sound, so that every
/// slider has something to change.
fn everything() -> Patch {
    let mut patch = Patch::DEFAULT.with(Param::Vco1Wave, Vco1Wave::Triangle.value());
    for (param, value) in [
        (Param::LfoRate, 75),
        (Param::VcoLfoMod, 20),
        (Param::VcoEnvMod, 10),
        (Param::PwmDepth, 50),
        (Param::CrossMod, 10),
        (Param::MixVco1, 90),
        (Param::MixVco2, 75),
        (Param::Vco2Wave, Vco2Wave::Pulse.value()),
        (Param::FineTune, 70),
        (Param::HpfCutoff, 20),
        (Param::VcfCutoff, 60),
        (Param::VcfResonance, 40),
        (Param::VcfEnvMod, 50),
        (Param::VcfLfoMod, 30),
        (Param::VcfKeyFollow, 60),
        (Param::VcaLevel, 90),
        // VCA LFO MOD's position 1 (S8-21 item 4).
        (Param::VcaLfoMod, 32),
        (Param::Env1Attack, 15),
        (Param::Env1Decay, 60),
        (Param::Env1Sustain, 75),
        (Param::Env1Release, 60),
        (Param::Env2Attack, 10),
        (Param::Env2Decay, 60),
        (Param::Env2Sustain, 100),
        (Param::Env2Release, 60),
    ] {
        patch.set(param, value);
    }
    patch
}

/// The modulation wheel of the sound with everything at work.
const WHEEL: u8 = 40;
/// The keys of the chord: no two share a harmonic below the fifth.
const CHORD: [u8; 5] = [45, 52, 59, 66, 73];

fn sounds() -> Vec<Sound<Synth8>> {
    let held = |name, key_mode, upper: fn() -> Patch, lower: fn() -> Patch, keys: &'static [u8], wheel: u8| Sound {
        name,
        make: Box::new(move || instrument(key_mode, &upper(), &lower(), wheel)),
        play: Box::new(move |synth8: &mut Synth8, clock: &mut Clock| {
            for &key in keys {
                event(synth8, NOTE_ON, key, 100);
            }
            to_crest(synth8, 4 * AFTER, clock)
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
            play: Box::new(|synth8: &mut Synth8, clock: &mut Clock| {
                event(synth8, NOTE_ON, 60, 100);
                to_crest(synth8, 4 * AFTER, clock);
                event(synth8, NOTE_OFF, 60, 0);
                to_crest(synth8, AFTER / 2, clock)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(|| instrument(KeyMode::Whole, &bright(), &bright(), 0)),
            play: Box::new(|synth8: &mut Synth8, _clock: &mut Clock| {
                event(synth8, NOTE_ON, 60, 100);
                run(synth8, 2 * AFTER);
                event(synth8, NOTE_OFF, 60, 0);
                run(synth8, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is, from where `synth8` has its controls.
fn changes(synth8: &Synth8) -> Vec<Change<Synth8>> {
    let wheel = if *synth8.patch() == everything() { i32::from(WHEEL) } else { 0 };
    let mut all = Vec::new();
    for (half, patch) in [("upper", *synth8.patch()), ("lower", *synth8.lower_patch())] {
        let lower = half == "lower";
        for param in Param::ALL {
            // Each control moved by its controller on its half's channel
            // (S8-2 item 4, S8-16).
            let channel = u8::from(lower);
            let name = format!("{half} {param:?}");
            match param.kind() {
                Kind::Slider | Kind::Modulation | Kind::Centred => {
                    let set = move |synth8: &mut Synth8, value: i32| {
                        event(synth8, CONTROL | channel, param.controller(), value as u8);
                    };
                    all.extend(continuous(&name, (0, 127), patch.get(param).into(), set));
                }
                Kind::Selector(positions) => {
                    let set = move |synth8: &mut Synth8, position: i32| {
                        event(synth8, CONTROL | channel, param.controller(), canonical(position as u8, positions));
                    };
                    let at = patch.position(param).into();
                    all.extend(switch(&name, positions.into(), at, |position| position.to_string(), set));
                }
                Kind::Switch => {
                    let set = move |synth8: &mut Synth8, position: i32| {
                        event(synth8, CONTROL | channel, param.controller(), if position == 0 { 0 } else { 127 });
                    };
                    all.extend(switch(&name, 2, patch.on(param).into(), |position| position.to_string(), set));
                }
            }
        }
    }
    let panel = |set: fn(&mut Performance, i32)| {
        move |synth8: &mut Synth8, value: i32| {
            let mut performance = *synth8.performance();
            set(&mut performance, value);
            synth8.set_performance(&performance);
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
        KEY_MODES.iter().zip(0i32..).find(|&(&mode, _)| mode == synth8.key_mode()).map_or(0, |(_, position)| position),
        |position| format!("{:?}", KEY_MODES[position as usize]),
        |synth8: &mut Synth8, value: i32| synth8.set_key_mode(KEY_MODES[value as usize]),
    ));
    all.extend(continuous("split point", (0, 127), 60, |synth8: &mut Synth8, value: i32| {
        synth8.set_split(value as u8);
    }));
    all.extend(continuous("balance", (0, 127), 64, |synth8: &mut Synth8, value: i32| {
        synth8.set_balance(value as u8);
    }));

    // The messages. A controller's place before the change is given by
    // a message before the keys, as a slider's is by the patch.
    let controller = |number: u8| move |synth8: &mut Synth8, value: i32| event(synth8, CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("portamento time (5)", (0, 127), 40, controller(5)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(continuous("balance (8)", (0, 127), 64, controller(8)));
    all.extend(continuous("expression (11)", (0, 127), 127, controller(11)));
    all.extend(continuous("VCO bend depth (41)", (0, 127), 21, controller(41)));
    for (name, number) in [("pedal (64)", 64), ("portamento (118)", 118)] {
        all.extend(switch(name, 2, 0, on_off, move |synth8: &mut Synth8, value| {
            event(synth8, CONTROL, number, if value == 0 { 0 } else { 127 });
        }));
    }
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |synth8: &mut Synth8, value: i32| {
        event(synth8, PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |synth8: &mut Synth8| event(synth8, status, data1, data2))
    };
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(Change::starting("all sound off (120), then a key", "sent", |synth8: &mut Synth8| {
        event(synth8, CONTROL, 120, 0);
        event(synth8, NOTE_ON, 64, 100);
    }));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike = |name: &str, key: u8| {
        Change::starting(name, "sent", move |synth8: &mut Synth8| event(synth8, NOTE_ON, key, 100))
    };
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    all.push(Change::starting("note on, nine more keys", "sent", |synth8: &mut Synth8| {
        for key in [40, 47, 54, 61, 68, 75, 82, 89, 94] {
            event(synth8, NOTE_ON, key, 100);
        }
    }));
    // What the instrument does not take: nothing may move.
    all.push(message("key pressure (ignored)", KEY_PRESSURE, 60, 127));
    all.push(message("channel pressure (ignored)", CHANNEL_PRESSURE, 127, 0));
    all.push(message("program change (ignored)", PROGRAM, 5, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(message("portamento switch 65 (ignored)", CONTROL, 65, 127));
    all.push(message("cutoff on channel 3 (ignored)", CONTROL | 2, 3, 0));
    all.push(Change::new("release_all", "called", Synth8::release_all));
    all.push(Change::new("reset", "called", Synth8::reset));
    all
}

#[test]
fn no_control_steps_the_signal_but_the_switches_that_do() {
    let rows = sweep(&sounds(), &changes);
    check(
        "synth8",
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
            // releases the voices (Synth8::set_key_mode, set_patch),
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
    all_sound_off_silences(&sounds(), &|synth8: &mut Synth8| event(synth8, CONTROL, 120, 0), 256 + 64);
}

#[test]
fn all_sound_off_leaves_the_pedal_where_it_is() {
    all_sound_off_leaves_the_pedal(&|| instrument(KeyMode::Whole, &bright(), &bright(), 0), &event);
}

#[test]
fn messages_that_come_during_all_sound_off_are_taken_after_it() {
    messages_after_all_sound_off_are_taken(&|| instrument(KeyMode::Whole, &bright(), &bright(), 0), &event);
}
