//! `wire-vectors`: the rows of neoteletext-common's `wire-vectors.tsv` that
//! the page rule table and animation added, at push protocol 10
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
use neoteletext_rules::command::{Command, MAX_BODY_BYTES, Operand, StartMode, VariableOperation};
use neoteletext_rules::limits::{MAX_STATE_BYTES, MAX_TABLE_BYTES, NO_RULE};
use neoteletext_rules::state::{PageState, SequenceState, TableState, TimerState};
use neoteletext_rules::table::{SOUND_LEVEL, Table, Target, Value, default_send_all};
use neoteletext_web::records::{animate, body_check, commands, frame, naplps_objects, rules, sheet, sprites};

/// The kinds of row this command writes, in the order the table has them.
#[cfg(test)]
const KINDS: [&str; 7] = ["rules", "command", "rule_event", "page_state", "rules_fault", "records", "object"];

/// The page every vector is for, an example value: 101.
const PAGE: u16 = 0x101;
/// Its sub-code, an example value.
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
/// The identifier of the state query's vector, as RULE-38's example has.
const QUERY: u32 = 42;
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

/// The page and the sub-code of the vectors.
fn place() -> Result<(PageAddress, SubCode), String> {
    Ok((PageAddress::from_mpp(PAGE).ok_or("the page is out of range")?, SubCode::new(SUB)))
}

/// The image of the default table (RULE-53), with or without its wheel
/// rule.
fn default_image(wheel: bool) -> Result<Vec<u8>, String> {
    let mut out = vec![0_u8; MAX_TABLE_BYTES];
    let mut table = Box::new(Table::new(PAGE, SUB));
    default_send_all(wheel, PAGE, SUB, &mut table);
    let length = neoteletext_rules::encode(&table, body_check, &mut out)
        .map_err(|fault| format!("a table does not encode: {fault:?}"))?;
    out.truncate(length);
    Ok(out)
}

/// The table images (RULE-41, RULE-53).
fn images(rows: &mut Vec<Row>) -> Result<(), String> {
    for (wheel, name, meaning) in [
        (false, "default_send_all", "the image of the default table of RULE-53 for page 101/0001"),
        (
            true,
            "default_send_all_wheel",
            "the image of the default table of RULE-53 for page 101/0001, with the wheel rule",
        ),
    ] {
        rows.push(Row { kind: "rules", name, value: hexadecimal(&default_image(wheel)?), meaning });
    }
    Ok(())
}

/// A `VARIABLE` command of `operation` on `v0` with `operand`.
const fn variable(operation: VariableOperation, operand: Operand) -> Command {
    Command::Variable { operation, variable: 0, operand }
}

