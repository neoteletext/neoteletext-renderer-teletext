//! The Prophet-5 as a whole: how keys take voices, what MIDI it hears,
//! that it is the same every time, that no patch and no message makes
//! it panic or leave its bounds, the factory programs, and its speed.

#![cfg(feature = "prophet5")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod prophet5_programs;
mod prophet5_support;

use neoteletext_audio_instruments::prophet5::patch::PATCH_BYTES;
use neoteletext_audio_instruments::prophet5::{Knob, Model, Patch, Prophet5, Switch, VOICES};
use prophet5_programs::SHEETS;
use prophet5_support::{
    CONTROL, MODELS, NOTE_OFF, NOTE_ON, PITCH_BEND, amplitude, instrument, plain, pure, render, rms,
};

/// The frequency of a MIDI key.
fn equal(key: u8) -> f64 {
    440.0 * ((f64::from(key) - 69.0) / 12.0).exp2()
}

/// How loud a key's fundamental is in a signal.
fn heard(signal: &[f64], key: u8) -> f64 {
    amplitude(signal, equal(key))
}

#[test]
fn a_sixth_key_takes_the_voice_of_the_first_whose_note_disappears() {
    // TM3 section 2-2: "holding C, D, E, F, and G, sustains Voices 1, 2,
    // 3, 4, 5, respectively. Adding A "steals" Voice 1 from the C-key
    // whose pitch disappears even though the key may still be held."
    for model in MODELS {
        let mut prophet = instrument(model, &pure(model));
        let keys = [60u8, 62, 64, 65, 67];
        for key in keys {
            prophet.event(NOTE_ON, key, 100);
        }
        let five = render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), VOICES);
        prophet.event(NOTE_ON, 69, 100);
        render(&mut prophet, 0.1);
        let six = render(&mut prophet, 0.5);
        let level = |signal: &[f64], key: u8| heard(&signal[signal.len() / 2..], key);
        println!(
            "{model:?}: the C before {:.4} and after {:.5}; the D after {:.4}; the A after {:.4}",
            level(&five, 60),
            level(&six, 60),
            level(&six, 62),
            level(&six, 69)
        );
        assert!(level(&five, 60) > 0.01 && level(&six, 60) < 0.001 && level(&six, 62) > 0.01 && level(&six, 69) > 0.01);
        assert_eq!(prophet.sounding_voices(), VOICES);
    }
}

#[test]
fn a_repeated_key_takes_its_own_voice_again_and_a_new_key_the_voice_let_go_longest_ago() {
    // TM3 section 2-2: "Repeated notes key the same voice", and "The
    // earliest-used voice is reassigned to each new note played."
    for model in MODELS {
        // A long release, so that a voice let go still sounds.
        let patch = pure(model).with(Knob::AmplifierRelease, 100);
        let mut prophet = instrument(model, &patch);
        for _ in 0..7 {
            prophet.event(NOTE_ON, 60, 100);
            render(&mut prophet, 0.05);
            prophet.event(NOTE_OFF, 60, 0);
            render(&mut prophet, 0.05);
        }
        assert_eq!(prophet.sounding_voices(), 1, "{model:?}: one key, one voice");

        // Five keys struck and let go ring on five voices; the sixth
        // takes the first key's voice, the one let go longest ago.
        let mut prophet = instrument(model, &patch);
        for key in [60u8, 62, 64, 65, 67] {
            prophet.event(NOTE_ON, key, 100);
            render(&mut prophet, 0.05);
            prophet.event(NOTE_OFF, key, 0);
        }
        assert_eq!(prophet.sounding_voices(), VOICES);
        prophet.event(NOTE_ON, 72, 100);
        render(&mut prophet, 0.2);
        let after = render(&mut prophet, 0.4);
        println!(
            "{model:?}: after a sixth key the first is at {:.5} and the second at {:.4}",
            heard(&after, 60),
            heard(&after, 62)
        );
        assert!(heard(&after, 60) < 0.0005 && heard(&after, 62) > 0.002 && heard(&after, 72) > 0.01);
    }
}

