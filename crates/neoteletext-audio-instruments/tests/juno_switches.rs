//! The Juno while its panel moves: a switch or a whole patch changed
//! under a note that sounds or dies away, and what a voice keeps from
//! one note to the next (TODO-6438).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

#[path = "juno_support.rs"]
mod support;

use neoteletext_audio_instruments::SAMPLE_RATE;
use neoteletext_audio_instruments::juno::{ChorusMode, Model, Patch, Slider, VOICES};
use support::{MODELS, NOTE_OFF, NOTE_ON, flat, instrument, play, render, rms, whistle};

#[test]
fn a_voice_on_the_gate_starts_its_next_note_with_its_envelope_at_rest() {
    for model in MODELS {
        // The amplifier on the gate, the envelope opening a nearly closed
        // filter over a second, and a release of a fifth of a second: a
        // note is silent a few thousandths of a second after its key, and
        // its envelope at rest long before its voice's turn comes again.
        let mut patch = flat(model, Patch::DEFAULT)
            .with(Slider::VcfFrequency, 30)
            .with(Slider::VcfEnvelope, 100)
            .with(Slider::Attack, 90)
            .with(Slider::Release, 32);
        patch.vca_gate = true;
        let mut juno = instrument(model, &patch);
        let mut heads = Vec::new();
        for _ in 0..=2 * VOICES {
            juno.event(NOTE_ON, 48, 100);
            heads.push(rms(&render(&mut juno, 0.05)));
            render(&mut juno, 1.0);
            juno.event(NOTE_OFF, 48, 0);
            render(&mut juno, 1.0);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: the envelope has come to rest");
        }
        println!("{model:?}: the first 50 ms of each note: {heads:.4?}");
        // Each voice's second note begins as dull as its first, where one
        // that kept its envelope begins some thirty times louder. The
        // two are not alike to the sample: a voice's first note of all
        // starts its oscillator and its filter from nothing.
        for (index, head) in heads.iter().enumerate() {
            let ratio = head / heads[0];
            assert!((0.5..2.0).contains(&ratio), "{model:?}: note {index} begins at {head}, the first at {}", heads[0]);
        }
    }
}

#[test]
fn the_chorus_switched_under_a_note_goes_and_comes_without_a_click() {
    let modes = [ChorusMode::Off, ChorusMode::One, ChorusMode::Two, ChorusMode::Both];
    // The largest step from one sample to the next.
    let step = |signal: &[f64]| signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0f64, f64::max);
    for model in MODELS {
        // The filter's own oscillation near middle C, a sine: its
        // largest step is its slope, and anything sudden is larger.
        let cutoff = match model {
            Model::Juno106 => 62,
            Model::Juno60 => 44,
        };
        let sine = flat(model, whistle(cutoff));
        for from in modes {
            for to in modes.into_iter().filter(|&to| to != from) {
                let (mut before, mut after) = (sine, sine);
                before.chorus = from;
                after.chorus = to;
                let (mut worst, mut steady) = (0.0f64, 0.0f64);
                // At sixteen instants of the sine's cycle and the sweep's.
                for instant in 0..16 {
                    let mut juno = instrument(model, &before);
                    let mut dry = instrument(model, &sine);
                    for juno in [&mut juno, &mut dry] {
                        juno.event(NOTE_ON, 60, 100);
                    }
                    let lead = 0.4 + 0.0371 * f64::from(instant);
                    render(&mut juno, lead);
                    let mut around = render(&mut juno, 0.05);
                    steady = steady.max(step(&around));
                    // From the last sample before the switch on.
                    around.drain(..around.len() - 1);
                    juno.set_patch(&after);
                    around.extend(render(&mut juno, 0.05));
                    worst = worst.max(step(&around));
                    let settled = render(&mut juno, 0.2);
                    steady = steady.max(step(&settled));
                    // What it comes to: the dry signal to the sample
                    // with the chorus off, and something else with it on.
                    render(&mut dry, lead + 0.1);
                    let alone = render(&mut dry, 0.2);
                    if to == ChorusMode::Off {
                        assert_eq!(settled, alone, "{model:?}: {from:?} to off is not the dry signal");
                    } else {
                        let lines: Vec<f64> = settled.iter().zip(&alone).map(|(both, dry)| both - dry).collect();
                        assert!(rms(&lines) > 0.1 * rms(&alone), "{model:?}: {from:?} to {to:?} has no chorus");
                    }
                }
                println!(
                    "{model:?}: {from:?} to {to:?}: a step of {worst:.4} at most, {steady:.4} in the steady sound"
                );
                assert!(worst < 1.2 * steady, "{model:?}: {from:?} to {to:?} clicks: {worst} against {steady}");
            }
        }
    }
}

