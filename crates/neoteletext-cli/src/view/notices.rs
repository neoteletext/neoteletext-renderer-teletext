//! The source's notices to the viewer: the service's pages, the channel's
//! input, and messages.

use super::ViewState;
use crate::clock::now_ms;
use neoteletext_app::interact::message_row;
use neoteletext_app::pipe::Notice;
use neoteletext_app::session::Session;
use neoteletext_app::source::Source;
use neoteletext_core::types::PageAddress;
use neoteletext_core::viewer::{SecurityIndicator, Viewer, ViewerEvent};

impl ViewState {
    /// A new initial page (REQ-SVC-004): the Index and Home keys' target
    /// from now on, and the page shown when the viewer is still where it
    /// started, has no history, and the address opened named no page.
    fn adopt_initial(&mut self, address: PageAddress) {
        let untouched = self.viewer.requested().0 == self.initial
            && !self.viewer.history().can_go_back()
            && !core::mem::take(&mut self.named);
        self.initial = address;
        if untouched && address != self.viewer.requested().0 {
            self.viewer = Viewer::new(address).with_trace(self.viewer.traces());
            self.action = self.viewer.event(ViewerEvent::Started);
        }
    }

    /// Takes the initial page of packet 8/30 when a live source's service
    /// data gains or changes one (REQ-SVC-004, EN 300 706 clause 9.8.1),
    /// as a channel's own initial page is taken; a channel's descriptor,
    /// once it has named one, stands over it (DEC-2410).
    pub(super) fn follow_service_initial(&mut self, session: &Session) {
        if self.initial_from_channel {
            return;
        }
        let Some((address, _)) = session.service().and_then(|info| info.initial_page) else { return };
        if address != self.initial {
            self.adopt_initial(address);
        }
    }

    /// Sets the security indicator from how the page on screen came, when the
    /// source knows (REQ-SEC-008): the unencrypted marker shows whenever,
    /// and only while, that page came over plain HTTP, so a secure page
    /// that replaces a plain one clears it.
    pub(super) fn mark_security(&mut self, came_unencrypted: Option<bool>) {
        if let Some(plain) = came_unencrypted {
            self.viewer.set_indicator(if plain { SecurityIndicator::Unencrypted } else { SecurityIndicator::None });
        }
    }

    /// Acts on one of the source's notices.
    pub(super) fn take_notice(&mut self, notice: Notice, source: &mut dyn Source) {
        match notice {
            Notice::Unencrypted => self.viewer.set_indicator(SecurityIndicator::Unencrypted),
            Notice::InitialPage(mpp) => {
                if let Some(address) = PageAddress::from_mpp(mpp) {
                    self.initial_from_channel = true;
                    self.adopt_initial(address);
                }
            }
            Notice::Service(_) => self.confirmed_services.clear(),
            Notice::Input(open) => {
                let clock_ms = now_ms(self.start);
                self.typing.connection(source, open, clock_ms);
            }
            Notice::Message(text) => self.messages.push(text),
            // REQ-WEB-009: a page that is not stored cannot be shown
            // while the network is away.
            Notice::Offline(mpp) if PageAddress::from_mpp(mpp) == Some(self.viewer.requested().0) => {
                self.prompt = Some(message_row("Page not available offline"));
            }
            Notice::Offline(_)
            | Notice::Current(_)
            | Notice::ServiceName(_)
            | Notice::Bundle(_)
            | Notice::Aliases(_)
            | Notice::Region(_)
            | Notice::Refresh(_) => {}
            Notice::RedirectToOtherChannel(address) => self.messages.push(format!("redirected to {address}")),
            Notice::PageRedirect(mpp, sub, query) => self.typing.redirected(mpp, sub, query),
            Notice::Denied(status, place) => {
                let place = if place.is_empty() { "the channel" } else { place.as_str() };
                self.messages.push(format!("{place} was refused: status {status}"));
            }
            Notice::PushStopped(why) => self.messages.push(why),
        }
    }
}
