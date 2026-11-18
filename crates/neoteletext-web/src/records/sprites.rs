//! The SPRITES record, type 24 (neoteletext-common `docs/animation.md`,
//! DEC-6524; ANIM-7): all of a sub-page's teletext sprites, replacing any
//! it had. A sprite is a numbered placement of teletext objects
//! (EN 300 706 clause 13) that the receiver draws over the page and can
//! move, show, hide, order and cycle.
//!
//! ```text
//! bytes 0-1   page: 0x100 to 0x8FF
//! bytes 2-3   sub: the sub-code, 0x0000 to 0x3F7E
//! byte  4     count: 0 to 32
//! byte  5     reserved: 0
//! then for each sprite:
//! bytes 0-1   number: 1 to 65,535, distinct within the record
//! byte  2     flags: bit 0 shown, bit 1 detectable
//! byte  3     layer: 1 to 3
//! byte  4     order: -128 to 127
//! byte  5     column: -39 to 39
//! byte  6     row: -24 to 24
//! byte  7     width: 1 to 40 cells
//! byte  8     height: 1 to 25 cells
//! byte  9     frames F: 1 to 16
//! bytes 10-   F object references, 2 bytes each
//! ```

use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_rules::limits::{MAX_FRAME, MAX_LAYER};

use super::{RecordFault, TYPE_SPRITES, take_place, take_u16, write_packets};

/// The most sprites a sub-page has (ANIM-7, ANIM-16): the engine core's
/// limit, which the renderer holds, so that a record never gives more
/// than is drawn.
pub const MAX_SPRITES: usize = neoteletext_core::limits::MAX_SPRITES;
/// The most frames a sprite has (ANIM-14): frames 0 to 15, the core's
/// limit too.
pub const MAX_OBJECT_FRAMES: usize = neoteletext_core::limits::MAX_OBJECT_FRAMES;
// And the highest frame an operation of the rule table's crate names is
// the last of them (ANIM-6).
const _: () = assert!(MAX_OBJECT_FRAMES == MAX_FRAME as usize + 1);
// A layer, likewise: the record's highest is the core's.
const _: () = assert!(MAX_LAYER == neoteletext_core::limits::LAST_LAYER);
/// The fixed fields: page, sub-code, count and the reserved byte.
const FIXED: usize = 2 + 2 + 1 + 1;
/// A sprite's fixed fields, before its object references.
const SPRITE_FIXED: usize = 2 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1;
/// The bytes of an object reference.
const REFERENCE_BYTES: usize = 2;
/// The most bytes of a SPRITES record's body.
pub const MAX_BYTES: usize = FIXED + MAX_SPRITES * (SPRITE_FIXED + MAX_OBJECT_FRAMES * REFERENCE_BYTES);
const _: () = assert!(MAX_BYTES <= neoteletext_audio::MAX_FRAMED_BODY);

/// A sprite's flag: shown.
const SHOWN: u8 = 1;
/// A sprite's flag: detectable, so that pointer triggers see it.
const DETECTABLE: u8 = 1 << 1;
/// The farthest column of a sprite's origin, either way: one short of the
/// page's 40 columns.
const MAX_COLUMN: i8 = 39;
/// The farthest row of a sprite's origin, either way: one short of the
/// page's 25 rows.
const MAX_ROW: i8 = 24;
/// The widest sprite, in cells: the page.
const MAX_WIDTH: u8 = 40;
/// The tallest sprite, in cells: the page.
const MAX_HEIGHT: u8 = 25;

/// The bits of an object reference that are zero (ANIM-7): 15 to 13.
const REFERENCE_RESERVED: u16 = 0b1110_0000_0000_0000;
/// Where an object reference's source is: bits 12 and 11.
const SOURCE_SHIFT: u16 = 11;
/// Where its type is: bits 10 and 9.
const TYPE_SHIFT: u16 = 9;
/// Two bits.
const TWO_BITS: u16 = 0b11;
/// The object number, N8 to N0: bits 8 to 0.
const NUMBER_MASK: u16 = 0x01FF;

/// A reference to a teletext object: the fields of an Object Invocation
/// triplet (EN 300 706 Table 28).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObjectReference(u16);

