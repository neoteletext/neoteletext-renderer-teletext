//! The DX7's low frequency oscillator at 48,000 samples a second in blocks
//! of 32, against the original's formulas, and what it does to a note
//! (REQ-6412 item 6).
#![cfg(feature = "dx7")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a test"
)]

use neoteletext_audio_instruments::dx7::msfa::lfo::{Lfo, Waveform};
use neoteletext_audio_instruments::dx7::{Bank, Patch, Rack, VoiceParams};

/// Blocks a second: 48,000 / 32.
const BLOCKS_A_SECOND: f64 = 1500.0;

const ALL: [Waveform; 6] = [
    Waveform::Triangle,
    Waveform::SawtoothDown,
    Waveform::SawtoothUp,
    Waveform::Square,
    Waveform::Sine,
    Waveform::SampleAndHold,
];

/// The frequency `lfo.cc` gives a speed, at the rate and block size the
/// original runs at (44,100 and 64): `unit = N * 25190424 / sample_rate +
/// 0.5`, `sr = speed == 0 ? 1 : (165 * speed) >> 6`, `sr *= sr < 160 ? 11 :
/// (11 + ((sr - 160) >> 4))`, and the phase, of which 2^32 is a turn, moves
/// by `unit * sr` a block.
fn original_hertz(speed: u32) -> f64 {
    let unit = (64.0 * 25_190_424.0 / 44_100.0 + 0.5_f64).floor();
    let mut sr = if speed == 0 { 1 } else { (165 * speed) >> 6 };
    sr *= if sr < 160 { 11 } else { 11 + ((sr - 160) >> 4) };
    unit * f64::from(sr) / 4_294_967_296.0 * 44_100.0 / 64.0
}

/// Each speed's frequency, counted from the sawtooth's falls over a
/// minute, is the original's within 0.1% (its unit is rounded to a whole
/// number, here as there).
#[test]
fn speeds_are_the_originals() {
    for speed in [0_u8, 1, 10, 35, 50, 75, 99] {
        let mut lfo = Lfo::new(0);
        lfo.reset(speed, 0, false, Waveform::SawtoothUp);
        let blocks = 60 * 1500;
        let mut last = lfo.getsample();
        let mut turns = 0;
        for _ in 0..blocks {
            let value = lfo.getsample();
            turns += u32::from(value < last);
            last = value;
        }
        let measured = f64::from(turns) / 60.0;
        let expected = original_hertz(u32::from(speed));
        assert!(
            (measured - expected).abs() <= 0.001 * expected + 1.0 / 60.0,
            "speed {speed}: {measured} Hz against {expected}"
        );
    }
    assert!((original_hertz(99) - 23.93).abs() < 0.01);
}

/// Every waveform stays within 0..1 in Q24 and uses most of it.
#[test]
fn waveforms_stay_in_range() {
    for waveform in ALL {
        let mut lfo = Lfo::new(7);
        lfo.reset(60, 0, false, waveform);
        let values: Vec<i32> = (0..30_000).map(|_| lfo.getsample()).collect();
        let (low, high) = (*values.iter().min().unwrap(), *values.iter().max().unwrap());
        assert!(low >= 0 && high <= 1 << 24, "{waveform:?}: {low} to {high}");
        assert!(low < 1 << 20 && high > 15 << 20, "{waveform:?}: {low} to {high}");
    }
    assert_eq!(Waveform::from_number(5), Some(Waveform::SampleAndHold));
    assert_eq!(Waveform::from_number(6), None);
    for (number, waveform) in ALL.into_iter().enumerate() {
        assert_eq!(Waveform::from_number(number as u8), Some(waveform));
    }
}

/// Sample and hold takes a new level once a period, from a sequence that
/// its seed alone decides: `(state * 179 + 17) & 0xff`.
#[test]
fn sample_and_hold_is_a_sequence_of_its_seed() {
    let run = |seed: u8| {
        let mut lfo = Lfo::new(seed);
        lfo.reset(99, 0, false, Waveform::SampleAndHold);
        let mut levels = Vec::new();
        for _ in 0..3000 {
            let value = lfo.getsample();
            if levels.last() != Some(&value) {
                levels.push(value);
            }
        }
        levels
    };
    let levels = run(0x5a);
    assert_eq!(levels, run(0x5a));
    assert_ne!(levels, run(0x5b));
    // About 24 periods a second for two seconds.
    assert!((45..=50).contains(&levels.len()), "{} levels", levels.len());
    let mut state = 0x5a_u8;
    for &level in &levels[1..] {
        state = state.wrapping_mul(179).wrapping_add(17);
        assert_eq!(level, (i32::from(state ^ 0x80) + 1) << 16);
    }
}

