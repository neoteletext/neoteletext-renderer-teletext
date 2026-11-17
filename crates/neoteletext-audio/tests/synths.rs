//! The analogue synthesizers end to end (DEC-6429 foundations item 4): a
//! sound of the Juno, the Minimoog, the Prophet-5, the Jupiter-8 or the
//! CS-80 and its patch bank, written by the publishing encoder, read back
//! by the receiver, rendered by the instrument and given to a recording
//! sink as samples.
#![cfg(all(
    feature = "encode",
    feature = "juno",
    feature = "minimoog",
    feature = "prophet5",
    feature = "jupiter8",
    feature = "cs80"
))]
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, on data it built to a known size"
)]

use neoteletext_audio::encode::{Command, Sound, frame};
use neoteletext_audio::port::{SoundFormat, SoundId, Start, VoiceSink};
use neoteletext_audio::receiver::Receiver;
use neoteletext_audio::record::{
    JUPITER8_KEYS_DUAL, JUPITER8_KEYS_SPLIT, JUPITER8_KEYS_WHOLE, Record, TAG_BANK_FAMILY, TAG_DX7_BANK,
    TAG_JUPITER8_KEYS, TAG_MODEL, TYPE_CONTROL, TYPE_DATA, TYPE_OBJECT, parse,
};
use neoteletext_audio::{Codec, INSTRUMENT_BLOCK_SAMPLES, INSTRUMENT_RATE, PATCH_BANK_PATCHES};
use neoteletext_audio_instruments::{cs80, juno, jupiter8, minimoog, prophet5};

/// The samples of a block, as a length.
const BLOCK: usize = INSTRUMENT_BLOCK_SAMPLES as usize;
/// The handle the tests' banks have.
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
        // A sink is given only samples of the sound it was told of, but a
        // test of random records must not fail on the recorder itself.
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

/// One family of the synthesizers, as the tests need it.
struct Family {
    codec: Codec,
    /// The instrument's default patch, in its bytes.
    plain: Vec<u8>,
    /// A patch that sounds unlike the default: its filter nearly closed.
    dull: Vec<u8>,
    /// Bytes of a patch's length that the instrument's reader refuses, if
    /// it refuses any.
    faulty: Option<Vec<u8>>,
}

fn families() -> Vec<Family> {
    let mut moog = minimoog::Patch::DEFAULT;
    moog.cutoff = -400;
    let mut tone = cs80::Tone::DEFAULT;
    tone.set(cs80::Control::Lpf, 20);
    let mut yamaha = cs80::Patch::DEFAULT;
    yamaha.tones[0] = tone;
    let faulty = |length: usize| Some(vec![0xFF; length]);
    let families = vec![
        Family {
            codec: Codec::Juno,
            plain: juno::Patch::DEFAULT.to_bytes().to_vec(),
            dull: juno::Patch::DEFAULT.with(juno::Slider::VcfFrequency, 30).to_bytes().to_vec(),
            faulty: faulty(juno::patch::PATCH_BYTES),
        },
        Family {
            codec: Codec::Minimoog,
            plain: minimoog::Patch::DEFAULT.to_bytes().to_vec(),
            dull: moog.to_bytes().to_vec(),
            faulty: faulty(minimoog::PATCH_BYTES),
        },
        Family {
            codec: Codec::Prophet5,
            plain: prophet5::Patch::DEFAULT.to_bytes().to_vec(),
            dull: prophet5::Patch::DEFAULT.with(prophet5::Knob::Cutoff, 20).to_bytes().to_vec(),
            // The Prophet-5's reader holds every byte to its range itself.
            faulty: None,
        },
        Family {
            codec: Codec::Jupiter8,
            plain: jupiter8::Patch::DEFAULT.to_bytes().to_vec(),
            dull: jupiter8::Patch::DEFAULT.with(jupiter8::Param::VcfCutoff, 40).to_bytes().to_vec(),
            faulty: faulty(jupiter8::PATCH_BYTES),
        },
        Family {
            codec: Codec::Cs80,
            plain: cs80::Patch::DEFAULT.to_bytes().to_vec(),
            dull: yamaha.to_bytes().to_vec(),
            faulty: faulty(cs80::PATCH_BYTES),
        },
    ];
    // The faulty patches are faulty: the instruments' own readers say so.
    assert!(juno::Patch::from_bytes(&[0xFF; juno::patch::PATCH_BYTES]).is_err());
    assert!(minimoog::Patch::from_bytes(&[0xFF; minimoog::PATCH_BYTES]).is_err());
    assert!(jupiter8::Patch::from_bytes(&[0xFF; jupiter8::PATCH_BYTES]).is_err());
    assert!(cs80::Patch::from_bytes(&[0xFF; cs80::PATCH_BYTES]).is_err());
    families
}

