//! The synthesizers end to end under the event carriage
//! (`neoteletext-common/docs/audio/midi-architecture.md`, cited as
//! "MIDI-n"): sounds whose blocks are event lists and their banks, written
//! by the publishing encoder, read back by the receiver, played through each
//! family's front end and given to a recording sink as streams, one for
//! each playing, named by its audio channel and sound
//! (`neoteletext-common/docs/audio/integration.md`, INT-33, INT-37).
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

use neoteletext_audio::encode::{
    BlocksError, Command, EncodeError, Sound, TimedCue, TimedEvent, event_blocks, event_blocks_with_cues, frame,
};
use neoteletext_audio::port::{LocalStart, SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::{
    Control, LOCAL_SOUND, Operation, Record, RecordError, SYNTH8_KEYS_SPLIT, TAG_LOCAL, TYPE_OBJECT, parse,
};
use neoteletext_audio::{
    Codec, INSTRUMENT_BLOCK_SAMPLES, INSTRUMENT_LEAD_MAX_BLOCKS, INSTRUMENT_LEAD_MIN_BLOCKS, INSTRUMENT_RATE,
    MOST_EVENT_BLOCK_BYTES,
};
use neoteletext_audio_instruments::midi::block::BlockFault;
use neoteletext_audio_instruments::midi::{ChannelMessage, Message, status};
use neoteletext_audio_instruments::{synth7, synth8, synth53, synth80, synth106, synth6581, synthd};

/// The samples of a block, as a length.
const BLOCK: usize = INSTRUMENT_BLOCK_SAMPLES as usize;
/// The handle of the tests' first bank.
const BANK: u8 = 9;
/// The handle the tests' sounds have.
const SOUND: u8 = 4;

/// One stream the sink was given (INT-37): its audio channel, sound and
/// rate, its samples in order, and whether it has ended.
#[derive(Debug)]
struct Streamed {
    channel: u8,
    sound: SoundId,
    rate_hz: u32,
    pcm: Vec<i16>,
    ended: bool,
}

/// What the sink was given, assembled.
#[derive(Default)]
struct Recorder {
    defined: Vec<(SoundId, SoundFormat)>,
    samples: usize,
    streams: Vec<Streamed>,
    starts: Vec<(u8, SoundId)>,
    concealed: Vec<u16>,
    /// Whether the sink plays local sounds (RULE-60).
    local: bool,
    /// The banks of local sounds given: channel, sound, bank index, image.
    local_banks: Vec<(u8, SoundId, u8, Vec<u8>)>,
    /// The local starts given: channel, the start with no setup, the setup.
    locals: Vec<(u8, LocalStart<'static>, Vec<u8>)>,
    /// The channels stopped, in order.
    stops: Vec<u8>,
    /// The gains set: channel and gain.
    gains: Vec<(u8, u8)>,
    /// The cue marks given (RULE-109): channel, sound, position, number,
    /// and the samples the stream held when the mark came.
    cues: Vec<(u8, SoundId, u64, u8, usize)>,
}

impl Recorder {
    /// The stream of `sound` on `channel` that has not ended.
    fn open(&mut self, channel: u8, sound: SoundId) -> Option<&mut Streamed> {
        self.streams.iter_mut().rev().find(|stream| stream.channel == channel && stream.sound == sound && !stream.ended)
    }

    /// The streams begun, as (channel, sound, rate).
    fn names(&self) -> Vec<(u8, SoundId, u32)> {
        self.streams.iter().map(|stream| (stream.channel, stream.sound, stream.rate_hz)).collect()
    }
}

impl VoiceSink for Recorder {
    fn define(&mut self, sound: SoundId, format: SoundFormat) {
        self.defined.push((sound, format));
    }
    fn samples(&mut self, _sound: SoundId, _first: u32, _pcm: &[i16]) {
        self.samples += 1;
    }
    fn forget(&mut self, sound: SoundId) {
        for stream in self.streams.iter_mut().filter(|stream| stream.sound == sound) {
            stream.ended = true;
        }
    }
    fn stream(&mut self, channel: u8, sound: SoundId, rate_hz: u32) {
        if let Some(old) = self.open(channel, sound) {
            old.ended = true;
        }
        self.streams.push(Streamed { channel, sound, rate_hz, pcm: Vec::new(), ended: false });
    }
    fn stream_samples(&mut self, channel: u8, sound: SoundId, pcm: &[i16]) {
        self.open(channel, sound)
            .expect("samples come only to a stream begun and not ended")
            .pcm
            .extend_from_slice(pcm);
    }
    fn stream_cue(&mut self, channel: u8, sound: SoundId, position: u64, number: u8) {
        let held = self.open(channel, sound).expect("a mark comes only to a stream begun and not ended").pcm.len();
        self.cues.push((channel, sound, position, number, held));
    }
    fn end_stream(&mut self, channel: u8, sound: SoundId) {
        self.open(channel, sound).expect("only a stream begun ends").ended = true;
    }
    fn start(&mut self, channel: u8, start: Start) {
        self.starts.push((channel, start.sound));
    }
    fn stop(&mut self, channel: u8, _fade_ms: u16) {
        self.stops.push(channel);
    }
    fn gain(&mut self, channel: u8, gain: u8, _fade_ms: u16) {
        self.gains.push((channel, gain));
    }
    fn concealed(&mut self, _sound: SoundId, block: u16, _samples: u32) {
        self.concealed.push(block);
    }
    fn plays_local(&self) -> bool {
        self.local
    }
    fn local_bank(&mut self, channel: u8, sound: SoundId, index: u8, image: &[u8]) {
        self.local_banks.push((channel, sound, index, image.to_vec()));
    }
    fn local(&mut self, channel: u8, start: LocalStart<'_>) {
        self.locals.push((channel, LocalStart { setup: &[], ..start }, start.setup.to_vec()));
    }
}

/// Ticks the receiver whenever it asks to be (INT-34), from `from_ms` until
/// it asks for nothing or for a moment past `until_ms`, as a port does.
/// Answers the receiver's time at the end.
fn run(receiver: &mut Receiver<'_, 8, 2>, sink: &mut Recorder, from_ms: u64, until_ms: u64) -> u64 {
    let mut now = from_ms;
    for _ in 0..1_000_000 {
        match receiver.next_tick_ms() {
            Some(at) if at <= until_ms => {
                now = now.max(at);
                receiver.tick(now, sink);
            }
            _ => return now,
        }
    }
    panic!("the receiver asked to be ticked without end");
}

/// The milliseconds of a block.
const BLOCK_MS: u64 = 20;
/// Longer than any test's sound.
const FOREVER_MS: u64 = 10 * 60 * 1_000;

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

/// synth53's bank image of 128-byte programs: program k as the program
/// dump of group 0 and k (S53-26).
fn synth53_bank(programs: &[&[u8]]) -> Vec<u8> {
    programs
        .iter()
        .zip(0..)
        .flat_map(|(bytes, number)| {
            let program = synth53::interface::Program::from_bytes((*bytes).try_into().unwrap());
            synth53::interface::program_dump(0, number, &program).unwrap()
        })
        .collect()
}

fn families() -> Vec<Family> {
    let synth53_plain = synth53::interface::Program::default_for(synth53::interface::Variant::Synth53);
    let mut synth53_dull = synth53_plain;
    synth53_dull.set(synth53::interface::parameter::CUTOFF, 20);
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
            plain: synth53_plain.bytes().to_vec(),
            dull: synth53_dull.bytes().to_vec(),
            image: synth53_bank,
            // A program dump one byte long at program 1 (S53-28).
            faulty: [
                synth53_bank(&[synth53_plain.bytes()]),
                synth53::interface::program_dump(0, 1, &synth53_plain).unwrap()[..158].to_vec(),
                vec![0, 0xF7],
            ]
            .concat(),
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
    run(&mut receiver, &mut sink, 0, FOREVER_MS);
    let id = SoundId { handle: SOUND, generation: 1 };
    assert_eq!(sink.starts, vec![(0, id)]);
    assert_eq!(sink.names(), vec![(0, id, INSTRUMENT_RATE)]);
    assert!(sink.streams[0].ended, "an unlooped sound's stream ends after its last block (INT-36)");
    assert!(sink.defined.is_empty() && sink.samples == 0, "a synthesizer's sound is never defined (INT-33)");
    let statistics = receiver.statistics();
    (
        std::mem::take(&mut sink.streams[0].pcm),
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
        assert_eq!(sink.names(), vec![(1, id, INSTRUMENT_RATE)]);
        assert_eq!(sink.defined, []);
        run(&mut receiver, &mut sink, 0, 10 * BLOCK_MS);
        assert!(loud(&sink.streams[0].pcm[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
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
            let (refused, counted) = rendered(&bank(BANK, family.codec, &family.faulty), &notes(&change));
            assert_eq!((refused, counted.refused), (plain, 1), "{:?}", family.codec);
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
        assert_eq!(whole.names(), vec![(0, SoundId { handle: SOUND, generation: 1 }, INSTRUMENT_RATE)]);
        assert_eq!(whole.concealed, [], "{:?}", family.codec);
        assert_eq!(whole.streams[0].pcm.len(), BLOCK * (usize::from(LAST) + 1), "{:?}", family.codec);
        let tail = BLOCK * (usize::from(LAST) - 10);
        assert!(loud(&whole.streams[0].pcm[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
        assert!(loud(&whole.streams[0].pcm[tail..]), "{:?}: and is held to the end", family.codec);
        let gapped = live(&family, LAST, &[5]);
        assert_eq!(gapped.concealed, vec![5], "{:?}", family.codec);
        assert_eq!(gapped.streams[0].pcm[..BLOCK * 5], whole.streams[0].pcm[..BLOCK * 5], "the same until the gap");
        assert!(
            energy(&gapped.streams[0].pcm[tail..]) * 4 < energy(&whole.streams[0].pcm[tail..]),
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

/// A synth6581 family block: a full register update at tick 0 (S6581-45)
/// sounding voice 1's sawtooth at full volume past the filter, then a start
/// message, which has no effect (S6581-18).
fn synth6581_block() -> Vec<u8> {
    let mut values = [0_u8; synth6581::interface::FULL_UPDATE_IDS];
    values[0] = 0x45; // voice 1 frequency, near 440 Hz on the PAL clock
    values[1] = 0x1D;
    values[5] = 0xF0; // sustain at the peak
    values[21] = 0x0F; // full volume
    values[22] = 0x21; // sawtooth, gated
    let packed = synth6581::interface::pack_patch(&synth6581::interface::Patch { values });
    let mut block = vec![0, status::SYSTEM_EXCLUSIVE, (packed.len() - 1) as u8];
    block.extend_from_slice(&packed[1..]);
    block.extend_from_slice(&[0, 0xF0, 3, 0x2D, 0x4C, 0xF7]);
    block
}

/// Plays `blocks` as a stored sound of the synth6581 family's codec with
/// the chip and clock byte `chip` (tag 3, S6581-39), giving the samples
/// and the receiver's count of events passed over.
fn play_synth6581(blocks: &[Vec<u8>], chip: u8) -> (Vec<i16>, u32) {
    let mut sound = Sound::events(SOUND, 1, Codec::Synth6581, blocks).unwrap();
    sound.chip = Some(chip);
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    run(&mut receiver, &mut sink, 0, FOREVER_MS);
    assert_eq!(sink.starts.len(), 1);
    (std::mem::take(&mut sink.streams[0].pcm), receiver.statistics().events_passed_over)
}

// Verifies MIDI-3 and S6581-1, S6581-9 and S6581-39 end to end: a sound of
// the synth6581 family's codec is stored and played through ASID, its
// register data written and its start messages taken without a count; the
// chip bit of tag 3 chooses synth8580 and the clock bit the NTSC clock.
#[test]
fn a_synth6581_sound_is_stored_and_played() {
    let blocks = vec![synth6581_block(), Vec::new(), Vec::new()];
    let (pal, passed_over) = play_synth6581(&blocks, 0);
    assert_eq!(pal.len(), 3 * BLOCK);
    assert_eq!(passed_over, 0);
    assert!(pal.iter().any(|&sample| sample != pal[0]));
    let (synth8580, _) = play_synth6581(&blocks, neoteletext_audio::record::SYNTH6581_CHIP_8580);
    let (ntsc, _) = play_synth6581(&blocks, neoteletext_audio::record::SYNTH6581_CLOCK_NTSC);
    assert_ne!(synth8580, pal);
    assert_ne!(ntsc, pal);
    // A channel message is passed over and counted (S6581-26).
    let note = Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, 60, 100));
    let mut noted = blocks.clone();
    let mut buffer = [0_u8; MOST_EVENT_BLOCK_BYTES];
    let mut writer = neoteletext_audio_instruments::midi::block::BlockWriter::new(&mut buffer);
    writer.push(0, &note).unwrap();
    noted[1] = writer.bytes().to_vec();
    let (with_note, passed_over) = play_synth6581(&noted, 0);
    assert_eq!(passed_over, 1);
    assert_eq!(with_note, pal);
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

/// A stored synth7 sound of `blocks` blocks, a note on in block 0 and
/// another key in every eighth block after it, so that each block's samples
/// depend on where the playing started.
fn synth7_tune(blocks: usize) -> Sound {
    let tune: Vec<Vec<u8>> = (0..blocks)
        .map(|index| if index % 8 == 0 { vec![0, 0x90, 57 + (index / 8 % 12) as u8, 100] } else { Vec::new() })
        .collect();
    let mut sound = Sound::events(SOUND, 1, Codec::Synth7, &tune).unwrap();
    sound.banks = vec![BANK];
    sound
}

/// The bank the tune plays: 32 plain voices.
fn synth7_bank() -> Sound {
    bank(BANK, Codec::Synth7, &dump(&[]))
}

/// What `sound`'s blocks render to played in order into a new synth7
/// instance with its bank: the samples a playing of it must stream.
fn synth7_alone(sound: &Sound) -> Vec<i16> {
    use neoteletext_audio_instruments::midi::{Banks, Instrument, play_block};
    let mut synth = synth7::midi::Midi::new();
    let image = dump(&[]);
    let mut banks = Banks::none();
    banks.push(&image);
    synth.start(&banks);
    let mut out = Vec::new();
    let mut at = 0;
    while at < sound.data.len() {
        let length = usize::from(u16::from_be_bytes([sound.data[at], sound.data[at + 1]]));
        let block = &sound.data[at + 2..at + 2 + length];
        let mut pcm = [0_i16; BLOCK];
        play_block(&mut synth, block, &banks, &mut pcm);
        out.extend_from_slice(&pcm);
        at += 2 + length;
    }
    out
}

// Verifies INT-33 and INT-34: a stored sound is streamed in order, its
// samples those of its blocks rendered one after another, never defined;
// with the clock stepped a millisecond at a time and the receiver ticked
// only when it asks, the stream stays between the least and the largest
// lead ahead of what has been heard, and it ends after the last block.
#[test]
fn a_stored_sound_streams_in_order_a_lead_ahead() {
    const BLOCKS: usize = 150;
    let sound = synth7_tune(BLOCKS);
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &synth7_bank(), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    let mut now = 0;
    while !sink.streams[0].ended {
        now += 1;
        assert!(now < FOREVER_MS);
        if receiver.next_tick_ms().is_some_and(|at| at <= now) {
            receiver.tick(now, &mut sink);
        }
        let rendered = (sink.streams[0].pcm.len() / BLOCK) as u64;
        let heard = now / BLOCK_MS;
        if rendered < BLOCKS as u64 {
            assert!(
                rendered >= heard + u64::from(INSTRUMENT_LEAD_MIN_BLOCKS)
                    && rendered <= heard + u64::from(INSTRUMENT_LEAD_MAX_BLOCKS) + 1,
                "at {now} ms {rendered} blocks rendered, {heard} heard"
            );
        }
    }
    assert_eq!(sink.streams.len(), 1);
    assert!(sink.defined.is_empty() && sink.samples == 0);
    assert_eq!(sink.streams[0].pcm, synth7_alone(&sound));
    assert!(loud(&sink.streams[0].pcm));
    // It ends once its last block is rendered, the lead before it is heard.
    let last_heard_ms = BLOCKS as u64 * BLOCK_MS;
    assert!(now <= last_heard_ms && now + u64::from(INSTRUMENT_LEAD_MAX_BLOCKS) * BLOCK_MS >= last_heard_ms, "{now}");
}

// Verifies INT-37 and MIDI-3: one stored sound played on audio channels 0
// and 1, the second 300 ms after the first, is two streams, each the
// sound's samples from its own start, from its own instance.
#[test]
fn one_sound_on_two_channels_is_two_independent_streams() {
    let sound = synth7_tune(60);
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &synth7_bank(), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(0, 1, &sound), &mut sink);
    let now = run(&mut receiver, &mut sink, 0, 300);
    receiver.tick(now.max(300), &mut sink);
    receiver.push_record(&play(1, 1, &sound), &mut sink);
    run(&mut receiver, &mut sink, 300, FOREVER_MS);
    let id = SoundId { handle: SOUND, generation: 1 };
    assert_eq!(sink.starts, vec![(0, id), (1, id)]);
    assert_eq!(sink.names(), vec![(0, id, INSTRUMENT_RATE), (1, id, INSTRUMENT_RATE)]);
    let alone = synth7_alone(&sound);
    for stream in &sink.streams {
        assert!(stream.ended, "channel {}", stream.channel);
        assert_eq!(stream.pcm, alone, "channel {} plays the sound from its own start", stream.channel);
    }
}

// Verifies MIDI-105 and INT-36: a looped stored sound goes on from its last
// block to block 0 in the same stream, without a new stream, until stopped.
#[test]
fn a_looped_sound_plays_on_in_one_stream() {
    const BLOCKS: usize = 10;
    let sound = synth7_tune(BLOCKS);
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &synth7_bank(), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    let mut looped = play(0, 1, &sound);
    let Ok(Record::Control(control)) = parse(&looped) else { panic!("a command") };
    looped = Command {
        channel: 0,
        sequence: 1,
        operation: 1,
        handle: SOUND,
        generation: 1,
        gain: 100,
        fade_ms: 0,
        flags: neoteletext_audio::record::FLAG_LOOP,
        duck: 0,
        page: control.page.filter(|_| false),
    }
    .record();
    receiver.push_record(&looped, &mut sink);
    run(&mut receiver, &mut sink, 0, 3 * BLOCKS as u64 * BLOCK_MS);
    assert_eq!(sink.streams.len(), 1);
    assert!(!sink.streams[0].ended);
    assert!(sink.streams[0].pcm.len() >= 3 * BLOCKS * BLOCK, "{}", sink.streams[0].pcm.len() / BLOCK);
    let alone = synth7_alone(&sound);
    assert_eq!(sink.streams[0].pcm[..BLOCKS * BLOCK], alone[..], "the first pass is the sound");
    assert!(loud(&sink.streams[0].pcm[BLOCKS * BLOCK..2 * BLOCKS * BLOCK]), "the second pass sounds");
    // A stop ends the stream at the end of its fade (INT-36).
    let stop = Command {
        channel: 0,
        sequence: 2,
        operation: 2,
        handle: SOUND,
        generation: 1,
        gain: 100,
        fade_ms: 100,
        flags: 0,
        duck: 0,
        page: None,
    }
    .record();
    let at = 3 * BLOCKS as u64 * BLOCK_MS;
    receiver.tick(at, &mut sink);
    receiver.push_record(&stop, &mut sink);
    assert!(!sink.streams[0].ended, "it plays on through the fade");
    let end = run(&mut receiver, &mut sink, at, FOREVER_MS);
    assert!(sink.streams[0].ended);
    assert_eq!(end, at + 100);
}

/// The page the tests' local sounds belong to.
const PAGE: u16 = 0x101;
/// A local sound's setup: a program change to program 1 on channel 1, then
/// controller 7 at 100, both at tick 0.
const SETUP: [u8; 7] = [0x00, 0xC0, 0x01, 0x00, 0xB0, 0x07, 100];

/// A local sound of `codec` with handle `handle`, naming the bank [`BANK`].
fn local(handle: u8, codec: Codec) -> Sound {
    let mut sound = Sound::local(handle, 1, codec, &SETUP).unwrap();
    sound.banks = vec![BANK];
    sound
}

/// A play of `sound` on `channel` that belongs to `page`.
fn play_on(channel: u8, sequence: u16, sound: &Sound, page: Option<u16>) -> Vec<u8> {
    Command {
        channel,
        sequence,
        operation: 1,
        handle: sound.handle,
        generation: sound.generation,
        gain: 80,
        fade_ms: 5,
        flags: 0,
        duck: 0,
        page,
    }
    .record()
}

// Verifies RULE-60 (REQ-6527): the OBJECT of the local sound RULE-61's
// example announces is these bytes (the wire vector RULE-85 names), and
// reads back as a stored sound of one block marked local.
#[test]
fn a_local_sounds_object_is_this_wire_vector() {
    // RULE-61: {"kind":"synth106_local","handle":1,"generation":1,"banks":[2],
    // "setup":[{"event":"program_change","channel":1,"program":0,"time_us":0}]}.
    let setup = [0x00, 0xC0, 0x00];
    let mut sound = Sound::local(1, 1, Codec::Synth106, &setup).unwrap();
    sound.banks = vec![2];
    let object = sound.object_record().unwrap();
    let expected = [
        0x02, 0x01, 0x00, 0x1D, // profile version 2, OBJECT, a body of 29 bytes
        0x01, 0x00, 0x01, // handle 1, generation 1
        0x00, 0x00, 0x00, 0x01, // id 1
        0x0E, 0x07, // codec 14, the synth106 family; rate code 7, 48,000 Hz
        0x00, 0x03, // block bytes: the setup's 3
        0x00, 0x00, 0x03, 0xC0, // samples: one block of 960
        0x00, 0x01, // blocks: 1
        0x04, 0x01, 0x02, // TAG_BANKS: bank handle 2
        0x08, 0x04, 0x00, 0x00, 0x00, 0x03, // TAG_TOTAL_BYTES: 3
        0x09, 0x01, 0x01, // TAG_LOCAL: 1
        0x0E, 0x74, 0xEC, 0x4D, // CRC-32 of the header and the body
    ];
    assert_eq!(object, expected, "{object:02X?}");
    // RULE-85: and they are the row `object local_sound` of the wire
    // vectors, which the server's and the browser's tests read too.
    let row = include_str!("../../neoteletext-web/src/wire_vectors.tsv")
        .lines()
        .find_map(|line| line.strip_prefix("object\tlocal_sound\t"))
        .and_then(|rest| rest.split('\t').next())
        .expect("the table has the row");
    let held: Vec<u8> =
        (0..row.len()).step_by(2).map(|at| u8::from_str_radix(&row[at..at + 2], 16).expect("hexadecimal")).collect();
    assert_eq!(object, held);
    assert_eq!((TAG_LOCAL, LOCAL_SOUND), (9, 1));
    let Record::Object(read) = parse(&object).unwrap() else { panic!("an OBJECT") };
    assert!(read.local);
    assert_eq!((read.blocks, read.samples, read.block_bytes, read.total), (1, 960, 3, Some(3)));
    assert_eq!((read.codec, read.banks()), (Some(Codec::Synth106), &[2][..]));
    // Its one DATA record carries the setup: offset 0, one block of 3 bytes.
    let data = sound.data_records();
    assert_eq!(data.len(), 1);
    let Record::Data(read) = parse(&data[0]).unwrap() else { panic!("a DATA") };
    assert_eq!(read.blocks, [0, 0, 0, 0, 0, 3, 0x00, 0xC0, 0x00]);
}

// Verifies RULE-60 (REQ-6527): what the tag refuses and where it means
// nothing.
#[test]
fn the_local_tag_is_one_byte_of_one_on_a_stored_sound_of_one_block() {
    let object = |codec: Codec, tags: &[u8], blocks: u16| {
        let total = 3 * u32::from(blocks);
        let mut body = vec![4, 0, 1, 0, 0, 0, 4, codec.code(), 7, 0, 3];
        body.extend_from_slice(&(960 * u32::from(blocks)).to_be_bytes());
        body.extend_from_slice(&blocks.to_be_bytes());
        if blocks != 0 {
            body.extend_from_slice(&[8, 4]);
            body.extend_from_slice(&total.to_be_bytes());
        }
        body.extend_from_slice(tags);
        match parse(&frame(TYPE_OBJECT, &body)) {
            Ok(Record::Object(object)) => Ok(object.local),
            Ok(_) => panic!("an OBJECT"),
            Err(error) => Err(error),
        }
    };
    let bad = Err(RecordError::BadValue("local"));
    for codec in [Codec::Synth7, Codec::Synth106, Codec::SynthD, Codec::Synth53, Codec::Synth8, Codec::Synth80] {
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 1, 1], 1), Ok(true), "{codec:?}");
        assert_eq!(object(codec, &[4, 1, 9], 1), Ok(false));
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 1, 0], 1), bad, "a value other than 1");
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 1, 2], 1), bad);
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 2, 1, 1], 1), bad, "two bytes");
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 0], 1), bad, "no byte");
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 1, 1], 2), bad, "two blocks");
        assert_eq!(object(codec, &[4, 1, 9, TAG_LOCAL, 1, 1], 0), bad, "a live sound");
    }
    // The synth6581 family has no notes and so no local sound: the tag is
    // skipped there, as any unknown tag is.
    assert_eq!(object(Codec::Synth6581, &[TAG_LOCAL, 1, 1], 1), Ok(false));
    assert_eq!(object(Codec::Synth6581, &[TAG_LOCAL, 1, 7], 1), Ok(false));
    // The encoder writes none for it, nor a setup with an event after tick 0.
    assert_eq!(Sound::local(1, 1, Codec::Synth6581, &[]).unwrap_err(), EncodeError::Encoder);
    assert_eq!(Sound::local(1, 1, Codec::Opus, &[]).unwrap_err(), EncodeError::Encoder);
    assert_eq!(Sound::local(1, 1, Codec::Synth7, &[0x01, 0x90, 60, 100]).unwrap_err(), EncodeError::Setup);
    assert_eq!(
        Sound::local(1, 1, Codec::Synth7, &[0x00, 0x90, 60, 100, 0x00, 0x80, 60, 0]).map(|sound| sound.local),
        Ok(true)
    );
    assert!(matches!(Sound::local(1, 1, Codec::Synth7, &[0x00, 0x90, 60]), Err(EncodeError::Block { index: 0, .. })));
}

