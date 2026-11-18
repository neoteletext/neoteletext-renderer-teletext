//! `wire-vectors`: the rows of neoteletext-common's `wire-vectors.tsv` that
//! the page rule table and animation added under push protocol 9
//! (page-rules.md RULE-85, animation.md ANIM-23), written from the
//! engine's own encoders so that their bytes are never typed by hand. The
//! rows are printed in the table's form, and a test holds that the copy of
//! the table the engine keeps has exactly these rows of these kinds.

use std::fmt::Write as _;

use neoteletext_audio::Codec;
use neoteletext_audio::encode::Sound;
use neoteletext_core::packet::Packet;
use neoteletext_core::types::{PageAddress, SubCode};
use neoteletext_input::MAX_MESSAGE;
use neoteletext_input::event::{Action, Event, Item, Key, KeyCode, Stamp, write};
use neoteletext_input::rules::{FaultCode, PageStateEvent, RuleBody, RuleEvent, RulesFault};
use neoteletext_rules::animate::{Operation, OperationKind};
use neoteletext_rules::limits::{MAX_STATE_BYTES, MAX_TABLE_BYTES};
use neoteletext_rules::state::{PageState, TableState, TimerState};
use neoteletext_rules::table::{SOUND_LEVEL, Table, Target, Value, default_send_all};
use neoteletext_web::records::{animate, frame, naplps_objects, sheet, sprites};

/// The kinds of row this command writes, in the order the table has them.
#[cfg(test)]
const KINDS: [&str; 6] = ["rules", "rule_event", "page_state", "rules_fault", "records", "object"];

/// The page every vector is for, an example value: 101.
const PAGE: u16 = 0x101;
/// Its sub-page, an example value.
const SUB: u16 = 1;
/// The audio and input service the records use (DOC-1951).
const SERVICE: u8 = 0;
/// The record id of each record's packets, an example value.
const RECORD_ID: u8 = 0;
/// The page tick the `RULE_EVENT` vectors fired at, an example value.
const FIRED_AT: u32 = 412;
/// The page tick of the `PAGE_STATE` vector's snapshot, an example value.
const STATE_AT: u32 = 9_120;
/// The object the animation vectors move and place, an example value.
const OBJECT: u16 = 10;
/// The NAPLPS object of its vector, an example value.
const NAPLPS_OBJECT: u16 = 301;
/// The identifier of the atomic frame's vectors, an example value.
const FRAME: u16 = 7;
/// The bank a local sound's vector names, as RULE-61's example does.
const BANK: u8 = 2;
/// The setup of RULE-61's example: at tick 0, a program change to program
/// 0 on MIDI channel 1.
const SETUP: [u8; 3] = [0x00, 0xC0, 0x00];

/// One row of the table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// The kind: `rules`, `rule_event`, `page_state`, `rules_fault`,
    /// `records` or `object`.
    pub kind: &'static str,
    /// The name.
    pub name: &'static str,
    /// The bytes, in hexadecimal; the packets of a `records` row are
    /// separated by spaces.
    pub value: String,
    /// What the bytes say.
    pub meaning: &'static str,
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// The bytes the input protocol's encoder writes for `item`.
fn message(item: &Item<'_>) -> Result<String, String> {
    let mut out = [0_u8; MAX_MESSAGE];
    let length = write(item, &mut out).map_err(|error| format!("it does not encode: {error:?}"))?;
    Ok(hexadecimal(out.get(..length).ok_or("the encoder overran its buffer")?))
}

/// The packets a record's encoder writes, separated by spaces.
fn packets<E: core::fmt::Debug>(
    encode: impl FnOnce(&mut dyn FnMut(Packet)) -> Result<usize, E>,
) -> Result<String, String> {
    let mut all = Vec::new();
    encode(&mut |packet: Packet| all.push(hexadecimal(&packet.bytes)))
        .map_err(|error| format!("a record does not encode: {error:?}"))?;
    Ok(all.join(" "))
}

