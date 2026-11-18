//! The bodies of the three records a script applies to a teletext page
//! (animation.md ANIM-18): FRAME (type 27, ANIM-2), ANIMATE (type 23,
//! ANIM-6) and SPRITES (type 24, ANIM-7), read into the engine's values.
//! Every integer is big-endian. A body is input from outside: a field
//! outside its range, a reserved bit or byte that is not 0, a count over
//! its limit, a short body and bytes after its end each refuse it whole.

use neoteletext_core::limits::{MAX_OBJECT_FRAMES, MAX_SPRITES};
use neoteletext_core::sprites::{ForcedLook, FrameRef, Operation, Sprite, SpriteError, Sprites, Target};
use neoteletext_core::types::FixedVec;

/// The sub-code of an ANIMATE record that names whichever sub-page of the
/// page is shown (ANIM-6): the sub-code that names no one sub-page.
pub const SUBCODE_SHOWN: u16 = neoteletext_core::limits::SUBCODE_ANY;
/// The most operations in an ANIMATE record (ANIM-6).
pub const MAX_ANIMATE_OPERATIONS: usize = 64;
/// The marks of a FRAME record (ANIM-2).
const MARK_BEGIN: u8 = 1;
const MARK_END: u8 = 2;
/// The kind bytes of a target (ANIM-6).
const TARGET_CONTROL: u8 = 1;
const TARGET_OBJECT: u8 = 2;
/// The code bytes of the operations (ANIM-6).
const CODE_SHOW: u8 = 5;
const CODE_HIDE: u8 = 6;
const CODE_LOOK: u8 = 7;
const CODE_MOVE: u8 = 15;
const CODE_ORDER: u8 = 16;
const CODE_FRAME: u8 = 17;
const CODE_CYCLE: u8 = 18;
const CODE_STOP_CYCLE: u8 = 19;
const CODE_RESET: u8 = 20;
/// Bit 0 of `move`'s flags: relative; of `cycle`'s: once. The other bits
/// are 0.
const FLAG: u8 = 0x01;
/// The flags of a sprite: bit 0 shown, bit 1 detectable; the other bits 0
/// (ANIM-7).
const SPRITE_SHOWN: u8 = 0x01;
const SPRITE_DETECTABLE: u8 = 0x02;

/// The bytes of a body, read from its start.
struct Reader<'a> {
    rest: &'a [u8],
}

impl Reader<'_> {
    fn u8(&mut self) -> Result<u8, String> {
        let (&byte, rest) = self.rest.split_first().ok_or("the body is short")?;
        self.rest = rest;
        Ok(byte)
    }

    fn i8(&mut self) -> Result<i8, String> {
        Ok(i8::from_be_bytes([self.u8()?]))
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes([self.u8()?, self.u8()?]))
    }

    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_be_bytes([self.u8()?, self.u8()?]))
    }

    fn reserved(&mut self) -> Result<(), String> {
        if self.u8()? == 0 { Ok(()) } else { Err("a reserved byte is not 0".to_owned()) }
    }

    /// A flags byte of which only `allowed` may be set.
    fn flags(&mut self, allowed: u8) -> Result<u8, String> {
        let flags = self.u8()?;
        if flags & !allowed == 0 { Ok(flags) } else { Err("a reserved bit is not 0".to_owned()) }
    }

    fn end(&self) -> Result<(), String> {
        if self.rest.is_empty() { Ok(()) } else { Err("bytes follow the body's end".to_owned()) }
    }
}

/// The beginning or the end of an atomic frame (ANIM-2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameMark {
    /// The page.
    pub page: u16,
    /// The sub-code.
    pub subcode: u16,
    /// True for a begin, false for an end.
    pub begin: bool,
    /// The identifier, which an end repeats from its begin.
    pub identifier: u16,
}

/// Reads a FRAME record's body: the page, the sub-code, a mark (1 begin,
/// 2 end), a reserved byte and an identifier.
///
/// # Errors
/// Why the body is refused.
pub fn frame(body: &[u8]) -> Result<FrameMark, String> {
    let mut reader = Reader { rest: body };
    let (page, subcode) = (reader.u16()?, reader.u16()?);
    let begin = match reader.u8()? {
        MARK_BEGIN => true,
        MARK_END => false,
        mark => return Err(format!("the mark is 1 or 2, not {mark}")),
    };
    reader.reserved()?;
    let identifier = reader.u16()?;
    reader.end()?;
    Ok(FrameMark { page, subcode, begin, identifier })
}

