//! The Minimoog as an instrument: its keyboard's priority and single
//! trigger, glide, modulation, waveforms, noise, the output returned to
//! the external input, its MIDI messages, any patch and any messages
//! without a panic, the same samples from the same input, and what it
//! costs (TODO-6432).

// Without the feature there is nothing here to test.
#![cfg(feature = "minimoog")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test"
)]

#[path = "analog_support.rs"]
mod analog;
#[path = "minimoog_charts.rs"]
mod charts;
#[path = "minimoog_support.rs"]
mod support;

use std::time::Instant;

use analog::{Random, amplitude, decibels};
use neoteletext_audio_instruments::minimoog::{
    ContourPatch, Minimoog, OscillatorPatch, PATCH_BYTES, Patch, Range, Waveform,
};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{
    CONTROL, NOTE_OFF, NOTE_ON, PITCH_BEND, RATE, cents, instrument, peaks, pitch, play, render, rms, samples, triangle,
};

/// The pitch of a key at 8'.
fn tuned(key: u8) -> f64 {
    440.0 * ((f64::from(key) - 69.0) / 12.0).exp2()
}

/// The pitch of the next `seconds`, after a tenth of a second to settle.
fn heard(minimoog: &mut Minimoog, seconds: f64) -> f64 {
    let signal = render(minimoog, seconds);
    pitch(&signal[samples(0.1)..])
}

#[test]
fn the_lowest_key_held_sounds() {
    // Owner's manual, page 8: "If more than one key is held down, only
    // the lowest one has effect."
    let mut minimoog = instrument(&triangle());
    minimoog.event(NOTE_ON, 60, 100);
    assert!(cents(tuned(60), heard(&mut minimoog, 0.5)).abs() < 1.0);
    minimoog.event(NOTE_ON, 72, 100);
    assert!(cents(tuned(60), heard(&mut minimoog, 0.5)).abs() < 1.0, "a higher key changes nothing");
    minimoog.event(NOTE_ON, 53, 100);
    assert!(cents(tuned(53), heard(&mut minimoog, 0.5)).abs() < 1.0, "a lower key takes the note");
    minimoog.event(NOTE_OFF, 53, 0);
    assert!(cents(tuned(60), heard(&mut minimoog, 0.5)).abs() < 1.0, "and gives it back");
    minimoog.event(NOTE_OFF, 60, 0);
    assert!(cents(tuned(72), heard(&mut minimoog, 0.5)).abs() < 1.0);
    minimoog.event(NOTE_OFF, 72, 0);
    let _ = render(&mut minimoog, 0.3);
    assert!(!minimoog.sounding());
    assert_eq!(rms(&render(&mut minimoog, 0.1)), 0.0);
}

#[test]
fn the_contours_start_only_when_a_key_goes_down_with_none_held() {
    // Owner's manual, page 2: "A timing signal begins whenever a key on
    // the keyboard is depressed, and stops when all keys are released."
    let mut patch = triangle();
    patch.oscillators[0].range = Range::Two;
    patch.loudness_contour = ContourPatch { attack: 0, decay: 100, sustain: 20 };
    let mut minimoog = instrument(&patch);
    let level = |minimoog: &mut Minimoog| peaks(&render(minimoog, 0.02), 0.02)[0];
    minimoog.event(NOTE_ON, 60, 100);
    let struck = level(&mut minimoog);
    let _ = render(&mut minimoog, 1.0);
    let settled = level(&mut minimoog);
    assert!(struck > 3.0 * settled, "{struck} against {settled}");
    // A second key, lower, while the first is held: the pitch changes
    // and the contour does not start again.
    minimoog.event(NOTE_ON, 48, 100);
    let legato = level(&mut minimoog);
    assert!((legato / settled - 1.0).abs() < 0.1, "{legato} against {settled}");
    // Every key up, then a key: it starts again.
    minimoog.event(NOTE_OFF, 48, 0);
    minimoog.event(NOTE_OFF, 60, 0);
    let _ = render(&mut minimoog, 0.3);
    minimoog.event(NOTE_ON, 48, 100);
    let again = level(&mut minimoog);
    assert!((again / struck - 1.0).abs() < 0.1, "{again} against {struck}");
}

