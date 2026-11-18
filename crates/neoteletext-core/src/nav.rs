//! Navigation (area NAV, layer L6): page arithmetic, links and history.

use crate::acq::{PageStore, StoredPage};
use crate::consts::EDITORIAL_INDEX_LINK;
use crate::consts::{FIRST_PAGE, LAST_PAGE, LINK_NULL_PAGE, MAGAZINE_ZERO_MEANS};
use crate::limits::{
    DECIMAL_DIGIT_MAX, KEYABLE_PAGES, MPP_MAGAZINE_SHIFT, PAGE_DIGIT_MAX, PAGE_NUMBERS, PAGE_TENS_SHIFT, S2_MASK,
    S4_MASK, SUBCODE_DIGITS, decimal_sub_code, is_reserved_page,
};
use crate::packet::Link;
use crate::types::{FixedVec, PageAddress, PageNumber, SubCode};

/// A link target: a page, a sub-code (3F7F for any), and the service it
/// belongs to (`None` for the current service).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkTarget {
    /// The page.
    pub address: PageAddress,
    /// The sub-code, or `SubCode::ANY`.
    pub subcode: SubCode,
    /// Index of another service in the descriptor's list (Appendix E), or
    /// `None` for this service.
    pub service: Option<u8>,
}

/// The links of a page: four coloured keys and Index (decision E.36,
/// REQ-NAV-003).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PageLinks {
    /// Red, green, yellow and cyan.
    pub keys: [Option<LinkTarget>; 4],
    /// The Index key.
    pub index: Option<LinkTarget>,
}

impl PageLinks {
    /// The links of a broadcast page, from packet X/27/0 (REQ-NAV-003).
    #[must_use]
    pub fn from_page(page: &StoredPage) -> Self {
        let Some(links) = page.links else { return Self::default() };
        let target = |index: usize| link_target(links.links.get(index).copied().flatten());
        Self { keys: [target(0), target(1), target(2), target(3)], index: target(EDITORIAL_INDEX_LINK) }
    }

    /// True when no coloured key has a link.
    #[must_use]
    pub fn no_coloured_links(&self) -> bool {
        self.keys.iter().all(Option::is_none)
    }
}

/// What an editorial link of X/27/0 to X/27/3 names (clause 9.6.1;
/// REQ-NUM-007, REQ-NUM-018, DEC-2344), the one rule for every kind: the
/// null address XFF:3F7F is no link at all, so its key is disabled (the
/// decoder gives `None`, REQ-PKT-005); page number FF names no page; a
/// sub-code of 3F7F names the page with no particular sub-page
/// (`SubCode::ANY`).
#[must_use]
pub fn link_target(link: Option<Link>) -> Option<LinkTarget> {
    let link = link.filter(|link| link.page.0 != LINK_NULL_PAGE)?;
    let mpp = (u16::from(link.magazine.number()) << MPP_MAGAZINE_SHIFT) | u16::from(link.page.0);
    PageAddress::from_mpp(mpp).map(|address| LinkTarget { address, subcode: link.subcode, service: None })
}

/// The page after `address` in numerical order; 899 wraps to 100
/// (REQ-NAV-002). Hexadecimal pages are skipped.
#[must_use]
pub fn next_page(address: PageAddress) -> PageAddress {
    step(address, true)
}

/// The page before `address` in numerical order; 100 wraps to 899.
#[must_use]
pub fn previous_page(address: PageAddress) -> PageAddress {
    step(address, false)
}

fn decimal(value: u16) -> Option<PageAddress> {
    let hundreds = value / 100;
    let tens = (value / 10) % 10;
    let units = value % 10;
    // Magazines above 8 do not exist; from_mpp rejects them.
    PageAddress::from_mpp((hundreds << 8) | (tens << 4) | units)
}

fn to_decimal(address: PageAddress) -> u16 {
    let page = address.page;
    u16::from(address.magazine.number())
        .saturating_mul(100)
        .saturating_add(u16::from(page.tens().min(9)).saturating_mul(10))
        .saturating_add(u16::from(page.units().min(9)))
}

