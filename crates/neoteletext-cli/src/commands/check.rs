//! `neottx check` (REQ-CLI-006): the page checker, with the service
//! descriptor its links are resolved against.

use super::{io_failure, sandbox_closed};
use neoteletext_app::input;
use neoteletext_app::{Failure, exit};
use neoteletext_formats::Item;
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
    for file in files {
        opened.push((file, File::open(file).map_err(|error| io_failure(file, error))?));
    }
    sandbox_closed(no_sandbox)?;
    let services: Vec<Vec<u8>> = match (descriptor, &descriptor_json) {
        (Some(path), Some(json)) => descriptor::parse_service_names(path, json)?,
        _ => Vec::new(),
    };
    let names: Vec<&[u8]> = services.iter().map(Vec::as_slice).collect();
    let options = CheckOptions { services: descriptor.map(|_| names.as_slice()) };
    let mut errors = 0u32;
    for (file, mut input) in opened {
        let mut reader = neoteletext_formats::tti::TtiReader::default();
        input::pump(&mut input, &mut reader, &mut |item| {
            if let Item::Page(page) = item {
                check_page(page, &options, &mut |finding| {
                    let severity = match finding.severity {
                        Severity::Error => {
                            errors = errors.saturating_add(1);
                            "error"
                        }
                        Severity::Warning => "warning",
                    };
                    println!(
                        "{}:{}: page {} sub-page {}: {severity}: {:?}",
                        file.display(),
                        finding.line,
                        String::from_utf8_lossy(&finding.page.ascii()),
                        finding.subpage,
                        finding.problem
                    );
                });
            }
        })?;
    }
    if errors > 0 { Err((exit::CHECK_ERRORS, format!("{errors} error(s)"))) } else { Ok(()) }
}
