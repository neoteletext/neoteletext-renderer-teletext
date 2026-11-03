//! The live feed's loop (REQ-INP-022): zxnet over the std transport, or any
//! transport in the tests.

use super::decoder_link::{page_request, send};
use super::transport::{StdTransport, SystemRandom};
use super::wakes::{Wake, Wakes};
use super::{clock, neottx};
use crate::pipe::{Frame, Notice};
use neoteletext_core::types::Instant;
use neoteletext_transport::{SOCKET_READ, web_address};
use neoteletext_web::port::{ConnectionId, Pump, Transport};
use neoteletext_web::proxy::Proxy;
use neoteletext_web::zxnet::{Channel, LiveEvent, LiveInput, LiveOutput, LiveStorage, ZxnetFeed};
use std::cell::Cell;
use std::net::IpAddr;
use std::sync::mpsc::Sender;
use std::time::Instant as StdInstant;

pub(super) fn run_zxnet(
    channel: &Channel,
    wakes: &mut Wakes,
    wake: Sender<Wake>,
    allow_local: &[IpAddr],
    proxy: Option<Proxy>,
    walk: bool,
) {
    let proxied = proxy.is_some();
    let transport = StdTransport::new(allow_local, wake, proxy);
    let options = LoopOptions { allow_local, proxied, walk };
    live_loop(channel, wakes, transport, &options, &mut |frame| send(frame));
}

/// How the live feed's loop runs.
pub(super) struct LoopOptions<'a> {
    /// The local addresses the feed may reach.
    pub(super) allow_local: &'a [IpAddr],
    /// Every connection goes through a proxy.
    pub(super) proxied: bool,
    /// Walk the whole service's `pageExists` list (REQ-186), at the feed's
    /// own pace, holding each page until its rows arrive or its time ends.
    pub(super) walk: bool,
}

/// The live feed's loop over any transport, sending what it receives
/// through `emit` until that fails or the decoder process goes.
pub(super) fn live_loop<T: Transport>(
    channel: &Channel,
    wakes: &mut Wakes,
    transport: T,
    options: &LoopOptions<'_>,
    emit: &mut dyn FnMut(&Frame) -> std::io::Result<()>,
) {
    let mut storage = Box::new(LiveStorage::default());
    let mut feed = new_feed(channel, &mut storage, options.allow_local, options.proxied);
    let mut pump = Pump::new(transport);
    let start = StdInstant::now();
    let mut buffer = vec![0u8; SOCKET_READ];
    let broken = Cell::new(false);
    // The feed says, after every input, when it next needs a tick, and
    // says nothing once it has stopped: the latest it said is the deadline.
    let deadline: Cell<Option<Instant>> = Cell::new(None);
    let mut sink = |output: LiveOutput<'_>, pump: &mut Pump<T>| {
        if let Some(net) = output.net() {
            pump.execute(net);
            return;
        }
        if forward(output, &deadline, emit).is_err() {
            broken.set(true);
        }
    };
    let mut step = |input: LiveInput<'_>, pump: &mut Pump<T>| {
        deadline.set(None);
        feed.handle(clock(start), input, &mut |output| sink(output, pump));
        deadline.get()
    };
    let mut next_tick = step(LiveInput::Start, &mut pump);
    // REQ-186: the walk waits for the list the server sends, and asks for
    // each page in turn once the connection is open.
    if options.walk {
        next_tick = step(LiveInput::Walk(true), &mut pump);
    }
    while !broken.get() && !wakes.ended() {
        wakes.turn();
        let mut busy = false;
        if let Some(input) = pump.poll(ConnectionId::Live, &mut buffer) {
            busy = true;
            next_tick = step(LiveInput::Net(input), &mut pump);
        }
        while let Some(frame) = wakes.next_request() {
            busy = true;
            if let Frame::Request(mpp, sub) = frame
                && let Some((page, subcode)) = page_request(mpp, sub)
            {
                next_tick = step(LiveInput::Request { page, subcode }, &mut pump);
            }
        }
        if next_tick.is_some_and(|at| clock(start) >= at) {
            // What a tick asks for, a reconnection above all, is taken up
            // on the next turn, not after another wait.
            busy = true;
            next_tick = step(LiveInput::Tick, &mut pump);
        }
        if !busy {
            wakes.wait(clock(start), next_tick);
        }
    }
}