/// The number a viewer reads in a decimal `0xMPP`: 0x100 is 100.
const fn decimal_number(mpp: u16) -> u16 {
    let base = DECIMAL_DIGIT_MAX as u16 + 1;
    let hundreds = (mpp >> MPP_MAGAZINE_SHIFT) & PAGE_DIGIT_MAX as u16;
    let tens = (mpp >> PAGE_TENS_SHIFT) & PAGE_DIGIT_MAX as u16;
    let units = mpp & PAGE_DIGIT_MAX as u16;
    hundreds.wrapping_mul(base).wrapping_add(tens).wrapping_mul(base).wrapping_add(units)
}

/// First and last pages as decimal numbers.
const FIRST_DECIMAL: u16 = decimal_number(FIRST_PAGE);
const LAST_DECIMAL: u16 = decimal_number(LAST_PAGE);
/// Number of keyable pages, bounding the search for a stored page.
const PAGE_COUNT: u16 = KEYABLE_PAGES as u16;
const _: () = assert!(LAST_DECIMAL - FIRST_DECIMAL + 1 == PAGE_COUNT);

fn step(address: PageAddress, forward: bool) -> PageAddress {
    let value = to_decimal(address);
    let next = if forward {
        if value >= LAST_DECIMAL { FIRST_DECIMAL } else { value.saturating_add(1) }
    } else if value <= FIRST_DECIMAL {
        LAST_DECIMAL
    } else {
        value.saturating_sub(1)
    };
    decimal(next).or_else(|| PageAddress::from_mpp(FIRST_PAGE)).unwrap_or(address)
}

/// The index of page 8FF when every page number is counted from 100.
const LAST_PAGE_INDEX: u16 = (PAGE_NUMBERS - 1) as u16;

/// The page after (or before) `address` among every page number that is
/// not reserved, hexadecimal ones included, in the order 100 to 1FF, 200
/// to 8FF, wrapping: navigation under the diagnostic setting of
/// REQ-ACQ-013 (REQ-ACQ-017). Reserved numbers are never offered
/// (REQ-NUM-010).
#[must_use]
pub fn step_any(address: PageAddress, forward: bool) -> PageAddress {
    let count = PAGE_NUMBERS as u16;
    let index = |address: PageAddress| {
        (u16::from(address.magazine.number().saturating_sub(1)) << MPP_MAGAZINE_SHIFT) | u16::from(address.page.0)
    };
    let mut at = index(address);
    for _ in 0..count {
        at = if forward {
            if at >= LAST_PAGE_INDEX { 0 } else { at.saturating_add(1) }
        } else {
            at.checked_sub(1).unwrap_or(LAST_PAGE_INDEX)
        };
        let mpp = at.wrapping_add(1 << MPP_MAGAZINE_SHIFT);
        let magazine = (mpp >> MPP_MAGAZINE_SHIFT) as u8;
        if !is_reserved_page(magazine, mpp as u8)
            && let Some(candidate) = PageAddress::from_mpp(mpp)
        {
            return candidate;
        }
    }
    address
}

/// The next or previous page in numerical order: decimal pages only, or
/// every unreserved number when `hexadecimal` (the diagnostic setting of
/// REQ-ACQ-013, REQ-ACQ-017).
#[must_use]
pub fn step_numerical(address: PageAddress, forward: bool, hexadecimal: bool) -> PageAddress {
    if hexadecimal { step_any(address, forward) } else { step(address, forward) }
}

/// The next or previous display page present in the store (REQ-NAV-002),
/// or the numerical neighbour when none is stored. A data page, by its
/// page function or its MIP code, is not offered (REQ-NUM-027,
/// REQ-NUM-028). Hexadecimal pages are offered only when `hexadecimal`,
/// the diagnostic setting of REQ-ACQ-013 (REQ-ACQ-017). Bounded: at most
/// 800 steps, or 2,048 with hexadecimal pages.
#[must_use]
pub fn step_stored(address: PageAddress, forward: bool, hexadecimal: bool, store: &PageStore<'_>) -> PageAddress {
    let steps = if hexadecimal { PAGE_NUMBERS as u16 } else { PAGE_COUNT };
    let mut candidate = address;
    for _ in 0..steps {
        candidate = step_numerical(candidate, forward, hexadecimal);
        if store.has_display_page(candidate) {
            return candidate;
        }
    }
    step_numerical(address, forward, hexadecimal)
}

