//! The packet trace of `-vv` (REQ-87, REQ-CLI-002): one line for every
//! packet, with its magazine and packet number, its designation code where
//! it has one, the fields the packet decoder read from it, and what its
//! error protection found (EN 300 706 clause 8: Hamming 8/4 for addresses,
//! headers' page numbers and control bits and designation codes, Hamming
//! 24/18 for triplets, odd parity for display bytes). Every command that
//! reads packets prints it, through the session or, for `convert --to
//! t42`, which has none, directly.
//!
//! The lines are made of the decoder's numbers only, never of the
//! packet's display bytes, so nothing a page holds reaches the terminal
//! (REQ-SEC-010).

use neoteletext_core::consts::{
    DATA_START, HEADER_DISPLAY_START, PACKET_BYTES, PACKET_ENHANCEMENT, PACKET_SERVICE, SERVICE_MAGAZINE,
};
use neoteletext_core::ecc::{hamming84, has_odd_parity};
use neoteletext_core::packet::{
    Decoded, MagazinePacket, Packet, PacketError, PagePacket, decode, decode_address, protection,
};
use neoteletext_core::types::{Magazine, PageAddress, PageNumber};
use std::fmt::Write as _;

/// The display bytes of `packet` from `start` that fail their odd parity
/// (clause 8.1).
fn parity_failures(packet: &Packet, start: usize) -> usize {
    packet.bytes.get(start..PACKET_BYTES).unwrap_or_default().iter().filter(|&&byte| !has_odd_parity(byte)).count()
}

/// A page number as the user types it, `mpp`.
fn page(magazine: Magazine, page: PageNumber) -> String {
    format!("{:03X}", PageAddress { magazine, page }.mpp())
}

/// What the packet decoder read from a packet whose address decoded.
fn fields(decoded: &Decoded, packet: &Packet) -> String {
    match decoded {
        Decoded::Header(header) => {
            let number = header.page.map_or_else(|| "uncorrectable".to_owned(), |number| page(header.magazine, number));
            let subcode =
                header.subcode.map_or_else(|| "uncorrectable".to_owned(), |subcode| format!("{:04X}", subcode.value()));
            let control =
                header.control.map_or_else(|| "uncorrectable".to_owned(), |control| format!("{:#06X}", control.0));
            let failures = parity_failures(packet, HEADER_DISPLAY_START);
            format!("header: page {number}, sub-code {subcode}, control bits {control}, {failures} parity errors")
        }
        Decoded::Row(row) => format!("display row {}: {} parity errors", row.row, parity_failures(packet, DATA_START)),
        Decoded::Enhancement(_) => "enhancement data".to_owned(),
        Decoded::Links(links) => {
            let count = links.links.iter().flatten().count();
            format!(
                "editorial links: {count} links, display row 24 {}",
                if links.display_row_24 { "on" } else { "off" }
            )
        }
        Decoded::LinksRaw { .. } => "editorial links, stored as they are".to_owned(),
        Decoded::CompositionalLinks { links, .. } => {
            format!("compositional links: {} readable", links.iter().flatten().count())
        }
        Decoded::PageData { packet, .. } => match packet {
            PagePacket::Format1(_) => "page enhancement data".to_owned(),
            PagePacket::PageFunction(format) => match format {
                Some(format) => format!("page function {}, coding {}", format.function, format.coding),
                None => "page function uncorrectable".to_owned(),
            },
            PagePacket::Level35(_) => "Level 3.5 page enhancement data".to_owned(),
        },
        Decoded::Magazine { packet, .. } => match packet {
            MagazinePacket::Data(_) => "magazine enhancement data".to_owned(),
            MagazinePacket::Level35(_) => "Level 3.5 magazine enhancement data".to_owned(),
        },
        Decoded::Service(service) => match service.initial_page {
            Some((magazine, number, subcode)) => {
                format!(
                    "broadcast service data: initial page {} sub-code {:04X}",
                    page(magazine, number),
                    subcode.value()
                )
            }
            None => "broadcast service data: initial page uncorrectable".to_owned(),
        },
        Decoded::Ignored(_) => "not used by the decoder".to_owned(),
    }
}

/// One line of the packet trace for `packet`: `packet M/N[/D]: fields;
/// protection`, where protection is `clean` or the Hamming corrections and
/// failures.
#[must_use]
pub fn line(packet: &Packet) -> String {
    let counts = protection(packet);
    let mut out = match decode_address(packet) {
        Err(_) => "packet ?/?: address uncorrectable".to_owned(),
        Ok(address) => {
            let mut out = format!("packet {}/{}", address.magazine.number(), address.number);
            // Clause 7.2.1: packets 26 to 29, and 8/30, carry a designation
            // code; other packets 30 and 31 are independent data.
            let designated = (PACKET_ENHANCEMENT..PACKET_SERVICE).contains(&address.number)
                || (address.number == PACKET_SERVICE && address.magazine.number() == SERVICE_MAGAZINE);
            if designated && let Some(designation) = hamming84(packet.byte(DATA_START)) {
                let _ = write!(out, "/{designation}");
            }
            match decode(packet) {
                Ok(decoded) => {
                    let _ = write!(out, ": {}", fields(&decoded, packet));
                }
                Err(PacketError::BadDesignation) => out.push_str(": designation code uncorrectable"),
                Err(PacketError::BadAddress) => out.push_str(": address uncorrectable"),
            }
            out
        }
    };
    let hamming = [
        ("Hamming 8/4", counts.hamming84_corrected, counts.hamming84_failures),
        ("Hamming 24/18", counts.hamming2418_corrected, counts.hamming2418_failures),
    ];
    let mut clean = true;
    for (code, corrected, failed) in hamming {
        if corrected > 0 || failed > 0 {
            let _ = write!(out, "; {code}: {corrected} corrected, {failed} uncorrectable");
            clean = false;
        }
    }
    if clean {
        out.push_str("; protection clean");
    }
    out
}

#[cfg(test)]
mod tests;
