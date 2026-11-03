//! The `<link>` elements at the head of an HTML page, read for pointers
//! (REQ-2476), as bytes: not a parser of HTML, but a loop that knows
//! comments, raw text, attribute quoting and the `link` and `base`
//! elements as the HTML standard's tokenizer reads them (WHATWG HTML,
//! section 13.2.5), and stops at the first thing it cannot read exactly.

use super::{DISCOVERY_BODY_BYTES, Found, MAX_ELEMENT_POINTERS, Pointer, PointerMedia, holds_alternate, is_space};

/// What begins and ends a tag.
const TAG_OPEN: u8 = b'<';
const TAG_CLOSE: u8 = b'>';
/// Ends an element's name, or closes a tag that is empty.
const SOLIDUS: u8 = b'/';
/// Between an attribute's name and its value.
const VALUE_MARK: u8 = b'=';
/// What quotes an attribute's value.
const DOUBLE_QUOTE: u8 = b'"';
const SINGLE_QUOTE: u8 = b'\'';
/// What opens a comment, a declaration (`<!DOCTYPE`) or a processing
/// instruction, which HTML reads as a bogus comment.
const COMMENT_OPEN: &[u8] = b"<!--";
const DECLARATION_OPEN: &[u8] = b"<!";
const INSTRUCTION_OPEN: &[u8] = b"<?";
/// What ends a comment; a browser also takes `--!>` (HTML's "incorrectly
/// closed comment"). The search starts at the comment's own `--`, so that
/// `<!-->` ends at once as it does in a browser ("abrupt closing of empty
/// comment").
const COMMENT_CLOSE: &[u8] = b"-->";
const COMMENT_CLOSE_BANG: &[u8] = b"--!>";
/// The comment's `<!` before its `--`.
const COMMENT_DASHES_AT: usize = 2;
/// An end tag's `</`.
const END_TAG_OPEN: &[u8] = b"</";
/// The byte-order marks of UTF-16 (big- and little-endian): a page in it
/// is not read at all.
const UTF16_BIG: &[u8] = &[0xFE, 0xFF];
const UTF16_LITTLE: &[u8] = &[0xFF, 0xFE];
/// The zero byte, at which reading stops.
const NUL: u8 = 0;
/// The elements that end the head: reading stops at either.
const BODY: &[u8] = b"body";
const HEAD: &[u8] = b"head";
/// The elements read.
const LINK: &[u8] = b"link";
const BASE: &[u8] = b"base";
/// The elements whose content is text, not markup, up to their end tag
/// (HTML's raw text and escapable raw text elements, with `noscript` and
/// `template`, whose content a pointer must not be taken from either).
const RAW_TEXT: [&[u8]; 7] = [b"script", b"style", b"noscript", b"template", b"textarea", b"title", b"xmp"];
/// The attributes a pointer is judged by.
const REL: &[u8] = b"rel";
const TYPE: &[u8] = b"type";
const HREF: &[u8] = b"href";

/// The attributes of one start tag that matter, each the first of its
/// name, as HTML takes the first of two of the same name.
#[derive(Clone, Copy, Default)]
struct Attributes<'a> {
    rel: Option<&'a [u8]>,
    media: Option<&'a [u8]>,
    href: Option<&'a [u8]>,
}

/// The reader could not read on exactly: it stops there.
struct Stop;

