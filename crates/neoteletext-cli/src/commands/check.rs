//! `neottx check` (REQ-CLI-006): the page checker, with the service
//! descriptor its links are resolved against.

use super::{io_failure, sandbox_closed};
use neoteletext_app::input;
use neoteletext_app::{Failure, exit};
use neoteletext_core::types::PageAddress;
use neoteletext_formats::check::Problem;
use neoteletext_formats::{DiagnosticKind, Item};
use std::fs::File;
use std::path::{Path, PathBuf};

/// A descriptor's file, read within its bound before the sandbox, and the
/// short names of the services it lists, parsed after it (REQ-SEC-017).
#[cfg(feature = "live")]
mod descriptor {
    pub(super) use neoteletext_app::descriptor::{parse_service_names, read_descriptor};
}

/// Service descriptors are read by the web module, which a build without
/// live feeds leaves out.
#[cfg(not(feature = "live"))]
mod descriptor {
    use neoteletext_app::Failure;
    use std::path::Path;

    pub(super) fn read_descriptor(_path: &Path) -> Result<Vec<u8>, Failure> {
        Err(neoteletext_app::not_in_build("service descriptors (--descriptor)", "live"))
    }

    pub(super) fn parse_service_names(_path: &Path, _json: &[u8]) -> Result<Vec<Vec<u8>>, Failure> {
        Err(neoteletext_app::not_in_build("service descriptors (--descriptor)", "live"))
    }
}

/// `neottx check`: the page checker (REQ-CLI-006). Exit status 4 on errors.
/// Every page file and the descriptor are opened first, then the sandbox
/// is applied with no network and nothing writable, and only then is any
/// of them parsed (REQ-SEC-017).
pub fn check(files: &[PathBuf], descriptor: Option<&Path>, no_sandbox: bool) -> Result<(), Failure> {
    use neoteletext_formats::check::{CheckOptions, Severity, check_page};
    let descriptor_json = descriptor.map(descriptor::read_descriptor).transpose()?;
    let mut opened = Vec::with_capacity(files.len());
    let mut pictures = Vec::new();
    for file in files {
        // PAR-7: a ReGIS or NAPLPS picture is checked as the server checks
        // it, its bytes read before the sandbox.
        if let Some(language) = file.to_str().and_then(crate::picture::language_of) {
            pictures.push((file, language, super::picture::read_checked(file)?));
            continue;
        }
        opened.push((file, File::open(file).map_err(|error| io_failure(file, error))?));
    }
    sandbox_closed(no_sandbox)?;
    let mut refused = 0_u32;
    for (file, language, bytes) in &pictures {
        if super::picture::check(*language, file, bytes)? {
            refused = refused.saturating_add(1);
        }
    }
    let services: Vec<Vec<u8>> = match (descriptor, &descriptor_json) {
        (Some(path), Some(json)) => descriptor::parse_service_names(path, json)?,
        _ => Vec::new(),
    };
    let names: Vec<&[u8]> = services.iter().map(Vec::as_slice).collect();
    let options = CheckOptions { services: descriptor.map(|_| names.as_slice()) };
    let mut errors = 0u32;
    for (file, mut input) in opened {
        let mut reader = neoteletext_formats::tti::TtiReader::default();
        // The reader names each problem as a diagnostic when it meets it, and
        // keeps at most `PAGE_ISSUES` of them with the page. A problem before
        // the first `PN` line belongs to no page, and one past that capacity
        // is not with its page, so both are reported from the diagnostics:
        // REQ-INP-026 and REQ-CLI-006 ask for every one.
        let mut pending: Vec<(u32, Problem)> = Vec::new();
        let mut first_page = true;
        input::pump(&mut input, &mut reader, &mut |item| match item {
            Item::Diagnostic(diagnostic) => {
                let problem = match diagnostic.kind {
                    DiagnosticKind::Page(kind) => Problem::of_issue(kind),
                    DiagnosticKind::RowTooLong { .. } => Problem::RowTooLong,
                    _ => return,
                };
                pending.push((diagnostic.line.unwrap_or(0), problem));
            }
            Item::Page(page) => {
                let before = pending.iter().filter(|(line, _)| *line < page.line);
                for &(line, problem) in before {
                    errors = errors.saturating_add(1);
                    report_outside_page(file, line, first_page, problem);
                }
                pending.retain(|(line, _)| *line >= page.line);
                first_page = false;
                check_page(page, &options, &mut |finding| {
                    pending.retain(|&(line, problem)| (line, problem) != (finding.line, finding.problem));
                    let severity = match finding.severity {
                        Severity::Error => {
                            errors = errors.saturating_add(1);
                            "error"
                        }
                        Severity::Warning => "warning",
                    };
                    report(file, finding.line, &page_name(finding.page, finding.subpage), severity, finding.problem);
                });
                // What the page could not hold is still its own.
                for (line, problem) in pending.drain(..) {
                    errors = errors.saturating_add(1);
                    report(file, line, &page_name(page.address, page.subpage), "error", problem);
                }
            }
            _ => {}
        })?;
        for (line, problem) in pending {
            errors = errors.saturating_add(1);
            report_outside_page(file, line, first_page, problem);
        }
    }
    let errors = errors.saturating_add(refused);
    if errors > 0 { Err((exit::CHECK_ERRORS, format!("{errors} error(s)"))) } else { Ok(()) }
}

/// How a finding names its page.
fn page_name(address: PageAddress, subpage: u8) -> String {
    format!("page {} sub-page {subpage}", String::from_utf8_lossy(&address.ascii()))
}

/// Prints one finding, on the line of the file it was found on.
fn report(file: &Path, line: u32, place: &str, severity: &str, problem: Problem) {
    println!("{}:{line}: {place}: {severity}: {problem:?}", file.display());
}

/// Prints an error found outside every page: before the first `PN` line, or
/// after a `PN` line that gave no page.
fn report_outside_page(file: &Path, line: u32, first_page: bool, problem: Problem) {
    let place = if first_page { "before the first page" } else { "outside a page" };
    report(file, line, place, "error", problem);
}