#[test]
fn the_glide_slides_to_the_new_key_and_the_switch_stops_it() {
    let mut patch = triangle();
    patch.glide = 50;
    patch.glide_on = true;
    let mut minimoog = instrument(&patch);
    minimoog.event(NOTE_ON, 57, 100);
    let _ = render(&mut minimoog, 8.0);
    minimoog.event(NOTE_ON, 45, 100);
    // The knob at 5 is an eighth of 5 s: after that long the pitch has
    // a share e^-1 of the octave left.
    let _ = render(&mut minimoog, 0.625 - 0.05);
    let passing = pitch(&render(&mut minimoog, 0.1));
    let left = cents(tuned(45), passing);
    println!("{left:.1} cents left of 1200 after one time constant");
    assert!((left - 1200.0 * f64::exp(-1.0)).abs() < 25.0);
    let _ = render(&mut minimoog, 8.0);
    assert!(cents(tuned(45), heard(&mut minimoog, 0.5)).abs() < 1.0);

    patch.glide_on = false;
    let mut minimoog = instrument(&patch);
    minimoog.event(NOTE_ON, 57, 100);
    let _ = render(&mut minimoog, 0.2);
    minimoog.event(NOTE_ON, 45, 100);
    assert!(cents(tuned(45), pitch(&render(&mut minimoog, 0.3))).abs() < 1.0);
}

#[test]
fn the_wheel_lets_oscillator_3_move_the_pitch_by_seven_eighths_of_an_octave() {
    // Oscillator 3 at LO, off the keyboard, a square: the pitch
    // alternates between two, 1.75 octaves apart with the wheel at its
    // top (service manual, figure 9-2: "1.75 P-P MAX", a volt an octave).
    let mut patch = triangle();
    patch.oscillator_modulation = true;
    patch.oscillator_3_control = false;
    patch.oscillators[2].waveform = Waveform::Square;
    patch.oscillators[2].frequency = -350;
    let swing = |wheel: u8| {
        let mut minimoog = instrument(&patch);
        minimoog.event(CONTROL, 1, wheel);
        minimoog.event(NOTE_ON, 69, 100);
        let signal = render(&mut minimoog, 6.0);
        let pitches: Vec<f64> = signal.chunks(samples(0.05)).map(pitch).filter(|&found| found > 0.0).collect();
        let high = pitches.iter().fold(0.0f64, |high, &found| high.max(found));
        let low = pitches.iter().fold(f64::MAX, |low, &found| low.min(found));
        cents(low, high)
    };
    let (none, full) = (swing(0), swing(127));
    println!("wheel down: {none:.2} cents; wheel up: {full:.1} cents");
    assert!(none < 0.5);
    assert!((full - 2100.0).abs() < 15.0);
    // The wheel's law is the square of its travel.
    assert!((swing(64) - 2100.0 * (64.0f64 / 127.0).powi(2)).abs() < 10.0);
}

#[test]
fn the_filter_modulation_switch_sends_the_same_to_the_cutoff() {
    let mut patch = support::whistle(0);
    patch.keyboard_control = [false, false];
    patch.filter_modulation = true;
    patch.oscillator_3_control = false;
    patch.oscillators[2].waveform = Waveform::Square;
    patch.oscillators[2].frequency = -700;
    let mut minimoog = instrument(&patch);
    // A quarter of the travel: a sixteenth of 5.3 octaves, peak to peak.
    minimoog.event(CONTROL, 1, 32);
    minimoog.event(NOTE_ON, 60, 100);
    let signal = render(&mut minimoog, 12.0);
    let pitches: Vec<f64> =
        signal[samples(4.0)..].chunks(samples(0.1)).map(pitch).filter(|&found| found > 0.0).collect();
    let high = pitches.iter().fold(0.0f64, |high, &found| high.max(found));
    let low = pitches.iter().fold(f64::MAX, |low, &found| low.min(found));
    let expected = 1200.0 * 1.75 * 100.0 / 33.0 * (32.0f64 / 127.0).powi(2);
    println!("{:.1} cents for {expected:.1}", cents(low, high));
    assert!((cents(low, high) - expected).abs() < 25.0);
}

