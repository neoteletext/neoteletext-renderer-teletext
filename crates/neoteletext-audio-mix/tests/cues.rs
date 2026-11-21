//! Cue points from a stored sound's blocks to the moment they are heard
//! (neoteletext-common `docs/page-rules.md`, RULE-109; REQ-6614): the audio
//! receiver renders a synthesizer's sound ahead of what is heard and marks
//! each cue at its place in the stream; the mixer reports each when a voice
//! plays its sample.
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_wrap,
    clippy::print_stdout,
    reason = "a test reports a failure by panicking, on data it built to a known size; its counts are small; a measurement prints its figures"
)]

use neoteletext_audio::Codec;
use neoteletext_audio::encode::{Command, Sound, TimedCue, TimedEvent, event_blocks_with_cues};
use neoteletext_audio::midi::{ChannelMessage, Message, status};
use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::FLAG_LOOP;
use neoteletext_audio_mix::{HeardCue, Mixer};

/// The device's rate, which is the stream's: a sample of one is a frame
/// of the other, but for the mixer's trim.
const RATE: u32 = 48_000;
/// The audio channel the sound plays on.
const CHANNEL: u8 = 1;
/// The sound's handle.
const HANDLE: u8 = 4;
/// The handle of the bank the sound names.
const BANK: u8 = 9;
/// The sound's blocks: 1.6 seconds.
const BLOCKS: u16 = 80;
/// The cues: a tick of 0.1 ms from the sound's start, and a number.
const CUES: [(u64, u8); 3] = [(0, 7), (2_500, 0), (9_000, 127)];
/// Frames of a page tick, a sixtieth of a second.
const TICK_FRAMES: i64 = RATE as i64 / 60;
/// The furthest the mixer's trim may move a sample from its nominal
/// frame over this sound: its rate follows the stream's buffer by at
/// most 5 per cent (`STREAM_TRIM_PPM`). Measured here on 18 November
/// 2026: a cue is reported at most 64 frames past its buffer.
const TRIM_FRAMES: i64 = 480;

/// A sink that plays the receiver's streams through a mixer, as a port's
/// sound output does.
struct Output {
    mixer: Mixer,
    /// The marks given: position and number, with the frames rendered then.
    marked: Vec<(u64, u8, usize)>,
    /// Frames rendered.
    frames: usize,
}

fn key(sound: SoundId) -> (u8, u16) {
    (sound.handle, sound.generation)
}

impl VoiceSink for Output {
    fn define(&mut self, _: SoundId, _: SoundFormat) {}
    fn samples(&mut self, _: SoundId, _: u32, _: &[i16]) {}
    fn forget(&mut self, sound: SoundId) {
        self.mixer.forget(key(sound));
    }
    fn stream(&mut self, _: u8, sound: SoundId, rate_hz: u32) {
        self.mixer.stream(key(sound), rate_hz);
    }
    fn stream_samples(&mut self, _: u8, sound: SoundId, pcm: &[i16]) {
        self.mixer.stream_samples(key(sound), pcm);
    }
    fn stream_cue(&mut self, _: u8, sound: SoundId, position: u64, number: u8) {
        self.marked.push((position, number, self.frames));
        assert!(self.mixer.stream_cue(key(sound), position, number));
    }
    fn end_stream(&mut self, _: u8, sound: SoundId) {
        self.mixer.forget(key(sound));
    }
    fn start(&mut self, channel: u8, start: Start) {
        self.mixer.start(
            usize::from(channel),
            neoteletext_audio_mix::Start {
                key: key(start.sound),
                gain: start.gain,
                fade_ms: start.fade_ms,
                looped: false,
                duck_percent: start.duck_percent,
                after_current: start.after_current,
            },
        );
    }
    fn stop(&mut self, channel: u8, fade_ms: u16) {
        self.mixer.stop(usize::from(channel), fade_ms);
    }
    fn gain(&mut self, channel: u8, gain: u8, fade_ms: u16) {
        self.mixer.gain(usize::from(channel), gain, fade_ms);
    }
}

/// The stored sound: a note at its start and the cues.
fn sound() -> Sound {
    let note = TimedEvent { tick: 0, message: Message::Channel(ChannelMessage::new(status::NOTE_ON, 0, 69, 127)) };
    let cues: Vec<TimedCue> = CUES.iter().map(|&(tick, number)| TimedCue { tick, number }).collect();
    let blocks = event_blocks_with_cues(&[note], &cues, BLOCKS).unwrap();
    let mut sound = Sound::events(HANDLE, 1, Codec::Synth7, &blocks).unwrap();
    sound.banks = vec![BANK];
    sound
}

/// The sound's bank: an image that is no bulk dump, so that every program
/// plays the family's default patch (MIDI-61), which is all the test needs
/// to hear.
fn bank() -> Sound {
    Sound::bank(BANK, 1, Codec::Synth7, &[0]).unwrap()
}

fn command(sequence: u16, operation: u8, flags: u8, fade_ms: u16) -> Vec<u8> {
    Command {
        channel: CHANNEL,
        sequence,
        operation,
        handle: HANDLE,
        generation: 1,
        gain: 100,
        fade_ms,
        flags,
        duck: 0,
        page: None,
        music: None,
    }
    .record()
}

/// What a playing was heard to do.
struct Heard {
    /// Every frame rendered.
    out: Vec<i16>,
    /// Each cue reported, with the frames rendered when it was.
    cues: Vec<(usize, HeardCue)>,
    /// Each mark the receiver gave: position, number, frames rendered then.
    marked: Vec<(u64, u8, usize)>,
}

