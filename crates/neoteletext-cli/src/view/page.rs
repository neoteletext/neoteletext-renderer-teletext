//! The page a turn shows: found and its state told to the viewer, and its
//! grid built with row 24.

use super::{Shown, ViewState};
use neoteletext_app::extras;
use neoteletext_app::session::Session;
use neoteletext_core::acq::{Event, StoredPage};
use neoteletext_core::fsm::PageState;
use neoteletext_core::trace::TraceSink;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_core::viewer::{ComposeContext, Composed, ViewerEvent, ViewerState};

impl ViewState {
    /// The page shown, once the viewer has been told it is present and
    /// whether it is stale.
    pub(super) fn page_shown<'s>(
        &mut self,
        session: &'s Session,
        requested: PageAddress,
        subcode: SubCode,
    ) -> Option<&'s StoredPage> {
        if self.viewer.state() == ViewerState::Searching && session.page(requested, subcode).is_some() {
            self.viewer.event(ViewerEvent::PagePresent);
        }
        let page = match self.viewer.state() {
            ViewerState::Searching | ViewerState::Starting => None,
            _ => session.page(requested, subcode),
        };
        // REQ-FSM-005: the status area says when the page shown has not
        // been received, or confirmed current, recently.
        match page.map(|page| page.state) {
            Some(PageState::Stale) => {
                let _ = self.viewer.event(ViewerEvent::PageStale);
            }
            Some(_) => {
                let _ = self.viewer.event(ViewerEvent::PageUpdated);
            }
            None => {}
        }
        page
    }

    /// Builds the grid of the page shown, with row 24's prompt or links.
    pub(super) fn compose(&mut self, session: &Session, requested: PageAddress, shown: &Shown<'_>) {
        let rolling = session.rolling_header();
        let magazine = session.decoder().magazine_data(requested.magazine);
        let store = session.decoder().store();
        let row24 = self.prompt.or_else(|| shown.extras.and_then(|extras| extras.row24.as_ref()).map(|(row, _)| *row));
        let lookup = session.decoder().enhancement_lookup();
        let context = ComposeContext {
            magazine: &magazine,
            config: shown.config,
            rolling_header: &rolling,
            store,
            row24,
            lookup,
        };
        // REQ-FSM-010: with `-vv` the walk of the presentation level is
        // traced, once for each reception of the page shown rather than on
        // every redraw.
        let reception = shown.page.map(|page| (page.key, page.last_received));
        let traced = shown.config.trace && reception.is_some() && reception != self.traced;
        let mut entries = Vec::new();
        let composed = if traced {
            self.traced = reception;
            let mut keep = |entry| entries.push(Event::Trace(entry));
            let mut sink = TraceSink { time: shown.now, entries: &mut keep };
            self.viewer.compose_traced(&mut self.grid, shown.page, &context, &mut sink)
        } else {
            self.viewer.compose(&mut self.grid, shown.page, &context)
        };
        self.walk_trace.extend(entries);
        if let Composed::Built(report) = composed {
            self.unrecorded = report;
        }
        if self.prompt.is_none()
            && let Some((_, marks)) = shown.extras.and_then(|extras| extras.row24.as_ref())
        {
            extras::draw_marks(&mut self.grid, marks);
        }
    }
}