#[test]
fn the_messages_it_hears_and_those_it_counts() {
    let mut prophet = instrument(Model::Rev3, &plain(Model::Rev3));
    // A note on with no velocity is a note off; velocity changes nothing.
    prophet.event(NOTE_ON, 60, 1);
    let soft = rms(&render(&mut prophet, 0.2));
    prophet.event(NOTE_ON, 60, 0);
    render(&mut prophet, 0.2);
    assert_eq!(prophet.sounding_voices(), 0);
    prophet.event(NOTE_ON | 5, 60, 127);
    let loud = rms(&render(&mut prophet, 0.2));
    assert!((soft / loud - 1.0).abs() < 0.05, "{soft} {loud}");
    // Volume is the square of its controller's share.
    prophet.event(CONTROL, 7, 64);
    render(&mut prophet, 0.05);
    let half = rms(&render(&mut prophet, 0.2));
    assert!((half / loud - (64.0f64 / 127.0).powi(2)).abs() < 0.02);
    prophet.event(CONTROL, 7, 127);
    // All notes off lets go; all sound off silences at once.
    prophet.event(CONTROL, 123, 0);
    render(&mut prophet, 0.3);
    assert_eq!(prophet.sounding_voices(), 0);
    prophet.event(NOTE_ON, 64, 100);
    render(&mut prophet, 0.1);
    prophet.event(CONTROL, 120, 0);
    // But for what the decimator still holds.
    render(&mut prophet, 0.01);
    assert!(render(&mut prophet, 0.1).iter().all(|&sample| sample == 0.0));
    assert_eq!(prophet.ignored_events(), 0);
    // What it does not hear, it counts.
    for (status, first, second) in
        [(0xa0, 60, 1), (0xc0, 1, 0), (0xd0, 1, 0), (CONTROL, 2, 1), (NOTE_ON, 200, 1), (0x10, 1, 1)]
    {
        prophet.event(status, first, second);
    }
    assert_eq!(prophet.ignored_events(), 6);
    prophet.release_all();
    prophet.reset();
    assert_eq!((prophet.ignored_events(), prophet.sounding_voices()), (0, 0));
    assert_eq!(prophet.patch(), &plain(Model::Rev3));
    assert_eq!(prophet.model(), Model::Rev3);
}

#[test]
fn all_notes_off_lets_go_of_every_key_held_in_unison_and_the_pedal_still_holds() {
    // In unison one key sounds and the others held wait their turn: a
    // note off for the one that sounds hands the note to the next. All
    // notes off is a note off for every one of them.
    for model in MODELS {
        let patch = plain(model).with_switch(Switch::Unison, true);
        let mut prophet = instrument(model, &patch);
        for key in [60u8, 64, 67, 72] {
            prophet.event(NOTE_ON, key, 100);
        }
        render(&mut prophet, 0.2);
        assert_eq!(prophet.sounding_voices(), VOICES);
        prophet.event(CONTROL, 123, 0);
        render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), 0, "{model:?}: a key was left held");
        // With the pedal down the note goes on until the pedal is up.
        prophet.event(CONTROL, 64, 127);
        for key in [60u8, 64, 67] {
            prophet.event(NOTE_ON, key, 100);
        }
        render(&mut prophet, 0.2);
        prophet.event(CONTROL, 123, 0);
        render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), VOICES, "{model:?}: the pedal holds");
        prophet.event(CONTROL, 64, 0);
        render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), 0, "{model:?}: the pedal is up");
    }
}

/// A 64-bit linear congruential generator with Knuth's MMIX constants.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn patch(&mut self) -> Patch {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = self.next() as u8;
        }
        Patch::from_bytes(&bytes)
    }
}

