//! The `Link` header fields of a response (RFC 8288 section 3), read for
//! pointers (REQ-2476).
//!
//! ```text
//! Link       = #link-value
//! link-value = "<" URI-Reference ">" *( OWS ";" OWS link-param )
//! link-param = token BWS [ "=" BWS ( token / quoted-string ) ]
//! ```

use super::{Found, MAX_HEADER_POINTERS, Pointer, PointerMedia, holds_alternate};
use crate::consts::header;
use crate::http::{each_field, is_token_character};

/// What opens and closes a link value's target.
const TARGET_OPEN: u8 = b'<';
const TARGET_CLOSE: u8 = b'>';
/// Between a link value's parameters.
const PARAMETER_MARK: u8 = b';';
/// Between link values (RFC 9110 section 5.6.1).
const LIST_MARK: u8 = b',';
/// Between a parameter's name and its value.
const VALUE_MARK: u8 = b'=';
/// Opens and closes a quoted string, and escapes within one (RFC 9110
/// section 5.6.4).
const QUOTE: u8 = b'"';
const ESCAPE: u8 = b'\\';
/// Between a media type's type and subtype (RFC 9110 section 8.3.1).
const MEDIA_TYPE_SLASH: u8 = b'/';
/// The parameters a pointer is judged by (RFC 8288 sections 3.2, 3.3 and
/// 3.4.1).
const REL: &[u8] = b"rel";
const TYPE: &[u8] = b"type";
const ANCHOR: &[u8] = b"anchor";

/// One link value's target and the parameters a pointer needs, each the
/// first of its name (RFC 8288 section 3.3: "occurrences after the first
/// MUST be ignored").
#[derive(Clone, Copy, Default)]
struct LinkValue<'a> {
    target: &'a [u8],
    rel: Option<&'a [u8]>,
    media: Option<&'a [u8]>,
    anchor: bool,
}

/// Calls `each` with every pointer of the `Link` header fields of a
/// complete response head `head`, in the order received: a link value
/// whose `rel` holds `alternate`, whose `type` is a [`PointerMedia`] and
/// which has no `anchor`, since an anchor makes it describe another
/// resource. A field that cannot be split into link values is ignored
/// whole; at most [`MAX_HEADER_POINTERS`] link values are examined in all.
/// A head that does not parse gives none.
pub fn read_headers(head: &[u8], each: &mut dyn FnMut(Pointer<'_>)) {
    let mut examined = 0_usize;
    each_field(head, header::LINK, &mut |field| {
        // The field is split once to see that it splits, then read.
        if split(field, &mut |_| true).is_err() {
            return;
        }
        let _ = split(field, &mut |value| {
            if examined >= MAX_HEADER_POINTERS {
                return false;
            }
            examined = examined.saturating_add(1);
            let media = value.media.and_then(PointerMedia::of);
            if let Some(media) = media
                && !value.anchor
                && value.rel.is_some_and(holds_alternate)
            {
                each(Pointer { target: value.target, media, found: Found::Header });
            }
            true
        });
    });
}

/// The field was not a list of link values.
struct Malformed;

/// Splits a field's value into link values, calling `each` with each until
/// it returns `false`.
fn split<'a>(field: &'a [u8], each: &mut dyn FnMut(LinkValue<'a>) -> bool) -> Result<(), Malformed> {
    let mut rest = field;
    loop {
        rest = skip_list_gap(rest);
        let Some((&first, after)) = rest.split_first() else { return Ok(()) };
        if first != TARGET_OPEN {
            return Err(Malformed);
        }
        let close = after.iter().position(|&byte| byte == TARGET_CLOSE).ok_or(Malformed)?;
        let mut value = LinkValue { target: after.get(..close).unwrap_or(&[]), ..LinkValue::default() };
        rest = after.get(close.saturating_add(1)..).unwrap_or(&[]);
        loop {
            rest = skip_space(rest);
            match rest.split_first() {
                None | Some((&LIST_MARK, _)) => break,
                Some((&PARAMETER_MARK, after)) => rest = parameter(skip_space(after), &mut value)?,
                Some(_) => return Err(Malformed),
            }
        }
        if !each(value) {
            return Ok(());
        }
    }
}

/// Reads one parameter at the start of `input` into `value`; what follows.
fn parameter<'a>(input: &'a [u8], value: &mut LinkValue<'a>) -> Result<&'a [u8], Malformed> {
    let name_end = input.iter().position(|&byte| !is_token_character(byte)).unwrap_or(input.len());
    if name_end == 0 {
        return Err(Malformed);
    }
    let name = input.get(..name_end).unwrap_or(&[]);
    let mut rest = skip_space(input.get(name_end..).unwrap_or(&[]));
    let mut text: &[u8] = &[];
    if let Some((&VALUE_MARK, after)) = rest.split_first() {
        let after = skip_space(after);
        (text, rest) = parameter_value(after)?;
    }
    if name.eq_ignore_ascii_case(REL) && value.rel.is_none() {
        value.rel = Some(text);
    } else if name.eq_ignore_ascii_case(TYPE) && value.media.is_none() {
        value.media = Some(text);
    } else if name.eq_ignore_ascii_case(ANCHOR) {
        value.anchor = true;
    }
    Ok(rest)
}

/// A token, an unquoted media type or a quoted string at the start of `input`, without its quotes,
/// and what follows. A quoted string is given as written between its
/// quotes: an escaped character keeps its backslash, so a value that needed
/// one matches no relation or media type, which only refuses.
fn parameter_value(input: &[u8]) -> Result<(&[u8], &[u8]), Malformed> {
    let Some((&QUOTE, after)) = input.split_first() else {
        // RFC 5988 section 5 let `type` be an unquoted media type, so a `/`
        // is taken in an unquoted value, as readers of the older form do.
        let end =
            input.iter().position(|&byte| !is_token_character(byte) && byte != MEDIA_TYPE_SLASH).unwrap_or(input.len());
        if end == 0 {
            return Err(Malformed);
        }
        return Ok((input.get(..end).unwrap_or(&[]), input.get(end..).unwrap_or(&[])));
    };
    let mut index = 0_usize;
    loop {
        match after.get(index) {
            None => return Err(Malformed),
            Some(&QUOTE) => {
                return Ok((after.get(..index).unwrap_or(&[]), after.get(index.saturating_add(1)..).unwrap_or(&[])));
            }
            Some(&ESCAPE) => index = index.saturating_add(2),
            Some(_) => index = index.saturating_add(1),
        }
    }
}

/// Skips optional white space.
fn skip_space(input: &[u8]) -> &[u8] {
    let start = input.iter().position(|&byte| byte != b' ' && byte != b'\t').unwrap_or(input.len());
    input.get(start..).unwrap_or(&[])
}

/// Skips white space and empty list elements (RFC 9110 section 5.6.1.2).
fn skip_list_gap(input: &[u8]) -> &[u8] {
    let start =
        input.iter().position(|&byte| byte != b' ' && byte != b'\t' && byte != LIST_MARK).unwrap_or(input.len());
    input.get(start..).unwrap_or(&[])
}
