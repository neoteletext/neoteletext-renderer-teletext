//! The CS-80's slow oscillators and what they move, against the
//! adjustment procedure's figures: the pulse width's oscillator, the sub
//! oscillator and its five functions, the touch response, the keyboard
//! control, the ring modulator and the tremolo and chorus (TODO-6434).

// Without the feature there is nothing here to test.
#![cfg(feature = "cs80")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test, in f64 and on indices it derives itself"
)]

mod cs80_support;

use cs80_support::{
    CHANNEL_PRESSURE, CONTROL, KEY_PRESSURE, NOTE_ON, SINE, component, decibels, envelope, instrument, panel, pitch,
    play, render, rms,
};
use neoteletext_audio_instruments::cs80::{Control, Function, Global, Patch};

/// The key the levels are measured on: 3,520 Hz.
const KEY: u8 = 105;
/// Samples the levels are taken over: two cycles.
const WINDOW: usize = 27;

/// The frequency at which a tone's level moves, in hertz.
fn moving(signal: &[f64]) -> f64 {
    pitch(&envelope(signal, WINDOW)[WINDOW..])
}

/// The sine with the sub oscillator on its level.
fn tremolo(function: Function, speed: u8) -> Patch {
    let mut patch = panel(&SINE);
    patch.set(Global::SubFunction, function as u8);
    patch.set(Global::SubSpeed, speed);
    patch.set(Global::SubVca, 127);
    patch
}

#[test]
fn the_sub_oscillator_is_150_ms_at_five_volts_and_18_ms_at_its_fastest() {
    // Adjustment procedure, page 17, sub oscillator: 150 ms with 5.0 V at
    // the speed terminal, 18 ms at FAST. The lever's 64 of 127 is 5.04 V.
    for (lever, period) in [(64u8, 0.150), (127, 0.018), (0, 1.25)] {
        let signal = play(&tremolo(Function::Sine, lever), KEY, 100, 6.0, 0.0);
        let found = 1.0 / moving(&signal[4_800..]);
        println!("sub oscillator speed {lever}: a cycle in {:.1} ms for {:.0} ms", found * 1e3, period * 1e3);
        assert!((found / period - 1.0).abs() < 0.03, "{lever}: {found}");
    }
}

#[test]
fn the_sub_oscillators_functions_have_their_shapes() {
    // On the level, at full depth: twice nominal at the waveform's crest
    // and nothing at its trough.
    let shape = |function: Function| {
        let signal = play(&tremolo(function, 40), KEY, 100, 3.0, 0.0);
        envelope(&signal, WINDOW)[48_000..].iter().map(|level| level / 0.125).collect::<Vec<f64>>()
    };
    let mean = |levels: &[f64]| levels.iter().sum::<f64>() / levels.len() as f64;
    // How much of the time the level, taken in means of 10 ms, rises.
    let rise = |levels: &[f64]| {
        let means: Vec<f64> = levels.chunks(480).map(mean).collect();
        means.windows(2).filter(|pair| pair[1] > pair[0]).count() as f64 / (means.len() - 1) as f64
    };
    let (sine, falling, rising, square) =
        (shape(Function::Sine), shape(Function::Sawtooth), shape(Function::InvertedSawtooth), shape(Function::Square));
    println!(
        "means: sine {:.3}, sawtooth {:.3}, square {:.3}; the sawtooth rises {:.0}% of the time, the inverted {:.0}%",
        mean(&sine),
        mean(&falling),
        mean(&square),
        100.0 * rise(&falling),
        100.0 * rise(&rising)
    );
    for levels in [&sine, &falling, &rising, &square] {
        assert!((mean(levels) - 1.0).abs() < 0.03);
        assert!(levels.iter().all(|&level| (-0.01..2.04).contains(&level)));
    }
    // "A sawtooth wave with rapid beginning and slow decay", and the
    // other way about (owner's manual, page 9).
    assert!(rise(&falling) < 0.12 && rise(&rising) > 0.88);
    // The square is at one end or the other.
    let between = square.iter().filter(|&&level| (0.2..1.8).contains(&level)).count();
    assert!(between < square.len() / 20);
    // Noise moves the level without a period; the external input, with
    // nothing connected, does nothing.
    let noise = shape(Function::Noise);
    assert!((mean(&noise) - 1.0).abs() < 0.1 && rms(&noise) > 1.0);
    assert!(shape(Function::External).iter().all(|&level| (level - 1.0).abs() < 0.01));
}

