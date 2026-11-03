//! The terminal's replies to the graphics queries of `neottx show` and
//! `view` (REQ-RND-029, REQ-382, DEC-2459): what it answered, read
//! strictly from bytes the terminal sent, which are untrusted input
//! (REQ-SEC-010, REQ-423).
//!
//! The queries are sent in this order, and the replies read back:
//! 1. the kitty graphics protocol's query, a one-pixel image to check and
//!    not keep (`ESC _ G i=<id>,s=1,v=1,a=q,t=d,f=24;AAAA ESC \`); a
//!    terminal with the protocol answers `ESC _ G i=<id>;OK ESC \`;
//! 2. XTVERSION (`ESC [ > 0 q`), answered by `ESC P > | <name and version>
//!    ESC \`, which names iTerm2 among others;
//! 3. Primary Device Attributes (`ESC [ c`), answered by `ESC [ ? <class>;
//!    <attributes> c`, where attribute 4 is Sixel graphics (the VT340 and
//!    xterm's own documentation of its control sequences). Every terminal
//!    of the VT100 line answers it, and terminals answer queries in order,
//!    so its reply marks the end of the replies.
//!
//! Anything else in the bytes is passed over. Only [`MAX_REPLY_BYTES`] are
//! ever read, and a name only [`MAX_VERSION_BYTES`] long is kept.

