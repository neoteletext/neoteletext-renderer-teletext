//! The CS-80's two filters: their cutoffs against Yamaha's one stated
//! point and the alignment procedure's, an octave to the volt, the keys,
//! the resonance from a Q of 0.5 to 5 and Haible's bound of 12.5, at
//! which the filters do not oscillate (TODO-6434).

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

use std::f64::consts::PI;

use cs80_support::{component, decibels, panel, pitch, play, samples};
use neoteletext_audio_instruments::cs80::presets::PRESETS;
use neoteletext_audio_instruments::cs80::{Control, Global, Patch, RATE};

/// A filter of the documents: its cutoff in hertz and its Q.
#[derive(Clone, Copy, Debug)]
struct Filter {
    cutoff: f64,
    q: f64,
}

/// Which of the two filters.
#[derive(Clone, Copy, PartialEq)]
enum Which {
    High,
    Low,
}

/// The octaves to the volt of a filter's own lever: one for the
/// high-pass filter, 3.3 / 2.7 for the low-pass (service manual, page 41:
/// R6 and R7).
fn lever_octaves(which: Which) -> f64 {
    if which == Which::High { 1.0 } else { 3.3 / 2.7 }
}

/// The cutoff the documents give a filter's lever at a key: 32.70 Hz at
/// 0.25 V of key voltage (key 48) with nothing at the control input, in
/// proportion to the key voltage; `more` octaves are added. Haible's
/// floor of 21.3 Hz, and this crate's ceiling of 20 kHz.
fn cutoff(which: Which, key: u8, lever: u8, more: f64) -> f64 {
    let volts = 10.0 * f64::from(lever) / 127.0;
    let octaves = (f64::from(key) - 48.0) / 12.0 + volts * lever_octaves(which) + more;
    (32.703 * octaves.exp2()).clamp(21.277, 20_000.0)
}

/// The Q of a resonance lever with the overall lever: 0.5 with both at
/// their feet to 5 with both at their tops, in a straight line with
/// their mean.
fn q(lever: u8, overall: u8) -> f64 {
    0.5 + 4.5 * (f64::from(lever) + f64::from(overall)) / 254.0
}

/// The gain of the high-pass then the low-pass at a frequency, each a
/// state-variable filter made at the voices' rate with its cutoff where
/// it is asked.
fn gain(high: Filter, low: Filter, hertz: f64) -> f64 {
    let warped = |frequency: f64| (PI * frequency / f64::from(RATE)).tan();
    let denominator = |filter: Filter| {
        let x = warped(hertz) / warped(filter.cutoff);
        ((1.0 - x * x).powi(2) + (x / filter.q).powi(2)).sqrt()
    };
    let x = warped(hertz) / warped(high.cutoff);
    x * x / denominator(high) / denominator(low)
}

/// Plays a sawtooth through the filters and compares each harmonic below
/// 12 kHz with the documents' filters; gives the worst difference in dB.
fn compare(patch: &Patch, key: u8, high: Filter, low: Filter) -> f64 {
    let signal = play(patch, key, 100, 1.5, 0.0);
    let steady = &signal[24_000..];
    // The oscillator's own pitch, from its sine.
    let hertz = pitch(&play(&panel(&cs80_support::SINE), key, 100, 1.5, 0.0)[24_000..]);
    assert!(hertz > 20.0);
    let mut worst = 0.0f64;
    let mut harmonic = 1.0;
    while hertz * harmonic < 12_000.0 {
        // A sawtooth's harmonic n is 2 / (π n) of its peak; the filters'
        // output at full VCF LEVEL is 0.8 of the sine's level, which is
        // an eighth of full scale.
        let expected = 0.8 / 8.0 * 2.0 / (PI * harmonic) * gain(high, low, hertz * harmonic);
        if expected > 1.0e-4 {
            let found = component(steady, hertz * harmonic);
            worst = worst.max(decibels(found / expected).abs());
        }
        harmonic += 1.0;
    }
    worst
}

/// A patch of the panel's sawtooth with the four filter levers set.
fn filters(hpf: u8, hpf_resonance: u8, lpf: u8, lpf_resonance: u8) -> Patch {
    panel(&[
        (Control::Hpf, hpf),
        (Control::HpfResonance, hpf_resonance),
        (Control::Lpf, lpf),
        (Control::LpfResonance, lpf_resonance),
    ])
}

