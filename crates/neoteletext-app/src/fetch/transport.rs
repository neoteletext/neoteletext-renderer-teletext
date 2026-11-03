//! The std transport: the engine's network port over `neoteletext-transport`,
//! each connection read by a thread of its own that wakes the loop when bytes
//! arrive (decision 27, REQ-WEB-032).

use super::decoder_link::send;
use super::wakes::Wake;
use crate::pipe::{Frame, Notice};
use neoteletext_transport::{
    Connection, SOCKET_READ, TlsFailure, client_config, connect_within_limit, mozilla_roots, open_tunnel, start_tls,
    web_address, write_records,
};
use neoteletext_web::port::{ConnectionId, LinkState, RandomSource, Received, Security, Transport};
use neoteletext_web::proxy::Proxy;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

/// A transport over std sockets and rustls, checking every resolved
/// address against the policy before connecting (decision 27), whose
/// connections each wake the loop when bytes arrive. A connection is opened
/// on a thread of its own, so that `connect` returns at once and the loop
/// goes on while a name resolves, a proxy answers and TLS shakes hands
/// (REQ-490, DEC-2490).
pub struct StdTransport {
    streams: [Option<Connection>; 3],
    states: [LinkState; 3],
    /// Each connection's opening while it is under way: the thread hands
    /// back the connection and its peer, or why it failed.
    openings: [Option<Receiver<Opened>>; 3],
    opener: Opener,
}

/// What an opening thread hands back.
type Opened = Result<(Connection, IpAddr), String>;

/// What opening a connection needs, owned, so that a thread of its own can
/// do it.
#[derive(Clone)]
struct Opener {
    tls: Arc<rustls::ClientConfig>,
    allowed_local: Vec<IpAddr>,
    wake: Sender<Wake>,
    /// Every connection goes through this proxy (REQ-WEB-032).
    proxy: Option<Proxy>,
}

fn slot(connection: ConnectionId) -> usize {
    match connection {
        ConnectionId::Fetch => 0,
        ConnectionId::Push => 1,
        ConnectionId::Live => 2,
    }
}

impl StdTransport {
    /// A transport trusting the Mozilla root certificates, waking the loop
    /// through `wake`.
    #[must_use]
    pub fn new(allowed_local: &[IpAddr], wake: Sender<Wake>, proxy: Option<Proxy>) -> Self {
        let tls = client_config(mozilla_roots());
        Self {
            streams: [None, None, None],
            states: [LinkState::Closed; 3],
            openings: [None, None, None],
            opener: Opener { tls: Arc::new(tls), allowed_local: allowed_local.to_vec(), wake, proxy },
        }
    }

    /// Takes up the opening of `connection` if it has ended: the connection
    /// is open, or has failed and the user is told why.
    fn settle(&mut self, connection: ConnectionId) {
        let index = slot(connection);
        let Some(opening) = self.openings.get(index).and_then(Option::as_ref) else { return };
        let opened = match opening.try_recv() {
            Ok(opened) => opened,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the connection's opening ended without a result".to_string()),
        };
        if let Some(opening) = self.openings.get_mut(index) {
            *opening = None;
        }
        let (state, stream) = match opened {
            Ok((stream, peer)) => (LinkState::Open(web_address(peer)), Some(stream)),
            Err(message) => {
                let _ = send(&Frame::Notice(Notice::Message(message)));
                (LinkState::Failed, None)
            }
        };
        if let (Some(slot_state), Some(slot_stream)) = (self.states.get_mut(index), self.streams.get_mut(index)) {
            *slot_state = state;
            *slot_stream = stream;
        }
    }
}

impl Opener {
    /// A socket that reaches `host` and `port`: directly, to an address the
    /// local-network rule allows, or through the proxy, whose address the
    /// user chose, after its handshake (REQ-WEB-032). With it, the host's
    /// first bytes when the proxy's answer brought them.
    fn reach(&self, host: &str, port: u16) -> Result<(TcpStream, IpAddr, Vec<u8>), String> {
        let (socket, early) = if let Some(proxy) = &self.proxy {
            open_tunnel(proxy, host, port).map_err(|error| error.to_string())?
        } else {
            let candidates: Vec<SocketAddr> =
                (host, port).to_socket_addrs().map_err(|error| format!("{host}: {error}"))?.collect();
            let allowed = candidates
                .iter()
                .find(|address| !web_address(address.ip()).is_blocked() || self.allowed_local.contains(&address.ip()))
                .ok_or_else(|| format!("{host}: every address is on a blocked local network (REQ-SEC-006)"))?;
            (connect_within_limit(allowed, host).map_err(|error| error.to_string())?, Vec::new())
        };
        socket.set_nodelay(true).map_err(|error| error.to_string())?;
        let peer = socket.peer_addr().map_err(|error| error.to_string())?.ip();
        Ok((socket, peer, early))
    }

