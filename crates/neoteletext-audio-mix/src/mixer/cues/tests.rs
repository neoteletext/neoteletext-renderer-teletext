#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "a test reports a failure by panicking, on data it built to a known size, and its counts are small"
)]

// Tests of page-rules.md RULE-109 (REQ-6614): cue marks in the mixer.
use super::super::{Mixer, Start};
use super::{HeardCue, MAX_PENDING_CUE_MARKS};
use std::format;
use std::vec;
use std::vec::Vec;

/// The rate of the stored sounds of these tests.
const RATE: u32 = 8_000;

fn start(key: (u8, u16), gain: u8) -> Start {
    Start { key, gain, fade_ms: 5, looped: false, duck_percent: 100, after_current: false }
}

fn constant(mixer: &mut Mixer, key: (u8, u16), value: i16, samples: usize) {
    mixer.define(key, RATE, samples);
    mixer.samples(key, 0, &vec![value; samples]);
}

fn render(mixer: &mut Mixer, samples: usize) -> Vec<i16> {
    let mut out = vec![0; samples];
    mixer.render(&mut out);
    out
}

/// The rate of a synthesizer's stream.
const STREAM_RATE: u32 = 48_000;
/// The samples of one block of a synthesizer's stream: 20 ms.
const STREAM_BLOCK: usize = 960;
/// The height of each step of [`stepped`].
const STEP: i16 = 8_000;

/// A stream whose level steps up by [`STEP`] at each of `cues`, so that
/// what is heard shows which cue's sample has been played.
fn stepped(cues: &[u64], samples: usize) -> Vec<i16> {
    (0..samples as u64).map(|at| STEP * cues.iter().filter(|&&cue| cue <= at).count() as i16).collect()
}

