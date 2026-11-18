//! A local sound's instrument and the state a page's rules read of it
//! (page-rules.md, RULE-83; RULE-28 to RULE-31, RULE-60, RULE-62): the
//! first three bytes choose the family, its variant and its option, the
//! next two the length of a bank image and the two after the length of a
//! setup block, both taken from the input, and the rest is read as steps of
//! four bytes: a status and two data bytes given as a `LOCAL_MIDI`
//! command's message, and a count of samples to render after it. A step
//! whose status is `0xFF` is instead a block with a cue event (RULE-109;
//! REQ-6614): the Cue Point meta event at the tick and with the number its
//! two data bytes give, played into the instrument with a note beside it.
//! Whatever the bank, the setup, the messages and the blocks hold, nothing
//! panics, every render gives its samples, a cue is reported with its
//! number at its sample only where it is one a block carries and changes
//! nothing the instrument plays, the meter stays within its ranges and
//! every field reads within the range its table states.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_audio_instruments::local::Local;
use neoteletext_audio_instruments::midi::{
    Banks, DATA_MAX, Instrument, LAST_TICK, MOST_BANK_BYTES, MOST_BLOCK_BYTES, PlayedCue, class, family, meta,
    play_block, sample_of_tick, status,
};
use neoteletext_audio_instruments::watch::Watched;

/// The families that play notes, which alone have a local sound (RULE-60).
const FAMILIES: [u8; 6] =
    [family::SYNTH7, family::SYNTH106, family::SYNTHD, family::SYNTH53, family::SYNTH8, family::SYNTH80];
/// The variants and key modes tried: one more than any family has.
const CHOICES: u8 = 4;
/// The most voices any family has: synth7's sixteen notes.
const MOST_VOICES: u8 = 16;
/// The bytes of a step: a status, two data bytes and a count of samples.
const STEP_BYTES: usize = 4;
/// The parts a rule can name: MIDI channels 1 to 16.
const PARTS: u8 = 16;

/// Checks everything a rule can read of the sound.
fn read_all(local: &Local, watched: &Watched, banks: &Banks<'_>) {
    for part in 0..PARTS {
        let meter = local.meter(part);
        assert!(meter.stage.code() <= 4 && meter.voices <= MOST_VOICES);
        assert!(meter.stage.code() != 0 || meter.level == 0, "an idle envelope is at level 0");
        for (number, field) in (0_u16..).zip(watched.fields()) {
            if let Some(value) = watched.field(part, number, banks) {
                assert!((field.least..=field.greatest).contains(&value), "{} reads {value}", field.name);
            }
        }
        assert!((0..=127).contains(&watched.read(class::PROGRAM, part, 0, banks)));
        assert!((0..=128).contains(&watched.read(class::NOTES, part, 0, banks)));
        assert!((0..=16_383).contains(&watched.read(class::BEND, part, 0, banks)));
        for index in [0, 60, 127, 128, u16::MAX] {
            assert!((0..=1).contains(&watched.read(class::NOTE, part, index, banks)));
            assert!((0..=127).contains(&watched.read(class::VELOCITY, part, index, banks)));
            assert!((0..=127).contains(&watched.read(class::CONTROLLER, part, index, banks)));
        }
    }
}

/// Plays a block of one cue event, at tick `time` with number `number`,
/// and a note on after it, into the sound, rendering `pcm`: the cue is
/// reported at its sample where its time and number are a block's, and
/// the block is malformed or the cue passed over where they are not.
fn play_cue(local: &mut Local, banks: &Banks<'_>, time: u8, number: u8, pcm: &mut [i16]) {
    let block = [time, status::META, meta::CUE_POINT, 1, number, 0, status::NOTE_ON, 60, 100];
    let played = play_block(local, &block, banks, pcm);
    let cues = played.cues.as_slice();
    if time > DATA_MAX {
        // A delta time of one byte has its top bit clear (MIDI-24).
        assert!(played.malformed && cues.is_empty());
    } else if number > DATA_MAX {
        assert!(cues.is_empty() && played.passed_over >= 1);
    } else {
        assert!(time <= LAST_TICK);
        assert_eq!(cues, [PlayedCue { number, sample: sample_of_tick(time) as u16 }]);
        assert_eq!(played.events, 1, "the note after the cue is given to the instrument");
    }
}

fuzz_target!(|data: &[u8]| {
    let [chosen, variant, option, bank_high, bank_low, setup_high, setup_low, rest @ ..] = data else { return };
    let code = FAMILIES[usize::from(*chosen) % FAMILIES.len()];
    let bank_length = usize::from(u16::from_be_bytes([*bank_high, *bank_low])).min(MOST_BANK_BYTES);
    let (image, rest) = rest.split_at(bank_length.min(rest.len()));
    let setup_length = usize::from(u16::from_be_bytes([*setup_high, *setup_low])).min(MOST_BLOCK_BYTES);
    let (setup, steps) = rest.split_at(setup_length.min(rest.len()));
    let images = [image, image, image];
    let banks = Banks::new(&images);
    let (variant, option, split) = (variant % CHOICES, option % CHOICES, option >> 1);
    // A family and variant that have a local sound have fields to read, and
    // the reverse.
    let (local, watched) = (Local::new(code, variant, option, split), Watched::new(code, variant, &banks));
    let (Some(mut local), Some(mut watched)) = (local, watched) else {
        assert_eq!(Local::new(code, variant, 0, split).is_none(), Watched::new(code, variant, &banks).is_none());
        return;
    };
    let played = local.start_with(&banks, setup);
    assert!(played.events as usize <= neoteletext_audio_instruments::midi::MOST_EVENTS);
    watched.take_block(setup, &banks);
    read_all(&local, &watched, &banks);
    let mut pcm = [0_i16; u8::MAX as usize];
    for &[first, data1, data2, samples] in steps.as_chunks::<STEP_BYTES>().0 {
        if first == status::META {
            play_cue(&mut local, &banks, data1, data2, &mut pcm[..usize::from(samples)]);
            // The tracker follows the note the block held, where it was read.
            if data1 <= DATA_MAX {
                watched.channel_message(status::NOTE_ON, 60, 100, &banks);
            }
            continue;
        }
        let _ = local.channel_message(first, data1, data2, &banks);
        watched.channel_message(first, data1, data2, &banks);
        local.render(&mut pcm[..usize::from(samples)]);
        if samples & 1 == 0 {
            read_all(&local, &watched, &banks);
        }
    }
    local.release_all();
    watched.release_all();
    local.render(&mut pcm);
    read_all(&local, &watched, &banks);
});