#[test]
fn random_patches_and_random_messages_neither_panic_nor_leave_the_bounds() {
    for model in MODELS {
        let mut random = Random(0x5eed + model as u64);
        let mut prophet = Prophet5::new(model);
        let (mut out, mut loud) = (vec![0i16; 480], 0u32);
        for round in 0..600 {
            if round % 8 == 0 {
                prophet.set_patch(&random.patch());
            }
            for _ in 0..random.next() % 6 {
                // Any three bytes at all, with notes and controllers
                // among them more often than chance.
                let status = match random.next() % 4 {
                    0 => NOTE_ON,
                    1 => NOTE_OFF,
                    2 => [CONTROL, PITCH_BEND][random.next() as usize % 2],
                    _ => random.next() as u8,
                };
                let narrow = !random.next().is_multiple_of(4);
                let data = |random: &mut Random| if narrow { (random.next() % 128) as u8 } else { random.next() as u8 };
                prophet.event(status, data(&mut random), data(&mut random));
            }
            prophet.set_master_tune(random.next().cast_signed());
            prophet.render(&mut out);
            assert!(prophet.sounding_voices() <= VOICES);
            loud += u32::from(out.iter().any(|&sample| sample.unsigned_abs() > 300));
        }
        println!("{model:?}: {loud} of 600 blocks sound");
        assert!(loud > 100, "{model:?}: the random patches are heard");
        // Whatever came before, the instrument falls silent when asked.
        prophet.event(CONTROL, 120, 0);
        prophet.render(&mut out);
        prophet.render(&mut out);
        assert!(out.iter().all(|&sample| sample == 0));
    }
}

#[test]
fn a_volume_message_between_two_renders_does_not_step_the_signal() {
    // A low, nearly pure tone moves little from one sample to the next.
    // The volume falling from full to a quarter between two calls moved
    // the gain all at once: a step of most of the signal's height.
    for model in MODELS {
        let mut prophet = instrument(model, &pure(model));
        prophet.event(NOTE_ON, 45, 100);
        render(&mut prophet, 0.5);
        let largest = |signal: &[f64]| signal.windows(2).fold(0.0f64, |most, pair| most.max((pair[1] - pair[0]).abs()));
        let steady = render(&mut prophet, 0.2513);
        let (peak, step) = (steady.iter().fold(0.0f64, |peak, &value| peak.max(value.abs())), largest(&steady));
        let mut worst = 0.0f64;
        // Twenty changes, each where the last call happened to end.
        for turn in 0..20 {
            let before = *render(&mut prophet, 0.0137).last().unwrap_or(&0.0);
            prophet.event(CONTROL, 7, if turn % 2 == 0 { 64 } else { 127 });
            let after = render(&mut prophet, 0.05);
            worst = worst.max((after[0] - before).abs()).max(largest(&after));
        }
        println!("{model:?}: peak {peak:.4}, largest step steady {step:.5}, across volume changes {worst:.5}");
        assert!(worst < 1.5 * step, "{model:?}: {worst} against {step}");
    }
}

#[test]
fn the_pedal_still_holds_after_all_sound_off() {
    // All sound off silences the voices; it is no message about the
    // pedal, which is still down: a key struck and let go after it is
    // held, and the pedal going up lets it go.
    for model in MODELS {
        let mut prophet = instrument(model, &pure(model));
        prophet.event(CONTROL, 64, 127);
        prophet.event(NOTE_ON, 60, 100);
        render(&mut prophet, 0.1);
        prophet.event(CONTROL, 120, 0);
        render(&mut prophet, 0.1);
        assert_eq!(prophet.sounding_voices(), 0, "{model:?}: silenced");
        prophet.event(NOTE_ON, 64, 100);
        render(&mut prophet, 0.1);
        prophet.event(NOTE_OFF, 64, 0);
        render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), 1, "{model:?}: the pedal holds the key let go");
        prophet.event(CONTROL, 64, 0);
        render(&mut prophet, 0.5);
        assert_eq!(prophet.sounding_voices(), 0, "{model:?}: the pedal up lets it go");
    }
}

