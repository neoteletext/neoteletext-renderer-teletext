//! The table of the wire's own bytes (`neoteletext-common/wire-vectors.tsv`,
//! DEC-1759 lesson 3): every row, read and written by the engine.
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking, so it uses `unwrap` on results it expects to be good, indexing into data it built to a known size and `panic!` on outcomes it rules out"
)]

use neoteletext_core::limits::SUBCODE_ANY;
use neoteletext_core::packet::Packet;
use neoteletext_input::VERSION;
use neoteletext_input::control::{Control, FRAGMENT_DATA, from_packet, packet_of};
use neoteletext_input::cursor::{self, Cursor};
use neoteletext_input::event::{
    Action, Device, Event, Item, KeyCode, MouseAction, PAGE_STATE, RULE_EVENT, RULES_FAULT, Select, read_message, write,
};
use neoteletext_input::rules::RuleBody;
use neoteletext_web::consts::{PUSH_SUBPROTOCOL, close_says_stop};
use neoteletext_web::records::denied::{self, DeniedRecord};
use neoteletext_web::records::redirect::{self, RedirectRecord};
use neoteletext_web::records::{ChannelRecord, Read, RecordReader};
use neoteletext_web::wire_vectors::{Vector, bytes_of, vectors};

/// `value` as bytes.
fn bytes(value: &str) -> Vec<u8> {
    let (bytes, length) = bytes_of(value).unwrap_or_else(|| panic!("{value} is not bytes"));
    bytes[..length].to_vec()
}

/// An item as the table says it.
fn said(item: &Item<'_>) -> String {
    let on = |page: u16, sub: u16| format!("on={page:03X}/{sub:04X}");
    match item {
        Item::Text(text) => format!("text {text}"),
        Item::Event(Event::Hello { version, caps, .. }) => format!("hello version={version} caps={caps}"),
        Item::Event(Event::Select(select, query)) => {
            let place = place_of(*select);
            if query.is_empty() {
                format!("select {place}").trim().to_owned()
            } else {
                format!("select {place}?{query}")
            }
        }
        Item::Event(Event::Key(key)) => {
            assert_eq!((key.action, key.mods, key.base, key.lost), (Action::Press, 0, None, false));
            let KeyCode::Character(character) = key.key else { panic!("{key:?}") };
            format!("key press {character} text={} {}", key.text.unwrap(), on(key.on.page, key.on.sub))
        }
        Item::Event(Event::Mouse(mouse)) => {
            assert_eq!((mouse.action, mouse.device), (MouseAction::Press, Device::Mouse));
            format!(
                "mouse press button={} at={},{} {}",
                mouse.button,
                mouse.at.column,
                mouse.at.row,
                on(mouse.on.page, mouse.on.sub)
            )
        }
        Item::Event(Event::FocusIn) => "focus_in".to_owned(),
        Item::Event(Event::FocusOut) => "focus_out".to_owned(),
        Item::Event(Event::Resize { columns, rows, .. }) => format!("resize {columns}x{rows}"),
        Item::Event(Event::Extension { kind, .. }) => format!("extension {kind:02X}"),
        Item::Event(other) => panic!("{other:?}"),
    }
}

/// A selection's text, as SELECT carries it.
fn place_of(select: Select) -> String {
    match (select.page, select.sub) {
        (0, _) => String::new(),
        (page, SUBCODE_ANY) => format!("{page:03X}"),
        (page, sub) => format!("{page:03X}/{sub:04X}"),
    }
}

fn input(vector: &Vector<'_>) {
    let message = bytes(vector.value);
    let mut read = Vec::new();
    read_message(&message, &mut |item| read.push(said(&item)))
        .unwrap_or_else(|error| panic!("{}: {error:?}", vector.name));
    assert_eq!(read.join(", "), vector.meaning, "{}", vector.name);
    if !vector.name.starts_with("skipped") {
        // Each item writes back to its own bytes, one after another: the
        // snapshot request is HELLO then SELECT in one message (REQ-6879).
        let mut written = Vec::new();
        read_message(&message, &mut |item| {
            let mut out = [0; neoteletext_input::MAX_MESSAGE];
            let length = write(&item, &mut out).unwrap();
            written.extend_from_slice(&out[..length]);
        })
        .unwrap();
        assert_eq!(written, message, "{} writes back", vector.name);
    }
    if vector.name == "hello_snapshot" {
        let mut out = [0; neoteletext_input::MAX_MESSAGE];
        let length = neoteletext_input::snapshot::write_request(0x100, SUBCODE_ANY, &mut out).unwrap();
        assert_eq!(out[..length], message[..], "the snapshot request's writer");
    }
}

