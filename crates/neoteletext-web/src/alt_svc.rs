//! The `Alt-Svc` header field (RFC 7838 section 3), read for one thing:
//! whether the origin names HTTP/3 (`h3`) on a port of its own host
//! (REQ-6983). A channel's server says so on its settings file's answer,
//! and a port may then carry the push connection as a stream of an HTTP/3
//! connection (neoteletext-common's DEC-6979 item 3).
//!
//! The reader is strict: a value that is wrong anywhere names nothing. It
//! is bounded by [`MAX_ALT_SVC_BYTES`] and [`MAX_ALT_SVC_ENTRIES`], holds
//! nothing and allocates nothing.

use crate::bytes::parse_decimal;
use crate::consts::{ALT_SVC_DEFAULT_MAX_AGE_SECONDS, MAX_ALT_SVC_BYTES, MAX_ALT_SVC_ENTRIES};
use crate::http::is_token_character;

/// The value that invalidates every alternative, case-sensitive (RFC 7838
/// section 3).
const CLEAR: &[u8] = b"clear";
/// The ALPN protocol identifier of HTTP/3 (RFC 9114 section 3.1).
const HTTP3: &[u8] = b"h3";
/// The parameter that says how long an alternative is fresh (RFC 7838
/// section 3.1).
const MAX_AGE: &[u8] = b"ma";
/// The most decimal digits of a port, 65535.
const PORT_DIGITS: usize = 5;
/// The most decimal digits of a `ma` taken, those of 2^32 - 1.
const MAX_AGE_DIGITS: usize = 10;

/// What an `Alt-Svc` value named.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AltSvc {
    /// Nothing this client uses: no `h3` entry on the origin's own host,
    /// or a value that is malformed or over a bound.
    None,
    /// `clear`: every alternative of the origin is invalidated.
    Clear,
    /// HTTP/3 on the origin's own host.
    Http3 {
        /// The UDP port, 1 to 65535.
        port: u16,
        /// How long the alternative is fresh, in seconds: the entry's `ma`,
        /// or a day (RFC 7838 section 3.1).
        max_age_seconds: u32,
    },
}

/// The bytes of a value, read from the front.
struct Cursor<'value> {
    rest: &'value [u8],
}

impl<'value> Cursor<'value> {
    /// Passes over optional white space (RFC 9110 section 5.6.3).
    fn skip_ows(&mut self) {
        while let Some((b' ' | b'\t', rest)) = self.rest.split_first() {
            self.rest = rest;
        }
    }

    /// Takes `byte` if it is next.
    fn take(&mut self, byte: u8) -> bool {
        match self.rest.split_first() {
            Some((&first, rest)) if first == byte => {
                self.rest = rest;
                true
            }
            _ => false,
        }
    }

    /// A token (RFC 9110 section 5.6.2), which is not empty.
    fn token(&mut self) -> Option<&'value [u8]> {
        let length = self.rest.iter().take_while(|&&byte| is_token_character(byte)).count();
        let (token, rest) = self.rest.split_at_checked(length)?;
        self.rest = rest;
        (!token.is_empty()).then_some(token)
    }

    /// A quoted string's content (RFC 9110 section 5.6.4). One with a
    /// backslash is refused: nothing read here needs a quoted pair, and an
    /// authority or a number with one is not what it seems.
    fn quoted(&mut self) -> Option<&'value [u8]> {
        if !self.take(b'"') {
            return None;
        }
        let length = self.rest.iter().position(|&byte| byte == b'"')?;
        let (content, rest) = self.rest.split_at_checked(length)?;
        self.rest = rest.get(1..)?;
        (!content.contains(&b'\\')).then_some(content)
    }
}

/// The port of an alternative authority with no host, `:443`; `None` for
/// one that names a host. `Err` for a port that is not one.
fn own_host_port(authority: &[u8]) -> Result<Option<u16>, ()> {
    let Some((b':', digits)) = authority.split_first() else { return Ok(None) };
    let port = parse_decimal(digits, PORT_DIGITS).and_then(|port| u16::try_from(port).ok()).filter(|&port| port != 0);
    port.map(Some).ok_or(())
}

