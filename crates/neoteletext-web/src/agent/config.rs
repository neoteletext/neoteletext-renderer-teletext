//! The agent's configuration and the buffers it works in, supplied by the
//! caller (REQ-GEN-003, REQ-SEC-020).

use crate::consts::{
    ALLOWED_LOCAL, DEFAULT_CONNECT_TIMEOUT_US, DEFAULT_DESCRIPTOR_LIMIT, DEFAULT_FETCH_INTERVAL_US,
    DEFAULT_HEADER_LIMIT, DEFAULT_PAGE_LIMIT, DEFAULT_PUSH_LIMIT, DEFAULT_RESPONSE_TIMEOUT_US,
};
use crate::url::IpAddress;
use neoteletext_core::types::{PageAddress, SubCode};

/// The one page a snapshot asks for (REQ-6880, DEC-6875 item 3): a display
/// page, and a sub-code, [`SubCode::ANY`] (3F7F) for the page as a whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotTarget {
    /// The page, 100 to 8FF.
    pub page: PageAddress,
    /// The sub-code, or [`SubCode::ANY`] for the page as a whole.
    pub sub: SubCode,
}

/// The agent's configuration. Every security setting defaults to the safer
/// choice (REQ-SEC-020).
#[allow(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent yes-or-no setting named by a requirement, not a state"
)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    /// Largest page response body (REQ-WEB-008), at most the body buffer.
    pub page_limit: usize,
    /// Largest push message (REQ-WEB-008), at most the push buffer.
    pub push_limit: usize,
    /// Largest descriptor (REQ-WEB-008), at most the body buffer.
    pub descriptor_limit: usize,
    /// Largest response head (REQ-WEB-008), at most the head buffers.
    pub header_limit: usize,
    /// Shortest interval between fetches (REQ-WEB-004).
    pub fetch_interval_us: u64,
    /// Time limit for opening a connection (REQ-SEC-011).
    pub connect_timeout_us: u64,
    /// Time limit for a request and its response (REQ-SEC-011).
    pub response_timeout_us: u64,
    /// The port can offer TLS. Without it, HTTPS addresses are reported
    /// unreachable (REQ-WEB-008).
    pub tls_available: bool,
    /// Local-network addresses explicitly allowed (REQ-SEC-006). None by
    /// default.
    pub allowed_local: [Option<IpAddress>; ALLOWED_LOCAL],
    /// Refuse redirects of descriptor, page, bundle and contents requests
    /// that leave the channel, reporting [`Event::RedirectToOtherChannel`](super::Event::RedirectToOtherChannel)
    /// instead, so that the viewer can confirm another channel before
    /// showing its content (REQ-WEB-021). An upgrade from `http` to `https`
    /// on the same host and folder, between the default ports, keeps the
    /// channel. For a service opened by its descriptor, the channel is its
    /// origin. On by default, the safer choice (REQ-SEC-020): a port that
    /// follows redirects out of the channel turns it off, as `neottx` does
    /// for a service opened by its descriptor.
    pub redirects_within_channel: bool,
    /// The program the agent names itself as, in its `User-Agent`
    /// (REQ-WEB-031); `None` names the engine.
    pub product: Option<crate::user_agent::Product>,
    /// The port's transport goes through a proxy (REQ-WEB-032), so the
    /// peer it reports is the proxy, which the user chose: the local
    /// network is then refused by what the agent can see of a host before
    /// it connects ([`crate::proxy::hidden_local`]), not by the peer.
    pub proxied: bool,
    /// Snapshot mode (REQ-6880): the agent fetches this one page as a
    /// stranger sees it and nothing else, then ends with
    /// [`Event::SnapshotEnded`](super::Event::SnapshotEnded). It opens the
    /// channel or service it is given once; for a live channel it opens one
    /// push connection and sends the snapshot request itself
    /// (`neoteletext_input::snapshot`), never reconnecting; for a static
    /// channel it fetches the target's file and no bundle, contents or list.
    /// It sends no conditional request and refuses every input message its
    /// caller gives it, and it ends by
    /// [`SNAPSHOT_TIME_LIMIT_US`](crate::consts::SNAPSHOT_TIME_LIMIT_US).
    /// `None`, the default, changes nothing.
    pub snapshot: Option<SnapshotTarget>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            page_limit: DEFAULT_PAGE_LIMIT,
            push_limit: DEFAULT_PUSH_LIMIT,
            descriptor_limit: DEFAULT_DESCRIPTOR_LIMIT,
            header_limit: DEFAULT_HEADER_LIMIT,
            fetch_interval_us: DEFAULT_FETCH_INTERVAL_US,
            connect_timeout_us: DEFAULT_CONNECT_TIMEOUT_US,
            response_timeout_us: DEFAULT_RESPONSE_TIMEOUT_US,
            tls_available: true,
            allowed_local: [None; ALLOWED_LOCAL],
            redirects_within_channel: true,
            product: None,
            proxied: false,
            snapshot: None,
        }
    }
}

