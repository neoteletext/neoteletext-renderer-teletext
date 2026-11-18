//! The program protocol's `rules` message (neoteletext-common
//! `docs/page-rules.md` RULE-35, protocol 15) read into a page's table, for
//! `neottx rules-run`: names turned into numbers (RULE-36), `do` and
//! `echo` made into unnamed groups after the named ones, two rules whose
//! `do` lists are equal sharing one (RULE-35), row sets laid out one after
//! another in the order of their names, and each command encoded as the
//! record the server would send (RULE-114).
//!
//! The server is where the protocol's meaning lives (DEC-6462); this
//! reader follows the same text so that a table written for the server can
//! be played here, and every table it builds is checked whole by
//! `neoteletext_rules::check` before it is evaluated, as the server checks
//! one before it sends it (RULE-39, RULE-59). The words of each code are
//! `neoteletext_rules::names`'; key names are `neoteletext_input::keys`';
//! a family's field names are its `PARAMETERS` table's (RULE-29).
//!
//! Not read here: the `sprites` and `naplps_objects` commands of a group
//! (ANIM-21), whose sprites and objects a run of `rules-run` has no
//! renderer to place. A table that holds one is refused with that reason.

use neoteletext_audio::record::{FLAG_LOOP, TYPE_CONTROL};
use neoteletext_audio_instruments::midi::family;
use neoteletext_audio_instruments::watch::fields as family_fields;
use neoteletext_rules::animate::{Operation, OperationKind, RECORD_FIXED, code as operation, length_of, write_record};
use neoteletext_rules::command::{
    self, Binding, Command, Operand, Source, StartMode, VariableOperation, field, midi_status,
};
use neoteletext_rules::limits::{
    ALL_SEQUENCES, ALL_VARIABLES, ANY_BUTTON, ANY_CUE, KEEP_LAYER, MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES,
    MAX_CONDITION_NODES, MAX_GROUP_COMMANDS, MAX_KEYS_PER_TRIGGER, MAX_SEQUENCE_REPEAT, MAX_SEQUENCE_TICKS,
    MAX_TABLE_BYTES, MAX_TRIGGERS_PER_RULE, PAGE_TIMER,
};
use neoteletext_rules::names::{self, code};
use neoteletext_rules::table::{
    Framed, Key, KeyKind, KeyMatch, Node, Operator, Rect, Row, RowRun, Rule, Sequence, Step, Target, TargetKind,
    Trigger, TriggerTarget, Value, condition,
};
use neoteletext_rules::{Table, decode, encode};
use neoteletext_web::records::body_check;
use serde_json::{Map, Value as Json};

/// The digits of a page number and of a sub-code in the protocol
/// (page-rules.md section 7: `PageNumber`, `SubCode`).
const PAGE_DIGITS: usize = 3;
const SUB_DIGITS: usize = 4;
/// Hexadecimal.
const HEX: u32 = 16;
/// Where a target's number is within an `ANIMATE` operation (ANIM-6): after
/// the operation's code and the target's kind.
const TARGET_NUMBER_AT: usize = 1 + 1;
/// The bytes of a `CONTROL` record's body (REQ-AUD-006): channel,
/// sequence, operation, handle, generation, gain, fade, flags, duck, page.
const CONTROL_BYTES: usize = command::CONTROL_BYTES;
/// A `CONTROL` record's operations (REQ-AUD-006).
const PLAY: u8 = 1;
const STOP: u8 = 2;
const GAIN: u8 = 3;
/// A MIDI part in the protocol is its channel, 1 to 16; on the wire the
/// channel less one (RULE-21).
const FIRST_PART: i64 = 1;
const LAST_PART: i64 = 16;
/// The voices a synth6581 part names, 1 to 3 (RULE-28).
const LAST_SID_VOICE: i64 = 3;

/// The family names of the `synth` value (RULE-28): each family's codec
/// code and variant, as the OBJECT record's variant tags choose them.
const FAMILIES: [(&str, u8, u8); 11] = [
    ("synth6581", family::SYNTH6581, 0),
    ("synth8580", family::SYNTH6581, 1),
    ("synth7", family::SYNTH7, 0),
    ("synth106", family::SYNTH106, 0),
    ("synth60", family::SYNTH106, 1),
    ("synthd", family::SYNTHD, 0),
    ("synth53", family::SYNTH53, 0),
    ("synth52", family::SYNTH53, 1),
    ("synth51", family::SYNTH53, 2),
    ("synth8", family::SYNTH8, 0),
    ("synth80", family::SYNTH80, 0),
];

/// One command of a group: its record type, its body and its bindings.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Built {
    kind: u8,
    body: Vec<u8>,
    bindings: Vec<Binding>,
}

/// The names of a table, each turned into its number (RULE-36).
struct Names<'a> {
    rects: Vec<&'a str>,
    /// Each row set's name, its first row and its count.
    rows: Vec<(&'a str, u8, u8)>,
    groups: Vec<&'a str>,
    sequences: Vec<&'a str>,
    page: u16,
    sub: u16,
}

type Read<T> = Result<T, String>;

fn object<'a>(json: &'a Json, path: &str) -> Read<&'a Map<String, Json>> {
    json.as_object().ok_or_else(|| format!("{path} is an object"))
}

fn list<'a>(json: Option<&'a Json>, path: &str) -> Read<&'a [Json]> {
    match json {
        None | Some(Json::Null) => Ok(&[]),
        Some(Json::Array(items)) => Ok(items),
        Some(_) => Err(format!("{path} is a list")),
    }
}

fn text<'a>(json: Option<&'a Json>, path: &str) -> Read<&'a str> {
    json.and_then(Json::as_str).ok_or_else(|| format!("{path} is text"))
}

fn integer(json: Option<&Json>, path: &str) -> Read<i64> {
    json.and_then(Json::as_i64).ok_or_else(|| format!("{path} is an integer"))
}

fn boolean(json: Option<&Json>, default: bool, path: &str) -> Read<bool> {
    match json {
        None => Ok(default),
        Some(value) => value.as_bool().ok_or_else(|| format!("{path} is true or false")),
    }
}

/// An integer of `path` within `least..=greatest`, as the type it fits.
fn bounded<T: TryFrom<i64>>(json: Option<&Json>, least: i64, greatest: i64, path: &str) -> Read<T> {
    let value = integer(json, path)?;
    if !(least..=greatest).contains(&value) {
        return Err(format!("{path} is {least} to {greatest}"));
    }
    T::try_from(value).map_err(|_| format!("{path} is {least} to {greatest}"))
}

fn word<T: Copy>(table: &[(&str, T)], json: Option<&Json>, path: &str) -> Read<T> {
    let name = text(json, path)?;
    code(table, name).ok_or_else(|| format!("{path}: `{name}` is not one of its words"))
}

fn place(names: &[&str], name: &str, what: &str) -> Read<u8> {
    let index = names.iter().position(|held| *held == name).ok_or_else(|| format!("no {what} is named `{name}`"))?;
    u8::try_from(index).map_err(|_| format!("too many {what}s"))
}

fn designator(names: &[&str], json: Option<&Json>, path: &str) -> Read<u8> {
    let name = text(json, path)?;
    place(names, name, path)
}