/// One slot at the block's start.
fn slot(status: u8, data1: u8, data2: u8) -> [u8; 4] {
    [0, status, data1, data2]
}

/// The blocks, one slot each, of `before` and then a note on, `held` empty
/// blocks and its note off.
fn note(before: &[[u8; 4]], held: usize) -> Vec<u8> {
    let mut blocks: Vec<u8> = before.iter().flatten().copied().collect();
    blocks.extend(slot(0x90, 57, 100));
    blocks.extend(std::iter::repeat_n([0_u8; 4], held).flatten());
    blocks.extend(slot(0x80, 57, 0));
    blocks
}

/// A stored sound of `codec` from blocks of one slot, naming the tests'
/// bank.
fn sound(codec: Codec, blocks: &[u8]) -> Sound {
    let mut sound = Sound::instrument(SOUND, 1, codec, 4, blocks).unwrap();
    sound.bank = Some(BANK);
    sound
}

fn bank(family: Codec, patches: &[&[u8]]) -> Sound {
    Sound::patch_bank(BANK, 1, family, &patches.concat()).unwrap()
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

/// What a stored `sound` renders to with `bank`, and what the receiver
/// passed over.
fn rendered(bank: &Sound, sound: &Sound) -> (Vec<i16>, u32) {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, bank, &mut sink);
    push(&mut receiver, sound, &mut sink);
    receiver.push_record(&play(0, 1, sound), &mut sink);
    while receiver.next_tick_ms() == Some(0) {
        receiver.tick(0, &mut sink);
    }
    assert_eq!(sink.starts, vec![(0, SoundId { handle: SOUND, generation: 1 })]);
    (sink.pcm, receiver.statistics().slots_passed_over)
}

