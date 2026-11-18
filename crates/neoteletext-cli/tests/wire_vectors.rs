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
        let mut items = 0;
        read_message(&message, &mut |item| {
            let mut out = [0; neoteletext_input::MAX_MESSAGE];
            let length = write(&item, &mut out).unwrap();
            assert_eq!(out[..length], message[..], "{} writes back", vector.name);
            items += 1;
        })
        .unwrap();
        assert_eq!(items, 1, "{}", vector.name);
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
            out(packet_of(&Control::Welcome { version: VERSION, caps: 4_325_376, resume: None }, 0, id).unwrap());
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
                    ("fired", RuleBody::Fired) | ("value", RuleBody::Value { .. }) => assert_eq!(carried, None),
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

/// A RULES record's body (RULE-41): it decodes, encodes back to the same
/// bytes, and the default table's rows are what `default_send_all` builds.
fn rules_body(vector: &Vector<'_>) {
    use neoteletext_rules::Table;
    let body = bytes(vector.value);
    let mut table = Box::new(Table::new());
    let header = neoteletext_rules::decode(&body, &mut table).unwrap_or_else(|fault| panic!("{fault:?}"));
    neoteletext_rules::check(&body).unwrap();
    let mut out = [0; neoteletext_rules::limits::MAX_TABLE_BYTES];
    let length =
        neoteletext_rules::encode(header.page, header.generation, header.has_table.then_some(&*table), &mut out)
            .unwrap();
    assert_eq!(out[..length], body[..], "{} encodes back", vector.name);
    let mut default = Box::new(Table::new());
    match vector.name {
        "default_send_all" => neoteletext_rules::default_send_all(false, &mut default),
        "default_send_all_wheel" => neoteletext_rules::default_send_all(true, &mut default),
        "no_table" => return assert!(!header.has_table),
        other => panic!("{other}"),
    }
    assert!(header.has_table && *table == *default, "{}", vector.name);
}

/// The packets of one record of animation (ANIM-23): the receiver's reader
/// takes them and gives one record, of the type the row's name begins
/// with, when the last arrives.
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
            ChannelRecord::Animate(_) => "animate",
            ChannelRecord::Sprites(_) => "sprites",
            ChannelRecord::NaplpsObjects(_) => "naplps_objects",
            ChannelRecord::Sheet(_) => "sheet",
            ChannelRecord::Frame(_) => "frame",
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
            "rules" => rules_body(&vector),
            "rule_event" => rules_message(&vector, RULE_EVENT),
            "page_state" => rules_message(&vector, PAGE_STATE),
            "rules_fault" => rules_message(&vector, RULES_FAULT),
            "records" => animation_record(&vector),
            "object" => local_object(&vector),
            other => panic!("{other}"),
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        [
            "close",
            "input",
            "object",
            "page_state",
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