/// `"page"` or `"t0"` to `"t7"`.
fn timer(json: Option<&Json>, default: Option<u8>, path: &str) -> Read<u8> {
    match (json, default) {
        (None, Some(timer)) => Ok(timer),
        _ if text(json, path)? == names::PAGE_TIMER_NAME => Ok(PAGE_TIMER),
        _ => designator(&names::TIMER_NAMES, json, path),
    }
}

/// A page number or a sub-code, as hexadecimal digits.
fn hexadecimal(json: Option<&Json>, digits: usize, path: &str) -> Read<u16> {
    let written = text(json, path)?;
    if written.len() != digits || !written.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{path} is {digits} hexadecimal digits"));
    }
    u16::from_str_radix(written, HEX).map_err(|_| format!("{path} is {digits} hexadecimal digits"))
}

/// The row parameter's designator, `p` (DEC-6635 item 8).
fn parameter(json: Option<&Json>, path: &str) -> Read<()> {
    let name = text(json, path)?;
    if names::is_parameter(name) { Ok(()) } else { Err(format!("{path}: a row has one parameter, `p`, not `{name}`")) }
}

/// A key's code, as a key set or a row writes it (RULE-10, RULE-35):
/// `{"char":"a"}`, `{"name":"Enter"}` or `{"physical":"a"}`.
fn key(json: &Json, path: &str) -> Read<Key> {
    let fields = object(json, path)?;
    let (kind, inner) = match (fields.get("char"), fields.get("name"), fields.get("physical")) {
        (Some(inner), None, None) => (KeyKind::Character, inner),
        (None, Some(inner), None) => (KeyKind::Named, inner),
        (None, None, Some(inner)) => (KeyKind::Physical, inner),
        _ => return Err(format!("{path} is one of char, name and physical")),
    };
    let written = text(Some(inner), path)?;
    let code = match kind {
        KeyKind::Named => {
            neoteletext_input::keys::code(written).ok_or_else(|| format!("{path}: no key is named `{written}`"))?
        }
        KeyKind::Character | KeyKind::Physical => {
            let mut characters = written.chars();
            let (Some(character), None) = (characters.next(), characters.next()) else {
                return Err(format!("{path} is one character"));
            };
            // RULE-35: a letter is written in lower case.
            if character.is_uppercase() {
                return Err(format!("{path}: a letter is written in lower case"));
            }
            u32::from(character)
        }
    };
    Ok(Key::of(kind, code))
}

/// A row's parameter: an integer, or a key standing for its code.
fn row_value(json: Option<&Json>, path: &str) -> Read<i32> {
    match json {
        None => Ok(0),
        Some(Json::Object(_)) => {
            let key = key(json.unwrap_or(&Json::Null), path)?;
            i32::try_from(key.code).map_err(|_| format!("{path} is a key whose code fits"))
        }
        Some(_) => bounded(json, i64::from(i32::MIN), i64::from(i32::MAX), path),
    }
}

/// A key set: `"any"` or a list of keys; with `by_row`, one key a row of
/// the rule's rows (RULE-105), which only a rule with rows may have.
fn keys(fields: &Map<String, Json>, rows: bool, path: &str) -> Read<KeyMatch> {
    let by_row = boolean(fields.get("by_row"), false, &format!("{path}.by_row"))?;
    if by_row && !rows {
        return Err(format!("{path}: by_row in a rule without rows (invalid_rules)"));
    }
    let mut matched = match fields.get("keys") {
        Some(Json::String(any)) if any == "any" => KeyMatch::any(),
        given => {
            let given = list(given, path)?;
            if given.is_empty() || given.len() > MAX_KEYS_PER_TRIGGER {
                return Err(format!("{path}.keys is \"any\" or 1 to {MAX_KEYS_PER_TRIGGER} keys"));
            }
            let mut set = Vec::with_capacity(given.len());
            for (index, each) in given.iter().enumerate() {
                set.push(key(each, &format!("{path}.keys[{index}]"))?);
            }
            if by_row { KeyMatch::by_row(&set) } else { KeyMatch::of(&set) }
        }
    };
    if let Some(mods) = fields.get("mods") {
        let mods = object(mods, path)?;
        matched.mods_mask = bounded(mods.get("mask"), 0, i64::from(u8::MAX), path)?;
        matched.mods_value = bounded(mods.get("value"), 0, i64::from(u8::MAX), path)?;
    }
    Ok(matched)
}