/// An ANIMATE record: operations on the objects and controls of a
/// sub-page (ANIM-6).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Animate {
    /// The page.
    pub page: u16,
    /// The sub-code, or [`SUBCODE_SHOWN`].
    pub subcode: u16,
    /// The operations, 1 to 64, applied together.
    pub operations: Vec<(Target, Operation)>,
}

fn operation(reader: &mut Reader<'_>) -> Result<(Target, Operation), String> {
    let code = reader.u8()?;
    let kind = reader.u8()?;
    let number = reader.u16()?;
    if number == 0 {
        return Err("a target's number is 0".to_owned());
    }
    let target = match kind {
        TARGET_CONTROL => Target::Control(number),
        TARGET_OBJECT => Target::Object(number),
        _ => return Err(format!("a target's kind is 1 or 2, not {kind}")),
    };
    let objects_only = || {
        if kind == TARGET_OBJECT { Ok(()) } else { Err(format!("operation {code} applies to objects only")) }
    };
    let operation = match code {
        CODE_SHOW => Operation::Show,
        CODE_HIDE => Operation::Hide,
        CODE_LOOK => Operation::Look(ForcedLook::from_code(reader.u8()?).ok_or("a look is 0 to 3")?),
        CODE_MOVE => {
            objects_only()?;
            let relative = reader.flags(FLAG)? & FLAG != 0;
            Operation::Move { relative, x: reader.i16()?, y: reader.i16()? }
        }
        CODE_ORDER => {
            objects_only()?;
            let layer = reader.u8()?;
            Operation::order(layer, reader.i8()?).ok_or("a layer is 0 to 3, or 255 to keep it")?
        }
        CODE_FRAME => {
            objects_only()?;
            Operation::Frame(reader.u8()?)
        }
        CODE_CYCLE => {
            objects_only()?;
            let (first, last, period) = (reader.u8()?, reader.u8()?, reader.u16()?);
            Operation::Cycle { first, last, period, once: reader.flags(FLAG)? & FLAG != 0 }
        }
        CODE_STOP_CYCLE => {
            objects_only()?;
            Operation::StopCycle
        }
        CODE_RESET => Operation::Reset,
        _ => return Err(format!("{code} is not an operation's code")),
    };
    Ok((target, operation))
}

/// Reads an ANIMATE record's body: the page, the sub-code, a count of 1 to
/// 64, a reserved byte, then that many operations, each a code byte, a
/// target (a kind byte and a number, not 0) and a body. The ranges of a
/// frame and a cycle are held when the operation is applied
/// (`Animation::apply` refuses one outside them).
///
/// # Errors
/// Why the body is refused.
pub fn animate(body: &[u8]) -> Result<Animate, String> {
    let mut reader = Reader { rest: body };
    let (page, subcode) = (reader.u16()?, reader.u16()?);
    let count = usize::from(reader.u8()?);
    if count == 0 || count > MAX_ANIMATE_OPERATIONS {
        return Err(format!("the count is 1 to {MAX_ANIMATE_OPERATIONS}, not {count}"));
    }
    reader.reserved()?;
    let operations = (0..count).map(|_| operation(&mut reader)).collect::<Result<Vec<_>, _>>()?;
    reader.end()?;
    Ok(Animate { page, subcode, operations })
}

/// A SPRITES record: all the sprites of a sub-page (ANIM-7).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SpritesRecord {
    /// The page.
    pub page: u16,
    /// The sub-code.
    pub subcode: u16,
    /// The sprites, checked.
    pub sprites: Sprites,
}

fn sprite(reader: &mut Reader<'_>) -> Result<Sprite, String> {
    let number = reader.u16()?;
    let flags = reader.flags(SPRITE_SHOWN | SPRITE_DETECTABLE)?;
    let (layer, order) = (reader.u8()?, reader.i8()?);
    let (column, row) = (reader.i8()?, reader.i8()?);
    let (width, height) = (reader.u8()?, reader.u8()?);
    let count = usize::from(reader.u8()?);
    if count == 0 || count > MAX_OBJECT_FRAMES {
        return Err(format!("a sprite has 1 to {MAX_OBJECT_FRAMES} frames, not {count}"));
    }
    let first = FrameRef::from_reference(reader.u16()?).ok_or("an object reference is refused")?;
    let mut sprite = Sprite::new(number, (row, column), (height, width), first);
    for _ in 1..count {
        let frame = FrameRef::from_reference(reader.u16()?).ok_or("an object reference is refused")?;
        // The count is within the frames a sprite holds.
        let _ = sprite.frames.push(frame);
    }
    Ok(Sprite { shown: flags & SPRITE_SHOWN != 0, detectable: flags & SPRITE_DETECTABLE != 0, layer, order, ..sprite })
}

