//! The Prophet-5's modulation: the low-frequency oscillator's rate and
//! shapes, Wheel-Mod's depths through the manuals' resistors, Poly-Mod
//! from the filter envelope and from oscillator B, pulse width, the
//! glide and unison.

#![cfg(feature = "prophet5")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64"
)]

mod prophet5_support;

use neoteletext_audio_instruments::prophet5::patch::dial;
use neoteletext_audio_instruments::prophet5::{Knob, Model, Patch, Switch};
use prophet5_support::{
    CONTROL, MODELS, NOTE_OFF, NOTE_ON, amplitude, cents, cycles, instrument, pitch, plain, play, pure, render, rms,
    samples,
};

/// A high key, so that the pitch is read often.
const KEY: u8 = 84;

/// The pitch of a patch's key over time with the wheel fully up, in
/// cents from the pitch with the wheel down.
fn wobble(model: Model, patch: &Patch, seconds: f64) -> Vec<(f64, f64)> {
    let mut prophet = instrument(model, patch);
    prophet.event(NOTE_ON, KEY, 100);
    let base = pitch(&render(&mut prophet, 0.5));
    prophet.event(CONTROL, 1, 127);
    render(&mut prophet, 0.05);
    cycles(&render(&mut prophet, seconds)).into_iter().map(|(time, hertz)| (time, cents(base, hertz))).collect()
}

/// The lowest and the highest of a track, but for the thirtieth of its
/// cycles at either end (a cycle in which the pitch jumps reads as
/// neither side of the jump), and how many times a second it rises
/// through the middle between them.
fn swing(track: &[(f64, f64)]) -> (f64, f64, f64) {
    let mut sorted: Vec<f64> = track.iter().map(|&(_, value)| value).collect();
    sorted.sort_by(f64::total_cmp);
    let (low, high) = (sorted[sorted.len() / 30], sorted[sorted.len() - 1 - sorted.len() / 30]);
    let (middle, quarter) = (f64::midpoint(low, high), (high - low) / 4.0);
    let (mut armed, mut crossings) = (false, Vec::new());
    for &(time, value) in track {
        if value < middle - quarter {
            armed = true;
        }
        if armed && value > middle + quarter {
            armed = false;
            crossings.push(time);
        }
    }
    let rate = match (crossings.first(), crossings.last()) {
        (Some(first), Some(last)) if crossings.len() > 1 => (crossings.len() - 1) as f64 / (last - first),
        _ => 0.0,
    };
    (low, high, rate)
}

/// Wheel-Mod of one shape to oscillator A's frequency.
fn vibrato(model: Model, shape: Switch, frequency: u8) -> Patch {
    pure(model)
        .with(Knob::LfoFrequency, frequency)
        .with_switch(Switch::LfoTriangle, false)
        .with_switch(shape, true)
        .with_switch(Switch::WheelFrequencyA, true)
}

#[test]
fn the_low_frequency_oscillator_runs_at_the_documents_rates() {
    // OM section 2-9: "approximatley 0.04 Hz ... to 20 Hz"; TM3 SD334: 110
    // kΩ, 0.909 octave to the volt. Revision 2: 5 Hz at 6 on the dial
    // (patch sheet 1-1) and 0.75 octave to the volt (TM2 SD312).
    for (model, steps) in [(Model::Rev3, [72u8, 96, 120]), (Model::Rev2, [72, 96, 127])] {
        for steps in steps {
            let expected = match model {
                Model::Rev3 => 0.04 * (f64::from(steps) / 12.0 * 100.0 / 110.0).exp2(),
                Model::Rev2 => 5.0 * ((f64::from(steps) - 72.0) / 12.0 * 0.75).exp2(),
            };
            let (_, _, rate) =
                swing(&wobble(model, &vibrato(model, Switch::LfoTriangle, steps), 30.0 / expected.max(3.0)));
            println!("{model:?} at {steps} steps: {rate:.3} Hz (the documents: {expected:.3})");
            assert!((rate / expected - 1.0).abs() < 0.01, "{model:?} {steps}: {rate} for {expected}");
        }
    }
}

