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
    FrameRecord { page: PageAddress::from_mpp(page).unwrap(), subcode: SUB, mark, identifier }
}

/// The sub-code of the pages of these tests.
const SUB: SubCode = SubCode::new(1);

fn page(mpp: u16) -> PageAddress {
    PageAddress::from_mpp(mpp).unwrap()
}

/// ANIM-2 (REQ-6529): a frame holds what arrives between its begin and
/// its end, and each of the five early endings ends it: the limit of
/// bytes, 120 ticks with no end, a second begin, the page no longer
/// shown, the connection's end. A fifth begin has no frame.
#[test]
fn a_frame_ends_at_its_end_or_early() {
    let mut frames = OpenFrames::new();
    assert_eq!(frames.hold(page(0x101), SUB, 10), None, "no frame: applied as it arrives");
    assert_eq!(frames.record(&mark(0x101, FrameMark::Begin, 7)), FrameChange::Opened);
    assert_eq!(frames.hold(page(0x101), SUB, 10), Some(None));
    assert_eq!(frames.hold(page(0x102), SUB, 10), None, "another page's passes");
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 8)), FrameChange::Nothing, "another identifier");
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 7)), FrameChange::Ended(FrameEnd::Ended));
    assert_eq!(frames.record(&mark(0x101, FrameMark::End, 7)), FrameChange::Nothing, "no frame open");
    // The limit of bytes.
    frames.record(&mark(0x101, FrameMark::Begin, 1));
    assert_eq!(frames.hold(page(0x101), SUB, MAX_FRAME_BYTES), Some(None));
    assert_eq!(frames.held(page(0x101), SUB), MAX_FRAME_BYTES);
    assert_eq!(frames.hold(page(0x101), SUB, 1), Some(Some(FrameEnd::Full)));
    assert!(!frames.is_open(page(0x101), SUB));
    // 120 ticks with no end.
    frames.record(&mark(0x101, FrameMark::Begin, 2));
    let mut late = std::vec::Vec::new();
    for _ in 0..MAX_FRAME_TICKS - 1 {
        frames.tick(&mut |page, sub, end| late.push((page.mpp(), sub, end)));
    }
    assert!(late.is_empty() && frames.is_open(page(0x101), SUB));
    frames.tick(&mut |page, sub, end| late.push((page.mpp(), sub, end)));
    assert_eq!(late, [(0x101, SUB, FrameEnd::Late)]);
    // A second begin, the page no longer shown, the connection's end.
    frames.record(&mark(0x101, FrameMark::Begin, 3));
    assert_eq!(frames.record(&mark(0x101, FrameMark::Begin, 4)), FrameChange::Reopened);
    assert!(frames.close(page(0x101), SUB) && !frames.close(page(0x101), SUB));
    for mpp in 0x101..=0x104 {
        assert_eq!(frames.record(&mark(mpp, FrameMark::Begin, 5)), FrameChange::Opened);
    }
    assert_eq!(frames.record(&mark(0x105, FrameMark::Begin, 5)), FrameChange::Nothing, "a fifth begin");
    assert_eq!((frames.count(), frames.is_open(page(0x105), SUB)), (MAX_OPEN_FRAMES, false));
    let mut closed = std::vec::Vec::new();
    frames.close_all(&mut |page, sub, end| closed.push((page.mpp(), sub, end)));
    let every = (0x101..=0x104).map(|mpp| (mpp, SUB, FrameEnd::Closed)).collect::<std::vec::Vec<_>>();
    assert_eq!((closed, frames.count()), (every, 0));
}

/// ANIM-2 with RULE-112 (the owner's DEC-6599, TODO-6681 item 1): a frame
/// is keyed on the page number and the sub-code together. A frame opened
/// for 150/0001 holds nothing of 150/0002, two sub-codes of one page
/// number each have a frame of their own, and each ends alone: by its own
/// end, its own limit of bytes, its own close.
#[test]
fn each_sub_code_of_a_page_number_has_a_frame_of_its_own() {
    let one = SubCode::new(0x0001);
    let two = SubCode::new(0x0002);
    let at = |subcode, mark, identifier| FrameRecord { page: page(0x150), subcode, mark, identifier };
    let mut frames = OpenFrames::new();
    assert_eq!(frames.record(&at(one, FrameMark::Begin, 1)), FrameChange::Opened);
    assert!(frames.is_open(page(0x150), one));
    assert!(!frames.is_open(page(0x150), two), "150/0002 has no frame");
    assert_eq!(frames.hold(page(0x150), two, 10), None, "150/0002's records pass as they arrive");
    assert_eq!(frames.hold(page(0x150), one, 10), Some(None));
    assert_eq!(frames.record(&at(two, FrameMark::End, 1)), FrameChange::Nothing, "an end for 150/0002");
    assert!(frames.is_open(page(0x150), one));
    assert_eq!(frames.record(&at(two, FrameMark::Begin, 1)), FrameChange::Opened, "not a second begin of 150/0001's");
    assert_eq!(frames.count(), 2);
    assert_eq!(frames.hold(page(0x150), two, 3), Some(None));
    assert_eq!((frames.held(page(0x150), one), frames.held(page(0x150), two)), (10, 3));
    assert_eq!(frames.record(&at(one, FrameMark::End, 1)), FrameChange::Ended(FrameEnd::Ended));
    assert!(!frames.is_open(page(0x150), one) && frames.is_open(page(0x150), two), "150/0002's frame stays");
    assert_eq!(frames.hold(page(0x150), two, MAX_FRAME_BYTES), Some(Some(FrameEnd::Full)));
    frames.record(&at(one, FrameMark::Begin, 2));
    frames.record(&at(two, FrameMark::Begin, 2));
    assert!(frames.close(page(0x150), two));
    assert!(frames.is_open(page(0x150), one) && !frames.is_open(page(0x150), two), "a close ends that sub-page's");
}
