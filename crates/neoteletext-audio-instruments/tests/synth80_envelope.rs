//! synth80's two envelopes against Yamaha's figures: the amplifier's
//! attack of 1 ms to 1 s with the adjustment procedure's two points, its
//! decay and release of 10 ms to 10 s and its sustain; the filters'
//! initial level, attack level and return; the sustain lever and its two
//! kinds (TODO-6434).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth80")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64 and on indices it derives itself"
)]

mod synth80_support;

use neoteletext_audio_instruments::synth80::{Control, Global, Patch};
use synth80_support::{CONTROL, NOTE_OFF, NOTE_ON, SINE, envelope, instrument, panel, reaches, render, samples};

/// The key the levels are measured on: 3,520 Hz, a cycle of 14 samples.
const KEY: u8 = 105;
/// Samples the levels are taken over: two cycles.
const WINDOW: usize = 27;
/// The sine's steady level, of full scale.
const FULL: f64 = 0.125;

/// The sine with the amplifier's envelope set.
fn shaped(attack: u8, decay: u8, sustain: u8, release: u8) -> Patch {
    let mut patch = panel(&SINE);
    for (control, value) in
        [(Control::Attack, attack), (Control::Decay, decay), (Control::Sustain, sustain), (Control::Release, release)]
    {
        patch.tones[0].set(control, value);
    }
    patch
}

/// The level through time of a key held `held` seconds and let go for
/// `after`.
fn levels(patch: &Patch, held: f64, after: f64) -> Vec<f64> {
    envelope(&synth80_support::play(patch, KEY, 127, held, after), WINDOW)
}

/// What the documents give a time lever: the shortest time doubled for
/// each of its ten volts.
fn time(shortest: f64, lever: u8) -> f64 {
    shortest * (10.0 * f64::from(lever) / 127.0).exp2()
}

#[test]
fn the_attack_is_a_millisecond_to_a_second_through_the_adjustments_two_points() {
    // Adjustment procedure, page 13, step 2: 8 V, 4 ms; 3 V, 125 ms. The
    // lever counts from SHORT, 10 V: 25 of 127 is 8.03 V, 89 is 2.99 V.
    for (lever, stated) in [(0u8, 0.001), (25, 0.004), (89, 0.125), (127, 1.0)] {
        let found = levels(&shaped(lever, 0, 127, 0), 1.5, 0.0);
        let start = reaches(&found, 0, 0.02 * FULL, true);
        let peak = reaches(&found, 0, 0.99 * FULL, true);
        let taken = peak - start;
        println!(
            "attack lever {lever}: {:.2} ms for Yamaha's {:.0} ms and the law's {:.2} ms",
            taken * 1e3,
            stated * 1e3,
            time(1.0, lever)
        );
        // The level is taken over half a millisecond, and the first 2%
        // and the last 1% of the rise are left out.
        assert!((taken - time(0.001, lever)).abs() < 0.0006 + 0.06 * time(0.001, lever), "{lever}: {taken}");
        assert!((taken / stated - 1.0).abs() < 0.15 || lever == 0, "{lever}: {taken} s");
    }
}

#[test]
fn the_decay_and_the_release_are_ten_milliseconds_to_ten_seconds() {
    // Service manual, page 52, VCA-EG, pins 10 and 11: "from 10 mS until
    // 10 S". The time is taken to a tenth of the way.
    for lever in [0u8, 64, 100, 127] {
        let expected = time(0.010, lever);
        let decay = levels(&shaped(0, lever, 0, 0), expected * 1.3 + 0.1, 0.0);
        // The peak is where the level is largest: a decay of 10 ms has
        // taken a hundredth of it within the two cycles a level is
        // taken over.
        let peak = decay
            .iter()
            .enumerate()
            .fold((0, 0.0), |peak, (at, &level)| if level > peak.1 { (at, level) } else { peak });
        assert!(peak.1 > 0.97 * FULL, "{lever}: a peak of {}", peak.1);
        let fallen = reaches(&decay, peak.0, 0.1 * FULL, false);
        let held = 0.05;
        let release = levels(&shaped(0, 0, 127, lever), held, expected * 1.3 + 0.1);
        let released = reaches(&release, samples(held), 0.1 * FULL, false);
        println!(
            "lever {lever}: decay {:.1} ms, release {:.1} ms, for the law's {:.1} ms",
            fallen * 1e3,
            released * 1e3,
            expected * 1e3
        );
        for found in [fallen, released] {
            assert!((found - expected).abs() < 0.0015 + 0.03 * expected, "{lever}: {found} s for {expected} s");
        }
    }
}

