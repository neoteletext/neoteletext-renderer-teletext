//! The Juno's envelope and low-frequency oscillator against the ranges
//! Roland's specifications give (TODO-6431).

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

use neoteletext_audio_instruments::juno::{Model, Patch, Slider};
use support::{MODELS, flat, peaks, pitch, play, samples, square};

/// Seconds in a window of the level's trace.
const WINDOW: f64 = 0.001;
/// The key played: high, so that every window holds a cycle.
const KEY: u8 = 96;

/// The level of a square wave through the amplifier on the envelope, a
/// reading each millisecond, the key held and then let go.
fn trace(model: Model, held: f64, after: f64, set: impl Fn(Patch) -> Patch) -> Vec<f64> {
    let mut patch = set(flat(model, square()));
    patch.vca_gate = false;
    peaks(&play(model, &patch, KEY, held, after), WINDOW)
}

/// Seconds from `from` until the trace first passes `level`, rising or
/// falling.
fn until(trace: &[f64], from: f64, level: f64, rising: bool) -> f64 {
    let start = (from / WINDOW) as usize;
    let found = trace[start..].iter().position(|&value| (value >= level) == rising);
    found.map_or(f64::INFINITY, |windows| windows as f64 * WINDOW)
}

/// The specifications' shortest and longest attack, and shortest and
/// longest decay and release, in seconds: JUNO-106 service notes, page
/// 1; JUNO-60 service notes, page 1.
fn specified(model: Model) -> ((f64, f64), (f64, f64)) {
    match model {
        Model::Juno106 => ((0.0015, 3.0), (0.0015, 12.0)),
        Model::Juno60 => ((0.001, 3.0), (0.002, 12.0)),
    }
}

#[test]
fn the_attack_runs_from_the_shortest_to_the_longest_the_specifications_give() {
    for model in MODELS {
        let ((shortest, longest), _) = specified(model);
        let slow = trace(model, 4.0, 0.0, |patch| patch.with(Slider::Attack, 127));
        let full = slow[3_500..].iter().fold(0.0f64, |peak, &value| peak.max(value));
        let time = until(&slow, 0.0, 0.99 * full, true);
        println!(
            "{model:?}: longest attack {time:.3} s to the peak, for {longest}; half way at {:.3} s",
            until(&slow, 0.0, 0.5 * full, true)
        );
        assert!((time / longest - 1.0).abs() < 0.03, "{model:?}: {time}");
        // The Juno-106's is a straight line, half way at half the time;
        // the Juno-60's a capacitor's curve, half way sooner.
        let half = until(&slow, 0.0, 0.5 * full, true) / time;
        match model {
            Model::Juno106 => assert!((half - 0.5).abs() < 0.02, "{half}"),
            Model::Juno60 => assert!((0.30..0.40).contains(&half), "{half}"),
        }

        let fast = trace(model, 0.1, 0.0, |patch| patch);
        let time = until(&fast, 0.0, 0.9 * full, true);
        println!("{model:?}: shortest attack {:.1} ms to nine tenths, for {} ms", time * 1e3, shortest * 1e3);
        // The Juno-106's processor writes the level once in 4.2 ms, and
        // 1 ms of resistor and capacitor follows: no attack is shorter.
        let bound = if model == Model::Juno106 { 0.008 } else { 0.003 };
        assert!(time <= bound, "{model:?}: {time}");
    }
}

#[test]
fn the_decay_and_the_release_run_to_twelve_seconds_to_a_tenth() {
    for model in MODELS {
        let (_, (shortest, longest)) = specified(model);
        let decay = trace(model, 14.0, 0.0, |patch| patch.with(Slider::Decay, 127).with(Slider::Sustain, 0));
        let full = decay.iter().fold(0.0f64, |peak, &value| peak.max(value));
        let slow = until(&decay, 0.02, 0.1 * full, false);
        let release = trace(model, 0.5, 14.0, |patch| patch.with(Slider::Release, 127));
        let let_go = until(&release, 0.5, 0.1 * full, false);
        println!("{model:?}: longest decay {slow:.2} s and release {let_go:.2} s to a tenth, for {longest}");
        assert!((slow / longest - 1.0).abs() < 0.03 && (let_go / longest - 1.0).abs() < 0.03, "{model:?}");

        let decay = trace(model, 0.2, 0.0, |patch| patch.with(Slider::Sustain, 0));
        // From the peak, which the attack reaches within a few windows.
        let fast = until(&decay, until(&decay, 0.0, 0.5 * full, true), 0.1 * full, false);
        let release = trace(model, 0.2, 0.2, |patch| patch);
        let quick = until(&release, 0.2, 0.1 * full, false);
        println!(
            "{model:?}: shortest decay {:.1} ms and release {:.1} ms, for {} ms",
            fast * 1e3,
            quick * 1e3,
            shortest * 1e3
        );
        let bound = if model == Model::Juno106 { 0.012 } else { 0.004 };
        assert!(fast <= bound && quick <= bound, "{model:?}: {fast} {quick}");

        // The sustain level is the slider's share of the level held at
        // the slider's top.
        let level = |sustain: u8| {
            let held = trace(model, 1.0, 0.0, |patch| patch.with(Slider::Sustain, sustain));
            held[800..1_000].iter().sum::<f64>()
        };
        let share = level(64) / level(127);
        assert!((share - 64.0 / 127.0).abs() < 0.01, "{model:?}: sustain {share}");
    }
}

