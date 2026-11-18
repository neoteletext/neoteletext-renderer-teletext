//! Project automation (REQ-GEN-010): `cargo xtask <command>`.
//!
//! - `glyphs`: regenerate the core's glyph tables: Noto Teletext from its
//!   BDF files, the G3 set from the project's own geometry (REQ-RND-037),
//!   and the list of required characters from the character set tables.
//! - `corpus`: fetch and pin the real-broadcast test corpus (REQ-TST-022).
//! - `check-layers`: enforce the module dependency table of ARCHITECTURE.md
//!   (REQ-ARCH-011).
//! - `check-deps`: enforce the permitted dependency lists (REQ-GEN-017),
//!   and that each application crate uses only the workspace crates below
//!   it.
//! - `check-constants`: find teletext constants restated outside the
//!   constants module (REQ-GEN-014).
//! - `check-io`: find operating-system access in library crates, and
//!   check that every crate is classed as a library or an application
//!   crate (REQ-SEC-016).
//! - `level25-stream OUT.t42 [--tti DIR]`: write the authored Level 2.5 and
//!   3.5 test stream (REQ-TST-024).
//! - `check-noto`: check the Noto Teletext files against the SHA-256s in
//!   their provenance record (REQ-FNT-001, REQ-FNT-012).
//! - `misc-fixed`: write the X11 misc-fixed 6x10 font at 12 by 20 from the
//!   original kept beside it (REQ-2223).
//! - `check-misc-fixed`: check the misc-fixed files against the SHA-256s in
//!   their provenance record, and that the converted fonts are the original
//!   with every pixel two by two (REQ-2223).
//! - `features`: lint and test the fixed list of feature builds of decision
//!   E.61.
//! - `size [--target TRIPLE]`: link the microcontroller test image for each
//!   build of decision E.61 and check its size and symbols (REQ-GEN-009,
//!   REQ-GEN-019).
//! - `spec-examples OUT.json`: write the byte examples of the generated
//!   protocol document from the engine's encoders (REQ-1734).
//! - `appendix-a`: print appendix A of neoteletext-common's page-rules.md,
//!   the readable patch fields of each synthesizer family, from the
//!   families' tables (REQ-6527; page-rules.md, RULE-29).
//! - `check-appendix-a`: check that the document's appendix A is that.

mod appendix_a;
mod checks;
mod corpus;
mod features;
mod font_folders;
mod g3;
mod glyphs;
mod level25;
mod misc_fixed;
mod size;
mod spec_examples;
mod test_code;

use std::path::PathBuf;
use std::process::ExitCode;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().map_or(manifest.clone(), PathBuf::from)
}

fn main() -> ExitCode {
    let command = std::env::args().nth(1).unwrap_or_default();
    let root = workspace_root();
    let result = match command.as_str() {
        "glyphs" => glyphs::generate(&root),
        "corpus" => corpus::fetch(&root),
        "check-layers" => checks::layers(&root),
        "check-deps" => checks::dependencies(&root),
        "check-constants" => checks::constants(&root),
        "check-io" => checks::io(&root),
        "level25-stream" => level25::write(&std::env::args().skip(2).collect::<Vec<_>>()),
        "check-noto" => font_folders::check(&root, &font_folders::NOTO),
        "misc-fixed" => misc_fixed::generate(&root),
        "check-misc-fixed" => misc_fixed::check(&root),
        "features" => features::run(&root),
        "size" => size::run(&root, &std::env::args().skip(2).collect::<Vec<_>>()),
        "spec-examples" => spec_examples::write_examples(&root, &std::env::args().skip(2).collect::<Vec<_>>()),
        "appendix-a" => appendix_a::print(),
        "check-appendix-a" => appendix_a::check(),
        _ => Err(format!(
            "usage: cargo xtask <glyphs|corpus|check-layers|check-deps|check-constants|check-io|\
             check-noto|misc-fixed|check-misc-fixed|level25-stream|features|size|spec-examples|\
             appendix-a|check-appendix-a> (got {command:?})"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask {command}: {message}");
            ExitCode::FAILURE
        }
    }
}
