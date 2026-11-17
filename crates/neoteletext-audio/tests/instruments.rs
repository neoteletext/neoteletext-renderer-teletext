//! Instrument sounds end to end (REQ-6411, DEC-6410): a synth6581 sound's register
//! images and a synth7 sound's slots, written by the publishing encoder, read
//! back by the receiver, rendered by the instruments and given to a
//! recording sink as samples.
#![cfg(all(feature = "encode", feature = "synth6581", feature = "synth7"))]
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

use neoteletext_audio::encode::{Command, Sound};
use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::{Record, SYNTH6581_CHIP_8580, SYNTH6581_CLOCK_NTSC, parse};
use neoteletext_audio::{Codec, INSTRUMENT_BLOCK_SAMPLES, INSTRUMENT_RATE};
use neoteletext_audio_instruments::synth7::Patch;

/// The samples of a block, as a length.
const BLOCK: usize = INSTRUMENT_BLOCK_SAMPLES as usize;

/// What the sink was given, assembled.
#[derive(Default)]
struct Recorder {
    defined: Vec<(SoundId, SoundFormat)>,
    /// Each `samples` call: the sound, its first sample and its length.
    stored: Vec<(SoundId, u32, usize)>,
    pcm: Vec<i16>,
    streams: Vec<(SoundId, u32)>,
    streamed: Vec<i16>,
    starts: Vec<(u8, SoundId)>,
    concealed: Vec<u16>,
}

impl VoiceSink for Recorder {
    fn define(&mut self, sound: SoundId, format: SoundFormat) {
        self.defined.push((sound, format));
        self.pcm = vec![0; format.samples as usize];
    }
    fn samples(&mut self, sound: SoundId, first: u32, pcm: &[i16]) {
        self.stored.push((sound, first, pcm.len()));
        self.pcm[first as usize..first as usize + pcm.len()].copy_from_slice(pcm);
    }
    fn forget(&mut self, _sound: SoundId) {}
    fn stream(&mut self, sound: SoundId, rate_hz: u32) {
        self.streams.push((sound, rate_hz));
    }
    fn stream_samples(&mut self, _sound: SoundId, pcm: &[i16]) {
        self.streamed.extend_from_slice(pcm);
    }
    fn start(&mut self, channel: u8, start: Start) {
        self.starts.push((channel, start.sound));
    }
    fn stop(&mut self, _channel: u8, _fade_ms: u16) {}
    fn gain(&mut self, _channel: u8, _gain: u8, _fade_ms: u16) {}
    fn concealed(&mut self, _sound: SoundId, block: u16, _samples: u32) {
        self.concealed.push(block);
    }
}

/// A register image that sounds: voice 1 a sawtooth with its gate set and a
/// held envelope, at full volume. The registers are the data sheet's: 0 and
/// 1 the frequency, 4 the control (bit 0 the gate, bit 5 the sawtooth), 5
/// attack and decay, 6 sustain and release, 24 the volume.
fn image() -> [u8; 25] {
    let mut image = [0_u8; 25];
    image[0] = 0x45;
    image[1] = 0x1D;
    image[4] = 0x21;
    image[5] = 0x00;
    image[6] = 0xF0;
    image[24] = 0x0F;
    image
}

fn synth6581_sound(handle: u8, blocks: usize) -> Sound {
    let data: Vec<u8> = (0..blocks).flat_map(|_| image()).collect();
    Sound::instrument(handle, 1, Codec::Synth6581, 25, &data).unwrap()
}

/// A play of `sound` on `channel`, for the whole service.
fn play(channel: u8, sequence: u16, sound: &Sound) -> Vec<u8> {
    Command {
        channel,
        sequence,
        operation: 1,
        handle: sound.handle,
        generation: sound.generation,
        gain: 100,
        fade_ms: 0,
        flags: 0,
        duck: 0,
        page: None,
    }
    .record()
}

fn push(receiver: &mut Receiver<'_, 8, 2>, sound: &Sound, sink: &mut Recorder) {
    receiver.push_record(&sound.object_record().unwrap(), sink);
    for record in sound.data_records() {
        receiver.push_record(&record, sink);
    }
}

