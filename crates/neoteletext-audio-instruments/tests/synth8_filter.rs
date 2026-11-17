//! synth8's filters against the service notes' adjustments: the
//! cutoff's law, the key follow, the envelope's six octaves, the two
//! slopes, the resonance that rings and does not oscillate, the
//! high-pass filter; and the low-pass filter against synth106's, which
//! is the same chip (TODO-6435).

// Without the feature there is nothing here to test.
#![cfg(feature = "synth8")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "synth8_support.rs"]
mod support;

use neoteletext_audio_instruments::synth8::{Param, Patch};
use support::{
    A2, C0, C2, C4, bin_hertz, cents, component, decibels, pitch, play, rms, samples, second, spectrum, square,
};

/// Samples of a spectrum: 1.4 s.
const LENGTH: usize = 1 << 16;
/// The resonance at the slider's top as the loop's feedback, and the
/// share of the cutoff that the ringing is at: the fourth root of the
/// feedback over four.
const RESONANCE_TOP: f64 = 3.95;

/// The frequency a patch's filter rings at: noise through it with the
/// resonance at the top, and the middle of the spectrum's peak, each bin
/// taken with its neighbours over a twentieth of an octave.
fn ringing(patch: &Patch, key: u8) -> f64 {
    let patch =
        patch.with(Param::MixVco1, 0).with(Param::MixVco2, 255).with(Param::Vco2Wave, 3).with(Param::VcfResonance, 255);
    let (mut sums, runs) = (vec![0.0f64; LENGTH / 2], 3usize);
    let signal = play(&patch, key, 0.3 + (runs * LENGTH) as f64 / support::RATE, 0.0);
    for run in 0..runs {
        let from = samples(0.25) + run * LENGTH;
        for (sum, level) in sums.iter_mut().zip(spectrum(&signal[from..], LENGTH)) {
            *sum += level * level;
        }
    }
    let width = bin_hertz(&sums);
    let smoothed = |bin: usize| {
        let reach = ((bin as f64) * 0.035).max(2.0) as usize;
        let window = &sums[bin.saturating_sub(reach)..(bin + reach).min(sums.len())];
        window.iter().sum::<f64>() / window.len() as f64
    };
    let first = (20.0 / width) as usize;
    let last = (20_000.0 / width) as usize;
    let best = (first..last).max_by(|&a, &b| smoothed(a).total_cmp(&smoothed(b))).unwrap_or(first);
    best as f64 * width
}

/// The cutoff the law gives for a slider's byte, a key and a key follow
/// in hundredths: 36.8 Hz at 0, 8.966 octaves to 255, about middle C.
fn law(slider: u8, key: u8, follow: f64) -> f64 {
    36.8 * (8.966 * f64::from(slider) / 255.0 + follow / 100.0 * (f64::from(key) - 60.0) / 12.0).exp2()
}

/// A slider's byte for a position of its scale of 0 to 10.
fn position(tenths: u32) -> u8 {
    ((255 * tenths + 50) / 100) as u8
}

#[test]
fn the_cutoff_follows_the_service_notes_figures() {
    let ring = (RESONANCE_TOP / 4.0f64).powf(0.25);
    // Adjustment 16: the slider at 4, any key, 442 Hz. Adjustment 14:
    // from 50 Hz at the foot to 25 kHz at the top, 8.97 octaves, of
    // which each division is 0.9; the top itself is out of the hearing
    // this measures in.
    let patch = Patch::DEFAULT;
    println!("slider  ringing   the law's cutoff times {ring:.4}");
    for tenths in [20u32, 40, 60, 80, 90] {
        let slider = position(tenths);
        let (found, wanted) = (ringing(&patch.with(Param::VcfCutoff, slider), A2), law(slider, A2, 0.0) * ring);
        println!(
            "{:4.1}  {found:9.1} Hz {wanted:9.1} Hz  {:6.1} cents",
            f64::from(tenths) / 10.0,
            cents(wanted, found)
        );
        assert!(cents(wanted, found).abs() < 60.0);
    }
    let at_four = law(position(40), A2, 0.0);
    println!("the law at 4: {at_four:.1} Hz (adjustment 16: 442 Hz, less the slider's nearest byte)");
    assert!(cents(442.0, at_four).abs() < 25.0);
}