#[test]
fn the_rectangular_waves_have_the_schematics_widths() {
    // The second harmonic of a pulse of width d against its first is
    // cos(pi d): none for the square, 0.613 at 29%, 0.891 at 15%.
    for (selected, width) in [(Waveform::Square, 0.5), (Waveform::Wide, 0.29), (Waveform::Narrow, 0.15)] {
        let mut patch = Patch::DEFAULT;
        patch.oscillators[0].waveform = selected;
        patch.oscillators[0].range = Range::Sixteen;
        patch.oscillators[0].volume = 10;
        let signal = play(&patch, 45, 1.5, 0.0);
        let steady = &signal[samples(0.5)..];
        let ratio = amplitude(steady, 110.0 / RATE) / amplitude(steady, 55.0 / RATE);
        let expected = (std::f64::consts::PI * width).cos().abs();
        println!("{selected:?}: {ratio:.4} for {expected:.4}");
        assert!((ratio - expected).abs() < 0.01);
    }
}

#[test]
fn the_second_waveform_is_a_triangle_with_a_sawtooth_and_on_oscillator_3_a_sawtooth_reversed() {
    let level = |patch: &Patch, harmonic: f64| {
        let signal = play(patch, 45, 1.5, 0.0);
        amplitude(&signal[samples(0.5)..], harmonic * 110.0 / RATE)
    };
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].volume = 10;
    let sawtooth = level(&patch, 2.0);
    patch.oscillators[0].waveform = Waveform::Shark;
    // A triangle has no second harmonic: what is there is the sawtooth's
    // 10 / 57.
    let shark = level(&patch, 2.0);
    println!("second harmonic: {:.4} of the sawtooth's", shark / sawtooth);
    assert!((shark / sawtooth - 10.0 / 57.0).abs() < 0.01);

    // Oscillator 3's: the sawtooth's spectrum, and with oscillator 1's
    // sawtooth at the same pitch and level, silence.
    patch.oscillators[0].waveform = Waveform::Sawtooth;
    patch.oscillators[2] = patch.oscillators[0];
    patch.oscillators[2].waveform = Waveform::Shark;
    let both = rms(&play(&patch, 45, 1.0, 0.0)[samples(0.5)..]);
    patch.oscillators[0].on = false;
    let alone = rms(&play(&patch, 45, 1.0, 0.0)[samples(0.5)..]);
    println!("reversed alone {:.1} dB, with the sawtooth {:.1} dB", decibels(alone), decibels(both));
    assert!(alone > 0.01 && both < alone / 20.0);
}

#[test]
fn a_loud_mixer_is_bent_and_a_quiet_one_is_not() {
    let level = |volumes: [u8; 3]| {
        let mut patch = Patch::DEFAULT;
        for (oscillator, volume) in patch.oscillators.iter_mut().zip(volumes) {
            *oscillator = OscillatorPatch { volume, on: volume > 0, ..Patch::DEFAULT.oscillators[0] };
        }
        rms(&play(&patch, 57, 1.0, 0.0)[samples(0.5)..])
    };
    // From 2 to 5 on one oscillator's volume: two and a half times.
    let quiet = decibels(level([50, 0, 0]) / level([20, 0, 0]));
    // Three oscillators at 10, in step, against one: three times, less
    // what the curve takes.
    let loud = decibels(level([100, 100, 100]) / level([100, 0, 0]));
    println!("2 to 5: {quiet:+.2} dB for +7.96; one to three at 10: {loud:+.2} dB for +9.54 unbent");
    assert!((quiet - 7.96).abs() < 0.15);
    assert!((1.5..4.0).contains(&(9.54 - loud)));
}