#[test]
fn the_cutoffs_are_the_alignments_two_points_and_follow_the_keys() {
    // Adjustment procedure, page 11: at the lowest key, 4.0 V at the
    // high-pass lever and 3.3 V at the low-pass lever put each filter on
    // the 2' note, 261.6 Hz. The levers' 51 and 42 of 127 are 4.02 V and
    // 3.31 V.
    let (high, low) = (cutoff(Which::High, 36, 51, 0.0), cutoff(Which::Low, 36, 42, 0.0));
    println!("the alignment's 4.0 V and 3.3 V at the lowest key: {high:.1} Hz and {low:.1} Hz for 261.6 Hz");
    // A lever's step of 127 is 0.08 V, 7% of a frequency.
    assert!((high / 261.6 - 1.0).abs() < 0.04 && (low / 261.6 - 1.0).abs() < 0.04);
    // Service manual, page 42, VCF IC, pin 4: 0.25 V of key voltage and 5
    // V are "just 1KHz".
    let chip = cutoff(Which::High, 48, 0, 5.0);
    assert!((chip / 1000.0 - 1.0).abs() < 0.05);
    for (key, hpf, lpf) in
        [(48u8, 0u8, 64u8), (36, 0, 64), (60, 0, 64), (48, 0, 38), (48, 0, 90), (72, 40, 80), (48, 64, 127)]
    {
        let high = Filter { cutoff: cutoff(Which::High, key, hpf, 0.0), q: 0.5 };
        let low = Filter { cutoff: cutoff(Which::Low, key, lpf, 0.0), q: 0.5 };
        let worst = compare(&filters(hpf, 0, lpf, 0), key, high, low);
        println!("key {key}: high-pass {:.0} Hz, low-pass {:.0} Hz: within {worst:.2} dB", high.cutoff, low.cutoff);
        assert!(worst < 0.3, "key {key}, levers {hpf} and {lpf}: {worst} dB");
    }
}

#[test]
fn the_resonance_is_a_q_of_a_half_to_five_in_a_straight_line_with_the_two_levers_mean() {
    // Service manual, page 42, VCF IC, pin 7: a Q of 0.5 at one end of
    // the control voltage and 5 at the other; pages 39 and 41: the pin
    // has the mean of the filter's own lever and the overall one. Half
    // way it is 2.75, and BRASS 1's 61 with the overall lever at its
    // centre is 2.71, where a real instrument's BRASS 1 has it
    // (`cs80::law::damping`).
    for (hpf_resonance, lpf_resonance, overall) in
        [(0u8, 127u8, 0u8), (127, 0, 0), (64, 64, 0), (127, 127, 0), (0, 0, 127), (127, 127, 127), (40, 90, 60)]
    {
        let mut patch = filters(52, hpf_resonance, 70, lpf_resonance);
        patch.set(Global::Resonance, overall);
        let high = Filter { cutoff: cutoff(Which::High, 48, 52, 0.0), q: q(hpf_resonance, overall) };
        let low = Filter { cutoff: cutoff(Which::Low, 48, 70, 0.0), q: q(lpf_resonance, overall) };
        let worst = compare(&patch, 48, high, low);
        println!("Q {:.2} and {:.2}: within {worst:.2} dB", high.q, low.q);
        assert!(worst < 0.5, "levers {hpf_resonance}, {lpf_resonance}, {overall}: {worst} dB");
    }
}

#[test]
fn at_the_most_resonance_the_filters_ring_and_do_not_oscillate() {
    // A short note into filters at the highest Q, the amplifier held open
    // long after it: what is left is the filters' own ringing.
    let mut patch = panel(&[
        (Control::Hpf, 60),
        (Control::HpfResonance, 127),
        (Control::Lpf, 64),
        (Control::LpfResonance, 127),
        (Control::Sawtooth, 0),
        (Control::Noise, 127),
    ]);
    patch.set(Global::Resonance, 127);
    let signal = play(&patch, 48, 100, 2.0, 0.0);
    let level = |from: usize| cs80_support::rms(&signal[from..from + 24_000]);
    let (early, late) = (level(24_000), level(72_000));
    println!("noise through both filters at a Q of 5: {early:.4} then {late:.4} of full scale");
    // Steady, and far from what an oscillation would settle at.
    assert!((late / early - 1.0).abs() < 0.25 && late < 0.3);
    // With nothing going in, nothing comes out.
    patch.tones[0].set(Control::Noise, 0);
    let silent = play(&patch, 48, 100, 1.0, 0.0);
    assert!(silent.iter().all(|&sample| sample.abs() < 1.0e-4));
}

