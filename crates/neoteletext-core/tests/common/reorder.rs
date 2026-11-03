//! The generator of the non-standard orders of REQ-FSM-008 (REQ-140) over
//! the packets of a page transmission, for the reordering tests of
//! REQ-FSM-009 (REQ-141). A transmission is its header and then its packets
//! in standard order: packets 26, 27 and 28 before the rows, designation
//! codes ascending, rows ascending.

use core::fmt::Write as _;

use neoteletext_core::consts::DATA_START;
use neoteletext_core::ecc::{hamming84, hamming84_encode};
use neoteletext_core::packet::{Packet, decode_address};

/// The deterministic shuffles made of each transmission besides the named
/// orders: enough to put every packet before and after every other in some
/// order, few enough to keep the test quick.
pub const SHUFFLES: u64 = 24;

/// The first packet number that is not a row: 26, 27 and 28 follow.
const FIRST_EXTRA: u8 = 26;
/// The last packet number a page carries.
const LAST_EXTRA: u8 = 28;
/// The highest designation code (Hamming 8/4 carries four bits).
const LAST_DESIGNATION: u8 = 15;

fn number(packet: &Packet) -> u8 {
    decode_address(packet).expect("an encoded packet's address decodes").number
}

fn is_extra(packet: &Packet) -> bool {
    (FIRST_EXTRA..=LAST_EXTRA).contains(&number(packet))
}

fn designation(packet: &Packet) -> u8 {
    hamming84(packet.byte(DATA_START)).expect("an encoded designation code decodes")
}

/// A small linear congruential generator (Knuth's MMIX constants): the
/// shuffles must be the same on every run, and no library is wanted for it.
struct Lcg(u64);

impl Lcg {
    const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
    const INCREMENT: u64 = 1_442_695_040_888_963_407;

    fn below(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_mul(Self::MULTIPLIER).wrapping_add(Self::INCREMENT);
        // The high half is the better-mixed part of an LCG's state.
        ((self.0 >> 32) % bound as u64) as usize
    }
}

/// `items` in a Fisher-Yates shuffle drawn from `seed`.
fn shuffled(items: &[Packet], seed: u64) -> Vec<Packet> {
    let mut out = items.to_vec();
    let mut random = Lcg(seed);
    for last in (1..out.len()).rev() {
        out.swap(last, random.below(last + 1));
    }
    out
}

/// `first` and `second` taken in turn, one packet from each.
fn alternate(first: &[Packet], second: &[Packet]) -> Vec<Packet> {
    let mut out = Vec::with_capacity(first.len() + second.len());
    for index in 0..first.len().max(second.len()) {
        out.extend(first.get(index).copied());
        out.extend(second.get(index).copied());
    }
    out
}

/// Every non-standard order of REQ-FSM-008 for one page transmission, each
/// named: rows in any order, packets 26 to 28 after or among the rows,
/// designation codes in any order, and shuffles of the whole. The header
/// stays first, as the standard makes the header that starts and ends a
/// page significant.
pub fn page_orders(transmission: &[Packet]) -> Vec<(String, Vec<Packet>)> {
    let (header, body) = transmission.split_first().expect("a transmission starts with its header");
    let rows: Vec<Packet> = body.iter().copied().filter(|packet| !is_extra(packet)).collect();
    let extras: Vec<Packet> = body.iter().copied().filter(is_extra).collect();
    let with_header = |packets: Vec<Packet>| [vec![*header], packets].concat();
    let mut descending = extras.clone();
    descending.sort_by_key(|packet| (number(packet), core::cmp::Reverse(designation(packet))));
    let mut reversed_rows = rows.clone();
    reversed_rows.reverse();
    let mut orders = vec![
        ("rows in reverse order".to_owned(), with_header([extras.clone(), reversed_rows].concat())),
        ("packets 26 to 28 after the rows".to_owned(), with_header([rows.clone(), extras.clone()].concat())),
        ("packets 26 to 28 among the rows".to_owned(), with_header(alternate(&rows, &extras))),
        ("designation codes descending".to_owned(), with_header([descending, rows.clone()].concat())),
        ("every packet in reverse order".to_owned(), with_header(body.iter().rev().copied().collect())),
    ];
    for seed in 1..=SHUFFLES {
        orders.push((format!("shuffle {seed}"), with_header(shuffled(body, seed))));
    }
    orders
}

/// The transmission with gaps in the designation codes of its packets X/26:
/// each renumbered from code c to 2c, when every code stays at or below 15.
/// Only X/26's codes are a sequence; those of X/27 and X/28 name what the
/// packet holds. The gapped page in another order is compared with the
/// gapped page in standard order, as the codes may name positions (local
/// objects).
pub fn with_gaps(transmission: &[Packet]) -> Option<Vec<Packet>> {
    let is_x26 = |packet: &&mut Packet| number(packet) == FIRST_EXTRA;
    let mut out = transmission.to_vec();
    let codes: Vec<u8> = out.iter_mut().skip(1).filter(is_x26).map(|packet| designation(packet)).collect();
    if codes.is_empty() || codes.iter().any(|&code| code * 2 > LAST_DESIGNATION) {
        return None;
    }
    for packet in out.iter_mut().skip(1).filter(is_x26) {
        packet.bytes[DATA_START] = hamming84_encode(designation(packet) * 2);
    }
    Some(out)
}

/// Two transmissions interleaved packet by packet, as parallel mode allows
/// for pages of different magazines.
pub fn interleaved(first: &[Packet], second: &[Packet]) -> Vec<Packet> {
    alternate(first, second)
}

/// Every order of the pages of a service: each permutation of `pages`, the
/// pages sent one after the other, each in standard order or in one of its
/// own orders, chosen in turn so that each page meets each of its orders.
pub fn service_orders(pages: &[Vec<Packet>]) -> Vec<(String, Vec<Packet>)> {
    let mut out = Vec::new();
    let page_orders: Vec<Vec<(String, Vec<Packet>)>> = pages.iter().map(|page| page_orders(page)).collect();
    for (permutation_index, permutation) in permutations(pages.len()).into_iter().enumerate() {
        let widest = page_orders.iter().map(Vec::len).max().unwrap_or(0);
        for order in 0..widest {
            let mut name = format!("pages {permutation:?}");
            let mut packets = Vec::new();
            for &page in &permutation {
                let orders = &page_orders[page];
                // Each permutation starts at a different order, so that
                // over them the pages' orders are mixed.
                let (order_name, order_packets) = &orders[(order + permutation_index) % orders.len()];
                let _ = write!(name, ", page {page} {order_name}");
                packets.extend_from_slice(order_packets);
            }
            out.push((name, packets));
        }
    }
    out
}

/// Every permutation of `0..count`, in lexicographic order.
fn permutations(count: usize) -> Vec<Vec<usize>> {
    if count == 0 {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    for first in 0..count {
        for rest in permutations(count - 1) {
            let mut permutation = vec![first];
            permutation.extend(rest.into_iter().map(|index| if index >= first { index + 1 } else { index }));
            out.push(permutation);
        }
    }
    out
}
