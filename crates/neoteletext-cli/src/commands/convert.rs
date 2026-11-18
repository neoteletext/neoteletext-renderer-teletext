//! `neottx convert` (REQ-INP-014, REQ-INP-018): between formats; with
//! `--to channel`, a ReGIS or NAPLPS picture carried in one sub-page's file
//! (renderer parity PAR-8, REQ-6763 item 2).

use super::picture::{MAX_PICTURED_PAGE_FILE, MAX_PICTURED_SUBPAGE_FILE};
use super::{ConvertTarget, Selection, io_failure, parse_selection};
use crate::args::ChannelPictureArgs;
use crate::picture::{Language, NaplpsObjectsFile, picture_packets};
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

/// The largest objects file read: 64 objects (`MAX_OBJECTS`), each at its
/// longest label and sixteen frames, written as JSON with room to spare.
const MAX_OBJECTS_FILE_BYTES: usize = 262_144;

/// The keys of the objects file: the `naplps_objects` message's, without
/// `type`, `connections`, `page` and `sub` (the program protocol's schema).
const OBJECTS_KEYS: [&str; 3] = ["objects", "one", "title"];
/// The keys of one object (the schema's `NaplpsObject`).
const OBJECT_KEYS: [&str; 18] = [
    "number",
    "kind",
    "shown",
    "detectable",
    "layer",
    "order",
    "x",
    "y",
    "width",
    "height",
    "origin_x",
    "origin_y",
    "frames",
    "cover",
    "hover",
    "pressed",
    "alternative",
    "label",
];

/// The objects of an objects file, each field in its type's range; the
/// record's writer then checks every rule its reader keeps (ANIM-11).
fn parse_objects(path: &Path, bytes: &[u8]) -> Result<NaplpsObjectsFile, Failure> {
    use neoteletext_web::records::naplps_objects::MAX_OBJECTS;
    let refused = |why: &str| io_failure(path, why);
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| io_failure(path, error))?;
    let file = value.as_object().ok_or_else(|| refused("the objects file is a JSON object"))?;
    if let Some(key) = file.keys().find(|key| !OBJECTS_KEYS.contains(&key.as_str())) {
        return Err(refused(&format!("the objects file has no field {key:?}")));
    }
    let items =
        file.get("objects").and_then(serde_json::Value::as_array).ok_or_else(|| refused("`objects` is an array"))?;
    if items.len() > MAX_OBJECTS {
        return Err(refused("more objects than a sub-page has (MAX_OBJECTS)"));
    }
    let one = match file.get("one") {
        None => false,
        Some(value) => value.as_bool().ok_or_else(|| refused("`one` is true or false"))?,
    };
    let title = match file.get("title") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_str().ok_or_else(|| refused("`title` is text or null"))?.to_owned()),
    };
    let mut objects = Vec::with_capacity(items.len());
    for item in items {
        objects.push(parse_object(item).map_err(|why| refused(&why))?);
    }
    Ok(NaplpsObjectsFile { objects, title, one })
}

/// One object of an objects file.
fn parse_object(item: &serde_json::Value) -> Result<naplps::Object, String> {
    let object = item.as_object().ok_or("each object is a JSON object")?;
    if let Some(key) = object.keys().find(|key| !OBJECT_KEYS.contains(&key.as_str())) {
        return Err(format!("an object has no field {key:?}"));
    }
    let integer = |name: &str| -> Result<i64, String> {
        object
            .get(name)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| format!("an object's `{name}` is a whole number"))
    };
    let flag = |name: &str| -> Result<bool, String> {
        object
            .get(name)
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| format!("an object's `{name}` is true or false"))
    };
    let narrow = |name: &str, value: i64| format!("an object's `{name}` is out of range: {value}");
    let field = |name: &str| -> Result<i16, String> {
        let value = integer(name)?;
        i16::try_from(value).map_err(|_| narrow(name, value))
    };
    let side = |name: &str| -> Result<u16, String> {
        let value = integer(name)?;
        u16::try_from(value).map_err(|_| narrow(name, value))
    };
    let small = |name: &str| -> Result<u8, String> {
        match object.get(name) {
            None | Some(serde_json::Value::Null) => Ok(0),
            Some(value) => {
                let value = value.as_i64().ok_or_else(|| format!("an object's `{name}` is a whole number or null"))?;
                u8::try_from(value).map_err(|_| narrow(name, value))
            }
        }
    };
    let kind = match object.get("kind").and_then(serde_json::Value::as_str) {
        Some("macro") => naplps::ObjectKind::Macro,
        Some("region") => naplps::ObjectKind::Region,
        _ => return Err("an object's `kind` is \"macro\" or \"region\"".to_owned()),
    };
    let frames =
        object.get("frames").and_then(serde_json::Value::as_array).ok_or("an object's `frames` is an array")?;
    let frames = frames
        .iter()
        .map(|frame| frame.as_u64().and_then(|code| u8::try_from(code).ok()).ok_or("a frame is a macro's code"))
        .collect::<Result<Vec<u8>, _>>()?;
    let label = match object.get("label") {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(value) => value.as_str().ok_or("an object's `label` is text or null")?.to_owned(),
    };
    let order = integer("order")?;
    Ok(naplps::Object {
        number: side("number")?,
        kind,
        shown: flag("shown")?,
        detectable: flag("detectable")?,
        layer: small("layer")?,
        order: i8::try_from(order).map_err(|_| narrow("order", order))?,
        x: field("x")?,
        y: field("y")?,
        width: side("width")?,
        height: side("height")?,
        origin_x: field("origin_x")?,
        origin_y: field("origin_y")?,
        cover: small("cover")?,
        hover: small("hover")?,
        pressed: small("pressed")?,
        alternative: small("alternative")?,
        frames,
        label,
    })
}