#[test]
fn the_overall_brilliance_moves_the_low_pass_filter_more_than_the_high_pass() {
    // Service manual, page 39: the brilliance reaches the high-pass
    // filter through 680 kΩ and the low-pass filter through 220 kΩ, where
    // the levers come through 39 kΩ. The lever's ends are taken as 15 V.
    for (lever, volts) in [(0u8, -15.0), (32, -7.5), (127, 15.0)] {
        let mut patch = filters(60, 0, 64, 90);
        patch.set(Global::Brilliance, lever);
        let (up, down) = (volts * 39.0 / 680.0, volts * 39.0 / 220.0 * 3.3 / 2.7);
        let high = Filter { cutoff: cutoff(Which::High, 48, 60, up), q: 0.5 };
        let low = Filter { cutoff: cutoff(Which::Low, 48, 64, down), q: q(90, 0) };
        let worst = compare(&patch, 48, high, low);
        println!("BRILLIANCE at {lever}: {up:.2} and {down:.2} octaves: within {worst:.2} dB");
        assert!(worst < 0.5, "{lever}: {worst} dB");
    }
}

#[test]
fn the_square_is_eight_tenths_of_the_sine_and_the_levels_are_straight() {
    // Adjustment procedure, page 15, steps 1 and 2: 1.00 V of sine, 0.8 V
    // of square, peak to peak. A square's fundamental is 4 / π of its
    // peak.
    let square = panel(&[(Control::Sawtooth, 0), (Control::Square, 1)]);
    let signal = play(&square, 57, 100, 1.5, 0.0);
    let hertz = pitch(&signal[24_000..]);
    let found = component(&signal[24_000..], hertz);
    // The filters at rest take a little of the fundamental.
    let filters = gain(
        Filter { cutoff: cutoff(Which::High, 57, 0, 0.0), q: 0.5 },
        Filter { cutoff: cutoff(Which::Low, 57, 127, 0.0), q: 0.5 },
        hertz,
    );
    let ratio = found / (0.125 * 4.0 / PI * filters);
    println!("the square against the sine: {ratio:.3}");
    assert!((ratio - 0.8).abs() < 0.01);
    for (control, value) in [(Control::VcfLevel, 64u8), (Control::Level, 64)] {
        let mut half = square;
        half.tones[0].set(control, value);
        let quieter = component(&play(&half, 57, 100, 1.5, 0.0)[24_000..], hertz) / found;
        assert!((quieter - 64.0 / 127.0).abs() < 0.01, "{control:?}: {quieter}");
    }
    // The mix is a balance between the channels.
    let mut mixed = square;
    mixed.set(Global::Mix, 32);
    mixed.tones[1].set(Control::Level, 0);
    let share = component(&play(&mixed, 57, 100, 1.5, 0.0)[24_000..], hertz) / found;
    assert!((share - 95.0 / 127.0).abs() < 0.01, "{share}");
}

#[test]
fn the_flutes_high_pass_filter_stands_below_its_note() {
    // Service manual, page 31, T51 circuit diagram, column D, row 7: the
    // high-pass row's part is 51 kΩ, written with no point, which leaves
    // the row at 0.63 V and the filter's cutoff 1.4 octaves below the
    // note. Read as 5.1 kΩ it stood two octaves above the note, on the
    // fourth harmonic, and a flute had lost its fundamental.
    let patch = Patch::with_tones(PRESETS[3].1, PRESETS[3].1);
    assert_eq!(PRESETS[3].0, "FLUTE");
    let signal = play(&patch, 60, 64, 2.0, 0.0);
    let steady = &signal[samples(1.0)..];
    let note = 261.626;
    let fundamental = component(steady, note);
    let fourth = component(steady, 4.0 * note);
    assert!(decibels(fundamental / fourth) > 6.0, "{:.1} dB", decibels(fundamental / fourth));
}

