//! Reading an encoder's Ogg Opus file (RFC 3533, RFC 7845) into the audio
//! profile's Opus packets, for publishing tools (TODO-1228, DEC-2448).
//!
//! The file is untrusted input. It is read in one pass over a slice the
//! caller has already bounded: every page's length is checked against what
//! is left before it is read, every page's CRC is checked, its sequence
//! number must follow the last, and only one logical stream may be in the
//! file. The identification header must be one channel of mapping family
//! 0; the comment header is passed over without being kept; and every audio
//! packet must be one of the profile (one 20 ms mono frame of at most
//! [`OPUS_MAX_PACKET`] bytes, REQ-AUD-022), all of one length (decision
//! E.124). Nothing longer than a profile packet is ever held.

use alloc::vec::Vec;

use crate::{OPUS_FRAME_SAMPLES, OPUS_MAX_PACKET};

/// The capture pattern that begins every page (RFC 3533 section 6).
const CAPTURE_PATTERN: [u8; 4] = *b"OggS";
/// The one stream structure version (RFC 3533 section 6).
const STREAM_VERSION: u8 = 0;
/// Header type flag: the page's first packet continues the last page's
/// (RFC 3533 section 6).
const CONTINUED: u8 = 0x01;
/// Header type flag: the first page of a logical stream.
const BEGINNING_OF_STREAM: u8 = 0x02;
/// Bytes of a page header before its segment table (RFC 3533 section 6).
const PAGE_HEADER_BYTES: usize = 27;
/// Where the header's fields are (RFC 3533 section 6).
const VERSION_AT: usize = 4;
const HEADER_TYPE_AT: usize = 5;
const GRANULE_AT: usize = 6;
const SERIAL_AT: usize = 14;
const SEQUENCE_AT: usize = 18;
const CRC_AT: usize = 22;
const SEGMENT_COUNT_AT: usize = 26;
/// A lacing value of 255 says the packet goes on in the next segment
/// (RFC 3533 section 5).
const SEGMENT_CONTINUES: u8 = 255;
/// The page CRC's generator polynomial, not reflected, with an initial
/// register and final XOR of zero (RFC 3533 section 6).
const CRC_POLYNOMIAL: u32 = 0x04C1_1DB7;

/// The identification header's magic signature (RFC 7845 section 5.1).
const OPUS_HEAD: [u8; 8] = *b"OpusHead";
/// The comment header's magic signature (RFC 7845 section 5.2).
const OPUS_TAGS: [u8; 8] = *b"OpusTags";
/// Bytes of an identification header of mapping family 0 (RFC 7845
/// section 5.1, Figure 2).
const OPUS_HEAD_BYTES: usize = 19;
/// The upper four bits of the version are the major version, which must
/// be 0 for a reader of this specification (RFC 7845 section 5.1 item 2).
const MAJOR_VERSION_MASK: u8 = 0xF0;
const HEAD_VERSION_AT: usize = 8;
const HEAD_CHANNELS_AT: usize = 9;
const HEAD_PRE_SKIP_AT: usize = 10;
const HEAD_RATE_AT: usize = 12;
const HEAD_FAMILY_AT: usize = 18;
/// Mapping family 0: one stream, mono or stereo (RFC 7845 section 5.1.1.1).
const FAMILY_ONE_STREAM: u8 = 0;
/// The profile's one channel (decision E.123).
const MONO: u8 = 1;

/// An encoder's Opus stream, read and checked against the profile.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OggOpus {
    /// The packets, back to back, each [`OggOpus::packet_bytes`] long.
    pub packets: Vec<u8>,
    /// The length of every packet.
    pub packet_bytes: u16,
    /// Samples at 48 kHz the decoder throws away at the start (RFC 7845
    /// section 4.2), as the header gives them.
    pub pre_skip: u16,
    /// The rate the encoder's input had, for information only (RFC 7845
    /// section 5.1 item 5).
    pub input_rate_hz: u32,
    /// The granule position of the last page: the samples at 48 kHz the
    /// stream decodes to, the pre-skip included (RFC 7845 section 4).
    pub last_granule: u64,
}

