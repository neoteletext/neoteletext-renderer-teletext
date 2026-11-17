//! The Jupiter-8 as an instrument: its MIDI messages, its assign modes,
//! its two halves, any patch and any messages without a panic, the same
//! samples from the same input, and what it costs (TODO-6435).

// Without the feature there is nothing here to test.
#![cfg(feature = "jupiter8")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod analog;
#[path = "jupiter8_factory.rs"]
mod factory;
#[path = "jupiter8_support.rs"]
mod support;

use std::time::Instant;

use analog::Random;
use neoteletext_audio_instruments::jupiter8::{
    Jupiter8, KeyMode, PATCH_BYTES, Param, Patch, Performance, Portamento, VOICES,
};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{
    A2, CONTROL, NOTE_OFF, NOTE_ON, PITCH_BEND, cents, component, decibels, instrument, pitch, render, rms, samples,
    square,
};

/// A patch from random bytes: most random bytes are no patch, and that
/// they are refused, not a panic, is part of the test.
fn random_patch(random: &mut Random) -> Patch {
    loop {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = random.word() as u8;
        }
        if let Ok(patch) = Patch::from_bytes(&bytes) {
            return patch;
        }
        // Held to each control's range, any bytes are one.
        let mut patch = Patch::DEFAULT;
        for (place, &byte) in bytes.iter().enumerate() {
            let mut held = patch.to_bytes();
            held[place] = byte;
            if let Ok(taken) = Patch::from_bytes(&held) {
                patch = taken;
            }
        }
        if random.word().is_multiple_of(2) {
            return patch;
        }
    }
}

/// A random message: mostly ones the instrument takes, some it ignores,
/// some with bytes no message has.
fn random_event(random: &mut Random) -> (u8, u8, u8) {
    let word = random.word();
    let [a, b, c, kind] = word.to_le_bytes();
    match kind % 16 {
        0..=6 => (NOTE_ON | (a & 15), 24 + b % 85, c & 0x7f),
        7..=10 => (NOTE_OFF | (a & 15), 24 + b % 85, c & 0x7f),
        11 => (CONTROL, [1, 5, 7, 8, 64, 65, 120, 121, 123, 10, 91][usize::from(b % 11)], c & 0x7f),
        12 => (PITCH_BEND, b & 0x7f, c & 0x7f),
        13 => (0xc0 | (a & 15), b & 0x7f, c & 0x7f),
        _ => (a, b, c),
    }
}

/// Plays random patches, settings and messages for `blocks` blocks of
/// 20 ms, the calls to render cut at `cut` samples, and gives every
/// sample.
fn random_run(seed: u64, blocks: usize, cut: usize) -> Vec<i16> {
    let mut random = Random(seed);
    let mut jupiter = Jupiter8::new();
    let mut out = vec![0i16; blocks * BLOCK_SAMPLES];
    for block in out.chunks_mut(BLOCK_SAMPLES) {
        match random.word() % 32 {
            0..=2 => jupiter.set_patch(&random_patch(&mut random)),
            3 | 4 => jupiter.set_lower_patch(&random_patch(&mut random)),
            5 => jupiter.set_key_mode([KeyMode::Whole, KeyMode::Dual, KeyMode::Split][random.word() as usize % 3]),
            6 => jupiter.set_split(random.word() as u8),
            7 => jupiter.set_balance(random.word() as u8),
            8 => jupiter.set_performance(&Performance {
                bend_vco: random.word() as u16,
                bend_vcf: random.word() as u16,
                lfo_vco: random.word() as u16,
                lfo_vcf: random.word() as u16,
                portamento: [Portamento::Off, Portamento::On, Portamento::UpperOnly][random.word() as usize % 3],
                portamento_time: random.word() as u8,
            }),
            _ => {}
        }
        for _ in 0..random.word() % 4 {
            let (status, first, second) = random_event(&mut random);
            jupiter.event(status, first, second);
        }
        for piece in block.chunks_mut(cut) {
            jupiter.render(piece);
        }
        assert!(jupiter.sounding_voices() <= VOICES);
    }
    out
}

