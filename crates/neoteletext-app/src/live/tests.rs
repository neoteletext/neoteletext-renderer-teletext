#![allow(
    clippy::unwrap_used,
    clippy::unreachable,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good and `unreachable!` on branches it rules out"
)]
use super::spawn::read_frames;
use super::*;
use neoteletext_core::consts::LINK_SCHEME;

/// A live source fed `frames` from a channel, writing its requests to
/// `requests`, for the tests.
fn routed(frames: Receiver<Frame>, requests: Box<dyn Write + Send>) -> Live {
    Live::new(None, requests, frames, false)
}

/// REQ-WEB-002: a service's region stands in for the receiver's own, a
/// service that gives none has the receiver's back, and the user's `--region`
/// stands above both.
#[test]
fn a_services_region_sets_the_default_unless_the_user_named_one() {
    const OWN: u8 = 3;
    const SERVICE: u8 = 6;
    let regions = |region_named: bool| {
        let (sender, frames) = mpsc::channel();
        let mut live = Live::new(None, Box::new(std::io::sink()), frames, region_named);
        let config = neoteletext_core::config::Config { default_region: OWN, ..Default::default() };
        let mut session = Session::new(config, 8);
        [Some(SERVICE), None].map(|region| {
            sender.send(Frame::Notice(Notice::Region(region))).unwrap();
            sender.send(Frame::Notice(Notice::Service(Vec::new()))).unwrap();
            live.poll(&mut session, Instant(0));
            session.decoder().config().default_region
        })
    };
    assert_eq!(regions(false), [SERVICE, OWN]);
    assert_eq!(regions(true), [OWN, OWN]);
}

#[test]
fn the_pipe_reader_wakes_the_viewer_for_each_frame_and_at_the_end() {
    // The viewer took a page only at its next 40 ms look; it is woken
    // as each frame arrives instead (REQ-INT-008).
    let mut bytes = Vec::new();
    for _ in 0..2 {
        pipe::write(&mut bytes, &Frame::Notice(Notice::Unencrypted)).unwrap();
    }
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = woken.clone();
    let wake: OnceLock<Wake> = OnceLock::new();
    let _ = wake.set(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }));
    let (sender, frames) = mpsc::channel();
    read_frames(&mut bytes.as_slice(), &sender, &wake);
    assert_eq!(frames.try_iter().count(), 2);
    assert_eq!(woken.load(std::sync::atomic::Ordering::Relaxed), 3, "each frame, then the end");
}
use std::sync::{Arc, Mutex};

/// A pipe end that keeps what is written.
#[derive(Clone, Default)]
struct Kept(Arc<Mutex<Vec<u8>>>);