/// The body of each `VARIABLE` operation with each operand kind
/// (RULE-99), and of one of each other command.
fn command_bodies(rows: &mut Vec<Row>) -> Result<(), String> {
    use VariableOperation::{Add, Clear, Dec, Inc, Max, Min, Set, Sub, Toggle, Wrap};
    let constant = Operand::Constant(4);
    let plus_variable = Operand::Variable { constant: 1, variable: 1 };
    let plus_fired = Operand::Fired { constant: 0, property: 0 };
    let all: [(&'static str, Command, &'static str); 30] = [
        ("variable_set_constant", variable(Set, constant), "set v0 to 4"),
        ("variable_set_variable", variable(Set, plus_variable), "set v0 to 1 plus v1"),
        ("variable_set_fired", variable(Set, plus_fired), "set v0 to 0 plus the firing's key"),
        (
            "variable_set_value",
            variable(Set, Operand::Value(Value::sound(0, SOUND_LEVEL))),
            "set v0 to the level of audio channel 0",
        ),
        ("variable_inc", variable(Inc, Operand::None), "inc v0"),
        ("variable_dec", variable(Dec, Operand::None), "dec v0"),
        ("variable_add_constant", variable(Add, constant), "add 4 to v0"),
        ("variable_add_variable", variable(Add, plus_variable), "add 1 plus v1 to v0"),
        ("variable_add_fired", variable(Add, plus_fired), "add 0 plus the firing's key to v0"),
        ("variable_sub_constant", variable(Sub, constant), "sub 4 from v0"),
        ("variable_sub_variable", variable(Sub, plus_variable), "sub 1 plus v1 from v0"),
        ("variable_sub_fired", variable(Sub, plus_fired), "sub 0 plus the firing's key from v0"),
        ("variable_toggle", variable(Toggle, Operand::None), "toggle v0"),
        ("variable_min_constant", variable(Min, constant), "hold v0 to at most 4"),
        ("variable_min_variable", variable(Min, plus_variable), "hold v0 to at most 1 plus v1"),
        ("variable_min_fired", variable(Min, plus_fired), "hold v0 to at most 0 plus the firing's key"),
        ("variable_max_constant", variable(Max, constant), "hold v0 to at least 4"),
        ("variable_max_variable", variable(Max, plus_variable), "hold v0 to at least 1 plus v1"),
        ("variable_max_fired", variable(Max, plus_fired), "hold v0 to at least 0 plus the firing's key"),
        ("variable_wrap", variable(Wrap, Operand::Range { low: 0, high: 3 }), "wrap v0 within 0 to 3"),
        ("variable_clear", variable(Clear, Operand::None), "clear v0"),
        (
            "variable_clear_all",
            Command::Variable { operation: Clear, variable: 255, operand: Operand::None },
            "clear all sixteen variables",
        ),
        ("report_firing", Command::Report { value: None }, "report the firing"),
        ("run", Command::Run { group: 1 }, "run group 1 now, in place"),
        (
            "sequence_start_next",
            Command::Sequence { start: true, sequence: 0, mode: StartMode::Next },
            "start sequence 0 at the next tick step",
        ),
        (
            "sequence_start_at",
            Command::Sequence { start: true, sequence: 0, mode: StartMode::At(5400) },
            "start sequence 0 at page tick 5400",
        ),
        (
            "sequence_start_beat",
            Command::Sequence { start: true, sequence: 0, mode: StartMode::Beat { timer: 0, ticks: 30 } },
            "start sequence 0 at the next expiry of 30 ticks of t0",
        ),
        (
            "sequence_start_beat_page",
            Command::Sequence { start: true, sequence: 1, mode: StartMode::Beat { timer: 255, ticks: 8 } },
            "start sequence 1 at the next expiry of 8 ticks of the page timer",
        ),
        ("sequence_stop", Command::Sequence { start: false, sequence: 0, mode: StartMode::Next }, "stop sequence 0"),
        (
            "sequence_stop_all",
            Command::Sequence { start: false, sequence: 255, mode: StartMode::Next },
            "stop every sequence",
        ),
    ];
    for (name, command, meaning) in all {
        let mut out = [0_u8; MAX_BODY_BYTES];
        let length = neoteletext_rules::command::encode(&command, PAGE, SUB, &mut out)
            .map_err(|fault| format!("{name} does not encode: {fault:?}"))?;
        let value = hexadecimal(out.get(..length).ok_or("the encoder overran its buffer")?);
        rows.push(Row { kind: "command", name, value, meaning });
    }
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
    let rule_event = |rule, row, body| {
        message(&Item::Event(Event::RuleEvent(RuleEvent {
            on,
            generation: 1,
            rule,
            row,
            tick: FIRED_AT,
            target: Target::NONE,
            body,
        })))
    };
    rows.push(Row {
        kind: "rule_event",
        name: "fired",
        value: rule_event(0, NO_RULE, RuleBody::Fired)?,
        meaning: "rule 0, no row, fired at tick 412 on=101/0001 generation=1",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "fired_row",
        value: rule_event(2, 5, RuleBody::Fired)?,
        meaning: "rule 2, row 5, fired at tick 412 on=101/0001 generation=1",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "fired_no_rule",
        value: rule_event(NO_RULE, NO_RULE, RuleBody::Fired)?,
        meaning: "a run no rule made fired at tick 412 on=101/0001 generation=1",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "input_key",
        value: rule_event(0, NO_RULE, RuleBody::Input(key))?,
        meaning: "rule 0, no row, at tick 412 carries: key press a text=a on=101/0001",
    });
    rows.push(Row {
        kind: "rule_event",
        name: "value",
        value: rule_event(0, NO_RULE, RuleBody::Value { value: Value::variable(0), reading: 5 })?,
        meaning: "rule 0, no row, at tick 412: variable v0 reads 5",
    });

    let full = "the state's lists are full";
    let mut table = TableState::new(1, STATE_AT);
    table.variables[0] = 5;
    table.timers[0] = TimerState { running: true, elapsed: 18 };
    table.sequences.push(SequenceState { running: true, passes: 3, position: 12, waiting: false }).map_err(|_| full)?;
    table.sequences.push(SequenceState { waiting: true, ..SequenceState::default() }).map_err(|_| full)?;
    table.values.push((Value::sound(0, SOUND_LEVEL), 41)).map_err(|_| full)?;
    let mut body = [0_u8; MAX_STATE_BYTES];
    let length = neoteletext_rules::state::encode(&PageState { identifier: QUERY, table: Some(table) }, &mut body)
        .map_err(|fault| format!("the state does not encode: {fault:?}"))?;
    let body = body.get(..length).ok_or("the encoder overran its buffer")?;
    rows.push(Row {
        kind: "page_state",
        name: "answer",
        value: message(&Item::Event(Event::PageState(PageStateEvent { on, body })))?,
        meaning: "identifier=42 generation=1 tick=9120 v0=5 t0=running 18 sequences=[running passes 3 position 12, waiting] targets=[] values=[sound 0 level = 41] on=101/0001",
    });
    let fault = RulesFault { on, generation: 3, fault: FaultCode::EvaluationLate, count: 12 };
    rows.push(Row {
        kind: "rules_fault",
        name: "evaluation_late",
        value: message(&Item::Event(Event::RulesFault(fault)))?,
        meaning: "generation=3 fault=evaluation_late count=12 on=101/0001",
    });
    Ok(())
}