#[test]
fn random_patches_and_messages_never_panic_and_the_output_stays_alive() {
    // An unoptimised build renders a tenth as fast: it runs a quarter
    // of the seeds.
    let seeds = if cfg!(debug_assertions) { 2 } else { 8 };
    for seed in 1..=seeds {
        let out = random_run(seed, 400, BLOCK_SAMPLES);
        // Not stuck at a rail, and not silent throughout. Eight voices
        // at the amplifier's full level, with resonance, can ask for
        // more than full scale, and what is beyond is saturated.
        let clipped = out.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
        let loud = out.iter().filter(|&&sample| sample.unsigned_abs() > 300).count();
        println!("seed {seed}: {clipped} samples at a rail and {loud} above 1% of {}", out.len());
        assert!(clipped < out.len() / 5 && loud > out.len() / 100, "seed {seed}");
    }
}

#[test]
fn the_same_input_gives_the_same_samples_however_the_calls_are_cut() {
    let blocks = if cfg!(debug_assertions) { 60 } else { 150 };
    let whole = random_run(42, blocks, BLOCK_SAMPLES);
    assert_eq!(whole, random_run(42, blocks, BLOCK_SAMPLES), "two instruments differ");
    assert_eq!(whole, random_run(42, blocks, 7), "calls of 7 samples differ");
    assert_ne!(whole, random_run(43, blocks, BLOCK_SAMPLES));
}

/// The levels, in decibels against the first, of the notes of some keys
/// in what an instrument plays for half a second.
fn heard(jupiter: &mut Jupiter8, keys: &[u8]) -> Vec<f64> {
    let signal = render(jupiter, 0.5);
    let level = |key: u8| component(&signal[samples(0.1)..], 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2());
    keys.iter().map(|&key| decibels(level(key), level(keys[0]).max(1.0e-9))).collect()
}

/// The keys of a chord that shares no harmonic below the sixth: the
/// first ten of them are told apart by their fundamentals.
const KEYS: [u8; 10] = [48, 50, 53, 57, 59, 62, 64, 66, 70, 71];

#[test]
fn a_volume_message_between_two_renders_does_not_step_the_signal() {
    // The largest step from one sample to the next.
    let step = |signal: &[i16]| signal.windows(2).map(|pair| (pair[1] - pair[0]).unsigned_abs()).max().unwrap_or(0);
    // VCO-1's triangle on the A above middle C: its largest step is its
    // slope, and anything sudden is larger. The volume's gain was worked
    // out once for each call to render, so a volume message moved it
    // between one sample and the next (TODO-6438).
    let patch = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    // Half a second for the note to settle; then twenty changes of the
    // volume 3,058 samples apart, the calls to render cut at `cut`
    // samples; then the volume at its half.
    let lead = SAMPLE_RATE as usize / 2;
    let run = |cut: usize| {
        let mut jupiter = instrument(&patch);
        jupiter.event(NOTE_ON, 69, 100);
        let mut out = vec![0i16; 21 * 3_058 + SAMPLE_RATE as usize / 2];
        jupiter.render(&mut vec![0i16; lead]);
        for (turn, piece) in out.chunks_mut(3_058).enumerate() {
            jupiter.event(CONTROL, 7, [127, 64, 127, 0][turn.min(21) % 4]);
            for part in piece.chunks_mut(cut) {
                jupiter.render(part);
            }
        }
        out
    };
    let out = run(3_058);
    assert_eq!(out, run(7), "the calls' lengths change the samples");
    // The note alone, at the full volume of the first 3,058 samples.
    let (alone, changes) = (&out[..3_058], &out[3_058..21 * 3_058]);
    let (steady, worst) = (step(alone), step(changes));
    println!("a step of {worst} units at most across the volume's changes, {steady} in the note");
    assert!(worst * 5 < steady * 6, "the volume clicks: {worst} against {steady}");
    // And the volume arrives: half the controller, to the sample of an
    // instrument that had it from the first.
    let mut there = instrument(&patch);
    there.event(CONTROL, 7, 64);
    there.event(NOTE_ON, 69, 100);
    let mut same = vec![0i16; lead + out.len()];
    there.render(&mut same);
    let settled = out.len() - SAMPLE_RATE as usize / 4;
    assert_eq!(out[settled..], same[lead + settled..], "the volume does not arrive");
    // A reset puts the gain back at once: the first sample of a note is
    // then the first sample of a new instrument's.
    let mut again = instrument(&patch);
    again.event(CONTROL, 7, 0);
    again.render(&mut vec![0i16; lead]);
    again.reset();
    again.event(NOTE_ON, 69, 100);
    let mut fresh = instrument(&patch);
    fresh.event(NOTE_ON, 69, 100);
    let (mut after_reset, mut from_new) = (vec![0i16; lead], vec![0i16; lead]);
    again.render(&mut after_reset);
    fresh.render(&mut from_new);
    assert_eq!(after_reset, from_new, "a reset leaves the volume on its way");
}

