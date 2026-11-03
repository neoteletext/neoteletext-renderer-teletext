#![allow(clippy::unwrap_used, reason = "a test")]
use super::*;

#[test]
fn frames_round_trip_and_bad_frames_are_rejected() {
    let frames = [
        Frame::Packet([7; 42]),
        Frame::Tti(PageData { secure: true, origin: Origin::Page, bytes: b"PN,10000\n".to_vec() }),
        Frame::T42(PageData { secure: false, origin: Origin::Push, bytes: vec![1, 2, 3] }),
        Frame::Undeclared(PageData { secure: true, origin: Origin::Page, bytes: vec![4] }),
        Frame::Notice(Notice::InitialPage(0x100)),
        Frame::Notice(Notice::Region(Some(neoteletext_core::consts::NIBBLE_MASK))),
        Frame::Notice(Notice::Region(None)),
        Frame::Notice(Notice::Unencrypted),
        Frame::Notice(Notice::Input(true)),
        Frame::Input(vec![0x61, 0xFF, 0x01, 0x04, 0x02]),
        Frame::Notice(Notice::Message("hello".into())),
        Frame::Notice(Notice::Service(vec!["other".into(), "weather".into()])),
        Frame::Notice(Notice::Current(0x100)),
        Frame::Notice(Notice::Offline(0x8FF)),
        Frame::Request(0x123, 0xFFFF),
        Frame::Follow("other".into(), 0x400, 0xFFFF),
        Frame::Tti(PageData { secure: true, origin: Origin::FrontDoor, bytes: vec![] }),
        Frame::Notice(Notice::RedirectToOtherChannel("https://elsewhere.example/".into())),
        Frame::Notice(Notice::PageRedirect(0x1A0, 0x3F7F, None)),
        Frame::Notice(Notice::PageRedirect(0x302, 2, Some("a=1".into()))),
        Frame::Notice(Notice::Denied(404, "302/0002".into())),
        Frame::Notice(Notice::Denied(503, String::new())),
        Frame::Notice(Notice::PushStopped("closed with stop code 4000".into())),
        Frame::Notice(Notice::ServiceName("Test".into())),
        Frame::Notice(Notice::Bundle(0x199)),
        Frame::Notice(Notice::Aliases(vec![(0x300, "weather:300".into())])),
        Frame::Notice(Notice::Refresh(60)),
    ];
    let mut buffer = Vec::new();
    for frame in &frames {
        write(&mut buffer, frame).unwrap();
    }
    let mut reader = buffer.as_slice();
    for frame in &frames {
        assert_eq!(read(&mut reader).unwrap().as_ref(), Some(frame));
    }
    assert_eq!(read(&mut reader).unwrap(), None);
    for bad in [
        &[b'T', 0xFF, 0xFF, 0xFF, 0xFF][..],
        &[b'X', 0, 0, 0, 0],
        &[b'P', 0, 0, 0, 1, 0],
        &[b'S', 0, 0, 0, 2, 0, 9],
        &[b'L', 0, 0, 0, 8, 1, 0, 0xFF, 0xFF, 0, 0, 0, 9],
        // A region is four bits, and one.
        &[b'r', 0, 0, 0, 1, 16],
        &[b'r', 0, 0, 0, 2, 1, 2],
        // Retired kinds are unknown now: the glyph choice, the browser's
        // input and screen, and a form's confirmation and outcome.
        &[b'Z', 0, 0, 0, 1, 0],
        &[b'k', 0, 0, 0, 5, 0, 0, 0, 0, 1],
        &[b'Y', 0, 0, 0, 6, 0, 1, 0, 1, 0, 0],
        &[b'C', 0, 0, 0, 1, 1],
        &[b'D', 0, 0, 0, 1, 1],
        // A form's response, once origin 2.
        &[b'T', 0, 0, 0, 2, 1, 2],
    ] {
        assert!(read(&mut &bad[..]).is_err(), "{bad:?}");
    }
}

// REQ-431: the decoder validates every frame it receives, so a frame of a
// retired kind must be refused, never read as the frame that once had it.
#[test]
fn a_frame_of_every_retired_kind_is_refused() {
    for kind in codec::RETIRED_KINDS {
        for payload in [&[][..], &[0], &[0, 0, 0, 0]] {
            let length = u32::try_from(payload.len()).unwrap().to_be_bytes();
            let frame = [&[kind][..], &length, payload].concat();
            assert!(read(&mut frame.as_slice()).is_err(), "retired kind {:?} was read", char::from(kind));
        }
    }
}

// REQ-431 (TODO-1135): the kinds are a small fixed set, each distinct and none
// retired; the build asserts that over `KINDS`, so every kind written must be
// in `KINDS`.
#[test]
fn every_frame_written_has_a_listed_kind() {
    let frames = [
        Frame::Packet([0; 42]),
        Frame::Tti(PageData { secure: true, origin: Origin::Page, bytes: vec![] }),
        Frame::T42(PageData { secure: true, origin: Origin::Page, bytes: vec![] }),
        Frame::Undeclared(PageData { secure: true, origin: Origin::Page, bytes: vec![] }),
        Frame::Notice(Notice::InitialPage(0x100)),
        Frame::Notice(Notice::Unencrypted),
        Frame::Notice(Notice::Message(String::new())),
        Frame::Notice(Notice::Service(vec![])),
        Frame::Notice(Notice::Region(None)),
        Frame::Notice(Notice::Current(0x100)),
        Frame::Notice(Notice::Offline(0x100)),
        Frame::Request(0x100, 0),
        Frame::Input(vec![]),
        Frame::Notice(Notice::Input(true)),
        Frame::Follow(String::new(), 0x100, 0),
        Frame::Notice(Notice::RedirectToOtherChannel(String::new())),
        Frame::Notice(Notice::PageRedirect(0x100, 0, None)),
        Frame::Notice(Notice::Denied(404, String::new())),
        Frame::Notice(Notice::PushStopped(String::new())),
        Frame::Notice(Notice::ServiceName(String::new())),
        Frame::Notice(Notice::Bundle(0x100)),
        Frame::Notice(Notice::Aliases(vec![])),
        Frame::Notice(Notice::Refresh(0)),
    ];
    let mut kinds = Vec::new();
    for frame in &frames {
        let mut buffer = Vec::new();
        write(&mut buffer, frame).unwrap();
        let kind = *buffer.first().unwrap();
        assert!(codec::KINDS.contains(&kind), "{frame:?} is written with kind {:?}, not in KINDS", char::from(kind));
        kinds.push(kind);
    }
    kinds.sort_unstable();
    let mut listed = codec::KINDS.to_vec();
    listed.sort_unstable();
    assert_eq!(kinds, listed, "a kind in KINDS is written by no frame");
}