impl Names<'_> {
    /// A target (RULE-12, RULE-35): `"page"`, `{"control":n}`,
    /// `{"object":n}`, `{"rect":"name"}`; in a pointer trigger `"any"`,
    /// and in a rule with rows `{"plus":k,"row":"p"}`, `k` a signed 16-bit
    /// constant to which each row's `p` is added.
    fn trigger_target(&self, json: Option<&Json>, any: bool, rows: bool, path: &str) -> Read<TriggerTarget> {
        let json = json.ok_or_else(|| format!("{path} names a target"))?;
        if json.as_str() == Some("page") {
            return Ok(TriggerTarget::PAGE);
        }
        let fields = object(json, path)?;
        if let Some(rect) = fields.get("rect") {
            let number = place(&self.rects, text(Some(rect), path)?, "rectangle")?;
            return Ok(TriggerTarget::of(Target::rect(u16::from(number))));
        }
        let (kind, inner) = match (fields.get("control"), fields.get("object")) {
            (Some(inner), None) => (TargetKind::Control, inner),
            (None, Some(inner)) => (TargetKind::Object, inner),
            _ => return Err(format!("{path} is page, a control, an object or a rectangle")),
        };
        if inner.as_str() == Some("any") {
            if !any {
                return Err(format!("{path}: \"any\" where one target is named"));
            }
            return Ok(TriggerTarget::of(Target { kind, number: 0 }));
        }
        if let Some(row) = inner.as_object() {
            if !rows {
                return Err(format!("{path}: a row parameter in a rule without rows"));
            }
            parameter(row.get("row"), path)?;
            let plus = match row.get("plus") {
                None => 0,
                given => bounded::<i16>(given, i64::from(i16::MIN), i64::from(i16::MAX), &format!("{path}.plus"))?,
            };
            return Ok(TriggerTarget::row(kind, plus));
        }
        let number = bounded(Some(inner), 1, i64::from(u16::MAX), path)?;
        Ok(TriggerTarget::of(Target { kind, number }))
    }

    /// A target that is one control, object or rectangle, or the page.
    fn target(&self, json: Option<&Json>, path: &str) -> Read<Target> {
        let target = self.trigger_target(json, false, false, path)?;
        Ok(Target { kind: target.kind, number: target.number })
    }

    fn button(json: Option<&Json>, path: &str) -> Read<u8> {
        match json {
            None => Ok(ANY_BUTTON),
            Some(Json::String(any)) if any == "any" => Ok(ANY_BUTTON),
            given => bounded(given, 0, i64::from(neoteletext_rules::limits::MAX_BUTTON), path),
        }
    }

    /// One trigger of `on` (RULE-35's table).
    fn trigger(&self, json: &Json, rows: bool, path: &str) -> Read<Trigger> {
        use neoteletext_rules::limits::{MAX_CUE, MAX_TIMER_TICKS};
        let fields = object(json, path)?;
        let event = text(fields.get("event"), &format!("{path}.event"))?;
        let target = |any| self.trigger_target(fields.get("target"), any, rows, &format!("{path}.target"));
        let button = || Self::button(fields.get("button"), &format!("{path}.button"));
        Ok(match event {
            "key" => {
                Trigger::Key { keys: keys(fields, rows, path)?, repeat: boolean(fields.get("repeat"), false, path)? }
            }
            "key_up" => Trigger::KeyUp { keys: keys(fields, rows, path)? },
            "press" => Trigger::Press { target: target(true)?, button: button()? },
            "release" => Trigger::Release { target: target(true)?, button: button()? },
            "enter" => Trigger::Enter { target: target(true)? },
            "leave" => Trigger::Leave { target: target(true)? },
            "wheel" => Trigger::Wheel { target: target(true)? },
            "move" => Trigger::Move { target: target(true)?, held: boolean(fields.get("held"), false, path)? },
            "open" => Trigger::Open,
            "close" => Trigger::Close,
            "text" => Trigger::Text,
            "focus" => Trigger::Focus,
            "blur" => Trigger::Blur,
            "becomes" => Trigger::Becomes,
            "every" | "after" => {
                let timer = timer(fields.get("timer"), Some(PAGE_TIMER), &format!("{path}.timer"))?;
                let ticks = bounded(fields.get("ticks"), 1, i64::from(MAX_TIMER_TICKS), &format!("{path}.ticks"))?;
                if event == "every" { Trigger::Every { timer, ticks } } else { Trigger::After { timer, ticks } }
            }
            "cue" => {
                let channel = bounded(fields.get("channel"), 0, audio_channel_last(), &format!("{path}.channel"))?;
                let number = match fields.get("number") {
                    None => ANY_CUE,
                    Some(Json::String(any)) if any == "any" => ANY_CUE,
                    given => bounded(given, 0, i64::from(MAX_CUE), &format!("{path}.number"))?,
                };
                Trigger::Cue { channel, number }
            }
            other => return Err(format!("{path}: no trigger is `{other}`")),
        })
    }

    /// A value `v` (RULE-35's table of `of`).
    fn value(&self, json: Option<&Json>, path: &str) -> Read<Value> {
        let fields = object(json.ok_or_else(|| format!("{path} is a value"))?, path)?;
        let of = text(fields.get("of"), &format!("{path}.of"))?;
        let property = |table: &[(&str, u16)]| word(table, fields.get("property"), &format!("{path}.property"));
        let channel = || bounded::<u8>(fields.get("channel"), 0, audio_channel_last(), &format!("{path}.channel"));
        Ok(match of {
            "variable" => Value::variable(designator(&names::VARIABLE_NAMES, fields.get("variable"), path)?),
            "key" => {
                let key = key(fields.get("key").unwrap_or(&Json::Null), &format!("{path}.key"))?;
                Value::key(key.kind.code(), key.code)
            }
            "button" => {
                Value::button(bounded(fields.get("button"), 0, i64::from(neoteletext_rules::limits::MAX_BUTTON), path)?)
            }
            "pointer" => Value::pointer(property(&names::POINTER_PROPERTIES)?),
            "keys_down" => Value::keys_down(),
            "timer" => {
                let named = timer(fields.get("timer"), None, path)?;
                let held = property(&names::TIMER_PROPERTIES)?;
                if held == neoteletext_rules::table::TIMER_PHASE {
                    // RULE-25: a phase names its period.
                    let period = fields
                        .get("period")
                        .ok_or_else(|| format!("{path}: a phase without a period (invalid_rules)"))?;
                    let period = bounded(
                        Some(period),
                        1,
                        i64::from(neoteletext_rules::limits::MAX_TIMER_TICKS),
                        &format!("{path}.period"),
                    )?;
                    Value::timer_phase(named, period)
                } else {
                    Value::timer(named, held)
                }
            }
            "target" => {
                let target = self.target(fields.get("target"), &format!("{path}.target"))?;
                Value::target(target.kind, target.number, property(&names::TARGET_PROPERTIES)?)
            }
            "sound" => Value::sound(channel()?, property(&names::SOUND_PROPERTIES)?),
            "synth" => Self::synth(fields, channel()?, path)?,
            "fired" => Value::fired(property(&names::FIRED_PROPERTIES)?),
            "sequence" => {
                let sequence = place(&self.sequences, text(fields.get("sequence"), path)?, "sequence")?;
                Value::sequence(sequence, property(&names::SEQUENCE_PROPERTIES)?)
            }
            other => return Err(format!("{path}: no value is of `{other}`")),
        })
    }

    /// A `synth` value (RULE-28): the family by name, the part from 1, the
    /// class, and its index (a key, a field by its name, a controller).
    fn synth(fields: &Map<String, Json>, channel: u8, path: &str) -> Read<Value> {
        use neoteletext_rules::table::{CLASS_CONTROLLER, CLASS_FIELD, CLASS_NOTE, CLASS_VELOCITY};
        let named = text(fields.get("family"), &format!("{path}.family"))?;
        let (_, code, variant) = FAMILIES
            .iter()
            .find(|(name, ..)| *name == named)
            .copied()
            .ok_or_else(|| format!("{path}.family: no family is `{named}`"))?;
        let last = if code == family::SYNTH6581 { LAST_SID_VOICE } else { LAST_PART };
        let part: u8 = bounded(fields.get("part"), FIRST_PART, last, &format!("{path}.part"))?;
        let class = word(&names::SYNTH_PROPERTIES, fields.get("property"), &format!("{path}.property"))?;
        let index = match class {
            CLASS_NOTE | CLASS_VELOCITY => midi_number(fields.get("key"), &format!("{path}.key"))?,
            CLASS_CONTROLLER => midi_number(fields.get("controller"), &format!("{path}.controller"))?,
            CLASS_FIELD => {
                let name = text(fields.get("field"), &format!("{path}.field"))?;
                let table = family_fields(code, variant).ok_or_else(|| format!("{path}: this build has no {named}"))?;
                let found = table.iter().position(|each| each.name == name);
                let found = found.ok_or_else(|| format!("{path}.field: {named} has no field `{name}`"))?;
                u16::try_from(found).map_err(|_| format!("{path}.field is out of range"))?
            }
            _ => 0,
        };
        Ok(Value::synth(channel, code, variant, part.saturating_sub(1), class, index))
    }

    /// A condition `when` (RULE-35), written in prefix order (RULE-44).
    fn condition(&self, json: &Json, rows: bool, out: &mut Vec<Node>, path: &str) -> Read<()> {
        let fields = object(json, path)?;
        if let Some(children) = fields.get("and").or_else(|| fields.get("or")) {
            let children = list(Some(children), path)?;
            let count = u8::try_from(children.len()).map_err(|_| format!("{path} has too many children"))?;
            out.push(if fields.contains_key("and") { Node::And(count) } else { Node::Or(count) });
            for (index, child) in children.iter().enumerate() {
                self.condition(child, rows, out, &format!("{path}[{index}]"))?;
            }
            return Ok(());
        }
        if let Some(child) = fields.get("not") {
            out.push(Node::Not);
            return self.condition(child, rows, out, &format!("{path}.not"));
        }
        for (word, overlaps) in [("overlaps", true), ("inside", false)] {
            if let Some(pair) = fields.get(word) {
                let pair = list(Some(pair), path)?;
                let [first, second] = pair else { return Err(format!("{path}.{word} names two targets")) };
                let first = self.target(Some(first), path)?;
                let second = self.target(Some(second), path)?;
                out.push(if overlaps { Node::Overlaps(first, second) } else { Node::Inside(first, second) });
                return Ok(());
            }
        }
        let value = self.value(fields.get("read"), &format!("{path}.read"))?;
        let whole =
            |json: Option<&Json>, path: &str| bounded::<i32>(json, i64::from(i32::MIN), i64::from(i32::MAX), path);
        if let Some(range) = fields.get("in_range") {
            let range = list(Some(range), path)?;
            let [low, high] = range else { return Err(format!("{path}.in_range is [low, high]")) };
            out.push(Node::InRange { value, low: whole(Some(low), path)?, high: whole(Some(high), path)? });
            return Ok(());
        }
        if let Some(set) = fields.get("in_set") {
            let mut members = neoteletext_rules::table::SetMembers::new(0);
            for each in list(Some(set), path)? {
                members.push(whole(Some(each), path)?).map_err(|_| format!("{path}.in_set has too many members"))?;
            }
            out.push(Node::InSet { value, members });
            return Ok(());
        }
        let operator = word(&names::OPERATORS, fields.get("op"), &format!("{path}.op"))?;
        let operator = Operator::from_code(operator).ok_or_else(|| format!("{path}.op is not an operator"))?;
        let (constant, plus_p) = match fields.get("value") {
            Some(Json::Object(added)) => {
                if !rows {
                    return Err(format!("{path}: a row parameter in a rule without rows"));
                }
                let plus = match added.get("plus") {
                    None => 0,
                    given => whole(given, path)?,
                };
                parameter(added.get("row"), path)?;
                (plus, true)
            }
            given => (whole(given, &format!("{path}.value"))?, false),
        };
        out.push(Node::Compare { value, operator, constant, plus_p });
        Ok(())
    }
}