#[test]
fn the_pedal_still_holds_after_a_patch_turns_unison_on_or_off() {
    // Changing between polyphonic and unison lets go of every key, but a
    // pedal that is down is down still: a key struck and let go after
    // the change sounds on until the pedal comes up. The instrument used
    // to keep the pedal for its releases and forget it for its keys.
    for model in MODELS {
        for unison in [true, false] {
            let before = plain(model).with_switch(Switch::Unison, !unison).with(Knob::AmplifierRelease, 0);
            let mut prophet = instrument(model, &before);
            prophet.event(CONTROL, 64, 127);
            prophet.set_patch(&before.with_switch(Switch::Unison, unison));
            prophet.event(NOTE_ON, 60, 100);
            render(&mut prophet, 0.2);
            prophet.event(NOTE_OFF, 60, 0);
            render(&mut prophet, 0.3);
            let held = rms(&render(&mut prophet, 0.1));
            let voices = prophet.sounding_voices();
            prophet.event(CONTROL, 64, 0);
            render(&mut prophet, 0.3);
            let lifted = rms(&render(&mut prophet, 0.1));
            println!("{model:?} to unison {unison}: under the pedal {voices} voices at {held:.4}; lifted {lifted:.6}");
            assert_eq!(voices, if unison { VOICES } else { 1 }, "{model:?} {unison}");
            assert!(held > 0.01 && lifted < 1.0e-4 && prophet.sounding_voices() == 0, "{model:?} {unison}");
        }
    }
}