/// The rate of the low-frequency oscillator in hertz, read from the
/// pitch it moves: the tone's pitch in stretches of `stretch` seconds, and
/// the time between that pitch's rises through its middle.
fn lfo_rate(model: Model, slider: u8, seconds: f64, stretch: f64) -> f64 {
    let patch = flat(model, square()).with(Slider::LfoRate, slider).with(Slider::DcoLfo, 127);
    let signal = play(model, &patch, KEY, seconds, 0.0);
    let pitches: Vec<f64> = signal.chunks_exact(samples(stretch)).map(|chunk| pitch(chunk).log2()).collect();
    let (low, high) =
        pitches.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), &p| (low.min(p), high.max(p)));
    let (middle, well_below) = (f64::midpoint(low, high), low + (high - low) / 4.0);
    let (mut rises, mut armed) = (Vec::new(), false);
    for (index, pair) in pitches.windows(2).enumerate() {
        if pair[0] < well_below {
            armed = true;
        }
        if armed && pair[0] < middle && pair[1] >= middle {
            armed = false;
            rises.push(index as f64 + (middle - pair[0]) / (pair[1] - pair[0]));
        }
    }
    assert!(rises.len() >= 2, "{model:?} at {slider}: {} cycles in {seconds} s", rises.len());
    (rises.len() - 1) as f64 / ((rises[rises.len() - 1] - rises[0]) * stretch)
}

#[test]
fn the_oscillator_runs_from_the_lowest_to_the_highest_rate_the_specifications_give() {
    // JUNO-106 service notes, page 1: 0.1 Hz to 30 Hz. JUNO-60 service
    // notes, page 1: 0.3 Hz to 20 Hz.
    for (model, lowest, highest, seconds) in [(Model::Juno106, 0.1, 30.0, 24.0), (Model::Juno60, 0.3, 20.0, 9.0)] {
        let (slow, fast) = (lfo_rate(model, 0, seconds, 0.1), lfo_rate(model, 127, 2.0, 0.004));
        println!("{model:?}: oscillator {slow:.4} Hz to {fast:.3} Hz, for {lowest} to {highest}");
        assert!((slow / lowest - 1.0).abs() < 0.02 && (fast / highest - 1.0).abs() < 0.02, "{model:?}");
    }
}

#[test]
fn the_oscillator_waits_for_its_delay_after_a_key() {
    // JUNO-106 service notes, page 1: "DELAY TIME 0 to 3s". JUNO-60
    // service notes, adjustment 7-4: at 10, "2 seconds later, begins to
    // reappear".
    for (model, delay) in [(Model::Juno106, 3.0), (Model::Juno60, 2.0)] {
        let patch =
            flat(model, square()).with(Slider::LfoRate, 100).with(Slider::DcoLfo, 127).with(Slider::LfoDelay, 127);
        let signal = play(model, &patch, 69, delay + 2.5, 0.0);
        // How far the pitch swings in a stretch: the spread of the
        // tone's cycle lengths.
        let swing = |from: f64, to: f64| {
            let stretch = &signal[samples(from)..samples(to)];
            let mut crossings = Vec::new();
            for (index, pair) in stretch.windows(2).enumerate() {
                if pair[0] < 0.0 && pair[1] >= 0.0 {
                    crossings.push(index as f64 + pair[0] / (pair[0] - pair[1]));
                }
            }
            let periods: Vec<f64> = crossings.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let (low, high) = periods
                .iter()
                .fold((f64::INFINITY, 0.0f64), |(low, high), &period| (low.min(period), high.max(period)));
            1200.0 * (high / low).log2()
        };
        let (before, after) = (swing(0.2, delay - 0.1), swing(delay + 1.6, delay + 2.4));
        println!("{model:?}: the pitch swings {before:.1} cents before {delay} s and {after:.0} cents after the rise");
        assert!(before < 2.0, "{model:?}: {before}");
        // The whole swing: twice 400 cents on the Juno-106, twice 300 on
        // the Juno-60.
        let whole = if model == Model::Juno106 { 800.0 } else { 600.0 };
        assert!((after / whole - 1.0).abs() < 0.05, "{model:?}: {after}");
    }
}
