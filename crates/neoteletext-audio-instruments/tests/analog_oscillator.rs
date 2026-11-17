//! The oscillators of the analogue building blocks: their shapes and
//! levels, and what aliasing their band-limited edges leave (TODO-6430).

// Without the feature there is nothing here to test.
#![cfg(feature = "analog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod support;

use std::f64::consts::PI;

use neoteletext_audio_instruments::analog::oscillator::{Noise, Oscillator, Waves, waveform};
use neoteletext_audio_instruments::analog::{LEVEL, STEP_LIMIT, frequency};
use support::{decibels, decimate, float, on_and_off_the_grid, spectrum};

/// Output samples of a spectrum: bins are 48,000 / 65,536 Hz apart.
const POINTS: usize = 1 << 16;
/// Output samples left out before a spectrum is taken.
const SETTLE: usize = 256;
/// The bin of 20 kHz.
const TOP: usize = 20_000 * POINTS / 48_000;

/// The step of a tone on bin `bin` of the output's spectrum, for an
/// oscillator at `factor` times the output rate.
fn bin_step(bin: usize, factor: u32) -> u32 {
    ((bin as u64) << 32).div_ceil(POINTS as u64 * u64::from(factor)) as u32
}

/// Renders a waveform at `factor` times the output rate and brings it
/// down: `control` sets the oscillator up, and may change it before each
/// sample.
fn render(factor: u32, pick: fn(&Waves) -> i32, mut control: impl FnMut(&mut Oscillator, usize)) -> Vec<f64> {
    let mut oscillator = Oscillator::new();
    let count = (POINTS + SETTLE) * factor as usize;
    let signal: Vec<i32> = (0..count)
        .map(|index| {
            control(&mut oscillator, index);
            pick(&oscillator.tick(None))
        })
        .collect();
    float(&decimate(&signal, factor)[SETTLE..])
}

/// How far below the strongest harmonic the strongest tone that is no
/// harmonic lies, below 20 kHz, in decibels, for a wave whose harmonics
/// are `base` bins apart.
fn aliasing(wave: &[f64], base: usize) -> f64 {
    let (on, off) = on_and_off_the_grid(&spectrum(wave), base, TOP);
    decibels(off / on)
}

/// The fundamentals measured, as bins: 80 Hz to 4 kHz. Each is odd, so
/// that no tone folded once lands on a harmonic.
const FUNDAMENTALS: [usize; 6] = [109, 601, 1_365, 2_731, 4_097, 5_461];

