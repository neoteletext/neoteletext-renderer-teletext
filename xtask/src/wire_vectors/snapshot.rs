//! The rows tab thumbnails add to the wire vectors (neoteletext-common's
//! `docs/tab-thumbnails.md` section 3, REQ-6879): the snapshot request,
//! HELLO offering `SNAPSHOT_OFFER` then SELECT of page 100 as a whole, and
//! WELCOME agreeing it. Both are the engine's own writers'.

use neoteletext_core::limits::SUBCODE_ANY;
use neoteletext_input::caps::SNAPSHOT_OFFER;
use neoteletext_input::control::{Control, packet_of};
use neoteletext_input::{MAX_MESSAGE, VERSION, snapshot};

use super::{RECORD_ID, Row, SERVICE, hexadecimal};

/// The page the snapshot request asks for, as a whole: the first display
/// page, an example value.
const PAGE: u16 = 0x100;

/// The snapshot request (REQ-6879 item 3): one input message.
pub fn hello(rows: &mut Vec<Row>) -> Result<(), String> {
    let mut out = [0_u8; MAX_MESSAGE];
    let length = snapshot::write_request(PAGE, SUBCODE_ANY, &mut out)
        .map_err(|error| format!("the snapshot request does not encode: {error:?}"))?;
    rows.push(Row {
        kind: "input",
        name: "hello_snapshot",
        value: hexadecimal(out.get(..length).ok_or("the writer overran its buffer")?),
        meaning: "hello version=11 caps=62914560, select 100",
    });
    Ok(())
}

/// WELCOME agreeing the snapshot offer (REQ-6879 item 4).
pub fn welcome(rows: &mut Vec<Row>) -> Result<(), String> {
    let welcome = Control::Welcome { version: VERSION, caps: SNAPSHOT_OFFER, resume: None };
    let packet =
        packet_of(&welcome, SERVICE, RECORD_ID).map_err(|error| format!("WELCOME does not encode: {error:?}"))?;
    rows.push(Row {
        kind: "record",
        name: "welcome_snapshot",
        value: hexadecimal(&packet.bytes),
        meaning: "welcome version=11 caps=62914560 record=0",
    });
    Ok(())
}