#[test]
fn the_noise_is_five_decibels_below_an_oscillator_and_pink_falls_three_decibels_an_octave() {
    // The noise's level and colour are the generators'; the ladder is
    // taken out of the measurement by opening it as far as it goes, with
    // the contour at 10 holding it 8.5 octaves above the dial. With the
    // dial alone, at 28 kHz, the four poles take 7 dB off the white noise
    // at 20 kHz and a third of a decibel off the pink noise's top octave,
    // and the reading is the filter's.
    let open = |patch: &Patch| {
        let mut patch = *patch;
        patch.contour_amount = 100;
        patch
    };
    let mut patch = Patch::DEFAULT;
    let sawtooth = rms(&play(&open(&patch), 69, 2.0, 0.0)[samples(0.5)..]);
    patch.oscillators[0].on = false;
    patch.noise_on = true;
    patch.noise_volume = 100;
    let white = play(&open(&patch), 69, 12.0, 0.0);
    let below = decibels(sawtooth / rms(&white));
    // What the dial's 28 kHz takes off the band the output carries: four
    // poles there take 2.7 dB off white noise to 24 kHz.
    let dial = decibels(sawtooth / rms(&play(&patch, 69, 12.0, 0.0))) - below;
    println!("white noise {below:.2} dB below the sawtooth; the filter at 28 kHz takes {dial:.2} dB more");
    assert!((below - 5.0).abs() < 0.5, "{below}");
    assert!((1.5..3.0).contains(&dial), "{dial}");

    patch.noise_pink = true;
    let pink = play(&open(&patch), 69, 12.0, 0.0);
    // The power in each octave band, from the spectrum's bins.
    let bands = |signal: &[f64]| -> Vec<f64> {
        let spectrum = analog::spectrum(&signal[..1 << 19]);
        let bin = RATE / f64::from(1u32 << 19);
        [100.0, 200.0, 400.0, 800.0, 1_600.0, 3_200.0]
            .iter()
            .map(|&low: &f64| {
                let (from, to) = ((low / bin) as usize, (2.0 * low / bin) as usize);
                spectrum[from..to].iter().map(|value| value * value).sum::<f64>()
            })
            .collect()
    };
    let (white_bands, pink_bands) = (bands(&white), bands(&pink));
    for (index, (white, pink)) in white_bands.windows(2).zip(pink_bands.windows(2)).enumerate() {
        let (white, pink) = (10.0 * (white[1] / white[0]).log10(), 10.0 * (pink[1] / pink[0]).log10());
        println!("octave {index}: white {white:+.2} dB, pink {pink:+.2} dB");
        // Equal power in each octave for pink, twice it for white.
        assert!((white - 3.0).abs() < 0.8 && pink.abs() < 0.8);
    }
    // Each of the generators' samples is held for two of the voice's,
    // which takes 0.6 dB off the white noise at 20 kHz and no more.
    let spectrum = analog::spectrum(&white[..1 << 19]);
    let bin = RATE / f64::from(1u32 << 19);
    let about = |hertz: f64| -> f64 {
        let (from, to) = (((hertz - 500.0) / bin) as usize, ((hertz + 500.0) / bin) as usize);
        spectrum[from..to].iter().map(|value| value * value).sum::<f64>()
    };
    let top = 10.0 * (about(19_500.0) / about(1_000.0)).log10();
    println!("white noise at 19.5 kHz: {top:+.2} dB of what it is at 1 kHz");
    assert!((-1.0..0.0).contains(&top), "{top}");
}

#[test]
fn the_output_returned_to_the_external_input_thickens_the_sound_and_stays_bounded() {
    let mut patch = Patch::DEFAULT;
    patch.oscillators[0].range = Range::ThirtyTwo;
    patch.cutoff = 0;
    let dry = play(&patch, 45, 2.0, 0.5);
    patch.external_on = true;
    let mut last = rms(&dry[samples(0.5)..samples(2.0)]);
    for volume in [20, 50, 100] {
        patch.external_volume = volume;
        let wet = play(&patch, 45, 2.0, 0.5);
        let level = rms(&wet[samples(0.5)..samples(2.0)]);
        let peak = wet.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
        println!("external {volume}: {:.2} dB, peak {:.2} dB", decibels(level), decibels(peak));
        assert!(level > last, "more returns, more level");
        assert!(peak < 0.99, "and never the rail");
        // The note is still the key's, and it still dies.
        assert!(amplitude(&wet[samples(0.5)..samples(2.0)], 27.5 / RATE) > 0.1);
        assert!(rms(&wet[samples(2.3)..]) < 0.001);
        last = level;
    }
    // With the switch off the knob does nothing.
    patch.external_on = false;
    assert_eq!(play(&patch, 45, 0.5, 0.0), dry[..samples(0.5)]);
}

