//! The Minimoog's control sweep: every knob and switch of the panel and
//! every message the instrument takes, changed under a sounding note,
//! and the step each makes in the signal (`control_sweep_support.rs`
//! says how it is measured).

#![cfg(feature = "minimoog")]
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
    AFTER, Allowed, CHANNEL_PRESSURE, CONTROL, Change, Instrument, KEY_PRESSURE, NOTE_OFF, NOTE_ON, PITCH_BEND,
    PROGRAM, Sound, all_sound_off_silences, check, continuous, on_off, run, sweep, switch, to_crest,
};
use neoteletext_audio_instruments::minimoog::{ContourPatch, Minimoog, OscillatorPatch, Patch, Range, Waveform};

impl Instrument for Minimoog {
    fn play(&mut self, out: &mut [i16]) {
        self.render(out);
    }
}

/// The ranges and the waveforms by their positions.
const RANGES: [Range; 6] = [Range::Lo, Range::ThirtyTwo, Range::Sixteen, Range::Eight, Range::Four, Range::Two];
const WAVEFORMS: [Waveform; 6] =
    [Waveform::Triangle, Waveform::Shark, Waveform::Sawtooth, Waveform::Square, Waveform::Wide, Waveform::Narrow];

/// Something that sets one field of the patch of an instrument.
fn patched(
    set: impl Fn(&mut Patch, i32) + Send + Sync + Clone + 'static,
) -> impl Fn(&mut Minimoog, i32) + Send + Sync + Clone {
    move |minimoog: &mut Minimoog, value: i32| {
        let mut patch = *minimoog.patch();
        set(&mut patch, value);
        minimoog.set_patch(&patch);
    }
}

/// An instrument playing a patch, its oscillators not drifting, its
/// modulation wheel at `wheel`.
fn instrument(patch: &Patch, wheel: u8) -> Minimoog {
    let mut minimoog = Minimoog::new();
    minimoog.set_drift(0);
    minimoog.set_patch(patch);
    minimoog.event(CONTROL, 1, wheel);
    minimoog
}

/// Two sawtooths through the open filter.
fn bright() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.oscillators[1].on = true;
    patch.oscillators[1].frequency = 7;
    patch
}

/// One triangle through the open filter: the sound whose own steps are
/// smallest, under which the least step of a level shows.
fn pure() -> Patch {
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].waveform = Waveform::Triangle;
    patch
}

/// Every part of the instrument at work on a mellow sound, so that every
/// control has something to change: three triangles, a little noise and
/// a little of the output returned, the filter part closed with some
/// EMPHASIS and its contour, both modulations on with the wheel up, the
/// keyboard's control of the filter, the glide and the decay.
fn everything() -> Patch {
    let oscillator =
        OscillatorPatch { range: Range::Eight, waveform: Waveform::Triangle, frequency: 0, volume: 70, on: true };
    Patch {
        tune: 0,
        glide: 30,
        modulation_mix: 30,
        oscillators: [
            oscillator,
            OscillatorPatch { frequency: 5, ..oscillator },
            OscillatorPatch { range: Range::Lo, frequency: 300, volume: 30, ..oscillator },
        ],
        external_volume: 20,
        external_on: true,
        noise_volume: 5,
        noise_on: true,
        noise_pink: false,
        cutoff: 0,
        emphasis: 30,
        contour_amount: 30,
        filter_contour: ContourPatch { attack: 20, decay: 600, sustain: 50 },
        loudness_contour: ContourPatch { attack: 5, decay: 600, sustain: 70 },
        volume: 70,
        oscillator_modulation: true,
        oscillator_3_control: true,
        filter_modulation: true,
        keyboard_control: [true, false],
        glide_on: true,
        decay_on: true,
    }
}

/// The modulation wheel of the sound with everything at work.
const WHEEL: u8 = 40;

fn sounds() -> Vec<Sound<Minimoog>> {
    let held = |name, patch: fn() -> Patch, key: u8, wheel: u8| Sound {
        name,
        make: Box::new(move || instrument(&patch(), wheel)),
        play: Box::new(move |minimoog: &mut Minimoog| {
            minimoog.event(NOTE_ON, key, 100);
            to_crest(minimoog, 4 * AFTER)
        }),
    };
    vec![
        held("low bright", bright, 33, 0),
        held("middle", everything, 60, WHEEL),
        held("high", bright, 96, 0),
        held("pure", pure, 60, 0),
        Sound {
            name: "release",
            make: Box::new(|| instrument(&everything(), WHEEL)),
            play: Box::new(|minimoog: &mut Minimoog| {
                minimoog.event(NOTE_ON, 60, 100);
                to_crest(minimoog, 4 * AFTER);
                minimoog.event(NOTE_OFF, 60, 0);
                to_crest(minimoog, AFTER / 2)
            }),
        },
        Sound {
            name: "silence",
            make: Box::new(|| instrument(&bright(), 0)),
            play: Box::new(|minimoog: &mut Minimoog| {
                minimoog.event(NOTE_ON, 60, 100);
                run(minimoog, 2 * AFTER);
                minimoog.event(NOTE_OFF, 60, 0);
                run(minimoog, 6 * AFTER).last().copied().unwrap_or(0)
            }),
        },
    ]
}

