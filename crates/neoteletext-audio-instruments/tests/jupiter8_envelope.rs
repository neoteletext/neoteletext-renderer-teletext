//! The Jupiter-8's envelopes and low-frequency oscillator against
//! Roland's figures: the times' documented points, the key follow, the
//! oscillator's rates, shapes and delay (TODO-6435).

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

#[path = "jupiter8_support.rs"]
mod support;

use neoteletext_audio_instruments::jupiter8::law::{KEY_FOLLOW, KEY_FOLLOW_ONE};
use neoteletext_audio_instruments::jupiter8::{Jupiter8, Param, Patch};
use support::{A2, C0, C5, NOTE_ON, cents, instrument, peaks, pitch, play, render, samples, square};

/// The window the amplifier's level is read in, in seconds.
const WINDOW: f64 = 0.0025;
/// The attack charges towards 1.3 times its peak, so it is at half its
/// peak after this share of its time: ln(1.3 / 0.8) / ln(1.3 / 0.3).
const HALF_OF_ATTACK: f64 = 0.331_1;

/// The amplifier's level through a note, as the square wave's peak in
/// each window against its peak at full level.
fn levels(patch: &Patch, key: u8, held: f64, after: f64) -> Vec<f64> {
    let full = peaks(&play(&square(), A2, 0.3, 0.0)[samples(0.1)..], 0.1)[0];
    peaks(&play(patch, key, held, after), WINDOW).iter().map(|peak| peak / full).collect()
}

/// When a level first passes a mark on its way up or down, in seconds.
fn passes(levels: &[f64], from: usize, mark: f64, rising: bool) -> f64 {
    let found = levels[from..].iter().position(|&level| if rising { level >= mark } else { level <= mark });
    (from + found.unwrap_or(0)) as f64 * WINDOW
}

#[test]
fn the_attack_is_a_millisecond_a_tenth_of_a_second_and_six_seconds() {
    // Roland's specifications: from 1 ms. Service notes, adjustment 20:
    // 100 ms at "around 4-5" (4.5 is 115 of 255), 6 s at the top.
    for (slider, seconds) in [(115u8, 0.1), (255, 6.0)] {
        let levels = levels(&square().with(Param::Env2Attack, slider), A2, seconds * 1.2, 0.0);
        let half = passes(&levels, 0, 0.5, true);
        let whole = passes(&levels, 0, 0.995, true);
        println!("attack at {slider}: half its peak after {half:.4} s, its peak after {whole:.3} s, for {seconds} s");
        assert!((half / (seconds * HALF_OF_ATTACK) - 1.0).abs() < 0.06 && (whole / seconds - 1.0).abs() < 0.06);
    }
    // At 0 the note is at its level within two milliseconds.
    let signal = play(&square(), C5, 0.1, 0.0);
    let full = support::peak(&signal[samples(0.05)..]);
    let reached = signal.iter().position(|sample| sample.abs() > 0.9 * full).unwrap_or(usize::MAX);
    println!("attack at 0: nine tenths after {:.2} ms", reached as f64 * 1000.0 / support::RATE);
    assert!(reached < samples(0.002));
}

#[test]
fn the_decay_and_the_release_fall_to_a_tenth_in_their_times() {
    // Roland's specifications: 1 ms to 10 s. Own choice: the time is the
    // fall to a tenth; own choice of the law: 131 ms at 4.5.
    let decaying = square().with(Param::Env2Sustain, 0).with(Param::Env2Decay, 255);
    let found = passes(&levels(&decaying, A2, 10.6, 0.0), 4, 0.1, false);
    println!("decay at 10: a tenth after {found:.2} s");
    assert!((found / 10.0 - 1.0).abs() < 0.03);
    for (slider, seconds) in [(115u8, 0.131), (255, 10.0)] {
        let levels = levels(&square().with(Param::Env2Release, slider), A2, 0.2, seconds * 1.1);
        let found = passes(&levels, samples(0.2) / samples(WINDOW), 0.1, false) - 0.2;
        println!("release at {slider}: a tenth after {found:.3} s, for {seconds} s");
        assert!((found / seconds - 1.0).abs() < 0.06);
    }
    // The sustain is the slider's share of the peak.
    let sustained = levels(&square().with(Param::Env2Sustain, 128).with(Param::Env2Decay, 60), A2, 1.0, 0.0);
    let level = sustained[sustained.len() - 4];
    assert!((level - 128.0 / 255.0).abs() < 0.01, "{level}");
}

