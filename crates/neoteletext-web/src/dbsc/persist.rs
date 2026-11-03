//! The persistence format of the bound sessions (REQ-WEB-017, decision
//! E.110 item 4, TODO-1207): export, and import of the data as untrusted,
//! in the manner of the cookie store's (decision E.104), so that a port
//! keeps its sessions beside its cookies across runs. The device keys are
//! never in it: a session names its key by the port's handle, and the key
//! stays in the port's protected storage (decision E.110 item 2).
//!
//! ```text
//! neoteletext-sessions 1
//! key expires credentials rules id-length url-length challenge-length id url challenge
//! name-length name                                  (one line a credential)
//! include domain-length path-length domain path      (one line a rule)
//! ```
//!
//! Numbers are decimal and separated by one space; texts follow their
//! lengths back to back. `challenge-length` is the challenge's length plus
//! one, or 0 for none.

use super::instructions::ScopeRule;
use super::session::{Session, Sessions};
use super::{
    KeyHandle, MAX_CHALLENGE, MAX_CREDENTIAL_NAME, MAX_CREDENTIALS, MAX_RULE_TEXT, MAX_SCOPE_RULES, MAX_SESSION_ID,
    MAX_SESSION_URL, MAX_SESSIONS,
};
use crate::bytes::{MAX_U64_DIGITS, Overflow, TextBuf, Writer, parse_decimal};
use crate::url::Url;

/// The first line of exported session data.
const EXPORT_VERSION: &[u8] = b"neoteletext-sessions 1\n";
/// A session line's numbers before its texts.
const SESSION_FIELDS: usize = 7;
/// A rule line's numbers before its texts.
const RULE_FIELDS: usize = 3;
/// What separates fields, and what ends a line.
const SPACE: u8 = b' ';
const NEWLINE: u8 = b'\n';
/// The most bytes of exported data read: every session at every bound,
/// with room for its numbers. A longer input is refused whole, unread.
pub const MAX_EXPORT_BYTES: usize = EXPORT_VERSION.len()
    + MAX_SESSIONS
        * (SESSION_FIELDS * (MAX_U64_DIGITS + 1)
            + MAX_SESSION_ID
            + MAX_SESSION_URL
            + MAX_CHALLENGE
            + 1
            + MAX_CREDENTIALS * (MAX_U64_DIGITS + 1 + MAX_CREDENTIAL_NAME + 1)
            + MAX_SCOPE_RULES * (RULE_FIELDS * (MAX_U64_DIGITS + 1) + 2 * MAX_RULE_TEXT + 1));

/// Why saved session data was refused. Nothing of it is kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportError {
    /// Not data this version wrote.
    Version,
    /// Longer than [`MAX_EXPORT_BYTES`].
    TooLarge,
    /// A line cut short, a number that does not read, a length past its
    /// bound, or bytes after the last line.
    Malformed,
    /// A session that could never have been registered: a refresh
    /// address that is not an HTTPS address, site scope (decision E.114
    /// item 6), or more sessions, credentials or rules than are kept.
    Refused,
    /// The agent has no wall clock (`Input::WallClock`), so its seconds
    /// mean nothing across runs.
    NoWallClock,
}

impl Sessions {
    /// Writes every session that has not lapsed by `now`, in the store's
    /// seconds. Returns the sessions written.
    ///
    /// # Errors
    /// `out` is too small.
    pub fn export(&self, now: u64, out: &mut Writer<'_>) -> Result<usize, Overflow> {
        out.put(EXPORT_VERSION)?;
        let mut count = 0_usize;
        for (_, session) in self.iter().filter(|(_, session)| session.expires > now) {
            write_session(session, out)?;
            count = count.saturating_add(1);
        }
        Ok(count)
    }

    /// Reads sessions written by [`Self::export`], in place of those held,
    /// treating the data as untrusted: every length and count is bounded
    /// before it is used, and a session is kept only if a registration
    /// could have made it. A session lapsed by `now` is read past and not
    /// kept. Returns the sessions kept.
    ///
    /// # Errors
    /// The data is refused whole, and the store left as it was, when any
    /// of it breaks the format or a session's rules.
    pub fn import(&mut self, data: &[u8], now: u64) -> Result<usize, ImportError> {
        if data.len() > MAX_EXPORT_BYTES {
            return Err(ImportError::TooLarge);
        }
        let mut rest = data.strip_prefix(EXPORT_VERSION).ok_or(ImportError::Version)?;
        let mut read = Self::new();
        let mut kept = 0_usize;
        while !rest.is_empty() {
            let (session, after) = read_session(rest)?;
            rest = after;
            if session.expires <= now {
                continue;
            }
            read.insert(session).map_err(|_| ImportError::Refused)?;
            kept = kept.saturating_add(1);
        }
        *self = read;
        Ok(kept)
    }
}