/// Three-digit page entry (REQ-NAV-001).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PageEntry {
    digits: [u8; 3],
    count: u8,
}

/// What a digit did to the entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryResult {
    /// The digit was refused: it cannot form a valid page number.
    Rejected,
    /// More digits are needed.
    Partial,
    /// A page number is complete.
    Complete(PageAddress),
}

impl PageEntry {
    /// No digits entered.
    pub const EMPTY: Self = Self { digits: [0; 3], count: 0 };

    /// Number of digits entered so far.
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// Clears the entry.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Adds a digit 0 to 9.
    pub fn push(&mut self, digit: u8) -> EntryResult {
        if digit > 9 {
            return EntryResult::Rejected;
        }
        // The first digit is the magazine, 1 to 8.
        if self.count == 0 && !(1..=MAGAZINE_ZERO_MEANS).contains(&digit) {
            return EntryResult::Rejected;
        }
        if let Some(slot) = self.digits.get_mut(usize::from(self.count)) {
            *slot = digit;
        }
        self.count = self.count.saturating_add(1);
        if self.count < 3 {
            return EntryResult::Partial;
        }
        let [m, t, u] = self.digits;
        self.clear();
        let mpp = (u16::from(m) << 8) | (u16::from(t) << 4) | u16::from(u);
        PageAddress::from_mpp(mpp).map_or(EntryResult::Rejected, EntryResult::Complete)
    }

    /// The digits entered, as ASCII, with `-` for the missing ones.
    #[must_use]
    pub fn display(&self) -> [u8; 3] {
        let mut out = [b'-'; 3];
        for (index, (slot, &digit)) in out.iter_mut().zip(self.digits.iter()).enumerate() {
            if index < usize::from(self.count) {
                *slot = b'0'.saturating_add(digit);
            }
        }
        out
    }
}

/// Four-digit sub-page entry (REQ-NAV-005, REQ-NUM-017): the 3,199 decimal
/// sub-codes 0001 to 3979, S4 0 to 3, S3 0 to 9, S2 0 to 7 and S1 0 to 9.
/// A digit that cannot begin or continue one is refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SubPageEntry {
    digits: [u8; SUBCODE_DIGITS],
    count: u8,
}

/// The highest digit at each place of a decimal sub-code, S4 S3 S2 S1:
/// S4 has two bits and S2 three (clause 9.3.1.2).
const SUBCODE_DIGIT_HIGHEST: [u8; SUBCODE_DIGITS] =
    [S4_MASK as u8, DECIMAL_DIGIT_MAX, S2_MASK as u8, DECIMAL_DIGIT_MAX];

/// What a digit did to a sub-page entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubPageEntryResult {
    /// The digit was refused: no decimal sub-code has it there.
    Rejected,
    /// More digits are needed.
    Partial,
    /// A sub-code is complete.
    Complete(SubCode),
}

impl SubPageEntry {
    /// No digits entered.
    pub const EMPTY: Self = Self { digits: [0; SUBCODE_DIGITS], count: 0 };

    /// Number of digits entered so far.
    #[must_use]
    pub const fn count(&self) -> u8 {
        self.count
    }

    /// Clears the entry.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Adds a digit 0 to 9.
    pub fn push(&mut self, digit: u8) -> SubPageEntryResult {
        let position = usize::from(self.count);
        if SUBCODE_DIGIT_HIGHEST.get(position).is_none_or(|&highest| digit > highest) {
            return SubPageEntryResult::Rejected;
        }
        if let Some(slot) = self.digits.get_mut(position) {
            *slot = digit;
        }
        let count = self.count.saturating_add(1);
        if usize::from(count) < SUBCODE_DIGITS {
            self.count = count;
            return SubPageEntryResult::Partial;
        }
        // The fourth digit completes a sub-code, or is refused with the
        // three before it kept, as for 0000, which is not a decimal
        // sub-code (REQ-NUM-017).
        match decimal_sub_code(self.digits) {
            Some(code) => {
                self.clear();
                SubPageEntryResult::Complete(SubCode::new(code))
            }
            None => SubPageEntryResult::Rejected,
        }
    }

