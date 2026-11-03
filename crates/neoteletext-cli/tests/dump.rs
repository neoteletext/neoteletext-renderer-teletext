//! Screen dumps through `neottx` (REQ-181, REQ-INP-017).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "a test")]

mod common;

use common::{folder, neottx};

// Verifies REQ-181: `--dump-repeated-rows` leaves out the row a BBC
// Microcomputer dump repeats under a double-height row; without it the
// repeat is read as a row of its own.
#[test]
fn the_repeated_row_option_leaves_out_the_lower_row() {
    let folder = folder("dump-repeated-rows");
    let file = folder.join("page.dump");
    let mut data = vec![0xA0_u8; 1000];
    // Row 3: Double Height (0x0D, top bit set) then "BIG"; row 4 repeats it.
    for row in [3, 4] {
        data[row * 40..row * 40 + 4].copy_from_slice(b"\x8dBIG");
    }
    data[5 * 40..5 * 40 + 4].copy_from_slice(b"NEXT");
    std::fs::write(&file, &data).unwrap();
    let converted = |name: &str, extra: &[&str]| {
        let output = folder.join(name);
        let mut arguments =
            vec!["convert", file.to_str().unwrap(), "--format", "dump", "--to", "tti", "-o", output.to_str().unwrap()];
        arguments.extend_from_slice(extra);
        let run = neottx(&arguments);
        assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
        String::from_utf8_lossy(&std::fs::read(output).unwrap()).into_owned()
    };
    let with = converted("with.tti", &["--dump-repeated-rows"]);
    let without = converted("without.tti", &[]);
    assert!(with.contains("OL,3,") && !with.contains("OL,4,") && with.contains("OL,5,"), "{with}");
    assert!(without.contains("OL,4,"), "{without}");
}
