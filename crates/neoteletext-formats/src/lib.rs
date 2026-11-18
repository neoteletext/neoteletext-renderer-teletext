//! Streaming readers, writers and the page encoder for teletext input
//! formats (area INP; layer L0 Ingress of `ARCHITECTURE.md`).
//!
//! Every reader implements [`Reader`]: it is fed bytes in pieces of any size
//! and reports what it finds through a callback as [`Item`]s: packets in the
//! internal form of REQ-ARCH-002 with their metadata, page files, transport
//! stream information and diagnostics carrying the byte offset of the problem
//! (REQ-INP-001 and REQ-INP-020). Memory use is fixed by each reader's type
//! and does not depend on the input.
//!
//! - Packet streams: [`t42`] (REQ-INP-002 to 004, always compiled),
//!   `transport` and `pes` (feature `transport`, REQ-INP-006 to 009),
//!   `sliced` (feature `sliced`, REQ-INP-010).
//! - Page files, all producing a [`page::PageFile`]: `tti` (REQ-INP-013),
//!   `ep1` (REQ-INP-015), `edittf` (REQ-INP-016) and `dump` (REQ-INP-017),
//!   each under the feature of its name.
//! - `encode` (feature `pages`): page files to packets (REQ-ARCH-004,
//!   REQ-INP-019).
//! - `links` (feature `pages`): a page's links as a receiver follows them,
//!   and the row 24 it draws (REQ-NAV-011).
//! - [`detect`]: format detection and the [`detect::AutoReader`]
//!   (REQ-INP-005), over the formats compiled in.
//! - [`log`]: a channel's log lines, `LG` in a TTI file, for its author's
//!   console (REQ-INP-026); the TTI reader gives them as [`Item::Log`].
//! - `check` (feature `check`): the TTI page checker behind `neottx check`
//!   (REQ-CLI-006).
//! - `bdf` (feature `bdf`): the strict reader of the 12 by 10 and 12 by 20
//!   BDF fonts `neottx render --font` loads (REQ-FNT-002). Fonts are not teletext input, but they
//!   are untrusted files read into fixed tables like every format here.
//! - `write` (feature `std`): the T42 writer (REQ-INP-018) and, with `tti`,
//!   the TTI writer (REQ-INP-014).
//! - `author` (features `std` and `tti`): the builder for Level 2.5 and 3.5
//!   test pages and the authored test streams (REQ-INP-025, REQ-TST-024).
//! - `write_text` (feature `write-text`): the one encoder of Unicode text
//!   in any language (REQ-2148): text runs at their places on a page
//!   (REQ-2214), and with `std` and `tti` whole pages as TTI files
//!   (REQ-CLI-010).
//!
//! Decision E.61: every format but T42 is a Cargo feature, all on by
//! default. Detection recognises only the formats compiled in, and a format
//! asked for that is not compiled in is reported as
//! [`DiagnosticKind::FormatNotCompiled`].
//!
//! Raw sampled video (REQ-INP-011) is read by `neoteletext-slicer`.
//!
//! Every format is parsed as data into fixed fields; unknown commands are
//! reported and ignored, and nothing read is executed (REQ-SEC-004).
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
// REQ-GEN-006 and REQ-GEN-008: no panicking constructs, as in the core.
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

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(any(test, feature = "std"))]
extern crate std;

pub mod consts;
pub mod item;
#[cfg(any(feature = "tti", feature = "edittf"))]
mod lines;
pub mod text;

// Decision E.61: the readers, the encoder and the checker are features; the
// data types they report (formats, configurations, items, pages) are
// always compiled, so that enabling a feature breaks no code written
// without it.
#[cfg(feature = "transport")]
pub mod pes;
pub mod sliced;
pub mod t42;
pub mod transport;

#[cfg(feature = "dump")]
pub mod dump;
#[cfg(feature = "edittf")]
pub mod edittf;
#[cfg(feature = "ep1")]
pub mod ep1;
pub mod log;
pub mod page;
#[cfg(feature = "tti")]
pub mod tti;

#[cfg(feature = "check")]
pub mod check;

#[cfg(feature = "bdf")]
pub mod bdf;
pub mod detect;
#[cfg(feature = "pages")]
pub mod encode;
#[cfg(feature = "pages")]
pub mod links;

#[cfg(feature = "std")]
pub mod write;

#[cfg(all(feature = "std", feature = "tti"))]
pub mod author;
#[cfg(feature = "picture-text")]
pub mod picture_text;
#[cfg(feature = "write-text")]
pub mod write_text;

#[cfg(test)]
mod testing;

pub use item::{Diagnostic, DiagnosticKind, Item, Reader};

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
    error_trait!(crate::item::DiagnosticKind, crate::log::LogRefused);
    #[cfg(feature = "bdf")]
    error_trait!(crate::bdf::FontError, crate::bdf::convert::ConvertError, crate::bdf::convert::MapError);
    #[cfg(all(feature = "std", feature = "tti"))]
    error_trait!(crate::author::AuthorError);

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::item::DiagnosticKind>();
            is_error::<crate::log::LogRefused>();
            #[cfg(feature = "bdf")]
            {
                is_error::<crate::bdf::FontError>();
                is_error::<crate::bdf::convert::ConvertError>();
                is_error::<crate::bdf::convert::MapError>();
            }
            #[cfg(all(feature = "std", feature = "tti"))]
            is_error::<crate::author::AuthorError>();
            #[cfg(feature = "write-text")]
            {
                is_error::<crate::write_text::WriteError>();
                is_error::<crate::write_text::RunError>();
                is_error::<crate::write_text::RunFault>();
            }
        }
    }
}

#[cfg(test)]
mod robustness;
