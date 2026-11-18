//! Viewer state's messages from the receiver to the channel's server
//! (neoteletext-common's viewer-state design, adopted by DEC-6471:
//! VS-204): `STATE_PROOF`, `STATE_NONE` and `STATE_OFFER`, carried as extension
//! events of the input stream ([`crate::event::STATE_PROOF`],
//! [`crate::event::STATE_NONE`], [`crate::event::STATE_OFFER`]).
//!
//! Every field has a fixed length and an enumerated value, so a message of
//! any other length, or with a value not listed here, is a protocol error
//! (VS-204's preamble).
//!
//! These messages are not the viewer's input. They count nothing in the
//! input count ([`crate::event::units`]), are no gesture (VS-212), and are
//! written with [`crate::event::write`] and sent straight on the push
//! connection, never through [`crate::session::Session::send`]: a proof is
//! bound to one challenge on one connection (VS-205, VS-207), so it is
//! never kept to be sent again on a resumed connection.

use crate::event::InputError;

/// The bytes of a challenge: 256 bits from the server's cryptographic
/// random source (VS-204, VS-205).
pub const CHALLENGE_BYTES: usize = 32;
/// The bytes of an Ed25519 public key (RFC 8032 section 5.1.5).
pub const PUBLIC_KEY_BYTES: usize = 32;
/// The bytes of an Ed25519 signature (RFC 8032 section 5.1.6).
pub const SIGNATURE_BYTES: usize = 64;
/// `STATE_PROOF`'s payload: the algorithm and the lifetime, one byte each,
/// the public key and the signature (VS-204).
pub const STATE_PROOF_BYTES: usize = 1 + 1 + PUBLIC_KEY_BYTES + SIGNATURE_BYTES;
/// `STATE_NONE`'s payload: the reason, one byte (VS-204).
pub const STATE_NONE_BYTES: usize = 1;
/// `STATE_OFFER`'s payload: none (VS-204).
pub const STATE_OFFER_BYTES: usize = 0;

// VS-204's table gives STATE_PROOF 98 bytes.
const _: () = assert!(STATE_PROOF_BYTES == 98);

/// The signature algorithm of a viewer key (VS-203 item 3, VS-204). Only
/// Ed25519 is defined; the byte keeps a change open (open question Q1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algorithm {
    /// Ed25519, pure, with no context and no prehash (RFC 8032 section
    /// 5.1): 1.
    Ed25519,
}

/// The byte of [`Algorithm::Ed25519`].
const ALGORITHM_ED25519: u8 = 1;

impl Algorithm {
    /// The algorithm's byte on the wire and in the signed bytes.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Ed25519 => ALGORITHM_ED25519,
        }
    }

    /// The algorithm of `code`, or `None` for a byte no algorithm has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            ALGORITHM_ED25519 => Some(Self::Ed25519),
            _ => None,
        }
    }
}

/// How long the browser keeps a key (VS-105 to VS-107), as the proof
/// claims it and signs it (VS-203 item 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lifetime {
    /// A visit key, in memory only: 0.
    Visit,
    /// A key the viewer chose to have remembered: 1.
    Remembered,
}

/// The byte of [`Lifetime::Visit`].
const LIFETIME_VISIT: u8 = 0;
/// The byte of [`Lifetime::Remembered`].
const LIFETIME_REMEMBERED: u8 = 1;

impl Lifetime {
    /// The lifetime's byte on the wire and in the signed bytes.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Visit => LIFETIME_VISIT,
            Self::Remembered => LIFETIME_REMEMBERED,
        }
    }

    /// The lifetime of `code`, or `None` for a byte no lifetime has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            LIFETIME_VISIT => Some(Self::Visit),
            LIFETIME_REMEMBERED => Some(Self::Remembered),
            _ => None,
        }
    }
}

