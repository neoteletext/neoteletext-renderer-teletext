use alloc::vec;
use alloc::vec::Vec;

use super::*;
use crate::qpack::Field;
use crate::{settings, stream};

const REQUEST: WebSocketRequest<'static> = WebSocketRequest {
    authority: b"example.org:8443",
    path: b"/ch/push",
    subprotocol: b"neoteletext",
    user_agent: b"NeoTeletext/0.1 (Linux)",
};

/// A HEADERS frame of `fields`.
fn head(fields: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut section = Vec::new();
    qpack::encode(fields.iter().copied(), &mut section);
    let mut out = Vec::new();
    frame::write(frame::HEADERS, &section, &mut out);
    out
}

/// Every event of `input` on an exchange, fed `piece` bytes at a time,
/// with the content joined.
fn exchange(input: &[u8], piece: usize) -> Result<(Vec<Answer>, Vec<u8>, usize, bool), Error> {
    let mut reader = Exchange::new();
    let (mut answers, mut content, mut trailers) = (Vec::new(), Vec::new(), 0);
    for chunk in input.chunks(piece) {
        let mut at = 0;
        loop {
            let (taken, event) = reader.read(&chunk[at..])?;
            at += taken;
            match event {
                Some(ExchangeEvent::Answered(answer)) => answers.push(answer),
                Some(ExchangeEvent::Data(part)) => content.extend_from_slice(part),
                Some(ExchangeEvent::Trailers) => trailers += 1,
                None if taken == 0 => break,
                None => {}
            }
        }
        assert_eq!(at, chunk.len(), "every byte is taken");
    }
    Ok((answers, content, trailers, reader.may_end()))
}

/// REQ-6986 item 1: the request written is one HEADERS frame whose field
/// section, decoded, is the extended CONNECT the server's reader accepts
/// (RFC 9220 section 3), with no origin and no key.
#[test]
fn the_request_written_is_an_extended_connect_the_servers_reader_accepts() {
    let mut out = Vec::new();
    open_websocket(&REQUEST, &mut out).unwrap();
    let mut frames = frame::Reader::new();
    let (taken, event) = frames.read(&out).unwrap();
    assert_eq!(taken, out.len(), "one frame and nothing after");
    let Some(frame::Event::Headers(section)) = event else { panic!("a HEADERS frame, not {event:?}") };
    let read = message::request(qpack::decode(&section, u64::from(MAX_FIELD_SECTION_BYTES)).unwrap()).unwrap();
    assert_eq!(read.method, b"CONNECT");
    assert_eq!(read.protocol.as_deref(), Some(&b"websocket"[..]));
    assert_eq!(read.scheme.as_deref(), Some(&b"https"[..]));
    assert_eq!(read.authority.as_deref(), Some(&b"example.org:8443"[..]));
    assert_eq!(read.path.as_deref(), Some(&b"/ch/push"[..]));
    assert_eq!(
        read.fields,
        vec![
            Field::new(b"sec-websocket-version", b"13"),
            Field::new(b"sec-websocket-protocol", b"neoteletext"),
            Field::new(b"user-agent", b"NeoTeletext/0.1 (Linux)"),
        ]
    );
}

/// REQ-6986 item 1: a value no field may carry, an empty one, or a path
/// that does not start with `/`, writes nothing.
#[test]
fn a_request_that_cannot_be_carried_is_not_written() {
    let refused = |request: WebSocketRequest<'_>| {
        let mut out = vec![0xAA];
        assert_eq!(open_websocket(&request, &mut out), Err(Unwritable));
        assert_eq!(out, [0xAA], "nothing appended");
    };
    refused(WebSocketRequest { authority: b"", ..REQUEST });
    refused(WebSocketRequest { path: b"", ..REQUEST });
    refused(WebSocketRequest { path: b"ch/push", ..REQUEST });
    refused(WebSocketRequest { path: b"/ch\r\nx: y", ..REQUEST });
    refused(WebSocketRequest { authority: b"a\nb", ..REQUEST });
    refused(WebSocketRequest { subprotocol: b"", ..REQUEST });
    refused(WebSocketRequest { user_agent: b"a\0b", ..REQUEST });
    refused(WebSocketRequest { user_agent: b" a", ..REQUEST });
}

/// A WebSocket's bytes go as one DATA frame, and no bytes as none.
#[test]
fn bytes_are_written_as_one_data_frame() {
    let mut out = Vec::new();
    data(b"abc", &mut out);
    assert_eq!(out, [0x00, 0x03, b'a', b'b', b'c']);
    data(b"", &mut out);
    assert_eq!(out.len(), 5, "an empty payload writes nothing");
}

