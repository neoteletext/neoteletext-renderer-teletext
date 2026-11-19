//! The receiver end to end: sounds written by the publishing encoder, cut
//! into broadcast packets, read back and played through a recording sink
//! (REQ-AUD-001, REQ-AUD-007, REQ-AUD-009, REQ-AUD-010, REQ-AUD-011,
//! REQ-AUD-012, REQ-AUD-013, REQ-AUD-014, REQ-AUD-016, REQ-AUD-018 and
//! REQ-AUD-020).
#![cfg(feature = "encode")]

use neoteletext_audio::encode::{Command, LiveEncoder, Sound, packets};
use neoteletext_audio::ima::decode_block;
use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::{FLAG_PROGRESSIVE, MAX_LABEL};
use neoteletext_audio::{Codec, IMA_BLOCK_SMALL, MIN_FADE_MS};
use neoteletext_core::packet::Packet;

/// What the sink was asked to do, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Call {
    Define(SoundId, SoundFormat),
    Samples(SoundId, u32, usize),
    Forget(SoundId),
    Stream(SoundId, u32),
    StreamSamples(SoundId, usize),
    Start(u8, Start),
    Stop(u8, u16),
    Gain(u8, u8, u16),
    Concealed(SoundId, u16, u32),
    StreamMagazine(u8, SoundId, Option<u8>),
    MusicLevel(u8, u8, u16),
    Heard(u8, u8, bool),
}

#[derive(Default)]
struct Recorder {
    calls: Vec<Call>,
    /// The samples received, per sound, assembled.
    pcm: Vec<(SoundId, Vec<i16>)>,
    channels: u8,
    /// What it answers for every channel's voice position, as a sink that
    /// knows its own output would (DEC-2470); `None` as one that does not.
    position: Option<u32>,
}

impl Recorder {
    fn new() -> Self {
        Self { channels: 4, ..Self::default() }
    }

    fn starts(&self) -> Vec<(u8, SoundId)> {
        self.calls
            .iter()
            .filter_map(
                |call| if let Call::Start(channel, start) = call { Some((*channel, start.sound)) } else { None },
            )
            .collect()
    }

    fn take(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.calls)
    }
}

impl VoiceSink for Recorder {
    fn channels(&self) -> u8 {
        self.channels
    }
    fn define(&mut self, sound: SoundId, format: SoundFormat) {
        self.calls.push(Call::Define(sound, format));
        self.pcm.push((sound, vec![0; format.samples as usize]));
    }
    fn samples(&mut self, sound: SoundId, first: u32, pcm: &[i16]) {
        self.calls.push(Call::Samples(sound, first, pcm.len()));
        if let Some((_, buffer)) = self.pcm.iter_mut().rev().find(|(id, _)| *id == sound) {
            buffer[first as usize..first as usize + pcm.len()].copy_from_slice(pcm);
        }
    }
    fn forget(&mut self, sound: SoundId) {
        self.calls.push(Call::Forget(sound));
    }
    fn end_stream(&mut self, _channel: u8, _sound: SoundId) {}
    fn stream(&mut self, _channel: u8, sound: SoundId, rate_hz: u32) {
        self.calls.push(Call::Stream(sound, rate_hz));
        self.pcm.push((sound, Vec::new()));
    }
    fn stream_samples(&mut self, _channel: u8, sound: SoundId, pcm: &[i16]) {
        self.calls.push(Call::StreamSamples(sound, pcm.len()));
        if let Some((_, buffer)) = self.pcm.iter_mut().rev().find(|(id, _)| *id == sound) {
            buffer.extend_from_slice(pcm);
        }
    }
    fn start(&mut self, channel: u8, start: Start) {
        self.calls.push(Call::Start(channel, start));
    }
    fn stop(&mut self, channel: u8, fade_ms: u16) {
        self.calls.push(Call::Stop(channel, fade_ms));
    }
    fn gain(&mut self, channel: u8, gain: u8, fade_ms: u16) {
        self.calls.push(Call::Gain(channel, gain, fade_ms));
    }
    fn concealed(&mut self, sound: SoundId, block: u16, samples: u32) {
        self.calls.push(Call::Concealed(sound, block, samples));
    }
    fn position(&self, _channel: u8) -> Option<u32> {
        self.position
    }
    fn stream_magazine(&mut self, channel: u8, sound: SoundId, magazine: Option<u8>) {
        self.calls.push(Call::StreamMagazine(channel, sound, magazine));
    }
    fn music_level(&mut self, channel: u8, percent: u8, fade_ms: u16) {
        self.calls.push(Call::MusicLevel(channel, percent, fade_ms));
    }
    fn magazine_heard(&mut self, channel: u8, magazine: u8, heard: bool) {
        self.calls.push(Call::Heard(channel, magazine, heard));
    }
}

/// A DATA record holding `blocks` of `sound` from `first`, framed. Every
/// codec's tests use it, so it is built whatever codecs are compiled in.
fn data_record(sound: &Sound, first: u16, blocks: &[u8]) -> Vec<u8> {
    let mut body = vec![sound.handle];
    body.extend_from_slice(&sound.generation.to_be_bytes());
    body.extend_from_slice(&first.to_be_bytes());
    body.extend_from_slice(blocks);
    neoteletext_audio::encode::frame(neoteletext_audio::record::TYPE_DATA, &body)
}

/// A tone of `samples` samples.
fn tone(samples: usize) -> Vec<i16> {
    (0..samples).map(|n| i16::try_from(i32::try_from(n % 50).unwrap() * 400 - 10_000).unwrap()).collect()
}

fn play(channel: u8, sequence: u16, sound: &Sound, page: Option<u16>) -> Command {
    Command {
        channel,
        sequence,
        operation: 1,
        handle: sound.handle,
        generation: sound.generation,
        gain: 80,
        fade_ms: 0,
        flags: 0,
        duck: 0,
        page,
        magazine: None,
    }
}

#[cfg(feature = "pcm16")]
fn stop(channel: u8, sequence: u16) -> Command {
    Command {
        channel,
        sequence,
        operation: 2,
        handle: 0,
        generation: 0,
        gain: 0,
        fade_ms: 50,
        flags: 0,
        duck: 0,
        page: None,
        magazine: None,
    }
}

/// The broadcast packets of a sound and some commands.
fn broadcast(sound: &Sound, commands: &[Command]) -> Vec<Packet> {
    let mut records = vec![sound.object_record().unwrap()];
    records.extend(sound.data_records());
    records.extend(commands.iter().map(Command::record));
    packets(0, 0, &records).0
}

const ARENA: usize = 64 * 1024;
const NO_STARTS: [(u8, SoundId); 0] = [];
const NO_CALLS: [Call; 0] = [];

// Verifies REQ-AUD-012.
#[test]
fn a_page_sound_plays_when_its_page_is_opened() {
    let pcm = tone(4000);
    let sound = Sound::new(1, 1, Codec::ImaAdpcm, 16_000, IMA_BLOCK_SMALL, &pcm).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    for packet in broadcast(&sound, &[play(0, 1, &sound, Some(0x100))]) {
        assert!(receiver.push_packet(&packet, &mut sink));
    }
    assert_eq!(sink.take(), NO_CALLS, "nothing plays before page 100 is open");
    receiver.open_page(Some(0x100), &mut sink);
    let calls = sink.take();
    let id = SoundId { handle: 1, generation: 1 };
    assert_eq!(calls.first(), Some(&Call::Define(id, SoundFormat { rate_hz: 16_000, samples: 4000 })));
    assert!(
        matches!(calls.last(), Some(Call::Start(0, Start { sound, gain: 80, fade_ms: MIN_FADE_MS, .. })) if *sound == id)
    );
    // The samples are exactly the encoded blocks decoded.
    let mut expected = Vec::new();
    for block in sound.data.chunks(usize::from(IMA_BLOCK_SMALL)) {
        let mut out = [0_i16; 1017];
        let count = decode_block(block, &mut out).unwrap();
        expected.extend_from_slice(&out[..count]);
    }
    expected.truncate(4000);
    assert_eq!(sink.pcm[0].1, expected);
    // Leaving the page fades the sound out; coming back plays it again.
    receiver.open_page(Some(0x101), &mut sink);
    assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS)]);
    receiver.open_page(Some(0x100), &mut sink);
    assert_eq!(sink.starts(), [(0, id)], "already at the sink, so only started");
}

// Verifies REQ-AUD-001 and REQ-AUD-018.
#[cfg(feature = "pcm16")]
#[test]
fn pcm_round_trips_exactly() {
    let pcm = tone(777);
    let sound = Sound::new(2, 1, Codec::Pcm16, 8_000, 1000, &pcm).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    for packet in broadcast(&sound, &[play(1, 1, &sound, None)]) {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.starts(), [(1, SoundId { handle: 2, generation: 1 })]);
    assert_eq!(sink.pcm[0].1, pcm);
}

// Verifies REQ-AUD-010.
#[cfg(feature = "pcm16")]
#[test]
fn a_stop_that_arrives_before_the_older_play_wins() {
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(100)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    // The stop (sequence 2) is sent before the play (sequence 1).
    for packet in broadcast(&sound, &[stop(0, 2), play(0, 1, &sound, None)]) {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.starts(), NO_STARTS);
    assert_eq!(receiver.statistics().stale_commands, 1);
    // Repeats never replay.
    let mut sink = Recorder::new();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let packets = broadcast(&sound, &[play(0, 1, &sound, None)]);
    for _ in 0..3 {
        for packet in &packets {
            receiver.push_packet(packet, &mut sink);
        }
    }
    assert_eq!(sink.starts().len(), 1);
}

