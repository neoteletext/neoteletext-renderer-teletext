//! `neottx view`'s keys on a channel of files, through a pseudo-terminal
//! and the fetcher process: what each key shows and what it asks the
//! server for (REQ-496, REQ-509). Only a build with live feeds and the
//! viewer has them (decision E.61).
#![cfg(all(feature = "live", feature = "view"))]

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

/// A channel of three pages, 101's index link leading to 102, and an
/// alias of 400 to another channel's address.
fn channel(root: &Path) {
    fs::write(
        root.join("channel.ttx"),
        r#"{"name":"Keys","initial_page":"100","aliases":{"400":"https://sport.example/400.t42"}}"#,
    )
    .unwrap();
    for page in ["100", "101", "102"] {
        let index = if page == "101" { "LN,I,102,\n" } else { "" };
        fs::write(root.join(format!("{page}.tti")), format!("PN,{page}00\nOL,1,PAGE {page}\n{index}")).unwrap();
    }
}

// Verifies REQ-496 through a live source: next page, Index and Home each
// show their page, and the first two fetch it from the channel's folder,
// once; REQ-509: a typed number with an alias to another channel's address
// is said in row 24.
#[test]
fn next_page_index_and_home_reach_the_server_through_a_live_source() {
    let root = common::folder("viewer-keys");
    channel(&root);
    let (port, asked) = common::serve_folder(root);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/web-server/drive_keys.py");
    let output = Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_neottx"))
        .arg(port.to_string())
        .output()
        .expect("python3 runs the driver");
    let asked = asked.lock().unwrap();
    assert!(output.status.success(), "{}: {asked:?}", String::from_utf8_lossy(&output.stdout));
    assert!(asked.iter().any(|target| target == "/ch/100.tti"), "{asked:?}");
    for page in ["/ch/101.tti", "/ch/102.tti"] {
        assert_eq!(asked.iter().filter(|target| *target == page).count(), 1, "{page}: {asked:?}");
    }
}
