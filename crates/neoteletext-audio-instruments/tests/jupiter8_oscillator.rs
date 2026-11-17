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
fn the_pulse_narrows_to_thirty_microseconds_on_the_key_c2_at_two_feet() {
    // Service notes, adjustment 11: "Press C2 key ... for 30µs space
    // width", with VCO-1's range knob drawn at 2', where that key is two
    // octaves above middle C. Measured as the pulse's mean: a pulse low
    // for a share s of its cycle between -1 and 1 has a mean of 1 - 2s.
    // The amplifier passes the mean, the high-pass filter being off.
    /// The key C2 at 2', in hertz.
    const HERTZ: f64 = 4.0 * 261.626;
    let pulse =
        |width: u8| Patch::DEFAULT.with(Param::Vco1Wave, 2).with(Param::Vco1Range, 3).with(Param::PwmDepth, width);
    let mean = |signal: &[f64]| signal.iter().sum::<f64>() / signal.len() as f64;
    let square = tone(&pulse(0), C2);
    let narrow = tone(&pulse(255), C2);
    assert!((pitch(&square) / HERTZ - 1.0).abs() < 0.002, "{} Hz", pitch(&square));
    // The pulse's 1 is the square's, whose fundamental is 4 / π of it.
    let unit = component(&square[..samples(1.0)], pitch(&square)) * std::f64::consts::FRAC_PI_4;
    let low = (1.0 - (mean(&narrow) - mean(&square)) / unit) / 2.0;
    let microseconds = low * 1.0e6 / HERTZ;
    println!("low for {:.3}% of the cycle: {microseconds:.1} µs", low * 100.0);
    assert!((microseconds - 30.0).abs() < 3.0);
}

