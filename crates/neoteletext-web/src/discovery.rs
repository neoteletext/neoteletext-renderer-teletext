//! The link-pointer reader of discovery (REQ-2476; the browser's REQ-693,
//! decision 14, DEC-913): the pointers an ordinary web page gives to its
//! teletext channel, read from its `Link` header fields and from the
//! `<link>` elements in the first [`DISCOVERY_BODY_BYTES`] of its body, and
//! nothing else of it.
//!
//! ```text
//! Link: <https://example.org/teletext/>; rel="alternate"; type="application/vnd.neoteletext+json"
//! <link rel="alternate" type="application/vnd.neoteletext+json" href="https://example.org/teletext/">
//! ```
//!
//! Neither reader is a parser of HTML or of every header: each is a byte
//! loop that knows only what finding a pointer needs, stops at the first
//! thing it cannot read exactly, allocates nothing and is a fuzz target
//! (`fuzz/fuzz_targets/discovery_header.rs` and `discovery_element.rs`).
//! A reader passes each pointer it finds to the caller as the bytes the
//! page wrote; [`resolve`] then checks the target and resolves it against
//! the address that answered. Every limit is a constant here, which the
//! browser uses by name (its requirement 2.4).

use crate::bytes::{split_once, trim_ows};
use crate::consts::{MAX_URL_LENGTH, media};
use crate::url::{Url, UrlError};

mod element;
mod header;

pub use element::read_elements;
pub use header::read_headers;

/// The most bytes of a page's body read for pointers (REQ-2476; the
/// browser's REQ-693): enough for a page's head, where a pointer is put,
/// and a bound on what a hostile server can make the reader work through.
pub const DISCOVERY_BODY_BYTES: usize = 8 * 1024;
/// The most link values of the `Link` header fields examined, in all
/// (REQ-2476).
pub const MAX_HEADER_POINTERS: usize = 16;
/// The most `<link>` elements whose `rel` holds `alternate` examined
/// (REQ-2476).
pub const MAX_ELEMENT_POINTERS: usize = 16;
/// The link relation of a pointer (RFC 8288 section 6.2.2; HTML's link
/// type "alternate").
pub const ALTERNATE: &[u8] = b"alternate";
/// The character no target may hold: no character reference is decoded,
/// so an encoded address cannot hide what it is (REQ-2476).
const AMPERSAND: u8 = b'&';
/// The first and last printable ASCII characters other than the space,
/// the only bytes a target may hold.
const PRINTABLE: core::ops::RangeInclusive<u8> = b'!'..=b'~';
/// Separates a media type from its parameters (RFC 9110 section 8.3.1).
const PARAMETER_MARK: u8 = b';';

/// What a pointer says it leads to, by its `type`, compared without regard
/// to letter case and with any parameters ignored (DEC-2475).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerMedia {
    /// `application/vnd.neoteletext+json`: a channel's folder.
    Channel,
    /// `text/x-teletext-tti`: a page in TTI.
    Tti,
    /// `application/x-teletext-t42`: a page as T42 packets.
    T42,
}

impl PointerMedia {
    /// The pointer media type named by a `type` value, if it is one.
    #[must_use]
    pub fn of(value: &[u8]) -> Option<Self> {
        let (essence, _) = split_once(value, PARAMETER_MARK);
        let essence = trim_ows(essence);
        [(media::CHANNEL, Self::Channel), (media::TTI, Self::Tti), (media::T42, Self::T42)]
            .into_iter()
            .find(|(name, _)| essence.eq_ignore_ascii_case(name))
            .map(|(_, kind)| kind)
    }
}

/// Where a pointer was found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Found {
    /// A `Link` header field.
    Header,
    /// A `<link>` element; `after_base` when a `<base>` element with an
    /// `href` came before it, so that a relative target would mean
    /// something other than what the reader resolves it to.
    Element {
        /// A `<base href>` came before.
        after_base: bool,
    },
}

/// One pointer as the page wrote it: unchecked bytes of the input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pointer<'a> {
    /// The target, as written, with no character reference decoded.
    pub target: &'a [u8],
    /// What it says it leads to.
    pub media: PointerMedia,
    /// Where it was found.
    pub found: Found,
}

/// Why a pointer's target was refused (REQ-2476).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerError {
    /// Empty, longer than [`MAX_URL_LENGTH`], or holding a byte that is
    /// not printable ASCII, or `&`.
    Characters,
    /// A relative target in a `<link>` element after a `<base href>`: the
    /// publisher is to write an absolute address.
    RelativeAfterBase,
    /// The target does not resolve to an address the strict parser takes
    /// (REQ-WEB-016).
    Address(UrlError),
    /// Not `http` or `https`.
    Scheme,
    /// An `http` target given by an answer that came over `https`
    /// (REQ-SEC-007).
    Downgrade,
    /// A literal address in a blocked range (REQ-SEC-006).
    Blocked,
}

/// Checks a pointer's target and resolves it against `answered`, the final
/// address of the answer that gave it (RFC 3986 section 5).
///
/// # Errors
/// - [`PointerError::Characters`]: the target is empty, longer than
///   [`MAX_URL_LENGTH`], or holds a byte outside printable ASCII or `&`;
/// - [`PointerError::RelativeAfterBase`]: a relative target of an element
///   after a `<base href>`;
/// - [`PointerError::Address`]: the resolved address does not parse;
/// - [`PointerError::Scheme`]: it is not `http` or `https`;
/// - [`PointerError::Downgrade`]: `answered` is `https` and the target is
///   not;
/// - [`PointerError::Blocked`]: its host is a literal address in a blocked
///   range.
pub fn resolve(pointer: &Pointer<'_>, answered: &Url) -> Result<Url, PointerError> {
    let target = pointer.target;
    let printable = target.iter().all(|byte| PRINTABLE.contains(byte) && *byte != AMPERSAND);
    if target.is_empty() || target.len() > MAX_URL_LENGTH || !printable {
        return Err(PointerError::Characters);
    }
    // Printable ASCII, so it is UTF-8.
    let text = core::str::from_utf8(target).map_err(|_| PointerError::Characters)?;
    if matches!(pointer.found, Found::Element { after_base: true }) && !has_scheme(target) {
        return Err(PointerError::RelativeAfterBase);
    }
    let url = answered.join(text).map_err(PointerError::Address)?;
    if url.scheme().is_websocket() {
        return Err(PointerError::Scheme);
    }
    if answered.is_secure() && !url.is_secure() {
        return Err(PointerError::Downgrade);
    }
    if url.ip().is_some_and(crate::url::IpAddress::is_blocked) {
        return Err(PointerError::Blocked);
    }
    Ok(url)
}

/// Whether a reference begins with a scheme (RFC 3986 section 3.1:
/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`), and so is absolute.
fn has_scheme(reference: &[u8]) -> bool {
    let Some(colon) = reference.iter().position(|&byte| byte == b':') else { return false };
    let scheme = reference.get(..colon).unwrap_or(&[]);
    scheme.first().is_some_and(u8::is_ascii_alphabetic)
        && scheme.iter().all(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Whether a relation list (`rel`) holds `alternate`, the list split at
/// white space and compared without regard to letter case (RFC 8288
/// section 3.3; HTML's set of space-separated tokens).
fn holds_alternate(relations: &[u8]) -> bool {
    relations.split(|byte| is_space(*byte)).any(|relation| relation.eq_ignore_ascii_case(ALTERNATE))
}

/// HTML's ASCII white space (tab, line feed, form feed, carriage return and
/// space), which also covers RFC 9110's optional white space.
const fn is_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

#[cfg(test)]
mod tests;