#[test]
fn a_play_waits_for_its_sound_to_arrive() {
    let sound = Sound::new(1, 1, Codec::ImaAdpcm, 8_000, IMA_BLOCK_SMALL, &tone(3000)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let command = packets(0, 200, &[play(0, 1, &sound, None).record()]).0;
    for packet in &command {
        receiver.push_packet(packet, &mut sink);
    }
    assert_eq!(sink.starts(), NO_STARTS, "no sound yet");
    let mut records = vec![sound.object_record().unwrap()];
    records.extend(sound.data_records());
    let (packets, _) = packets(0, 0, &records);
    // Every packet but the last: the sound is incomplete.
    for packet in &packets[..packets.len() - 1] {
        receiver.push_packet(packet, &mut sink);
    }
    assert_eq!(sink.starts(), NO_STARTS, "incomplete");
    receiver.push_packet(packets.last().unwrap(), &mut sink);
    assert_eq!(sink.starts().len(), 1);
}

#[test]
fn a_progressive_play_starts_after_forty_milliseconds() {
    // 8 kHz IMA ADPCM: 505 samples, 63 ms, a block; the first DATA record
    // holds three blocks.
    let sound = Sound::new(1, 1, Codec::ImaAdpcm, 8_000, IMA_BLOCK_SMALL, &tone(8000)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    let mut records = vec![command.record(), sound.object_record().unwrap()];
    let data = sound.data_records();
    records.push(data[0].clone());
    for packet in packets(0, 0, &records).0 {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.starts().len(), 1, "started on the first record's blocks");
    let before = sink.calls.len();
    // Later blocks go to the sink as they arrive.
    for packet in packets(0, 10, &data[1..2]).0 {
        receiver.push_packet(&packet, &mut sink);
    }
    assert!(sink.calls[before..].iter().all(|call| matches!(call, Call::Samples(..))) && sink.calls.len() > before);
}

#[cfg(feature = "pcm16")]
#[test]
fn damaged_packets_are_dropped_and_the_carousel_repeat_recovers() {
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(2000)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let packets = broadcast(&sound, &[play(0, 1, &sound, None)]);
    let mut damaged = packets.clone();
    damaged[3].bytes[20] ^= 0x40;
    for packet in &damaged {
        receiver.push_packet(packet, &mut sink);
    }
    assert_eq!(sink.starts(), NO_STARTS);
    assert_eq!(receiver.statistics().packets_dropped, 1);
    for packet in &packets {
        receiver.push_packet(packet, &mut sink);
    }
    assert_eq!(sink.starts().len(), 1);
}

// Verifies REQ-AUD-012.
#[cfg(feature = "pcm16")]
#[test]
fn page_sounds_off_keeps_a_page_quiet() {
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(100)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.set_page_sounds(false, &mut sink);
    receiver.open_page(Some(0x100), &mut sink);
    for packet in broadcast(&sound, &[play(0, 1, &sound, Some(0x100))]) {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.starts(), NO_STARTS);
    receiver.set_page_sounds(true, &mut sink);
    assert_eq!(sink.starts().len(), 1);
}

// Verifies REQ-AUD-016.
#[cfg(feature = "pcm16")]
#[test]
fn channels_beyond_the_sink_are_ignored_and_a_lost_sink_is_refilled() {
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(100)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder { channels: 2, ..Recorder::new() };
    for packet in broadcast(&sound, &[play(3, 1, &sound, None), play(1, 1, &sound, None)]) {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.starts(), [(1, SoundId { handle: 1, generation: 1 })]);
    // The sink forgot everything: the next play defines the sound again.
    receiver.sink_lost();
    sink.take();
    for packet in packets(0, 50, &[play(1, 2, &sound, None).record()]).0 {
        receiver.push_packet(&packet, &mut sink);
    }
    assert!(matches!(sink.calls.first(), Some(Call::Define(..))));
}

// Verifies REQ-AUD-018.
#[cfg(feature = "pcm16")]
#[test]
fn a_newer_generation_replaces_the_sound_at_the_sink() {
    let first = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(100)).unwrap();
    let second = Sound::new(1, 2, Codec::Pcm16, 8_000, 1000, &tone(200)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    for packet in broadcast(&first, &[play(0, 1, &first, None)]) {
        receiver.push_packet(&packet, &mut sink);
    }
    sink.take();
    let mut records = vec![second.object_record().unwrap()];
    records.extend(second.data_records());
    for packet in packets(0, 100, &records).0 {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.take(), [Call::Forget(SoundId { handle: 1, generation: 1 })]);
}

#[test]
fn a_label_longer_than_the_bound_is_not_written() {
    let mut sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(10)).unwrap();
    sound.label = Some(vec![b'x'; MAX_LABEL + 1]);
    assert!(sound.object_record().is_err());
}

#[cfg(feature = "pcm16")]
/// A live PCM sound at 8 kHz, 100-byte blocks of 50 samples.
fn live_sound() -> Sound {
    Sound::live(4, 1, Codec::Pcm16, 8_000, 100).unwrap()
}

/// The packets of live blocks `first..first + count`, block `n` holding the
/// value `n` in every sample, one block a record.
#[cfg(feature = "pcm16")]
fn live_blocks(sound: &Sound, first: u16, count: u16) -> Vec<Packet> {
    let mut encoder = LiveEncoder::new(sound, first).unwrap();
    let records: Vec<Vec<u8>> = (0..count)
        .flat_map(|offset| encoder.records(&[i16::try_from(first.wrapping_add(offset) % 1000).unwrap(); 50]))
        .collect();
    packets(0, 0, &records).0
}

#[cfg(any(feature = "pcm16", feature = "opus"))]
fn push(receiver: &mut Receiver<'_, 8, 2>, packets: &[Packet], sink: &mut Recorder) {
    for packet in packets {
        assert!(receiver.push_packet(packet, sink));
    }
}

#[cfg(feature = "pcm16")]
/// The value of each block of 50 samples the sink received, in order: its
/// loudest sample, since a block next to a silence is faded at that edge.
fn live_values(sink: &Recorder, sound: &Sound) -> Vec<i16> {
    live_samples(sink, sound).chunks(50).map(|block| block.iter().copied().max().unwrap()).collect()
}

/// Every sample of a live sound the sink received, in order.
#[cfg(feature = "pcm16")]
fn live_samples(sink: &Recorder, sound: &Sound) -> Vec<i16> {
    let id = SoundId { handle: sound.handle, generation: sound.generation };
    sink.pcm.iter().rev().find(|(sound, _)| *sound == id).unwrap().1.clone()
}

// Verifies REQ-AUD-014.
#[cfg(feature = "pcm16")]
#[test]
fn a_live_sound_joins_at_its_newest_block_and_follows_in_order() {
    let sound = live_sound();
    let id = SoundId { handle: 4, generation: 1 };
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let mut announce = vec![sound.object_record().unwrap()];
    announce.push(play(0, 1, &sound, None).record());
    receiver.tick(1_000, &mut sink);
    push(&mut receiver, &packets(0, 0, &announce).0, &mut sink);
    assert_eq!(sink.take(), NO_CALLS, "no block yet: the play waits");
    push(&mut receiver, &live_blocks(&sound, 100, 3), &mut sink);
    let calls = sink.take();
    assert_eq!(calls[0], Call::Stream(id, 8_000));
    assert!(matches!(calls[2], Call::Start(0, Start { sound, looped: false, .. }) if sound == id), "{calls:?}");
    // Joined at block 100, the newest when the play could start, then 101 and 102.
    push(&mut receiver, &live_blocks(&sound, 103, 2), &mut sink);
    assert_eq!(live_values(&sink, &sound), [100, 101, 102, 103, 104]);
    assert_eq!(receiver.statistics().concealed, 0);
}

#[cfg(feature = "pcm16")]
#[test]
fn a_lost_live_block_is_heard_as_silence_once_three_newer_have_come() {
    let sound = live_sound();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let announce = [sound.object_record().unwrap(), play(0, 1, &sound, None).record()];
    push(&mut receiver, &packets(0, 0, &announce).0, &mut sink);
    push(&mut receiver, &live_blocks(&sound, 10, 1), &mut sink);
    // Block 11 is lost; 12 and 13 wait for it.
    push(&mut receiver, &live_blocks(&sound, 12, 2), &mut sink);
    assert_eq!(live_values(&sink, &sound), [10]);
    // A late 11 is played in its place.
    push(&mut receiver, &live_blocks(&sound, 11, 1), &mut sink);
    assert_eq!(live_values(&sink, &sound), [10, 11, 12, 13]);
    // 15 is lost for good: once 18 is here, three ahead, it is silence.
    push(&mut receiver, &live_blocks(&sound, 14, 1), &mut sink);
    push(&mut receiver, &live_blocks(&sound, 16, 2), &mut sink);
    assert_eq!(live_values(&sink, &sound), [10, 11, 12, 13, 14]);
    push(&mut receiver, &live_blocks(&sound, 18, 1), &mut sink);
    assert_eq!(live_values(&sink, &sound), [10, 11, 12, 13, 14, 0, 16, 17, 18]);
    assert_eq!(receiver.statistics().concealed, 1);
}

#[cfg(feature = "pcm16")]
#[test]
fn a_stalled_live_sound_fades_out_and_resumes_when_its_blocks_return() {
    let sound = live_sound();
    let id = SoundId { handle: 4, generation: 1 };
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let announce = [sound.object_record().unwrap(), play(0, 1, &sound, None).record()];
    receiver.tick(5_000, &mut sink);
    assert_eq!(receiver.next_tick_ms(), None, "nothing live: no tick needed");
    push(&mut receiver, &packets(0, 0, &announce).0, &mut sink);
    push(&mut receiver, &live_blocks(&sound, 0, 4), &mut sink);
    sink.take();
    assert_eq!(receiver.next_tick_ms(), Some(7_000), "the stall, two seconds after the last block");
    // Just short of two seconds without a block: still playing.
    receiver.tick(6_999, &mut sink);
    assert_eq!(sink.take(), NO_CALLS);
    // Two seconds (EN 300 708 clause 6.7.1.1): it fades out and is forgotten.
    receiver.tick(7_000, &mut sink);
    assert_eq!(sink.take(), [Call::Stop(0, neoteletext_audio::LIVE_STALL_FADE_MS), Call::Forget(id)]);
    assert_eq!(receiver.next_tick_ms(), None, "faded: no tick needed");
    receiver.tick(9_000, &mut sink);
    assert_eq!(sink.take(), NO_CALLS, "once only");
    // The sender comes back: the play starts again at the newest block.
    push(&mut receiver, &live_blocks(&sound, 500, 1), &mut sink);
    let calls = sink.take();
    assert_eq!(calls[0], Call::Stream(id, 8_000), "{calls:?}");
    assert!(calls.iter().any(|call| matches!(call, Call::Start(0, _))));
    assert_eq!(live_values(&sink, &sound), [500]);
}

#[cfg(feature = "pcm16")]
#[test]
fn a_stopped_live_sound_is_not_restarted_by_its_blocks() {
    let sound = live_sound();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let announce = [sound.object_record().unwrap(), play(0, 1, &sound, None).record(), stop(0, 2).record()];
    push(&mut receiver, &live_blocks(&sound, 0, 1), &mut sink);
    push(&mut receiver, &packets(0, 0, &announce).0, &mut sink);
    sink.take();
    push(&mut receiver, &live_blocks(&sound, 1, 3), &mut sink);
    assert_eq!(sink.starts(), NO_STARTS);
}

#[cfg(feature = "pcm16")]
#[test]
fn a_cursor_record_on_the_audio_channel_is_left_to_the_input_profile() {
    // The input profile's cursor record shares data channel 12 (decision
    // E.91); with the record id of a sound's fragment in flight it must not
    // disturb the sound, nor count as a refused record.
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 1000, &tone(777)).unwrap();
    let packets = broadcast(&sound, &[play(1, 1, &sound, None)]);
    let mut cursor = [0_u8; 16];
    let length = neoteletext_audio::record::write(
        neoteletext_audio::record::TYPE_CURSOR,
        &[0x01, 0x00, 0, 0, 255, 0, 0, 0],
        &mut cursor,
    )
    .unwrap();
    let cursor = neoteletext_audio::idl::packet(0, 0, 0, true, &cursor[..length]);
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let (first, rest) = packets.split_first().unwrap();
    receiver.push_packet(first, &mut sink);
    assert!(receiver.push_packet(&cursor, &mut sink), "it is a packet of the audio channel");
    for packet in rest {
        receiver.push_packet(packet, &mut sink);
    }
    assert_eq!(receiver.statistics().records_refused, 0);
    assert_eq!(sink.starts(), [(1, SoundId { handle: 1, generation: 1 })]);
}

