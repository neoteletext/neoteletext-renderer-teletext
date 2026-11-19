//! The `NAPLPS_OBJECTS` record, type 25 (neoteletext-common
//! `docs/animation.md`, DEC-6524; ANIM-11): all of a sub-page's NAPLPS
//! objects, replacing any it had, or one object redefined alone. NAPLPS
//! (ANSI X3.110-1983) keeps no scene, so the layer of numbered objects
//! over a stream's picture is defined by this record, and every stream
//! stays a conforming one. It also carries what the NAPLPS renderer holds
//! beside the stream and never draws (DEC-6638 items 3 and 6, TODO-6657):
//! each object's label, for a screen reader, and the picture's title, for
//! page lists.
//!
//! ```text
//! bytes 0-1    page: 0x100 to 0x8FF
//! bytes 2-3    sub: the sub-code, 0x0000 to 0x3F7E
//! bytes 4-5    generation: of the NAPLPS stream the objects belong to
//! byte  6      count: 0 to 64; with ONE, exactly 1
//! byte  7      flags: bit 0 ONE, the record redefines one object and
//!              keeps every other and the title; bit 1 TITLED, a title
//!              follows the last object (never with ONE); other bits 0
//! then for each object:
//! bytes 0-1    number: 1 to 65,535, distinct within the record
//! byte  2      kind: 1 a macro object, 2 a region
//! byte  3      flags: bit 0 shown, bit 1 detectable
//! byte  4      layer: 1 to 3
//! byte  5      order: -128 to 127
//! bytes 6-9    x, y: the raster dot of its rectangle's top left (480 by
//!              500, origin at the top left, y down; DEC-6853)
//! bytes 10-13  width, height: 1 to 480, 1 to 500, in dots and lines
//! bytes 14-17  origin x, origin y: raster dots; 0 for a region
//! byte  18     frames F: a macro object 1 to 16, a region 0
//! byte  19     cover: a region's colour-map entry; a macro object 0
//! bytes 20-22  hover, pressed and alternative look: a macro's code, or 0
//! byte  23     label length L: 0 (no label) to 64
//! bytes 24-    F frame bytes, each a macro's code, then L bytes of the
//!              label: UTF-8, no control character
//! with TITLED, after the last object:
//! byte  0      title length T: 1 to 64
//! bytes 1-     T bytes of the title: UTF-8, no control character
//! ```

use neoteletext_core::consts::{PAGE_HEIGHT, PAGE_WIDTH};
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::limits::{MAX_FRAME, MAX_LAYER};

use super::{RecordFault, TYPE_NAPLPS_OBJECTS, take_place, take_u16, write_packets};

/// The most objects a sub-page has (ANIM-11, ANIM-16). The NAPLPS
/// renderer's limits define the same figure; this is the record's.
pub const MAX_OBJECTS: usize = 64;
/// The most frames an object has (ANIM-14): frames 0 to 15.
pub const MAX_OBJECT_FRAMES: usize = MAX_FRAME as usize + 1;
/// The most bytes of an object's label, UTF-8: the text a screen reader
/// speaks for it (DEC-6638 item 6; the reading of DEC-6656 item 2). The
/// NAPLPS renderer's limits define the same figure, `MAX_LABEL_BYTES`;
/// this is the record's.
pub const MAX_LABEL_BYTES: usize = 64;
/// The most bytes of a picture's title, UTF-8 (DEC-6638 item 3; DEC-6656
/// item 2). The NAPLPS renderer's `MAX_TITLE_BYTES` is the same figure.
pub const MAX_TITLE_BYTES: usize = 64;
/// The fixed fields: page, sub-code, generation, count and the flags.
const FIXED: usize = 2 + 2 + 2 + 1 + 1;
/// An object's fixed fields, before its frame bytes.
const OBJECT_FIXED: usize = 24;
/// The title's length byte.
const TITLE_FIXED: usize = 1;
/// The most bytes of a `NAPLPS_OBJECTS` record's body.
pub const MAX_BYTES: usize =
    FIXED + MAX_OBJECTS * (OBJECT_FIXED + MAX_OBJECT_FRAMES + MAX_LABEL_BYTES) + TITLE_FIXED + MAX_TITLE_BYTES;
const _: () = assert!(MAX_BYTES <= neoteletext_audio::MAX_FRAMED_BODY);
// A length byte holds every length it gives.
const _: () = assert!(MAX_LABEL_BYTES <= u8::MAX as usize && MAX_TITLE_BYTES <= u8::MAX as usize);

