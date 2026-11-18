//! The web agent's loop (area WEB): a service or a channel's folder over
//! the std transport, its pages and notices sent to the decoder.

use super::decoder_link::{page_request, send};
use super::services::{Opened, SessionServices};
use super::transport::{StdTransport, SystemRandom};
use super::wakes::{Wake, Wakes};
use super::{clock, neottx};
use crate::pipe::{self, Frame, Notice};
use neoteletext_core::types::Instant;
use neoteletext_formats::page::{AddressSpan, PageFile};
use neoteletext_input::event::{Event as InputEvent, Item, write};
use neoteletext_input::state::NoneReason;
use neoteletext_transport::{SOCKET_READ, web_address};
use neoteletext_web::agent::{
    Config, Event, Failure, Input, InputMessage, Output, PageBytes, PushStop, Source as Came, Storage, UserAgent,
};
use neoteletext_web::channel::{broken_channel_rule, is_channel_address};
use neoteletext_web::consts::{INVALID_PACKET_REPORT_SECONDS, PUSH_SUBPROTOCOL};
use neoteletext_web::descriptor::Descriptor;
use neoteletext_web::http::MediaType;
use neoteletext_web::port::{ConnectionId, Pump};
use neoteletext_web::proxy::Proxy;
use neoteletext_web::records::links::LinksRecord;
use neoteletext_web::records::redirect::RedirectRecord;
use neoteletext_web::subpages::{self, PartNumber};
use std::cell::{Cell, RefCell};
use std::net::IpAddr;
use std::sync::mpsc::Sender;
use std::time::Instant as StdInstant;

/// Microseconds in a second.
const MICROSECONDS_PER_SECOND: u64 = 1_000_000;
/// The window of a report of invalid packets, in the clock's microseconds.
const INVALID_WINDOW_US: u64 = INVALID_PACKET_REPORT_SECONDS * MICROSECONDS_PER_SECOND;

// The pipe carries a part of a list with the web module's bounds.
const _: () = assert!(pipe::MAX_LIST_ROWS == subpages::ROWS_PER_PART);
const _: () = assert!(pipe::MAX_LIST_VERSION == subpages::MAX_VALIDATOR);

/// The agent the loop runs.
type Agent<'a> = UserAgent<'a, SystemRandom>;

