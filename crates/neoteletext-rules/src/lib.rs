//! The page rule table (neoteletext-common `docs/page-rules.md`,
//! requirements `RULE-n`; DEC-6522): a bounded, declarative list of rules,
//! each binding one event to a short list of local actions, which a
//! receiver evaluates sixty times a second. It is event binding, never
//! executable content (RULE-1): no loop, no jump, no rule that runs
//! another within a tick, and no arithmetic beyond the saturating addition
//! of a constant to a declared variable.
//!
//! - [`limits`]: every bound (RULE-57), each defined once.
//! - [`table`]: the decoded table in fixed arrays, [`table::decode`],
//!   [`table::encode`] and [`table::check`] of the RULES record's body
//!   (RULE-41 to RULE-45, RULE-70), what a table reads and names
//!   ([`table::Index`]: the watch list of RULE-33 and the interest mask of
//!   RULE-52), and [`table::default_send_all`] (RULE-53).
//! - [`animate`]: the animation operations a rule's action and an
//!   `ANIMATE` record share, the animation state they fold into, and frame
//!   cycling (ANIM-6, ANIM-14).
//! - [`evaluator`]: the rule state and the tick, in the fixed order of
//!   RULE-5, with the [`evaluator::World`] it reads and writes through.
//! - [`state`]: the `PAGE_STATE` payload (RULE-47).
//! - [`clock`]: which ticks of the presentation clock are due (RULE-4).
//!
//! The crate is `no_std`, allocates nothing, uses integers only and has
//! no dependency (RULE-56), so the server checks a table with the code
//! every receiver decodes it with. Key codes, family codes and parameter
//! numbers are plain numbers to it: the callers that know them check what
//! it cannot.
//!
//! # Threads and shared state
//!
//! The crate holds no global or static mutable state, and no type in it
//! uses interior mutability, so each of its types is `Send` and `Sync`.
//! Every function that changes state takes `&mut self` or a `&mut`
//! argument. The crate starts no thread and reads no clock.
//!
//! # Memory, ranges and errors
//!
//! Every structure is a fixed array sized by a limit of RULE-57: no table
//! can make the crate allocate (RULE-71). A decoded [`table::Table`] is
//! about 80 kilobytes and an [`evaluator::Evaluator`] holds one, so a
//! caller with a small stack keeps them in static or heap memory of its
//! own. A value outside its range is refused with a [`Fault`] when a table
//! is decoded, never when it runs; nothing panics on any input
//! (REQ-GEN-006).
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

pub mod animate;
mod bounded;
pub mod clock;
pub mod evaluator;
pub mod limits;
pub mod state;
pub mod table;
mod wire;

pub use bounded::Bounded;
pub use evaluator::{Evaluator, World};
pub use table::{Fault, Table, check, decode, default_send_all, encode};

/// REQ-GEN-011: the error type implements `core::error::Error`. Its
/// `Display` text is its `Debug` text, and both need `core::fmt`, which
/// REQ-GEN-016 keeps behind this optional feature so that a small build
/// leaves it out.
#[cfg(feature = "error-trait")]
mod error_trait {
    impl core::fmt::Display for crate::Fault {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            core::fmt::Debug::fmt(self, formatter)
        }
    }
    impl core::error::Error for crate::Fault {}
}
