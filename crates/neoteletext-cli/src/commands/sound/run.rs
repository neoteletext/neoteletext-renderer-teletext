//! `neottx sound` in a build with it: the options checked, the WAVE file
//! read, and the packets written, paced for a player when live.

use super::SoundOptions;
use super::wave::read_wave;
use crate::commands::{io_failure, page_number, sandbox_closed};
use neoteletext_app::bounded_file::read_bounded;
use neoteletext_app::{Failure, exit};
use neoteletext_audio::Codec;
use neoteletext_audio::encode::{Command, LiveEncoder, Sound, packets};
use neoteletext_audio::ogg::{OggError, OggOpus, read_ogg_opus};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// The largest input read: 32 MB, almost three minutes of 48 kHz stereo
/// WAVE, and far more than the 65,535 packets of 160 bytes an Opus sound
/// may hold.
#[cfg(feature = "sound")]
const INPUT_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
/// The operation code of a play command (REQ-AUD-006).
#[cfg(feature = "sound")]
const PLAY: u8 = 1;
/// The command's flag for a looping sound.
#[cfg(feature = "sound")]
const LOOP: u8 = neoteletext_audio::record::FLAG_LOOP;

/// `neottx sound`.
pub fn sound(options: &SoundOptions, input: &Path, output: &Path, no_sandbox: bool) -> Result<(), Failure> {
    let page = page_number(&options.page)?;
    let codec = checked_codec(options)?;
    let bytes = read_bounded(input, INPUT_MAX_FILE_BYTES)?;
    let mut out = File::create(output).map_err(|error| io_failure(output, error))?;
    // Every file is open (REQ-SEC-017).
    sandbox_closed(no_sandbox)?;
    let refused = |message: String| (exit::INPUT, format!("{}: {message}", input.display()));
    let command = Command {
        channel: options.channel,
        sequence: 1,
        operation: PLAY,
        handle: options.handle,
        generation: 1,
        gain: options.gain,
        fade_ms: 0,
        flags: if options.looped && !options.live { LOOP } else { 0 },
        duck: 0,
        page: Some(page),
    };
    let source = match codec {
        Codec::Opus => Source::Opus(read_ogg_opus(&bytes).map_err(|error| refused(ogg_refusal(error)))?),
        Codec::ImaAdpcm | Codec::Pcm16 => Source::Wave(read_wave(&bytes).map_err(refused)?),
    };
    let rate_refused =
        |error| refused(format!("{error:?}; the rate must be one of {:?}", neoteletext_audio::SAMPLE_RATES));
    if options.live {
        let (sound, blocks) = match &source {
            Source::Opus(opus) => {
                let sound =
                    Sound::live(options.handle, 1, Codec::Opus, neoteletext_audio::OPUS_RATE, opus.packet_bytes)
                        .map_err(rate_refused)?;
                let blocks = opus_live_blocks(&sound, &opus.packets, options.repeat).map_err(refused)?;
                (sound, blocks)
            }
            Source::Wave(wave) => {
                // A live sound's blocks are small, so each goes as soon as it
                // exists.
                let block = match codec {
                    Codec::ImaAdpcm => neoteletext_audio::IMA_BLOCK_SMALL,
                    Codec::Pcm16 | Codec::Opus => neoteletext_audio::MAX_PCM_BLOCK,
                };
                let sound = Sound::live(options.handle, 1, codec, wave.rate_hz(), block).map_err(rate_refused)?;
                let blocks = wave_live_blocks(&sound, wave.samples(), options.repeat).map_err(refused)?;
                (sound, blocks)
            }
        };
        let packets = live_packets(&sound, &command, &blocks, options.pace).map_err(refused)?;
        for packet in &packets {
            out.write_all(&packet.bytes).map_err(|error| io_failure(output, error))?;
        }
        out.flush().map_err(|error| io_failure(output, error))?;
        // Every block sent is whole, the last padded with silence, so the
        // bytes of sound are the blocks times the block's size.
        eprintln!(
            "{}: {} samples at {} Hz, {} bytes of {} sent live {} times, {} packets at {} a second for page {page:03X}",
            output.display(),
            source.samples(),
            sound.rate_hz,
            blocks.len().saturating_mul(usize::from(sound.block_bytes)),
            options.codec,
            options.repeat,
            packets.len(),
            options.pace
        );
        return Ok(());
    }
    let sound = match &source {
        Source::Opus(opus) => Sound::opus(options.handle, 1, opus.packet_bytes, &opus.packets, opus.samples())
            .map_err(|error| refused(format!("{error:?}")))?,
        Source::Wave(wave) => {
            let block = match codec {
                Codec::ImaAdpcm => neoteletext_audio::IMA_BLOCK_LARGE,
                Codec::Pcm16 | Codec::Opus => neoteletext_audio::MAX_PCM_BLOCK,
            };
            Sound::new(options.handle, 1, codec, wave.rate_hz(), block, wave.samples()).map_err(rate_refused)?
        }
    };
    let mut records = vec![sound.object_record().map_err(|error| (exit::INPUT, format!("{error:?}")))?];
    records.extend(sound.data_records());
    records.push(command.record());
    let written = write_repeats(&mut out, output, &records, options.repeat)?;
    out.flush().map_err(|error| io_failure(output, error))?;
    eprintln!(
        "{}: {} samples at {} Hz, {} bytes of {}, {written} packets for page {page:03X}",
        output.display(),
        sound.samples,
        sound.rate_hz,
        sound.data.len(),
        options.codec
    );
    Ok(())
}