#[test]
fn poly_1_has_eight_voices_and_the_ninth_key_waits() {
    // Owner's manual, printed page 12: "When eight notes are played and
    // held, no new notes can be sounded until one of the original eight
    // is released."
    let mut jupiter = instrument(&Patch::DEFAULT.with(Param::Vco1Wave, 0));
    for &key in &KEYS[..9] {
        jupiter.event(NOTE_ON, key, 100);
    }
    let levels = heard(&mut jupiter, &KEYS[..9]);
    assert_eq!(jupiter.sounding_voices(), 8);
    assert!(levels[..8].iter().all(|level| level.abs() < 1.5) && levels[8] < -40.0, "{levels:?}");
    // One released: the next key has its voice.
    jupiter.event(NOTE_OFF, KEYS[2], 0);
    jupiter.event(NOTE_ON, KEYS[9], 100);
    let levels = heard(&mut jupiter, &[KEYS[0], KEYS[9], KEYS[2]]);
    assert!(levels[1].abs() < 1.5 && levels[2] < -40.0, "{levels:?}");
}

#[test]
fn poly_1_lets_a_release_ring_and_poly_2_cuts_it_short() {
    // Owner's manual: in poly 2 "any notes played previously will have
    // instantaneous Releases regardless of the patch programs".
    for (mode, rings) in [(0u8, true), (3, false)] {
        let patch = Patch::DEFAULT.with(Param::Vco1Wave, 0).with(Param::Env2Release, 220).with(Param::AssignMode, mode);
        let mut jupiter = instrument(&patch);
        jupiter.event(NOTE_ON, KEYS[0], 100);
        render(&mut jupiter, 0.2);
        jupiter.event(NOTE_OFF, KEYS[0], 0);
        // Alone, the release rings on in both modes.
        let alone = heard(&mut jupiter, &[KEYS[0]]);
        assert_eq!((alone.len(), jupiter.sounding_voices()), (1, 1));
        jupiter.event(NOTE_ON, KEYS[3], 100);
        let levels = heard(&mut jupiter, &[KEYS[3], KEYS[0]]);
        println!("assign mode {mode}: the released note {:.1} dB against the new one", levels[1]);
        assert_eq!(levels[1] > -12.0, rings, "{levels:?}");
    }
}

#[test]
fn poly_1_goes_round_the_voices_and_poly_2_keeps_the_first() {
    // Service notes, page 24: in poly 1 "tapping single key (same key)
    // will assign modules from A to H one by one". Each voice's tuning is
    // its own, within 0.8 cent: the same key on another voice is another
    // pitch, and on the same voice the same.
    let tapped = |mode: u8| {
        let mut jupiter = instrument(&square().with(Param::AssignMode, mode));
        (0..9)
            .map(|_| {
                jupiter.event(NOTE_ON, A2, 100);
                let found = pitch(&render(&mut jupiter, 0.6)[samples(0.1)..]);
                jupiter.event(NOTE_OFF, A2, 0);
                render(&mut jupiter, 0.05);
                cents(440.0, found)
            })
            .collect::<Vec<f64>>()
    };
    let round = tapped(0);
    println!("poly 1, one key tapped nine times, cents: {round:.3?}");
    let distinct =
        round[..8].iter().filter(|&&a| round[..8].iter().filter(|&&b| (a - b).abs() < 0.002).count() == 1).count();
    assert!(distinct >= 6 && (round[8] - round[0]).abs() < 0.002);
    let first = tapped(3);
    assert!(first.iter().all(|pitch| (pitch - first[0]).abs() < 0.002), "{first:?}");
}

