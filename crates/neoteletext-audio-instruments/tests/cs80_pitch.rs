//! The CS-80's pitch: every key against Yamaha's table of key voltage and
//! frequency, the stops, the detuning, the ribbon, the glide and the
//! vibrato, and the sine's purity (TODO-6434).

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
    CONTROL, NOTE_OFF, NOTE_ON, PITCH_BEND, SINE, cents, component, decibels, instrument, panel, pitch, play, render,
    samples,
};
use neoteletext_audio_instruments::cs80::{Control, Global, Patch};

/// The frequency of a key at 8' in equal temperament from an A of 440 Hz.
fn tempered(key: u8) -> f64 {
    440.0 * ((f64::from(key) - 69.0) / 12.0).exp2()
}

/// The pitch a patch gives a key, held a second.
fn pitch_of(patch: &Patch, key: u8) -> f64 {
    pitch(&play(patch, key, 100, 1.0, 0.0)[4_800..])
}

#[test]
fn every_key_is_within_a_cent_of_yamahas_table() {
    let patch = panel(&SINE);
    let mut worst = 0.0f64;
    // Each key takes the next voice, so every oscillator is heard.
    for key in 36..=96u8 {
        let error = cents(tempered(key), pitch_of(&patch, key));
        worst = worst.max(error.abs());
        assert!(error.abs() < 1.0, "key {key}: {error} cents");
    }
    println!("the 61 keys: within {worst:.2} cents of equal temperament");
    // All sixteen oscillators, each a voice's turn on each channel.
    for mix in [0u8, 127] {
        let mut both = Patch::DEFAULT;
        both.tones = [patch.tones[0]; 2];
        both.set(Global::Mix, mix);
        let mut cs80 = instrument(&both);
        for key in 60..68u8 {
            cs80.event(NOTE_ON, key, 100);
            let error = cents(tempered(key), pitch(&render(&mut cs80, 1.0)[4_800..]));
            cs80.event(NOTE_OFF, key, 0);
            render(&mut cs80, 0.2);
            worst = worst.max(error.abs());
            assert!(error.abs() < 1.0, "key {key}, mix {mix}: {error} cents");
        }
    }
    println!("the sixteen oscillators: within {worst:.2} cents");
    // Service manual, page 42, VCO III, the table under pin 2: key
    // voltage against frequency, as printed.
    for (key, hertz) in [(48u8, 130.8), (60, 261.6), (72, 523.2), (84, 1046.0), (96, 2093.0)] {
        let found = pitch_of(&patch, key);
        println!("key {key}: {found:.2} Hz for Yamaha's {hertz}");
        assert!((found / hertz - 1.0).abs() < 1.0e-3, "key {key}: {found} Hz");
    }
}

#[test]
fn the_stops_are_whole_shares_of_eight_feet() {
    // Channel I's oscillator of the first voice at each stop, against
    // the same oscillator at 8': the fifths are pure, 3 to 2.
    let at = |feet: u8| {
        let mut patch = panel(&SINE);
        patch.set(Global::FeetOne, feet);
        pitch_of(&patch, 57)
    };
    let eight = at(1);
    for (feet, ratio, name) in
        [(0u8, 0.5, "16'"), (2, 1.5, "5 1/3'"), (3, 2.0, "4'"), (4, 3.0, "2 2/3'"), (5, 4.0, "2'")]
    {
        let found = at(feet) / eight;
        println!("{name}: {found:.5} of 8'");
        assert!((found / ratio - 1.0).abs() < 5.0e-5, "{name}: {found}");
    }
}

#[test]
fn the_detune_lever_moves_channel_two_alone() {
    let at = |detune: u8, mix: u8| {
        let mut patch = Patch::DEFAULT;
        for tone in &mut patch.tones {
            for (control, value) in SINE {
                tone.set(control, value);
            }
        }
        patch.set(Global::Detune, detune);
        patch.set(Global::Mix, mix);
        pitch_of(&patch, 69)
    };
    let (one, two) = (at(64, 0), at(64, 127));
    let (flat, sharp) = (cents(two, at(0, 127)), cents(two, at(127, 127)));
    println!(
        "channel II by the detune lever: {flat:.1} to {sharp:.1} cents; channel I moves {:.2}",
        cents(one, at(127, 0))
    );
    assert!((flat + 100.0).abs() < 0.5 && (sharp - 100.0).abs() < 0.5);
    assert!(cents(one, at(127, 0)).abs() < 0.01);
}