#[test]
fn the_drift_is_seeded_bounded_and_nothing_when_off() {
    let mut patch = triangle();
    patch.oscillators[0].range = Range::Two;
    let wander = |cents_of_drift: u8| {
        let mut minimoog = Minimoog::new();
        minimoog.set_drift(cents_of_drift);
        minimoog.set_patch(&patch);
        minimoog.event(NOTE_ON, 81, 100);
        let signal = render(&mut minimoog, 30.0);
        let found: Vec<f64> = signal.chunks(samples(0.25)).map(|chunk| cents(3_520.0, pitch(chunk))).collect();
        (
            found.iter().fold(f64::MAX, |low, &value| low.min(value)),
            found.iter().fold(f64::MIN, |high, &value| high.max(value)),
        )
    };
    let (low, high) = wander(0);
    println!("off: {low:+.3} to {high:+.3} cents");
    assert!(low > -0.05 && high < 0.05);
    let (low, high) = wander(10);
    println!("10 cents: {low:+.3} to {high:+.3} cents");
    assert!(low >= -10.1 && high <= 10.1 && high - low > 2.0);
    // The same instrument every time.
    let take = || {
        let mut minimoog = Minimoog::new();
        minimoog.event(NOTE_ON, 60, 100);
        let mut out = vec![0i16; 48_000];
        minimoog.render(&mut out);
        out
    };
    assert_eq!(take(), take());
}

#[test]
fn the_messages_it_takes_and_those_it_counts() {
    let mut minimoog = instrument(&triangle());
    // Aftertouch, program change, a controller it has not, a data byte
    // that is none.
    for (status, first, second) in
        [(0xa0, 60, 5), (0xc0, 1, 0), (0xd0, 9, 0), (CONTROL, 74, 9), (NOTE_ON, 200, 1), (0xf8, 0, 0)]
    {
        minimoog.event(status, first, second);
    }
    assert_eq!(minimoog.ignored_events(), 6);
    assert!(!minimoog.sounding());

    // The pedal holds the note as a key does; all notes off leaves it to
    // the pedal; all sound off silences at once.
    minimoog.event(NOTE_ON | 5, 60, 1);
    minimoog.event(CONTROL, 64, 127);
    minimoog.event(NOTE_OFF, 60, 0);
    assert!(rms(&render(&mut minimoog, 0.3)) > 0.01);
    minimoog.event(CONTROL, 123, 0);
    assert!(rms(&render(&mut minimoog, 0.3)) > 0.01);
    minimoog.event(CONTROL, 120, 0);
    // What the decimator still holds comes out first.
    let _ = render(&mut minimoog, 0.01);
    assert_eq!(rms(&render(&mut minimoog, 0.1)), 0.0);
    assert!(!minimoog.sounding());

    // Note on with velocity nothing is a note off; the volume's law is
    // its square; reset all controllers centres the wheel.
    minimoog.event(NOTE_ON, 60, 100);
    let loud = rms(&render(&mut minimoog, 0.3));
    minimoog.event(CONTROL, 7, 64);
    let _ = render(&mut minimoog, 0.1);
    let soft = rms(&render(&mut minimoog, 0.3));
    assert!((soft / loud - (64.0f64 / 127.0).powi(2)).abs() < 0.01);
    minimoog.event(PITCH_BEND, 0, 0);
    minimoog.event(CONTROL, 121, 0);
    assert!(cents(tuned(60), heard(&mut minimoog, 0.4)).abs() < 1.0);
    minimoog.event(NOTE_ON, 60, 0);
    let _ = render(&mut minimoog, 0.3);
    assert!(!minimoog.sounding());
    assert_eq!(minimoog.ignored_events(), 6);

    // Reset keeps the patch and forgets the rest.
    minimoog.event(NOTE_ON, 60, 100);
    minimoog.reset();
    assert_eq!((minimoog.ignored_events(), minimoog.sounding(), *minimoog.patch()), (0, false, triangle()));
}