#[test]
fn solo_plays_the_lowest_key_on_one_voice() {
    // Owner's manual: "using only one voice ... The Solo Mode uses Low
    // Note priority."
    let mut jupiter = instrument(&square().with(Param::AssignMode, 1));
    jupiter.event(NOTE_ON, A2, 100);
    jupiter.event(NOTE_ON, A2 + 7, 100);
    assert!(cents(440.0, pitch(&render(&mut jupiter, 0.5)[samples(0.1)..])).abs() < 1.0);
    jupiter.event(NOTE_ON, A2 - 5, 100);
    assert!(cents(440.0 * (-5.0f64 / 12.0).exp2(), pitch(&render(&mut jupiter, 0.5)[samples(0.1)..])).abs() < 1.0);
    assert_eq!(jupiter.sounding_voices(), 1);
    // The lowest released, the note goes back to the lowest still held.
    jupiter.event(NOTE_OFF, A2 - 5, 0);
    assert!(cents(440.0, pitch(&render(&mut jupiter, 0.5)[samples(0.1)..])).abs() < 1.0);
    jupiter.event(NOTE_OFF, A2, 0);
    jupiter.event(NOTE_OFF, A2 + 7, 0);
    render(&mut jupiter, 0.5);
    assert_eq!(jupiter.sounding_voices(), 0);
}

#[test]
fn solo_plays_the_lowest_of_more_keys_than_it_remembers() {
    // Sixteen keys are remembered; a seventeenth below them all, which
    // MIDI can send, is the lowest key down and the one solo plays.
    let mut jupiter = instrument(&square().with(Param::AssignMode, 1));
    for key in (A2 + 1..=A2 + 16).rev() {
        jupiter.event(NOTE_ON, key, 100);
    }
    jupiter.event(NOTE_ON, A2 - 12, 100);
    let lowest = cents(220.0, pitch(&render(&mut jupiter, 0.4)[samples(0.1)..]));
    assert!(lowest.abs() < 2.0, "the seventeenth key, the lowest: {lowest} cents from it");
    // Let go, it gives the note back to the lowest of the others; a key
    // above them all changes nothing; and every key up ends the note.
    jupiter.event(NOTE_OFF, A2 - 12, 0);
    jupiter.event(NOTE_ON, A2 + 30, 100);
    let next = cents(440.0 * (1.0f64 / 12.0).exp2(), pitch(&render(&mut jupiter, 0.4)[samples(0.1)..]));
    assert!(next.abs() < 2.0, "the lowest of the sixteen: {next} cents from it");
    for key in 0..=127 {
        jupiter.event(NOTE_OFF, key, 0);
    }
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 0);
}

#[test]
fn unison_shares_all_eight_voices_among_the_keys() {
    // Owner's manual: "All 8 synthesizers will be assigned to any one key
    // pressed, dividing to four each for two keys, and so on".
    let mut jupiter = instrument(&Patch::DEFAULT.with(Param::Vco1Wave, 0).with(Param::AssignMode, 2));
    jupiter.event(NOTE_ON, KEYS[0], 100);
    let one = rms(&render(&mut jupiter, 0.5)[samples(0.2)..]);
    assert_eq!(jupiter.sounding_voices(), 8);
    let single = rms(&{
        let mut solo = instrument(&Patch::DEFAULT.with(Param::Vco1Wave, 0).with(Param::AssignMode, 1));
        solo.event(NOTE_ON, KEYS[0], 100);
        render(&mut solo, 0.5)[samples(0.2)..].to_vec()
    });
    // Eight voices a fraction of a cent apart add to between the root of
    // eight and eight times one.
    println!("eight voices on one key: {:.1} dB above one", decibels(one, single));
    assert!(decibels(one, single) > 8.0 && decibels(one, single) < 18.2);
    jupiter.event(NOTE_ON, KEYS[3], 100);
    let levels = heard(&mut jupiter, &[KEYS[0], KEYS[3]]);
    assert_eq!(jupiter.sounding_voices(), 8);
    assert!(levels[1].abs() < 6.0, "{levels:?}");
    jupiter.event(NOTE_OFF, KEYS[0], 0);
    jupiter.event(NOTE_OFF, KEYS[3], 0);
    render(&mut jupiter, 0.5);
    assert_eq!(jupiter.sounding_voices(), 0);
}