#[test]
fn the_chorus_switched_after_a_wide_pulse_steps_no_more_than_its_fade() {
    // A pulse at its widest on the gate: the coupling capacitor takes
    // the pulse's steady part away while the note sounds and gives it up
    // over seconds once the key is up. No voice sounds then, and what
    // passes into the chorus is still a fifth of full scale: a line cut
    // out of it in one sample stepped the output by 0.12 of full scale,
    // and one let in by 0.04, 3.5 ms later (TODO-6438).
    let step = |signal: &[f64]| signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0f64, f64::max);
    for model in MODELS {
        let mut pulse = flat(model, Patch::DEFAULT).with(Slider::DcoPwm, 127);
        pulse.sawtooth = false;
        pulse.pulse = true;
        pulse.vca_gate = true;
        for (from, to) in [
            (ChorusMode::One, ChorusMode::Off),
            (ChorusMode::Off, ChorusMode::One),
            (ChorusMode::Two, ChorusMode::Off),
            (ChorusMode::Both, ChorusMode::One),
        ] {
            let (mut before, mut after) = (pulse, pulse);
            before.chorus = from;
            after.chorus = to;
            let mut juno = instrument(model, &before);
            juno.event(NOTE_ON, 60, 100);
            render(&mut juno, 1.0);
            juno.event(NOTE_OFF, 60, 0);
            let mut around = render(&mut juno, 0.05);
            assert_eq!(juno.sounding_voices(), 0, "{model:?}: the voice is at rest");
            let left = around[around.len() - 1].abs();
            assert!(left > 0.05, "{model:?} {from:?}: the capacitor still holds {left}");
            // From the last sample before the switch on.
            around.drain(..around.len() - 1);
            juno.set_patch(&after);
            around.extend(render(&mut juno, 0.05));
            let worst = step(&around);
            println!("{model:?}: {from:?} to {to:?} with {left:.3} left: a step of {worst:.5} at most");
            // The lines' whole share of what is left, over the fade's
            // 480 samples, is 0.0003 a sample at the most.
            assert!(worst < 0.001, "{model:?}: {from:?} to {to:?} clicks: {worst}");
        }
    }
}

#[test]
fn the_chorus_switched_in_silence_is_switched_at_once() {
    // Once nothing passes into the chorus any more its switches move at
    // once, whatever the voices' envelopes are still doing unheard: a
    // note struck straight after has no line fading under it. Here a
    // release of 12 s on the gate keeps the voice's envelope running, 54
    // s in all, long after the note is silent.
    for model in MODELS {
        let mut off = flat(model, Patch::DEFAULT).with(Slider::DcoPwm, 127).with(Slider::Release, 127);
        off.sawtooth = false;
        off.pulse = true;
        off.vca_gate = true;
        let mut on = off;
        on.chorus = ChorusMode::One;
        let mut switched = instrument(model, &on);
        let mut dry = instrument(model, &off);
        for juno in [&mut switched, &mut dry] {
            juno.event(NOTE_ON, 60, 100);
            render(juno, 1.0);
            juno.event(NOTE_OFF, 60, 0);
            render(juno, 6.0);
            assert_eq!(juno.sounding_voices(), 1, "{model:?}: the envelope is still running");
        }
        switched.set_patch(&off);
        for juno in [&mut switched, &mut dry] {
            juno.event(NOTE_ON, 64, 100);
        }
        assert_eq!(render(&mut switched, 0.1), render(&mut dry, 0.1), "{model:?}: a line is still fading");
    }
}

#[test]
fn the_amplifiers_level_moved_under_a_note_does_not_step() {
    // The largest step from one sample to the next.
    let step = |signal: &[f64]| signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0f64, f64::max);
    for model in MODELS {
        // The filter's own oscillation near middle C, a sine: its
        // largest step is its slope, and anything sudden is larger. The
        // level set in one sample stepped the output by 0.13 of full
        // scale where the sine's own steps are 0.016 (TODO-6438).
        let cutoff = match model {
            Model::Juno106 => 62,
            Model::Juno60 => 44,
        };
        let sine = flat(model, whistle(cutoff));
        for level in [0, 48, 80, 127] {
            let moved = sine.with(Slider::VcaLevel, level);
            let (mut worst, mut steady) = (0.0f64, 0.0f64);
            // At sixteen instants of the sine's cycle.
            for instant in 0..16 {
                let mut juno = instrument(model, &sine);
                let mut there = instrument(model, &moved);
                for juno in [&mut juno, &mut there] {
                    juno.event(NOTE_ON, 60, 100);
                }
                let lead = 0.4 + 0.00037 * f64::from(instant);
                let mut around = render(&mut juno, lead);
                steady = steady.max(step(&around[around.len() / 2..]));
                // From the last sample before the level moves on.
                around.drain(..around.len() - 1);
                juno.set_patch(&moved);
                around.extend(render(&mut juno, 0.05));
                worst = worst.max(step(&around));
                // What it comes to: the note of an instrument that had
                // the level from the first, to the sample.
                render(&mut there, lead + 0.05);
                let (settled, alone) = (render(&mut juno, 0.1), render(&mut there, 0.1));
                steady = steady.max(step(&alone));
                assert_eq!(settled, alone, "{model:?}: the level moved to {level} is not the level {level}");
            }
            println!("{model:?}: the level moved to {level}: a step of {worst:.4} at most, {steady:.4} in the notes");
            // The note's own steps are those of the louder of the two.
            assert!(worst < 1.2 * steady, "{model:?}: to {level} clicks: {worst} against {steady}");
        }
    }
}

