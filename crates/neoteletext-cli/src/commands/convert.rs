//! `neottx convert` (REQ-INP-014, REQ-INP-018): between formats; with
//! `--to channel`, a ReGIS or NAPLPS picture carried in one sub-page's file
//! (renderer parity PAR-8, REQ-6763 item 2).

use super::{ConvertTarget, Selection, io_failure, parse_selection};
use crate::args::ChannelPictureArgs;
use crate::picture::{Language, picture_packets};
use neoteletext_app::convert as conversion;
use neoteletext_app::input::Options;
use neoteletext_app::{Failure, exit};
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_formats::picture_text::MAX_PICTURE_STREAM_BYTES;
use neoteletext_web::records::drawing_links::{MAX_DRAWING_LINKS, is_target};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// The largest links file read: every link at its longest target, written
/// as JSON with room to spare.
const MAX_LINKS_FILE_BYTES: usize = 1_048_576;

/// A file's bytes, refused past `bound`, naming it.
fn read_bounded(path: &Path, bound: usize, name: &str) -> Result<Vec<u8>, Failure> {
    let file = std::fs::File::open(path).map_err(|error| io_failure(path, error))?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(bound).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut bytes).map_err(|error| io_failure(path, error))?;
    if bytes.len() > bound {
        return Err(io_failure(path, format!("larger than {name} ({bound} bytes)")));
    }
    Ok(bytes)
}

/// The links of a links file: each drawing or object, 1 to 65,535, once,
/// and a target the TTI reader's parser reads.
fn parse_links(path: &Path, bytes: &[u8]) -> Result<Vec<(u16, Vec<u8>)>, Failure> {
    let refused = |why: &str| io_failure(path, why);
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| io_failure(path, error))?;
    let items = value.as_array().ok_or_else(|| refused("the links are a JSON array"))?;
    if items.len() > MAX_DRAWING_LINKS {
        return Err(refused("more links than a picture has (MAX_DRAWING_LINKS)"));
    }
    let mut links: Vec<(u16, Vec<u8>)> = Vec::with_capacity(items.len());
    for item in items {
        let number = item.get("drawing").or_else(|| item.get("object")).and_then(serde_json::Value::as_u64);
        let number = number.and_then(|number| u16::try_from(number).ok()).filter(|&number| number > 0);
        let number = number.ok_or_else(|| refused("each link names a drawing or object, 1 to 65,535"))?;
        let target = item.get("target").and_then(serde_json::Value::as_str).unwrap_or_default().as_bytes().to_vec();
        if !is_target(&target) {
            return Err(refused("a link's target is not one a text link takes"));
        }
        if links.iter().any(|(known, _)| *known == number) {
            return Err(refused("a drawing or object is named twice"));
        }
        links.push((number, target));
    }
    Ok(links)
}

/// The sub-page's file in a channel's folder: `NNN/SSSS.t42` when the page
/// has a sub-page folder, else `NNN.t42` (DOC-1868).
fn sub_page_file(folder: &Path, page: PageAddress, subcode: SubCode) -> PathBuf {
    let name = String::from_utf8_lossy(&page.ascii()).into_owned();
    let sub_page = folder.join(&name).join(format!("{:04X}.t42", subcode.value()));
    if sub_page.is_file() { sub_page } else { folder.join(format!("{name}.t42")) }
}

/// `neottx convert`.
pub fn convert(
    options: &Options,
    output: &Path,
    to: ConvertTarget,
    page: Option<&str>,
    picture: &ChannelPictureArgs,
) -> Result<(), Failure> {
    let filter = match page {
        Some(text) => match parse_selection(text)? {
            Selection::Page(address, subcode) => Some((address, subcode)),
            Selection::All => None,
        },
        None => None,
    };
    let language = match (&picture.regis, &picture.naplps) {
        (Some(path), _) => Some((Language::Regis, path)),
        (None, Some(path)) => Some((Language::Naplps, path)),
        (None, None) => None,
    };
    let wants_picture = language.is_some() || picture.links.is_some() || picture.cursor.is_some();
    if wants_picture && !matches!(to, ConvertTarget::Channel) {
        return Err((exit::USAGE, "a picture is carried only by --to channel".to_owned()));
    }
    let place = filter.filter(|_| wants_picture);
    if wants_picture && place.is_none() {
        return Err((exit::USAGE, "a picture needs --page naming its page and sub-code, e.g. 200/0001".to_owned()));
    }
    // Every file is read before the conversion closes the sandbox
    // (REQ-SEC-017); a picture is bounded as a kept one is (PAR-8, PAR-17).
    let stream = language
        .map(|(language, path)| {
            read_bounded(path, MAX_PICTURE_STREAM_BYTES, "MAX_PICTURE_STREAM_BYTES").map(|bytes| (language, bytes))
        })
        .transpose()?;
    let links = picture
        .links
        .as_deref()
        .map(|path| {
            read_bounded(path, MAX_LINKS_FILE_BYTES, "a links file's bound").and_then(|bytes| parse_links(path, &bytes))
        })
        .transpose()?
        .unwrap_or_default();
    let target = match to {
        ConvertTarget::T42 => conversion::Target::T42,
        ConvertTarget::Tti => conversion::Target::Tti,
        ConvertTarget::Edittf => conversion::Target::Edittf,
        ConvertTarget::Channel => conversion::Target::Channel,
    };
    conversion::convert(options, output, target, filter)?;
    let Some((page, subcode)) = place else { return Ok(()) };
    let (language, stream) = stream.unwrap_or((Language::Regis, Vec::new()));
    let packets =
        picture_packets(page, subcode, language, &stream, &links, picture.cursor).map_err(|why| (exit::INPUT, why))?;
    let file = sub_page_file(output, page, subcode);
    let mut out = std::fs::OpenOptions::new().append(true).open(&file).map_err(|error| io_failure(&file, error))?;
    out.write_all(&packets).and_then(|()| out.flush()).map_err(|error| io_failure(&file, error))
}