#[test]
fn dual_layers_the_two_patches_and_split_shares_the_keyboard() {
    // Owner's manual, printed page 3. The upper patch at 8', the lower
    // two octaves up at 2': which half plays is heard from the pitch.
    let upper = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    let lower = upper.with(Param::Vco1Range, 3);
    let mut jupiter = Jupiter8::new();
    jupiter.set_patch(&upper);
    jupiter.set_lower_patch(&lower);
    let both = |jupiter: &mut Jupiter8, key: u8| {
        jupiter.event(NOTE_ON, key, 100);
        let levels = heard(jupiter, &[key, key + 24]);
        let voices = jupiter.sounding_voices();
        jupiter.event(CONTROL, 120, 0);
        // A triangle has no fourth harmonic: two octaves up is the lower
        // patch or nothing.
        (voices, levels[1] > -12.0, levels[1] < 12.0)
    };
    assert_eq!(both(&mut jupiter, 57), (1, false, true), "whole");
    jupiter.set_key_mode(KeyMode::Dual);
    assert_eq!(both(&mut jupiter, 57), (2, true, true), "dual");
    jupiter.set_key_mode(KeyMode::Split);
    assert_eq!(both(&mut jupiter, 59), (1, true, false), "split, below");
    assert_eq!(both(&mut jupiter, 60), (1, false, true), "split, from the point up");
    jupiter.set_split(48);
    assert_eq!(both(&mut jupiter, 50), (1, false, true), "split moved");
    // Four voices each: a fifth key of a half waits.
    for &key in &KEYS[4..9] {
        jupiter.event(NOTE_ON, key, 100);
    }
    render(&mut jupiter, 0.1);
    assert_eq!(jupiter.sounding_voices(), 4);
    // The balance: at either end one half alone (controller 8).
    jupiter.event(CONTROL, 120, 0);
    jupiter.set_key_mode(KeyMode::Dual);
    for (balance, upper_heard, lower_heard) in [(0u8, false, true), (64, true, true), (127, true, false)] {
        jupiter.event(CONTROL, 8, balance);
        jupiter.event(NOTE_ON, 57, 100);
        let signal = render(&mut jupiter, 0.5);
        jupiter.event(CONTROL, 120, 0);
        let level = |hertz: f64| component(&signal[samples(0.1)..], hertz) > 1.0e-3;
        assert_eq!((level(220.0), level(880.0)), (upper_heard, lower_heard), "balance {balance}");
    }
}

#[test]
fn a_key_plays_both_halves_whatever_was_played_before_the_key_mode_changed() {
    // The last four voices read the upper half's assigner in whole and
    // the lower's in dual, and each assigner counts its triggers by
    // itself: the fifth key of whole and the first of dual are both a
    // slot's first trigger. Every key of the first five, played again
    // in dual, sounds on a voice of each half.
    for again in &KEYS[..5] {
        let mut jupiter = instrument(&Patch::DEFAULT.with(Param::Vco1Wave, 0));
        for &key in &KEYS[..5] {
            jupiter.event(NOTE_ON, key, 100);
            render(&mut jupiter, 0.05);
            jupiter.event(NOTE_OFF, key, 0);
            render(&mut jupiter, 0.05);
        }
        assert_eq!(jupiter.sounding_voices(), 0);
        jupiter.set_key_mode(KeyMode::Dual);
        jupiter.event(NOTE_ON, *again, 100);
        render(&mut jupiter, 0.05);
        assert_eq!(jupiter.sounding_voices(), 2, "key {again} in dual after whole");
        // And back: the fifth voice of whole after the lower half's first.
        jupiter.event(CONTROL, 120, 0);
        jupiter.set_key_mode(KeyMode::Whole);
        for &key in &KEYS[..5] {
            jupiter.event(NOTE_ON, key, 100);
        }
        render(&mut jupiter, 0.05);
        assert_eq!(jupiter.sounding_voices(), 5, "five keys in whole after dual");
    }
}

