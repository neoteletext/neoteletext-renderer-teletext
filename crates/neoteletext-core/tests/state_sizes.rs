//! Tests of REQ-147 (REQ-GEN-003): the size of every state type a port allocates,
//! reported for the default build so that `docs/spec/design/porting.md`
//! can publish it, and bounded so that growth is seen.
#![cfg(all(feature = "level15", feature = "render", feature = "viewer"))]
#![allow(missing_docs, clippy::print_stdout, reason = "a test that reports")]

use core::mem::size_of;
use neoteletext_core::acq::{Decoder, EnhancementPacket, EnhancementSubPage, StoredPage};
use neoteletext_core::glyphs::GlyphSet;
use neoteletext_core::level1::Grid;
use neoteletext_core::viewer::Viewer;

/// Each state type, its size in bytes on this target, and the most it may
/// be: the size measured on 3 November 2026 (the `Decoder` and `Viewer` on
/// 4 November) on `x86_64` Linux with every
/// feature and with only `level15`, `render` and `viewer` (the same in both), rounded up to the next 64 bytes, so that a change that grows
/// one is seen and the published table is revisited. The decoder's grew
/// to 1,936 bytes on 4 November 2026 with its configuration's fixed
/// designation and SAA5246A ROM variant (DEC-2430).
#[test]
fn every_state_type_has_a_bounded_size() {
    let sizes: [(&str, usize, usize); 7] = [
        ("StoredPage", size_of::<StoredPage>(), 2_688),
        ("EnhancementSubPage", size_of::<EnhancementSubPage>(), 128),
        ("EnhancementPacket", size_of::<EnhancementPacket>(), 64),
        ("Decoder", size_of::<Decoder<'static>>(), 1_984),
        ("Grid", size_of::<Grid>(), 16_128),
        ("Viewer", size_of::<Viewer>(), 320),
        ("GlyphSet", size_of::<GlyphSet<'static>>(), 1_472),
    ];
    for (name, size, _) in sizes {
        println!("{name:<20} {size:>6} bytes");
    }
    for (name, size, bound) in sizes {
        assert!(size <= bound, "{name} is {size} bytes, over its bound of {bound}: update porting.md and the bound");
    }
}