/// The record's flag: it redefines one object, keeping the others.
const ONE: u8 = 1;
/// The record's flag: a title follows the last object.
const TITLED: u8 = 1 << 1;

/// An object's flag: shown.
const SHOWN: u8 = 1;
/// An object's flag: detectable.
const DETECTABLE: u8 = 1 << 1;
/// The first macro code (ANSI X3.110-1983 section 6.2): 2/0.
pub const FIRST_MACRO: u8 = 0x20;
/// The last macro code: 7/15. The 96 macro names lie between.
pub const LAST_MACRO: u8 = 0x7F;
/// The teletext raster's width in dots, the widest object (ANIM-11,
/// neoteletext-common `docs/naplps-raster.md` RAS-4): every NAPLPS
/// coordinate NeoTeletext defines is a dot of the page's raster (DEC-6853).
pub const RASTER_WIDTH: u16 = narrow(PAGE_WIDTH);
/// The raster's height in lines, the tallest object (RAS-4).
pub const RASTER_HEIGHT: u16 = narrow(PAGE_HEIGHT);

/// `value` as a record's 16-bit field, at compile time; a raster that did
/// not fit would fail the build here.
const fn narrow(value: usize) -> u16 {
    assert!(value <= u16::MAX as usize);
    #[expect(clippy::cast_possible_truncation, reason = "the assertion above holds it to 16 bits")]
    let narrowed = value as u16;
    narrowed
}

/// What a NAPLPS object is (ANIM-11).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjectKind {
    /// 1: drawn by executing its frame's macro.
    Macro,
    /// 2: a rectangle of the stream's own picture, which its cover entry
    /// fills while it is hidden.
    Region,
}

impl ObjectKind {
    /// The kind's byte in the record.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Macro => 1,
            Self::Region => 2,
        }
    }

    /// The kind of `code`, or `None` for a byte no kind has.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Macro),
            2 => Some(Self::Region),
            _ => None,
        }
    }
}

/// One object of a `NAPLPS_OBJECTS` record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NaplpsObject<'a> {
    /// Its number, 1 to 65,535.
    pub number: u16,
    /// A macro object or a region.
    pub kind: ObjectKind,
    /// Whether it is shown.
    pub shown: bool,
    /// Whether pointer triggers see it.
    pub detectable: bool,
    /// Its layer, 1 to 3.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
    /// The raster dot of its rectangle's left (RAS-4).
    pub x: i16,
    /// The raster line of its rectangle's top.
    pub y: i16,
    /// Its rectangle's width in dots, 1 to [`RASTER_WIDTH`]: dots `x` to
    /// `x + width - 1`.
    pub width: u16,
    /// Its rectangle's height in lines, 1 to [`RASTER_HEIGHT`]: lines `y`
    /// to `y + height - 1`.
    pub height: u16,
    /// The raster dot at which the macro, drawn where the stream would
    /// draw it, puts what is to be the rectangle's top left; (0, 0) for a
    /// region.
    pub origin: (i16, i16),
    /// A region's colour-map entry while it is hidden; 0 for a macro
    /// object.
    pub cover: u8,
    /// The hover, pressed and alternative looks: each a macro's code, or 0
    /// for none.
    pub looks: [u8; 3],
    /// The frames' macro codes: 1 to 16 for a macro object, none for a
    /// region.
    pub frames: &'a [u8],
}

/// What a `NAPLPS_OBJECTS` record does to its sub-page's objects.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Its objects are all the sub-page's, replacing any it had, and its
    /// title, or none, is the picture's (ANIM-11).
    Whole,
    /// Its one object replaces the sub-page's object of that number, or is
    /// added when there is none; every other object, with its animation
    /// state, and the picture's title are kept (DEC-6638 item 3, row 3.2;
    /// the NAPLPS renderer's `Terminal::replace_object`). The object's own
    /// animation state and pointer look are forgotten, as a whole set
    /// forgets those of what it replaces (ANIM-6).
    One,
}

/// An object with its label, as a writer of labels and titles takes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LabelledObject<'a> {
    /// The object.
    pub object: NaplpsObject<'a>,
    /// What a screen reader says for it: plain text of at most
    /// [`MAX_LABEL_BYTES`], or empty for none.
    pub label: &'a str,
}

/// Whether `text` is plain text of at most `most` bytes: no control
/// character, so that it can be shown or spoken as it stands and carries
/// no command (the NAPLPS renderer's `is_plain_text`; DEC-6656 item 2).
fn is_plain(text: &str, most: usize) -> bool {
    text.len() <= most && !text.chars().any(char::is_control)
}