impl ObjectReference {
    /// The reference of `bits`, or `None` when a reserved bit is set, or
    /// the source or the type is 0.
    #[must_use]
    pub const fn new(bits: u16) -> Option<Self> {
        let reference = Self(bits);
        if bits & REFERENCE_RESERVED == 0 && reference.source() != 0 && reference.kind() != 0 {
            Some(reference)
        } else {
            None
        }
    }

    /// The reference's 16 bits, as the record carries them.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// The source: 1 a local object of the page's own packets X/26, 2 an
    /// object of its Public Object Page, 3 of its Global Public Object
    /// Page.
    #[must_use]
    pub const fn source(self) -> u8 {
        ((self.0 >> SOURCE_SHIFT) & TWO_BITS) as u8
    }

    /// The type: 1 active, 2 adaptive, 3 passive.
    #[must_use]
    pub const fn kind(self) -> u8 {
        ((self.0 >> TYPE_SHIFT) & TWO_BITS) as u8
    }

    /// The object number, N8 to N0.
    #[must_use]
    pub const fn number(self) -> u16 {
        self.0 & NUMBER_MASK
    }
}

/// One sprite of a SPRITES record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sprite<'a> {
    /// Its number, 1 to 65,535: what a rule's target and an animation
    /// operation name.
    pub number: u16,
    /// Whether it is shown.
    pub shown: bool,
    /// Whether pointer triggers see it.
    pub detectable: bool,
    /// Its layer, 1 to 3: layer 0 is the page itself.
    pub layer: u8,
    /// Its order within the layer.
    pub order: i8,
    /// The column of its origin, -39 to 39.
    pub column: i8,
    /// The row of its origin, -24 to 24.
    pub row: i8,
    /// Its width in cells, 1 to 40.
    pub width: u8,
    /// Its height in cells, 1 to 25.
    pub height: u8,
    /// Its frames' object references, two bytes each, checked: 1 to 16 of
    /// them. [`Sprite::frames`] reads them.
    references: &'a [u8],
}

impl<'a> Sprite<'a> {
    /// A sprite with `frames` for its frames' object references, two
    /// bytes each, big-endian. [`encode`] checks them.
    #[must_use]
    #[allow(clippy::too_many_arguments, reason = "the record's fields, in its order: a sprite has this many")]
    pub const fn new(
        number: u16,
        shown: bool,
        detectable: bool,
        layer: u8,
        order: i8,
        (column, row): (i8, i8),
        (width, height): (u8, u8),
        frames: &'a [u8],
    ) -> Self {
        Self { number, shown, detectable, layer, order, column, row, width, height, references: frames }
    }

    /// The object each frame draws, frame 0 first.
    pub fn frames(&self) -> impl Iterator<Item = ObjectReference> + 'a {
        let (pairs, _) = self.references.as_chunks::<REFERENCE_BYTES>();
        pairs.iter().filter_map(|pair| ObjectReference::new(u16::from_be_bytes(*pair)))
    }
}

/// One SPRITES record, checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpritesRecord<'a> {
    /// The page.
    pub page: PageAddress,
    /// Its sub-page.
    pub subcode: SubCode,
    /// How many sprites: 0 to 32. None takes every sprite away.
    pub count: u8,
    /// The sprites' bytes, checked: [`SpritesRecord::sprites`] reads them.
    bytes: &'a [u8],
}

impl<'a> SpritesRecord<'a> {
    /// The sprites, in the record's order.
    pub fn sprites(&self) -> impl Iterator<Item = Sprite<'a>> + 'a {
        let mut rest = self.bytes;
        core::iter::from_fn(move || {
            let (sprite, after) = read_sprite(rest).ok()?;
            rest = after;
            Some(sprite)
        })
    }
}

