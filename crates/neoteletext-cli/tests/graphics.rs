//! `neottx show` drawing a page with each terminal graphics protocol when
//! one is forced (REQ-382, REQ-RND-029).
#![allow(clippy::unwrap_used, reason = "a test")]

mod common;

use common::{folder, neottx};

/// What `neottx show` writes for page 100 of a small TTI file with
/// `--graphics choice`.
fn shown(name: &str, choice: &str) -> Vec<u8> {
    let folder = folder(name);
    let file = folder.join("page.tti");
    std::fs::write(&file, "PN,10000\r\nOL,1,\x01RED \x07WHITE\r\n").unwrap();
    let run = neottx(&["show", file.to_str().unwrap(), "100", "--graphics", choice, "--scale", "1", "--no-sandbox"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    run.stdout
}

// Verifies REQ-382: forced, kitty draws the page as one PNG image sent by
// the kitty graphics protocol, its last chunk ending the transmission.
#[test]
fn kitty_draws_the_page_as_a_png_image() {
    let out = shown("graphics-kitty", "kitty");
    assert!(out.starts_with(b"\x1b_Ga=T,f=100,i=1,q=2,m="), "{:?}", &out[..40]);
    // The PNG signature, in base64, after the first chunk's keys.
    assert_eq!(&out[24..35], b";iVBORw0KGg");
    let text = String::from_utf8(out).unwrap();
    assert!(text.ends_with("\x1b\\\n"));
    assert_eq!(text.matches("m=0;").count(), 1, "one last chunk");
}

// Verifies REQ-382 and DEC-561: forced, Sixel draws the 480 by 500 page with
// the palette indices as colour registers, red (index 1) and white
// (index 7) among them, and ends the Device Control String.
#[test]
fn sixel_draws_the_page_with_its_palette_registers() {
    let out = shown("graphics-sixel", "sixel");
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("\x1bP0;1;0q\"1;1;480;500#0;2;0;0;0"), "{:?}", &text[..40]);
    assert!(text.contains("#1;2;100;0;0") && text.contains("#7;2;100;100;100"));
    assert!(text.ends_with("\x1b\\\n"));
}
