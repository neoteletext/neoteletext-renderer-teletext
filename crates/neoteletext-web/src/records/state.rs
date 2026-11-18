//! Viewer state's records, server to receiver (neoteletext-common's
//! viewer-state design, adopted by DEC-6471: VS-204, VS-205, VS-208), in
//! the record frame on the push connection beside the channel's other
//! records (DOC-1951).
//!
//! ```text
//! `STATE_CHALLENGE`, type 18   bytes 0-31  challenge
//!                            byte 32     create: 0 or 1
//! `STATE_END`, type 19         bytes 0-31  key_id: the key identifier (VS-302)
//! `STATE_KEEP_OFFER`, type 20  no bytes
//! ```
//!
//! Every body has a fixed length and its byte values are listed, so a
//! record of any other length, or with a value not listed, is refused
//! whole (VS-204's preamble). Type 8, `SET_COOKIE`, is retired and never
//! reused.
//!
//! VS-204 named types 13 to 15, but 13 is kept for the CHANNEL record
//! (REQ-3581) and 14 and 15 are REGIS and `DRAWING_LINKS`; these take the
//! next types free, 18 to 20.

use neoteletext_core::packet::Packet;

use super::{RecordFault, write_packets};

pub use neoteletext_input::state::CHALLENGE_BYTES;

/// Record type: `STATE_CHALLENGE` (VS-204), read by the web agent.
pub const TYPE_STATE_CHALLENGE: u8 = 18;
/// Record type: `STATE_END` (VS-204), read by the web agent.
pub const TYPE_STATE_END: u8 = 19;
/// Record type: `STATE_KEEP_OFFER` (VS-204, VS-208), read by the web
/// agent.
pub const TYPE_STATE_KEEP_OFFER: u8 = 20;

/// The CHANNEL record's type, kept for it (REQ-3581).
const KEPT_FOR_CHANNEL: u8 = 13;
/// `SET_COOKIE`'s type, retired (VS-204).
const RETIRED_SET_COOKIE: u8 = 8;

/// Whether `kind` is a type the record table does not already give
/// (DOC-1951), counting 8, `SET_COOKIE`'s, which is never reused, and 13,
/// kept for CHANNEL.
const fn is_new_type(kind: u8) -> bool {
    use neoteletext_audio::record::{
        TYPE_CONTROL, TYPE_CURSOR, TYPE_DATA, TYPE_DENIED, TYPE_DRAWING_LINKS, TYPE_INDEX, TYPE_INPUT_CONTROL,
        TYPE_LINKS, TYPE_LOG, TYPE_NAPLPS, TYPE_OBJECT, TYPE_REDIRECT, TYPE_REGIS, TYPE_REGIS_CURSOR, TYPE_SUBPAGES,
    };
    !matches!(
        kind,
        TYPE_OBJECT
            | TYPE_DATA
            | TYPE_CONTROL
            | TYPE_CURSOR
            | TYPE_INPUT_CONTROL
            | TYPE_INDEX
            | TYPE_LOG
            | RETIRED_SET_COOKIE
            | TYPE_LINKS
            | TYPE_SUBPAGES
            | TYPE_REDIRECT
            | TYPE_DENIED
            | KEPT_FOR_CHANNEL
            | TYPE_REGIS
            | TYPE_DRAWING_LINKS
            | TYPE_REGIS_CURSOR
            | TYPE_NAPLPS
    )
}
const _: () =
    assert!(is_new_type(TYPE_STATE_CHALLENGE) && is_new_type(TYPE_STATE_END) && is_new_type(TYPE_STATE_KEEP_OFFER));

/// The bytes of a key identifier: a SHA-256 digest (VS-302, FIPS 180-4).
pub const KEY_ID_BYTES: usize = crate::consts::SHA256_DIGEST_BYTES;
/// `STATE_CHALLENGE`'s body: the challenge and the `create` byte.
pub const CHALLENGE_RECORD_BYTES: usize = CHALLENGE_BYTES + 1;
/// `STATE_END`'s body: the key identifier.
pub const END_RECORD_BYTES: usize = KEY_ID_BYTES;
/// `STATE_KEEP_OFFER`'s body: nothing.
pub const KEEP_OFFER_RECORD_BYTES: usize = 0;

