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
    // the speed terminal, 18 ms at FAST. The lever is on the panel's
    // 10.6 V (service manual, page 64), so its 60 of 127 is 5.01 V, and
    // its foot, 5 V below at log₂ (150 / 18) octaves in 5.6 V, is 1.0 s.
    for (lever, period) in [(60u8, 0.150), (127, 0.018), (0, 0.996)] {
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
fn the_sub_oscillator_reaches_the_filters_with_its_own_1_6_volts() {
    // Service manual, pages 64, 29 and 52: the VCF lever's top is the sub
    // oscillator's own output, 3.2 Vp-p, and the low-pass filter moves
    // 1.222 octaves for each volt of it. The square function holds each
    // end for half of the slowest cycle, 0.996 s, from the instrument's
    // start. A sawtooth through that filter with its lever at 2.05 V:
    // the third harmonic's gain at a Q of 0.5 is 1 / (1 + x²).
    let mut patch = panel(&[(Control::Lpf, 26)]);
    patch.set(Global::SubFunction, Function::Square as u8);
    patch.set(Global::SubSpeed, 0);
    patch.set(Global::SubVcf, 127);
    let signal = play(&patch, 69, 100, 1.0, 0.0);
    let third = |from: usize| decibels(component(&signal[from..from + 14_400], 1320.0));
    let moved = third(4_800) - third(28_800);
    let scale = 3.3 / 2.7;
    let cutoff = 32.703 * (21.0f64 / 12.0 + 10.0 * 26.0 / 127.0 * scale).exp2();
    let gain = |volts: f64| -decibels(1.0 + (1320.0 / (cutoff * (volts * scale).exp2())).powi(2));
    let expected = gain(1.6) - gain(-1.6);
    println!("the sub oscillator's square on the filters: {moved:.1} dB of the third harmonic for {expected:.1} dB");
    assert!((moved - expected).abs() < 0.3, "{moved} for {expected}");
}

#[test]
fn pressure_speeds_the_sub_oscillator_by_six_volts() {
    // Adjustment procedure, page 25, step 6: +6.0 V at the speed's
    // terminal with the touch response's SPEED at the top; the sub
    // oscillator's 0.546 octaves to the volt make that 9.7 times.
    let mut patch = tremolo(Function::Sine, 60);
    patch.set(Global::TouchSpeed, 127);
    let mut cs80 = instrument(&patch);
    cs80.event(NOTE_ON, KEY, 100);
    let light = moving(&render(&mut cs80, 3.0)[4_800..]);
    cs80.event(CHANNEL_PRESSURE, 127, 0);
    let pressed = moving(&render(&mut cs80, 3.0)[4_800..]);
    println!("the sub oscillator: {light:.2} Hz, and {pressed:.1} Hz under full pressure");
    assert!((pressed / light / 6.0f64.mul_add(0.546_231, 0.0).exp2() - 1.0).abs() < 0.03);
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
fn the_pitch_set_at_the_ticks_leaves_little_at_their_rate() {
    // A vibrato at the sub oscillator's fastest, 55.6 Hz, a tenth of an
    // octave either way. The pitch is set once a tick, a staircase of
    // 6,000 steps a second, which adds components the ticks' rate away
    // from the note, at 880 + 6,000 Hz and the vibrato's rate about it,
    // where the vibrato's own sidebands, 55.6 Hz apart about the note,
    // have long died away: 83 dB below the note's strongest part.
    //
    // Moving the frequency through every sample was tried and is not
    // done: by the oscillator's `run` it left 90 dB and cost a seventh
    // of the instrument's speed with everything on (16.7 to 14.1 times
    // real time) and more than two fifths with sixteen sines (50 to 28);
    // by setting the step each sample, 89 dB for a tenth and a quarter.
    let mut patch = panel(&SINE);
    patch.set(Global::SubSpeed, 127);
    patch.set(Global::SubVco, 40);
    let signal = &play(&patch, 81, 100, 2.5, 0.0)[24_000..];
    let rate = 1_000.0 / 18.0;
    let strongest = (-3..=3).map(|side| component(signal, 880.0 + rate * f64::from(side))).fold(0.0, f64::max);
    let added = (-2..=2).map(|side| component(signal, 880.0 + 6_000.0 + rate * f64::from(side))).fold(0.0, f64::max);
    println!("the ticks' rate above a fast vibrato, against its strongest part: {:.1} dB", decibels(added / strongest));
    assert!(decibels(added / strongest) < -80.0, "{} dB", decibels(added / strongest));
}

#[test]
fn the_touch_moves_the_level_and_the_brilliance() {
    // The level: the amplifier's linear input has the LEVEL lever's 10 V
    // through 560 kΩ and the touch's voltages through 270 kΩ (service
    // manual, pages 39, 41 and 54), so the firmest pressure's 6.4 V make
    // the gain 1 + 0.64 × 560 / 270 = 2.327 times the lever's own, and
    // the hardest stroke's 7.9 / 8.5 of them 2.234 times, which is taken
    // as nominal. What a stroke gives drains away with a time constant of
    // 3.3 s (service manual, page 17: 10 MΩ across 0.33 µF), so that over
    // the quarter of a second measured, 0.375 s after the key in the
    // middle, 0.8926 of it is left.
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
    let mut struck = panel(&SINE);
    struck.tones[0].set(Control::TouchLevel, 127);
    let (soft, middle, hard) = (level(&struck, 1, 0), level(&struck, 64, 0), level(&struck, 127, 0));
    println!("INITIAL LEVEL at 10: {soft:.1} dB, {middle:.1} dB and {hard:.1} dB at velocities 1, 64 and 127");
    let left = (-0.375f64 / 3.3).exp();
    let touch = 0.64 * 560.0 / 270.0;
    let hardest = touch * 7.9 / 8.5;
    let stroke = |velocity: f64| decibels((1.0 + hardest * left * velocity / 127.0) / (1.0 + hardest));
    assert!(
        (soft - stroke(1.0)).abs() < 0.1 && (middle - stroke(64.0)).abs() < 0.1 && (hard - stroke(127.0)).abs() < 0.1,
        "{soft} {middle} {hard}"
    );
    // Held, the hardest stroke's level comes down to the softest's: 4 s
    // on, three tenths of it are left.
    let held = {
        let mut cs80 = instrument(&struck);
        cs80.event(NOTE_ON, 69, 127);
        let _ = render(&mut cs80, 3.875);
        decibels(rms(&render(&mut cs80, 0.25)) * 2.0f64.sqrt() / 0.125)
    };
    println!("INITIAL LEVEL at 10, the hardest stroke 4 s on: {held:.2} dB");
    let expected = decibels((1.0 + hardest * (-4.0f64 / 3.3).exp()) / (1.0 + hardest));
    assert!((held - expected).abs() < 0.1 && expected < -3.0, "{held} for {expected}");
    let mut after = panel(&SINE);
    after.tones[0].set(Control::PressureLevel, 127);
    let (light, half, full) = (level(&after, 100, 0), level(&after, 100, 64), level(&after, 100, 127));
    println!("AFTER LEVEL at 10: {light:.1} dB, {half:.1} dB and {full:.1} dB at pressures 0, 64 and 127");
    let pressed = |pressure: f64| decibels(1.0 + touch * pressure / 127.0);
    assert!(
        light.abs() < 0.1 && (half - pressed(64.0)).abs() < 0.1 && (full - pressed(127.0)).abs() < 0.1,
        "{light} {half} {full} for 0, {} and {}",
        pressed(64.0),
        pressed(127.0)
    );
    // The firmest pressure is 7.3 dB, and in a straight line, not in
    // decibels: half of it is more than half of that.
    assert!((pressed(127.0) - 7.34).abs() < 0.01 && pressed(64.0) > 4.4);
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
    // 0.375 s after the key, 0.8926 of a stroke's voltage is left.
    let left = (-0.375f64 / 3.3).exp();
    let expected_stroke = gain(stroke * left * 64.0 / 127.0) - gain(stroke * left / 127.0);
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
    let expected_hardest = gain(stroke * left) - gain(reach);
    println!("the hardest stroke against the firmest pressure: {hardest:.2} dB for {expected_hardest:.2} dB");
    assert!(expected_hardest < -0.2 && (hardest - expected_hardest).abs() < 0.1);
}

#[test]
fn pressure_on_one_key_moves_that_key_alone() {
    let mut patch = panel(&SINE);
    patch.tones[0].set(Control::PressureLevel, 127);
    // Quiet enough that the pressed key stays below full scale: on a
    // LEVEL lever at 32 of 127 the pressure's 6.4 V through 270 kΩ make
    // the gain (32 / 127 + 1.327) / (32 / 127) times, 15.9 dB.
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
    let lever = 32.0 / 127.0;
    let expected = decibels((lever + 0.64 * 560.0 / 270.0) / lever);
    assert!(change(440.0).abs() < 0.1 && (change(659.26) - expected).abs() < 0.1, "{expected}");
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
    // top; nothing at the centre. They reach the amplifier's linear
    // input through 270 kΩ beside the LEVEL lever's 10 V through 560 kΩ
    // (service manual, pages 39 and 41): 1 ∓ 0.276 × 560 / 270 of the
    // gain, 7.39 dB down and 3.93 dB up.
    let level = |global: Global, lever: u8, key: u8| {
        let mut patch = panel(&SINE);
        patch.set(global, lever);
        decibels(rms(&play(&patch, key, 100, 0.5, 0.0)[12_000..]) * 2.0f64.sqrt() / 0.125)
    };
    let low = level(Global::KeyboardLevelLow, 0, 48);
    let high = level(Global::KeyboardLevelHigh, 127, 85);
    println!("the keyboard's level levers at their trimmed keys: {low:.2} dB and {high:.2} dB");
    let by = |volts: f64| decibels(1.0 + volts / 10.0 * 560.0 / 270.0);
    assert!((low - by(-2.76)).abs() < 0.05 && (high - by(2.76)).abs() < 0.05, "{} {}", by(-2.76), by(2.76));
    assert!((by(-2.76) + 7.39).abs() < 0.01 && (by(2.76) - 3.93).abs() < 0.01);
    // Each lever has its own half of the keyboard, and the middle none.
    // The level is taken over a quarter of a second, which on the key 48
    // is 32.7 cycles: the part of a cycle left over moves it by up to
    // 1 / (4 π 32.7) of itself, 0.021 dB.
    let none = [
        level(Global::KeyboardLevelLow, 0, 85),
        level(Global::KeyboardLevelHigh, 127, 48),
        level(Global::KeyboardLevelLow, 0, 66),
        level(Global::KeyboardLevelHigh, 0, 66),
    ];
    assert!(none.iter().all(|level| level.abs() < 0.03), "{none:?}");
    // The curve is the square of the distance: half way, a quarter.
    assert!((level(Global::KeyboardLevelLow, 0, 57) - by(-2.76 / 4.0)).abs() < 0.05);
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
    // The lever is on the panel's 10.6 V: 60 of 127 is 5.01 V.
    patch.set(Global::RingSpeed, 60);
    let slow = play(&patch, KEY, 100, 4.0, 0.0);
    // The level of a product with a sine goes through nothing twice a
    // cycle.
    let found = moving(&slow[4_800..]) / 2.0;
    println!("ring modulator speed at 5 V: a cycle in {:.0} ms", 1e3 / found);
    assert!((1.0 / found / 0.150 - 1.0).abs() < 0.06);
}

#[test]
fn the_ring_modulators_envelope_speeds_it_up_and_lets_it_fall_back() {
    // DEPTH at the top adds five volts at the envelope's peak: 20.9 times
    // the speed, at log₂ 30 octaves in the 5.6 V from 5 V to the SPEED
    // lever's top. The decay at 80 is 0.8 s to a tenth of the way.
    let mut patch = panel(&SINE);
    for (global, value) in [
        (Global::RingModulation, 127),
        (Global::RingSpeed, 60),
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
    assert!((last / 6.70 - 1.0).abs() < 0.03, "{last}");
    // Within the first 60 ms the envelope has fallen a little from its
    // peak.
    assert!(first > 84.0 && first < 6.70 * 20.9);
}

/// The effect's parts, from the service manual's pages 55 to 58: the dry
/// sound's share with the effect on, the delayed sound's at low
/// frequencies, the cutoff in hertz of the low-pass filter the delayed
/// sound goes through, and the lines' delay in seconds.
const EFFECT: (f64, f64, f64, f64) = (68.0 / 168.0, 100.0 / 168.0 * 2.2365, 499.1, 2.56e-3);

/// What the effect makes of a frequency, as a share of what comes out
/// with the effect off, for a delay in seconds: the dry sound's share
/// less the delayed sound's, which has been through the low-pass filter.
fn comb(hertz: f64, delay: f64) -> f64 {
    let (dry, wet, cutoff, _) = EFFECT;
    // 1 / (1 + j f / fc) turned by the delay.
    let ratio = hertz / cutoff;
    let size = wet / ratio.hypot(1.0);
    let angle = -std::f64::consts::TAU * hertz * delay - ratio.atan();
    (dry - size * angle.cos()).hypot(size * angle.sin())
}

#[test]
fn the_effect_is_the_comb_the_drawings_make() {
    // Service manual, page 55: the dry sound stays in through 100 kΩ and
    // the delayed sound joins it inverted through 68 kΩ; pages 57 and 58:
    // the delayed sound is 2.56 ms late and has been through a low-pass
    // filter of 499 Hz. Noise through the open filters, the same noise
    // with the effect on and off, shows what it does to each frequency.
    let noise = |effect: u8| {
        let mut patch = panel(&[(Control::Sawtooth, 0), (Control::Noise, 127)]);
        patch.set(Global::Effect, effect);
        play(&patch, 60, 100, 5.0, 0.0)[48_000..].to_vec()
    };
    let (on, off) = (noise(1), noise(0));
    let grid: Vec<f64> = (2..=160).map(|step| f64::from(step) * 12.5).collect();
    let measured: Vec<f64> = grid.iter().map(|&hertz| component(&on, hertz) / component(&off, hertz)).collect();
    // The delay that fits best: the lines' own, and a little for the
    // filters about them.
    let misfit = |delay: f64| {
        grid.iter()
            .zip(&measured)
            .map(|(&hertz, &found)| decibels(found / comb(hertz, delay)).abs())
            .fold(0.0f64, f64::max)
    };
    let (delay, worst) = (2_400..2_800)
        .map(|microseconds| f64::from(microseconds) * 1.0e-6)
        .map(|delay| (delay, misfit(delay)))
        .fold((0.0, f64::MAX), |best, tried| if tried.1 < best.1 { tried } else { best });
    // The deepest notch and the highest crest, and where they are.
    let at = |pick: fn(f64, f64) -> bool| {
        grid.iter().zip(&measured).fold(
            (0.0, measured[0]),
            |best, (&hertz, &found)| {
                if pick(found, best.1) { (hertz, found) } else { best }
            },
        )
    };
    let (notch, crest) = (at(|found, best| found < best), at(|found, best| found > best));
    println!(
        "the effect's comb: delay {:.0} µs, worst misfit {worst:.2} dB of 25 Hz to 2 kHz; crest {:+.2} dB at {:.1} Hz, notch {:+.2} dB at {:.1} Hz; 25 Hz {:+.2} dB, 2 kHz {:+.2} dB",
        delay * 1.0e6,
        decibels(crest.1),
        crest.0,
        decibels(notch.1),
        notch.0,
        decibels(measured[0]),
        decibels(measured[measured.len() - 1])
    );
    // The filters about the lines add some tens of microseconds.
    assert!(delay >= EFFECT.3 && delay < EFFECT.3 + 0.12e-3, "{delay}");
    assert!(worst < 0.6, "{worst}");
    // The crests are a delay apart in frequency: the first is where the
    // inverted, delayed sound is back in step, half that from nothing,
    // pulled down a little by the filter's own delay.
    assert!((crest.0 - 175.0).abs() <= 12.5 && (decibels(crest.1) - decibels(comb(crest.0, delay))).abs() < 0.3);
    // At the bottom the delayed sound all but takes the dry sound's
    // place, at about the level the sound has with the effect off; at
    // the top the dry sound's share is what is left.
    assert!((decibels(measured[0]) - decibels(comb(25.0, delay))).abs() < 0.3 && decibels(measured[0]) > -1.5);
    let top = |effect: u8| {
        let mut patch = panel(&SINE);
        patch.set(Global::Effect, effect);
        let signal = play(&patch, 120, 100, 1.5, 0.0);
        component(&signal[24_000..], 8_372.0)
    };
    let left = decibels(top(1) / top(0));
    println!("the effect on a sine of 8,372 Hz: {left:+.2} dB (the dry share alone is {:+.2} dB)", decibels(EFFECT.0));
    assert!((left - decibels(EFFECT.0)).abs() < 1.0, "{left}");
}

/// The strongest frequency between two bounds in how a tone's level
/// moves, in hertz, and its size as a share of the level.
fn swing(signal: &[f64], low: f64, high: f64) -> (f64, f64) {
    // The level over two cycles of the 880 Hz the tests play, taken a
    // hundred times a second.
    const STEP: usize = 480;
    let levels: Vec<f64> = signal.as_chunks::<STEP>().0.iter().map(|chunk| rms(chunk)).collect();
    let mean = levels.iter().sum::<f64>() / levels.len() as f64;
    let size = |hertz: f64| {
        let (mut real, mut imaginary) = (0.0, 0.0);
        for (index, &level) in levels.iter().enumerate() {
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / levels.len() as f64).cos();
            let angle = std::f64::consts::TAU * hertz * index as f64 * STEP as f64 / RATE_HZ;
            real += (level - mean) * window * angle.cos();
            imaginary += (level - mean) * window * angle.sin();
        }
        4.0 * real.hypot(imaginary) / levels.len() as f64 / mean
    };
    (0..=400)
        .map(|step| low + (high - low) * f64::from(step) / 400.0)
        .map(|hertz| (hertz, size(hertz)))
        .fold((0.0, 0.0), |best, tried| if tried.1 > best.1 { tried } else { best })
}

#[test]
fn both_of_the_effects_oscillators_run_under_either_tab() {
    // Adjustment procedure, page 21: with SPEED at its top and the
    // TREMOLO tab down the two oscillators' cycles are 56 ms and 0.5 s.
    // Service manual, pages 55 and 66: the CHORUS tab gives the same two
    // oscillators 1.2 / 9.4 of the knob's voltage where TREMOLO gives
    // them 100 / 122, which is 6.17 octaves slower at the knob's top; and
    // TREMOLO has their outputs through 150 kΩ, 333 / 483 of the depth.
    let run = |tremolo: u8, chorus: u8, speed: u8, depth: u8, seconds: f64| {
        let mut patch = panel(&SINE);
        patch.set(Global::Effect, 1);
        patch.set(Global::Tremolo, tremolo);
        patch.set(Global::Chorus, chorus);
        patch.set(Global::EffectSpeed, speed);
        patch.set(Global::EffectDepth, depth);
        play(&patch, 81, 100, seconds, 0.0)[48_000..].to_vec()
    };
    let tremolo = run(1, 0, 127, 127, 13.0);
    let (fast, slow) = (swing(&tremolo, 15.0, 21.0), swing(&tremolo, 1.6, 2.4));
    println!(
        "TREMOLO, SPEED at its top: the level moves at {:.3} Hz by {:.4} and at {:.3} Hz by {:.4} of itself",
        fast.0, fast.1, slow.0, slow.1
    );
    assert!((fast.0 / 17.857 - 1.0).abs() < 0.01 && (slow.0 / 2.0 - 1.0).abs() < 0.01);
    // Each oscillator moves the delayed sound's level by the same share.
    assert!(fast.1 > 0.005 && (fast.1 / slow.1 - 1.0).abs() < 0.1);
    // Both tabs down are the tremolo.
    let both = run(1, 1, 127, 127, 13.0);
    assert!((swing(&both, 15.0, 21.0).1 / fast.1 - 1.0).abs() < 0.01);
    // CHORUS: the first oscillator's cycle is 4.03 s, and nothing moves
    // at the tremolo's speeds.
    let chorus = run(0, 1, 127, 127, 61.0);
    let first = swing(&chorus, 0.2, 0.3);
    println!("CHORUS, SPEED at its top: the level moves at {:.4} Hz by {:.4} of itself", first.0, first.1);
    assert!((first.0 / 0.2483 - 1.0).abs() < 0.02, "{}", first.0);
    assert!((first.1 / fast.1 / (483.0 / 333.0) - 1.0).abs() < 0.1, "{}", first.1 / fast.1);
    assert!(swing(&chorus, 15.0, 21.0).1 < 1.0e-4 && swing(&chorus, 1.6, 2.4).1 < 3.0e-4);
    // The knob at its middle, loaded by the two dividers, gives 4.15 V of
    // its 10.6 V, and the tremolo 4.44 octaves below its fastest.
    let middle = swing(&run(1, 0, 64, 127, 25.0), 0.7, 0.95);
    println!("TREMOLO, SPEED at its middle: the level moves at {:.4} Hz", middle.0);
    assert!((middle.0 / 0.8203 - 1.0).abs() < 0.02, "{}", middle.0);
    // With no depth, or with neither tab, the comb stands still.
    for still in [run(1, 0, 127, 0, 13.0), run(0, 0, 127, 127, 13.0)] {
        assert!(swing(&still, 15.0, 21.0).1 < 1.0e-4 && swing(&still, 1.6, 2.4).1 < 1.0e-4);
    }
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

/// The level in decibels of each octave band of a signal, from the band
/// centred on 125 Hz to the one on 8 kHz, and of the whole: the mean
/// square of the samples, by a discrete Fourier transform of 32,768 of
/// them under a Hann window.
fn bands(signal: &[f64]) -> ([f64; 7], f64) {
    const SIZE: usize = 1 << 15;
    let (mut real, mut imaginary) = (vec![0.0; SIZE], vec![0.0; SIZE]);
    for (index, value) in signal.iter().take(SIZE).enumerate() {
        real[index] = value * (0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / SIZE as f64).cos());
    }
    // Cooley and Tukey's transform, in place.
    let mut swapped = 0;
    for index in 1..SIZE {
        let mut bit = SIZE >> 1;
        while swapped & bit != 0 {
            swapped ^= bit;
            bit >>= 1;
        }
        swapped |= bit;
        if index < swapped {
            real.swap(index, swapped);
        }
    }
    let mut length = 2;
    while length <= SIZE {
        let angle = -std::f64::consts::TAU / length as f64;
        for start in (0..SIZE).step_by(length) {
            for offset in 0..length / 2 {
                let (sine, cosine) = (angle * offset as f64).sin_cos();
                let (low, high) = (start + offset, start + offset + length / 2);
                let (turned_real, turned_imaginary) =
                    (real[high] * cosine - imaginary[high] * sine, real[high] * sine + imaginary[high] * cosine);
                (real[high], imaginary[high]) = (real[low] - turned_real, imaginary[low] - turned_imaginary);
                (real[low], imaginary[low]) = (real[low] + turned_real, imaginary[low] + turned_imaginary);
            }
        }
        length <<= 1;
    }
    let mut levels = [0.0; 7];
    let mut whole = 0.0;
    for bin in 1..SIZE / 2 {
        let hertz = bin as f64 * RATE_HZ / SIZE as f64;
        let power = real[bin] * real[bin] + imaginary[bin] * imaginary[bin];
        whole += power;
        let band = (hertz / 125.0).log2() + 0.5;
        if (0.0..7.0).contains(&band) {
            levels[band as usize] += power;
        }
    }
    (levels.map(|power: f64| 10.0 * power.max(1.0e-30).log10()), 10.0 * whole.max(1.0e-30).log10())
}

/// The output rate in hertz.
const RATE_HZ: f64 = 48_000.0;

/// What the effect does to a held chord of STRING 1, in figures: each
/// octave band's level and the whole's with the effect on, against off,
/// and how far the level swings.
#[test]
fn the_effect_on_a_held_string_chord_in_figures() {
    use neoteletext_audio_instruments::cs80::presets::PRESETS;
    assert_eq!(PRESETS[0].0, "STRING 1");
    let chord = |effect: u8, tremolo: u8, chorus: u8| {
        let mut patch = Patch::with_tones(PRESETS[0].1, PRESETS[0].1);
        patch.set(Global::Mix, 64);
        patch.set(Global::Effect, effect);
        patch.set(Global::Tremolo, tremolo);
        patch.set(Global::Chorus, chorus);
        patch.set(Global::EffectSpeed, 127);
        patch.set(Global::EffectDepth, 127);
        let mut cs80 = instrument(&patch);
        for key in [48u8, 52, 55, 60] {
            cs80.event(NOTE_ON, key, 100);
        }
        render(&mut cs80, 8.0)[96_000..].to_vec()
    };
    let off = chord(0, 0, 0);
    let (off_bands, off_whole) = bands(&off);
    let spread = |signal: &[f64]| {
        let levels: Vec<f64> = signal.chunks(4_800).map(rms).collect();
        let (least, most) =
            levels.iter().fold((f64::MAX, 0.0f64), |(least, most), &level| (least.min(level), most.max(level)));
        decibels(most / least)
    };
    println!("STRING 1 chord, effect off: the level over tenths of a second swings {:.2} dB", spread(&off));
    for (name, signal) in [("neither tab", chord(1, 0, 0)), ("TREMOLO", chord(1, 1, 0)), ("CHORUS", chord(1, 0, 1))] {
        let (on_bands, on_whole) = bands(&signal);
        let differences: Vec<String> =
            on_bands.iter().zip(&off_bands).map(|(on, off)| format!("{:+.1}", on - off)).collect();
        println!(
            "STRING 1 chord, effect on with {name}: whole {:+.2} dB; octave bands 125 Hz to 8 kHz {} dB; the level over tenths of a second swings {:.2} dB",
            on_whole - off_whole,
            differences.join(" "),
            spread(&signal)
        );
    }
}