/// The RULES record bodies (RULE-41, RULE-53).
fn rules(rows: &mut Vec<Row>) -> Result<(), String> {
    let refused = |fault| format!("a table does not encode: {fault:?}");
    let mut out = [0_u8; MAX_TABLE_BYTES];
    let mut table = Box::new(Table::new());
    for (wheel, name, meaning) in [
        (false, "default_send_all", "the default table of RULE-53 for page 101, generation 1"),
        (
            true,
            "default_send_all_wheel",
            "the default table of RULE-53 for page 101, generation 1, with the wheel rule",
        ),
    ] {
        default_send_all(wheel, &mut table);
        let length = neoteletext_rules::encode(PAGE, 1, Some(&table), &mut out).map_err(refused)?;
        let value = hexadecimal(out.get(..length).ok_or("the encoder overran its buffer")?);
        rows.push(Row { kind: "rules", name, value, meaning });
    }
    let length = neoteletext_rules::encode(PAGE, 2, None, &mut out).map_err(refused)?;
    rows.push(Row {
        kind: "rules",
        name: "no_table",
        value: hexadecimal(out.get(..length).ok_or("the encoder overran its buffer")?),
        meaning: "page 101, generation 2, no table",
    });
    Ok(())
}

/// The receiver's three messages (RULE-46 to RULE-48).
fn messages(rows: &mut Vec<Row>) -> Result<(), String> {
    let on = Stamp::new(PAGE, SUB).ok_or("the stamp is out of range")?;
    let press = Key {
        on,
        action: Action::Press,
        key: KeyCode::Character('a'),
        mods: 0,
        text: Some("a"),
        base: None,
        lost: false,
    };
    let mut key = [0_u8; MAX_MESSAGE];
    let length = write(&Item::Event(Event::Key(press)), &mut key).map_err(|error| format!("{error:?}"))?;
    let key = key.get(..length).ok_or("the encoder overran its buffer")?;
    let rule_event = |body| {
        message(&Item::Event(Event::RuleEvent(RuleEvent {
            on,
            generation: 1,
            rule: 0,
            tick: FIRED_AT,
            target: Target::NONE,
            body,
        })))
    };
    rows.push(Row {
        kind: "rule_event",
        name: "fired",
        value: rule_event(RuleBody::Fired)?,
        meaning: "rule 0 fired at tick 412 on=101/0001 generation=1",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "input_key",
        value: rule_event(RuleBody::Input(key))?,
        meaning: "rule 0 at tick 412 carries: key press a text=a on=101/0001",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "value",
        value: rule_event(RuleBody::Value { value: Value::variable(0), reading: 5 })?,
        meaning: "rule 0 at tick 412: variable 0 reads 5",
    });

    let full = "the state's lists are full";
    let mut table = TableState::new(1, STATE_AT, 0b111);
    table.variables.push(5).map_err(|_| full)?;
    table.timers.push(TimerState { running: true, elapsed: 18 }).map_err(|_| full)?;
    table.values.push((Value::sound(0, SOUND_LEVEL), 41)).map_err(|_| full)?;
    let mut body = [0_u8; MAX_STATE_BYTES];
    let length = neoteletext_rules::state::encode(&PageState { identifier: 42, table: Some(table) }, &mut body)
        .map_err(|fault| format!("the state does not encode: {fault:?}"))?;
    let body = body.get(..length).ok_or("the encoder overran its buffer")?;
    rows.push(Row {
        kind: "page_state",
        name: "answer",
        value: message(&Item::Event(Event::PageState(PageStateEvent { on, body })))?,
        meaning: "identifier=42 generation=1 tick=9120 enabled=7 variables=[5] timers=[running 18] values=[sound 0 level = 41] on=101/0001",
    });
    let fault = RulesFault { on, generation: 3, fault: FaultCode::TickOverrun, count: 12 };
    rows.push(Row {
        kind: "rules_fault",
        name: "tick_overrun",
        value: message(&Item::Event(Event::RulesFault(fault)))?,
        meaning: "generation=3 fault=tick_overrun count=12 on=101/0001",
    });
    Ok(())
}