/// Text of `length` bytes at the front of `bytes`, checked to be plain
/// text of at most `most`, and what follows it.
fn take_text(bytes: &[u8], length: u8, most: usize) -> Result<(&str, &[u8]), RecordFault> {
    let (text, rest) = bytes.split_at_checked(usize::from(length)).ok_or(RecordFault::Length)?;
    let text = core::str::from_utf8(text).map_err(|_| RecordFault::BadValue("naplps text not UTF-8"))?;
    if !is_plain(text, most) {
        return Err(RecordFault::BadValue("naplps text"));
    }
    Ok((text, rest))
}

/// Whether `code` is a macro's code, 2/0 to 7/15.
const fn is_macro(code: u8) -> bool {
    code >= FIRST_MACRO && code <= LAST_MACRO
}

/// A signed 16-bit field at the front of `bytes`, and what follows it.
fn take_i16(bytes: &[u8]) -> Result<(i16, &[u8]), RecordFault> {
    let (value, rest) = take_u16(bytes)?;
    Ok((value.cast_signed(), rest))
}

/// Reads one object and its label from the front of `bytes`, and what
/// follows them.
fn read_object(bytes: &[u8]) -> Result<(LabelledObject<'_>, &[u8]), RecordFault> {
    let refused = RecordFault::BadValue("naplps object");
    let (number, rest) = take_u16(bytes)?;
    let (&[kind, flags, layer, order], rest) = rest.split_first_chunk::<4>().ok_or(RecordFault::Length)?;
    let kind = ObjectKind::from_code(kind).ok_or(refused)?;
    let (x, rest) = take_i16(rest)?;
    let (y, rest) = take_i16(rest)?;
    let (width, rest) = take_u16(rest)?;
    let (height, rest) = take_u16(rest)?;
    let (origin_x, rest) = take_i16(rest)?;
    let (origin_y, rest) = take_i16(rest)?;
    let (&[frame_count, cover, hover, pressed, alternative, label_length], rest) =
        rest.split_first_chunk::<6>().ok_or(RecordFault::Length)?;
    let (frames, rest) = rest.split_at_checked(usize::from(frame_count)).ok_or(RecordFault::Length)?;
    let (label, rest) = take_text(rest, label_length, MAX_LABEL_BYTES)?;
    let looks = [hover, pressed, alternative];
    let of_its_kind = match kind {
        ObjectKind::Macro => (1..=MAX_OBJECT_FRAMES).contains(&frames.len()) && cover == 0,
        ObjectKind::Region => frames.is_empty() && (origin_x, origin_y) == (0, 0),
    };
    let in_range = number != 0
        && flags & !(SHOWN | DETECTABLE) == 0
        && (1..=MAX_LAYER).contains(&layer)
        && (1..=RASTER_WIDTH).contains(&width)
        && (1..=RASTER_HEIGHT).contains(&height)
        && looks.iter().all(|look| *look == 0 || is_macro(*look))
        && frames.iter().all(|frame| is_macro(*frame));
    if !of_its_kind || !in_range {
        return Err(refused);
    }
    let object = NaplpsObject {
        number,
        kind,
        shown: flags & SHOWN != 0,
        detectable: flags & DETECTABLE != 0,
        layer,
        order: order.cast_signed(),
        x,
        y,
        width,
        height,
        origin: (origin_x, origin_y),
        cover,
        looks,
        frames,
    };
    Ok((LabelledObject { object, label }, rest))
}

/// One `NAPLPS_OBJECTS` record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NaplpsObjectsRecord<'a> {
    /// The page.
    pub page: PageAddress,
    /// Its sub-page.
    pub subcode: SubCode,
    /// The generation of the NAPLPS stream the objects belong to: objects
    /// for another generation than the one shown are kept until that one
    /// is, and dropped when a later one arrives.
    pub generation: u16,
    /// How many objects: 0 to 64; 1 when the scope is [`Scope::One`].
    pub count: u8,
    /// Whether the record gives all the sub-page's objects or redefines
    /// one.
    pub scope: Scope,
    /// The picture's title, with [`Scope::Whole`]: `None` for no title.
    /// Always `None` with [`Scope::One`], which keeps the title.
    pub title: Option<&'a str>,
    /// The objects' bytes, checked: [`NaplpsObjectsRecord::objects`] reads
    /// them.
    bytes: &'a [u8],
}

