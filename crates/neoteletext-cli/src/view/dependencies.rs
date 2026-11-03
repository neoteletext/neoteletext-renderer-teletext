//! A page's enhancement pages asked for from its service (REQ-511,
//! REQ-WEB-022; decision E.63 item 5): the Organization Table of its
//! magazine, then the object pages the table and the page name, each once.

use super::ViewState;
use neoteletext_app::session::Session;
use neoteletext_app::source::Source;
use neoteletext_core::acq::StoredPage;
use neoteletext_core::config::Config;
use neoteletext_core::limits::PAGE_MOT;
use neoteletext_core::types::{PageAddress, PageNumber};

impl ViewState {
    /// Asks the source for the enhancement pages `page` uses at the
    /// configured level that the store does not hold: its magazine's
    /// Organization Table (page xFE) and its Global and Public Object Pages
    /// (clause 10.6, REQ-OBJ-030 to REQ-OBJ-032). Each is asked for once in
    /// a service, as a page the shown one depends on, which leaves the page
    /// kept current as it was; a page already stored is not asked for. The
    /// page is drawn with what has arrived, and again as the rest arrives.
    pub(super) fn ask_enhancements(
        &mut self,
        session: &Session,
        source: &mut dyn Source,
        page: Option<&StoredPage>,
        config: &Config,
    ) {
        let Some(page) = page.filter(|_| config.level.is_enhanced()) else { return };
        let decoder = session.decoder();
        let lookup = decoder.enhancement_lookup();
        let magazine = page.key.address.magazine;
        let table = PageAddress { magazine, page: PageNumber(PAGE_MOT) };
        let links = neoteletext_core::objects::links(page, &lookup, config.level);
        let object_pages = [links.gpop, links.pop]
            .into_iter()
            .flatten()
            .map(|link| PageAddress { magazine: link.magazine, page: link.page });
        let wanted =
            lookup.magazine_organization_table(magazine).is_none().then_some(table).into_iter().chain(object_pages);
        for address in wanted {
            if !decoder.enhancement_store().contains_address(address) && self.enhancements_asked.insert(address.mpp()) {
                source.prefetch(address);
            }
        }
    }
}
