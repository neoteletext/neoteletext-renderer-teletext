//! The LOG record at its bounds (REQ-INP-026, DOC-1956).

use neoteletext_core::packet::Packet;
use neoteletext_core::types::PageAddress;
use neoteletext_formats::log::{LogLevel, MAX_LOG_TEXT_BYTES};

use super::{encode, read};
use crate::records::{ChannelRecord, Read, RecordReader};
use std::string::String;
use std::vec::Vec;

fn round_trip(
    level: LogLevel,
    page: Option<PageAddress>,
    text: &str,
) -> Option<(LogLevel, Option<PageAddress>, String)> {
    let mut packets: Vec<Packet> = Vec::new();
    encode(level, page, text, 4, 0, &mut |packet| packets.push(packet)).ok()?;
    let mut reader = RecordReader::new();
    for packet in &packets {
        if let Read::Record(ChannelRecord::Log(record)) = reader.push(packet) {
            return Some((record.line.level, record.page, String::from(record.line.text)));
        }
    }
    None
}

/// REQ-INP-026, DOC-1956: text of 1 and of 200 bytes, at each level, with
/// and without a page, goes through a record and reads back the same.
#[test]
fn a_log_line_of_1_and_200_bytes_round_trips() {
    let longest: String = core::iter::repeat_n('x', MAX_LOG_TEXT_BYTES).collect();
    assert_eq!(MAX_LOG_TEXT_BYTES, 200);
    let page = PageAddress::from_mpp(0x8FF);
    for (level, page, text) in
        [(LogLevel::Error, None, "e"), (LogLevel::Warning, page, longest.as_str()), (LogLevel::Information, page, "i")]
    {
        assert_eq!(round_trip(level, page, text), Some((level, page, String::from(text))));
    }
}

/// REQ-INP-026, DOC-1956: empty text, 201 bytes, a control character and a
/// bidirectional formatting character are refused by `encode`; and by
/// `read`, those and a level other than E, W and I, and a page neither 0
/// nor a page number.
#[test]
fn a_log_line_past_its_bounds_is_refused() {
    let over: String = core::iter::repeat_n('x', MAX_LOG_TEXT_BYTES + 1).collect();
    let refused = ["", over.as_str(), "bell\u{7}", "right\u{202E}left"];
    for text in refused {
        assert_eq!(round_trip(LogLevel::Error, None, text), None, "encode refuses {text:?}");
        let body = [b"E\0\0".as_slice(), text.as_bytes()].concat();
        assert!(read(&body).is_err(), "read refuses {text:?}");
    }
    assert!(read(b"E\0\0ok").is_ok());
    assert!(read(b"X\0\0ok").is_err(), "a level");
    assert!(read(b"E\x00\x99ok").is_err(), "a page");
    assert!(read(b"E\x09\x00ok").is_err(), "magazine 9");
    assert!(read(b"E\0").is_err(), "cut short");
}
