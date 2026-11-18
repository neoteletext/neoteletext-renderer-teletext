//! The synthesizers end to end under the event carriage
//! (`neoteletext-common/docs/audio/midi-architecture.md`, cited as
//! "MIDI-n"): sounds whose blocks are event lists and their banks, written
//! by the publishing encoder, read back by the receiver, played through each
//! family's front end and given to a recording sink as samples.
#![cfg(all(
    feature = "encode",
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
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

use neoteletext_audio::encode::{BlocksError, Command, EncodeError, Sound, TimedEvent, event_blocks, frame};
use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::{Record, TYPE_OBJECT, parse};
use neoteletext_audio::{Codec, INSTRUMENT_BLOCK_SAMPLES, INSTRUMENT_RATE, MOST_EVENT_BLOCK_BYTES};
use neoteletext_audio_instruments::midi::block::BlockFault;
use neoteletext_audio_instruments::midi::{ChannelMessage, Message, status};
use neoteletext_audio_instruments::{synth7, synth8, synth53, synth80, synth106, synthd};

/// The samples of a block, as a length.
const BLOCK: usize = INSTRUMENT_BLOCK_SAMPLES as usize;
/// The handle of the tests' first bank.
const BANK: u8 = 9;
/// The handle the tests' sounds have.
const SOUND: u8 = 4;