/// Every change there is, from where `minimoog` has its controls.
fn changes(minimoog: &Minimoog) -> Vec<Change<Minimoog>> {
    let patch = *minimoog.patch();
    // The sound with every part at work is the one with the wheel up.
    let wheel = if patch == everything() { i32::from(WHEEL) } else { 0 };
    let mut all = Vec::new();
    let mut knob = |name: &str, range: (i32, i32), at: i32, set: fn(&mut Patch, i32)| {
        all.extend(continuous(name, range, at, patched(set)));
    };
    knob("TUNE", (-242, 242), patch.tune.into(), |patch, value| patch.tune = value as i16);
    knob("GLIDE", (0, 100), patch.glide.into(), |patch, value| patch.glide = value as u8);
    knob("MODULATION MIX", (0, 100), patch.modulation_mix.into(), |patch, value| patch.modulation_mix = value as u8);
    knob("OSC 2 FREQUENCY", (-700, 700), patch.oscillators[1].frequency.into(), |patch, value| {
        patch.oscillators[1].frequency = value as i16;
    });
    knob("OSC 3 FREQUENCY", (-700, 700), patch.oscillators[2].frequency.into(), |patch, value| {
        patch.oscillators[2].frequency = value as i16;
    });
    knob("MIXER VOLUME 1", (0, 100), patch.oscillators[0].volume.into(), |patch, value| {
        patch.oscillators[0].volume = value as u8;
    });
    knob("MIXER VOLUME 2", (0, 100), patch.oscillators[1].volume.into(), |patch, value| {
        patch.oscillators[1].volume = value as u8;
    });
    knob("MIXER VOLUME 3", (0, 100), patch.oscillators[2].volume.into(), |patch, value| {
        patch.oscillators[2].volume = value as u8;
    });
    knob("EXTERNAL INPUT VOLUME", (0, 100), patch.external_volume.into(), |patch, value| {
        patch.external_volume = value as u8;
    });
    knob("NOISE VOLUME", (0, 100), patch.noise_volume.into(), |patch, value| patch.noise_volume = value as u8);
    knob("CUTOFF FREQUENCY", (-500, 500), patch.cutoff.into(), |patch, value| patch.cutoff = value as i16);
    knob("EMPHASIS", (0, 100), patch.emphasis.into(), |patch, value| patch.emphasis = value as u8);
    knob("AMOUNT OF CONTOUR", (0, 100), patch.contour_amount.into(), |patch, value| patch.contour_amount = value as u8);
    knob("FILTER ATTACK", (0, 10_000), patch.filter_contour.attack.into(), |patch, value| {
        patch.filter_contour.attack = value as u16;
    });
    knob("FILTER DECAY", (0, 10_000), patch.filter_contour.decay.into(), |patch, value| {
        patch.filter_contour.decay = value as u16;
    });
    knob("FILTER SUSTAIN", (0, 100), patch.filter_contour.sustain.into(), |patch, value| {
        patch.filter_contour.sustain = value as u8;
    });
    knob("LOUDNESS ATTACK", (0, 10_000), patch.loudness_contour.attack.into(), |patch, value| {
        patch.loudness_contour.attack = value as u16;
    });
    knob("LOUDNESS DECAY", (0, 10_000), patch.loudness_contour.decay.into(), |patch, value| {
        patch.loudness_contour.decay = value as u16;
    });
    knob("LOUDNESS SUSTAIN", (0, 100), patch.loudness_contour.sustain.into(), |patch, value| {
        patch.loudness_contour.sustain = value as u8;
    });
    knob("VOLUME", (0, 100), patch.volume.into(), |patch, value| patch.volume = value as u8);

    let range_name = |position: i32| format!("{:?}", RANGES[position as usize]);
    let wave_name = |position: i32| format!("{:?}", WAVEFORMS[position as usize]);
    for (number, oscillator) in patch.oscillators.iter().enumerate() {
        all.extend(switch(
            &format!("OSC {} RANGE", number + 1),
            6,
            oscillator.range as i32,
            range_name,
            patched(move |patch, value| patch.oscillators[number].range = RANGES[value as usize]),
        ));
        all.extend(switch(
            &format!("OSC {} WAVEFORM", number + 1),
            6,
            oscillator.waveform as i32,
            wave_name,
            patched(move |patch, value| patch.oscillators[number].waveform = WAVEFORMS[value as usize]),
        ));
        all.extend(switch(
            &format!("MIXER SWITCH {}", number + 1),
            2,
            oscillator.on.into(),
            on_off,
            patched(move |patch, value| patch.oscillators[number].on = value != 0),
        ));
    }
    let mut toggle = |name: &str, at: bool, set: fn(&mut Patch, bool)| {
        all.extend(switch(name, 2, at.into(), on_off, patched(move |patch, value| set(patch, value != 0))));
    };
    toggle("MIXER SWITCH EXTERNAL", patch.external_on, |patch, on| patch.external_on = on);
    toggle("MIXER SWITCH NOISE", patch.noise_on, |patch, on| patch.noise_on = on);
    toggle("NOISE PINK", patch.noise_pink, |patch, on| patch.noise_pink = on);
    toggle("OSCILLATOR MODULATION", patch.oscillator_modulation, |patch, on| patch.oscillator_modulation = on);
    toggle("OSC 3 CONTROL", patch.oscillator_3_control, |patch, on| patch.oscillator_3_control = on);
    toggle("FILTER MODULATION", patch.filter_modulation, |patch, on| patch.filter_modulation = on);
    toggle("KEYBOARD CONTROL 1", patch.keyboard_control[0], |patch, on| patch.keyboard_control[0] = on);
    toggle("KEYBOARD CONTROL 2", patch.keyboard_control[1], |patch, on| patch.keyboard_control[1] = on);
    toggle("GLIDE SWITCH", patch.glide_on, |patch, on| patch.glide_on = on);
    toggle("DECAY SWITCH", patch.decay_on, |patch, on| patch.decay_on = on);

    // The messages. A controller's place before the change is given by
    // a message before the keys, as a knob's is by the patch.
    let controller =
        |number: u8| move |minimoog: &mut Minimoog, value: i32| minimoog.event(CONTROL, number, value as u8);
    all.extend(continuous("modulation wheel (1)", (0, 127), wheel, controller(1)));
    all.extend(continuous("volume (7)", (0, 127), 127, controller(7)));
    all.extend(switch("pedal (64)", 2, 0, on_off, |minimoog: &mut Minimoog, value| {
        minimoog.event(CONTROL, 64, if value == 0 { 0 } else { 127 });
    }));
    all.extend(continuous("pitch bend", (0, 0x3fff), 0x2000, |minimoog: &mut Minimoog, value: i32| {
        minimoog.event(PITCH_BEND, (value & 0x7f) as u8, (value >> 7) as u8);
    }));
    let message = |name: &str, status: u8, data1: u8, data2: u8| {
        Change::new(name, "sent", move |minimoog: &mut Minimoog| minimoog.event(status, data1, data2))
    };
    all.push(message("all sound off (120)", CONTROL, 120, 0));
    all.push(message("reset all controllers (121)", CONTROL, 121, 0));
    all.push(message("all notes off (123)", CONTROL, 123, 0));
    all.push(message("note off", NOTE_OFF, 60, 0));
    let strike = |name: &str, key: u8| {
        Change::starting(name, "sent", move |minimoog: &mut Minimoog| minimoog.event(NOTE_ON, key, 100))
    };
    all.push(strike("note on, the key again", 60));
    all.push(strike("note on, a key below", 31));
    all.push(strike("note on, a key above", 100));
    // What the instrument does not take: nothing may move.
    all.push(message("key pressure (ignored)", KEY_PRESSURE, 60, 127));
    all.push(message("channel pressure (ignored)", CHANNEL_PRESSURE, 127, 0));
    all.push(message("program change (ignored)", PROGRAM, 5, 0));
    all.push(message("controller 74 (ignored)", CONTROL, 74, 127));
    all.push(Change::new("release_all", "called", Minimoog::release_all));
    all.push(Change::new("reset", "called", Minimoog::reset));
    all.extend(continuous("drift", (0, 50), 0, |minimoog: &mut Minimoog, value: i32| minimoog.set_drift(value as u8)));
    all
}

