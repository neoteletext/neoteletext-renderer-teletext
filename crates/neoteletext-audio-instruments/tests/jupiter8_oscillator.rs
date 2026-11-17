//! The Jupiter-8's oscillators: their waveforms, the sync, the cross
//! modulation's depth, and what aliasing the cross modulation leaves
//! (TODO-6435).

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

use neoteletext_audio_instruments::jupiter8::{Param, Patch};
use support::{
    A2, C0, C2, bin_hertz, cents, component, decibels, off_the_grid, peak, pitch, play, samples, second,
    sound_and_the_rest, spectrum, square, strongest,
};

/// Samples of a spectrum: 1.4 s, bins of 0.73 Hz.
const LENGTH: usize = 1 << 16;
/// The top of what is heard, in hertz.
const AUDIBLE: f64 = 20_000.0;

/// A patch's steady tone on a key, after its start.
fn tone(patch: &Patch, key: u8) -> Vec<f64> {
    play(patch, key, 0.3 + LENGTH as f64 / support::RATE, 0.0)[samples(0.25)..].to_vec()
}

#[test]
fn each_waveform_has_its_harmonics() {
    // Each at its own pitch, which the voice's tolerance puts a fraction
    // of a cent off the scale.
    let level = |patch: &Patch, harmonic: f64| {
        let signal = tone(patch, A2);
        component(&signal[..samples(1.0)], pitch(&signal) * harmonic)
    };
    // A sawtooth: every harmonic at 1 / n. A square: the odd ones at
    // 1 / n, the even ones absent. A triangle: the odd ones at 1 / n².
    // The filter, open at 18 kHz, takes a little off the fifth.
    let sawtooth = Patch::DEFAULT;
    let first = level(&sawtooth, 1.0);
    for harmonic in [2.0, 3.0, 5.0] {
        let found = decibels(level(&sawtooth, harmonic), first);
        println!("sawtooth, harmonic {harmonic}: {found:.2} dB");
        assert!((found + decibels(harmonic, 1.0)).abs() < 0.6, "sawtooth {harmonic}: {found} dB");
    }
    let first = level(&square(), 1.0);
    assert!(decibels(level(&square(), 2.0), first) < -60.0);
    assert!((decibels(level(&square(), 3.0), first) + decibels(3.0, 1.0)).abs() < 0.6);
    let triangle = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    let first = level(&triangle, 1.0);
    assert!((decibels(level(&triangle, 3.0), first) + decibels(9.0, 1.0)).abs() < 0.6);
    // The sine: its strongest overtone.
    let first = level(&second(0), 1.0);
    let worst = (2..=9).map(|harmonic| level(&second(0), f64::from(harmonic))).fold(0.0f64, f64::max);
    println!("the sine's strongest overtone: {:.1} dB", decibels(worst, first));
    assert!(decibels(worst, first) < -60.0);
    // Service notes, adjustment 12: both oscillators at the same level.
    let levels = (peak(&tone(&Patch::DEFAULT, A2)), peak(&tone(&second(1), A2)));
    assert!(decibels(levels.0, levels.1).abs() < 0.3, "{levels:?}");
}

#[test]
fn the_pulse_narrows_to_thirty_microseconds_at_middle_c() {
    // Service notes, adjustment 11: "Press C2 key ... for 30µs space
    // width". Measured as the pulse's mean: a pulse low for a share s of
    // its cycle between -1 and 1 has a mean of 1 - 2s. The amplifier
    // passes the mean, the high-pass filter being off.
    let pulse = |width: u8| Patch::DEFAULT.with(Param::Vco1Wave, 2).with(Param::PwmDepth, width);
    let mean = |signal: &[f64]| signal.iter().sum::<f64>() / signal.len() as f64;
    let square = tone(&pulse(0), C2);
    let narrow = tone(&pulse(255), C2);
    // The pulse's 1 is the square's, whose fundamental is 4 / π of it.
    let unit = component(&square[..samples(1.0)], pitch(&square)) * std::f64::consts::FRAC_PI_4;
    let low = (1.0 - (mean(&narrow) - mean(&square)) / unit) / 2.0;
    let microseconds = low * 1.0e6 / 261.626;
    println!("low for {:.3}% of the cycle: {microseconds:.1} µs", low * 100.0);
    assert!((microseconds - 30.0).abs() < 4.0);
}

#[test]
fn the_sync_gives_vco_2_the_period_of_vco_1() {
    // VCO-2 a fourth and an octave above, heard alone: free, it sounds
    // its own pitch; synchronised, VCO-1's.
    let patch = second(1).with(Param::Vco2Range, 29);
    let free = pitch(&tone(&patch, A2 - 12));
    assert!(cents(220.0 * (17.0f64 / 12.0).exp2(), free).abs() < 1.0);
    // Synchronised, its partials are the multiples of VCO-1's own pitch,
    // tolerance and all, and of nothing else.
    let first = pitch(&tone(&Patch::DEFAULT, A2 - 12));
    let spectrum = spectrum(&tone(&patch.with(Param::Sync, 1), A2 - 12), LENGTH);
    let clear = CLEAR_BINS * bin_hertz(&spectrum);
    let (sound, rest) = sound_and_the_rest(&spectrum, clear, AUDIBLE, |hertz| off_the_grid(hertz, first) <= clear);
    let fundamental = spectrum[(first / bin_hertz(&spectrum)).round() as usize];
    println!(
        "free {free:.2} Hz; synchronised to {first:.2} Hz: its fundamental {:.1} dB, what is off its multiples {:.1} dB",
        decibels(fundamental, sound),
        decibels(rest, sound)
    );
    assert!(decibels(fundamental, sound) > -20.0 && decibels(rest, sound) < -75.0);
}

