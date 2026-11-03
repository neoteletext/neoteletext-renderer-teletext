//! Converting between formats (REQ-INP-014, REQ-INP-018), and stored
//! pages back to page files for `neottx convert --to tti`: display pages with
//! their packets 26 to 28, enhancement pages coded by their page coding,
//! and each magazine's packets M/29 on its page mFF (REQ-INP-023).

use crate::input::{self, Options};
use crate::{Failure, exit};
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_formats::Item;
use neoteletext_formats::page::PageFile;
use std::fs::File;
use std::io::Write;
use std::path::Path;

mod pages;

pub use pages::{enhancement_page_file, magazine_page_file, page_file, session_page_files};

/// Conversion targets (REQ-CLI-001 `convert`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// T42 packets.
    T42,
    /// TTI page files.
    Tti,
    /// edit.tf page addresses (REQ-INP-016), one page a line, each a web
    /// address whose fragment holds the page, as the editor opens it.
    Edittf,
}

/// The web address an edit.tf page address follows: the editor reads the
/// page from the fragment (REQ-INP-016), and the reader ignores what
/// comes before the `#`.
pub const EDITTF_ADDRESS_PREFIX: &[u8] = b"https://edit.tf/#";

/// Converts the input to `to`, written to `output`; with a filter, only
/// that page, and with a sub-code other than `SubCode::ANY` only that
/// sub-page (REQ-INP-014, REQ-INP-018). The output and the input are
/// opened, then the sandbox is applied, before the input is parsed
/// (REQ-SEC-017).
///
/// # Errors
/// A [`Failure`]: the exit status and the message for the user (DEC-2451),
/// when an input or output cannot be opened, the sandbox cannot be applied,
/// the input holds no teletext, or the page asked for is not in it.
pub fn convert(
    options: &Options,
    output: &Path,
    to: Target,
    filter: Option<(PageAddress, SubCode)>,
) -> Result<(), Failure> {
    let file = File::create(output).map_err(|error| (exit::INPUT, format!("{}: {error}", output.display())))?;
    match to {
        Target::T42 => {
            let wanted = |page: &PageFile| {
                filter.is_none_or(|(address, subcode)| {
                    page.address == address && (subcode.is_any() || page.subcode == subcode)
                })
            };
            let filter = neoteletext_formats::write::PacketFilter {
                magazine: filter.map(|(address, _)| address.magazine),
                page: filter.map(|(address, _)| address.page),
                subcode: filter.map(|(_, subcode)| subcode).filter(|subcode| !subcode.is_any()),
            };
            let mut writer = neoteletext_formats::write::T42Writer::with_filter(std::io::BufWriter::new(file), filter);
            let mut input = input::open(options)?;
            // REQ-SEC-017: the input and the output are open; nothing else is.
            input::sandbox(options, None)?;
            // Page files come as pages, so that they can be sent in the
            // order of Annex B.7 (REQ-INP-024); packets keep the order they
            // arrived in, which is the broadcast's own.
            let mut reader = input::reader(options, false);
            let mut pages: Vec<PageFile> = Vec::new();
            let mut failure = None;
            input::pump(input.as_mut(), reader.as_mut(), &mut |item| match item {
                Item::Packet { packet, .. } => {
                    if let Err(error) = writer.write_packet(&packet) {
                        failure = Some(error);
                    }
                }
                Item::Page(page) if wanted(page) => pages.push(page.clone()),
                _ => {}
            })?;
            if let Some(error) = failure {
                return Err((exit::INPUT, error.to_string()));
            }
            let references: Vec<&PageFile> = pages.iter().collect();
            writer.write_pages(&references).map_err(|error| (exit::INPUT, error.to_string()))?;
            writer.into_inner().flush().map_err(|error| (exit::INPUT, error.to_string()))
        }
        Target::Tti => {
            let session = input::load_sandboxed(options, None, || input::sandbox(options, None))?;
            let pages = session_page_files(&session, filter);
            let references: Vec<&PageFile> = pages.iter().collect();
            let mut out = std::io::BufWriter::new(file);
            neoteletext_formats::write::write_tti(&references, &mut out)
                .map_err(|error| (exit::INPUT, error.to_string()))?;
            out.flush().map_err(|error| (exit::INPUT, error.to_string()))
        }
        Target::Edittf => {
            let session = input::load(options)?;
            let pages = session_page_files(&session, filter);
            let mut out = std::io::BufWriter::new(file);
            let mut failure = None;
            for page in &pages {
                let mut write = |bytes: &[u8]| {
                    if failure.is_none() {
                        failure = out.write_all(bytes).err();
                    }
                };
                write(EDITTF_ADDRESS_PREFIX);
                neoteletext_formats::edittf::write(page, &mut write);
                write(b"\n");
            }
            if let Some(error) = failure {
                return Err((exit::INPUT, error.to_string()));
            }
            out.flush().map_err(|error| (exit::INPUT, error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests;