/// The one record `packet` holds, by the receiver's reader.
fn record(packet: &Packet) -> String {
    let mut buffer = [0; FRAGMENT_DATA];
    if let Some(Ok(Control::Welcome { version, caps, resume: None })) = from_packet(packet, &mut buffer) {
        return format!("welcome version={version} caps={caps}");
    }
    if let Some(Ok(cursor)) = cursor::from_packet(packet) {
        return format!("cursor page={:03X} acked={} none", cursor.page, cursor.acked);
    }
    match RecordReader::new().push(packet) {
        Read::Record(ChannelRecord::Denied(DeniedRecord { status, place })) => {
            format!("denied {status} {place}").trim().to_owned()
        }
        Read::Record(ChannelRecord::Redirect(RedirectRecord::Address(address))) => format!("redirect {address}"),
        Read::Record(ChannelRecord::Redirect(RedirectRecord::Page { page, subcode, parameters })) => {
            let place = place_of(Select::new(page.mpp(), subcode.value()).unwrap());
            parameters.map_or_else(|| format!("redirect {place}"), |query| format!("redirect {place}?{query}"))
        }
        other => panic!("{other:?}"),
    }
}

/// `meaning` with its record id taken off.
fn split_record(meaning: &str) -> (&str, u8) {
    let (said, id) = meaning.rsplit_once(" record=").unwrap();
    (said, id.parse().unwrap())
}

/// The packet the engine's writer makes for what `said` says.
fn written(said: &str, id: u8) -> [u8; neoteletext_core::consts::PACKET_BYTES] {
    let mut words = said.split(' ');
    let mut one = None;
    let mut out = |packet: Packet| {
        assert!(one.replace(packet).is_none(), "one packet");
    };
    match words.next().unwrap() {
        "welcome" => {
            // The capabilities the row says: the bits of the `hello` row,
            // or with renderer parity's picture languages (PAR-1, PAR-44).
            words.next().unwrap();
            let caps = words.next().unwrap().strip_prefix("caps=").unwrap().parse().unwrap();
            out(packet_of(&Control::Welcome { version: VERSION, caps, resume: None }, 0, id).unwrap());
        }
        "cursor" => out(Cursor { page: 0x100, acked: 1, place: None, flags: 0 }.packet(0, id).unwrap()),
        "denied" => {
            let status = words.next().unwrap().parse().unwrap();
            denied::encode(DeniedRecord { status, place: words.next().unwrap_or("") }, 0, id, &mut out).unwrap();
        }
        "redirect" => {
            let read = redirect::read(words.next().unwrap().as_bytes()).unwrap();
            redirect::encode(read, 0, id, &mut out).unwrap();
        }
        other => panic!("{other}"),
    }
    one.unwrap().bytes
}