/// The sound a command reads: a WAVE file's samples, or an encoder's Opus
/// packets.
enum Source {
    Wave(super::wave::Wave),
    Opus(OggOpus),
}

impl Source {
    /// The samples the input holds.
    fn samples(&self) -> usize {
        match self {
            Self::Wave(wave) => wave.samples().len(),
            Self::Opus(opus) => usize::try_from(opus.samples()).unwrap_or(usize::MAX),
        }
    }
}

/// Why an Ogg Opus file was refused, in words, with the byte offset or the
/// packet's number (TODO-1228).
fn ogg_refusal(error: OggError) -> String {
    match error {
        OggError::NotAPage(offset) => format!("byte {offset}: not an Ogg page"),
        OggError::Version(offset) => format!("byte {offset}: an Ogg page of another version"),
        OggError::Truncated(offset) => format!("byte {offset}: the Ogg page is cut short"),
        OggError::Crc(offset) => format!("byte {offset}: the Ogg page fails its CRC"),
        OggError::SecondStream(offset) => format!("byte {offset}: a second logical stream"),
        OggError::Sequence(offset) => format!("byte {offset}: an Ogg page is missing before this one"),
        OggError::Continuation(offset) => format!("byte {offset}: the Ogg page's packets do not follow on"),
        OggError::UnfinishedPacket => "the file ends inside a packet".to_owned(),
        OggError::Head => "no Opus identification header of version 0".to_owned(),
        OggError::Channels(channels) => format!("{channels} channels; the profile is mono"),
        OggError::MappingFamily(family) => format!("channel mapping family {family}; the profile is family 0"),
        OggError::Tags => "no Opus comment header after the identification header".to_owned(),
        OggError::NoAudio => "no audio packets".to_owned(),
        OggError::PacketTooLong(number) => {
            format!("packet {number} is longer than the profile's {} bytes", neoteletext_audio::OPUS_MAX_PACKET)
        }
        OggError::NotProfile(number) => format!("packet {number} is not one 20 ms mono frame"),
        OggError::PacketLength(number) => {
            format!("packet {number} is not the length of the first: the profile carries constant-rate packets")
        }
        OggError::TooManyPackets => "more packets than a sound may have".to_owned(),
    }
}

/// The codec the options name, once the channel, gain and repeat are
/// checked to be in range.
fn checked_codec(options: &SoundOptions) -> Result<Codec, Failure> {
    let codec = match options.codec.as_str() {
        "ima" => Codec::ImaAdpcm,
        "pcm" => Codec::Pcm16,
        "opus" => Codec::Opus,
        other => return Err((exit::USAGE, format!("{other}: the codec is ima, pcm or opus"))),
    };
    if options.channel >= neoteletext_audio::CHANNELS
        || options.gain > neoteletext_audio::MAX_GAIN
        || options.repeat == 0
    {
        return Err((exit::USAGE, "the channel is 0 to 3, the gain 0 to 100 and the repeat at least 1".to_owned()));
    }
    Ok(codec)
}