    /// The digits entered so far, for the status area, with `-` for each
    /// still to come.
    #[must_use]
    pub fn display(&self) -> [u8; SUBCODE_DIGITS] {
        let mut out = [b'-'; SUBCODE_DIGITS];
        for (slot, &digit) in out.iter_mut().zip(self.digits.iter()).take(usize::from(self.count)) {
            *slot = b'0'.saturating_add(digit);
        }
        out
    }
}

/// A history entry (REQ-NAV-010).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryEntry {
    /// The service: the port's number for it, its place in the port's
    /// table of the services opened this session (REQ-NAV-010), which
    /// stays the same when other services open; 0 for a broadcast and the
    /// service opened first.
    pub service: u8,
    /// The page.
    pub address: PageAddress,
    /// The sub-page.
    pub subcode: SubCode,
}

/// Back and forward history in caller-sized memory (REQ-NAV-010).
#[derive(Clone, Copy, Debug)]
pub struct History<const N: usize> {
    entries: FixedVec<HistoryEntry, N>,
    position: usize,
}

const EMPTY_ENTRY: HistoryEntry = HistoryEntry {
    service: 0,
    address: PageAddress { magazine: crate::types::Magazine::from_bits(1), page: PageNumber(0) },
    subcode: SubCode::ANY,
};

impl<const N: usize> Default for History<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> History<N> {
    /// An empty history.
    #[must_use]
    pub const fn new() -> Self {
        Self { entries: FixedVec::new(EMPTY_ENTRY), position: 0 }
    }

    /// Records a newly selected page, clearing the forward history. When
    /// full, the oldest entry is dropped.
    pub fn visit(&mut self, entry: HistoryEntry) {
        if self.current() == Some(entry) {
            return;
        }
        self.entries.truncate(self.position);
        if self.entries.is_full() {
            let _ = self.entries.remove(0);
        }
        let _ = self.entries.push(entry);
        self.position = self.entries.len();
    }

    /// Puts `entry` in place of the entry being shown, keeping the forward
    /// history: a selection that leads at once to another page, as a typed
    /// number's link does (REQ-WEB-020), is one visit. With no entry it is
    /// visited.
    pub fn replace_current(&mut self, entry: HistoryEntry) {
        match self.position.checked_sub(1).and_then(|index| self.entries.as_mut_slice().get_mut(index)) {
            Some(current) => *current = entry,
            None => self.visit(entry),
        }
    }

    /// The entry being shown.
    #[must_use]
    pub fn current(&self) -> Option<HistoryEntry> {
        self.position.checked_sub(1).and_then(|index| self.entries.as_slice().get(index).copied())
    }

    /// Moves back, returning the entry to show.
    pub fn back(&mut self) -> Option<HistoryEntry> {
        if self.position <= 1 {
            return None;
        }
        self.position = self.position.saturating_sub(1);
        self.current()
    }

    /// Moves forward, undoing a back.
    pub fn forward(&mut self) -> Option<HistoryEntry> {
        if self.position >= self.entries.len() {
            return None;
        }
        self.position = self.position.saturating_add(1);
        self.current()
    }

    /// The entry Back would move to, without moving.
    #[must_use]
    pub fn peek_back(&self) -> Option<HistoryEntry> {
        // The current entry is at `position - 1`, the one before it at
        // `position - 2`; `back` refuses when `position` is 1 or less.
        let index = self.position.checked_sub(2)?;
        self.entries.as_slice().get(index).copied()
    }

    /// The entry Forward would move to, without moving.
    #[must_use]
    pub fn peek_forward(&self) -> Option<HistoryEntry> {
        // The entry after the current one is at `position`.
        self.entries.as_slice().get(self.position).copied()
    }

    /// True when Back has somewhere to go.
    #[must_use]
    pub const fn can_go_back(&self) -> bool {
        self.position > 1
    }

    /// True when Forward has somewhere to go.
    #[must_use]
    pub const fn can_go_forward(&self) -> bool {
        self.position < self.entries.len()
    }

    /// Erases the history (REQ-NAV-010).
    pub fn wipe(&mut self) {
        self.entries.clear();
        self.position = 0;
    }
}

#[cfg(test)]
mod tests;
