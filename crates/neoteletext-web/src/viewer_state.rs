//! Viewer state (neoteletext-common's viewer-state design, adopted by
//! DEC-6471): the one definition, used by the browser and the server alike,
//! of the bytes a viewer key signs (VS-203), of the channel identity those
//! bytes name, and of the key identifier the server keeps (VS-302).
//!
//! The engine signs nothing and verifies nothing: the browser's trusted
//! core signs, and the server verifies, each with its own approved library.
//! Both build the bytes here, so they cannot disagree.
//!
//! The messages themselves are the input stream's
//! ([`neoteletext_input::state`]: `STATE_PROOF`, `STATE_NONE`,
//! `STATE_OFFER`) and the channel's records ([`crate::records::state`]:
//! `STATE_CHALLENGE`, `STATE_END`, `STATE_KEEP_OFFER`).

use crate::bytes::{Overflow, Writer};
use crate::channel::{self, MAX_FOLDER_PATH};
use crate::consts::{MAX_HOST_NAME_LENGTH, SHA256_DIGEST_BYTES};
use crate::sha256::Sha256;
use crate::url::{Host, Scheme, Url};

pub use neoteletext_input::state::{
    Algorithm, CHALLENGE_BYTES, Lifetime, NoneReason, PUBLIC_KEY_BYTES, SIGNATURE_BYTES, StateProof,
};

/// The domain-separation text that begins the signed bytes (VS-203 item
/// 1), followed there by one zero byte.
pub const SIGNED_DOMAIN: &[u8] = b"NeoTeletext viewer state v1";
/// The domain-separation text that begins the key identifier's input
/// (VS-302), followed there by one zero byte.
pub const KEY_ID_DOMAIN: &[u8] = b"NeoTeletext key id v1";
/// The byte that ends each domain-separation text.
const DOMAIN_END: u8 = 0;

// VS-203 and VS-302 count the texts' bytes: 27 and 21.
const _: () = assert!(SIGNED_DOMAIN.len() == 27 && KEY_ID_DOMAIN.len() == 21);

/// The bytes of a key identifier: SHA-256 (FIPS 180-4) of the domain, the
/// algorithm and the public key (VS-302).
pub const KEY_ID_BYTES: usize = SHA256_DIGEST_BYTES;
/// A key identifier.
pub type KeyId = [u8; KEY_ID_BYTES];
const _: () = assert!(KEY_ID_BYTES == crate::records::state::KEY_ID_BYTES);

/// The longest scheme and its separator a channel identity has: `https://`.
const LONGEST_SCHEME: usize = "https://".len();
/// The longest port a channel identity writes, with its colon: `:65535`.
const LONGEST_PORT: usize = ":65535".len();

/// The longest channel identity (VS-203 item 6): the scheme, the longest
/// host name (RFC 1035 section 2.3.4 as clarified by RFC 1123 section 2.1),
/// the longest port and the longest folder path (DOC-1863). A bracketed
/// IPv6 address is far shorter than the longest host name.
pub const MAX_CHANNEL_IDENTITY_BYTES: usize = LONGEST_SCHEME + MAX_HOST_NAME_LENGTH + LONGEST_PORT + MAX_FOLDER_PATH;
/// The shortest channel identity (VS-203 item 6): `http://a/`.
pub const MIN_CHANNEL_IDENTITY_BYTES: usize = "http://a/".len();

// VS-203 derives 523 from 8, 253, 6 and 256.
const _: () = assert!(MAX_CHANNEL_IDENTITY_BYTES == 523 && MIN_CHANNEL_IDENTITY_BYTES == 9);
// Its length goes in two bytes.
const _: () = assert!(MAX_CHANNEL_IDENTITY_BYTES <= u16::MAX as usize);

/// The bytes before the channel identity in the signed bytes: the domain
/// and its zero byte, the version, the algorithm, the lifetime, the
/// challenge and the identity's two-byte length (VS-203 items 1 to 6).
pub const SIGNED_PREFIX_BYTES: usize = SIGNED_DOMAIN.len() + 1 + 1 + 1 + 1 + CHALLENGE_BYTES + 2;
/// The most bytes a key ever signs (VS-203).
pub const MAX_SIGNED_BYTES: usize = SIGNED_PREFIX_BYTES + MAX_CHANNEL_IDENTITY_BYTES;