/// One record of each of animation's five types (ANIM-23), as packets.
fn records(rows: &mut Vec<Row>) -> Result<(), String> {
    let page = PageAddress::from_mpp(PAGE).ok_or("the page is out of range")?;
    let sub = SubCode::new(SUB);
    let operations = [
        Operation { target: Target::object(OBJECT), kind: OperationKind::Move { relative: true, x: -8, y: 0 } },
        Operation { target: Target::control(3), kind: OperationKind::Look(2) },
    ];
    rows.push(Row {
        kind: "records",
        name: "animate",
        value: packets(|out| animate::encode(page, Some(sub), &operations, SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001: move object 10 by (-8,0); look control 3 pressed",
    });
    // One frame: the local active object 5, as a triplet's reference
    // names it (EN 300 706 clause 12.3.3).
    let frames = [0x0A, 0x05];
    let sprite = sprites::Sprite::new(OBJECT, true, true, 2, 0, (5, 6), (3, 2), &frames);
    rows.push(Row {
        kind: "records",
        name: "sprites",
        value: packets(|out| sprites::encode(page, sub, &[sprite], SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001: sprite 10 shown detectable layer 2 at column 5 row 6, 3 by 2, one frame: local active object 5",
    });
    let object = naplps_objects::NaplpsObject {
        number: NAPLPS_OBJECT,
        kind: naplps_objects::ObjectKind::Macro,
        shown: true,
        detectable: true,
        layer: 2,
        order: 0,
        x: 100,
        y: 50,
        width: 64,
        height: 32,
        origin: (0, 0),
        cover: 0,
        looks: [0, 0x21, 0],
        frames: &[0x20],
    };
    rows.push(Row {
        kind: "records",
        name: "naplps_objects",
        value: packets(|out| naplps_objects::encode((page, sub, 1), &[object], SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001 generation 1: macro object 301 at (100,50) 64 by 32, frame macro 2/0, pressed look 2/1",
    });
    let bytes = sheet::SheetRecord { page, kind: sheet::SheetKind::Regis, generation: 1, offset: 0, data: b"P[0,0]" };
    rows.push(Row {
        kind: "records",
        name: "sheet",
        value: packets(|out| sheet::encode(bytes, SERVICE, RECORD_ID, out))?,
        meaning: "page 101 ReGIS sheet generation 1 offset 0: P[0,0]",
    });
    for (mark, name, meaning) in [
        (frame::FrameMark::Begin, "frame_begin", "page 101 sub 0001 begin identifier 7"),
        (frame::FrameMark::End, "frame_end", "page 101 sub 0001 end identifier 7"),
    ] {
        let record = frame::FrameRecord { page, subcode: sub, mark, identifier: FRAME };
        rows.push(Row {
            kind: "records",
            name,
            value: packets(|out| frame::encode(record, SERVICE, RECORD_ID, out))?,
            meaning,
        });
    }
    Ok(())
}

/// The OBJECT of the local sound RULE-61's example announces (RULE-60).
fn object(rows: &mut Vec<Row>) -> Result<(), String> {
    let refused = |error| format!("the local sound does not encode: {error:?}");
    let mut sound = Sound::local(1, 1, Codec::Synth106, &SETUP).map_err(refused)?;
    sound.banks = vec![BANK];
    rows.push(Row {
        kind: "object",
        name: "local_sound",
        value: hexadecimal(&sound.object_record().map_err(refused)?),
        meaning: "the OBJECT of RULE-61's example: synth106_local handle=1 generation=1 banks=[2], a setup block of 3 bytes",
    });
    Ok(())
}

/// Every row this command writes, in the table's order.
///
/// # Errors
/// A vector the engine's own encoder refuses.
pub fn rows() -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    rules(&mut rows)?;
    messages(&mut rows)?;
    records(&mut rows)?;
    object(&mut rows)?;
    Ok(rows)
}

/// `cargo xtask wire-vectors`: prints the rows, a tab between columns.
///
/// # Errors
/// As [`rows`].
pub fn print() -> Result<(), String> {
    for row in rows()? {
        println!("{}\t{}\t{}\t{}", row.kind, row.name, row.value, row.meaning);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use neoteletext_web::wire_vectors::vectors;

    use super::*;

    /// RULE-85, ANIM-23 (REQ-6528, REQ-6529): the table's rows of these
    /// kinds are exactly what the engine's encoders write, in order, so a
    /// change to an encoder or to the table that parts them fails here.
    #[test]
    fn the_tables_rows_of_these_kinds_are_what_the_engine_writes() {
        let written = rows().expect("they encode");
        let held: Vec<Row> = vectors()
            .filter_map(|vector| {
                let kind = KINDS.into_iter().find(|kind| *kind == vector.kind)?;
                let row = written.iter().find(|row| row.kind == kind && row.name == vector.name);
                Some(Row {
                    kind,
                    name: row.map_or("a row the engine does not write", |row| row.name),
                    value: vector.value.to_owned(),
                    meaning: row
                        .filter(|row| row.meaning == vector.meaning)
                        .map_or("another meaning", |row| row.meaning),
                })
            })
            .collect();
        assert_eq!(held, written);
    }
}
