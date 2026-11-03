#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
//! The record reader's reassembly (DOC-1952): a record followed from its
//! fragment 0, other records of the same service left alone, and a damaged
//! fragment or a record that fails its check counted invalid (REQ-1462).

use neoteletext_audio::MAX_FRAMED_RECORD;
use neoteletext_audio::idl::{FRAGMENT_DATA, packet};
use neoteletext_audio::record::{TYPE_DATA, write};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::PageAddress;
use neoteletext_formats::log::LogLevel;

use super::{ChannelRecord, Read, RecordReader, fragments, log, write_packets};
use std::borrow::ToOwned;
use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

/// The audio service the channel's records travel in, in these tests.
const SERVICE: u8 = 3;
/// A LOG line long enough to need several fragments.
const LONG_TEXT: &str = "a log line long enough to be cut into several fragments of the record frame, and more";

fn log_packets(record_id: u8) -> Vec<Packet> {
    let mut packets = Vec::new();
    let page = PageAddress::from_mpp(0x100);
    log::encode(LogLevel::Warning, page, LONG_TEXT, SERVICE, record_id, &mut |packet| packets.push(packet))
        .expect("the line is a valid LOG line");
    packets
}

/// Packets of a record of the audio profile's own type, `TYPE_DATA`, in
/// the same service: the channel's reader must leave them alone.
fn audio_packets(record_id: u8) -> Vec<Packet> {
    let mut packets = Vec::new();
    let body = [0x55_u8; 3 * FRAGMENT_DATA];
    write_packets(TYPE_DATA, &body, SERVICE, record_id, &mut |packet| packets.push(packet)).expect("fits the frame");
    packets
}

/// Every whole record and invalid count the reader gives for `packets`.
fn read_all(packets: &[Packet]) -> (Vec<String>, usize) {
    let mut reader = RecordReader::new();
    let mut records = Vec::new();
    let mut invalid = 0;
    for packet in packets {
        match reader.push(packet) {
            Read::Nothing => {}
            Read::Record(ChannelRecord::Log(record)) => records.push(record.line.text.to_owned()),
            Read::Record(other) => records.push(format!("{other:?}")),
            Read::Invalid(count) => invalid += count,
        }
    }
    (records, invalid)
}

// DOC-1952 (REQ-1462): a record whose fragments are interleaved with an
// audio record's fragments of the same service is still read whole, and
// the audio record is neither read nor counted.
#[test]
fn a_record_interleaved_with_audio_fragments_is_read() {
    let log = log_packets(7);
    let audio = audio_packets(8);
    assert!(log.len() >= 3 && audio.len() >= 3, "both records take several fragments");
    let mut stream = Vec::new();
    for index in 0..log.len().max(audio.len()) {
        stream.extend(audio.get(index).copied());
        stream.extend(log.get(index).copied());
    }
    assert_eq!(read_all(&stream), (vec![LONG_TEXT.to_owned()], 0));
}

// REQ-1462: a fragment that fails its CRC-16 counts one invalid packet, and
// the record it belonged to never completes.
#[test]
fn a_damaged_fragment_counts_one() {
    let mut packets = log_packets(1);
    // A byte of the fragment's data, inside its CRC-16.
    let data_byte = 12;
    packets[1].bytes[data_byte] ^= 0x01;
    let (records, invalid) = read_all(&packets);
    assert_eq!(invalid, 1);
    assert!(records.is_empty(), "the record lost a fragment: {records:?}");
}

// REQ-1462, DOC-1952: a record whose fragments each pass but whose frame
// fails its CRC-32 is refused whole, every one of its packets counted.
#[test]
fn a_record_that_fails_its_check_counts_its_fragments() {
    let body = [b'E', 0, 0, b'h', b'i'];
    let mut record = [0_u8; MAX_FRAMED_RECORD];
    let length = write(super::TYPE_LOG, &body, &mut record).expect("fits the frame");
    let mut record = record[..length].to_vec();
    // A byte of the body, under the frame's CRC-32 and not the fragments'.
    let body_byte = length - 6;
    record[body_byte] ^= 0x20;
    let chunks: Vec<&[u8]> = record.chunks(FRAGMENT_DATA).collect();
    let packets: Vec<Packet> = chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let index = u8::try_from(index).expect("few fragments");
            packet(SERVICE, 2, index, usize::from(index) + 1 == chunks.len(), chunk)
        })
        .collect();
    assert_eq!(read_all(&packets), (Vec::new(), fragments(body.len())));
}

// DOC-1952: `push_counted` gives a whole record with the packets it took,
// so that a sending refused across its records counts each one.
#[test]
fn a_whole_record_comes_with_its_packet_count() {
    let packets = log_packets(4);
    let mut reader = RecordReader::new();
    let mut counts = Vec::new();
    for packet in &packets {
        let (read, count) = reader.push_counted(packet);
        if matches!(read, Read::Record(_)) {
            counts.push(count);
        }
    }
    assert_eq!(counts, vec![packets.len()]);
}

// DOC-1952: a later fragment whose record's fragment 0 was never seen is
// not followed, and is not counted: it may be a record another reader
// takes.
#[test]
fn a_record_seen_from_its_middle_is_left_alone() {
    let packets = log_packets(5);
    assert_eq!(read_all(&packets[1..]), (Vec::new(), 0));
}
