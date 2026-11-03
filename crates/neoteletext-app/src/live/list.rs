//! A page's list of sub-pages as the decoder side keeps it (REQ-1888,
//! DOC-2038): only the parts stepping has needed, so that any of a page's
//! 8,190 sub-pages is reached without the whole list (REQ-328), and in the
//! port, since the `no_std` viewer holds no list (decision E.122).

use crate::pipe::{ListPart, MAX_LIST_ROWS};
use crate::source::SubPageStep;
use neoteletext_core::types::SubCode;
use std::collections::{BTreeMap, BTreeSet};

/// The most parts kept at once: the part of the sub-page shown, the ones
/// either side of it that a step reaches, and one more being found.
const MAX_HELD_PARTS: usize = 4;

/// The list of one page.
#[derive(Clone, Debug)]
pub(super) struct PageList {
    /// The page, `0xMPP`.
    pub(super) page: u16,
    /// The number of sub-pages and the version, once a part has said.
    shape: Option<(u16, String)>,
    /// The parts held: each one's rows' sub-codes, ascending.
    parts: BTreeMap<u8, Vec<u16>>,
    /// Parts asked for and not yet answered.
    asked: BTreeSet<u8>,
    /// The page has no list, or a part of it could not be had: stepping
    /// goes through the store.
    unusable: bool,
}

/// Where a sub-code lies in a list.
enum Place {
    /// In this part, at this row.
    Found(u8, usize),
    /// In a part not held, which is to be asked for first.
    Ask(u8),
    /// Nowhere the list says.
    Absent,
}

impl PageList {
    /// Nothing known of `page`'s list yet.
    pub(super) fn new(page: u16) -> Self {
        Self { page, shape: None, parts: BTreeMap::new(), asked: BTreeSet::new(), unusable: false }
    }

    /// How many parts the list has.
    fn part_count(&self) -> Option<usize> {
        self.shape.as_ref().map(|(total, _)| usize::from(*total).div_ceil(MAX_LIST_ROWS))
    }

    /// Takes a part that arrived. A part of another version, or of another
    /// size, begins the list again; a part whose rows do not ascend, or that
    /// holds more rows than its place in the list allows, makes the list
    /// unusable (REQ-1890, REQ-1893).
    pub(super) fn take(&mut self, part: &ListPart) {
        self.asked.remove(&part.part);
        let shape = (part.total, part.version.clone());
        if self.shape.as_ref() != Some(&shape) {
            self.parts.clear();
            self.shape = Some(shape);
        }
        let ascending = part.subcodes.windows(2).all(|pair| pair.first() < pair.last());
        let first_place = usize::from(part.part).saturating_mul(MAX_LIST_ROWS);
        let fits = first_place.saturating_add(part.subcodes.len()) <= usize::from(part.total);
        let in_list = self.part_count().is_some_and(|count| usize::from(part.part) < count);
        if !ascending || !fits || !in_list || part.subcodes.is_empty() {
            self.unusable = true;
            return;
        }
        self.parts.insert(part.part, part.subcodes.clone());
        while self.parts.len() > MAX_HELD_PARTS {
            // The part farthest from the one that came goes first.
            let farthest = self.parts.keys().copied().max_by_key(|held| held.abs_diff(part.part));
            if let Some(farthest) = farthest {
                self.parts.remove(&farthest);
            }
        }
    }

    /// A part is missing or failed: with no part the page has no list, and
    /// a hole in one leaves it unusable; either way stepping uses the store.
    pub(super) fn missing(&mut self, part: u8) {
        self.asked.remove(&part);
        self.unusable = true;
    }

    /// Where `subcode` lies. A part not held between the parts that bracket
    /// it is asked for at the middle of the run, so that a sub-code far into
    /// a long list is found in a few requests.
    fn place(&self, subcode: u16) -> Place {
        let Some(count) = self.part_count() else { return Place::Ask(0) };
        let mut run: Option<(usize, usize)> = None;
        for number in 0..count {
            let Ok(key) = u8::try_from(number) else { break };
            if let Some(rows) = self.parts.get(&key) {
                if let Ok(row) = rows.binary_search(&subcode) {
                    return Place::Found(key, row);
                }
                if rows.first().is_some_and(|first| subcode < *first) {
                    break;
                }
                run = None;
            } else {
                let start = run.map_or(number, |(start, _)| start);
                run = Some((start, number));
            }
        }
        match run {
            Some((start, end)) => {
                let middle = start.saturating_add(end.saturating_sub(start) / 2);
                u8::try_from(middle).map_or(Place::Absent, Place::Ask)
            }
            None => Place::Absent,
        }
    }

    /// Whether a part is held, asking for it once when it is not.
    fn ensure(&mut self, part: u8, ask: &mut dyn FnMut(u8)) -> bool {
        if self.parts.contains_key(&part) {
            return true;
        }
        if self.asked.insert(part) {
            ask(part);
        }
        false
    }

    /// The first row of `part`, or its last, once it is held.
    fn end_of(&mut self, part: u8, first: bool, ask: &mut dyn FnMut(u8)) -> SubPageStep {
        if !self.ensure(part, ask) {
            return SubPageStep::Waiting;
        }
        let rows = self.parts.get(&part);
        let row = if first { rows.and_then(|rows| rows.first()) } else { rows.and_then(|rows| rows.last()) };
        row.map_or(SubPageStep::NoList, |&subcode| SubPageStep::To(SubCode::new(subcode)))
    }

    /// The sub-page after or before `current`, wrapping at either end, in
    /// the list's ascending order (DEC-1526, REQ-1890); `ask` is called for
    /// each part the step needs and the list does not hold.
    pub(super) fn step(&mut self, current: SubCode, forward: bool, ask: &mut dyn FnMut(u8)) -> SubPageStep {
        if self.unusable {
            return SubPageStep::NoList;
        }
        let Some(count) = self.part_count() else {
            return if self.ensure(0, ask) { SubPageStep::NoList } else { SubPageStep::Waiting };
        };
        let last_part = u8::try_from(count.saturating_sub(1)).unwrap_or(u8::MAX);
        if current.is_any() {
            return if forward { self.end_of(0, true, ask) } else { self.end_of(last_part, false, ask) };
        }
        let (part, row) = match self.place(current.value()) {
            Place::Found(part, row) => (part, row),
            Place::Ask(part) => {
                self.ensure(part, ask);
                return SubPageStep::Waiting;
            }
            // Not a sub-page the list names: from its start, or its end.
            Place::Absent => {
                return if forward { self.end_of(0, true, ask) } else { self.end_of(last_part, false, ask) };
            }
        };
        let rows = self.parts.get(&part).map_or(0, Vec::len);
        let neighbour = if forward { row.checked_add(1).filter(|&next| next < rows) } else { row.checked_sub(1) };
        if let Some(next) = neighbour
            && let Some(&subcode) = self.parts.get(&part).and_then(|rows| rows.get(next))
        {
            return SubPageStep::To(SubCode::new(subcode));
        }
        if forward {
            let next = if part >= last_part { 0 } else { part.saturating_add(1) };
            self.end_of(next, true, ask)
        } else {
            let previous = if part == 0 { last_part } else { part.saturating_sub(1) };
            self.end_of(previous, false, ask)
        }
    }
}

#[cfg(test)]
mod tests;