/// The RULES and `RULES_QUERY` records and one record of each of the
/// types 28 to 33 (RULE-41, RULE-47, RULE-85), as packets.
fn rule_records(rows: &mut Vec<Row>) -> Result<(), String> {
    let (page, sub) = place()?;
    let image = default_image(false)?;
    rows.push(Row {
        kind: "records",
        name: "rules_default_send_all",
        value: packets(|out| rules::encode_image(page, sub, 1, &image, SERVICE, &mut |_| RECORD_ID, out))?,
        meaning: "page 101 sub 0001 generation 1: the whole image of the rules row default_send_all, in one record",
    });
    let none = rules::RulesRecord { page, subcode: sub, generation: 2, fragment: None };
    rows.push(Row {
        kind: "records",
        name: "rules_no_table",
        value: packets(|out| rules::encode(&none, SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001 generation 2: no table",
    });
    let query = rules::RulesQueryRecord { page, subcode: sub, identifier: QUERY };
    rows.push(Row {
        kind: "records",
        name: "rules_query",
        value: packets(|out| rules::encode_query(query, SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001 identifier 42",
    });
    let each: [(&'static str, Command, &'static str); 6] = [
        (
            "local_midi",
            Command::LocalMidi { channel: 0, status: 0x90, data1: 60, data2: 100 },
            "page 101 sub 0001: audio channel 0, note on, MIDI channel 1, key 60, velocity 100",
        ),
        ("variable", variable(VariableOperation::Set, Operand::Constant(4)), "page 101 sub 0001: set v0 to 4"),
        ("timer", Command::Timer { start: true, timer: 0 }, "page 101 sub 0001: start t0"),
        ("report", Command::Report { value: Some(Value::variable(0)) }, "page 101 sub 0001: report variable v0"),
        ("run", Command::Run { group: 1 }, "page 101 sub 0001: run group 1"),
        (
            "sequence",
            Command::Sequence { start: true, sequence: 0, mode: StartMode::At(5400) },
            "page 101 sub 0001: start sequence 0 at page tick 5400",
        ),
    ];
    for (name, command, meaning) in each {
        let value = packets(|out| commands::encode(&command, page, sub, SERVICE, RECORD_ID, out))?;
        rows.push(Row { kind: "records", name, value, meaning });
    }
    Ok(())
}

/// One record of each of animation's five types (ANIM-23), as packets.
fn records(rows: &mut Vec<Row>) -> Result<(), String> {
    let (page, sub) = place()?;
    let operations = [
        Operation { target: Target::object(OBJECT), kind: OperationKind::Move { relative: true, x: -8, y: 0 } },
        Operation { target: Target::control(3), kind: OperationKind::Look(2) },
    ];
    rows.push(Row {
        kind: "records",
        name: "animate",
        value: packets(|out| animate::encode(page, sub, &operations, SERVICE, RECORD_ID, out))?,
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
    let bytes = sheet::SheetRecord {
        page,
        subcode: sub,
        kind: sheet::SheetKind::Regis,
        generation: 1,
        offset: 0,
        data: b"P[0,0]",
    };
    rows.push(Row {
        kind: "records",
        name: "sheet",
        value: packets(|out| sheet::encode(bytes, SERVICE, RECORD_ID, out))?,
        meaning: "page 101 sub 0001 ReGIS sheet generation 1 offset 0: P[0,0]",
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
    // The server names the variant of every sound of a family that has
    // more than one (MIDI-48), so its OBJECT of the same sound has that tag.
    sound.variant = Some(0);
    rows.push(Row {
        kind: "object",
        name: "local_sound_as_the_server_writes_it",
        value: hexadecimal(&sound.object_record().map_err(refused)?),
        meaning: "the OBJECT of RULE-61's example as the server writes it: synth106_local handle=1 generation=1 variant=0 banks=[2], a setup block of 3 bytes",
    });
    Ok(())
}

/// Every row this command writes, in the table's order.
///
/// # Errors
/// A vector the engine's own encoder refuses.
pub fn rows() -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    images(&mut rows)?;
    command_bodies(&mut rows)?;
    messages(&mut rows)?;
    rule_records(&mut rows)?;
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

    /// The `rules` rows the server writes (RULE-85): the images of the
    /// worked examples of page-rules.md, whose names the server turns into
    /// numbers. The engine's libraries have no encoder of a table by names,
    /// so it cannot write them itself; the table holds them once the server
    /// has written them at this version.
    const SERVER_WRITTEN_RULES: [&str; 8] =
        ["rule_35", "example_1", "example_2", "example_3", "example_4", "example_5", "example_6", "music_player"];

    /// Whether `value`, a table's image in hexadecimal, passes the check,
    /// decodes to a table for the vectors' page and encodes back to the
    /// same bytes.
    fn decodes_and_encodes_back(value: &str) -> bool {
        let Some((bytes, length)) = neoteletext_web::wire_vectors::bytes_of(value) else { return false };
        let Some(image) = bytes.get(..length) else { return false };
        let mut table = Box::new(Table::new(PAGE, SUB));
        if neoteletext_rules::decode(image, PAGE, SUB, body_check, &mut table).is_err() {
            return false;
        }
        let mut out = vec![0_u8; MAX_TABLE_BYTES];
        let encoded = neoteletext_rules::encode(&table, body_check, &mut out);
        neoteletext_rules::check(image, PAGE, SUB, body_check).is_ok()
            && encoded.is_ok_and(|length| out.get(..length) == Some(image))
    }

    /// RULE-85, ANIM-23 (REQ-6528, REQ-6529): the table's rows of these
    /// kinds are exactly what the engine's encoders write, in order, so a
    /// change to an encoder or to the table that parts them fails here. A
    /// row the server writes is held to decode and encode back instead.
    #[test]
    fn the_tables_rows_of_these_kinds_are_what_the_engine_writes() {
        let written = rows().expect("they encode");
        let held: Vec<Row> = vectors()
            .filter_map(|vector| {
                let kind = KINDS.into_iter().find(|kind| *kind == vector.kind)?;
                if kind == "rules" && SERVER_WRITTEN_RULES.contains(&vector.name) {
                    assert!(decodes_and_encodes_back(vector.value), "{}", vector.name);
                    return None;
                }
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
