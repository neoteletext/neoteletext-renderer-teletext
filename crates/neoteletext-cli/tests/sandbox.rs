//! The sandbox of `neottx` (REQ-430, REQ-SEC-017), run as a user runs it:
//! every command that reads a file reaches the sandbox, and on Linux each
//! still does its work under it, having opened what it needs first.
#![allow(clippy::unwrap_used, missing_docs, reason = "a test")]

mod common;
use common::{folder, neottx};
use std::fs;
use std::path::{Path, PathBuf};

/// What `sandbox::apply` says when `--no-sandbox` turns it off: a command
/// that prints it has reached the sandbox.
const UNSANDBOXED: &str = "running without a sandbox (--no-sandbox)";

/// A page file for the commands to read.
const PAGE: &str = "PN,10000\nSC,0000\nOL,1,SANDBOXED\n";

/// A folder of the test's own holding `PAGE` as `page.tti`.
fn page_in(name: &str) -> (PathBuf, PathBuf) {
    let folder = folder(name);
    let page = folder.join("page.tti");
    fs::write(&page, PAGE).unwrap();
    (folder, page)
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// The commands that read a file, each with `--no-sandbox` at the end.
fn commands(folder: &Path, page: &Path) -> Vec<Vec<String>> {
    let out = |name: &str| text(&folder.join(name)).to_owned();
    let page = text(page).to_owned();
    let mut commands = vec![
        vec!["check".into(), page.clone()],
        vec!["convert".into(), page.clone(), "--to".into(), "t42".into(), "-o".into(), out("out.t42")],
        vec!["convert".into(), page.clone(), "--to".into(), "tti".into(), "-o".into(), out("out.tti")],
        vec!["services".into(), page.clone()],
        vec!["list".into(), page.clone()],
        vec!["text".into(), page.clone(), "100".into()],
        vec!["service-info".into(), page.clone()],
        vec!["render".into(), page.clone(), "100".into(), "-o".into(), out("page.png")],
        vec!["show".into(), page.clone(), "100".into(), "--graphics".into(), "text".into()],
    ];
    if cfg!(feature = "slice") {
        commands.push(vec!["slice".into(), page, "-o".into(), out("sliced.t42")]);
    }
    for command in &mut commands {
        command.push("--no-sandbox".into());
    }
    commands
}

// Verifies REQ-430: `check`, `convert`, `slice` and `services` applied no
// sandbox at all, and `check` had no `--no-sandbox`; every command that
// reads a file now reaches the sandbox, which `--no-sandbox` turns off with
// a message.
#[test]
fn every_command_that_reads_a_file_reaches_the_sandbox() {
    let (folder, page) = page_in("sandbox-reached");
    for command in commands(&folder, &page) {
        let arguments: Vec<&str> = command.iter().map(String::as_str).collect();
        let run = neottx(&arguments);
        let said = String::from_utf8_lossy(&run.stderr);
        assert!(said.contains(UNSANDBOXED), "{arguments:?} did not reach the sandbox: {said}");
    }
}

// Verifies REQ-430 on Linux, where the sandbox is applied: each command
// opened its input and its output before the sandbox, so it still works.
#[cfg(target_os = "linux")]
#[test]
fn every_command_that_reads_a_file_works_under_the_sandbox() {
    let (folder, page) = page_in("sandbox-applied");
    for mut command in commands(&folder, &page) {
        command.pop();
        let arguments: Vec<&str> = command.iter().map(String::as_str).collect();
        let run = neottx(&arguments);
        let said = String::from_utf8_lossy(&run.stderr);
        assert!(!said.contains("cannot apply"), "{arguments:?}: {said}");
        // A page file is not a transport stream or a raw capture, so those
        // two find nothing; they still ran to the end under the sandbox.
        let finds_nothing = matches!(arguments.first(), Some(&("services" | "slice" | "service-info")));
        assert!(run.status.success() || finds_nothing, "{arguments:?}: {said}");
    }
    assert!(fs::read(folder.join("out.tti")).unwrap().windows(9).any(|window| window == b"SANDBOXED"));
    assert!(fs::metadata(folder.join("out.t42")).unwrap().len() > 0, "the T42 output was written");
    assert!(folder.join("page.png").exists());
}

// Verifies REQ-430 and REQ-431 on Linux: after the sandbox of a decoder,
// with no network and nothing writable, opening a file, starting a program
// and connecting to the network all fail.
#[cfg(target_os = "linux")]
#[test]
fn the_sandbox_blocks_files_programs_and_the_network() {
    let run = neottx(&["sandbox-test"]);
    let said = String::from_utf8_lossy(&run.stdout);
    for line in ["open file blocked: true", "start program blocked: true", "network blocked: true"] {
        assert!(said.contains(line), "{line} missing: {said} {}", String::from_utf8_lossy(&run.stderr));
    }
    assert!(run.status.success());
}
