//! Which lines are expected to carry teletext (DEC-1092, REQ-447): the
//! one-bit framing tolerance of decision E.24 applies on those lines alone,
//! and every other line must match the framing code exactly.
//!
//! A line is expected when [`EXPECTED_LINE_EXACT_PACKETS`] packets with an
//! exact framing code and a valid address came on it, each within
//! [`EXPECTED_LINE_FIELDS`] fields of the one before, the last within that
//! many fields of now. A packet accepted through the tolerance counts for
//! nothing: noise passes the tolerance often enough to keep a line expected
//! by itself.
//!
//! A line is known by its television line number, or failing that by its
//! field and its place within the field. Fields are counted by watching
//! those change: a line of the other field, or a line no later than the
//! one before, starts a new field.

use crate::consts::{EXPECTED_LINE_EXACT_PACKETS, EXPECTED_LINE_FIELDS, EXPECTED_LINE_SLOTS, MAX_LINES_PER_FIELD};

use super::line::Line;

/// The exact packets seen on one line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Exact {
    /// The field of the last, 0 for none (fields are counted from 1).
    field: u32,
    /// Exact packets in a row, each within [`EXPECTED_LINE_FIELDS`] of the
    /// one before, at most [`EXPECTED_LINE_EXACT_PACKETS`].
    run: u8,
}

/// The expectation of each line, and the count of fields.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct ExpectedLines {
    lines: [Exact; EXPECTED_LINE_SLOTS],
    /// The field of the line last seen. It stops at `u32::MAX`, after more
    /// than two years of fields.
    field: u32,
    /// The last line seen: its field flag and its place.
    previous: Option<(Option<bool>, usize)>,
}

impl ExpectedLines {
    pub(super) const fn new() -> Self {
        Self { lines: [Exact { field: 0, run: 0 }; EXPECTED_LINE_SLOTS], field: 1, previous: None }
    }

    /// Notes a line arriving and returns its slot, `None` when the line
    /// cannot be told from the others.
    pub(super) fn arrive(&mut self, line: &Line<'_>) -> Option<usize> {
        let (place, slot) = if let Some(number) = line.line {
            let number = usize::from(number);
            (number, number)
        } else {
            let position = usize::from(line.position_in_field?);
            let field_base = if line.second_field == Some(true) { MAX_LINES_PER_FIELD } else { 0 };
            (position, position.checked_add(field_base)?)
        };
        if let Some((field_flag, previous_place)) = self.previous
            && (field_flag != line.second_field || place <= previous_place)
        {
            self.field = self.field.saturating_add(1);
        }
        self.previous = Some((line.second_field, place));
        (slot < EXPECTED_LINE_SLOTS && place < EXPECTED_LINE_SLOTS).then_some(slot)
    }

    /// Whether `field` is recent enough to count, seen from now.
    fn recent(&self, field: u32) -> bool {
        field != 0 && self.field.saturating_sub(field) <= EXPECTED_LINE_FIELDS
    }

    /// True when the slot's line carried teletext recently enough.
    pub(super) fn is_expected(&self, slot: Option<usize>) -> bool {
        slot.and_then(|slot| self.lines.get(slot))
            .is_some_and(|exact| exact.run >= EXPECTED_LINE_EXACT_PACKETS && self.recent(exact.field))
    }

    /// Notes an exact packet on the slot's line.
    pub(super) fn exact_packet(&mut self, slot: Option<usize>) {
        let now = self.field;
        let recent = slot.and_then(|slot| self.lines.get(slot)).is_some_and(|exact| self.recent(exact.field));
        if let Some(exact) = slot.and_then(|slot| self.lines.get_mut(slot)) {
            exact.run = if recent { exact.run.saturating_add(1).min(EXPECTED_LINE_EXACT_PACKETS) } else { 1 };
            exact.field = now;
        }
    }
}