/// The last audio channel, 3 (RULE-27).
fn audio_channel_last() -> i64 {
    i64::try_from(neoteletext_rules::limits::AUDIO_CHANNELS).unwrap_or(0).saturating_sub(1)
}

/// A MIDI data number, 0 to 127.
fn midi_number(json: Option<&Json>, path: &str) -> Read<u16> {
    bounded(json, 0, i64::from(neoteletext_rules::limits::MAX_MIDI_DATA), path)
}

/// A field that may be an amount in a group (RULE-106, RULE-35): an
/// integer, or `{"plus":k,"variable":"vN"}`, `{"plus":k,"row":"p"}` or
/// `{"plus":k,"fired":"key"}`. Gives the constant and the binding's source.
fn amount(json: Option<&Json>, least: i64, greatest: i64, path: &str) -> Read<(i64, Option<Source>)> {
    let Some(Json::Object(fields)) = json else {
        return Ok((bounded(json, least, greatest, path)?, None));
    };
    let plus = match fields.get("plus") {
        None => 0,
        given => integer(given, &format!("{path}.plus"))?,
    };
    let source = if let Some(variable) = fields.get("variable") {
        Source::Variable(designator(&names::VARIABLE_NAMES, Some(variable), path)?)
    } else if let Some(row) = fields.get("row") {
        parameter(Some(row), path)?;
        Source::Parameter
    } else if let Some(fired) = fields.get("fired") {
        let property = word(&names::FIRED_PROPERTIES, Some(fired), path)?;
        let first_four =
            u8::try_from(property).ok().filter(|property| u16::from(*property) < names::FIRED_PROPERTIES[4].1);
        Source::Fired(first_four.ok_or_else(|| format!("{path}: an amount names key, target, row or cue"))?)
    } else {
        return Err(format!("{path} is an integer or an amount"));
    };
    // The constant written in the body, held to the field's range; the
    // sum is held at run time (RULE-106).
    Ok((plus.clamp(least, greatest), Some(source)))
}

/// What one command's fields are written with: its bindings so far.
struct Binder {
    bindings: Vec<Binding>,
    /// The `ANIMATE` operations whose target's number is bound, each with
    /// the constant written for it.
    targets: Vec<(u8, u16)>,
}

impl Binder {
    fn bind(&mut self, item: u8, field: u8, source: Option<Source>, path: &str) -> Read<()> {
        if let Some(source) = source {
            if self.bindings.len() >= MAX_COMMAND_BINDINGS {
                return Err(format!("{path}: more than {MAX_COMMAND_BINDINGS} amounts"));
            }
            self.bindings.push(Binding::of_operation(item, field, source));
        }
        Ok(())
    }
}

