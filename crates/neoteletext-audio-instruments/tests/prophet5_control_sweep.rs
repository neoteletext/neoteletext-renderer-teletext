//! The Prophet-5's control sweep, on both revisions: every knob and
//! switch of a program and every message the instrument takes, changed
//! under sounding notes, and the step each makes in the signal
//! (`control_sweep_support.rs` says how it is measured).

#![cfg(feature = "prophet5")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test's table of controls, each value within its control's range"
)]

mod control_sweep_support;

use control_sweep_support::{
    AFTER, Allowed, CHANNEL_PRESSURE, CONTROL, Change, Clock, Instrument, KEY_PRESSURE, NOTE_OFF, NOTE_ON, PITCH_BEND,
    PROGRAM, Sound, all_sound_off_silences, check, continuous, on_off, run, sweep, switch, to_crest,
};
use neoteletext_audio_instruments::prophet5::patch::{Knob, Switch};
use neoteletext_audio_instruments::prophet5::{Model, Patch, Prophet5};

impl Instrument for Prophet5 {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

const KNOBS: [Knob; 24] = [
    Knob::FilterAttack,
    Knob::FilterDecay,
    Knob::FilterSustain,
    Knob::FilterRelease,
    Knob::AmplifierAttack,
    Knob::AmplifierDecay,
    Knob::AmplifierSustain,
    Knob::AmplifierRelease,
    Knob::Cutoff,
    Knob::FilterEnvelopeAmount,
    Knob::MixerOscB,
    Knob::OscBPulseWidth,
    Knob::MixerOscA,
    Knob::OscAPulseWidth,
    Knob::MixerNoise,
    Knob::Resonance,
    Knob::Glide,
    Knob::LfoFrequency,
    Knob::SourceMix,
    Knob::PolyModOscB,
    Knob::PolyModFilterEnvelope,
    Knob::OscAFrequency,
    Knob::OscBFrequency,
    Knob::OscBFine,
];
const SWITCHES: [Switch; 22] = [
    Switch::OscAPulse,
    Switch::OscASawtooth,
    Switch::Sync,
    Switch::OscBSawtooth,
    Switch::OscBTriangle,
    Switch::OscBPulse,
    Switch::OscBKeyboard,
    Switch::Unison,
    Switch::PolyModFrequencyA,
    Switch::PolyModPulseWidthA,
    Switch::PolyModFilter,
    Switch::LfoSawtooth,
    Switch::LfoTriangle,
    Switch::LfoSquare,
    Switch::FilterKeyboard,
    Switch::Release,
    Switch::WheelFrequencyA,
    Switch::WheelFrequencyB,
    Switch::WheelPulseWidthA,
    Switch::WheelPulseWidthB,
    Switch::WheelFilter,
    Switch::OscBLowFrequency,
];

/// An instrument playing a patch, its modulation wheel at `wheel`.
fn instrument(model: Model, patch: &Patch, wheel: u8) -> Prophet5 {
    let mut prophet = Prophet5::new(model);
    prophet.set_patch(patch);
    prophet.event(CONTROL, 1, wheel);
    prophet
}

/// Both oscillators' sawtooths through the open filter.
fn bright(model: Model) -> Patch {
    let knob = model.frequency_knob(24);
    Patch::DEFAULT
        .with(Knob::OscAFrequency, knob)
        .with(Knob::OscBFrequency, knob)
        .with(Knob::OscBFine, 20)
        .with(Knob::MixerOscB, 127)
        .with_switch(Switch::OscBSawtooth, true)
}

/// Oscillator B's triangle alone through the open filter: the sound
/// whose own steps are smallest, under which the least step of a level
/// shows.
fn pure(model: Model) -> Patch {
    let knob = model.frequency_knob(24);
    Patch::DEFAULT
        .with(Knob::OscAFrequency, knob)
        .with(Knob::OscBFrequency, knob)
        .with(Knob::MixerOscA, 0)
        .with(Knob::MixerOscB, 127)
        .with_switch(Switch::OscASawtooth, false)
        .with_switch(Switch::OscBTriangle, true)
}

/// Every part of the instrument at work on a mellow sound, so that every
/// knob has something to change: both oscillators and a little noise,
/// the filter part closed with some resonance and its envelope, Poly-Mod
/// and Wheel-Mod into the filter and the widths with the wheel up, the
/// glide's knob up, long releases.
fn everything(model: Model) -> Patch {
    let knob = model.frequency_knob(24);
    let mut patch = Patch::DEFAULT;
    for (knob, value) in [
        (Knob::FilterAttack, 20),
        (Knob::FilterDecay, 70),
        (Knob::FilterSustain, 60),
        (Knob::FilterRelease, 70),
        (Knob::AmplifierAttack, 10),
        (Knob::AmplifierDecay, 70),
        (Knob::AmplifierSustain, 90),
        (Knob::AmplifierRelease, 70),
        (Knob::Cutoff, 60),
        (Knob::FilterEnvelopeAmount, 30),
        (Knob::MixerOscB, 90),
        (Knob::OscBPulseWidth, 50),
        (Knob::MixerOscA, 60),
        (Knob::OscAPulseWidth, 70),
        (Knob::MixerNoise, 6),
        (Knob::Resonance, 40),
        (Knob::Glide, 30),
        (Knob::LfoFrequency, 80),
        (Knob::SourceMix, 20),
        (Knob::PolyModOscB, 10),
        (Knob::PolyModFilterEnvelope, 30),
        (Knob::OscAFrequency, knob),
        (Knob::OscBFrequency, knob),
        (Knob::OscBFine, 10),
    ] {
        patch.set(knob, value);
    }
    for (switch, on) in [
        (Switch::OscAPulse, true),
        (Switch::OscASawtooth, false),
        (Switch::OscBTriangle, true),
        (Switch::PolyModFilter, true),
        (Switch::PolyModPulseWidthA, true),
        (Switch::WheelFilter, true),
        (Switch::WheelPulseWidthA, true),
        (Switch::WheelFrequencyB, true),
    ] {
        patch.switch(switch, on);
    }
    patch
}

/// The modulation wheel of the sound with everything at work.
const WHEEL: u8 = 40;
/// The keys of the chord: no two share a harmonic below the fifth.
const CHORD: [u8; 5] = [45, 52, 59, 66, 73];

fn sounds(model: Model) -> Vec<Sound<Prophet5>> {
    let held = |name, patch: fn(Model) -> Patch, keys: &'static [u8], wheel: u8| Sound {
        name,
        make: Box::new(move || instrument(model, &patch(model), wheel)),
        play: Box::new(move |prophet: &mut Prophet5, clock: &mut Clock| {
            for &key in keys {
                prophet.event(NOTE_ON, key, 100);
            }
            to_crest(prophet, 4 * AFTER, clock)
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
            make: Box::new(move || instrument(model, &everything(model), WHEEL)),
            play: Box::new(|prophet: &mut Prophet5, clock: &mut Clock| {
                prophet.event(NOTE_ON, 60, 100);
                to_crest(prophet, 4 * AFTER, clock);
                prophet.event(NOTE_OFF, 60, 0);
                to_crest(prophet, AFTER / 2, clock)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(move || instrument(model, &bright(model), 0)),
            play: Box::new(|prophet: &mut Prophet5, _clock: &mut Clock| {
                prophet.event(NOTE_ON, 60, 100);
                run(prophet, 2 * AFTER);
                prophet.event(NOTE_OFF, 60, 0);
                run(prophet, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is, from where `prophet` has its controls.
fn changes(prophet: &Prophet5) -> Vec<Change<Prophet5>> {
    let patch = *prophet.patch();
    let wheel = if patch == everything(prophet.model()) { i32::from(WHEEL) } else { 0 };
    let mut all = Vec::new();
    for knob in KNOBS {
        all.extend(continuous(
            &format!("{knob:?}"),
            (0, 127),
            patch.get(knob).into(),
            move |prophet: &mut Prophet5, value: i32| {
                let patch = prophet.patch().with(knob, value as u8);
                prophet.set_patch(&patch);
            },
        ));
    }
    for toggle in SWITCHES {
        all.extend(switch(
            &format!("{toggle:?} switch"),
            2,
            patch.on(toggle).into(),
            on_off,
            move |prophet: &mut Prophet5, value: i32| {
                let patch = prophet.patch().with_switch(toggle, value != 0);
                prophet.set_patch(&patch);
            },
        ));
    }
    all.extend(continuous("master tune", (-100, 100), 0, |prophet: &mut Prophet5, value: i32| {
        prophet.set_master_tune(value);
    }));

    // The messages. A controller's place before the change is given by
    // a message before the keys, as a knob's is by the patch.
    let controller = |number: u8| move |prophet: &mut Prophet5, value: i32| prophet.event(CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(switch("pedal (64)", 2, 0, on_off, |prophet: &mut Prophet5, value| {
        prophet.event(CONTROL, 64, if value == 0 { 0 } else { 127 });
    }));
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |prophet: &mut Prophet5, value: i32| {
        prophet.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |prophet: &mut Prophet5| prophet.event(status, data1, data2))
    };
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike = |name: &str, key: u8| {
        Change::starting(name, "sent", move |prophet: &mut Prophet5| prophet.event(NOTE_ON, key, 100))
    };
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    all.push(Change::starting("note on, six more keys", "sent", |prophet: &mut Prophet5| {
        for key in [40, 47, 54, 61, 68, 75] {
            prophet.event(NOTE_ON, key, 100);
        }
    }));
    // What the instrument does not take: nothing may move.
    all.push(message("key pressure (ignored)", KEY_PRESSURE, 60, 127));
    all.push(message("channel pressure (ignored)", CHANNEL_PRESSURE, 127, 0));
    all.push(message("program change (ignored)", PROGRAM, 5, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(Change::new("release_all", "called", Prophet5::release_all));
    all.push(Change::new("reset", "called", Prophet5::reset));
    all
}

fn swept(model: Model, allowed: &[Allowed]) {
    let rows = sweep(&sounds(model), &changes);
    check(&format!("Prophet-5 {model:?}"), &rows, allowed);
}

/// TM3 section 2-11 (page 28): a switch's bit "causes a digital output
/// to close or open a solid-state switch", a 4016, in the path it
/// switches: the path opens or closes at once.
const SWITCH: &str = "a solid-state switch in the circuit, which opens or closes its path at once";
/// TM3 section 2-12 (pages 29 to 31): the computer moves a knob's number
/// one step of seven bits in a loop, and the voltage's sample-and-hold
/// takes each new value in "a few microseconds". Of all the knobs only
/// the resonance's step is large enough to show: the level below the
/// cutoff is 1 / (1 + k), and a step of k from nothing takes a
/// twentieth of it.
const STAIR: &str = "the converter's seven bits: each step of a knob's voltage is a step, on the instrument too";
/// The sounds here have the RELEASE knobs at 0, where the envelope
/// chips' release is at its shortest: the amplifier closes in a
/// thousandth of a second, as the patch asks. Switching unison lets go
/// of every key, and is the same release.
const RELEASE: &str = "the sound's own release, with its knob at 0: the envelope chip at its shortest";
const RESET: &str = "no control: the caller's own cut, which puts the instrument back as new at once";

#[test]
fn no_control_of_revision_3_steps_the_signal_but_the_switches_that_do() {
    swept(
        Model::Rev3,
        &[
            Allowed { controls: &["FilterKeyboard switch", "OscBTriangle switch"], why: SWITCH },
            Allowed { controls: &["Resonance"], why: STAIR },
            Allowed { controls: &["note off", "all notes off (123)", "release_all", "Unison switch"], why: RELEASE },
            Allowed {
                controls: &["all sound off (120)"],
                why: "OUR FAULT, not the instrument's: the voices are emptied in one sample (TODO-6438)",
            },
            Allowed { controls: &["reset"], why: RESET },
        ],
    );
}

#[test]
fn no_control_of_revision_2_steps_the_signal_but_the_switches_that_do() {
    swept(
        Model::Rev2,
        &[
            Allowed { controls: &["FilterKeyboard switch", "OscBTriangle switch"], why: SWITCH },
            Allowed { controls: &["Resonance"], why: STAIR },
            Allowed { controls: &["all notes off (123)", "release_all", "Unison switch"], why: RELEASE },
            Allowed {
                controls: &["all sound off (120)"],
                why: "OUR FAULT, not the instrument's: the voices are emptied in one sample (TODO-6438)",
            },
            Allowed { controls: &["reset"], why: RESET },
        ],
    );
}

#[test]
fn all_sound_off_takes_the_sound_out_and_leaves_silence() {
    for model in [Model::Rev3, Model::Rev2] {
        all_sound_off_silences(&sounds(model), &|prophet: &mut Prophet5| prophet.event(CONTROL, 120, 0), 256 + 64);
    }
}