// Verifies RULE-60 (REQ-6527): a play of a local sound of each family gives
// the sink its banks and its local start, with the family, the variant, the
// option and the setup, in place of a stream; stop and gain act on its
// channel as on any sound; and it ends when its page is no longer open.
#[test]
fn a_local_sound_of_each_family_is_handed_to_the_sink_and_never_streamed() {
    for family in families() {
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder { local: true, ..Recorder::default() };
        let image = (family.image)(&[&family.plain, &family.dull]);
        let mut sound = local(SOUND, family.codec);
        sound.banks = vec![BANK, BANK + 1];
        let (variant, option, split) = match family.codec {
            Codec::Synth106 => (1, 0, 0),
            Codec::Synth53 => (2, 0, 0),
            Codec::Synth8 => (0, SYNTH8_KEYS_SPLIT, 60),
            _ => (0, 0, 0),
        };
        if variant != 0 {
            sound.variant = Some(variant);
        }
        if family.codec == Codec::Synth8 {
            sound.keys = Some([option, split]);
        }
        receiver.open_page(Some(PAGE), &mut sink);
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play_on(2, 1, &sound, Some(PAGE)), &mut sink);
        push(&mut receiver, &bank(BANK, family.codec, &image), &mut sink);
        assert!(sink.locals.is_empty(), "{:?}: it waits for its banks, as any sound of its family", family.codec);
        push(&mut receiver, &bank(BANK + 1, family.codec, &[image.clone(), image.clone()].concat()), &mut sink);
        let id = SoundId { handle: SOUND, generation: 1 };
        let start = LocalStart {
            sound: id,
            family: family.codec.code(),
            variant,
            option,
            split,
            gain: 80,
            fade_ms: 5,
            duck_percent: 100,
            setup: &[],
        };
        assert_eq!(sink.locals, vec![(2, start, SETUP.to_vec())], "{:?}", family.codec);
        assert_eq!(
            sink.local_banks,
            vec![(2, id, 0, image.clone()), (2, id, 1, [image.clone(), image.clone()].concat())],
            "{:?}: its banks, index 0 first, before its start",
            family.codec
        );
        assert_eq!(receiver.local_sound(2), Some(id));
        // Nothing is rendered, streamed, defined or started as a stream.
        run(&mut receiver, &mut sink, 0, 50 * BLOCK_MS);
        assert!(sink.streams.is_empty() && sink.starts.is_empty() && sink.defined.is_empty() && sink.samples == 0);
        assert_eq!(receiver.next_tick_ms(), None, "a local sound asks for no tick");
        // Gain and stop act on its channel.
        let command = |operation, sequence| {
            Command {
                channel: 2,
                sequence,
                operation,
                handle: 0,
                generation: 0,
                gain: 40,
                fade_ms: 5,
                flags: 0,
                duck: 0,
                page: None,
            }
            .record()
        };
        receiver.push_record(&command(3, 2), &mut sink);
        assert_eq!(sink.gains, vec![(2, 40)]);
        sink.stops.clear();
        receiver.push_record(&command(2, 3), &mut sink);
        assert_eq!((sink.stops.clone(), receiver.local_sound(2)), (vec![2], None));
        // Played again, it ends when its page is no longer the open page.
        receiver.push_record(&play_on(2, 4, &sound, Some(PAGE)), &mut sink);
        assert_eq!((sink.locals.len(), receiver.local_sound(2)), (2, Some(id)));
        sink.stops.clear();
        receiver.open_page(Some(0x102), &mut sink);
        assert_eq!((sink.stops.clone(), receiver.local_sound(2)), (vec![2], None));
        // And starts again when the page is opened again: it is armed.
        receiver.open_page(Some(PAGE), &mut sink);
        assert_eq!((sink.locals.len(), receiver.local_sound(2)), (3, Some(id)));
        assert_eq!(receiver.statistics().local_refused, 0);
    }
}