#[test]
fn wheel_mod_bends_pitch_through_182_kilohms_and_the_shapes_stand_as_the_chip_makes_them() {
    // One unit of Wheel-Mod is 100 / 182 octave, 659 cents: the sawtooth
    // from 0 to that, the square from 0 to its level against the
    // sawtooth, the triangle a quarter either way.
    for model in MODELS {
        let unit = 1200.0 * 100.0 / 182.0;
        let square = if model == Model::Rev3 { 1.08 } else { 0.75 };
        for (shape, low, high) in [
            (Switch::LfoTriangle, -0.25 * unit, 0.25 * unit),
            (Switch::LfoSawtooth, 0.0, unit),
            (Switch::LfoSquare, 0.0, square * unit),
        ] {
            let (found_low, found_high, _) = swing(&wobble(model, &vibrato(model, shape, 96), 1.5));
            println!("{model:?} {shape:?}: {found_low:.1} to {found_high:.1} cents (expected {low:.1} to {high:.1})");
            assert!((found_low - low).abs() < 35.0 && (found_high - high).abs() < 35.0, "{model:?} {shape:?}");
        }
        // The wheel down, nothing moves; the source mix at noise, the
        // pitch wanders with no period.
        let mut prophet = instrument(model, &vibrato(model, Switch::LfoTriangle, 96));
        prophet.event(NOTE_ON, KEY, 100);
        render(&mut prophet, 0.3);
        let still = cycles(&render(&mut prophet, 0.5));
        let (low, high) =
            still.iter().fold((f64::MAX, f64::MIN), |(low, high), &(_, hertz)| (low.min(hertz), high.max(hertz)));
        assert!(cents(low, high) < 5.0, "{}", cents(low, high));
        let noisy = wobble(model, &vibrato(model, Switch::LfoTriangle, 96).with(Knob::SourceMix, 127), 1.5);
        let (low, high, _) = swing(&noisy);
        println!("{model:?} noise: {low:.1} to {high:.1} cents");
        assert!(high - low > 100.0);
    }
}

#[test]
fn poly_mod_from_the_filter_envelope_raises_oscillator_a_through_301_kilohms() {
    // Five volts of envelope (own choice) through 301 kΩ: 1.661 octaves
    // at the top of the amount; half the amount at full sustain is half.
    for model in MODELS {
        let patch = pure(model)
            .with(Knob::Cutoff, 0)
            .with_switch(Switch::FilterKeyboard, false)
            .with(Knob::Cutoff, 100)
            .with(Knob::PolyModFilterEnvelope, 60)
            .with_switch(Switch::PolyModFrequencyA, true);
        let base = pitch(&play(model, &pure(model), 60, 0.6, 0.0)[samples(0.3)..]);
        let raised = pitch(&play(model, &patch, 60, 0.6, 0.0)[samples(0.3)..]);
        let found = cents(base, raised);
        println!("{model:?}: half the filter envelope's amount raises oscillator A {found:.1} cents (996.7)");
        assert!((found - 1200.0 * 5.0 / 3.01 / 2.0).abs() < 2.0, "{found}");
    }
}

#[test]
fn poly_mod_from_oscillator_b_sweeps_oscillator_a_and_the_filter() {
    for model in MODELS {
        // Oscillator B slow and off the keyboard, its sawtooth to
        // oscillator A's frequency: from nothing up to 3.5 V through
        // 301 kΩ (revision 2: 300 kΩ, and the node's 0.952 of it).
        let slow = plain(model)
            .with_switch(Switch::OscBSawtooth, true)
            .with_switch(Switch::OscBLowFrequency, true)
            .with_switch(Switch::OscBKeyboard, false)
            .with(Knob::OscBFrequency, 40)
            .with(Knob::PolyModOscB, 120);
        let base = pitch(&play(model, &slow, KEY, 0.5, 0.0)[samples(0.2)..]);
        let track: Vec<(f64, f64)> =
            cycles(&play(model, &slow.with_switch(Switch::PolyModFrequencyA, true), KEY, 8.0, 0.0))
                .into_iter()
                .map(|(time, hertz)| (time, cents(base, hertz)))
                .collect();
        let (low, high, rate) = swing(&track[track.len() / 4..]);
        let share = if model == Model::Rev3 { 1.0 } else { 100.0 / 105.0 };
        let expected = 1200.0 * 3.5 / 3.01 * share;
        println!(
            "{model:?}: oscillator B's sawtooth sweeps A from {low:.0} to {high:.0} cents at {rate:.2} Hz (0 to {expected:.0})"
        );
        assert!(low.abs() < 80.0 && (high - expected).abs() < 80.0 && rate > 0.5, "{model:?}");

        // At audio rate into the frequency it makes sidebands: tones
        // that are no harmonic of the note.
        let clang = plain(model)
            .with_switch(Switch::OscBTriangle, true)
            .with(Knob::OscBFrequency, model.frequency_knob(31))
            .with(Knob::PolyModOscB, 120);
        let quiet = play(model, &clang, 69, 1.0, 0.0);
        let loud = play(model, &clang.with_switch(Switch::PolyModFrequencyA, true), 69, 1.0, 0.0);
        // The difference tone of a fifth, about an octave below the
        // note: the strongest tone from 190 to 250 Hz.
        let strongest = |signal: &[f64]| {
            (190..250).map(|hertz| amplitude(&signal[samples(0.5)..], f64::from(hertz))).fold(0.0f64, f64::max)
        };
        let (before, after) = (strongest(&quiet), strongest(&loud));
        println!("{model:?}: the tone an octave below, without and with Poly-Mod: {before:.5} and {after:.5}");
        assert!(after > 10.0 * before.max(1.0e-5));

        // Into the filter, a slow oscillator B opens and closes it.
        let sweep = slow.with(Knob::Cutoff, 24).with_switch(Switch::PolyModFilter, true);
        let levels = prophet5_support::peaks(&play(model, &sweep, KEY, 5.0, 0.0)[samples(1.0)..], 0.01);
        let (least, most) =
            levels.iter().fold((f64::MAX, 0.0f64), |(least, most), &level| (least.min(level), most.max(level)));
        println!("{model:?}: through the filter the level swings from {least:.5} to {most:.4}");
        assert!(most > 30.0 * least);
    }
}