fn loud(pcm: &[i16]) -> bool {
    pcm.iter().any(|sample| sample.unsigned_abs() > 500)
}

// Verifies REQ-6411 items 1, 2 and 6.
#[test]
fn a_stored_synth6581_sound_is_rendered_block_by_block_and_started() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    let sound = synth6581_sound(3, 3);
    push(&mut receiver, &sound, &mut sink);
    assert!(sink.defined.is_empty(), "receiving data never starts a sound");
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    let id = SoundId { handle: 3, generation: 1 };
    assert_eq!(
        sink.defined,
        vec![(id, SoundFormat { rate_hz: INSTRUMENT_RATE, samples: 3 * INSTRUMENT_BLOCK_SAMPLES })]
    );
    assert_eq!(sink.stored, vec![(id, 0, BLOCK), (id, 960, BLOCK), (id, 1920, BLOCK)]);
    assert_eq!(sink.starts, vec![(0, id)]);
    assert!(loud(&sink.pcm), "the chip sounds");
    assert_eq!(receiver.next_tick_ms(), None, "nothing is left to render");
}

// Verifies REQ-6411: a long sound is rendered a budget at a time.
#[test]
fn a_long_sound_is_rendered_over_several_ticks_and_asks_for_them_at_once() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    let sound = synth6581_sound(3, 120);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    assert_eq!(sink.stored.len(), 50, "one budget's blocks");
    assert_eq!(sink.starts.len(), 1, "the play starts without waiting for the rest");
    assert_eq!(receiver.next_tick_ms(), Some(0), "more waits: tick at once");
    receiver.tick(0, &mut sink);
    assert_eq!(sink.stored.len(), 100);
    receiver.tick(0, &mut sink);
    assert_eq!(sink.stored.len(), 120);
    assert_eq!(sink.stored.last().unwrap().1, 119 * INSTRUMENT_BLOCK_SAMPLES);
    assert_eq!(receiver.next_tick_ms(), None);
}

/// A synth7 block of one slot: `status`, `data1` and `data2` at the block's
/// start.
fn slot(status: u8, data1: u8, data2: u8) -> [u8; 4] {
    [0, status, data1, data2]
}

fn bank_sound(handle: u8) -> Sound {
    Sound::instrument(handle, 1, Codec::Synth7Bank, 128, &Patch::DEFAULT.to_packed()).unwrap()
}

// Verifies REQ-6411 items 3, 4 and 5.
#[test]
fn a_synth7_sound_waits_for_its_bank_and_then_plays_its_notes() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    // A note on, twenty blocks held, then its note off.
    let mut blocks: Vec<u8> = slot(0x90, 69, 100).to_vec();
    blocks.extend(std::iter::repeat_n([0_u8; 4], 20).flatten());
    blocks.extend(slot(0x80, 69, 0));
    let mut sound = Sound::instrument(4, 1, Codec::Synth7, 4, &blocks).unwrap();
    sound.bank = Some(9);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(1, 1, &sound), &mut sink);
    assert!(sink.starts.is_empty(), "its bank has not arrived");
    push(&mut receiver, &bank_sound(9), &mut sink);
    assert_eq!(sink.starts, vec![(1, SoundId { handle: 4, generation: 1 })]);
    assert_eq!(sink.stored.len(), 22);
    assert!(loud(&sink.pcm[..BLOCK * 4]), "the note sounds");
    assert_eq!(receiver.statistics().slots_passed_over, 0);
}

// Verifies REQ-6411 item 3: a slot that is not a message the codec takes is
// passed over and counted, and its neighbours still apply.
#[test]
fn slots_the_codec_does_not_take_are_passed_over_and_counted() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    let faulty: [[u8; 4]; 6] = [
        [200, 0x90, 60, 100], // a time past the block's end
        [0, 0xF0, 1, 2],      // a system message
        [0, 0x90, 0x80, 100], // a data byte with its top bit set
        [0, 0xB0, 2, 0],      // a controller the codec does not take
        [0, 0xC0, 32, 0],     // a program past a bank's end
        [0, 0x40, 0, 0],      // not a status byte
    ];
    let mut block: Vec<u8> = faulty.iter().flatten().copied().collect();
    block.extend(slot(0x90, 69, 100));
    block.extend([0_u8; 4]);
    let mut sound = Sound::instrument(4, 1, Codec::Synth7, 32, &block).unwrap();
    sound.bank = Some(9);
    push(&mut receiver, &bank_sound(9), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    assert_eq!(receiver.statistics().slots_passed_over, 6);
    assert!(loud(&sink.pcm), "the good slot beside them still plays its note");
}

