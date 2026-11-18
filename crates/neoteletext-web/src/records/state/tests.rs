#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader, read_record};
use neoteletext_audio::idl::FRAGMENT_DATA;
use neoteletext_audio::record::record_length;
use std::string::String;
use std::vec::Vec;

/// The service the records are written on.
const SERVICE: u8 = 0;

/// The packets of one record, and every record a reader reads from them.
fn written_and_read(encode: impl FnOnce(&mut dyn FnMut(Packet)) -> Result<usize, RecordFault>) -> (usize, Vec<String>) {
    let mut packets = Vec::new();
    let count = encode(&mut |packet| packets.push(packet)).unwrap();
    assert_eq!(count, packets.len());
    let mut reader = RecordReader::new();
    let mut read = Vec::new();
    for packet in &packets {
        match reader.push(packet) {
            Read::Record(record) => read.push(std::format!("{record:?}")),
            Read::Invalid(packets) => read.push(std::format!("{packets} packets refused")),
            Read::Nothing => {}
        }
    }
    (count, read)
}

/// VS-204: `STATE_CHALLENGE` is 33 bytes, the challenge and `create`, in
/// two fragments, and reads back.
#[test]
fn a_state_challenge_reads_back() {
    for create in [false, true] {
        let challenge = StateChallenge { challenge: [0x3C; CHALLENGE_BYTES], create };
        let (count, read) = written_and_read(|out| encode_challenge(challenge, SERVICE, 1, out));
        assert_eq!(count, record_length(CHALLENGE_RECORD_BYTES).div_ceil(FRAGMENT_DATA));
        assert_eq!(read, [std::format!("{:?}", ChannelRecord::StateChallenge(challenge))]);
    }
}

/// VS-204: `STATE_END` is the 32-byte key identifier, and
/// `STATE_KEEP_OFFER` nothing; both read back.
#[test]
fn a_state_end_and_a_keep_offer_read_back() {
    let end = StateEnd { key_id: core::array::from_fn(|index| u8::try_from(index).unwrap()) };
    let (_, read) = written_and_read(|out| encode_end(end, SERVICE, 2, out));
    assert_eq!(read, [std::format!("{:?}", ChannelRecord::StateEnd(end))]);
    let (count, read) = written_and_read(|out| encode_keep_offer(SERVICE, 3, out));
    assert_eq!(count, 1);
    assert_eq!(read, [std::format!("{:?}", ChannelRecord::StateKeepOffer)]);
}

/// VS-204: a body of any other length, or a `create` byte other than 0 or
/// 1, is refused whole.
#[test]
fn a_state_record_of_another_length_or_value_is_refused() {
    let good = [0_u8; CHALLENGE_RECORD_BYTES];
    assert!(read_challenge(&good).is_ok());
    assert_eq!(read_challenge(&good[..32]), Err(RecordFault::Length));
    assert_eq!(read_challenge(&[0_u8; 34]), Err(RecordFault::Length));
    for create in 2..=u8::MAX {
        let mut body = good;
        body[32] = create;
        assert_eq!(read_challenge(&body), Err(RecordFault::BadValue("state challenge create")), "{create}");
    }
    assert_eq!(read_end(&[0_u8; 31]), Err(RecordFault::Length));
    assert_eq!(read_end(&[0_u8; 33]), Err(RecordFault::Length));
    assert_eq!(read_keep_offer(&[0]), Err(RecordFault::Length));
    // Through the frame as well.
    let mut record = [0_u8; 64];
    let length = neoteletext_audio::record::write(TYPE_STATE_END, &[0_u8; 31], &mut record).unwrap();
    assert_eq!(read_record(&record[..length]), Err(RecordFault::Length));
}

/// VS-204: type 8, `SET_COOKIE`'s, is retired: the reader follows no
/// record of it.
#[test]
fn the_retired_set_cookie_type_is_not_read() {
    /// `SET_COOKIE`'s type, retired.
    const RETIRED: u8 = 8;
    let mut record = [0_u8; 64];
    let length = neoteletext_audio::record::write(RETIRED, b"a=1", &mut record).unwrap();
    assert_eq!(read_record(&record[..length]), Err(RecordFault::BadValue("record type")));
}