pub(super) fn run_web(
    address: &str,
    wakes: &mut Wakes,
    wake: Sender<Wake>,
    allow_local: &[IpAddr],
    proxy: Option<Proxy>,
) {
    // REQ-WEB-023: an address that is a channel's in form and breaks a rule
    // is refused, naming the rule, and never fetched as a descriptor.
    if let Some(rule) = broken_channel_rule(address) {
        let _ = send(&Frame::Notice(Notice::Message(format!("not a channel's address: {rule:?}"))));
        return;
    }
    let mut storage = Box::new(Storage::default());
    let channel = is_channel_address(address);
    let config = agent_config(channel, allow_local, proxy.is_some());
    let mut agent: Box<Agent<'_>> = Box::new(UserAgent::new(config, storage.buffers(), SystemRandom));
    let mut pump = Pump::new(StdTransport::new(allow_local, wake, proxy));
    let start = StdInstant::now();
    let mut buffer = vec![0u8; SOCKET_READ];
    let broken = Cell::new(false);
    let service_ready = Cell::new(false);
    // The push connection opened (true) or was lost (false), for the
    // decoder to know when keys may go (area INT).
    let push_state: Cell<Option<bool>> = Cell::new(None);
    let invalid = Cell::new(InvalidReport::default());
    // A live sub-page's links while its LINKS sending is in progress.
    let links = RefCell::new(None);
    // The channel's server challenged the viewer's key: neottx keeps no
    // state, and answers so (VS-501).
    let challenged = Cell::new(false);
    let mut next_tick: Option<Instant> = Some(Instant(0));
    let sink = |output: Output<'_>, pump: &mut Pump<StdTransport>, next_tick: &mut Option<Instant>| {
        if let Some(net) = output.net() {
            pump.execute(net);
            return;
        }
        let noted = Noted {
            service_ready: &service_ready,
            push_state: &push_state,
            invalid: &invalid,
            links: &links,
            challenged: &challenged,
        };
        if forward(output, next_tick, &noted, clock(start)).is_err() {
            broken.set(true);
        }
    };
    let mut services = SessionServices::new(Opened { address: address.to_owned(), channel });
    let open = if channel { Input::OpenChannel(address) } else { Input::OpenService(address) };
    agent.handle(clock(start), open, &mut |output| sink(output, &mut pump, &mut next_tick));
    while !broken.get() && !wakes.ended() {
        wakes.turn();
        let mut busy = false;
        for connection in [ConnectionId::Fetch, ConnectionId::Push] {
            if let Some(input) = pump.poll(connection, &mut buffer) {
                busy = true;
                agent.handle(clock(start), Input::Net(input), &mut |output| sink(output, &mut pump, &mut next_tick));
            }
            // Answered at once, before anything the decoder sends, so that
            // the answer goes before SELECT (VS-202).
            if challenged.replace(false) {
                let answer = stateless_answer();
                let message = Input::Send(InputMessage::alone(&answer));
                agent.handle(clock(start), message, &mut |output| sink(output, &mut pump, &mut next_tick));
            }
        }
        if let Some(open) = push_state.take()
            && agent.descriptor().is_some_and(Descriptor::input)
            && send(&Frame::Notice(Notice::Input(open))).is_err()
        {
            break;
        }
        if service_ready.replace(false) && announce_service(agent.descriptor()).is_err() {
            break;
        }
        while let Some(frame) = wakes.next_request() {
            busy = true;
            let now = clock(start);
            let more = wakes.next_is_input();
            hand_frame(&mut agent, &mut services, now, frame, more, &mut |output| {
                sink(output, &mut pump, &mut next_tick);
            });
        }
        if next_tick.is_some_and(|at| clock(start) >= at) {
            // What a tick asks for, a reconnection above all, is taken up
            // on the next turn, not after another wait.
            busy = true;
            next_tick = None;
            agent.handle(clock(start), Input::Tick, &mut |output| sink(output, &mut pump, &mut next_tick));
        }
        let mut report = invalid.get();
        let line = report.due(clock(start));
        invalid.set(report);
        if line.is_some_and(|line| send(&Frame::Notice(Notice::Message(line))).is_err()) {
            break;
        }
        if !busy {
            let window_end = invalid.get().window_end;
            wakes.wait(clock(start), [next_tick, window_end].into_iter().flatten().min());
        }
    }
}

/// The push connection's invalid packets, for the warning line of REQ-1462
/// item 4: a rise opens a window of [`INVALID_PACKET_REPORT_SECONDS`], and at
/// its end one line gives what it gathered and the total. So at most one line
/// comes in any window, and none in a quiet one.
#[derive(Clone, Copy, Debug, Default)]
struct InvalidReport {
    total: u64,
    /// The total the last line gave.
    reported: u64,
    /// The end of the window the count rose in, while one is open.
    window_end: Option<Instant>,
}

impl InvalidReport {
    /// The count rose to `total`, at `now`. A new connection counts from 0.
    fn rose(&mut self, total: u64, now: Instant) {
        if total < self.total {
            self.reported = 0;
        }
        self.total = total;
        self.window_end.get_or_insert(Instant(now.0.saturating_add(INVALID_WINDOW_US)));
    }

    /// The warning line, when the window has ended at `now`.
    fn due(&mut self, now: Instant) -> Option<String> {
        self.window_end.filter(|end| now >= *end)?;
        self.window_end = None;
        let gathered = self.total.saturating_sub(self.reported);
        self.reported = self.total;
        (gathered > 0).then(|| {
            format!(
                "{gathered} invalid packets discarded in the last {INVALID_PACKET_REPORT_SECONDS} seconds, {} since the connection opened",
                self.total
            )
        })
    }
}

