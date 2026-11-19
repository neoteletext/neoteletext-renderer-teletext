//! The rows magazine music adds to the wire vectors (REQ-6927, DEC-6953):
//! HELLO of protocol version 11 offering `MAGAZINE_SOUND_CAP`, bit 26,
//! with the bits of the `hello` row, and WELCOME agreeing them. Both are
//! the engine's own writers'.

use neoteletext_input::VERSION;
use neoteletext_input::caps::{CURSOR_CAP, MAGAZINE_SOUND_CAP, SELECT_CAP};
use neoteletext_input::control::{Control, packet_of};
use neoteletext_input::event::{Event, Item};

use super::{RECORD_ID, Row, SERVICE, hexadecimal, message};

/// The bits of the `hello` row, CURSOR and SELECT, with magazine music.
const CAPS: u32 = CURSOR_CAP | SELECT_CAP | MAGAZINE_SOUND_CAP;
// What the rows' meanings say.
const _: () = assert!(CAPS == 71_434_240 && VERSION == 11);

/// HELLO offering magazine music (REQ-6927 item 1).
pub fn hello(rows: &mut Vec<Row>) -> Result<(), String> {
    let hello = Item::Event(Event::Hello { version: VERSION, caps: CAPS, agent: None });
    rows.push(Row {
        kind: "input",
        name: "hello_magazine",
        value: message(&hello)?,
        meaning: "hello version=11 caps=71434240",
    });
    Ok(())
}

/// WELCOME agreeing magazine music (REQ-6927 item 1).
pub fn welcome(rows: &mut Vec<Row>) -> Result<(), String> {
    let welcome = Control::Welcome { version: VERSION, caps: CAPS, resume: None };
    let packet =
        packet_of(&welcome, SERVICE, RECORD_ID).map_err(|error| format!("WELCOME does not encode: {error:?}"))?;
    rows.push(Row {
        kind: "record",
        name: "welcome_magazine",
        value: hexadecimal(&packet.bytes),
        meaning: "welcome version=11 caps=71434240 record=0",
    });
    Ok(())
}