#[test]
fn pressure_speeds_the_sub_oscillator_by_six_volts() {
    // Adjustment procedure, page 25, step 6: +6.0 V at the speed's
    // terminal with the touch response's SPEED at the top; the sub
    // oscillator's 0.612 octaves to the volt make that 12.7 times.
    let mut patch = tremolo(Function::Sine, 64);
    patch.set(Global::TouchSpeed, 127);
    let mut cs80 = instrument(&patch);
    cs80.event(NOTE_ON, KEY, 100);
    let light = moving(&render(&mut cs80, 3.0)[4_800..]);
    cs80.event(CHANNEL_PRESSURE, 127, 0);
    let pressed = moving(&render(&mut cs80, 3.0)[4_800..]);
    println!("the sub oscillator: {light:.2} Hz, and {pressed:.1} Hz under full pressure");
    assert!((pressed / light / 6.0f64.mul_add(0.611_78, 0.0).exp2() - 1.0).abs() < 0.03);
}

#[test]
fn the_pulse_widths_oscillator_is_150_ms_at_five_volts_and_5_ms_at_its_fastest() {
    // Adjustment procedure, page 17, pulse width modulator. The width
    // moving changes the fundamental's level, which is what is measured:
    // from 70% the pulse narrows to 90% and widens to 50%.
    let patch = |speed: u8| {
        panel(&[
            (Control::Sawtooth, 0),
            (Control::Square, 1),
            (Control::PulseWidth, 64),
            (Control::Pwm, 127),
            (Control::PwmSpeed, speed),
        ])
    };
    let slow = play(&patch(64), 69, 100, 4.0, 0.0);
    // The fundamental alone: its level in windows of 10 ms.
    let levels: Vec<f64> = slow[4_800..].chunks(480).map(|window| component(window, 440.0)).collect();
    let mean = levels.iter().sum::<f64>() / levels.len() as f64;
    let crossings = levels.windows(2).filter(|pair| pair[0] < mean && pair[1] >= mean).count();
    let period = (levels.len() as f64 * 0.010) / crossings as f64;
    let (low, high) =
        levels.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &level| (low.min(level), high.max(level)));
    println!(
        "PWM speed at 5 V: a cycle in {:.0} ms; the fundamental moves between {low:.4} and {high:.4}",
        period * 1e3
    );
    assert!((period / 0.150 - 1.0).abs() < 0.08);
    // A pulse's fundamental is sin(π × width) of a square's.
    let expected = (std::f64::consts::PI * 0.9).sin();
    assert!((low / high - expected).abs() < 0.05, "{}", low / high);
    // At the fastest the width moves 200 times a second: the fundamental
    // has neighbours 200 Hz either side.
    let fast = play(&patch(127), 69, 100, 2.0, 0.0);
    let steady = &fast[24_000..];
    let side = |offset: f64| decibels(component(steady, 440.0 + offset) / component(steady, 440.0));
    println!(
        "PWM speed at its fastest: {:.1} dB at 200 Hz from the fundamental, {:.1} dB at 150 Hz",
        side(200.0),
        side(150.0)
    );
    assert!(side(200.0) > -30.0 && side(-200.0) > -30.0 && side(150.0) < -50.0);
}

#[test]
fn the_pulse_width_moves_between_the_ticks_and_not_in_their_steps() {
    // The width set once a tick is a staircase of 6,000 steps a second,
    // and a staircase adds, beside the sidebands the modulation itself
    // gives, components the ticks' rate away from them: at 6,000 Hz less
    // the modulation's 200 Hz, where the sound itself has nothing (the
    // nearest of its own are the note's harmonics and their sidebands
    // at multiples of 200 Hz from them, none within 100 Hz of 5,800 Hz
    // for this G).
    let patch = panel(&[
        (Control::Sawtooth, 0),
        (Control::Square, 1),
        (Control::PulseWidth, 64),
        (Control::Pwm, 127),
        (Control::PwmSpeed, 127),
    ]);
    let signal = &play(&patch, 79, 100, 2.5, 0.0)[24_000..];
    let strongest = (1..=8).map(|harmonic| component(signal, 783.99 * f64::from(harmonic))).fold(0.0, f64::max);
    let added = decibels(component(signal, 5_800.0) / strongest);
    println!("the ticks' rate less the modulation's, against the strongest harmonic: {added:.1} dB");
    assert!(added < -60.0, "{added} dB");
}