impl OggOpus {
    /// The number of packets.
    #[must_use]
    pub fn count(&self) -> usize {
        self.packets.len().checked_div(usize::from(self.packet_bytes)).unwrap_or(0)
    }

    /// The samples the sound keeps: what the last granule position says,
    /// which trims the last frame (RFC 7845 section 4.4), when it falls in
    /// the last packet's frame; otherwise every packet's whole frame. The
    /// pre-skip is kept, since the profile has no field for it (DEC-2448).
    #[must_use]
    pub fn samples(&self) -> u32 {
        let count = u32::try_from(self.count()).unwrap_or(u32::MAX);
        let whole = count.saturating_mul(OPUS_FRAME_SAMPLES);
        let before_last = count.saturating_sub(1).saturating_mul(OPUS_FRAME_SAMPLES);
        match u32::try_from(self.last_granule) {
            Ok(granule) if granule > before_last && granule <= whole => granule,
            _ => whole,
        }
    }
}

/// Why an Ogg Opus file was refused; each names where.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OggError {
    /// No capture pattern at this byte offset, where a page should begin.
    NotAPage(usize),
    /// A stream structure version other than 0, in the page at this offset.
    Version(usize),
    /// The page at this offset runs past the end of the file.
    Truncated(usize),
    /// The page at this offset fails its CRC.
    Crc(usize),
    /// The page at this offset belongs to a second logical stream.
    SecondStream(usize),
    /// The page at this offset is not the next in sequence: one is missing.
    Sequence(usize),
    /// The page at this offset continues a packet when none was open, or
    /// does not when one was.
    Continuation(usize),
    /// The file ends inside a packet.
    UnfinishedPacket,
    /// The first packet is not an identification header of major version 0.
    Head,
    /// The identification header gives this many channels; the profile is
    /// mono.
    Channels(u8),
    /// The identification header gives a channel mapping family other than
    /// 0.
    MappingFamily(u8),
    /// The second packet is not a comment header.
    Tags,
    /// No audio packet.
    NoAudio,
    /// Audio packet number this (counted from 1) is longer than a profile
    /// packet.
    PacketTooLong(usize),
    /// Audio packet number this is not one 20 ms mono frame of the profile
    /// (REQ-AUD-022).
    NotProfile(usize),
    /// Audio packet number this is not the length of the first: the
    /// profile carries constant-rate packets (decision E.124).
    PacketLength(usize),
    /// More packets than a sound may have.
    TooManyPackets,
}

/// Which header or packet is being read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expecting {
    Head,
    Tags,
    Audio,
}

/// What has been read so far.
struct Reading {
    expecting: Expecting,
    /// The packet being put together, never longer than a profile packet
    /// or an identification header; past that only its length is counted.
    packet: Vec<u8>,
    /// The length of the packet being put together.
    packet_length: usize,
    head: Option<(u16, u32)>,
    /// Audio packets read, counting from 1 for the errors.
    audio_packets: usize,
    out: Vec<u8>,
    packet_bytes: Option<u16>,
}

/// The most bytes of a packet that are kept: a profile packet, or an
/// identification header of mapping family 0, whichever is longer.
const KEPT_BYTES: usize =
    if OPUS_MAX_PACKET as usize > OPUS_HEAD_BYTES { OPUS_MAX_PACKET as usize } else { OPUS_HEAD_BYTES };

/// The most packets a sound may have: its blocks are numbered in 16 bits.
const MAX_PACKETS: usize = u16::MAX as usize;