/// The protocol version the signed bytes name (VS-203 item 2): the push
/// connection's, which HELLO and WELCOME agree, so that a proof made under
/// one version is never accepted under another.
pub const SIGNED_VERSION: u8 = {
    assert!(neoteletext_input::VERSION <= u8::MAX as u32);
    neoteletext_input::VERSION as u8
};

/// Why a channel identity or the bytes to sign were refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateError {
    /// The channel identity is shorter than [`MIN_CHANNEL_IDENTITY_BYTES`]
    /// or longer than [`MAX_CHANNEL_IDENTITY_BYTES`].
    IdentityLength,
    /// The text is not a channel's folder in the canonical form of DOC-1863:
    /// the channel reader refuses it, it names a file, or reading it changes
    /// it (a capital letter in the host, a default port written, a scheme
    /// in capitals); or a folder is not an `http` or `https` folder.
    NotCanonical,
}

/// A channel identity (DOC-1863) in its canonical form: the scheme, `://`,
/// the host in lower case, the port only when it is not the scheme's
/// default, and the folder path, which begins and ends with `/`. It is
/// what a viewer key is for (VS-102) and what the signed bytes name
/// (VS-203). Made only by [`ChannelIdentity::of`] and
/// [`ChannelIdentity::read`], so that browser and server sign and verify
/// the same bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChannelIdentity {
    bytes: [u8; MAX_CHANNEL_IDENTITY_BYTES],
    length: usize,
}

impl ChannelIdentity {
    /// The identity of a channel's folder, as the channel reader gives it
    /// ([`channel::parse_address`]).
    ///
    /// # Errors
    /// [`StateError::NotCanonical`] when `folder` is not an `http` or
    /// `https` folder; [`StateError::IdentityLength`] when the identity is
    /// outside the bounds.
    pub fn of(folder: &Url) -> Result<Self, StateError> {
        if !matches!(folder.scheme(), Scheme::Http | Scheme::Https) || !folder.path().ends_with('/') {
            return Err(StateError::NotCanonical);
        }
        if folder.query().is_some() || folder.fragment().is_some() {
            return Err(StateError::NotCanonical);
        }
        let mut bytes = [0_u8; MAX_CHANNEL_IDENTITY_BYTES];
        let mut writer = Writer::new(&mut bytes);
        let written = (|| -> Result<(), Overflow> {
            writer.put(folder.scheme().name())?;
            writer.put(SCHEME_END)?;
            // RFC 3986 section 3.2.2: an IPv6 address is written in
            // brackets, which the host's text leaves out.
            let bracketed = matches!(folder.host(), Host::Ipv6(_));
            if bracketed {
                writer.byte(IPV6_OPEN)?;
            }
            writer.put(folder.host_text().as_bytes())?;
            if bracketed {
                writer.byte(IPV6_CLOSE)?;
            }
            if let Some(port) = folder.explicit_port().filter(|&port| port != folder.scheme().default_port()) {
                writer.byte(PORT_MARK)?;
                write_decimal(&mut writer, port)?;
            }
            writer.put(folder.path().as_bytes())
        })();
        written.map_err(|Overflow| StateError::IdentityLength)?;
        let length = writer.len();
        if length < MIN_CHANNEL_IDENTITY_BYTES {
            return Err(StateError::IdentityLength);
        }
        Ok(Self { bytes, length })
    }

    /// Reads `text` as a channel identity, which it must already be exactly:
    /// read by the channel reader as a folder and no file, and written back
    /// the same. A server reads each address it is configured with by this
    /// before it builds the bytes to verify (VS-301).
    ///
    /// # Errors
    /// [`StateError::IdentityLength`] when it is outside the bounds;
    /// [`StateError::NotCanonical`] otherwise.
    pub fn read(text: &str) -> Result<Self, StateError> {
        if !(MIN_CHANNEL_IDENTITY_BYTES..=MAX_CHANNEL_IDENTITY_BYTES).contains(&text.len()) {
            return Err(StateError::IdentityLength);
        }
        let read = channel::parse_address(text).map_err(|_| StateError::NotCanonical)?;
        let identity = Self::of(&read.folder)?;
        if read.file.is_some() || identity.as_str() != text {
            return Err(StateError::NotCanonical);
        }
        Ok(identity)
    }

