//! The program `neottx-picture` (REQ-2337).
#![allow(clippy::unwrap_used, reason = "tests")]

use std::path::PathBuf;
use std::process::Command;

use neoteletext_picture::{Level, Options, convert};

const PROGRAM: &str = env!("CARGO_BIN_EXE_neottx-picture");

/// A scratch folder of this test's own under Cargo's target directory.
fn scratch(name: &str) -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// A BMP of 64 by 48 pixels, written by hand so the test needs no encoder:
/// a 24-bit bottom-up bitmap with rows padded to four bytes.
fn bmp() -> Vec<u8> {
    let (width, height) = (64u32, 48u32);
    let row = (width * 3).next_multiple_of(4);
    let size = 54 + row * height;
    let mut bytes = b"BM".to_vec();
    for value in [size, 0, 54, 40, width, height] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(24u16.to_le_bytes());
    for value in [0u32, row * height, 2835, 2835, 0, 0] {
        bytes.extend(value.to_le_bytes());
    }
    for y in 0..height {
        for x in 0..width {
            bytes.extend([(x * 4) as u8, (y * 5) as u8, ((x + y) * 2) as u8]);
        }
    }
    bytes
}

/// REQ-2337: the program writes the library's TTI and T42 for the options
/// given.
#[test]
fn the_program_writes_the_librarys_page() {
    let folder = scratch("program");
    let (picture, tti, t42) = (folder.join("p.bmp"), folder.join("p.tti"), folder.join("p.t42"));
    std::fs::write(&picture, bmp()).unwrap();
    let status = Command::new(PROGRAM)
        .args([picture.to_str().unwrap(), tti.to_str().unwrap(), "--t42", t42.to_str().unwrap(), "--level", "1.5"])
        .status()
        .unwrap();
    assert!(status.success());
    let page = convert(&bmp(), &Options { level: Level::OnePointFive, ..Options::default() }).unwrap();
    assert_eq!(std::fs::read_to_string(&tti).unwrap(), page.tti);
    assert_eq!(std::fs::read(&t42).unwrap(), page.t42().unwrap());
}

/// REQ-2337: an unknown option, a bad value and a missing argument fail.
#[test]
fn bad_arguments_fail() {
    for arguments in [&["a.png", "b.tti", "--sharpen"][..], &["a.png", "b.tti", "--fit", "squash"], &["a.png"]] {
        let output = Command::new(PROGRAM).args(arguments).output().unwrap();
        assert!(!output.status.success(), "{arguments:?}");
        assert!(!output.stderr.is_empty(), "{arguments:?}");
    }
}
