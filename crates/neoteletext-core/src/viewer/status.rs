//! The status area of row 0 (REQ-SEC-005): the requested page, search
//! state and indicators the receiver draws in columns 0 to 6.

use super::{SecurityIndicator, Viewer, ViewerState};
use crate::acq::PageStore;
use crate::consts::{ALPHA_RED, ALPHA_YELLOW, SPACE, STATUS_COLUMNS};
use crate::limits::DECIMAL_DIGIT_MAX;

/// The base of the place's digits.
const DECIMAL_BASE: usize = DECIMAL_DIGIT_MAX as usize + 1;
/// The highest number shown after the whole page number, "P100/nn": two
/// digits.
const TWO_DIGITS: usize = DECIMAL_BASE * DECIMAL_BASE - 1;
/// The highest number shown as "100/nnn", the "P" given up for a third
/// digit; above it the page number is given up too, "P/nnnn" (DEC-2349).
const THREE_DIGITS: usize = DECIMAL_BASE * DECIMAL_BASE * DECIMAL_BASE - 1;
/// Separates the page number from a chosen sub-page's place (DEC-2349).
const PLACE_SEPARATOR: u8 = b'/';
/// Separates the page number from the count of a rotating page's stored
/// sub-pages (DEC-2349).
const COUNT_SEPARATOR: u8 = b'+';
/// Marks a page not recently received (REQ-NAV-006), in place of the
/// separator, or alone after the page number.
const STALE_MARK: u8 = b'?';
/// Where the stale mark stands when no separator is shown.
const STALE_COLUMN: usize = 5;
/// The fewest stored sub-pages whose count is shown.
const COUNTED_FROM: usize = 2;

impl Viewer {
    /// Row 0 columns 0 to 6 (decision E.10, REQ-SEC-005).
    #[must_use]
    pub fn status(&self, store: &PageStore<'_>) -> [u8; STATUS_COLUMNS] {
        let mut status = [SPACE; STATUS_COLUMNS];
        let digits = match self.state {
            ViewerState::Entering1 | ViewerState::Entering2 => self.entry.display(),
            _ => self.requested.ascii(),
        };
        let write = |status: &mut [u8; STATUS_COLUMNS], at: usize, bytes: &[u8]| {
            for (slot, &byte) in status.iter_mut().skip(at).zip(bytes) {
                *slot = byte;
            }
        };
        if self.confirming() {
            write(&mut status, 0, &[ALPHA_YELLOW]);
            write(&mut status, 1, b"LEAVE?");
            return status;
        }
        match self.indicator {
            SecurityIndicator::None => {
                // DEC-2349: a chosen sub-page shows its place among those
                // stored, a rotating page with sub-pages their count, both
                // counted by the store (decision E.122); "?" stands in the
                // separator's place on a stale page.
                let entering = matches!(self.state, ViewerState::Entering1 | ViewerState::Entering2);
                let number = if entering {
                    None
                } else if self.subcode.is_any() {
                    let count = store.subpage_count(self.requested);
                    (count >= COUNTED_FROM).then_some((COUNT_SEPARATOR, count))
                } else {
                    Some((PLACE_SEPARATOR, store.subpage_place(self.requested, self.subcode).unwrap_or(0)))
                };
                if let Some((separator, value)) = number {
                    let separator = if self.stale { STALE_MARK } else { separator };
                    if value <= TWO_DIGITS {
                        write(&mut status, 0, b"P");
                        write(&mut status, 1, &digits);
                        write(&mut status, 4, &[separator]);
                        write(&mut status, 5, &decimal_digits::<2>(value));
                    } else if value <= THREE_DIGITS {
                        write(&mut status, 0, &digits);
                        write(&mut status, 3, &[separator]);
                        write(&mut status, 4, &decimal_digits::<3>(value));
                    } else {
                        write(&mut status, 0, b"P");
                        write(&mut status, 1, &[separator]);
                        write(&mut status, 2, &decimal_digits::<4>(value));
                    }
                } else {
                    write(&mut status, 0, b"P");
                    write(&mut status, 1, &digits);
                    if self.stale {
                        write(&mut status, STALE_COLUMN, &[STALE_MARK]);
                    }
                }
            }
            SecurityIndicator::Unencrypted => {
                // A red marker the page cannot forge: the number and "!".
                write(&mut status, 0, &[ALPHA_RED]);
                write(&mut status, 1, &digits);
                write(&mut status, 5, b"!");
            }
        }
        status
    }
}

/// The last `N` decimal digits of `value`, as ASCII.
fn decimal_digits<const N: usize>(value: usize) -> [u8; N] {
    let mut out = [b'0'; N];
    let mut rest = value;
    for slot in out.iter_mut().rev() {
        *slot = b'0'.saturating_add(rest.checked_rem(DECIMAL_BASE).unwrap_or(0) as u8);
        rest = rest.checked_div(DECIMAL_BASE).unwrap_or(0);
    }
    out
}