    /// The identity's text, ASCII.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.bytes.get(..self.length).and_then(|bytes| core::str::from_utf8(bytes).ok()).unwrap_or("")
    }
}

/// Between a scheme and a host.
const SCHEME_END: &[u8] = b"://";
/// Between a host and a port.
const PORT_MARK: u8 = b':';
/// Around an IPv6 address (RFC 3986 section 3.2.2).
const IPV6_OPEN: u8 = b'[';
const IPV6_CLOSE: u8 = b']';
/// The radix of a port's digits.
const DECIMAL: u16 = 10;

/// Writes `value` in decimal, without leading zeros.
fn write_decimal(writer: &mut Writer<'_>, value: u16) -> Result<(), Overflow> {
    /// The most digits a `u16` has.
    const MOST_DIGITS: usize = 5;
    let mut digits = [0_u8; MOST_DIGITS];
    let mut rest = value;
    let mut start = MOST_DIGITS;
    loop {
        start = start.saturating_sub(1);
        let digit = u8::try_from(rest % DECIMAL).unwrap_or(0);
        if let Some(place) = digits.get_mut(start) {
            *place = b'0'.saturating_add(digit);
        }
        rest /= DECIMAL;
        if rest == 0 || start == 0 {
            break;
        }
    }
    writer.put(digits.get(start..).unwrap_or(&[]))
}

/// Writes exactly the bytes a proof signs (VS-203), and nothing else: the
/// domain and a zero byte, [`SIGNED_VERSION`], the algorithm, the lifetime,
/// the challenge, the channel identity's length in two bytes, big-endian,
/// and the channel identity. Returns the length written into `out`, at most
/// [`MAX_SIGNED_BYTES`].
///
/// The browser's trusted core signs these bytes with Ed25519 (RFC 8032
/// section 5.1.6, pure, no context, no prehash), building them itself from
/// the tab's own channel (VS-103); the server builds them for each address
/// it serves and verifies strictly (VS-301).
///
/// # Errors
/// [`StateError::IdentityLength`], which a [`ChannelIdentity`] never
/// causes.
pub fn signed_bytes(
    algorithm: Algorithm,
    lifetime: Lifetime,
    challenge: &[u8; CHALLENGE_BYTES],
    channel_identity: &ChannelIdentity,
    out: &mut [u8; MAX_SIGNED_BYTES],
) -> Result<usize, StateError> {
    let channel_identity = channel_identity.as_str();
    let length = u16::try_from(channel_identity.len()).map_err(|_| StateError::IdentityLength)?;
    let mut writer = Writer::new(out);
    let written = (|| -> Result<(), Overflow> {
        writer.put(SIGNED_DOMAIN)?;
        writer.byte(DOMAIN_END)?;
        writer.byte(SIGNED_VERSION)?;
        writer.byte(algorithm.code())?;
        writer.byte(lifetime.code())?;
        writer.put(challenge)?;
        writer.put(&length.to_be_bytes())?;
        writer.put(channel_identity.as_bytes())
    })();
    // The buffer holds the longest bytes, so writing cannot overflow.
    written.map_err(|Overflow| StateError::IdentityLength)?;
    Ok(writer.len())
}

/// The key identifier of a public key (VS-302): SHA-256 of the domain and a
/// zero byte, the algorithm and the public key. It is all the server keeps
/// of a key.
#[must_use]
pub fn key_id(algorithm: Algorithm, public_key: &[u8; PUBLIC_KEY_BYTES]) -> KeyId {
    let mut hash = Sha256::new();
    hash.update(KEY_ID_DOMAIN);
    hash.update(&[DOMAIN_END, algorithm.code()]);
    hash.update(public_key);
    hash.finish()
}

#[cfg(test)]
mod tests;