#[test]
fn the_sustain_is_the_levers_share_of_the_peak() {
    for lever in [0u8, 32, 64, 127] {
        let found = levels(&shaped(0, 20, lever, 0), 0.6, 0.0);
        let level = found[found.len() - 1] / FULL;
        println!("sustain lever {lever}: {level:.3} of the peak");
        assert!((level - f64::from(lever) / 127.0).abs() < 0.005);
    }
}

/// Noise through the low-pass filter at its highest Q, the amplifier
/// open long after the key: a band whose middle is the filter's place.
fn band(initial: u8, peak: u8, attack: u8, decay: u8, release: u8) -> Patch {
    let mut patch = panel(&[
        (Control::Sawtooth, 0),
        (Control::Noise, 127),
        (Control::Lpf, 52),
        (Control::LpfResonance, 127),
        (Control::InitialLevel, initial),
        (Control::AttackLevel, peak),
        (Control::FilterAttack, attack),
        (Control::FilterDecay, decay),
        (Control::FilterRelease, release),
        (Control::Release, 127),
    ]);
    patch.set(Global::Resonance, 127);
    patch
}

/// The strongest frequency of a piece of a signal between 100 Hz and 12
/// kHz, in steps of a fiftieth of an octave.
fn strongest(piece: &[f64]) -> f64 {
    let mut best = (0.0, 0.0);
    let mut hertz = 100.0f64;
    while hertz < 12_000.0 {
        let level = synth80_support::component(piece, hertz);
        if level > best.0 {
            best = (level, hertz);
        }
        hertz *= (1.0f64 / 50.0).exp2();
    }
    best.1
}

/// The middle of the band in hertz through time, in windows of 50 ms: a
/// key held `held` seconds and let go for `after`.
fn places(patch: &Patch, held: f64, after: f64) -> Vec<f64> {
    synth80_support::play(patch, 60, 127, held, after).chunks(2_400).map(strongest).collect()
}

/// The middle of a few windows about a time, in octaves from `steady`:
/// the median, since a window of noise may peak anywhere in the band.
fn place(found: &[f64], steady: f64, seconds: f64) -> f64 {
    let index = (seconds / 0.05) as usize;
    let mut near: Vec<f64> = found[index.saturating_sub(2)..(index + 3).min(found.len())].to_vec();
    near.sort_by(f64::total_cmp);
    (near[near.len() / 2] / steady).log2()
}