/// Reads `file`, an Ogg Opus file of one logical stream, into the profile's
/// packets.
///
/// # Errors
/// What [`OggError`] lists: a malformed or damaged page, a header outside
/// the profile, or a packet outside the profile, each with where it is.
pub fn read_ogg_opus(file: &[u8]) -> Result<OggOpus, OggError> {
    let mut reading = Reading {
        expecting: Expecting::Head,
        packet: Vec::new(),
        packet_length: 0,
        head: None,
        audio_packets: 0,
        out: Vec::new(),
        packet_bytes: None,
    };
    let mut offset = 0_usize;
    let mut serial = None;
    let mut next_sequence = 0_u32;
    let mut last_granule = 0_u64;
    while offset < file.len() {
        let rest = file.get(offset..).ok_or(OggError::Truncated(offset))?;
        let page = page_at(rest, offset)?;
        let header = page.header;
        if serial.is_some_and(|serial| serial != header.serial) {
            return Err(OggError::SecondStream(offset));
        }
        if serial.is_none() && header.kind & BEGINNING_OF_STREAM == 0 {
            return Err(OggError::NotAPage(offset));
        }
        if serial.is_some() && header.sequence != next_sequence {
            return Err(OggError::Sequence(offset));
        }
        serial = Some(header.serial);
        next_sequence = header.sequence.wrapping_add(1);
        let open = reading.packet_length > 0;
        if (header.kind & CONTINUED != 0) != open {
            return Err(OggError::Continuation(offset));
        }
        let mut body = page.body;
        for &lacing in page.segments {
            let (segment, after) = body.split_at_checked(usize::from(lacing)).ok_or(OggError::Truncated(offset))?;
            body = after;
            reading.take(segment);
            if lacing != SEGMENT_CONTINUES {
                reading.packet_done()?;
            }
        }
        // A granule position of all ones says no packet ends on the page
        // (RFC 3533 section 6).
        if header.granule != u64::MAX {
            last_granule = header.granule;
        }
        offset = offset.saturating_add(page.length);
    }
    if reading.packet_length > 0 {
        return Err(OggError::UnfinishedPacket);
    }
    let (pre_skip, input_rate_hz) = reading.head.ok_or(OggError::Head)?;
    if reading.expecting != Expecting::Audio {
        return Err(OggError::Tags);
    }
    let packet_bytes = reading.packet_bytes.ok_or(OggError::NoAudio)?;
    Ok(OggOpus { packets: reading.out, packet_bytes, pre_skip, input_rate_hz, last_granule })
}

impl Reading {
    /// Adds a segment to the packet being put together.
    fn take(&mut self, segment: &[u8]) {
        let room = KEPT_BYTES.saturating_sub(self.packet.len());
        self.packet.extend(segment.iter().take(room));
        self.packet_length = self.packet_length.saturating_add(segment.len());
    }

    /// The packet being put together is whole.
    fn packet_done(&mut self) -> Result<(), OggError> {
        let length = self.packet_length;
        let packet = core::mem::take(&mut self.packet);
        self.packet_length = 0;
        match self.expecting {
            Expecting::Head => {
                self.head = Some(identification(&packet, length)?);
                self.expecting = Expecting::Tags;
            }
            Expecting::Tags => {
                if packet.get(..OPUS_TAGS.len()) != Some(&OPUS_TAGS[..]) {
                    return Err(OggError::Tags);
                }
                self.expecting = Expecting::Audio;
            }
            Expecting::Audio => self.audio(&packet, length)?,
        }
        Ok(())
    }

    /// Checks an audio packet against the profile and keeps it.
    fn audio(&mut self, packet: &[u8], length: usize) -> Result<(), OggError> {
        self.audio_packets = self.audio_packets.saturating_add(1);
        let number = self.audio_packets;
        if number > MAX_PACKETS {
            return Err(OggError::TooManyPackets);
        }
        if length > usize::from(OPUS_MAX_PACKET) {
            return Err(OggError::PacketTooLong(number));
        }
        if neoteletext_opus::check_packet(packet).is_err() {
            return Err(OggError::NotProfile(number));
        }
        let bytes = u16::try_from(length).map_err(|_| OggError::PacketTooLong(number))?;
        if *self.packet_bytes.get_or_insert(bytes) != bytes {
            return Err(OggError::PacketLength(number));
        }
        self.out.extend_from_slice(packet);
        Ok(())
    }
}

