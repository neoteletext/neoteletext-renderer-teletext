//! The Juno's chorus: its sweep's rates against the Juno-60's schematic
//! and Anwander's figures, its delays, and how the mono output mixes the
//! two lines with the dry signal (TODO-6431).

// Without the feature there is nothing here to test.
#![cfg(feature = "juno")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "juno_support.rs"]
mod support;

use neoteletext_audio_instruments::juno::{ChorusMode, Model, Patch, Range, Slider};
use support::{CONTROL, MODELS, NOTE_ON, RATE, flat, instrument, play, render, rms, samples};

/// Seconds between two readings of the delays, and the length of signal
/// each is read from.
const HOP: f64 = 0.005;
const WINDOW: f64 = 0.02;
/// The longest delay looked for, in seconds: beyond the chip's 12.8 ms.
const LONGEST: f64 = 0.014;
/// The shortest: below the chip's 0.64 ms, and clear of a click's own
/// width.
const SHORTEST: f64 = 0.0005;

/// A patch through an open filter and the chorus, the amplifier on the
/// gate.
fn through(model: Model, chorus: ChorusMode, patch: Patch) -> Patch {
    let mut patch = flat(model, patch);
    patch.sawtooth = false;
    patch.vca_gate = true;
    patch.chorus = chorus;
    patch
}

/// Clicks: the edges of a square wave of 32.7 Hz through the high-pass
/// filter at its highest, 15 ms apart, each followed by its echoes.
fn clicks(model: Model, chorus: ChorusMode, seconds: f64) -> Vec<f64> {
    let mut patch = through(model, chorus, Patch::DEFAULT);
    patch.pulse = true;
    patch.range = Range::Sixteen;
    patch.hpf = 3;
    play(model, &patch, 36, seconds, 0.0).split_off(samples(0.5))
}

/// Noise alone, at full level, with the filter's cutoff at `cutoff`.
fn noise(model: Model, chorus: ChorusMode, cutoff: u8, seconds: f64) -> Vec<f64> {
    let patch = through(model, chorus, Patch::DEFAULT.with(Slider::Noise, 127).with(Slider::VcfFrequency, cutoff));
    play(model, &patch, 60, seconds, 0.0).split_off(samples(0.5))
}

/// How alike a signal is to itself `lag` samples later, as a share of
/// its power: an echo of gain g of white noise gives g / (1 + g²).
fn likeness(signal: &[f64], lag: usize) -> f64 {
    let sum: f64 = signal.iter().zip(&signal[lag..]).map(|(a, b)| a * b).sum();
    sum / signal.iter().map(|a| a * a).sum::<f64>()
}

/// The delay, in milliseconds, of the strongest echo between two lags in
/// each stretch of the signal, with its likeness.
fn echoes(signal: &[f64], from: f64, to: f64) -> Vec<(f64, f64)> {
    let (hop, window) = (samples(HOP), samples(WINDOW));
    let mut found = Vec::new();
    let mut start = 0;
    while start + window <= signal.len() {
        let stretch = &signal[start..start + window];
        let strongest = (samples(from)..samples(to))
            .map(|lag| (lag, likeness(stretch, lag)))
            .fold((0, 0.0), |best, next| if next.1 > best.1 { next } else { best });
        found.push((strongest.0 as f64 * 1000.0 / RATE, strongest.1));
        start += hop;
    }
    found
}

/// The delays alone, in milliseconds: where a stretch shows no echo half
/// as strong as the strongest (the echo sought has left the lags looked
/// at), the delay last seen stands.
fn delays(signal: &[f64], from: f64, to: f64) -> Vec<f64> {
    let found = echoes(signal, from, to);
    let strongest = found.iter().fold(0.0f64, |a, &(_, likeness)| a.max(likeness));
    let mut last = found.iter().find(|&&(_, likeness)| likeness > strongest / 2.0).map_or(0.0, |&(delay, _)| delay);
    found
        .iter()
        .map(|&(delay, likeness)| {
            if likeness > strongest / 2.0 {
                last = delay;
            }
            last
        })
        .collect()
}

/// The lines' filters delay an echo by this much besides, in
/// milliseconds: measured, about three samples.
const FILTERS: f64 = 0.075;
/// The middle delay and the sweep either way, in milliseconds: an own
/// choice of the instrument's.
const MIDDLE: f64 = 3.5 + FILTERS;
const SWEEP: f64 = 1.85;

/// The rate in hertz at which a sequence of readings, [`HOP`] apart,
/// rises through its middle.
fn rate(readings: &[f64]) -> f64 {
    let (low, high) = readings.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &r| (low.min(r), high.max(r)));
    let (middle, well_below) = (f64::midpoint(low, high), low + (high - low) / 4.0);
    let (mut rises, mut armed) = (Vec::new(), false);
    for (index, pair) in readings.windows(2).enumerate() {
        if pair[0] < well_below {
            armed = true;
        }
        if armed && pair[0] < middle && pair[1] >= middle {
            armed = false;
            rises.push(index as f64 + (middle - pair[0]) / (pair[1] - pair[0]));
        }
    }
    assert!(rises.len() >= 2, "{} rises", rises.len());
    (rises.len() - 1) as f64 / ((rises[rises.len() - 1] - rises[0]) * HOP)
}

#[test]
fn chorus_off_is_the_dry_signal_alone() {
    for model in MODELS {
        let dry = noise(model, ChorusMode::Off, 127, 1.5);
        let strongest = (samples(SHORTEST)..samples(LONGEST)).map(|lag| likeness(&dry, lag)).fold(0.0, f64::max);
        assert!(strongest < 0.03, "{model:?}: an echo of {strongest}");
    }
}