impl<'a> NaplpsObjectsRecord<'a> {
    /// The objects, in the record's order.
    pub fn objects(&self) -> impl Iterator<Item = NaplpsObject<'a>> + 'a {
        self.labelled_objects().map(|labelled| labelled.object)
    }

    /// The objects with their labels, in the record's order.
    pub fn labelled_objects(&self) -> impl Iterator<Item = LabelledObject<'a>> + 'a {
        let mut rest = self.bytes;
        let mut left = self.count;
        core::iter::from_fn(move || {
            left = left.checked_sub(1)?;
            let (object, after) = read_object(rest).ok()?;
            rest = after;
            Some(object)
        })
    }
}

/// Reads a `NAPLPS_OBJECTS` record's body.
///
/// # Errors
/// A page or sub-code outside its range, more than [`MAX_OBJECTS`], a
/// reserved bit that is not 0, ONE with a count but 1 or with TITLED, an
/// object's field outside its range or not of its kind, a label or a title
/// too long, not UTF-8 or holding a control character, an empty title, a
/// number repeated, or bytes after the last object or the title.
pub fn read(body: &[u8]) -> Result<NaplpsObjectsRecord<'_>, RecordFault> {
    let (page, subcode, rest) = take_place(body)?;
    let (generation, rest) = take_u16(rest)?;
    let (&[count, flags], bytes) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    if usize::from(count) > MAX_OBJECTS {
        return Err(RecordFault::BadValue("naplps object count"));
    }
    if flags & !(ONE | TITLED) != 0 {
        return Err(RecordFault::BadValue("naplps objects reserved flag"));
    }
    let scope = if flags & ONE == 0 { Scope::Whole } else { Scope::One };
    if scope == Scope::One && (count != 1 || flags & TITLED != 0) {
        return Err(RecordFault::BadValue("naplps one object"));
    }
    let mut numbers = [0_u16; MAX_OBJECTS];
    let mut rest = bytes;
    for place in 0..usize::from(count) {
        let (LabelledObject { object, .. }, after) = read_object(rest)?;
        if numbers.iter().take(place).any(|number| *number == object.number) {
            return Err(RecordFault::BadValue("naplps object number repeated"));
        }
        if let Some(slot) = numbers.get_mut(place) {
            *slot = object.number;
        }
        rest = after;
    }
    let title = if flags & TITLED == 0 {
        None
    } else {
        let (&length, after) = rest.split_first().ok_or(RecordFault::Length)?;
        if length == 0 {
            return Err(RecordFault::BadValue("naplps title empty"));
        }
        let (title, after) = take_text(after, length, MAX_TITLE_BYTES)?;
        rest = after;
        Some(title)
    };
    if !rest.is_empty() {
        return Err(RecordFault::Length);
    }
    Ok(NaplpsObjectsRecord { page, subcode, generation, count, scope, title, bytes })
}

/// What a writer of a `NAPLPS_OBJECTS` record writes: all the sub-page's
/// objects with the picture's title, or one object redefined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contents<'a> {
    /// All the sub-page's objects, replacing any it had, and the
    /// picture's title, `None` for none ([`Scope::Whole`]).
    Whole {
        /// The objects, 0 to [`MAX_OBJECTS`].
        objects: &'a [LabelledObject<'a>],
        /// The picture's title: 1 to [`MAX_TITLE_BYTES`] of plain text.
        title: Option<&'a str>,
    },
    /// One object redefined, every other and the title kept
    /// ([`Scope::One`]).
    One(LabelledObject<'a>),
}