/// The share of each 35 µs in which the envelope runs on a key: the
/// service notes' sawtooth, a capacitor of 3,300 pF charging through
/// 8.2 kΩ to reach 7.5 V after 35 µs, against the key's volt an octave
/// through 27 kΩ with 56 kΩ to 15 V.
fn duty(key: u8) -> f64 {
    let (tau, period, peak): (f64, f64, f64) = (8.2e3 * 3_300.0e-12, 35.0e-6, 7.5);
    let asymptote = peak / (1.0 - (-period / tau).exp());
    let volts = ((f64::from(key) - 36.0) / 12.0).max(-3.0);
    let input = (volts / 27.0e3 + 15.0 / 56.0e3) / (1.0 / 27.0e3 + 1.0 / 56.0e3);
    if input >= peak { 1.0 } else { -tau * (1.0 - input / asymptote).ln() / period }
}

#[test]
fn the_key_follow_table_is_the_circuits_formula() {
    for (key, &entry) in (0u8..).zip(&KEY_FOLLOW) {
        let wanted = (f64::from(KEY_FOLLOW_ONE) / duty(key)).round();
        assert!((f64::from(entry) - wanted).abs() <= 1.0, "key {key}: {entry} for {wanted}");
    }
    // Past the table every key has the slider's own time.
    assert!(duty(KEY_FOLLOW.len() as u8) >= 1.0 && KEY_FOLLOW.last() == Some(&(KEY_FOLLOW_ONE as u16)));
    println!(
        "times against the sliders': C0 {:.3}, C2 {:.3}, C4 {:.3}, C5 {:.3}",
        1.0 / duty(36),
        1.0 / duty(60),
        1.0 / duty(84),
        1.0
    );
}

#[test]
fn with_the_key_follow_on_higher_keys_have_shorter_times() {
    // Owner's manual, printed page 3: the attack, decay and release are
    // "shortened in proportion to the height of any pitch". The attack at
    // 4.5 and the release at 4.5, at the keyboard's ends.
    let patch = square().with(Param::Env2Attack, 115).with(Param::Env2Release, 115);
    let times = |patch: &Patch, key: u8| {
        let levels = levels(patch, key, 0.4, 0.5);
        (passes(&levels, 0, 0.5, true), passes(&levels, samples(0.4) / samples(WINDOW), 0.1, false) - 0.4)
    };
    let off = (times(&patch, C0), times(&patch, C5));
    let followed = patch.with(Param::Env2KeyFollow, 1);
    let on = (times(&followed, C0), times(&followed, C5));
    println!(
        "off: attack to half {:.4} s and {:.4} s, release {:.3} s and {:.3} s",
        off.0.0, off.1.0, off.0.1, off.1.1
    );
    println!("on:  attack to half {:.4} s and {:.4} s, release {:.3} s and {:.3} s", on.0.0, on.1.0, on.0.1, on.1.1);
    // Off, the key changes nothing; on, the lowest key's times are
    // longer by 1 / duty, 2.02, and the highest key's are the sliders'.
    let wanted = 1.0 / duty(C0);
    assert!((off.0.0 / off.1.0 - 1.0).abs() < 0.1 && (off.0.1 / off.1.1 - 1.0).abs() < 0.05);
    assert!((on.0.0 / on.1.0 / wanted - 1.0).abs() < 0.1 && (on.0.1 / on.1.1 / wanted - 1.0).abs() < 0.05);
    assert!((on.1.1 / off.1.1 - 1.0).abs() < 0.05);
}

/// The low-frequency oscillator's rate, from the pitch it moves: the
/// crossings of the pitch's own mean, the pitch read every 5 ms.
fn lfo_rate(patch: &Patch, seconds: f64) -> f64 {
    let mut jupiter = instrument(&patch.with(Param::VcoLfoMod, 128 + 40));
    jupiter.event(NOTE_ON, A2 + 24, 100);
    render(&mut jupiter, 0.3);
    let signal = render(&mut jupiter, seconds);
    let track: Vec<f64> = signal.chunks(samples(0.005)).map(|chunk| cents(1760.0, pitch(chunk))).collect();
    // The track's own rate of 200 a second stands in for the samples'.
    pitch(&track) * 200.0 / support::RATE
}