impl Names<'_> {
    /// One command of a group (RULE-35): a program message without its
    /// `connections`, `page` and `sub`, which the table supplies.
    fn command(&self, json: &Json, path: &str) -> Read<Built> {
        let fields = object(json, path)?;
        let kind = text(fields.get("type"), &format!("{path}.type"))?;
        let mut binder = Binder { bindings: Vec::new(), targets: Vec::new() };
        let (kind, body) = match kind {
            "sound" => (TYPE_CONTROL, self.control(fields, &mut binder, path)?),
            "local_midi" => (command::TYPE_LOCAL_MIDI, self.local_midi(fields, &mut binder, path)?),
            "animate" => (command::TYPE_ANIMATE, self.animate(fields, &mut binder, path)?),
            "sprites" | "naplps_objects" => {
                return Err(format!("{path}: rules-run does not read the `{kind}` command"));
            }
            "variable" | "timer" | "report" | "run" | "sequence" => {
                let command = self.housekeeping(kind, fields, path)?;
                let mut body = vec![0_u8; command::MAX_BODY_BYTES];
                let length = command::encode(&command, self.page, self.sub, &mut body)
                    .map_err(|fault| format!("{path}: {fault:?}"))?;
                body.truncate(length);
                (command.kind(), body)
            }
            other => return Err(format!("{path}: a group holds no `{other}` (invalid_group)")),
        };
        Ok(Built { kind, body, bindings: binder.bindings })
    }

    /// A `sound` command (RULE-119): `play`, `stop` or `gain`, its page the
    /// table's page number, its sequence 0.
    fn control(&self, fields: &Map<String, Json>, binder: &mut Binder, path: &str) -> Read<Vec<u8>> {
        use neoteletext_rules::limits::{MAX_FADE_MS, MAX_GAIN};
        let operation = match text(fields.get("kind"), &format!("{path}.kind"))? {
            "play" => PLAY,
            "stop" => STOP,
            "gain" => GAIN,
            other => return Err(format!("{path}: a group's sound is play, stop or gain, not `{other}`")),
        };
        if fields.contains_key("page") || boolean(fields.get("after"), false, path)? {
            return Err(format!("{path}: a play in a group names no page and is not after (invalid_group)"));
        }
        let channel: u8 = bounded(fields.get("channel"), 0, audio_channel_last(), &format!("{path}.channel"))?;
        let (handle, generation, duck, looped): (u8, u16, u8, bool) = if operation == PLAY {
            (
                bounded(fields.get("handle"), 0, i64::from(u8::MAX), &format!("{path}.handle"))?,
                bounded(fields.get("generation"), 0, i64::from(u16::MAX), &format!("{path}.generation"))?,
                match fields.get("duck") {
                    None => 0,
                    given => bounded(given, 0, i64::from(u8::MAX), &format!("{path}.duck"))?,
                },
                boolean(fields.get("loop"), false, path)?,
            )
        } else {
            (0, 0, 0, false)
        };
        let (gain, gain_source) = match fields.get("gain") {
            None => (i64::from(MAX_GAIN), None),
            given => amount(given, 0, i64::from(MAX_GAIN), &format!("{path}.gain"))?,
        };
        let (fade, fade_source) = match fields.get("fade_ms") {
            None => (0, None),
            given => amount(given, 0, i64::from(MAX_FADE_MS), &format!("{path}.fade_ms"))?,
        };
        binder.bind(0, field::CONTROL_GAIN, gain_source, path)?;
        binder.bind(0, field::CONTROL_FADE, fade_source, path)?;
        let mut body = Vec::with_capacity(CONTROL_BYTES);
        body.push(channel);
        body.extend_from_slice(&0_u16.to_be_bytes());
        body.extend_from_slice(&[operation, handle]);
        body.extend_from_slice(&generation.to_be_bytes());
        body.push(u8::try_from(gain).unwrap_or(0));
        body.extend_from_slice(&u16::try_from(fade).unwrap_or(0).to_be_bytes());
        body.extend_from_slice(&[if looped { FLAG_LOOP } else { 0 }, duck]);
        body.extend_from_slice(&self.page.to_be_bytes());
        Ok(body)
    }

    /// A `local_midi` command (RULE-21, RULE-114).
    fn local_midi(&self, fields: &Map<String, Json>, binder: &mut Binder, path: &str) -> Read<Vec<u8>> {
        use neoteletext_rules::limits::{MAX_MIDI_DATA, MAX_PITCH_BEND, MIDI_DATA_BITS};
        let channel: u8 = bounded(fields.get("channel"), 0, audio_channel_last(), &format!("{path}.channel"))?;
        let message = object(fields.get("message").unwrap_or(&Json::Null), &format!("{path}.message"))?;
        let status = word(&names::MIDI_EVENTS, message.get("event"), &format!("{path}.message.event"))?;
        let part: u8 = bounded(message.get("channel"), FIRST_PART, LAST_PART, &format!("{path}.message.channel"))?;
        let data = |name: &str, field: u8, binder: &mut Binder| -> Read<u8> {
            let (value, source) =
                amount(message.get(name), 0, i64::from(MAX_MIDI_DATA), &format!("{path}.message.{name}"))?;
            binder.bind(0, field, source, path)?;
            Ok(u8::try_from(value).unwrap_or(0))
        };
        let (first, second) = match status {
            midi_status::NOTE_ON | midi_status::NOTE_OFF => {
                (data("key", field::MIDI_DATA1, binder)?, data("velocity", field::MIDI_DATA2, binder)?)
            }
            midi_status::CONTROL_CHANGE => {
                (data("controller", field::MIDI_DATA1, binder)?, data("value", field::MIDI_DATA2, binder)?)
            }
            midi_status::PROGRAM_CHANGE => (data("program", field::MIDI_DATA1, binder)?, 0),
            _ => {
                let (bend, source) =
                    amount(message.get("value"), 0, i64::from(MAX_PITCH_BEND), &format!("{path}.message.value"))?;
                binder.bind(0, field::MIDI_BEND, source, path)?;
                let bend = u16::try_from(bend).unwrap_or(0);
                // RULE-21: data 1 the low seven bits, data 2 the high.
                let low = u8::try_from(bend & u16::from(MAX_MIDI_DATA)).unwrap_or(0);
                let high = u8::try_from((bend >> MIDI_DATA_BITS) & u16::from(MAX_MIDI_DATA)).unwrap_or(0);
                (low, high)
            }
        };
        let command =
            Command::LocalMidi { channel, status: status | part.saturating_sub(1), data1: first, data2: second };
        let mut body = vec![0_u8; command::MAX_BODY_BYTES];
        let length =
            command::encode(&command, self.page, self.sub, &mut body).map_err(|fault| format!("{path}: {fault:?}"))?;
        body.truncate(length);
        Ok(body)
    }

    /// An `animate` command (ANIM-21): its operations, each target and
    /// each field that RULE-106 lists possibly an amount.
    fn animate(&self, fields: &Map<String, Json>, binder: &mut Binder, path: &str) -> Read<Vec<u8>> {
        use neoteletext_rules::limits::{MAX_CYCLE_PERIOD, MAX_FRAME, MAX_LAYER, MAX_LOOK};
        let ops = list(fields.get("ops"), &format!("{path}.ops"))?;
        let mut operations = Vec::with_capacity(ops.len());
        for (index, op) in ops.iter().enumerate() {
            let path = format!("{path}.ops[{index}]");
            let item = u8::try_from(index).map_err(|_| format!("{path}: too many operations"))?;
            let op = object(op, &path)?;
            let action = word(&names::OPERATIONS, op.get("action"), &format!("{path}.action"))?;
            let target = Self::operation_target(op.get("target"), item, binder, &path)?;
            let small = |name: &str, field: u8, least: i64, greatest: i64, binder: &mut Binder| -> Read<i64> {
                let (value, source) = amount(op.get(name), least, greatest, &format!("{path}.{name}"))?;
                binder.bind(item, field, source, &path)?;
                Ok(value)
            };
            let kind = match action {
                operation::SHOW => OperationKind::Show,
                operation::HIDE => OperationKind::Hide,
                operation::STOP_CYCLE => OperationKind::StopCycle,
                operation::RESET => OperationKind::Reset,
                operation::LOOK => {
                    let look = match op.get("look") {
                        Some(Json::String(_)) => i64::from(word(&names::LOOKS, op.get("look"), &path)?),
                        given => small_given(given, field::LOOK, 0, i64::from(MAX_LOOK), item, binder, &path)?,
                    };
                    OperationKind::Look(u8::try_from(look).unwrap_or(0))
                }
                operation::MOVE => OperationKind::Move {
                    relative: boolean(op.get("relative"), false, &path)?,
                    x: i16::try_from(small("x", field::X, i64::from(i16::MIN), i64::from(i16::MAX), binder)?)
                        .unwrap_or(0),
                    y: i16::try_from(small("y", field::Y, i64::from(i16::MIN), i64::from(i16::MAX), binder)?)
                        .unwrap_or(0),
                },
                operation::ORDER => OperationKind::Order {
                    layer: match op.get("layer") {
                        None | Some(Json::Null) => KEEP_LAYER,
                        given => bounded(given, 0, i64::from(MAX_LAYER), &format!("{path}.layer"))?,
                    },
                    order: i8::try_from(small("order", field::ORDER, i64::from(i8::MIN), i64::from(i8::MAX), binder)?)
                        .unwrap_or(0),
                },
                operation::FRAME => OperationKind::Frame(
                    u8::try_from(small("frame", field::FRAME, 0, i64::from(MAX_FRAME), binder)?).unwrap_or(0),
                ),
                _ => OperationKind::Cycle {
                    first: bounded(op.get("first"), 0, i64::from(MAX_FRAME), &format!("{path}.first"))?,
                    last: bounded(op.get("last"), 0, i64::from(MAX_FRAME), &format!("{path}.last"))?,
                    period: u16::try_from(small("period", field::PERIOD, 1, i64::from(MAX_CYCLE_PERIOD), binder)?)
                        .unwrap_or(1),
                    once: boolean(op.get("once"), false, &path)?,
                },
            };
            operations.push(Operation { target, kind });
        }
        let mut body = vec![0_u8; MAX_COMMAND_BYTES];
        let length = write_record(self.page, self.sub, &operations, &mut body)
            .map_err(|fault| format!("{path}: the operations do not fit a command: {fault:?}"))?;
        body.truncate(length);
        // Each bound target's constant, at its operation's number field: the
        // operation's code and the target's kind come before it (ANIM-6).
        for (item, constant) in &binder.targets {
            let mut at = RECORD_FIXED;
            for operation in operations.iter().take(usize::from(*item)) {
                at = at.saturating_add(length_of(operation.code()).unwrap_or(0));
            }
            let number_at = at.saturating_add(TARGET_NUMBER_AT);
            let slot = body.get_mut(number_at..number_at.saturating_add(2)).ok_or("a bound target's place")?;
            slot.copy_from_slice(&constant.to_be_bytes());
        }
        Ok(body)
    }

    /// An operation's target (RULE-106): `{"control":n}` or `{"object":n}`,
    /// `n` a number, `{"row":"pN","plus":k}` or `{"fired":"target","plus":k}`,
    /// never a variable and never the page.
    fn operation_target(json: Option<&Json>, item: u8, binder: &mut Binder, path: &str) -> Read<Target> {
        let fields = object(json.ok_or_else(|| format!("{path} names a target"))?, path)?;
        let (kind, inner) = match (fields.get("control"), fields.get("object")) {
            (Some(inner), None) => (TargetKind::Control, inner),
            (None, Some(inner)) => (TargetKind::Object, inner),
            _ => return Err(format!("{path}.target is a control or an object")),
        };
        if let Json::Object(amount_fields) = inner {
            if amount_fields.contains_key("variable") {
                return Err(format!("{path}: a variable never chooses a target (invalid_group)"));
            }
            let (number, source) = amount(Some(inner), 0, i64::from(u16::MAX), &format!("{path}.target"))?;
            binder.bind(item, field::TARGET, source, path)?;
            // A bound number's constant may be 0, which the record's writer
            // refuses as "any": it is written as 1 and set after (RULE-106).
            let constant = u16::try_from(number).unwrap_or(0);
            binder.targets.push((item, constant));
            return Ok(Target { kind, number: constant.max(1) });
        }
        let number = bounded(Some(inner), 1, i64::from(u16::MAX), &format!("{path}.target"))?;
        Ok(Target { kind, number })
    }

    /// `variable`, `timer`, `report`, `run` and `sequence` (RULE-114).
    fn housekeeping(&self, kind: &str, fields: &Map<String, Json>, path: &str) -> Read<Command> {
        use neoteletext_rules::limits::MAX_TIMER_TICKS;
        let start = |path: &str| -> Read<bool> {
            match text(fields.get("op"), path)? {
                "start" => Ok(true),
                "stop" => Ok(false),
                other => Err(format!("{path} is start or stop, not `{other}`")),
            }
        };
        Ok(match kind {
            "variable" => self.variable(fields, path)?,
            "timer" => Command::Timer {
                start: start(&format!("{path}.op"))?,
                timer: designator(&names::TIMER_NAMES, fields.get("timer"), &format!("{path}.timer"))?,
            },
            "report" => Command::Report {
                value: match fields.get("value") {
                    None | Some(Json::Null) => None,
                    given => Some(self.value(given, &format!("{path}.value"))?),
                },
            },
            "run" => {
                // DEC-6635 item 6: `run` has no `at`.
                if fields.contains_key("at") {
                    return Err(format!("{path}.at: a run has no `at` (unknown_field)"));
                }
                Command::Run {
                    group: place(&self.groups, text(fields.get("group"), &format!("{path}.group"))?, "group")?,
                }
            }
            _ => {
                let start = start(&format!("{path}.op"))?;
                let sequence = match fields.get("sequence") {
                    None | Some(Json::Null) if !start => ALL_SEQUENCES,
                    given => place(&self.sequences, text(given, &format!("{path}.sequence"))?, "sequence")?,
                };
                let mode = match fields.get("at") {
                    None | Some(Json::Null) => StartMode::Next,
                    Some(_) if !start => return Err(format!("{path}.at: a stop has no `at` (invalid_rules)")),
                    Some(at) => {
                        let at = object(at, &format!("{path}.at"))?;
                        if let Some(tick) = at.get("tick") {
                            StartMode::At(bounded(Some(tick), 0, i64::from(u32::MAX), &format!("{path}.at.tick"))?)
                        } else if let Some(beat) = at.get("beat") {
                            let beat = object(beat, &format!("{path}.at.beat"))?;
                            StartMode::Beat {
                                timer: timer(beat.get("timer"), Some(PAGE_TIMER), &format!("{path}.at.beat.timer"))?,
                                ticks: bounded(
                                    beat.get("ticks"),
                                    1,
                                    i64::from(MAX_TIMER_TICKS),
                                    &format!("{path}.at.beat.ticks"),
                                )?,
                            }
                        } else {
                            return Err(format!("{path}.at is null, a tick or a beat"));
                        }
                    }
                };
                Command::Sequence { start, sequence, mode }
            }
        })
    }

    /// A `variable` command (RULE-99, RULE-114).
    fn variable(&self, fields: &Map<String, Json>, path: &str) -> Read<Command> {
        let operation = word(&names::VARIABLE_OPERATIONS, fields.get("op"), &format!("{path}.op"))?;
        let operation =
            VariableOperation::from_code(operation).ok_or_else(|| format!("{path}.op is not an operation"))?;
        let variable = match fields.get("variable") {
            Some(Json::String(all)) if all == "all" && operation == VariableOperation::Clear => ALL_VARIABLES,
            given => designator(&names::VARIABLE_NAMES, given, &format!("{path}.variable"))?,
        };
        let whole =
            |json: Option<&Json>, path: &str| bounded::<i32>(json, i64::from(i32::MIN), i64::from(i32::MAX), path);
        let operand = match operation {
            VariableOperation::Inc | VariableOperation::Dec | VariableOperation::Toggle | VariableOperation::Clear => {
                Operand::None
            }
            VariableOperation::Wrap => Operand::Range {
                low: whole(fields.get("low"), &format!("{path}.low"))?,
                high: whole(fields.get("high"), &format!("{path}.high"))?,
            },
            _ => match fields.get("value") {
                Some(Json::Object(given)) if given.contains_key("read") => {
                    Operand::Value(self.value(given.get("read"), &format!("{path}.value.read"))?)
                }
                Some(Json::Object(given)) => {
                    let constant = match given.get("plus") {
                        None => 0,
                        plus => whole(plus, &format!("{path}.value.plus"))?,
                    };
                    if let Some(variable) = given.get("variable") {
                        Operand::Variable {
                            constant,
                            variable: designator(&names::VARIABLE_NAMES, Some(variable), path)?,
                        }
                    } else {
                        let property =
                            word(&names::FIRED_PROPERTIES, given.get("fired"), &format!("{path}.value.fired"))?;
                        Operand::Fired { constant, property: u8::try_from(property).unwrap_or(0) }
                    }
                }
                given => Operand::Constant(whole(given, &format!("{path}.value"))?),
            },
        };
        Ok(Command::Variable { operation, variable, operand })
    }
}