/// REQ-6986 items 2 and 6: the answer, then the content as it comes,
/// however the bytes are cut.
#[test]
fn an_answer_and_its_content_are_read_however_the_bytes_are_cut() {
    let mut input = head(&[(b":status", b"200"), (b"sec-websocket-protocol", b"neoteletext")]);
    data(b"first", &mut input);
    data(b"second", &mut input);
    for piece in [1, 2, 3, 7, input.len()] {
        let (answers, content, trailers, may_end) = exchange(&input, piece).unwrap();
        assert_eq!(answers, [Answer { status: 200, subprotocol: Some(b"neoteletext".to_vec()) }], "piece {piece}");
        assert_eq!(content, b"firstsecond");
        assert_eq!(trailers, 0);
        assert!(may_end, "between frames after the answer");
    }
}

/// REQ-6986 item 2: the subprotocol is the one field's value; none or two
/// choose nothing; another status is reported with its number.
#[test]
fn the_answer_says_the_status_and_the_one_subprotocol_chosen() {
    let answer = |fields: &[(&[u8], &[u8])]| exchange(&head(fields), usize::MAX).unwrap().0;
    assert_eq!(answer(&[(b":status", b"200")]), [Answer { status: 200, subprotocol: None }]);
    assert_eq!(
        answer(&[(b":status", b"200"), (b"sec-websocket-protocol", b"a"), (b"sec-websocket-protocol", b"b")]),
        [Answer { status: 200, subprotocol: None }]
    );
    assert_eq!(answer(&[(b":status", b"403")]), [Answer { status: 403, subprotocol: None }]);
    assert_eq!(answer(&[(b":status", b"501")]), [Answer { status: 501, subprotocol: None }]);
}

/// REQ-6986 item 2: interim responses are passed over, eight at most.
#[test]
fn interim_responses_are_passed_over_up_to_the_bound() {
    let interim = head(&[(b":status", b"103")]);
    let last = head(&[(b":status", b"200")]);
    let series = |count: usize| [interim.repeat(count), last.clone()].concat();
    let (answers, ..) = exchange(&series(MAX_INTERIM_RESPONSES), 5).unwrap();
    assert_eq!(answers, [Answer { status: 200, subprotocol: None }]);
    assert_eq!(exchange(&series(MAX_INTERIM_RESPONSES + 1), 5), Err(Error::stream(Code::ExcessiveLoad)));
    // Not yet answered: the stream may not end after an interim response.
    assert!(!exchange(&interim, usize::MAX).unwrap().3);
}

/// RFC 9114 section 4.1: content before the answer, and content after
/// trailers, are `H3_FRAME_UNEXPECTED`; a malformed answer is
/// `H3_MESSAGE_ERROR`; a field section this decoder cannot read is
/// `QPACK_DECOMPRESSION_FAILED`.
#[test]
fn frames_out_of_order_and_bad_answers_are_refused() {
    let mut early = Vec::new();
    data(b"x", &mut early);
    assert_eq!(exchange(&early, usize::MAX), Err(Error::connection(Code::FrameUnexpected)));

    let mut after = head(&[(b":status", b"200")]);
    data(b"x", &mut after);
    after.extend(head(&[(b"x-trailer", b"1")]));
    let (answers, content, trailers, may_end) = exchange(&after, 3).unwrap();
    assert_eq!((answers.len(), content.as_slice(), trailers, may_end), (1, &b"x"[..], 1, true));
    data(b"y", &mut after);
    assert_eq!(exchange(&after, 3), Err(Error::connection(Code::FrameUnexpected)));

    assert_eq!(exchange(&head(&[(b":status", b"20")]), usize::MAX), Err(Error::stream(Code::MessageError)));
    assert_eq!(exchange(&head(&[(b":status", b"101")]), usize::MAX), Err(Error::stream(Code::MessageError)));
    assert_eq!(exchange(&head(&[(b"server", b"x")]), usize::MAX), Err(Error::stream(Code::MessageError)));
    let mut dynamic = Vec::new();
    // Required Insert Count 1: a reference to the dynamic table.
    frame::write(frame::HEADERS, &[0x02, 0x00, 0x80], &mut dynamic);
    assert_eq!(exchange(&dynamic, usize::MAX), Err(Error::connection(Code::QpackDecompressionFailed)));
    let mut settings = Vec::new();
    frame::write(frame::SETTINGS, &[], &mut settings);
    assert_eq!(exchange(&settings, usize::MAX), Err(Error::connection(Code::FrameUnexpected)));
}

