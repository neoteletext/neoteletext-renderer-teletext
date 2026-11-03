//! Tests of the Ogg Opus reader (TODO-1228, REQ-AUD-022, REQ-SEC rules for
//! a reader of outside input).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and arithmetic on small values it chose, where an overflow would also fail it"
)]

use super::*;
use alloc::vec;

/// The table of contents of a CELT-only full-band 20 ms mono frame, one a
/// packet (RFC 6716 section 3.1: configuration 31, code 0).
const CELT_FULL_BAND_20_MS: u8 = 31 << 3;
/// The same frame at 10 ms (configuration 30), which the profile refuses.
const CELT_FULL_BAND_10_MS: u8 = 30 << 3;
const PACKET: u16 = 40;

fn packets(count: usize) -> Vec<u8> {
    (0..count)
        .flat_map(|index| {
            let mut packet = vec![CELT_FULL_BAND_20_MS];
            packet.extend((1..PACKET).map(|byte| (byte as u8).wrapping_mul(index as u8 | 1)));
            packet
        })
        .collect()
}

/// RFC 3533 section 6: the page CRC is CRC-32 with polynomial 0x04C11DB7,
/// not reflected, initial and final value zero; its check value for ASCII
/// `123456789` (with no CRC field in reach) is 0x89A1897F.
#[test]
fn the_page_crc_has_the_ogg_check_value() {
    // The check value's input is shorter than the CRC field's offset.
    assert_eq!(page_crc(b"123456789"), 0x89A1_897F);
}

/// TODO-1228: a file the writer makes reads back as the same packets, with
/// the sample count its last granule gives.
#[test]
fn a_written_file_reads_back() {
    let data = packets(120);
    let samples = 119 * 960 + 333;
    let file = write_ogg_opus(&data, PACKET, samples, 7);
    let read = read_ogg_opus(&file).unwrap();
    assert_eq!((read.packets.clone(), read.packet_bytes, read.count()), (data, PACKET, 120));
    assert_eq!(read.samples(), u32::try_from(samples).unwrap());
    assert_eq!((read.pre_skip, read.input_rate_hz), (0, 48_000));
    // A granule outside the last frame is not believed.
    let odd = write_ogg_opus(&packets(3), PACKET, 99_999, 7);
    assert_eq!(read_ogg_opus(&odd).unwrap().samples(), 3 * 960);
}

/// The offset of page `index` of `file`.
fn page_offset(file: &[u8], index: usize) -> usize {
    let mut offset = 0;
    for _ in 0..index {
        let count = usize::from(file[offset + SEGMENT_COUNT_AT]);
        let body: usize = file[offset + PAGE_HEADER_BYTES..offset + PAGE_HEADER_BYTES + count]
            .iter()
            .map(|&lacing| usize::from(lacing))
            .sum();
        offset += PAGE_HEADER_BYTES + count + body;
    }
    offset
}

/// Recomputes page `index`'s CRC after a test changed it.
fn reseal(file: &mut [u8], index: usize) {
    let start = page_offset(file, index);
    let end = page_offset(file, index + 1);
    let crc = page_crc(&file[start..end]).to_le_bytes();
    file[start + CRC_AT..start + CRC_AT + 4].copy_from_slice(&crc);
}

