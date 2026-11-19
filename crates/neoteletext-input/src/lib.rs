//! The input system (area INT, DOC-1781 and DOC-1782 of `neoteletext-common/PROTOCOL.html`, protocol
//! version 3): a receiver connected to a channel's push WebSocket sends the
//! viewer's input as one stream of UTF-8 text and small events, and the
//! channel answers in teletext, with records that place a cursor and
//! acknowledge input. There are no input modes: everything is sent
//! (DOC-1922, decision E.125).
//!
//! - [`event`]: the input stream, receiver to channel, over [`wire`]'s
//!   primitives, with [`keys`]' functional codes and [`names`]' W3C table.
//! - [`caps`]: the capabilities a receiver offers and a channel agrees.
//! - [`snapshot`]: the request a receiver sends for one page as a stranger
//!   sees it, and its test of WELCOME (REQ-6879).
//! - [`cursor`] and [`control`]: the channel's records, of the audio
//!   profile's frame on the same independent data lines.
//! - [`session`]: a receiver's side of one session, the rate limit with it,
//!   and the messages kept to send again when a lost connection's session
//!   is resumed (REQ-RCH-013).
//! - [`state`]: viewer state's messages from the receiver (DEC-6471,
//!   VS-204), `STATE_PROOF`, `STATE_NONE` and `STATE_OFFER`, which are extension
//!   events of the input stream but no input.
//! - [`rules`]: the page rule table's messages from the receiver
//!   (DEC-6522, RULE-46 to RULE-48), `RULE_EVENT`, `PAGE_STATE` and
//!   `RULES_FAULT`. Under protocol version 10 a viewer's input reaches a
//!   channel only inside a `RULE_EVENT`.
//! - [`pace`]: the pointer's moves at 60 a second, one slot a contact
//!   (REQ-RCH-031 to REQ-RCH-033); [`contacts`]: the numbers of a touch
//!   screen's contacts (REQ-RCH-043).
//!
//! Everything here is `no_std` and allocation-free, so a microcontroller
//! can be a receiver or a channel.
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

pub mod caps;
mod carriage;
pub mod contacts;
pub mod control;
pub mod cursor;
pub mod digits;
pub mod event;
pub mod held;
pub mod keys;
mod limits;
pub mod names;
pub mod pace;
pub mod rules;
pub mod session;
pub mod snapshot;
pub mod state;
pub mod wire;

pub use carriage::is_input_packet;
pub use limits::*;

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
        crate::session::WelcomeError,
        crate::session::SendError,
        crate::control::ControlError,
        crate::cursor::CursorError,
        crate::wire::WireError,
        crate::event::InputError,
    );

    #[cfg(test)]
    mod tests {
        const fn is_error<T: core::error::Error>() {}

        // REQ-155: every error type of the crate is an error.
        #[test]
        fn every_error_type_is_an_error() {
            is_error::<crate::session::WelcomeError>();
            is_error::<crate::session::SendError>();
            is_error::<crate::control::ControlError>();
            is_error::<crate::cursor::CursorError>();
            is_error::<crate::wire::WireError>();
            is_error::<crate::event::InputError>();
        }
    }
}
