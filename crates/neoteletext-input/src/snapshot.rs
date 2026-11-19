//! The snapshot request (REQ-6879 items 3 and 4, DEC-6875 item 3;
//! neoteletext-common `docs/tab-thumbnails.md` section 3): the one input
//! message a receiver sends to ask a live channel for one page as a
//! stranger sees it, HELLO offering [`SNAPSHOT_OFFER`] then one SELECT, and
//! the test a receiver applies to WELCOME before it sends anything more.
//! Nothing here allocates.

use crate::VERSION;
use crate::caps::{SNAPSHOT_CAP, SNAPSHOT_OFFER};
use crate::event::{Event, InputError, Item, Select, write};
use crate::wire::WireError;

/// Writes the snapshot request for `page` and `sub` into `out` and returns
/// its length: HELLO (version [`VERSION`], caps [`SNAPSHOT_OFFER`], no
/// agent), then SELECT of the page alone when `sub` is
/// [`SUBCODE_ANY`](neoteletext_core::limits::SUBCODE_ANY) (3F7F, the page as a whole) and of the page and the
/// sub-code otherwise, with no query (DOC-1753). It is one input message:
/// the two items follow each other in `out`.
///
/// # Errors
/// [`InputError::BadValue`] when `page` is not a display page, `0x100` to
/// `0x8FF` (page 0 selects nothing, which is no snapshot), or `sub` is not a
/// sub-code; [`InputError::Wire`] with [`WireError::Full`] when `out` is too
/// small.
pub fn write_request(page: u16, sub: u16, out: &mut [u8]) -> Result<usize, InputError> {
    if !crate::is_page(page) {
        return Err(InputError::BadValue("snapshot page"));
    }
    let select = Select::new(page, sub).ok_or(InputError::BadValue("snapshot sub-code"))?;
    let hello = Item::Event(Event::Hello { version: VERSION, caps: SNAPSHOT_OFFER, agent: None });
    let hello_length = write(&hello, out)?;
    let rest = out.get_mut(hello_length..).ok_or(InputError::Wire(WireError::Full))?;
    let select_length = write(&Item::Event(Event::Select(select, "")), rest)?;
    Ok(hello_length.saturating_add(select_length))
}

/// Whether a WELCOME's `caps` agree SNAPSHOT (REQ-6879 item 4). A snapshot
/// receiver whose WELCOME does not closes the connection at once with a
/// normal close (1000), sending nothing more and answering no
/// `STATE_CHALLENGE`.
#[must_use]
pub const fn welcome_agrees_snapshot(caps: u32) -> bool {
    caps & SNAPSHOT_CAP != 0
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic,
        reason = "a test reports a failure by panicking, so it unwraps results it expects to be good and indexes data it built"
    )]

    use std::vec::Vec;

    use neoteletext_core::limits::SUBCODE_ANY;

    use super::*;
    use crate::MAX_MESSAGE;
    use crate::caps::{AGENT, ALL_CAPS, CURSOR_CAP, NAPLPS_CAP, REGIS_CAP, RESUME_CAP, SELECT_CAP, TOUCH_CAP};
    use crate::caps::{RETIRED_ECHO_BIT, RETIRED_MODE_BITS, RETIRED_PAGE_BIT};
    use crate::control::{Control, read_body, write_body};
    use crate::event::read_message;

    /// Every item of a written message.
    fn read_request(message: &[u8]) -> Vec<Item<'_>> {
        let mut items = Vec::new();
        read_message(message, &mut |item| items.push(item)).unwrap();
        items
    }

    // REQ-6879 item 1: bit 25, a capability, no retired bit.
    #[test]
    fn snapshot_cap_is_bit_25_and_a_capability() {
        assert_eq!(SNAPSHOT_CAP, 33_554_432);
        assert_eq!(SNAPSHOT_CAP, 1 << 25);
        assert_ne!(ALL_CAPS & SNAPSHOT_CAP, 0);
        assert_eq!(ALL_CAPS & (RETIRED_MODE_BITS | RETIRED_ECHO_BIT | RETIRED_PAGE_BIT), 0);
        // REQ-6879 item 2: exactly SNAPSHOT, SELECT and the picture languages.
        assert_eq!(SNAPSHOT_OFFER, SNAPSHOT_CAP | SELECT_CAP | REGIS_CAP | NAPLPS_CAP);
        assert_eq!(SNAPSHOT_OFFER & (CURSOR_CAP | TOUCH_CAP | RESUME_CAP | AGENT), 0);
    }

    // REQ-6879 item 3: HELLO with exactly SNAPSHOT_OFFER and no agent, then
    // SELECT of the page and sub-code, and of the page alone for 3F7F.
    #[test]
    fn the_snapshot_request_is_hello_then_select() {
        let hello = Item::Event(Event::Hello { version: VERSION, caps: SNAPSHOT_OFFER, agent: None });
        for (page, sub) in [(0x100, SUBCODE_ANY), (0x302, 0x0002), (0x8FF, 0)] {
            let mut out = [0_u8; MAX_MESSAGE];
            let length = write_request(page, sub, &mut out).unwrap();
            let items = read_request(&out[..length]);
            assert_eq!(items, [hello, Item::Event(Event::Select(Select { page, sub }, ""))]);
        }
        // The page as a whole is the page's three digits alone (DOC-1753).
        let mut out = [0_u8; MAX_MESSAGE];
        let length = write_request(0x100, SUBCODE_ANY, &mut out).unwrap();
        assert!(out[..length].ends_with(&[0xFF, 0x40, 3, b'1', b'0', b'0']));
    }

    // REQ-6879 item 3: page 0, a page outside 100 to 8FF, a sub-code with a
    // bit a sub-code has not, and a buffer too small are refused.
    #[test]
    fn the_snapshot_request_refuses_what_is_no_snapshot() {
        let mut out = [0_u8; MAX_MESSAGE];
        assert_eq!(write_request(0, SUBCODE_ANY, &mut out), Err(InputError::BadValue("snapshot page")));
        assert_eq!(write_request(0x900, SUBCODE_ANY, &mut out), Err(InputError::BadValue("snapshot page")));
        assert_eq!(write_request(0x100, 0xFFFF, &mut out), Err(InputError::BadValue("snapshot sub-code")));
        let whole = write_request(0x100, 0x0001, &mut out).unwrap();
        for short in 0..whole {
            let mut small = std::vec![0_u8; short];
            assert!(matches!(write_request(0x100, 0x0001, &mut small), Err(InputError::Wire(_))), "{short}");
        }
    }

    // REQ-6879 item 4.
    #[test]
    fn a_welcome_without_snapshot_does_not_agree_it() {
        assert!(!welcome_agrees_snapshot(SELECT_CAP | REGIS_CAP | NAPLPS_CAP));
        assert!(!welcome_agrees_snapshot(0));
        assert!(welcome_agrees_snapshot(SNAPSHOT_OFFER));
        assert!(welcome_agrees_snapshot(SNAPSHOT_CAP));
    }

    // REQ-6879 item 1: a WELCOME agreeing bit 25 is read. Before bit 25 was
    // a capability it was refused as an unknown bit.
    #[test]
    fn a_welcome_agreeing_snapshot_is_read() {
        let welcome = Control::Welcome { version: VERSION, caps: SNAPSHOT_OFFER, resume: None };
        let mut out = [0_u8; crate::control::MAX_BODY];
        let length = write_body(&welcome, &mut out).unwrap();
        assert_eq!(read_body(&out[..length]), Ok(welcome));
    }
}