/// The pre-skip and input rate of an identification header (RFC 7845
/// section 5.1), checked against the profile.
fn identification(packet: &[u8], length: usize) -> Result<(u16, u32), OggError> {
    if length < OPUS_HEAD_BYTES || packet.get(..OPUS_HEAD.len()) != Some(&OPUS_HEAD[..]) {
        return Err(OggError::Head);
    }
    let byte = |at: usize| packet.get(at).copied().ok_or(OggError::Head);
    if byte(HEAD_VERSION_AT)? & MAJOR_VERSION_MASK != 0 {
        return Err(OggError::Head);
    }
    let channels = byte(HEAD_CHANNELS_AT)?;
    if channels != MONO {
        return Err(OggError::Channels(channels));
    }
    let family = byte(HEAD_FAMILY_AT)?;
    if family != FAMILY_ONE_STREAM {
        return Err(OggError::MappingFamily(family));
    }
    let pre_skip = u16::from_le_bytes([byte(HEAD_PRE_SKIP_AT)?, byte(HEAD_PRE_SKIP_AT.saturating_add(1))?]);
    let rate = little_endian_u32(packet, HEAD_RATE_AT).ok_or(OggError::Head)?;
    Ok((pre_skip, rate))
}

/// A page's header fields.
#[derive(Clone, Copy)]
struct PageHeader {
    kind: u8,
    granule: u64,
    serial: u32,
    sequence: u32,
}

/// One page: its header, segment table and body, and its whole length.
struct Page<'a> {
    header: PageHeader,
    segments: &'a [u8],
    body: &'a [u8],
    length: usize,
}

/// The page at the start of `bytes`, which is at `offset` in the file,
/// checked for its pattern, version, length and CRC.
fn page_at(bytes: &[u8], offset: usize) -> Result<Page<'_>, OggError> {
    if bytes.get(..CAPTURE_PATTERN.len()) != Some(&CAPTURE_PATTERN[..]) {
        return Err(OggError::NotAPage(offset));
    }
    let header = bytes.get(..PAGE_HEADER_BYTES).ok_or(OggError::Truncated(offset))?;
    if header.get(VERSION_AT) != Some(&STREAM_VERSION) {
        return Err(OggError::Version(offset));
    }
    let count = usize::from(header.get(SEGMENT_COUNT_AT).copied().ok_or(OggError::Truncated(offset))?);
    let table_end = PAGE_HEADER_BYTES.saturating_add(count);
    let segments = bytes.get(PAGE_HEADER_BYTES..table_end).ok_or(OggError::Truncated(offset))?;
    let body_length = segments.iter().map(|&lacing| usize::from(lacing)).sum::<usize>();
    let length = table_end.saturating_add(body_length);
    let whole = bytes.get(..length).ok_or(OggError::Truncated(offset))?;
    let body = whole.get(table_end..).ok_or(OggError::Truncated(offset))?;
    let stored = little_endian_u32(header, CRC_AT).ok_or(OggError::Truncated(offset))?;
    if page_crc(whole) != stored {
        return Err(OggError::Crc(offset));
    }
    let granule = header
        .get(GRANULE_AT..SERIAL_AT)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(OggError::Truncated(offset))?;
    let page_header = PageHeader {
        kind: header.get(HEADER_TYPE_AT).copied().ok_or(OggError::Truncated(offset))?,
        granule,
        serial: little_endian_u32(header, SERIAL_AT).ok_or(OggError::Truncated(offset))?,
        sequence: little_endian_u32(header, SEQUENCE_AT).ok_or(OggError::Truncated(offset))?,
    };
    Ok(Page { header: page_header, segments, body, length })
}

fn little_endian_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let field = bytes.get(at..at.checked_add(4)?)?;
    field.try_into().ok().map(u32::from_le_bytes)
}

/// The CRC of a whole page, its CRC field taken as zero (RFC 3533
/// section 6).
#[must_use]
pub fn page_crc(page: &[u8]) -> u32 {
    let crc_field = CRC_AT..CRC_AT.saturating_add(4);
    let mut register = 0_u32;
    for (index, &byte) in page.iter().enumerate() {
        let byte = if crc_field.contains(&index) { 0 } else { byte };
        register ^= u32::from(byte) << 24;
        for _ in 0..8 {
            let high = register & (1 << 31) != 0;
            register <<= 1;
            if high {
                register ^= CRC_POLYNOMIAL;
            }
        }
    }
    register
}

