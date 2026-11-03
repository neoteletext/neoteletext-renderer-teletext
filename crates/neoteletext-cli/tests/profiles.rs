//! The decoder profiles' character set settings through `neottx`
//! (area PRF).
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;

use common::{folder, neottx};
use std::fs;

/// A TTI page 100 whose row 1 holds `@`, code 4/0, which each national
/// option of Table 32 that changes it draws differently; C12 to C14 are 000.
fn page_with_code_4_0(name: &str) -> String {
    let folder = folder(name);
    let file = folder.join("page.tti");
    fs::write(&file, "PN,10000\r\nOL,1,AT@SIGN\r\n").unwrap();
    file.to_str().unwrap().to_owned()
}

fn text(arguments: &[&str]) -> String {
    let run = neottx(arguments);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    String::from_utf8(run.stdout).unwrap()
}

// Verifies REQ-369: `--variant` chooses the SAA5246A's ROM variant, whose
// default language takes a national option it lacks: /I has no set for
// C12 to C14 000 and gives Turkish, where 4/0 is İ; the profile's own /E
// gives English, where it is @. A letter that names no variant is refused.
#[test]
fn the_variant_option_chooses_the_saa5246a_rom() {
    let file = page_with_code_4_0("profiles-variant");
    let west = text(&["text", &file, "100", "--profile", "saa5246a"]);
    assert!(west.contains("AT@SIGN"), "{west}");
    let turkish = text(&["text", &file, "100", "--profile", "saa5246a", "--variant", "I"]);
    assert!(turkish.contains("ATİSIGN"), "{turkish}");
    let refused = neottx(&["text", &file, "100", "--profile", "saa5246a", "--variant", "Q"]);
    assert_eq!(refused.status.code(), Some(2));
}
