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
        Frame::Notice(Notice::Service(vec![
            ServiceEntry {
                short_name: "other".into(),
                display_name: "Other service".into(),
                origin: "https://other.example".into(),
            },
            ServiceEntry::default(),
        ])),
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
        Frame::SubPages(0x100, 3),
        Frame::Notice(Notice::SubPagesMissing(0x100, 0)),
        Frame::Notice(Notice::SubPages(ListPart {
            page: 0x302,
            part: 1,
            total: 66,
            version: "4be81f7a09c2d635".into(),
            subcodes: vec![0x65, 0x66],
        })),
        Frame::Notice(Notice::Index(IndexPart {
            generation: 7,
            pages: vec![IndexEntry { page: 0x100, subpages: 1, title: "Index".into() }],
        })),
        Frame::Notice(Notice::IndexComplete(7)),
        Frame::Notice(Notice::SubPageList(LiveList { page: 0x302, version: 2, subcodes: vec![1, 2, 3] })),
        Frame::Notice(Notice::SubPageListComplete(0x302, 2)),
        Frame::Notice(Notice::SubPageListRefused(0x302, 3)),
        Frame::Notice(Notice::Links(b"PN,30201\nSC,0001\nLN,R,100,Index\n".to_vec())),
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
        // A service with its short name alone, as the pipe carried before.
        &[b'S', 0, 0, 0, 7, 0, 1, 0, 0, 0, 1, b'a'],
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
        // A part of a list with more rows than a part holds, 65.
        &[b'p', 0, 0, 0, 11, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 65],
        // An index generation with a byte left over.
        &[b'u', 0, 0, 0, 3, 0, 7, 0],
        // A request for a part with a byte left over.
        &[b'x', 0, 0, 0, 4, 1, 0, 0, 9],
    ] {
        assert!(read(&mut &bad[..]).is_err(), "{bad:?}");
    }
}