/// Reads an `Alt-Svc` field's value (REQ-6983): the first `h3` entry on
/// the origin's own host, `clear`, or nothing.
#[must_use]
pub fn read(value: &[u8]) -> AltSvc {
    if value.len() > MAX_ALT_SVC_BYTES || !value.iter().all(|&byte| matches!(byte, b' ' | b'\t' | 0x21..=0x7E)) {
        return AltSvc::None;
    }
    let mut cursor = Cursor { rest: value };
    cursor.skip_ows();
    if cursor.rest == CLEAR {
        return AltSvc::Clear;
    }
    entries(&mut cursor).unwrap_or(AltSvc::None)
}

/// Every entry of a value that is not `clear`; `None` when it is malformed
/// anywhere or has more than [`MAX_ALT_SVC_ENTRIES`].
fn entries(cursor: &mut Cursor<'_>) -> Option<AltSvc> {
    let mut found = AltSvc::None;
    let mut count = 0_usize;
    loop {
        count = count.checked_add(1).filter(|&count| count <= MAX_ALT_SVC_ENTRIES)?;
        // RFC 7838 section 3: alternative = protocol-id "=" alt-authority.
        let protocol = cursor.token()?;
        if !cursor.take(b'=') {
            return None;
        }
        let authority = cursor.quoted()?;
        let port = if protocol == HTTP3 { own_host_port(authority).ok()? } else { None };
        let mut max_age = None;
        loop {
            cursor.skip_ows();
            if !cursor.take(b';') {
                break;
            }
            cursor.skip_ows();
            // parameter = token "=" ( token / quoted-string ).
            let name = cursor.token()?;
            if !cursor.take(b'=') {
                return None;
            }
            let argument = if cursor.rest.first() == Some(&b'"') { cursor.quoted()? } else { cursor.token()? };
            if name == MAX_AGE && max_age.is_none() {
                max_age = parse_decimal(argument, MAX_AGE_DIGITS).and_then(|seconds| u32::try_from(seconds).ok());
            }
        }
        if let (AltSvc::None, Some(port)) = (found, port) {
            found = AltSvc::Http3 { port, max_age_seconds: max_age.unwrap_or(ALT_SVC_DEFAULT_MAX_AGE_SECONDS) };
        }
        if cursor.rest.is_empty() {
            return Some(found);
        }
        if !cursor.take(b',') {
            return None;
        }
        cursor.skip_ows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    const DAY: u32 = ALT_SVC_DEFAULT_MAX_AGE_SECONDS;

    fn http3(port: u16) -> AltSvc {
        AltSvc::Http3 { port, max_age_seconds: DAY }
    }

    /// REQ-6983, RFC 7838 section 3: `h3` on the origin's own host names
    /// its port, with a day's freshness when the entry gives none.
    #[test]
    fn an_h3_entry_on_the_origins_host_names_its_port() {
        assert_eq!(read(b"h3=\":443\""), http3(443));
        assert_eq!(read(b"h3=\":8443\""), http3(8443));
        assert_eq!(read(b"h3=\":1\""), http3(1));
        assert_eq!(read(b"h3=\":65535\""), http3(65535));
        assert_eq!(read(b"  h3=\":443\""), http3(443), "white space before the list");
    }

    /// RFC 7838 section 3.1: `ma` says how long; unknown parameters are
    /// ignored; a `ma` that is no number of seconds leaves the default.
    #[test]
    fn the_entrys_ma_is_its_freshness_and_other_parameters_are_passed_over() {
        assert_eq!(read(b"h3=\":443\"; ma=3600"), AltSvc::Http3 { port: 443, max_age_seconds: 3600 });
        assert_eq!(read(b"h3=\":443\";ma=\"60\";persist=1"), AltSvc::Http3 { port: 443, max_age_seconds: 60 });
        assert_eq!(read(b"h3=\":443\"; ma=4294967295"), AltSvc::Http3 { port: 443, max_age_seconds: u32::MAX });
        assert_eq!(read(b"h3=\":443\"; ma=4294967296"), http3(443), "over 2^32 - 1");
        assert_eq!(read(b"h3=\":443\"; ma=soon"), http3(443));
        assert_eq!(read(b"h3=\":443\"; v=\"46,43\""), http3(443), "a comma inside a quoted string");
    }

    /// The first such entry is taken; other protocols and other hosts are
    /// passed over, never followed.
    #[test]
    fn other_protocols_and_other_hosts_are_passed_over() {
        assert_eq!(
            read(b"h2=\":443\", h3-29=\":443\", h3=\":444\"; ma=10, h3=\":445\""),
            AltSvc::Http3 { port: 444, max_age_seconds: 10 }
        );
        assert_eq!(read(b"h3=\"other.example:443\""), AltSvc::None);
        assert_eq!(read(b"h3=\"other.example:443\", h3=\":8443\""), http3(8443));
        assert_eq!(read(b"h2=\":443\""), AltSvc::None);
        assert_eq!(read(b"H3=\":443\""), AltSvc::None, "an ALPN identifier is octets, not letters of either case");
        assert_eq!(read(b"h2=\"\""), AltSvc::None, "another protocol's authority is not read");
    }

    /// RFC 7838 section 3: `clear`, case-sensitive and alone.
    #[test]
    fn clear_is_clear_alone() {
        assert_eq!(read(b"clear"), AltSvc::Clear);
        assert_eq!(read(b" clear"), AltSvc::Clear);
        assert_eq!(read(b"Clear"), AltSvc::None);
        assert_eq!(read(b"clear, h3=\":443\""), AltSvc::None);
        assert_eq!(read(b"clear "), AltSvc::None);
    }

    /// REQ-6983: a value that is wrong anywhere names nothing.
    #[test]
    fn a_malformed_value_names_nothing() {
        for wrong in [
            &b""[..],
            b"h3",
            b"h3=",
            b"h3=:443",
            b"h3=\":443",
            b"=\":443\"",
            b"h3=\":0\"",
            b"h3=\":65536\"",
            b"h3=\":123456\"",
            b"h3=\":\"",
            b"h3=\":44a\"",
            b"h3=\":+44\"",
            b"h3=\":4\\\"43\"",
            b"h3=\":443\",",
            b"h3=\":443\",,h2=\":443\"",
            b",h3=\":443\"",
            b"h3=\":443\" h2=\":443\"",
            b"h3=\":443\";",
            b"h3=\":443\"; ma",
            b"h3=\":443\"; ma=",
            b"h3=\":443\"; =1",
            b"h3=\":443\", h2",
            b"h3=\":443\", h2=\":443",
            b"h3 =\":443\"",
            b"h3= \":443\"",
            b"h3=\":443\"\x00",
            b"h3=\":443\"\r\n",
            b"h3=\":443\"\x7F",
            b"h3=\":443\"; x=\xC3\xA9",
        ] {
            assert_eq!(read(wrong), AltSvc::None, "{:?}", String::from_utf8_lossy(wrong));
        }
    }

    /// REQ-6983: at most `MAX_ALT_SVC_ENTRIES` entries and
    /// `MAX_ALT_SVC_BYTES` bytes; one more of either names nothing.
    #[test]
    fn the_bounds_hold_at_and_past_them() {
        let list = |entries: usize| {
            let mut parts: Vec<String> = (1..entries).map(|_| String::from("h2=\":443\"")).collect();
            parts.push(String::from("h3=\":443\""));
            parts.join(", ")
        };
        assert_eq!(read(list(MAX_ALT_SVC_ENTRIES).as_bytes()), http3(443));
        assert_eq!(read(list(MAX_ALT_SVC_ENTRIES + 1).as_bytes()), AltSvc::None);

        let base = "h3=\":443\"; x=";
        let padded = |length: usize| format!("{base}{}", "a".repeat(length - base.len()));
        assert_eq!(padded(MAX_ALT_SVC_BYTES).len(), MAX_ALT_SVC_BYTES);
        assert_eq!(read(padded(MAX_ALT_SVC_BYTES).as_bytes()), http3(443));
        assert_eq!(read(padded(MAX_ALT_SVC_BYTES + 1).as_bytes()), AltSvc::None);
    }
}
