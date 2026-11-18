//! The `NAPLPS_OBJECTS` record (animation.md ANIM-11, ANIM-23; REQ-6529,
//! REQ-TST-040): the reader never panics, holds a record to 64 objects of
//! distinct numbers and 16 frames each, and accepts only what its writer
//! can write.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::records::naplps_objects::{MAX_OBJECT_FRAMES, MAX_OBJECTS, NaplpsObject, encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    let objects: Vec<NaplpsObject<'_>> = record.objects().collect();
    assert!(objects.len() <= MAX_OBJECTS && objects.len() == usize::from(record.count));
    for (place, object) in objects.iter().enumerate() {
        assert!(object.frames.len() <= MAX_OBJECT_FRAMES);
        assert!(objects[..place].iter().all(|other| other.number != object.number), "a number repeated");
    }
    let mut reader = RecordReader::new();
    let mut back = false;
    encode((record.page, record.subcode, record.generation), &objects, 0, 0, &mut |packet| {
        if let Read::Record(ChannelRecord::NaplpsObjects(again)) = reader.push(&packet) {
            back = again == record;
        }
    })
    .expect("what was read is written");
    assert!(back, "what was written reads back as it was read");
});
