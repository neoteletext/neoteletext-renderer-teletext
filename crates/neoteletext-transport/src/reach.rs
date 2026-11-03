//! Opening a socket, each step within the connect limit: to an address the
//! caller's own rule chose, or to the proxy the user chose, which then
//! opens a tunnel to the host (REQ-WEB-032).

use neoteletext_web::consts::DEFAULT_CONNECT_TIMEOUT_US;
use neoteletext_web::proxy::{Proxy, TunnelError};
use neoteletext_web::url::IpAddress;
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// Time allowed to connect and to complete the TLS handshake; a write and
/// a read of the proxy's answer are held to it too.
pub const CONNECT_TIMEOUT: Duration = Duration::from_micros(DEFAULT_CONNECT_TIMEOUT_US);

/// Why a socket could not be opened: each names what the user knows the
/// host or the proxy as, which the message repeats.
#[derive(Debug)]
pub enum ReachError {
    /// The name did not resolve: the system's resolver failed.
    Resolve {
        /// The name.
        host: String,
        /// What the resolver said.
        error: std::io::Error,
    },
    /// The name resolved to no address at all.
    NoAddress {
        /// The name.
        host: String,
    },
    /// The connection was refused or did not complete within
    /// [`CONNECT_TIMEOUT`].
    Connect {
        /// What the user knows the target as.
        name: String,
        /// What the system said.
        error: std::io::Error,
    },
    /// The socket's read and write limits could not be set.
    Limits(std::io::Error),
    /// The proxy refused the tunnel to the host, broke its protocol, or
    /// its connection failed.
    Tunnel {
        /// The host the tunnel was to reach.
        host: String,
        /// Why.
        error: TunnelError,
    },
}

impl core::fmt::Display for ReachError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Resolve { host, error } => write!(formatter, "{host}: {error}"),
            Self::NoAddress { host } => write!(formatter, "{host}: no address"),
            Self::Connect { name, error } => write!(formatter, "{name}: {error}"),
            Self::Limits(error) => write!(formatter, "{error}"),
            Self::Tunnel { host, error } => write!(formatter, "the proxy refused the connection to {host}: {error}"),
        }
    }
}

impl core::error::Error for ReachError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Resolve { error, .. } | Self::Connect { error, .. } | Self::Limits(error) => Some(error),
            Self::NoAddress { .. } => None,
            Self::Tunnel { error, .. } => Some(error),
        }
    }
}

/// The first address `host` resolves to.
///
/// # Errors
/// [`ReachError::Resolve`] when the name does not resolve,
/// [`ReachError::NoAddress`] when it resolves to none.
pub fn first_address(host: &str, port: u16) -> Result<SocketAddr, ReachError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| ReachError::Resolve { host: host.to_owned(), error })?
        .next()
        .ok_or_else(|| ReachError::NoAddress { host: host.to_owned() })
}

/// A socket to `target`, whose reads and writes are held to
/// [`CONNECT_TIMEOUT`]. `name` is what the user knows the target as.
///
/// # Errors
/// [`ReachError::Connect`] when the connection is refused or times out,
/// [`ReachError::Limits`] when the socket's limits cannot be set.
pub fn connect_within_limit(target: &SocketAddr, name: &str) -> Result<TcpStream, ReachError> {
    let socket = TcpStream::connect_timeout(target, CONNECT_TIMEOUT)
        .map_err(|error| ReachError::Connect { name: name.to_owned(), error })?;
    socket.set_read_timeout(Some(CONNECT_TIMEOUT)).map_err(ReachError::Limits)?;
    socket.set_write_timeout(Some(CONNECT_TIMEOUT)).map_err(ReachError::Limits)?;
    Ok(socket)
}

/// A socket through `proxy` that reaches `host` and `port`, with the
/// host's first bytes when the proxy's answer brought them, which the
/// caller hands on before anything it reads next (decision E.102). The
/// proxy's address may be local: the user chose it.
///
/// # Errors
/// The proxy cannot be reached ([`first_address`] and
/// [`connect_within_limit`]'s errors, naming the proxy), or
/// [`ReachError::Tunnel`] when it refuses; nothing goes direct instead.
pub fn open_tunnel(proxy: &Proxy, host: &str, port: u16) -> Result<(TcpStream, Vec<u8>), ReachError> {
    let target = first_address(&proxy.host, proxy.port)?;
    let mut socket = connect_within_limit(&target, &proxy.host)?;
    let first = proxy
        .open_tunnel(&mut socket, host, port)
        .map_err(|error| ReachError::Tunnel { host: host.to_owned(), error })?;
    Ok((socket, first))
}

/// The address the operating system gave, as the engine writes one.
#[must_use]
pub fn web_address(address: IpAddr) -> IpAddress {
    match address {
        IpAddr::V4(v4) => IpAddress::V4(v4.octets()),
        IpAddr::V6(v6) => IpAddress::V6(v6.segments()),
    }
}