/// A sub-page's files in a channel's folder: its page's, `NNN.t42`, and
/// its own in the page's folder, `NNN/SSSS.t42`, which exists when the
/// page has a folder (DOC-1868).
fn sub_page_files(folder: &Path, page: PageAddress, subcode: SubCode) -> (PathBuf, PathBuf) {
    let name = String::from_utf8_lossy(&page.ascii()).into_owned();
    (folder.join(format!("{name}.t42")), folder.join(&name).join(format!("{:04X}.t42", subcode.value())))
}

/// Appends a picture's record packets to its sub-page's files in the
/// channel folder `folder`, and gives the first file. The sub-page's file
/// is `NNN/SSSS.t42` when the page has a folder, else `NNN.t42`; when the
/// page has a folder and the sub-page is its first, `NNN.t42` holds the
/// same sub-page (DOC-2034) and carries the records too, so that a reader
/// of either finds the picture (PAR-8). Each file with them must stay within
/// the bound every reader holds it to (REQ-1904): `MAX_PICTURED_PAGE_FILE`
/// for a page's file, `MAX_PICTURED_SUBPAGE_FILE` for a sub-page's own;
/// past it nothing is written, since every reader would skip the file.
pub(super) fn append_picture(
    folder: &Path,
    page: PageAddress,
    subcode: SubCode,
    packets: &[u8],
) -> Result<PathBuf, Failure> {
    let (page_file, own_file) = sub_page_files(folder, page, subcode);
    let mut files = Vec::new();
    if own_file.is_file() {
        let own = std::fs::read(&own_file).map_err(|error| io_failure(&own_file, error))?;
        let same_size = std::fs::metadata(&page_file).is_ok_and(|metadata| metadata.len() == own.len() as u64);
        let first = same_size && std::fs::read(&page_file).is_ok_and(|bytes| bytes == own);
        files.push((own_file, MAX_PICTURED_SUBPAGE_FILE));
        if first {
            files.push((page_file, MAX_PICTURED_PAGE_FILE));
        }
    } else {
        files.push((page_file, MAX_PICTURED_PAGE_FILE));
    }
    for (file, bound) in &files {
        let written = std::fs::metadata(file).map_err(|error| io_failure(file, error))?.len();
        let total = usize::try_from(written).unwrap_or(usize::MAX).saturating_add(packets.len());
        if total > *bound {
            return Err((
                exit::INPUT,
                format!("{}: with its picture it would be larger than {bound} bytes", file.display()),
            ));
        }
    }
    for (file, _) in &files {
        let mut out = std::fs::OpenOptions::new().append(true).open(file).map_err(|error| io_failure(file, error))?;
        out.write_all(packets).and_then(|()| out.flush()).map_err(|error| io_failure(file, error))?;
    }
    files.into_iter().next().map(|(file, _)| file).ok_or_else(|| (exit::INPUT, "no file for the picture".to_owned()))
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
    let wants_picture =
        language.is_some() || picture.objects.is_some() || picture.links.is_some() || picture.cursor.is_some();
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
    let objects = picture
        .objects
        .as_deref()
        .map(|path| {
            read_bounded(path, MAX_OBJECTS_FILE_BYTES, "an objects file's bound")
                .and_then(|bytes| parse_objects(path, &bytes))
        })
        .transpose()?;
    let target = match to {
        ConvertTarget::T42 => conversion::Target::T42,
        ConvertTarget::Tti => conversion::Target::Tti,
        ConvertTarget::Edittf => conversion::Target::Edittf,
        ConvertTarget::Channel => conversion::Target::Channel,
    };
    // The sub-page that carries a picture keeps its page whole: a page of
    // several sub-pages keeps its folder, and the picture goes into the
    // sub-page's own file there, `NNN/SSSS.t42` (PAR-8, DOC-1868), not into
    // a page cut down to that one sub-page.
    let kept = match place {
        Some((page, _)) => Some((page, SubCode::ANY)),
        None => filter,
    };
    conversion::convert(options, output, target, kept)?;
    let Some((page, subcode)) = place else { return Ok(()) };
    let (language, stream) = stream.unwrap_or((Language::Regis, Vec::new()));
    let packets = picture_packets(page, subcode, language, &stream, objects.as_ref(), &links, picture.cursor)
        .map_err(|why| (exit::INPUT, why))?;
    append_picture(output, page, subcode, &packets).map(|_| ())
}
