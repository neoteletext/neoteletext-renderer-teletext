//! The neoteletext web user agent (layer L7): a deliberately minimal,
//! sans-IO browser for teletext pages served over the web (area WEB,
//! decision E.34), and the reader of the zxnet live feed (REQ-INP-022).
//!
//! `no_std`, no allocation and no input or output (REQ-WEB-001): every
//! protocol is a state machine fed with received bytes, the current time
//! and user requests, producing bytes to send, timers, page bytes (TTI or
//! T42) for the format readers, and events. The port supplies the network
//! transport, including encryption, through [`port::Transport`]. There is
//! no socket and no cryptography here.
//!
//! Everything received is data (REQ-SEC-004): pages go to the format
//! readers, and descriptors and pushed messages are parsed into fixed
//! fields. Nothing received is executed or loaded as code.
//!
//! - Foundation: [`consts`], [`bytes`], [`base64`], [`sha1`], [`json`];
//! - addresses and origins: [`url`];
//! - protocols: [`http`], [`ws`], and [`viewer_state`]: the bytes a viewer
//!   key signs and the key identifier, which replace cookies (DEC-6471);
//! - content: [`channel`], [`contents`], [`font_library`] (a font
//!   library's `fonts.txt` and a font's checksum), [`subpages`] (a page's list
//!   of sub-pages, and with `std` a page written as a channel's files),
//!   [`descriptor`], [`discovery`] (the pointers a web page gives to its
//!   channel), and `small_page` (feature `alloc`): what a web browser
//!   is shown of a channel;
//! - the machines: [`agent`] (the [`agent::UserAgent`]) and [`zxnet`]
//!   (the [`zxnet::ZxnetFeed`]);
//! - the web half of the platform interface (layer L8): [`port`].
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
// REQ-GEN-006: no panicking constructs in library code.
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

pub mod base64;
pub mod bytes;
pub mod json;
pub mod sha1;
pub mod sha256;

pub mod proxy;
pub mod url;
pub mod user_agent;

pub mod viewer_state;

pub mod http;
pub mod ws;

pub mod channel;
pub mod contents;
pub mod descriptor;
pub mod discovery;
pub mod font_library;
pub mod map;
pub mod records;
#[cfg(feature = "alloc")]
pub mod small_page;
pub mod subpages;

pub mod agent;
pub mod zxnet;

pub mod port;
pub mod wire_vectors;

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
    error_trait!(
        crate::bytes::Overflow,
        crate::descriptor::DescriptorError,
        crate::http::HttpError,
        crate::ws::WsError,
        crate::agent::Failure,
        crate::url::UrlError,
        crate::zxnet::ChannelError,
        crate::contents::ContentsError,
        crate::contents::Fault,
        crate::contents::LineFault,
        crate::subpages::SubPagesError,
        crate::subpages::Fault,
        crate::subpages::LineFault,
        crate::json::JsonError,
        crate::map::MapError,
        crate::proxy::ProxyError,
        crate::records::RecordFault,
        crate::viewer_state::StateError,
        crate::records::index::IndexError,
        crate::user_agent::UserAgentError,
        crate::zxnet::Malformed,
    );
    #[cfg(feature = "std")]
    error_trait!(crate::subpages::LayoutError);

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::bytes::Overflow>();
            is_error::<crate::descriptor::DescriptorError>();
            is_error::<crate::http::HttpError>();
            is_error::<crate::ws::WsError>();
            is_error::<crate::agent::Failure>();
            is_error::<crate::url::UrlError>();
            is_error::<crate::zxnet::ChannelError>();
            is_error::<crate::contents::ContentsError>();
            is_error::<crate::contents::Fault>();
            is_error::<crate::contents::LineFault>();
            is_error::<crate::subpages::SubPagesError>();
            is_error::<crate::subpages::Fault>();
            is_error::<crate::subpages::LineFault>();
            is_error::<crate::json::JsonError>();
            is_error::<crate::map::MapError>();
            is_error::<crate::proxy::ProxyError>();
            is_error::<crate::records::RecordFault>();
            is_error::<crate::viewer_state::StateError>();
            is_error::<crate::records::index::IndexError>();
            is_error::<crate::user_agent::UserAgentError>();
            is_error::<crate::zxnet::Malformed>();
            #[cfg(feature = "std")]
            {
                is_error::<crate::proxy::TunnelError>();
                is_error::<crate::proxy::ProxyParseError>();
                is_error::<crate::subpages::LayoutError>();
            }
        }
    }
}