#[test]
fn the_pulse_is_square_at_five_and_gone_at_the_ends() {
    // OM section 2-3: a square "by setting the knob to approximately 5";
    // "At the extreme knob settings the pulses ... degenerate to dc".
    for model in MODELS {
        let pulse = |width: u8| {
            let patch = plain(model)
                .with_switch(Switch::OscASawtooth, false)
                .with_switch(Switch::OscAPulse, true)
                .with(Knob::OscAPulseWidth, width);
            let signal = play(model, &patch, 57, 0.6, 0.0);
            let tone = &signal[samples(0.3)..];
            (rms(tone), amplitude(tone, 440.0) / amplitude(tone, 220.0).max(1.0e-9))
        };
        let ((level, second), (narrow, _), (none, _), (full, _)) =
            (pulse(dial(50)), pulse(dial(10)), pulse(0), pulse(127));
        println!(
            "{model:?}: at 5 the second harmonic is {second:.3} of the first; level at 5 {level:.4}, at 1 {narrow:.4}, at 0 {none:.6}, at the top {full:.6}"
        );
        assert!(second < 0.1 && narrow < level && narrow > 0.01 && none < 0.001 && full < 0.001, "{model:?}");
    }
}

#[test]
fn unison_plays_five_voices_on_the_lowest_key_without_a_new_attack() {
    for model in MODELS {
        // A sustain of a quarter: an attack that started again would
        // show as four times the level.
        let quarter = pure(model).with(Knob::AmplifierSustain, 30);
        let one = rms(&play(model, &quarter, 60, 0.5, 0.0)[samples(0.4)..]);
        let patch = quarter.with_switch(Switch::Unison, true);
        let mut prophet = instrument(model, &patch);
        prophet.event(NOTE_ON, 72, 100);
        assert_eq!(prophet.sounding_voices(), 0);
        let first = render(&mut prophet, 1.0);
        assert_eq!(prophet.sounding_voices(), 5);
        let five = rms(&first[samples(0.5)..]);
        // A lower key takes all five voices, a higher one none, and
        // neither starts the envelopes again: the level does not dip.
        prophet.event(NOTE_ON, 60, 100);
        prophet.event(NOTE_ON, 79, 100);
        let second = render(&mut prophet, 0.3);
        let low = pitch(&second[samples(0.1)..]);
        let level = rms(&second[samples(0.02)..samples(0.1)]);
        prophet.event(NOTE_OFF, 60, 0);
        let back = pitch(&render(&mut prophet, 0.3)[samples(0.1)..]);
        println!(
            "{model:?}: five voices are {:.2} times one; the lowest key sounds {low:.1} Hz, then {back:.1} Hz; level after it {:.2} of before",
            five / one,
            level / five
        );
        // Five voices a few cents apart: between the root of five and
        // five times one.
        assert!(five / one > 2.0 && five / one < 5.1);
        assert!(
            cents(261.63, low).abs() < 5.0
                && cents(523.25, back).abs() < 5.0
                && level / five < 2.0
                && level / five > 0.4
        );
    }
}

#[test]
fn the_glide_slews_an_octave_a_second_at_its_slowest_and_only_in_unison() {
    // TM3 test table: glide 10, "MINIMUM: 5 SECS TO SLEW 5 OCTAVES"; 0,
    // "NO GLIDE"; unison off, "NO DETUNING".
    for model in MODELS {
        let top = if model == Model::Rev3 { 120 } else { 127 };
        let patch = pure(model).with(Knob::Glide, top).with_switch(Switch::Unison, true);
        let mut prophet = instrument(model, &patch);
        prophet.event(NOTE_ON, 48, 100);
        render(&mut prophet, 6.0);
        prophet.event(NOTE_OFF, 48, 0);
        prophet.event(NOTE_ON, 84, 100);
        // Three octaves to go: after one second, one octave is done.
        render(&mut prophet, 0.95);
        let after = cents(130.81, pitch(&render(&mut prophet, 0.1)));
        println!("{model:?}: a second into a glide of three octaves: {after:.0} cents up (1200)");
        assert!((after - 1200.0).abs() < 40.0, "{after}");

        for patch in [patch.with(Knob::Glide, 0), patch.with_switch(Switch::Unison, false)] {
            let mut prophet = instrument(model, &patch);
            prophet.event(NOTE_ON, 48, 100);
            render(&mut prophet, 0.5);
            prophet.event(NOTE_OFF, 48, 0);
            prophet.event(NOTE_ON, 84, 100);
            render(&mut prophet, 0.3);
            let at_once = cents(1046.5, pitch(&render(&mut prophet, 0.3)));
            assert!(at_once.abs() < 5.0, "{model:?}: {at_once}");
        }
    }
}
