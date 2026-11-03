//! What feeds a viewer: a file read whole at start, or a live source that
//! keeps delivering pages and takes the viewer's requests.

use crate::extras::PageExtras;
use crate::pipe::Notice;
use crate::session::Session;
use neoteletext_core::types::{Instant, PageAddress, SubCode};
use std::sync::Arc;

/// Wakes a viewer that is waiting: a source calls it when it has something
/// for [`Source::poll`] (REQ-INT-008).
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// The next sub-page to step to, from a page's list of sub-pages kept by
/// the source, outside the store (REQ-328, REQ-1888, decision E.122).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubPageStep {
    /// Show this sub-page, asking for it if the store does not hold it.
    To(SubCode),
    /// The part of the list that says has been asked for: step again when
    /// the source has more.
    Waiting,
    /// The source has no list for the page: step through what the store
    /// holds.
    NoList,
}

/// Something that feeds packets into the session while the viewer runs.
pub trait Source {
    /// Asks the source to call `wake` whenever something arrives for
    /// [`Source::poll`], so the viewer takes it at once rather than at its
    /// next look. A source read whole at start never calls it.
    fn set_wake(&mut self, _wake: Wake) {}

    /// Feeds whatever is available now. Returns false at the end of input.
    fn poll(&mut self, session: &mut Session, now: Instant) -> bool;

    /// Asks for a page, for sources that fetch on demand.
    fn request(&mut self, _page: PageAddress, _subcode: SubCode) {}

    /// Asks for a page the shown page depends on, without making it the
    /// page kept fresh.
    fn prefetch(&mut self, _page: PageAddress) {}

    /// Notices for the viewer since the last call.
    fn notices(&mut self) -> Vec<Notice> {
        Vec::new()
    }

    /// Whether `page` last came over plain HTTP (`Some(true)`) or encrypted
    /// (`Some(false)`), for the unencrypted marker of the page on screen
    /// (REQ-SEC-008); `None` when the source does not know, such as a file.
    fn came_unencrypted(&self, _page: PageAddress) -> Option<bool> {
        None
    }

    /// Extension data of a page: links, the drawn link row, input modes.
    fn extras(&self, _page: PageAddress, _subcode: SubCode) -> Option<&PageExtras> {
        None
    }

    /// Sends an input message to the channel being typed into (area INT,
    /// REQ-INT-101).
    fn send_input(&mut self, _message: &[u8]) {}

    /// The sub-page after (`forward`) or before `current` in `page`'s list
    /// of sub-pages, wrapping at either end; `SubCode::ANY` stands before
    /// the first. A source without such a list steps nothing.
    fn subpage_step(&mut self, _page: PageAddress, _current: SubCode, _forward: bool) -> SubPageStep {
        SubPageStep::NoList
    }

    /// A live channel's index, page by page, when it sent one (DOC-1955).
    fn index(&self) -> Vec<crate::pipe::IndexEntry> {
        Vec::new()
    }

    /// Follows a link to another service.
    fn follow(&mut self, _service: u8, _page: PageAddress, _subcode: SubCode) {}

    /// A linked service: its short name, the name shown and its origin
    /// (REQ-288).
    fn linked_service(&self, _service: u8) -> Option<crate::pipe::ServiceEntry> {
        None
    }
}

/// A source with nothing more to give: the input was read at start.
pub struct Finished;

impl Source for Finished {
    fn poll(&mut self, _session: &mut Session, _now: Instant) -> bool {
        false
    }
}