/// `small` for a field given directly.
fn small_given(
    json: Option<&Json>,
    field: u8,
    least: i64,
    greatest: i64,
    item: u8,
    binder: &mut Binder,
    path: &str,
) -> Read<i64> {
    let (value, source) = amount(json, least, greatest, path)?;
    binder.bind(item, field, source, path)?;
    Ok(value)
}

/// The commands of a group, or of a rule's `do` (RULE-35, RULE-118).
fn commands(names: &Names<'_>, json: Option<&Json>, path: &str) -> Read<Vec<Built>> {
    let given = list(json, path)?;
    if given.is_empty() || given.len() > MAX_GROUP_COMMANDS {
        return Err(format!("{path} holds 1 to {MAX_GROUP_COMMANDS} commands"));
    }
    given.iter().enumerate().map(|(index, each)| names.command(each, &format!("{path}[{index}]"))).collect()
}

/// The report of the firing that `echo` adds (RULE-35).
fn echo(page: u16, sub: u16) -> Read<Built> {
    let mut body = vec![0_u8; command::REPORT_BYTES];
    let length = command::encode(&Command::Report { value: None }, page, sub, &mut body)
        .map_err(|fault| format!("the echo's report: {fault:?}"))?;
    body.truncate(length);
    Ok(Built { kind: command::TYPE_REPORT, body, bindings: Vec::new() })
}

