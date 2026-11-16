//! The fixed list of feature builds the gate lints and tests (decision
//! E.61, rule 6): `cargo xtask features`.
//!
//! The workspace run with all features covers every crate's default build
//! but the core's, whose default is not every feature (decision E.105) and
//! is a build here. This list adds each crate with no optional area and
//! with each area alone, because a `cfg` mistake shows only in the builds that leave
//! something out. It is a fixed list rather than every combination: the
//! features are additive, so each area alone and none at all cover the
//! seams.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

/// One build: the package and its features ("" for none).
struct Build {
    package: &'static str,
    features: &'static str,
}

/// The builds, crate by crate, with no default features.
const BUILDS: &[Build] = &[
    Build { package: "neoteletext-core", features: "" },
    Build { package: "neoteletext-core", features: "level15" },
    Build { package: "neoteletext-core", features: "level25" },
    // Levels 2.5 and 3.5 with every capability compiled out, and the
    // modern-classic profile (decision E.71, REQ-GEN-020).
    Build { package: "neoteletext-core", features: "level15,level25" },
    Build { package: "neoteletext-core", features: "modern-classic" },
    // The default build, and the Colour Map of Level 2.5 without that of
    // Level 3.5 (decision E.105).
    Build { package: "neoteletext-core", features: "default" },
    Build { package: "neoteletext-core", features: "level15,level25,level25-32colors" },
    Build { package: "neoteletext-core", features: "classic" },
    Build { package: "neoteletext-core", features: "render" },
    // The glyph line cache of REQ-392 (TODO-2068), not default.
    Build { package: "neoteletext-core", features: "glyph-cache" },
    Build { package: "neoteletext-core", features: "extended-sets" },
    Build { package: "neoteletext-core", features: "viewer" },
    // Error and Display on the core's types, which needs core::fmt
    // (REQ-GEN-016 keeps it out of small builds).
    Build { package: "neoteletext-core", features: "error-trait" },
    Build { package: "neoteletext-formats", features: "" },
    Build { package: "neoteletext-formats", features: "transport" },
    Build { package: "neoteletext-formats", features: "sliced" },
    Build { package: "neoteletext-formats", features: "pages" },
    Build { package: "neoteletext-formats", features: "tti" },
    Build { package: "neoteletext-formats", features: "ep1,edittf,dump" },
    Build { package: "neoteletext-formats", features: "check" },
    Build { package: "neoteletext-formats", features: "bdf" },
    Build { package: "neoteletext-formats", features: "alloc" },
    Build { package: "neoteletext-formats", features: "std" },
    Build { package: "neoteletext-formats", features: "std,tti" },
    Build { package: "neoteletext-formats", features: "write-text" },
    Build { package: "neoteletext-formats", features: "write-text,std,tti" },
    Build { package: "neoteletext-formats", features: "error-trait" },
    Build { package: "neoteletext-web", features: "" },
    Build { package: "neoteletext-web", features: "push" },
    Build { package: "neoteletext-web", features: "cookies" },
    Build { package: "neoteletext-web", features: "dbsc" },
    Build { package: "neoteletext-web", features: "error-trait" },
    Build { package: "neoteletext-web", features: "alloc" },
    Build { package: "neoteletext-web", features: "std" },
    // The software data slicer and the image helpers, with no optional
    // area and each alone (TODO-1142).
    Build { package: "neoteletext-slicer", features: "" },
    Build { package: "neoteletext-slicer", features: "error-trait" },
    Build { package: "neoteletext-image", features: "" },
    Build { package: "neoteletext-image", features: "error-trait" },
    Build { package: "neoteletext-image", features: "alloc" },
    Build { package: "neoteletext-image", features: "std" },
    Build { package: "neoteletext-audio", features: "" },
    Build { package: "neoteletext-audio", features: "idl" },
    Build { package: "neoteletext-audio", features: "ima" },
    Build { package: "neoteletext-audio", features: "opus" },
    Build { package: "neoteletext-audio", features: "pcm16" },
    Build { package: "neoteletext-audio", features: "encode" },
    Build { package: "neoteletext-audio", features: "error-trait" },
    // The desktop build (Windows, macOS, Linux): PCM16 and IMA ADPCM
    // compiled out, Opus alone (decision E.131).
    Build { package: "neoteletext-audio", features: "idl,opus" },
    // The instruments, each alone and both with the desktop build
    // (REQ-6411, REQ-6412).
    Build { package: "neoteletext-audio", features: "sid" },
    Build { package: "neoteletext-audio", features: "dx7" },
    Build { package: "neoteletext-audio", features: "idl,opus,sid,dx7" },
    Build { package: "neoteletext-audio-instruments", features: "" },
    Build { package: "neoteletext-audio-instruments", features: "sid" },
    Build { package: "neoteletext-audio-instruments", features: "dx7" },
    Build { package: "neoteletext-opus", features: "" },
    Build { package: "neoteletext-opus", features: "error-trait" },
    Build { package: "neoteletext-audio-mix", features: "" },
    Build { package: "neoteletext-input", features: "" },
    Build { package: "neoteletext-input", features: "error-trait" },
    Build { package: "neoteletext-transport", features: "" },
    Build { package: "neoteletext-qrkeyboard", features: "" },
    // The picture converter and its program (DEC-2325), which have no
    // optional area: each alone, so the core's features they name are
    // checked to be enough (REQ-1642 rule 5).
    Build { package: "neoteletext-picture", features: "" },
    Build { package: "neoteletext-picture-cli", features: "" },
    Build { package: "neoteletext-app", features: "" },
    Build { package: "neoteletext-app", features: "live" },
    Build { package: "neoteletext-app", features: "slice" },
    Build { package: "neoteletext-app", features: "write-text" },
    Build { package: "neoteletext-cli", features: "" },
    Build { package: "neoteletext-cli", features: "view" },
    Build { package: "neoteletext-cli", features: "live" },
    Build { package: "neoteletext-cli", features: "slice" },
    Build { package: "neoteletext-cli", features: "sound" },
    Build { package: "neoteletext-cli", features: "keyboard" },
    Build { package: "neoteletext-cli", features: "write-text" },
    // The capabilities of Levels 2.5 and 3.5 that no build has by default
    // (decision E.105).
    Build { package: "neoteletext-cli", features: "objects" },
    Build { package: "neoteletext-cli", features: "additional-flash-functions" },
];