/// Key sync starts the waveform again when a key goes down; the delay is
/// nothing, then a rise, then everything, and longer for a larger setting.
#[test]
fn a_key_restarts_the_delay_and_a_synced_waveform() {
    let mut lfo = Lfo::new(0);
    lfo.reset(40, 0, true, Waveform::Triangle);
    lfo.keydown();
    let first: Vec<i32> = (0..50).map(|_| lfo.getsample()).collect();
    for _ in 0..777 {
        lfo.getsample();
    }
    lfo.keydown();
    assert_eq!((0..50).map(|_| lfo.getsample()).collect::<Vec<i32>>(), first);

    let seconds_to_full = |delay: u8| {
        let mut lfo = Lfo::new(0);
        lfo.reset(40, delay, false, Waveform::Triangle);
        lfo.keydown();
        let mut last = 0;
        for block in 0..20 * 1500 {
            let value = lfo.getdelay();
            assert!((0..=1 << 24).contains(&value) && value >= last, "delay {delay} block {block}");
            last = value;
            if value == 1 << 24 {
                return f64::from(block) / BLOCKS_A_SECOND;
            }
        }
        panic!("delay {delay} never ends");
    };
    assert!(seconds_to_full(0) < 0.001);
    let times = [seconds_to_full(25), seconds_to_full(50), seconds_to_full(75), seconds_to_full(99)];
    println!("delay 25, 50, 75, 99 reach full depth after {times:?} s");
    assert!(times.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(times[0] > 0.05 && times[3] < 10.0);
}

fn rack_with(change: impl FnOnce(&mut VoiceParams)) -> Rack {
    let mut params = *Patch::DEFAULT.params();
    change(&mut params);
    let mut bank = Bank::new();
    bank.set(0, Patch::new(params).unwrap());
    let mut rack = Rack::new();
    rack.set_bank(&bank);
    rack
}

/// The loudest and quietest hundredth of a second of two seconds of a
/// note, as peaks.
fn loudest_and_quietest(rack: &mut Rack) -> (i32, i32) {
    rack.event(0x90, 81, 100);
    let mut samples = vec![0_i16; 96_000];
    rack.render(&mut samples);
    let peaks: Vec<i32> =
        samples[4800..].chunks(480).map(|chunk| chunk.iter().map(|&s| i32::from(s).abs()).max().unwrap()).collect();
    (*peaks.iter().max().unwrap(), *peaks.iter().min().unwrap())
}

/// Amplitude modulation: none at sensitivity 0, and at full depth a swing
/// of about a half, two and eight doublings at sensitivities 1, 2 and 3
/// (this crate's own depths).
#[test]
fn amplitude_modulation_follows_its_sensitivity() {
    let swing = |sensitivity: u8, depth: u8| {
        let (loud, quiet) = loudest_and_quietest(&mut rack_with(|voice| {
            voice.lfo_speed = 40;
            voice.lfo_amp_mod_depth = depth;
            voice.operators[0].amp_mod_sensitivity = sensitivity;
        }));
        assert!((2000..=2047).contains(&loud), "sensitivity {sensitivity}: loudest {loud}");
        (f64::from(loud) / f64::from(quiet.max(1))).log2()
    };
    assert!(swing(0, 99) < 0.01);
    assert!(swing(3, 0) < 0.01);
    let swings = [swing(1, 99), swing(2, 99), swing(3, 99)];
    println!("amplitude modulation at full depth swings {swings:?} doublings");
    for (measured, expected) in swings.into_iter().zip([0.5, 2.0, 8.0]) {
        assert!((measured - expected).abs() < 0.1 * expected + 0.05, "{measured} against {expected}");
    }
    assert!(swing(2, 50) < swing(2, 99));
}

/// The patch's pitch modulation depth makes vibrato after the delay, and
/// a rack is about 12 kilobytes.
#[test]
fn pitch_modulation_waits_for_the_delay() {
    let mut rack = rack_with(|voice| {
        voice.lfo_speed = 45;
        voice.lfo_delay = 70;
        voice.lfo_pitch_mod_depth = 99;
        voice.pitch_mod_sensitivity = 5;
    });
    rack.event(0x90, 69, 100);
    let mut samples = vec![0_i16; 144_000];
    rack.render(&mut samples);
    // The spread of the periods, in cents, in a stretch of samples.
    let spread = |samples: &[i16]| {
        let mut last = None;
        let (mut shortest, mut longest) = (f64::MAX, 0.0_f64);
        for (index, pair) in samples.windows(2).enumerate() {
            let (before, after) = (f64::from(pair[0]), f64::from(pair[1]));
            if before < 0.0 && after >= 0.0 {
                let at = index as f64 + before / (before - after);
                if let Some(last) = last {
                    shortest = shortest.min(at - last);
                    longest = longest.max(at - last);
                }
                last = Some(at);
            }
        }
        1200.0 * (longest / shortest).log2()
    };
    let (early, late) = (spread(&samples[480..4800]), spread(&samples[96_000..]));
    println!("vibrato: {early} cents in the first tenth of a second, {late} cents after two seconds");
    assert!(early < 1.0 && late > 100.0);
    let size = size_of::<Rack>();
    println!("a rack is {size} bytes");
    assert!((8_000..16_000).contains(&size));
}