/// Writes the body of a `NAPLPS_OBJECTS` record of `objects` for a
/// sub-page and a stream generation into `body`, and gives its length:
/// the bytes [`read`] reads, which are also the body a group of a page's
/// rule table holds for this command (page-rules.md RULE-119), without the
/// record frame. `body` of [`MAX_BYTES`] always has room. The objects have
/// no label and the picture no title; [`write_contents`] writes those, and
/// one object redefined.
///
/// # Errors
/// What the reader would refuse, or [`RecordFault::Length`] when `body`
/// is too short.
pub fn write_body(
    place: (PageAddress, SubCode, u16),
    objects: &[NaplpsObject<'_>],
    body: &mut [u8],
) -> Result<usize, RecordFault> {
    let labelled = objects.iter().map(|object| LabelledObject { object: *object, label: "" });
    write_any(place, Scope::Whole, labelled, None, body)
}

/// Writes the body of a `NAPLPS_OBJECTS` record of `contents` for a
/// sub-page and a stream generation into `body`, and gives its length, as
/// [`write_body`] does, with each object's label, and either all the
/// objects with the picture's title or one object redefined (DEC-6638
/// items 3 and 6, TODO-6657).
///
/// # Errors
/// What the reader would refuse, or [`RecordFault::Length`] when `body`
/// is too short.
pub fn write_contents(
    place: (PageAddress, SubCode, u16),
    contents: &Contents<'_>,
    body: &mut [u8],
) -> Result<usize, RecordFault> {
    match *contents {
        Contents::Whole { objects, title } => write_any(place, Scope::Whole, objects.iter().copied(), title, body),
        Contents::One(object) => write_any(place, Scope::One, core::iter::once(object), None, body),
    }
}

/// The one writer of the body, for every form.
fn write_any<'o, I>(
    (page, subcode, generation): (PageAddress, SubCode, u16),
    scope: Scope,
    objects: I,
    title: Option<&str>,
    body: &mut [u8],
) -> Result<usize, RecordFault>
where
    I: Iterator<Item = LabelledObject<'o>> + Clone,
{
    let mut length = 0_usize;
    let mut put = |bytes: &[u8]| {
        let end = length.checked_add(bytes.len()).ok_or(RecordFault::Length)?;
        let room = body.get_mut(length..end).ok_or(RecordFault::Length)?;
        room.copy_from_slice(bytes);
        length = end;
        Ok::<(), RecordFault>(())
    };
    let count = u8::try_from(objects.clone().count()).map_err(|_| RecordFault::BadValue("naplps object count"))?;
    let flags = (if scope == Scope::One { ONE } else { 0 }) | (if title.is_some() { TITLED } else { 0 });
    put(&page.mpp().to_be_bytes())?;
    put(&subcode.value().to_be_bytes())?;
    put(&generation.to_be_bytes())?;
    put(&[count, flags])?;
    let text_length = |text: &str| u8::try_from(text.len()).map_err(|_| RecordFault::BadValue("naplps text"));
    for LabelledObject { object, label } in objects.clone() {
        let frames = u8::try_from(object.frames.len()).map_err(|_| RecordFault::BadValue("naplps object"))?;
        let object_flags = (if object.shown { SHOWN } else { 0 }) | (if object.detectable { DETECTABLE } else { 0 });
        put(&object.number.to_be_bytes())?;
        put(&[object.kind.code(), object_flags, object.layer])?;
        put(&object.order.to_be_bytes())?;
        for field in [object.x, object.y] {
            put(&field.to_be_bytes())?;
        }
        for field in [object.width, object.height] {
            put(&field.to_be_bytes())?;
        }
        for field in [object.origin.0, object.origin.1] {
            put(&field.to_be_bytes())?;
        }
        put(&[frames, object.cover])?;
        put(&object.looks)?;
        put(&[text_length(label)?])?;
        put(object.frames)?;
        put(label.as_bytes())?;
    }
    if let Some(title) = title {
        put(&[text_length(title)?])?;
        put(title.as_bytes())?;
    }
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    let back = read(body)?;
    if (back.page, back.subcode, back.generation, back.scope, back.title) != (page, subcode, generation, scope, title)
        || back.labelled_objects().count() != usize::from(count)
        || !back.labelled_objects().zip(objects).all(|(read, written)| read == written)
    {
        return Err(RecordFault::BadValue("does not read back"));
    }
    Ok(length)
}

/// Writes a `NAPLPS_OBJECTS` record of `objects` for a sub-page and a
/// stream generation as packets 4/31 of audio service `service`, record
/// id `record_id`, with no label and no title, as [`write_body`]. Returns
/// the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    place: (PageAddress, SubCode, u16),
    objects: &[NaplpsObject<'_>],
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BYTES];
    let length = write_body(place, objects, &mut body)?;
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    write_packets(TYPE_NAPLPS_OBJECTS, body, service, record_id, out).ok_or(RecordFault::Length)
}

/// Writes a `NAPLPS_OBJECTS` record of `contents` as packets 4/31 of
/// audio service `service`, record id `record_id`, as [`write_contents`]
/// writes its body. Returns the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode_contents(
    place: (PageAddress, SubCode, u16),
    contents: &Contents<'_>,
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BYTES];
    let length = write_contents(place, contents, &mut body)?;
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    write_packets(TYPE_NAPLPS_OBJECTS, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
