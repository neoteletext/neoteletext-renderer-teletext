#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses indexing into data it built to a known size, `unwrap` on results it expects to be good and `panic!` on outcomes it rules out"
)]

// Tests of REQ-AUD-002, REQ-AUD-003, REQ-AUD-004 and REQ-AUD-006.
use super::*;
use crate::crc::crc32;
use crate::{MAX_FRAMED_BODY, PROFILE_VERSION};
use std::vec::Vec;

fn frame(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = std::vec![PROFILE_VERSION, kind];
    bytes.extend_from_slice(&u16::try_from(body.len()).unwrap().to_be_bytes());
    bytes.extend_from_slice(body);
    let crc = crc32(&bytes);
    bytes.extend_from_slice(&crc.to_be_bytes());
    bytes
}

fn object_body() -> Vec<u8> {
    // Handle 3, generation 1, id 9, IMA ADPCM, 16 kHz, 256-byte blocks,
    // 1 000 samples in 2 blocks of 505.
    let mut body = std::vec![3, 0, 1, 0, 0, 0, 9, 1, 2, 1, 0];
    body.extend_from_slice(&1000_u32.to_be_bytes());
    body.extend_from_slice(&2_u16.to_be_bytes());
    body
}

// Verifies REQ-AUD-004.
#[cfg(feature = "ima")]
#[test]
fn an_object_with_a_label_is_read() {
    let mut body = object_body();
    body.extend_from_slice(&[TAG_LABEL, 5]);
    body.extend_from_slice(b"Chime");
    body.extend_from_slice(&[0x7F, 1, 0]); // an unknown tag, skipped
    let bytes = frame(TYPE_OBJECT, &body);
    let Record::Object(object) = parse(&bytes).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec, Some(Codec::ImaAdpcm));
    assert_eq!((object.rate_hz, object.samples, object.blocks), (16_000, 1000, 2));
    assert_eq!(object.label, Some(&b"Chime"[..]));
}

// Verifies REQ-AUD-004.
#[cfg(feature = "ima")]
#[test]
fn a_wrong_block_count_is_refused() {
    let mut body = object_body();
    let last = body.len() - 1;
    body[last] = 3;
    assert_eq!(parse(&frame(TYPE_OBJECT, &body)), Err(RecordError::BadValue("block count")));
}

// Verifies REQ-AUD-004.
#[test]
fn a_reserved_codec_is_read_and_left_for_the_receiver_to_ignore() {
    let mut body = object_body();
    // Codes 0 to 2 and 12 to 19 are taken: the first reserved one after
    // them (MIDI-107).
    body[7] = 20;
    let bytes = frame(TYPE_OBJECT, &body);
    let Record::Object(object) = parse(&bytes).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec, None);
}

/// Decision E.131: a build without `ima` or `pcm16` reads their objects as
/// of no codec it decodes, so the receiver ignores them as unsupported.
#[test]
fn codecs_left_out_of_the_build_are_read_as_unsupported() {
    let ima = frame(TYPE_OBJECT, &object_body());
    let Record::Object(object) = parse(&ima).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec.is_some(), cfg!(feature = "ima"));
    let mut pcm = object_body();
    // PCM, 100-byte blocks: 1 000 samples in 20 blocks of 50.
    pcm[7] = 0;
    pcm[9..11].copy_from_slice(&100_u16.to_be_bytes());
    let last = pcm.len() - 2;
    pcm[last..].copy_from_slice(&20_u16.to_be_bytes());
    let pcm = frame(TYPE_OBJECT, &pcm);
    let Record::Object(object) = parse(&pcm).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec.is_some(), cfg!(feature = "pcm16"));
}