// Verifies RULE-60 (REQ-6527): at most two local sounds play at once, a
// third play is refused and counted; so is a play with no page, a looped
// one and one that waits for the current sound.
#[test]
fn a_third_local_sound_and_a_play_with_no_page_are_refused_and_counted() {
    let family = &families()[1];
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder { local: true, ..Recorder::default() };
    receiver.open_page(Some(PAGE), &mut sink);
    push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
    let sounds = [local(1, family.codec), local(2, family.codec), local(3, family.codec)];
    for sound in &sounds {
        push(&mut receiver, sound, &mut sink);
    }
    assert_eq!(neoteletext_audio::MOST_LOCAL_SOUNDS, 2);
    receiver.push_record(&play_on(0, 1, &sounds[0], None), &mut sink);
    assert_eq!((sink.locals.len(), receiver.statistics().local_refused), (0, 1), "a local sound belongs to a page");
    for (flags, sequence) in [(2, 2), (4, 3)] {
        let play = Command {
            channel: 0,
            sequence,
            operation: 1,
            handle: 1,
            generation: 1,
            gain: 100,
            fade_ms: 5,
            flags,
            duck: 0,
            page: Some(PAGE),
        };
        receiver.push_record(&play.record(), &mut sink);
    }
    assert_eq!((sink.locals.len(), receiver.statistics().local_refused), (0, 3), "looped, and after the current");
    receiver.push_record(&play_on(0, 4, &sounds[0], Some(PAGE)), &mut sink);
    receiver.push_record(&play_on(1, 1, &sounds[1], Some(PAGE)), &mut sink);
    receiver.push_record(&play_on(2, 1, &sounds[2], Some(PAGE)), &mut sink);
    assert_eq!(sink.locals.iter().map(|(channel, ..)| *channel).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(receiver.statistics().local_refused, 4, "the third");
    assert_eq!(receiver.local_sound(2), None);
    // A local sound replacing one on its own channel is no third.
    receiver.push_record(&play_on(1, 2, &sounds[2], Some(PAGE)), &mut sink);
    assert_eq!((sink.locals.len(), receiver.local_sound(1)), (3, Some(SoundId { handle: 3, generation: 1 })));
    // One stopped makes room.
    receiver.rule_command(
        &Control {
            channel: 0,
            sequence: 0,
            operation: Operation::Stop,
            handle: 0,
            generation: 0,
            gain: 0,
            fade_ms: 5,
            flags: 0,
            duck: 0,
            page: Some(PAGE),
        },
        &mut sink,
    );
    assert_eq!(receiver.local_sound(0), None);
    // A rule's play starts it as the server's does, whatever its sequence.
    receiver.rule_command(
        &Control {
            channel: 3,
            sequence: 0,
            operation: Operation::Play,
            handle: 2,
            generation: 1,
            gain: 100,
            fade_ms: 5,
            flags: 0,
            duck: 0,
            page: Some(PAGE),
        },
        &mut sink,
    );
    assert_eq!((sink.locals.len(), receiver.local_sound(3)), (4, Some(SoundId { handle: 2, generation: 1 })));
    assert_eq!(receiver.statistics().local_refused, 4);
    // A stream on a local sound's channel takes the channel from it.
    let tune = sound(family.codec, note_block(&[]), 3, &[BANK]);
    let tune = Sound { handle: 7, ..tune };
    push(&mut receiver, &tune, &mut sink);
    receiver.push_record(&play_on(3, 1, &tune, None), &mut sink);
    assert_eq!((receiver.local_sound(3), sink.starts.len()), (None, 1));
}

// Verifies RULE-60 (REQ-6527): a sink with no sound output stage for local
// sounds has their OBJECT ignored, as a codec the receiver does not have.
#[test]
fn a_sink_that_plays_no_local_sound_ignores_its_object() {
    let family = &families()[0];
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    receiver.open_page(Some(PAGE), &mut sink);
    push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
    let sound = local(SOUND, family.codec);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play_on(0, 1, &sound, Some(PAGE)), &mut sink);
    run(&mut receiver, &mut sink, 0, 10 * BLOCK_MS);
    assert!(sink.locals.is_empty() && sink.streams.is_empty() && sink.starts.is_empty());
    assert_eq!(receiver.statistics().unsupported, 1);
    assert!(receiver.store().entry(SOUND).is_none());
}

