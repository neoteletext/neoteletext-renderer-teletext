//! The packet trace of `-vv` (REQ-87, REQ-CLI-002): every packet with its
//! decoded fields and error status, for a command that reads a session and
//! for `convert --to t42`, which has none.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test: a failed unwrap or index is a failed test, and test functions need no documentation"
)]

mod common;

use common::{folder, neottx};
use neoteletext_core::consts::DATA_START;
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::{Packet, Triplet, encode};
use neoteletext_core::types::{ControlBits, Magazine, PageNumber, SubCode};

/// Two pages of magazine 1, 100 and 101, a row each, then an X/26 of page
/// 101 whose designation code has two bit errors, which Hamming 8/4 cannot
/// correct, and the header that ends the magazine's last page.
fn stream() -> Vec<Packet> {
    let magazine = Magazine::new(1).unwrap();
    let header = |page| {
        encode::header(magazine, PageNumber(page), SubCode::new(0), ControlBits(0), &[with_odd_parity(b' '); 32])
            .unwrap()
    };
    let row = encode::row(magazine, 1, &[with_odd_parity(b'A'); 40]);
    let mut damaged = encode::triplet_packet(magazine, 26, 0, &[Triplet::default(); 13]);
    damaged.bytes[DATA_START] ^= 0b0000_0110;
    vec![header(0x00), row, header(0x01), row, damaged, header(0xFF)]
}

fn t42(packets: &[Packet]) -> Vec<u8> {
    packets.iter().flat_map(|packet| packet.bytes).collect()
}

/// The packet lines of a trace.
fn packet_lines(trace: &str) -> Vec<&str> {
    trace.lines().filter(|line| line.starts_with("neottx: packet ")).collect()
}

/// REQ-87: `neottx list -vv` prints one line per packet, in order, with its
/// magazine and packet number and the fields read, and marks the damaged
/// designation code; `-v` prints none.
#[test]
fn every_packet_is_traced_with_its_fields_and_errors() {
    let dir = folder("packet_trace_list");
    let input = dir.join("two.t42");
    std::fs::write(&input, t42(&stream())).unwrap();
    let input = input.to_str().unwrap();
    let output = neottx(&["list", input, "-vv"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let trace = String::from_utf8_lossy(&output.stderr);
    let lines = packet_lines(&trace);
    assert_eq!(lines.len(), stream().len(), "{trace}");
    assert!(lines[0].starts_with("neottx: packet 1/0: header: page 100, sub-code 0000"), "{}", lines[0]);
    assert!(lines[1].starts_with("neottx: packet 1/1: display row 1: 0 parity errors"), "{}", lines[1]);
    assert_eq!(
        lines[4],
        "neottx: packet 1/26: designation code uncorrectable; Hamming 8/4: 0 corrected, 1 uncorrectable"
    );
    let quiet = neottx(&["list", input, "-v"]);
    assert_eq!(packet_lines(&String::from_utf8_lossy(&quiet.stderr)), Vec::<&str>::new());
}

/// REQ-87: `neottx convert --to t42 -vv`, which reads no session, prints
/// the same lines.
#[test]
fn convert_to_t42_traces_its_packets() {
    let dir = folder("packet_trace_convert");
    let input = dir.join("two.t42");
    let output_file = dir.join("out.t42");
    std::fs::write(&input, t42(&stream())).unwrap();
    let output =
        neottx(&["convert", input.to_str().unwrap(), "--to", "t42", "-o", output_file.to_str().unwrap(), "-vv"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let trace = String::from_utf8_lossy(&output.stderr);
    let lines = packet_lines(&trace);
    assert_eq!(lines.len(), stream().len(), "{trace}");
    assert!(lines[2].starts_with("neottx: packet 1/0: header: page 101"), "{}", lines[2]);
}
