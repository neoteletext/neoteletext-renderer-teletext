//! REQ-147 (REQ-GEN-003): a decoder's memory is the caller's. Here every
//! store a decoder fills, the pages and the enhancement pages, is a
//! `static` value made at compile time, lent to the decoder; nothing is
//! allocated for it, and what it received is found in those statics after
//! the decoder is gone.
//!
//! The decoder value itself, which borrows those stores, cannot also be a
//! `static`: a `static` can lend a mutable borrow only through a lock, whose
//! guard does not live for the program's whole life, or through unsafe
//! code, which the core forbids. It is a local value of the caller, as any
//! port's would be (DEC-2440).
#![allow(clippy::unwrap_used, missing_docs, reason = "a test")]

use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementStore, EnhancementSubPage, StoredPage};
use neoteletext_core::config::Config;
use neoteletext_core::consts::DATA_BYTES;
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::{PacketMeta, encode};
use neoteletext_core::types::{ControlBits, Instant, PageAddress, SubCode};
use std::sync::Mutex;

/// Page slots of the test's store.
const PAGES: usize = 8;
/// Enhancement sub-pages and packets of the test's enhancement store.
const ENHANCEMENT_SUB_PAGES: usize = 2;
const ENHANCEMENT_PACKETS: usize = 16;

static PAGE_STORE: Mutex<[StoredPage; PAGES]> = Mutex::new([StoredPage::EMPTY; PAGES]);
static SUB_PAGES: Mutex<[EnhancementSubPage; ENHANCEMENT_SUB_PAGES]> =
    Mutex::new([EnhancementSubPage::EMPTY; ENHANCEMENT_SUB_PAGES]);
static PACKETS: Mutex<[EnhancementPacket; ENHANCEMENT_PACKETS]> =
    Mutex::new([EnhancementPacket::EMPTY; ENHANCEMENT_PACKETS]);

fn header(mpp: u16) -> neoteletext_core::packet::Packet {
    let address = PageAddress::from_mpp(mpp).unwrap();
    encode::header(address.magazine, address.page, SubCode::new(0), ControlBits(0), &[with_odd_parity(b' '); 32])
        .unwrap()
}

// Verifies REQ-147: a decoder runs on stores that are `static` values made
// at compile time, and the page it received is held in the static.
#[test]
fn a_decoder_runs_on_static_stores_made_at_compile_time() {
    {
        let mut pages = PAGE_STORE.lock().unwrap();
        let mut sub_pages = SUB_PAGES.lock().unwrap();
        let mut packets = PACKETS.lock().unwrap();
        let enhancement = EnhancementStore::new(&mut *sub_pages, &mut *packets);
        let mut decoder = Decoder::with_enhancement_store(&mut *pages, enhancement, Config::default());
        let mut row = [with_odd_parity(b' '); DATA_BYTES];
        row[0] = with_odd_parity(b'S');
        let address = PageAddress::from_mpp(0x100).unwrap();
        let packets = [header(0x100), encode::row(address.magazine, 1, &row), header(0x1FF)];
        for (time, packet) in (1..).zip(&packets) {
            decoder.push(packet, &PacketMeta::default(), Instant(time), &mut |_| {});
        }
    }
    let pages = PAGE_STORE.lock().unwrap();
    let held = pages.iter().find(|page| page.key.address == PageAddress::from_mpp(0x100).unwrap());
    let held = held.expect("page 100 is in the static store");
    assert_eq!(held.cells[1][0], with_odd_parity(b'S'));
}
