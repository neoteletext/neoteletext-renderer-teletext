//! synth6581 as a whole: its registers, determinism, reset, and what any
//! byte at any address does (REQ-6412 item 4 and item 7).

// Without synth6581's feature there is nothing here to test.
#![cfg(feature = "synth6581")]
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    clippy::cast_sign_loss,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::print_stdout,
    reason = "a test"
)]

#[path = "synth6581_support.rs"]
mod support;

use neoteletext_audio_instruments::synth6581::{ChipClock, REGISTERS, Synth6581, Variant};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, SAMPLE_RATE};
use support::{CLOCKS, MODELS, Random, decibels, frequency_register, render, rms, start};

const TRIANGLE: u8 = 0x10;
const PULSE: u8 = 0x40;
const MODE_VOLUME: u8 = 24;
const FULL_VOLUME: u8 = 0x0F;

/// A busy register image: three voices, each with another waveform, the
/// filter in use.
const BUSY: [u8; REGISTERS] = [
    0x25, 0x1D, 0x00, 0x08, 0x41, 0x09, 0xA8, // voice 1: pulse
    0x3C, 0x2B, 0x00, 0x00, 0x21, 0x0A, 0xC9, // voice 2: sawtooth
    0x77, 0x07, 0x00, 0x00, 0x15, 0x08, 0xF8, // voice 3: ring-modulated triangle
    0x04, 0x60, 0xA3, 0x1F, // filter: voices 1 and 2 through the low-pass
];

#[test]
fn a_new_chip_is_silent() {
    for model in MODELS {
        for clock in CLOCKS {
            let mut synth6581 = Synth6581::new(model, clock);
            assert_eq!((synth6581.model(), synth6581.clock(), synth6581.cycles()), (model, clock, 0));
            assert!(render(&mut synth6581, 0.1).iter().all(|&sample| sample == 0));
        }
    }
}

#[test]
fn two_chips_given_the_same_writes_give_the_same_samples() {
    for model in MODELS {
        for clock in CLOCKS {
            let mut random = Random(42);
            let mut first = Synth6581::new(model, clock);
            let mut second = Synth6581::new(model, clock);
            for _ in 0..200 {
                let (address, value) = (random.byte() % 32, random.byte());
                first.write(address, value);
                second.write(address, value);
                // One chip renders in blocks, the other a sample at a time.
                let mut block = [0i16; 97];
                first.render(&mut block);
                let mut single = [0i16; 97];
                for sample in single.chunks_mut(1) {
                    second.render(sample);
                }
                assert_eq!(block, single);
            }
            assert_eq!(first, second);
        }
    }
}

#[test]
fn reset_leaves_the_chip_as_new() {
    for model in MODELS {
        for clock in CLOCKS {
            let mut synth6581 = Synth6581::new(model, clock);
            synth6581.write_image(&BUSY);
            let first = render(&mut synth6581, 0.25);
            assert!(rms(&first) > 500.0);
            assert_ne!(synth6581, Synth6581::new(model, clock));
            synth6581.reset();
            assert_eq!(synth6581, Synth6581::new(model, clock));
            // And it plays the same again.
            synth6581.write_image(&BUSY);
            assert_eq!(render(&mut synth6581, 0.25), first);
        }
    }
}

#[test]
fn an_image_is_its_registers_written_in_order() {
    let mut image = Synth6581::new(Variant::Synth6581, ChipClock::Pal);
    let mut written = image.clone();
    image.write_image(&BUSY);
    for (address, &value) in BUSY.iter().enumerate() {
        written.write(address as u8, value);
    }
    assert_eq!(image, written);
}

#[test]
fn addresses_above_the_registers_are_ignored() {
    let mut synth6581 = Synth6581::new(Variant::Synth8580, ChipClock::Ntsc);
    synth6581.write_image(&BUSY);
    let _ = render(&mut synth6581, 0.05);
    let before = synth6581.clone();
    for address in REGISTERS as u8..=255 {
        for value in [0x00, 0b0101_0101, 0xFF] {
            synth6581.write(address, value);
        }
    }
    assert_eq!(synth6581, before);
}

