//! The instruments of the teletext audio profile (area AUD, DEC-6410): the
//! Commodore 64's SID chip and the Yamaha DX7, each rendered from its own
//! input to 16-bit mono samples at 48,000 Hz (REQ-6412).
//!
//! An instrument is a codec of the profile, and a block is 20 milliseconds
//! of the instrument's input: register images for the SID, timed MIDI
//! messages for the DX7. This crate holds the instruments alone. It reads
//! no record and drives no hardware; the audio receiver gives it blocks and
//! a port's voice sink takes its samples.
//!
//! Everything here is `no_std`, integer-only and allocation-free; every
//! buffer has a size fixed by its type. No input makes a block cost more
//! than the bound of its instrument (REQ-6412 item 7).
//!
//! Behind the feature `analog`, which is not on by default, are the
//! building blocks the analogue synthesizers of DEC-6429 are to be made
//! from ([`analog`]): oscillators, filters, envelopes, a chorus and a
//! voice allocator. They are no codec yet.
//!
//! # Whose code this is
//!
//! Only permissively licensed code is the basis of anything here
//! (DEC-6420). The SID emulator is written from the chip's public
//! documentation, with no code and no table from any emulator (DEC-6418).
//! The DX7's parameter layer, under `dx7::msfa`, is translated from
//! Google's music-synthesizer-for-android and stays under Apache-2.0, each
//! file carrying its notice; the DX7's sound kernel and rack are this
//! crate's own. The analogue building blocks are written from public
//! documents alone, under the clean-room procedure of DEC-6428. Every constant names its source where it is defined
//! (REQ-6412 item 3).
//!
//! # Threads and shared state
//!
//! The crate holds no global or static mutable state, and no type in it
//! uses interior mutability, so each of its types is `Send` and `Sync`
//! whenever what it borrows from the caller is (REQ-DOC-001). Every
//! function that changes state takes `&mut self` or a `&mut` argument, so
//! a value shared between threads needs the caller's own lock; a value used
//! by one thread at a time needs nothing. The crate starts no thread.
//!
//! # Memory, ranges and errors
//!
//! Who owns memory is in each signature, and the documentation says only
//! what a signature cannot (REQ-DOC-001): a function borrows what it takes
//! by reference for the call alone and keeps nothing of it afterwards; the
//! crate allocates nothing. The range an argument may take is held by its
//! type where a type can hold it, and otherwise stated in its documentation
//! with the constant that bounds it; a value outside its range is refused
//! with an error, or clamped or ignored as its documentation says, and
//! never panics (REQ-GEN-006). A function that can fail returns `Result`,
//! and each error type is an enumeration whose variants say what each means
//! (REQ-GEN-011).
#![no_std]
// REQ-SEC-015: only the C interface crate may contain unsafe code.
#![forbid(unsafe_code)]
// REQ-SEC-003: everything here reads untrusted input.
#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

#[cfg(test)]
extern crate std;

#[cfg(feature = "analog")]
pub mod analog;
#[cfg(feature = "dx7")]
pub mod dx7;
#[cfg(feature = "sid")]
pub mod sid;

/// Samples a second of everything this crate renders: the rate of an
/// instrument block of the audio profile (REQ-6411 item 1).
pub const SAMPLE_RATE: u32 = 48_000;

/// Samples in one block of 20 milliseconds at [`SAMPLE_RATE`].
pub const BLOCK_SAMPLES: usize = 960;
