//! Reading a descriptor file for `neottx check --descriptor` (REQ-CLI-006).

use crate::bounded_file::read_bounded;
use crate::{Failure, exit};
use neoteletext_web::channel::SETTINGS_FILE;
use neoteletext_web::consts::DEFAULT_DESCRIPTOR_LIMIT;
use neoteletext_web::descriptor::Descriptor;
use std::path::Path;

/// Reads a descriptor file, within its bound, to be parsed by
/// `parse_service_names` once the sandbox is applied (REQ-SEC-017).
///
/// # Errors
/// A file that cannot be read or is over its bound.
pub fn read_descriptor(path: &Path) -> Result<Vec<u8>, Failure> {
    read_bounded(path, DEFAULT_DESCRIPTOR_LIMIT)
}

/// The short names of the services the descriptor `json`, read from
/// `path`, lists, for `neottx check --descriptor` (REQ-CLI-006). The file's
/// name says which kind it is, as it does for a reader (REQ-WEB-018): a
/// channel's own file, whose members are all optional, or a service's
/// descriptor.
///
/// # Errors
/// Bytes that are not a descriptor of that kind.
pub fn parse_service_names(path: &Path, json: &[u8]) -> Result<Vec<Vec<u8>>, Failure> {
    let base = neoteletext_web::url::Url::parse("https://localhost/descriptor.json")
        .map_err(|error| (exit::INPUT, format!("{error:?}")))?;
    let parse = if path.file_name().is_some_and(|name| name == SETTINGS_FILE) {
        Descriptor::parse_channel
    } else {
        Descriptor::parse
    };
    let descriptor = parse(json, &base).map_err(|error| (exit::INPUT, format!("{}: {error:?}", path.display())))?;
    Ok(descriptor.services().map(|service| service.short_name.as_bytes().to_vec()).collect())
}