/// The live feed for `channel`, named, told whether it goes through a
/// proxy, and given the local address it may reach.
fn new_feed<'a>(
    channel: &Channel,
    storage: &'a mut LiveStorage,
    allow_local: &[IpAddr],
    proxied: bool,
) -> ZxnetFeed<'a, SystemRandom> {
    let mut feed = ZxnetFeed::new(*channel, storage.buffers(), SystemRandom);
    feed.name_product(neottx().as_ref());
    if proxied {
        feed.through_proxy();
    }
    if let Some(&address) = allow_local.first() {
        feed.allow_local(web_address(address));
    }
    feed
}

/// Acts on one of the feed's outputs that is not for the network: a timer
/// sets the deadline, and packets and the events the decoder shows go to
/// `emit`.
fn forward(
    output: LiveOutput<'_>,
    deadline: &Cell<Option<Instant>>,
    emit: &mut dyn FnMut(&Frame) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match output {
        LiveOutput::Timer(at) => {
            deadline.set(Some(at));
            Ok(())
        }
        LiveOutput::Packet(packet) => emit(&Frame::Packet(packet.bytes)),
        LiveOutput::Event(LiveEvent::InitialPage { page, .. }) => emit(&Frame::Notice(Notice::InitialPage(page.mpp()))),
        LiveOutput::Event(LiveEvent::GaveUp) => {
            emit(&Frame::Notice(Notice::Message("live feed: gave up reconnecting".into())))
        }
        LiveOutput::Event(LiveEvent::WalkComplete) => {
            emit(&Frame::Notice(Notice::Message("live feed: walked every page the service lists".into())))
        }
        LiveOutput::Event(LiveEvent::WrongName) => {
            emit(&Frame::Notice(Notice::Message("live feed: wrong service name".into())))
        }
        // REQ-186: any disagreement between the server's header fields and
        // the decoded packet is reported.
        LiveOutput::Event(LiveEvent::HeaderMismatch { field, server, decoded }) => {
            let decoded = decoded.map_or_else(|| "uncorrectable".to_owned(), |value| format!("{value:#X}"));
            emit(&Frame::Notice(Notice::Message(format!(
                "live feed: header {field:?} disagrees: the server says {server:#X}, the packet decodes to {decoded}"
            ))))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, reason = "a test")]
mod tests {
    use super::*;
    use neoteletext_web::zxnet::HeaderField;

    /// The message `forward` gives for `event`.
    fn message(event: LiveEvent) -> String {
        let deadline = Cell::new(None);
        let mut sent = Vec::new();
        forward(LiveOutput::Event(event), &deadline, &mut |frame| {
            sent.push(frame.clone());
            Ok(())
        })
        .unwrap();
        match sent.as_slice() {
            [Frame::Notice(Notice::Message(text))] => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    // REQ-186: a disagreement between the server's header fields and the
    // decoded packet is reported, naming the field and both values.
    #[test]
    fn a_header_disagreement_is_reported() {
        let text = message(LiveEvent::HeaderMismatch { field: HeaderField::SubCode, server: 3, decoded: Some(4) });
        assert!(text.contains("SubCode") && text.contains("0x3") && text.contains("0x4"), "{text}");
        let text = message(LiveEvent::HeaderMismatch { field: HeaderField::Page, server: 0x104, decoded: None });
        assert!(text.contains("Page") && text.contains("0x104") && text.contains("uncorrectable"), "{text}");
    }
}