/// Why a receiver answers without a proof (`STATE_NONE`, VS-204).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoneReason {
    /// 0: no key for this channel, and none asked to be made.
    NoKey,
    /// 1: the viewer blocked the channel or refused state.
    Declined,
    /// 2: the client never keeps state (VS-501, VS-504).
    Stateless,
    /// 3: the key in use on this connection has just been forgotten.
    Forgotten,
    /// 4: asked to create a key without a recent gesture (VS-104, VS-212).
    NoGesture,
}

/// The bytes of each [`NoneReason`], in VS-204's order.
const NONE_REASONS: [NoneReason; 5] =
    [NoneReason::NoKey, NoneReason::Declined, NoneReason::Stateless, NoneReason::Forgotten, NoneReason::NoGesture];

impl NoneReason {
    /// The reason's byte on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::NoKey => 0,
            Self::Declined => 1,
            Self::Stateless => 2,
            Self::Forgotten => 3,
            Self::NoGesture => 4,
        }
    }

    /// The reason of `code`, or `None` for a byte no reason has.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        NONE_REASONS.get(usize::from(code)).copied()
    }
}

/// A `STATE_PROOF` (VS-204): the public key and its signature over the
/// bytes VS-203 names. It borrows the bytes it was read from, or the
/// sender's own arrays, so that an [`crate::event::Event`] stays small.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateProof<'a> {
    /// The signature algorithm.
    pub algorithm: Algorithm,
    /// The lifetime the browser claims for the key, which it signs.
    pub lifetime: Lifetime,
    /// The public key.
    pub public_key: &'a [u8; PUBLIC_KEY_BYTES],
    /// The signature.
    pub signature: &'a [u8; SIGNATURE_BYTES],
}

/// Reads `STATE_PROOF`'s payload: exactly [`STATE_PROOF_BYTES`], each byte
/// value listed.
///
/// # Errors
/// [`InputError::BadValue`] for a payload of another length, an algorithm
/// or a lifetime not listed.
pub fn read_proof(payload: &[u8]) -> Result<StateProof<'_>, InputError> {
    let refused = InputError::BadValue("state proof");
    let whole: &[u8; STATE_PROOF_BYTES] = payload.try_into().map_err(|_| refused)?;
    let (&[algorithm, lifetime], rest) = whole.split_first_chunk::<2>().ok_or(refused)?;
    let (public_key, signature) = rest.split_first_chunk::<PUBLIC_KEY_BYTES>().ok_or(refused)?;
    let signature: &[u8; SIGNATURE_BYTES] = signature.try_into().map_err(|_| refused)?;
    Ok(StateProof {
        algorithm: Algorithm::from_code(algorithm).ok_or(refused)?,
        lifetime: Lifetime::from_code(lifetime).ok_or(refused)?,
        public_key,
        signature,
    })
}

/// `STATE_PROOF`'s payload.
#[must_use]
pub fn proof_payload(proof: &StateProof<'_>) -> [u8; STATE_PROOF_BYTES] {
    let mut payload = [0_u8; STATE_PROOF_BYTES];
    let (head, rest) = payload.split_at_mut(2);
    head.copy_from_slice(&[proof.algorithm.code(), proof.lifetime.code()]);
    let (public_key, signature) = rest.split_at_mut(PUBLIC_KEY_BYTES);
    public_key.copy_from_slice(proof.public_key);
    signature.copy_from_slice(proof.signature);
    payload
}

/// Reads `STATE_NONE`'s payload: exactly one byte, a reason listed.
///
/// # Errors
/// [`InputError::BadValue`] for a payload of another length or a reason
/// not listed.
pub fn read_none(payload: &[u8]) -> Result<NoneReason, InputError> {
    let refused = InputError::BadValue("state none");
    let [code] = payload else { return Err(refused) };
    NoneReason::from_code(*code).ok_or(refused)
}

/// Reads `STATE_OFFER`'s payload: nothing.
///
/// # Errors
/// [`InputError::BadValue`] for a payload that is not empty.
pub const fn read_offer(payload: &[u8]) -> Result<(), InputError> {
    if payload.len() == STATE_OFFER_BYTES { Ok(()) } else { Err(InputError::BadValue("state offer")) }
}

#[cfg(test)]
mod tests;
