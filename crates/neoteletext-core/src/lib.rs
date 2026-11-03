//! The neoteletext decoder core: World System Teletext (ETSI EN 300 706)
//! Presentation Levels 1.0, 1.5, 2.5 and 3.5.
//!
//! `no_std`, no allocation, no input or output and no clock (REQ-GEN-002).
//! Every piece of state lives in memory the caller provides (REQ-GEN-003),
//! so there are no mutable or lazily initialised statics and several
//! decoders can run side by side (REQ-GEN-004).
//!
//! These are the core modules of REQ-ARCH-001. The core depends on no
//! other crate, so no core module can depend on an optional module. The
//! layers and their allowed dependencies are described in `ARCHITECTURE.md`:
//!
//! - Foundation: [`consts`], [`ecc`], [`limits`], [`types`];
//! - L1 packet decoding: [`packet`];
//! - L2 acquisition: [`fsm`], [`acq`], [`service`];
//! - L3 presentation model: [`charset`], [`level1`], [`compose`], [`level15`], and for
//!   Levels 2.5 and 3.5 [`colour`], [`objects`] and [`enhance`];
//! - L4 rendering: [`glyphs`], [`render`];
//! - L6 interaction: [`viewer`], [`nav`], [`controls`];
//! - L8 platform interface: [`port`];
//! - cross-cutting: [`config`], [`policy`], [`trace`].
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
// REQ-GEN-006: no panicking constructs in the core.
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

pub mod consts;
pub mod ecc;
pub mod limits;
pub mod types;

pub mod packet;

pub mod acq;
pub mod fsm;
pub mod service;

pub mod charset;
pub mod colour;
pub mod compose;
#[cfg(feature = "level25")]
pub mod enhance;
pub mod level1;
#[cfg(feature = "level15")]
pub mod level15;
pub mod objects;

#[cfg(feature = "render")]
pub mod glyphs;
#[cfg(feature = "extended-sets")]
pub mod joining;
#[cfg(feature = "render")]
pub mod render;

pub mod controls;
#[cfg(feature = "viewer")]
pub mod nav;
#[cfg(feature = "viewer")]
pub mod viewer;

pub mod port;

pub mod config;
pub mod policy;
pub mod trace;

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
    error_trait!(crate::packet::PacketError, crate::objects::Absent);
    #[cfg(feature = "render")]
    error_trait!(crate::render::RenderError, crate::glyphs::FontTableError);

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::packet::PacketError>();
            is_error::<crate::objects::Absent>();
            #[cfg(feature = "render")]
            {
                is_error::<crate::render::RenderError>();
                is_error::<crate::glyphs::FontTableError>();
            }
        }
    }
}