/// The samples of a 5 ms loss fade at 8 kHz (REQ-AUD-013).
#[cfg(feature = "pcm16")]
const FADE_8_KHZ: usize = 40;

/// Whether `got` ramps `level` down to silence (or up from it, `rising`)
/// over its length, as (L − n) / (L + 1) of the level, within the rounding
/// of 16-bit fixed point.
#[cfg(feature = "pcm16")]
fn ramps(got: &[i16], level: i16, rising: bool) -> bool {
    let length = i32::try_from(got.len()).unwrap();
    got.iter().zip(0..).all(|(&sample, n)| {
        let steps = if rising { n + 1 } else { length - n };
        let want = i32::from(level) * steps / (length + 1);
        (i32::from(sample) - want).abs() <= 4
    })
}

/// The packets of live blocks `first..first + count`, every sample `level`.
#[cfg(feature = "pcm16")]
fn level_blocks(sound: &Sound, first: u16, count: u16, level: i16) -> Vec<Packet> {
    let mut encoder = LiveEncoder::new(sound, first).unwrap();
    let records: Vec<Vec<u8>> = (0..count).flat_map(|_| encoder.records(&[level; 50])).collect();
    packets(0, 0, &records).0
}

/// REQ-AUD-013: a live block given up is silence, faded into from the block
/// before and out of into the block after over 5 ms, and reported.
#[cfg(feature = "pcm16")]
#[test]
fn a_lost_live_block_is_faded_into_and_out_of_and_reported_concealed() {
    const LEVEL: i16 = 8000;
    let sound = live_sound();
    let id = SoundId { handle: 4, generation: 1 };
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let announce = [sound.object_record().unwrap(), play(0, 1, &sound, None).record()];
    push(&mut receiver, &packets(0, 0, &announce).0, &mut sink);
    push(&mut receiver, &level_blocks(&sound, 10, 1, LEVEL), &mut sink);
    // Block 11 is lost: once 14 is here, three ahead, it is silence.
    push(&mut receiver, &level_blocks(&sound, 12, 3, LEVEL), &mut sink);
    let heard = live_samples(&sink, &sound);
    let (before, rest) = heard.split_at(50);
    let (gap, after) = rest.split_at(50);
    assert!(before[..50 - FADE_8_KHZ].iter().all(|&sample| sample == LEVEL));
    assert!(ramps(&before[50 - FADE_8_KHZ..], LEVEL, false), "faded into the gap: {before:?}");
    assert!(gap.iter().all(|&sample| sample == 0), "{gap:?}");
    assert!(ramps(&after[..FADE_8_KHZ], LEVEL, true), "faded out of the gap: {after:?}");
    // Then whole blocks, the newest's last 5 ms held until its successor.
    assert!(after[FADE_8_KHZ..].iter().all(|&sample| sample == LEVEL));
    assert_eq!(after.len(), 3 * 50 - FADE_8_KHZ);
    let reports: Vec<&Call> = sink.calls.iter().filter(|call| matches!(call, Call::Concealed(..))).collect();
    assert_eq!(reports, [&Call::Concealed(id, 11, 50)]);
    assert_eq!(receiver.statistics().concealed, 1);
    // The held tail goes, whole, once the next block comes.
    push(&mut receiver, &level_blocks(&sound, 15, 1, LEVEL), &mut sink);
    let heard = live_samples(&sink, &sound);
    assert!(heard[100 + FADE_8_KHZ..].iter().all(|&sample| sample == LEVEL));
    assert_eq!(heard.len(), 100 + 4 * 50 - FADE_8_KHZ);
}

/// A stored PCM sound of four 500-sample blocks at one level, and its
/// records, one block each.
#[cfg(feature = "pcm16")]
fn level_sound(level: i16) -> (Sound, Vec<Vec<u8>>) {
    let sound = Sound::new(3, 1, Codec::Pcm16, 8_000, 1000, &[level; 2000]).unwrap();
    let data = sound.data_records();
    assert_eq!(data.len(), 4);
    (sound, data)
}

/// REQ-AUD-013: a missing stored block is silence at the sink, with the
/// blocks either side faded over 5 ms, and whole once it arrives.
#[cfg(feature = "pcm16")]
#[test]
fn a_missing_stored_block_is_faded_either_side_and_mended_when_it_comes() {
    const LEVEL: i16 = -9000;
    let (sound, data) = level_sound(LEVEL);
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let records = [sound.object_record().unwrap(), command.record(), data[0].clone(), data[1].clone(), data[3].clone()];
    push(&mut receiver, &packets(0, 0, &records).0, &mut sink);
    assert_eq!(sink.starts().len(), 1);
    let held = &sink.pcm[0].1;
    assert!(held[..1000 - FADE_8_KHZ].iter().all(|&sample| sample == LEVEL));
    assert!(ramps(&held[1000 - FADE_8_KHZ..1000], LEVEL, false), "block 1 fades into the gap");
    assert!(held[1000..1500].iter().all(|&sample| sample == 0), "block 2 is silence");
    assert!(ramps(&held[1500..1500 + FADE_8_KHZ], LEVEL, true), "block 3 fades out of it");
    assert!(held[1500 + FADE_8_KHZ..].iter().all(|&sample| sample == LEVEL));
    push(&mut receiver, &packets(0, 9, &data[2..3]).0, &mut sink);
    assert_eq!(sink.pcm[0].1, [LEVEL; 2000], "whole once block 2 is here");
}

/// REQ-AUD-013: the receiver follows a progressive play by its clock and
/// reports each block not there when the play reaches it, once; it asks
/// for a tick at that moment.
#[cfg(feature = "pcm16")]
#[test]
fn a_progressive_play_reports_each_block_not_there_when_it_is_due() {
    let (sound, data) = level_sound(1000);
    let id = SoundId { handle: 3, generation: 1 };
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.tick(1_000, &mut sink);
    let records = [sound.object_record().unwrap(), command.record(), data[0].clone(), data[1].clone()];
    push(&mut receiver, &packets(0, 0, &records).0, &mut sink);
    assert_eq!(sink.starts().len(), 1);
    // Block 2 starts 1 000 samples in: 125 ms at 8 kHz.
    assert_eq!(receiver.next_tick_ms(), Some(1_125));
    receiver.tick(1_124, &mut sink);
    let concealed = |sink: &Recorder| -> Vec<Call> {
        sink.calls.iter().filter(|call| matches!(call, Call::Concealed(..))).cloned().collect()
    };
    assert_eq!(concealed(&sink), NO_CALLS);
    receiver.tick(1_125, &mut sink);
    assert_eq!(concealed(&sink), [Call::Concealed(id, 2, 500)]);
    // Block 3 comes in time, block 2 late: nothing more is reported.
    push(&mut receiver, &packets(0, 7, &data[3..4]).0, &mut sink);
    assert_eq!(receiver.next_tick_ms(), None, "nothing more can be missed");
    push(&mut receiver, &packets(0, 9, &data[2..3]).0, &mut sink);
    receiver.tick(2_000, &mut sink);
    assert_eq!(concealed(&sink), [Call::Concealed(id, 2, 500)]);
    assert_eq!(receiver.statistics().concealed, 1);
}

/// REQ-AUD-013 (DEC-2470, TODO-1146): a sink that reports its voice's
/// position is believed over the receiver's clock. Its voice runs 500
/// samples behind that clock, as a sink with 62.5 ms of output latency
/// would, so block 2 is not due when the clock says it is, and arrives in
/// time; nothing is reported concealed. Before the sink was asked, block 2
/// was reported concealed at 1,125 ms though the voice never met it
/// missing.
#[cfg(feature = "pcm16")]
#[test]
fn a_sink_behind_the_receivers_clock_is_judged_by_its_own_position() {
    let (sound, data) = level_sound(1000);
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.tick(1_000, &mut sink);
    let records = [sound.object_record().unwrap(), command.record(), data[0].clone(), data[1].clone()];
    push(&mut receiver, &packets(0, 0, &records).0, &mut sink);
    // By the clock, block 2 starts 1 000 samples in: 125 ms at 8 kHz.
    assert_eq!(receiver.next_tick_ms(), Some(1_125));
    sink.position = Some(500);
    receiver.tick(1_125, &mut sink);
    let concealed = |sink: &Recorder| sink.calls.iter().filter(|call| matches!(call, Call::Concealed(..))).count();
    assert_eq!(concealed(&sink), 0, "the voice is 500 samples short of block 2");
    // From the voice at 500, block 2 is 500 samples, 62.5 ms, away.
    assert_eq!(receiver.next_tick_ms(), Some(1_188));
    push(&mut receiver, &packets(0, 7, &data[2..4]).0, &mut sink);
    sink.position = Some(1_900);
    receiver.tick(1_400, &mut sink);
    assert_eq!(concealed(&sink), 0);
    assert_eq!(receiver.statistics().concealed, 0);
}

/// REQ-AUD-013 (DEC-2470, TODO-1146): a sink whose voice runs ahead of the
/// receiver's clock is believed too, so a missing block is reported when
/// the voice reaches it, before the clock would have said.
#[cfg(feature = "pcm16")]
#[test]
fn a_sink_ahead_of_the_receivers_clock_has_a_missing_block_reported_sooner() {
    let (sound, data) = level_sound(1000);
    let id = SoundId { handle: 3, generation: 1 };
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.tick(1_000, &mut sink);
    let records = [sound.object_record().unwrap(), command.record(), data[0].clone(), data[1].clone()];
    push(&mut receiver, &packets(0, 0, &records).0, &mut sink);
    sink.position = Some(1_010);
    receiver.tick(1_050, &mut sink);
    let concealed: Vec<Call> = sink.calls.iter().filter(|call| matches!(call, Call::Concealed(..))).cloned().collect();
    assert_eq!(concealed, [Call::Concealed(id, 2, 500)]);
}

/// An IMA ADPCM sound, its records and a play of it, as a carousel sends them.
fn ima_records() -> (Sound, Vec<Vec<u8>>) {
    let sound = Sound::new(1, 1, Codec::ImaAdpcm, 8_000, IMA_BLOCK_SMALL, &tone(3000)).unwrap();
    let mut records = vec![sound.object_record().unwrap()];
    records.extend(sound.data_records());
    records.push(play(0, 1, &sound, None).record());
    (sound, records)
}