// Verifies RULE-30 and RULE-31 (REQ-6527): the envelope position and the
// voice count of a sound played from blocks are read from the receiver's
// instrument at the position rendered, and there is none to read on a
// channel that plays no such sound.
#[test]
fn a_streamed_sounds_meter_is_read_from_the_receivers_instrument() {
    use neoteletext_audio::midi::MeterStage;
    for family in families() {
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
        // A note held from the first block to the end of the fortieth:
        // longer than the lead a playing is rendered ahead by (INT-34).
        let sound = sound(family.codec, note_block(&[]), 40, &[BANK]);
        push(&mut receiver, &sound, &mut sink);
        assert_eq!(receiver.meter(1, 0), None, "{:?}: nothing plays yet", family.codec);
        receiver.push_record(&play(1, 1, &sound), &mut sink);
        let meter = receiver.meter(1, 0).unwrap();
        assert_eq!(meter.voices, 1, "{:?}", family.codec);
        assert_ne!(meter.stage, MeterStage::Idle, "{:?}: the note's envelope", family.codec);
        assert_eq!(receiver.meter(0, 0), None, "another channel plays nothing");
        run(&mut receiver, &mut sink, 0, FOREVER_MS);
        assert_eq!(receiver.meter(1, 0), None, "{:?}: the stream has ended", family.codec);
    }
}

