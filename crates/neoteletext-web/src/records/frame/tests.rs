//! The FRAME record (REQ-6529, ANIM-2).
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and indexing into data it built to a known size"
)]

use super::*;
use crate::records::{ChannelRecord, Read, RecordReader};

/// ANIM-2 (REQ-6529): a begin and its end are eight bytes each, one
/// packet each, the end repeating the begin's identifier.
#[test]
fn a_frame_record_is_its_eight_bytes() {
    let page = PageAddress::from_mpp(0x101).unwrap();
    let begin = FrameRecord { page, subcode: SubCode::new(0x0001), mark: FrameMark::Begin, identifier: 0x1234 };
    let end = FrameRecord { mark: FrameMark::End, ..begin };
    assert_eq!(read(&[0x01, 0x01, 0x00, 0x01, 1, 0, 0x12, 0x34]), Ok(begin));
    assert_eq!(read(&[0x01, 0x01, 0x00, 0x01, 2, 0, 0x12, 0x34]), Ok(end));
    for record in [begin, end] {
        let mut reader = RecordReader::new();
        let mut seen = None;
        let packets = encode(record, 3, 7, &mut |packet| {
            if let Read::Record(ChannelRecord::Frame(read)) = reader.push(&packet) {
                seen = Some(read);
            }
        })
        .unwrap();
        assert_eq!((packets, seen), (1, Some(record)));
    }
}

/// ANIM-2 (REQ-6529): what the reader refuses.
#[test]
fn anything_else_is_refused() {
    let good = [0x01, 0x01, 0x00, 0x01, 1, 0, 0x12, 0x34];
    for (at, value) in [(0, 0x09), (2, 0x3F), (4, 0), (4, 3), (5, 1)] {
        let mut body = good;
        body[at] = value;
        if at == 2 {
            // 0x3F7F names no sub-page.
            body[3] = 0x7F;
        }
        assert!(read(&body).is_err(), "byte {at} as {value}");
    }
    assert_eq!(read(&good[..7]), Err(RecordFault::Length));
    assert_eq!(read(&[good.as_slice(), &[0]].concat()), Err(RecordFault::Length));
}

/// ANIM-2, ANIM-16 (REQ-6529): the figures this crate defines for a
/// receiver's holding of frames.
#[test]
fn the_frame_limits_are_the_documents() {
    assert_eq!((MAX_FRAME_BYTES, MAX_FRAME_TICKS, MAX_OPEN_FRAMES), (262_144, 120, 4));
}

fn mark(page: u16, mark: FrameMark, identifier: u16) -> FrameRecord {
    FrameRecord { page: PageAddress::from_mpp(page).unwrap(), subcode: SubCode::new(1), mark, identifier }
}

/// ANIM-2 (REQ-6529): a frame holds what arrives between its begin and
/// its end, and each of the five early endings ends it: the limit of
/// bytes, 120 ticks with no end, a second begin, the page no longer
/// shown, the connection's end. A fifth begin has no frame.
#[test]
fn a_frame_ends_at_its_end_or_early() {
    let page = |mpp: u16| PageAddress::from_mpp(mpp).unwrap();
    let mut frames = OpenFrames::new();
    assert_eq!(frames.hold(page(0x101), 10), None, "no frame: applied as it arrives");
    assert_eq!(frames.record(&mark(0x101, FrameMark::Begin, 7)), FrameChange::Opened);
    assert_eq!(frames.hold(page(0x101), 10), Some(None));
    assert_eq!(frames.hold(page(0x102), 10), None, "another page's passes");
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 8)), FrameChange::Nothing, "another identifier");
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 7)), FrameChange::Ended(FrameEnd::Ended));
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 7)), FrameChange::Nothing, "no frame open");
    // The limit of bytes.
    frames.record(&mark(0x101, FrameMark::Begin, 1));
    assert_eq!(frames.hold(page(0x101), MAX_FRAME_BYTES), Some(None));
    assert_eq!(frames.held(page(0x101)), MAX_FRAME_BYTES);
    assert_eq!(frames.hold(page(0x101), 1), Some(Some(FrameEnd::Full)));
    assert!(!frames.is_open(page(0x101)));
    // 120 ticks with no end.
    frames.record(&mark(0x101, FrameMark::Begin, 2));
    let mut late = std::vec::Vec::new();
    for _ in 0..MAX_FRAME_TICKS - 1 {
        frames.tick(&mut |page, end| late.push((page.mpp(), end)));
    }
    assert!(late.is_empty() && frames.is_open(page(0x101)));
    frames.tick(&mut |page, end| late.push((page.mpp(), end)));
    assert_eq!(late, [(0x101, FrameEnd::Late)]);
    // A second begin, the page no longer shown, the connection's end.
    frames.record(&mark(0x101, FrameMark::Begin, 3));
    assert_eq!(frames.record(&mark(0x101, FrameMark::Begin, 4)), FrameChange::Reopened);
    assert!(frames.close(page(0x101)) && !frames.close(page(0x101)));
    for mpp in 0x101..=0x104 {
        assert_eq!(frames.record(&mark(mpp, FrameMark::Begin, 5)), FrameChange::Opened);
    }
    assert_eq!(frames.record(&mark(0x105, FrameMark::Begin, 5)), FrameChange::Nothing, "a fifth begin");
    assert_eq!((frames.count(), frames.is_open(page(0x105))), (MAX_OPEN_FRAMES, false));
    let mut closed = 0;
    frames.close_all(&mut |_, end| closed += usize::from(end == FrameEnd::Closed));
    assert_eq!((closed, frames.count()), (4, 0));
}