/// Writes a stored sound's records `repeat` times; returns the number of
/// packets written.
fn write_repeats(out: &mut File, output: &Path, records: &[Vec<u8>], repeat: u8) -> Result<usize, Failure> {
    let mut written = 0_usize;
    let mut record_id = 0;
    for _ in 0..repeat {
        // A repeat keeps its record ids: the same records again.
        let (packets, _) = packets(0, record_id, records);
        for packet in &packets {
            out.write_all(&packet.bytes).map_err(|error| io_failure(output, error))?;
        }
        written = written.saturating_add(packets.len());
        record_id = 0;
    }
    Ok(written)
}

/// A live WAVE sound's blocks: `repeat` runs of `samples` in a row, cut
/// into blocks by one live encoder, so the IMA ADPCM step index runs on from
/// block to block as it would in one call, each with its records and the
/// samples captured when it is whole; the last, part block is padded with
/// silence.
fn wave_live_blocks(sound: &Sound, samples: &[i16], repeat: u8) -> Result<Vec<LiveBlock>, String> {
    let per_block = usize::try_from(sound.samples_per_block()).unwrap_or(usize::MAX).max(1);
    let all: Vec<i16> =
        samples.iter().copied().cycle().take(samples.len().saturating_mul(usize::from(repeat))).collect();
    let mut encoder = LiveEncoder::new(sound, 0).map_err(|error| format!("{error:?}"))?;
    Ok(all
        .chunks(per_block)
        .map(|block| {
            let mut records = encoder.records(block);
            records.extend(encoder.finish());
            LiveBlock { records, samples: per_block }
        })
        .collect())
}

/// A live Opus sound's blocks: `repeat` runs of the encoder's packets in a
/// row, one packet a block, as decision E.124 carries them.
fn opus_live_blocks(sound: &Sound, packets: &[u8], repeat: u8) -> Result<Vec<LiveBlock>, String> {
    let per_packet = usize::from(sound.block_bytes).max(1);
    let all =
        packets.chunks(per_packet).cycle().take(packets.len().div_ceil(per_packet).saturating_mul(usize::from(repeat)));
    let samples = usize::try_from(neoteletext_audio::OPUS_FRAME_SAMPLES).unwrap_or(usize::MAX);
    (0_u16..)
        .zip(all)
        .map(|(index, packet)| {
            let records = sound.packet_records(index, packet).map_err(|error| format!("{error:?}"))?;
            Ok(LiveBlock { records, samples })
        })
        .collect()
}

/// One block of a live sound: its records, and the samples it holds.
struct LiveBlock {
    records: Vec<Vec<u8>>,
    samples: usize,
}

/// A live sound's packets as a player reading `pace` packets a second
/// meets them: the announcement and the play first, then each block when
/// its last sample has been captured. The packets between are repeats of
/// the announcement and of the latest block, as a carousel repeats records,
/// which also mends a lost packet.
fn live_packets(
    sound: &Sound,
    command: &Command,
    blocks: &[LiveBlock],
    pace: u32,
) -> Result<Vec<neoteletext_core::packet::Packet>, String> {
    let announce = vec![sound.object_record().map_err(|error| format!("{error:?}"))?, command.record()];
    let (announcement, mut record_id) = packets(0, 0, &announce);
    let mut out = announcement.clone();
    let mut latest: Vec<neoteletext_core::packet::Packet> = Vec::new();
    let mut filler = 0_usize;
    let mut captured = 0_u64;
    for block in blocks {
        // The block is whole once its last sample is captured.
        captured = captured.saturating_add(u64::try_from(block.samples).unwrap_or(u64::MAX));
        let due = usize::try_from(
            captured.saturating_mul(u64::from(pace)).checked_div(u64::from(sound.rate_hz.max(1))).unwrap_or(0),
        )
        .unwrap_or(usize::MAX);
        while out.len() < due {
            let cycle = announcement.len().saturating_add(latest.len()).max(1);
            let at = filler.checked_rem(cycle).unwrap_or(0);
            let packet = announcement.get(at).or_else(|| latest.get(at.saturating_sub(announcement.len())));
            out.extend(packet.copied());
            filler = filler.saturating_add(1);
        }
        let (block_packets, next) = packets(0, record_id, &block.records);
        if out.len().saturating_sub(due) > block_packets.len().saturating_mul(2) {
            return Err(format!(
                "{} Hz in {}-byte blocks needs more than {pace} packets a second",
                sound.rate_hz, sound.block_bytes
            ));
        }
        record_id = next;
        out.extend_from_slice(&block_packets);
        latest = block_packets;
    }
    Ok(out)
}