#[test]
fn a_key_beyond_the_voices_takes_the_voice_the_hold_has_kept_longest() {
    // Owner's manual, printed page 13: notes under the hold "sustain
    // until that Hold function is removed or until more notes are played
    // than there are voices available. In this case, voices will be taken
    // in the order that they were originally used so long as their keys
    // are not physically held on the keyboard."
    let triangle = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    for mode in [0u8, 3] {
        let mut jupiter = instrument(&triangle.with(Param::AssignMode, mode));
        jupiter.event(CONTROL, 64, 127);
        for &key in &KEYS[..8] {
            jupiter.event(NOTE_ON, key, 100);
            render(&mut jupiter, 0.02);
            jupiter.event(NOTE_OFF, key, 0);
        }
        // The ninth key sounds, in the place of the first; the second
        // goes on.
        jupiter.event(NOTE_ON, KEYS[8], 100);
        let levels = heard(&mut jupiter, &[KEYS[8], KEYS[1], KEYS[0]]);
        assert_eq!(jupiter.sounding_voices(), 8);
        assert!(levels[1].abs() < 1.5 && levels[2] < -40.0, "assign mode {mode}: {levels:?}");
        // A key under a finger keeps its voice: with all eight down, the
        // ninth waits, hold or no hold.
        let mut jupiter = instrument(&triangle.with(Param::AssignMode, mode));
        jupiter.event(CONTROL, 64, 127);
        for &key in &KEYS[..9] {
            jupiter.event(NOTE_ON, key, 100);
        }
        let levels = heard(&mut jupiter, &[KEYS[0], KEYS[8]]);
        assert!(levels[1] < -40.0, "assign mode {mode}: {levels:?}");
    }
    // Solo has one voice: a key played after one the hold keeps takes it,
    // though it is the higher.
    let mut jupiter = instrument(&square().with(Param::AssignMode, 1));
    jupiter.event(CONTROL, 64, 127);
    jupiter.event(NOTE_ON, A2 - 12, 100);
    render(&mut jupiter, 0.1);
    jupiter.event(NOTE_OFF, A2 - 12, 0);
    jupiter.event(NOTE_ON, A2, 100);
    assert!(cents(440.0, pitch(&render(&mut jupiter, 0.5)[samples(0.1)..])).abs() < 1.0);
    jupiter.event(NOTE_OFF, A2, 0);
    jupiter.event(CONTROL, 64, 0);
    render(&mut jupiter, 0.5);
    assert_eq!(jupiter.sounding_voices(), 0);
    // Unison: the ninth key takes the place of the first the hold keeps.
    let mut jupiter = instrument(&triangle.with(Param::AssignMode, 2));
    jupiter.event(CONTROL, 64, 127);
    for &key in &KEYS[..8] {
        jupiter.event(NOTE_ON, key, 100);
        jupiter.event(NOTE_OFF, key, 0);
    }
    jupiter.event(NOTE_ON, KEYS[8], 100);
    let levels = heard(&mut jupiter, &[KEYS[8], KEYS[1], KEYS[0]]);
    assert!(levels[1].abs() < 1.5 && levels[2] < -40.0, "unison: {levels:?}");
}