/// An event sound's OBJECT (MIDI-40): handle 3, generation 1, id 9,
/// `codec`, 48 kHz, longest block `block_bytes`, `blocks` blocks (0 for a
/// live sound), then `tags`.
fn event_body(codec: u8, block_bytes: u16, blocks: u16, tags: &[u8]) -> Vec<u8> {
    let mut body = std::vec![3, 0, 1, 0, 0, 0, 9, codec, 7];
    body.extend_from_slice(&block_bytes.to_be_bytes());
    body.extend_from_slice(&(u32::from(blocks) * crate::INSTRUMENT_BLOCK_SAMPLES).to_be_bytes());
    body.extend_from_slice(&blocks.to_be_bytes());
    body.extend_from_slice(tags);
    body
}

/// A bank's OBJECT (MIDI-55): handle 9, generation 1, id 9, an image of
/// `bytes` in pieces of `piece`, then `tags`.
fn bank_body(piece: u16, bytes: u32, tags: &[u8]) -> Vec<u8> {
    let mut body = std::vec![9, 0, 1, 0, 0, 0, 9, Codec::Bank.code(), 7];
    body.extend_from_slice(&piece.to_be_bytes());
    body.extend_from_slice(&bytes.to_be_bytes());
    body.extend_from_slice(&u16::try_from(bytes.div_ceil(u32::from(piece))).unwrap().to_be_bytes());
    body.extend_from_slice(tags);
    body
}

fn read(body: &[u8]) -> Result<Object<'static>, RecordError> {
    // The frame is built and read here; the object borrows nothing of it
    // that a test keeps.
    let bytes = std::boxed::Box::leak(frame(TYPE_OBJECT, body).into_boxed_slice());
    match parse(bytes)? {
        Record::Object(object) => Ok(object),
        other => panic!("not an object: {other:?}"),
    }
}

/// The tags of a stored event sound naming bank 9 and stating `total`.
fn stored_tags(total: u32) -> Vec<u8> {
    let mut tags = std::vec![TAG_BANKS, 1, 9, TAG_TOTAL_BYTES, 4];
    tags.extend_from_slice(&total.to_be_bytes());
    tags
}

/// MIDI-107, MIDI-108: codes 12 to 18 are the synthesizers and 19 the bank,
/// each read only with its family built in; codes 3 to 11 are reserved.
#[test]
fn the_synthesizer_codes_and_the_retired_ones() {
    for (codec, built) in [
        (Codec::Synth6581, cfg!(feature = "synth6581")),
        (Codec::Synth7, cfg!(feature = "synth7")),
        (Codec::Synth106, cfg!(feature = "synth106")),
        (Codec::SynthD, cfg!(feature = "synthd")),
        (Codec::Synth53, cfg!(feature = "synth53")),
        (Codec::Synth8, cfg!(feature = "synth8")),
        (Codec::Synth80, cfg!(feature = "synth80")),
    ] {
        assert_eq!(Codec::from_code(codec.code()), built.then_some(codec));
        let tags = if codec.has_banks() { stored_tags(10) } else { stored_tags(10)[3..].to_vec() };
        let object = read(&event_body(codec.code(), 10, 2, &tags)).unwrap();
        assert_eq!(object.codec, built.then_some(codec), "{codec:?}");
        // A bank for a family left out of the build is ignored with it.
        let bank = read(&bank_body(100, 100, &[TAG_BANK_FAMILY, 1, codec.code()]));
        if codec.has_banks() {
            assert_eq!(bank.unwrap().codec, built.then_some(Codec::Bank), "a bank for {codec:?}");
        } else if Codec::from_code(Codec::Bank.code()).is_some() {
            assert_eq!(bank, Err(RecordError::BadValue("family")));
        }
    }
    for code in 3..=11 {
        assert_eq!(Codec::from_code(code), None, "code {code} is retired");
    }
}

