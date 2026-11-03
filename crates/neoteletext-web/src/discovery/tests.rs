#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "a test: a fault here fails the test"
)]
//! The tests of discovery's link-pointer reader (REQ-2476): hostile markup
//! and headers, the bounds, and the checks of a target.

use super::*;
use core::fmt::Write as _;
use std::format;
use std::string::String;
use std::vec::Vec;

const CHANNEL_TYPE: &str = "application/vnd.neoteletext+json";

fn elements(body: &str) -> Vec<(String, PointerMedia, Found)> {
    elements_of(body.as_bytes())
}

fn elements_of(body: &[u8]) -> Vec<(String, PointerMedia, Found)> {
    let mut found = Vec::new();
    read_elements(body, &mut |pointer| {
        found.push((String::from_utf8_lossy(pointer.target).into_owned(), pointer.media, pointer.found));
    });
    found
}

fn targets(body: &str) -> Vec<String> {
    elements(body).into_iter().map(|(target, _, _)| target).collect()
}

fn headers(fields: &[&str]) -> Vec<(String, PointerMedia)> {
    let mut head = String::from("HTTP/1.1 200 OK\r\n");
    for field in fields {
        head.push_str(field);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let mut found = Vec::new();
    read_headers(head.as_bytes(), &mut |pointer| {
        assert_eq!(pointer.found, Found::Header);
        found.push((String::from_utf8_lossy(pointer.target).into_owned(), pointer.media));
    });
    found
}

fn link(href: &str) -> String {
    format!(r#"<link rel="alternate" type="{CHANNEL_TYPE}" href="{href}">"#)
}

// REQ-2476: a pointer in the head is found, with its media type, in each
// letter case and with parameters.
#[test]
fn a_link_element_in_the_head_is_a_pointer() {
    let body = format!(
        "<!DOCTYPE html><html><head><meta charset=utf-8>{}<LINK REL='Next Alternate' TYPE='Text/X-Teletext-TTI; charset=x' HREF=' /100.tti '><link rel=alternate type=application/x-teletext-t42 href=b/></head>",
        link("https://example.org/teletext/")
    );
    assert_eq!(
        elements(&body),
        [
            ("https://example.org/teletext/".into(), PointerMedia::Channel, Found::Element { after_base: false }),
            ("/100.tti".into(), PointerMedia::Tti, Found::Element { after_base: false }),
            ("b/".into(), PointerMedia::T42, Found::Element { after_base: false }),
        ]
    );
}

// REQ-2476: an element that is not a pointer is not taken.
#[test]
fn other_links_are_not_pointers() {
    let body = format!(
        r#"<link rel="stylesheet" type="{CHANNEL_TYPE}" href="a"><link rel="alternate" type="text/html" href="b"><link rel="alternate" href="c"><link rel="alternate" type="{CHANNEL_TYPE}"><linkx rel="alternate" type="{CHANNEL_TYPE}" href="d">"#
    );
    assert_eq!(targets(&body), Vec::<String>::new());
}

// REQ-2476: comments, declarations, processing instructions and the
// content of raw text elements hide no pointer; reading goes on after them.
#[test]
fn hidden_pointers_are_not_taken() {
    let mut body = String::from("<!-- ");
    body.push_str(&link("comment"));
    body.push_str(" --><!-->");
    body.push_str(&link("after-empty-comment"));
    body.push_str("<!-- a --!>");
    for element in ["script", "style", "noscript", "template", "textarea", "title", "xmp"] {
        let _ = write!(body, "<{element} type=x>{}</{element}x></{}>", link(element), element.to_uppercase());
    }
    body.push_str("<?php ");
    body.push_str(&link("instruction"));
    body.push_str(" ?>");
    body.push_str(&link("found"));
    assert_eq!(targets(&body), ["after-empty-comment", "found"]);
}

// REQ-2476: reading stops at the body and at the end of the head, so that
// text injected into a page's body is never read.
#[test]
fn reading_stops_at_the_body_and_the_end_of_the_head() {
    assert_eq!(targets(&format!("{}<body>{}", link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!("{}<BODY\nclass=x>{}", link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!("{}</head >{}", link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!("<bodyx>{}", link("a"))), ["a"]);
}

// REQ-2476: a `>` inside a quoted value does not end the tag, and of two
// attributes of the same name the first is used.
#[test]
fn quoting_and_duplicate_attributes_are_read_as_html_reads_them() {
    let body = format!(
        r#"<meta content="<link rel=alternate type={CHANNEL_TYPE} href=fake>"><link href="first" rel="alternate" href="second" type="{CHANNEL_TYPE}" type="text/html">"#
    );
    assert_eq!(targets(&body), ["first"]);
    // A `>` in a quoted value of an end tag does not end it either.
    let body = format!(r#"</div title="{}">{}"#, link("hidden"), link("shown"));
    assert_eq!(targets(&body), ["shown"]);
}

// REQ-2476: what cannot be read to its end stops the reading, and nothing
// after it is taken.
#[test]
fn unterminated_constructs_stop_the_reading() {
    assert_eq!(targets(&format!("{}<!-- {}", link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!(r#"{}<meta content="{}"#, link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!("{}<script>{}", link("a"), link("b"))), ["a"]);
    assert_eq!(targets(&format!("{}<!DOCTYPE {}", link("a"), link("b")).replace('>', "")), Vec::<String>::new());
}

// REQ-2476: a pointer past byte 8,192 is not seen, nor one split by the
// limit; a zero byte stops the reading, and a page in UTF-16 is not read.
#[test]
fn the_limit_zero_bytes_and_utf16_bound_the_reading() {
    let first = link("a");
    let padding = DISCOVERY_BODY_BYTES - first.len() - 10;
    let body = format!("{first}{}{}", " ".repeat(padding), link("split"));
    assert_eq!(targets(&body), ["a"]);
    let body = format!("{first}{}{}", "x".repeat(4 * 1024 * 1024), link("far"));
    assert_eq!(targets(&body), ["a"]);
    let mut body = first.clone().into_bytes();
    body.push(0);
    body.extend_from_slice(link("after-zero").as_bytes());
    assert_eq!(elements_of(&body).len(), 1);
    let mut utf16 = Vec::from(UTF16_MARK);
    for byte in first.bytes() {
        utf16.extend_from_slice(&[byte, 0]);
    }
    assert_eq!(elements_of(&utf16), []);
}

/// The little-endian byte-order mark of UTF-16.
const UTF16_MARK: [u8; 2] = [0xFF, 0xFE];

// REQ-2476: at most 16 elements whose `rel` holds `alternate` are examined.
#[test]
fn at_most_sixteen_elements_are_examined() {
    let body: String = (0..MAX_ELEMENT_POINTERS + 4).map(|index| link(&format!("t{index}"))).collect();
    let expected: Vec<String> = (0..MAX_ELEMENT_POINTERS).map(|index| format!("t{index}")).collect();
    assert_eq!(targets(&body), expected);
}

// REQ-2476: a `<base href>` marks the pointers after it.
#[test]
fn pointers_after_a_base_are_marked() {
    let body = format!("{}<base href='https://elsewhere.example/'>{}", link("a"), link("b"));
    let found: Vec<Found> = elements(&body).into_iter().map(|(_, _, found)| found).collect();
    assert_eq!(found, [Found::Element { after_base: false }, Found::Element { after_base: true }]);
}

// REQ-2476: Link header fields by RFC 8288's grammar.
#[test]
fn link_header_fields_are_read() {
    let found = headers(&[
        "Content-Type: text/html",
        r#"Link: <https://example.org/teletext/>; rel="alternate"; type="application/vnd.neoteletext+json", <style.css>; rel=stylesheet"#,
        r#"link: <a/> ; REL = "next ALTERNATE" ; type = text/x-teletext-tti ; title="a, b; c""#,
        r#"Link: <b/>; rel=alternate; type="application/x-teletext-t42"; anchor="x""#,
        r#"Link: <c/>; rel=alternate; type="text/html""#,
        "Link: <d/>; type=text/x-teletext-tti; rel=alternate; rel=next; type=text/html",
    ]);
    assert_eq!(
        found,
        [
            ("https://example.org/teletext/".into(), PointerMedia::Channel),
            ("a/".into(), PointerMedia::Tti),
            ("d/".into(), PointerMedia::Tti),
        ]
    );
}

// REQ-2476: a field that cannot be split into link values is ignored
// whole, even its well-formed first value.
#[test]
fn a_malformed_link_field_is_ignored_whole() {
    let good = "<a/>; rel=alternate; type=text/x-teletext-tti";
    for bad in [
        format!("Link: {good}, <b/"),
        format!(r#"Link: {good}, <b/>; rel="alternate"#),
        format!("Link: {good}, b/; rel=alternate"),
        format!("Link: {good}; =x"),
        format!("Link: {good} junk"),
    ] {
        assert_eq!(
            headers(&[&bad, &format!("Link: <c/>; rel=alternate; type={CHANNEL_TYPE}")]),
            [("c/".into(), PointerMedia::Channel)],
            "{bad}"
        );
    }
}

// REQ-2476: at most 16 link values are examined, across every field.
#[test]
fn at_most_sixteen_link_values_are_examined() {
    let value = |index: usize| format!("<t{index}/>; rel=alternate; type=text/x-teletext-tti");
    let first: Vec<String> = (0..10).map(value).collect();
    let second: Vec<String> = (10..MAX_HEADER_POINTERS + 5).map(value).collect();
    let found = headers(&[&format!("Link: {}", first.join(", ")), &format!("Link: {}", second.join(","))]);
    let expected: Vec<(String, PointerMedia)> =
        (0..MAX_HEADER_POINTERS).map(|index| (format!("t{index}/"), PointerMedia::Tti)).collect();
    assert_eq!(found, expected);
}

fn resolved(target: &str, found: Found, answered: &str) -> Result<String, PointerError> {
    let pointer = Pointer { target: target.as_bytes(), media: PointerMedia::Channel, found };
    resolve(&pointer, &Url::parse(answered).unwrap()).map(|url| String::from(url.as_str()))
}

// REQ-2476: each target is checked and resolved against the final address.
#[test]
fn targets_are_checked_and_resolved() {
    let header = Found::Header;
    let element = Found::Element { after_base: false };
    let after_base = Found::Element { after_base: true };
    let page = "https://example.org/news/index.html";
    assert_eq!(resolved("/teletext/", header, page), Ok("https://example.org/teletext/".into()));
    assert_eq!(resolved("teletext/", element, page), Ok("https://example.org/news/teletext/".into()));
    assert_eq!(resolved("https://other.example/tv/", after_base, page), Ok("https://other.example/tv/".into()));
    assert_eq!(resolved("tv/", after_base, page), Err(PointerError::RelativeAfterBase));
    assert_eq!(resolved("/a?x=1&amp;y=2", element, page), Err(PointerError::Characters));
    assert_eq!(resolved("/a&b", header, page), Err(PointerError::Characters));
    assert_eq!(resolved("/a b/", header, page), Err(PointerError::Characters));
    assert_eq!(resolved("", header, page), Err(PointerError::Characters));
    assert_eq!(resolved("/caf\u{e9}/", header, page), Err(PointerError::Characters));
    let long = format!("/{}/", "a".repeat(MAX_URL_LENGTH));
    assert_eq!(resolved(&long, header, page), Err(PointerError::Characters));
    assert_eq!(resolved("http://example.org/teletext/", header, page), Err(PointerError::Downgrade));
    assert_eq!(
        resolved("http://example.org/teletext/", header, "http://example.org/"),
        Ok("http://example.org/teletext/".into())
    );
    assert_eq!(resolved("wss://example.org/teletext/", header, page), Err(PointerError::Scheme));
    assert!(matches!(resolved("javascript:alert(1)", header, page), Err(PointerError::Address(_))));
    assert!(matches!(resolved("file:///etc/", header, page), Err(PointerError::Address(_))));
    assert_eq!(resolved("https://127.0.0.1/tv/", header, page), Err(PointerError::Blocked));
    assert_eq!(resolved("https://192.168.1.1/tv/", header, page), Err(PointerError::Blocked));
}
