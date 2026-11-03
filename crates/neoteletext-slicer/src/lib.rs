//! The neoteletext software data slicer (area SLC, layer L0): sampled
//! television lines in, 42-byte teletext packets out.
//!
//! - [`slicer`]: the slicer itself, running the line slicer machine of
//!   REQ-FSM-002 ([`fsm`]) once per line.
//! - [`raw`]: the streaming reader of raw sampled video files (REQ-INP-011),
//!   which splits a file into lines and feeds the slicer.
//! - [`generator`]: the synthetic test signal generator (REQ-TST-060).
//!
//! `no_std` and no allocation. The slicer uses floating point, as
//! REQ-GEN-016 allows; its fixed-point form (REQ-SLC-015) is Phase 2.
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
// REQ-GEN-006: no panicking constructs.
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

pub mod consts;
pub mod fsm;
pub mod generator;
pub mod math;
pub mod raw;
pub mod slicer;

pub use raw::{CutLine, GeometryError, Preset, RawGeometry, RawReader};
pub use slicer::{Config, Counters, Line, Rejection, SampleFormat, Sliced, Slicer};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

/// REQ-GEN-011: the error types implement `core::error::Error`. Their
/// `Display` text is their `Debug` text, and both need `core::fmt`, which
/// REQ-GEN-016 keeps behind this optional feature so that a small build
/// leaves it out.
#[cfg(feature = "error-trait")]
mod error_trait {
    macro_rules! error_trait {
        ($($error:ty),+ $(,)?) => {$(
            impl core::fmt::Display for $error {
                fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    core::fmt::Debug::fmt(self, formatter)
                }
            }
            impl core::error::Error for $error {}
        )+};
    }
    error_trait!(crate::raw::GeometryError, crate::Rejection);

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::raw::GeometryError>();
            is_error::<crate::Rejection>();
        }
    }
}