#[test]
fn the_pedal_holds_the_lower_half_alone_in_split_and_outlasts_a_change_of_mode() {
    // Owner's manual, printed page 13: "In either the Whole or Dual Key
    // Modes, the DP-2 will activate the Hold function for the entire
    // keyboard ... When used with the Split Key Mode, the DP-2 will
    // activate the Hold function on the Lower end of the keyboard only".
    let patch = square().with(Param::Env2Release, 0);
    let (low, high) = (A2 - 24, A2 + 12);
    let tap = |jupiter: &mut Jupiter8, key: u8| {
        jupiter.event(NOTE_ON, key, 100);
        render(jupiter, 0.05);
        jupiter.event(NOTE_OFF, key, 0);
    };
    let mut jupiter = instrument(&patch);
    jupiter.set_lower_patch(&patch);
    jupiter.set_key_mode(KeyMode::Split);
    jupiter.event(CONTROL, 64, 127);
    tap(&mut jupiter, low);
    tap(&mut jupiter, high);
    let held = render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 1, "split: the pedal holds the lower key alone");
    assert!(cents(110.0, pitch(&held[samples(0.1)..])).abs() < 2.0);
    // In dual it holds both halves; going to split lets the upper go.
    jupiter.event(CONTROL, 64, 0);
    jupiter.set_key_mode(KeyMode::Dual);
    jupiter.event(CONTROL, 64, 127);
    tap(&mut jupiter, high);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 2, "dual: the pedal holds both halves");
    jupiter.set_key_mode(KeyMode::Split);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 1, "dual to split: the upper half's hold is let go");
    // The pedal is still down after a change of the key mode or of the
    // assign mode, each of which releases every voice: what is played
    // then is held.
    jupiter.set_key_mode(KeyMode::Whole);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 0, "split to whole releases every voice");
    tap(&mut jupiter, high);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 1, "the pedal holds after a change of the key mode");
    jupiter.set_patch(&patch.with(Param::AssignMode, 3));
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 0, "a change of the assign mode releases every voice");
    tap(&mut jupiter, high);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 1, "the pedal holds after a change of the assign mode");
    jupiter.event(CONTROL, 64, 0);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 0);
}

#[test]
fn the_hold_holds_and_the_controllers_do_what_midi_says() {
    let mut jupiter = instrument(&square().with(Param::Env2Release, 0));
    jupiter.event(CONTROL, 64, 127);
    jupiter.event(NOTE_ON, A2, 100);
    jupiter.event(NOTE_OFF, A2, 0);
    render(&mut jupiter, 0.3);
    assert_eq!(jupiter.sounding_voices(), 1);
    // All notes off leaves what the hold keeps; the hold let go ends it.
    jupiter.event(CONTROL, 123, 0);
    render(&mut jupiter, 0.1);
    assert_eq!(jupiter.sounding_voices(), 1);
    jupiter.event(CONTROL, 64, 0);
    render(&mut jupiter, 0.1);
    assert_eq!(jupiter.sounding_voices(), 0);
    // A note on with no velocity is a note off; all notes off releases
    // keys under fingers; all sound off silences at once.
    jupiter.event(NOTE_ON, A2, 100);
    jupiter.event(NOTE_ON, A2, 0);
    render(&mut jupiter, 0.1);
    assert_eq!(jupiter.sounding_voices(), 0);
    jupiter.event(NOTE_ON, A2, 100);
    jupiter.event(CONTROL, 123, 0);
    render(&mut jupiter, 0.1);
    assert_eq!(jupiter.sounding_voices(), 0);
    jupiter.set_patch(&square().with(Param::Env2Release, 255));
    jupiter.event(NOTE_ON, A2, 100);
    render(&mut jupiter, 0.1);
    jupiter.event(CONTROL, 120, 0);
    assert_eq!((jupiter.sounding_voices(), rms(&render(&mut jupiter, 0.1)[samples(0.02)..]) < 1.0e-4), (0, true));
    // The volume is the square of the controller's share.
    let level = |jupiter: &mut Jupiter8| {
        jupiter.event(NOTE_ON, A2, 100);
        let level = rms(&render(jupiter, 0.3)[samples(0.1)..]);
        jupiter.event(CONTROL, 120, 0);
        level
    };
    let full = level(&mut jupiter);
    jupiter.event(CONTROL, 7, 64);
    let half = level(&mut jupiter);
    assert!((decibels(half, full) - decibels(64.0 * 64.0, 127.0 * 127.0)).abs() < 0.2);
    // Reset all controllers leaves the volume, and puts the bender and
    // the wheel back.
    jupiter.event(PITCH_BEND, 0, 0);
    jupiter.event(CONTROL, 1, 127);
    jupiter.event(CONTROL, 121, 0);
    assert!((decibels(level(&mut jupiter), half)).abs() < 0.2);
    jupiter.event(NOTE_ON, A2, 100);
    assert!(cents(440.0, pitch(&render(&mut jupiter, 0.6)[samples(0.1)..])).abs() < 1.0);
    // reset() keeps the patch and forgets the rest.
    jupiter.reset();
    assert_eq!(
        (jupiter.sounding_voices(), jupiter.ignored_events(), *jupiter.patch()),
        (0, 0, square().with(Param::Env2Release, 255))
    );
    assert!((decibels(level(&mut jupiter), full)).abs() < 0.2);
}

