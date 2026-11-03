//! Resources fetched outside any channel ([`Input::FetchResource`](super::Input::FetchResource)):
//! the page discovery reads for its pointers (REQ-2476) and a font the user
//! named by its address (REQ-123). Each is one `GET` that carries no cookie
//! and keeps none, touches nothing of a channel or a bound session, and
//! closes its connection when it ends.

use neoteletext_core::consts::FONT_MAX_FILE_BYTES;

use super::{Event, Exchange, Failure, ResourceRole, Sink, UserAgent, event};
use crate::consts::{ACCEPT_DISCOVERY, ACCEPT_FONT, STATUS_OK};
use crate::discovery::{self, DISCOVERY_BODY_BYTES};
use crate::http::{MediaType, ResponseHead};
use crate::port::RandomSource;
use crate::url::{Url, UrlError};

/// A limit no body framing reaches: discovery's decoder never refuses a
/// body for its size, since it stops reading at [`DISCOVERY_BODY_BYTES`]
/// and closes the connection (REQ-2476).
const UNBOUNDED: usize = usize::MAX;

impl ResourceRole {
    /// The `Accept` header's value for the role.
    pub(super) const fn accept(self) -> &'static [u8] {
        match self {
            Self::Discovery => ACCEPT_DISCOVERY,
            Self::Font => ACCEPT_FONT,
        }
    }

    /// The limit the body decoder holds the whole body to.
    pub(super) const fn body_limit(self) -> usize {
        match self {
            Self::Discovery => UNBOUNDED,
            Self::Font => FONT_MAX_FILE_BYTES,
        }
    }

    /// The bytes of the body kept, after which reading ends: discovery's
    /// prefix; a font is read whole.
    pub(super) const fn body_kept(self) -> Option<usize> {
        match self {
            Self::Discovery => Some(DISCOVERY_BODY_BYTES),
            Self::Font => None,
        }
    }
}

impl<R: RandomSource, const SLOTS: usize, const COOKIE_BYTES: usize> UserAgent<'_, R, SLOTS, COOKIE_BYTES> {
    /// Queues a resource the port asked for; it replaces one still waiting.
    pub(super) fn request_resource(&mut self, role: ResourceRole, address: &str, sink: &mut Sink<'_>) {
        match Url::parse(address) {
            Ok(url) if !url.scheme().is_websocket() => self.pending_resource = Some((url, role)),
            Ok(_) => event(sink, Event::ResourceFailed { role, failure: Failure::Address(UrlError::Scheme) }),
            Err(error) => event(sink, Event::ResourceFailed { role, failure: Failure::Address(error) }),
        }
    }

    /// A resource's answer is complete and was not a redirect.
    pub(super) fn complete_resource(
        &self,
        exchange: &Exchange,
        head: &ResponseHead,
        role: ResourceRole,
        sink: &mut Sink<'_>,
    ) {
        if head.status != STATUS_OK {
            event(sink, Event::ResourceFailed { role, failure: Failure::Status(head.status) });
            return;
        }
        let body = self.buffers.body.get(..self.fetch.body_len).unwrap_or(&[]);
        match role {
            ResourceRole::Discovery if head.media == MediaType::Html => {
                let head_bytes = self.buffers.fetch_head.get(..self.fetch.head.len()).unwrap_or(&[]);
                let answered = &exchange.url;
                let mut refused = 0_usize;
                let mut report = |pointer: discovery::Pointer<'_>| match discovery::resolve(&pointer, answered) {
                    Ok(target) => event(
                        sink,
                        Event::Pointer { target: target.as_str(), media: pointer.media, found: pointer.found },
                    ),
                    Err(_) => refused = refused.saturating_add(1),
                };
                discovery::read_headers(head_bytes, &mut report);
                discovery::read_elements(body, &mut report);
                event(sink, Event::DiscoveryDone { answered: answered.as_str(), refused });
            }
            ResourceRole::Font if matches!(head.media, MediaType::FontBdf | MediaType::OctetStream) => {
                event(sink, Event::Font { address: exchange.url.as_str(), bytes: body });
            }
            ResourceRole::Discovery | ResourceRole::Font => {
                event(sink, Event::ResourceFailed { role, failure: Failure::MediaType(head.media) });
            }
        }
    }
}