/// The switches of the panel are switches in the circuit: each opens or
/// closes a path at once, and the signal steps by what the path
/// carried. The instrument's own do the same.
const SWITCHES: &str = "a switch of the panel, which opens or closes its path in the circuit at once";

#[test]
fn no_control_steps_the_signal_but_the_switches_that_do() {
    let rows = sweep(&sounds(), &changes);
    check(
        "Minimoog",
        &rows,
        &[
            // Service manual, figure 9-2: each oscillator's wave goes
            // through its mixer switch and volume to the mixer's node,
            // with nothing after the switch that holds a charge but the
            // 10 µF that couples the node, which passes a step.
            Allowed { controls: &["MIXER SWITCH 1", "MIXER SWITCH 2", "MIXER SWITCH 3"], why: SWITCHES },
            // Service manual, figure 9-17: the waveform switch chooses
            // among the waveshapers' outputs. Oscillator 3's is here the
            // source of the modulation, so the cutoff and the pitches
            // jump with it.
            Allowed { controls: &["OSC 3 WAVEFORM"], why: SWITCHES },
            Allowed {
                controls: &["reset"],
                why: "no control: the caller's own cut, which puts the instrument back as new at once",
            },
        ],
    );
}

#[test]
fn all_sound_off_takes_the_sound_out_and_leaves_silence() {
    // The 256 samples the sound takes to go, and the decimators' tail.
    all_sound_off_silences(&sounds(), &|minimoog: &mut Minimoog| minimoog.event(CONTROL, 120, 0), 256 + 64);
}
