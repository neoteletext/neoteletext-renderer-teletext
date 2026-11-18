//! How soon a local sound's note is heard (`neoteletext-common/docs/page-rules.md`,
//! RULE-62 and RULE-64; rdm REQ-6527, TODO-6577): for every family that
//! plays notes, a note on given before a block sounds within that block's
//! first samples, wherever in the instrument's own cycles the block falls.

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
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

#[path = "rules_support.rs"]
mod support;

use neoteletext_audio_instruments::local::Local;
use neoteletext_audio_instruments::midi::{Banks, Instrument, Outcome, family, status};
use support::{Kind, note_kinds};

/// The samples of one block of a local sound (RULE-62,
/// `neoteletext_audio_mix::LOCAL_BLOCK_SAMPLES`, which this crate does not
/// depend on).
const BLOCK: usize = 128;
/// The sample of a block by which a family's default patch has sounded
/// after a note on given before it. The slowest but synth106, synth80,
/// was measured at sample 30 (18 November 2026), and a third again is
/// allowed over it.
const FIRST_SAMPLE_BOUND: usize = 40;
/// The same for synth106, whose voice takes its key at the next tick of 8
/// samples and whose filter's control voltage then rises from rest
/// through the instrument's 1 ms of 10 kilohms onto 0.1 microfarad
/// (`synth106::voice`, `HOLD_SMOOTHING_MICROSECONDS`): the output stays
/// under one unit of a 16-bit sample for 48 samples after the voice
/// starts, measured, and 13 with that voltage set at once (TODO-6577).
/// It is the patch's own opening, not a wait: the voice runs from the
/// tick after the key.
const SYNTH106_FIRST_SAMPLE_BOUND: usize = 48 + 8;
const _: () = assert!(SYNTH106_FIRST_SAMPLE_BOUND < BLOCK && FIRST_SAMPLE_BOUND < SYNTH106_FIRST_SAMPLE_BOUND);
/// The silences rendered before the note, in samples: none, whole device
/// buffers of 480, and lengths that are no multiple of any family's
/// control period, so that the note falls at many places in it.
const SILENCES: [usize; 8] = [0, 1, 480, 487, 960, 1_000, 1_337, 4_799];

/// The place of the first sample that is not zero in the block rendered
/// after a note on, with the family's own default patch, after `silence`
/// samples of rest.
fn first_sample(kind: Kind, silence: usize) -> Option<usize> {
    let banks = Banks::none();
    let mut local = Local::new(kind.code, kind.variant, 0, 60).unwrap();
    local.start_with(&banks, &[]);
    let mut rest = vec![0_i16; silence];
    local.render(&mut rest);
    assert!(rest.iter().all(|sample| *sample == 0), "{}: silent before any note", kind.name);
    assert_eq!(local.channel_message(status::NOTE_ON, 60, 127, &banks), Outcome::Applied, "{}", kind.name);
    let mut block = [0_i16; BLOCK];
    local.render(&mut block);
    block.iter().position(|sample| *sample != 0)
}

/// RULE-62 (REQ-6527, TODO-6577): a note on given before a block is heard
/// within the block's first 40 samples in every family, and within 56 in
/// synth106 (variant 0), wherever the block falls in the instrument's
/// cycles. synth106's voices take their control voltages once a frame of
/// 200 samples and used to take their keys then too, so that its note
/// began up to 200 samples later: at sample 242 after one sample of rest.
#[test]
fn a_note_sounds_within_the_first_samples_of_its_block() {
    for kind in note_kinds() {
        let bound = if (kind.code, kind.variant) == (family::SYNTH106, 0) {
            SYNTH106_FIRST_SAMPLE_BOUND
        } else {
            FIRST_SAMPLE_BOUND
        };
        for silence in SILENCES {
            let first = first_sample(kind, silence);
            assert!(
                first.is_some_and(|first| first < bound),
                "{} after {silence} samples of rest: first sample {first:?}, bound {bound}",
                kind.name
            );
        }
    }
}