/// What the sink was given, assembled.
#[derive(Default)]
struct Recorder {
    defined: Vec<(SoundId, SoundFormat)>,
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
    fn samples(&mut self, _sound: SoundId, first: u32, pcm: &[i16]) {
        let first = first as usize;
        if let Some(place) = self.pcm.get_mut(first..first + pcm.len()) {
            place.copy_from_slice(pcm);
        }
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

/// One family with banks, as the tests need it: its patches in the form
/// its interim front end reads, and how a bank image is made of them.
struct Family {
    codec: Codec,
    plain: Vec<u8>,
    /// A patch that sounds unlike the default.
    dull: Vec<u8>,
    /// The bank image of `patches`.
    image: fn(&[&[u8]]) -> Vec<u8>,
    /// A bank image of a plain and a faulty patch, which the family's
    /// reader refuses.
    faulty: Vec<u8>,
}

/// The DX7's 32-voice bulk dump of `voices`, the rest the plain voice
/// (DX7 manual, page 30): `F0 43 00 09 20 00`, 4,096 bytes, a checksum,
/// `F7`.
fn dump(voices: &[&[u8]]) -> Vec<u8> {
    let plain = synth7::Patch::DEFAULT.to_packed();
    let mut data = Vec::new();
    for index in 0..synth7::patch::BANK_VOICES {
        data.extend_from_slice(voices.get(index).copied().unwrap_or(&plain));
    }
    let sum = data.iter().fold(0_u8, |sum, &byte| sum.wrapping_add(byte));
    let mut image = vec![0xF0, 0x43, 0x00, 0x09, 0x20, 0x00];
    image.extend_from_slice(&data);
    image.push(sum.wrapping_neg() & 0x7F);
    image.push(0xF7);
    image
}

fn families() -> Vec<Family> {
    let concat: fn(&[&[u8]]) -> Vec<u8> = |patches| patches.concat();
    let mut quiet_voice = synth7::Patch::DEFAULT.to_packed();
    // Operator 1's output level, the last operator's byte 14 of the packed
    // voice (TX7 manual, page 44): half way down.
    quiet_voice[5 * 17 + 14] = 50;
    assert!(synth7::Patch::from_packed(&quiet_voice).is_ok());
    let mut bad_dump = dump(&[]);
    let at = bad_dump.len() - 2;
    bad_dump[at] ^= 1;
    let mut synthd_patch = synthd::Patch::DEFAULT;
    synthd_patch.cutoff = -400;
    let mut tone = synth80::Tone::DEFAULT;
    tone.set(synth80::Control::Lpf, 20);
    let mut synth80_patch = synth80::Patch::DEFAULT;
    synth80_patch.tones[0] = tone;
    let faulty = |plain: &[u8]| [plain.to_vec(), vec![0xFF; plain.len()]].concat();
    let analogue =
        |codec, plain: Vec<u8>, dull: Vec<u8>| Family { codec, faulty: faulty(&plain), plain, dull, image: concat };
    vec![
        Family {
            codec: Codec::Synth7,
            plain: synth7::Patch::DEFAULT.to_packed().to_vec(),
            dull: quiet_voice.to_vec(),
            image: dump,
            faulty: bad_dump,
        },
        {
            // S106-38: a bank image of function type 30 messages, patch k
            // carrying k in its byte e.
            let packed = |patch: &synth106::Patch| {
                let mut bytes = [0; synth106::interface::MOST_PATCH_BYTES];
                let length = synth106::interface::pack_patch(synth106::Model::Synth106, patch, 0, 0, &mut bytes);
                bytes[..length].to_vec()
            };
            let plain = packed(&synth106::Patch::DEFAULT);
            Family {
                codec: Codec::Synth106,
                faulty: faulty(&plain),
                dull: packed(&synth106::Patch::DEFAULT.with(synth106::Slider::VcfFrequency, 30)),
                plain,
                image: |patches| {
                    let mut image = patches.concat();
                    for (program, patch) in image.chunks_mut(synth106::interface::PATCH_MESSAGE_BYTES).enumerate() {
                        patch[4] = program as u8;
                    }
                    image
                },
            }
        },
        analogue(Codec::SynthD, synthd::Patch::DEFAULT.to_bytes().to_vec(), synthd_patch.to_bytes().to_vec()),
        analogue(
            Codec::Synth8,
            synth8::Patch::DEFAULT.to_bytes().to_vec(),
            synth8::Patch::DEFAULT.with(synth8::Param::VcfCutoff, 40).to_bytes().to_vec(),
        ),
        analogue(Codec::Synth80, synth80::Patch::DEFAULT.to_bytes().to_vec(), synth80_patch.to_bytes().to_vec()),
        Family {
            codec: Codec::Synth53,
            plain: synth53::Patch::DEFAULT.to_bytes().to_vec(),
            dull: synth53::Patch::DEFAULT.with(synth53::Knob::Cutoff, 20).to_bytes().to_vec(),
            image: concat,
            // synth53's interim reader holds every byte to its range
            // itself: a faulty patch is one of another length, which a bank
            // of fixed patches cannot hold, so the faulty bank is the plain
            // one alone.
            faulty: synth53::Patch::DEFAULT.to_bytes().to_vec(),
        },
    ]
}

/// A block of `before`'s events at tick 0, then a note on of key 57.
fn note_block(before: &[u8]) -> Vec<u8> {
    [before, &[0, 0x90, 57, 100]].concat()
}

/// A stored sound of `codec` whose first block is `first` and the rest of
/// its `blocks` empty, naming `banks`.
fn sound(codec: Codec, first: Vec<u8>, blocks: usize, banks: &[u8]) -> Sound {
    let mut all = vec![first];
    all.resize(blocks, Vec::new());
    let mut sound = Sound::events(SOUND, 1, codec, &all).unwrap();
    sound.banks = banks.to_vec();
    sound
}

/// A stored sound of `blocks` as they are, unchecked: what a sender that is
/// not the server might send, which the encoder refuses (MIDI-21).
fn unchecked(codec: Codec, blocks: &[Vec<u8>], banks: &[u8]) -> Sound {
    let mut sound = sound(codec, Vec::new(), blocks.len(), banks);
    sound.data = blocks.iter().flat_map(|block| [&(block.len() as u16).to_be_bytes()[..], block].concat()).collect();
    sound.block_bytes = blocks.iter().map(Vec::len).max().unwrap_or(0) as u16;
    sound
}

fn bank(handle: u8, family: Codec, image: &[u8]) -> Sound {
    Sound::bank(handle, 1, family, image).unwrap()
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

/// The sum of the samples' magnitudes: how much sound there is.
fn energy(pcm: &[i16]) -> u64 {
    pcm.iter().map(|sample| u64::from(sample.unsigned_abs())).sum()
}

/// What the receiver counted of a sound's playing.
#[derive(Debug, PartialEq, Eq)]
struct Counted {
    passed_over: u32,
    malformed: u32,
    refused: u32,
}

/// What a stored `sound` renders to with `bank`, and what was counted.
fn rendered(bank: &Sound, sound: &Sound) -> (Vec<i16>, Counted) {
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, bank, &mut sink);
    push(&mut receiver, sound, &mut sink);
    receiver.push_record(&play(0, 1, sound), &mut sink);
    while receiver.next_tick_ms() == Some(0) {
        receiver.tick(0, &mut sink);
    }
    assert_eq!(sink.starts, vec![(0, SoundId { handle: SOUND, generation: 1 })]);
    let statistics = receiver.statistics();
    (
        sink.pcm,
        Counted {
            passed_over: statistics.events_passed_over,
            malformed: statistics.blocks_malformed,
            refused: statistics.patches_refused,
        },
    )
}

const NOTHING: Counted = Counted { passed_over: 0, malformed: 0, refused: 0 };

// Verifies MIDI-52, MIDI-57 and MIDI-62: a sound of each family that names
// three banks waits until the last of them has wholly arrived, and then
// plays its notes; a bank is never played (MIDI-55).
#[test]
fn a_sound_of_each_family_waits_for_its_banks_and_then_plays() {
    for family in families() {
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        let image = (family.image)(&[&family.plain]);
        let banks = [
            bank(BANK, family.codec, &image),
            bank(BANK + 1, family.codec, &image),
            bank(BANK + 2, family.codec, &image),
        ];
        let sound = sound(family.codec, note_block(&[]), 22, &[BANK, BANK + 1, BANK + 2]);
        push(&mut receiver, &banks[0], &mut sink);
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play(1, 1, &sound), &mut sink);
        push(&mut receiver, &banks[1], &mut sink);
        // All but the last piece of the third.
        receiver.push_record(&banks[2].object_record().unwrap(), &mut sink);
        let pieces = banks[2].data_records();
        for record in &pieces[..pieces.len() - 1] {
            receiver.push_record(record, &mut sink);
        }
        assert!(sink.starts.is_empty() && sink.defined.is_empty(), "{:?}: a bank has not arrived", family.codec);
        receiver.push_record(pieces.last().unwrap(), &mut sink);
        let id = SoundId { handle: SOUND, generation: 1 };
        assert_eq!(sink.starts, vec![(1, id)], "{:?}", family.codec);
        assert_eq!(
            sink.defined,
            vec![(id, SoundFormat { rate_hz: INSTRUMENT_RATE, samples: 22 * INSTRUMENT_BLOCK_SAMPLES })]
        );
        assert!(loud(&sink.pcm[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
        assert_eq!(receiver.statistics().events_passed_over, 0);
        // A bank is never played.
        receiver.push_record(&play(2, 1, &banks[0]), &mut sink);
        assert_eq!(sink.starts.len(), 1);
    }
}

// Verifies MIDI-48 and MIDI-57: a bank for another family is not a sound's
// bank, and the sound waits.
#[test]
fn a_bank_for_another_family_is_not_a_sounds_bank() {
    let families = families();
    for (family, other) in families.iter().zip(families.iter().cycle().skip(1)) {
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        let sound = sound(family.codec, note_block(&[]), 4, &[BANK]);
        push(&mut receiver, &bank(BANK, other.codec, &(other.image)(&[&other.plain])), &mut sink);
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play(0, 1, &sound), &mut sink);
        assert!(sink.starts.is_empty(), "{:?} with a bank for {:?}", family.codec, other.codec);
    }
}

// Verifies MIDI-59, MIDI-61 and MIDI-63: program 0 of bank index 0 plays
// until a program change chooses another; a faulty patch is replaced by the
// default and counted; a program the bank lacks plays the default and is
// counted as passed over.
#[test]
fn a_program_change_chooses_a_patch_and_a_faulty_one_is_the_default() {
    for family in families() {
        let two = bank(BANK, family.codec, &(family.image)(&[&family.plain, &family.dull]));
        let one = bank(BANK, family.codec, &(family.image)(&[&family.plain]));
        let dull = bank(BANK, family.codec, &(family.image)(&[&family.dull]));
        let notes = |before: &[u8]| sound(family.codec, note_block(before), 10, &[BANK]);
        let change = [0, 0xC0, 1];
        let (plain, counted) = rendered(&one, &notes(&[]));
        assert_eq!(counted, NOTHING, "{:?}", family.codec);
        let (first, _) = rendered(&two, &notes(&[]));
        let (second, counted) = rendered(&two, &notes(&change));
        assert_eq!(counted, NOTHING, "{:?}", family.codec);
        let (dull_alone, _) = rendered(&dull, &notes(&[]));
        assert!(loud(&plain), "{:?}", family.codec);
        assert_ne!(plain, dull_alone, "{:?}: the two patches sound unlike", family.codec);
        assert_eq!(first, plain, "{:?}: no program change plays program 0", family.codec);
        assert_eq!(second, dull_alone, "{:?}: program 1 plays patch 1", family.codec);
        if family.codec == Codec::Synth7 {
            // A dump whose checksum is wrong is refused whole at the start.
            let (refused, counted) = rendered(&bank(BANK, family.codec, &family.faulty), &notes(&[]));
            assert_eq!((refused, counted.refused), (plain, 1));
        } else {
            // Program 1 of a bank of one patch: the default, passed over.
            let (missing, counted) = rendered(&dull, &notes(&change));
            assert_eq!((missing, counted.passed_over), (plain.clone(), 1), "{:?}", family.codec);
            if family.codec != Codec::Synth53 {
                let (refused, counted) = rendered(&bank(BANK, family.codec, &family.faulty), &notes(&change));
                assert_eq!((refused, counted.refused), (plain, 1), "{:?}", family.codec);
            }
        }
    }
}

// Verifies MIDI-29, MIDI-33 and MIDI-34: events no family acts on are passed
// over and counted, the block's other events played as if they were not
// there; a framing error keeps what was applied and counts the block.
#[test]
fn what_a_block_holds_that_is_not_played_is_counted() {
    for family in families() {
        let image = bank(BANK, family.codec, &(family.image)(&[&family.plain]));
        let (clean, _) = rendered(&image, &sound(family.codec, note_block(&[]), 6, &[BANK]));
        // Timing clock, a meta event, a foreign system exclusive message and
        // local control.
        let skipped = [0, 0xF8, 0, 0xFF, 0x51, 3, 1, 2, 3, 0, 0xF0, 3, 0x7D, 0x01, 0xF7, 0, 0xB0, 122, 0];
        let mut blocks = vec![note_block(&skipped)];
        blocks.resize(6, Vec::new());
        let (pcm, counted) = rendered(&image, &unchecked(family.codec, &blocks, &[BANK]));
        assert_eq!(counted, Counted { passed_over: 4, ..NOTHING }, "{:?}", family.codec);
        assert_eq!(pcm, clean, "{:?}: the note plays as without them", family.codec);
        // Running status after the note: a framing error that keeps the note.
        blocks[0] = [note_block(&[]), vec![0, 58, 100, 0, 0x90, 59, 100]].concat();
        assert_eq!(
            Sound::events(SOUND, 1, family.codec, &blocks).unwrap_err(),
            EncodeError::Block { index: 0, fault: BlockFault::Malformed(1) }
        );
        let (pcm, counted) = rendered(&image, &unchecked(family.codec, &blocks, &[BANK]));
        assert_eq!(counted, Counted { malformed: 1, ..NOTHING }, "{:?}", family.codec);
        assert_eq!(pcm, clean, "{:?}: the note before the error plays, and nothing after it", family.codec);
    }
}

/// Plays a live sound of `family`: a note on in block 0 and empty blocks to
/// `last`, without the blocks in `missing`. Answers what the sink was given.
fn live(family: &Family, last: u16, missing: &[u16]) -> Recorder {
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
    let mut sound = Sound::live_events(SOUND, 1, family.codec).unwrap();
    sound.banks = vec![BANK];
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    for index in (0..=last).filter(|index| !missing.contains(index)) {
        let block = if index == 0 { note_block(&[]) } else { Vec::new() };
        for record in sound.live_event_records(index, &[block]).unwrap() {
            receiver.push_record(&record, &mut sink);
        }
        if index == 0 {
            receiver.push_record(&play(0, 1, &sound), &mut sink);
        }
    }
    sink
}

// Verifies MIDI-101 and MIDI-102 for each family: a live sound is played as
// its blocks come, and a block played through as lost releases every note.
#[test]
fn a_live_sound_of_each_family_plays_and_a_lost_block_releases_its_notes() {
    /// Three seconds of blocks: long enough for any default patch's release.
    const LAST: u16 = 150;
    for family in families() {
        let whole = live(&family, LAST, &[]);
        assert_eq!(whole.streams, vec![(SoundId { handle: SOUND, generation: 1 }, INSTRUMENT_RATE)]);
        assert_eq!(whole.concealed, [], "{:?}", family.codec);
        assert_eq!(whole.streamed.len(), BLOCK * (usize::from(LAST) + 1), "{:?}", family.codec);
        let tail = BLOCK * (usize::from(LAST) - 10);
        assert!(loud(&whole.streamed[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
        assert!(loud(&whole.streamed[tail..]), "{:?}: and is held to the end", family.codec);
        let gapped = live(&family, LAST, &[5]);
        assert_eq!(gapped.concealed, vec![5], "{:?}", family.codec);
        assert_eq!(gapped.streamed[..BLOCK * 5], whole.streamed[..BLOCK * 5], "the same until the gap");
        assert!(
            energy(&gapped.streamed[tail..]) * 4 < energy(&whole.streamed[tail..]),
            "{:?}: the note was released at the gap",
            family.codec
        );
    }
}

// Verifies MIDI-47 and MIDI-42: the encoder packs as many whole blocks into
// each record as fit, 503 empty blocks or one of 1,005 bytes, and the
// receiver stores them as they were.
#[test]
fn records_hold_as_many_whole_blocks_as_fit() {
    let empty = Sound::events(SOUND, 1, Codec::Synth7, &vec![Vec::<u8>::new(); 503]).unwrap();
    assert_eq!(empty.data_records().len(), 1);
    let more = Sound::events(SOUND, 1, Codec::Synth7, &vec![Vec::<u8>::new(); 504]).unwrap();
    assert_eq!(more.data_records().len(), 2);
    // 335 program changes of three bytes: 1,005 bytes, one record each.
    let full: Vec<u8> = (0..335).flat_map(|_| [0, 0xD0, 1]).collect();
    assert_eq!(full.len(), MOST_EVENT_BLOCK_BYTES);
    let big = Sound::events(SOUND, 1, Codec::Synth7, &[full.clone(), full.clone()]).unwrap_err();
    // 335 events are more than a block may hold.
    assert_eq!(big, EncodeError::Block { index: 0, fault: BlockFault::PassedOver(256) });
    let long: Vec<u8> = [vec![0, 0xF0, 0x82, 0x00], vec![0; 255], vec![0xF7]].concat();
    let three = [long.clone(), long.clone(), long.clone(), long];
    let sound = Sound::events(SOUND, 1, Codec::Synth7, &[three.concat()[..1_000].to_vec()]);
    assert!(sound.is_err(), "a block that cuts a message is refused");
    // Two blocks of 780 bytes do not share a record; the empty and the
    // short block after the second do.
    let blocks = vec![three[..3].concat(), three[..3].concat(), Vec::new(), vec![0, 0xD0, 1]];
    let mut sound = Sound::events(SOUND, 1, Codec::Synth7, &blocks).unwrap();
    sound.banks = vec![BANK];
    assert_eq!(sound.data_records().len(), 2);
    let object_record = sound.object_record().unwrap();
    let Ok(Record::Object(object)) = parse(&object_record) else { panic!("an object") };
    assert_eq!((object.block_bytes, object.total, object.banks()), (780, Some(1_563), &[BANK][..]));
}

// Verifies MIDI-16, MIDI-17 and MIDI-23: timed events go into the block and
// tick their absolute tick gives, in order.
#[test]
fn timed_events_are_written_into_their_blocks() {
    let note = |key| Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, key, 100));
    let tick = neoteletext_audio_instruments::midi::tick_of_microseconds;
    let events = [
        TimedEvent { tick: tick(0), message: note(60) },
        TimedEvent { tick: tick(19_949), message: note(61) },
        TimedEvent { tick: tick(19_950), message: note(62) },
        TimedEvent { tick: tick(60_000), message: note(63) },
    ];
    let blocks = event_blocks(&events, 4).unwrap();
    assert_eq!(
        blocks,
        vec![vec![0, 0x90, 60, 100, 0x81, 0x47, 0x90, 61, 100], vec![0, 0x90, 62, 100], vec![], vec![0, 0x90, 63, 100]]
    );
    assert_eq!(event_blocks(&events, 3), Err(BlocksError::PastEnd(3)));
    let backwards = [events[1], events[0]];
    assert_eq!(event_blocks(&backwards, 4), Err(BlocksError::Order(1)));
    let local =
        [TimedEvent { tick: 0, message: Message::Channel(ChannelMessage::new(status::CONTROL_CHANGE, 0, 122, 0)) }];
    assert_eq!(event_blocks(&local, 1), Err(BlocksError::Message(0)));
}

// Verifies MIDI-108: an OBJECT of a retired code, 3 to 11, is ignored and
// counted as unsupported.
#[test]
fn the_retired_codes_are_ignored_and_counted() {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    for code in 3..=11_u8 {
        let mut body = vec![SOUND, 0, 1, 0, 0, 0, 4, code, 7, 0, 4];
        body.extend_from_slice(&INSTRUMENT_BLOCK_SAMPLES.to_be_bytes());
        body.extend_from_slice(&1_u16.to_be_bytes());
        receiver.push_record(&frame(TYPE_OBJECT, &body), &mut sink);
    }
    assert_eq!(receiver.statistics().unsupported, 9);
    assert_eq!(receiver.store().entries().count(), 0);
}

// Verifies MIDI-3 for the synth6581 family under the new carriage: its
// sound is stored and played, the chip silent until its interface is
// built, and every message passed over.
#[test]
fn a_synth6581_sound_is_stored_and_played() {
    let blocks = vec![vec![0, 0xF0, 3, 0x2D, 0x4C, 0xF7]; 3];
    let sound = Sound::events(SOUND, 1, Codec::Synth6581, &blocks).unwrap();
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    while receiver.next_tick_ms() == Some(0) {
        receiver.tick(0, &mut sink);
    }
    assert_eq!(sink.starts.len(), 1);
    assert_eq!(sink.pcm.len(), 3 * BLOCK);
    assert_eq!(receiver.statistics().events_passed_over, 3);
}

/// A small generator of random numbers, the same on every run: xorshift64
/// (Marsaglia, "Xorshift RNGs", 2003, the triple 13, 7, 17).
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| (self.next() >> 32) as u8).collect()
    }
}

// Verifies MIDI-123: random DATA records of stored and live event sounds,
// and random bank images, are played without a panic.
#[test]
fn random_records_and_random_banks_never_panic() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for family in families() {
        for round in 0..4 {
            let mut arena = vec![0_u8; 128 * 1024];
            let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
            let mut sink = Recorder::default();
            let length = 1 + (random.next() % 6_000) as usize;
            push(&mut receiver, &bank(BANK, family.codec, &random.bytes(length)), &mut sink);
            let stored = sound(family.codec, note_block(&[]), 8, &[BANK]);
            receiver.push_record(&stored.object_record().unwrap(), &mut sink);
            receiver.push_record(&play(0, 1, &stored), &mut sink);
            for _ in 0..8 {
                let length = (random.next() % 1_000) as usize;
                let mut body = vec![SOUND, 0, 1, 0, (random.next() % 8) as u8];
                body.extend_from_slice(&random.bytes(length));
                receiver.push_record(&frame(2, &body), &mut sink);
            }
            let mut live = Sound::live_events(SOUND + 1, 1, family.codec).unwrap();
            live.banks = vec![BANK];
            receiver.push_record(&live.object_record().unwrap(), &mut sink);
            receiver.push_record(&play(1, 1, &live), &mut sink);
            for index in 0..40_u16 {
                let mut body = vec![SOUND + 1, 0, 1];
                body.extend_from_slice(&index.to_be_bytes());
                body.extend_from_slice(&[0, 0, 0, 0]);
                let length = (random.next() % 900) as usize;
                body.extend_from_slice(&u16::try_from(length).unwrap().to_be_bytes());
                body.extend_from_slice(&random.bytes(length));
                receiver.push_record(&frame(2, &body), &mut sink);
                receiver.tick(u64::from(index) * 20, &mut sink);
            }
            assert!(round < 4);
        }
    }
}
