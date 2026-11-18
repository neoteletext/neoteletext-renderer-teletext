//! The `NAPLPS_OBJECTS` record (animation.md ANIM-11, ANIM-23; REQ-6529,
//! REQ-TST-040; TODO-6657): the reader never panics, holds a record to 64
//! objects of distinct numbers and 16 frames each, labels and a title to
//! their bounds of plain text, one object redefined to exactly one, and
//! accepts only what its writer can write.
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_web::records::naplps_objects::{
    Contents, LabelledObject, MAX_LABEL_BYTES, MAX_OBJECT_FRAMES, MAX_OBJECTS, MAX_TITLE_BYTES, Scope, encode_contents,
    read,
};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};

fuzz_target!(|data: &[u8]| {
    let Ok(record) = read(data) else { return };
    let objects: Vec<LabelledObject<'_>> = record.labelled_objects().collect();
    assert!(objects.len() <= MAX_OBJECTS && objects.len() == usize::from(record.count));
    for (place, labelled) in objects.iter().enumerate() {
        assert!(labelled.object.frames.len() <= MAX_OBJECT_FRAMES);
        assert!(labelled.label.len() <= MAX_LABEL_BYTES && !labelled.label.chars().any(char::is_control));
        assert!(
            objects[..place].iter().all(|other| other.object.number != labelled.object.number),
            "a number repeated"
        );
    }
    assert!(record.title.is_none_or(|title| !title.is_empty() && title.len() <= MAX_TITLE_BYTES));
    let contents = match record.scope {
        Scope::Whole => Contents::Whole { objects: &objects, title: record.title },
        Scope::One => {
            assert!(objects.len() == 1 && record.title.is_none(), "one object, no title");
            Contents::One(objects[0])
        }
    };
    let mut reader = RecordReader::new();
    let mut back = false;
    encode_contents((record.page, record.subcode, record.generation), &contents, 0, 0, &mut |packet| {
        if let Read::Record(ChannelRecord::NaplpsObjects(again)) = reader.push(&packet) {
            back = again == record;
        }
    })
    .expect("what was read is written");
    assert!(back, "what was written reads back as it was read");
});