/// The state a page's rules read of a sound of `family` with `image` as
/// its one bank, after `blocks` in order: what a receiver that tracked it
/// from its start holds.
#[cfg(feature = "watch")]
fn tracked_after(family: &Family, image: &[u8], blocks: &[Vec<u8>]) -> neoteletext_audio_instruments::watch::Watched {
    use neoteletext_audio_instruments::midi::Banks;
    let mut banks = Banks::none();
    banks.push(image);
    let mut watched = neoteletext_audio_instruments::watch::Watched::new(family.codec.code(), 0, &banks).unwrap();
    for block in blocks {
        watched.take_block(block, &banks);
    }
    watched
}

/// A stored sound of 40 blocks: in block 0 a change to program 1, the
/// modulation wheel to 64 and a note on of key 57 at velocity 100; in
/// block 30, later than the lead a playing is rendered ahead by (INT-34),
/// its note off.
#[cfg(feature = "watch")]
fn tracked_tune(codec: Codec) -> (Sound, Vec<Vec<u8>>) {
    let mut blocks = vec![Vec::new(); 40];
    blocks[0] = note_block(&[0, 0xC0, 1, 0, 0xB0, 1, 64]);
    blocks[30] = vec![0, 0x80, 57, 0];
    let mut sound = Sound::events(SOUND, 1, codec, &blocks).unwrap();
    sound.banks = vec![BANK];
    (sound, blocks)
}