#[test]
fn the_low_frequency_oscillator_runs_from_a_twentieth_of_a_hertz_to_thirty() {
    // Roland's specifications for the foot: 0.05 Hz. Service notes,
    // adjustment 21, for the top: three cycles in 100 ms, 30 Hz, with
    // the RATE slider drawn at 10. Between them the law is exponential:
    // 0.05 Hz x 600^(128 / 255) = 1.240 Hz at the middle.
    for (slider, hertz) in [(128u8, 1.240), (255, 30.0)] {
        let found = lfo_rate(&square().with(Param::LfoRate, slider), 3.0);
        println!("rate at {slider}: {found:.3} Hz, for {hertz} Hz");
        assert!((found / hertz - 1.0).abs() < 0.03);
    }
    // At 0 a square holds each half for ten seconds.
    let slow = square().with(Param::LfoWave, 2).with(Param::VcoLfoMod, 255);
    let mut jupiter = instrument(&slow);
    jupiter.event(NOTE_ON, A2, 100);
    let track: Vec<f64> = (0..20).map(|_| pitch(&render(&mut jupiter, 1.0)[samples(0.5)..])).collect();
    let high = track.iter().filter(|&&hertz| hertz > 600.0).count();
    println!("rate at 0: the upper half lasts {high} s, then the lower");
    assert!(
        high == 10
            && track[..10].iter().all(|&hertz| hertz > 600.0)
            && track[11..20].iter().all(|&hertz| hertz < 300.0)
    );
}

#[test]
fn the_shapes_are_a_sine_a_falling_sawtooth_a_square_and_steps() {
    // The pitch through two seconds at 1.9 Hz, in cents from the note,
    // every 10 ms.
    let track = |wave: u8| {
        let patch = square().with(Param::LfoWave, wave).with(Param::LfoRate, 138).with(Param::VcoLfoMod, 128 + 40);
        let mut jupiter = instrument(&patch);
        jupiter.event(NOTE_ON, A2 + 24, 100);
        render(&mut jupiter, 0.2);
        render(&mut jupiter, 2.0).chunks(samples(0.01)).map(|chunk| cents(1760.0, pitch(chunk))).collect::<Vec<f64>>()
    };
    let falls = |track: &[f64]| track.windows(2).filter(|pair| pair[1] < pair[0] - 1.0).count();
    let rises = |track: &[f64]| track.windows(2).filter(|pair| pair[1] > pair[0] + 1.0).count();
    let (sine, sawtooth, square, random) = (track(0), track(1), track(2), track(3));
    // Owner's manual, printed page 17: "inverted Sawtooth": it falls all
    // the way and rises in a step. Service notes, adjustment 21.
    println!(
        "sawtooth: {} falls, {} rises; sine: {} and {}",
        falls(&sawtooth),
        rises(&sawtooth),
        falls(&sine),
        rises(&sine)
    );
    assert!(falls(&sawtooth) > 180 && rises(&sawtooth) <= 10);
    assert!(falls(&sine).abs_diff(rises(&sine)) < 20 && falls(&sine) > 80);
    // A square and the random steps hold their levels: few moves.
    assert!(falls(&square) + rises(&square) <= 20 && falls(&random) + rises(&random) <= 20);
    let levels = |track: &[f64]| {
        let mut rounded: Vec<i64> = track.iter().map(|cents| (cents / 5.0).round() as i64).collect();
        rounded.sort_unstable();
        rounded.dedup();
        rounded.len()
    };
    assert!(levels(&random) >= 3, "{}", levels(&random));
}

#[test]
fn the_delay_is_four_seconds_at_its_top() {
    // Roland's specifications: 0 to 4 s. Service notes, adjustment 21:
    // "modulated approx. 4 sec after the key first depressed". The
    // oscillator's swing of the pitch in each half second.
    let patch = square().with(Param::LfoDelay, 255).with(Param::LfoRate, 200).with(Param::VcoLfoMod, 128 + 40);
    let mut jupiter = instrument(&patch);
    jupiter.event(NOTE_ON, A2 + 24, 100);
    let swings: Vec<f64> = (0..14)
        .map(|_| {
            let track: Vec<f64> =
                render(&mut jupiter, 0.5).chunks(samples(0.005)).map(|chunk| cents(1760.0, pitch(chunk))).collect();
            let inner = &track[2..track.len() - 2];
            inner.iter().copied().fold(f64::MIN, f64::max) - inner.iter().copied().fold(f64::MAX, f64::min)
        })
        .collect();
    println!("the swing in cents, each half second: {swings:.0?}");
    let full = swings[13];
    // Still until four seconds, then rising for two (own choice), then
    // full.
    assert!(swings[..7].iter().all(|&swing| swing < 0.03 * full));
    assert!(swings[8] > 0.05 * full && swings[9] < 0.9 * full && swings[12] > 0.95 * full);
}

