//! `neottx convert --to channel` (DEC-2413): the input's pages written as a
//! static channel's folder, laid out by the web module's one layout library
//! (REQ-1887, REQ-1888, REQ-WEB-019): each page `NNN.t42`; a page of
//! several sub-pages also as `NNN/SSSS.t42` for each with its list in parts
//! `NNN/P.sub`; and the bundle `index.t42`.

use super::session_page_files;
use crate::input::{self, Options};
use crate::{Failure, exit};
use neoteletext_core::consts::{HEADER_DISPLAY_BYTES, PAGE_TIME_FILLING, SPACE};
use neoteletext_core::ecc::with_odd_parity;
use neoteletext_core::packet::encode;
use neoteletext_core::types::{ControlBits, Magazine, PageAddress, PageNumber, SubCode};
use neoteletext_formats::encode::{TransmissionClass, encode_page, transmission_class};
use neoteletext_formats::page::PageFile;
use neoteletext_web::subpages::{ChannelFiles, LayoutError, SubPageFile};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path};

/// The header that ends a page's transmission in its magazine: a
/// time-filling header (EN 300 706 clause 7.2.1), so that a reader of the
/// file completes the page without waiting for its next header.
fn ending_header(magazine: Magazine) -> Option<[u8; neoteletext_core::consts::PACKET_BYTES]> {
    let display = [with_odd_parity(SPACE); HEADER_DISPLAY_BYTES];
    encode::header(magazine, PageNumber(PAGE_TIME_FILLING), SubCode::new(0), ControlBits(0), &display)
        .map(|packet| packet.bytes)
}

/// Pages as T42, in order, ended by a time-filling header.
fn t42_of(pages: &[&PageFile], magazine: Magazine) -> Vec<u8> {
    let mut bytes = Vec::new();
    for page in pages {
        encode_page(page, &mut |packet| bytes.extend_from_slice(&packet.bytes));
    }
    if let Some(end) = ending_header(magazine) {
        bytes.extend_from_slice(&end);
    }
    bytes
}

/// What went wrong laying out a page, in words.
fn layout_failure(page: PageAddress, error: LayoutError) -> Failure {
    let page = page.mpp();
    let why = match error {
        LayoutError::Empty => "it has no sub-page".to_owned(),
        LayoutError::TooMany => "it has more sub-pages than a page's list may hold".to_owned(),
        LayoutError::Duplicate(subcode) => format!("sub-code {:04X} is given twice", subcode.value()),
        LayoutError::Content(Some(subcode)) => {
            format!("sub-page {:04X} is larger than a sub-page's file may be", subcode.value())
        }
        LayoutError::Content(None) => "it is larger than a page's file may be".to_owned(),
        LayoutError::SubCode(subcode) => format!("sub-code {:04X} names no sub-page's file", subcode.value()),
        LayoutError::PageTwice(_) => "it is given twice".to_owned(),
    };
    (exit::INPUT, format!("page {page:03X} cannot be written to a channel: {why}"))
}

/// The input's pages laid out as a channel's files: each display page's
/// sub-pages as files of their own, and every page for further processing
/// (magazine packets, tables, object pages) whole, as a display page asks
/// for all of its sub-pages together (REQ-WEB-022).
fn channel_files(pages: &[PageFile]) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut by_page: BTreeMap<PageAddress, Vec<&PageFile>> = BTreeMap::new();
    for page in pages {
        by_page.entry(page.address).or_default().push(page);
    }
    let mut files = ChannelFiles::new();
    for (address, sub_pages) in by_page {
        let display = sub_pages.iter().all(|page| transmission_class(page) == TransmissionClass::Display);
        if display {
            let encoded: Vec<(SubCode, Vec<u8>)> =
                sub_pages.iter().map(|page| (page.subcode, t42_of(&[page], address.magazine))).collect();
            // A title from row 1 is REQ-1891's rule, not yet built: none is
            // written, and a reader shows the sub-code.
            let entries: Vec<SubPageFile<'_>> =
                encoded.iter().map(|(subcode, t42)| SubPageFile { subcode: *subcode, t42, title: "" }).collect();
            files.add_page(address, &entries).map_err(|error| layout_failure(address, error))?;
        } else {
            files
                .add_whole_page(address, &t42_of(&sub_pages, address.magazine))
                .map_err(|error| layout_failure(address, error))?;
        }
    }
    Ok(files.finish())
}

/// A file's place under the folder, when its name stays inside it: only
/// plain names, never a root, a drive or `..`.
fn inside(folder: &Path, name: &str) -> Option<std::path::PathBuf> {
    let relative = Path::new(name);
    let plain = relative.components().all(|component| matches!(component, Component::Normal(_)));
    (plain && !name.is_empty()).then(|| folder.join(relative))
}

/// Writes every file under `folder`, creating it and its page folders, and
/// never writing over a file that is there.
fn write_files(folder: &Path, files: &[(String, Vec<u8>)]) -> Result<(), Failure> {
    let failed = |path: &Path, error: std::io::Error| (exit::INPUT, format!("{}: {error}", path.display()));
    std::fs::create_dir_all(folder).map_err(|error| failed(folder, error))?;
    for (name, bytes) in files {
        let path = inside(folder, name)
            .ok_or_else(|| (exit::INPUT, format!("{name:?} would be written outside the folder")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| failed(parent, error))?;
        }
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&path).map_err(|error| failed(&path, error))?;
        file.write_all(bytes).and_then(|()| file.flush()).map_err(|error| failed(&path, error))?;
    }
    Ok(())
}

/// Writes the input's pages as a channel's folder at `folder` (DEC-2413).
/// With a filter, only that page's sub-pages.
pub(super) fn convert(options: &Options, folder: &Path, filter: Option<(PageAddress, SubCode)>) -> Result<(), Failure> {
    let session = input::load(options)?;
    let pages = session_page_files(&session, filter);
    let files = channel_files(&pages)?;
    write_files(folder, &files)
}

#[cfg(test)]
mod tests;