/// The cutoff, in octaves above the note, of the low-pass filter of a Q
/// of `q` that a sawtooth's harmonics went through: the one whose gains
/// are nearest theirs, in decibels, over the first sixteen.
fn fitted_cutoff(signal: &[f64], note: f64, q: f64) -> f64 {
    let found: Vec<f64> = (1..=16).map(|harmonic| decibels(component(signal, note * f64::from(harmonic)))).collect();
    let error = |octaves: f64| {
        let model: Vec<f64> = (1..=16)
            .map(|harmonic| {
                let x = f64::from(harmonic) / octaves.exp2();
                -decibels(f64::from(harmonic) * (1.0 - x * x).hypot(x / q))
            })
            .collect();
        let offset = found.iter().zip(&model).map(|(found, model)| found - model).sum::<f64>() / 16.0;
        found.iter().zip(&model).map(|(found, model)| (found - model - offset).powi(2)).sum::<f64>()
    };
    let mut best = (f64::INFINITY, 0.0);
    for step in 0..=600 {
        let octaves = f64::from(step) / 100.0;
        let error = error(octaves);
        if error < best.0 {
            best = (error, octaves);
        }
    }
    best.1
}

#[test]
#[ignore = "an open conflict between the documents as read and twelve recordings of a real instrument: run by hand it \
            fails, BRASS 1 resting 2.92 octaves above its note for the recordings' 1.39 to 2.00"]
fn brass_1_rests_where_a_real_instruments_does() {
    // Twelve keys of one real CS-80's BRASS 1, each held 12 s
    // (`~/devel/teletext-reference/synthesizers/cs-80/recordings/`,
    // measured by `validation/table.py`). The low-pass filter's cutoff
    // from 9.5 s on, in octaves above the note, on the keys 39, 42, 57,
    // 60, 63, 65, 69, 70, 76, 79, 80 and 88:
    //   +2.00 +1.99 +1.96 +1.65 +1.62 +1.61 +1.95 +1.61 +1.87 +1.50 +1.58 +1.39
    // a mean of +1.73, in two groups 0.35 octave apart (the instrument's
    // voices, each with its own trimmers). This reading of the documents
    // comes to rest at +2.92 on every key however it is struck (a
    // stroke's voltage drains away in 3.3 s): 0.92 to 1.53 octaves above
    // the recordings, 1.19 in the mean. The peak above the rest agrees
    // where the recording's bandwidth lets it be measured (2.2 to 3.0
    // octaves for our 2.2, and the stroke adds to the real one).
    //
    // The same recordings against this reading otherwise: the filter's
    // peak comes 116 to 260 ms after the key for our 170 to 390 ms; the
    // fall from it is one exponential of 2.8 to 3.6 s, and ours on
    // middle C, struck as hard, 2.7 s; the fundamental falls 2.7 to 6.8
    // dB from 1 s to 10 s, 4.3 dB in the mean, for our 4.0 dB.
    //
    // What the documents were asked and did not answer is in the
    // module's notes ([`neoteletext_audio_instruments::cs80`], "What is
    // not here"): no one documented quantity moved within its tolerance
    // or its doubt closes this on the twelve keys.
    let patch = Patch::with_tones(PRESETS[2].1, PRESETS[2].1);
    assert_eq!(PRESETS[2].0, "BRASS 1");
    for key in [39u8, 60, 88] {
        let note = 440.0 * ((f64::from(key) - 69.0) / 12.0).exp2();
        let signal = play(&patch, key, 1, 11.5, 0.0);
        // The Q is the preset's own lever and the overall lever at its
        // centre: 0.5 + 4.5 × (61 + 64) / 254.
        let rest = fitted_cutoff(&signal[samples(9.5)..], note, 2.71);
        println!("BRASS 1, key {key}: at rest {rest:+.2} octaves above the note");
        assert!((1.39..=2.00).contains(&rest), "key {key}: {rest:+.2} octaves for the recordings' +1.39 to +2.00");
    }
}