fn write_session(session: &Session, out: &mut Writer<'_>) -> Result<(), Overflow> {
    let credentials = session.credentials.iter().flatten().count();
    let rules = session.rules.iter().flatten().count();
    let challenge = session.challenge.as_ref().map_or(0, |challenge| challenge.len().saturating_add(1));
    let fields = [
        u64::from(session.key.0),
        session.expires,
        credentials as u64,
        rules as u64,
        session.id.len() as u64,
        session.refresh_url.len() as u64,
        challenge as u64,
    ];
    for field in fields {
        out.decimal(field)?;
        out.byte(SPACE)?;
    }
    out.put(session.id.as_bytes())?;
    out.put(session.refresh_url.as_bytes())?;
    if let Some(challenge) = &session.challenge {
        out.put(challenge.as_bytes())?;
    }
    out.byte(NEWLINE)?;
    for name in session.credentials.iter().flatten() {
        out.decimal(name.len() as u64)?;
        out.byte(SPACE)?;
        out.put(name.as_bytes())?;
        out.byte(NEWLINE)?;
    }
    for rule in session.rules.iter().flatten() {
        for field in [u64::from(rule.include), rule.domain.len() as u64, rule.path.len() as u64] {
            out.decimal(field)?;
            out.byte(SPACE)?;
        }
        out.put(rule.domain.as_bytes())?;
        out.put(rule.path.as_bytes())?;
        out.byte(NEWLINE)?;
    }
    Ok(())
}

/// Reads `N` decimal numbers, each followed by a space.
fn numbers<const N: usize>(mut rest: &[u8]) -> Result<([u64; N], &[u8]), ImportError> {
    let mut fields = [0_u64; N];
    for field in &mut fields {
        let end = rest.iter().position(|&byte| byte == SPACE).ok_or(ImportError::Malformed)?;
        *field = parse_decimal(rest.get(..end).ok_or(ImportError::Malformed)?, MAX_U64_DIGITS)
            .ok_or(ImportError::Malformed)?;
        rest = rest.get(end.saturating_add(1)..).ok_or(ImportError::Malformed)?;
    }
    Ok((fields, rest))
}

/// A length read from the data, refused past `bound` before it is used.
fn bounded(value: u64, bound: usize) -> Result<usize, ImportError> {
    usize::try_from(value).ok().filter(|&length| length <= bound).ok_or(ImportError::Malformed)
}

/// Takes `length` bytes, then whatever follows.
fn take(rest: &[u8], length: usize) -> Result<(&[u8], &[u8]), ImportError> {
    rest.split_at_checked(length).ok_or(ImportError::Malformed)
}

/// Takes the end of a line.
fn end_of_line(rest: &[u8]) -> Result<&[u8], ImportError> {
    rest.strip_prefix(&[NEWLINE]).ok_or(ImportError::Malformed)
}

fn text<const N: usize>(bytes: &[u8]) -> Result<TextBuf<N>, ImportError> {
    TextBuf::from_bytes(bytes).ok_or(ImportError::Malformed)
}

fn read_session(rest: &[u8]) -> Result<(Session, &[u8]), ImportError> {
    let ([key, expires, credentials, rules, id_length, url_length, challenge_length], rest) =
        numbers::<SESSION_FIELDS>(rest)?;
    let credentials = usize::try_from(credentials).ok().filter(|&count| count <= MAX_CREDENTIALS);
    let rules = usize::try_from(rules).ok().filter(|&count| count <= MAX_SCOPE_RULES);
    let (Some(credentials), Some(rules)) = (credentials, rules) else { return Err(ImportError::Refused) };
    let key = u32::try_from(key).map_err(|_| ImportError::Malformed)?;
    let (id, rest) = take(rest, bounded(id_length, MAX_SESSION_ID)?)?;
    let (url, rest) = take(rest, bounded(url_length, MAX_SESSION_URL)?)?;
    let challenge_length = bounded(challenge_length, MAX_CHALLENGE.saturating_add(1))?;
    let (challenge, rest) = take(rest, challenge_length.saturating_sub(1))?;
    let mut rest = end_of_line(rest)?;
    // As a registration makes it (`session_of`): an HTTPS refresh address,
    // whose origin is the session's, and never site scope.
    let refresh = core::str::from_utf8(url).ok().and_then(|url| Url::parse(url).ok());
    let refresh = refresh.filter(Url::is_secure).ok_or(ImportError::Refused)?;
    if id.is_empty() {
        return Err(ImportError::Refused);
    }
    let mut session = Session {
        id: text(id)?,
        key: KeyHandle(key),
        refresh_url: text(url)?,
        origin: refresh.origin(),
        include_site: false,
        rules: [None; MAX_SCOPE_RULES],
        credentials: [None; MAX_CREDENTIALS],
        challenge: if challenge_length == 0 { None } else { Some(text(challenge)?) },
        expires,
    };
    for slot in session.credentials.iter_mut().take(credentials) {
        let ([length], after) = numbers::<1>(rest)?;
        let (name, after) = take(after, bounded(length, MAX_CREDENTIAL_NAME)?)?;
        *slot = Some(text(name)?);
        rest = end_of_line(after)?;
    }
    for slot in session.rules.iter_mut().take(rules) {
        let ([include, domain_length, path_length], after) = numbers::<RULE_FIELDS>(rest)?;
        let include = match include {
            0 => false,
            1 => true,
            _ => return Err(ImportError::Malformed),
        };
        let (domain, after) = take(after, bounded(domain_length, MAX_RULE_TEXT)?)?;
        let (path, after) = take(after, bounded(path_length, MAX_RULE_TEXT)?)?;
        *slot = Some(ScopeRule { include, domain: text(domain)?, path: text(path)? });
        rest = end_of_line(after)?;
    }
    Ok((session, rest))
}
