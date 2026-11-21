//! The rows renderer parity adds to the wire vectors (neoteletext-common's
//! `docs/renderer-parity.md` PAR-44): HELLO offering and WELCOME agreeing
//! the picture languages' bits (PAR-1); `DRAWING_LINKS` and `REGIS_CURSOR`
//! for a NAPLPS sub-page, whose bytes are those for a ReGIS one (PAR-2);
//! the four picture commands a rule table's group holds, and a table whose
//! group holds a NAPLPS picture's links and cursor (PAR-20, RULE-119); and
//! the ReGIS and NAPLPS bytes of a Greek and an Arabic text part (PAR-4,
//! PAR-5). Every one is the engine's own encoder's.

use neoteletext_core::packet::Packet;
use neoteletext_input::VERSION;
use neoteletext_input::caps::{CURSOR_CAP, NAPLPS_CAP, REGIS_CAP, SELECT_CAP};
use neoteletext_input::control::{Control, packet_of};
use neoteletext_input::event::{Event, Item};
use neoteletext_rules::command::{TYPE_DRAWING_LINKS, TYPE_REGIS_CURSOR};
use neoteletext_rules::limits::MAX_TABLE_BYTES;
use neoteletext_rules::table::{Framed, Rule, Table, Trigger};
use neoteletext_web::records::{body_check, drawing_links, regis_cursor, rules};

use super::{PAGE, RECORD_ID, Row, SERVICE, SUB, hexadecimal, message, packets, place};

/// The capabilities of the parity HELLO: those of the table's `hello` row
/// (`CURSOR_CAP` and `SELECT_CAP`) and both picture languages (PAR-1).
pub const PICTURE_CAPS: u32 = CURSOR_CAP | SELECT_CAP | REGIS_CAP | NAPLPS_CAP;

/// The NAPLPS object of the links and cursor vectors, as the
/// `naplps_objects` rows' object: an example value.
const OBJECT: u16 = 301;
/// The ReGIS numbered drawing of the group's `regis_links` and
/// `regis_cursor` commands, an example value.
const DRAWING: u16 = 3;
/// Where the object's link leads, an example value.
const OBJECT_TARGET: &str = "302";
/// Where the drawing's link leads, an example value.
const DRAWING_TARGET: &str = "300";

/// The name of the table row whose group holds a NAPLPS picture's links and
/// cursor (PAR-20); its `records` row is `rules_` and this name.
pub const GROUP_TABLE: &str = "picture_links_group";

/// The Greek text part, as a program gives it (PAR-21): an example value.
pub const GREEK: (&str, &str) = ("Καλημέρα", "el");
/// The Arabic text part, as a program gives it (PAR-21): an example value.
pub const ARABIC: (&str, &str) = ("مرحبا", "ar");

/// HELLO offering both picture languages (PAR-1, PAR-44).
pub fn hello(rows: &mut Vec<Row>) -> Result<(), String> {
    let hello = Item::Event(Event::Hello { version: VERSION, caps: PICTURE_CAPS, agent: None });
    rows.push(Row {
        kind: "input",
        name: "hello_pictures",
        value: message(&hello)?,
        meaning: "hello version=11 caps=29491200",
    });
    Ok(())
}

/// WELCOME agreeing both picture languages (PAR-1, PAR-44).
pub fn welcome(rows: &mut Vec<Row>) -> Result<(), String> {
    let welcome = Control::Welcome { version: VERSION, caps: PICTURE_CAPS, resume: None };
    let packet =
        packet_of(&welcome, SERVICE, RECORD_ID).map_err(|error| format!("WELCOME does not encode: {error:?}"))?;
    rows.push(Row {
        kind: "record",
        name: "welcome_pictures",
        value: hexadecimal(&packet.bytes),
        meaning: "welcome version=11 caps=29491200 record=0",
    });
    Ok(())
}

/// The packets of the `DRAWING_LINKS` sending of one link, `number` to
/// `target`, for the vectors' sub-page.
fn links_packets(number: u16, target: &str) -> Result<String, String> {
    let (page, sub) = place()?;
    let links = [drawing_links::DrawingLink { drawing: number, target: target.as_bytes() }];
    packets(|out| drawing_links::encode(page, sub, &links, SERVICE, RECORD_ID, out).map(|sent| sent.packets))
}

/// The packets of the `REGIS_CURSOR` record naming `number`.
fn cursor_packets(number: u16) -> Result<String, String> {
    let (page, subcode) = place()?;
    let record = regis_cursor::RegisCursorRecord { page, subcode, drawing: number };
    packets(|out| regis_cursor::encode(record, SERVICE, RECORD_ID, out))
}

/// The body of the one record whose packets, in hexadecimal separated by
/// spaces, are `packets`: what a group holds of it (RULE-45).
fn body_of(packets: &str) -> Result<Vec<u8>, String> {
    let mut record = Vec::new();
    for packet in packets.split(' ') {
        let (bytes, length) = neoteletext_web::wire_vectors::bytes_of(packet).ok_or("a packet is not bytes")?;
        let bytes: [u8; neoteletext_core::consts::PACKET_BYTES] =
            bytes.get(..length).and_then(|bytes| bytes.try_into().ok()).ok_or("a packet is not 42 bytes")?;
        let fragment = neoteletext_audio::idl::fragment(&Packet { bytes })
            .ok_or("no fragment")?
            .map_err(|error| format!("{error:?}"))?;
        record.extend_from_slice(&fragment.data);
        if fragment.last {
            let length = neoteletext_audio::record::framed_length(&record).map_err(|error| format!("{error:?}"))?;
            let framed = record.get(..length).ok_or("the record is cut short")?;
            let (_, body) = neoteletext_audio::record::split(framed).map_err(|error| format!("{error:?}"))?;
            return Ok(body.to_vec());
        }
    }
    Err("the record has no last fragment".to_owned())
}