#[test]
fn the_key_follow_tracks_the_keyboard_and_turns_about_middle_c() {
    // Adjustment 13: with the KEY FOLLOW slider where its drawing has
    // it, at 7.5, the key C4 rings at four times the key C2's frequency:
    // 100%.
    let followed = Patch::DEFAULT.with(Param::VcfCutoff, position(40)).at(Param::VcfKeyFollow, 75);
    let (low, high) = (ringing(&followed, C2), ringing(&followed, C4));
    println!("C2 {low:.1} Hz, C4 {high:.1} Hz: {:.3} octaves", (high / low).log2());
    assert!(((high / low).log2() - 2.0).abs() < 0.06);
    // Owner's manual: the key follow's "center point is center C": there
    // the slider changes nothing.
    let unfollowed = ringing(&followed.with(Param::VcfKeyFollow, 128), C2);
    assert!(cents(unfollowed, low).abs() < 40.0);
    // At its top, the specifications' 120%; and half way to 7.5, half
    // of all.
    let most = followed.with(Param::VcfKeyFollow, 255);
    let octaves = (ringing(&most, C4) / ringing(&most, C2)).log2();
    println!("at the slider's top: {octaves:.3} octaves for two");
    assert!((octaves - 2.4).abs() < 0.06);
    let half = followed.at(Param::VcfKeyFollow, 37);
    let octaves = (ringing(&half, C4) / ringing(&half, C2)).log2();
    println!("at 3.7: {octaves:.3} octaves for two");
    assert!((octaves - 1.0).abs() < 0.06);
    // Upside down, the table's other half: as far the other way.
    let down = followed.with(Param::VcfKeyFollow, 128 - 95);
    let octaves = (ringing(&down, C4) / ringing(&down, C2)).log2();
    println!("upside down at 7.5: {octaves:.3} octaves for two");
    assert!((octaves + 2.0).abs() < 0.06);
}

#[test]
fn the_envelope_moves_the_cutoff_six_octaves_at_its_top() {
    // Adjustment 15: 500 Hz becomes 32 kHz with ENV MOD at 10. Half the
    // slider is measured, which stays in hearing: three octaves, from
    // the envelope at full sustain.
    let base = Patch::DEFAULT.with(Param::VcfCutoff, position(40));
    let (without, with) = (ringing(&base, A2), ringing(&base.with(Param::VcfEnvMod, 128 + 64), A2));
    let octaves = (with / without).log2();
    println!("ENV MOD at half: {octaves:.3} octaves (64 of 127 steps of six: 3.024)");
    assert!((octaves - 6.0 * 64.0 / 127.0).abs() < 0.06);
    // Upside down, the envelope moves it down as far.
    let down = ringing(
        &base.with(Param::VcfCutoff, position(80)).with(Param::VcfEnvMod, 128 + 64).with(Param::Env1Polarity, 1),
        A2,
    );
    let octaves = (down / ringing(&base.with(Param::VcfCutoff, position(80)), A2)).log2();
    assert!((octaves + 6.0 * 64.0 / 127.0).abs() < 0.06, "{octaves}");
}

#[test]
fn the_two_slopes_are_two_poles_and_four() {
    // VCO-2's sine two octaves above the cutoff against the same sine
    // far below it. Each pole takes 1 / sqrt(1 + 16) there, 12.30 dB.
    let cutoff = position(40);
    let level = |slope: u8, key: u8| {
        let patch = second(0).with(Param::VcfCutoff, cutoff).with(Param::VcfSlope, slope);
        let signal = play(&patch, key, 1.3, 0.0)[samples(0.25)..].to_vec();
        component(&signal, pitch(&signal))
    };
    let hertz = |key: u8| 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
    let poles = |key: u8| -10.0 * (1.0 + (hertz(key) / law(cutoff, key, 0.0)).powi(2)).log10();
    for (slope, count, name) in [(0u8, 4.0, "24 dB"), (1, 2.0, "12 dB")] {
        let found = decibels(level(slope, A2 + 24), level(slope, C0));
        let wanted = count * (poles(A2 + 24) - poles(C0));
        println!("{name} an octave: {found:.2} dB two octaves above the cutoff, for {wanted:.2} dB");
        assert!((found - wanted).abs() < 0.8);
    }
}