// Verifies REQ-6411 item 4.
#[test]
fn a_bank_is_never_played() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    let bank = bank_sound(9);
    push(&mut receiver, &bank, &mut sink);
    receiver.push_record(&play(0, 1, &bank), &mut sink);
    assert!(sink.starts.is_empty() && sink.defined.is_empty() && sink.streams.is_empty());
}

// Verifies REQ-6411 items 1 and 2 for a live sound, and what a lost block
// costs.
#[test]
fn a_live_synth6581_sound_is_rendered_as_its_blocks_come_and_plays_through_a_lost_one() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    let sound = Sound::live(5, 1, Codec::Synth6581, INSTRUMENT_RATE, 25).unwrap();
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    for record in sound.packet_records(0, &image()).unwrap() {
        receiver.push_record(&record, &mut sink);
    }
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    assert_eq!(sink.streams, vec![(SoundId { handle: 5, generation: 1 }, INSTRUMENT_RATE)]);
    assert_eq!(sink.streamed.len(), BLOCK, "it joins at its newest block");
    // Block 1 is lost; blocks 2, 3 and 4 come.
    for index in 2..=4 {
        for record in sound.packet_records(index, &image()).unwrap() {
            receiver.push_record(&record, &mut sink);
        }
    }
    assert_eq!(sink.concealed, vec![1], "given up once three newer have come");
    assert_eq!(sink.streamed.len(), BLOCK * 5);
    assert!(loud(&sink.streamed[BLOCK..BLOCK * 2]), "the chip sounds on from the registers it holds");
}

// Verifies REQ-6411 items 2 to 4: what an OBJECT of each codec must say.
#[test]
fn an_object_names_its_chip_or_its_bank_and_a_faulty_one_is_refused() {
    let mut synth6581 = synth6581_sound(3, 1);
    synth6581.chip = Some(SYNTH6581_CHIP_8580 | SYNTH6581_CLOCK_NTSC);
    let record = synth6581.object_record().unwrap();
    let Ok(Record::Object(object)) = parse(&record) else { panic!("not an OBJECT") };
    assert_eq!((object.codec, object.chip, object.bank), (Some(Codec::Synth6581), Some(3), None));
    synth6581.chip = Some(4);
    assert!(parse(&synth6581.object_record().unwrap()).is_err(), "a reserved bit of the chip byte");

    let mut synth7 = Sound::instrument(4, 1, Codec::Synth7, 8, &[0; 8]).unwrap();
    assert!(parse(&synth7.object_record().unwrap()).is_err(), "a synth7 sound that names no bank");
    synth7.bank = Some(200);
    let record = synth7.object_record().unwrap();
    let Ok(Record::Object(object)) = parse(&record) else { panic!("not an OBJECT") };
    assert_eq!((object.codec, object.bank), (Some(Codec::Synth7), Some(200)));

    for (codec, block, allowed) in [
        (Codec::Synth6581, 25, true),
        (Codec::Synth6581, 50, true),
        (Codec::Synth6581, 100, true),
        (Codec::Synth6581, 75, false),
        (Codec::Synth7, 4, true),
        (Codec::Synth7, 64, true),
        (Codec::Synth7, 68, false),
        (Codec::Synth7, 6, false),
        (Codec::Synth7Bank, 128, true),
        (Codec::Synth7Bank, 155, false),
    ] {
        assert_eq!(codec.allows_block(block), allowed, "{codec:?} with blocks of {block}");
    }
    assert!(Sound::instrument(9, 1, Codec::Synth7Bank, 128, &[0; 128 * 33]).is_err(), "more than 32 patches");
}
