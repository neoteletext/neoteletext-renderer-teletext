//! The input count the channel acknowledges (DOC-1943), and the
//! messages kept until it does, to send again after a reconnection
//! (DOC-1923, REQ-RCH-013): a ring of the messages of input units not
//! yet acknowledged. A message of no input units, a move, gone, HOVER or
//! COMPOSE, is never acknowledged, and so never kept (REQ-INT-126,
//! REQ-RCH-012).

use super::{Sent, SessionOf};
use crate::event::{Event, Item, units};

/// The most unacknowledged messages remembered (REQ-INT-110): when more
/// are sent, the oldest is forgotten, and the session can no longer be
/// resumed whole.
pub const REMEMBERED: usize = 256;
/// RFC 1982 serial arithmetic on 16 bits: a count this far ahead or more
/// is behind.
const SERIAL_HALF: u16 = 0x8000;

/// Whether `later` is after `earlier` by RFC 1982 serial arithmetic on 16
/// bits.
pub(super) const fn serial_after(later: u16, earlier: u16) -> bool {
    let ahead = later.wrapping_sub(earlier);
    ahead != 0 && ahead < SERIAL_HALF
}

impl<const KEPT: usize> SessionOf<KEPT> {
    /// Records that `item` went as `message`: counts its units and, when it
    /// has any, remembers it until it is acknowledged, its bytes kept to
    /// send again when the session may be resumed.
    pub(super) fn record(&mut self, item: &Item<'_>, message: &[u8]) {
        let units = units(item);
        if units == 0 {
            return;
        }
        // The count is modulo 2¹⁶, so only the low 16 bits of the units add.
        self.count = self.count.wrapping_add(u16::try_from(units & u32::from(u16::MAX)).unwrap_or(u16::MAX));
        if self.sent_length == REMEMBERED {
            // An unacknowledged message forgotten cannot be sent again.
            self.drop_oldest();
            self.kept_whole = false;
        }
        let mut length = 0_u16;
        if KEPT > 0 && self.kept_whole {
            let end = self.kept_length.saturating_add(message.len());
            match (u16::try_from(message.len()), self.kept.get_mut(self.kept_length..end)) {
                (Ok(fits), Some(room)) => {
                    room.copy_from_slice(message);
                    self.kept_length = end;
                    length = fits;
                }
                _ => self.kept_whole = false,
            }
        }
        let rule_event = matches!(item, Item::Event(Event::RuleEvent(_)));
        let slot = self.ring(self.sent_length);
        if let Some(entry) = self.sent.get_mut(slot) {
            *entry = Sent { after: self.count, length, rule_event };
            self.sent_length = self.sent_length.saturating_add(1);
        }
    }

    /// Forgets the oldest unacknowledged message and its kept bytes.
    fn drop_oldest(&mut self) {
        let Some(&Sent { after, length, .. }) = self.sent.get(self.sent_first) else { return };
        let length = usize::from(length).min(self.kept_length);
        self.move_kept(length, 0, self.kept_length.saturating_sub(length));
        self.kept_length = self.kept_length.saturating_sub(length);
        self.kept_from = after;
        self.sent_first = self.sent_first.saturating_add(1).checked_rem(REMEMBERED).unwrap_or(0);
        self.sent_length = self.sent_length.saturating_sub(1);
    }

    /// A cursor record's acknowledgement: taken when it is newer than the
    /// last taken and not beyond what was sent (RFC 1982 serial arithmetic:
    /// an older one changes nothing), and the messages it covers are
    /// forgotten. Returns whether it was taken.
    pub fn acknowledged(&mut self, acked: u16) -> bool {
        if !serial_after(acked, self.acked) || serial_after(acked, self.count) {
            return false;
        }
        self.acked = acked;
        while self.sent_length > 0 {
            let Some(&Sent { after, .. }) = self.sent.get(self.sent_first) else { break };
            if serial_after(after, acked) {
                break;
            }
            self.drop_oldest();
        }
        true
    }

    /// Moves `length` kept bytes from offset `from` to offset `to`, no
    /// later than `from`, one by one through checked accesses:
    /// `copy_within` panics on a range it cannot prove in bounds, which
    /// REQ-GEN-019 forbids linking.
    fn move_kept(&mut self, from: usize, to: usize, length: usize) {
        for index in 0..length {
            if let Some(byte) = self.kept.get(from.saturating_add(index)).copied()
                && let Some(slot) = self.kept.get_mut(to.saturating_add(index))
            {
                *slot = byte;
            }
        }
    }

    /// A resumed WELCOME said the channel had taken the input up to
    /// `taken`: when [`SessionOf::discard_rule_events`] marked the
    /// session, every marked `RULE_EVENT` after `taken` is dropped with
    /// its kept bytes, and the messages left after `taken` are counted
    /// again from `taken`, in order, each by its own units, so that the
    /// receiver's count is the one the channel reaches when they are sent
    /// again (RULE-111 case 2, RULE-46). What the channel had taken keeps
    /// its count: nothing before it is dropped.
    pub(super) fn drop_discarded(&mut self, taken: u16) {
        let Some(through) = self.discard_through.take() else { return };
        // Before each message, its count in the old numbering and in the
        // new; the ring and the kept bytes are rewritten in place, the
        // places written never after the places read.
        let mut before = self.kept_from;
        let mut count = self.kept_from;
        let (mut read, mut written) = (0_usize, 0_usize);
        let mut left = 0_usize;
        for index in 0..self.sent_length {
            let Some(&sent) = self.sent.get(self.ring(index)) else { break };
            let units = sent.after.wrapping_sub(before);
            before = sent.after;
            let length = usize::from(sent.length);
            let dropped = sent.rule_event && serial_after(sent.after, taken) && !serial_after(sent.after, through);
            if !dropped {
                self.move_kept(read, written, length);
                written = written.saturating_add(length);
                count = count.wrapping_add(units);
                let slot = self.ring(left);
                if let Some(entry) = self.sent.get_mut(slot) {
                    *entry = Sent { after: count, ..sent };
                }
                left = left.saturating_add(1);
            }
            read = read.saturating_add(length);
        }
        self.sent_length = left;
        self.kept_length = written;
        self.count = count;
    }
}
