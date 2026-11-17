//! A complete response: acted on by its purpose, or redirected
//! (REQ-WEB-003, REQ-WEB-021, REQ-SEC-007).

use neoteletext_core::types::Instant;

use super::{Event, Exchange, Failure, Link, Output, Purpose, Sink, UserAgent, event};
use crate::channel::{self, ChannelFile};
use crate::consts::{
    MAX_REDIRECTS, MAX_RETRY_AFTER_US, MICROSECONDS_PER_SECOND, STATUS_FOUND, STATUS_MOVED_PERMANENTLY, STATUS_OK,
    STATUS_PERMANENT_REDIRECT, STATUS_SEE_OTHER, STATUS_TEMPORARY_REDIRECT,
};
use crate::descriptor::Descriptor;
use crate::http::{MediaType, ResponseHead};
use crate::port::{ConnectionId, RandomSource};
use crate::url::{Url, UrlError};

impl<R: RandomSource> UserAgent<'_, R> {
    pub(super) fn complete(&mut self, now: Instant, sink: &mut Sink<'_>) {
        let (Some(exchange), Some(head)) = (self.fetch.exchange, self.fetch.response) else { return };
        self.fetch.deadline = None;
        self.fetch.used = true;
        self.fetch.body = None;
        self.fetch.response = None;
        // A resource's connection is never used again (REQ-2476): the next
        // request, for a channel or another resource, opens its own.
        let resource = matches!(exchange.purpose, Purpose::Resource(_));
        if head.close || core::mem::take(&mut self.close_fetch_after) || resource {
            sink(Output::Close { connection: ConnectionId::Fetch });
            self.fetch.link = Link::Closed;
        }
        if let Some(seconds) = head.retry_after {
            let delay = u64::from(seconds).saturating_mul(MICROSECONDS_PER_SECOND).min(MAX_RETRY_AFTER_US);
            self.next_fetch_at = self.next_fetch_at.max(Instant(now.0.saturating_add(delay)));
        }
        let status = head.status;
        self.report_exchange(now, &exchange, status, sink);
        let redirect = matches!(
            status,
            STATUS_MOVED_PERMANENTLY
                | STATUS_FOUND
                | STATUS_SEE_OTHER
                | STATUS_TEMPORARY_REDIRECT
                | STATUS_PERMANENT_REDIRECT
        );
        if redirect {
            self.redirect(now, exchange, &head, sink);
            return;
        }
        self.fetch.exchange = None;
        if self.complete_channel(now, &exchange, &head, sink) {
            return;
        }
        match exchange.purpose {
            // complete_channel handled every one of these.
            Purpose::FrontDoor | Purpose::Contents => {}
            Purpose::Descriptor { channel } => {
                self.complete_descriptor(now, &exchange, &head, channel, sink);
            }
            Purpose::Page { page, subcode, file } => {
                self.complete_page(now, &exchange, &head, (page, subcode, file), sink);
            }
            Purpose::SubPages { page, part } => self.complete_subpages(now, &head, page, part, sink),
            Purpose::Resource(role) => self.complete_resource(&exchange, &head, role, sink),
        }
    }

    /// A service's descriptor or a channel's entry file has come: read it
    /// strictly and open the service, or report why not.
    fn complete_descriptor(
        &mut self,
        now: Instant,
        exchange: &Exchange,
        head: &ResponseHead,
        channel: bool,
        sink: &mut Sink<'_>,
    ) {
        let status = head.status;
        let body = self.buffers.body.get(..self.fetch.body_len).unwrap_or(&[]);
        // REQ-WEB-027 (DEC-1726): no type table lists a channel's
        // entry file's extension, so a host gives it what it gives
        // anything unknown, which may be no type at all. The strict
        // reader decides what the file is, and only an HTML page, a
        // host's answer for a missing file, is refused by its type.
        // A service's descriptor comes only as JSON.
        let media_accepted = if channel { head.media != MediaType::Html } else { head.media == MediaType::Json };
        if status != STATUS_OK {
            event(sink, Event::DescriptorFailed(Failure::Status(status)));
        } else if !media_accepted {
            event(sink, Event::DescriptorFailed(Failure::MediaType(head.media)));
        } else {
            let parsed = if channel {
                Descriptor::parse_channel(body, &exchange.url)
            } else {
                Descriptor::parse(body, &exchange.url)
            };
            match parsed {
                Ok(descriptor) => {
                    self.open_descriptor(now, descriptor, sink);
                    if channel && self.push.url.is_some() {
                        // REQ-WEB-018 (DEC-1467): a live channel's
                        // connection brings its pages and contents.
                        self.live = true;
                        self.connecting = true;
                        event(sink, Event::ChannelConnecting);
                    } else if channel {
                        self.front_door =
                            self.channel.and_then(|folder| channel::file_url(&folder, ChannelFile::Bundle).ok());
                    }
                }
                Err(error) => event(sink, Event::DescriptorRejected(error)),
            }
        }
    }

    fn redirect(&mut self, now: Instant, mut exchange: Exchange, head: &ResponseHead, sink: &mut Sink<'_>) {
        self.fetch.exchange = None;
        if exchange.redirects >= MAX_REDIRECTS {
            Self::finish_purpose(exchange.purpose, Err(Failure::TooManyRedirects), sink);
            return;
        }
        let next = match redirect_location(&exchange.url, head) {
            Ok(next) => next,
            Err(failure) => {
                Self::finish_purpose(exchange.purpose, Err(failure), sink);
                return;
            }
        };
        // REQ-SEC-007: a redirect from HTTPS to plain HTTP is refused,
        // whichever host it names.
        let downgrade = exchange.url.is_secure() && !next.is_secure();
        let failure = if downgrade { Some(Failure::Downgrade) } else { self.refusal(&next) };
        if let Some(failure) = failure {
            Self::report_refusal(ConnectionId::Fetch, &next, failure, sink);
            Self::finish_purpose(exchange.purpose, Err(failure), sink);
            return;
        }
        // Every request is a GET, which every redirect keeps (RFC 9110
        // section 15.4), so the same address is the same request again.
        if next.as_str() == exchange.url.as_str() {
            Self::finish_purpose(exchange.purpose, Err(Failure::BadRedirect), sink);
            return;
        }
        // Another channel is confirmed before anything from it is shown
        // (REQ-WEB-021).
        // A resource belongs to no channel, and follows a redirect to any
        // origin within the checks above (REQ-2476, REQ-123).
        let resource = matches!(exchange.purpose, Purpose::Resource(_));
        if !resource && self.config.redirects_within_channel && !self.redirect_keeps_channel(&exchange.url, &next) {
            event(sink, Event::RedirectToOtherChannel { location: next.as_str() });
            Self::finish_purpose(exchange.purpose, Err(Failure::RedirectToOtherChannel), sink);
            return;
        }
        exchange.url = next;
        exchange.redirects = exchange.redirects.saturating_add(1);
        exchange.sent = false;
        exchange.retried = false;
        exchange.started = now;
        self.fetch.exchange = Some(exchange);
        self.start_exchange(now, sink);
    }

    /// A redirect from `from` to `next` keeps the channel (REQ-WEB-021,
    /// REQ-507): inside the folder of the open channel, or of the open
    /// service's descriptor; with neither, on the same origin. Each way, an
    /// upgrade from `http` to `https` on the same host between the default
    /// ports keeps it.
    fn redirect_keeps_channel(&self, from: &Url, next: &Url) -> bool {
        if let Some(folder) = self.identity() {
            return channel::redirect_keeps_channel(folder, next);
        }
        let upgrade = !from.is_secure()
            && next.is_secure()
            && next.host_text() == from.host_text()
            && from.port() == crate::url::Scheme::Http.default_port()
            && next.port() == crate::url::Scheme::Https.default_port();
        next.origin().same(&from.origin()) || upgrade
    }
}

/// The address a redirect from `from` leads to: its `Location` resolved
/// against `from`, which must not be a WebSocket address.
fn redirect_location(from: &Url, head: &ResponseHead) -> Result<Url, Failure> {
    // An empty or non-UTF-8 `Location` leads nowhere: it would resolve
    // to the request's own address (decision E.114 item 4, F-44).
    let location = head.location.as_ref().and_then(|location| core::str::from_utf8(location.as_bytes()).ok());
    let Some(location) = location.filter(|location| !location.is_empty()) else {
        return Err(Failure::BadRedirect);
    };
    match from.join(location) {
        Ok(url) if !url.scheme().is_websocket() => Ok(url),
        Ok(_) => Err(Failure::Address(UrlError::Scheme)),
        Err(error) => Err(Failure::Address(error)),
    }
}