// VS-204's table: 33 and 32 bytes.
const _: () = assert!(CHALLENGE_RECORD_BYTES == 33 && END_RECORD_BYTES == 32);

/// `create`'s byte when the server asks the browser to make a key if it
/// holds none (VS-104).
const CREATE: u8 = 1;
/// `create`'s byte when it does not.
const NO_CREATE: u8 = 0;

/// A `STATE_CHALLENGE` (VS-204, VS-205): a fresh challenge, valid once on
/// this connection, and whether a key may be made to answer it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateChallenge {
    /// The challenge, from the server's cryptographic random source.
    pub challenge: [u8; CHALLENGE_BYTES],
    /// The program asked for state: a browser with no key for the channel
    /// may make one, under VS-104's conditions.
    pub create: bool,
}

/// A `STATE_END` (VS-204): delete the key whose identifier this is, and
/// only that key, so that a late `STATE_END` never deletes a key made
/// after a reset.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateEnd {
    /// The key identifier of the key to delete (VS-302).
    pub key_id: [u8; KEY_ID_BYTES],
}

/// Reads a `STATE_CHALLENGE`'s body.
///
/// # Errors
/// [`RecordFault::Length`] for a body of another length;
/// [`RecordFault::BadValue`] for a `create` byte other than 0 or 1.
pub fn read_challenge(body: &[u8]) -> Result<StateChallenge, RecordFault> {
    let whole: &[u8; CHALLENGE_RECORD_BYTES] = body.try_into().map_err(|_| RecordFault::Length)?;
    let Some((challenge, &[create])) = whole.split_first_chunk::<CHALLENGE_BYTES>() else {
        return Err(RecordFault::Length);
    };
    let create = match create {
        CREATE => true,
        NO_CREATE => false,
        _ => return Err(RecordFault::BadValue("state challenge create")),
    };
    Ok(StateChallenge { challenge: *challenge, create })
}

/// Reads a `STATE_END`'s body.
///
/// # Errors
/// [`RecordFault::Length`] for a body of another length.
pub fn read_end(body: &[u8]) -> Result<StateEnd, RecordFault> {
    let key_id: [u8; KEY_ID_BYTES] = body.try_into().map_err(|_| RecordFault::Length)?;
    Ok(StateEnd { key_id })
}

/// Reads a `STATE_KEEP_OFFER`'s body.
///
/// # Errors
/// [`RecordFault::Length`] for a body that is not empty.
pub const fn read_keep_offer(body: &[u8]) -> Result<(), RecordFault> {
    if body.len() == KEEP_OFFER_RECORD_BYTES { Ok(()) } else { Err(RecordFault::Length) }
}

/// Writes a `STATE_CHALLENGE` as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse; none arises from a [`StateChallenge`].
pub fn encode_challenge(
    challenge: StateChallenge,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; CHALLENGE_RECORD_BYTES];
    let (bytes, create) = body.split_at_mut(CHALLENGE_BYTES);
    bytes.copy_from_slice(&challenge.challenge);
    create.copy_from_slice(&[if challenge.create { CREATE } else { NO_CREATE }]);
    // The writer checks with the reader, so the two cannot disagree.
    if read_challenge(&body)? != challenge {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_STATE_CHALLENGE, &body, service, record_id, out).ok_or(RecordFault::Length)
}

/// Writes a `STATE_END` as packets 4/31 of audio service `service`, record
/// id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse; none arises from a [`StateEnd`].
pub fn encode_end(
    end: StateEnd,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    if read_end(&end.key_id)? != end {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_STATE_END, &end.key_id, service, record_id, out).ok_or(RecordFault::Length)
}

/// Writes a `STATE_KEEP_OFFER` as packets 4/31 of audio service `service`,
/// record id `record_id`. Returns the packets written.
///
/// # Errors
/// [`RecordFault::Length`] when the frame cannot be written, which does not
/// arise.
pub fn encode_keep_offer(service: u8, record_id: u8, out: &mut dyn FnMut(Packet)) -> Result<usize, RecordFault> {
    write_packets(TYPE_STATE_KEEP_OFFER, &[], service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