/// Calls `each` with every pointer of the `<link>` elements in the first
/// [`DISCOVERY_BODY_BYTES`] of an HTML body, in order: an element whose
/// `rel` holds `alternate`, whose `type` is a [`PointerMedia`] and which
/// has an `href`, its leading and trailing white space removed. Reading
/// stops at `<body`, at `</head`, at a zero byte, at the limit, at the
/// first construct it cannot read to its end, and after
/// [`MAX_ELEMENT_POINTERS`] elements whose `rel` holds `alternate`; a page
/// in UTF-16 is not read. No character reference is decoded.
pub fn read_elements(body: &[u8], each: &mut dyn FnMut(Pointer<'_>)) {
    let mut input = body.get(..DISCOVERY_BODY_BYTES.min(body.len())).unwrap_or(&[]);
    if input.starts_with(UTF16_BIG) || input.starts_with(UTF16_LITTLE) {
        return;
    }
    if let Some(zero) = input.iter().position(|&byte| byte == NUL) {
        input = input.get(..zero).unwrap_or(&[]);
    }
    let _ = scan(input, each);
}

fn scan(input: &[u8], each: &mut dyn FnMut(Pointer<'_>)) -> Result<(), Stop> {
    let mut at = 0_usize;
    let mut examined = 0_usize;
    let mut after_base = false;
    while let Some(offset) = input.get(at..).and_then(|rest| rest.iter().position(|&byte| byte == TAG_OPEN)) {
        at = at.saturating_add(offset);
        let rest = input.get(at..).unwrap_or(&[]);
        if rest.starts_with(COMMENT_OPEN) {
            let dashes = rest.get(COMMENT_DASHES_AT..).unwrap_or(&[]);
            at = at.saturating_add(COMMENT_DASHES_AT).saturating_add(comment_end(dashes).ok_or(Stop)?);
        } else if rest.starts_with(DECLARATION_OPEN) || rest.starts_with(INSTRUCTION_OPEN) {
            at = at.saturating_add(past(rest, TAG_CLOSE).ok_or(Stop)?);
        } else if rest.starts_with(END_TAG_OPEN) && rest.get(END_TAG_OPEN.len()).is_some_and(u8::is_ascii_alphabetic) {
            let (name, after_name) = tag_name(rest.get(END_TAG_OPEN.len()..).unwrap_or(&[]));
            if name.eq_ignore_ascii_case(HEAD) {
                return Ok(());
            }
            // An end tag's attributes mean nothing, but are read so that a
            // `>` in a quoted value does not end it early, as in a browser.
            let (_, after) = attributes(after_name)?;
            at = input.len().saturating_sub(after.len());
        } else if rest.get(1).is_some_and(u8::is_ascii_alphabetic) {
            let (name, after_name) = tag_name(rest.get(1..).unwrap_or(&[]));
            if name.eq_ignore_ascii_case(BODY) {
                return Ok(());
            }
            let (found, after) = attributes(after_name)?;
            at = input.len().saturating_sub(after.len());
            if name.eq_ignore_ascii_case(LINK) && found.rel.is_some_and(holds_alternate) {
                if examined >= MAX_ELEMENT_POINTERS {
                    return Ok(());
                }
                examined = examined.saturating_add(1);
                if let (Some(media), Some(href)) = (found.media.and_then(PointerMedia::of), found.href) {
                    each(Pointer { target: trim_space(href), media, found: Found::Element { after_base } });
                }
            } else if name.eq_ignore_ascii_case(BASE) && found.href.is_some() {
                after_base = true;
            } else if RAW_TEXT.iter().any(|raw| name.eq_ignore_ascii_case(raw)) {
                at = at.saturating_add(raw_text_end(input.get(at..).unwrap_or(&[]), name).ok_or(Stop)?);
            }
        } else {
            // A `<` that opens no tag is text.
            at = at.saturating_add(1);
        }
    }
    Ok(())
}

/// The offset just past the end of a comment whose text starts at `input`
/// (with its opening dashes), if it ends.
fn comment_end(input: &[u8]) -> Option<usize> {
    let close = find(input, COMMENT_CLOSE).map(|at| at.saturating_add(COMMENT_CLOSE.len()));
    let bang = find(input, COMMENT_CLOSE_BANG).map(|at| at.saturating_add(COMMENT_CLOSE_BANG.len()));
    match (close, bang) {
        (Some(close), Some(bang)) => Some(close.min(bang)),
        (one, other) => one.or(other),
    }
}

/// The offset of the end tag that ends a raw text element named `name` in
/// `input`: `</name` followed by white space, `/` or `>`.
fn raw_text_end(input: &[u8], name: &[u8]) -> Option<usize> {
    let mut at = 0_usize;
    loop {
        let start = at.saturating_add(find(input.get(at..)?, END_TAG_OPEN)?);
        let after = input.get(start.saturating_add(END_TAG_OPEN.len())..)?;
        let closes = after.get(..name.len()).is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            && after.get(name.len()).is_some_and(|&byte| is_space(byte) || byte == SOLIDUS || byte == TAG_CLOSE);
        if closes {
            return Some(start);
        }
        at = start.saturating_add(END_TAG_OPEN.len());
    }
}

/// An element's name at the start of `input`, up to white space, `/` or
/// `>`, and what follows it.
fn tag_name(input: &[u8]) -> (&[u8], &[u8]) {
    let end =
        input.iter().position(|&byte| is_space(byte) || byte == SOLIDUS || byte == TAG_CLOSE).unwrap_or(input.len());
    (input.get(..end).unwrap_or(&[]), input.get(end..).unwrap_or(&[]))
}

/// Reads a tag's attributes up to and past its `>`: the ones that matter,
/// and what follows the tag.
fn attributes(mut input: &[u8]) -> Result<(Attributes<'_>, &[u8]), Stop> {
    let mut found = Attributes::default();
    loop {
        input = skip(input, |byte| is_space(byte) || byte == SOLIDUS);
        let (&first, after_first) = input.split_first().ok_or(Stop)?;
        if first == TAG_CLOSE {
            return Ok((found, after_first));
        }
        // HTML: a name runs to white space, `/`, `>` or `=`, and a `=` that
        // begins it is part of it.
        let end = after_first
            .iter()
            .position(|&byte| is_space(byte) || byte == SOLIDUS || byte == TAG_CLOSE || byte == VALUE_MARK)
            .map_or(input.len(), |at| at.saturating_add(1));
        let name = input.get(..end).unwrap_or(&[]);
        input = skip(input.get(end..).unwrap_or(&[]), is_space);
        let mut value: &[u8] = &[];
        if let Some((&VALUE_MARK, after)) = input.split_first() {
            (value, input) = attribute_value(skip(after, is_space))?;
        }
        let slot = if name.eq_ignore_ascii_case(REL) {
            &mut found.rel
        } else if name.eq_ignore_ascii_case(TYPE) {
            &mut found.media
        } else if name.eq_ignore_ascii_case(HREF) {
            &mut found.href
        } else {
            continue;
        };
        if slot.is_none() {
            *slot = Some(value);
        }
    }
}

/// An attribute's value at the start of `input`: double-quoted,
/// single-quoted, or unquoted up to white space or `>`; and what follows.
fn attribute_value(input: &[u8]) -> Result<(&[u8], &[u8]), Stop> {
    match input.split_first() {
        Some((&quote, after)) if quote == DOUBLE_QUOTE || quote == SINGLE_QUOTE => {
            let close = after.iter().position(|&byte| byte == quote).ok_or(Stop)?;
            Ok((after.get(..close).unwrap_or(&[]), after.get(close.saturating_add(1)..).unwrap_or(&[])))
        }
        _ => {
            let end = input.iter().position(|&byte| is_space(byte) || byte == TAG_CLOSE).unwrap_or(input.len());
            Ok((input.get(..end).unwrap_or(&[]), input.get(end..).unwrap_or(&[])))
        }
    }
}

/// The offset just past the first `byte` in `input`.
fn past(input: &[u8], byte: u8) -> Option<usize> {
    input.iter().position(|&candidate| candidate == byte).map(|at| at.saturating_add(1))
}

/// The offset of the first `needle` in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// `input` without the leading bytes `skipped` takes.
fn skip(input: &[u8], skipped: impl Fn(u8) -> bool) -> &[u8] {
    let start = input.iter().position(|&byte| !skipped(byte)).unwrap_or(input.len());
    input.get(start..).unwrap_or(&[])
}

/// `value` without leading or trailing white space.
fn trim_space(value: &[u8]) -> &[u8] {
    let start = value.iter().position(|&byte| !is_space(byte)).unwrap_or(value.len());
    let end = value.iter().rposition(|&byte| !is_space(byte)).map_or(start, |last| last.saturating_add(1));
    value.get(start..end.max(start)).unwrap_or(&[])
}