/// Packets put on one audio page by [`write_ogg_opus`]: one second of
/// 20 ms frames, as encoders commonly page them.
const PACKETS_PER_PAGE: usize = 50;
/// The vendor string of the comment header [`write_ogg_opus`] writes.
const VENDOR: &[u8] = b"neoteletext";
/// The version [`write_ogg_opus`] writes in the identification header:
/// major 0, minor 1 (RFC 7845 section 5.1 item 2).
const HEAD_VERSION: u8 = 1;

/// Writes `packets`, each `packet_bytes` long, back to back, as an Ogg
/// Opus file of one logical stream with serial number `serial` (RFC 3533,
/// RFC 7845), whose last granule position is `samples`: the inverse of
/// [`read_ogg_opus`], for tests and for tools that make a file of the
/// profile's packets.
#[must_use]
pub fn write_ogg_opus(packets: &[u8], packet_bytes: u16, samples: u64, serial: u32) -> Vec<u8> {
    let mut head = Vec::with_capacity(OPUS_HEAD_BYTES);
    head.extend_from_slice(&OPUS_HEAD);
    head.extend_from_slice(&[HEAD_VERSION, MONO]);
    head.extend_from_slice(&0_u16.to_le_bytes());
    head.extend_from_slice(&crate::OPUS_RATE.to_le_bytes());
    head.extend_from_slice(&0_u16.to_le_bytes());
    head.push(FAMILY_ONE_STREAM);
    let mut tags = OPUS_TAGS.to_vec();
    tags.extend_from_slice(&u32::try_from(VENDOR.len()).unwrap_or(0).to_le_bytes());
    tags.extend_from_slice(VENDOR);
    tags.extend_from_slice(&0_u32.to_le_bytes());
    let mut file = Vec::new();
    let mut sequence = 0_u32;
    let mut page = |kind: u8, granule: u64, packets: &[&[u8]]| {
        let mut segments = Vec::new();
        let mut body = Vec::new();
        for packet in packets {
            let mut left = packet.len();
            loop {
                let lacing = left.min(usize::from(SEGMENT_CONTINUES));
                segments.push(u8::try_from(lacing).unwrap_or(SEGMENT_CONTINUES));
                left = left.saturating_sub(lacing);
                if lacing < usize::from(SEGMENT_CONTINUES) {
                    break;
                }
            }
            body.extend_from_slice(packet);
        }
        let start = file.len();
        file.extend_from_slice(&CAPTURE_PATTERN);
        file.extend_from_slice(&[STREAM_VERSION, kind]);
        file.extend_from_slice(&granule.to_le_bytes());
        file.extend_from_slice(&serial.to_le_bytes());
        file.extend_from_slice(&sequence.to_le_bytes());
        file.extend_from_slice(&0_u32.to_le_bytes());
        file.push(u8::try_from(segments.len()).unwrap_or(u8::MAX));
        file.extend_from_slice(&segments);
        file.extend_from_slice(&body);
        if let Some(whole) = file.get_mut(start..) {
            let crc = page_crc(whole).to_le_bytes();
            if let Some(field) = whole.get_mut(CRC_AT..CRC_AT.saturating_add(4)) {
                field.copy_from_slice(&crc);
            }
        }
        sequence = sequence.wrapping_add(1);
    };
    page(BEGINNING_OF_STREAM, 0, &[&head]);
    page(0, 0, &[&tags]);
    let all: Vec<&[u8]> = packets.chunks(usize::from(packet_bytes.max(1))).collect();
    let pages = all.chunks(PACKETS_PER_PAGE).count();
    for (index, chunk) in all.chunks(PACKETS_PER_PAGE).enumerate() {
        let through = index.saturating_add(1).saturating_mul(PACKETS_PER_PAGE).min(all.len());
        let whole = u64::try_from(through).unwrap_or(u64::MAX).saturating_mul(u64::from(OPUS_FRAME_SAMPLES));
        let granule = if index.saturating_add(1) == pages { samples } else { whole };
        page(0, granule, chunk);
    }
    file
}

#[cfg(test)]
mod tests;