/// MIDI-40, MIDI-41: an event sound's block bytes, block count, samples and
/// total bytes.
#[cfg(feature = "synth7")]
#[test]
fn an_event_sound_states_its_longest_block_and_total() {
    let code = Codec::Synth7.code();
    let object = read(&event_body(code, 1_005, 6_000, &stored_tags(960_000))).unwrap();
    assert_eq!(
        (object.block_bytes, object.blocks, object.total, object.banks()),
        (1_005, 6_000, Some(960_000), &[9][..])
    );
    // An empty sound of empty blocks.
    assert_eq!(read(&event_body(code, 0, 1, &stored_tags(0))).unwrap().total, Some(0));
    for (body, why) in [
        (event_body(code, 1_006, 1, &stored_tags(0)), "block size"),
        (event_body(code, 1_005, 6_001, &stored_tags(0)), "block count"),
        (event_body(code, 10, 2, &stored_tags(21)), "total bytes"),
        (event_body(code, 1_005, 6_000, &stored_tags(960_001)), "total bytes"),
        (event_body(code, 10, 2, &[TAG_BANKS, 1, 9]), "total bytes"),
        (event_body(code, 10, 2, &stored_tags(10)[3..]), "banks"),
        (event_body(code, 1_005, 0, &[TAG_BANKS, 1, 9, TAG_TOTAL_BYTES, 4, 0, 0, 0, 0]), "total bytes"),
    ] {
        assert_eq!(read(&body), Err(RecordError::BadValue(why)), "{why}");
    }
    // Samples that are not whole blocks.
    let mut body = event_body(code, 10, 2, &stored_tags(10));
    body[14] ^= 1;
    assert_eq!(read(&body), Err(RecordError::BadValue("block count")));
    // A live sound: no blocks, no samples, no total.
    let live = read(&event_body(code, 1_005, 0, &[TAG_BANKS, 1, 9])).unwrap();
    assert_eq!((live.samples, live.blocks, live.total), (0, 0, None));
}