/// The names of a list of objects that each have a `name`, refusing a
/// duplicate (RULE-36).
fn names_of<'a>(items: &'a [Json], what: &str) -> Read<Vec<&'a str>> {
    let mut held: Vec<&str> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let name = text(item.get("name"), &format!("{what}[{index}].name"))?;
        if held.contains(&name) {
            return Err(format!("{what}[{index}]: the name `{name}` is declared twice (duplicate_name)"));
        }
        held.push(name);
    }
    Ok(held)
}

/// The table's rectangles (RULE-13), and their names.
fn read_rects<'a>(table_fields: &'a Map<String, Json>, table: &mut Table) -> Read<Vec<&'a str>> {
    let rects = list(table_fields.get("rects"), "table.rects")?;
    let rect_names = names_of(rects, "table.rects")?;
    if rect_names.contains(&"page") {
        return Err("table.rects: `page` is not a rectangle's name (reserved)".to_owned());
    }
    for (index, rect) in rects.iter().enumerate() {
        let path = format!("table.rects[{index}]");
        let side = |name: &str, least: i64| {
            bounded::<u16>(rect.get(name), least, i64::from(u16::MAX), &format!("{path}.{name}"))
        };
        let held = Rect { x: side("x", 0)?, y: side("y", 0)?, width: side("width", 1)?, height: side("height", 1)? };
        table.rects.push(held).map_err(|_| format!("{path}: too many rectangles (rules_over_limit, MAX_RECTS)"))?;
    }
    Ok(rect_names)
}

/// The table's row sets (RULE-105), laid out one after another in the
/// order of their names: each name, its first row and its count.
fn read_rows<'a>(table_fields: &'a Map<String, Json>, table: &mut Table) -> Read<Vec<(&'a str, u8, u8)>> {
    let mut row_sets = Vec::new();
    let Some(sets) = table_fields.get("rows") else { return Ok(row_sets) };
    for (name, set) in object(sets, "table.rows")? {
        let path = format!("table.rows.{name}");
        let rows = list(Some(set), &path)?;
        let first = u8::try_from(table.rows.len()).map_err(|_| format!("{path}: too many rows"))?;
        let count = u8::try_from(rows.len()).map_err(|_| format!("{path}: too many rows"))?;
        for (index, row) in rows.iter().enumerate() {
            let path = format!("{path}[{index}]");
            // DEC-6635 item 8: a row is its one parameter, `p`: an integer,
            // or a key standing for its code.
            let held: Row = row_value(Some(row), &path)?;
            table.rows.push(held).map_err(|_| format!("{path}: too many rows (rules_over_limit, MAX_ROWS)"))?;
        }
        row_sets.push((name.as_str(), first, count));
    }
    Ok(row_sets)
}

/// The group a rule's `run` or `do` and `echo` want (RULE-35): `Ok(Err(n))`
/// for a named group run as it is, `Ok(Ok(commands))` for an unnamed one.
fn wanted_group(names: &Names<'_>, rule: &Json, path: &str) -> Read<Result<Vec<Built>, usize>> {
    let echoed = boolean(rule.get("echo"), false, path)?;
    match (rule.get("run"), rule.get("do")) {
        (Some(run), None) => {
            let group = place(&names.groups, text(Some(run), &format!("{path}.run"))?, "group")?;
            if !echoed {
                return Ok(Err(usize::from(group)));
            }
            let run = Command::Run { group };
            let mut body = vec![0_u8; command::RUN_BYTES];
            let length = command::encode(&run, names.page, names.sub, &mut body)
                .map_err(|fault| format!("{path}: {fault:?}"))?;
            body.truncate(length);
            Ok(Ok(vec![Built { kind: command::TYPE_RUN, body, bindings: Vec::new() }, echo(names.page, names.sub)?]))
        }
        (None, Some(list)) => {
            let mut wanted = commands(names, Some(list), &format!("{path}.do"))?;
            if echoed {
                wanted.push(echo(names.page, names.sub)?);
            }
            Ok(Ok(wanted))
        }
        _ => Err(format!("{path} has exactly one of run and do (invalid_rules)")),
    }
}