#[test]
fn the_touch_moves_the_level_and_the_brilliance() {
    // The level: 6.4 V at 3 dB to the volt, 19.2 dB, from the softest
    // stroke to the hardest, and as much again under pressure, held to
    // the 18 dB there is room for.
    let level = |patch: &Patch, velocity: u8, pressure: u8| {
        // The firmest pressure brings one line to the whole of the mixed
        // output, which the output's limiter would bend: the volume is
        // at 64 of 127, a quarter by its square law, and allowed for.
        const VOLUME: u8 = 64;
        let quieter = (f64::from(VOLUME) / 127.0).powi(2);
        let mut cs80 = instrument(patch);
        cs80.event(CONTROL, 7, VOLUME);
        cs80.event(NOTE_ON, 69, velocity);
        cs80.event(KEY_PRESSURE, 69, pressure);
        decibels(rms(&render(&mut cs80, 0.5)[12_000..]) * 2.0f64.sqrt() / 0.125 / quieter)
    };
    let mut touch = panel(&SINE);
    touch.tones[0].set(Control::TouchLevel, 127);
    let (soft, middle, hard) = (level(&touch, 1, 0), level(&touch, 64, 0), level(&touch, 127, 0));
    println!("INITIAL LEVEL at 10: {soft:.1} dB, {middle:.1} dB and {hard:.1} dB at velocities 1, 64 and 127");
    assert!(
        (soft + 19.2 * 126.0 / 127.0).abs() < 0.2 && (middle + 19.2 * 63.0 / 127.0).abs() < 0.2 && hard.abs() < 0.1
    );
    let mut after = panel(&SINE);
    after.tones[0].set(Control::PressureLevel, 127);
    let (light, half, full) = (level(&after, 100, 0), level(&after, 100, 64), level(&after, 100, 127));
    println!("AFTER LEVEL at 10: {light:.1} dB, {half:.1} dB and {full:.1} dB at pressures 0, 64 and 127");
    assert!(light.abs() < 0.1 && (half - 19.2 * 64.0 / 127.0).abs() < 0.2 && (full - 18.06).abs() < 0.2);
    // With the levers down the touch does nothing.
    assert!((level(&panel(&SINE), 1, 127) - level(&panel(&SINE), 127, 0)).abs() < 0.01);

    // The brilliance: 6.4 V at the levers' tops for a firm pressure and
    // for the 8.5 V the stroke's amplifier is trimmed with, of which the
    // hardest stroke gives 7.9 V (adjustment procedure, page 23; service
    // manual, page 11, the detector's diode); 1.222 octaves to the volt
    // at the low-pass filter. A sawtooth through that filter with
    // its lever at 2.05 V: the third harmonic's gain at a Q of 0.5 is
    // 1 / (1 + x²), x its frequency over the cutoff.
    let bright = |control: Control, velocity: u8, pressure: u8| {
        let patch = panel(&[(Control::Lpf, 26), (control, 40)]);
        let mut cs80 = instrument(&patch);
        cs80.event(NOTE_ON, 69, velocity);
        cs80.event(KEY_PRESSURE, 69, pressure);
        decibels(component(&render(&mut cs80, 0.5)[12_000..], 440.0 * 3.0))
    };
    let scale = 3.3 / 2.7;
    let cutoff = 32.703 * (21.0f64 / 12.0 + 10.0 * 26.0 / 127.0 * scale).exp2();
    let gain = |volts: f64| -decibels(1.0 + (1320.0 / (cutoff * (volts * scale).exp2())).powi(2));
    // 40 of 127 of 6.4 V is 2.016 V.
    let reach = 6.4 * 40.0 / 127.0;
    let stroke = reach * 7.9 / 8.5;
    let expected_stroke = gain(stroke * 64.0 / 127.0) - gain(stroke / 127.0);
    let expected_pressure = gain(reach * 64.0 / 127.0) - gain(0.0);
    let by_stroke = bright(Control::TouchBrilliance, 64, 0) - bright(Control::TouchBrilliance, 1, 0);
    let by_pressure = bright(Control::PressureBrilliance, 100, 64) - bright(Control::PressureBrilliance, 100, 0);
    println!(
        "a volt of brilliance by the stroke: {by_stroke:.1} dB for {expected_stroke:.1} dB; by pressure: {by_pressure:.1} dB for {expected_pressure:.1} dB"
    );
    assert!((by_stroke - expected_stroke).abs() < 0.3 && (by_pressure - expected_pressure).abs() < 0.3);
    // The hardest stroke against the firmest pressure, each alone: the
    // stroke's 0.6 V less at the lever's top are 40 / 127 of them here.
    let hardest = bright(Control::TouchBrilliance, 127, 0) - bright(Control::PressureBrilliance, 127, 127);
    let expected_hardest = gain(stroke) - gain(reach);
    println!("the hardest stroke against the firmest pressure: {hardest:.2} dB for {expected_hardest:.2} dB");
    assert!(expected_hardest < -0.2 && (hardest - expected_hardest).abs() < 0.1);
}

