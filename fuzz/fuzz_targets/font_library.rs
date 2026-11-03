//! A font library's index (REQ-124's `fonts.txt`, REQ-TST-040): its size
//! and line checks, then every entry the input's lines hold, with each
//! `c` line's sets read and each listed font's address resolved against a
//! library's folder.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::font_library::{self, Entry};
use neoteletext_web::url::Url;

/// The set names the fuzzer accepts: a short list, so that both known and
/// unknown names are reached.
fn known(name: &str) -> bool {
    matches!(name, "G0-Latin" | "G0-Latin-English" | "G2-Latin")
}

fuzz_target!(|data: &[u8]| {
    let Ok(folder) = Url::parse("https://fonts.example/library/") else { return };
    let Ok(entries) = font_library::read(data, known) else { return };
    for entry in entries {
        match entry {
            Ok(Entry::Font(font)) => {
                let _ = core::hint::black_box(font.location.address(&folder));
            }
            Ok(Entry::Conformance(conformance)) => {
                for set in conformance.sets() {
                    core::hint::black_box(set);
                }
            }
            Err(fault) => {
                core::hint::black_box(fault);
            }
        }
    }
});