/// MIDI-48, MIDI-57: the banks a sound names, 1 to 10; the variant; the
/// synth8 keyboard; and tags of other codecs skipped.
#[cfg(all(feature = "synth106", feature = "synth53", feature = "synth8", feature = "synth6581"))]
#[test]
fn the_synthesizers_tags() {
    let total = [TAG_TOTAL_BYTES, 4, 0, 0, 0, 0];
    let with = |codec: Codec, tags: &[u8]| {
        let mut all = tags.to_vec();
        all.extend_from_slice(&total);
        read(&event_body(codec.code(), 0, 1, &all))
    };
    let ten: Vec<u8> = [TAG_BANKS, 10].into_iter().chain(1..=10).collect();
    assert_eq!(with(Codec::Synth53, &ten).unwrap().banks(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    let eleven: Vec<u8> = [TAG_BANKS, 11].into_iter().chain(1..=11).collect();
    assert_eq!(with(Codec::Synth53, &eleven), Err(RecordError::BadValue("banks")));
    assert_eq!(with(Codec::Synth53, &[TAG_BANKS, 0]), Err(RecordError::BadValue("banks")));
    assert_eq!(with(Codec::Synth53, &[TAG_BANKS, 1, 4, TAG_VARIANT, 1, 2]).unwrap().variant, Some(2));
    assert_eq!(with(Codec::Synth53, &[TAG_BANKS, 1, 4, TAG_VARIANT, 1, 3]), Err(RecordError::BadValue("variant")));
    assert_eq!(with(Codec::Synth106, &[TAG_BANKS, 1, 4, TAG_VARIANT, 1, 1]).unwrap().variant, Some(1));
    assert_eq!(with(Codec::Synth106, &[TAG_BANKS, 1, 4, TAG_VARIANT, 1, 2]), Err(RecordError::BadValue("variant")));
    assert_eq!(with(Codec::Synth8, &[TAG_BANKS, 1, 4, TAG_SYNTH8_KEYS, 2, 2, 60]).unwrap().keys, Some([2, 60]));
    assert_eq!(with(Codec::Synth6581, &[TAG_SYNTH6581_CHIP, 1, 3]).unwrap().chip, Some(3));
    // The synth6581 family names no bank, and its banks tag is skipped.
    let chip = with(Codec::Synth6581, &[TAG_BANKS, 1, 4, TAG_VARIANT, 1, 7]).unwrap();
    assert_eq!((chip.banks(), chip.variant), (&[][..], None));
}

/// MIDI-55: a bank's image of 1 to 16,384 bytes in pieces of at most 1,011,
/// its family required.
#[cfg(feature = "synth7")]
#[test]
fn a_bank_states_its_image_and_family() {
    let family = [TAG_BANK_FAMILY, 1, Codec::Synth7.code()];
    for (piece, bytes) in [(1, 1), (1_011, 1_011), (1_011, 1_012), (1_011, 16_384)] {
        let object = read(&bank_body(piece, bytes, &family)).unwrap();
        assert_eq!((object.codec, object.samples, object.family), (Some(Codec::Bank), bytes, Some(13)));
    }
    assert_eq!(read(&bank_body(1_012, 1_012, &family)), Err(RecordError::BadValue("block size")));
    assert_eq!(read(&bank_body(1_011, 16_385, &family)), Err(RecordError::BadValue("bank bytes")));
    assert_eq!(read(&bank_body(100, 100, &[])), Err(RecordError::BadValue("family")));
    assert_eq!(read(&bank_body(100, 100, &[TAG_BANK_FAMILY, 1, 12])), Err(RecordError::BadValue("family")));
}

/// MIDI-42: an event codec's DATA record: the offset, then blocks each with
/// its length; a length past the record ends the walk as faulty.
#[test]
fn event_data_records_carry_an_offset_and_lengths() {
    let body = [0, 0, 1, 0, 0, 0, 0, 2, 0x90, 60, 0, 1, 7, 0, 5, 1];
    let (offset, blocks) = event_blocks(&body).unwrap();
    assert_eq!(offset, 256);
    assert_eq!(blocks.collect::<Vec<_>>(), [Some(&[] as &[u8]), Some(&[0x90, 60][..]), Some(&[7][..]), None]);
    assert!(event_blocks(&[0, 0, 1]).is_none());
}

/// An Opus object: handle 3, generation 1, id 9, codec 2, rate code `rate`,
/// packets of `packet` bytes, `samples` samples in `blocks` blocks.
fn opus_body(rate: u8, packet: u16, samples: u32, blocks: u16) -> Vec<u8> {
    let mut body = std::vec![3, 0, 1, 0, 0, 0, 9, 2, rate];
    body.extend_from_slice(&packet.to_be_bytes());
    body.extend_from_slice(&samples.to_be_bytes());
    body.extend_from_slice(&blocks.to_be_bytes());
    body
}

/// Decision E.124: code 2 is Opus, one 960-sample packet a block, at 48 kHz
/// only, packets of 1 to 160 bytes.
#[cfg(feature = "opus")]
#[test]
fn opus_objects_are_read_at_48_khz_with_packets_the_decoder_takes() {
    const RATE_48_KHZ: u8 = 7;
    const RATE_16_KHZ: u8 = 2;
    let bytes = frame(TYPE_OBJECT, &opus_body(RATE_48_KHZ, 40, 2000, 3));
    let Record::Object(object) = parse(&bytes).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec, Some(Codec::Opus));
    assert_eq!((object.rate_hz, object.block_bytes, object.blocks), (48_000, 40, 3));
    assert_eq!(
        parse(&frame(TYPE_OBJECT, &opus_body(RATE_16_KHZ, 40, 2000, 3))),
        Err(RecordError::BadValue("sample rate"))
    );
    assert_eq!(
        parse(&frame(TYPE_OBJECT, &opus_body(RATE_48_KHZ, 161, 2000, 3))),
        Err(RecordError::BadValue("block size"))
    );
    assert_eq!(
        parse(&frame(TYPE_OBJECT, &opus_body(RATE_48_KHZ, 40, 2000, 2))),
        Err(RecordError::BadValue("block count"))
    );
}

/// Built without the decoder, a receiver ignores Opus as a reserved code.
#[cfg(not(feature = "opus"))]
#[test]
fn without_the_decoder_opus_is_ignored() {
    let bytes = frame(TYPE_OBJECT, &opus_body(7, 40, 2000, 3));
    let Record::Object(object) = parse(&bytes).unwrap() else { panic!("not an object") };
    assert_eq!(object.codec, None);
}

// Verifies REQ-AUD-006.
#[test]
fn controls_are_read_and_their_bounds_kept() {
    let body = [1, 0, 7, 1, 3, 0, 1, 80, 0, 200, FLAG_LOOP, 2, 0x01, 0x00];
    let Record::Control(control) = parse(&frame(TYPE_CONTROL, &body)).unwrap() else { panic!("not a control") };
    assert_eq!(control.page, Some(0x100));
    assert_eq!((control.channel, control.sequence, control.gain, control.fade_ms), (1, 7, 80, 200));
    for (index, value, error) in [
        (0, 4, "channel"),
        (3, 9, "operation"),
        (7, 101, "gain"),
        (10, 8, "flags"),
        (11, 7, "duck"),
        (12, 0x09, "page"),
    ] {
        let mut bad = body;
        bad[index] = value;
        assert_eq!(parse(&frame(TYPE_CONTROL, &bad)), Err(RecordError::BadValue(error)), "field {index}");
    }
    // A hexadecimal page, and a page for the whole service.
    let mut bad = body;
    bad[13] = 0x0A;
    assert_eq!(parse(&frame(TYPE_CONTROL, &bad)), Err(RecordError::BadValue("page")));
    let mut service = body;
    service[12..14].copy_from_slice(&SERVICE_SCOPE.to_be_bytes());
    assert!(matches!(parse(&frame(TYPE_CONTROL, &service)), Ok(Record::Control(Control { page: None, .. }))));
}

// Verifies REQ-AUD-003.
#[test]
fn damaged_and_malformed_frames_are_refused() {
    let good = frame(TYPE_OBJECT, &object_body());
    assert!(parse(&good).is_ok());
    let mut damaged = good.clone();
    damaged[6] ^= 1;
    assert_eq!(parse(&damaged), Err(RecordError::Crc));
    assert_eq!(parse(&good[..good.len() - 1]), Err(RecordError::Truncated));
    let mut longer = good.clone();
    longer.push(0);
    assert_eq!(parse(&longer), Err(RecordError::TrailingBytes));
    assert_eq!(parse(&frame(9, &[])), Err(RecordError::UnknownType(9)));
    // Version 1 with a correct CRC.
    let mut version = std::vec![1, TYPE_CONTROL];
    version.extend_from_slice(&u16::try_from(CONTROL_BYTES).unwrap().to_be_bytes());
    version.extend_from_slice(&[0; CONTROL_BYTES]);
    let crc = crc32(&version);
    version.extend_from_slice(&crc.to_be_bytes());
    assert_eq!(parse(&version), Err(RecordError::Version(1)));
    let mut too_long = std::vec![PROFILE_VERSION, TYPE_DATA];
    too_long.extend_from_slice(&u16::try_from(MAX_FRAMED_BODY + 1).unwrap().to_be_bytes());
    assert_eq!(framed_length(&too_long), Err(RecordError::TooLong));
}

// Verifies REQ-AUD-002: each code names its rate, and a code beyond the table
// names none.
#[test]
fn every_rate_code_names_its_rate_and_no_other_code_does() {
    let rates = [8_000, 11_025, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000];
    let mut next = 0_u8;
    for rate in rates {
        assert_eq!(sample_rate(next), Some(rate), "code {next}");
        next += 1;
    }
    assert_eq!(sample_rate(next), None);
}
