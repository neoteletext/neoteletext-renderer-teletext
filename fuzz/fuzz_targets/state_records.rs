//! Viewer state's records, server to receiver (DEC-6471, VS-204; REQ-TST-040):
//! the readers never panic, on a record's body or on packets, and every
//! `STATE_CHALLENGE`, `STATE_END` and `STATE_KEEP_OFFER` they accept, written
//! again by its encoder, reads back the same.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::consts::PACKET_BYTES;
use neoteletext_core::packet::Packet;
use neoteletext_web::records::state::{
    encode_challenge, encode_end, encode_keep_offer, read_challenge, read_end, read_keep_offer,
};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

/// A service any record may be written on.
const SERVICE: u8 = 0;

/// The records `packets` hold, read back.
fn read_back(packets: &[Packet]) -> Vec<String> {
    let mut reader = RecordReader::new();
    let mut read = Vec::new();
    for packet in packets {
        match reader.push(packet) {
            Read::Record(record) => read.push(format!("{record:?}")),
            Read::Invalid(count) => panic!("{count} packets of a record its encoder wrote were refused"),
            Read::Nothing => {}
        }
    }
    read
}

fn written(encode: impl FnOnce(&mut dyn FnMut(Packet))) -> Vec<Packet> {
    let mut packets = Vec::new();
    encode(&mut |packet| packets.push(packet));
    packets
}

fuzz_target!(|data: &[u8]| {
    let Some((&choice, body)) = data.split_first() else { return };
    match choice % 4 {
        0 => {
            let Ok(challenge) = read_challenge(body) else { return };
            let packets = written(|out| {
                encode_challenge(challenge, SERVICE, 0, out).expect("what was read is written");
            });
            assert_eq!(read_back(&packets), [format!("{:?}", ChannelRecord::StateChallenge(challenge))]);
        }
        1 => {
            let Ok(end) = read_end(body) else { return };
            let packets = written(|out| {
                encode_end(end, SERVICE, 0, out).expect("what was read is written");
            });
            assert_eq!(read_back(&packets), [format!("{:?}", ChannelRecord::StateEnd(end))]);
        }
        2 => {
            if read_keep_offer(body).is_err() {
                return;
            }
            let packets = written(|out| {
                encode_keep_offer(SERVICE, 0, out).expect("an empty record is written");
            });
            assert_eq!(read_back(&packets), [format!("{:?}", ChannelRecord::StateKeepOffer)]);
        }
        _ => {
            // Packets: the reader never panics, whatever arrives.
            let mut reader = RecordReader::new();
            for &bytes in body.as_chunks::<PACKET_BYTES>().0 {
                let _ = reader.push(&Packet { bytes });
            }
        }
    }
});