/// Reads one sprite from the front of `bytes`, and what follows it.
fn read_sprite(bytes: &[u8]) -> Result<(Sprite<'_>, &[u8]), RecordFault> {
    let refused = RecordFault::BadValue("sprite");
    let (number, rest) = take_u16(bytes)?;
    let (&[flags, layer, order, column, row, width, height, frames], rest) =
        rest.split_first_chunk::<8>().ok_or(RecordFault::Length)?;
    let (order, column, row) = (order.cast_signed(), column.cast_signed(), row.cast_signed());
    let in_range = number != 0
        && flags & !(SHOWN | DETECTABLE) == 0
        && (1..=MAX_LAYER).contains(&layer)
        && (-MAX_COLUMN..=MAX_COLUMN).contains(&column)
        && (-MAX_ROW..=MAX_ROW).contains(&row)
        && (1..=MAX_WIDTH).contains(&width)
        && (1..=MAX_HEIGHT).contains(&height)
        && (1..=MAX_OBJECT_FRAMES).contains(&usize::from(frames));
    if !in_range {
        return Err(refused);
    }
    let length = usize::from(frames).saturating_mul(REFERENCE_BYTES);
    let (references, rest) = rest.split_at_checked(length).ok_or(RecordFault::Length)?;
    for pair in references.as_chunks::<REFERENCE_BYTES>().0 {
        ObjectReference::new(u16::from_be_bytes(*pair)).ok_or(RecordFault::BadValue("sprite object reference"))?;
    }
    let sprite = Sprite {
        number,
        shown: flags & SHOWN != 0,
        detectable: flags & DETECTABLE != 0,
        layer,
        order,
        column,
        row,
        width,
        height,
        references,
    };
    Ok((sprite, rest))
}

/// Reads a SPRITES record's body.
///
/// # Errors
/// A page or sub-code outside its range, more than [`MAX_SPRITES`], a
/// reserved byte or bit that is not 0, a sprite's field outside its
/// range, a number repeated, an object reference with a reserved bit or a
/// source or type of 0, or bytes after the last sprite.
pub fn read(body: &[u8]) -> Result<SpritesRecord<'_>, RecordFault> {
    let (page, subcode, rest) = take_place(body)?;
    let (&[count, reserved], bytes) = rest.split_first_chunk::<2>().ok_or(RecordFault::Length)?;
    if usize::from(count) > MAX_SPRITES {
        return Err(RecordFault::BadValue("sprite count"));
    }
    if reserved != 0 {
        return Err(RecordFault::BadValue("sprites reserved byte"));
    }
    let mut numbers = [0_u16; MAX_SPRITES];
    let mut rest = bytes;
    for place in 0..usize::from(count) {
        let (sprite, after) = read_sprite(rest)?;
        if numbers.iter().take(place).any(|number| *number == sprite.number) {
            return Err(RecordFault::BadValue("sprite number repeated"));
        }
        if let Some(slot) = numbers.get_mut(place) {
            *slot = sprite.number;
        }
        rest = after;
    }
    if !rest.is_empty() {
        return Err(RecordFault::Length);
    }
    Ok(SpritesRecord { page, subcode, count, bytes })
}

/// Writes a SPRITES record of `sprites` for a sub-page as packets 4/31 of
/// audio service `service`, record id `record_id`. Returns the packets
/// written.
///
/// # Errors
/// What the reader would refuse.
pub fn encode(
    page: PageAddress,
    subcode: SubCode,
    sprites: &[Sprite<'_>],
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
    let count = u8::try_from(sprites.len()).map_err(|_| RecordFault::BadValue("sprite count"))?;
    put(&page.mpp().to_be_bytes())?;
    put(&subcode.value().to_be_bytes())?;
    put(&[count, 0])?;
    for sprite in sprites {
        let frames = u8::try_from(sprite.references.len().checked_div(REFERENCE_BYTES).unwrap_or(0))
            .map_err(|_| RecordFault::BadValue("sprite"))?;
        let flags = (if sprite.shown { SHOWN } else { 0 }) | (if sprite.detectable { DETECTABLE } else { 0 });
        put(&sprite.number.to_be_bytes())?;
        put(&[flags, sprite.layer])?;
        put(&sprite.order.to_be_bytes())?;
        put(&sprite.column.to_be_bytes())?;
        put(&sprite.row.to_be_bytes())?;
        put(&[sprite.width, sprite.height, frames])?;
        put(sprite.references)?;
    }
    let body = body.get(..length).ok_or(RecordFault::Length)?;
    // The writer checks with the reader, so the two cannot disagree.
    let back = read(body)?;
    if (back.page, back.subcode) != (page, subcode) || !back.sprites().eq(sprites.iter().copied()) {
        return Err(RecordFault::BadValue("does not read back"));
    }
    write_packets(TYPE_SPRITES, body, service, record_id, out).ok_or(RecordFault::Length)
}

#[cfg(test)]
mod tests;