// Verifies RULE-30 (REQ-6527): the tracked values of a sound played from
// blocks, classes 0 to 4, 8 and 9, are read from the receiver at the
// position rendered, for each family, while its audio channel is watched;
// a channel that is not watched, and one that plays nothing, has none.
#[cfg(feature = "watch")]
#[test]
fn a_streamed_sounds_tracked_values_are_read_at_the_position_rendered() {
    use neoteletext_audio::midi::class;
    use neoteletext_audio_instruments::midi::Banks;
    for family in families() {
        let image = (family.image)(&[&family.plain, &family.dull]);
        let mut banks = Banks::none();
        banks.push(&image);
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        push(&mut receiver, &bank(BANK, family.codec, &image), &mut sink);
        let (sound, blocks) = tracked_tune(family.codec);
        push(&mut receiver, &sound, &mut sink);
        receiver.watch(1, true);
        assert!(receiver.is_watched(1) && !receiver.is_watched(0));
        assert_eq!(receiver.tracked(1, class::NOTES, 0, 0), None, "{:?}: nothing plays yet", family.codec);
        receiver.push_record(&play(1, 1, &sound), &mut sink);
        // The lead is rendered: block 0, and not yet block 30.
        let read = |receiver: &Receiver<'_, 8, 2>, class: u8, index: u16| receiver.tracked(1, class, 0, index);
        let held = [
            (class::PROGRAM, 0, 1),
            (class::NOTES, 0, 1),
            (class::NOTE, 57, 1),
            (class::NOTE, 58, 0),
            (class::VELOCITY, 57, 100),
            (class::CONTROLLER, 1, 64),
            (class::BEND, 0, 8_192),
        ];
        for (class, index, value) in held {
            assert_eq!(read(&receiver, class, index), Some(value), "{:?}: class {class} of {index}", family.codec);
        }
        // Every patch field is the field of the patch program 1 chose.
        let chosen = tracked_after(&family, &image, &blocks[..1]);
        let untouched = tracked_after(&family, &image, &[]);
        let fields = u16::try_from(chosen.fields().len()).unwrap();
        assert!(fields > 0, "{:?}", family.codec);
        for number in 0..fields {
            let expected = chosen.read(class::FIELD, 0, number, &banks);
            assert_eq!(read(&receiver, class::FIELD, number), Some(expected), "{:?}: field {number}", family.codec);
        }
        assert!(
            (0..fields).any(|number| {
                chosen.read(class::FIELD, 0, number, &banks) != untouched.read(class::FIELD, 0, number, &banks)
            }),
            "{:?}: the two patches differ in a field a rule can read",
            family.codec
        );
        assert_eq!(read(&receiver, class::FIELD, fields), Some(0), "a field past the table reads 0 (RULE-28)");
        assert_eq!(receiver.tracked(0, class::NOTES, 0, 0), None, "channel 0 is not watched and plays nothing");

        // Past block 30 the key is up: the value follows what is rendered.
        run(&mut receiver, &mut sink, 0, 32 * BLOCK_MS);
        assert_eq!(read(&receiver, class::NOTE, 57), Some(0), "{:?}: the note off was rendered", family.codec);
        assert_eq!(read(&receiver, class::NOTES, 0), Some(0), "{:?}", family.codec);
        assert_eq!(read(&receiver, class::PROGRAM, 0), Some(1), "{:?}", family.codec);
        // Not watched, nothing is kept; watched again, it is caught up.
        receiver.watch(1, false);
        assert_eq!(read(&receiver, class::PROGRAM, 0), None, "{:?}: the channel is watched no longer", family.codec);
        receiver.watch(1, true);
        assert_eq!(read(&receiver, class::PROGRAM, 0), Some(1), "{:?}", family.codec);
        run(&mut receiver, &mut sink, 32 * BLOCK_MS, FOREVER_MS);
        assert_eq!(read(&receiver, class::NOTES, 0), None, "{:?}: the stream has ended", family.codec);
    }
}