#[test]
fn the_cross_modulation_at_five_moves_vco_1_three_octaves() {
    // Service notes, adjustment 10: with CROSS MOD at 5 and VCO-2 a
    // square wave in its low range, VCO-1 at 16' sounds as it did at 2'.
    // The low range at 16' is 1 Hz here: each half of the square lasts
    // half a second.
    let patch = Patch::DEFAULT
        .with(Param::Vco1Range, 0)
        .with(Param::CrossMod, 128)
        .with(Param::Vco2Wave, 2)
        .with(Param::LowFrequency, 1)
        .with(Param::Vco2Range, 0);
    let signal = play(&patch, C0 + 9, 2.5, 0.0);
    let highest = signal[samples(0.3)..].chunks(samples(0.1)).map(pitch).fold(0.0f64, f64::max);
    // The key A0 at 16' is 55 Hz, and at 2' 440 Hz.
    // The slider's 128 of 255 is a little above its 5.
    let error = cents(55.0 * (6.0f64 * 128.0 / 255.0).exp2(), highest);
    println!("the upper half: {highest:.2} Hz, {error:.1} cents from three octaves");
    assert!(error.abs() < 10.0);
}

/// What the cross modulation's aliasing is held below, in decibels, with
/// the slider at 5 or less and keys to the A of 880 Hz, and at any
/// setting: the worst measured when this was written (-70.5 dB and
/// -30.1 dB), and 3 dB.
const MODERATE_ALIASING: f64 = -67.5;
/// See [`MODERATE_ALIASING`].
const DEEPEST_ALIASING: f64 = -27.0;

/// How near a line of the sound a bin must be to count as it: six bins,
/// beyond the window's main lobe.
const CLEAR_BINS: f64 = 6.0;

/// What of VCO-1's sound under cross modulation with the sync on, below
/// 20 kHz, belongs to no partial of it, in decibels against the strongest
/// partial.
///
/// With the sync every cycle of VCO-1 is the same, so the partials are
/// the multiples of one frequency: the strongest line over the whole
/// number that puts the most of the power on multiples, beyond what
/// multiples so close would catch of any sound.
fn aliasing(key: u8, depth: u8, wave: u8) -> f64 {
    let patch = Patch::DEFAULT
        .with(Param::CrossMod, depth)
        .with(Param::Vco2Wave, wave)
        .with(Param::Sync, 1)
        .with(Param::Vco2Range, 24);
    let spectrum = spectrum(&tone(&patch, key), LENGTH);
    let clear = CLEAR_BINS * bin_hertz(&spectrum);
    let (line, _) = strongest(&spectrum, clear, AUDIBLE);
    let caught = |spacing: f64| {
        let (mut on, mut all) = (0.0f64, 0.0f64);
        for (bin, level) in spectrum.iter().enumerate() {
            let hertz = bin as f64 * bin_hertz(&spectrum);
            if hertz > clear && hertz < AUDIBLE {
                all += level * level;
                if off_the_grid(hertz, spacing) <= clear {
                    on += level * level;
                }
            }
        }
        // Less the share of the band that the multiples cover.
        on / all - 2.0 * clear / spacing
    };
    let spacing = (1..=64)
        .map(|number| line / f64::from(number))
        .filter(|&spacing| spacing >= 20.0)
        .max_by(|&a, &b| caught(a).total_cmp(&caught(b)))
        .unwrap_or(line);
    let (sound, rest) = sound_and_the_rest(&spectrum, clear, AUDIBLE, |hertz| off_the_grid(hertz, spacing) <= clear);
    // A sound 80 dB below an oscillator is none.
    if sound < 1.0e-5 { f64::NAN } else { decibels(rest, sound) }
}

#[test]
fn what_the_cross_modulation_aliases_is_measured() {
    // With the sync on, where the sound has one period and what belongs
    // to it can be told from what does not. Without it the partials are
    // sums of multiples of two unrelated frequencies and lie everywhere:
    // nothing is measured there. The slider at 1, 2.5, 5 and 10; where
    // VCO-1 is driven out of hearing altogether there is nothing to
    // measure against.
    println!("what is no partial of the sound, below 20 kHz, against the strongest partial, dB:");
    println!("key  depth    sine  sawtooth   pulse");
    let (mut moderate, mut deepest) = (f64::MIN, f64::MIN);
    // An unoptimised build measures the two keys that hold the worst
    // figures, a release build all five.
    let keys: &[u8] = if cfg!(debug_assertions) { &[A2, A2 + 12] } else { &[A2 - 24, A2 - 12, A2, A2 + 12, A2 + 24] };
    for &key in keys {
        for depth in [26u8, 64, 128, 255] {
            let found = [0, 1, 2].map(|wave| aliasing(key, depth, wave));
            println!("{key:3}  {depth:5}  {:6.1}  {:6.1}  {:6.1}", found[0], found[1], found[2]);
            let worst = found.iter().copied().filter(|level| level.is_finite()).fold(f64::MIN, f64::max);
            if depth <= 128 && key <= A2 + 12 {
                moderate = moderate.max(worst);
            }
            deepest = deepest.max(worst);
        }
    }
    println!("worst with the slider at 5 or less, to the A of 880 Hz: {moderate:.1} dB; worst of all: {deepest:.1} dB");
    assert!(moderate < MODERATE_ALIASING && deepest < DEEPEST_ALIASING);
}
