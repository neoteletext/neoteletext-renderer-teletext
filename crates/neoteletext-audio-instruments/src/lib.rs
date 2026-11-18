//! The instruments of the teletext audio profile (area AUD, DEC-6410):
//! synth6581, synth7 and the analogue synthesizers, each rendered from its own
//! input to 16-bit mono samples at 48,000 Hz (REQ-6412).
//!
//! An instrument is a codec of the profile, and a block is 20 milliseconds
//! of the instrument's input: a list of timed MIDI 1.0 events (module
//! [`midi`], `neoteletext-common/docs/audio/midi-architecture.md`), which
//! the receiver plays into each family through [`midi::Instrument`]. This
//! crate holds the instruments and that MIDI layer alone. It reads no
//! record and drives no hardware; the audio receiver gives it blocks and a
//! port's voice sink takes its samples.
//!
//! Everything here is `no_std`, integer-only and allocation-free; every
//! buffer has a size fixed by its type. No input makes a block cost more
//! than the bound of its instrument (REQ-6412 item 7).
//!
//! Behind the feature `analog`, which is not on by default, are the
//! building blocks the analogue synthesizers of DEC-6429 are to be made
//! from (module `analog`): oscillators, filters, envelopes, a chorus and a
//! voice allocator. They are no codec yet. Behind the feature `synth106`, also
//! not on by default, is the first instrument made from them:
//! synth106 and synth60 (module `synth106`). Behind the feature `synthd`, not on
//! by default either, is synthD (module `synthd`).
//! Behind the feature `synth53`, also not on by default, is synth53 (module `synth53`).
//! Behind the feature `synth8`, not on by default either, is synth8 (module `synth8`);
//! behind the feature `interface` alone, its interface module
//! (`synth8::interface`) and its patch (`synth8::patch`) without its synthesis.
//! Behind the feature `synth80`, not on by default either, is synth80 (module `synth80`).
//!
//! # Whose code this is
//!
//! Only permissively licensed code is the basis of anything here
//! (DEC-6420). synth6581 is written from the chip's public
//! documentation, with no code and no table from any emulator (DEC-6418).
//! synth7's parameter layer, under `synth7::msfa`, is translated from
//! Google's music-synthesizer-for-android and stays under Apache-2.0, each
//! file carrying its notice; synth7's sound kernel and rack are this
//! crate's own. The analogue building blocks are written from public
//! documents alone, under the clean-room procedure of DEC-6428. Every constant names its source where it is defined
//! (REQ-6412 item 3). Which instrument each
//! synthesizer models is said once, in
//! `neoteletext-common/docs/audio/synthesizers.md`.
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
// REQ-GEN-016: nothing that ships computes in floating point, so that the
// crate runs the same on a processor with no floating-point unit. The
// tests inside the crate compare against f64 and are left out.
#![cfg_attr(not(test), deny(clippy::float_arithmetic, clippy::float_cmp))]

#[cfg(test)]
extern crate std;

#[cfg(feature = "analog")]
pub mod analog;
pub mod midi;
pub mod output;
#[cfg(feature = "synth106")]
pub mod synth106;
#[cfg(feature = "synth53")]
pub mod synth53;
#[cfg(feature = "synth6581")]
pub mod synth6581;
#[cfg(feature = "synth7")]
pub mod synth7;
#[cfg(any(feature = "synth8", feature = "interface"))]
pub mod synth8;
#[cfg(feature = "synth80")]
pub mod synth80;
#[cfg(feature = "synthd")]
pub mod synthd;

/// Samples a second of everything this crate renders: the rate of an
/// instrument block of the audio profile (REQ-6411 item 1).
pub const SAMPLE_RATE: u32 = 48_000;

/// The level every instrument plays its reference note at, in tenths of a
/// decibel of full scale, RMS: -23.0 dBFS.
///
/// An instrument's reference note is one sustained note of its plainest
/// full-level sound with a sawtooth's harmonics: one voice's sawtooth
/// through an open filter with the amplifier held full on an analogue
/// instrument, one voice's sawtooth at full volume past the filter on
/// synth6581, and one carrier at full output level, a sine, on synth7. Each
/// instrument's last gain is set so that the note stands at this level,
/// so that the same score is as loud on one instrument as on another
/// (`tests/levels.rs`). synth8580 alone stands below it, by the 2.2 dB by
/// which that chip is quieter than synth6581.
///
/// Every note an instrument can sound, struck together, is then more
/// than a sample holds on some of them: synth7's sixteen sines in step
/// are 24.1 dB above one and would peak 4.1 dB above full scale. Nothing
/// is left under the level for that. Each instrument's last sample goes
/// through [`output::limit`], which changes nothing up to 2.5 dB below
/// full scale and bends what is above under it, so no output wraps and
/// none is cut flat.
///
/// Source: own choice, from what the instruments measured when each
/// still had a level of its own (dBFS RMS of that note: synthD -17.3,
/// synth6581 -21.6, synth106 and synth60 -22.9, synth53 -22.9 and synth52 -23.3,
/// synth80 -25.2, synth8 -25.4, synth7 -27.1): the level synth106, synth60 and
/// synth53 had, which the owner had heard and found good, with
/// synth8 a little under it. A level 4 dB lower, the highest at
/// which synth7's whole polyphony in step stays under full scale with no
/// limiter, was tried first and made the whole set quiet.
pub const REFERENCE_LEVEL_TENTHS: i32 = -230;

/// Samples in one block of 20 milliseconds at [`SAMPLE_RATE`].
pub const BLOCK_SAMPLES: usize = 960;