/// Reads a SPRITES record's body: the page, the sub-code, a count of 0 to
/// 32, a reserved byte, then for each sprite its number, flags, layer,
/// order, column, row, width, height, a count of frames and that many
/// two-byte object references.
///
/// # Errors
/// Why the body is refused: its own form, or a field outside the range
/// ANIM-7 gives it.
pub fn sprites(body: &[u8]) -> Result<SpritesRecord, String> {
    let mut reader = Reader { rest: body };
    let (page, subcode) = (reader.u16()?, reader.u16()?);
    let count = usize::from(reader.u8()?);
    if count > MAX_SPRITES {
        return Err(format!("the count is 0 to {MAX_SPRITES}, not {count}"));
    }
    reader.reserved()?;
    let mut list: FixedVec<Sprite, MAX_SPRITES> = FixedVec::new(Sprite::new(1, (0, 0), (1, 1), FILLER));
    for _ in 0..count {
        // The count is within the sprites a sub-page holds.
        let _ = list.push(sprite(&mut reader)?);
    }
    reader.end()?;
    let sprites = Sprites::new(list.as_slice()).map_err(|error| match error {
        SpriteError::TooMany => "more than 32 sprites",
        SpriteError::Number => "a sprite's number is 0",
        SpriteError::RepeatedNumber => "two sprites have one number",
        SpriteError::Layer => "a layer is not 1 to 3",
        SpriteError::Origin => "an origin is outside columns -39 to 39 or rows -24 to 24",
        SpriteError::Size => "a width is not 1 to 40, or a height not 1 to 25",
        SpriteError::Frames => "a sprite has no frame, or a frame of source 0",
    })?;
    Ok(SpritesRecord { page, subcode, sprites })
}

/// The filler of the unused slots of the list a record is read into.
const FILLER: FrameRef = FrameRef {
    source: neoteletext_core::packet::ObjectSource::Local,
    kind: neoteletext_core::packet::ObjectKind::Passive,
    number: neoteletext_core::packet::ObjectNumber(0),
};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]
mod tests {
    use super::*;
    use neoteletext_core::packet::{ObjectKind, ObjectNumber, ObjectSource};

    /// ANIM-2 (REQ-6529): the eight bytes of a FRAME record.
    #[test]
    fn a_frame_record_is_eight_bytes() {
        let begin = frame(&[0x01, 0x50, 0x00, 0x01, 1, 0, 0x12, 0x34]).unwrap();
        assert_eq!(begin, FrameMark { page: 0x150, subcode: 1, begin: true, identifier: 0x1234 });
        assert!(!frame(&[0x01, 0x50, 0x00, 0x01, 2, 0, 0x12, 0x34]).unwrap().begin);
        for refused in [
            &[0x01, 0x50, 0, 1, 0, 0, 0, 0][..],
            &[0x01, 0x50, 0, 1, 3, 0, 0, 0],
            &[0x01, 0x50, 0, 1, 1, 1, 0, 0],
            &[0x01, 0x50, 0, 1, 1, 0, 0],
            &[0x01, 0x50, 0, 1, 1, 0, 0, 0, 0],
        ] {
            assert!(frame(refused).is_err(), "{refused:?}");
        }
    }

