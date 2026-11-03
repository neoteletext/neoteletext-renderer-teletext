//! The viewer's keys (REQ-FSM-005, REQ-NAV-012, REQ-SEC-024): each key's
//! facts resolved from its surroundings into a [`ViewerInput`] for the
//! machine of `machine.rs`: the link and whether its prompt is shown and its
//! service confirmed, the Index key's target, the page or sub-page a step
//! leads to, and the history's next entry.

use super::{ColourInput, Key, KeyContext, RevealInput, Viewer, ViewerInput};
use crate::acq::PageStore;
use crate::config::{RevealStyle, StepOrder};
use crate::nav::{PageLinks, step_numerical, step_stored};
use crate::types::{Instant, PageAddress, SubCode};

impl Viewer {
    /// Handles a key (REQ-FSM-005). `links` are the links of the page shown
    /// (from X/27/0 or the TTI Link Extension); `confirmed` says whether
    /// another service's origin has been confirmed this session.
    pub fn key(&mut self, key: Key, context: &KeyContext<'_>) -> super::Action {
        let input = self.key_input(key, context);
        self.run(input)
    }

    /// The machine's input for a key, with the facts it needs.
    fn key_input(&self, key: Key, context: &KeyContext<'_>) -> ViewerInput {
        let KeyContext { links, store, initial, config, now, confirmed, prompts_shown } = *context;
        match key {
            Key::Digit(digit) => ViewerInput::Digit(digit),
            Key::SubPage => ViewerInput::SubPageKey,
            Key::Hold => ViewerInput::Hold,
            Key::Reveal => ViewerInput::Reveal(match config.reveal {
                RevealStyle::Latched => RevealInput::Latched,
                RevealStyle::Momentary => {
                    RevealInput::Momentary(Instant(now.0.saturating_add(config.momentary_reveal_us)))
                }
            }),
            Key::Mix => ViewerInput::Mix,
            Key::Zoom => ViewerInput::Zoom,
            Key::Red | Key::Green | Key::Yellow | Key::Cyan => {
                let index = colour_index(key);
                let target = links.keys.get(usize::from(index)).copied().flatten();
                ViewerInput::Colour(ColourInput {
                    key: index,
                    target,
                    // REQ-SEC-024: a link may be followed only while its
                    // prompt is shown whole in row 24.
                    prompt_shown: prompts_shown.get(usize::from(index)).copied().unwrap_or(false),
                    confirmed: target.and_then(|target| target.service).is_some_and(confirmed),
                })
            }
            Key::Index => ViewerInput::Index(index_target(links, initial)),
            Key::Home => ViewerInput::Home(initial),
            Key::NextPage | Key::PreviousPage => ViewerInput::StepPage(self.step_page(
                key == Key::NextPage,
                config.step_order,
                config.store_hexadecimal_pages,
                store,
            )),
            Key::NextSubPage | Key::PreviousSubPage => {
                ViewerInput::StepSubPage(self.neighbour_subpage(key == Key::NextSubPage, store))
            }
            Key::Back => ViewerInput::Back(self.history.peek_back()),
            Key::Forward => ViewerInput::Forward(self.history.peek_forward()),
        }
    }

    /// Next or previous page, in the configured step order.
    fn step_page(&self, forward: bool, step_order: StepOrder, hexadecimal: bool, store: &PageStore<'_>) -> PageAddress {
        // REQ-ACQ-017: hexadecimal pages are stepped to only under the
        // diagnostic setting of REQ-ACQ-013.
        match step_order {
            StepOrder::Numerical => step_numerical(self.requested, forward, hexadecimal),
            StepOrder::Stored => step_stored(self.requested, forward, hexadecimal, store),
        }
    }

    /// The next or previous stored sub-page, wrapping at either end
    /// (REQ-NAV-005). The store is asked for the neighbour of the sub-page
    /// shown, so every one of up to 8,190 is reached and the viewer holds
    /// no list (decision E.122). A page with no listed sub-pages, such as
    /// one that keeps its latest version only (REQ-NUM-021) or a data page
    /// (REQ-NUM-024), has none.
    fn neighbour_subpage(&self, forward: bool, store: &PageStore<'_>) -> Option<SubCode> {
        let address = self.requested;
        let current =
            if self.subcode.is_any() { store.latest(address).map(|page| page.key.subcode) } else { Some(self.subcode) };
        match (current, forward) {
            (Some(current), true) => store.next_subpage(address, current).or_else(|| store.first_subpage(address)),
            (Some(current), false) => store.previous_subpage(address, current).or_else(|| store.last_subpage(address)),
            (None, true) => store.first_subpage(address),
            (None, false) => store.last_subpage(address),
        }
    }
}

/// Where the Index key leads (REQ-286, Annex H.4): the page's index link;
/// on a page with no links at all the initial page; nowhere on a page with
/// coloured links and no index link. An index link to another service stays
/// in this service, at its initial page (DEC-2198).
fn index_target(links: &PageLinks, initial: PageAddress) -> Option<(PageAddress, SubCode)> {
    match links.index {
        Some(target) if target.service.is_none() => Some((target.address, target.subcode)),
        Some(_) => Some((initial, SubCode::ANY)),
        None if links.no_coloured_links() => Some((initial, SubCode::ANY)),
        None => None,
    }
}

/// Red 0, green 1, yellow 2, cyan 3; the order of [`PageLinks::keys`].
const fn colour_index(key: Key) -> u8 {
    match key {
        Key::Green => 1,
        Key::Yellow => 2,
        Key::Cyan => 3,
        Key::Red
        | Key::Digit(_)
        | Key::Index
        | Key::Home
        | Key::NextPage
        | Key::PreviousPage
        | Key::NextSubPage
        | Key::PreviousSubPage
        | Key::Reveal
        | Key::Hold
        | Key::Mix
        | Key::Zoom
        | Key::SubPage
        | Key::Back
        | Key::Forward => 0,
    }
}
