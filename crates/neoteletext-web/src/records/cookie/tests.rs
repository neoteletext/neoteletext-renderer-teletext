#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
//! The `SET_COOKIE` record (REQ-WEB-006, DOC-1957): what
//! [`SetCookie::line`] writes the cookie jar's parser reads back.

use neoteletext_core::packet::Packet;

use super::{MAX_LINE, SetCookie, read};
use crate::cookie::SameSite;
use crate::cookie::SetCookie as Parsed;
use crate::records::{ChannelRecord, Read, RecordReader};
use std::vec::Vec;

const BASE: SetCookie<'static> = SetCookie {
    name: "session",
    value: "abc123",
    path: None,
    max_age: None,
    same_site: SameSite::Default,
    secure: false,
    http_only: false,
};

/// The line a record carries, read back through the packets and the reader.
fn through_record(cookie: &SetCookie<'_>) -> Option<Vec<u8>> {
    let mut packets: Vec<Packet> = Vec::new();
    cookie.encode(5, 0, &mut |packet| packets.push(packet)).ok()?;
    let mut reader = RecordReader::new();
    for packet in &packets {
        if let Read::Record(ChannelRecord::SetCookie(line)) = reader.push(packet) {
            return Some(line.to_vec());
        }
    }
    None
}

/// REQ-WEB-006, DOC-1957: every attribute `line` writes, through a record,
/// is read back by the cookie jar's own parser.
#[test]
fn every_attribute_written_is_parsed_back() {
    for same_site in [SameSite::Strict, SameSite::Lax, SameSite::None, SameSite::Default] {
        let cookie = SetCookie {
            path: Some("/channel/"),
            max_age: Some(3600),
            same_site,
            secure: true,
            http_only: true,
            ..BASE
        };
        let line = through_record(&cookie).unwrap();
        let parsed = Parsed::parse(&line, true).unwrap();
        assert_eq!((parsed.name, parsed.value), (b"session".as_slice(), b"abc123".as_slice()));
        assert_eq!(parsed.path, Some(b"/channel/".as_slice()));
        assert_eq!(parsed.max_age, Some(3600));
        assert_eq!(parsed.same_site, same_site);
        assert!(parsed.secure && parsed.http_only && !parsed.domain);
    }
    let line = through_record(&BASE).unwrap();
    let bare = Parsed::parse(&line, true).unwrap();
    assert_eq!((bare.path, bare.max_age, bare.secure, bare.http_only), (None, None, false, false));
}

/// REQ-WEB-006, DOC-1957: a name that is not a token, a value with a space,
/// a path without `/` or with `;`, and a line over 1,016 bytes are never
/// written; `read` refuses an empty line and a byte outside printable
/// ASCII.
#[test]
fn a_cookie_past_its_rules_is_refused() {
    let mut out = [0_u8; MAX_LINE];
    let long: std::string::String = core::iter::repeat_n('v', MAX_LINE).collect();
    for (what, cookie) in [
        ("a name that is not a token", SetCookie { name: "a b", ..BASE }),
        ("an empty name", SetCookie { name: "", ..BASE }),
        ("a value with a space", SetCookie { value: "a b", ..BASE }),
        ("a path without /", SetCookie { path: Some("channel"), ..BASE }),
        ("a path with ;", SetCookie { path: Some("/a;b"), ..BASE }),
        ("a line over the frame", SetCookie { value: &long, ..BASE }),
    ] {
        assert_eq!(cookie.line(&mut out), None, "{what}");
        assert!(through_record(&cookie).is_none(), "{what}");
    }
    assert_eq!(MAX_LINE, 1016);
    assert!(read(b"").is_err());
    assert!(read(b"a=b\x7f").is_err());
    assert!(read(b"a=\xc3\xa9").is_err());
    assert!(read(b"a=b").is_ok());
}
