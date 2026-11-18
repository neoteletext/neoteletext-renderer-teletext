//! The `NAPLPS_OBJECTS` record, type 25 (neoteletext-common
//! `docs/animation.md`, DEC-6524; ANIM-11): all of a sub-page's NAPLPS
//! objects, replacing any it had. NAPLPS (ANSI X3.110-1983) keeps no
//! scene, so the layer of numbered objects over a stream's picture is
//! defined by this record, and every stream stays a conforming one.
//!
//! ```text
//! bytes 0-1    page: 0x100 to 0x8FF
//! bytes 2-3    sub: the sub-code, 0x0000 to 0x3F7E
//! bytes 4-5    generation: of the NAPLPS stream the objects belong to
//! byte  6      count: 0 to 64
//! byte  7      reserved: 0
//! then for each object:
//! bytes 0-1    number: 1 to 65,535, distinct within the record
//! byte  2      kind: 1 a macro object, 2 a region
//! byte  3      flags: bit 0 shown, bit 1 detectable
//! byte  4      layer: 1 to 3
//! byte  5      order: -128 to 127
//! bytes 6-9    x, y: the top left of its rectangle, in the picture's pixels
//! bytes 10-13  width, height: 1 to 640, 1 to 480
//! bytes 14-17  origin x, origin y: 0 for a region
//! byte  18     frames F: a macro object 1 to 16, a region 0
//! byte  19     cover: a region's colour-map entry; a macro object 0
//! bytes 20-22  hover, pressed and alternative look: a macro's code, or 0
//! byte  23     reserved: 0
//! bytes 24-    F frame bytes, each a macro's code
//! ```

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::limits::{MAX_FRAME, MAX_LAYER};

use super::{RecordFault, TYPE_NAPLPS_OBJECTS, take_place, take_u16, write_packets};

/// The most objects a sub-page has (ANIM-11, ANIM-16). The NAPLPS
/// renderer's limits define the same figure; this is the record's.
pub const MAX_OBJECTS: usize = 64;
/// The most frames an object has (ANIM-14): frames 0 to 15.
pub const MAX_OBJECT_FRAMES: usize = MAX_FRAME as usize + 1;
/// The fixed fields: page, sub-code, generation, count and the reserved
/// byte.
const FIXED: usize = 2 + 2 + 2 + 1 + 1;
/// An object's fixed fields, before its frame bytes.
const OBJECT_FIXED: usize = 24;
/// The most bytes of a `NAPLPS_OBJECTS` record's body.
pub const MAX_BYTES: usize = FIXED + MAX_OBJECTS * (OBJECT_FIXED + MAX_OBJECT_FRAMES);
const _: () = assert!(MAX_BYTES <= neoteletext_audio::MAX_FRAMED_BODY);

/// An object's flag: shown.
const SHOWN: u8 = 1;
/// An object's flag: detectable.
const DETECTABLE: u8 = 1 << 1;
/// The first macro code (ANSI X3.110-1983 section 6.2): 2/0.
pub const FIRST_MACRO: u8 = 0x20;
/// The last macro code: 7/15. The 96 macro names lie between.
pub const LAST_MACRO: u8 = 0x7F;
/// The picture's width in pixels (ANIM-11).
pub const PICTURE_WIDTH: u16 = 640;
/// The picture's height in pixels.
pub const PICTURE_HEIGHT: u16 = 480;

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
    /// The left of its rectangle, in the picture's pixels.
    pub x: i16,
    /// The top of its rectangle.
    pub y: i16,
    /// Its rectangle's width, 1 to 640.
    pub width: u16,
    /// Its rectangle's height, 1 to 480.
    pub height: u16,
    /// Where the rectangle's top left lies in the macro's own drawing;
    /// (0, 0) for a region.
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

/// Whether `code` is a macro's code, 2/0 to 7/15.
const fn is_macro(code: u8) -> bool {
    code >= FIRST_MACRO && code <= LAST_MACRO
}

/// A signed 16-bit field at the front of `bytes`, and what follows it.
fn take_i16(bytes: &[u8]) -> Result<(i16, &[u8]), RecordFault> {
    let (value, rest) = take_u16(bytes)?;
    Ok((value.cast_signed(), rest))
}

