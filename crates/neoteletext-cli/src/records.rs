//! The records a script applies to a teletext page (animation.md ANIM-18):
//! FRAME (type 27, ANIM-2), ANIMATE (type 23, ANIM-6) and SPRITES (type
//! 24, ANIM-7). Their bodies are read by `neoteletext-web`'s readers, the
//! ones a receiver of a channel uses, so that each record has one reader;
//! what is here turns what those give into the engine core's values, which
//! draws them and depends on no other crate (REQ-GEN-017).

use neoteletext_core::limits::MAX_SPRITES;
use neoteletext_core::sprites::{FrameRef, Operation, Sprite, Sprites, Target};
use neoteletext_core::types::FixedVec;
use neoteletext_web::records::{RecordFault, animate, sprites};

// ANIM-16: the record's bounds are the renderer's.
const _: () = assert!(
    sprites::MAX_SPRITES == MAX_SPRITES && sprites::MAX_OBJECT_FRAMES == neoteletext_core::limits::MAX_OBJECT_FRAMES
);

/// Why a record's reader refused a body, in words for a script's author.
#[must_use]
pub fn why(fault: RecordFault) -> String {
    match fault {
        RecordFault::BadValue(what) => format!("the record's {what} is outside its range"),
        RecordFault::Length => "the record's body is short, or bytes follow its end".to_owned(),
        RecordFault::Frame(error) => format!("the record's frame is refused: {error:?}"),
    }
}

/// The operations of an ANIMATE record as the core applies them, in order.
///
/// # Errors
/// An operation whose target is neither a control nor an object, which the
/// record's reader lets through only for a rule's action.
pub fn operations(record: &animate::AnimateRecord<'_>) -> Result<Vec<(Target, Operation)>, String> {
    record
        .operations()
        .map(|operation| {
            crate::operations::to_core(&operation)
                .ok_or_else(|| "an operation's target is not a control or an object".to_owned())
        })
        .collect()
}

/// The sprites of a SPRITES record as the core draws them.
///
/// # Errors
/// What the core refuses of a set of sprites; the record's reader has
/// refused the same before it, so none is expected.
pub fn sprites_of(record: &sprites::SpritesRecord<'_>) -> Result<Sprites, String> {
    let mut list: FixedVec<Sprite, MAX_SPRITES> = FixedVec::new(Sprite::new(1, (0, 0), (1, 1), FILLER));
    for read in record.sprites() {
        let mut frames = read.frames().map(|reference| FrameRef::from_reference(reference.bits()));
        let first = frames.next().flatten().ok_or("an object reference is refused")?;
        let mut sprite = Sprite::new(read.number, (read.row, read.column), (read.height, read.width), first);
        for frame in frames {
            // The reader holds a sprite to the frames one may have.
            let _ = sprite.frames.push(frame.ok_or("an object reference is refused")?);
        }
        // And a record to the sprites a sub-page may have.
        let _ = list.push(Sprite {
            shown: read.shown,
            detectable: read.detectable,
            layer: read.layer,
            order: read.order,
            ..sprite
        });
    }
    Sprites::new(list.as_slice()).map_err(|error| format!("the sprites are refused: {error:?}"))
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
    use neoteletext_core::sprites::ForcedLook;

    /// ANIM-6 (REQ-6529): each operation of an ANIMATE record, as the
    /// record's reader gives it, is the core's operation of the same name
    /// and fields.
    #[test]
    fn an_animate_records_operations_are_the_cores() {
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
            &[20, 1, 0, 32],
        ]
        .concat();
        let record = animate::read(&body).unwrap();
        assert_eq!((record.page.mpp(), record.subcode), (0x150, None), "whichever sub-page is shown");
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
            (Target::Control(32), Operation::Reset),
        ];
        assert_eq!(operations(&record).unwrap(), expected);
    }

    /// ANIM-7 (REQ-6529): a SPRITES record's sprites, as the record's
    /// reader gives them, are the core's sprites field for field, at the
    /// record's bounds.
    #[test]
    fn a_sprites_records_sprites_are_the_cores() {
        let passive_local: u16 = 0b01 << 11 | 0b11 << 9;
        let one = |number: u16, flags: u8| {
            let mut bytes = number.to_be_bytes().to_vec();
            bytes.extend_from_slice(&[flags, 2, 0xFF, 0xD9, 24, 40, 25, 2]);
            bytes.extend_from_slice(&passive_local.to_be_bytes());
            bytes.extend_from_slice(&(passive_local | 0x1FF).to_be_bytes());
            bytes
        };
        let record = |list: &[Vec<u8>]| {
            let mut body = vec![0x01, 0x60, 0, 1, u8::try_from(list.len()).unwrap(), 0];
            body.extend(list.iter().flatten());
            body
        };
        let body = record(&[one(1, 0b11), one(65_535, 0)]);
        let read = sprites::read(&body).unwrap();
        assert_eq!((read.page.mpp(), read.subcode.value()), (0x160, 1));
        let converted = sprites_of(&read).unwrap();
        let first = converted.as_slice()[0];
        assert_eq!((first.number, first.shown, first.detectable, first.layer, first.order), (1, true, true, 2, -1));
        assert_eq!((first.column, first.row, first.width, first.height), (-39, 24, 40, 25));
        let frames = [
            FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(0) },
            FrameRef { source: ObjectSource::Local, kind: ObjectKind::Passive, number: ObjectNumber(0x1FF) },
        ];
        assert_eq!(first.frames.as_slice(), frames);
        assert!(!converted.as_slice()[1].shown);
        assert_eq!(sprites_of(&sprites::read(&record(&[])).unwrap()).unwrap(), Sprites::NONE);
        let all: Vec<Vec<u8>> = (1..=32).map(|number| one(number, 1)).collect();
        assert_eq!(sprites_of(&sprites::read(&record(&all)).unwrap()).unwrap().as_slice().len(), 32);
    }
}
