//! The fetcher process of web and live mode (REQ-SEC-018): it holds the
//! network and runs the web module's state machines over a std transport
//! with TLS, sending packets and page bytes to the decoder process on
//! standard output (decision 26 of IMPLEMENTATION-LOG.md).

use neoteletext_core::types::Instant;
use neoteletext_web::proxy::Proxy;
use neoteletext_web::zxnet::{Channel, ChannelError, KNOWN_CHANNELS};
use std::net::IpAddr;
use std::sync::mpsc;
use std::time::Instant as StdInstant;

mod decoder_link;
mod font;
mod transport;
mod wakes;
mod web;
mod zxnet;

pub use transport::StdTransport;
pub use wakes::Wake;

/// Directories the fetcher may read to resolve names (decision 28).
const RESOLVER_PATHS: [&str; 6] = ["/etc", "/run/systemd/resolve", "/usr/lib", "/lib", "/usr/lib64", "/lib64"];

/// Runs the fetcher for `address` until the decoder process goes away.
/// With `walk`, a live feed walks its whole service (REQ-186); a web
/// service or channel has no such walk, and says so.
///
/// # Errors
/// The exit status and the message for the user (DEC-2451): a proxy
/// that is not `http://host:port` or `socks5://host:port`, a sandbox that
/// cannot be applied, or an unknown zxnet live feed.
pub fn run(
    address: &str,
    no_sandbox: bool,
    allow_local: &[IpAddr],
    proxy: Option<&str>,
    walk: bool,
) -> Result<(), (u8, String)> {
    run_as(address, no_sandbox, allow_local, proxy, if walk { Role::Walk } else { Role::Read })
}

/// What a fetcher process is started for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    /// A live feed, web service or channel, read as the decoder asks.
    Read,
    /// A live feed's whole service, walked (REQ-186).
    Walk,
    /// One font file at an address the user named (REQ-123).
    Font,
}

/// Runs the fetcher for the font at `address` (REQ-123): it sends the
/// font's bytes, or why they could not be fetched, to the decoder process
/// as one frame, and ends.
///
/// # Errors
/// As [`run`]'s: a proxy of another form, or a sandbox that cannot be
/// applied.
pub fn run_font(
    address: &str,
    no_sandbox: bool,
    allow_local: &[IpAddr],
    proxy: Option<&str>,
) -> Result<(), (u8, String)> {
    run_as(address, no_sandbox, allow_local, proxy, Role::Font)
}

fn run_as(
    address: &str,
    no_sandbox: bool,
    allow_local: &[IpAddr],
    proxy: Option<&str>,
    role: Role,
) -> Result<(), (u8, String)> {
    let walk = role == Role::Walk;
    let proxy = proxy.map(Proxy::parse).transpose().map_err(|error| (crate::exit::USAGE, error.to_string()))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (wake, receiver) = mpsc::channel();
    decoder_link::requests(wake.clone());
    let mut wakes = wakes::Wakes::new(receiver);
    // Name resolution reads the resolver configuration and loads the C
    // library's name-service modules (decision 28 of IMPLEMENTATION-LOG.md).
    let readable: Vec<&std::path::Path> =
        RESOLVER_PATHS.iter().map(std::path::Path::new).filter(|path| path.exists()).collect();
    crate::sandbox::apply(
        crate::sandbox::Policy { writable_directory: None, network: true, readable: &readable },
        no_sandbox,
    )
    .map_err(|error| (crate::exit::INPUT, error.to_string()))?;
    if role == Role::Font {
        font::run_font(address, &mut wakes, wake, allow_local, proxy);
    } else if let Some(channel) = feed(address) {
        let channel = channel.map_err(|error| (crate::exit::USAGE, format!("unknown live feed: {error:?}")))?;
        zxnet::run_zxnet(&channel, &mut wakes, wake, allow_local, proxy, walk);
    } else {
        if walk {
            let note = "--walk walks a zxnet feed's service; a web service or channel is read as asked";
            let _ = decoder_link::send(&crate::pipe::Frame::Notice(crate::pipe::Notice::Message(note.into())));
        }
        web::run_web(address, &mut wakes, wake, allow_local, proxy);
    }
    Ok(())
}

/// The live feed an input names, if it names one (REQ-INP-022): a short
/// name or a channel identifier after `zxnet:`, or a `wss` address.
fn feed(address: &str) -> Option<Result<Channel, ChannelError>> {
    if let Some(name) = address.strip_prefix("zxnet:") {
        return Some(
            Channel::from_short_name(name).or_else(|unknown| Channel::from_identifier(name).map_err(|_| unknown)),
        );
    }
    address.starts_with("wss://").then(|| feed_at(address))
}

/// The feed at a `wss` address, which carries no service name: a known
/// feed's own, or the last segment of the path (DEC-2134). A wrong guess
/// draws `wrongname`, which stops the feed with its message.
fn feed_at(address: &str) -> Result<Channel, ChannelError> {
    let known = KNOWN_CHANNELS
        .iter()
        .filter_map(|(short, _)| Channel::from_short_name(short).ok())
        .find(|channel| channel.address().as_str() == address);
    known.map_or_else(|| Channel::from_address(address, address.rsplit('/').next().unwrap_or_default()), Ok)
}

/// `neottx` as it names itself to a channel (REQ-WEB-031): `neottx/0.1
/// (macOS)`, one of the engine's own programs; on a system Rust does not
/// name, the engine's own string.
pub(super) fn neottx() -> Option<neoteletext_web::user_agent::Product> {
    neoteletext_web::user_agent::os_family().map(|family| neoteletext_web::user_agent::Product {
        name: "neottx",
        version: env!("CARGO_PKG_VERSION"),
        family,
        engine_own: true,
    })
}

pub(super) fn clock(start: StdInstant) -> Instant {
    Instant(u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests;
