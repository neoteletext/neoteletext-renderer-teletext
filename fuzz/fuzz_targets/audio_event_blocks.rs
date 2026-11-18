//! The synthesizers' event-list reader with each family's front end behind
//! it (midi-architecture.md, MIDI-23, MIDI-33, MIDI-34, MIDI-123,
//! MIDI-124): the first byte chooses the family, the next two the length of
//! a bank image taken from the input, and the rest is read as a sequence of
//! blocks, each a two-byte length and its bytes, played in order with a
//! lost block wherever a length's top bit is set. Whatever a block holds,
//! the reader ends, every block renders exactly its 960 samples, and the
//! writer's own blocks read back as written.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_audio_instruments::midi::block::{BlockWriter, Events, Read, check};
use neoteletext_audio_instruments::midi::{Banks, Instrument, MOST_BLOCK_BYTES, play_block};
use neoteletext_audio_instruments::{BLOCK_SAMPLES, synth7, synth8, synth53, synth80, synth106, synth6581, synthd};

/// A length's bit that marks the block lost.
const LOST: u16 = 0x8000;

/// Plays every block of `blocks` into `instrument`.
fn play(instrument: &mut dyn Instrument, banks: &Banks<'_>, mut blocks: &[u8]) {
    let _ = instrument.start(banks);
    let mut pcm = [0_i16; BLOCK_SAMPLES];
    while let [high, low, rest @ ..] = blocks {
        let length = u16::from_be_bytes([*high, *low]);
        let (block, after) = rest.split_at(usize::from(length & !LOST).min(rest.len()));
        if length & LOST != 0 {
            instrument.lost();
            instrument.render(&mut pcm);
        } else {
            let played = play_block(instrument, block, banks, &mut pcm);
            assert!(played.events as usize <= neoteletext_audio_instruments::midi::MOST_EVENTS);
        }
        blocks = after;
    }
}

/// Rewrites every event the reader finds with the writer and checks the
/// writer's block reads back the same (MIDI-24, MIDI-44).
fn round_trip(block: &[u8]) {
    let mut buffer = [0_u8; MOST_BLOCK_BYTES];
    let mut writer = BlockWriter::new(&mut buffer);
    let mut events = Vec::new();
    for read in Events::new(block) {
        if let Read::Event(event) = read
            && writer.push(event.time, &event.message).is_ok()
        {
            events.push(event);
        }
    }
    let again: Vec<_> = Events::new(writer.bytes())
        .map(|read| match read {
            Read::Event(event) => event,
            other => panic!("the writer's block read as {other:?}"),
        })
        .collect();
    assert_eq!(again, events);
    assert!(check(writer.bytes()).is_ok());
}

fuzz_target!(|data: &[u8]| {
    let [family, high, low, rest @ ..] = data else { return };
    let (image, blocks) = rest.split_at(usize::from(u16::from_be_bytes([*high, *low])).min(rest.len()));
    let images = [image, image];
    let banks = Banks::new(&images);
    let mut instrument: Box<dyn Instrument> = match family % 7 {
        0 => Box::new(synth6581::midi::Midi::new(synth6581::Variant::Synth8580, synth6581::ChipClock::Ntsc)),
        1 => Box::new(synth7::midi::Midi::new()),
        2 => Box::new(synth106::midi::Midi::new(synth106::Synth106::new(synth106::Model::Synth60))),
        3 => Box::new(synthd::midi::Midi::new(synthd::SynthD::new())),
        4 => Box::new(synth53::midi::Midi::new(synth53::Synth53::new(synth53::Model::Synth53))),
        5 => Box::new(synth8::midi::Midi::new(synth8::Synth8::new())),
        _ => Box::new(synth80::midi::Midi::new(synth80::Synth80::new())),
    };
    play(instrument.as_mut(), &banks, blocks);
    round_trip(blocks);
});