// Verifies RULE-30 (REQ-6527): a watch asked for while a stored sound
// already plays is caught up to the position rendered, so its values are
// those of a watch asked for before the play.
#[cfg(feature = "watch")]
#[test]
fn a_watch_asked_for_during_a_playing_is_caught_up() {
    use neoteletext_audio::midi::class;
    for family in families() {
        let image = (family.image)(&[&family.plain, &family.dull]);
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        push(&mut receiver, &bank(BANK, family.codec, &image), &mut sink);
        let (sound, _) = tracked_tune(family.codec);
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play(2, 1, &sound), &mut sink);
        assert_eq!(receiver.tracked(2, class::NOTE, 0, 57), None, "{:?}: not watched", family.codec);
        receiver.watch(2, true);
        let read = |receiver: &Receiver<'_, 8, 2>, class: u8, index: u16| receiver.tracked(2, class, 0, index);
        assert_eq!(read(&receiver, class::NOTE, 57), Some(1), "{:?}", family.codec);
        assert_eq!(read(&receiver, class::VELOCITY, 57), Some(100), "{:?}", family.codec);
        assert_eq!(read(&receiver, class::PROGRAM, 0), Some(1), "{:?}", family.codec);
        assert_eq!(read(&receiver, class::CONTROLLER, 1), Some(64), "{:?}", family.codec);
        run(&mut receiver, &mut sink, 0, 32 * BLOCK_MS);
        assert_eq!(read(&receiver, class::NOTE, 57), Some(0), "{:?}", family.codec);
    }
}

// Verifies RULE-30 with MIDI-102 (REQ-6527): a live sound's values are
// followed as its blocks are rendered, and a block played through as lost
// leaves no key down for the rules, as it leaves none sounding.
#[cfg(feature = "watch")]
#[test]
fn a_live_sounds_tracked_notes_end_at_a_lost_block() {
    use neoteletext_audio::midi::class;
    for family in families() {
        let mut arena = vec![0_u8; 128 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
        let mut sound = Sound::live_events(SOUND, 1, family.codec).unwrap();
        sound.banks = vec![BANK];
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        receiver.watch(0, true);
        for index in (0..=20_u16).filter(|index| *index != 5) {
            let block = if index == 0 { note_block(&[]) } else { Vec::new() };
            for record in sound.live_event_records(index, &[block]).unwrap() {
                receiver.push_record(&record, &mut sink);
            }
            if index == 0 {
                receiver.push_record(&play(0, 1, &sound), &mut sink);
            }
            if index == 4 {
                assert_eq!(receiver.tracked(0, class::NOTE, 0, 57), Some(1), "{:?}: the key is down", family.codec);
            }
        }
        assert_eq!(sink.concealed, vec![5], "{:?}", family.codec);
        assert_eq!(receiver.tracked(0, class::NOTES, 0, 0), Some(0), "{:?}: released at the gap", family.codec);
    }
}

/// A stored synth7 sound of `blocks` blocks with a note in block 0 and the
/// cues given, each a tick from the sound's start and a number.
fn cued_tune(blocks: u16, cues: &[(u64, u8)]) -> Sound {
    let note = TimedEvent { tick: 0, message: Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, 57, 100)) };
    let cues: Vec<TimedCue> = cues.iter().map(|&(tick, number)| TimedCue { tick, number }).collect();
    let tune = event_blocks_with_cues(&[note], &cues, blocks).unwrap();
    let mut sound = Sound::events(SOUND, 1, Codec::Synth7, &tune).unwrap();
    sound.banks = vec![BANK];
    sound
}

/// The sample of a stream a tick from its start stands at: 4.8 samples a
/// tick (MIDI-5), by block.
fn sample_of(tick: u64) -> u64 {
    (tick / 200) * BLOCK as u64 + (tick % 200) * 24 / 5
}

// Verifies RULE-109 (REQ-6614 items 3 and 5): the server's writer places
// each cue in the block and at the tick its time gives, before an event of
// the same tick, and refuses a fifth in a block, a number above the
// highest, a cue out of order and one past the sound's end.
#[test]
fn timed_cues_are_written_into_their_blocks() {
    let note = |tick| TimedEvent { tick, message: Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, 60, 100)) };
    let cue = |tick, number| TimedCue { tick, number };
    let blocks = event_blocks_with_cues(&[note(0), note(210)], &[cue(0, 1), cue(205, 2), cue(210, 3)], 3).unwrap();
    assert_eq!(
        blocks,
        vec![
            vec![0, 0xFF, 0x07, 0x01, 1, 0, 0x90, 60, 100],
            vec![5, 0xFF, 0x07, 0x01, 2, 5, 0xFF, 0x07, 0x01, 3, 0, 0x90, 60, 100],
            vec![]
        ]
    );
    assert_eq!(
        event_blocks(&[note(0), note(210)], 3).unwrap(),
        event_blocks_with_cues(&[note(0), note(210)], &[], 3).unwrap()
    );
    let five: Vec<TimedCue> = (0..5).map(|number| cue(400, number)).collect();
    assert_eq!(event_blocks_with_cues(&[], &five, 3), Err(BlocksError::TooManyCues(2)));
    assert_eq!(event_blocks_with_cues(&[], &[cue(0, 128)], 3), Err(BlocksError::CueNumber(0)));
    assert_eq!(event_blocks_with_cues(&[], &[cue(9, 1), cue(8, 1)], 3), Err(BlocksError::CueOrder(1)));
    assert_eq!(event_blocks_with_cues(&[], &[cue(600, 1)], 3), Err(BlocksError::CuePastEnd(0)));
    assert_eq!(event_blocks_with_cues(&[note(600)], &[cue(1, 1)], 3), Err(BlocksError::PastEnd(0)));
}