#[test]
fn the_ribbon_multiplies_the_frequency() {
    let bent = |bend: u16| {
        let mut cs80 = instrument(&panel(&SINE));
        cs80.event(PITCH_BEND, (bend & 0x7f) as u8, (bend >> 7) as u8);
        cs80.event(NOTE_ON, 69, 100);
        pitch(&render(&mut cs80, 1.0)[4_800..])
    };
    let rest = bent(0x2000);
    // 1 + b: full up is an octave, half down an octave, and the ends of
    // a straight line in frequency between.
    for (bend, factor) in
        [(0x3fffu16, 2.0 - 1.0 / 8192.0), (0x3000, 1.5), (0x1000, 0.5), (0x0800, 0.25), (0x0080, 1.0 / 64.0)]
    {
        let found = bent(bend) / rest;
        println!("pitch bend {bend:#06x}: {found:.5} times the frequency");
        assert!((found / factor - 1.0).abs() < 2.0e-4, "{bend:#x}: {found}");
    }
    // Full down is held eight octaves below.
    let floor = bent(0) / rest;
    assert!((floor * 256.0 - 1.0).abs() < 0.01, "{floor}");
}

/// The lengths of a tone's cycles, each from one rising crossing to the
/// next, as the times in seconds of their starts and their frequencies.
fn cycles(signal: &[f64]) -> Vec<(f64, f64)> {
    let mut crossings = Vec::new();
    for (index, pair) in signal.windows(2).enumerate() {
        if pair[0] < 0.0 && pair[1] >= 0.0 {
            crossings.push((index as f64 + pair[0] / (pair[0] - pair[1])) / cs80_support::RATE);
        }
    }
    crossings.windows(2).map(|pair| (pair[0], 1.0 / (pair[1] - pair[0]))).collect()
}

#[test]
fn the_glide_takes_two_seconds_an_octave_at_its_longest_and_steps_in_semitones() {
    // One voice at a time: the second key is the first's again after
    // seven others, so each key is played on the voice that glides.
    let glide = |time: u8, glissando: u8| {
        let mut patch = panel(&SINE);
        patch.set(Global::GlideTime, time);
        patch.set(Global::Glissando, glissando);
        let mut cs80 = instrument(&patch);
        // Eight keys, one for each voice in turn, the first long enough
        // to arrive; the ninth key is then the first voice's again.
        for (key, seconds) in
            [(57u8, 3.0), (40, 0.01), (41, 0.01), (42, 0.01), (43, 0.01), (44, 0.01), (45, 0.01), (46, 0.01)]
        {
            cs80.event(NOTE_ON, key, 100);
            render(&mut cs80, seconds);
            cs80.event(NOTE_OFF, key, 0);
            render(&mut cs80, 0.2);
            assert_eq!(cs80.sounding_voices(), 0);
        }
        cs80.event(NOTE_ON, 69, 100);
        // From the first whole cycle after the amplifier has opened.
        cycles(&render(&mut cs80, 3.0)).into_iter().filter(|&(time, _)| time > 0.004).collect::<Vec<_>>()
    };
    // Adjustment procedure, page 27: the clock's period at LONG is 0.16
    // s, a semitone; an octave is 1.92 s.
    let long = glide(127, 0);
    let arrival = long.iter().find(|&&(_, hertz)| hertz > 440.0 * (-0.5f64 / 12.0).exp2()).expect("it arrives").0;
    let halfway = long.iter().find(|&&(_, hertz)| hertz > 220.0 * 2.0f64.sqrt()).expect("it passes the tritone").0;
    println!("the longest portamento: an octave in {arrival:.2} s, half of it in {halfway:.2} s");
    assert!((arrival - 1.84).abs() < 0.1 && (halfway - 0.96).abs() < 0.05);
    // At SHORT the clock's period is 0.17 ms: an octave in 2 ms.
    let short = glide(0, 0);
    assert!(short.iter().all(|&(time, hertz)| time < 0.01 || (hertz / 440.0 - 1.0).abs() < 0.002));
    // A glissando dwells on each semitone.
    let stepped = glide(127, 1);
    let mut steps = Vec::new();
    let mut between = 0usize;
    for &(_, hertz) in &stepped {
        let semitone = 12.0 * (hertz / 220.0).log2();
        // The one cycle in which a step falls is neither note.
        if (semitone - semitone.round()).abs() > 0.05 {
            between += 1;
        } else if steps.last() != Some(&(semitone.round() as i32)) {
            steps.push(semitone.round() as i32);
        }
    }
    assert!(between <= 12, "{between} cycles between semitones");
    println!("the longest glissando passes {steps:?} semitones above the A it left");
    assert_eq!(steps, (0..=12).collect::<Vec<_>>());
}