/// REQ-AUD-008: a `.ttau` file is records back to back; a truncated final
/// record is discarded and a length beyond the bound ends the file.
#[test]
fn a_ttau_file_is_read_as_records_back_to_back() {
    let (sound, records) = ima_records();
    let file: Vec<u8> = records.concat();
    let id = SoundId { handle: sound.handle, generation: sound.generation };
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    assert_eq!(receiver.push_file(&file, &mut sink), neoteletext_audio::ttau::End::Complete);
    assert_eq!(sink.starts(), [(0, id)]);
    assert_eq!(receiver.statistics().records, u32::try_from(records.len()).unwrap());
    // Cut short, the last record (the play) is discarded.
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    assert_eq!(receiver.push_file(&file[..file.len() - 1], &mut sink), neoteletext_audio::ttau::End::Truncated);
    assert_eq!(sink.starts(), NO_STARTS);
    assert_eq!(receiver.statistics().records_refused, 0);
    // A length beyond the bound: nothing after it is read.
    let mut bad = records[0].clone();
    bad.extend_from_slice(&[neoteletext_audio::PROFILE_VERSION, 2, 0xFF, 0xFF]);
    bad.extend(records[1..].concat());
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    assert_eq!(receiver.push_file(&bad, &mut sink), neoteletext_audio::ttau::End::BadLength);
    assert_eq!(receiver.statistics().records, 1);
}

/// REQ-AUD-008 and REQ-WEB-005: a WebSocket's binary message is whole
/// packets, the audio ones read as broadcast; any other length is refused
/// whole.
#[test]
fn a_websocket_message_is_packets_and_a_ragged_one_is_refused_whole() {
    let (sound, records) = ima_records();
    let packets = packets(0, 0, &records).0;
    let mut message: Vec<u8> = packets.iter().flat_map(|packet| packet.bytes).collect();
    // A teletext packet among them is left to the decoder.
    message.extend_from_slice(&[0x15; 42]);
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    let mut ragged = message.clone();
    ragged.push(0);
    assert_eq!(receiver.push_message(&ragged, &mut sink), Err(neoteletext_audio::receiver::MessageError::PacketLength));
    assert_eq!((receiver.statistics().messages_refused, receiver.statistics().records), (1, 0));
    assert_eq!(receiver.push_message(&message, &mut sink), Ok(packets.len()));
    assert_eq!(sink.starts(), [(0, SoundId { handle: sound.handle, generation: sound.generation })]);
}

/// Decision E.131: built without `pcm16`, a PCM16 sound is unsupported and
/// skipped; the commands that name it play nothing.
#[cfg(not(feature = "pcm16"))]
#[test]
fn without_pcm16_a_pcm_sound_is_unsupported_and_skipped() {
    let sound = Sound::new(2, 1, Codec::Pcm16, 8_000, 1000, &tone(777)).unwrap();
    let mut arena = vec![0; ARENA];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    for packet in broadcast(&sound, &[play(1, 1, &sound, None)]) {
        receiver.push_packet(&packet, &mut sink);
    }
    assert_eq!(sink.take(), NO_CALLS);
    assert_eq!(receiver.statistics().unsupported, 1);
    assert_eq!(receiver.statistics().records_refused, 0);
}

/// Decision E.124: Opus sounds reach the sink exactly as one decoder fed
/// their packets in order writes them, however the blocks arrive.
#[cfg(feature = "opus")]
mod opus {
    use super::*;
    use neoteletext_audio::record::FLAG_PROGRESSIVE as PROGRESSIVE;
    use neoteletext_opus::{Decoder, FRAME_SAMPLES};

    /// A constant-rate packet's length: 16 kbit/s at 20 ms.
    const PACKET: u16 = 40;
    /// The table of contents of a CELT-only full-band 20 ms mono frame, one a
    /// packet (RFC 6716 section 3.1: configuration 31, code 0).
    const CELT_FULL_BAND_20_MS: u8 = 31 << 3;

    /// `count` packets of `PACKET` bytes made from a seed: the engine holds no
    /// sample stream, and a CELT frame decodes whatever its bytes.
    fn packets(count: usize, seed: u32) -> Vec<u8> {
        let mut state = seed | 1;
        let mut out = Vec::new();
        for _ in 0..count {
            let mut packet = vec![CELT_FULL_BAND_20_MS];
            packet.extend((1..PACKET).map(|_| {
                // xorshift32 (Marsaglia 2003).
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state.to_le_bytes()[0]
            }));
            out.extend(packet);
        }
        out
    }

    /// What one decoder writes for `packets`, `None` a lost one concealed.
    fn decoded(packets: &[Option<&[u8]>]) -> Vec<i16> {
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        for packet in packets {
            let mut pcm = [0_i16; FRAME_SAMPLES];
            match packet {
                Some(packet) => assert!(decoder.decode(packet, &mut pcm).is_ok(), "a made packet decodes"),
                None => decoder.conceal(&mut pcm),
            }
            out.extend_from_slice(&pcm);
        }
        out
    }

    fn assembled(sink: &Recorder, sound: &Sound) -> Vec<i16> {
        let id = SoundId { handle: sound.handle, generation: sound.generation };
        sink.pcm.iter().rev().find(|(held, _)| *held == id).unwrap().1.clone()
    }

    /// REQ-47: a stored sound arriving in reverse order equals one decoder's output, a packet at a time.
    #[test]
    fn a_stored_opus_sound_arriving_out_of_order_is_decoded_in_order() {
        let data = packets(30, 7);
        let samples = 29 * 960 + 500;
        let sound = Sound::opus(5, 1, PACKET, &data, samples).unwrap();
        assert_eq!(sound.blocks(), 30);
        let mut records = vec![sound.object_record().unwrap()];
        let mut blocks = sound.data_records();
        blocks.reverse();
        records.extend(blocks);
        records.push(play(0, 1, &sound, None).record());
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &records).0, &mut sink);
        let expected: Vec<Option<&[u8]>> = data.chunks(usize::from(PACKET)).map(Some).collect();
        let mut want = decoded(&expected);
        want.truncate(samples as usize);
        assert_eq!(assembled(&sink, &sound), want);
        assert_eq!(receiver.statistics().decode_failures, 0);
    }

    /// REQ-47: a progressive play is decoded as far as its blocks have arrived.
    #[test]
    fn a_progressive_opus_sound_is_decoded_as_its_blocks_arrive() {
        let data = packets(60, 11);
        let samples = 60 * 960;
        let sound = Sound::opus(5, 1, PACKET, &data, samples).unwrap();
        let mut start = play(0, 1, &sound, None);
        start.flags = PROGRESSIVE;
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        let announce = [sound.object_record().unwrap(), start.record()];
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &announce).0, &mut sink);
        for record in sound.data_records() {
            push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &[record]).0, &mut sink);
        }
        let expected: Vec<Option<&[u8]>> = data.chunks(usize::from(PACKET)).map(Some).collect();
        assert_eq!(assembled(&sink, &sound), decoded(&expected));
    }

    /// REQ-AUD-013: a stored Opus sound's samples end where its blocks stop
    /// arriving, faded out over 5 ms, and are whole once the next comes.
    #[test]
    fn a_stored_opus_sound_fades_where_its_blocks_stop_and_mends_when_they_come() {
        const FADE_48_KHZ: usize = 240;
        let data = packets(8, 5);
        let samples = 8 * 960;
        let sound = Sound::opus(5, 1, PACKET, &data, samples).unwrap();
        let packet_bytes = usize::from(PACKET);
        let mut start = play(0, 1, &sound, None);
        start.flags = PROGRESSIVE;
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        let mut records = vec![sound.object_record().unwrap(), start.record()];
        // Blocks 0 to 2, then 4 onwards: block 3 is late.
        records.extend(sound.packet_records(0, &data[..3 * packet_bytes]).unwrap());
        records.extend(sound.packet_records(4, &data[4 * packet_bytes..]).unwrap());
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &records).0, &mut sink);
        let expected: Vec<Option<&[u8]>> = data.chunks(packet_bytes).map(Some).collect();
        let want = decoded(&expected);
        let held = assembled(&sink, &sound);
        let edge = 3 * 960;
        assert_eq!(held[..edge - FADE_48_KHZ], want[..edge - FADE_48_KHZ]);
        let length = i32::try_from(FADE_48_KHZ).unwrap();
        for (n, (&got, &whole)) in (0..).zip(held[edge - FADE_48_KHZ..edge].iter().zip(&want[edge - FADE_48_KHZ..edge]))
        {
            let faded = i32::from(whole) * (length - n) / (length + 1);
            assert!(
                (i32::from(got) - faded).abs() <= i32::from(whole).abs() / 200 + 2,
                "sample {n}: {got} for {whole}"
            );
        }
        assert!(held[edge..].iter().all(|&sample| sample == 0), "nothing decoded after the gap");
        assert!(held[edge - 1].abs() <= want[edge - 1].abs() / 100 + 2, "faded to silence");
        push(
            &mut receiver,
            &neoteletext_audio::encode::packets(
                0,
                9,
                &sound.packet_records(3, &data[3 * packet_bytes..4 * packet_bytes]).unwrap(),
            )
            .0,
            &mut sink,
        );
        assert_eq!(assembled(&sink, &sound), want, "whole once block 3 is here");
    }

    /// REQ-AUD-013 (DEC-2470, TODO-1146): when block 3 comes after the voice
    /// has passed the faded end of block 2 into silence, the samples the
    /// voice meets are faded in from where it is, so it does not step from
    /// silence into a sample mid-block. Before the sink was asked where its
    /// voice was, they were written whole.
    #[test]
    fn a_stored_opus_sound_resumed_behind_its_voice_fades_in_where_the_voice_is() {
        const FADE_48_KHZ: usize = 240;
        let data = packets(8, 5);
        let sound = Sound::opus(5, 1, PACKET, &data, 8 * 960).unwrap();
        let packet_bytes = usize::from(PACKET);
        let mut start = play(0, 1, &sound, None);
        start.flags = PROGRESSIVE;
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        let mut records = vec![sound.object_record().unwrap(), start.record()];
        records.extend(sound.packet_records(0, &data[..3 * packet_bytes]).unwrap());
        records.extend(sound.packet_records(4, &data[4 * packet_bytes..]).unwrap());
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &records).0, &mut sink);
        let expected: Vec<Option<&[u8]>> = data.chunks(packet_bytes).map(Some).collect();
        let want = decoded(&expected);
        // The voice is 100 samples into the silence where block 3 belongs.
        let voice = 3 * 960 + 100;
        sink.position = Some(u32::try_from(voice).unwrap());
        let late = sound.packet_records(3, &data[3 * packet_bytes..4 * packet_bytes]).unwrap();
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 9, &late).0, &mut sink);
        let held = assembled(&sink, &sound);
        assert_eq!(held[voice + FADE_48_KHZ..], want[voice + FADE_48_KHZ..], "whole after the fade");
        let length = i32::try_from(FADE_48_KHZ).unwrap();
        for (n, (&got, &whole)) in (0..).zip(held[voice..voice + FADE_48_KHZ].iter().zip(&want[voice..])) {
            let faded = i32::from(whole) * (n + 1) / (length + 1);
            // The ramp is in 16-bit fixed point: within half a per cent.
            assert!(
                (i32::from(got) - faded).abs() <= i32::from(whole).abs() / 200 + 2,
                "sample {n} after the voice: {got} for {whole}, want {faded}"
            );
        }
        assert!(want[voice..voice + 10].iter().any(|&sample| sample.abs() > 200), "the sound is not silent there");
    }

    // Verifies REQ-AUD-005 and REQ-AUD-013 for Opus (TODO-1146 item 3): a
    // block in a refused record is not taken for one at the sink. The
    // decoding in order stops at the refused block and goes on, exactly as
    // one decoder's output, once its good record comes.
    #[test]
    fn an_opus_block_in_a_refused_record_reaches_the_sink_from_its_good_record() {
        let data = packets(6, 13);
        let samples = 6 * 960;
        let sound = Sound::opus(5, 1, PACKET, &data, samples).unwrap();
        let packet_bytes = usize::from(PACKET);
        let packet = |index: usize| &data[index * packet_bytes..(index + 1) * packet_bytes];
        let mut start = play(0, 1, &sound, None);
        start.flags = PROGRESSIVE;
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        let mut records = vec![sound.object_record().unwrap(), start.record()];
        for index in [0, 1, 3, 4, 5] {
            records.extend(sound.packet_records(u16::try_from(index).unwrap(), packet(index)).unwrap());
        }
        // Block 2 followed by two stray bytes.
        let mut bad = packet(2).to_vec();
        bad.extend_from_slice(&[0, 0]);
        records.push(data_record(&sound, 2, &bad));
        let good = sound.packet_records(2, packet(2)).unwrap();
        records.extend(good.iter().cloned());
        records.extend(good);
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &records).0, &mut sink);
        let expected: Vec<Option<&[u8]>> = data.chunks(packet_bytes).map(Some).collect();
        assert_eq!(assembled(&sink, &sound), decoded(&expected));
        assert!(receiver.store().entry(sound.handle).unwrap().complete());
    }

    /// REQ-47: a live sound's lost packet is concealed by the decoder that follows it.
    #[test]
    fn a_lost_live_opus_packet_is_concealed_by_the_decoder() {
        let data = packets(10, 3);
        let packet = |index: usize| &data[index * usize::from(PACKET)..(index + 1) * usize::from(PACKET)];
        let sound = Sound::live(6, 1, Codec::Opus, 48_000, PACKET).unwrap();
        let mut arena = vec![0; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = Recorder::new();
        let announce = [sound.object_record().unwrap(), play(0, 1, &sound, None).record()];
        push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &announce).0, &mut sink);
        // Packet 4 is lost for good.
        for index in (0..10).filter(|&index| index != 4) {
            let records = sound.packet_records(u16::try_from(index).unwrap(), packet(index)).unwrap();
            push(&mut receiver, &neoteletext_audio::encode::packets(0, 0, &records).0, &mut sink);
        }
        let expected: Vec<Option<&[u8]>> = (0..10).map(|index| (index != 4).then(|| packet(index))).collect();
        assert_eq!(assembled(&sink, &sound), decoded(&expected));
        assert_eq!(receiver.statistics().concealed, 1);
    }

    /// REQ-47: the engine carries an encoder's packets and encodes no Opus.
    #[test]
    fn opus_is_carried_as_packets_and_not_encoded_here() {
        assert_eq!(
            Sound::new(1, 1, Codec::Opus, 48_000, PACKET, &tone(960)),
            Err(neoteletext_audio::encode::EncodeError::Encoder)
        );
        let uneven = vec![0_u8; usize::from(PACKET) * 3 + 1];
        assert_eq!(Sound::opus(1, 1, PACKET, &uneven, 960), Err(neoteletext_audio::encode::EncodeError::PacketLength));
        let data = packets(3, 1);
        assert_eq!(
            Sound::opus(1, 1, PACKET, &data, 4 * 960),
            Err(neoteletext_audio::encode::EncodeError::Length),
            "more samples than the packets hold"
        );
        assert_eq!(
            Sound::opus(1, 1, PACKET, &data, 960),
            Err(neoteletext_audio::encode::EncodeError::Length),
            "a packet beyond the sound's end"
        );
    }
}