    /// ANIM-6 (REQ-6529): each operation of an ANIMATE record, byte for
    /// byte, and what its reader refuses.
    #[test]
    fn an_animate_record_holds_each_operation() {
        let body = [
            &[0x01, 0x50, 0x3F, 0x7F, 9, 0][..],
            &[5, 2, 0, 10],
            &[6, 1, 0, 3],
            &[7, 2, 0, 10, 3],
            &[15, 2, 0, 10, 1, 0xFF, 0xF8, 0x00, 0x08],
            &[16, 2, 0, 10, 255, 0x80],
            &[17, 2, 0, 10, 15],
            &[18, 2, 0, 10, 0, 3, 0x0E, 0x10, 1],
            &[19, 2, 0, 10],
            &[20, 1, 0xFF, 0xFF],
        ]
        .concat();
        let record = animate(&body).unwrap();
        assert_eq!((record.page, record.subcode), (0x150, SUBCODE_SHOWN));
        let object = Target::Object(10);
        let expected = [
            (object, Operation::Show),
            (Target::Control(3), Operation::Hide),
            (object, Operation::Look(ForcedLook::Alternative)),
            (object, Operation::Move { relative: true, x: -8, y: 8 }),
            (object, Operation::Order { layer: None, order: -128 }),
            (object, Operation::Frame(15)),
            (object, Operation::Cycle { first: 0, last: 3, period: 3_600, once: true }),
            (object, Operation::StopCycle),
            (Target::Control(65_535), Operation::Reset),
        ];
        assert_eq!(record.operations, expected);

        let with = |operation: &[u8]| animate(&[&[0x01, 0x50, 0, 1, 1, 0][..], operation].concat());
        assert!(with(&[5, 2, 0, 10]).is_ok());
        for (refused, why) in [
            (&[5, 2, 0, 0][..], "number 0"),
            (&[5, 0, 0, 1], "kind 0"),
            (&[5, 3, 0, 1], "kind 3"),
            (&[4, 2, 0, 1], "not a code"),
            (&[21, 2, 0, 1], "not a code"),
            (&[7, 2, 0, 1, 4], "look 4"),
            (&[15, 1, 0, 1, 0, 0, 0, 0, 0], "move on a control"),
            (&[15, 2, 0, 1, 2, 0, 0, 0, 0], "a reserved flag"),
            (&[16, 2, 0, 1, 4, 0], "layer 4"),
            (&[18, 2, 0, 1, 0, 3, 0, 6, 2], "a reserved flag"),
            (&[5, 2, 0, 1, 0], "bytes after the end"),
            (&[15, 2, 0, 1, 0, 0, 0, 0], "short"),
        ] {
            assert!(with(refused).is_err(), "{why}");
        }
        assert!(animate(&[0x01, 0x50, 0, 1, 0, 0]).is_err(), "a count of 0");
        assert!(animate(&[0x01, 0x50, 0, 1, 1, 1, 5, 2, 0, 10]).is_err(), "the reserved byte");
        let many = |count: u8| {
            let mut body = vec![0x01, 0x50, 0, 1, count, 0];
            for _ in 0..count {
                body.extend_from_slice(&[5, 2, 0, 10]);
            }
            animate(&body)
        };
        assert_eq!(many(64).unwrap().operations.len(), 64);
        assert!(many(65).is_err());
    }

    /// ANIM-7 (REQ-6529): a SPRITES record at its bounds; its reader refuses
    /// a repeated number, a reserved bit and a source of 0.
    #[test]
    fn a_sprites_record_holds_a_sub_pages_sprites() {
        let passive_local: u16 = 0b01 << 11 | 0b11 << 9;
        let one = |number: u16, flags: u8, reference: u16| {
            let mut bytes = number.to_be_bytes().to_vec();
            bytes.extend_from_slice(&[flags, 2, 0xFF, 0xD9, 24, 40, 25, 2]);
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.extend_from_slice(&(passive_local | 0x1FF).to_be_bytes());
            bytes
        };
        let record = |list: &[Vec<u8>]| {
            let mut body = vec![0x01, 0x60, 0, 1, u8::try_from(list.len()).unwrap(), 0];
            body.extend(list.iter().flatten());
            sprites(&body)
        };
        let read = record(&[one(1, 0b11, passive_local), one(65_535, 0, passive_local)]).unwrap();
        assert_eq!((read.page, read.subcode, read.sprites.as_slice().len()), (0x160, 1, 2));
        let first = read.sprites.as_slice()[0];
        assert_eq!((first.number, first.shown, first.detectable, first.layer, first.order), (1, true, true, 2, -1));
        assert_eq!((first.column, first.row, first.width, first.height), (-39, 24, 40, 25));
        let frames = [
            FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(0) },
            FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(0x1FF) },
        ];
        assert_eq!(first.frames.as_slice(), frames);
        assert!(!read.sprites.as_slice()[1].shown);
        assert_eq!(record(&[]).unwrap().sprites, Sprites::NONE);

        assert!(record(&[one(1, 0, passive_local), one(1, 0, passive_local)]).is_err(), "a repeated number");
        assert!(record(&[one(1, 0b100, passive_local)]).is_err(), "a reserved flag");
        assert!(record(&[one(1, 0, 0b11 << 9)]).is_err(), "a source of 0");
        assert!(record(&[one(1, 0, passive_local | 0x2000)]).is_err(), "a reserved bit of a reference");
        assert!(record(&[one(0, 0, passive_local)]).is_err(), "number 0");
        let all: Vec<Vec<u8>> = (1..=33).map(|number| one(number, 1, passive_local)).collect();
        assert_eq!(record(&all[..32]).unwrap().sprites.as_slice().len(), 32);
        assert!(record(&all).is_err(), "a 33rd sprite");
        let mut long = vec![0x01, 0x60, 0, 1, 1, 0];
        long.extend(one(1, 1, passive_local));
        long.push(0);
        assert!(sprites(&long).is_err(), "bytes after the end");
        assert!(sprites(&long[..long.len() - 2]).is_err(), "short");
    }
}