/// Reads one object from the front of `bytes`, and what follows it.
fn read_object(bytes: &[u8]) -> Result<(NaplpsObject<'_>, &[u8]), RecordFault> {
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
    let (&[frame_count, cover, hover, pressed, alternative, reserved], rest) =
        rest.split_first_chunk::<6>().ok_or(RecordFault::Length)?;
    let (frames, rest) = rest.split_at_checked(usize::from(frame_count)).ok_or(RecordFault::Length)?;
    let looks = [hover, pressed, alternative];
    let of_its_kind = match kind {
        ObjectKind::Macro => (1..=MAX_OBJECT_FRAMES).contains(&frames.len()) && cover == 0,
        ObjectKind::Region => frames.is_empty() && (origin_x, origin_y) == (0, 0),
    };
    let in_range = number != 0
        && flags & !(SHOWN | DETECTABLE) == 0
        && (1..=MAX_LAYER).contains(&layer)
        && (1..=PICTURE_WIDTH).contains(&width)
        && (1..=PICTURE_HEIGHT).contains(&height)
        && reserved == 0
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
    Ok((object, rest))
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
    /// How many objects: 0 to 64.
    pub count: u8,
    /// The objects' bytes, checked: [`NaplpsObjectsRecord::objects`] reads
    /// them.
    bytes: &'a [u8],
}

impl<'a> NaplpsObjectsRecord<'a> {
    /// The objects, in the record's order.
    pub fn objects(&self) -> impl Iterator<Item = NaplpsObject<'a>> + 'a {
        let mut rest = self.bytes;
        core::iter::from_fn(move || {
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
/// reserved byte or bit that is not 0, an object's field outside its
/// range or not of its kind, a number repeated, or bytes after the last
/// object.
pub fn read(body: &[u8]) -> Result<NaplpsObjectsRecord<'_>, RecordFault> {
    let (page, subcode, rest) = take_place(body)?;
    let (generation, rest) = take_u16(rest)?;
    let (&[count, reserved], bytes) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    if usize::from(count) > MAX_OBJECTS {
        return Err(RecordFault::BadValue("naplps object count"));
    }
    if reserved != 0 {
        return Err(RecordFault::BadValue("naplps objects reserved byte"));
    }
    let mut numbers = [0_u16; MAX_OBJECTS];
    let mut rest = bytes;
    for place in 0..usize::from(count) {
        let (object, after) = read_object(rest)?;
        if numbers.iter().take(place).any(|number| *number == object.number) {
            return Err(RecordFault::BadValue("naplps object number repeated"));
        }
        if let Some(slot) = numbers.get_mut(place) {
            *slot = object.number;
        }
        rest = after;
    }
    if !rest.is_empty() {
        return Err(RecordFault::Length);
    }
    Ok(NaplpsObjectsRecord { page, subcode, generation, count, bytes })
}

/// Writes a `NAPLPS_OBJECTS` record of `objects` for a sub-page and a
/// stream generation as packets 4/31 of audio service `service`, record
/// id `record_id`. Returns the packets written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    (page, subcode, generation): (PageAddress, SubCode, u16),
    objects: &[NaplpsObject<'_>],
    service: u8,
    record_id: u8,
    out: &mut dyn FnMut(Packet),
) -> Result<usize, RecordFault> {
    let mut body = [0_u8; MAX_BYTES];
    let mut length = 0_usize;
    let mut put = |bytes: &[u8]| {
        let end = length.checked_add(bytes.len()).ok_or(RecordFault::Length)?;
        let room = body.get_mut(length..end).ok_or(RecordFault::Length)?;
        room.copy_from_slice(bytes);
        length = end;
        Ok::<(), RecordFault>(())
    };
    let count = u8::try_from(objects.len()).map_err(|_| RecordFault::BadValue("naplps object count"))?;
    put(&page.mpp().to_be_bytes())?;
    put(&subcode.value().to_be_bytes())?;
    put(&generation.to_be_bytes())?;
    put(&[count, 0])?;
    for object in objects {
        let frames = u8::try_from(object.frames.len()).map_err(|_| RecordFault::BadValue("naplps object"))?;
        let flags = (if object.shown { SHOWN } else { 0 }) | (if object.detectable { DETECTABLE } else { 0 });
        put(&object.number.to_be_bytes())?;
        put(&[object.kind.code(), flags, object.layer])?;
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
        put(&[0])?;
        put(object.frames)?;
    }
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    let back = read(body)?;
    if (back.page, back.subcode, back.generation) != (page, subcode, generation)
        || !back.objects().eq(objects.iter().copied())
    {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_NAPLPS_OBJECTS, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