// Verifies REQ-AUD-003, REQ-AUD-005 and REQ-AUD-021: a record refused for a
// bad block stores nothing, is counted, and the good record completes the
// sound for the play waiting on it.
#[cfg(feature = "pcm16")]
#[test]
fn a_refused_record_leaves_a_play_to_start_on_the_good_one() {
    let pcm = tone(100);
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 100, &pcm).unwrap();
    let bytes = sound.data.clone();
    let mut arena = vec![0_u8; 4_096];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    receiver.push_record(&play(0, 1, &sound, None).record(), &mut sink);
    receiver.push_record(&data_record(&sound, 0, &bytes[..150]), &mut sink);
    assert_eq!(receiver.statistics().records_refused, 1, "the refused record is counted");
    let good = data_record(&sound, 0, &bytes);
    receiver.push_record(&good, &mut sink);
    receiver.push_record(&good, &mut sink);
    assert_eq!(sink.starts().len(), 1);
    assert!(receiver.store().entry(1).unwrap().complete());
}

// Verifies REQ-AUD-005 and REQ-AUD-013: a block in a refused record is not
// taken for one at the sink: its good record reaches the sink, with no hole.
#[cfg(feature = "pcm16")]
#[test]
fn a_block_in_a_refused_record_reaches_the_sink_from_its_good_record() {
    let (sound, records) = level_sound(1_000);
    let mut arena = vec![0_u8; 16_384];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    let mut command = play(0, 1, &sound, None);
    command.flags = FLAG_PROGRESSIVE;
    receiver.push_record(&command.record(), &mut sink);
    receiver.push_record(&records[0], &mut sink);
    receiver.push_record(&records[1], &mut sink);
    assert_eq!(sink.starts().len(), 1);
    receiver.push_record(&records[3], &mut sink);
    sink.take();
    // Block 2 followed by two stray bytes.
    let mut bad = sound.data[2_000..3_000].to_vec();
    bad.extend_from_slice(&[0, 0]);
    receiver.push_record(&data_record(&sound, 2, &bad), &mut sink);
    receiver.push_record(&records[2], &mut sink);
    receiver.push_record(&records[2], &mut sink);
    assert!(
        sink.calls.contains(&Call::Samples(SoundId { handle: sound.handle, generation: sound.generation }, 1_000, 500)),
        "block 2's samples: {:?}",
        sink.calls
    );
    assert!(receiver.store().entry(sound.handle).unwrap().complete());
}

// Verifies REQ-AUD-012 and REQ-AUD-021: a page's play waiting for its sound
// does not start once the user has turned page sounds off.
#[cfg(feature = "pcm16")]
#[test]
fn a_waiting_page_sound_does_not_start_after_the_setting_goes_off() {
    let pcm = tone(100);
    let sound = Sound::new(1, 1, Codec::Pcm16, 8_000, 100, &pcm).unwrap();
    let mut arena = vec![0_u8; 4_096];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.open_page(Some(0x100), &mut sink);
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    receiver.push_record(&play(0, 1, &sound, Some(0x100)).record(), &mut sink);
    receiver.set_page_sounds(false, &mut sink);
    for record in sound.data_records() {
        receiver.push_record(&record, &mut sink);
    }
    assert_eq!(sink.starts(), [], "the setting is off");
    // Turned on again, the armed play starts: the page is still open.
    receiver.set_page_sounds(true, &mut sink);
    assert_eq!(sink.starts().len(), 1);
}

// Verifies REQ-AUD-012: a live page sound that stalls after its page was left
// does not start again on another page, nor with page sounds off.
#[cfg(feature = "pcm16")]
#[test]
fn a_left_pages_stalled_live_sound_does_not_restart() {
    let sound = live_sound();
    let mut arena = vec![0_u8; 32_768];
    let mut sink = Recorder::new();
    for by_opening_another in [true, false] {
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        sink.take();
        receiver.open_page(Some(0x100), &mut sink);
        push(&mut receiver, &broadcast(&sound, &[play(0, 1, &sound, Some(0x100))]), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 0, 2), &mut sink);
        assert_eq!(sink.starts().len(), 1);
        if by_opening_another {
            receiver.open_page(Some(0x200), &mut sink);
        } else {
            receiver.set_page_sounds(false, &mut sink);
        }
        let later = neoteletext_audio::LIVE_STALL_MS + 1;
        receiver.tick(later, &mut sink);
        push(&mut receiver, &live_blocks(&sound, 2, 2), &mut sink);
        assert_eq!(sink.starts().len(), 1, "no second start");
    }
}

// Verifies REQ-AUD-009: an evicted sound the sink holds is forgotten there.
#[cfg(feature = "pcm16")]
#[test]
fn an_evicted_sound_is_forgotten_at_the_sink() {
    let pcm = tone(50);
    let first = Sound::new(1, 1, Codec::Pcm16, 8_000, 100, &pcm).unwrap();
    let second = Sound::new(2, 1, Codec::Pcm16, 8_000, 100, &pcm).unwrap();
    let mut arena = vec![0_u8; 120];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.push_record(&first.object_record().unwrap(), &mut sink);
    for record in first.data_records() {
        receiver.push_record(&record, &mut sink);
    }
    receiver.push_record(&play(0, 1, &first, None).record(), &mut sink);
    receiver.push_record(&stop(0, 2).record(), &mut sink);
    sink.take();
    receiver.push_record(&second.object_record().unwrap(), &mut sink);
    assert!(receiver.store().entry(1).is_none() && receiver.store().entry(2).is_some());
    assert_eq!(sink.take(), [Call::Forget(SoundId { handle: 1, generation: 1 })]);
}

// Verifies REQ-AUD-009 and DEC-2113: a newer generation that does not fit
// leaves the older one in the store, playing at the sink.
#[cfg(feature = "pcm16")]
#[test]
fn a_newer_generation_too_large_leaves_the_older_playing() {
    let older = Sound::new(1, 1, Codec::Pcm16, 8_000, 100, &tone(50)).unwrap();
    let newer = Sound::new(1, 2, Codec::Pcm16, 8_000, 100, &tone(500)).unwrap();
    let mut arena = vec![0_u8; 120];
    let mut receiver = Receiver::<8, 2>::new(&mut arena);
    let mut sink = Recorder::new();
    receiver.push_record(&older.object_record().unwrap(), &mut sink);
    for record in older.data_records() {
        receiver.push_record(&record, &mut sink);
    }
    receiver.push_record(&play(0, 1, &older, None).record(), &mut sink);
    sink.take();
    receiver.push_record(&newer.object_record().unwrap(), &mut sink);
    assert_eq!(receiver.statistics().resource_limits, 1);
    assert_eq!(receiver.store().entry(1).unwrap().sound.generation, 1);
    assert_eq!(sink.take(), []);
}