#[test]
fn pressure_on_one_key_moves_that_key_alone() {
    let mut patch = panel(&SINE);
    patch.tones[0].set(Control::PressureLevel, 127);
    // Quiet enough that the pressed key's 18 dB stay below full scale.
    patch.tones[0].set(Control::Level, 32);
    let mut cs80 = instrument(&patch);
    cs80.event(NOTE_ON, 69, 100);
    cs80.event(NOTE_ON, 76, 100);
    render(&mut cs80, 0.2);
    let before = render(&mut cs80, 0.25);
    cs80.event(KEY_PRESSURE, 76, 127);
    render(&mut cs80, 0.05);
    let after = render(&mut cs80, 0.25);
    let change = |hertz: f64| decibels(component(&after, hertz) / component(&before, hertz));
    println!("pressure on the upper key: the lower moves {:.2} dB, the upper {:.2} dB", change(440.0), change(659.26));
    assert!(change(440.0).abs() < 0.1 && (change(659.26) - 18.06).abs() < 0.3);
    // Channel pressure presses every key that is down.
    cs80.event(CHANNEL_PRESSURE, 0, 0);
    render(&mut cs80, 0.05);
    let released = render(&mut cs80, 0.25);
    assert!(decibels(component(&released, 659.26) / component(&before, 659.26)).abs() < 0.1);
}

#[test]
fn a_key_that_is_up_is_no_longer_pressed() {
    // The sensor is under the key: once the key is up nothing presses
    // it, whatever pressure the last message before the note off gave.
    // A long release, so that the tail is there to measure.
    let mut patch = panel(&SINE);
    patch.tones[0].set(Control::PressureLevel, 127);
    patch.tones[0].set(Control::Release, 100);
    patch.tones[0].set(Control::Level, 32);
    let tail = |pressure: Option<u8>| {
        let mut cs80 = instrument(&patch);
        cs80.event(NOTE_ON, 69, 100);
        render(&mut cs80, 0.2);
        if let Some(pressure) = pressure {
            cs80.event(KEY_PRESSURE, 69, pressure);
        }
        render(&mut cs80, 0.2);
        cs80.event(cs80_support::NOTE_OFF, 69, 0);
        render(&mut cs80, 0.05);
        component(&render(&mut cs80, 0.1), 440.0)
    };
    let (plain, pressed) = (tail(None), tail(Some(127)));
    println!(
        "the tail of a key pressed until it was let go: {:.2} dB against one never pressed",
        decibels(pressed / plain)
    );
    assert!(plain > 1.0e-3 && decibels(pressed / plain).abs() < 0.1);
}