/// The agent's configuration: this program's name, the proxy, the local
/// addresses it may reach and, for a channel, redirects kept inside it.
fn agent_config(channel: bool, allow_local: &[IpAddr], proxied: bool) -> Config {
    let mut config = Config {
        // An address naming a channel's folder keeps redirects inside it
        // (REQ-WEB-021).
        redirects_within_channel: channel,
        product: neottx(),
        proxied,
        ..Config::default()
    };
    for (slot, &address) in config.allowed_local.iter_mut().zip(allow_local) {
        *slot = Some(web_address(address));
    }
    config
}

/// Why a push connection ended for good, in words.
fn stopped(stop: PushStop) -> String {
    match stop {
        PushStop::Closed(code) => format!("the channel closed the connection with stop code {code}"),
        PushStop::Refused(status) => format!("the server refused the connection with status {status}"),
        PushStop::WrongProtocol => format!("the server does not speak {PUSH_SUBPROTOCOL}"),
    }
}

/// What the loop notes from the agent's outputs between its turns.
struct Noted<'a> {
    /// A new service is ready.
    service_ready: &'a Cell<bool>,
    /// The push connection opened (true) or was lost (false).
    push_state: &'a Cell<Option<bool>>,
    /// The invalid packets' report.
    invalid: &'a Cell<InvalidReport>,
    /// A live sub-page's links while its sending is in progress.
    links: &'a RefCell<Option<Box<PageFile>>>,
    /// A `STATE_CHALLENGE` came, to be answered (VS-501).
    challenged: &'a Cell<bool>,
}

/// neottx's answer to every `STATE_CHALLENGE`: `STATE_NONE`, stateless
/// (VS-501). It keeps no key, so it never signs, and sends no cookie.
fn stateless_answer() -> Vec<u8> {
    let mut out = [0_u8; STATE_NONE_MESSAGE_BYTES];
    let item = Item::Event(InputEvent::StateNone(NoneReason::Stateless));
    let length = write(&item, &mut out).unwrap_or(0);
    out.get(..length).unwrap_or(&[]).to_vec()
}

/// `STATE_NONE` written: the event byte, its type, its length and its
/// reason.
const STATE_NONE_MESSAGE_BYTES: usize = 3 + neoteletext_input::state::STATE_NONE_BYTES;