// Verifies DEC-6429 foundations item 4 with REQ-6411 item 5: a sound of each
// synthesizer waits for its bank and then plays its notes.
#[test]
fn a_stored_sound_of_each_synthesizer_waits_for_its_bank_and_then_plays() {
    for family in families() {
        let mut arena = vec![0_u8; 64 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        let sound = sound(family.codec, &note(&[], 20));
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play(1, 1, &sound), &mut sink);
        assert!(sink.starts.is_empty() && sink.defined.is_empty(), "{:?}: its bank has not arrived", family.codec);
        push(&mut receiver, &bank(family.codec, &[&family.plain]), &mut sink);
        let id = SoundId { handle: SOUND, generation: 1 };
        assert_eq!(sink.starts, vec![(1, id)], "{:?}", family.codec);
        assert_eq!(
            sink.defined,
            vec![(id, SoundFormat { rate_hz: INSTRUMENT_RATE, samples: 22 * INSTRUMENT_BLOCK_SAMPLES })]
        );
        assert!(loud(&sink.pcm[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
        assert_eq!(receiver.statistics().slots_passed_over, 0);
    }
}

// Verifies REQ-6411 item 5 for a patch bank: only a bank for the sound's own
// family is its bank, and a bank is never played.
#[test]
fn a_bank_for_another_family_is_not_a_sounds_bank_and_no_bank_is_played() {
    let families = families();
    for (family, other) in families.iter().zip(families.iter().cycle().skip(1)) {
        let mut arena = vec![0_u8; 64 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        let sound = sound(family.codec, &note(&[], 4));
        let wrong = bank(other.codec, &[&other.plain]);
        push(&mut receiver, &wrong, &mut sink);
        push(&mut receiver, &sound, &mut sink);
        receiver.push_record(&play(0, 1, &sound), &mut sink);
        receiver.push_record(&play(1, 1, &wrong), &mut sink);
        assert!(
            sink.starts.is_empty() && sink.defined.is_empty() && sink.streams.is_empty(),
            "{:?} with a bank for {:?}",
            family.codec,
            other.codec
        );
    }
}

// Verifies DEC-6429 foundations item 4: a program change chooses the patch
// of that number, and until one comes the instrument plays patch 0.
#[test]
fn a_program_change_chooses_the_patch_and_patch_0_plays_until_one_comes() {
    for family in families() {
        let two = bank(family.codec, &[&family.plain, &family.dull]);
        let (first, _) = rendered(&two, &sound(family.codec, &note(&[[0; 4]], 10)));
        let (second, passed) = rendered(&two, &sound(family.codec, &note(&[slot(0xC0, 1, 0)], 10)));
        assert_eq!(passed, 0);
        let (plain, _) = rendered(&bank(family.codec, &[&family.plain]), &sound(family.codec, &note(&[[0; 4]], 10)));
        let (dull, _) = rendered(&bank(family.codec, &[&family.dull]), &sound(family.codec, &note(&[[0; 4]], 10)));
        assert!(loud(&plain) && dull.iter().any(|&sample| sample != 0), "{:?}", family.codec);
        assert_ne!(plain, dull, "{:?}: the two patches sound unlike", family.codec);
        assert_eq!(first, plain, "{:?}: no program change plays patch 0", family.codec);
        assert_eq!(second, dull, "{:?}: program 1 plays patch 1", family.codec);
    }
}

// Verifies REQ-6411 item 4 as DEC-6421 item 4 built it, for a patch bank: a
// patch the instrument's reader refuses, or one that is not in the bank,
// leaves the instrument's default patch in its place.
#[test]
fn a_faulty_or_missing_patch_leaves_the_default_patch_in_its_place() {
    for family in families() {
        let notes = sound(family.codec, &note(&[slot(0xC0, 1, 0)], 10));
        let (plain, _) = rendered(&bank(family.codec, &[&family.plain]), &sound(family.codec, &note(&[[0; 4]], 10)));
        // Program 1 of a bank of one patch.
        let (missing, passed) = rendered(&bank(family.codec, &[&family.dull]), &notes);
        assert_eq!((missing, passed), (plain.clone(), 0), "{:?}: a patch the bank has not", family.codec);
        let Some(faulty) = &family.faulty else { continue };
        let (refused, passed) = rendered(&bank(family.codec, &[&family.dull, faulty]), &notes);
        assert_eq!((refused, passed), (plain, 0), "{:?}: a patch its reader refuses", family.codec);
    }
}

// Verifies REQ-6411 item 3 for each synthesizer's codec: the messages each
// takes beyond the DX7's, and that the rest are passed over and counted.
#[test]
fn each_codec_takes_its_own_messages_and_passes_over_the_rest() {
    let messages: [[u8; 4]; 8] = [
        slot(0xA0, 57, 90), // polyphonic key pressure
        slot(0xD0, 90, 0),  // channel pressure
        slot(0xB0, 5, 40),  // portamento time
        slot(0xB0, 8, 64),  // balance
        slot(0xB0, 65, 0),  // portamento off
        slot(0xC1, 0, 0),   // program change on MIDI channel 2
        slot(0xC2, 0, 0),   // program change on MIDI channel 3
        slot(0xF0, 1, 2),   // a system message
    ];
    for family in families() {
        let expected = match family.codec {
            // The CS-80 takes both pressures, and not the three controllers
            // or the system message. The Jupiter-8 takes the three
            // controllers and its two patches' channels, and neither
            // pressure, a third channel's program or the system message.
            Codec::Cs80 | Codec::Jupiter8 => 4,
            // Any channel's program change, as the DX7.
            _ => 6,
        };
        let mut block: Vec<u8> = messages.iter().flatten().copied().collect();
        // After them, since a program change on a Jupiter-8 releases notes.
        block.extend([10, 0x90, 57, 100]);
        let mut sound = Sound::instrument(SOUND, 1, family.codec, 36, &[block, vec![0; 36 * 5]].concat()).unwrap();
        sound.bank = Some(BANK);
        let (pcm, passed) = rendered(&bank(family.codec, &[&family.plain]), &sound);
        assert_eq!(passed, expected, "{:?}", family.codec);
        assert!(loud(&pcm), "{:?}: the note beside them still sounds", family.codec);
    }
}

// Verifies DEC-6429 foundations item 4: the model tag chooses the Juno-60
// or the Prophet-5 of revisions 1 and 2, and absent means the first model.
#[test]
fn the_model_tag_chooses_the_model() {
    for family in families().iter().filter(|family| family.codec.models() > 1) {
        let patches = bank(family.codec, &[&family.plain]);
        let mut notes = sound(family.codec, &note(&[], 10));
        let (absent, _) = rendered(&patches, &notes);
        notes.model = Some(0);
        let (first, _) = rendered(&patches, &notes);
        notes.model = Some(1);
        let (second, _) = rendered(&patches, &notes);
        assert_eq!(absent, first, "{:?}: no tag is the first model", family.codec);
        assert_ne!(first, second, "{:?}: the second model is another instrument", family.codec);
        assert!(loud(&second));
    }
}

// Verifies DEC-6429 foundations item 4 for the Jupiter-8: MIDI channel 1's
// program change chooses the upper patch and channel 2's the lower, and the
// keys tag sets the key mode and the split point.
#[test]
fn a_jupiter_8_has_two_patches_a_key_mode_and_a_split_point() {
    let families = families();
    let family = families.iter().find(|family| family.codec == Codec::Jupiter8).unwrap();
    let patches = bank(Codec::Jupiter8, &[&family.plain, &family.dull]);
    let render = |keys: Option<[u8; 2]>, before: &[[u8; 4]]| {
        let mut notes = sound(Codec::Jupiter8, &note(before, 10));
        notes.keys = keys;
        rendered(&patches, &notes).0
    };
    let lower_dull = [slot(0xC1, 1, 0)];
    let upper_dull = [slot(0xC0, 1, 0)];
    let whole = render(None, &[[0; 4]]);
    assert_eq!(render(Some([JUPITER8_KEYS_WHOLE, 60]), &[[0; 4]]), whole, "no tag is the whole keyboard");
    // The whole keyboard plays the upper patch alone.
    assert_eq!(render(None, &lower_dull), whole, "the lower patch is not heard on a whole keyboard");
    assert_ne!(render(None, &upper_dull), whole, "the upper patch is");
    // A dual keyboard plays both.
    let dual = render(Some([JUPITER8_KEYS_DUAL, 60]), &[[0; 4]]);
    assert_ne!(render(Some([JUPITER8_KEYS_DUAL, 60]), &lower_dull), dual, "a dual keyboard plays the lower patch");
    assert_ne!(render(Some([JUPITER8_KEYS_DUAL, 60]), &upper_dull), dual, "and the upper");
    // The note is key 57: below a split at 60, so the lower patch plays it;
    // at a split at 50 the upper does.
    let low = render(Some([JUPITER8_KEYS_SPLIT, 60]), &[[0; 4]]);
    assert_ne!(render(Some([JUPITER8_KEYS_SPLIT, 60]), &lower_dull), low, "below the split, the lower patch");
    assert_eq!(render(Some([JUPITER8_KEYS_SPLIT, 60]), &upper_dull), low, "and not the upper");
    let high = render(Some([JUPITER8_KEYS_SPLIT, 50]), &[[0; 4]]);
    assert_eq!(render(Some([JUPITER8_KEYS_SPLIT, 50]), &lower_dull), high, "from the split up, not the lower patch");
    assert_ne!(render(Some([JUPITER8_KEYS_SPLIT, 50]), &upper_dull), high, "but the upper");
}

/// Plays a live sound of `family`: a note on in block 0 and empty blocks to
/// `last`, without the blocks in `missing`. Answers what the sink was given.
fn live(family: &Family, last: u16, missing: &[u16]) -> Recorder {
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    push(&mut receiver, &bank(family.codec, &[&family.plain]), &mut sink);
    let mut sound = Sound::live(SOUND, 1, family.codec, INSTRUMENT_RATE, 4).unwrap();
    sound.bank = Some(BANK);
    receiver.push_record(&sound.object_record().unwrap(), &mut sink);
    for index in (0..=last).filter(|index| !missing.contains(index)) {
        let block = if index == 0 { slot(0x90, 57, 100) } else { [0; 4] };
        for record in sound.packet_records(index, &block).unwrap() {
            receiver.push_record(&record, &mut sink);
        }
        if index == 0 {
            receiver.push_record(&play(0, 1, &sound), &mut sink);
        }
    }
    sink
}

// Verifies DEC-6410 item 12 for each synthesizer: a live sound is rendered
// as its blocks come, and before the first block after a gap every note is
// released, since the lost block may have held its note off.
#[test]
fn a_live_sound_of_each_synthesizer_plays_and_a_lost_block_releases_its_notes() {
    /// Three seconds of blocks: long enough for any default patch's release.
    const LAST: u16 = 150;
    for family in families() {
        let whole = live(&family, LAST, &[]);
        assert_eq!(whole.streams, vec![(SoundId { handle: SOUND, generation: 1 }, INSTRUMENT_RATE)]);
        assert_eq!(whole.concealed, [], "{:?}: no block was lost", family.codec);
        assert_eq!(whole.streamed.len(), BLOCK * (usize::from(LAST) + 1), "{:?}", family.codec);
        let tail = BLOCK * (usize::from(LAST) - 10);
        assert!(loud(&whole.streamed[..BLOCK * 10]), "{:?}: the note sounds", family.codec);
        assert!(loud(&whole.streamed[tail..]), "{:?}: and is held to the end", family.codec);

        let gapped = live(&family, LAST, &[5]);
        assert_eq!(gapped.concealed, vec![5], "{:?}", family.codec);
        assert_eq!(gapped.streamed.len(), whole.streamed.len());
        assert_eq!(gapped.streamed[..BLOCK * 5], whole.streamed[..BLOCK * 5], "the same until the gap");
        assert!(
            energy(&gapped.streamed[tail..]) * 4 < energy(&whole.streamed[tail..]),
            "{:?}: the note was released at the gap",
            family.codec
        );
    }
}

/// The body of an OBJECT: handle 4, generation 1, id 4, then `codec`, the
/// rate code of 48 kHz, `block_bytes`, `samples`, `blocks` and `tags`.
fn object(codec: u8, block_bytes: u16, samples: u32, blocks: u16, tags: &[u8]) -> Vec<u8> {
    /// 48,000 Hz in [`neoteletext_audio::SAMPLE_RATES`].
    const RATE_48_KHZ: u8 = 7;
    let mut body = vec![SOUND, 0, 1, 0, 0, 0, 4, codec, RATE_48_KHZ];
    body.extend_from_slice(&block_bytes.to_be_bytes());
    body.extend_from_slice(&samples.to_be_bytes());
    body.extend_from_slice(&blocks.to_be_bytes());
    body.extend_from_slice(tags);
    frame(TYPE_OBJECT, &body)
}

// Verifies DEC-6429 foundations item 4: what an OBJECT of each new codec
// must say, and that a faulty one is refused.
#[test]
fn an_object_names_its_bank_model_family_and_keys_and_a_faulty_one_is_refused() {
    let one_block = INSTRUMENT_BLOCK_SAMPLES;
    let good = |record: &[u8]| match parse(record) {
        Ok(Record::Object(object)) => object.codec.map(|codec| (codec, object.bank, object.model, object.keys)),
        _ => None,
    };
    for family in families() {
        let code = family.codec.code();
        let patch = u16::try_from(family.plain.len()).unwrap();
        assert_eq!(family.codec.patch_bytes(), Some(patch));
        // A sound: slots of 4 bytes, 1 to 16 of them, and a bank.
        assert_eq!(
            good(&object(code, 4, one_block, 1, &[TAG_DX7_BANK, 1, BANK])),
            Some((family.codec, Some(BANK), None, None))
        );
        assert!(good(&object(code, 64, one_block, 1, &[TAG_DX7_BANK, 1, BANK])).is_some());
        assert!(parse(&object(code, 4, one_block, 1, &[])).is_err(), "{code}: no bank");
        assert!(parse(&object(code, 4, one_block, 1, &[TAG_DX7_BANK, 2, BANK, 0])).is_err(), "{code}: a long bank");
        for size in [0, 3, 6, 68] {
            assert!(parse(&object(code, size, one_block, 1, &[TAG_DX7_BANK, 1, BANK])).is_err(), "{code}: {size}");
        }
        // The model: one the family has.
        for model in 0..=u8::MAX {
            let record = object(code, 4, one_block, 1, &[TAG_DX7_BANK, 1, BANK, TAG_MODEL, 1, model]);
            assert_eq!(parse(&record).is_ok(), model < family.codec.models(), "{code}: model {model}");
        }
        assert!(parse(&object(code, 4, one_block, 1, &[TAG_DX7_BANK, 1, BANK, TAG_MODEL, 0])).is_err());
        assert!(parse(&object(code, 4, one_block, 1, &[TAG_DX7_BANK, 1, BANK, TAG_MODEL, 2, 0, 0])).is_err());
        // A bank: the family's patches, 1 to 32 of them, never live.
        let bank = |block: u16, patches: u32, tags: &[u8]| {
            object(Codec::PatchBank.code(), block, patches, u16::try_from(patches).unwrap(), tags)
        };
        let full = bank(patch, PATCH_BANK_PATCHES, &[TAG_BANK_FAMILY, 1, code]);
        let Ok(Record::Object(read)) = parse(&full) else { panic!("{code}: not an OBJECT") };
        assert_eq!((read.codec, read.family), (Some(Codec::PatchBank), Some(code)));
        assert!(parse(&bank(patch, 1, &[])).is_err(), "{code}: a bank for no family");
        assert!(parse(&bank(patch, 0, &[TAG_BANK_FAMILY, 1, code])).is_err(), "{code}: a live bank");
        assert!(parse(&bank(patch, PATCH_BANK_PATCHES + 1, &[TAG_BANK_FAMILY, 1, code])).is_err(), "{code}: 33");
        assert!(parse(&bank(patch + 1, 1, &[TAG_BANK_FAMILY, 1, code])).is_err(), "{code}: a longer patch");
        assert!(parse(&bank(patch, 1, &[TAG_BANK_FAMILY, 2, code, 0])).is_err(), "{code}: a long family");
        // Another family's patch size.
        let other = if family.codec == Codec::Juno { Codec::Cs80 } else { Codec::Juno };
        assert!(parse(&bank(other.patch_bytes().unwrap(), 1, &[TAG_BANK_FAMILY, 1, code])).is_err());
        assert!(Sound::patch_bank(BANK, 1, family.codec, &vec![0; family.plain.len() * 33]).is_err());
        assert!(Sound::patch_bank(BANK, 1, family.codec, &vec![0; family.plain.len() + 1]).is_err());
    }
    // A family that is none: every code but the five.
    for family in (0..=u8::MAX).filter(|code| !Codec::is_family_code(*code)) {
        let record = object(Codec::PatchBank.code(), 18, 1, 1, &[TAG_BANK_FAMILY, 1, family]);
        assert!(parse(&record).is_err(), "a bank for family {family}");
    }
    assert!(Sound::patch_bank(BANK, 1, Codec::Dx7, &[0; 128]).is_err(), "the DX7 has its own bank");
    assert!(Sound::instrument(BANK, 1, Codec::PatchBank, 18, &[0; 18]).is_err(), "a bank names its family");

    // The Jupiter-8's keys: a mode it has and a MIDI key, in two bytes.
    let jupiter = Codec::Jupiter8.code();
    let keys = |value: &[u8]| {
        let mut tags = vec![TAG_DX7_BANK, 1, BANK, TAG_JUPITER8_KEYS, u8::try_from(value.len()).unwrap()];
        tags.extend_from_slice(value);
        object(jupiter, 4, one_block, 1, &tags)
    };
    assert_eq!(
        good(&keys(&[JUPITER8_KEYS_SPLIT, 127])),
        Some((Codec::Jupiter8, Some(BANK), None, Some([JUPITER8_KEYS_SPLIT, 127])))
    );
    assert!(parse(&keys(&[JUPITER8_KEYS_SPLIT + 1, 60])).is_err(), "a key mode it has not");
    assert!(parse(&keys(&[JUPITER8_KEYS_DUAL, 128])).is_err(), "a split point that is no key");
    assert!(parse(&keys(&[JUPITER8_KEYS_DUAL])).is_err() && parse(&keys(&[0, 60, 0])).is_err(), "another length");
    // On another codec the tag is skipped, as any unknown tag is.
    let tags = [TAG_DX7_BANK, 1, BANK, TAG_JUPITER8_KEYS, 2, 9, 200];
    assert!(parse(&object(Codec::Juno.code(), 4, one_block, 1, &tags)).is_ok());
}

// Verifies REQ-AUD-004 for the codes after the last: an unknown codec's
// OBJECT is read, ignored and counted, and its data is refused.
#[test]
fn an_unknown_codec_is_ignored_and_counted() {
    /// The first code no codec has.
    const RESERVED: u8 = 12;
    let mut arena = vec![0_u8; 64 * 1024];
    let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
    let mut sink = Recorder::default();
    assert_eq!(Codec::from_code(RESERVED), None);
    receiver.push_record(&object(RESERVED, 4, INSTRUMENT_BLOCK_SAMPLES, 1, &[TAG_DX7_BANK, 1, BANK]), &mut sink);
    assert_eq!(receiver.statistics().unsupported, 1);
    assert_eq!(receiver.store().entries().count(), 0);
    assert!(sink.defined.is_empty() && sink.starts.is_empty());
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

    fn byte(&mut self) -> u8 {
        (self.next() >> 32) as u8
    }

    fn below(&mut self, bound: usize) -> usize {
        ((self.next() >> 16) as usize) % bound
    }

    fn bytes(&mut self, length: usize) -> Vec<u8> {
        (0..length).map(|_| self.byte()).collect()
    }
}

// Verifies REQ-SEC-003 for the new codecs: sounds whose every block is
// random, with banks whose every patch is random, are rendered without a
// panic, stored and live.
#[test]
fn random_blocks_and_random_patches_never_panic() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for family in families() {
        for round in 0..6 {
            let mut arena = vec![0_u8; 64 * 1024];
            let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
            let mut sink = Recorder::default();
            let patches = 1 + random.below(PATCH_BANK_PATCHES as usize);
            let bank = Sound::patch_bank(BANK, 1, family.codec, &random.bytes(patches * family.plain.len())).unwrap();
            push(&mut receiver, &bank, &mut sink);
            let slots = 1 + random.below(16);
            let block = u16::try_from(slots * 4).unwrap();
            // Mostly messages a block may hold, so that the instruments are
            // reached, and some bytes of any value.
            let mut data = random.bytes(slots * 4 * 30);
            for [time, status, data1, data2] in data.as_chunks_mut::<4>().0.iter_mut().filter(|_| round % 2 == 0) {
                *time %= 220;
                *status |= 0x80;
                *data1 &= 0x7F;
                *data2 &= 0x7F;
            }
            let mut stored = Sound::instrument(SOUND, 1, family.codec, block, &data).unwrap();
            stored.bank = Some(BANK);
            stored.model = Some(random.byte() % family.codec.models());
            if family.codec == Codec::Jupiter8 {
                stored.keys = Some([random.byte() % 3, random.byte() & 0x7F]);
            }
            push(&mut receiver, &stored, &mut sink);
            receiver.push_record(&play(0, 1, &stored), &mut sink);
            while receiver.next_tick_ms() == Some(0) {
                receiver.tick(0, &mut sink);
            }
            assert_eq!(sink.starts.len(), 1, "{:?}", family.codec);

            let mut live = Sound::live(SOUND + 1, 1, family.codec, INSTRUMENT_RATE, block).unwrap();
            live.bank = Some(BANK);
            receiver.push_record(&live.object_record().unwrap(), &mut sink);
            receiver.push_record(&play(1, 1, &live), &mut sink);
            for (index, chunk) in data.chunks_exact(usize::from(block)).enumerate() {
                // Some blocks are lost.
                if random.below(5) == 0 {
                    continue;
                }
                for record in live.packet_records(u16::try_from(index).unwrap(), chunk).unwrap() {
                    receiver.push_record(&record, &mut sink);
                }
            }
            assert!(!sink.streamed.is_empty(), "{:?}", family.codec);
        }
    }
}

/// The good records of a session of `family`, in the order a channel sends
/// them: a bank, a stored sound and its play, a live sound, its play and
/// its first blocks.
fn session(family: &Family) -> Vec<Vec<u8>> {
    let bank = bank(family.codec, &[&family.plain, &family.dull]);
    let stored = sound(family.codec, &note(&[slot(0xC0, 1, 0)], 6));
    let mut live = Sound::live(SOUND + 1, 1, family.codec, INSTRUMENT_RATE, 8).unwrap();
    live.bank = Some(BANK);
    let mut good = vec![bank.object_record().unwrap()];
    good.extend(bank.data_records());
    good.push(stored.object_record().unwrap());
    good.extend(stored.data_records());
    good.push(play(0, 1, &stored));
    good.push(live.object_record().unwrap());
    for index in 0..6 {
        good.extend(live.packet_records(index, &[0, 0x90, 60, 100, 100, 0x80, 60, 0]).unwrap());
        if index == 0 {
            good.push(play(1, 1, &live));
        }
    }
    good
}

/// A record to trouble a receiver with: a good one spoiled, or one of
/// random fields in a good frame.
fn troubling(random: &mut Random, good: &[Vec<u8>]) -> Vec<u8> {
    match random.below(6) {
        // A good record again, out of its order.
        0 => good[random.below(good.len())].clone(),
        // A good record with one byte changed: its CRC refuses it.
        1 => {
            let mut record = good[random.below(good.len())].clone();
            let at = random.below(record.len());
            record[at] = random.byte();
            record
        }
        // A good record cut short.
        2 => {
            let record = &good[random.below(good.len())];
            record[..random.below(record.len())].to_vec()
        }
        // An OBJECT whose every field is random, in a good frame, the codec
        // mostly one of the new ones or the first reserved.
        3 => {
            let length = 13 + random.below(12);
            let mut body = random.bytes(length);
            if random.below(4) != 0 {
                body[7] = Codec::Juno.code() + random.byte() % 7;
                body[8] = 7;
            }
            // Often with tags that are well formed and of any value.
            for tag in [TAG_DX7_BANK, TAG_MODEL, TAG_BANK_FAMILY, TAG_JUPITER8_KEYS] {
                if random.below(2) == 0 {
                    let length = random.below(3);
                    body.extend([tag, u8::try_from(length).unwrap()]);
                    body.extend(random.bytes(length));
                }
            }
            frame(TYPE_OBJECT, &body)
        }
        // DATA of random blocks for a handle in use, in a good frame.
        4 => {
            let mut body = vec![[SOUND, SOUND + 1, BANK][random.below(3)], 0, random.byte() % 3, 0, random.byte() % 8];
            let length = 1 + random.below(200);
            body.extend(random.bytes(length));
            frame(TYPE_DATA, &body)
        }
        // A CONTROL whose every field is random, in a good frame.
        _ => frame(TYPE_CONTROL, &random.bytes(14)),
    }
}

// Verifies REQ-SEC-003: among a channel's good records, records of random
// fields, good records with a byte changed and good records cut short never
// panic the receiver, whatever they do to the sounds it holds.
#[test]
fn random_records_never_panic() {
    /// The sessions: each a receiver, a family and its records.
    const SESSIONS: usize = 400;
    let mut random = Random(0xD1B5_4A32_D192_ED03);
    let families = families();
    let (mut started, mut unsupported, mut refused) = (0, 0, 0);
    for family in families.iter().cycle().take(SESSIONS) {
        let good = session(family);
        let mut arena = vec![0_u8; 64 * 1024];
        let mut receiver: Receiver<'_, 8, 2> = Receiver::new(&mut arena);
        let mut sink = Recorder::default();
        let mut now = 0_u64;
        for record in &good {
            for _ in 0..random.below(3) {
                receiver.push_record(&troubling(&mut random, &good), &mut sink);
            }
            receiver.push_record(record, &mut sink);
            if random.below(4) == 0 {
                now += random.below(40) as u64;
                receiver.tick(now, &mut sink);
            }
        }
        started += sink.starts.len();
        unsupported += receiver.statistics().unsupported;
        refused += receiver.statistics().records_refused;
    }
    // The troubling records did reach the receiver's every part.
    assert!(started > 0, "some sound was played");
    assert!(unsupported > 0, "some OBJECT named a codec that is none");
    assert!(refused > 0, "some record was refused");
}