#[test]
fn the_keyboard_control_is_2_76_volts_at_its_trimmed_keys() {
    // Adjustment procedure, page 27: -2.76 V at the key 48 with a low
    // lever at its foot, +2.76 V at the key 85 with a high lever at its
    // top; nothing at the centre. A volt is 3 dB of level.
    let level = |global: Global, lever: u8, key: u8| {
        let mut patch = panel(&SINE);
        patch.set(global, lever);
        decibels(rms(&play(&patch, key, 100, 0.5, 0.0)[12_000..]) * 2.0f64.sqrt() / 0.125)
    };
    let low = level(Global::KeyboardLevelLow, 0, 48);
    let high = level(Global::KeyboardLevelHigh, 127, 85);
    println!("the keyboard's level levers at their trimmed keys: {low:.2} dB and {high:.2} dB");
    assert!((low + 8.28).abs() < 0.1 && (high - 8.28).abs() < 0.1);
    // Each lever has its own half of the keyboard, and the middle none.
    assert!(
        level(Global::KeyboardLevelLow, 0, 85).abs() < 0.01 && level(Global::KeyboardLevelHigh, 127, 48).abs() < 0.01
    );
    assert!(
        level(Global::KeyboardLevelLow, 0, 66).abs() < 0.01 && level(Global::KeyboardLevelHigh, 0, 66).abs() < 0.01
    );
    // The curve is the square of the distance: half way, a quarter.
    assert!((level(Global::KeyboardLevelLow, 0, 57) + 8.28 / 4.0).abs() < 0.1);
    // The brilliance levers move the filters: 2.76 V at the same keys,
    // 3.37 octaves of the low-pass filter. Its lever at 2.99 V puts it at
    // 412 Hz on this key, and the fourth harmonic's gain at a Q of 0.5 is
    // 1 / (1 + x²).
    let bright = |lever: u8| {
        let mut patch = panel(&[(Control::Lpf, 38)]);
        patch.set(Global::KeyboardBrillianceLow, lever);
        decibels(component(&play(&patch, 48, 100, 0.6, 0.0)[12_000..], 130.81 * 4.0))
    };
    let moved = bright(64) - bright(0);
    println!("the keyboard's brilliance lever at its trimmed key: {moved:.1} dB of the fourth harmonic");
    let scale = 3.3 / 2.7;
    let cutoff = 32.703 * (10.0f64 * 38.0 / 127.0 * scale).exp2();
    let gain = |volts: f64| -decibels(1.0 + (523.25 / (cutoff * (volts * scale).exp2())).powi(2));
    assert!((moved - (gain(0.0) - gain(-2.76))).abs() < 0.3, "{moved} for {}", gain(0.0) - gain(-2.76));
}

#[test]
fn the_ring_modulator_puts_the_sum_and_the_difference_in_place_of_the_sound() {
    // Adjustment procedure, page 19, step 3: 150 ms with 5.0 V at the
    // speed terminal, 5 ms at the top.
    let mut patch = panel(&SINE);
    patch.set(Global::RingModulation, 127);
    patch.set(Global::RingSpeed, 127);
    let signal = play(&patch, 81, 100, 1.5, 0.0);
    let steady = &signal[24_000..];
    let (carrier, lower, upper) = (component(steady, 880.0), component(steady, 680.0), component(steady, 1_080.0));
    println!(
        "ring modulation at 200 Hz: the note {:.1} dB, 680 Hz {:.1} dB, 1,080 Hz {:.1} dB of the sine",
        decibels(carrier / 0.125),
        decibels(lower / 0.125),
        decibels(upper / 0.125)
    );
    // Each side is half the sound; the note itself is gone.
    assert!((lower / 0.0625 - 1.0).abs() < 0.03 && (upper / 0.0625 - 1.0).abs() < 0.03 && carrier < 0.001);
    // Half the lever: half the sound stays.
    patch.set(Global::RingModulation, 64);
    let half = play(&patch, 81, 100, 1.5, 0.0);
    assert!((component(&half[24_000..], 880.0) / 0.125 - 63.0 / 127.0).abs() < 0.02);
    // At 5 V the level moves 6.67 times a second.
    patch.set(Global::RingModulation, 127);
    patch.set(Global::RingSpeed, 64);
    let slow = play(&patch, KEY, 100, 4.0, 0.0);
    // The level of a product with a sine goes through nothing twice a
    // cycle.
    let found = moving(&slow[4_800..]) / 2.0;
    println!("ring modulator speed at 5 V: a cycle in {:.0} ms", 1e3 / found);
    assert!((1.0 / found / 0.150 - 1.0).abs() < 0.06);
}

