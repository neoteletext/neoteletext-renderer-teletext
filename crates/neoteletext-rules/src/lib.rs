//! The page rule table (neoteletext-common `docs/page-rules.md`,
//! requirements `RULE-n`; DEC-6522, DEC-6587): a bounded, declarative
//! list of rules, each binding events to one group of protocol commands,
//! which a receiver evaluates sixty times a second. It is event binding,
//! never executable content (RULE-1): no loop, no jump, no rule that runs
//! another within a tick, no computed address, and no arithmetic beyond
//! the saturating addition and subtraction of 32-bit integers in the
//! variable commands and in a command's value.
//!
//! - [`limits`]: every bound (RULE-57, REQ-6608), each defined once.
//! - [`table`]: the decoded table in fixed arrays, [`table::decode`],
//!   [`table::encode`] and [`table::check`] of the table's image (RULE-41
//!   to RULE-45, RULE-70), what a table reads and names
//!   ([`table::Index`]: the watch list of RULE-33, the interest mask of
//!   RULE-52 and the drawings its `painted` triggers name, DEC-7031), and
//!   [`table::default_send_all`] (RULE-53).
//! - [`command`]: the bodies of the records of types 28 to 33, which a
//!   group holds and the server may send directly (RULE-21, RULE-22,
//!   RULE-99, RULE-100, RULE-102, RULE-103), and the bindings of a
//!   command's fields (RULE-106).
//! - [`animate`]: the animation operations and the `ANIMATE` record's
//!   body, the animation state they fold into, and frame cycling (ANIM-6,
//!   ANIM-14).
//! - [`evaluator`]: the rule state, the arrival step of RULE-96 and the
//!   tick, in the fixed order of RULE-5, with the [`evaluator::World`] it
//!   reads and writes through. Beside input, cues and the server's records
//!   it takes the end of a painting ([`evaluator::Event::Painted`]), which
//!   fires the `painted` trigger, code 22 (DEC-7031 item 4).
//! - [`state`]: the `PAGE_STATE` payload (RULE-47).
//! - [`names`]: the words the program protocol writes each code as
//!   (page-rules.md section 7), for the callers that read its JSON.
//! - [`clock`]: which ticks of the presentation clock are due (RULE-4).
//!
//! The crate is `no_std`, allocates nothing, uses integers only and has
//! no dependency (RULE-56), so the server checks a table with the code
//! every receiver decodes it with. Key codes, family codes and parameter
//! numbers are plain numbers to it, and the bodies of `CONTROL`, `SPRITES`
//! and `NAPLPS_OBJECTS` records are read through the [`command::BodyCheck`]
//! its caller passes: the callers that know them check what it cannot.
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
//! about 110 kilobytes and an [`evaluator::Evaluator`] holds one in about
//! 160, so a caller with a small stack keeps them in static or heap
//! memory of its own. A value outside its range is refused with a
//! [`Fault`] when a table is decoded, never when it runs; nothing panics
//! on any input (REQ-GEN-006).
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
pub mod command;
pub mod evaluator;
pub mod limits;
pub mod names;
pub mod state;
pub mod table;
#[cfg(test)]
mod testing;
mod wire;

pub use bounded::Bounded;
pub use command::{BodyCheck, BodyFacts};
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