/// A patch from random bytes brought inside their ranges.
fn random_patch(random: &mut Random) -> Patch {
    loop {
        let mut bytes = [0u8; PATCH_BYTES];
        for byte in &mut bytes {
            *byte = random.word() as u8;
        }
        for at in [0, 8, 12, 19] {
            let limit = [(0, 242), (8, 700), (12, 700), (19, 500)].iter().find(|pair| pair.0 == at).unwrap().1;
            let value = (random.word() % (2 * limit + 1)) as i16 - limit as i16;
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        for at in [23, 25, 28, 30] {
            // Short times mostly, so that much happens in a short test.
            let value =
                if random.word().is_multiple_of(4) { random.word() % 10_001 } else { random.word() % 300 } as u16;
            bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        for at in [4, 5, 6, 7, 10, 11] {
            bytes[at] %= 6;
        }
        for at in [2, 3, 14, 15, 16, 17, 18, 21, 22, 27, 32, 33] {
            bytes[at] %= 101;
        }
        bytes[34] &= 0x7f;
        // One patch in a hundred is left a byte out of range: that it is
        // refused, and not a panic, is part of the test.
        bytes[35] &= if random.word().is_multiple_of(100) { 0xff } else { 0x3f };
        if let Ok(patch) = Patch::from_bytes(&bytes) {
            return patch;
        }
    }
}

/// Plays random patches and random messages, rendered in pieces of
/// `piece` samples; gives every sample.
fn storm(seed: u64, blocks: usize, piece: usize) -> Vec<i16> {
    let mut random = Random(seed);
    let mut minimoog = Minimoog::new();
    let mut out = vec![0i16; blocks * BLOCK_SAMPLES];
    let mut done = 0;
    for block in out.chunks_mut(BLOCK_SAMPLES) {
        if random.word().is_multiple_of(16) {
            minimoog.set_patch(&random_patch(&mut random));
        }
        for _ in 0..random.word() % 4 {
            let status = match random.word() % 16 {
                0..=5 => NOTE_ON,
                6..=9 => NOTE_OFF,
                10..=12 => CONTROL,
                13 => PITCH_BEND,
                _ => random.word() as u8,
            };
            let first = match (status, random.word() % 4) {
                (CONTROL, 0) => [1, 7, 64, 120, 121, 123][random.word() as usize % 6],
                (NOTE_ON | NOTE_OFF, 0..=2) => 30 + (random.word() % 60) as u8,
                _ => random.word() as u8,
            };
            minimoog.event(
                status,
                first,
                random.word() as u8 & if random.word().is_multiple_of(32) { 0xff } else { 0x7f },
            );
        }
        for part in block.chunks_mut(piece) {
            minimoog.render(part);
        }
        done += block.len();
    }
    assert_eq!(done, out.len());
    out
}

#[test]
fn random_patches_and_messages_never_panic_and_never_stick_at_the_rail() {
    let blocks = if cfg!(debug_assertions) { 600 } else { 6_000 };
    for seed in [1, 2, 3] {
        let out = storm(seed, blocks, BLOCK_SAMPLES);
        let railed = out.iter().filter(|&&sample| sample == i16::MAX || sample == i16::MIN).count();
        // A hundredth of the reference note's peak.
        let loud = out.iter().filter(|&&sample| sample.unsigned_abs() > 26).count();
        println!("seed {seed}: {loud} of {} samples sound, {railed} at the rail", out.len());
        assert!(loud > out.len() / 20, "the storm makes sound");
        assert!(railed < out.len() / 1_000, "{railed}");
    }
}

#[test]
fn a_volume_message_between_two_renders_does_not_step_the_signal() {
    // The largest step from one sample to the next.
    let step = |signal: &[i16]| signal.windows(2).map(|pair| (pair[1] - pair[0]).unsigned_abs()).max().unwrap_or(0);
    // Oscillator 1's triangle on the A above middle C: its largest step is its
    // slope, and anything sudden is larger. The volume's gain was worked
    // out once for each call to render, so a volume message moved it
    // between one sample and the next (TODO-6438).
    let patch = triangle();
    // Half a second for the note to settle; then twenty changes of the
    // volume 3,058 samples apart, the calls to render cut at `cut`
    // samples; then the volume at its half.
    let lead = SAMPLE_RATE as usize / 2;
    let run = |cut: usize| {
        let mut minimoog = instrument(&patch);
        minimoog.event(NOTE_ON, 69, 100);
        let mut out = vec![0i16; 21 * 3_058 + SAMPLE_RATE as usize / 2];
        minimoog.render(&mut vec![0i16; lead]);
        for (turn, piece) in out.chunks_mut(3_058).enumerate() {
            minimoog.event(CONTROL, 7, [127, 64, 127, 0][turn.min(21) % 4]);
            for part in piece.chunks_mut(cut) {
                minimoog.render(part);
            }
        }
        out
    };
    let out = run(3_058);
    assert_eq!(out, run(7), "the calls' lengths change the samples");
    // The note alone, at the full volume of the first 3,058 samples.
    let (alone, changes) = (&out[..3_058], &out[3_058..21 * 3_058]);
    let (steady, worst) = (step(alone), step(changes));
    println!("a step of {worst} units at most across the volume's changes, {steady} in the note");
    assert!(worst * 5 < steady * 6, "the volume clicks: {worst} against {steady}");
    // And the volume arrives: half the controller, to the sample of an
    // instrument that had it from the first.
    let mut there = instrument(&patch);
    there.event(CONTROL, 7, 64);
    there.event(NOTE_ON, 69, 100);
    let mut same = vec![0i16; lead + out.len()];
    there.render(&mut same);
    let settled = out.len() - SAMPLE_RATE as usize / 4;
    assert_eq!(out[settled..], same[lead + settled..], "the volume does not arrive");
    // A reset puts the gain back at once: the first sample of a note is
    // then the first sample of a new instrument's.
    let mut again = instrument(&patch);
    again.event(CONTROL, 7, 0);
    again.render(&mut vec![0i16; lead]);
    again.reset();
    again.event(NOTE_ON, 69, 100);
    let mut fresh = instrument(&patch);
    fresh.event(NOTE_ON, 69, 100);
    let (mut after_reset, mut from_new) = (vec![0i16; lead], vec![0i16; lead]);
    again.render(&mut after_reset);
    fresh.render(&mut from_new);
    assert_eq!(after_reset, from_new, "a reset leaves the volume on its way");
}

#[test]
fn the_same_input_gives_the_same_samples_however_the_rendering_is_cut() {
    let blocks = if cfg!(debug_assertions) { 150 } else { 1_500 };
    let whole = storm(7, blocks, BLOCK_SAMPLES);
    assert_eq!(whole, storm(7, blocks, BLOCK_SAMPLES));
    assert_eq!(whole, storm(7, blocks, 1));
    assert_eq!(whole, storm(7, blocks, 97));
    assert_ne!(whole, storm(8, blocks, BLOCK_SAMPLES));
}

#[test]
fn every_chart_sounds_at_a_sensible_level() {
    for (name, patch) in charts::CHARTS {
        let signal = play(&patch, 60, 1.0, 1.0);
        let peak = signal.iter().fold(0.0f64, |peak, &value| peak.max(value.abs()));
        println!("{name}: peak {:.1} dB, RMS {:.1} dB", decibels(peak), decibels(rms(&signal[..samples(1.0)])));
        assert!(peak > 0.01 && peak < 0.9, "{name}: {peak}");
    }
}

#[test]
#[ignore = "a measurement, not a check: run it by name, in a release build"]
fn rendering_speed() {
    // The heaviest patch: three oscillators, pink noise, both
    // modulations, the output returned, the filter oscillating.
    let mut patch = charts::VIOLIN;
    patch.oscillators[2] = patch.oscillators[0];
    patch.oscillator_3_control = true;
    patch.noise_on = true;
    patch.noise_pink = true;
    patch.noise_volume = 30;
    patch.modulation_mix = 50;
    patch.filter_modulation = true;
    patch.external_on = true;
    patch.external_volume = 40;
    patch.emphasis = 100;
    let mut minimoog = Minimoog::new();
    minimoog.set_patch(&patch);
    minimoog.event(CONTROL, 1, 100);
    minimoog.event(NOTE_ON, 48, 100);
    let seconds = 60;
    let mut out = vec![0i16; BLOCK_SAMPLES];
    let start = Instant::now();
    for _ in 0..seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES {
        minimoog.render(&mut out);
    }
    let taken = start.elapsed().as_secs_f64();
    println!("{seconds} s in {taken:.2} s: {:.1} times real time", seconds as f64 / taken);
}