#[test]
fn no_run_of_keys_pedals_and_patches_leaves_a_note_sounding() {
    // Random keys down and up in a small compass, the pedal, all notes
    // off, all sound off, reset all controllers, random patches and
    // unison switched, rendered between; then every key up, the pedal
    // up and both releases short: nothing may sound a second later.
    for model in MODELS {
        for seed in 1..=60u64 {
            let mut random = Random(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            let mut prophet = Prophet5::new(model);
            let mut out = vec![0i16; 3000];
            for _ in 0..20 + random.next() % 60 {
                let key = 36 + (random.next() % 12) as u8;
                match random.next() % 20 {
                    0..=7 => prophet.event(NOTE_ON, key, 100),
                    8..=12 => prophet.event(NOTE_OFF, key, 0),
                    13 => prophet.event(CONTROL, 64, if random.next().is_multiple_of(2) { 0 } else { 127 }),
                    14 => prophet.event(CONTROL, 123, 0),
                    15 => prophet.event(CONTROL, 121, 0),
                    16 => prophet.event(CONTROL, 120, 0),
                    17 | 18 => prophet.set_patch(&random.patch()),
                    _ => {
                        let patch = *prophet.patch();
                        prophet.set_patch(&patch.with_switch(Switch::Unison, !patch.on(Switch::Unison)));
                    }
                }
                let length = 1 + random.next() as usize % out.len();
                prophet.render(&mut out[..length]);
            }
            for key in 0..=127 {
                prophet.event(NOTE_OFF, key, 0);
            }
            prophet.event(CONTROL, 64, 0);
            let unison = prophet.patch().on(Switch::Unison);
            let short = plain(model).with(Knob::AmplifierRelease, 0).with(Knob::FilterRelease, 0);
            prophet.set_patch(&short.with_switch(Switch::Unison, unison));
            render(&mut prophet, 1.0);
            let tail = rms(&render(&mut prophet, 0.1));
            assert!(prophet.sounding_voices() == 0 && tail < 1.0e-4, "{model:?} seed {seed}: {tail}");
        }
    }
}

#[test]
fn the_same_messages_give_the_same_samples_however_they_are_rendered() {
    for model in MODELS {
        let score = |prophet: &mut Prophet5, lengths: &[usize]| {
            let mut random = Random(77);
            prophet.set_patch(&SHEETS[4].program(model));
            let mut samples = Vec::new();
            let mut lengths = lengths.iter().cycle();
            while samples.len() < 48_000 {
                if random.next().is_multiple_of(3) {
                    prophet.event(NOTE_ON, 40 + (random.next() % 40) as u8, 100);
                }
                if random.next().is_multiple_of(5) {
                    prophet.event(CONTROL, 1, (random.next() % 128) as u8);
                }
                let mut block = vec![0i16; 960];
                // The same 960 samples, in pieces of other lengths.
                let mut done = 0;
                while done < block.len() {
                    let length = (*lengths.next().unwrap()).min(block.len() - done);
                    prophet.render(&mut block[done..done + length]);
                    done += length;
                }
                samples.extend(block);
            }
            samples
        };
        let whole = score(&mut Prophet5::new(model), &[960]);
        let pieces = score(&mut Prophet5::new(model), &[1, 7, 64, 333]);
        assert!(whole == pieces, "{model:?}");
        let mut again = Prophet5::new(model);
        score(&mut again, &[960]);
        again.reset();
        assert!(whole == score(&mut again, &[100]), "{model:?}: after a reset");
    }
}

#[test]
fn the_factory_programs_have_their_knobs_where_the_sheets_draw_them() {
    // Seven pointers measured again on the scans of Sequential's patch
    // sheets (the disc's middle and the direction of what stands out of
    // it, on the dial's 300 degrees), in tenths of the dial: each is at
    // a printed number or between two, and the fixture is within two
    // tenths of it. A knob that is not entered stands at 0.
    let drawn: [(usize, Knob, u8); 7] = [
        (0, Knob::FilterEnvelopeAmount, 70),
        (2, Knob::AmplifierRelease, 65),
        (4, Knob::SourceMix, 0),
        (4, Knob::Resonance, 20),
        (4, Knob::AmplifierAttack, 1),
        (6, Knob::FilterRelease, 74),
        (7, Knob::FilterRelease, 69),
    ];
    for (sheet, knob, tenths) in drawn {
        let sheet = SHEETS[sheet];
        let entered = sheet.knobs.iter().find(|&&(entry, _)| entry == knob).map_or(0, |&(_, tenths)| tenths);
        assert!(entered.abs_diff(tenths) <= 2, "{}: {knob:?} is entered at {entered}, drawn at {tenths}", sheet.name);
    }
}

#[test]
fn the_factory_programs_sound_and_keep_inside_full_scale() {
    for model in MODELS {
        for sheet in SHEETS {
            let mut prophet = instrument(model, &sheet.program(model));
            prophet.event(CONTROL, 1, 40);
            for key in [48u8, 55, 60, 64, 67] {
                prophet.event(NOTE_ON, key, 100);
            }
            let held = render(&mut prophet, 1.5);
            for key in [48u8, 55, 60, 64, 67] {
                prophet.event(NOTE_OFF, key, 0);
            }
            let peak = held.iter().fold(0.0f64, |peak, &sample| peak.max(sample.abs()));
            let level = 20.0 * rms(&held).log10();
            println!("{model:?} {}: five keys peak {peak:.3}, {level:.1} dB", sheet.name);
            assert!(peak < 0.999 && level > -50.0, "{model:?} {}", sheet.name);
        }
    }
}

#[test]
#[ignore = "a measurement of speed, which needs a release build: cargo test --release -- --ignored rendering_speed"]
fn rendering_speed() {
    for model in MODELS {
        // Five voices, both oscillators with every waveform, oscillator B
        // through Poly-Mod to all three at audio rate: the most a
        // sample costs.
        let mut bytes = [0xffu8; PATCH_BYTES];
        // Not unison, which is five voices too but one key; the filter
        // open, no glide.
        bytes[7] = 0x7f;
        let patch = Patch::from_bytes(&bytes).with(Knob::Resonance, 60).with(Knob::AmplifierRelease, 127);
        // Oscillator B out of its low-frequency mode and at the top of
        // its knob, under the keyboard's top octave: its voltage jumps
        // or turns in every other sample, and each of those samples is
        // made in two stretches or three.
        let fast = patch.with_switch(Switch::OscBLowFrequency, false).with(Knob::OscBPulseWidth, 40);
        let (middle, top) = ([48u8, 55, 60, 64, 67], [96u8, 100, 103, 106, 108]);
        for (name, patch, keys) in [
            ("everything on", patch, middle),
            ("everything on, oscillator B at its fastest", fast, top),
            ("the plain patch", plain(model), middle),
        ] {
            let mut prophet = instrument(model, &patch);
            for key in keys {
                prophet.event(NOTE_ON, key, 100);
            }
            let mut out = vec![0i16; 48_000];
            prophet.render(&mut out);
            assert_eq!(prophet.sounding_voices(), VOICES);
            let start = std::time::Instant::now();
            let seconds = 20;
            for _ in 0..seconds {
                prophet.render(&mut out);
            }
            let taken = start.elapsed().as_secs_f64();
            println!(
                "{model:?}, {name}: five voices render {:.1} times faster than real time",
                f64::from(seconds) / taken
            );
        }
    }
}