#[test]
fn the_resonance_at_its_top_rings_and_does_not_oscillate() {
    // Service notes, adjustment 17: a burst of ringing after each edge
    // of a square wave, dying to a quarter before the next edge. With
    // no oscillator in the mixer the filter is silent.
    let silent =
        Patch::DEFAULT.with(Param::MixVco1, 0).with(Param::VcfResonance, 255).with(Param::VcfCutoff, position(60));
    let level = rms(&play(&silent, A2, 1.0, 0.0)[samples(0.5)..]);
    println!("no oscillator, the resonance at 10: {level:.6} of full scale");
    assert!(level < 1.0e-4);

    // A square of 32.7 Hz at 16' leaves 15 ms between edges; the cutoff
    // at 6 rings at 1.5 kHz. The ringing's size is half the swing within
    // one of its cycles, the square's own level being flat there.
    let patch = square().with(Param::Vco1Range, 0).with(Param::VcfResonance, 255).with(Param::VcfCutoff, position(60));
    let signal = play(&patch, C0, 1.0, 0.0);
    let steady = &signal[samples(0.5)..];
    let ring = law(position(60), A2, 0.0) * (RESONANCE_TOP / 4.0f64).powf(0.25);
    let cycle = (support::RATE / ring).ceil() as usize;
    // An edge: the largest step between two samples.
    let edge = (1..steady.len() - samples(0.02))
        .max_by(|&a, &b| (steady[a] - steady[a - 1]).abs().total_cmp(&(steady[b] - steady[b - 1]).abs()));
    let edge = edge.unwrap_or(1);
    let swing = |cycles: f64| {
        let from = edge + (cycles * support::RATE / ring) as usize;
        let window = &steady[from..from + cycle];
        window.iter().copied().fold(f64::MIN, f64::max) - window.iter().copied().fold(f64::MAX, f64::min)
    };
    let (early, late) = (swing(3.0), swing(11.0));
    // The cycles in which it falls to a quarter, from eight cycles.
    let to_a_quarter = 8.0 * 4.0f64.ln() / (early / late).ln();
    println!(
        "the ringing falls to a quarter in {to_a_quarter:.1} cycles ({:.1} dB a cycle)",
        decibels(late, early) / 8.0
    );
    // The figure shows a burst of 10 to 20 cycles. The square's own level
    // stands in the feedback amplifier and takes a twelfth off its gain,
    // so the ringing dies sooner than the feedback of 3.95 by itself
    // would have it (some 80 cycles).
    assert!((8.0..=20.0).contains(&to_a_quarter));
}