#[test]
fn every_byte_at_every_address_is_valid() {
    for model in MODELS {
        let mut synth6581 = Synth6581::new(model, ChipClock::Pal);
        let mut block = [0i16; 2];
        for value in 0..=255u8 {
            for address in 0..=255u8 {
                synth6581.write(address, value);
                let _ = synth6581.read(address);
                synth6581.render(&mut block);
            }
        }
        // An empty render is no render.
        let cycles = synth6581.cycles();
        synth6581.render(&mut []);
        assert_eq!(synth6581.cycles(), cycles);
    }
}

#[test]
fn the_output_level_leaves_headroom_for_three_voices() {
    // TODO-6427 fault 5: at first the output clipped three of nine tunes.
    // The scale is an own choice; no real chip was measured for it. One
    // triangle at full envelope and full volume spans the middle of what
    // it spans in two emulators run as separate programs (sidplayfp
    // 2.16.0, both engines, measured from their output on 17 November
    // 2026): 9,271 and 9,530 output units on a synth6581, 7,017 and 7,437 on
    // an synth8580. Before, synth8580's span was 7,534, above both. The gains
    // that gave those spans were 40 and 31; since every instrument plays
    // at one level (`tests/levels.rs`) they are 34 and 26, and the spans
    // that share of the emulators'.
    const NEAR_DB: f64 = 0.25;
    for (model, spans, share) in
        [(Variant::Synth6581, (9_271.0, 9_530.0), 34.0 / 40.0), (Variant::Synth8580, (7_017.0, 7_437.0), 26.0 / 31.0)]
    {
        let clock = ChipClock::Pal;
        let register = frequency_register(440.0, clock);
        let extremes = |synth6581: &mut Synth6581| {
            let _ = render(synth6581, 0.5);
            let samples = render(synth6581, 0.5);
            let lowest = i32::from(*samples.iter().min().unwrap());
            let highest = i32::from(*samples.iter().max().unwrap());
            (lowest, highest)
        };
        let mut synth6581 = Synth6581::new(model, clock);
        synth6581.write(MODE_VOLUME, FULL_VOLUME);
        start(&mut synth6581, 0, register, TRIANGLE);
        let (lowest, highest) = extremes(&mut synth6581);
        let middle = f64::midpoint(spans.0, spans.1) * share;
        let found = decibels(f64::from(highest - lowest) / middle);
        assert!(found.abs() < NEAR_DB, "{model:?}: a triangle spans {} units, {found} dB", highest - lowest);
        // Three pulses in step, the loudest the three voices can be with
        // no filter, stay under half of a sample's range.
        let mut synth6581 = Synth6581::new(model, clock);
        synth6581.write(MODE_VOLUME, FULL_VOLUME);
        for voice in 0..3 {
            synth6581.write(support::voice(voice) + 3, 0x08);
            start(&mut synth6581, voice, register, PULSE);
        }
        let (lowest, highest) = extremes(&mut synth6581);
        let half = i32::from(i16::MAX) / 2;
        assert!(lowest > -half && highest < half, "{model:?}: three pulses reach {lowest} and {highest}");
    }
}

/// How many seconds of audio one chip renders in a second, in a release
/// build: `cargo test --release ... -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, which needs a release build and an idle machine to mean anything"]
fn rendering_speed() {
    for model in MODELS {
        let mut synth6581 = Synth6581::new(model, ChipClock::Pal);
        synth6581.write_image(&BUSY);
        let mut block = [0i16; BLOCK_SAMPLES];
        let seconds = 60usize;
        let blocks = seconds * SAMPLE_RATE as usize / BLOCK_SAMPLES;
        let started = std::time::Instant::now();
        let mut slowest = std::time::Duration::ZERO;
        let mut sum = 0i64;
        for _ in 0..blocks {
            let block_started = std::time::Instant::now();
            synth6581.render(&mut block);
            slowest = slowest.max(block_started.elapsed());
            sum += block.iter().map(|&sample| i64::from(sample)).sum::<i64>();
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "{model:?}: {seconds} s of audio in {elapsed:.3} s: {:.1} times real time; slowest 20 ms block {:.3} ms (checksum {sum})",
            seconds as f64 / elapsed,
            slowest.as_secs_f64() * 1000.0
        );
    }
}