/// How many builds run at once, each with its own target directory. The
/// builds are small, so one cargo invocation uses a few processors at most
/// and the rest wait: run one after another on 20 processors, the 76
/// builds took 9m09s and kept the machine 18% busy (16 November 2026).
/// More directories cost disk (a few gigabytes each) and share less work.
const PARALLEL_BUILDS: usize = 4;

/// `cargo xtask features`: lints and tests each build of [`BUILDS`],
/// [`PARALLEL_BUILDS`] at a time. Each build's output is printed whole when
/// it ends, so builds running together do not interleave.
pub fn run(root: &Path) -> Result<(), String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    let print = Mutex::new(());
    thread::scope(|scope| {
        for worker in 0..PARALLEL_BUILDS {
            let (next, failures, print) = (&next, &failures, &print);
            scope.spawn(move || {
                let target_dir = root.join("target").join("features").join(worker.to_string());
                while let Some(build) = BUILDS.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let label = format!("{} [{}]", build.package, build.features);
                    let base = ["--quiet", "-p", build.package, "--no-default-features", "--features", build.features];
                    let mut output = Vec::new();
                    let lint = cargo(
                        root,
                        &target_dir,
                        &[&["clippy", "--all-targets"][..], &base, &["--", "-D", "warnings"]].concat(),
                        &mut output,
                    );
                    let test = lint && cargo(root, &target_dir, &[&["test"][..], &base].concat(), &mut output);
                    let _printing = print.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if !test {
                        print!("{}", String::from_utf8_lossy(&output));
                        failures.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(label.clone());
                    }
                    println!("{label:<40} {}", if test { "ok" } else { "FAILED" });
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
    // In the list's order, whichever worker found them.
    failures.sort_by_key(|label| {
        BUILDS.iter().position(|build| format!("{} [{}]", build.package, build.features) == *label)
    });
    if failures.is_empty() { Ok(()) } else { Err(format!("failed: {}", failures.join(", "))) }
}

/// Runs cargo with `args` in `target_dir`, adding its output to `output`;
/// true on success.
fn cargo(root: &Path, target_dir: &Path, args: &[&str], output: &mut Vec<u8>) -> bool {
    match Command::new("cargo").current_dir(root).env("CARGO_TARGET_DIR", target_dir).args(args).output() {
        Ok(result) => {
            output.extend_from_slice(&result.stdout);
            output.extend_from_slice(&result.stderr);
            result.status.success()
        }
        Err(error) => {
            output.extend_from_slice(format!("cargo did not start: {error}\n").as_bytes());
            false
        }
    }
}