/// The table's groups (RULE-35, RULE-118): the named groups first,
/// numbered in the order written; then the unnamed groups the rules' `do`
/// and `echo` make, equal ones shared. Gives the group each rule runs.
fn build_groups(names: &Names<'_>, groups: &[Json], rules: &[Json], table: &mut Table) -> Read<Vec<usize>> {
    let mut built: Vec<Vec<Built>> = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        built.push(commands(names, group.get("commands"), &format!("table.groups[{index}].commands"))?);
    }
    let mut rule_groups = Vec::with_capacity(rules.len());
    for (index, rule) in rules.iter().enumerate() {
        let wanted = match wanted_group(names, rule, &format!("table.rules[{index}]"))? {
            Err(run_as_it_is) => {
                rule_groups.push(run_as_it_is);
                continue;
            }
            Ok(wanted) => wanted,
        };
        let shared = built.iter().skip(groups.len()).position(|held| *held == wanted);
        let number = if let Some(place) = shared {
            place.saturating_add(groups.len())
        } else {
            built.push(wanted);
            built.len().saturating_sub(1)
        };
        rule_groups.push(number);
    }
    for (index, group) in built.iter().enumerate() {
        let framed: Vec<Framed<'_>> =
            group.iter().map(|each| Framed { kind: each.kind, body: &each.body, bindings: &each.bindings }).collect();
        table
            .push_group(&framed)
            .map_err(|fault| format!("group {index}: {fault:?} (rules_over_limit or invalid_group)"))?;
    }
    Ok(rule_groups)
}

/// The table's sequences (RULE-103).
fn read_sequences(names: &Names<'_>, sequences: &[Json], table: &mut Table) -> Read<()> {
    for (index, sequence) in sequences.iter().enumerate() {
        let path = format!("table.sequences[{index}]");
        let length = bounded(sequence.get("length"), 1, i64::from(MAX_SEQUENCE_TICKS), &format!("{path}.length"))?;
        let repeat = bounded(sequence.get("repeat"), 0, i64::from(MAX_SEQUENCE_REPEAT), &format!("{path}.repeat"))?;
        let mut held = Sequence::new(length, repeat);
        for (place_of, step) in list(sequence.get("steps"), &format!("{path}.steps"))?.iter().enumerate() {
            let path = format!("{path}.steps[{place_of}]");
            let offset = bounded(step.get("tick"), 0, i64::from(u32::MAX), &format!("{path}.tick"))?;
            let group = place(&names.groups, text(step.get("run"), &format!("{path}.run"))?, "group")?;
            held.steps.push(Step { offset, group }).map_err(|_| format!("{path}: too many steps"))?;
        }
        table
            .sequences
            .push(held)
            .map_err(|_| format!("{path}: too many sequences (rules_over_limit, MAX_SEQUENCES)"))?;
    }
    Ok(())
}

/// One rule (RULE-35): its triggers, its rows and its condition, running
/// group `group`.
fn read_rule(names: &Names<'_>, rule: &Json, group: usize, path: &str) -> Read<Rule> {
    let rows = match rule.get("rows") {
        None | Some(Json::Null) => None,
        given => {
            let name = text(given, &format!("{path}.rows"))?;
            let (_, first, count) = names
                .rows
                .iter()
                .find(|(held, ..)| *held == name)
                .copied()
                .ok_or_else(|| format!("{path}.rows: no row set is named `{name}` (unknown_name)"))?;
            Some(RowRun { first, count })
        }
    };
    let triggers: Vec<&Json> = match rule.get("on") {
        Some(Json::Array(each)) => each.iter().collect(),
        Some(one) => vec![one],
        None => return Err(format!("{path}.on is missing")),
    };
    if triggers.is_empty() || triggers.len() > MAX_TRIGGERS_PER_RULE {
        return Err(format!("{path}.on holds 1 to {MAX_TRIGGERS_PER_RULE} triggers"));
    }
    let group = u8::try_from(group).map_err(|_| format!("{path}: too many groups (rules_over_limit, MAX_GROUPS)"))?;
    let mut held: Option<Rule> = None;
    for (place_of, trigger) in triggers.into_iter().enumerate() {
        let trigger = names.trigger(trigger, rows.is_some(), &format!("{path}.on[{place_of}]"))?;
        match held.as_mut() {
            None => held = Some(Rule::new(trigger, group)),
            Some(rule) => {
                rule.triggers.push(trigger).map_err(|_| format!("{path}.on: too many triggers"))?;
            }
        }
    }
    let mut held = held.ok_or_else(|| format!("{path}.on is missing"))?;
    held.rows = rows;
    if let Some(when) = rule.get("when").filter(|when| !when.is_null()) {
        let mut nodes = Vec::new();
        names.condition(when, rows.is_some(), &mut nodes, &format!("{path}.when"))?;
        if nodes.len() > MAX_CONDITION_NODES {
            return Err(format!("{path}.when: too many nodes (rules_over_limit, MAX_CONDITION_NODES)"));
        }
        held.condition = condition(&nodes);
    }
    Ok(held)
}

/// Reads a `rules` message (RULE-35) into its page, its sub-code and its
/// table, checked whole as a receiver decodes one (RULE-39, RULE-70).
///
/// # Errors
/// Text that is not JSON, a message that is not a `rules` message with a
/// table, and every fault RULE-39 names, as a sentence with the message's
/// path.
pub(crate) fn read_rules_message(source: &str) -> Read<(u16, u16, Box<Table>)> {
    let message: Json = serde_json::from_str(source).map_err(|error| format!("not JSON: {error}"))?;
    let message = object(&message, "the message")?;
    if text(message.get("type"), "type")? != "rules" {
        return Err("the message is not a `rules` message".to_owned());
    }
    let page = hexadecimal(message.get("page"), PAGE_DIGITS, "page")?;
    let sub = hexadecimal(message.get("sub"), SUB_DIGITS, "sub")?;
    let table_json = message.get("table").ok_or("table is missing")?;
    if table_json.is_null() {
        return Err("table is null: the page has no table to evaluate".to_owned());
    }
    let table_fields = object(table_json, "table")?;
    let mut table = Box::new(Table::new(page, sub));
    let rects = read_rects(table_fields, &mut table)?;
    let rows = read_rows(table_fields, &mut table)?;
    let groups = list(table_fields.get("groups"), "table.groups")?;
    let sequences = list(table_fields.get("sequences"), "table.sequences")?;
    let rules = list(Some(table_fields.get("rules").ok_or("table.rules is missing")?), "table.rules")?;
    let names = Names {
        rects,
        rows,
        groups: names_of(groups, "table.groups")?,
        sequences: names_of(sequences, "table.sequences")?,
        page,
        sub,
    };
    names_of(rules, "table.rules")?;
    let rule_groups = build_groups(&names, groups, rules, &mut table)?;
    read_sequences(&names, sequences, &mut table)?;
    for ((index, rule), group) in rules.iter().enumerate().zip(rule_groups) {
        let path = format!("table.rules[{index}]");
        let held = read_rule(&names, rule, group, &path)?;
        table.rules.push(held).map_err(|_| format!("{path}: too many rules (rules_over_limit, MAX_RULES)"))?;
    }
    // RULE-39, RULE-59: the table is encoded and the image checked by the
    // code a receiver decodes it with, then read back from that image, so
    // that what is evaluated is what a receiver would hold.
    let refused = |fault| format!("the table is refused: {fault:?} (invalid_rules)");
    let mut image = vec![0_u8; MAX_TABLE_BYTES];
    let length = encode(&table, body_check, &mut image).map_err(refused)?;
    let image = image.get(..length).ok_or("the image's length")?;
    decode(image, page, sub, body_check, &mut table).map_err(refused)?;
    Ok((page, sub, table))
}