/// A message of the page rule table (RULE-46 to RULE-48): it is the one
/// item of its type, what it carries reads, and it writes back to the same
/// bytes.
fn rules_message(vector: &Vector<'_>, kind: u8) {
    let message = bytes(vector.value);
    assert_eq!(message[..2], [0xFF, kind], "{}", vector.name);
    let mut items = 0;
    read_message(&message, &mut |item| {
        match (kind, &item) {
            (RULE_EVENT, Item::Event(Event::RuleEvent(event))) => {
                // The body is the one its name says, and an input it
                // carries is a key as the table's own KEY row has it.
                let carried = event.input().map(|(input, _)| said(&input));
                match (vector.name, event.body) {
                    ("fired" | "fired_row" | "fired_no_rule", RuleBody::Fired) | ("value", RuleBody::Value { .. }) => {
                        assert_eq!(carried, None);
                        // The rule and the row the row's name says.
                        let named = match vector.name {
                            "fired_row" => (2, 5),
                            "fired_no_rule" => (255, 255),
                            _ => (0, 255),
                        };
                        assert_eq!((event.rule, event.row), named, "{}", vector.name);
                    }
                    ("input_key", RuleBody::Input(_)) => {
                        assert_eq!(carried.as_deref(), Some("key press a text=a on=101/0001"));
                    }
                    other => panic!("{other:?}"),
                }
            }
            (PAGE_STATE, Item::Event(Event::PageState(state))) => {
                assert!(state.state().is_some_and(|state| state.table.is_some()), "{}", vector.name);
            }
            (RULES_FAULT, Item::Event(Event::RulesFault(fault))) => {
                assert!(vector.meaning.contains(&format!("fault={}", fault.fault.name())), "{}", vector.name);
            }
            other => panic!("{other:?}"),
        }
        let mut out = [0; neoteletext_input::MAX_MESSAGE];
        let length = write(&item, &mut out).unwrap();
        assert_eq!(out[..length], message[..], "{} writes back", vector.name);
        items += 1;
    })
    .unwrap_or_else(|error| panic!("{}: {error:?}", vector.name));
    assert_eq!(items, 1, "{}", vector.name);
}

/// The page the vectors are for: 101, sub-code 0001.
const PAGE: (u16, u16) = (0x101, 0x0001);