/// The byte buffers the agent works in, supplied by the caller
/// (REQ-GEN-003). Their lengths bound the corresponding limits.
#[derive(Debug)]
pub struct Buffers<'m> {
    /// Response heads on the fetch connection.
    pub fetch_head: &'m mut [u8],
    /// Requests on the fetch connection.
    pub fetch_request: &'m mut [u8],
    /// Page, descriptor, bundle, contents and list response bodies.
    pub body: &'m mut [u8],
    /// The push connection's handshake response head.
    pub push_head: &'m mut [u8],
    /// Requests and frames on the push connection.
    pub push_request: &'m mut [u8],
    /// One push message.
    pub push_message: &'m mut [u8],
}

/// The default body buffer: a page response. A part of a page's list is far
/// smaller, so every sub-page of a page of the standard's maximum is reachable
/// (REQ-NUM-030).
const BODY_BYTES: usize = DEFAULT_PAGE_LIMIT;
const _: () = assert!(crate::subpages::MAX_BYTES <= BODY_BYTES, "a part of a list fits the body buffer");

/// The push connection's default buffers: none in a build without push
/// (decision E.61), whose agent never opens the connection, so that such a
/// build does not carry them.
const PUSH_HEAD_BYTES: usize = if cfg!(feature = "push") { DEFAULT_HEADER_LIMIT } else { 0 };
const PUSH_REQUEST_BYTES: usize = if cfg!(feature = "push") { crate::consts::DEFAULT_REQUEST_LIMIT } else { 0 };
const PUSH_MESSAGE_BYTES: usize = if cfg!(feature = "push") { DEFAULT_PUSH_LIMIT } else { 0 };

/// Buffers of the default sizes, for callers that do not need to choose.
#[derive(Debug)]
pub struct Storage {
    fetch_head: [u8; DEFAULT_HEADER_LIMIT],
    fetch_request: [u8; crate::consts::DEFAULT_REQUEST_LIMIT],
    body: [u8; BODY_BYTES],
    push_head: [u8; PUSH_HEAD_BYTES],
    push_request: [u8; PUSH_REQUEST_BYTES],
    push_message: [u8; PUSH_MESSAGE_BYTES],
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage {
    /// Zeroed buffers. Large: the caller places them in static memory or
    /// on the heap.
    #[must_use]
    #[allow(
        clippy::large_stack_arrays,
        reason = "the arrays are the buffers themselves, so building them is the point; the caller places the value in static memory or on the heap"
    )]
    pub const fn new() -> Self {
        Self {
            fetch_head: [0; DEFAULT_HEADER_LIMIT],
            fetch_request: [0; crate::consts::DEFAULT_REQUEST_LIMIT],
            body: [0; BODY_BYTES],
            push_head: [0; PUSH_HEAD_BYTES],
            push_request: [0; PUSH_REQUEST_BYTES],
            push_message: [0; PUSH_MESSAGE_BYTES],
        }
    }

    /// The buffers, borrowed for an agent.
    pub fn buffers(&mut self) -> Buffers<'_> {
        Buffers {
            fetch_head: &mut self.fetch_head,
            fetch_request: &mut self.fetch_request,
            body: &mut self.body,
            push_head: &mut self.push_head,
            push_request: &mut self.push_request,
            push_message: &mut self.push_message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BODY_BYTES, DEFAULT_HEADER_LIMIT, DEFAULT_PUSH_LIMIT, Storage};
    use crate::consts::DEFAULT_REQUEST_LIMIT;

    // Decision E.61 (TODO-1106): the default buffers follow the features: a
    // build without push carries the fetch connection's buffers alone, and
    // one with push the push connection's as well.
    #[test]
    fn the_default_buffers_follow_the_push_feature() {
        let fetch = DEFAULT_HEADER_LIMIT + DEFAULT_REQUEST_LIMIT + BODY_BYTES;
        let push = DEFAULT_HEADER_LIMIT + DEFAULT_REQUEST_LIMIT + DEFAULT_PUSH_LIMIT;
        let expected = if cfg!(feature = "push") { fetch + push } else { fetch };
        assert_eq!(size_of::<Storage>(), expected);
    }
}