/// REQ-SEC (a reader of outside input): damaged, cut, reordered and mixed
/// files are refused, each saying where.
#[test]
fn damaged_files_are_refused_with_where() {
    let file = write_ogg_opus(&packets(60), PACKET, 60 * 960, 7);
    let third = page_offset(&file, 2);
    let mut bad = file.clone();
    bad[third + PAGE_HEADER_BYTES + 3] ^= 1;
    assert_eq!(read_ogg_opus(&bad), Err(OggError::Crc(third)));
    assert_eq!(read_ogg_opus(&file[..file.len() - 1]), Err(OggError::Truncated(page_offset(&file, 3))));
    let mut wrong_pattern = file.clone();
    wrong_pattern[third] = b'X';
    assert_eq!(read_ogg_opus(&wrong_pattern), Err(OggError::NotAPage(third)));
    let mut version = file.clone();
    version[third + VERSION_AT] = 1;
    assert_eq!(read_ogg_opus(&version), Err(OggError::Version(third)));
    // A page left out.
    let fourth = page_offset(&file, 3);
    let mut missing = file[..third].to_vec();
    missing.extend_from_slice(&file[fourth..]);
    assert_eq!(read_ogg_opus(&missing), Err(OggError::Sequence(third)));
    // Another stream's page.
    let mut other = file.clone();
    other[third + SERIAL_AT] ^= 1;
    reseal(&mut other, 2);
    assert_eq!(read_ogg_opus(&other), Err(OggError::SecondStream(third)));
    // A page claiming to continue a packet when none is open.
    let mut continued = file.clone();
    continued[third + HEADER_TYPE_AT] |= CONTINUED;
    reseal(&mut continued, 2);
    assert_eq!(read_ogg_opus(&continued), Err(OggError::Continuation(third)));
    assert_eq!(read_ogg_opus(&[]), Err(OggError::Head));
    assert_eq!(read_ogg_opus(b"OggS"), Err(OggError::Truncated(0)));
}

/// REQ-AUD-022 and decision E.124: a stereo header, a packet that is not a
/// 20 ms frame, one too long and one of another length are refused, the
/// packet by its number counted from 1.
#[test]
fn headers_and_packets_outside_the_profile_are_refused() {
    let file = write_ogg_opus(&packets(5), PACKET, 5 * 960, 7);
    let mut stereo = file.clone();
    stereo[PAGE_HEADER_BYTES + 1 + HEAD_CHANNELS_AT] = 2;
    reseal(&mut stereo, 0);
    assert_eq!(read_ogg_opus(&stereo), Err(OggError::Channels(2)));
    let mut family = file.clone();
    family[PAGE_HEADER_BYTES + 1 + HEAD_FAMILY_AT] = 1;
    reseal(&mut family, 0);
    assert_eq!(read_ogg_opus(&family), Err(OggError::MappingFamily(1)));
    let mut ten_ms = packets(5);
    ten_ms[3 * usize::from(PACKET)] = CELT_FULL_BAND_10_MS;
    assert_eq!(read_ogg_opus(&write_ogg_opus(&ten_ms, PACKET, 4800, 7)), Err(OggError::NotProfile(4)));
    let long = vec![CELT_FULL_BAND_20_MS; usize::from(OPUS_MAX_PACKET) + 1];
    let file = write_ogg_opus(&long, OPUS_MAX_PACKET + 1, 960, 7);
    assert_eq!(read_ogg_opus(&file), Err(OggError::PacketTooLong(1)));
    // Two packets of 40 bytes, then one of 39.
    let mut uneven = packets(2);
    uneven.extend_from_slice(&packets(1)[..39]);
    let mut file = write_ogg_opus(&uneven[..80], PACKET, 1920, 7);
    let last = page_offset(&file, 2);
    let extra = write_ogg_opus(&uneven[80..], 39, 2880, 7);
    let mut page = extra[page_offset(&extra, 2)..].to_vec();
    page[SEQUENCE_AT..SEQUENCE_AT + 4].copy_from_slice(&3_u32.to_le_bytes());
    let crc = page_crc(&page).to_le_bytes();
    page[CRC_AT..CRC_AT + 4].copy_from_slice(&crc);
    assert!(last < file.len());
    file.extend_from_slice(&page);
    assert_eq!(read_ogg_opus(&file), Err(OggError::PacketLength(3)));
    // Headers alone.
    let headers =
        &write_ogg_opus(&packets(1), PACKET, 960, 7)[..page_offset(&write_ogg_opus(&packets(1), PACKET, 960, 7), 2)];
    assert_eq!(read_ogg_opus(headers), Err(OggError::NoAudio));
}