/// REQ-51 (REQ-AUD-017): a receiver publishes what it holds, and a port
/// checks it against its level's minimum; one record short of Level A's two
/// in flight, or one byte short of its store, does not meet it.
#[test]
fn a_receiver_publishes_its_limits_against_its_levels_minimum() {
    use neoteletext_audio::{LEVEL_A_MINIMUM, LEVEL_B_MINIMUM, ReceiverLimits};
    let mut arena = vec![0; LEVEL_A_MINIMUM.store_bytes];
    let level_a = Receiver::<8, 2>::new(&mut arena).limits();
    assert_eq!(level_a, ReceiverLimits { store_bytes: 16 * 1024, objects: 8, records_in_flight: 2 });
    assert!(level_a.meets(&LEVEL_A_MINIMUM));
    assert!(!level_a.meets(&LEVEL_B_MINIMUM));
    assert!(!Receiver::<8, 1>::new(&mut arena).limits().meets(&LEVEL_A_MINIMUM));
    let mut short = vec![0; LEVEL_A_MINIMUM.store_bytes - 1];
    assert!(!Receiver::<8, 2>::new(&mut short).limits().meets(&LEVEL_A_MINIMUM));
    assert_eq!(LEVEL_B_MINIMUM, ReceiverLimits { store_bytes: 4 * 1024 * 1024, objects: 256, records_in_flight: 8 });
}

/// REQ-AUD-018 (TODO-1146 item 5, DEC-2449): a sink that decodes IMA ADPCM
/// itself is given the blocks undecoded, with their fades, and no samples.
mod ima_to_the_sink {
    use super::*;
    use neoteletext_audio::port::ImaBlock;

    /// What an IMA-decoding sink was given: (first sample or none for a
    /// live block, the block, its samples, fade in, fade out).
    type Given = (Option<u32>, Vec<u8>, u32, bool, bool);

    #[derive(Default)]
    struct DecodingSink {
        given: Vec<Given>,
        samples_calls: usize,
        silences: usize,
    }

    impl VoiceSink for DecodingSink {
        fn define(&mut self, _: SoundId, _: SoundFormat) {}
        fn samples(&mut self, _: SoundId, _: u32, _: &[i16]) {
            self.samples_calls += 1;
        }
        fn forget(&mut self, _: SoundId) {}
        fn end_stream(&mut self, _: u8, _: SoundId) {}
        fn stream(&mut self, _: u8, _: SoundId, _: u32) {}
        fn stream_samples(&mut self, _: u8, _: SoundId, pcm: &[i16]) {
            assert!(pcm.iter().all(|&sample| sample == 0), "only silence comes as samples");
            self.silences += 1;
        }
        fn start(&mut self, _: u8, _: Start) {}
        fn stop(&mut self, _: u8, _: u16) {}
        fn gain(&mut self, _: u8, _: u8, _: u16) {}
        fn decodes_ima(&self) -> bool {
            true
        }
        fn ima_samples(&mut self, _: SoundId, first: u32, block: ImaBlock<'_>) {
            self.given.push((Some(first), block.bytes.to_vec(), block.samples, block.fade_in, block.fade_out));
        }
        fn ima_stream(&mut self, _: u8, _: SoundId, block: ImaBlock<'_>) {
            self.given.push((None, block.bytes.to_vec(), block.samples, block.fade_in, block.fade_out));
        }
    }

    #[test]
    fn a_stored_ima_sound_reaches_a_decoding_sink_as_its_blocks() {
        let pcm = tone(2_000);
        let sound = Sound::new(2, 1, Codec::ImaAdpcm, 8_000, 256, &pcm).unwrap();
        let block = 256;
        let per_block = 505;
        let blocks: Vec<&[u8]> = sound.data.chunks(block).collect();
        assert_eq!(blocks.len(), 4);
        let mut arena = vec![0_u8; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = DecodingSink::default();
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        let mut command = play(0, 1, &sound, None);
        command.flags = FLAG_PROGRESSIVE;
        receiver.push_record(&command.record(), &mut sink);
        let records = sound.data_records();
        // Blocks 0, then 2: block 1 is missing between them, and block 3
        // after, so each fades towards what is missing.
        receiver.push_record(&data_record(&sound, 0, blocks[0]), &mut sink);
        receiver.push_record(&data_record(&sound, 2, blocks[2]), &mut sink);
        assert_eq!(
            sink.given,
            [
                (Some(0), blocks[0].to_vec(), per_block, false, true),
                (Some(2 * per_block), blocks[2].to_vec(), per_block, true, true),
            ]
        );
        sink.given.clear();
        for record in &records {
            receiver.push_record(record, &mut sink);
        }
        assert!(
            sink.given.iter().any(|given| given == &(Some(per_block), blocks[1].to_vec(), per_block, false, false))
        );
        let last = (Some(3 * per_block), blocks[3].to_vec(), 2_000 - 3 * per_block, false, false);
        assert!(sink.given.contains(&last), "the final block keeps the sound's samples: {:?}", sink.given.len());
        assert_eq!(sink.samples_calls, 0, "no samples for a sink that decodes IMA ADPCM");
        // The blocks decode to what the receiver would have sent.
        let mut out = [0_i16; 1_100];
        assert_eq!(decode_block(blocks[1], &mut out).unwrap(), usize::try_from(per_block).unwrap());
    }

    #[test]
    fn a_live_ima_sound_reaches_a_decoding_sink_in_order_and_fades_in_after_a_silence() {
        let sound = Sound::live(7, 1, Codec::ImaAdpcm, 8_000, IMA_BLOCK_SMALL).unwrap();
        let per_block = usize::try_from(sound.samples_per_block()).unwrap();
        let pcm = tone(per_block * 12);
        let mut arena = vec![0_u8; ARENA];
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        let mut sink = DecodingSink::default();
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        receiver.push_record(&play(0, 1, &sound, None).record(), &mut sink);
        // Block 2 is lost for good.
        let mut encoder = LiveEncoder::new(&sound, 0).unwrap();
        for (index, block) in (0_u16..).zip(pcm.chunks(per_block)) {
            let records = encoder.records(block);
            if index != 2 {
                for record in records {
                    receiver.push_record(&record, &mut sink);
                }
            }
        }
        let blocks: Vec<(Option<u32>, bool)> = sink.given.iter().map(|given| (given.0, given.3)).collect();
        assert!(sink.given.len() >= 3 && blocks.iter().all(|&(first, _)| first.is_none()));
        assert_eq!(blocks.iter().filter(|&&(_, faded_in)| faded_in).count(), 1, "block 3 fades in after the silence");
        assert!(sink.given[2].3, "the third block given is block 3, after the silence");
        assert_eq!(sink.silences, 1, "block 2 as silence");
        assert_eq!(sink.samples_calls, 0);
    }
}

/// REQ-AUD-014: a live sound encoded in many calls is the sound encoded in
/// one. Before the live encoder, each call started the IMA ADPCM step index
/// afresh, so a sine wave sent one block a call differed from block 1 on;
/// and a call that ended part way through a block padded it with silence.
#[test]
fn a_live_sound_encoded_in_batches_equals_the_sound_encoded_in_one_call() {
    // A sine wave of about 89 hertz at 8 kilohertz: 2 pi times 89 over
    // 8,000 radians a sample, at a level of 12,000 so the steps adapt.
    let sine = |n: usize| {
        let radians = f64::from(u32::try_from(n).unwrap()) * 2.0 * std::f64::consts::PI * 89.0 / 8_000.0;
        let level = (radians.sin() * 12_000.0).round();
        // Within the range of i16 by the level above.
        #[allow(clippy::cast_possible_truncation, reason = "the level is at most 12,000")]
        let sample = level as i16;
        sample
    };
    for codec in [Codec::ImaAdpcm, Codec::Pcm16] {
        let block = if codec == Codec::ImaAdpcm { IMA_BLOCK_SMALL } else { 100 };
        let sound = Sound::live(7, 1, codec, 8_000, block).unwrap();
        let per_block = usize::try_from(sound.samples_per_block()).unwrap();
        let pcm: Vec<i16> = (0..per_block * 6 + per_block / 3).map(sine).collect();
        let mut whole = LiveEncoder::new(&sound, 40).unwrap();
        let mut one = whole.records(&pcm);
        one.extend(whole.finish());
        // One block a call, and then batches of an awkward length that end
        // part way through blocks.
        for batch in [per_block, per_block / 2 + 7] {
            let mut encoder = LiveEncoder::new(&sound, 40).unwrap();
            let mut many: Vec<Vec<u8>> = pcm.chunks(batch).flat_map(|samples| encoder.records(samples)).collect();
            many.extend(encoder.finish());
            assert_eq!(many, one, "{codec:?} in batches of {batch} samples");
            assert_eq!(encoder.next_block(), 47, "seven blocks numbered from 40");
        }
        // And the blocks decode to the sine wave, each on its own.
        if codec == Codec::ImaAdpcm {
            let header = neoteletext_audio::record::DATA_FIXED;
            let mut out = vec![0_i16; per_block];
            for (record, samples) in one.iter().zip(pcm.chunks(per_block)) {
                let (_, body) = neoteletext_audio::record::split(record).unwrap();
                assert_eq!(decode_block(&body[header..], &mut out).unwrap(), per_block);
                let error: i64 = samples.iter().zip(&out).map(|(&a, &b)| (i64::from(a) - i64::from(b)).abs()).sum();
                assert!(error / i64::try_from(samples.len()).unwrap() < 600, "a mean error of {error}");
            }
        }
    }
}

/// Magazine music (REQ-6925, REQ-6926, REQ-6927 item 3, REQ-6938 items 2
/// and 4): a live sound armed for a magazine, and each page's setting.
#[cfg(feature = "pcm16")]
mod magazine {
    use neoteletext_audio::encode::page_music;
    use neoteletext_audio::receiver::MagazineMusic;
    use neoteletext_audio::record::{Control, MusicSetting, Operation, PageMusic, Record, TYPE_CONTROL, parse};
    use neoteletext_audio::{BASE_PROFILE_VERSION, DUCK_PERCENT, MOST_PAGE_MUSIC_SETTINGS, PROFILE_VERSION};

    use super::*;

    const ID: SoundId = SoundId { handle: 4, generation: 1 };

    /// A play of `sound` for magazine `magazine`.
    fn magazine_play(channel: u8, sequence: u16, sound: &Sound, magazine: u8) -> Command {
        Command { magazine: Some(magazine), ..play(channel, sequence, sound, None) }
    }