/// Plays `stream` on voice 2 of a mixer at `device_hz`, its samples and
/// the marks of `cues` given `lead_blocks` blocks ahead of the blocks
/// rendered, in device buffers of `frames`. Answers what was rendered and
/// each cue heard with the count of buffers rendered when it was reported.
fn hear(
    device_hz: u32,
    frames: usize,
    lead_blocks: usize,
    stream: &[i16],
    cues: &[(u64, u8)],
) -> (Vec<i16>, Vec<(usize, HeardCue)>) {
    let mut mixer = Mixer::new(device_hz);
    mixer.stream((7, 1), STREAM_RATE);
    mixer
        .start(2, Start { key: (7, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false });
    let blocks: Vec<&[i16]> = stream.chunks(STREAM_BLOCK).collect();
    let (mut given, mut out, mut heard) = (0, Vec::new(), Vec::new());
    let mut buffers = 0;
    while given < blocks.len() || mixer.active() {
        // The blocks due by now, and the lead ahead of them.
        let due = out.len() as u64 * u64::from(STREAM_RATE) / u64::from(device_hz) / STREAM_BLOCK as u64;
        while given < blocks.len() && given < due as usize + lead_blocks {
            let first = (given * STREAM_BLOCK) as u64;
            for &(position, number) in cues {
                if (first..first + STREAM_BLOCK as u64).contains(&position) {
                    assert!(mixer.stream_cue((7, 1), position, number));
                }
            }
            mixer.stream_samples((7, 1), blocks[given]);
            given += 1;
            if given == blocks.len() {
                mixer.forget((7, 1));
            }
        }
        let mut buffer = vec![0; frames];
        mixer.render(&mut buffer);
        out.extend_from_slice(&buffer);
        buffers += 1;
        mixer.take_heard_cues(&mut |cue| heard.push((buffers, cue)));
        assert!(buffers < 100_000, "the stream never ended");
    }
    (out, heard)
}

// Verifies RULE-109 (REQ-6614 item 3): a cue is reported when it is heard,
// not when it is rendered ahead. Three marks of a stream, given with its
// samples 400 ms, 600 ms and a second ahead of what is played, are each
// reported once, in order, for the voice that plays the stream, after the
// device buffer in which the sample each stands at was rendered and before
// the next, at several device rates and buffer sizes.
#[test]
fn a_cue_is_reported_after_the_buffer_that_holds_its_sample() {
    let cues = [(24_000, 5), (24_960, 0), (61_234, 127)];
    let positions: Vec<u64> = cues.iter().map(|cue| cue.0).collect();
    let stream = stepped(&positions, 96_000);
    for device_hz in [48_000, 44_100] {
        for frames in [128, 256, 480, 1_024] {
            for lead_blocks in [20, 30, 50] {
                let (out, heard) = hear(device_hz, frames, lead_blocks, &stream, &cues);
                let case = format!("{device_hz} Hz, {frames} frames, {lead_blocks} blocks ahead");
                let told: Vec<(usize, u8)> = heard.iter().map(|(_, cue)| (cue.voice, cue.number)).collect();
                assert_eq!(told, vec![(2, 5), (2, 0), (2, 127)], "{case}");
                assert!(heard.iter().all(|(_, cue)| cue.key == (7, 1)), "{case}");
                for (step, &(buffers, _)) in (1_i32..).zip(&heard) {
                    // The first sample rendered past the middle of the
                    // step: the cue's sample, or the one before it between
                    // samples.
                    let middle = i32::from(STEP) * step - i32::from(STEP) / 2;
                    let crossed = out.iter().position(|&sample| i32::from(sample) > middle).unwrap();
                    let holding = [crossed / frames + 1, (crossed + 1) / frames + 1];
                    assert!(holding.contains(&buffers), "{case}: step {step} at {crossed}, told after {buffers}");
                }
            }
        }
    }
}

// Verifies RULE-109: a stream holds at most 32 marks, one more is dropped
// and counted; a mark for a sound that is no live sound is dropped and
// counted; a stopped sound and a replaced one forget their marks, and so
// does a stream whose marks the port forgets; nothing is reported twice.
#[test]
fn cue_marks_are_bounded_and_a_stopped_sound_forgets_them() {
    let mut mixer = Mixer::new(STREAM_RATE);
    let count = |mixer: &mut Mixer| {
        let mut heard = Vec::new();
        mixer.take_heard_cues(&mut |cue| heard.push(cue.number));
        heard
    };
    constant(&mut mixer, (1, 1), 100, 4_000);
    assert!(!mixer.stream_cue((1, 1), 0, 1), "a stored sound has no marks");
    assert!(!mixer.stream_cue((9, 9), 0, 1), "nor a sound the mixer does not hold");
    assert_eq!(mixer.cue_marks_dropped(), 2);

    mixer.stream((7, 1), STREAM_RATE);
    for number in 0..MAX_PENDING_CUE_MARKS as u8 {
        assert!(mixer.stream_cue((7, 1), 100 + u64::from(number), number));
    }
    assert!(!mixer.stream_cue((7, 1), 200, 99));
    assert_eq!(mixer.cue_marks_dropped(), 3);
    // Nothing is heard before a voice plays it.
    mixer.stream_samples((7, 1), &vec![1_000; 48_000]);
    render(&mut mixer, 480);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());
    mixer
        .start(0, Start { key: (7, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false });
    render(&mut mixer, 480);
    assert_eq!(count(&mut mixer), (0..MAX_PENDING_CUE_MARKS as u8).collect::<Vec<u8>>(), "all 32, in order, once");
    render(&mut mixer, 480);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());
    // Heard marks make room for more.
    assert!(mixer.stream_cue((7, 1), 2_000, 1));
    assert!(mixer.stream_cue((7, 1), 2_001, 2));

    // A stopped sound has no further cue, though it fades for a second.
    mixer.stop(0, 1_000);
    render(&mut mixer, 4_800);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());
    assert!(mixer.active(), "still fading");
    // Its marks are forgotten, not held: played again, it reports none.
    mixer
        .start(0, Start { key: (7, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false });
    render(&mut mixer, 480);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());

    // A sound replaced on its voice forgets its marks too.
    mixer.stream((8, 1), STREAM_RATE);
    mixer.stream_samples((8, 1), &vec![1_000; 48_000]);
    mixer
        .start(1, Start { key: (8, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false });
    assert!(mixer.stream_cue((8, 1), 30_000, 3));
    mixer.start(1, start((1, 1), 100));
    render(&mut mixer, 48_000);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());

    // The port forgets a paused page's marks.
    mixer.stream((6, 1), STREAM_RATE);
    mixer.stream_samples((6, 1), &vec![1_000; 48_000]);
    mixer
        .start(3, Start { key: (6, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false });
    assert!(mixer.stream_cue((6, 1), 100, 4));
    assert!(mixer.stream_cue((6, 1), 30_000, 5));
    mixer.forget_cues((6, 1));
    render(&mut mixer, 4_800);
    assert_eq!(count(&mut mixer), Vec::<u8>::new());
    assert_eq!(mixer.cue_marks_dropped(), 3);
}

// Verifies RULE-109: one stream played on two voices reports each cue once
// for each voice, as each plays the cue's sample.
#[test]
fn a_stream_on_two_voices_reports_a_cue_for_each() {
    let mut mixer = Mixer::new(STREAM_RATE);
    mixer.stream((7, 1), STREAM_RATE);
    mixer.stream_samples((7, 1), &vec![1_000; 48_000]);
    for voice in [0, 3] {
        mixer.start(
            voice,
            Start { key: (7, 1), gain: 100, fade_ms: 0, looped: false, duck_percent: 100, after_current: false },
        );
    }
    assert!(mixer.stream_cue((7, 1), 29_000, 9));
    let mut heard = Vec::new();
    for _ in 0..20 {
        render(&mut mixer, 480);
        mixer.take_heard_cues(&mut |cue| heard.push((cue.voice, cue.number)));
    }
    assert_eq!(heard, vec![(0, 9), (3, 9)]);
}