/// Acts on one of the agent's outputs that is not for the network: a timer
/// moves the next tick, pages and notices go to the decoder, and a new
/// service or a push connection's change is noted for the loop.
fn forward(
    output: Output<'_>,
    next_tick: &mut Option<Instant>,
    noted: &Noted<'_>,
    now: Instant,
) -> std::io::Result<()> {
    let Noted { service_ready, push_state, invalid, links, challenged } = *noted;
    match output {
        Output::Timer { at } => {
            *next_tick = Some(next_tick.map_or(at, |current| current.min(at)));
            Ok(())
        }
        Output::PageBytes(page) => send_page(&page),
        Output::Event(Event::ServiceReady { initial_page, name, .. }) => {
            service_ready.set(true);
            send(&Frame::Notice(Notice::ServiceName(name.to_string())))
                .and_then(|()| send(&Frame::Notice(Notice::InitialPage(initial_page.mpp()))))
        }
        Output::Event(Event::RedirectToOtherChannel { location }) => {
            send(&Frame::Notice(Notice::RedirectToOtherChannel(location.to_string())))
        }
        Output::Event(Event::Redirect(RedirectRecord::Page { page, subcode, parameters })) => {
            send(&Frame::Notice(Notice::PageRedirect(page.mpp(), subcode.value(), parameters.map(str::to_owned))))
        }
        Output::Event(Event::Redirect(RedirectRecord::Address(address))) => {
            send(&Frame::Notice(Notice::RedirectToOtherChannel(address.to_string())))
        }
        Output::Event(Event::Denied(denied)) => {
            send(&Frame::Notice(Notice::Denied(denied.status, denied.place.to_owned())))
        }
        Output::Event(Event::BundleFailed(failure)) => {
            send(&Frame::Notice(Notice::Message(format!("index.t42 was not used: {failure:?}"))))
        }
        Output::Event(Event::Unencrypted { .. }) => send(&Frame::Notice(Notice::Unencrypted)),
        Output::Event(Event::StateChallenge(_)) => {
            challenged.set(true);
            Ok(())
        }
        Output::Event(Event::PushConnected) => {
            push_state.set(Some(true));
            Ok(())
        }
        Output::Event(Event::PushStopped(stop)) => {
            push_state.set(Some(false));
            send(&Frame::Notice(Notice::PushStopped(stopped(stop))))
        }
        // A bad message keeps the connection and says so with no wait.
        Output::Event(Event::PushLost { retry_in_us, .. }) if retry_in_us > 0 => {
            push_state.set(Some(false));
            Ok(())
        }
        // REQ-INP-026: a channel's log line from a LOG record, as an LG
        // line's would be.
        Output::Event(Event::Log { line, page }) => {
            let page = page.map_or_else(|| "no page".to_owned(), |page| format!("page {:03X}", page.mpp()));
            send(&Frame::Notice(Notice::Message(format!("log {:?}, {page}: {}", line.level, line.text))))
        }
        Output::Event(Event::ChannelConnecting) => send(&Frame::Notice(Notice::Message("connecting".into()))),
        Output::Event(Event::InvalidPackets { total }) => {
            let mut report = invalid.get();
            report.rose(total, now);
            invalid.set(report);
            Ok(())
        }
        Output::Event(Event::PageCurrent { page, .. }) => send(&Frame::Notice(Notice::Current(page.mpp()))),
        Output::Event(Event::SubPages { page, part, bytes }) => send_list_part(page.mpp(), part, bytes),
        Output::Event(
            event @ (Event::Index(_)
            | Event::IndexComplete { .. }
            | Event::SubPageList(_)
            | Event::SubPageListComplete { .. }
            | Event::SubPageListRefused { .. }
            | Event::Links(_)
            | Event::LinksRefused { .. }),
        ) => forward_record(event, links),
        Output::Event(Event::SubPagesMissing { page, part } | Event::SubPagesFailed { page, part, .. }) => {
            send(&Frame::Notice(Notice::SubPagesMissing(page.mpp(), part.number())))
        }
        Output::Event(Event::FetchFailed { page, failure: Failure::Network | Failure::Timeout, .. }) => {
            send(&Frame::Notice(Notice::Offline(page.mpp())))
        }
        Output::Event(Event::FetchFailed { page, failure, .. }) => send(&Frame::Notice(Notice::Message(format!(
            "Page {} not used: {failure:?}",
            String::from_utf8_lossy(&page.ascii())
        )))),
        Output::Event(
            event @ (Event::DescriptorRejected(_)
            | Event::DescriptorFailed(_)
            | Event::Blocked { .. }
            | Event::DowngradeRefused { .. }
            | Event::LinkUnknown),
        ) => send(&Frame::Notice(Notice::Message(format!("{event:?}")))),
        _ => Ok(()),
    }
}

/// Passes a live channel's record on to the decoder: its index (DOC-1955),
/// a page's list of sub-pages (DOC-1959), and a sub-page's links when their
/// sending is whole (DOC-1958), a refused sending dropping every link held
/// for the sub-page.
fn forward_record(event: Event<'_>, links: &RefCell<Option<Box<PageFile>>>) -> std::io::Result<()> {
    match event {
        Event::Index(record) => send(&Frame::Notice(Notice::Index(pipe::IndexPart {
            generation: record.generation,
            pages: record
                .pages()
                .map(|page| pipe::IndexEntry { page: page.page, subpages: page.subpages, title: page.title.to_owned() })
                .collect(),
        }))),
        Event::IndexComplete { generation } => send(&Frame::Notice(Notice::IndexComplete(generation))),
        Event::SubPageList(list) => send(&Frame::Notice(Notice::SubPageList(pipe::LiveList {
            page: list.page.mpp(),
            version: list.version,
            subcodes: list.entries().map(|entry| entry.subcode.value()).collect(),
        }))),
        Event::SubPageListComplete { page, version } => {
            send(&Frame::Notice(Notice::SubPageListComplete(page.mpp(), version)))
        }
        Event::SubPageListRefused { page, version } => {
            send(&Frame::Notice(Notice::SubPageListRefused(page.mpp(), version)))
        }
        Event::Links(record) => match take_links(&mut links.borrow_mut(), &record) {
            Some(page) => send_links(&page),
            None => Ok(()),
        },
        Event::LinksRefused { page, subcode } => {
            links.borrow_mut().take();
            let none = Box::new(PageFile { address: page, subcode, ..PageFile::default() });
            send_links(&none)
        }
        _ => Ok(()),
    }
}

