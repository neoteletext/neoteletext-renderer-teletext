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

/// `cargo xtask features`: lints and tests each build of [`BUILDS`].
pub fn run(root: &Path) -> Result<(), String> {
    let mut failures = Vec::new();
    for build in BUILDS {
        let label = format!("{} [{}]", build.package, build.features);
        let base = ["--quiet", "-p", build.package, "--no-default-features", "--features", build.features];
        let lint = cargo(root, &[&["clippy", "--all-targets"][..], &base, &["--", "-D", "warnings"]].concat());
        let test = lint && cargo(root, &[&["test"][..], &base].concat());
        println!("{label:<40} {}", if test { "ok" } else { "FAILED" });
        if !test {
            failures.push(label);
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(format!("failed: {}", failures.join(", "))) }
}

/// Runs cargo with `args`, its output shown; true on success.
fn cargo(root: &Path, args: &[&str]) -> bool {
    Command::new("cargo").current_dir(root).args(args).status().is_ok_and(|status| status.success())
}