    /// A receiver on a connection that agreed magazine music, with the live
    /// sound announced, armed for magazine 1 on channel 0, and `open` open.
    fn armed<'a>(arena: &'a mut [u8], sink: &mut Recorder, open: Option<u16>) -> (Receiver<'a, 8, 2>, Sound) {
        let sound = live_sound();
        let mut receiver = Receiver::<8, 2>::new(arena);
        receiver.set_magazine_sound(true, sink);
        receiver.open_page(open, sink);
        receiver.push_record(&sound.object_record().unwrap(), sink);
        receiver.push_record(&magazine_play(0, 1, &sound, 1).record(), sink);
        (receiver, sound)
    }

    fn setting(page: u16, sub: u16, setting: MusicSetting) -> Vec<u8> {
        page_music(&PageMusic { page, sub, setting })
    }

    fn stops(calls: &[Call]) -> Vec<&Call> {
        calls.iter().filter(|call| matches!(call, Call::Stop(..))).collect()
    }

    fn start_count(calls: &[Call]) -> usize {
        calls.iter().filter(|call| matches!(call, Call::Start(..))).count()
    }

    // REQ-6925 items 3, 4 and 6; REQ-6926 item 6: armed by the server's
    // play, started when a page of the magazine is open, unbroken across
    // 101, 150 and 199, faded within the ramp on 200, and started again at
    // the stream's present block on returning.
    #[test]
    fn magazine_music_plays_across_its_magazines_pages_and_fades_on_leaving() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, None);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        assert_eq!(sink.take(), NO_CALLS, "armed, and no page of the magazine is open");
        assert_eq!(receiver.magazine_music(0), Some(MagazineMusic { magazine: 1, heard: false }));

        receiver.open_page(Some(0x101), &mut sink);
        let calls = sink.take();
        // Heard, a stream labelled before its first sample, then the start.
        assert_eq!(calls[0], Call::Heard(0, 1, true));
        assert_eq!(calls[1], Call::Stream(ID, 8_000));
        assert_eq!(calls[2], Call::StreamMagazine(0, ID, Some(1)));
        assert!(matches!(calls[3], Call::StreamSamples(..)), "{calls:?}");
        assert!(
            matches!(calls.last(), Some(Call::Start(0, Start { sound, magazine: Some(1), music_percent: 100, .. })) if *sound == ID),
            "{calls:?}"
        );
        assert_eq!(receiver.magazine_music(0), Some(MagazineMusic { magazine: 1, heard: true }));

        // Across the magazine's pages nothing stops, starts or is labelled
        // again, and the blocks go on in order.
        for (page, block) in [(0x150, 12), (0x199, 13), (0x1FF, 14)] {
            receiver.open_page(Some(page), &mut sink);
            push(&mut receiver, &live_blocks(&sound, block, 1), &mut sink);
            let calls = sink.take();
            assert!(calls.iter().all(|call| matches!(call, Call::StreamSamples(..))), "{page:X}: {calls:?}");
        }
        assert_eq!(live_values(&sink, &sound), [11, 12, 13, 14]);

        // Another magazine: faded out over the 5 ms ramp, and told.
        receiver.open_page(Some(0x200), &mut sink);
        assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS), Call::Heard(0, 1, false)]);
        assert_eq!(receiver.magazine_music(0), Some(MagazineMusic { magazine: 1, heard: false }));
        // No page at all is not the magazine either.
        receiver.open_page(None, &mut sink);
        assert_eq!(sink.take(), NO_CALLS);

        // The stream goes on meanwhile; on returning the voice starts again
        // on the same stream, which the sink joins at what is written now
        // (`neoteletext-audio-mix`), and no block is given twice.
        push(&mut receiver, &live_blocks(&sound, 15, 3), &mut sink);
        sink.take();
        receiver.open_page(Some(0x150), &mut sink);
        let calls = sink.take();
        assert_eq!(calls[0], Call::Heard(0, 1, true));
        assert_eq!(start_count(&calls), 1);
        assert!(!calls.iter().any(|call| matches!(call, Call::Stream(..) | Call::StreamMagazine(..))), "{calls:?}");
        push(&mut receiver, &live_blocks(&sound, 18, 1), &mut sink);
        assert_eq!(live_values(&sink, &sound), [11, 12, 13, 14, 15, 16, 17, 18]);
    }

    // REQ-6925 item 3: after the magazine was left long enough for the
    // sender to stop its blocks, returning starts at the block the stream
    // has reached, never at the block where it stopped.
    #[test]
    fn returning_to_the_magazine_resumes_at_the_streams_present_block() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, Some(0x101));
        push(&mut receiver, &live_blocks(&sound, 10, 3), &mut sink);
        assert_eq!(live_values(&sink, &sound), [10, 11, 12]);
        receiver.open_page(Some(0x200), &mut sink);
        receiver.tick(neoteletext_audio::LIVE_STALL_MS + 1, &mut sink);
        sink.take();
        // Back on the magazine: the play is sent again, then blocks from
        // where the stream is now.
        receiver.open_page(Some(0x150), &mut sink);
        receiver.push_record(&magazine_play(0, 2, &sound, 1).record(), &mut sink);
        assert_eq!(start_count(&sink.take()), 0, "no block has come yet");
        push(&mut receiver, &live_blocks(&sound, 500, 2), &mut sink);
        let calls = sink.take();
        assert_eq!(start_count(&calls), 1, "{calls:?}");
        // A new stream, labelled before its samples.
        let stream = calls.iter().position(|call| *call == Call::Stream(ID, 8_000)).unwrap();
        assert_eq!(calls[stream + 1], Call::StreamMagazine(0, ID, Some(1)));
        assert_eq!(live_values(&sink, &sound), [500, 501]);
    }

    // REQ-6925 item 2: a magazine play of a stored sound is refused, whether
    // the sound is known when the play comes or after.
    #[test]
    fn a_magazine_play_of_a_stored_sound_is_refused() {
        let stored = Sound::new(1, 1, Codec::Pcm16, 8_000, 100, &tone(50)).unwrap();
        for sound_first in [true, false] {
            let mut arena = vec![0; ARENA];
            let mut sink = Recorder::new();
            let mut receiver = Receiver::<8, 2>::new(&mut arena);
            receiver.set_magazine_sound(true, &mut sink);
            receiver.open_page(Some(0x101), &mut sink);
            let mut records = vec![stored.object_record().unwrap()];
            records.extend(stored.data_records());
            let play = magazine_play(0, 1, &stored, 1).record();
            if sound_first {
                records.push(play);
            } else {
                records.insert(0, play);
            }
            for record in &records {
                receiver.push_record(record, &mut sink);
            }
            assert_eq!(sink.starts(), NO_STARTS);
            assert_eq!(receiver.statistics().magazine_refused, 1);
            assert_eq!(receiver.magazine_music(0), None, "disarmed");
        }
    }

    // REQ-6925 item 5: with the user's page sounds off no magazine music is
    // heard, and turning them on while on the magazine starts it.
    #[test]
    fn page_sounds_off_keeps_the_magazine_silent() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let sound = live_sound();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.set_magazine_sound(true, &mut sink);
        receiver.set_page_sounds(false, &mut sink);
        receiver.open_page(Some(0x101), &mut sink);
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        receiver.push_record(&magazine_play(0, 1, &sound, 1).record(), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        assert_eq!(sink.take(), NO_CALLS);
        receiver.set_page_sounds(true, &mut sink);
        assert_eq!(sink.starts(), [(0, ID)]);
        assert_eq!(live_values(&sink, &sound), [11], "from the newest block");
        receiver.set_page_sounds(false, &mut sink);
        assert_eq!(stops(&sink.calls), [&Call::Stop(0, MIN_FADE_MS)]);
    }

    // REQ-6925 item 4: a stop of the channel disarms the music, and nothing
    // starts it again.
    #[test]
    fn a_stop_disarms_magazine_music() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, Some(0x101));
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        assert_eq!(sink.starts().len(), 1);
        sink.take();
        receiver.push_record(&stop(0, 2).record(), &mut sink);
        assert!(matches!(sink.take()[..], [Call::Stop(0, _), Call::Heard(0, 1, false)]));
        assert_eq!(receiver.magazine_music(0), None);
        receiver.open_page(Some(0x200), &mut sink);
        receiver.open_page(Some(0x101), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 12, 2), &mut sink);
        assert_eq!(sink.starts(), NO_STARTS);
    }

    /// A CONTROL record's 14 bytes with page field `scope`, framed as
    /// profile version `version`.
    fn control_record(version: u8, operation: u8, scope: u16) -> Vec<u8> {
        let mut body = vec![0, 0, 1, operation, 4, 0, 1, 80, 0, 0, 0, 0];
        body.extend_from_slice(&scope.to_be_bytes());
        neoteletext_audio::encode::frame_version(version, TYPE_CONTROL, &body)
    }

    // REQ-6925 items 1 and 7, DEC-6951: 0xFF01 to 0xFF08 name magazines 1
    // to 8 in a play of profile version 3; 0xFF00 and 0xFF09 are refused; a
    // record of version 2 refuses every one, as an older receiver does; a
    // stop or a set gain names no magazine.
    #[test]
    fn the_magazine_scope_is_read_and_its_bounds_are_refused() {
        for magazine in 1..=8_u8 {
            let record = control_record(PROFILE_VERSION, 1, 0xFF00 + u16::from(magazine));
            let Ok(Record::Control(control)) = parse(&record) else { panic!("magazine {magazine}") };
            assert_eq!((control.magazine, control.page), (Some(magazine), None));
            assert_eq!(control.operation, Operation::Play);
        }
        for scope in [0xFF00, 0xFF09, 0xFF10, 0xFFFE] {
            assert!(parse(&control_record(PROFILE_VERSION, 1, scope)).is_err(), "{scope:04X}");
        }
        assert!(parse(&control_record(BASE_PROFILE_VERSION, 1, 0xFF01)).is_err(), "profile 2 has no magazine");
        for operation in [2, 3] {
            assert!(parse(&control_record(PROFILE_VERSION, operation, 0xFF01)).is_err(), "operation {operation}");
        }
        // What the encoder writes is what the reader reads, and only the
        // magazine's form carries version 3.
        let sound = live_sound();
        let written = magazine_play(2, 9, &sound, 8).record();
        assert_eq!(written[0], PROFILE_VERSION);
        assert_eq!(written[written.len() - 6..written.len() - 4], [0xFF, 0x08]);
        assert_eq!(play(2, 9, &sound, Some(0x123)).record()[0], BASE_PROFILE_VERSION);
        assert_eq!(sound.object_record().unwrap()[0], BASE_PROFILE_VERSION);
        // A refused record counts and moves nothing.
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.set_magazine_sound(true, &mut sink);
        receiver.push_record(&control_record(PROFILE_VERSION, 1, 0xFF09), &mut sink);
        assert_eq!(receiver.statistics().records_refused, 1);
        assert_eq!(receiver.magazine_music(0), None);
    }

    // REQ-6927 item 3, REQ-6938 item 2, DEC-6953 item 5: where the
    // capability was not agreed, magazine music and a page's setting are
    // refused and counted, start nothing and move no channel's sequence.
    #[test]
    fn magazine_music_is_refused_where_its_capability_was_not_agreed() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let sound = live_sound();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.open_page(Some(0x101), &mut sink);
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        receiver.push_record(&magazine_play(0, 5, &sound, 1).record(), &mut sink);
        receiver.push_record(&setting(0x101, 0, MusicSetting::Silent), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        assert_eq!(sink.take(), NO_CALLS);
        assert_eq!(receiver.statistics().magazine_refused, 2);
        assert_eq!(receiver.magazine_music(0), None);
        // Sequence 5 was not taken: an older command of the channel is
        // still accepted.
        receiver.push_record(&play(0, 3, &sound, None).record(), &mut sink);
        assert_eq!(sink.starts(), [(0, ID)]);
        assert_eq!(receiver.statistics().stale_commands, 0);

        // Agreed, then no longer: the music stops and is disarmed.
        let mut arena = vec![0; ARENA];
        let (mut receiver, sound) = armed(&mut arena, &mut sink, Some(0x101));
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        sink.take();
        receiver.set_magazine_sound(false, &mut sink);
        assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS), Call::Heard(0, 1, false)]);
        assert_eq!(receiver.magazine_music(0), None);
    }

    // REQ-6938 item 2: a rule's command cannot start magazine music.
    #[test]
    fn a_rules_command_naming_a_magazine_is_refused() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let sound = live_sound();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.set_magazine_sound(true, &mut sink);
        receiver.open_page(Some(0x101), &mut sink);
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        let Ok(Record::Control(control)) = parse(&magazine_play(0, 1, &sound, 1).record()) else { panic!() };
        assert_eq!(control, Control { magazine: Some(1), ..control });
        receiver.rule_command(&control, &mut sink);
        assert_eq!(sink.take(), NO_CALLS);
        assert_eq!(receiver.statistics().magazine_refused, 1);
        assert_eq!(receiver.magazine_music(0), None);
    }

    // REQ-6926 items 1, 2, 5 and 6: a silent page fades the music within
    // the ramp and a playing page brings it back, on the stream as it is
    // now; a duck page holds it at exactly Table 19's step.
    #[test]
    fn a_pages_setting_silences_ducks_and_returns_the_music() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, Some(0x101));
        receiver.push_record(&setting(0x102, 0, MusicSetting::Silent), &mut sink);
        receiver.push_record(&setting(0x103, 0, MusicSetting::Duck(2)), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        assert_eq!(start_count(&sink.take()), 1);

        receiver.open_page(Some(0x102), &mut sink);
        assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS), Call::Heard(0, 1, false)]);
        // The stream goes on under the silent page.
        push(&mut receiver, &live_blocks(&sound, 12, 2), &mut sink);
        assert!(sink.take().iter().all(|call| matches!(call, Call::StreamSamples(..))));

        // To the duck page: started at 40 per cent, duck code 2's 8 dB.
        assert_eq!(DUCK_PERCENT[2], 40);
        receiver.open_page(Some(0x103), &mut sink);
        let calls = sink.take();
        assert_eq!(calls[0], Call::Heard(0, 1, true));
        assert!(matches!(calls[1], Call::Start(0, Start { music_percent: 40, magazine: Some(1), .. })), "{calls:?}");
        assert_eq!(calls.len(), 2);

        // To a playing page and back: the level moves over the ramp, and
        // the voice is neither stopped nor started.
        receiver.open_page(Some(0x101), &mut sink);
        assert_eq!(sink.take(), [Call::MusicLevel(0, 100, MIN_FADE_MS)]);
        receiver.open_page(Some(0x103), &mut sink);
        assert_eq!(sink.take(), [Call::MusicLevel(0, 40, MIN_FADE_MS)]);

        // The setting of the open page changes under it.
        receiver.push_record(&setting(0x103, 0, MusicSetting::Duck(6)), &mut sink);
        assert_eq!(sink.take(), [Call::MusicLevel(0, 3, MIN_FADE_MS)]);
        receiver.push_record(&setting(0x103, 0, MusicSetting::Silent), &mut sink);
        assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS), Call::Heard(0, 1, false)]);
        receiver.push_record(&setting(0x103, 0, MusicSetting::Play), &mut sink);
        let calls = sink.take();
        assert!(matches!(calls[1], Call::Start(0, Start { music_percent: 100, .. })), "{calls:?}");
        push(&mut receiver, &live_blocks(&sound, 14, 1), &mut sink);
        assert_eq!(live_values(&sink, &sound), [10, 11, 12, 13, 14], "every block once, in order");
    }

    // REQ-6926 item 3 (DEC-6599): two sub-pages of one page each apply
    // their own setting, and nothing is inherited from another sub-page.
    #[test]
    fn two_sub_pages_of_one_page_each_apply_their_own_setting() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, None);
        receiver.push_record(&setting(0x101, 0x0001, MusicSetting::Silent), &mut sink);
        receiver.push_record(&setting(0x101, 0x0002, MusicSetting::Duck(3)), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        receiver.open(Some(0x101), 0x0001, &mut sink);
        assert_eq!(sink.take(), NO_CALLS, "silent");
        receiver.open(Some(0x101), 0x0002, &mut sink);
        assert!(
            matches!(sink.take().last(), Some(Call::Start(0, Start { music_percent: 25, .. }))),
            "duck code 3 is 25 per cent"
        );
        // Sub-code 0003 was sent no setting, and 0000 is another sub-page
        // again: both play.
        receiver.open(Some(0x101), 0x0003, &mut sink);
        assert_eq!(sink.take(), [Call::MusicLevel(0, 100, MIN_FADE_MS)]);
        receiver.open(Some(0x101), 0x0001, &mut sink);
        assert_eq!(sink.take(), [Call::Stop(0, MIN_FADE_MS), Call::Heard(0, 1, false)]);
        receiver.open(Some(0x101), 0x0000, &mut sink);
        assert_eq!(start_count(&sink.take()), 1);
        // Another page with the same sub-code inherits nothing either.
        receiver.open(Some(0x102), 0x0001, &mut sink);
        assert_eq!(sink.take(), NO_CALLS, "it plays on");
    }

    // REQ-6926 item 2: a setting starts nothing where no music is armed,
    // nor on a page of another magazine than the armed one.
    #[test]
    fn a_setting_starts_no_sound() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let sound = live_sound();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.set_magazine_sound(true, &mut sink);
        receiver.open_page(Some(0x101), &mut sink);
        receiver.push_record(&sound.object_record().unwrap(), &mut sink);
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        for music in [MusicSetting::Play, MusicSetting::Duck(1), MusicSetting::Silent, MusicSetting::Play] {
            receiver.push_record(&setting(0x101, 0, music), &mut sink);
        }
        assert_eq!(sink.take(), NO_CALLS);
        receiver.push_record(&magazine_play(0, 1, &sound, 2).record(), &mut sink);
        receiver.push_record(&setting(0x101, 0, MusicSetting::Duck(1)), &mut sink);
        assert_eq!(sink.take(), NO_CALLS, "magazine 2's music on a page of magazine 1");
    }

    // REQ-6926 item 1, REQ-6938 item 4, DEC-6952 item 1: every field of the
    // record is bounded, and a record outside a bound is refused whole.
    #[test]
    fn a_malformed_setting_is_refused() {
        let framed = |version: u8, body: &[u8]| {
            neoteletext_audio::encode::frame_version(version, neoteletext_audio::record::TYPE_MAGAZINE_MUSIC, body)
        };
        let good = [0x01, 0x50, 0x00, 0x02, 3];
        assert_eq!(
            parse(&framed(PROFILE_VERSION, &good)),
            Ok(Record::PageMusic(PageMusic { page: 0x150, sub: 2, setting: MusicSetting::Duck(3) }))
        );
        assert_eq!(setting(0x150, 2, MusicSetting::Duck(3)), framed(PROFILE_VERSION, &good));
        for (bad, why) in [
            (&[0x01, 0x50, 0x00, 0x02, 7][..], "duck code 7"),
            (&[0x01, 0x50, 0x00, 0x02, 0xFE], "an unknown setting"),
            (&[0x00, 0x99, 0x00, 0x02, 0], "a page below 100"),
            (&[0x09, 0x00, 0x00, 0x02, 0], "a page above 8FF"),
            (&[0x01, 0x50, 0x3F, 0x7F, 0], "the sub-code that means any"),
            (&[0x01, 0x50, 0x80, 0x00, 0], "a bit no sub-code has"),
            (&[0x01, 0x50, 0x00, 0x02], "too short"),
            (&[0x01, 0x50, 0x00, 0x02, 0, 0], "too long"),
        ] {
            assert!(parse(&framed(PROFILE_VERSION, bad)).is_err(), "{why}");
        }
        assert!(parse(&framed(BASE_PROFILE_VERSION, &good)).is_err(), "profile 2 has no such record");
        assert!(setting(0x150, 2, MusicSetting::Duck(7)).is_empty(), "never written");
        assert_eq!(MusicSetting::from_code(0), Some(MusicSetting::Play));
        assert_eq!(MusicSetting::from_code(0xFF), Some(MusicSetting::Silent));
        // Refused by the receiver, and counted.
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let mut receiver = Receiver::<8, 2>::new(&mut arena);
        receiver.set_magazine_sound(true, &mut sink);
        receiver.push_record(&framed(PROFILE_VERSION, &[0x01, 0x50, 0x00, 0x02, 7]), &mut sink);
        assert_eq!(receiver.statistics().records_refused, 1);
    }

    // DEC-6952 item 4, REQ-6938 item 4: the receiver keeps a bounded number
    // of settings; the oldest gives way, never the open page's.
    #[test]
    fn the_settings_kept_are_bounded_and_the_open_pages_is_never_dropped() {
        let mut arena = vec![0; ARENA];
        let mut sink = Recorder::new();
        let (mut receiver, sound) = armed(&mut arena, &mut sink, Some(0x101));
        push(&mut receiver, &live_blocks(&sound, 10, 2), &mut sink);
        receiver.push_record(&setting(0x101, 0, MusicSetting::Silent), &mut sink);
        receiver.push_record(&setting(0x102, 0, MusicSetting::Silent), &mut sink);
        // Twice as many more as are kept.
        for page in 0..u16::try_from(MOST_PAGE_MUSIC_SETTINGS * 2).unwrap() {
            receiver.push_record(&setting(0x110 + page, 0, MusicSetting::Silent), &mut sink);
        }
        sink.take();
        // The open page's is still held: nothing started.
        receiver.push_record(&setting(0x1FF, 0, MusicSetting::Silent), &mut sink);
        assert_eq!(sink.take(), NO_CALLS);
        // Page 102's, the oldest but the open one's, gave way: it plays.
        receiver.open_page(Some(0x102), &mut sink);
        assert_eq!(start_count(&sink.take()), 1);
        // The newest is held.
        receiver.open_page(Some(0x1FF), &mut sink);
        assert_eq!(stops(&sink.take()).len(), 1);
    }
}