#[test]
fn what_is_no_message_of_the_instrument_is_counted_and_changes_nothing() {
    let mut jupiter = Jupiter8::new();
    for (status, first, second) in
        [(0xc0, 1, 0), (0xa0, 60, 9), (0xd0, 9, 0), (CONTROL, 10, 5), (0xf8, 0, 0), (NOTE_ON, 200, 9)]
    {
        jupiter.event(status, first, second);
    }
    jupiter.event(NOTE_ON, 60, 255);
    jupiter.event(0x12, 3, 4);
    assert_eq!((jupiter.ignored_events(), jupiter.sounding_voices()), (8, 0));
    assert!(render(&mut jupiter, 0.1).iter().all(|&sample| sample == 0.0));
}

#[test]
fn the_wheel_adds_the_oscillators_swing_to_the_pitch() {
    // The bender panel's LFO modulation at the wheel's top: 50 cents
    // either way as the instrument is made, and what the panel says.
    let swing = |performance: &Performance| {
        let mut jupiter = instrument(&square().with(Param::LfoRate, 150));
        jupiter.set_performance(performance);
        jupiter.event(CONTROL, 1, 127);
        jupiter.event(NOTE_ON, A2 + 24, 100);
        render(&mut jupiter, 0.2);
        let track: Vec<f64> =
            render(&mut jupiter, 1.5).chunks(samples(0.005)).map(|chunk| cents(1760.0, pitch(chunk))).collect();
        let inner = &track[2..track.len() - 2];
        (inner.iter().copied().fold(f64::MIN, f64::max), inner.iter().copied().fold(f64::MAX, f64::min))
    };
    let (up, down) = swing(&Performance::DEFAULT);
    println!("the wheel at its top: {down:.1} to {up:.1} cents");
    assert!((up - 50.0).abs() < 4.0 && (down + 50.0).abs() < 4.0);
    let (up, down) = swing(&Performance { lfo_vco: 400, ..Performance::DEFAULT });
    assert!((up - 400.0).abs() < 20.0 && (down + 400.0).abs() < 20.0);
}

#[test]
#[ignore = "a measurement, not a check: run it by name, in a release build, for the figure"]
fn rendering_speed() {
    let seconds = 20usize;
    let mut out = vec![0i16; seconds * SAMPLE_RATE as usize];
    let cross = Patch::DEFAULT
        .with(Param::CrossMod, 128)
        .with(Param::Sync, 1)
        .with(Param::MixVco2, 255)
        .with(Param::VcfResonance, 128);
    // The most work found: the slider at 10 under VCO-2's sine, both
    // oscillators at 2' on the keyboard's top octaves, where VCO-1 is at
    // its bound of 48 kHz for much of every cycle of VCO-2 and starts a
    // cycle in every fourth sample; the pulse's width swept as well.
    let hardest = Patch::DEFAULT
        .with(Param::CrossMod, 255)
        .with(Param::Vco2Wave, 0)
        .with(Param::Vco1Range, 3)
        .with(Param::Vco2Range, 36)
        .with(Param::MixVco2, 255)
        .with(Param::PwmSource, 0)
        .with(Param::PwmDepth, 255)
        .with(Param::LfoRate, 255)
        .with(Param::HpfCutoff, 100)
        .with(Param::VcfResonance, 128);
    for (name, patch, higher) in [
        ("a sawtooth each", Patch::DEFAULT, 0),
        ("LO STRINGS", factory::lo_strings(), 0),
        ("cross modulation and sync", cross, 0),
        ("cross modulation at 10 with both oscillators at 2', four octaves up", hardest, 48),
    ] {
        let mut jupiter = instrument(&patch);
        for &key in &KEYS[..8] {
            jupiter.event(NOTE_ON, key + higher, 100);
        }
        let start = Instant::now();
        jupiter.render(&mut out);
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(jupiter.sounding_voices(), 8);
        println!("eight voices, {name}: {:.1} times real time", seconds as f64 / elapsed);
    }
}