#[test]
fn the_sub_oscillator_the_wheel_and_pressure_move_the_pitch() {
    let swing = |patch: &Patch, wheel: u8, pressure: u8| {
        let mut cs80 = instrument(patch);
        cs80.event(CONTROL, 1, wheel);
        cs80.event(NOTE_ON, 69, 100);
        cs80.event(cs80_support::KEY_PRESSURE, 69, pressure);
        let found = cycles(&render(&mut cs80, 1.5)[4_800..]);
        let (low, high) =
            found.iter().fold((f64::INFINITY, 0.0f64), |(low, high), &(_, hertz)| (low.min(hertz), high.max(hertz)));
        cents(low, high)
    };
    let mut patch = panel(&SINE);
    patch.set(Global::SubSpeed, 64);
    let still = swing(&patch, 0, 0);
    let wheel = swing(&patch, 127, 0);
    // The VCO lever's position is squared: half way is a quarter of an
    // octave either way.
    patch.set(Global::SubVco, 64);
    let lever = swing(&patch, 0, 0);
    patch.set(Global::SubVco, 0);
    patch.set(Global::TouchVco, 127);
    let (light, pressed) = (swing(&patch, 0, 0), swing(&patch, 0, 127));
    println!(
        "vibrato, peak to peak: none {still:.1}, wheel {wheel:.1}, lever at half {lever:.0}, pressure {pressed:.0} cents"
    );
    assert!(still < 1.0 && light < 1.0);
    assert!((wheel - 100.0).abs() < 5.0 && (pressed - 200.0).abs() < 10.0);
    assert!((lever - 2.0 * 1200.0 * (64.0f64 / 127.0).powi(2)).abs() < 30.0);
}

#[test]
fn a_hard_stroke_starts_flat_and_comes_back() {
    let mut patch = panel(&SINE);
    patch.set(Global::TouchPitchBend, 127);
    let hard = cycles(&play(&patch, 81, 127, 0.5, 0.0));
    let soft = cycles(&play(&patch, 81, 1, 0.5, 0.0));
    // The first whole cycle after the amplifier has opened.
    let start = |found: &[(f64, f64)]| cents(880.0, found.iter().find(|&&(time, _)| time > 0.004).expect("a cycle").1);
    let end = |found: &[(f64, f64)]| cents(880.0, found[found.len() - 2].1);
    println!("PITCHBEND at 10: a hard stroke starts {:.0} cents off, a soft one {:.0}", start(&hard), start(&soft));
    assert!(start(&hard) < -150.0 && start(&soft).abs() < 5.0);
    assert!(end(&hard).abs() < 2.0);
}

#[test]
fn the_sine_is_a_sine() {
    let signal = play(&panel(&SINE), 69, 100, 1.5, 0.0);
    let steady = &signal[24_000..];
    let hertz = pitch(steady);
    let fundamental = component(steady, hertz);
    let worst = (2..=9).map(|harmonic| component(steady, hertz * f64::from(harmonic))).fold(0.0f64, f64::max);
    println!(
        "the sine: {fundamental:.4} of full scale, its strongest harmonic {:.1} dB down",
        -decibels(worst / fundamental)
    );
    // One line's sine at full level peaks 18 dB below full scale.
    assert!((decibels(fundamental) + 18.06).abs() < 0.1);
    assert!(decibels(worst / fundamental) < -60.0);
    let _ = Control::Sine;
}

#[test]
fn with_the_glide_lever_at_its_foot_a_voice_takes_its_key_at_once() {
    // Service manual, page 15, the D-A converter's pin 3: the glide works
    // "when the portament VR is turned on". With the lever at its foot a
    // voice that last played the lowest C went up to the highest in
    // semitones of 0.17 ms, a chirp of 10 ms from 65 Hz to 2,093 Hz.
    let patch = panel(&SINE);
    let mut cs80 = instrument(&patch);
    for key in 36..44 {
        cs80.event(NOTE_ON, key, 100);
        let _ = render(&mut cs80, 0.01);
        cs80.event(NOTE_OFF, key, 0);
    }
    let _ = render(&mut cs80, 0.5);
    // The ninth key takes the first voice again.
    cs80.event(NOTE_ON, 96, 100);
    let start = render(&mut cs80, 0.006);
    let found = pitch(&start[samples(0.001)..]);
    assert!(cents(tempered(96), found).abs() < 5.0, "{found} Hz");
}
