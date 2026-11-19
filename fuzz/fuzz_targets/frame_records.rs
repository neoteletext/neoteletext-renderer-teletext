//! The FRAME record and a receiver's open frames (animation.md ANIM-2,
//! ANIM-23; REQ-6529, REQ-TST-040): the reader never panics and accepts
//! only what its writer can write; and given any sequence of FRAME
//! records, of things to hold and of ticks, a receiver never has more
//! than `MAX_OPEN_FRAMES` frames open, never holds more than
//! `MAX_FRAME_BYTES` for one, never keeps one open `MAX_FRAME_TICKS`, and
//! never applies inside a frame what it was told to hold. Frames are
//! keyed on the page and its sub-code together, as the model is: a
//! sub-page is exactly a page and shares nothing with its siblings
//! (DEC-6599).
#![no_main]

use libfuzzer_sys::fuzz_target;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_web::consts::{MAX_FRAME_BYTES, MAX_FRAME_TICKS, MAX_OPEN_FRAMES};
use neoteletext_web::records::frame::{BYTES, FrameChange, FrameEnd, OpenFrames, encode, read};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};
use std::collections::BTreeMap;

/// The bytes of one step: a kind, a page of eight, an amount and a
/// sub-code.
const STEP_BYTES: usize = 1 + BYTES;

/// How many sub-codes the steps that name a page choose among: few, so
/// that steps meet the frames records opened, and more than one, so that
/// sibling sub-pages are told apart.
const SUBCODES: u8 = 4;

/// The page and sub-code a step names, from its body's first byte and its
/// fourth.
fn place(body: &[u8]) -> Option<(PageAddress, SubCode)> {
    let page = PageAddress::from_mpp(0x100 + u16::from(body[0] % 8))?;
    Some((page, SubCode::new(u16::from(body[3] % SUBCODES))))
}

fuzz_target!(|data: &[u8]| {
    let mut frames = OpenFrames::new();
    // What the model holds for each page and sub-code, and for how many ticks.
    let mut held: BTreeMap<(u16, SubCode), (usize, u32)> = BTreeMap::new();
    for step in data.as_chunks::<STEP_BYTES>().0 {
        let (kind, body) = (step[0], &step[1..]);
        match kind % 4 {
            0 => {
                let Ok(record) = read(body) else { continue };
                let mut reader = RecordReader::new();
                let mut back = false;
                encode(record, 0, 0, &mut |packet| {
                    back = reader.push(&packet) == Read::Record(ChannelRecord::Frame(record));
                })
                .expect("what was read is written");
                assert!(back, "what was written reads back as it was read");
                let page = (record.page.mpp(), record.subcode);
                match frames.record(&record) {
                    FrameChange::Opened => assert!(held.insert(page, (0, 0)).is_none()),
                    FrameChange::Reopened => assert!(held.insert(page, (0, 0)).is_some()),
                    FrameChange::Ended(end) => {
                        assert_eq!(end, FrameEnd::Ended);
                        assert!(held.remove(&page).is_some(), "a frame that was not open ended");
                    }
                    FrameChange::Nothing => {}
                }
            }
            1 => {
                let Some((page, subcode)) = place(body) else { continue };
                let key = (page.mpp(), subcode);
                let bytes = usize::from(u16::from_be_bytes([body[1], body[2]])) * 16;
                match frames.hold(page, subcode, bytes) {
                    None => assert!(!held.contains_key(&key), "applied inside a frame"),
                    Some(None) => held.get_mut(&key).expect("held for a frame that is open").0 += bytes,
                    Some(Some(end)) => {
                        assert_eq!(end, FrameEnd::Full);
                        let (before, _) = held.remove(&key).expect("a frame that was open");
                        assert!(before + bytes > MAX_FRAME_BYTES);
                    }
                }
            }
            2 => {
                frames.tick(&mut |page, subcode, end| {
                    assert_eq!(end, FrameEnd::Late);
                    let (_, ticks) = held.remove(&(page.mpp(), subcode)).expect("a frame that was open");
                    assert_eq!(ticks + 1, MAX_FRAME_TICKS);
                });
                for (_, ticks) in held.values_mut() {
                    *ticks += 1;
                }
            }
            _ => {
                let Some((page, subcode)) = place(body) else { continue };
                assert_eq!(frames.close(page, subcode), held.remove(&(page.mpp(), subcode)).is_some());
            }
        }
        assert!(frames.count() <= MAX_OPEN_FRAMES && frames.count() == held.len());
        for ((page, subcode), (bytes, ticks)) in &held {
            let page = PageAddress::from_mpp(*page).expect("a page that was read");
            assert!(frames.is_open(page, *subcode) && frames.held(page, *subcode) == *bytes);
            assert!(*bytes <= MAX_FRAME_BYTES && *ticks < MAX_FRAME_TICKS);
        }
    }
    let mut closed = 0;
    frames.close_all(&mut |_, _, _| closed += 1);
    assert_eq!((closed, frames.count()), (held.len(), 0));
});
