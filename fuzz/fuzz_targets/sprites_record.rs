//! The SPRITES record (animation.md ANIM-7, ANIM-23; REQ-6529,
//! REQ-TST-040): the reader never panics, holds a record to 32 sprites of
//! distinct numbers and 16 frames each, and accepts only what its writer
//! can write.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::records::sprites::{MAX_OBJECT_FRAMES, MAX_SPRITES, Sprite, encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    let sprites: Vec<Sprite<'_>> = record.sprites().collect();
    assert!(sprites.len() <= MAX_SPRITES && sprites.len() == usize::from(record.count));
    for (place, sprite) in sprites.iter().enumerate() {
        assert!((1..=MAX_OBJECT_FRAMES).contains(&sprite.frames().count()));
        assert!(sprites[..place].iter().all(|other| other.number != sprite.number), "a number repeated");
    }
    let mut reader = RecordReader::new();
    let mut back = false;
    encode(record.page, record.subcode, &sprites, 0, 0, &mut |packet| {
        if let Read::Record(ChannelRecord::Sprites(again)) = reader.push(&packet) {
            back = again == record;
        }
    })
    .expect("what was read is written");
    assert!(back, "what was written reads back as it was read");
});