    fn open(&self, host: &str, port: u16, security: Security) -> Result<(Connection, IpAddr), String> {
        let (mut socket, peer, early) = self.reach(host, port)?;
        let (tls, first) = match security {
            Security::Plain => (None, early),
            Security::Tls => {
                let connection =
                    start_tls(self.tls.clone(), host, &early, &mut socket).map_err(|failure| match failure {
                        TlsFailure::Name(error) => format!("{host}: {error}"),
                        TlsFailure::Start(error) => error.to_string(),
                        TlsFailure::Read(_) | TlsFailure::Records(_) | TlsFailure::Handshake(_) => {
                            format!("{host}: TLS: {failure}")
                        }
                    })?;
                (Some(Box::new(connection)), Vec::new())
            }
        };
        let wake = self.wake.clone();
        let wake = move || {
            let _ = wake.send(Wake::Network);
        };
        let connection = Connection::start(socket, tls, first, SOCKET_READ, wake).map_err(|error| error.to_string())?;
        Ok((connection, peer))
    }
}

/// What has arrived on `connection`, looking without waiting: any failure
/// is the connection's end.
fn receive(connection: &mut Connection, buffer: &mut [u8]) -> Received {
    connection.take(buffer, None, write_records).unwrap_or(Received::Closed)
}

impl embedded_io::ErrorType for StdTransport {
    type Error = embedded_io::ErrorKind;
}

impl Transport for StdTransport {
    fn connect(
        &mut self,
        connection: ConnectionId,
        host: &str,
        port: u16,
        security: Security,
    ) -> Result<(), Self::Error> {
        // REQ-490: the name's resolution, the connection, the proxy's and
        // TLS's handshakes run on a thread of their own, each within its
        // limit, and the loop hears the result as the connection's state,
        // woken when it comes.
        self.close(connection);
        let index = slot(connection);
        let (handed, opening) = mpsc::channel();
        let opener = self.opener.clone();
        let host = host.to_owned();
        std::thread::spawn(move || {
            let result = opener.open(&host, port, security);
            match handed.send(result) {
                Ok(()) => {
                    let _ = opener.wake.send(Wake::Network);
                }
                // Closed while it was opening: the connection is ended.
                Err(mpsc::SendError(Ok((connection, _)))) => connection.shutdown(),
                Err(mpsc::SendError(Err(_))) => {}
            }
        });
        if let (Some(state), Some(slot_opening)) = (self.states.get_mut(index), self.openings.get_mut(index)) {
            *state = LinkState::Connecting;
            *slot_opening = Some(opening);
        }
        Ok(())
    }

    fn state(&mut self, connection: ConnectionId) -> LinkState {
        self.settle(connection);
        self.states.get(slot(connection)).copied().unwrap_or(LinkState::Closed)
    }

    fn send(&mut self, connection: ConnectionId, bytes: &[u8]) -> Result<(), Self::Error> {
        let open = self
            .streams
            .get_mut(slot(connection))
            .and_then(Option::as_mut)
            .ok_or(embedded_io::ErrorKind::NotConnected)?;
        // Written whole, blocking, within the write limit: a request or an
        // input message is small, and the kernel takes it at once unless
        // the peer has stopped reading for long enough to fill the socket's
        // buffer (DEC-2490 item 2; the writer thread that would make this
        // non-blocking too is owed in TODO-2058).
        open.write_all(bytes).map_err(|_| embedded_io::ErrorKind::Other)
    }

    /// Nothing waits here: a send is written whole, blocking, to the
    /// kernel, and what the kernel has not sent cannot be read without
    /// unsafe code (REQ-SEC-015), so moves are never held for this
    /// transport (REQ-RCH-035; an rdm to-do).
    fn unsent(&mut self, _: ConnectionId) -> usize {
        0
    }

    fn receive(&mut self, connection: ConnectionId, buffer: &mut [u8]) -> Result<Received, Self::Error> {
        let open = self.streams.get_mut(slot(connection)).and_then(Option::as_mut);
        Ok(open.map_or(Received::Closed, |open| receive(open, buffer)))
    }

    fn close(&mut self, connection: ConnectionId) {
        let index = slot(connection);
        // An opening under way is abandoned: its thread ends the connection
        // it opens, since nothing takes it.
        if let Some(opening) = self.openings.get_mut(index) {
            *opening = None;
        }
        if let Some(open) = self.streams.get_mut(index).and_then(Option::take) {
            open.shutdown();
        }
        if let Some(state) = self.states.get_mut(index) {
            *state = LinkState::Closed;
        }
    }
}

/// Random bytes from the TLS library's system generator.
pub(super) struct SystemRandom;

impl RandomSource for SystemRandom {
    fn fill(&mut self, bytes: &mut [u8]) {
        let provider = rustls::crypto::ring::default_provider();
        let _ = provider.secure_random.fill(bytes);
    }
}

#[cfg(test)]
mod tests;