#[test]
fn the_low_pass_filter_starts_at_the_initial_level_rises_to_the_attack_level_and_comes_back() {
    // The band's place with no envelope.
    let rest = places(&band(0, 0, 0, 0, 0), 2.0, 0.0);
    let mut sorted = rest[4..].to_vec();
    sorted.sort_by(f64::total_cmp);
    let steady = sorted[sorted.len() / 2];
    println!("the band at rest: {steady:.0} Hz");
    // Service manual, page 42, VCF-EG, pins 14 and 16: the levels are 0
    // to +5 V and 0 to -5 V; page 41: the envelope reaches the low-pass
    // filter through 47 kΩ, 1.014 octaves to the volt. 51 of 127 is 2.01
    // V, 2.04 octaves.
    let octaves = 5.0 * 51.0 / 127.0 * (39.0 / 47.0) * (3.3 / 2.7);
    // The attack lever at 114 is 0.5 s to the peak; the decay and the
    // release at 64 are 0.33 s to a tenth of the way.
    let found = places(&band(51, 51, 114, 64, 64), 4.0, 3.0);
    let at = |seconds: f64| place(&found, steady, seconds);
    println!(
        "IL and AL at 2 V: {:.2} octaves at the start, {:.2} at 0.5 s, {:.2} at rest, {:.2} after the release",
        at(0.0),
        at(0.5),
        at(3.8),
        at(6.8)
    );
    // The first windows are already a fifth of the way up the attack.
    assert!(at(0.0) < -0.7 && at(0.0) > -octaves);
    assert!((at(0.5) - octaves).abs() < 0.35);
    assert!(at(3.8).abs() < 0.15 && (at(6.8) + octaves).abs() < 0.15);
    // With the levels at nothing the times do nothing.
    let still = places(&band(0, 0, 127, 127, 127), 2.0, 0.0);
    assert!(place(&still, steady, 0.3).abs() < 0.15 && place(&still, steady, 1.5).abs() < 0.15);
}

#[test]
fn the_envelope_moves_the_high_pass_filter_by_39_hundredths_of_a_volt() {
    // A sawtooth's fundamental far below the high-pass filter's cutoff
    // falls 12 dB for each octave the cutoff rises. The attack level at
    // its top is 5 V, 1.95 octaves, held by the longest decay.
    let level = |peak: u8| {
        let patch = panel(&[
            (Control::Hpf, 90),
            (Control::AttackLevel, peak),
            (Control::FilterDecay, 127),
            (Control::Lpf, 127),
        ]);
        let signal = synth80_support::play(&patch, 48, 127, 0.3, 0.0);
        synth80_support::decibels(synth80_support::component(&signal[4_800..], 130.81))
    };
    let moved = level(0) - level(127);
    println!("AL at +5 on the high-pass filter: the fundamental falls {moved:.1} dB");
    assert!((moved - 1.95 * 12.04).abs() < 1.5);
}

#[test]
fn the_foot_switch_gives_the_release_the_sustain_levers_time() {
    // Owner's manual, page 11: the SUSTAIN lever is "from SHORT (no
    // sustain) to LONG (about 10 seconds)", and acts while the foot
    // switch is down.
    let tail = |pedal: u8, sustain: u8, release: u8| {
        let mut patch = shaped(0, 0, 127, release);
        patch.set(Global::Sustain, sustain);
        let mut synth80 = instrument(&patch);
        synth80.event(CONTROL, 64, pedal);
        synth80.event(NOTE_ON, KEY, 127);
        render(&mut synth80, 0.05);
        synth80.event(NOTE_OFF, KEY, 0);
        reaches(&envelope(&render(&mut synth80, 3.0), WINDOW), WINDOW, 0.1 * FULL, false)
    };
    let (up, down) = (tail(0, 100, 0), tail(127, 100, 0));
    println!("SUSTAIN at 100 of 127: {:.0} ms with the switch up, {:.0} ms with it down", up * 1e3, down * 1e3);
    assert!((up - 0.010).abs() < 0.002 && (down / time(0.010, 100) - 1.0).abs() < 0.03);
    // It never shortens a release.
    assert!((tail(127, 0, 64) / time(0.010, 64) - 1.0).abs() < 0.03);
}

#[test]
fn a_sustain_of_the_second_kind_ends_when_the_next_key_is_played() {
    let run = |mode: u8| {
        let mut patch = shaped(0, 0, 127, 110);
        patch.set(Global::SustainMode, mode);
        let mut synth80 = instrument(&patch);
        synth80.event(NOTE_ON, KEY, 127);
        render(&mut synth80, 0.05);
        synth80.event(NOTE_OFF, KEY, 0);
        render(&mut synth80, 0.05);
        // The next key is too low to be in the level's window.
        synth80.event(NOTE_ON, KEY - 24, 127);
        render(&mut synth80, 0.1);
        synth80.sounding_voices()
    };
    assert_eq!((run(0), run(1)), (2, 1));
}