#[test]
fn a_shorter_delay_set_while_waiting_shortens_the_wait() {
    // The delay at its top, 4 s, and a key; a tenth of a second later
    // the panel's DELAY TIME is brought down to 0.2, 80 ms: the
    // modulation is there within its rise, not after the 4 s that were
    // running. The oscillator's swing of the pitch in each half second.
    let slow = square().with(Param::LfoDelay, 255).with(Param::LfoRate, 200).with(Param::VcoLfoMod, 128 + 40);
    let swings = |jupiter: &mut Jupiter8| -> Vec<f64> {
        (0..3)
            .map(|_| {
                let track: Vec<f64> =
                    render(jupiter, 0.5).chunks(samples(0.005)).map(|chunk| cents(1760.0, pitch(chunk))).collect();
                let inner = &track[2..track.len() - 2];
                inner.iter().copied().fold(f64::MIN, f64::max) - inner.iter().copied().fold(f64::MAX, f64::min)
            })
            .collect()
    };
    let mut steady = instrument(&slow.with(Param::LfoDelay, 0));
    steady.event(NOTE_ON, A2 + 24, 100);
    let full = swings(&mut steady)[2];
    let mut jupiter = instrument(&slow);
    jupiter.event(NOTE_ON, A2 + 24, 100);
    render(&mut jupiter, 0.1);
    jupiter.set_patch(&slow.with(Param::LfoDelay, 5));
    let found = swings(&mut jupiter);
    println!("the swing in cents, each half second after the delay is shortened: {found:.0?}; in full {full:.0}");
    assert!(found.iter().all(|&swing| swing > 0.95 * full), "{found:?} against {full}");
    // A longer delay set while waiting does not start the wait again,
    // and none at all ends it.
    let mut jupiter = instrument(&slow.with(Param::LfoDelay, 64));
    jupiter.event(NOTE_ON, A2 + 24, 100);
    render(&mut jupiter, 0.1);
    jupiter.set_patch(&slow);
    render(&mut jupiter, 3.0);
    let late = swings(&mut jupiter)[2];
    jupiter.set_patch(&slow.with(Param::LfoDelay, 0));
    let none = swings(&mut jupiter)[0];
    assert!(late > 0.95 * full && none > 0.95 * full, "{late} and {none} against {full}");
}

#[test]
fn the_oscillator_moves_the_amplifier_by_its_switch() {
    // Owner's manual, printed page 20: none at 0, "three progressively
    // deeper Levels" at 1, 2 and 3. Service notes, module controller
    // schematic, VCA MOD: the switch leaves open a path of 47 kΩ and
    // 470 kΩ, one of 47 kΩ and 180 kΩ, or those two and a second of
    // 517 kΩ together. Own choice of the depth at 3: all of the level,
    // either way. A square at 1.9 Hz holds each level for a quarter of a
    // second.
    let swing = |position: u8| {
        let patch = square()
            .with(Param::LfoWave, 2)
            .with(Param::LfoRate, 138)
            .with(Param::VcaLfoMod, 128 + position * 42 + position / 3);
        let levels = peaks(&play(&patch, A2, 1.5, 0.0)[samples(0.2)..], 0.02);
        let (high, low) =
            (levels.iter().copied().fold(f64::MIN, f64::max), levels.iter().copied().fold(f64::MAX, f64::min));
        (high - low) / (high + low)
    };
    let swings = [0u8, 1, 2, 3].map(swing);
    println!("the amplifier's swing as a share of its level, positions 0 to 3: {swings:.3?}");
    let (least, middle) = (1.0 / 517.0, 1.0 / 227.0);
    let all = 2.0 * least + middle;
    for (found, wanted) in swings.iter().zip([0.0, least / all, middle / all, 1.0]) {
        assert!((found - wanted).abs() < 0.015, "{found} for {wanted}");
    }
}