#[test]
fn the_ring_modulators_envelope_speeds_it_up_and_lets_it_fall_back() {
    // DEPTH at the top adds five volts at the envelope's peak: 30 times
    // the speed. Attack at 64 of 127 is the 31 ms of the adjustment's 5
    // V; the decay at 80 is 0.8 s to a tenth of the way.
    let mut patch = panel(&SINE);
    for (global, value) in [
        (Global::RingModulation, 127),
        (Global::RingSpeed, 64),
        (Global::RingDepth, 127),
        (Global::RingAttack, 0),
        (Global::RingDecay, 80),
    ] {
        patch.set(global, value);
    }
    let signal = play(&patch, KEY, 100, 6.0, 0.0);
    let speed = |from: f64, to: f64| moving(&signal[(from * 48_000.0) as usize..(to * 48_000.0) as usize]) / 2.0;
    let (first, last) = (speed(0.01, 0.06), speed(4.0, 6.0));
    println!("ring modulator with its envelope: {first:.0} Hz just after the key, {last:.1} Hz at rest");
    assert!((last / 6.85 - 1.0).abs() < 0.08);
    // Within the first 60 ms the envelope has fallen a little from its
    // peak.
    assert!(first > 120.0 && first < 6.85 * 31.0);
}

#[test]
fn the_tremolo_and_the_chorus_are_what_the_mono_output_has_of_them() {
    // Adjustment procedure, page 21: the tremolo's fastest cycle is 56
    // ms, the chorus's 0.5 s; the four amplifiers' gains of 0.65 and 0.58
    // leave 2.85% of each oscillator's swing in the sum of the outputs.
    let run = |tremolo: u8, chorus: u8, depth: u8| {
        let mut patch = panel(&SINE);
        patch.set(Global::Effect, 1);
        patch.set(Global::Tremolo, tremolo);
        patch.set(Global::Chorus, chorus);
        patch.set(Global::EffectSpeed, 127);
        patch.set(Global::EffectDepth, depth);
        play(&patch, 81, 100, 6.0, 0.0)
    };
    let side =
        |signal: &[f64], hertz: f64| component(&signal[48_000..], 880.0 + hertz) / component(&signal[48_000..], 880.0);
    let tremolo = run(1, 0, 127);
    let chorus = run(0, 1, 127);
    println!(
        "the effect in mono: tremolo sidebands at 17.86 Hz {:.4}, chorus sidebands at 2 Hz {:.4} of the note",
        side(&tremolo, 17.857),
        side(&chorus, 2.0)
    );
    // A swing of m is two sidebands of m / 2; the mean level is the full
    // level less the swing.
    let expected = (7.0 / 246.0) / (1.0 - 14.0 / 246.0) / 2.0;
    assert!(
        (side(&tremolo, 17.857) / expected - 1.0).abs() < 0.05
            && (side(&tremolo, -17.857) / expected - 1.0).abs() < 0.05
    );
    assert!((side(&chorus, 2.0) / expected - 1.0).abs() < 0.05 && side(&chorus, 17.857) < 1.0e-4);
    assert!(side(&run(1, 1, 0), 17.857) < 1.0e-4 && side(&run(0, 0, 127), 17.857) < 1.0e-4);
    // The level drops slightly, and the lines' filters take the top.
    let plain = play(&panel(&SINE), 81, 100, 2.0, 0.0);
    let drop = decibels(component(&run(0, 0, 0)[48_000..], 880.0) / component(&plain[48_000..], 880.0));
    println!("the effect on: the level {drop:.2} dB");
    assert!(drop < -0.5 && drop > -2.0);
}

#[test]
fn the_effect_switched_on_in_silence_lets_nothing_out() {
    // The effect's delay lines are fed only while it is on. Switched off
    // in the middle of a note they kept their 2.56 ms of it, and gave it
    // back whenever the effect was next switched on.
    let mut on = panel(&SINE);
    on.set(Global::Effect, 1);
    let off = panel(&SINE);
    let mut cs80 = instrument(&on);
    cs80.event(NOTE_ON, 72, 127);
    let _ = render(&mut cs80, 0.5);
    cs80.set_patch(&off);
    cs80.event(CONTROL, 120, 0);
    let _ = render(&mut cs80, 1.0);
    cs80.set_patch(&on);
    let after = render(&mut cs80, 0.05);
    let peak = after.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
    assert!(peak < 1.0e-4, "{peak}");
}
