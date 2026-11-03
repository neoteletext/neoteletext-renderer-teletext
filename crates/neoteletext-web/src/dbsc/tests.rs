#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]
//! The tests of Device Bound Session Credentials' records: the headers,
//! the instructions, the proofs and the session store.

use super::*;
use crate::bytes::{TextBuf, Writer};
use crate::url::Url;
use std::string::String;
use std::vec::Vec;

#[test]
fn registration_offers_are_read() {
    let read = read_registration(br#"(ES256 RS256);path="/dbsc/start";challenge="c-1";authorization="code""#).unwrap();
    assert_eq!(read.path.as_str(), "/dbsc/start");
    assert_eq!(read.challenge.unwrap().as_str(), "c-1");
    assert_eq!(read.authorization.unwrap().as_str(), "code");
    // The first member offering ES256 is taken; others are read past.
    let read = read_registration(br#"(RS256);path="/rsa", (ES256);path="/ec";x=?1;n=5"#).unwrap();
    assert_eq!(read.path.as_str(), "/ec");
    assert_eq!(read.challenge, None);
    let escaped = read_registration(br#"(ES256);path="/a";challenge="q\"b\\c""#).unwrap();
    assert_eq!(escaped.challenge.unwrap().as_str(), r#"q"b\c"#);
    for refused in [
        &b"(RS256);path=\"/rsa\""[..],
        b"(ES256)",
        b"(ES256);path=/bare",
        b"ES256;path=\"/x\"",
        b"(ES256);path=\"/x\",",
        b"(ES256);path=\"unterminated",
        b"(ES256);path=\"tab\there\"",
        b"(ES256);challenge=\"\\x\";path=\"/x\"",
    ] {
        assert_eq!(read_registration(refused), None, "{}", String::from_utf8_lossy(refused));
    }
    let long = [b"(ES256);path=\"".as_slice(), &[b'a'; MAX_SESSION_URL + 1], b"\""].concat();
    assert_eq!(read_registration(&long), None, "a path longer than kept");
}

#[test]
fn challenges_name_their_session() {
    let read = read_challenge(br#""fresh";id="s1""#).unwrap();
    assert_eq!((read.challenge.as_str(), read.session.as_str()), ("fresh", "s1"));
    assert_eq!(read_challenge(br#""fresh""#), None, "no id");
    assert_eq!(read_challenge(br#""fresh";id=s1"#), None, "a token id");
    assert_eq!(read_challenge(br#""a", "b";id="s""#), None, "a list");
}

const INSTRUCTIONS: &[u8] = br#"{
        "session_identifier": "s1",
        "refresh_url": "/dbsc/refresh",
        "scope": {"origin": "https://news.example", "include_site": false,
                  "scope_specification": [{"type": "exclude", "domain": "news.example", "path": "/public"}]},
        "credentials": [{"type": "cookie", "name": "auth", "attributes": "Domain=news.example; Secure"}],
        "allowed_refresh_initiators": ["*.news.example"],
        "newer_member": {"ignored": [1, 2, 3]}
    }"#;

#[test]
fn instructions_are_read() {
    let Ok(Answer::Continue(read)) = read_instructions(INSTRUCTIONS) else { panic!() };
    assert_eq!(read.id.as_str(), "s1");
    assert_eq!(read.refresh_url.unwrap().as_str(), "/dbsc/refresh");
    assert_eq!(read.origin.unwrap().as_str(), "https://news.example");
    assert!(!read.include_site);
    let rule = read.rules[0].unwrap();
    assert_eq!((rule.include, rule.domain.as_str(), rule.path.as_str()), (false, "news.example", "/public"));
    assert_eq!(read.credentials[0].unwrap().as_str(), "auth");
    assert_eq!(read_instructions(br#"{"continue": false}"#), Ok(Answer::End));
}

#[test]
fn instructions_outside_the_profile_are_refused() {
    let base =
        |members: &str| std::format!(r#"{{"session_identifier":"s","scope":{{"include_site":true}},{members}}}"#);
    for (body, why) in [
        (String::from("{"), InstructionsError::Malformed),
        (String::from(r#"{"continue":true} trailing"#), InstructionsError::Malformed),
        (base(r#""credentials":[]"#), InstructionsError::Missing),
        (base(r#""credentials":[{"type":"token","name":"a"}]"#), InstructionsError::Credential),
        (base(r#""credentials":[{"type":"cookie","name":""}]"#), InstructionsError::Credential),
        (
            base(r#""credentials":[{"type":"cookie","name":"a","attributes":"Secure; Partitioned"}]"#),
            InstructionsError::Credential,
        ),
        (
            r#"{"scope":{"include_site":true},"credentials":[{"type":"cookie","name":"a"}]}"#.into(),
            InstructionsError::Missing,
        ),
        (
            r#"{"session_identifier":"s","scope":{},"credentials":[{"type":"cookie","name":"a"}]}"#.into(),
            InstructionsError::Missing,
        ),
        (
            base(&std::format!(
                r#""credentials":[{}]"#,
                [r#"{"type":"cookie","name":"a"}"#; MAX_CREDENTIALS + 1].join(",")
            )),
            InstructionsError::TooLarge,
        ),
    ] {
        assert_eq!(read_instructions(body.as_bytes()), Err(why), "{body}");
    }
    let long = std::format!(r#"{{"session_identifier":"{}"}}"#, "a".repeat(MAX_SESSION_ID + 1));
    assert_eq!(read_instructions(long.as_bytes()), Err(InstructionsError::TooLarge));
}

/// Finding F-14 (decision E.114 item 6): a session identifier is written
/// into `Sec-Secure-Session-Id`, so one holding CR, LF, NUL or any byte
/// outside `0x21..=0x7E`, or none at all, is refused.
#[test]
fn a_session_identifier_outside_visible_ascii_is_refused() {
    let with_id = |id: &str| {
        std::format!(
            r#"{{"session_identifier":"{id}","scope":{{"include_site":false}},"credentials":[{{"type":"cookie","name":"a"}}]}}"#
        )
    };
    assert!(matches!(read_instructions(with_id("a-1").as_bytes()), Ok(Answer::Continue(_))));
    for id in [r"a\r\nX: y", r"a\u0000", "", "a b", r"\u007f", "\u{e9}"] {
        assert_eq!(read_instructions(with_id(id).as_bytes()), Err(InstructionsError::Malformed), "{id:?}");
    }
}

/// A key store that signs with a stand-in signature and records what
/// it signed.
#[derive(Debug)]
struct Keys {
    signed: Vec<Vec<u8>>,
}

impl DeviceKeys for Keys {
    fn create(&mut self) -> Option<(KeyHandle, PublicKey)> {
        Some((KeyHandle(7), PublicKey { x: [1; COORDINATE_BYTES], y: [2; COORDINATE_BYTES] }))
    }
    fn sign(&mut self, key: KeyHandle, message: &[u8]) -> Option<[u8; SIGNATURE_BYTES]> {
        assert_eq!(key, KeyHandle(7));
        self.signed.push(message.to_vec());
        Some([0xAB; SIGNATURE_BYTES])
    }
    fn delete(&mut self, _: KeyHandle) {}
}

fn unbase64url(text: &[u8]) -> Vec<u8> {
    let mut standard: Vec<u8> = text
        .iter()
        .map(|&byte| match byte {
            b'-' => b'+',
            b'_' => b'/',
            other => other,
        })
        .collect();
    while !standard.len().is_multiple_of(4) {
        standard.push(b'=');
    }
    let mut out = std::vec![0_u8; standard.len()];
    let length = crate::base64::decode(&standard, &mut out).unwrap();
    out.truncate(length);
    out
}

#[test]
fn proofs_are_compact_jws_signed_by_the_port() {
    let mut keys = Keys { signed: Vec::new() };
    let (key, public) = keys.create().unwrap();
    let mut buffer = [0_u8; 2048];
    let mut out = Writer::new(&mut buffer);
    out.put(b"Secure-Session-Response: ").unwrap();
    let start = out.len();
    write_proof(&mut keys, key, Some(&public), Some(br#"c"1"#), Some(b"code"), &mut out).unwrap();
    let jws = out.written()[start..].to_vec();
    let parts: Vec<&[u8]> = jws.split(|&byte| byte == b'.').collect();
    assert_eq!(parts.len(), 3);
    let header = String::from_utf8(unbase64url(parts[0])).unwrap();
    let x = String::from_utf8(parts[0].to_vec()).unwrap();
    assert!(!x.contains('=') && !x.contains('+') && !x.contains('/'), "base64url without padding");
    assert_eq!(
        header,
        std::format!(
            r#"{{"alg":"ES256","typ":"dbsc+jwt","jwk":{{"kty":"EC","crv":"P-256","x":"{x}","y":"{y}"}}}}"#,
            x = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE",
            y = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI"
        )
    );
    assert_eq!(String::from_utf8(unbase64url(parts[1])).unwrap(), r#"{"jti":"c\"1","authorization":"code"}"#);
    assert_eq!(unbase64url(parts[2]), [0xAB; SIGNATURE_BYTES]);
    // What was signed is the JWS signing input (RFC 7515 section 5.1).
    assert_eq!(keys.signed, [[parts[0], b".", parts[1]].concat()]);

    let mut buffer = [0_u8; 512];
    let mut out = Writer::new(&mut buffer);
    write_proof(&mut keys, key, None, Some(b"r"), None, &mut out).unwrap();
    let parts: Vec<Vec<u8>> = out.written().split(|&byte| byte == b'.').map(<[u8]>::to_vec).collect();
    assert_eq!(String::from_utf8(unbase64url(&parts[0])).unwrap(), r#"{"alg":"ES256","typ":"dbsc+jwt"}"#);
    assert_eq!(String::from_utf8(unbase64url(&parts[1])).unwrap(), r#"{"jti":"r"}"#);
}

fn session(include_site: bool, rules: &[(bool, &str, &str)]) -> Session {
    let mut kept = [None; MAX_SCOPE_RULES];
    for (slot, &(include, domain, path)) in kept.iter_mut().zip(rules) {
        *slot = Some(ScopeRule {
            include,
            domain: TextBuf::from_bytes(domain.as_bytes()).unwrap(),
            path: TextBuf::from_bytes(path.as_bytes()).unwrap(),
        });
    }
    let mut credentials = [None; MAX_CREDENTIALS];
    credentials[0] = TextBuf::from_bytes(b"auth");
    Session {
        id: TextBuf::from_bytes(b"s1").unwrap(),
        key: KeyHandle(7),
        refresh_url: TextBuf::from_bytes(b"https://news.example/dbsc/refresh").unwrap(),
        origin: Url::parse("https://news.example/").unwrap().origin(),
        include_site,
        rules: kept,
        credentials,
        challenge: None,
        expires: 100,
    }
}

#[test]
fn scope_is_the_origin_or_the_site_less_what_rules_exclude() {
    let covers = |session: &Session, url: &str| session.covers(&Url::parse(url).unwrap());
    let origin = session(false, &[]);
    assert!(covers(&origin, "https://news.example/p/100.tti"));
    assert!(!covers(&origin, "https://www.news.example/p/100.tti"), "another origin");
    assert!(!covers(&origin, "http://news.example/p/100.tti"), "plain HTTP");
    assert!(!covers(&origin, "https://news.example/dbsc/refresh"), "the refresh address itself");
    let site = session(true, &[(false, "*", "/public"), (true, "*", "/public/mine")]);
    assert!(covers(&site, "https://www.news.example/p/100.tti"));
    assert!(!covers(&site, "https://othernews.example/p"), "not below the site");
    assert!(covers(&site, "https://news.example/public/mine/1"), "the last matching rule decides");
    assert!(!covers(&site, "https://news.example/public/other"));
    let excluded = session(true, &[(false, "*.news.example", "/")]);
    assert!(!covers(&excluded, "https://cdn.news.example/a") && covers(&excluded, "https://news.example/a"));
}

#[test]
fn a_missing_cookie_is_found_by_name() {
    let session = session(false, &[]);
    assert!(!session.missing(b"theme=dark; auth=xyz"));
    assert!(session.missing(b"theme=dark"));
    assert!(session.missing(b"author=1; xauth=2"));
    assert!(session.missing(b""));
}

#[test]
fn the_store_is_bounded_and_replaces_by_identifier() {
    let mut sessions = Sessions::new();
    let mut first = session(false, &[]);
    assert_eq!(sessions.insert(first), Ok(None));
    first.key = KeyHandle(8);
    assert_eq!(sessions.insert(first), Ok(Some(KeyHandle(7))), "the replaced key is the caller's to delete");
    for index in 1..MAX_SESSIONS {
        let mut other = session(false, &[]);
        other.id = TextBuf::from_bytes(std::format!("s{index}x").as_bytes()).unwrap();
        assert_eq!(sessions.insert(other), Ok(None));
    }
    let mut more = session(false, &[]);
    more.id = TextBuf::from_bytes(b"over").unwrap();
    more.key = KeyHandle(9);
    assert_eq!(sessions.insert(more), Err(KeyHandle(9)));
    let mut ended = Vec::new();
    sessions.expire(100, |session| ended.push(session.key));
    assert_eq!(ended.len(), MAX_SESSIONS);
    assert!(sessions.iter().next().is_none());
}

/// Finding F-92 (decision E.114 item 6): instructions naming a member
/// twice, at any level, are refused; the later one overwrote the earlier.
#[test]
fn instructions_naming_a_member_twice_are_refused() {
    let credentials = r#""credentials":[{"type":"cookie","name":"a"}]"#;
    let scope = r#""scope":{"include_site":false}"#;
    for body in [
        std::format!(r#"{{"session_identifier":"s","session_identifier":"t",{scope},{credentials}}}"#),
        std::format!(r#"{{"session_identifier":"s",{scope},{scope},{credentials}}}"#),
        std::format!(r#"{{"session_identifier":"s","continue":true,"continue":false,{scope},{credentials}}}"#),
        std::format!(
            r#"{{"session_identifier":"s","scope":{{"include_site":false,"include_site":true}},{credentials}}}"#
        ),
        std::format!(
            r#"{{"session_identifier":"s","scope":{{"include_site":false,"scope_specification":[{{"type":"include","path":"/a","path":"/"}}]}},{credentials}}}"#
        ),
        std::format!(
            r#"{{"session_identifier":"s",{scope},"credentials":[{{"type":"cookie","name":"a","name":"b"}}]}}"#
        ),
    ] {
        assert_eq!(read_instructions(body.as_bytes()), Err(InstructionsError::Malformed), "{body}");
    }
}

mod persistence {
    use super::super::{ImportError, KeyHandle, MAX_EXPORT_BYTES, ScopeRule, Session, Sessions};
    use crate::bytes::{TextBuf, Writer};
    use crate::url::Url;

    const NOW: u64 = 1_000;

    fn session(id: &str, key: u32, expires: u64) -> Session {
        let url = Url::parse("https://news.example/dbsc/refresh").unwrap();
        let mut session = Session {
            id: TextBuf::from_bytes(id.as_bytes()).unwrap(),
            key: KeyHandle(key),
            refresh_url: TextBuf::from_bytes(url.as_str().as_bytes()).unwrap(),
            origin: url.origin(),
            include_site: false,
            rules: [None; super::super::MAX_SCOPE_RULES],
            credentials: [None; super::super::MAX_CREDENTIALS],
            challenge: Some(TextBuf::from_bytes(b"c 2").unwrap()),
            expires,
        };
        session.credentials[0] = TextBuf::from_bytes(b"auth");
        session.credentials[1] = TextBuf::from_bytes(b"id");
        session.rules[0] = Some(ScopeRule {
            include: false,
            domain: TextBuf::from_bytes(b"*").unwrap(),
            path: TextBuf::from_bytes(b"/static").unwrap(),
        });
        session
    }

    fn export(sessions: &Sessions) -> std::vec::Vec<u8> {
        let mut out = std::vec![0_u8; MAX_EXPORT_BYTES];
        let mut writer = Writer::new(&mut out);
        sessions.export(NOW, &mut writer).unwrap();
        let length = writer.len();
        out.truncate(length);
        out
    }

    // REQ-WEB-017 (TODO-1207): sessions written and read again are the same,
    // key handle, scope rules, credentials and challenge included; one lapsed
    // by the time of reading is not kept.
    #[test]
    fn sessions_round_trip_and_a_lapsed_one_is_dropped() {
        let mut sessions = Sessions::new();
        sessions.insert(session("s1", 7, NOW + 500)).unwrap();
        sessions.insert(session("s2", 9, NOW + 10)).unwrap();
        let saved = export(&sessions);
        let mut read = Sessions::new();
        assert_eq!(read.import(&saved, NOW), Ok(2));
        assert_eq!(read, sessions);
        assert_eq!(read.import(&saved, NOW + 10), Ok(1), "s2 has lapsed");
        assert_eq!(read.iter().map(|(_, session)| session.id.as_str()).collect::<std::vec::Vec<_>>(), ["s1"]);
    }

    // REQ-WEB-017, REQ-SEC-013: saved data is untrusted: data cut short,
    // longer than the bound, of another version, with a length past its
    // bound, or with a session no registration could make is refused whole,
    // and the sessions held stay as they were.
    #[test]
    fn saved_sessions_that_break_the_format_are_refused_whole() {
        let mut held = Sessions::new();
        held.insert(session("kept", 1, NOW + 500)).unwrap();
        let mut sessions = Sessions::new();
        sessions.insert(session("s1", 7, NOW + 500)).unwrap();
        let saved = export(&sessions);
        let text = std::string::String::from_utf8(saved.clone()).unwrap();
        let cases: [(&str, std::vec::Vec<u8>, ImportError); 6] = [
            ("cut short", saved[..saved.len() - 3].to_vec(), ImportError::Malformed),
            ("too large", std::vec![b'0'; MAX_EXPORT_BYTES + 1], ImportError::TooLarge),
            ("another version", text.replacen("sessions 1", "sessions 9", 1).into_bytes(), ImportError::Version),
            (
                "an identifier past its bound",
                text.replacen(" 2 1 2 33 ", " 2 1 99 33 ", 1).into_bytes(),
                ImportError::Malformed,
            ),
            ("plain HTTP", text.replacen("https://", "http://x", 1).into_bytes(), ImportError::Refused),
            ("too many credentials", text.replacen(" 1500 2 1 ", " 1500 9 1 ", 1).into_bytes(), ImportError::Refused),
        ];
        for (what, data, error) in cases {
            let mut store = held;
            assert_eq!(store.import(&data, NOW), Err(error), "{what}: {}", std::string::String::from_utf8_lossy(&data));
            assert_eq!(store, held, "{what}: nothing kept");
        }
    }
}