/// The four picture commands of a group as their bodies (PAR-20, RULE-45):
/// `regis_links`, `naplps_links`, `regis_cursor` and `naplps_cursor`
/// without `connections`, `page` and `sub`, which the table supplies.
pub fn commands(rows: &mut Vec<Row>) -> Result<(), String> {
    for (name, value, meaning) in [
        (
            "drawing_links_of_regis",
            links_packets(DRAWING, DRAWING_TARGET)?,
            "regis_links: drawing 3 leads to 300, part 0 of 1, on page 101 sub 0001",
        ),
        (
            "drawing_links_of_naplps",
            links_packets(OBJECT, OBJECT_TARGET)?,
            "naplps_links: object 301 leads to 302, part 0 of 1, on page 101 sub 0001",
        ),
        ("regis_cursor_of_regis", cursor_packets(DRAWING)?, "regis_cursor: drawing 3 on page 101 sub 0001"),
        ("regis_cursor_of_naplps", cursor_packets(OBJECT)?, "naplps_cursor: object 301 on page 101 sub 0001"),
    ] {
        rows.push(Row { kind: "command", name, value: hexadecimal(&body_of(&value)?), meaning });
    }
    Ok(())
}

/// The image of a table whose one rule, on the page's opening, runs a
/// group holding a NAPLPS picture's links and cursor (PAR-20, RULE-119).
fn group_image() -> Result<Vec<u8>, String> {
    let links = body_of(&links_packets(OBJECT, OBJECT_TARGET)?)?;
    let cursor = body_of(&cursor_packets(OBJECT)?)?;
    let mut table = Box::new(Table::new(PAGE, SUB));
    let group = table
        .push_group(&[
            Framed { kind: TYPE_DRAWING_LINKS, body: &links, bindings: &[] },
            Framed { kind: TYPE_REGIS_CURSOR, body: &cursor, bindings: &[] },
        ])
        .map_err(|fault| format!("the group does not hold: {fault:?}"))?;
    table.rules.push(Rule::new(Trigger::Open, group)).map_err(|_| "the rules are full")?;
    let mut out = vec![0_u8; MAX_TABLE_BYTES];
    let length = neoteletext_rules::encode(&table, body_check, &mut out)
        .map_err(|fault| format!("the table does not encode: {fault:?}"))?;
    out.truncate(length);
    Ok(out)
}

/// The table whose group holds the picture's links and cursor.
pub fn group_table(rows: &mut Vec<Row>) -> Result<(), String> {
    rows.push(Row {
        kind: "rules",
        name: GROUP_TABLE,
        value: hexadecimal(&group_image()?),
        meaning: "the image of a table for page 101/0001 whose rule open, on the page's opening, runs group 0: naplps_links (object 301 leads to 302) and naplps_cursor (object 301)",
    });
    Ok(())
}

/// `DRAWING_LINKS` and `REGIS_CURSOR` for a NAPLPS sub-page (PAR-2), and the
/// RULES record of the group's table.
pub fn records(rows: &mut Vec<Row>) -> Result<(), String> {
    rows.push(Row {
        kind: "records",
        name: "drawing_links",
        value: links_packets(OBJECT, OBJECT_TARGET)?,
        meaning: "page 101 sub 0001 part 0 of 1: object 301 of the NAPLPS picture leads to 302; the same bytes name drawing 301 of a ReGIS picture",
    });
    rows.push(Row {
        kind: "records",
        name: "regis_cursor",
        value: cursor_packets(OBJECT)?,
        meaning: "page 101 sub 0001: object 301 of the NAPLPS picture is the cursor; the same bytes name drawing 301 of a ReGIS picture",
    });
    let (page, sub) = place()?;
    let image = group_image()?;
    rows.push(Row {
        kind: "records",
        name: "rules_picture_links_group",
        value: packets(|out| rules::encode_image(page, sub, 1, &image, SERVICE, &mut |_| RECORD_ID, out))?,
        meaning: "page 101 sub 0001 generation 1: the whole image of the rules row picture_links_group, in one record",
    });
    Ok(())
}

/// The bytes the engine's picture text encoder writes for `text` in
/// `language`, as ReGIS or as NAPLPS (PAR-3 to PAR-5).
pub fn text_part(text: &str, language: &str, regis: bool) -> Result<Vec<u8>, String> {
    use neoteletext_formats::picture_text;
    let stretches = picture_text::stretches(text, Some(language), false).map_err(|error| format!("{error:?}"))?;
    let mut out = Vec::new();
    if regis { picture_text::regis(&stretches, &mut out) } else { picture_text::naplps(&stretches, &mut out) }
        .map_err(|error| format!("{error:?}"))?;
    Ok(out)
}

/// The ReGIS and NAPLPS bytes of a Greek and an Arabic text part (PAR-4,
/// PAR-5, PAR-44).
pub fn text_parts(rows: &mut Vec<Row>) -> Result<(), String> {
    for (name, (text, language), regis, meaning) in [
        ("regis_greek", GREEK, true, r#"{"text":"Καλημέρα","language":"el","outside":false} as ReGIS"#),
        ("regis_arabic", ARABIC, true, r#"{"text":"مرحبا","language":"ar","outside":false} as ReGIS"#),
        ("naplps_greek", GREEK, false, r#"{"text":"Καλημέρα","language":"el","outside":false} as NAPLPS"#),
        ("naplps_arabic", ARABIC, false, r#"{"text":"مرحبا","language":"ar","outside":false} as NAPLPS"#),
    ] {
        rows.push(Row { kind: "picture_text", name, value: hexadecimal(&text_part(text, language, regis)?), meaning });
    }
    Ok(())
}
