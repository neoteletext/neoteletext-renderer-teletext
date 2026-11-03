//! `neottx check` (REQ-CLI-006): the page checker, with the service
//! descriptor its links are resolved against.

use super::io_failure;
use neoteletext_app::input;
use neoteletext_app::{Failure, exit};
use neoteletext_core::types::PageAddress;
use neoteletext_formats::check::Problem;
use neoteletext_formats::{DiagnosticKind, Item};
use std::fs::File;
use std::path::{Path, PathBuf};

/// The short names of the services a descriptor lists, for `--descriptor`.
#[cfg(feature = "live")]
fn descriptor_service_names(path: &Path) -> Result<Vec<Vec<u8>>, Failure> {
    neoteletext_app::descriptor::descriptor_service_names(path)
}

/// Service descriptors are read by the web module, which a build without
/// live feeds leaves out.
#[cfg(not(feature = "live"))]
fn descriptor_service_names(_path: &Path) -> Result<Vec<Vec<u8>>, Failure> {
    Err(neoteletext_app::not_in_build("service descriptors (--descriptor)", "live"))
}

/// `neottx check`: the page checker (REQ-CLI-006). Exit status 4 on errors.
pub fn check(files: &[PathBuf], descriptor: Option<&Path>) -> Result<(), Failure> {
    use neoteletext_formats::check::{CheckOptions, Severity, check_page};
    let services: Vec<Vec<u8>> = match descriptor {
        Some(path) => descriptor_service_names(path)?,
        None => Vec::new(),
    };
    let names: Vec<&[u8]> = services.iter().map(Vec::as_slice).collect();
    let options = CheckOptions { services: descriptor.map(|_| names.as_slice()) };
    let mut errors = 0u32;
    for file in files {
        let mut input = File::open(file).map_err(|error| io_failure(file, error))?;
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