/// Every event of `input` on a stream the server opened, fed `piece`
/// bytes at a time.
fn incoming(opened: &mut Opened, input: &[u8], piece: usize) -> Result<(Vec<IncomingEvent>, bool), Error> {
    let mut reader = Incoming::new();
    let mut found = Vec::new();
    for chunk in input.chunks(piece) {
        let mut at = 0;
        loop {
            let (taken, event) = reader.read(opened, &chunk[at..])?;
            at += taken;
            match event {
                Some(event) => found.push(event),
                None if taken == 0 => break,
                None => {}
            }
        }
        assert_eq!(at, chunk.len(), "every byte is taken");
    }
    Ok((found, reader.critical()))
}

/// REQ-6986 items 4 and 5: the server's control stream gives its settings,
/// which say whether extended CONNECT may be sent, and a GOAWAY after.
#[test]
fn the_servers_control_stream_gives_its_settings_and_goaway() {
    let mut bytes = Vec::new();
    stream::open_control(&mut bytes);
    frame::write(frame::GOAWAY, &[0x04], &mut bytes);
    for piece in [1, 2, bytes.len()] {
        let (events, critical) = incoming(&mut Opened::new(Role::Client), &bytes, piece).unwrap();
        let [IncomingEvent::Settings(read), IncomingEvent::GoAway(4)] = events[..] else {
            panic!("piece {piece}: {events:?}")
        };
        assert!(read.connect_protocol, "the server's own settings offer extended CONNECT");
        assert!(critical);
    }
    // A server that does not offer it: the client's own SETTINGS, which
    // leaves the setting out, stands in for one.
    let mut without = Vec::new();
    stream::open_client_control(&mut without);
    let (events, _) = incoming(&mut Opened::new(Role::Client), &without, usize::MAX).unwrap();
    assert_eq!(events, [IncomingEvent::Settings(settings::read(&without[3..]).unwrap())]);
    let [IncomingEvent::Settings(read)] = events[..] else { panic!("{events:?}") };
    assert!(!read.connect_protocol);
}

/// REQ-6986 item 4: a control stream that does not start with SETTINGS is
/// `H3_MISSING_SETTINGS`; a second control stream is
/// `H3_STREAM_CREATION_ERROR`; a push stream is `H3_ID_ERROR`.
#[test]
fn a_servers_streams_are_held_to_one_of_each_and_no_push() {
    let mut opened = Opened::new(Role::Client);
    let mut goaway_first = vec![0x00];
    frame::write(frame::GOAWAY, &[0x00], &mut goaway_first);
    assert_eq!(incoming(&mut opened, &goaway_first, 1), Err(Error::connection(Code::MissingSettings)));
    assert_eq!(incoming(&mut opened, &[0x00], 1), Err(Error::connection(Code::StreamCreationError)), "a second");
    assert_eq!(incoming(&mut Opened::new(Role::Client), &[0x01, 0x00], 1), Err(Error::connection(Code::IdError)));
}

/// REQ-6986 item 4: a QPACK stream's bytes are dropped, whatever they are,
/// and a stream of an unknown type is refused once and passed over.
#[test]
fn qpack_streams_are_drained_and_unknown_ones_refused_alone() {
    let mut opened = Opened::new(Role::Client);
    assert_eq!(incoming(&mut opened, &[0x02, 0xFF, 0xFF, 0x00], 1), Ok((vec![], true)));
    assert_eq!(incoming(&mut opened, &[0x03, 0x80], 2), Ok((vec![], true)));
    // 0x21 is of the reserved form 0x1f * N + 0x21 (RFC 9114 section 6.2.3).
    assert_eq!(incoming(&mut opened, &[0x21, 0x01, 0x02, 0x03], 1), Ok((vec![IncomingEvent::Refuse], false)));
    // A type in two bytes, cut between them.
    assert_eq!(incoming(&mut opened, &[0x40, 0x54, 0x09], 1), Ok((vec![IncomingEvent::Refuse], false)));
    // And the connection's control stream is still to come.
    let mut control = Vec::new();
    stream::open_control(&mut control);
    assert_eq!(incoming(&mut opened, &control, 4).map(|(events, _)| events.len()), Ok(1));
}