/// Adds a LINKS record's links to the sub-page's sending: part 0 begins it,
/// a later part adds to it, as the agent checked their order. The sub-page
/// with every link of the sending, when this record ends it.
fn take_links(held: &mut Option<Box<PageFile>>, record: &LinksRecord<'_>) -> Option<Box<PageFile>> {
    if record.part == 0 {
        *held = Some(Box::new(PageFile { address: record.page, subcode: record.subcode, ..PageFile::default() }));
    }
    let page = held.as_mut()?;
    // The record's addresses are spans of its own bytes, moved to the page's.
    let mut rebase = |span: AddressSpan| {
        let address = record.address(span);
        if address.is_empty() { Some(AddressSpan::NONE) } else { page.link_text.add(address) }
    };
    let mut fitted = true;
    for mut line in record.key_links() {
        line.url = rebase(line.url).unwrap_or(AddressSpan::NONE);
        fitted &= page.links.push(line);
    }
    for mut link in record.number_links() {
        link.destination.url = rebase(link.destination.url).unwrap_or(AddressSpan::NONE);
        fitted &= page.number_links.push(link);
    }
    for mut link in record.text_links() {
        link.destination.url = rebase(link.destination.url).unwrap_or(AddressSpan::NONE);
        fitted &= page.text_links.push(link);
    }
    // The agent held the sending to a page file's bounds (DOC-1958).
    page.links_valid = fitted && !page.links.is_empty();
    (record.part.saturating_add(1) >= record.parts).then(|| held.take()).flatten()
}

/// Sends a sub-page's links to the decoder as the TTI lines a page file
/// would give them.
fn send_links(page: &PageFile) -> std::io::Result<()> {
    let mut tti = Vec::new();
    neoteletext_formats::write::write_tti(&[page], &mut tti)?;
    send(&Frame::Notice(Notice::Links(tti)))
}

/// Reads a part of a page's list with the web module's bounded reader
/// (REQ-516, REQ-1888) and passes its rows' sub-codes on; a part that does
/// not read is missing, and a row that does not is ignored and counted in a
/// message (REQ-523).
fn send_list_part(page: u16, part: PartNumber, bytes: &[u8]) -> std::io::Result<()> {
    let Ok(list) = subpages::read(bytes) else {
        send(&Frame::Notice(Notice::Message(format!(
            "page {page:03X}'s list of sub-pages, part {}, was not used: it does not read",
            part.number()
        ))))?;
        return send(&Frame::Notice(Notice::SubPagesMissing(page, part.number())));
    };
    let mut subcodes = Vec::new();
    let mut validators = Vec::new();
    let mut ignored = 0usize;
    for row in list.entries {
        match row {
            Ok(entry) => {
                subcodes.push(entry.subcode.value());
                validators.push(entry.validator.as_str().to_owned());
            }
            Err(_) => ignored = ignored.saturating_add(1),
        }
    }
    if ignored > 0 {
        send(&Frame::Notice(Notice::Message(format!(
            "page {page:03X}'s list of sub-pages, part {}: {ignored} rows ignored",
            part.number()
        ))))?;
    }
    send(&Frame::Notice(Notice::SubPages(pipe::ListPart {
        page,
        part: part.number(),
        total: list.total,
        version: list.version.as_str().to_owned(),
        subcodes,
        validators,
    })))
}