// Verifies RULE-109 (REQ-6614 item 3): the receiver gives the sink a mark
// for each cue of a stored sound it renders, once and in order, with the
// cue's number and its place in the stream as a count of samples from the
// stream's start, before the samples of the block that holds it; the
// samples streamed are those of the sound without its cues; and a sound
// that is looped marks its cues again on each pass, the positions rising.
#[test]
fn a_stored_sounds_cues_are_marked_at_their_place_in_the_stream() {
    const BLOCKS: u16 = 40;
    let cues = [(0, 7), (1_234, 0), (5_999, 127)];
    let sound = cued_tune(BLOCKS, &cues);
    let bare = cued_tune(BLOCKS, &[]);
    let id = SoundId { handle: SOUND, generation: 1 };
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &synth7_bank(), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    receiver.push_record(&play(1, 1, &sound), &mut sink);
    let ended = run(&mut receiver, &mut sink, 0, FOREVER_MS);
    assert!(sink.streams[0].ended);
    assert_eq!(sink.streams[0].pcm, synth7_alone(&bare), "a cue changes no sample");
    let marks: Vec<(u8, SoundId, u64, u8)> =
        sink.cues.iter().map(|&(channel, sound, position, number, _)| (channel, sound, position, number)).collect();
    let expected: Vec<(u8, SoundId, u64, u8)> =
        cues.iter().map(|&(tick, number)| (1, id, sample_of(tick), number)).collect();
    assert_eq!(marks, expected);
    assert_eq!(expected[1].2, 6 * BLOCK as u64 + 163);
    for &(_, _, position, _, held) in &sink.cues {
        // The mark comes before its block's samples: the sink can never
        // have played past it.
        assert!(held as u64 <= position && position < (held + BLOCK) as u64, "{position} with {held} held");
    }
    assert_eq!(receiver.statistics().events_passed_over, 0);

    // Looped, it plays on in one stream (MIDI-105) and marks each pass.
    let mut command = Command {
        channel: 1,
        sequence: 2,
        operation: 1,
        handle: sound.handle,
        generation: sound.generation,
        gain: 100,
        fade_ms: 0,
        flags: neoteletext_audio::record::FLAG_LOOP,
        duck: 0,
        page: None,
    };
    sink.cues.clear();
    receiver.push_record(&command.record(), &mut sink);
    let passes = 3_u64;
    let now = run(&mut receiver, &mut sink, ended, ended + passes * u64::from(BLOCKS) * BLOCK_MS);
    let marked: Vec<(u64, u8)> = sink.cues.iter().map(|&(_, _, position, number, _)| (position, number)).collect();
    let pass = u64::from(BLOCKS) * BLOCK as u64;
    assert!(marked.len() >= 3 * passes as usize, "{marked:?}");
    for (index, &(position, number)) in marked.iter().enumerate() {
        let (tick, expected) = cues[index % 3];
        assert_eq!((position, number), ((index / 3) as u64 * pass + sample_of(tick), expected));
    }

    // Verifies RULE-109: a stopped sound has no further cue, though its
    // stream is rendered on to the end of its fade.
    command.sequence = 3;
    command.operation = 2;
    command.fade_ms = 2_000;
    let before = sink.cues.len();
    let rendered = sink.streams[1].pcm.len();
    receiver.tick(now, &mut sink);
    receiver.push_record(&command.record(), &mut sink);
    run(&mut receiver, &mut sink, now, now + FOREVER_MS);
    assert!(sink.streams[1].ended);
    assert!(sink.streams[1].pcm.len() >= rendered + u64::from(BLOCKS) as usize * BLOCK, "the fade spans a whole pass");
    assert_eq!(sink.cues.len(), before, "{:?}", &sink.cues[before..]);
}

// Verifies RULE-109 (REQ-6614 item 3): a live sound's cues are marked as
// its blocks are rendered, counted from where its stream began; a block
// that never arrived, played through as lost, marks none.
#[test]
fn a_live_sounds_cues_are_marked_and_a_lost_blocks_are_not() {
    let family = &families()[0];
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &bank(BANK, family.codec, &(family.image)(&[&family.plain])), &mut sink);
    let mut sound = Sound::live_events(SOUND, 1, family.codec).unwrap();
    sound.banks = vec![BANK];
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    // Block 4 never arrives.
    for index in (0..=12_u16).filter(|index| *index != 4) {
        let block = if index == 0 { Vec::new() } else { vec![100, 0xFF, 0x07, 0x01, index as u8] };
        for record in sound.live_event_records(index, &[block]).unwrap() {
            receiver.push_record(&record, &mut sink);
        }
        if index == 0 {
            receiver.push_record(&play(0, 1, &sound), &mut sink);
        }
    }
    let numbers: Vec<u8> = sink.cues.iter().map(|cue| cue.3).collect();
    assert!(numbers.len() >= 6 && numbers.contains(&5) && !numbers.contains(&4), "{numbers:?}");
    assert!(numbers.windows(2).all(|pair| pair[0] < pair[1]), "{numbers:?}");
    for &(_, _, position, _, held) in &sink.cues {
        assert_eq!(position, held as u64 + 480, "tick 100 of the block rendered next");
    }
    assert_eq!(sink.concealed, vec![4]);
}

// Verifies RULE-109 and RULE-60: a local sound has no timeline and so no
// cues; the encoder refuses a setup that holds one.
#[test]
fn a_local_sounds_setup_holds_no_cue() {
    let setup = [&SETUP[..], &[0, 0xFF, 0x07, 0x01, 1]].concat();
    assert!(matches!(Sound::local(SOUND, 1, Codec::Synth7, &setup), Err(EncodeError::Setup)));
    assert!(Sound::local(SOUND, 1, Codec::Synth7, &SETUP).is_ok());
}

// Verifies RULE-112 (DEC-6599): a local sound belongs to the page, number
// and sub-code, open when its play was carried out. The CONTROL record
// names the number alone, so the port says when the sub-code changed: the
// local sound ends, with its channel stopped, and does not start again by
// itself, while a page sound that is not local plays on.
#[test]
fn a_local_sound_ends_when_the_ports_sub_code_changes() {
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder { local: true, ..Recorder::default() };
    receiver.open_page(Some(PAGE), &mut sink);
    let sound = local(SOUND, Codec::Synth7);
    let id = SoundId { handle: SOUND, generation: 1 };
    push(&mut receiver, &synth7_bank(), &mut sink);
    push(&mut receiver, &sound, &mut sink);
    let mut tune = synth7_tune(200);
    tune.handle = SOUND + 1;
    push(&mut receiver, &tune, &mut sink);
    receiver.push_record(&play_on(2, 1, &sound, Some(PAGE)), &mut sink);
    receiver.push_record(&play_on(0, 1, &tune, Some(PAGE)), &mut sink);
    assert_eq!((receiver.local_sound(2), sink.locals.len(), sink.streams.len()), (Some(id), 1, 1));
    sink.stops.clear();
    receiver.end_local_sounds(&mut sink);
    assert_eq!((receiver.local_sound(2), sink.stops.clone()), (None, vec![2]));
    assert!(!sink.streams[0].ended, "a page sound that is not local follows the page number");
    // It is armed no longer: nothing starts it again on this page number.
    receiver.open_page(Some(0x102), &mut sink);
    receiver.open_page(Some(PAGE), &mut sink);
    assert_eq!((receiver.local_sound(2), sink.locals.len()), (None, 1));
    // A play of a local sound still waiting for its sound is given up too.
    let late = {
        let mut late = local(SOUND + 2, Codec::Synth7);
        late.banks = vec![BANK];
        late
    };
    receiver.push_record(&late.object_record().unwrap(), &mut sink);
    receiver.push_record(&play_on(3, 1, &late, Some(PAGE)), &mut sink);
    receiver.end_local_sounds(&mut sink);
    for record in late.data_records() {
        receiver.push_record(&record, &mut sink);
    }
    assert_eq!((receiver.local_sound(3), sink.locals.len()), (None, 1));
}