/// A table's image (RULE-41): it decodes for the vectors' page, encodes
/// back to the same bytes, and the default table's rows are what
/// `default_send_all` builds. The rows of the worked examples (RULE-85)
/// are written by the server, which turns a table's names into numbers;
/// the engine's libraries have no such table to compare them with, so
/// that they pass the check, decode and encode back to their own bytes is
/// what it holds of them.
fn rules_image(vector: &Vector<'_>) {
    use neoteletext_rules::Table;
    use neoteletext_web::records::body_check;
    let image = bytes(vector.value);
    let (page, sub) = PAGE;
    let mut table = Box::new(Table::new(page, sub));
    neoteletext_rules::decode(&image, page, sub, body_check, &mut table).unwrap_or_else(|fault| panic!("{fault:?}"));
    neoteletext_rules::check(&image, page, sub, body_check).unwrap();
    let mut out = vec![0; neoteletext_rules::limits::MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(&table, body_check, &mut out).unwrap();
    assert_eq!(out[..length], image[..], "{} encodes back", vector.name);
    let mut default = Box::new(Table::new(page, sub));
    match vector.name {
        "default_send_all" => neoteletext_rules::default_send_all(false, page, sub, &mut default),
        "default_send_all_wheel" => neoteletext_rules::default_send_all(true, page, sub, &mut default),
        _ => return,
    }
    assert!(*table == *default, "{}", vector.name);
}

/// A command's body (RULE-99 and the records beside it): the reader of
/// the record its name begins with takes it for the vectors' page, and
/// writes back the same bytes.
fn command_body(vector: &Vector<'_>) {
    use neoteletext_rules::command::{self, MAX_BODY_BYTES};
    if picture_command(vector) {
        return;
    }
    let names = [
        ("local_midi", command::TYPE_LOCAL_MIDI),
        ("variable", command::TYPE_VARIABLE),
        ("timer", command::TYPE_TIMER),
        ("report", command::TYPE_REPORT),
        ("run", command::TYPE_RUN),
        ("sequence", command::TYPE_SEQUENCE),
    ];
    let kind = names.into_iter().find(|(name, _)| vector.name.starts_with(name)).map(|(_, kind)| kind);
    let kind = kind.unwrap_or_else(|| panic!("{} names no command", vector.name));
    let body = bytes(vector.value);
    let (page, sub, read) = command::decode(kind, &body).unwrap_or_else(|fault| panic!("{}: {fault:?}", vector.name));
    assert_eq!((page, sub), PAGE, "{}", vector.name);
    let mut out = [0_u8; MAX_BODY_BYTES];
    let length = command::encode(&read, page, sub, &mut out).unwrap();
    assert_eq!(out[..length], body[..], "{} writes back", vector.name);
}

/// The page of the vectors as the records name it.
fn vectors_place() -> (neoteletext_core::types::PageAddress, neoteletext_core::types::SubCode) {
    (neoteletext_core::types::PageAddress::from_mpp(PAGE.0).unwrap(), neoteletext_core::types::SubCode::new(PAGE.1))
}

/// The body of the one record `packets` (whole packets 4/31) carry.
fn body_of(packets: &[Packet]) -> Vec<u8> {
    let mut record = Vec::new();
    for packet in packets {
        let fragment = neoteletext_audio::idl::fragment(packet).unwrap().unwrap();
        record.extend_from_slice(&fragment.data);
    }
    let length = neoteletext_audio::record::framed_length(&record).unwrap();
    neoteletext_audio::record::split(&record[..length]).unwrap().1.to_vec()
}

/// Renderer parity PAR-20 (RULE-119): a group's `regis_links`,
/// `naplps_links`, `regis_cursor` or `naplps_cursor` is the body of the
/// `DRAWING_LINKS` or `REGIS_CURSOR` record its name begins with: the
/// record's reader takes it for the vectors' page, a group may hold that
/// type, and the record's writer gives the same body for what it read.
/// Whether the row was one; the others are left to the caller.
fn picture_command(vector: &Vector<'_>) -> bool {
    use neoteletext_rules::command::{TYPE_DRAWING_LINKS, TYPE_REGIS_CURSOR, is_allowed};
    use neoteletext_web::records::{drawing_links, regis_cursor};
    let body = bytes(vector.value);
    let (page, subcode) = vectors_place();
    let mut packets = Vec::new();
    if vector.name.starts_with("drawing_links") {
        let read = drawing_links::read(&body).unwrap_or_else(|fault| panic!("{}: {fault:?}", vector.name));
        assert_eq!((read.page, read.subcode, read.part, read.parts), (page, subcode, 0, 1), "{}", vector.name);
        let links: Vec<_> = read.links().collect();
        drawing_links::encode(page, subcode, &links, 0, 0, &mut |packet| packets.push(packet)).unwrap();
        assert!(is_allowed(TYPE_DRAWING_LINKS));
    } else if vector.name.starts_with("regis_cursor") {
        let read = regis_cursor::read(&body).unwrap_or_else(|fault| panic!("{}: {fault:?}", vector.name));
        assert_eq!((read.page, read.subcode), (page, subcode), "{}", vector.name);
        regis_cursor::encode(read, 0, 0, &mut |packet| packets.push(packet)).unwrap();
        assert!(is_allowed(TYPE_REGIS_CURSOR));
    } else {
        return false;
    }
    assert_eq!(body_of(&packets), body, "{} writes back", vector.name);
    true
}

/// Renderer parity PAR-3 to PAR-5: a text part's bytes are what the
/// engine's picture text encoder writes for the part the meaning gives, in
/// the language the name begins with.
fn picture_text(vector: &Vector<'_>) {
    use neoteletext_formats::picture_text::{naplps, regis, stretches};
    let (part, language) = vector.meaning.rsplit_once(" as ").unwrap();
    let field = |name: &str| {
        let start = part.find(&format!("\"{name}\":")).unwrap() + name.len() + 3;
        let rest = &part[start..];
        rest.strip_prefix('"').map_or_else(
            || rest.split([',', '}']).next().unwrap().to_owned(),
            |quoted| quoted.split('"').next().unwrap().to_owned(),
        )
    };
    let outside = field("outside") == "true";
    let stretches = stretches(&field("text"), Some(&field("language")), outside).unwrap();
    let mut out = Vec::new();
    match (language, vector.name.split('_').next().unwrap()) {
        ("ReGIS", "regis") => regis(&stretches, &mut out).unwrap(),
        ("NAPLPS", "naplps") => naplps(&stretches, &mut out).unwrap(),
        other => panic!("{}: {other:?}", vector.name),
    }
    assert_eq!(out, bytes(vector.value), "{}", vector.name);
}

/// The packets of one record of the page rule table or of animation
/// (RULE-85, ANIM-23): the receiver's reader takes them and gives one
/// record, of the type the row's name begins with, when the last arrives.
fn animation_record(vector: &Vector<'_>) {
    let mut reader = RecordReader::new();
    let packets: Vec<&str> = vector.value.split(' ').collect();
    for (index, packet) in packets.iter().enumerate() {
        let bytes: [u8; neoteletext_core::consts::PACKET_BYTES] = bytes(packet).as_slice().try_into().unwrap();
        let read = reader.push(&Packet { bytes });
        if index + 1 < packets.len() {
            assert_eq!(read, Read::Nothing, "{}", vector.name);
            continue;
        }
        let Read::Record(record) = read else { panic!("{}: {read:?}", vector.name) };
        let kind = match record {
            ChannelRecord::Rules(_) | ChannelRecord::RulesQuery(_) => "rules",
            ChannelRecord::LocalMidi(_) => "local_midi",
            ChannelRecord::Variable(_) => "variable",
            ChannelRecord::Timer(_) => "timer",
            ChannelRecord::Report(_) => "report",
            ChannelRecord::Run(_) => "run",
            ChannelRecord::Sequence(_) => "sequence",
            ChannelRecord::Animate(_) => "animate",
            ChannelRecord::Sprites(_) => "sprites",
            ChannelRecord::NaplpsObjects(_) => "naplps_objects",
            ChannelRecord::Sheet(_) => "sheet",
            ChannelRecord::Frame(_) => "frame",
            ChannelRecord::Paint(_) => "paint",
            // Renderer parity PAR-2: the same bytes for a NAPLPS sub-page
            // as for a ReGIS one.
            ChannelRecord::DrawingLinks(_) => "drawing_links",
            ChannelRecord::RegisCursor(_) => "regis_cursor",
            other => panic!("{other:?}"),
        };
        assert!(vector.name.starts_with(kind), "{} is a {kind} record", vector.name);
    }
}

/// A local sound's OBJECT (RULE-60): the audio profile's reader takes it
/// as a sound of one block. Its banks and that it is local are read only
/// by a build with its synthesizer family, so the audio crate's own test
/// of this row holds those (`a_local_sounds_object_is_this_wire_vector`).
fn local_object(vector: &Vector<'_>) {
    use neoteletext_audio::record::{Record, parse};
    let record = bytes(vector.value);
    let Record::Object(object) = parse(&record).unwrap() else { panic!("{}", vector.name) };
    assert_eq!((object.handle, object.generation, object.blocks), (1, 1, 1), "{}", vector.name);
}

/// DEC-1759 lesson 3: every row of every kind, and every kind is there.
#[test]
fn every_row_of_the_wire_vectors_holds() {
    let mut kinds = std::collections::BTreeSet::new();
    for vector in vectors() {
        kinds.insert(vector.kind);
        match vector.kind {
            "version" if vector.name == "input" => assert_eq!(vector.value, VERSION.to_string()),
            "version" => {
                assert_eq!(vector.value, PUSH_SUBPROTOCOL);
                assert_eq!(PUSH_SUBPROTOCOL, format!("neottx.{VERSION}"), "the name says the version");
            }
            "close" => {
                let says = if close_says_stop(vector.value.parse().unwrap()) { "stop" } else { "retry" };
                assert_eq!(says, vector.meaning, "{}", vector.name);
            }
            "input" => input(&vector),
            "refused" => assert!(read_message(&bytes(vector.value), &mut |_| {}).is_err(), "{}", vector.name),
            "record" => {
                let packet = bytes(vector.value);
                let bytes: [u8; neoteletext_core::consts::PACKET_BYTES] = packet.as_slice().try_into().unwrap();
                let (meaning, id) = split_record(vector.meaning);
                assert_eq!(record(&Packet { bytes }), meaning, "{}", vector.name);
                assert_eq!(written(meaning, id), bytes, "{}", vector.name);
            }
            "rules" => rules_image(&vector),
            "command" => command_body(&vector),
            "rule_event" => rules_message(&vector, RULE_EVENT),
            "page_state" => rules_message(&vector, PAGE_STATE),
            "rules_fault" => rules_message(&vector, RULES_FAULT),
            "records" => animation_record(&vector),
            "object" => local_object(&vector),
            "picture_text" => picture_text(&vector),
            other => panic!("{other}"),
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        [
            "close",
            "command",
            "input",
            "object",
            "page_state",
            "picture_text",
            "record",
            "records",
            "refused",
            "rule_event",
            "rules",
            "rules_fault",
            "version"
        ]
    );
}
