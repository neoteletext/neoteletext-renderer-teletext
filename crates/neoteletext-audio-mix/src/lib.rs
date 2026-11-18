//! The software mixer of the audio profile (REQ-AUD-019), for ports with an
//! operating system's audio output: desktops and phones. It behaves as the
//! tellyboy driver does on a microcontroller, so a page sounds alike
//! everywhere: four voices, gains in whole per cent, every change ramped,
//! ducking in EN 300 708 Table 19's steps, and a clamp after the sum.
//!
//! It holds sounds as 16-bit PCM and renders mono 16-bit samples at the
//! output's rate, resampling each voice with a windowed-sinc filter
//! ([`sinc`]). It is pure: no clock, no thread and no device. The port
//! calls [`Mixer::render`] from its audio callback, which never allocates
//! or frees; everything that allocates is a call the port makes elsewhere.
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
//! by reference for the call alone, unless a lifetime ties what it returns
//! to an argument, as an item borrows the input it was read from, and
//! keeps nothing of it afterwards; what the crate allocates, if anything,
//! is said at the head of this page. The range an argument may take is held
//! by its type where a type can hold it (a page address, a magazine, a row
//! number), and otherwise stated in its documentation with the constant
//! that bounds it; a value outside its range is refused with an error, or
//! clamped or ignored as its documentation says, and never panics
//! (REQ-GEN-006). A function that can fail returns `Result`, its
//! `# Errors` section names the variants it returns and when (Clippy's
//! `missing_errors_doc`, a warning in the workspace), and each error type
//! is an enumeration whose variants say what each means (REQ-GEN-011).
#![no_std]
// REQ-SEC-015: only the C interface crate may contain unsafe code.
#![forbid(unsafe_code)]
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

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod sinc;

mod mixer;
mod ramp;
mod sound;

pub use mixer::{
    LOCAL_BLOCK_SAMPLES, Mixer, PEAK_FULL_SCALE, PEAK_PER_LEVEL, PULLED_LOOKAHEAD_SAMPLES, PULLED_RATE_HZ, Pull, Start,
    VOICES, meter_level,
};
pub use sound::{STREAM_RING_MS, STREAM_TARGET_MS, STREAM_TRIM_PPM, SoundKey};