#[test]
fn the_chorus_switched_at_random_however_fast_does_not_click() {
    let modes = [ChorusMode::Off, ChorusMode::One, ChorusMode::Two, ChorusMode::Both];
    // The largest step from one sample to the next.
    let step = |signal: &[f64]| signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0f64, f64::max);
    for model in MODELS {
        // The filter's own oscillation near middle C, a sine.
        let cutoff = match model {
            Model::Juno106 => 62,
            Model::Juno60 => 44,
        };
        let sine = flat(model, whistle(cutoff));
        // The sine's largest step with the chorus on, which adds to it.
        let steady = modes.iter().fold(0.0f64, |steady, &mode| {
            let mut patch = sine;
            patch.chorus = mode;
            steady.max(step(&play(model, &patch, 60, 2.5, 0.0)[SAMPLE_RATE as usize / 2..]))
        });
        // A new setting every so many samples: faster than the fade
        // moves at all, inside the fade out, inside the 288 samples the
        // lines fill in, inside the fade in, and about the fade's whole
        // 480 + 288 + 480.
        for every in [1usize, 7, 50, 200, 479, 481, 700, 768, 1300] {
            let run = |cut: usize| {
                let mut juno = instrument(model, &sine);
                juno.event(NOTE_ON, 60, 100);
                render(&mut juno, 0.5);
                let mut random = 12_345u32;
                let mut out = vec![0i16; every * (2 * SAMPLE_RATE as usize / every).min(20_000)];
                for piece in out.chunks_mut(every) {
                    random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let mut patch = sine;
                    patch.chorus = modes[(random >> 24) as usize % modes.len()];
                    juno.set_patch(&patch);
                    for part in piece.chunks_mut(cut) {
                        juno.render(part);
                    }
                }
                out
            };
            let out = run(every);
            assert_eq!(out, run(5), "{model:?}: every {every} samples: the calls' lengths change the samples");
            let signal: Vec<f64> = out.iter().map(|&sample| f64::from(sample) / 32_768.0).collect();
            let worst = step(&signal);
            println!("{model:?}: a setting every {every} samples: a step of {worst:.4} at most, {steady:.4} steady");
            assert!(worst < 1.1 * steady, "{model:?}: every {every} samples clicks: {worst} against {steady}");
        }
    }
}

#[test]
fn a_voice_whose_envelope_runs_on_unheard_is_silent_and_gives_way_to_a_seventh_key() {
    for model in MODELS {
        // The amplifier on the gate and the longest release: each note
        // is silent a few thousandths of a second after its key, and its
        // envelope takes 54 s to come to rest.
        let mut patch = flat(model, Patch::DEFAULT).with(Slider::Release, 127);
        patch.vca_gate = true;
        let mut alone = instrument(model, &patch);
        alone.event(NOTE_ON, 72, 100);
        let fresh = rms(&render(&mut alone, 0.1));
        let mut juno = instrument(model, &patch);
        for key in 60..66 {
            juno.event(NOTE_ON, key, 100);
            render(&mut juno, 0.05);
        }
        for key in 60..66 {
            juno.event(NOTE_OFF, key, 0);
        }
        // Counted, since their envelopes run, and not heard.
        let after = render(&mut juno, 3.0);
        assert_eq!(juno.sounding_voices(), VOICES, "{model:?}");
        assert!(after[after.len() / 2..].iter().all(|&sample| sample == 0.0), "{model:?}: a voice is heard");
        // A seventh key has a voice at once, and as loud as on an
        // instrument that has played nothing.
        juno.event(NOTE_ON, 72, 100);
        let seventh = rms(&render(&mut juno, 0.1));
        assert_eq!(juno.sounding_voices(), VOICES, "{model:?}");
        assert!((seventh / fresh - 1.0).abs() < 0.02, "{model:?}: the seventh key sounds at {seventh}, not {fresh}");
        // Six keys more are all held: none is refused for a voice that
        // only counts.
        for key in 48..54 {
            juno.event(NOTE_ON, key, 100);
        }
        let seven = rms(&render(&mut juno, 0.2));
        assert!(seven > 1.5 * fresh, "{model:?}: six keys sound at {seven}");
        // And every envelope comes to rest in the end.
        juno.event(0xb0, 123, 0);
        render(&mut juno, 60.0);
        assert_eq!(juno.sounding_voices(), 0, "{model:?}: an envelope never rests");
    }
}