#[test]
fn the_high_pass_filter_is_off_at_its_foot_and_one_pole_above() {
    // Owner's manual, printed page 18: unchanged at "Low"; "-6dB/octave".
    // Own choice of the range: 5 kHz at the top, 20 Hz at the slider's
    // first step.
    let level = |slider: u8, key: u8| {
        let signal = play(&second(0).with(Param::HpfCutoff, slider), key, 1.3, 0.0)[samples(0.25)..].to_vec();
        component(&signal, pitch(&signal))
    };
    let flat = level(0, A2);
    // At 2.09 kHz and 1.05 kHz, below the top's 5 kHz, one pole takes
    // 8.3 dB and 13.8 dB.
    let (octave, two) = (decibels(level(255, A2 + 27), flat), decibels(level(255, A2 + 15), flat));
    println!("the slider at 10: {octave:.2} dB at 2.09 kHz, {two:.2} dB at 1.05 kHz");
    let pole = |hertz: f64| -10.0 * (1.0 + (5_000.0 / hertz).powi(2)).log10();
    let (upper, lower) = (440.0 * (27.0f64 / 12.0).exp2(), 440.0 * (15.0f64 / 12.0).exp2());
    // The low-pass filter, open at 18 kHz, takes a little of its own.
    assert!((octave - pole(upper)).abs() < 0.6 && (two - pole(lower)).abs() < 0.6);
    assert!(decibels(level(1, A2), flat).abs() < 0.1);
    // Far below the cutoff the slope is one pole's 6 dB an octave: the
    // lowest key's 65.4 Hz, six octaves and a quarter under 5 kHz, is
    // 37.7 dB down.
    let lowest = 440.0 * (f64::from(C0) - f64::from(A2)).exp2().powf(1.0 / 12.0);
    let tone = |slider: u8| {
        let signal = play(&second(0).with(Param::HpfCutoff, slider), C0, 1.3, 0.0)[samples(0.25)..].to_vec();
        component(&signal, lowest)
    };
    let foot = decibels(tone(255), tone(0));
    println!("the slider at 10: {foot:.2} dB at {lowest:.1} Hz, for {:.2} dB", pole(lowest));
    assert!((foot - pole(lowest)).abs() < 0.6);
}

/// synth106's filter is the same chip in the same circuit, and both
/// instruments are made from the same building block: with the cutoffs
/// set alike, a sawtooth comes out of both with the same harmonics.
#[cfg(feature = "synth106")]
#[test]
fn the_low_pass_filter_is_the_synth106s() {
    use neoteletext_audio_instruments::synth106::{Model, Patch as Synth106Patch, Slider, Synth106};

    // synth60's law: 6.94 Hz at 0 and 17.2 octaves to 127; its
    // slider at 53 is 1,006 Hz. synth8's at 136 is 1,012 Hz.
    let (synth106_slider, synth8_slider) = (53u8, 136u8);
    let synth106_cutoff = 6.94 * (17.2 * f64::from(synth106_slider) / 127.0).exp2();
    let synth8_cutoff = law(synth8_slider, A2, 0.0);
    println!("cutoffs: synth60 {synth106_cutoff:.0} Hz, synth8 {synth8_cutoff:.0} Hz");

    let mut synth106_patch = Synth106Patch::DEFAULT.with(Slider::VcfFrequency, synth106_slider);
    synth106_patch.hpf = 0;
    let mut synth106 = Synth106::new(Model::Synth60);
    synth106.set_patch(&synth106_patch);
    synth106.event(support::NOTE_ON, A2 - 24, 100);
    let mut out = vec![0i16; samples(1.5)];
    synth106.render(&mut out);
    let synth106_signal: Vec<f64> = out[samples(0.4)..].iter().map(|&sample| f64::from(sample) / 32_768.0).collect();
    let synth8_signal =
        play(&Patch::DEFAULT.with(Param::VcfCutoff, synth8_slider), A2 - 24, 1.5, 0.0)[samples(0.4)..].to_vec();

    let harmonics = |signal: &[f64]| {
        let fundamental = pitch(signal);
        let first = component(signal, fundamental);
        (2..=30).map(|number| decibels(component(signal, fundamental * f64::from(number)), first)).collect::<Vec<f64>>()
    };
    let (synth106_levels, synth8_levels) = (harmonics(&synth106_signal), harmonics(&synth8_signal));
    let mut worst = 0.0f64;
    for (number, (synth106_level, synth8_level)) in (2..).zip(synth106_levels.iter().zip(&synth8_levels)) {
        if number % 7 == 2 {
            println!("harmonic {number:2}: synth60 {synth106_level:7.2} dB, synth8 {synth8_level:7.2} dB");
        }
        worst = worst.max((synth106_level - synth8_level).abs());
    }
    println!("the largest difference over 29 harmonics, to 3.3 kHz: {worst:.2} dB");
    // The two cutoffs are 10 cents apart, and each voice's trim is up to
    // 10 cents: a quarter of a decibel on the filter's slope.
    assert!(worst < 0.6);
}