/// Plays the sound in device buffers of `frames`, the receiver ticked when
/// it asks by the device's clock, for `seconds`; `looped` plays it round,
/// and `stop_at` stops it, with a fade of a second, once that many frames
/// are rendered.
fn play(frames: usize, seconds: usize, looped: bool, stop_at: Option<usize>) -> Heard {
    let mut arena = vec![0_u8; 128 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut output = Output { mixer: Mixer::new(RATE), marked: Vec::new(), frames: 0 };
    let sound = sound();
    for sound in [&bank(), &sound] {
        receiver.push_record(&sound.object_record().unwrap(), &mut output);
        for record in sound.data_records() {
            receiver.push_record(&record, &mut output);
        }
    }
    receiver.push_record(&command(1, 1, if looped { FLAG_LOOP } else { 0 }, 0), &mut output);
    let mut heard = Heard { out: Vec::new(), cues: Vec::new(), marked: Vec::new() };
    let mut stopped = false;
    while output.frames < seconds * RATE as usize {
        let now_ms = output.frames as u64 * 1_000 / u64::from(RATE);
        if receiver.next_tick_ms().is_some_and(|at| at <= now_ms) {
            receiver.tick(now_ms, &mut output);
        }
        if stop_at.is_some_and(|at| output.frames >= at) && !stopped {
            receiver.tick(now_ms, &mut output);
            receiver.push_record(&command(2, 2, 0, 1_000), &mut output);
            stopped = true;
        }
        let mut buffer = vec![0; frames];
        output.mixer.render(&mut buffer);
        heard.out.extend_from_slice(&buffer);
        output.frames += frames;
        let rendered = output.frames;
        output.mixer.take_heard_cues(&mut |cue| heard.cues.push((rendered, cue)));
    }
    heard.marked = output.marked;
    heard
}

// Verifies RULE-109 (REQ-6614 item 3): a stored event-list sound with three
// cues reports each once, in order, at the tick it is heard and not when
// it is rendered ahead, with device buffers of several sizes. The note at
// the sound's start is heard at the first frame that is not silence, so a
// cue at sample `s` of the stream is heard `s` frames after it, but for the
// mixer's trim; each cue is reported in the buffer that ends within one
// buffer and one page tick after that, and the receiver had marked it well
// before.
#[test]
fn a_stored_sounds_cues_are_each_reported_once_when_heard() {
    println!("frames  cue  marked-ahead-of-heard(ms)  reported-after-heard(frames)");
    for frames in [128, 256, 480, 1_024, 2_048] {
        let heard = play(frames, 3, false, None);
        let told: Vec<(usize, (u8, u16), u8)> =
            heard.cues.iter().map(|(_, cue)| (cue.voice, cue.key, cue.number)).collect();
        let expected: Vec<(usize, (u8, u16), u8)> =
            CUES.iter().map(|&(_, number)| (usize::from(CHANNEL), (HANDLE, 1), number)).collect();
        assert_eq!(told, expected, "{frames} frames: each once, in order");
        let onset = heard.out.iter().position(|&sample| sample != 0).unwrap() as i64;
        for ((&(tick, number), &(reported, _)), &(position, _, marked_at)) in
            CUES.iter().zip(&heard.cues).zip(&heard.marked)
        {
            assert_eq!(position, tick * 24 / 5, "the mark's place in the stream");
            let heard_at = onset + position as i64;
            let late = reported as i64 - heard_at;
            let ahead_ms = (heard_at - marked_at as i64) * 1_000 / i64::from(RATE);
            println!("{frames:6}  {number:3}  {ahead_ms:25}  {late:28}");
            assert!(
                (-TRIM_FRAMES..=frames as i64 + TICK_FRAMES + TRIM_FRAMES).contains(&late),
                "{frames} frames: cue {number} heard at frame {heard_at}, reported at {reported}"
            );
            // Rendered ahead: the mark came at least the receiver's least
            // lead before its sample was heard.
            assert!(ahead_ms >= 100, "{frames} frames: cue {number} marked only {ahead_ms} ms ahead");
        }
    }
}

// Verifies RULE-109: a looped sound reports its cues on every pass, in
// order; once it is stopped no further cue is reported, though marks for
// what was rendered ahead were waiting and its fade plays on.
#[test]
fn a_stopped_sound_reports_no_further_cue() {
    let pass = usize::from(BLOCKS) * 960;
    // Stopped between the second pass's first cue being heard, some 0.3 s
    // into the pass by the stream's buffer, and its second, 0.25 s later.
    let stop_at = pass + 48_000 * 4 / 10;
    let heard = play(256, 5, true, Some(stop_at));
    let numbers: Vec<u8> = heard.cues.iter().map(|(_, cue)| cue.number).collect();
    assert_eq!(numbers, vec![7, 0, 127, 7], "{:?}", heard.cues);
    assert!(heard.cues.iter().all(|&(reported, _)| reported <= stop_at + 256));
    // The second pass's cues were marked before the stop, ahead of what
    // was heard, and forgotten by it.
    let waiting = heard.marked.iter().filter(|&&(_, _, at)| at <= stop_at).count();
    assert!(waiting > numbers.len(), "{waiting} marks before the stop");
    // The fade was heard after the stop.
    assert!(heard.out[stop_at + 4_800..stop_at + 9_600].iter().any(|&sample| sample != 0));
}
