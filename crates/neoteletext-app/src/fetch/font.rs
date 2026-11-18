//! The fetcher process's font fetch (REQ-123): one font file at the address
//! the user named, fetched by the web agent's font role under the rules of a
//! channel's files, sent to the decoder process as one frame, after which
//! the fetcher ends.

use super::decoder_link::send;
use super::transport::{StdTransport, SystemRandom};
use super::wakes::{Wake, Wakes};
use super::{clock, neottx};
use crate::pipe::{Frame, Notice};
use neoteletext_core::types::Instant;
use neoteletext_transport::{SOCKET_READ, web_address};
use neoteletext_web::agent::{Config, Event, Input, Output, ResourceRole, Storage, UserAgent};
use neoteletext_web::port::{ConnectionId, Pump};
use neoteletext_web::proxy::Proxy;
use std::cell::Cell;
use std::net::IpAddr;
use std::sync::mpsc::Sender;
use std::time::Instant as StdInstant;

/// Fetches the font at `address` and sends it, or why it failed, to the
/// decoder process.
pub(super) fn run_font(
    address: &str,
    wakes: &mut Wakes,
    wake: Sender<Wake>,
    allow_local: &[IpAddr],
    proxy: Option<Proxy>,
) {
    let mut storage = Box::new(Storage::default());
    let mut config = Config { product: neottx(), proxied: proxy.is_some(), ..Config::default() };
    for (slot, &local) in config.allowed_local.iter_mut().zip(allow_local) {
        *slot = Some(web_address(local));
    }
    let mut agent: Box<UserAgent<'_, SystemRandom>> = Box::new(UserAgent::new(config, storage.buffers(), SystemRandom));
    let mut pump = Pump::new(StdTransport::new(allow_local, wake, proxy));
    let start = StdInstant::now();
    let mut buffer = vec![0_u8; SOCKET_READ];
    // The fetch has ended, by the font's bytes or by its failure, each
    // sent to the decoder; or the pipe to the decoder broke.
    let done = Cell::new(false);
    let next_tick: Cell<Option<Instant>> = Cell::new(None);
    let sink = |output: Output<'_>, pump: &mut Pump<StdTransport>| {
        if let Some(net) = output.net() {
            pump.execute(net);
            return;
        }
        let sent = match output {
            Output::Timer { at } => {
                next_tick.set(Some(at));
                Ok(())
            }
            // REQ-SEC-005, REQ-SEC-008: the user is told a font came over
            // plain HTTP.
            Output::Event(Event::Unencrypted { .. }) => send(&Frame::Notice(Notice::Unencrypted)),
            Output::Event(Event::Font { bytes, .. }) => {
                done.set(true);
                send(&Frame::Font(bytes.to_vec()))
            }
            Output::Event(Event::ResourceFailed { failure, .. }) => {
                done.set(true);
                send(&Frame::Notice(Notice::Message(format!("the font could not be fetched: {failure:?}"))))
            }
            _ => Ok(()),
        };
        if sent.is_err() {
            done.set(true);
        }
    };
    let fetch = Input::FetchResource { role: ResourceRole::Font, address };
    agent.handle(clock(start), fetch, &mut |output| sink(output, &mut pump));
    while !done.get() && !wakes.ended() {
        wakes.turn();
        let mut busy = false;
        if let Some(input) = pump.poll(ConnectionId::Fetch, &mut buffer) {
            busy = true;
            agent.handle(clock(start), Input::Net(input), &mut |output| sink(output, &mut pump));
        }
        if next_tick.get().is_some_and(|at| clock(start) >= at) {
            busy = true;
            next_tick.set(None);
            agent.handle(clock(start), Input::Tick, &mut |output| sink(output, &mut pump));
        }
        if !busy && !done.get() {
            wakes.wait(clock(start), next_tick.get());
        }
    }
}
