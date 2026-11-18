//! Requests on the fetch connection (REQ-WEB-008, REQ-WEB-014): what is
//! refused before connecting (REQ-SEC-006, REQ-SEC-007), the scheduler
//! with the rate limit and the queue, and the request written.

use neoteletext_core::types::{Instant, PageAddress, SubCode};

use super::page::cached_as;
use super::{Cached, Event, Exchange, Failure, Link, Output, PageFile, Purpose, Sink, Target, UserAgent, Want, event};
use crate::bytes::{Overflow, Writer};
use crate::channel::{self, ChannelFile};
use crate::consts::{ACCEPT_PAGES, CRLF, IDENTITY, header};
use crate::http::{self, Method};
use crate::port::{ConnectionId, RandomSource, Security};
use crate::subpages::PartNumber;
use crate::url::{IpAddress, Url};

impl<R: RandomSource> UserAgent<'_, R> {
    /// The first refusal that applies to a request to `url`, before any
    /// connection is made.
    pub(super) fn refusal(&self, url: &Url) -> Option<Failure> {
        if url.is_secure() && !self.config.tls_available {
            return Some(Failure::TlsUnavailable);
        }
        if self.is_downgrade(url) {
            return Some(Failure::Downgrade);
        }
        if url.ip().is_some_and(|ip| self.blocked(ip)) {
            return Some(Failure::Blocked);
        }
        // Through a proxy, names are resolved there: a local name is
        // refused before the proxy is asked (REQ-WEB-032, REQ-SEC-006).
        let allowed = url.ip().is_some_and(|ip| self.config.allowed_local.contains(&Some(ip)));
        if self.config.proxied && !allowed && crate::proxy::hidden_local(url.host_text()) {
            return Some(Failure::Blocked);
        }
        None
    }

    fn blocked(&self, address: IpAddress) -> bool {
        address.is_blocked() && !self.config.allowed_local.contains(&Some(address))
    }

    /// Whether the peer a connection reached is refused: never through a
    /// proxy, whose address it is (REQ-WEB-032).
    pub(super) fn peer_blocked(&self, peer: IpAddress) -> bool {
        !self.config.proxied && self.blocked(peer)
    }

    pub(super) fn report_refusal(connection: ConnectionId, url: &Url, failure: Failure, sink: &mut Sink<'_>) {
        match failure {
            Failure::Blocked => event(sink, Event::Blocked { connection, host: url.host_text() }),
            Failure::Downgrade => event(sink, Event::DowngradeRefused { host: url.host_text() }),
            _ => {}
        }
    }

    /// Starts the next request when the fetch connection is free.
    pub(super) fn dispatch(&mut self, now: Instant, sink: &mut Sink<'_>) {
        while self.fetch.exchange.is_none() {
            let (purpose, url, method) = if let Some((url, channel)) = self.pending_descriptor.take() {
                (Purpose::Descriptor { channel }, url, Method::Get)
            } else if let Some((url, role)) = self.pending_resource.take() {
                (Purpose::Resource(role), url, Method::Get)
            } else if let Some(url) = self.front_door.take() {
                (Purpose::FrontDoor, url, Method::Get)
            } else if let Some((page, subcode, file)) = self.second_try.take() {
                let Some(request) = self.second_try_request(page, subcode, file, sink) else { continue };
                request
            } else if let Some(url) = self.contents.take() {
                (Purpose::Contents, url, Method::Get)
            } else if !self.live && self.queue.iter().any(Option::is_some) && now >= self.next_fetch_at {
                let Some(queued) = self.take_queued() else { return };
                let request = match queued.want {
                    Want::Page => self.page_request(now, queued.page, queued.subcode, sink),
                    Want::SubPages(part) => self.subpages_request(now, queued.page, part, sink),
                };
                let Some(request) = request else { continue };
                request
            } else {
                return;
            };
            self.next_fetch_at = Instant(now.0.saturating_add(self.config.fetch_interval_us));
            if let Some(failure) = self.refusal(&url) {
                Self::report_refusal(ConnectionId::Fetch, &url, failure, sink);
                Self::finish_purpose(purpose, Err(failure), sink);
                continue;
            }
            self.fetch.exchange = Some(Exchange {
                purpose,
                url,
                method,
                redirects: 0,
                deadline: Instant(now.0.saturating_add(self.config.response_timeout_us)),
                sent: false,
                reused: false,
                retried: false,
                started: now,
            });
            self.start_exchange(now, sink);
        }
    }

    /// The other file of a page or sub-page whose first answered 404 or 410
    /// (REQ-WEB-019, REQ-1887), without waiting for the rate limit: it is the
    /// same request, completed. `None` when there is no channel or the address
    /// fails, which is reported.
    fn second_try_request(
        &self,
        page: PageAddress,
        subcode: Option<SubCode>,
        file: PageFile,
        sink: &mut Sink<'_>,
    ) -> Option<(Purpose, Url, Method)> {
        let folder = self.channel?;
        let name = match (file, subcode) {
            (PageFile::SubPage { t42, .. }, Some(subcode)) => ChannelFile::SubPage { page, subcode, t42 },
            (PageFile::Channel { t42, .. }, _) => ChannelFile::Page { page, t42 },
            _ => return None,
        };
        match channel::file_url(&folder, name) {
            Ok(url) => Some((Purpose::Page { page, subcode, file }, url, Method::Get)),
            Err(error) => {
                event(sink, Event::FetchFailed { page, subcode, failure: Failure::Address(error) });
                None
            }
        }
    }

    /// The request for a queued page. `None` when there is no service, the
    /// cached copy is still fresh, or the address fails, each reported.
    fn page_request(
        &mut self,
        now: Instant,
        page: PageAddress,
        subcode: Option<SubCode>,
        sink: &mut Sink<'_>,
    ) -> Option<(Purpose, Url, Method)> {
        let Some(descriptor) = &self.descriptor else {
            event(sink, Event::FetchFailed { page, subcode, failure: Failure::NoService });
            return None;
        };
        // A sub-page of a page whose list answered is a file of its
        // own (REQ-WEB-019); any other request is for the page's file, and
        // so is 3F7F, which in a link names no particular sub-page
        // (EN 300 706 clause 9.6.1).
        let sub_page = match (self.channel, subcode) {
            (Some(_), Some(subcode)) if !subcode.is_any() && self.is_catalogued(page) => Some(subcode),
            _ => None,
        };
        let cached = if sub_page.is_some() { Cached::SubPage } else { Cached::Page };
        let entry = self.cache_entry(page, subcode, cached);
        if entry.is_some_and(|entry| entry.fresh_until.is_some_and(|until| now < until)) {
            event(sink, Event::PageCurrent { page, subcode });
            return None;
        }
        let (address, file) = match (self.channel, sub_page) {
            (Some(folder), Some(subcode)) => {
                let t42 = entry.map_or(self.sub_page_t42_first, |entry| entry.t42);
                (
                    channel::file_url(&folder, ChannelFile::SubPage { page, subcode, t42 }),
                    PageFile::SubPage { t42, second: false },
                )
            }
            (Some(folder), None) => {
                // The file that answered last time is asked again,
                // so its validators apply; otherwise the extension
                // that last answered in the channel.
                let t42 = entry.map_or(self.t42_first, |entry| entry.t42);
                (channel::file_url(&folder, ChannelFile::Page { page, t42 }), PageFile::Channel { t42, second: false })
            }
            (None, _) => (descriptor.page_url(page, subcode), PageFile::Template),
        };
        let url = match address {
            Ok(url) => url,
            Err(error) => {
                event(sink, Event::FetchFailed { page, subcode, failure: Failure::Address(error) });
                return None;
            }
        };
        Some((Purpose::Page { page, subcode, file }, url, Method::Get))
    }

    /// The request for a queued part of a page's list (REQ-1888). `None` when
    /// no channel is open, the cached copy is still fresh, or the address
    /// fails, each reported.
    fn subpages_request(
        &mut self,
        now: Instant,
        page: PageAddress,
        part: PartNumber,
        sink: &mut Sink<'_>,
    ) -> Option<(Purpose, Url, Method)> {
        let Some(folder) = self.channel else {
            event(sink, Event::SubPagesFailed { page, part, failure: Failure::NoService });
            return None;
        };
        let entry = self.cache_entry(page, None, Cached::SubPages(part));
        if entry.is_some_and(|entry| entry.fresh_until.is_some_and(|until| now < until)) {
            event(sink, Event::SubPagesCurrent { page, part });
            return None;
        }
        match channel::file_url(&folder, ChannelFile::SubPages { page, part }) {
            Ok(url) => Some((Purpose::SubPages { page, part }, url, Method::Get)),
            Err(error) => {
                event(sink, Event::SubPagesFailed { page, part, failure: Failure::Address(error) });
                None
            }
        }
    }

    /// Sends the current exchange's request, connecting first if needed.
    pub(super) fn start_exchange(&mut self, now: Instant, sink: &mut Sink<'_>) {
        let Some(exchange) = self.fetch.exchange else { return };
        // A resource fetched outside any channel opens a connection of its
        // own, which ends with it (REQ-2476): nothing it reads shares a
        // connection with a channel's requests.
        let reusable = self.fetch.link == Link::Open
            && !matches!(exchange.purpose, Purpose::Resource(_))
            && self.fetch.target.is_some_and(|target| target.matches(&exchange.url));
        if reusable {
            self.send_request(sink);
            return;
        }
        if self.fetch.link != Link::Closed {
            sink(Output::Close { connection: ConnectionId::Fetch });
        }
        let target = Target::of(&exchange.url);
        self.fetch.target = Some(target);
        self.fetch.link = Link::Connecting;
        self.fetch.used = false;
        self.fetch.deadline = Some(Instant(now.0.saturating_add(self.config.connect_timeout_us)));
        let security = if target.secure { Security::Tls } else { Security::Plain };
        sink(Output::Connect {
            connection: ConnectionId::Fetch,
            host: exchange.url.host_text(),
            port: target.port,
            security,
        });
    }

    fn write_request(&mut self, exchange: &Exchange) -> Result<usize, Overflow> {
        let mut writer = Writer::new(self.buffers.fetch_request);
        http::write_request_line(&mut writer, exchange.method, exchange.url.target())?;
        http::write_host(&mut writer, &exchange.url)?;
        http::write_header(&mut writer, header::USER_AGENT, self.named_as.as_bytes())?;
        let accept = match exchange.purpose {
            Purpose::Resource(role) => role.accept(),
            _ => ACCEPT_PAGES,
        };
        http::write_header(&mut writer, header::ACCEPT, accept)?;
        http::write_header(&mut writer, header::ACCEPT_ENCODING, IDENTITY)?;
        // A channel's page or sub-page is a TTI or a T42 file, and validators
        // are a file's own (RFC 9110 section 8.8): the other file's never go
        // with a request, as they would when the file that answered before
        // has gone and the other is tried (REQ-WEB-019).
        let validated = match exchange.purpose {
            Purpose::Page { page, subcode, file } => {
                let t42 = match file {
                    PageFile::Channel { t42, .. } | PageFile::SubPage { t42, .. } => Some(t42),
                    PageFile::Template => None,
                };
                Some((page, subcode, cached_as(file), t42))
            }
            Purpose::SubPages { page, part } => Some((page, None, Cached::SubPages(part), None)),
            _ => None,
        };
        if let Some((page, subcode, file, t42)) = validated
            && exchange.redirects == 0
            && let Some(entry) = self.cache.iter().flatten().find(|e| {
                e.page == page && e.subcode == subcode && e.file == file && t42.is_none_or(|t42| e.t42 == t42)
            })
        {
            if let Some(etag) = &entry.etag {
                http::write_header(&mut writer, header::IF_NONE_MATCH, etag.as_bytes())?;
            }
            if let Some(date) = &entry.last_modified {
                http::write_header(&mut writer, header::IF_MODIFIED_SINCE, date.as_bytes())?;
            }
        }
        // No `Cookie` header on any request: NeoTeletext keeps no cookies,
        // and viewer state goes only on the push connection (VS-116,
        // VS-201).
        writer.put(CRLF)?;
        Ok(writer.len())
    }

    pub(super) fn send_request(&mut self, sink: &mut Sink<'_>) {
        let Some(exchange) = self.fetch.exchange else { return };
        match self.write_request(&exchange) {
            Ok(len) => {
                if let Some(current) = &mut self.fetch.exchange {
                    current.sent = true;
                    current.reused = self.fetch.used;
                }
                self.fetch.head.reset();
                self.fetch.response = None;
                self.fetch.body = None;
                self.fetch.body_len = 0;
                self.fetch.started = false;
                self.fetch.deadline = Some(exchange.deadline);
                let bytes = self.buffers.fetch_request.get(..len).unwrap_or(&[]);
                sink(Output::Send { connection: ConnectionId::Fetch, bytes });
            }
            Err(Overflow) => self.fail_exchange(Failure::RequestTooLarge, sink),
        }
    }
}