/// Sends a page the agent received to the decoder, with where it came
/// from and whether it came securely.
fn send_page(page: &PageBytes<'_>) -> std::io::Result<()> {
    let data = pipe::PageData {
        secure: page.secure,
        origin: match page.source {
            Came::Page { .. } => pipe::Origin::Page,
            Came::Push => pipe::Origin::Push,
            Came::FrontDoor => pipe::Origin::FrontDoor,
        },
        bytes: page.bytes.to_vec(),
    };
    match page.media {
        MediaType::T42 => send(&Frame::T42(data)),
        MediaType::Tti => send(&Frame::Tti(data)),
        // REQ-WEB-003: the declared type says nothing of the format.
        _ => send(&Frame::Undeclared(data)),
    }
}

/// Tells the decoder about a (new) service's refresh period, linked
/// services and aliases before any of its pages (Appendix E).
fn announce_service(descriptor: Option<&Descriptor>) -> std::io::Result<()> {
    let services: Vec<pipe::ServiceEntry> = descriptor
        .map(|descriptor| {
            descriptor
                .services()
                .map(|service| pipe::ServiceEntry {
                    short_name: String::from_utf8_lossy(service.short_name.as_bytes()).into_owned(),
                    display_name: String::from_utf8_lossy(service.display_name.as_bytes()).into_owned(),
                    origin: origin_of(descriptor.service_address(service)),
                })
                .collect()
        })
        .unwrap_or_default();
    let aliases: Vec<(u16, String)> = descriptor
        .map(|descriptor| {
            descriptor.aliases().map(|alias| (alias.page.mpp(), descriptor.alias_target(alias).to_string())).collect()
        })
        .unwrap_or_default();
    // A service that pushes keeps its pages current itself.
    let refresh =
        descriptor.filter(|descriptor| descriptor.push().is_none()).and_then(|descriptor| descriptor.refresh_seconds);
    if let Some(seconds) = refresh {
        send(&Frame::Notice(Notice::Refresh(seconds)))?;
    }
    send(&Frame::Notice(Notice::Region(descriptor.and_then(|descriptor| descriptor.region))))?;
    send(&Frame::Notice(Notice::Service(services)))?;
    send(&Frame::Notice(Notice::Aliases(aliases)))
}

/// An address's origin (REQ-WEB-015) serialised as RFC 6454 section 6.2
/// does: the scheme, the host in lower case, and the port when it is not the
/// scheme's own; empty for an address that does not parse.
fn origin_of(address: &str) -> String {
    let Ok(url) = neoteletext_web::url::Url::parse(address) else { return String::new() };
    let origin = url.origin();
    let scheme = String::from_utf8_lossy(url.scheme().name()).into_owned();
    // RFC 3986 section 3.2.2: an IPv6 address is written in brackets.
    let host = origin.host.as_str();
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
    if origin.port == url.scheme().default_port() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}:{}", origin.port)
    }
}

