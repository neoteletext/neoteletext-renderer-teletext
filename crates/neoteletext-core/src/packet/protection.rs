//! How much of a packet's error protection had to act (REQ-ARCH-007,
//! REQ-27, REQ-29): the Hamming 8/4 bytes and Hamming 24/18 triplets of a
//! packet, each counted as clean, corrected or uncorrectable. A pure
//! function of one packet (layer L1), so the decoder can count what the
//! packet decoder itself does not report.

use super::Packet;
use super::editorial_links::{LINK_BYTES, LINK_CONTROL, LINK_START};
use super::service_data::SERVICE_INITIAL_PAGE;
use crate::consts::{
    ADDRESS_BYTE_0, ADDRESS_BYTE_1, DATA_START, HEADER_DISPLAY_START, PACKET_ENHANCEMENT, PACKET_HEADER, PACKET_LINKS,
    PACKET_MAGAZINE_ENHANCEMENT, PACKET_SERVICE, SERVICE_MAGAZINE, TRIPLET_BYTES, TRIPLETS_PER_PACKET,
};
use crate::ecc::{Status, hamming84_decode, hamming2418_decode};

/// Links of an editorial link packet X/27/0 to X/27/3 (Figure 12).
const EDITORIAL_LINKS: usize = 6;
/// The highest designation code of an editorial link packet (clause 9.6.1).
const EDITORIAL_LAST_DESIGNATION: u8 = 3;
/// Bytes of an initial page reference in packet 8/30 (Figure 12's layout).
const PAGE_REFERENCE_BYTES: usize = 6;

/// What a packet's error protection found.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Protection {
    /// Hamming 8/4 bytes with one error, corrected.
    pub hamming84_corrected: u16,
    /// Hamming 8/4 bytes that could not be decoded.
    pub hamming84_failures: u16,
    /// Hamming 24/18 triplets with one error, corrected.
    pub hamming2418_corrected: u16,
    /// Hamming 24/18 triplets that could not be decoded.
    pub hamming2418_failures: u16,
}

impl Protection {
    fn hamming84(&mut self, packet: &Packet, indices: core::ops::Range<usize>) {
        for index in indices {
            match hamming84_decode(packet.byte(index)).1 {
                Status::Clean => {}
                Status::Corrected => self.hamming84_corrected = self.hamming84_corrected.saturating_add(1),
                Status::Uncorrectable => self.hamming84_failures = self.hamming84_failures.saturating_add(1),
            }
        }
    }

    fn triplets(&mut self, packet: &Packet) {
        let first = DATA_START.saturating_add(1);
        for triplet in 0..TRIPLETS_PER_PACKET {
            let at = first.saturating_add(triplet.saturating_mul(TRIPLET_BYTES));
            let bytes = [packet.byte(at), packet.byte(at.saturating_add(1)), packet.byte(at.saturating_add(2))];
            match hamming2418_decode(bytes).1 {
                Status::Clean => {}
                Status::Corrected => self.hamming2418_corrected = self.hamming2418_corrected.saturating_add(1),
                Status::Uncorrectable => self.hamming2418_failures = self.hamming2418_failures.saturating_add(1),
            }
        }
    }
}

/// Counts the corrections and failures of a packet's protected bytes:
/// the two address bytes of every packet (clause 7.1.2); a header's page
/// number, sub-code and control bytes (clause 9.3.1); the designation code
/// of packets 26 to 31 (clause 7.2.1); the links of X/27/0 to X/27/3 and
/// X/27/0's link control byte (clause 9.6.1, Table 14); the thirteen
/// triplets of X/26, X/27/4 to X/27/7, X/28 and M/29 (clauses 9.4 to 9.6);
/// and the initial page of packet 8/30 (clauses 9.8.1 and 9.8.2). The
/// other bytes of packet 8/30 and every display byte, which parity
/// protects, are not counted here. A packet whose address cannot be
/// decoded is counted for its address alone.
#[must_use]
pub fn protection(packet: &Packet) -> Protection {
    let mut counts = Protection::default();
    counts.hamming84(packet, ADDRESS_BYTE_0..ADDRESS_BYTE_1.saturating_add(1));
    let Ok(address) = super::decode_address(packet) else { return counts };
    let number = address.number;
    let after_designation = DATA_START.saturating_add(1);
    if number == PACKET_HEADER {
        counts.hamming84(packet, DATA_START..HEADER_DISPLAY_START);
        return counts;
    }
    // Rows are parity coded; packets 30 outside magazine 8 and 31 carry
    // independent data services, whose coding is not teletext's.
    let service = number == PACKET_SERVICE && address.magazine.number() == SERVICE_MAGAZINE;
    if !(PACKET_ENHANCEMENT..=PACKET_MAGAZINE_ENHANCEMENT).contains(&number) && !service {
        return counts;
    }
    counts.hamming84(packet, DATA_START..after_designation);
    let designation = crate::ecc::hamming84(packet.byte(DATA_START));
    let last_link = LINK_START.saturating_add(EDITORIAL_LINKS.saturating_mul(LINK_BYTES));
    match (number, designation) {
        (_, None) => {}
        (PACKET_LINKS, Some(code)) if code <= EDITORIAL_LAST_DESIGNATION => {
            let end = if code == 0 { LINK_CONTROL.saturating_add(1) } else { last_link };
            counts.hamming84(packet, LINK_START..end);
        }
        (PACKET_SERVICE, Some(_)) => {
            counts.hamming84(packet, SERVICE_INITIAL_PAGE..SERVICE_INITIAL_PAGE.saturating_add(PAGE_REFERENCE_BYTES));
        }
        _ => counts.triplets(packet),
    }
    counts
}