impl Write for Kept {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().map_err(|_| std::io::Error::other("poisoned"))?.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Kept {
    fn requests(&self) -> usize {
        let bytes = self.0.lock().map(|bytes| bytes.clone()).unwrap_or_default();
        let mut reader = bytes.as_slice();
        std::iter::from_fn(|| pipe::read(&mut reader).ok().flatten())
            .filter(|frame| matches!(frame, Frame::Request(..)))
            .count()
    }
}

const SECOND: u64 = 1_000_000;

/// A link from outside to `address`.
fn link(address: &str) -> String {
    format!("{LINK_SCHEME}{address}")
}

/// REQ-1672 items 1 and 2: a link holds an `http` or `https` address, its
/// scheme in either case, and nothing else.
#[test]
fn a_link_from_outside_unwraps_to_its_address() {
    use crate::input::{OpeningError, is_live, link_address};
    let https = link("https://a.example/demo/");
    assert_eq!(link_address(&https), Ok("https://a.example/demo/"));
    let http = link("http://a.example/demo/?id=7");
    assert_eq!(link_address(&http), Ok("http://a.example/demo/?id=7"));
    let capitals = format!("{}HTTPS://a.example/demo/", LINK_SCHEME.to_ascii_uppercase());
    assert_eq!(link_address(&capitals), Ok("HTTPS://a.example/demo/"));
    assert_eq!(link_address("pages.t42"), Ok("pages.t42"));
    for refused in [
        "home",
        "",
        "file:///etc/passwd",
        "web:https://a.example/",
        &format!("{LINK_SCHEME}https://a.example/"),
        "wss://a.example/",
    ] {
        // REQ-155: the refusal is its variant of an error enumeration.
        assert_eq!(link_address(&link(refused)), Err(OpeningError::LinkWrapsNoWebAddress), "{refused}");
    }
    assert!(is_live(&https));
    assert!(
        is_live(&format!("{}home", LINK_SCHEME.to_ascii_uppercase())),
        "a refused link is refused, not read as a file"
    );
}

/// DEC-1726: the old scheme is read as nothing: it unwraps to no address, and
/// it is not live, so it goes no further than a file's name would.
#[test]
fn the_old_link_scheme_is_not_read() {
    use crate::input::{is_live, link_address};
    for old in ["neoteletext://https://a.example/demo/", "NEOTELETEXT://https://a.example/demo/", "neoteletext://home"]
    {
        assert_eq!(link_address(old), Ok(old), "not unwrapped");
        assert!(!is_live(old), "{old}");
    }
    assert_eq!(LINK_SCHEME, "neottx://", "DEC-1726: documents name the scheme literally");
}

#[test]
fn a_new_service_keeps_the_records_the_viewer_asked_for() {
    // The viewer asks once, before the fetcher announces the service; had
    // the announcement forgotten, no WELCOME would reach it and keys would
    // never go to the channel.
    let (sender, frames) = mpsc::channel();
    let mut live = routed(frames, Box::new(std::io::sink()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 1);
    session.keep(|_| true);
    sender.send(Frame::Notice(Notice::Service(Vec::new()))).unwrap();
    sender.send(Frame::Packet([0; 42])).unwrap();
    live.poll(&mut session, Instant(0));
    assert_eq!(session.take_kept().len(), 1);
}

/// REQ-WEB-020, concept decision 13: a service's refresh interval
/// brings the page on screen again sooner than the stale timeout would.
#[test]
fn the_refresh_interval_asks_for_the_page_again() {
    let (sender, frames) = mpsc::channel();
    let kept = Kept::default();
    let mut live = routed(frames, Box::new(kept.clone()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
    let page = PageAddress::from_mpp(0x100).unwrap_or_else(|| unreachable!());
    live.poll(&mut session, Instant(0));
    live.request(page, SubCode::ANY);
    assert_eq!(kept.requests(), 1);
    live.poll(&mut session, Instant(6 * SECOND));
    assert_eq!(kept.requests(), 1, "without a refresh interval, not yet");
    sender.send(Frame::Notice(Notice::Refresh(5))).unwrap_or_else(|_| unreachable!());
    live.poll(&mut session, Instant(7 * SECOND));
    assert_eq!(kept.requests(), 2, "five seconds have passed since the request");
    live.poll(&mut session, Instant(9 * SECOND));
    assert_eq!(kept.requests(), 2);
    live.poll(&mut session, Instant(13 * SECOND));
    assert_eq!(kept.requests(), 3);
}

/// REQ-WEB-019, concept section 2 item 4: a page's own file fills only
/// that page, while a front door fills every page it holds.
#[test]
fn a_page_file_fills_only_its_page() {
    let two_pages = b"PN,10000\nOL,1,ONE\nPN,20000\nOL,1,TWO\nPN,1A000\nOL,1,HEX\n".to_vec();
    let address = |mpp| PageAddress::from_mpp(mpp).unwrap_or_else(|| unreachable!());
    for (origin, stored) in [(Origin::Page, [true, false, true]), (Origin::FrontDoor, [true, true, true])] {
        let (sender, frames) = mpsc::channel();
        let mut live = routed(frames, Box::new(Kept::default()));
        let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
        live.request(address(0x100), SubCode::ANY);
        let page = PageData { secure: true, origin, bytes: two_pages.clone() };
        sender.send(Frame::Tti(page)).unwrap_or_else(|_| unreachable!());
        live.poll(&mut session, Instant(SECOND));
        let present: Vec<bool> =
            [0x100, 0x200, 0x1A0].iter().map(|&mpp| session.page(address(mpp), SubCode::ANY).is_some()).collect();
        assert_eq!(present, stored, "{origin:?}");
    }
}

/// REQ-1693: a viewer opens the map's addresses, normalised, and refuses the
/// rest; a service's descriptor and a channel's own file name no place.
#[test]
fn an_address_says_where_a_viewer_opens() {
    use crate::input::{Opening, OpeningError, link_address, opening};
    use neoteletext_core::types::{PageAddress, SubCode};
    use neoteletext_web::channel::SETTINGS_FILE;
    let page = |mpp| PageAddress::from_mpp(mpp).unwrap();
    let at = |mpp, code, parameters: &str| Opening { page: Some((page(mpp), code)), parameters: parameters.to_owned() };
    for (address, opens) in [
        ("https://a.example/home/302", at(0x302, SubCode::ANY, "")),
        (&link("https://a.example/home/302"), at(0x302, SubCode::ANY, "")),
        ("https://a.example/home/1fe?id=7", at(0x1FE, SubCode::ANY, "id=7")),
        ("https://a.example/home/302/0002", at(0x302, SubCode::new(2), "")),
        ("https://a.example/home/302?a=1&b=", at(0x302, SubCode::ANY, "a=1&b=")),
        ("https://a.example/home/?id=7", Opening { page: None, parameters: "id=7".to_owned() }),
        ("https://a.example/home/302.tti", at(0x302, SubCode::ANY, "")),
        ("https://a.example/home/302/0002.t42", at(0x302, SubCode::new(2), "")),
        ("https://a.example/home/", Opening::default()),
        ("https://a.example/home/contents", Opening::default()),
        ("https://a.example/home/wall", Opening::default()),
        (&format!("https://a.example/home/{SETTINGS_FILE}"), Opening::default()),
        ("https://a.example/service.json", Opening::default()),
        ("pages.t42", Opening::default()),
    ] {
        assert_eq!(opening(address), Ok(opens), "{address}");
    }
    for address in [
        "https://a.example/home/#",
        "https://a.example/home/#G02",
        "https://a.example/home/#302:0002",
        "https://a.example/home/#302/3F7F",
        "https://a.example/home/302.tti#303",
        &format!("https://a.example/home/{SETTINGS_FILE}#302"),
        &link("https://a.example/home/#WALL"),
    ] {
        // REQ-155: the refusal is its variant of an error enumeration.
        // A link is refused for the address it wraps.
        let refused = link_address(address).unwrap().to_owned();
        assert_eq!(opening(address), Err(OpeningError::NotAChannelAddress(refused)), "{address}");
    }
}

/// The frames a pipe end kept.
fn kept_frames(kept: &Kept) -> Vec<Frame> {
    let bytes = kept.0.lock().map(|bytes| bytes.clone()).unwrap_or_default();
    let mut reader = bytes.as_slice();
    std::iter::from_fn(|| pipe::read(&mut reader).ok().flatten()).collect()
}

// Verifies REQ-1888 and REQ-328: a sub-page step on the page asked for asks
// the fetcher for part 0 of its list and waits; once the part comes it steps
// to the next sub-code the list names, which the store does not hold; a part
// of another page's list is not taken.
#[test]
fn a_sub_page_step_asks_for_the_list_and_takes_its_rows() {
    let (sender, frames) = mpsc::channel();
    let kept = Kept::default();
    let mut live = routed(frames, Box::new(kept.clone()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
    let page = PageAddress::from_mpp(0x100).unwrap();
    live.request(page, SubCode::ANY);
    assert_eq!(live.subpage_step(page, SubCode::new(1), true), SubPageStep::Waiting);
    assert!(kept_frames(&kept).contains(&Frame::SubPages(0x100, 0)));
    let part = |page: u16| {
        Frame::Notice(Notice::SubPages(pipe::ListPart {
            page,
            part: 0,
            total: 3,
            version: "v".into(),
            subcodes: vec![1, 2, 3],
        }))
    };
    sender.send(part(0x200)).unwrap();
    live.poll(&mut session, Instant(0));
    assert_eq!(live.subpage_step(page, SubCode::new(1), true), SubPageStep::Waiting, "another page's list");
    sender.send(part(0x100)).unwrap();
    live.poll(&mut session, Instant(0));
    assert_eq!(live.subpage_step(page, SubCode::new(1), true), SubPageStep::To(SubCode::new(2)));
    assert_eq!(live.subpage_step(page, SubCode::new(1), false), SubPageStep::To(SubCode::new(3)));
}

// Verifies REQ-508: a page's file fills only the pages asked for, and the
// pages it held beyond them are named in a message.
#[test]
fn a_page_files_other_pages_are_left_out_and_said() {
    let (sender, frames) = mpsc::channel();
    let mut live = routed(frames, Box::new(std::io::sink()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
    live.request(PageAddress::from_mpp(0x100).unwrap(), SubCode::ANY);
    let bytes = b"PN,10000\r\nSC,0000\r\nOL,1,Asked\r\nPN,20000\r\nSC,0000\r\nOL,1,Not asked\r\n".to_vec();
    sender.send(Frame::Tti(PageData { secure: true, origin: Origin::Page, bytes })).unwrap();
    live.poll(&mut session, Instant(0));
    assert!(session.page(PageAddress::from_mpp(0x200).unwrap(), SubCode::ANY).is_none());
    let said: Vec<Notice> = live.notices();
    assert!(said.iter().any(|notice| matches!(notice, Notice::Message(text) if text.ends_with(": 200"))), "{said:?}");
}

/// REQ-421 (REQ-SEC-008): the unencrypted marker follows the page on
/// screen, so what is known of a page is how it last came: a secure page
/// after a plain one clears it, a plain one after a secure one sets it, and
/// a pushed page, whose connection the fetcher reports, leaves it.
#[test]
fn how_a_page_came_follows_its_latest_arrival() {
    let (sender, frames) = mpsc::channel();
    let mut live = routed(frames, Box::new(std::io::sink()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
    let page = PageAddress::from_mpp(0x100).unwrap();
    let other = PageAddress::from_mpp(0x200).unwrap();
    let arrive = |live: &mut Live, session: &mut Session, secure: bool, origin: Origin| {
        let bytes = b"PN,10000\nOL,1,HELLO\n".to_vec();
        sender.send(Frame::Tti(PageData { secure, origin, bytes })).unwrap();
        live.poll(session, Instant(0));
    };
    assert_eq!(live.came_unencrypted(page), None, "nothing is known before it arrives");
    arrive(&mut live, &mut session, false, Origin::FrontDoor);
    assert_eq!(live.came_unencrypted(page), Some(true));
    arrive(&mut live, &mut session, true, Origin::FrontDoor);
    assert_eq!(live.came_unencrypted(page), Some(false), "a secure page clears the marker");
    arrive(&mut live, &mut session, false, Origin::Push);
    assert_eq!(live.came_unencrypted(page), Some(false), "a pushed page leaves it");
    arrive(&mut live, &mut session, false, Origin::FrontDoor);
    assert_eq!(live.came_unencrypted(page), Some(true));
    assert_eq!(live.came_unencrypted(other), None);
}

// Verifies REQ-295 (REQ-NAV-010): each link followed takes the session's next
// number, the first service being 0, as the fetcher counts them; a link to a
// name the descriptor does not hold is not followed and takes none; a return
// names the number, never an address; and the numbers stop at the bound the
// fetcher keeps.
#[test]
fn followed_services_are_numbered_and_reopened_by_number() {
    let kept = Kept::default();
    let (sender, frames) = mpsc::channel();
    let mut live = routed(frames, Box::new(kept.clone()));
    let mut session = Session::new(neoteletext_core::config::Config::default(), 8);
    let weather = pipe::ServiceEntry { short_name: "weather".into(), ..pipe::ServiceEntry::default() };
    sender.send(Frame::Notice(Notice::Service(vec![weather]))).unwrap();
    live.poll(&mut session, Instant(0));
    let page = PageAddress::from_mpp(0x301).unwrap();
    assert_eq!(live.follow(0, page, SubCode::ANY), Some(1));
    assert_eq!(live.follow(1, page, SubCode::ANY), None, "no such linked service");
    assert_eq!(live.follow(0, page, SubCode::new(2)), Some(2));
    live.reopen(0, PageAddress::from_mpp(0x100).unwrap(), SubCode::ANY);
    assert_eq!(
        kept_frames(&kept),
        [
            Frame::Follow("weather".into(), 0x301, u16::MAX),
            Frame::Follow("weather".into(), 0x301, 2),
            Frame::Reopen(0, 0x100, u16::MAX),
        ]
    );
    while live.follow(0, page, SubCode::ANY).is_some() {}
    assert_eq!(live.session_services, pipe::MAX_SESSION_SERVICES);
}
