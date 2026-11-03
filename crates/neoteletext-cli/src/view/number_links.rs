//! A typed page number that leads elsewhere (REQ-509, REQ-WEB-020; the TTI
//! Link Extension's page-number links, decision E.64 item 4): the page
//! shown's own `LK` link for the number first, then the service's alias
//! for it from its descriptor.

use super::ViewState;
use neoteletext_app::extras::{Destination, PageExtras};
use neoteletext_app::interact::{message_row, prompt_with};
use neoteletext_app::source::Source;
use neoteletext_core::nav::LinkTarget;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_core::viewer::Action;

/// The instruction of a typed number's question before it leaves the
/// service (DEC-2480): Enter goes, any other key stays.
const ENTER_INSTRUCTION: &str = "Enter: ";

impl ViewState {
    /// The number `address` was typed: follows its link or alias, if it has
    /// one. A page of this service is selected in its place, as one visit;
    /// a linked service is followed at once when its origin was confirmed
    /// this session (REQ-288), and otherwise asked about, Enter going
    /// there; another channel's address is said, since `neottx` opens one
    /// channel at a time.
    pub(super) fn typed_number(&mut self, address: PageAddress, source: &dyn Source, extras: Option<&PageExtras>) {
        let destination = extras.and_then(|extras| extras.number_link(address)).cloned().or_else(|| {
            let (_, target) = self.aliases.iter().find(|(mpp, _)| *mpp == address.mpp())?;
            alias_destination(target, source)
        });
        match destination {
            None => {}
            Some(Destination::Page(target)) if target.service.is_none() => {
                self.action = self.viewer.redirect(target.address, target.subcode);
            }
            Some(Destination::Page(target)) => {
                let linked = target.service.and_then(|service| source.linked_service(service)).unwrap_or_default();
                if !linked.origin.is_empty() && self.confirmed_origins.contains(&linked.origin) {
                    self.action = Action::FollowService(target);
                } else {
                    let name = if linked.display_name.is_empty() { &linked.short_name } else { &linked.display_name };
                    self.prompt = Some(prompt_with(ENTER_INSTRUCTION, name, &linked.origin));
                    self.pending_link = Some(target);
                }
            }
            Some(Destination::Address(other)) => {
                // Row 24 says so once the page's request has gone, which
                // clears the row before it (DEC-2480).
                self.prompt_after_request =
                    Some(message_row(&format!("Page {:03X} is another channel's", address.mpp())));
                self.messages.push(format!("page {:03X} is at {other}: open it with neottx view", address.mpp()));
            }
        }
    }
}

/// Where an alias's target leads: `name:mpp`, a page of the linked
/// service the descriptor names `name`, or an address of another channel.
/// `None` for a name the descriptor does not hold, a null link as in
/// Appendix E.2 rule 2.
fn alias_destination(target: &str, source: &dyn Source) -> Option<Destination> {
    if target.contains("://") {
        return Some(Destination::Address(target.to_owned()));
    }
    let (name, mpp) = target.split_once(':')?;
    let address = PageAddress::from_mpp(u16::from_str_radix(mpp, 16).ok()?)?;
    let service = (0..=u8::MAX)
        .map_while(|service| source.linked_service(service).map(|linked| (service, linked)))
        .find(|(_, linked)| linked.short_name == name)
        .map(|(service, _)| service)?;
    Some(Destination::Page(LinkTarget { address, subcode: SubCode::ANY, service: Some(service) }))
}
