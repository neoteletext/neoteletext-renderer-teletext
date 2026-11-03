//! `neottx slice` (REQ-CLI-001): what cannot be sliced is reported.
#![cfg(feature = "slice")]
#![allow(
    clippy::unwrap_used,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good"
)]

mod common;
use common::{folder, neottx};

// Verifies REQ-184: 5,000 bytes are two lines of 2,048 and 904 left over, a
// partial line that starts at byte 4,096.
#[test]
fn a_partial_line_is_reported_with_its_offset() {
    let folder = folder("slice-partial-line");
    let input = folder.join("r.raw");
    std::fs::write(&input, [0_u8; 5000]).unwrap();
    let output = folder.join("r.t42");
    let run = neottx(&["slice", input.to_str().unwrap(), "-o", output.to_str().unwrap(), "-v"]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(said.contains("input byte 4096: a partial line of 904 bytes"), "{said}");
}

// Verifies REQ-175: the geometry may be given in full on the command line,
// over a preset: lines of 1,500 one-byte samples make the 5,000 bytes three
// lines and a partial line at byte 4,500.
#[test]
fn a_geometry_given_on_the_command_line_is_used() {
    let folder = folder("slice-geometry");
    let input = folder.join("r.raw");
    std::fs::write(&input, [0_u8; 5000]).unwrap();
    let output = folder.join("r.t42");
    let path = |p: &std::path::Path| p.to_str().unwrap().to_owned();
    let run = neottx(&[
        "slice",
        &path(&input),
        "-o",
        &path(&output),
        "-v",
        "--sample-rate",
        "27000000",
        "--samples-per-line",
        "1500",
        "--sample-size",
        "8",
        "--lines-per-field",
        "2",
        "--used-lines",
        "0-1",
        "--first-lines",
        "7,320",
    ]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let said = String::from_utf8_lossy(&run.stderr);
    assert!(said.contains("input byte 4500: a partial line of 500 bytes"), "{said}");
}

// Verifies REQ-175: a geometry the reader cannot use is refused with the
// usage status.
#[test]
fn a_geometry_that_cannot_be_used_is_refused() {
    let folder = folder("slice-bad-geometry");
    let input = folder.join("r.raw");
    std::fs::write(&input, [0_u8; 100]).unwrap();
    let output = folder.join("r.t42");
    for bad in [["--samples-per-line", "0"], ["--used-lines", "5-400"], ["--sample-size", "12"]] {
        let run = neottx(&["slice", input.to_str().unwrap(), "-o", output.to_str().unwrap(), bad[0], bad[1]]);
        assert_eq!(run.status.code(), Some(2), "{bad:?}: {}", String::from_utf8_lossy(&run.stderr));
    }
}