#[test]
fn the_envelope_and_the_oscillator_move_the_pulses_width() {
    // Owner's manual, printed page 17: with ENV-1 the width follows the
    // envelope from the square; with LFO it goes "to the extreme limit
    // set by the slider and back again". The width is read from the
    // pulse's mean, a square's being none.
    let mean = |signal: &[f64]| signal.iter().sum::<f64>() / signal.len() as f64;
    let pulse = Patch::DEFAULT.with(Param::Vco1Wave, 2).with(Param::PwmDepth, 128);
    let manual = mean(&tone(&pulse, C2));
    // The envelope at full sustain holds the width the slider has by
    // hand; at none it leaves the square.
    let held = mean(&tone(&pulse.with(Param::PwmSource, 2), C2));
    let unmoved = mean(&tone(&pulse.with(Param::PwmSource, 2).with(Param::Env1Sustain, 0), C2));
    println!("the pulse's mean: by hand {manual:.4}, by the envelope at its peak {held:.4}, at none {unmoved:.4}");
    assert!((held / manual - 1.0).abs() < 0.02 && unmoved.abs() < 0.02 * manual.abs());
    // The oscillator, a sine at 3.6 Hz, swings between the square and
    // the slider's width. Each mean is over 20 ms, which is not a whole
    // number of the pulse's cycles: a tenth of the swing either way.
    let swept = pulse.with(Param::PwmSource, 0).with(Param::LfoRate, 150);
    let means: Vec<f64> = tone(&swept, C2).chunks(samples(0.02)).map(mean).collect();
    let (most, least) =
        (means.iter().copied().fold(f64::MIN, f64::max), means.iter().copied().fold(f64::MAX, f64::min));
    println!("swept by the oscillator: from {least:.4} to {most:.4}");
    assert!((most / manual - 1.0).abs() < 0.12 && least.abs() < 0.12 * manual.abs());
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

#[test]
fn each_waveform_of_vco_2_moves_vco_1_by_its_own_voltage() {
    // Service notes, module board schematic: VCO-2's waveform reaches
    // the cross modulation's amplifier with no capacitor between, the
    // sine from -5 V to +5 V and the sawtooth and the pulse from 0 V to
    // 10 V; adjustment 10 makes ten volts three octaves at 5. So at 5
    // the pulse and the sawtooth take VCO-1 from its own pitch to three
    // octaves above and nowhere below, and the sine an octave and a
    // half either way. VCO-2 in its low range at 16', 1 Hz; VCO-1 at
    // 16' on the key A0, 55 Hz.
    let moved = |wave: u8| {
        let patch = Patch::DEFAULT
            .with(Param::Vco1Range, 0)
            .with(Param::CrossMod, 128)
            .with(Param::Vco2Wave, wave)
            .with(Param::LowFrequency, 1)
            .with(Param::Vco2Range, 0);
        let signal = play(&patch, C0 + 9, 3.3, 0.0);
        let pitches: Vec<f64> = signal[samples(0.3)..].chunks_exact(samples(0.1)).map(pitch).collect();
        (pitches.iter().copied().fold(f64::MAX, f64::min), pitches.iter().copied().fold(0.0f64, f64::max))
    };
    // The slider's 128 of 255 is a little above its 5.
    let depth = 6.0f64 * 128.0 / 255.0;
    let (lowest, highest) = moved(2);
    println!("the pulse: from {lowest:.2} Hz to {highest:.2} Hz");
    assert!(cents(55.0, lowest).abs() < 10.0 && cents(55.0 * depth.exp2(), highest).abs() < 10.0);
    // A tenth of a second holds the sawtooth's first and last tenth and
    // the sine's top and bottom only in the mean.
    let (lowest, highest) = moved(1);
    println!("the sawtooth: from {lowest:.2} Hz to {highest:.2} Hz");
    assert!(lowest > 55.0 && lowest < 55.0 * (0.15 * depth).exp2(), "{lowest}");
    assert!(highest < 55.0 * depth.exp2() && highest > 55.0 * (0.85 * depth).exp2(), "{highest}");
    let (lowest, highest) = moved(0);
    println!("the sine: from {lowest:.2} Hz to {highest:.2} Hz");
    assert!(highest < 55.0 * (depth / 2.0).exp2() && highest > 55.0 * (0.9 * depth / 2.0).exp2(), "{highest}");
    assert!(lowest > 55.0 * (-depth / 2.0).exp2() && lowest < 55.0 * (-0.9 * depth / 2.0).exp2(), "{lowest}");
}

#[test]
fn switching_the_cross_modulation_during_a_note_does_not_click() {
    // With cross modulation the oscillators run at twice the rate they
    // run at without, through a filter of their own. Moving the slider
    // from 0 to its first step under a held note, and back, changes the
    // sound by next to nothing: the largest step from one sample to the
    // next is what the steady note has.
    let plain = Patch::DEFAULT.with(Param::Vco1Wave, 0);
    let crossed = plain.with(Param::CrossMod, 1);
    let largest = |signal: &[f64]| signal.windows(2).map(|pair| (pair[1] - pair[0]).abs()).fold(0.0f64, f64::max);
    for (name, from, to) in [("on", &plain, &crossed), ("off", &crossed, &plain)] {
        let mut jupiter = support::instrument(from);
        jupiter.event(support::NOTE_ON, A2 + 12, 100);
        let steady = largest(&support::render(&mut jupiter, 0.3)[samples(0.2)..]);
        // The sample before the change, and three milliseconds after.
        let last = support::render(&mut jupiter, 1.0 / support::RATE);
        jupiter.set_patch(to);
        let mut after = last;
        after.extend(support::render(&mut jupiter, 0.003));
        let found = largest(&after);
        println!("cross modulation switched {name}: the largest step {found:.4}, {steady:.4} in the steady note");
        assert!(found < 1.25 * steady, "switched {name}: {found} against {steady}");
    }
}

/// What the cross modulation's aliasing is held below, in decibels, with
/// the slider at 5 or less and keys to the A of 880 Hz, and at any
/// setting: the worst measured (-77.2 dB and -38.8 dB), and 3 dB. Both
/// are where VCO-2's pulse holds VCO-1 a steady three and six octaves
/// up, and are what the filter after the oscillators, its cutoff at
/// 20 kHz, folds back of a plain sawtooth of 7 kHz and of 28 kHz
/// (`tests/analog_filter_aliasing.rs`).
const MODERATE_ALIASING: f64 = -74.0;
/// See [`MODERATE_ALIASING`].
const DEEPEST_ALIASING: f64 = -35.0;
/// The same with VCO-2's sine or sawtooth moving VCO-1, the slider at 5
/// or less and keys to the A of 880 Hz, for VCO-1's sawtooth, triangle
/// and square: the worst measured (-87.9 dB, -92.0 dB and -86.6 dB), and
/// 3 dB or more. Before the edges of VCO-1 were placed by its moving
/// frequency and its corners rounded, these were -70.5 dB, -64.5 dB and
/// -69.7 dB.
const SMOOTH_ALIASING: [(u8, f64); 3] = [(1, -83.0), (0, -88.5), (3, -79.0)];

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
fn aliasing(key: u8, depth: u8, wave: u8, first: u8) -> f64 {
    let patch = Patch::DEFAULT
        .with(Param::Vco1Wave, first)
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
    // that is measured before the filter, against the same sound three
    // octaves lower, by the voice's own test. The slider at 1, 2.5, 5
    // and 10; where
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
            let found = [0, 1, 2].map(|wave| aliasing(key, depth, wave, 1));
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

#[test]
fn a_sine_or_a_sawtooth_moving_vco_1_leaves_its_edges_and_corners_clean() {
    // VCO-1's frequency moves through every sample, so its edges fall
    // off where one frequency for the sample would put them; and where
    // the sync cuts VCO-2's cycle short, VCO-1's frequency jumps and its
    // sawtooth and triangle turn a corner.
    let keys: &[u8] = if cfg!(debug_assertions) { &[A2 + 12] } else { &[A2 - 24, A2 - 12, A2, A2 + 12] };
    for (first, bound) in SMOOTH_ALIASING {
        let mut worst = f64::MIN;
        for &key in keys {
            for depth in [26u8, 64, 128] {
                for wave in [0, 1] {
                    let found = aliasing(key, depth, wave, first);
                    if found.is_finite() {
                        worst = worst.max(found);
                    }
                }
            }
        }
        println!("VCO-1's waveform {first}: {worst:.1} dB at worst");
        assert!(worst < bound, "VCO-1's waveform {first}: {worst} dB");
    }
}