type Pick = (&'static str, fn(&Waves) -> i32, u32);

/// Each waveform, with the pulse at a half and at a tenth.
const WAVES: [Pick; 5] = [
    ("sawtooth", |waves| waves.sawtooth, 1 << 31),
    ("square", |waves| waves.pulse, 1 << 31),
    ("pulse 10%", |waves| waves.pulse, 429_496_730),
    ("triangle", |waves| waves.triangle, 1 << 31),
    ("sub", |waves| waves.sub, 1 << 31),
];

/// The worst aliasing of every waveform at every fundamental, at a rate.
fn worst_aliasing(factor: u32, fundamentals: &[usize]) -> f64 {
    let mut worst = f64::NEG_INFINITY;
    for (name, pick, width) in WAVES {
        for &bin in fundamentals {
            let wave = render(factor, pick, |oscillator, index| {
                if index == 0 {
                    oscillator.set_step(bin_step(bin, factor));
                    oscillator.set_width(width);
                }
            });
            let found = if name == "sub" { sub_aliasing(&wave, bin) } else { aliasing(&wave, bin) };
            println!("{factor}x {name} at {:.0} Hz: {found:.1} dB", bin as f64 * 48_000.0 / POINTS as f64);
            worst = worst.max(found);
        }
    }
    worst
}

/// The sub-oscillator's aliasing: its harmonics lie on the odd multiples
/// of half the fundamental's bin, which is no whole bin; the grid is
/// taken at half bins by measuring against twice the spectrum's length.
fn sub_aliasing(wave: &[f64], bin: usize) -> f64 {
    let magnitudes = spectrum(wave);
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for (index, &magnitude) in magnitudes.iter().enumerate().take(TOP).skip(support::LOBE + 1) {
        // Twice the bin against the harmonics' spacing in half bins.
        let from_line = (2 * index % bin).min(bin - 2 * index % bin);
        if from_line <= 1 {
            on = on.max(magnitude);
        } else if from_line > 2 * support::LOBE {
            off = off.max(magnitude);
        }
    }
    decibels(off / on)
}

#[test]
fn aliasing_is_80_db_down_at_twice_the_output_rate() {
    let worst = worst_aliasing(2, &FUNDAMENTALS);
    println!("worst at twice the rate: {worst:.1} dB");
    assert!(worst < -80.0, "{worst}");
}

#[test]
fn aliasing_is_80_db_down_at_four_times_the_output_rate() {
    let worst = worst_aliasing(4, &FUNDAMENTALS);
    println!("worst at four times the rate: {worst:.1} dB");
    assert!(worst < -80.0, "{worst}");
}

#[test]
fn aliasing_at_the_output_rate_is_what_the_module_says() {
    // The table's low-pass falls between 0.3 and 0.7 of the sampling
    // rate: at the output rate the harmonics between 28 kHz and 33 kHz
    // come back between 15 kHz and 20 kHz only partly reduced.
    let low = worst_aliasing(1, &FUNDAMENTALS[..3]);
    let high = worst_aliasing(1, &FUNDAMENTALS[3..]);
    println!("worst at the output rate: {low:.1} dB to 1 kHz, {high:.1} dB to 4 kHz");
    assert!(low < -45.0, "{low}");
    assert!(high < -30.0, "{high}");
}

#[test]
fn a_pulse_whose_width_moves_at_audio_rate_has_no_clicks() {
    // The width swings between 10% and 90% at 7/20 of the pulse's
    // frequency: every tone of the result is a multiple of one twentieth
    // of the fundamental, here 37 bins, and whatever lies between those
    // lines is aliasing or a click.
    let base = 37;
    for factor in [2, 4] {
        let rate = f64::from(factor) * POINTS as f64;
        let wave = render(
            factor,
            |waves| waves.pulse,
            |oscillator, index| {
                if index == 0 {
                    oscillator.set_step(bin_step(20 * base, factor));
                }
                let swing = (2.0 * PI * 7.0 * base as f64 * index as f64 / rate).sin();
                oscillator.set_width(((0.5 + 0.4 * swing) * 4_294_967_296.0) as u32);
            },
        );
        let found = aliasing(&wave, base);
        println!("{factor}x pulse of moving width: {found:.1} dB");
        assert!(found < -80.0, "{found}");
    }
}

#[test]
fn a_synchronised_oscillator_is_band_limited_too() {
    // The follower's waveform repeats with the leader, so its tones are
    // the leader's harmonics, whatever its own frequency.
    let leader_bin = 601;
    for factor in [2, 4] {
        for (name, pick, width) in WAVES {
            if name == "sub" {
                continue;
            }
            let mut leader = Oscillator::new();
            let mut follower = Oscillator::new();
            leader.enable(0);
            leader.set_step(bin_step(leader_bin, factor));
            // The follower runs 2.83 times as fast as the leader.
            follower.set_step(bin_step(1_701, factor));
            follower.set_width(width);
            let count = (POINTS + SETTLE) * factor as usize;
            let signal: Vec<i32> = (0..count)
                .map(|_| {
                    let _ = leader.tick(None);
                    pick(&follower.tick(leader.started()))
                })
                .collect();
            let wave = float(&decimate(&signal, factor)[SETTLE..]);
            let found = aliasing(&wave, leader_bin);
            println!("{factor}x synchronised {name}: {found:.1} dB");
            assert!(found < -80.0, "{name} at {factor}x: {found}");
        }
    }
}

/// One cycle's worth of samples of each waveform at 100 Hz and the
/// output rate, after the oscillator's delay.
fn shapes() -> Vec<Waves> {
    let mut oscillator = Oscillator::new();
    oscillator.set_step(frequency(100_000, 48_000));
    oscillator.set_width(1 << 30);
    (0..480 * 3).map(|_| oscillator.tick(None)).collect()
}

#[test]
fn the_waveforms_have_their_shapes_and_levels() {
    let waves = shapes();
    let level = f64::from(LEVEL);
    // The first cycle after the delay: sample `DELAY + n` is the naive
    // wave at phase (n + 1) / 480, away from the edges.
    for (n, wave) in waves[Oscillator::DELAY..Oscillator::DELAY + 480].iter().enumerate() {
        let phase = (n + 1) as f64 / 480.0;
        let near_edge = |edge: f64| (phase - edge).abs() < 10.0 / 480.0 || (phase - edge).abs() > 470.0 / 480.0;
        if !near_edge(0.0) {
            assert!((f64::from(wave.sawtooth) / level - (2.0 * phase - 1.0)).abs() < 1e-3, "sawtooth at {phase}");
        }
        if !near_edge(0.0) && !near_edge(0.25) {
            let expected = if phase < 0.25 { 1.0 } else { -1.0 };
            assert!((f64::from(wave.pulse) / level - expected).abs() < 1e-3, "pulse at {phase}");
        }
        if !near_edge(0.0) && !near_edge(0.5) {
            let expected = if phase < 0.5 { 4.0 * phase - 1.0 } else { 3.0 - 4.0 * phase };
            assert!((f64::from(wave.triangle) / level - expected).abs() < 1e-3, "triangle at {phase}");
        }
    }
    // The sub-oscillator turns once a cycle: two cycles are one of its.
    let sub: Vec<i32> = waves[Oscillator::DELAY + 20..].iter().step_by(480).take(2).map(|wave| wave.sub).collect();
    assert!((f64::from(sub[0]) / level).abs() > 0.999 && sub[0].signum() == -sub[1].signum(), "{sub:?}");
    // No waveform overshoots by more than the low-pass's ripple, 9%.
    for wave in &waves {
        for value in [wave.sawtooth, wave.pulse, wave.triangle, wave.sub] {
            assert!(f64::from(value).abs() < 1.2 * level);
        }
    }
}

#[test]
fn the_harmonics_have_the_levels_of_the_ideal_waves_to_20_khz() {
    // At twice the rate: a sawtooth's harmonic n is 2 / (pi n) of its
    // peak, and the 16th of 1,100 Hz is at 17.6 kHz.
    let bin = 1_501;
    let wave = render(
        2,
        |waves| waves.sawtooth,
        |oscillator, index| {
            if index == 0 {
                oscillator.set_step(bin_step(bin, 2));
            }
        },
    );
    let magnitudes = spectrum(&wave);
    for harmonic in [1, 2, 8, 16, 18] {
        let expected = 2.0 * f64::from(LEVEL) / (PI * harmonic as f64);
        let found = decibels(magnitudes[bin * harmonic] / expected);
        assert!(found.abs() < 0.1, "harmonic {harmonic}: {found} dB");
    }
}

#[test]
fn a_disabled_waveform_is_silent_and_the_rest_do_not_change() {
    let mut all = Oscillator::new();
    let mut one = Oscillator::new();
    one.enable(waveform::SAWTOOTH);
    for oscillator in [&mut all, &mut one] {
        oscillator.set_step(frequency(440_000, 96_000));
    }
    for _ in 0..2_000 {
        let (all, one) = (all.tick(None), one.tick(None));
        assert_eq!(one, Waves { sawtooth: all.sawtooth, ..Waves::default() });
    }
}

#[test]
fn every_step_and_width_is_valid() {
    let mut random = support::Random(7);
    let mut oscillator = Oscillator::new();
    let mut leader = Oscillator::new();
    for round in 0..200_000 {
        if round % 16 == 0 {
            oscillator.set_step(random.word() >> (random.word() % 32));
            leader.set_step(random.word() >> (random.word() % 32));
        }
        oscillator.set_width(random.word() & (random.word() | random.word()));
        let _ = leader.tick(None);
        let waves = oscillator.tick(if round % 3 == 0 { leader.started() } else { None });
        for value in [waves.sawtooth, waves.pulse, waves.triangle, waves.sub] {
            // Edges closer than the residual's length add up, but stay
            // far inside the range.
            assert!(value.abs() < 8 * LEVEL, "{value}");
        }
    }
    assert!(oscillator.step() <= STEP_LIMIT);
    oscillator.set_step(u32::MAX);
    assert_eq!(oscillator.step(), STEP_LIMIT);
    oscillator.set_step(0);
    let phase = oscillator.phase();
    let _ = oscillator.tick(None);
    assert_eq!(oscillator.phase(), phase);
}

#[test]
fn noise_is_the_same_from_the_same_seed_and_white() {
    let mut first = Noise::new(1234);
    let mut second = Noise::new(1234);
    let mut other = Noise::new(1235);
    let samples: Vec<i32> = (0..1 << 16).map(|_| first.tick()).collect();
    assert!(samples.iter().all(|&sample| sample == second.tick()));
    assert!(samples.iter().any(|&sample| sample != other.tick()));
    // Zero is a seed like any other.
    assert_ne!(Noise::new(0).tick(), 0);

    // Uniform between the levels: a mean of nothing and a mean square of
    // a third; and nothing of one sample in the next.
    let level = f64::from(LEVEL);
    let count = samples.len() as f64;
    let mean = samples.iter().map(|&sample| f64::from(sample) / level).sum::<f64>() / count;
    let square = samples.iter().map(|&sample| (f64::from(sample) / level).powi(2)).sum::<f64>() / count;
    let lagged =
        samples.windows(2).map(|pair| f64::from(pair[0]) / level * f64::from(pair[1]) / level).sum::<f64>() / count;
    assert!(mean.abs() < 0.01 && (square - 1.0 / 3.0).abs() < 0.01 && lagged.abs() < 0.01, "{mean} {square} {lagged}");
    // Flat: each octave band from 750 Hz up holds its share of the power,
    // within 1 dB.
    let magnitudes = spectrum(&float(&samples));
    let power = |from: usize, to: usize| magnitudes[from..to].iter().map(|m| m * m).sum::<f64>() / (to - from) as f64;
    let reference = power(1 << 10, 1 << 15);
    for octave in 10..15 {
        let found = 10.0 * (power(1 << octave, 2 << octave) / reference).log10();
        assert!(found.abs() < 1.0, "octave from bin {}: {found} dB", 1 << octave);
    }
}

/// 2³², a cycle and a sample, as f64.
const WORD: f64 = 4_294_967_296.0;

#[test]
fn a_sample_made_in_stretches_of_one_frequency_is_the_tick() {
    // However a sample is cut into stretches, at one frequency and one
    // width it is the sample `tick` makes: the edges fall where they did,
    // to the 2⁻²⁸ of a sample a division is good for and the unit of
    // phase a stretch's length is rounded to. An entry of the step's
    // table is 2⁻¹⁵ of a step, 1,024 of a pulse's 2²⁵, and an edge so
    // little moved changes an entry by one, in each of the residuals
    // that overlap at the highest frequencies.
    let mut random = support::Random(11);
    let (mut ticked, mut stretched) = (Oscillator::new(), Oscillator::new());
    let (mut leader, mut worst) = (Oscillator::new(), 0u32);
    for round in 0..400_000 {
        if round % 1_000 == 0 {
            let step = random.word() >> (2 + random.word() % 10);
            let width = random.word();
            for oscillator in [&mut ticked, &mut stretched] {
                oscillator.set_step(step);
                oscillator.set_width(width);
                // The units of phase that the stretches' rounding has
                // moved the one from the other are set to nothing.
                oscillator.restart();
            }
            leader.set_step(random.word() >> (2 + random.word() % 12));
        }
        let _ = leader.tick(None);
        let sync = leader.started().filter(|_| round % 3_000 >= 1_500);
        let expected = ticked.tick(sync);
        let step = stretched.step();
        // Cut anywhere, and at the leader's start, where the cycle
        // starts again; but not where the width has just been set, which
        // it reaches at the end of a stretch as of a tick.
        for cut in [random.word(), random.word()] {
            if round % 1_000 != 0 && cut > sync.unwrap_or(0) && cut < stretched.now() {
                stretched.run(cut, step, step);
            }
        }
        if let Some(at) = sync {
            stretched.run(at, step, step);
            stretched.synchronise();
        }
        stretched.run(0, step, step);
        assert_eq!(stretched.started().is_some(), ticked.started().is_some(), "round {round}");
        let found = stretched.finish();
        for (found, expected) in [
            (found.sawtooth, expected.sawtooth),
            (found.pulse, expected.pulse),
            (found.triangle, expected.triangle),
            (found.sub, expected.sub),
        ] {
            worst = worst.max(found.abs_diff(expected));
        }
    }
    println!("a sample in stretches is within {worst} of the tick's, of a level of {LEVEL}");
    assert!(worst <= 4 * 1_024, "{worst}");
}

/// What moves the carrier's frequency in the test below: its value, from
/// -1 to 1, at a phase of its cycle, and the phases at which it jumps.
type Mover = (&'static str, fn(f64) -> f64, &'static [f64]);

/// A sine, which jumps nowhere; a sawtooth, which jumps where it starts;
/// and a pulse of 30%, which jumps twice.
const MOVERS: [Mover; 3] = [
    ("sine", |phase| (2.0 * PI * phase).sin(), &[]),
    ("sawtooth", |phase| 2.0 * phase - 1.0, &[1.0]),
    ("pulse", |phase| if phase < 0.3 { 1.0 } else { -1.0 }, &[0.3, 1.0]),
];

/// A carrier at `factor` times the output rate whose frequency another
/// waveform moves by `octaves` either way, exponentially, and which that
/// waveform's cycle starts again, so that everything it makes is a
/// harmonic of the mover's `bin`. With `stretches` each sample is made in
/// stretches between the mover's jumps, the frequency of each end of a
/// stretch given; without, as could be done before: one frequency for a
/// sample, the mover's at the sample's middle.
fn moved(factor: u32, mover: Mover, pick: fn(&Waves) -> i32, bin: usize, octaves: f64, stretches: bool) -> Vec<f64> {
    let (_, shape, jumps) = mover;
    // The carrier rests 2.83 times as fast as the mover.
    let rest = f64::from(bin_step(bin, factor)) * 2.83;
    let step = |phase: f64| (rest * (octaves * shape(phase)).exp2()) as u32;
    let advance = f64::from(bin_step(bin, factor)) / WORD;
    let mut carrier = Oscillator::new();
    carrier.set_width(1 << 30);
    let mut phase = 0.0f64;
    let count = (POINTS + SETTLE) * factor as usize;
    let signal: Vec<i32> = (0..count)
        .map(|_| {
            let end = phase + advance;
            // How long before the sample the mover's cycle started.
            let started = (end >= 1.0).then(|| (((end - 1.0) / advance) * WORD) as u32);
            if !stretches {
                let middle = phase + advance / 2.0;
                carrier.set_step(step(middle - middle.floor()));
                phase = end - end.floor();
                return pick(&carrier.tick(started));
            }
            let mut from = phase;
            for &jump in jumps.iter().filter(|&&jump| jump > phase && jump <= end) {
                // Up to the jump, from below; the cycle starts at one.
                let until = (((end - jump) / advance) * WORD) as u32;
                carrier.run(until, step(from), step(jump - 1.0e-12));
                from = if jump >= 1.0 { 0.0 } else { jump };
            }
            if started.is_some() {
                if jumps.is_empty() {
                    carrier.run(started.unwrap_or(0), step(from), step(0.0));
                    from = 0.0;
                }
                carrier.synchronise();
            }
            phase = end - end.floor();
            carrier.run(0, step(from), step(phase));
            pick(&carrier.finish())
        })
        .collect();
    float(&decimate(&signal, factor)[SETTLE..])
}

#[test]
fn a_frequency_that_moves_within_a_sample_leaves_the_edges_and_corners_clean() {
    // A frequency moved at audio rate through an exponential: with one
    // frequency for each sample every edge of the carrier is put where
    // that frequency would have it and not where the moving one does,
    // and where the mover jumps the carrier's sawtooth and triangle turn
    // a corner that nothing rounds. Half an octave either way, the mover
    // at 880 Hz.
    let bin = 1_201;
    let waves: [Pick; 3] = [WAVES[0], ("pulse 25%", |waves| waves.pulse, 0), WAVES[3]];
    for factor in [2, 4] {
        for mover in MOVERS {
            for (name, pick, _) in waves {
                let before = aliasing(&moved(factor, mover, pick, bin, 0.5, false), bin);
                let found = aliasing(&moved(factor, mover, pick, bin, 0.5, true), bin);
                println!(
                    "{factor}x {name} moved by a {}: {found:.1} dB ({before:.1} dB at one frequency a sample)",
                    mover.0
                );
                assert!(found < -80.0, "{name} moved by a {} at {factor}x: {found}", mover.0);
            }
        }
    }
}

#[test]
fn a_width_that_jumps_within_a_sample_turns_the_pulse_where_it_jumps() {
    // A sawtooth moves the pulse's width from 10% to 90% and starts the
    // pulse's cycle with its own, 880 Hz: where the sawtooth starts
    // again the width falls by most of a cycle at once, somewhere within
    // a sample. Given one width for each sample, the pulse turns where a
    // width sliding through that sample would have turned it.
    let bin = 1_201;
    let width = |phase: f64| ((0.1 + 0.8 * phase) * WORD) as u32;
    for factor in [2, 4] {
        let advance = f64::from(bin_step(bin, factor)) / WORD;
        let step = (f64::from(bin_step(bin, factor)) * 2.83) as u32;
        let count = (POINTS + SETTLE) * factor as usize;
        let [before, found] = [false, true].map(|stretches| {
            let mut carrier = Oscillator::new();
            carrier.set_step(step);
            let mut phase = 0.0f64;
            let signal: Vec<i32> = (0..count)
                .map(|_| {
                    let end = phase + advance;
                    let started = (end >= 1.0).then(|| (((end - 1.0) / advance) * WORD) as u32);
                    phase = end - end.floor();
                    if !stretches {
                        carrier.set_width(width(phase));
                        return carrier.tick(started).pulse;
                    }
                    if let Some(at) = started {
                        carrier.set_width(width(1.0));
                        carrier.run(at, step, step);
                        carrier.synchronise();
                        carrier.jump_width(width(0.0));
                    }
                    carrier.set_width(width(phase));
                    carrier.run(0, step, step);
                    carrier.finish().pulse
                })
                .collect();
            aliasing(&float(&decimate(&signal, factor)[SETTLE..]), bin)
        });
        println!("{factor}x pulse whose width a sawtooth moves: {found:.1} dB ({before:.1} dB at one width a sample)");
        assert!(found < -80.0, "{factor}x: {found}");
    }
}

#[test]
fn every_stretch_is_valid() {
    // Any steps, any widths, any cuts, in any order: nothing leaves its
    // range, and a sample never runs backwards.
    let mut random = support::Random(13);
    let mut oscillator = Oscillator::new();
    for round in 0..200_000 {
        let mut stretches = 0;
        while oscillator.now() > 0 && stretches < 4 {
            let (first, last) = (random.word() >> (random.word() % 32), random.word() >> (random.word() % 32));
            let until = if stretches == 3 { 0 } else { random.word() & random.word() };
            match random.word() % 5 {
                0 => oscillator.synchronise(),
                1 => oscillator.jump_width(random.word()),
                2 => oscillator.set_width(random.word() & random.word()),
                _ => {}
            }
            let before = oscillator.now();
            if round % 2 == 0 {
                oscillator.run(until, first, last);
            } else if let Some(at) = oscillator.run_to_start(until, first, last) {
                assert!(
                    at <= before && at >= until.min(before) && oscillator.phase() == 0,
                    "{at} of {before} to {until}"
                );
            }
            assert!(oscillator.now() <= before);
            stretches += 1;
        }
        let waves = oscillator.finish();
        assert_eq!(oscillator.now(), u32::MAX);
        for value in [waves.sawtooth, waves.pulse, waves.triangle, waves.sub] {
            assert!(value.abs() < 8 * LEVEL, "{value}");
        }
        // A tick after stretches, and stretches after a tick.
        if round % 7 == 0 {
            let waves = oscillator.tick(None);
            assert!(waves.sawtooth.abs() < 8 * LEVEL);
        }
    }
}

#[test]
fn a_synchronised_pulse_narrower_than_a_sample_falls_where_its_width_ends() {
    // A pulse of 2% at 6 kHz is high for a third of a sample of twice
    // the output rate. Started again by the leader within a sample, it
    // has risen and fallen before the sample ends: it falls its width
    // after it rose, not at the sample.
    let leader_bin = 601;
    for factor in [2, 4] {
        let mut leader = Oscillator::new();
        let mut follower = Oscillator::new();
        leader.enable(0);
        leader.set_step(bin_step(leader_bin, factor));
        follower.set_step(bin_step(8_191, factor));
        follower.set_width(bin_step(8_191, 2) / 3);
        let count = (POINTS + SETTLE) * factor as usize;
        let signal: Vec<i32> = (0..count)
            .map(|_| {
                let _ = leader.tick(None);
                follower.tick(leader.started()).pulse
            })
            .collect();
        let found = aliasing(&float(&decimate(&signal, factor)[SETTLE..]), leader_bin);
        println!("{factor}x synchronised pulse narrower than a sample: {found:.1} dB");
        assert!(found < -80.0, "{factor}x: {found}");
    }
}

#[test]
fn an_oscillator_set_back_makes_again_what_it_made() {
    // Set back by a number of its steps, with everything on its way out
    // forgotten, an oscillator makes the same samples again that number
    // of samples later, once the samples it is told to discard are
    // past: twice its delay.
    let mut random = support::Random(17);
    for back in [1u32, 5, 23, 31] {
        let mut oscillator = Oscillator::new();
        let step = random.word() >> 4;
        oscillator.set_step(step);
        oscillator.set_width(random.word());
        for _ in 0..1_000 {
            let _ = oscillator.tick(None);
        }
        let mut again = oscillator.clone();
        again.rewind(step.wrapping_mul(back));
        let first: Vec<Waves> = (0..100).map(|_| oscillator.tick(None)).collect();
        let second: Vec<Waves> = (0..100 + back).map(|_| again.tick(None)).collect();
        let discarded = 2 * Oscillator::DELAY;
        for (index, (first, second)) in first.iter().zip(&second[back as usize..]).enumerate().skip(discarded) {
            // The sub-oscillator keeps its side through the starts made
            // again; the rest are the same to a unit of the tables.
            for (first, second) in
                [(first.sawtooth, second.sawtooth), (first.pulse, second.pulse), (first.triangle, second.triangle)]
            {
                assert!(first.abs_diff(second) <= 2, "{back} back, sample {index}: {first} and {second}");
            }
        }
    }
}

#[test]
fn a_waveform_switched_off_and_on_again_starts_from_nothing() {
    // A waveform that is switched off stops leaving its ring, and what
    // was on its way out stayed there: switched on again, however much
    // later, it began with those samples of the old moment.
    let mut oscillator = Oscillator::new();
    oscillator.set_step(frequency(100_000, 96_000));
    for _ in 0..300 {
        let _ = oscillator.tick(None);
    }
    oscillator.enable(waveform::ALL & !waveform::SAWTOOTH);
    for _ in 0..200 {
        assert_eq!(oscillator.tick(None).sawtooth, 0);
    }
    oscillator.enable(waveform::ALL);
    // No edge falls in these samples: the sawtooth's own delay passes
    // in silence, and then it is the sawtooth of the phase.
    let after: Vec<i32> = (0..2 * Oscillator::DELAY).map(|_| oscillator.tick(None).sawtooth).collect();
    assert!(after[..Oscillator::DELAY].iter().all(|&sample| sample == 0), "{after:?}");
    let expected = (oscillator.phase() >> 7).cast_signed() - LEVEL;
    let step = (oscillator.step() >> 7).cast_signed();
    let last = after[2 * Oscillator::DELAY - 1];
    let delay = i32::try_from(Oscillator::DELAY).unwrap();
    assert!((last - (expected - step * delay)).abs() <= step, "{last} for {expected}");
}
