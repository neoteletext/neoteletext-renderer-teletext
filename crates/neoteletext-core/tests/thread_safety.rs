//! The core's state can move between threads and be shared read-only
//! (REQ-DOC-001, the thread-safety the crate's documentation states): its
//! main types are `Send` and `Sync`. A type that gained interior
//! mutability or a raw pointer would stop this file compiling.

use neoteletext_core::acq::{Decoder, PageStore};
use neoteletext_core::config::Config;
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::Grid;
use neoteletext_core::viewer::Viewer;

/// Compiles only for a type that is both `Send` and `Sync`.
const fn send_and_sync<T: Send + Sync>() {}

// REQ-99 (REQ-DOC-001): the thread-safety the crate's documentation states.
#[test]
fn the_main_types_are_send_and_sync() {
    send_and_sync::<Decoder<'static>>();
    send_and_sync::<PageStore<'static>>();
    send_and_sync::<Config>();
    send_and_sync::<GlyphSet<'static>>();
    send_and_sync::<Grid>();
    send_and_sync::<Viewer>();
}