/// The kitty query's image id, which its reply must name.
pub const KITTY_QUERY_ID: u32 = 31;
/// The queries, in the order above.
pub const QUERIES: &[u8] = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[>0q\x1b[c";
/// The most reply bytes read: room for the three replies many times over.
pub const MAX_REPLY_BYTES: usize = 1024;
/// The most bytes of a terminal's name and version kept.
pub const MAX_VERSION_BYTES: usize = 64;

/// The escape character.
const ESC: u8 = 0x1B;
/// What follows ESC to begin an Application Program Command (kitty's
/// replies), a Device Control String (XTVERSION's) and a Control Sequence
/// (Device Attributes').
const APC: u8 = b'_';
const DCS: u8 = b'P';
const CSI: u8 = b'[';
/// The String Terminator: ESC then a backslash.
const STRING_TERMINATOR: u8 = b'\\';
/// Sixel graphics among the Primary Device Attributes.
const SIXEL_ATTRIBUTE: u32 = 4;
/// The most digits of one Device Attributes parameter believed.
const MAX_PARAMETER_DIGITS: usize = 5;

/// What the terminal answered.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Replies {
    /// The kitty graphics query was answered OK.
    pub kitty_graphics: bool,
    /// The Primary Device Attributes include Sixel graphics.
    pub sixel: bool,
    /// The name and version XTVERSION gave, printable ASCII only.
    pub version: Option<String>,
    /// The Primary Device Attributes reply came: every reply is in.
    pub complete: bool,
}

impl Replies {
    /// Whether XTVERSION named iTerm2.
    #[must_use]
    pub fn is_iterm2(&self) -> bool {
        self.version.as_deref().is_some_and(|version| version.starts_with("iTerm2"))
    }
}

/// Reads the replies in `bytes`, at most [`MAX_REPLY_BYTES`] of them.
#[must_use]
pub fn parse(bytes: &[u8]) -> Replies {
    let bytes = bytes.get(..bytes.len().min(MAX_REPLY_BYTES)).unwrap_or(&[]);
    let mut replies = Replies::default();
    let mut at = 0;
    while let Some(rest) = bytes.get(at..) {
        let Some(start) = rest.iter().position(|&byte| byte == ESC) else { break };
        let sequence = rest.get(start..).unwrap_or(&[]);
        let used = match sequence.get(1) {
            Some(&APC) => string(sequence).map(|(body, used)| {
                kitty(body, &mut replies);
                used
            }),
            Some(&DCS) => string(sequence).map(|(body, used)| {
                version(body, &mut replies);
                used
            }),
            Some(&CSI) => attributes(sequence, &mut replies),
            _ => None,
        };
        // A sequence that is not whole, or not one of these, is passed over
        // by its escape character alone.
        at = at.saturating_add(start).saturating_add(used.unwrap_or(1));
        if replies.complete {
            break;
        }
    }
    replies
}

/// The body of a string sequence (ESC, its introducer, the body, then ESC
/// and a backslash), and the bytes it used; `None` if it is not ended.
fn string(sequence: &[u8]) -> Option<(&[u8], usize)> {
    let body = sequence.get(2..)?;
    let end = body.windows(2).position(|pair| pair == [ESC, STRING_TERMINATOR])?;
    Some((body.get(..end)?, end.checked_add(4)?))
}

/// A kitty graphics reply: `G`, the keys, `;`, the message.
fn kitty(body: &[u8], replies: &mut Replies) {
    let Some(rest) = body.strip_prefix(b"G") else { return };
    let Some(split) = rest.iter().position(|&byte| byte == b';') else { return };
    let (keys, message) = (rest.get(..split).unwrap_or(&[]), rest.get(split.saturating_add(1)..).unwrap_or(&[]));
    let id = format!("i={KITTY_QUERY_ID}");
    let named = keys.split(|&byte| byte == b',').any(|key| key == id.as_bytes());
    if named && message == b"OK" {
        replies.kitty_graphics = true;
    }
}

/// An XTVERSION reply: `>|`, then the name and version.
fn version(body: &[u8], replies: &mut Replies) {
    let Some(name) = body.strip_prefix(b">|") else { return };
    if name.is_empty() || name.len() > MAX_VERSION_BYTES || !name.iter().all(|&byte| (b' '..=b'~').contains(&byte)) {
        return;
    }
    replies.version = Some(name.iter().map(|&byte| char::from(byte)).collect());
}

/// A Primary Device Attributes reply, `ESC [ ? digits and semicolons c`,
/// and the bytes it used; `None` if it is not one.
fn attributes(sequence: &[u8], replies: &mut Replies) -> Option<usize> {
    let body = sequence.get(2..)?.strip_prefix(b"?")?;
    let end = body.iter().position(|&byte| !(byte.is_ascii_digit() || byte == b';'))?;
    if body.get(end) != Some(&b'c') {
        return None;
    }
    let parameters = body.get(..end)?;
    for parameter in parameters.split(|&byte| byte == b';') {
        if parameter.len() <= MAX_PARAMETER_DIGITS
            && core::str::from_utf8(parameter).ok().and_then(|text| text.parse::<u32>().ok()) == Some(SIXEL_ATTRIBUTE)
        {
            replies.sixel = true;
        }
    }
    replies.complete = true;
    end.checked_add(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// REQ-382: kitty, XTVERSION and the Sixel attribute are each read from
    /// a well-formed reply, in any mix, with other bytes around them.
    #[test]
    fn each_reply_is_read() {
        let replies = parse(b"\x1b_Gi=31;OK\x1b\\\x1bP>|kitty(0.35.2)\x1b\\\x1b[?62;22c");
        assert_eq!(
            replies,
            Replies { kitty_graphics: true, sixel: false, version: Some("kitty(0.35.2)".into()), complete: true }
        );
        let replies = parse(b"x\x1bP>|XTerm(390)\x1b\\\x1b[?63;1;2;4;6;9;15;16;22;28c");
        assert!(replies.sixel && replies.complete && !replies.kitty_graphics && !replies.is_iterm2());
        let replies = parse(b"\x1bP>|iTerm2 3.5.0\x1b\\\x1b[?62;4c");
        assert!(replies.is_iterm2() && replies.sixel);
    }

    /// REQ-SEC-010 and REQ-423: malformed, oversized and missing replies
    /// give no graphics: an error or another image's id from kitty, a
    /// version with control bytes or too long, an attribute of 4 inside a
    /// longer number, a reply never ended, and nothing at all.
    #[test]
    fn malformed_oversized_and_missing_replies_give_nothing() {
        assert_eq!(parse(b""), Replies::default());
        assert!(!parse(b"\x1b_Gi=31;ENOENT:no\x1b\\\x1b[?62c").kitty_graphics);
        assert!(!parse(b"\x1b_Gi=7;OK\x1b\\\x1b[?62c").kitty_graphics);
        assert!(!parse(b"\x1b_Gi=31;OK").kitty_graphics, "not ended");
        assert_eq!(parse(b"\x1bP>|bad\x07name\x1b\\").version, None);
        let long = [b"\x1bP>|".as_slice(), &[b'a'; MAX_VERSION_BYTES + 1], b"\x1b\\"].concat();
        assert_eq!(parse(&long).version, None);
        let replies = parse(b"\x1b[?62;44;400004c");
        assert!(replies.complete && !replies.sixel);
        assert!(!parse(b"\x1b[?62;4").complete, "not ended");
        // Past the bound nothing is read.
        let late = [vec![b'.'; MAX_REPLY_BYTES], b"\x1b[?62;4c".to_vec()].concat();
        assert_eq!(parse(&late), Replies::default());
    }
}