/// Gives the agent what the decoder asked for: a page, a key, a link to
/// another service, or a return to a service of the session, which
/// `services` numbers. `more` says another input message is already
/// waiting, to be written with this one (REQ-RCH-010).
fn hand_frame(
    agent: &mut Agent<'_>,
    services: &mut SessionServices,
    now: Instant,
    frame: Frame,
    more: bool,
    out: &mut dyn FnMut(Output<'_>),
) {
    match frame {
        Frame::Request(mpp, sub) => {
            if let Some((page, subcode)) = page_request(mpp, sub) {
                // The first part of a page's list says whether its sub-pages
                // are files in its folder, where the page's own file holds
                // the first alone (DOC-2038). The request after it goes when
                // it has answered, so the first sub-page of a page costs one
                // more request, and the next ones none once the list answered.
                if subcode.is_some_and(|code| !code.is_any()) && !agent.is_catalogued(page) {
                    agent.handle(now, Input::FetchSubPages { page, part: PartNumber::FIRST }, out);
                }
                agent.handle(now, Input::FetchPage { page, subcode }, out);
            }
        }
        Frame::SubPages(mpp, part) => {
            let page = neoteletext_core::types::PageAddress::from_mpp(mpp);
            match (page, PartNumber::new(usize::from(part))) {
                // A live channel has no files: its lists come on its
                // connection (DOC-1959), so the decoder steps by those or
                // the store.
                _ if agent.is_live() => {
                    let _ = send(&Frame::Notice(Notice::SubPagesMissing(mpp, part)));
                }
                (Some(page), Some(part)) => agent.handle(now, Input::FetchSubPages { page, part }, out),
                _ => {}
            }
        }
        Frame::Input(message) => {
            agent.handle(now, Input::Send(InputMessage { bytes: &message, move_of: None, more }), out);
        }
        Frame::Follow(service, mpp, sub) => {
            // The service's address, before following replaces the
            // descriptor that names it, for a return to it (REQ-295).
            let opened = agent.descriptor().and_then(|descriptor| {
                let linked = descriptor.service(&service)?;
                Some(Opened { address: descriptor.service_address(linked).to_string(), channel: linked.channel })
            });
            if services.followed(opened)
                && let Some((page, subcode)) = page_request(mpp, sub)
            {
                agent.handle(now, Input::FollowLink { service: &service, page, subcode }, out);
            }
        }
        Frame::Reopen(number, mpp, sub) => match (services.get(number), page_request(mpp, sub)) {
            (Some(opened), Some((page, subcode))) => {
                let open = if opened.channel {
                    Input::OpenChannel(&opened.address)
                } else {
                    Input::OpenService(&opened.address)
                };
                agent.handle(now, open, out);
                agent.handle(now, Input::FetchPage { page, subcode }, out);
            }
            _ => {
                let _ = send(&Frame::Notice(Notice::Message(format!(
                    "service {number} of this session cannot be opened again"
                ))));
            }
        },
        _ => {}
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and arithmetic on small values it chose, where an overflow would also fail it"
)]
mod report_tests {
    use super::*;

    /// A moment `seconds` after the clock's start.
    fn at(seconds: u64) -> Instant {
        Instant(seconds * MICROSECONDS_PER_SECOND)
    }

    // Verifies REQ-1462 item 4: nothing while the count is 0.
    #[test]
    fn no_line_while_the_count_is_0() {
        let mut report = InvalidReport::default();
        assert_eq!(report.due(at(1_000)), None);
    }

    // Verifies REQ-1462 item 4: one line after the window, with the total and
    // the interval's count, gathered from every rise in it.
    #[test]
    fn one_line_ends_the_window_with_its_count_and_the_total() {
        let mut report = InvalidReport::default();
        report.rose(1, at(5));
        report.rose(4, at(10));
        assert_eq!(report.due(at(24)), None, "the window is still open");
        let line = report.due(at(25)).unwrap();
        assert!(line.starts_with("4 invalid packets"), "{line}");
        assert!(line.contains("4 since the connection opened"), "{line}");
        assert_eq!(report.due(at(26)), None, "one line");
    }

    // Verifies REQ-1462 item 4: none in a quiet interval, at most one in any
    // 20 seconds, and the next line counts only its own window.
    #[test]
    fn the_next_window_counts_only_its_own() {
        let mut report = InvalidReport::default();
        report.rose(4, at(0));
        assert!(report.due(at(20)).is_some());
        assert_eq!(report.due(at(100)), None, "a quiet interval");
        report.rose(6, at(101));
        assert_eq!(report.due(at(120)), None, "fewer than 20 seconds after the rise");
        let line = report.due(at(121)).unwrap();
        assert!(line.starts_with("2 invalid packets") && line.contains("6 since"), "{line}");
    }

    // Verifies REQ-1462 item 4: the cumulative number is the connection's, so
    // a new connection counts from nothing, and its first rise opens a window.
    #[test]
    fn a_new_connection_counts_again() {
        let mut report = InvalidReport::default();
        report.rose(9, at(0));
        assert!(report.due(at(20)).is_some());
        report.rose(1, at(30));
        let line = report.due(at(50)).unwrap();
        assert!(line.starts_with("1 invalid packets") && line.contains("1 since"), "{line}");
    }

    /// VS-501: neottx answers every challenge `STATE_NONE`, stateless: the
    /// event byte, type 0x49, length 1, reason 2.
    #[test]
    fn neottx_answers_every_challenge_stateless() {
        assert_eq!(stateless_answer(), [0xFF, 0x49, 1, 2]);
    }
}
