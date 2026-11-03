//! The minimal port example run as a port runs it (REQ-31, REQ-ARCH-009):
//! a stream of T42 packets through its data source, key presses through its
//! `Keys`, and the page it prints, using only the platform interface.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

#[path = "../examples/minimal_port.rs"]
mod minimal_port;

use minimal_port::{DigitKeys, run};
use neoteletext_core::consts::{DATA_BYTES, PACKET_BYTES};
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::encode;
use neoteletext_core::port::DataSource;
use neoteletext_core::types::{ControlBits, PageAddress, SubCode};

/// A data source over bytes held in memory, given out a few at a time so
/// the port must gather whole packets.
struct Bytes<'a>(&'a [u8]);

/// The most bytes one read gives: less than a packet.
const READ_SIZE: usize = 17;

impl DataSource for Bytes<'_> {
    type Error = std::io::Error;
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
        let count = buffer.len().min(self.0.len()).min(READ_SIZE);
        buffer[..count].copy_from_slice(&self.0[..count]);
        self.0 = &self.0[count..];
        Ok(count)
    }
}

/// A page of magazine 1 whose row 1 holds `text`, then the next page's
/// header to end it (EN 300 706 clause 7.1: a page ends at the next header
/// of its magazine).
fn page(mpp: u16, text: &[u8], stream: &mut Vec<u8>) {
    let address = PageAddress::from_mpp(mpp).unwrap();
    let header =
        encode::header(address.magazine, address.page, SubCode::new(0), ControlBits(0), &[with_odd_parity(b' '); 32])
            .unwrap();
    stream.extend_from_slice(&header.bytes);
    let mut row = [with_odd_parity(b' '); DATA_BYTES];
    for (slot, &code) in row.iter_mut().zip(text) {
        *slot = with_odd_parity(code);
    }
    stream.extend_from_slice(&encode::row(address.magazine, 1, &row).bytes);
}

/// Pages 100 (blank) and 101 (a block of solid mosaics), and a closing
/// header, as a T42 stream.
fn stream() -> Vec<u8> {
    let mut stream = Vec::new();
    page(0x100, b"", &mut stream);
    // Graphics white (0x17), then solid mosaics (0x7F) across the row.
    let mut solid = vec![0x17];
    solid.extend([0x7F; 20]);
    page(0x101, &solid, &mut stream);
    page(0x1FF, b"", &mut stream);
    assert_eq!(stream.len() % PACKET_BYTES, 0);
    stream
}

fn printed(keys: &str) -> String {
    let bytes = stream();
    let mut out = Vec::new();
    run(&mut Bytes(&bytes), &mut DigitKeys::new(keys), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

// Verifies REQ-31: the example port, using only the platform interface,
// decodes a stream, follows the key presses `1`, `0`, `1` to page 101
// through `Keys` and the viewer, and prints that page with the block of
// mosaics on its row 1; with no keys it shows the initial page, 100, blank.
#[test]
fn the_minimal_port_shows_the_page_its_keys_select() {
    let chosen = printed("101");
    let lines: Vec<&str> = chosen.lines().collect();
    assert_eq!(lines[0], "page 101");
    // Three printed lines per text row; row 1 is lines 4 to 6 after the title.
    let row_1 = &lines[4..7];
    assert!(row_1.iter().all(|line| line.contains("####################")), "{chosen}");

    let initial = printed("");
    assert_eq!(initial.lines().next(), Some("page 100"));
    assert!(initial.lines().skip(4).take(3).all(|line| !line.contains('#')), "{initial}");

    assert_eq!(printed("777").lines().next(), Some("page 777 not found"));
}
