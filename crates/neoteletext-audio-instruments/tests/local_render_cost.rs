//! What a local sound costs to render (`neoteletext-common/docs/page-rules.md`,
//! RULE-62: "a measured figure for each family's cost of rendering one
//! buffer of eight held notes, recorded with the date"; rdm REQ-6527,
//! TODO-6551 step 3): for every family that plays notes, the time to render
//! a device buffer in the blocks the sound helper renders it in, with eight
//! notes held. The test prints the figures and holds each to the buffer's
//! own time, past which RULE-62 counts an overrun.

// Without every family there is nothing here to test.
#![cfg(all(
    feature = "synth6581",
    feature = "synth7",
    feature = "synth106",
    feature = "synthd",
    feature = "synth53",
    feature = "synth8",
    feature = "synth80"
))]
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::print_stdout,
    reason = "a test reports a failure by panicking, on data it built to a known size, and a measurement prints its figures"
)]

#[path = "rules_support.rs"]
mod support;

use std::time::{Duration, Instant};

use neoteletext_audio_instruments::local::Local;
use neoteletext_audio_instruments::midi::{Banks, Instrument, status};
use support::note_kinds;

/// The samples of one block of a local sound (RULE-62,
/// `neoteletext_audio_mix::LOCAL_BLOCK_SAMPLES`, which this crate does not
/// depend on).
const BLOCK: usize = 128;
/// The frames of the device buffers measured: the two small buffers
/// RULE-97 asks for and the usual one it falls back to (RULE-64's table).
const BUFFERS: [usize; 3] = [128, 256, 480];
/// The samples a second of a local sound (RULE-62).
const RATE: u64 = 48_000;
/// The notes held (RULE-62: "eight held notes"): two octaves of a chord.
const KEYS: [u8; 8] = [48, 52, 55, 60, 64, 67, 72, 76];
/// Buffers rendered before the measurement, so that every envelope has
/// reached the level it holds.
const WARM_UP: usize = 200;
/// Buffers measured.
const MEASURED: usize = 500;

/// The mean and the longest time to render one buffer of `frames`, in
/// blocks of [`BLOCK`], of `local` as it stands.
fn cost(local: &mut Local, frames: usize) -> (Duration, Duration) {
    let mut buffer = vec![0_i16; frames];
    let mut render = |local: &mut Local| {
        for block in buffer.chunks_mut(BLOCK) {
            local.render(block);
        }
        std::hint::black_box(&buffer);
    };
    for _ in 0..WARM_UP {
        render(local);
    }
    let (mut total, mut longest) = (Duration::ZERO, Duration::ZERO);
    for _ in 0..MEASURED {
        let began = Instant::now();
        render(local);
        let took = began.elapsed();
        total += took;
        longest = longest.max(took);
    }
    (total / MEASURED as u32, longest)
}

/// RULE-62 (REQ-6527, TODO-6551 step 3): each family renders a device
/// buffer of eight held notes in less than the buffer's own time, the
/// bound past which the sound helper counts an overrun. It measures a
/// release build: a debug build is some tens of times slower and says
/// nothing of the browser's.
#[test]
#[cfg_attr(debug_assertions, ignore = "it measures a release build: run it with --release")]
fn each_family_renders_eight_held_notes_within_the_buffers_time() {
    println!("family     voices  frames  buffer(us)  mean(us)  longest(us)  mean share of the buffer");
    for kind in note_kinds() {
        let banks = Banks::none();
        let mut local = Local::new(kind.code, kind.variant, 0, 60).unwrap();
        local.start_with(&banks, &[]);
        for key in KEYS {
            let _ = local.channel_message(status::NOTE_ON, key, 100, &banks);
        }
        for frames in BUFFERS {
            let (mean, longest) = cost(&mut local, frames);
            // A family with fewer voices than eight holds as many as it has.
            let voices = local.meter(0).voices;
            let buffer = Duration::from_micros(frames as u64 * 1_000_000 / RATE);
            println!(
                "{:9}  {voices:6}  {frames:6}  {:10}  {:8.1}  {:11.1}  {:5.1} %",
                kind.name,
                buffer.as_micros(),
                mean.as_secs_f64() * 1e6,
                longest.as_secs_f64() * 1e6,
                mean.as_secs_f64() / buffer.as_secs_f64() * 100.0
            );
            assert!(voices >= 1, "{}: no note is held", kind.name);
            assert!(mean < buffer, "{}: {frames} frames took {mean:?}, the buffer is {buffer:?}", kind.name);
        }
    }
}