#[test]
fn chorus_one_and_two_sweep_two_lines_oppositely_at_their_rates() {
    // The Juno-60's chorus oscillator: I, one cycle in 2 s; II, in 1.2 s
    // (Anwander's schematic and page: 0.5 Hz and 0.8 Hz). The delays
    // are an own choice: 3.5 ms, swept 1.85 ms either way.
    for model in MODELS {
        for (mode, hertz) in [(ChorusMode::One, 0.5), (ChorusMode::Two, 1.0 / 1.2)] {
            let wet = clicks(model, mode, 6.5);
            // One line is above the middle delay while the other is
            // below: each of the two is the sweep folded over, and rises
            // through its middle twice in a cycle.
            // The two echoes are also each other's echo, at a lag of up
            // to twice the sweep: the longer is looked for beyond that.
            let longer = delays(&wet, (2.0 * SWEEP + 0.1) / 1000.0, LONGEST);
            let shorter = delays(&wet, 2.0 * SHORTEST, (MIDDLE - 0.1) / 1000.0);
            let longest = longer.iter().fold(0.0f64, |a, &b| a.max(b));
            let shortest = shorter.iter().fold(f64::INFINITY, |a, &b| a.min(b));
            let found = rate(&shorter) / 2.0;
            println!("{model:?} {mode:?}: delays from {shortest:.2} ms to {longest:.2} ms, swept at {found:.3} Hz");
            assert!((found / hertz - 1.0).abs() < 0.03, "{model:?} {mode:?}: {found} Hz");
            let ends = ((shortest - (MIDDLE - SWEEP)).abs(), (longest - (MIDDLE + SWEEP)).abs());
            assert!(ends.0 < 0.08 && ends.1 < 0.08, "{model:?} {mode:?}: {shortest} to {longest}");
            // Opposite: where one is longest the other is shortest.
            let at_longest = longer.iter().position(|&delay| delay >= longest).unwrap_or(0);
            let there = shorter[at_longest];
            assert!((there - shortest).abs() < 0.15, "{model:?} {mode:?}: {there} ms where the other is longest");
        }
    }
}

#[test]
fn each_line_is_47_parts_to_the_dry_signals_78_in_the_mono_output() {
    // JUNO-106 service notes, jack board: 47 kΩ for the dry signal and
    // 39 kΩ for a line in each output, and the mono jack the mean of the
    // two outputs: each line g = 47 / 78 of the dry signal. Noise that
    // lies below the lines' filters, and that is unlike itself a
    // millisecond later, comes out with the three's powers added:
    // 1 + 2 g² is 2.38 dB.
    for (model, cutoff) in [(Model::Juno106, 84), (Model::Juno60, 62)] {
        let wet = noise(model, ChorusMode::One, cutoff, 8.5);
        let dry = noise(model, ChorusMode::Off, cutoff, 8.5);
        let louder = 20.0 * (rms(&wet) / rms(&dry)).log10();
        println!("{model:?}: the chorus makes noise below 2 kHz {louder:.2} dB louder");
        assert!((louder - 2.38).abs() < 0.3, "{model:?}: {louder} dB");
    }
}

#[test]
fn both_buttons_of_the_juno_60_sweep_both_lines_alike_a_little_and_fast() {
    // Anwander's schematic of the Juno-60's chorus oscillator: 1.6 V
    // against 20 V, 8% of the sweep; its cycle is read as 0.1 s; and
    // "the second BBD is controlled in a non inverted way".
    let wet = clicks(Model::Juno60, ChorusMode::Both, 3.5);
    let both = delays(&wet, 2.0 * SHORTEST, LONGEST);
    let (low, high) = both.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &d| (low.min(d), high.max(d)));
    let found = rate(&both);
    println!("Juno60 Both: one delay from {low:.2} ms to {high:.2} ms, swept at {found:.2} Hz");
    // One echo, of both lines together: 3.5 ms, 0.148 ms either way.
    assert!(
        (low - (MIDDLE - 0.08 * SWEEP)).abs() < 0.04 && (high - (MIDDLE + 0.08 * SWEEP)).abs() < 0.04,
        "{low} to {high}"
    );
    assert!((found / 10.0 - 1.0).abs() < 0.05, "{found} Hz");
    // The Juno-106 has no such setting, and takes it as II: two echoes.
    let wet = clicks(Model::Juno106, ChorusMode::Both, 2.0);
    let longest = delays(&wet, (2.0 * SWEEP + 0.1) / 1000.0, LONGEST).iter().fold(0.0f64, |a, &delay| a.max(delay));
    assert!(longest > 5.0, "{longest}");
}

#[test]
fn a_chorus_switched_on_again_brings_back_nothing_of_what_it_last_held() {
    // A note through the chorus; the chorus switched off and the note
    // silenced; then, out of silence, the chorus switched on again. The
    // lines were not run while it was off (TODO-6438: they held the note
    // and played 5 ms of it back, at half its level).
    for model in MODELS {
        let off = flat(model, Patch::DEFAULT);
        let mut on = off;
        on.chorus = ChorusMode::One;
        let mut juno = instrument(model, &on);
        juno.event(NOTE_ON, 57, 100);
        let peak = |signal: &[f64]| signal.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
        let loud = peak(&render(&mut juno, 0.5));
        assert!(loud > 0.1, "{model:?}: {loud}");
        juno.set_patch(&off);
        juno.event(CONTROL, 120, 0);
        assert!(render(&mut juno, 1.0)[samples(0.5)..].iter().all(|&sample| sample == 0.0), "{model:?}: not silent");
        juno.set_patch(&on);
        let back = peak(&render(&mut juno, 0.1));
        println!("{model:?}: the chorus switched on in silence gives a peak of {back:.5}, after a note of {loud:.3}");
        assert!(back == 0.0, "{model:?}: {back}");
    }
}
